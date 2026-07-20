//! NetCDF file writer implementation.
//!
//! This module provides functionality for writing NetCDF files, including
//! creating dimensions, variables, attributes, and writing data.

use std::path::Path;

use crate::attribute::{Attribute, AttributeValue};
use crate::dimension::Dimension;
use crate::error::{NetCdfError, Result};
use crate::metadata::{NetCdfMetadata, NetCdfVersion};
use crate::variable::{DataType, Variable};

/// Pending variable data to write.
enum PendingData {
    F32(Vec<f32>),
    F64(Vec<f64>),
    I32(Vec<i32>),
    I16(Vec<i16>),
    I8(Vec<i8>),
}

/// NetCDF file writer.
///
/// Provides methods for creating and writing NetCDF files. NetCDF-4 files are
/// written with the Pure-Rust [`oxinetcdf`] backend (real HDF5, no FFI);
/// NetCDF-3 classic files use the optional `netcdf3` backend.
pub struct NetCdfWriter {
    metadata: NetCdfMetadata,
    #[cfg(feature = "netcdf3")]
    dataset_nc3: Option<netcdf3::DataSet>,
    /// Buffered variable data, flushed to the backend on [`NetCdfWriter::close`].
    pending_data: std::collections::HashMap<String, PendingData>,
    #[cfg(feature = "netcdf4")]
    file_nc4: Option<netcdf::FileMut>,
    path: std::path::PathBuf,
    is_define_mode: bool,
}

impl std::fmt::Debug for NetCdfWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetCdfWriter")
            .field("path", &self.path)
            .field("is_define_mode", &self.is_define_mode)
            .finish_non_exhaustive()
    }
}

impl NetCdfWriter {
    /// Create a new NetCDF file for writing.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the NetCDF file
    /// * `version` - NetCDF format version
    ///
    /// # Errors
    ///
    /// Returns error if the file cannot be created.
    pub fn create(path: impl AsRef<Path>, version: NetCdfVersion) -> Result<Self> {
        let path = path.as_ref();

        if version.is_netcdf4() {
            Self::new_netcdf4(path, version)
        } else {
            #[cfg(feature = "netcdf3")]
            {
                Self::create_netcdf3(path)
            }
            #[cfg(not(feature = "netcdf3"))]
            {
                Err(NetCdfError::FeatureNotEnabled {
                    feature: "netcdf3".to_string(),
                    message: "Enable 'netcdf3' feature to write NetCDF-3 files".to_string(),
                })
            }
        }
    }

    /// Create a NetCDF-3 Classic file.
    ///
    /// # Errors
    ///
    /// Returns error if the file cannot be created.
    #[cfg(feature = "netcdf3")]
    pub fn create_netcdf3(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let dataset = netcdf3::DataSet::new();
        let metadata = NetCdfMetadata::new_classic();

        Ok(Self {
            metadata,
            dataset_nc3: Some(dataset),
            pending_data: std::collections::HashMap::new(),
            #[cfg(feature = "netcdf4")]
            file_nc4: None,
            path: path.to_path_buf(),
            is_define_mode: true,
        })
    }

    /// Create a NetCDF-4 (HDF5) file.
    ///
    /// The file is written with the Pure-Rust [`oxinetcdf`] backend on
    /// [`NetCdfWriter::close`]. Defaults to the full NetCDF-4 model.
    ///
    /// # Errors
    ///
    /// Returns error if the file cannot be created.
    pub fn create_netcdf4(path: impl AsRef<Path>) -> Result<Self> {
        Self::new_netcdf4(path, NetCdfVersion::NetCdf4)
    }

    /// Internal constructor for the Pure-Rust NetCDF-4 backend.
    fn new_netcdf4(path: impl AsRef<Path>, version: NetCdfVersion) -> Result<Self> {
        Ok(Self {
            metadata: NetCdfMetadata::new(version),
            #[cfg(feature = "netcdf3")]
            dataset_nc3: None,
            pending_data: std::collections::HashMap::new(),
            #[cfg(feature = "netcdf4")]
            file_nc4: None,
            path: path.as_ref().to_path_buf(),
            is_define_mode: true,
        })
    }

    /// Get the file metadata.
    #[must_use]
    pub const fn metadata(&self) -> &NetCdfMetadata {
        &self.metadata
    }

