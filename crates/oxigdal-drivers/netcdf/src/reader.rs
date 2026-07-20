//! NetCDF file reader implementation.
//!
//! This module reads NetCDF files and exposes their dimensions, variables,
//! attributes, and array data.
//!
//! # Backends
//!
//! * **NetCDF-4 / HDF5 (Pure Rust)** — the primary backend. Real NetCDF-4
//!   files (which are HDF5 files carrying the NetCDF-4 conventions) are read
//!   with the Pure-Rust [`oxinetcdf`] crate atop `oxih5`. No `libnetcdf`,
//!   no `libhdf5`, no FFI. This backend honours the NetCDF-4 conventions:
//!   dimension scales, coordinate variables, `DIMENSION_LIST` axis linkage,
//!   and user attributes (`units`, `_FillValue`, `scale_factor`, …).
//! * **NetCDF-3 classic / 64-bit offset (Pure Rust, optional)** — enabled with
//!   the `netcdf3` feature and served by the `netcdf3` crate.
//!
//! When neither backend can read the file a typed
//! [`NetCdfError::InvalidFormat`] is returned — the reader never fabricates an
//! empty dataset.

use std::collections::HashMap;
use std::path::Path;

use crate::attribute::{Attribute, AttributeValue, Attributes};
use crate::dimension::{Dimension, Dimensions};
use crate::error::{NetCdfError, Result};
use crate::metadata::{CfMetadata, NetCdfMetadata, NetCdfVersion};
use crate::variable::{DataType, Variable, Variables};

use oxinetcdf::{ByteOrder, Dtype, NcAttribute, NcFile, NcGroup};

#[cfg(feature = "netcdf3")]
use std::cell::RefCell;

/// NetCDF file reader.
///
/// Provides methods for reading NetCDF files, including metadata and data.
pub struct NetCdfReader {
    metadata: NetCdfMetadata,
    /// Pure-Rust NetCDF-4 (HDF5) backend, present when the file was opened via
    /// [`oxinetcdf`].
    nc4: Option<Nc4Backend>,
    #[cfg(feature = "netcdf3")]
    file_nc3: Option<RefCell<netcdf3::FileReader>>,
}

/// Pure-Rust NetCDF-4 backend state.
///
/// Holds the open [`oxinetcdf::NcFile`] plus a map from variable name to the
/// underlying HDF5 dataset path, so array data can be read on demand.
struct Nc4Backend {
    file: NcFile,
    var_paths: HashMap<String, String>,
}

impl std::fmt::Debug for NetCdfReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetCdfReader")
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

impl NetCdfReader {
    /// Open a NetCDF file for reading.
    ///
    /// Real NetCDF-4 / HDF5 files are read with the Pure-Rust [`oxinetcdf`]
    /// backend. When the `netcdf3` feature is enabled, NetCDF-3 classic and
    /// 64-bit offset files are read first via the `netcdf3` crate.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the NetCDF file
    ///
    /// # Errors
    ///
    /// Returns [`NetCdfError::InvalidFormat`] if the file is neither a readable
    /// NetCDF-3 file nor a readable NetCDF-4 / HDF5 file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        // NetCDF-3 (Pure Rust) — try first so classic files are served by the
        // dedicated NetCDF-3 reader instead of failing the HDF5 magic check.
        #[cfg(feature = "netcdf3")]
        {
            if let Ok(file) = netcdf3::FileReader::open(path) {
                return Self::from_netcdf3(file);
            }
        }

