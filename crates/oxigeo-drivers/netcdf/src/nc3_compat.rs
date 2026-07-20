//! Compatibility layer for netcdf3 v0.6 API.
//!
//! This module provides helper functions to work with the netcdf3 v0.6 DataSet API.

#[cfg(feature = "netcdf3")]
use netcdf3::{DataSet, DataType as Nc3Type, DataVector};

use crate::attribute::{Attribute, AttributeValue};
use crate::dimension::Dimension;
use crate::error::{NetCdfError, Result};
use crate::variable::{DataType, Variable};

/// Convert netcdf3 DataType to our DataType.
#[cfg(feature = "netcdf3")]
pub fn convert_datatype_from_nc3(nc3_type: Nc3Type) -> Result<DataType> {
    match nc3_type {
        Nc3Type::I8 => Ok(DataType::I8),
        Nc3Type::U8 => Ok(DataType::Char), // U8 in netcdf3 v0.6 is for character data
        Nc3Type::I16 => Ok(DataType::I16),
        Nc3Type::I32 => Ok(DataType::I32),
        Nc3Type::F32 => Ok(DataType::F32),
        Nc3Type::F64 => Ok(DataType::F64),
    }
}

/// Convert our DataType to netcdf3 DataType.
#[cfg(feature = "netcdf3")]
pub fn convert_datatype_to_nc3(dtype: DataType) -> Result<Nc3Type> {
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

/// Read a global attribute from a DataSet.
#[cfg(feature = "netcdf3")]
pub fn read_global_attribute(dataset: &DataSet, attr_name: &str) -> Result<Option<Attribute>> {
    if let Some(attr) = dataset.get_global_attr(attr_name) {
        let value = convert_attr_value(attr)?;
        Ok(Some(Attribute::new(attr_name, value)?))
    } else {
        Ok(None)
    }
}

/// Read a variable attribute from a DataSet.
#[cfg(feature = "netcdf3")]
pub fn read_variable_attribute(
    dataset: &DataSet,
    var_name: &str,
    attr_name: &str,
) -> Result<Option<Attribute>> {
    if let Some(attr) = dataset.get_var_attr(var_name, attr_name) {
        let value = convert_attr_value(attr)?;
        Ok(Some(Attribute::new(attr_name, value)?))
    } else {
        Ok(None)
    }
}

/// Convert netcdf3 attribute value to our AttributeValue.
#[cfg(feature = "netcdf3")]
fn convert_attr_value(attr: &netcdf3::Attribute) -> Result<AttributeValue> {
    // In netcdf3 v0.6, use type-specific getters instead of value()
    match attr.data_type() {
        Nc3Type::I8 => {
            if let Some(v) = attr.get_i8() {
                Ok(AttributeValue::i8_array(v.to_vec()))
            } else {
                Err(NetCdfError::DataTypeMismatch {
                    expected: "I8".to_string(),
                    found: "attribute data".to_string(),
                })
            }
        }
        Nc3Type::U8 => {
            if let Some(v) = attr.get_u8() {
                // Try to decode as UTF-8 string
                match String::from_utf8(v.to_vec()) {
                    Ok(s) => Ok(AttributeValue::text(s)),
                    Err(_) => Ok(AttributeValue::u8_array(v.to_vec())),
                }
            } else {
                Err(NetCdfError::DataTypeMismatch {
                    expected: "U8".to_string(),
                    found: "attribute data".to_string(),
                })
            }
        }
        Nc3Type::I16 => {
            if let Some(v) = attr.get_i16() {
                Ok(AttributeValue::i16_array(v.to_vec()))
            } else {
                Err(NetCdfError::DataTypeMismatch {
                    expected: "I16".to_string(),
                    found: "attribute data".to_string(),
                })
            }
        }
        Nc3Type::I32 => {
            if let Some(v) = attr.get_i32() {
                Ok(AttributeValue::i32_array(v.to_vec()))
            } else {
                Err(NetCdfError::DataTypeMismatch {
                    expected: "I32".to_string(),
                    found: "attribute data".to_string(),
                })
            }
        }
        Nc3Type::F32 => {
            if let Some(v) = attr.get_f32() {
                Ok(AttributeValue::f32_array(v.to_vec()))
            } else {
                Err(NetCdfError::DataTypeMismatch {
                    expected: "F32".to_string(),
                    found: "attribute data".to_string(),
                })
            }
        }
        Nc3Type::F64 => {
            if let Some(v) = attr.get_f64() {
                Ok(AttributeValue::f64_array(v.to_vec()))
            } else {
                Err(NetCdfError::DataTypeMismatch {
                    expected: "F64".to_string(),
                    found: "attribute data".to_string(),
                })
            }
        }
    }
}

/// Read a variable from a DataSet.
#[cfg(feature = "netcdf3")]
pub fn read_variable(dataset: &DataSet, var_name: &str) -> Result<Variable> {
    // Get variable info
    let var = dataset
        .get_var(var_name)
        .ok_or_else(|| NetCdfError::VariableNotFound {
            name: var_name.to_string(),
        })?;

    // Convert data type
    let data_type = convert_datatype_from_nc3(var.data_type())?;

    // Get dimension names (dim_names() returns Vec<String> in v0.6)
    let dimension_names: Vec<String> = var.dim_names();

    // Create variable
    let mut variable = Variable::new(var_name, data_type, dimension_names.clone())?;

    // Check if coordinate variable
    let is_coordinate =
        dimension_names.len() == 1 && dimension_names.first().is_some_and(|d| d == var_name);
    variable.set_coordinate(is_coordinate);

    // Read attributes (use get_attr_names() in v0.6)
    for attr_name in var.get_attr_names() {
        if let Some(attr) = read_variable_attribute(dataset, var_name, &attr_name)? {
            variable.attributes_mut().add(attr)?;
        }
    }

    Ok(variable)
}

/// Read dimensions from a DataSet.
#[cfg(feature = "netcdf3")]
pub fn read_dimensions(dataset: &DataSet) -> Result<Vec<Dimension>> {
    let mut dimensions = Vec::new();

    for dim_name in dataset.dim_names() {
        let size = dataset
            .dim_size(&dim_name)
            .ok_or_else(|| NetCdfError::DimensionNotFound {
                name: dim_name.to_string(),
            })?;

        // In netcdf3 v0.6, use DimensionType::UnlimitedSize instead of Unlimited
        let is_unlimited =
            dataset.dim_type(&dim_name) == Some(netcdf3::DimensionType::UnlimitedSize);
        let dimension = if is_unlimited {
            Dimension::new_unlimited(&dim_name, size)?
        } else {
            Dimension::new(&dim_name, size)?
        };

        dimensions.push(dimension);
    }

    Ok(dimensions)
}
