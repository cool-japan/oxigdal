//! STAC Extensions.
//!
//! This module provides support for common STAC extensions:
//! - EO (Electro-Optical): For satellite imagery metadata
//! - Projection: For coordinate reference system information
//! - SAR (Synthetic Aperture Radar): For radar data metadata
//! - View: For viewing geometry metadata
//! - Scientific: For scientific citation and publication metadata

pub mod eo;
pub mod projection;
pub mod sar;
pub mod scientific;
pub mod timestamps;
pub mod version;
pub mod view;

use crate::error::{Result, StacError};
use serde_json::Value;

/// Trait for STAC extensions.
pub trait Extension {
    /// Returns the schema URI for this extension.
    fn schema_uri() -> &'static str;

    /// Validates the extension data.
    fn validate(&self) -> Result<()>;

    /// Converts the extension to a JSON value.
    fn to_value(&self) -> Result<Value>;

    /// Creates the extension from a JSON value.
    fn from_value(value: &Value) -> Result<Self>
    where
        Self: Sized;
}

/// Helper function to get an extension field from additional fields.
///
/// # Arguments
///
/// * `additional_fields` - Map of additional fields
/// * `key` - Field name to retrieve
///
/// # Returns
///
/// The field value if it exists
pub fn get_extension_field<T>(
    additional_fields: &std::collections::HashMap<String, Value>,
    key: &str,
) -> Result<Option<T>>
where
    T: serde::de::DeserializeOwned,
{
    match additional_fields.get(key) {
        Some(value) => {
            let result =
                serde_json::from_value(value.clone()).map_err(|e| StacError::InvalidExtension {
                    extension: key.to_string(),
                    reason: e.to_string(),
                })?;
            Ok(Some(result))
        }
        None => Ok(None),
    }
}

/// Helper function to set an extension field in additional fields.
///
/// # Arguments
///
/// * `additional_fields` - Map of additional fields
/// * `key` - Field name to set
/// * `value` - Value to set
///
/// # Returns
///
/// Result indicating success or failure
pub fn set_extension_field<T>(
    additional_fields: &mut std::collections::HashMap<String, Value>,
    key: &str,
    value: Option<T>,
) -> Result<()>
where
    T: serde::Serialize,
{
    match value {
        Some(v) => {
            let json_value =
                serde_json::to_value(v).map_err(|e| StacError::Serialization(e.to_string()))?;
            additional_fields.insert(key.to_string(), json_value);
            Ok(())
        }
        None => {
            additional_fields.remove(key);
            Ok(())
        }
    }
}