        // NetCDF-4 / HDF5 (Pure Rust, primary backend).
        match NcFile::open(path) {
            Ok(file) => Self::from_oxinetcdf(file),
            Err(e) => Err(NetCdfError::InvalidFormat(format!(
                "file is not a readable NetCDF-3 or NetCDF-4/HDF5 file: {e}"
            ))),
        }
    }

    /// Build a reader from an open Pure-Rust NetCDF-4 file.
    fn from_oxinetcdf(file: NcFile) -> Result<Self> {
        let root = file.root_group().map_err(map_nc_err)?;
        let mut metadata = build_metadata_from_group(&root)?;
        metadata.parse_cf_metadata();
        let var_paths = root
            .variables
            .iter()
            .map(|v| (v.name.clone(), v.h5_path.clone()))
            .collect();

        Ok(Self {
            metadata,
            nc4: Some(Nc4Backend { file, var_paths }),
            #[cfg(feature = "netcdf3")]
            file_nc3: None,
        })
    }

    /// Create a reader from a NetCDF-3 file.
    ///
    /// # Errors
    ///
    /// Returns error if metadata cannot be read.
    #[cfg(feature = "netcdf3")]
    pub fn from_netcdf3(file: netcdf3::FileReader) -> Result<Self> {
        let metadata = Self::read_metadata_nc3(&file)?;
        Ok(Self {
            metadata,
            nc4: None,
            file_nc3: Some(RefCell::new(file)),
        })
    }

    /// Get the file metadata.
    #[must_use]
    pub const fn metadata(&self) -> &NetCdfMetadata {
        &self.metadata
    }

    /// Get the file format version.
    #[must_use]
    pub fn version(&self) -> NetCdfVersion {
        self.metadata.version()
    }

    /// Get dimensions.
    #[must_use]
    pub fn dimensions(&self) -> &Dimensions {
        self.metadata.dimensions()
    }

    /// Get variables.
    #[must_use]
    pub fn variables(&self) -> &Variables {
        self.metadata.variables()
    }

    /// Get global attributes.
    #[must_use]
    pub fn global_attributes(&self) -> &Attributes {
        self.metadata.global_attributes()
    }

    /// Get CF metadata if available.
    #[must_use]
    pub fn cf_metadata(&self) -> Option<&CfMetadata> {
        self.metadata.cf_metadata()
    }

    /// Read metadata from NetCDF-3 file.
    #[cfg(feature = "netcdf3")]
    fn read_metadata_nc3(file: &netcdf3::FileReader) -> Result<NetCdfMetadata> {
        use crate::nc3_compat;

        let mut metadata = NetCdfMetadata::new_classic();
        let dataset = file.data_set();

        // Read dimensions
        let dimensions = nc3_compat::read_dimensions(dataset)?;
        for dimension in dimensions {
            metadata.dimensions_mut().add(dimension)?;
        }

        // Read global attributes
        for attr_name in dataset.get_global_attr_names() {
            if let Some(attr) = nc3_compat::read_global_attribute(dataset, &attr_name)? {
                metadata.global_attributes_mut().add(attr)?;
            }
        }

        // Read variables
        for var_name in dataset.get_var_names() {
            let var = nc3_compat::read_variable(dataset, &var_name)?;
            metadata.variables_mut().add(var)?;
        }

        // Parse CF metadata
        metadata.parse_cf_metadata();

        Ok(metadata)
    }

    /// Convert NetCDF-3 data type to our data type.
    #[cfg(feature = "netcdf3")]
    fn convert_datatype_nc3(nc3_type: netcdf3::DataType) -> Result<DataType> {
        use netcdf3::DataType as Nc3Type;

        match nc3_type {
            Nc3Type::I8 => Ok(DataType::I8),
            Nc3Type::I16 => Ok(DataType::I16),
            Nc3Type::I32 => Ok(DataType::I32),
            Nc3Type::F32 => Ok(DataType::F32),
            Nc3Type::F64 => Ok(DataType::F64),
            Nc3Type::U8 => Ok(DataType::Char), // U8 in netcdf3 v0.6 represents character data
        }
    }

    /// Read variable data as f32.
    ///
    /// # Errors
    ///
    /// Returns error if variable not found or data cannot be read.
    pub fn read_f32(&self, var_name: &str) -> Result<Vec<f32>> {
        if let Some(nc4) = &self.nc4 {
            return nc4.read_f32(var_name);
        }

        #[cfg(feature = "netcdf3")]
        if let Some(ref file_cell) = self.file_nc3 {
            return Self::read_f32_nc3(&mut file_cell.borrow_mut(), var_name);
        }

        Err(NetCdfError::FeatureNotEnabled {
            feature: "netcdf reader".to_string(),
            message: "No reader backend available".to_string(),
        })
    }

    /// Read variable data as f64.
    ///
    /// # Errors
    ///
    /// Returns error if variable not found or data cannot be read.
    pub fn read_f64(&self, var_name: &str) -> Result<Vec<f64>> {
        if let Some(nc4) = &self.nc4 {
            return nc4.read_f64(var_name);
        }

        #[cfg(feature = "netcdf3")]
        if let Some(ref file_cell) = self.file_nc3 {
            return Self::read_f64_nc3(&mut file_cell.borrow_mut(), var_name);
        }

        Err(NetCdfError::FeatureNotEnabled {
            feature: "netcdf reader".to_string(),
            message: "No reader backend available".to_string(),
        })
    }

    /// Read variable data as i32.
    ///
    /// # Errors
    ///
    /// Returns error if variable not found or data cannot be read.
    pub fn read_i32(&self, var_name: &str) -> Result<Vec<i32>> {
        if let Some(nc4) = &self.nc4 {
            return nc4.read_i32(var_name);
        }

        #[cfg(feature = "netcdf3")]
        if let Some(ref file_cell) = self.file_nc3 {
            return Self::read_i32_nc3(&mut file_cell.borrow_mut(), var_name);
        }

        Err(NetCdfError::FeatureNotEnabled {
            feature: "netcdf reader".to_string(),
            message: "No reader backend available".to_string(),
        })
    }

    /// Read f32 data from NetCDF-3 file.
    #[cfg(feature = "netcdf3")]
    fn read_f32_nc3(file: &mut netcdf3::FileReader, var_name: &str) -> Result<Vec<f32>> {
        let dataset = file.data_set();
        let var_info = dataset
            .get_var(var_name)
            .ok_or_else(|| NetCdfError::VariableNotFound {
                name: var_name.to_string(),
            })?;

        use netcdf3::DataType as Nc3Type;
        let data_type = var_info.data_type();

        if data_type != Nc3Type::F32 {
            return Err(NetCdfError::DataTypeMismatch {
                expected: "F32".to_string(),
                found: format!("{:?}", data_type),
            });
        }

        let data = file.read_var_f32(var_name)?;
        Ok(data)
    }

    /// Read f64 data from NetCDF-3 file.
    #[cfg(feature = "netcdf3")]
    fn read_f64_nc3(file: &mut netcdf3::FileReader, var_name: &str) -> Result<Vec<f64>> {
        let dataset = file.data_set();
        let var_info = dataset
            .get_var(var_name)
            .ok_or_else(|| NetCdfError::VariableNotFound {
                name: var_name.to_string(),
            })?;

        use netcdf3::DataType as Nc3Type;
        let data_type = var_info.data_type();

        if data_type != Nc3Type::F64 {
            return Err(NetCdfError::DataTypeMismatch {
                expected: "F64".to_string(),
                found: format!("{:?}", data_type),
            });
        }

        let data = file.read_var_f64(var_name)?;
        Ok(data)
    }

    /// Read i32 data from NetCDF-3 file.
    #[cfg(feature = "netcdf3")]
    fn read_i32_nc3(file: &mut netcdf3::FileReader, var_name: &str) -> Result<Vec<i32>> {
        let dataset = file.data_set();
        let var_info = dataset
            .get_var(var_name)
            .ok_or_else(|| NetCdfError::VariableNotFound {
                name: var_name.to_string(),
            })?;

        use netcdf3::DataType as Nc3Type;
        let data_type = var_info.data_type();

        if data_type != Nc3Type::I32 {
            return Err(NetCdfError::DataTypeMismatch {
                expected: "I32".to_string(),
                found: format!("{:?}", data_type),
            });
        }

        let data = file.read_var_i32(var_name)?;
        Ok(data)
    }
}

