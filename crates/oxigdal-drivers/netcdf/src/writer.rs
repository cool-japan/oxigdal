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
#[cfg(feature = "netcdf3")]
enum PendingData {
    F32(Vec<f32>),
    F64(Vec<f64>),
    I32(Vec<i32>),
    I16(Vec<i16>),
    I8(Vec<i8>),
}

/// NetCDF file writer.
///
/// Provides methods for creating and writing NetCDF files.
pub struct NetCdfWriter {
    metadata: NetCdfMetadata,
    #[cfg(feature = "netcdf3")]
    dataset_nc3: Option<netcdf3::DataSet>,
    #[cfg(feature = "netcdf3")]
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
    #[allow(unused_variables)]
    pub fn create(path: impl AsRef<Path>, version: NetCdfVersion) -> Result<Self> {
        let path = path.as_ref();

        if version.is_netcdf4() {
            #[cfg(feature = "netcdf4")]
            {
                Self::create_netcdf4(path)
            }
            #[cfg(not(feature = "netcdf4"))]
            {
                Err(NetCdfError::NetCdf4NotAvailable)
            }
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

    /// Create a NetCDF-4 file.
    ///
    /// # Errors
    ///
    /// Returns error if the file cannot be created.
    #[cfg(feature = "netcdf4")]
    pub fn create_netcdf4(_path: impl AsRef<Path>) -> Result<Self> {
        // NetCDF-4 support is placeholder for now
        Err(NetCdfError::NetCdf4NotAvailable)
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
        #[cfg(feature = "netcdf3")]
        {
            self.pending_data
                .insert(var_name.to_string(), PendingData::F32(data.to_vec()));
        }

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

        #[cfg(feature = "netcdf3")]
        {
            self.pending_data
                .insert(var_name.to_string(), PendingData::F64(data.to_vec()));
        }

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

        #[cfg(feature = "netcdf3")]
        {
            self.pending_data
                .insert(var_name.to_string(), PendingData::I32(data.to_vec()));
        }

        Ok(())
    }

    /// Finalize and close the file.
    ///
    /// This method consumes the writer and ensures all data is written to disk.
    ///
    /// # Errors
    ///
    /// Returns error if file cannot be closed.
    #[cfg(feature = "netcdf3")]
    pub fn close(self) -> Result<()> {
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

    /// Finalize and close the file.
    #[cfg(not(feature = "netcdf3"))]
    pub fn close(self) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