    /// Add a dimension.
    ///
    /// # Errors
    ///
    /// Returns error if not in define mode or dimension already exists.
    pub fn add_dimension(&mut self, dimension: Dimension) -> Result<()> {
        if !self.is_define_mode {
            return Err(NetCdfError::Other(
                "Cannot add dimension outside of define mode".to_string(),
            ));
        }

        // Add to metadata
        self.metadata.dimensions_mut().add(dimension.clone())?;

        // Add to dataset
        #[cfg(feature = "netcdf3")]
        if let Some(ref mut dataset) = self.dataset_nc3 {
            if dimension.is_unlimited() {
                dataset.set_unlimited_dim(dimension.name(), dimension.len())?;
            } else {
                dataset.add_fixed_dim(dimension.name(), dimension.len())?;
            }
        }

        Ok(())
    }

    /// Add a variable.
    ///
    /// # Errors
    ///
    /// Returns error if not in define mode, variable already exists,
    /// or variable dimensions don't exist.
    pub fn add_variable(&mut self, variable: Variable) -> Result<()> {
        if !self.is_define_mode {
            return Err(NetCdfError::Other(
                "Cannot add variable outside of define mode".to_string(),
            ));
        }

        // Validate dimensions exist
        for dim_name in variable.dimension_names() {
            if !self.metadata.dimensions().contains(dim_name) {
                return Err(NetCdfError::DimensionNotFound {
                    name: dim_name.clone(),
                });
            }
        }

        // Add to metadata
        self.metadata.variables_mut().add(variable.clone())?;

        // Add to dataset
        #[cfg(feature = "netcdf3")]
        if let Some(ref mut dataset) = self.dataset_nc3 {
            let nc3_type = Self::convert_datatype_to_nc3(variable.data_type())?;
            let dims: Vec<&str> = variable
                .dimension_names()
                .iter()
                .map(|s| s.as_str())
                .collect();
            dataset.add_var(variable.name(), &dims, nc3_type)?;
        }

        Ok(())
    }

    /// Add a global attribute.
    ///
    /// # Errors
    ///
    /// Returns error if not in define mode.
    pub fn add_global_attribute(&mut self, attribute: Attribute) -> Result<()> {
        if !self.is_define_mode {
            return Err(NetCdfError::Other(
                "Cannot add global attribute outside of define mode".to_string(),
            ));
        }

        // Add to metadata
        self.metadata
            .global_attributes_mut()
            .add(attribute.clone())?;

        // Add to dataset
        #[cfg(feature = "netcdf3")]
        if let Some(ref mut dataset) = self.dataset_nc3 {
            Self::write_global_attribute_nc3(dataset, &attribute)?;
        }

        Ok(())
    }

    /// Add a variable attribute.
    ///
    /// # Errors
    ///
    /// Returns error if not in define mode or variable doesn't exist.
    pub fn add_variable_attribute(&mut self, var_name: &str, attribute: Attribute) -> Result<()> {
        if !self.is_define_mode {
            return Err(NetCdfError::Other(
                "Cannot add variable attribute outside of define mode".to_string(),
            ));
        }

        // Add to metadata
        let var = self
            .metadata
            .variables_mut()
            .get_mut(var_name)
            .ok_or_else(|| NetCdfError::VariableNotFound {
                name: var_name.to_string(),
            })?;
        var.attributes_mut().add(attribute.clone())?;

        // Add to dataset
        #[cfg(feature = "netcdf3")]
        if let Some(ref mut dataset) = self.dataset_nc3 {
            Self::write_variable_attribute_nc3(dataset, var_name, &attribute)?;
        }

        Ok(())
    }

    /// End define mode and enter data mode.
    ///
    /// After calling this, you can write data but cannot add dimensions,
    /// variables, or attributes.
    ///
    /// # Errors
    ///
    /// Returns error if already in data mode or if metadata is invalid.
    pub fn end_define_mode(&mut self) -> Result<()> {
        if !self.is_define_mode {
            return Err(NetCdfError::Other("Already in data mode".to_string()));
        }

        // Validate metadata
        self.metadata.validate()?;

        self.is_define_mode = false;
        Ok(())
    }

