//! STAC Asset representation.

use crate::error::{Result, StacError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A STAC Asset object represents a file associated with an Item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Asset {
    /// URI to the asset.
    pub href: String,

    /// Displayed title for clients and users.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Description of the asset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Media type of the asset.
    #[serde(skip_serializing_if = "Option::is_none", rename = "type")]
    pub media_type: Option<String>,

    /// Roles of the asset (e.g., "thumbnail", "data", "metadata").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roles: Option<Vec<String>>,

    /// Additional fields for extensions.
    #[serde(flatten)]
    pub additional_fields: HashMap<String, serde_json::Value>,
}

impl Asset {
    /// Creates a new Asset with the given href.
    ///
    /// # Arguments
    ///
    /// * `href` - URI to the asset
    ///
    /// # Returns
    ///
    /// A new Asset instance
    pub fn new(href: impl Into<String>) -> Self {
        Self {
            href: href.into(),
            title: None,
            description: None,
            media_type: None,
            roles: None,
            additional_fields: HashMap::new(),
        }
    }

    /// Sets the title of the asset.
    ///
    /// # Arguments
    ///
    /// * `title` - Title for the asset
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets the description of the asset.
    ///
    /// # Arguments
    ///
    /// * `description` - Description of the asset
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets the media type of the asset.
    ///
    /// # Arguments
    ///
    /// * `media_type` - Media type (MIME type)
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_type(mut self, media_type: impl Into<String>) -> Self {
        self.media_type = Some(media_type.into());
        self
    }

    /// Sets the role of the asset.
    ///
    /// # Arguments
    ///
    /// * `role` - Role identifier
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.roles = Some(vec![role.into()]);
        self
    }

    /// Adds a role to the asset.
    ///
    /// # Arguments
    ///
    /// * `role` - Role identifier to add
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn add_role(mut self, role: impl Into<String>) -> Self {
        match &mut self.roles {
            Some(roles) => roles.push(role.into()),
            None => self.roles = Some(vec![role.into()]),
        }
        self
    }

    /// Sets multiple roles for the asset.
    ///
    /// # Arguments
    ///
    /// * `roles` - Vector of role identifiers
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_roles(mut self, roles: Vec<String>) -> Self {
        self.roles = Some(roles);
        self
    }

    /// Sets an additional field on the asset.
    ///
    /// # Arguments
    ///
    /// * `key` - Field name
    /// * `value` - Field value
    ///
    /// # Returns
    ///
    /// Self for method chaining
    pub fn with_additional_field(
        mut self,
        key: impl Into<String>,
        value: serde_json::Value,
    ) -> Self {
        self.additional_fields.insert(key.into(), value);
        self
    }

    /// Gets an additional field value.
    ///
    /// # Arguments
    ///
    /// * `key` - Field name
    ///
    /// # Returns
    ///
    /// Reference to the field value if it exists
    pub fn get_additional_field(&self, key: &str) -> Option<&serde_json::Value> {
        self.additional_fields.get(key)
    }

    /// Checks if the asset has a specific role.
    ///
    /// # Arguments
    ///
    /// * `role` - Role to check for
    ///
    /// # Returns
    ///
    /// `true` if the asset has the role, `false` otherwise
    pub fn has_role(&self, role: &str) -> bool {
        self.roles
            .as_ref()
            .is_some_and(|roles| roles.iter().any(|r| r == role))
    }

    /// Validates the asset.
    ///
    /// # Returns
    ///
    /// `Ok(())` if valid, otherwise an error
    pub fn validate(&self) -> Result<()> {
        if self.href.is_empty() {
            return Err(StacError::MissingField("href".to_string()));
        }

        // Validate URL format
        url::Url::parse(&self.href).or_else(|_| {
            // If it's not a valid URL, it might be a relative path
            if self.href.starts_with('/') || self.href.starts_with('.') {
                Ok(url::Url::parse("file:///dummy")
                    .map_err(|e| StacError::InvalidUrl(e.to_string()))?)
            } else {
                Err(StacError::InvalidUrl(format!(
                    "Invalid href: {}",
                    self.href
                )))
            }
        })?;

        Ok(())
    }
}

