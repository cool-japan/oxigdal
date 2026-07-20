//! STAC Versioning Extension v1.2.0
//!
//! Enables version tracking for STAC Items and Collections using semantic or
//! arbitrary version strings, successor/predecessor links, and experimental flags.
//! <https://github.com/stac-extensions/version>

use serde::{Deserialize, Serialize};

use super::Extension;
use crate::error::{Result, StacError};

/// Schema URI for the Version extension.
pub const SCHEMA_URI: &str = "https://stac-extensions.github.io/version/v1.2.0/schema.json";

/// Version extension data for a STAC item or collection.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct VersionExtension {
    /// Version string (e.g., `"1.0"`, `"2023-06"`, `"v2.1.3"`).
    #[serde(rename = "version", skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Marks the item as experimental / pre-release.
    ///
    /// When `true` the item should be treated with caution and may change
    /// without notice.
    #[serde(rename = "experimental", skip_serializing_if = "Option::is_none")]
    pub experimental: Option<bool>,

    /// Marks the item as deprecated.
    ///
    /// Deprecated items still exist but consumers should prefer the successor.
    #[serde(rename = "deprecated", skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<bool>,
}

impl VersionExtension {
    /// Creates a new, empty [`VersionExtension`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the version string.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Marks the item as experimental.
    pub fn mark_experimental(mut self) -> Self {
        self.experimental = Some(true);
        self
    }

    /// Marks the item as deprecated.
    pub fn mark_deprecated(mut self) -> Self {
        self.deprecated = Some(true);
        self
    }

    /// Returns `true` when `experimental` is explicitly set to `true`.
    pub fn is_experimental(&self) -> bool {
        self.experimental == Some(true)
    }

    /// Returns `true` when `deprecated` is explicitly set to `true`.
    pub fn is_deprecated(&self) -> bool {
        self.deprecated == Some(true)
    }

    /// Validates the extension data.
    ///
    /// Currently validates that the version string is non-empty if present.
    pub fn validate(&self) -> Result<()> {
        if let Some(ref v) = self.version {
            if v.trim().is_empty() {
                return Err(StacError::InvalidExtension {
                    extension: "version".to_string(),
                    reason: "version string must not be empty".to_string(),
                });
            }
        }
        Ok(())
    }
}

impl Extension for VersionExtension {
    fn schema_uri() -> &'static str {
        SCHEMA_URI
    }

    fn validate(&self) -> Result<()> {
        self.validate()
    }

    fn to_value(&self) -> Result<serde_json::Value> {
        serde_json::to_value(self).map_err(|e| StacError::Serialization(e.to_string()))
    }

    fn from_value(value: &serde_json::Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|e| StacError::Deserialization(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_extension_new() {
        let v = VersionExtension::new();
        assert!(v.version.is_none());
        assert!(!v.is_experimental());
        assert!(!v.is_deprecated());
    }

    #[test]
    fn test_with_version() {
        let v = VersionExtension::new().with_version("1.2.3");
        assert_eq!(v.version, Some("1.2.3".to_string()));
    }

    #[test]
    fn test_mark_experimental() {
        let v = VersionExtension::new().mark_experimental();
        assert!(v.is_experimental());
    }

    #[test]
    fn test_mark_deprecated() {
        let v = VersionExtension::new().mark_deprecated();
        assert!(v.is_deprecated());
    }

    #[test]
    fn test_validate_empty_version_fails() {
        let v = VersionExtension::new().with_version("   ");
        assert!(v.validate().is_err());
    }

    #[test]
    fn test_validate_ok() {
        let v = VersionExtension::new().with_version("v2.0.0-beta");
        assert!(v.validate().is_ok());
    }

    #[test]
    fn test_serialization_roundtrip() {
        let v = VersionExtension::new()
            .with_version("2024-03")
            .mark_experimental();
        let json = serde_json::to_string(&v).expect("serialize");
        let back: VersionExtension = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(v, back);
    }

    #[test]
    fn test_schema_uri() {
        assert!(VersionExtension::schema_uri().contains("version"));
    }
}