    /// Write f32 data to a variable.
    ///
    /// # Errors
    ///
    /// Returns error if in define mode, variable doesn't exist,
    /// or data size doesn't match variable size.
    pub fn write_f32(&mut self, var_name: &str, data: &[f32]) -> Result<()> {
        if self.is_define_mode {
            return Err(NetCdfError::Other(
                "Cannot write data in define mode. Call end_define_mode() first.".to_string(),
            ));
        }

        // Get variable
        let var = self.metadata.variables().get(var_name).ok_or_else(|| {
            NetCdfError::VariableNotFound {
                name: var_name.to_string(),
            }
        })?;

        // Validate data size
        let expected_size = var.size(self.metadata.dimensions())?;
        if data.len() != expected_size {
            return Err(NetCdfError::InvalidShape {
                message: format!(
                    "Data size {} does not match variable size {}",
                    data.len(),
                    expected_size
                ),
            });
        }

        // Store pending data for later write
        self.pending_data
            .insert(var_name.to_string(), PendingData::F32(data.to_vec()));

        Ok(())
    }

    /// Write f64 data to a variable.
    ///
    /// # Errors
    ///
    /// Returns error if in define mode, variable doesn't exist,
    /// or data size doesn't match variable size.
    pub fn write_f64(&mut self, var_name: &str, data: &[f64]) -> Result<()> {
        if self.is_define_mode {
            return Err(NetCdfError::Other(
                "Cannot write data in define mode. Call end_define_mode() first.".to_string(),
            ));
        }

        let var = self.metadata.variables().get(var_name).ok_or_else(|| {
            NetCdfError::VariableNotFound {
                name: var_name.to_string(),
            }
        })?;

        let expected_size = var.size(self.metadata.dimensions())?;
        if data.len() != expected_size {
            return Err(NetCdfError::InvalidShape {
                message: format!(
                    "Data size {} does not match variable size {}",
                    data.len(),
                    expected_size
                ),
            });
        }

        self.pending_data
            .insert(var_name.to_string(), PendingData::F64(data.to_vec()));

        Ok(())
    }

    /// Write i32 data to a variable.
    ///
    /// # Errors
    ///
    /// Returns error if in define mode, variable doesn't exist,
    /// or data size doesn't match variable size.
    pub fn write_i32(&mut self, var_name: &str, data: &[i32]) -> Result<()> {
        if self.is_define_mode {
            return Err(NetCdfError::Other(
                "Cannot write data in define mode. Call end_define_mode() first.".to_string(),
            ));
        }

        let var = self.metadata.variables().get(var_name).ok_or_else(|| {
            NetCdfError::VariableNotFound {
                name: var_name.to_string(),
            }
        })?;

        let expected_size = var.size(self.metadata.dimensions())?;
        if data.len() != expected_size {
            return Err(NetCdfError::InvalidShape {
                message: format!(
                    "Data size {} does not match variable size {}",
                    data.len(),
                    expected_size
                ),
            });
        }

        self.pending_data
            .insert(var_name.to_string(), PendingData::I32(data.to_vec()));

        Ok(())
    }

    /// Finalize and close the file, flushing all buffered data to disk.
    ///
    /// NetCDF-4 files are written with the Pure-Rust [`oxinetcdf`] backend
    /// (real HDF5, no FFI); NetCDF-3 classic files use the optional `netcdf3`
    /// backend. The writer never silently succeeds without producing a file —
    /// unsupported constructs return a typed error.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written, or if a requested
    /// construct is not supported by the active backend.
    pub fn close(self) -> Result<()> {
        if self.metadata.version().is_netcdf4() {
            return self.write_netcdf4();
        }

        #[cfg(feature = "netcdf3")]
        {
            self.close_netcdf3()
        }
        #[cfg(not(feature = "netcdf3"))]
        {
            Err(NetCdfError::FeatureNotEnabled {
                feature: "netcdf3".to_string(),
                message: "Enable the 'netcdf3' feature to write NetCDF-3 files".to_string(),
            })
        }
    }

    /// Write the buffered dataset as a NetCDF-3 classic file.
    #[cfg(feature = "netcdf3")]
    fn close_netcdf3(self) -> Result<()> {
        if let Some(dataset) = self.dataset_nc3 {
            // Remove the file if it already exists (e.g., created by NamedTempFile)
            if self.path.exists() {
                std::fs::remove_file(&self.path).map_err(|e| {
                    NetCdfError::Io(format!("Failed to remove existing file: {}", e))
                })?;
            }
            let mut writer = netcdf3::FileWriter::create_new(&self.path)?;
            writer.set_def(&dataset, netcdf3::Version::Classic, 0)?;

            // Write all pending data
            for (var_name, data) in &self.pending_data {
                match data {
                    PendingData::F32(values) => {
                        writer.write_var_f32(var_name, values)?;
                    }
                    PendingData::F64(values) => {
                        writer.write_var_f64(var_name, values)?;
                    }
                    PendingData::I32(values) => {
                        writer.write_var_i32(var_name, values)?;
                    }
                    PendingData::I16(values) => {
                        writer.write_var_i16(var_name, values)?;
                    }
                    PendingData::I8(values) => {
                        writer.write_var_i8(var_name, values)?;
                    }
                }
            }

            writer.close()?;
        }
        Ok(())
    }