/// Common media types for STAC assets.
pub mod media_types {
    /// GeoTIFF media type.
    pub const GEOTIFF: &str = "image/tiff; application=geotiff";

    /// Cloud Optimized GeoTIFF media type.
    pub const COG: &str = "image/tiff; application=geotiff; profile=cloud-optimized";

    /// JPEG media type.
    pub const JPEG: &str = "image/jpeg";

    /// PNG media type.
    pub const PNG: &str = "image/png";

    /// GeoJSON media type.
    pub const GEOJSON: &str = "application/geo+json";

    /// JSON media type.
    pub const JSON: &str = "application/json";

    /// HDF5 media type.
    pub const HDF5: &str = "application/x-hdf5";

    /// NetCDF media type.
    pub const NETCDF: &str = "application/netcdf";

    /// Zarr media type.
    pub const ZARR: &str = "application/vnd+zarr";

    /// XML media type.
    pub const XML: &str = "application/xml";

    /// Text media type.
    pub const TEXT: &str = "text/plain";

    /// HTML media type.
    pub const HTML: &str = "text/html";
}

/// Common asset roles.
pub mod roles {
    /// Thumbnail role.
    pub const THUMBNAIL: &str = "thumbnail";

    /// Overview role.
    pub const OVERVIEW: &str = "overview";

    /// Data role.
    pub const DATA: &str = "data";

    /// Metadata role.
    pub const METADATA: &str = "metadata";

    /// Visual role.
    pub const VISUAL: &str = "visual";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asset_new() {
        let asset = Asset::new("https://example.com/image.tif");
        assert_eq!(asset.href, "https://example.com/image.tif");
        assert!(asset.title.is_none());
        assert!(asset.description.is_none());
        assert!(asset.media_type.is_none());
        assert!(asset.roles.is_none());
    }

    #[test]
    fn test_asset_builder() {
        let asset = Asset::new("https://example.com/image.tif")
            .with_title("Test Image")
            .with_description("A test image")
            .with_type(media_types::COG)
            .with_role(roles::DATA);

        assert_eq!(asset.title, Some("Test Image".to_string()));
        assert_eq!(asset.description, Some("A test image".to_string()));
        assert_eq!(asset.media_type, Some(media_types::COG.to_string()));
        assert_eq!(asset.roles, Some(vec![roles::DATA.to_string()]));
    }

    #[test]
    fn test_asset_multiple_roles() {
        let asset = Asset::new("https://example.com/image.tif")
            .with_role(roles::DATA)
            .add_role(roles::VISUAL);

        assert!(asset.has_role(roles::DATA));
        assert!(asset.has_role(roles::VISUAL));
        assert!(!asset.has_role(roles::THUMBNAIL));
    }

    #[test]
    fn test_asset_additional_fields() {
        let asset = Asset::new("https://example.com/image.tif")
            .with_additional_field("custom_field", serde_json::json!("custom_value"));

        assert_eq!(
            asset.get_additional_field("custom_field"),
            Some(&serde_json::json!("custom_value"))
        );
    }

    #[test]
    fn test_asset_validate() {
        let asset = Asset::new("https://example.com/image.tif");
        assert!(asset.validate().is_ok());

        let invalid_asset = Asset::new("");
        assert!(invalid_asset.validate().is_err());
    }

    #[test]
    fn test_asset_serialization() {
        let asset = Asset::new("https://example.com/image.tif")
            .with_title("Test")
            .with_type(media_types::COG);

        let json = serde_json::to_string(&asset);
        assert!(json.is_ok());

        let deserialized: Asset = serde_json::from_str(&json.expect("JSON serialization failed"))
            .expect("Deserialization failed");
        assert_eq!(asset, deserialized);
    }
}
