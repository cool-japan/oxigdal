//! NetCDF file reader implementation.
//!
//! This module provides functionality for reading NetCDF files, including
//! dimensions, variables, attributes, and data.

use std::path::Path;

use crate::attribute::{Attribute, AttributeValue, Attributes};
use crate::dimension::{Dimension, Dimensions};
use crate::error::{NetCdfError, Result};
use crate::metadata::{CfMetadata, NetCdfMetadata, NetCdfVersion};
use crate::variable::{DataType, Variable, Variables};

#[cfg(feature = "netcdf3")]
use std::cell::RefCell;

/// NetCDF file reader.
///
/// Provides methods for reading NetCDF files, including metadata and data.
pub struct NetCdfReader {
    metadata: NetCdfMetadata,
    #[cfg(feature = "netcdf3")]
    file_nc3: Option<RefCell<netcdf3::FileReader>>,
    #[cfg(feature = "netcdf4")]
    file_nc4: Option<netcdf::File>,
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
    /// Automatically detects the file format (NetCDF-3 or NetCDF-4).
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the NetCDF file
    ///
    /// # Errors
    ///
    /// Returns error if the file cannot be opened or read.
    #[allow(unused_variables)]
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        // Try NetCDF-3 first (Pure Rust)
        #[cfg(feature = "netcdf3")]
        {
            if let Ok(file) = netcdf3::FileReader::open(path) {
                return Self::from_netcdf3(file);
            }
        }

        // Try NetCDF-4 if feature is enabled
        #[cfg(feature = "netcdf4")]
        {
            if let Ok(file) = netcdf::open(path) {
                return Self::from_netcdf4(file);
            }
        }

        // If neither worked, return error
        #[cfg(not(feature = "netcdf3"))]
        #[cfg(not(feature = "netcdf4"))]
        {
            Err(NetCdfError::FeatureNotEnabled {
                feature: "netcdf3 or netcdf4".to_string(),
                message: "Enable 'netcdf3' or 'netcdf4' feature to read NetCDF files".to_string(),
            })
        }

        #[cfg(feature = "netcdf3")]
        #[cfg(not(feature = "netcdf4"))]
        {
            Err(NetCdfError::InvalidFormat(
                "File is not a valid NetCDF-3 file".to_string(),
            ))
        }

        #[cfg(feature = "netcdf4")]
        #[cfg(not(feature = "netcdf3"))]
        {
            Err(NetCdfError::InvalidFormat(
                "File is not a valid NetCDF-4 file".to_string(),
            ))
        }

        #[cfg(feature = "netcdf3")]
        #[cfg(feature = "netcdf4")]
        {
            Err(NetCdfError::InvalidFormat(
                "File is not a valid NetCDF file".to_string(),
            ))
        }
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
            file_nc3: Some(RefCell::new(file)),
            #[cfg(feature = "netcdf4")]
            file_nc4: None,
        })
    }

    /// Create a reader from a NetCDF-4 file.
    ///
    /// # Errors
    ///
    /// Returns error if metadata cannot be read.
    #[cfg(feature = "netcdf4")]
    pub fn from_netcdf4(file: netcdf::File) -> Result<Self> {
        let metadata = Self::read_metadata_nc4(&file)?;
        Ok(Self {
            metadata,
            file_nc3: None,
            file_nc4: Some(file),
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

    /// Read metadata from NetCDF-4 file.
    #[cfg(feature = "netcdf4")]
    fn read_metadata_nc4(_file: &netcdf::File) -> Result<NetCdfMetadata> {
        // NetCDF-4 support is placeholder for now
        Err(NetCdfError::NetCdf4NotAvailable)
    }

    /// Read variable data as f32.
    ///
    /// # Errors
    ///
    /// Returns error if variable not found or data cannot be read.
    #[allow(unused_variables)]
    pub fn read_f32(&self, var_name: &str) -> Result<Vec<f32>> {
        #[cfg(feature = "netcdf3")]
        if let Some(ref file_cell) = self.file_nc3 {
            return Self::read_f32_nc3(&mut file_cell.borrow_mut(), var_name);
        }

        #[cfg(feature = "netcdf4")]
        if let Some(ref _file) = self.file_nc4 {
            return Err(NetCdfError::NetCdf4NotAvailable);
        }

        Err(NetCdfError::FeatureNotEnabled {
            feature: "netcdf3 or netcdf4".to_string(),
            message: "No reader available".to_string(),
        })
    }

    /// Read variable data as f64.
    ///
    /// # Errors
    ///
    /// Returns error if variable not found or data cannot be read.
    #[allow(unused_variables)]
    pub fn read_f64(&self, var_name: &str) -> Result<Vec<f64>> {
        #[cfg(feature = "netcdf3")]
        if let Some(ref file_cell) = self.file_nc3 {
            return Self::read_f64_nc3(&mut file_cell.borrow_mut(), var_name);
        }

        #[cfg(feature = "netcdf4")]
        if let Some(ref _file) = self.file_nc4 {
            return Err(NetCdfError::NetCdf4NotAvailable);
        }

        Err(NetCdfError::FeatureNotEnabled {
            feature: "netcdf3 or netcdf4".to_string(),
            message: "No reader available".to_string(),
        })
    }

    /// Read variable data as i32.
    ///
    /// # Errors
    ///
    /// Returns error if variable not found or data cannot be read.
    #[allow(unused_variables)]
    pub fn read_i32(&self, var_name: &str) -> Result<Vec<i32>> {
        #[cfg(feature = "netcdf3")]
        if let Some(ref file_cell) = self.file_nc3 {
            return Self::read_i32_nc3(&mut file_cell.borrow_mut(), var_name);
        }

        #[cfg(feature = "netcdf4")]
        if let Some(ref _file) = self.file_nc4 {
            return Err(NetCdfError::NetCdf4NotAvailable);
        }

        Err(NetCdfError::FeatureNotEnabled {
            feature: "netcdf3 or netcdf4".to_string(),
            message: "No reader available".to_string(),
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

        // Verify data type matches
        if data_type != Nc3Type::F32 {
            return Err(NetCdfError::DataTypeMismatch {
                expected: "F32".to_string(),
                found: format!("{:?}", data_type),
            });
        }

        // Read the data using the mutable reference
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

        // Verify data type matches
        if data_type != Nc3Type::F64 {
            return Err(NetCdfError::DataTypeMismatch {
                expected: "F64".to_string(),
                found: format!("{:?}", data_type),
            });
        }

        // Read the data using the mutable reference
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

        // Verify data type matches
        if data_type != Nc3Type::I32 {
            return Err(NetCdfError::DataTypeMismatch {
                expected: "I32".to_string(),
                found: format!("{:?}", data_type),
            });
        }

        // Read the data using the mutable reference
        let data = file.read_var_i32(var_name)?;
        Ok(data)
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
                NetCdfReader::convert_datatype_nc3(Nc3Type::F32)
                    .expect("Failed to convert NetCDF datatype in test"),
                DataType::F32
            );
            assert_eq!(
                NetCdfReader::convert_datatype_nc3(Nc3Type::F64)
                    .expect("Failed to convert NetCDF datatype in test"),
                DataType::F64
            );
            assert_eq!(
                NetCdfReader::convert_datatype_nc3(Nc3Type::I32)
                    .expect("Failed to convert NetCDF datatype in test"),
                DataType::I32
            );
        }
    }
}