    /// Write the buffered dataset as a real NetCDF-4 (HDF5) file using the
    /// Pure-Rust [`oxinetcdf`] backend.
    ///
    /// # Errors
    ///
    /// Returns a typed error when a construct cannot be represented by the
    /// Pure-Rust backend (e.g. explicit coordinate-variable values or a
    /// non-string attribute), rather than writing an incomplete file.
    fn write_netcdf4(self) -> Result<()> {
        use oxinetcdf::{NcDimId, NcFileWriter, VarOrGroup};

        let mut writer = NcFileWriter::new();
        if self.metadata.version() == NetCdfVersion::NetCdf4Classic {
            writer.set_classic_mode();
        }

        // Global attributes (string only — the NetCDF-4 backend cannot encode
        // numeric global attributes).
        for attr in self.metadata.global_attributes().iter() {
            match attr.value() {
                AttributeValue::Text(s) => {
                    writer
                        .put_att_str(VarOrGroup::Root, attr.name(), s)
                        .map_err(map_ncw_err)?;
                }
                other => {
                    return Err(NetCdfError::AttributeError(format!(
                        "Pure-Rust NetCDF-4 backend supports only string global attributes; \
                         '{}' ({}) cannot be written",
                        attr.name(),
                        other.type_name()
                    )));
                }
            }
        }

        // Dimensions.
        let mut dim_ids: std::collections::HashMap<String, NcDimId> =
            std::collections::HashMap::new();
        for dim in self.metadata.dimensions().iter() {
            let id = if dim.is_unlimited() {
                writer
                    .def_dim_unlimited(dim.name(), dim.len())
                    .map_err(map_ncw_err)?
            } else {
                writer.def_dim(dim.name(), dim.len()).map_err(map_ncw_err)?
            };
            dim_ids.insert(dim.name().to_string(), id);
        }

        // Variables.
        for var in self.metadata.variables().iter() {
            // A pure coordinate variable (1-D, named after its own dimension)
            // is written automatically by the backend as the dimension index.
            let is_pure_coord = var.dimension_names().len() == 1
                && var.dimension_names()[0] == var.name()
                && dim_ids.contains_key(var.name());
            if is_pure_coord {
                if self.pending_data.contains_key(var.name()) {
                    return Err(NetCdfError::Other(format!(
                        "Pure-Rust NetCDF-4 backend cannot write explicit values for \
                         coordinate variable '{}'; the dimension index is written automatically",
                        var.name()
                    )));
                }
                if !var.attributes().is_empty() {
                    return Err(NetCdfError::Other(format!(
                        "Pure-Rust NetCDF-4 backend cannot attach attributes to coordinate \
                         variable '{}'",
                        var.name()
                    )));
                }
                continue;
            }

            let nc_type = datatype_to_nctype(var.data_type())?;
            let dims: Vec<NcDimId> = var
                .dimension_names()
                .iter()
                .map(|name| {
                    dim_ids
                        .get(name)
                        .copied()
                        .ok_or_else(|| NetCdfError::DimensionNotFound { name: name.clone() })
                })
                .collect::<Result<_>>()?;
            let var_id = writer
                .def_var(var.name(), &dims, nc_type)
                .map_err(map_ncw_err)?;

            // Data.
            if let Some(pending) = self.pending_data.get(var.name()) {
                match pending {
                    PendingData::F32(v) => {
                        let widened: Vec<f64> = v.iter().map(|&x| f64::from(x)).collect();
                        writer.put_var_f64(var_id, &widened).map_err(map_ncw_err)?;
                    }
                    PendingData::F64(v) => {
                        writer.put_var_f64(var_id, v).map_err(map_ncw_err)?;
                    }
                    PendingData::I32(v) => {
                        writer.put_var_i32(var_id, v).map_err(map_ncw_err)?;
                    }
                    PendingData::I16(_) | PendingData::I8(_) => {
                        return Err(NetCdfError::Other(format!(
                            "Pure-Rust NetCDF-4 backend cannot write i16/i8 data for '{}'",
                            var.name()
                        )));
                    }
                }
            }

            // String attributes.
            for attr in var.attributes().iter() {
                match attr.value() {
                    AttributeValue::Text(s) => {
                        writer
                            .put_att_str(VarOrGroup::Var(var_id), attr.name(), s)
                            .map_err(map_ncw_err)?;
                    }
                    other => {
                        return Err(NetCdfError::AttributeError(format!(
                            "Pure-Rust NetCDF-4 backend supports only string variable attributes; \
                             '{}' ({}) on '{}' cannot be written",
                            attr.name(),
                            other.type_name(),
                            var.name()
                        )));
                    }
                }
            }
        }

        // Overwrite any pre-existing file (e.g. a NamedTempFile placeholder).
        if self.path.exists() {
            std::fs::remove_file(&self.path)
                .map_err(|e| NetCdfError::Io(format!("failed to remove existing file: {e}")))?;
        }
        writer.close(&self.path).map_err(map_ncw_err)?;
        Ok(())
    }