impl Nc4Backend {
    /// Resolve `var_name` to its HDF5 dataset path.
    fn path(&self, var_name: &str) -> Result<&str> {
        self.var_paths
            .get(var_name)
            .map(String::as_str)
            .ok_or_else(|| NetCdfError::VariableNotFound {
                name: var_name.to_string(),
            })
    }

    /// Read a variable's data as `f32`.
    fn read_f32(&self, var_name: &str) -> Result<Vec<f32>> {
        let path = self.path(var_name)?;
        let ds = self
            .file
            .h5()
            .dataset(path)
            .map_err(|e| NetCdfError::Io(format!("read '{var_name}': {e}")))?;
        ds.as_f32().map_err(|e| NetCdfError::DataTypeMismatch {
            expected: "f32".to_string(),
            found: format!("{e}"),
        })
    }

    /// Read a variable's data as `f64`.
    fn read_f64(&self, var_name: &str) -> Result<Vec<f64>> {
        let path = self.path(var_name)?;
        let ds = self
            .file
            .h5()
            .dataset(path)
            .map_err(|e| NetCdfError::Io(format!("read '{var_name}': {e}")))?;
        ds.as_f64().map_err(|e| NetCdfError::DataTypeMismatch {
            expected: "f64".to_string(),
            found: format!("{e}"),
        })
    }

