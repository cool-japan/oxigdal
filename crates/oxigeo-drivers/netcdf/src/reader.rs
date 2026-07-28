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
//!   sub-groups (flattened into `"<group>/<var>"` names, recursively), and
//!   user attributes (`units`, `_FillValue`, `scale_factor`, …) — surfaced
//!   as-is via [`Variable::attributes`]. `scale_factor`/`add_offset`
//!   unpacking and `_FillValue`/`missing_value` masking (CF §8.1 / §2.5.1)
//!   are an explicit **opt-in**: [`NetCdfReader::read_f32`]/
//!   [`NetCdfReader::read_f64`]/[`NetCdfReader::read_i32`] are raw,
//!   unprocessed reads (and require an exact on-disk type match); use
//!   [`NetCdfReader::read_f64_cf`]/[`NetCdfReader::read_f32_cf`] for
//!   CF-unpacked, physical-unit values from any on-disk numeric element type.
//! * **NetCDF-3 classic / 64-bit offset (Pure Rust, optional)** — enabled with
//!   the `netcdf3` feature and served by the `netcdf3` crate. The same
//!   `read_f64_cf`/`read_f32_cf` opt-in CF unpacking applies here too.
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
        let (mut metadata, var_paths) = build_metadata_from_group(&root)?;
        metadata.parse_cf_metadata();

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

    /// Read a variable's data as CF-unpacked, physical-unit `f64`.
    ///
    /// This is the opt-in CF §8.1 "packed data" convention: the on-disk
    /// (possibly integer, e.g. packed `i16`) values are read as whatever
    /// numeric type they're actually stored as, then
    /// `physical = add_offset + scale_factor * packed` is applied using the
    /// variable's `scale_factor`/`add_offset` attributes (default `1.0`/`0.0`
    /// when absent), and any element equal to `_FillValue` (or, if that's
    /// absent, `missing_value`) is replaced with `NaN` per CF §2.5.1.
    ///
    /// [`NetCdfReader::read_f32`]/[`NetCdfReader::read_f64`]/[`NetCdfReader::read_i32`]
    /// remain raw, unprocessed reads (and require an exact on-disk type
    /// match) — this method is the documented, explicit opt-in for CF
    /// unpacking, reachable for any on-disk numeric element type.
    ///
    /// # Errors
    ///
    /// Returns an error if the variable is not found, is not numeric, or its
    /// data cannot be read.
    pub fn read_f64_cf(&self, var_name: &str) -> Result<Vec<f64>> {
        let attrs = self
            .metadata
            .variables()
            .get(var_name)
            .map(Variable::attributes)
            .cloned()
            .unwrap_or_default();

        let raw = if let Some(nc4) = &self.nc4 {
            nc4.read_numeric_as_f64(var_name)?
        } else {
            #[cfg(feature = "netcdf3")]
            if let Some(ref file_cell) = self.file_nc3 {
                Self::read_numeric_as_f64_nc3(&mut file_cell.borrow_mut(), var_name)?
            } else {
                return Err(NetCdfError::FeatureNotEnabled {
                    feature: "netcdf reader".to_string(),
                    message: "No reader backend available".to_string(),
                });
            }
            #[cfg(not(feature = "netcdf3"))]
            return Err(NetCdfError::FeatureNotEnabled {
                feature: "netcdf reader".to_string(),
                message: "No reader backend available".to_string(),
            });
        };

        Ok(apply_cf_scale_and_fill(&raw, &attrs))
    }

    /// Read a variable's data as CF-unpacked, physical-unit `f32`.
    ///
    /// Convenience narrowing wrapper over [`NetCdfReader::read_f64_cf`]; see
    /// its documentation for the exact CF unpacking semantics.
    ///
    /// # Errors
    ///
    /// Returns an error if the variable is not found, is not numeric, or its
    /// data cannot be read.
    pub fn read_f32_cf(&self, var_name: &str) -> Result<Vec<f32>> {
        #[allow(clippy::cast_possible_truncation)]
        Ok(self
            .read_f64_cf(var_name)?
            .into_iter()
            .map(|v| v as f32)
            .collect())
    }

    /// Read numeric data from a NetCDF-3 variable as `f64`, regardless of its
    /// on-disk numeric type (`i8`/`u8`/`i16`/`i32`/`f32`/`f64` — the full set
    /// NetCDF-3 classic supports).
    #[cfg(feature = "netcdf3")]
    fn read_numeric_as_f64_nc3(file: &mut netcdf3::FileReader, var_name: &str) -> Result<Vec<f64>> {
        use netcdf3::DataType as Nc3Type;

        let data_type = {
            let dataset = file.data_set();
            let var_info =
                dataset
                    .get_var(var_name)
                    .ok_or_else(|| NetCdfError::VariableNotFound {
                        name: var_name.to_string(),
                    })?;
            var_info.data_type()
        };

        let values: Vec<f64> = match data_type {
            Nc3Type::I8 => file
                .read_var_i8(var_name)?
                .into_iter()
                .map(f64::from)
                .collect(),
            Nc3Type::U8 => file
                .read_var_u8(var_name)?
                .into_iter()
                .map(f64::from)
                .collect(),
            Nc3Type::I16 => file
                .read_var_i16(var_name)?
                .into_iter()
                .map(f64::from)
                .collect(),
            Nc3Type::I32 => file
                .read_var_i32(var_name)?
                .into_iter()
                .map(f64::from)
                .collect(),
            Nc3Type::F32 => file
                .read_var_f32(var_name)?
                .into_iter()
                .map(f64::from)
                .collect(),
            Nc3Type::F64 => file.read_var_f64(var_name)?,
        };
        Ok(values)
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

    /// Read a variable's real on-disk values as `f64`, regardless of its
    /// concrete on-disk numeric element type.
    ///
    /// Used by [`NetCdfReader::read_f64_cf`] to reach CF-packed variables
    /// (which are frequently a narrower integer type than the physical
    /// values they represent, e.g. packed `i16`) without requiring the
    /// caller to know the on-disk type up front.
    fn read_numeric_as_f64(&self, var_name: &str) -> Result<Vec<f64>> {
        let path = self.path(var_name)?;
        let ds = self
            .file
            .h5()
            .dataset(path)
            .map_err(|e| NetCdfError::Io(format!("read '{var_name}': {e}")))?;

        fn type_mismatch(e: impl std::fmt::Display) -> NetCdfError {
            NetCdfError::DataTypeMismatch {
                expected: "numeric".to_string(),
                found: format!("{e}"),
            }
        }

        match &ds.dtype {
            Dtype::Float { size: 4, .. } => Ok(ds
                .as_f32()
                .map_err(type_mismatch)?
                .into_iter()
                .map(f64::from)
                .collect()),
            Dtype::Float { .. } => ds.as_f64().map_err(type_mismatch),
            Dtype::Int {
                size: 1,
                signed: true,
                ..
            } => Ok(ds
                .as_i8()
                .map_err(type_mismatch)?
                .into_iter()
                .map(f64::from)
                .collect()),
            Dtype::Int {
                size: 1,
                signed: false,
                ..
            } => Ok(ds
                .as_u8()
                .map_err(type_mismatch)?
                .into_iter()
                .map(f64::from)
                .collect()),
            Dtype::Int {
                size: 2,
                signed: true,
                ..
            } => Ok(ds
                .as_i16()
                .map_err(type_mismatch)?
                .into_iter()
                .map(f64::from)
                .collect()),
            Dtype::Int {
                size: 2,
                signed: false,
                ..
            } => Ok(ds
                .as_u16()
                .map_err(type_mismatch)?
                .into_iter()
                .map(f64::from)
                .collect()),
            Dtype::Int {
                size: 4,
                signed: true,
                ..
            } => Ok(ds
                .as_i32()
                .map_err(type_mismatch)?
                .into_iter()
                .map(f64::from)
                .collect()),
            Dtype::Int {
                size: 4,
                signed: false,
                ..
            } => Ok(ds
                .as_u32()
                .map_err(type_mismatch)?
                .into_iter()
                .map(f64::from)
                .collect()),
            Dtype::Int { signed: true, .. } =>
            {
                #[allow(clippy::cast_precision_loss)]
                Ok(ds
                    .as_i64()
                    .map_err(type_mismatch)?
                    .into_iter()
                    .map(|v| v as f64)
                    .collect())
            }
            Dtype::Int { signed: false, .. } =>
            {
                #[allow(clippy::cast_precision_loss)]
                Ok(ds
                    .as_u64()
                    .map_err(type_mismatch)?
                    .into_iter()
                    .map(|v| v as f64)
                    .collect())
            }
            other => Err(NetCdfError::DataTypeMismatch {
                expected: "numeric".to_string(),
                found: format!("{other:?}"),
            }),
        }
    }
}