    /// Convert our data type to NetCDF-3 data type.
    #[cfg(feature = "netcdf3")]
    fn convert_datatype_to_nc3(dtype: DataType) -> Result<netcdf3::DataType> {
        use netcdf3::DataType as Nc3Type;

        match dtype {
            DataType::I8 => Ok(Nc3Type::I8),
            DataType::I16 => Ok(Nc3Type::I16),
            DataType::I32 => Ok(Nc3Type::I32),
            DataType::F32 => Ok(Nc3Type::F32),
            DataType::F64 => Ok(Nc3Type::F64),
            DataType::Char => Ok(Nc3Type::U8), // Character data uses U8 in netcdf3 v0.6
            _ => Err(NetCdfError::DataTypeMismatch {
                expected: "NetCDF-3 compatible type".to_string(),
                found: dtype.name().to_string(),
            }),
        }
    }

    /// Write a global attribute to NetCDF-3 dataset.
    #[cfg(feature = "netcdf3")]
    fn write_global_attribute_nc3(dataset: &mut netcdf3::DataSet, attr: &Attribute) -> Result<()> {
        match attr.value() {
            AttributeValue::Text(s) => {
                dataset.add_global_attr_string(attr.name(), s)?;
            }
            AttributeValue::I8(v) => {
                dataset.add_global_attr_i8(attr.name(), v.clone())?;
            }
            AttributeValue::U8(v) => {
                dataset.add_global_attr_u8(attr.name(), v.clone())?;
            }
            AttributeValue::I16(v) => {
                dataset.add_global_attr_i16(attr.name(), v.clone())?;
            }
            AttributeValue::I32(v) => {
                dataset.add_global_attr_i32(attr.name(), v.clone())?;
            }
            AttributeValue::F32(v) => {
                dataset.add_global_attr_f32(attr.name(), v.clone())?;
            }
            AttributeValue::F64(v) => {
                dataset.add_global_attr_f64(attr.name(), v.clone())?;
            }
            _ => {
                return Err(NetCdfError::AttributeError(
                    "Attribute type not supported in NetCDF-3".to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Write a variable attribute to NetCDF-3 dataset.
    #[cfg(feature = "netcdf3")]
    fn write_variable_attribute_nc3(
        dataset: &mut netcdf3::DataSet,
        var_name: &str,
        attr: &Attribute,
    ) -> Result<()> {
        match attr.value() {
            AttributeValue::Text(s) => {
                dataset.add_var_attr_string(var_name, attr.name(), s)?;
            }
            AttributeValue::I8(v) => {
                dataset.add_var_attr_i8(var_name, attr.name(), v.clone())?;
            }
            AttributeValue::U8(v) => {
                dataset.add_var_attr_u8(var_name, attr.name(), v.clone())?;
            }
            AttributeValue::I16(v) => {
                dataset.add_var_attr_i16(var_name, attr.name(), v.clone())?;
            }
            AttributeValue::I32(v) => {
                dataset.add_var_attr_i32(var_name, attr.name(), v.clone())?;
            }
            AttributeValue::F32(v) => {
                dataset.add_var_attr_f32(var_name, attr.name(), v.clone())?;
            }
            AttributeValue::F64(v) => {
                dataset.add_var_attr_f64(var_name, attr.name(), v.clone())?;
            }
            _ => {
                return Err(NetCdfError::AttributeError(
                    "Attribute type not supported in NetCDF-3".to_string(),
                ));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// NetCDF-4 (oxinetcdf) mapping helpers
// ---------------------------------------------------------------------------

/// Map the crate's [`DataType`] to the oxinetcdf NetCDF-4 type vocabulary.
///
/// # Errors
///
/// Returns [`NetCdfError::DataTypeMismatch`] for types the Pure-Rust NetCDF-4
/// backend cannot write.
fn datatype_to_nctype(dtype: DataType) -> Result<oxinetcdf::NcType> {
    use oxinetcdf::NcType;
    Ok(match dtype {
        DataType::F32 => NcType::Float32,
        DataType::F64 => NcType::Float64,
        DataType::I32 => NcType::Int32,
        DataType::I64 => NcType::Int64,
        DataType::U8 => NcType::UInt8,
        DataType::String => NcType::String,
        other => {
            return Err(NetCdfError::DataTypeMismatch {
                expected: "F32/F64/I32/I64/U8/String".to_string(),
                found: other.name().to_string(),
            });
        }
    })
}

/// Map an oxinetcdf writer error into the crate's error type.
fn map_ncw_err(e: oxinetcdf::NcError) -> NetCdfError {
    NetCdfError::Other(format!("NetCDF-4 writer error: {e}"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn test_netcdf4_write_read_roundtrip() {
        use crate::reader::NetCdfReader;

        let path =
            std::env::temp_dir().join(format!("oxigeo_nc4_writer_{}.nc", std::process::id()));

        let mut writer =
            NetCdfWriter::create(&path, NetCdfVersion::NetCdf4).expect("create NetCDF-4");
        writer
            .add_dimension(Dimension::new("x", 4).expect("dim"))
            .expect("add dim");
        writer
            .add_variable(Variable::new("temp", DataType::F64, vec!["x".to_string()]).expect("var"))
            .expect("add var");
        writer
            .add_variable_attribute(
                "temp",
                Attribute::new("units", AttributeValue::text("kelvin")).expect("attr"),
            )
            .expect("add var attr");
        writer
            .add_global_attribute(
                Attribute::new("Conventions", AttributeValue::text("CF-1.8")).expect("attr"),
            )
            .expect("add global attr");
        writer.end_define_mode().expect("end define");
        writer
            .write_f64("temp", &[1.0, 2.0, 3.0, 4.0])
            .expect("write data");
        // close() must write a real HDF5/NetCDF-4 file, not silently succeed.
        writer.close().expect("close writes real file");

        // Read it back with the real oxinetcdf-backed reader.
        let reader = NetCdfReader::open(&path).expect("open real NetCDF-4");
        assert!(reader.version().is_netcdf4());
        let temp = reader.variables().get("temp").expect("temp var");
        assert_eq!(temp.data_type(), DataType::F64);
        let units = temp.attributes().get("units").expect("units attr");
        assert_eq!(units.value().as_text().expect("text"), "kelvin");

        let data = reader.read_f64("temp").expect("read temp");
        assert_eq!(data, vec![1.0, 2.0, 3.0, 4.0]);

        let cf = reader.cf_metadata().expect("cf metadata");
        assert_eq!(cf.conventions.as_deref(), Some("CF-1.8"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_netcdf4_rejects_numeric_global_attribute() {
        let path = std::env::temp_dir().join(format!(
            "oxigeo_nc4_writer_numattr_{}.nc",
            std::process::id()
        ));

        let mut writer =
            NetCdfWriter::create(&path, NetCdfVersion::NetCdf4).expect("create NetCDF-4");
        writer
            .add_global_attribute(Attribute::new("answer", AttributeValue::i32(42)).expect("attr"))
            .expect("add global attr");
        writer.end_define_mode().expect("end define");

        // A numeric global attribute is not representable: fail loud, no file.
        let result = writer.close();
        assert!(matches!(result, Err(NetCdfError::AttributeError(_))));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_data_type_conversion() {
        #[cfg(feature = "netcdf3")]
        {
            use netcdf3::DataType as Nc3Type;
            assert_eq!(
                NetCdfWriter::convert_datatype_to_nc3(DataType::F32).expect("F32 conversion"),
                Nc3Type::F32
            );
            assert_eq!(
                NetCdfWriter::convert_datatype_to_nc3(DataType::F64).expect("F64 conversion"),
                Nc3Type::F64
            );
            assert_eq!(
                NetCdfWriter::convert_datatype_to_nc3(DataType::I32).expect("I32 conversion"),
                Nc3Type::I32
            );

            // U16 is not supported in NetCDF-3
            assert!(NetCdfWriter::convert_datatype_to_nc3(DataType::U16).is_err());
        }
    }
}