    /// Read a variable's data as `i32`.
    fn read_i32(&self, var_name: &str) -> Result<Vec<i32>> {
        let path = self.path(var_name)?;
        let ds = self
            .file
            .h5()
            .dataset(path)
            .map_err(|e| NetCdfError::Io(format!("read '{var_name}': {e}")))?;
        ds.as_i32().map_err(|e| NetCdfError::DataTypeMismatch {
            expected: "i32".to_string(),
            found: format!("{e}"),
        })
    }
}

// ---------------------------------------------------------------------------
// oxinetcdf → crate model mapping
// ---------------------------------------------------------------------------

/// Map an oxinetcdf error into the crate's error type.
fn map_nc_err(e: oxinetcdf::NcError) -> NetCdfError {
    match e {
        oxinetcdf::NcError::VariableNotFound(name) => NetCdfError::VariableNotFound { name },
        oxinetcdf::NcError::Unsupported(msg) => NetCdfError::Other(format!("unsupported: {msg}")),
        other => NetCdfError::InvalidFormat(other.to_string()),
    }
}

/// Build [`NetCdfMetadata`] from a resolved oxinetcdf root group.
fn build_metadata_from_group(root: &NcGroup) -> Result<NetCdfMetadata> {
    // NETCDF4_CLASSIC files carry a `_nc3_strict` root attribute.
    let version = if root.attrs.iter().any(|a| a.name == "_nc3_strict") {
        NetCdfVersion::NetCdf4Classic
    } else {
        NetCdfVersion::NetCdf4
    };
    let mut metadata = NetCdfMetadata::new(version);

    // Dimensions declared on the group.
    for d in &root.dimensions {
        add_dimension(&mut metadata, &d.name, d.len, d.is_unlimited)?;
    }

    // Variables.
    for v in &root.variables {
        let Some(data_type) = dtype_to_datatype(&v.dtype) else {
            // Enum / compound / opaque / vlen have no faithful DataType mapping;
            // skip rather than misreport the type.
            continue;
        };

        // Ensure every referenced axis exists as a dimension (covers oxinetcdf
        // phony dimensions synthesised for undimensioned datasets).
        for axis in &v.dims {
            if !metadata.dimensions().contains(&axis.name) {
                add_dimension(&mut metadata, &axis.name, axis.len, axis.is_unlimited)?;
            }
        }

        let dim_names: Vec<String> = v.dims.iter().map(|a| a.name.clone()).collect();
        let mut variable = Variable::new(&v.name, data_type, dim_names)?;
        variable.set_coordinate(v.is_coordinate);
        for attr in &v.attrs {
            if let Some(value) = nc_attr_to_value(attr)
                && let Ok(a) = Attribute::new(attr.name.clone(), value)
            {
                variable.attributes_mut().set(a);
            }
        }
        // Ignore duplicate variable names defensively.
        let _ = metadata.variables_mut().add(variable);
    }

    // Global attributes.
    for attr in &root.attrs {
        if let Some(value) = nc_attr_to_value(attr)
            && let Ok(a) = Attribute::new(attr.name.clone(), value)
        {
            let _ = metadata.global_attributes_mut().add(a);
        }
    }

    Ok(metadata)
}

/// Add a dimension to `metadata`, ignoring duplicates.
fn add_dimension(
    metadata: &mut NetCdfMetadata,
    name: &str,
    len: u64,
    unlimited: bool,
) -> Result<()> {
    if metadata.dimensions().contains(name) {
        return Ok(());
    }
    let len = usize::try_from(len).map_err(|_| NetCdfError::InvalidShape {
        message: format!("dimension '{name}' length {len} exceeds usize"),
    })?;
    let dim = if unlimited {
        Dimension::new_unlimited(name, len)?
    } else {
        Dimension::new(name, len)?
    };
    let _ = metadata.dimensions_mut().add(dim);
    Ok(())
}

/// Map an oxih5 datatype to the crate's [`DataType`].
///
/// Returns `None` for datatypes (enum, compound, opaque, vlen, …) that have no
/// faithful NetCDF scalar mapping.
fn dtype_to_datatype(dtype: &Dtype) -> Option<DataType> {
    Some(match dtype {
        Dtype::Float { size: 4, .. } => DataType::F32,
        Dtype::Float { size: 8, .. } => DataType::F64,
        Dtype::Int {
            size: 1,
            signed: true,
            ..
        } => DataType::I8,
        Dtype::Int {
            size: 1,
            signed: false,
            ..
        } => DataType::U8,
        Dtype::Int {
            size: 2,
            signed: true,
            ..
        } => DataType::I16,
        Dtype::Int {
            size: 2,
            signed: false,
            ..
        } => DataType::U16,
        Dtype::Int {
            size: 4,
            signed: true,
            ..
        } => DataType::I32,
        Dtype::Int {
            size: 4,
            signed: false,
            ..
        } => DataType::U32,
        Dtype::Int {
            size: 8,
            signed: true,
            ..
        } => DataType::I64,
        Dtype::Int {
            size: 8,
            signed: false,
            ..
        } => DataType::U64,
        Dtype::String {
            fixed_len: Some(1), ..
        } => DataType::Char,
        Dtype::String { .. } => DataType::String,
        _ => return None,
    })
}