/// Apply the CF §8.1 packed-data unpacking (`physical = add_offset +
/// scale_factor * packed`) and CF §2.5.1 fill-value masking (`_FillValue` /
/// `missing_value` → `NaN`) to already-decoded raw numeric values.
///
/// `scale_factor` defaults to `1.0` and `add_offset` to `0.0` when the
/// variable carries neither attribute (a no-op unpacking, matching a
/// non-packed variable). `_FillValue` is preferred over `missing_value` when
/// both are present, per CF convention precedence.
fn apply_cf_scale_and_fill(raw: &[f64], attrs: &Attributes) -> Vec<f64> {
    let scale = attrs
        .get("scale_factor")
        .and_then(|a| a.value().as_numeric_f64().ok())
        .unwrap_or(1.0);
    let offset = attrs
        .get("add_offset")
        .and_then(|a| a.value().as_numeric_f64().ok())
        .unwrap_or(0.0);
    let fill = attrs
        .get("_FillValue")
        .or_else(|| attrs.get("missing_value"))
        .and_then(|a| a.value().as_numeric_f64().ok());

    raw.iter()
        .map(|&v| match fill {
            Some(fill) if v == fill => f64::NAN,
            _ => offset + scale * v,
        })
        .collect()
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

/// Build [`NetCdfMetadata`] from a resolved oxinetcdf root group, recursing
/// into every sub-group (`root.children`, B3) so nested variables,
/// dimensions, and attributes are visible rather than silently dropped.
///
/// # Flattening scheme
///
/// A variable or dimension declared inside a sub-group is exposed under a
/// flattened name equal to its real HDF5 path with the leading `/` stripped
/// (e.g. the variable at HDF5 path `/sensor1/temperature` becomes
/// `"sensor1/temperature"`). Root-level names are therefore unchanged
/// (`/temperature` → `"temperature"`), preserving the metadata surface for
/// files that don't use groups at all. The returned `HashMap` maps each
/// flattened variable name to its real HDF5 dataset path, so
/// [`Nc4Backend::path`] (and therefore `read_f32`/`read_f64`/`read_i32`) can
/// reach variables at any nesting depth by their flattened name, e.g.
/// `reader.read_f64("sensor1/temperature")`.
///
/// Only the root group's own attributes become the file's
/// [`NetCdfMetadata::global_attributes`] (matching NetCDF-4 group-attribute
/// scoping — group attributes apply to that group only); sub-group
/// attributes are not currently surfaced as they have no group-scoped home in
/// [`NetCdfMetadata`] yet.
fn build_metadata_from_group(root: &NcGroup) -> Result<(NetCdfMetadata, HashMap<String, String>)> {
    // NETCDF4_CLASSIC files carry a `_nc3_strict` root attribute.
    let version = if root.attrs.iter().any(|a| a.name == "_nc3_strict") {
        NetCdfVersion::NetCdf4Classic
    } else {
        NetCdfVersion::NetCdf4
    };
    let mut metadata = NetCdfMetadata::new(version);
    let mut var_paths: HashMap<String, String> = HashMap::new();

    collect_group_recursive(root, &mut metadata, &mut var_paths)?;

    // Global attributes: root group only.
    for attr in &root.attrs {
        if let Some(value) = nc_attr_to_value(attr)
            && let Ok(a) = Attribute::new(attr.name.clone(), value)
        {
            let _ = metadata.global_attributes_mut().add(a);
        }
    }

    Ok((metadata, var_paths))
}

/// Flatten an absolute HDF5 path (e.g. `/sensor1/temperature`) into the
/// dotless-prefix name scheme documented on [`build_metadata_from_group`]
/// (`"sensor1/temperature"`; `/temperature` → `"temperature"`).
fn flatten_h5_path(h5_path: &str) -> String {
    h5_path.trim_start_matches('/').to_string()
}

/// Flatten a dimension's owning-group HDF5 path + its local name into the
/// same scheme `flatten_h5_path` uses for variables, so a dimension shared
/// across groups (via `DIMENSION_LIST` cross-group linkage) always resolves
/// to the same flattened name regardless of which group's variable
/// references it.
fn flatten_dim_name(owning_group_path: &str, dim_name: &str) -> String {
    if owning_group_path == "/" {
        dim_name.to_string()
    } else {
        format!("{}/{dim_name}", owning_group_path.trim_start_matches('/'))
    }
}

/// Recursively walk `group` and its `children`, registering every
/// dimension/variable under its flattened name into `metadata` and
/// `var_paths`.
fn collect_group_recursive(
    group: &NcGroup,
    metadata: &mut NetCdfMetadata,
    var_paths: &mut HashMap<String, String>,
) -> Result<()> {
    // Dimensions declared directly on this group.
    for d in &group.dimensions {
        let flat_name = flatten_dim_name(&group.path, &d.name);
        add_dimension(metadata, &flat_name, d.len, d.is_unlimited)?;
    }

    // Variables declared directly on this group.
    for v in &group.variables {
        let Some(data_type) = dtype_to_datatype(&v.dtype) else {
            // Enum / compound / opaque / vlen have no faithful DataType mapping;
            // skip rather than misreport the type.
            continue;
        };

        // Ensure every referenced axis exists as a dimension (covers oxinetcdf
        // phony dimensions synthesised for undimensioned datasets, and
        // cross-group DIMENSION_LIST linkage — `axis.group_path` names the
        // group that actually owns the dimension scale, which may differ
        // from `group` itself).
        let mut dim_names = Vec::with_capacity(v.dims.len());
        for axis in &v.dims {
            let flat_dim_name = flatten_dim_name(&axis.group_path, &axis.name);
            if !metadata.dimensions().contains(&flat_dim_name) {
                add_dimension(metadata, &flat_dim_name, axis.len, axis.is_unlimited)?;
            }
            dim_names.push(flat_dim_name);
        }

        let flat_var_name = flatten_h5_path(&v.h5_path);
        let mut variable = Variable::new(&flat_var_name, data_type, dim_names)?;
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
        var_paths.insert(flat_var_name, v.h5_path.clone());
    }

    // Recurse into sub-groups (B3).
    for child in &group.children {
        collect_group_recursive(child, metadata, var_paths)?;
    }

    Ok(())
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
    // Trust the attribute's DECLARED element count (its dataspace), not the raw
    // byte length. Some HDF5/NetCDF-4 writers pad scalar or small attributes
    // with trailing zero bytes up to an 8-byte floor; decoding the whole buffer
    // by dtype-sized chunks would then surface phantom extra elements (e.g. a
    // scalar `_FillValue = -9999` reading back as `[-9999, 0]`). The dataspace
    // shape yields the true count (`Scalar` → `[]` → 1, `Null` → `[0]` → 0,
    // `Simple` → product of dims), so the real payload is `count * dtype_size`
    // bytes and any trailing padding beyond that is ignored.
    let count = usize::try_from(raw.shape().iter().copied().product::<u64>()).unwrap_or(usize::MAX);
    let value = match &raw.dtype {
        Dtype::String { .. } => AttributeValue::Text(attr.as_text().ok()?),
        Dtype::Float { size: 4, order } => {
            AttributeValue::F32(decode_f32(trim_padding(&raw.data, count, 4), order))
        }
        Dtype::Float { size: 8, order } => {
            AttributeValue::F64(decode_f64(trim_padding(&raw.data, count, 8), order))
        }
        Dtype::Int {
            size: 1,
            signed: true,
            ..
        } => AttributeValue::I8(
            trim_padding(&raw.data, count, 1)
                .iter()
                .map(|&b| b as i8)
                .collect(),
        ),
        Dtype::Int {
            size: 1,
            signed: false,
            ..
        } => AttributeValue::U8(trim_padding(&raw.data, count, 1).to_vec()),
        Dtype::Int {
            size: 2,
            signed: true,
            order,
        } => AttributeValue::I16(decode_chunks(
            trim_padding(&raw.data, count, 2),
            order,
            i16::from_le_bytes,
            i16::from_be_bytes,
        )),
        Dtype::Int {
            size: 2,
            signed: false,
            order,
        } => AttributeValue::U16(decode_chunks(
            trim_padding(&raw.data, count, 2),
            order,
            u16::from_le_bytes,
            u16::from_be_bytes,
        )),
        Dtype::Int {
            size: 4,
            signed: true,
            order,
        } => AttributeValue::I32(decode_chunks(
            trim_padding(&raw.data, count, 4),
            order,
            i32::from_le_bytes,
            i32::from_be_bytes,
        )),
        Dtype::Int {
            size: 4,
            signed: false,
            order,
        } => AttributeValue::U32(decode_chunks(
            trim_padding(&raw.data, count, 4),
            order,
            u32::from_le_bytes,
            u32::from_be_bytes,
        )),
        Dtype::Int {
            size: 8,
            signed: true,
            order,
        } => AttributeValue::I64(decode_chunks(
            trim_padding(&raw.data, count, 8),
            order,
            i64::from_le_bytes,
            i64::from_be_bytes,
        )),
        Dtype::Int {
            size: 8,
            signed: false,
            order,
        } => AttributeValue::U64(decode_chunks(
            trim_padding(&raw.data, count, 8),
            order,
            u64::from_le_bytes,
            u64::from_be_bytes,
        )),
        _ => return None,
    };
    Some(value)
}

/// Drop trailing writer padding from an attribute's raw byte buffer.
///
/// Returns the prefix of `data` holding exactly `count` elements of `size`
/// bytes each. When `data` is longer than that declared payload (the writer
/// appended zero padding), the excess is dropped. When `data` is already the
/// right length — or is *shorter* than expected — it is returned verbatim, so
/// genuine multi-element arrays are never truncated and short/degenerate
/// buffers keep their existing lenient decoding.
fn trim_padding(data: &[u8], count: usize, size: usize) -> &[u8] {
    let expected = count.saturating_mul(size);
    if expected > 0 && data.len() > expected {
        &data[..expected]
    } else {
        data
    }
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
            "oxigeo_nc4_reader_{}_{}.nc",
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
        let temp_file = temp_dir.join(format!("oxigeo_reader_garbage_{}.bin", std::process::id()));
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

    /// A NetCDF-4 file with a real HDF5 sub-group must expose the sub-group's
    /// variable under its flattened `"<group>/<var>"` name (not silently drop
    /// it), and that name must be readable end-to-end.
    ///
    /// `oxinetcdf::NcFileWriter` has no group-scoped `def_var` API, so this
    /// fixture is built one level lower, directly via `oxih5::FileWriter`
    /// (which does support single-level sub-groups + group datasets) — the
    /// same real HDF5 primitives a group-aware writer would eventually use.
    #[test]
    fn test_open_real_netcdf4_flattens_subgroup_variable() {
        let path = std::env::temp_dir().join(format!(
            "oxigeo_nc4_reader_subgroup_{}.nc",
            std::process::id()
        ));

        {
            let mut w = oxih5::FileWriter::new();
            w.create_group("sensor1").expect("create group");
            w.write_group_dataset_f64("sensor1", "temp", &[1.0, 2.0, 3.0], &[3])
                .expect("write group dataset");
            w.build(&path).expect("build real hdf5");
        }

        let reader = NetCdfReader::open(&path).expect("open real NetCDF-4");

        // The flattened name must be visible in variable metadata …
        let var = reader
            .variables()
            .get("sensor1/temp")
            .expect("flattened sub-group variable must be visible");
        assert_eq!(var.data_type(), DataType::F64);

        // … and readable end-to-end by that same flattened name.
        let data = reader.read_f64("sensor1/temp").expect("read sub-group var");
        assert_eq!(data, vec![1.0, 2.0, 3.0]);

        let _ = std::fs::remove_file(&path);
    }

    /// CF §8.1 packed-data: a variable stored as raw `i32` with
    /// `scale_factor`/`add_offset`/`_FillValue` attributes must be unpacked
    /// into physical-unit values by `read_f64_cf`/`read_f32_cf`, with the
    /// fill-value element replaced by `NaN` — while the raw, unprocessed
    /// `read_i32` path must keep returning the untouched packed integers.
    #[test]
    fn test_read_f64_cf_unpacks_scale_offset_and_fill_value() {
        let path = std::env::temp_dir().join(format!(
            "oxigeo_nc4_reader_cf_unpack_{}.nc",
            std::process::id()
        ));

        {
            let mut w = oxih5::FileWriter::new();
            w.write_dataset_i32("packed_temp", &[0, 10, 20, -9999], &[4])
                .expect("write packed i32 dataset");
            w.write_f64_attr("packed_temp", "scale_factor", 0.01)
                .expect("scale_factor");
            w.write_f64_attr("packed_temp", "add_offset", 250.0)
                .expect("add_offset");
            w.write_i32_attr("packed_temp", "_FillValue", -9999)
                .expect("_FillValue");
            w.build(&path).expect("build real hdf5");
        }

        let reader = NetCdfReader::open(&path).expect("open real NetCDF-4");

        // Raw read: untouched packed integers, unaffected by the new opt-in
        // unpacking path.
        let raw = reader.read_i32("packed_temp").expect("raw read_i32");
        assert_eq!(raw, vec![0, 10, 20, -9999]);

        // CF-unpacked read: physical = add_offset + scale_factor * packed,
        // with the _FillValue element masked to NaN.
        let physical = reader
            .read_f64_cf("packed_temp")
            .expect("CF-unpacked read_f64_cf");
        assert_eq!(physical.len(), 4);
        assert!((physical[0] - 250.0).abs() < 1e-9);
        assert!((physical[1] - 250.1).abs() < 1e-9);
        assert!((physical[2] - 250.2).abs() < 1e-9);
        assert!(physical[3].is_nan(), "_FillValue element must become NaN");

        // f32 convenience wrapper must agree (within f32 precision).
        let physical_f32 = reader
            .read_f32_cf("packed_temp")
            .expect("CF-unpacked read_f32_cf");
        assert!((physical_f32[0] - 250.0).abs() < 1e-3);
        assert!(physical_f32[3].is_nan());

        let _ = std::fs::remove_file(&path);
    }

    /// A variable with no `scale_factor`/`add_offset`/`_FillValue` attributes
    /// at all must be a pure no-op passthrough through `read_f64_cf`.
    #[test]
    fn test_read_f64_cf_is_noop_without_cf_attributes() {
        let path = std::env::temp_dir().join(format!(
            "oxigeo_nc4_reader_cf_noop_{}.nc",
            std::process::id()
        ));

        {
            let mut w = oxih5::FileWriter::new();
            w.write_dataset_f64("plain", &[1.5, 2.5, 3.5], &[3])
                .expect("write plain f64 dataset");
            w.build(&path).expect("build real hdf5");
        }

        let reader = NetCdfReader::open(&path).expect("open real NetCDF-4");
        let physical = reader.read_f64_cf("plain").expect("read_f64_cf");
        assert_eq!(physical, vec![1.5, 2.5, 3.5]);

        let _ = std::fs::remove_file(&path);
    }

    /// Regression for the oxih5 0.2.1 FileWriter padding quirk: a *scalar* i32
    /// attribute whose 4-byte payload is written with trailing zero padding up
    /// to an 8-byte floor (`_FillValue = -9999` → `[F1 D8 FF FF 00 00 00 00]`)
    /// must decode to exactly one element, not two. The dataspace still
    /// declares a single element, so the phantom trailing zero is dropped.
    #[test]
    fn test_nc_attr_scalar_i32_ignores_writer_padding() {
        let attr = oxinetcdf::NcAttribute::new(oxih5_core::Attribute {
            name: "_FillValue".to_string(),
            dtype: Dtype::Int {
                size: 4,
                signed: true,
                order: ByteOrder::Little,
            },
            dataspace: oxih5_core::Dataspace::Scalar,
            data: vec![0xF1, 0xD8, 0xFF, 0xFF, 0, 0, 0, 0],
        });
        assert_eq!(
            nc_attr_to_value(&attr),
            Some(AttributeValue::I32(vec![-9999]))
        );
    }

    /// A genuine multi-element i32 array (dataspace declares 3 elements, 12
    /// bytes, no padding) must decode to all three values — the padding guard
    /// must never truncate legitimate arrays.
    #[test]
    fn test_nc_attr_multi_element_array_not_truncated() {
        let mut data = Vec::new();
        for v in [1i32, 2, 3] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        let attr = oxinetcdf::NcAttribute::new(oxih5_core::Attribute {
            name: "flags".to_string(),
            dtype: Dtype::Int {
                size: 4,
                signed: true,
                order: ByteOrder::Little,
            },
            dataspace: oxih5_core::Dataspace::Simple {
                dims: vec![3],
                max_dims: None,
            },
            data,
        });
        assert_eq!(
            nc_attr_to_value(&attr),
            Some(AttributeValue::I32(vec![1, 2, 3]))
        );
    }

    /// The same padding guard applies to floating-point scalars: a scalar f32
    /// attribute padded to 8 bytes must decode to exactly one value.
    #[test]
    fn test_nc_attr_scalar_f32_ignores_writer_padding() {
        let mut data = 1.5f32.to_le_bytes().to_vec();
        data.extend_from_slice(&[0, 0, 0, 0]);
        let attr = oxinetcdf::NcAttribute::new(oxih5_core::Attribute {
            name: "scale_factor".to_string(),
            dtype: Dtype::Float {
                size: 4,
                order: ByteOrder::Little,
            },
            dataspace: oxih5_core::Dataspace::Scalar,
            data,
        });
        assert_eq!(
            nc_attr_to_value(&attr),
            Some(AttributeValue::F32(vec![1.5]))
        );
    }
}