/// Map an oxinetcdf attribute to the crate's [`AttributeValue`].
///
/// Returns `None` for datatypes that cannot be represented (e.g. compound).
fn nc_attr_to_value(attr: &NcAttribute) -> Option<AttributeValue> {
    let raw = attr.raw();
    let value = match &raw.dtype {
        Dtype::String { .. } => AttributeValue::Text(attr.as_text().ok()?),
        Dtype::Float { size: 4, order } => AttributeValue::F32(decode_f32(&raw.data, order)),
        Dtype::Float { size: 8, order } => AttributeValue::F64(decode_f64(&raw.data, order)),
        Dtype::Int {
            size: 1,
            signed: true,
            ..
        } => AttributeValue::I8(raw.data.iter().map(|&b| b as i8).collect()),
        Dtype::Int {
            size: 1,
            signed: false,
            ..
        } => AttributeValue::U8(raw.data.clone()),
        Dtype::Int {
            size: 2,
            signed: true,
            order,
        } => AttributeValue::I16(decode_chunks(
            &raw.data,
            order,
            i16::from_le_bytes,
            i16::from_be_bytes,
        )),
        Dtype::Int {
            size: 2,
            signed: false,
            order,
        } => AttributeValue::U16(decode_chunks(
            &raw.data,
            order,
            u16::from_le_bytes,
            u16::from_be_bytes,
        )),
        Dtype::Int {
            size: 4,
            signed: true,
            order,
        } => AttributeValue::I32(decode_chunks(
            &raw.data,
            order,
            i32::from_le_bytes,
            i32::from_be_bytes,
        )),
        Dtype::Int {
            size: 4,
            signed: false,
            order,
        } => AttributeValue::U32(decode_chunks(
            &raw.data,
            order,
            u32::from_le_bytes,
            u32::from_be_bytes,
        )),
        Dtype::Int {
            size: 8,
            signed: true,
            order,
        } => AttributeValue::I64(decode_chunks(
            &raw.data,
            order,
            i64::from_le_bytes,
            i64::from_be_bytes,
        )),
        Dtype::Int {
            size: 8,
            signed: false,
            order,
        } => AttributeValue::U64(decode_chunks(
            &raw.data,
            order,
            u64::from_le_bytes,
            u64::from_be_bytes,
        )),
        _ => return None,
    };
    Some(value)
}

/// Decode a little/big-endian byte buffer into a `Vec<T>` using fixed-width
/// array conversions. The chunk width is inferred from `N`.
fn decode_chunks<T, const N: usize>(
    data: &[u8],
    order: &ByteOrder,
    from_le: fn([u8; N]) -> T,
    from_be: fn([u8; N]) -> T,
) -> Vec<T> {
    data.chunks_exact(N)
        .filter_map(|c| <[u8; N]>::try_from(c).ok())
        .map(|arr| match order {
            ByteOrder::Little => from_le(arr),
            ByteOrder::Big => from_be(arr),
        })
        .collect()
}

/// Decode a byte buffer into `Vec<f32>`.
fn decode_f32(data: &[u8], order: &ByteOrder) -> Vec<f32> {
    decode_chunks(data, order, f32::from_le_bytes, f32::from_be_bytes)
}

/// Decode a byte buffer into `Vec<f64>`.
fn decode_f64(data: &[u8], order: &ByteOrder) -> Vec<f64> {
    decode_chunks(data, order, f64::from_le_bytes, f64::from_be_bytes)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use oxinetcdf::{NcFileWriter, NcType, VarOrGroup};

    /// Write a real NetCDF-4 (HDF5) file with the Pure-Rust oxinetcdf writer
    /// and return its path. The file carries a `lat`/`lon` grid, a `temp`
    /// data variable, CF-style attributes, and a global `Conventions`.
    fn write_real_nc4_fixture(tag: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "oxigdal_nc4_reader_{}_{}.nc",
            tag,
            std::process::id()
        ));

        let mut nc = NcFileWriter::new();
        let lat = nc.def_dim("lat", 3).expect("def lat");
        let lon = nc.def_dim("lon", 4).expect("def lon");
        let temp = nc
            .def_var("temp", &[lat, lon], NcType::Float64)
            .expect("def temp");
        let data: Vec<f64> = (0..12).map(|i| i as f64 * 0.5).collect();
        nc.put_var_f64(temp, &data).expect("put temp");
        nc.put_att_str(VarOrGroup::Var(temp), "units", "kelvin")
            .expect("units");
        nc.put_att_str(VarOrGroup::Var(temp), "standard_name", "air_temperature")
            .expect("standard_name");
        nc.put_att_str(VarOrGroup::Root, "Conventions", "CF-1.8")
            .expect("conventions");
        nc.put_att_str(VarOrGroup::Root, "title", "Reader Fixture")
            .expect("title");
        nc.close(&path).expect("close fixture");
        path
    }

    #[test]
    fn test_open_real_netcdf4_reads_dimensions_and_variables() {
        let path = write_real_nc4_fixture("dims");
        let reader = NetCdfReader::open(&path).expect("open real NetCDF-4");

        assert!(reader.version().is_netcdf4());

        // lat + lon dimensions.
        assert_eq!(reader.dimensions().len(), 2);
        assert_eq!(reader.dimensions().get("lat").expect("lat dim").len(), 3);
        assert_eq!(reader.dimensions().get("lon").expect("lon dim").len(), 4);

        // lat, lon coordinate variables + temp data variable.
        let temp = reader.variables().get("temp").expect("temp var");
        assert_eq!(temp.data_type(), DataType::F64);
        assert_eq!(temp.dimension_names(), &["lat", "lon"]);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_open_real_netcdf4_reads_variable_data() {
        let path = write_real_nc4_fixture("data");
        let reader = NetCdfReader::open(&path).expect("open real NetCDF-4");

        let data = reader.read_f64("temp").expect("read temp");
        assert_eq!(data.len(), 12);
        for (i, &v) in data.iter().enumerate() {
            assert!((v - i as f64 * 0.5).abs() < 1e-12);
        }

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_open_real_netcdf4_reads_attributes_and_cf() {
        let path = write_real_nc4_fixture("attrs");
        let reader = NetCdfReader::open(&path).expect("open real NetCDF-4");

        // Global CF metadata.
        let cf = reader.cf_metadata().expect("cf metadata");
        assert!(cf.is_cf_compliant());
        assert_eq!(cf.conventions.as_deref(), Some("CF-1.8"));
        assert_eq!(cf.title.as_deref(), Some("Reader Fixture"));

        // Variable attributes.
        let temp = reader.variables().get("temp").expect("temp var");
        let units = temp.attributes().get("units").expect("units attr");
        assert_eq!(units.value().as_text().expect("text"), "kelvin");
        assert!(temp.attributes().get("standard_name").is_some());

        // Reserved HDF5 convention attributes must not leak through.
        assert!(temp.attributes().get("DIMENSION_LIST").is_none());
        assert!(temp.attributes().get("CLASS").is_none());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_open_missing_variable_errors() {
        let path = write_real_nc4_fixture("missing");
        let reader = NetCdfReader::open(&path).expect("open real NetCDF-4");

        let result = reader.read_f64("does_not_exist");
        assert!(matches!(result, Err(NetCdfError::VariableNotFound { .. })));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_open_garbage_file_errors() {
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!("oxigdal_reader_garbage_{}.bin", std::process::id()));
        std::fs::write(&temp_file, b"this is not any netcdf file").expect("write temp");

        // Not NetCDF-3 and not a readable HDF5/NetCDF-4 file: fail loud.
        let result = NetCdfReader::open(&temp_file);
        assert!(result.is_err());
        assert!(!matches!(result, Err(NetCdfError::NetCdf4NotAvailable)));

        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn test_data_type_conversion() {
        #[cfg(feature = "netcdf3")]
        {
            use netcdf3::DataType as Nc3Type;
            assert_eq!(
                NetCdfReader::convert_datatype_nc3(Nc3Type::F32).expect("convert F32"),
                DataType::F32
            );
            assert_eq!(
                NetCdfReader::convert_datatype_nc3(Nc3Type::F64).expect("convert F64"),
                DataType::F64
            );
            assert_eq!(
                NetCdfReader::convert_datatype_nc3(Nc3Type::I32).expect("convert I32"),
                DataType::I32
            );
        }
    }
}
