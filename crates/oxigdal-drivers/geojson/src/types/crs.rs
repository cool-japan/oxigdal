//! CRS (Coordinate Reference System) support for GeoJSON
//!
//! Note: CRS support is deprecated in RFC 7946 but still commonly used
//! in practice. This module provides support for reading and writing
//! CRS information.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{GeoJsonError, Result};

/// CRS type discriminator
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CrsType {
    /// Named CRS
    Name,
    /// Linked CRS
    Link,
}

/// Named CRS
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedCrs {
    /// The CRS name (e.g., "urn:ogc:def:crs:OGC:1.3:CRS84" or "EPSG:4326")
    pub name: String,
}

impl NamedCrs {
    /// Creates a new named CRS
    pub fn new<S: Into<String>>(name: S) -> Self {
        Self { name: name.into() }
    }

    /// Creates a CRS from an EPSG code
    pub fn from_epsg(code: u32) -> Self {
        Self {
            name: format!("EPSG:{code}"),
        }
    }

    /// Creates the default WGS84 CRS (OGC CRS84)
    pub fn wgs84() -> Self {
        Self {
            name: "urn:ogc:def:crs:OGC:1.3:CRS84".to_string(),
        }
    }

    /// Creates an EPSG:4326 CRS (WGS84 with lat/lon order)
    pub fn epsg4326() -> Self {
        Self::from_epsg(4326)
    }

    /// Creates an EPSG:3857 CRS (Web Mercator)
    pub fn web_mercator() -> Self {
        Self::from_epsg(3857)
    }

    /// Parses the EPSG code if the name is in EPSG format
    pub fn parse_epsg(&self) -> Option<u32> {
        if self.name.starts_with("EPSG:") {
            self.name[5..].parse().ok()
        } else if self.name.starts_with("urn:ogc:def:crs:EPSG::") {
            self.name[22..].parse().ok()
        } else {
            None
        }
    }

    /// Returns true if this is WGS84
    #[must_use]
    pub fn is_wgs84(&self) -> bool {
        self.name == "urn:ogc:def:crs:OGC:1.3:CRS84"
            || self.name == "EPSG:4326"
            || self.name == "urn:ogc:def:crs:EPSG::4326"
    }

    /// Returns true if this is Web Mercator
    #[must_use]
    pub fn is_web_mercator(&self) -> bool {
        self.name == "EPSG:3857" || self.name == "urn:ogc:def:crs:EPSG::3857"
    }
}

/// Linked CRS
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkedCrs {
    /// The link URL or reference
    pub href: String,
    /// The link type (e.g., "proj4", "ogcwkt", "esriwkt")
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub link_type: Option<String>,
}

impl LinkedCrs {
    /// Creates a new linked CRS
    pub fn new<S: Into<String>>(href: S) -> Self {
        Self {
            href: href.into(),
            link_type: None,
        }
    }

    /// Creates a new linked CRS with type
    pub fn with_type<S: Into<String>, T: Into<String>>(href: S, link_type: T) -> Self {
        Self {
            href: href.into(),
            link_type: Some(link_type.into()),
        }
    }
}

/// CRS (Coordinate Reference System)
///
/// Note: CRS is deprecated in RFC 7946. The specification recommends using
/// WGS84 (EPSG:4326) as the default CRS. However, many existing GeoJSON
/// files still use CRS, so we support it for compatibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Crs {
    /// CRS type
    #[serde(rename = "type")]
    pub crs_type: CrsType,

    /// CRS properties
    pub properties: Value,
}

impl Crs {
    /// Creates a new CRS from a named CRS
    pub fn named(name: NamedCrs) -> Self {
        Self {
            crs_type: CrsType::Name,
            properties: serde_json::json!({ "name": name.name }),
        }
    }

    /// Creates a new CRS from a linked CRS
    pub fn linked(link: LinkedCrs) -> Self {
        let props = if let Some(link_type) = link.link_type {
            serde_json::json!({
                "href": link.href,
                "type": link_type,
            })
        } else {
            serde_json::json!({
                "href": link.href,
            })
        };

        Self {
            crs_type: CrsType::Link,
            properties: props,
        }
    }

    /// Creates a CRS from an EPSG code
    pub fn from_epsg(code: u32) -> Self {
        Self::named(NamedCrs::from_epsg(code))
    }

    /// Creates the default WGS84 CRS
    pub fn wgs84() -> Self {
        Self::named(NamedCrs::wgs84())
    }

    /// Creates an EPSG:4326 CRS
    pub fn epsg4326() -> Self {
        Self::named(NamedCrs::epsg4326())
    }

    /// Creates a Web Mercator CRS (EPSG:3857)
    pub fn web_mercator() -> Self {
        Self::named(NamedCrs::web_mercator())
    }

    /// Returns the CRS name if this is a named CRS
    pub fn name(&self) -> Option<String> {
        if self.crs_type == CrsType::Name {
            self.properties
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        } else {
            None
        }
    }

    /// Returns the EPSG code if this is an EPSG CRS
    pub fn epsg_code(&self) -> Option<u32> {
        self.name().and_then(|name| {
            if name.starts_with("EPSG:") {
                name[5..].parse().ok()
            } else if name.starts_with("urn:ogc:def:crs:EPSG::") {
                name[22..].parse().ok()
            } else {
                None
            }
        })
    }

    /// Returns true if this is a named CRS
    #[must_use]
    pub const fn is_named(&self) -> bool {
        matches!(self.crs_type, CrsType::Name)
    }

    /// Returns true if this is a linked CRS
    #[must_use]
    pub const fn is_linked(&self) -> bool {
        matches!(self.crs_type, CrsType::Link)
    }

    /// Returns true if this is WGS84
    #[must_use]
    pub fn is_wgs84(&self) -> bool {
        if let Some(name) = self.name() {
            name == "urn:ogc:def:crs:OGC:1.3:CRS84"
                || name == "EPSG:4326"
                || name == "urn:ogc:def:crs:EPSG::4326"
        } else {
            false
        }
    }

    /// Validates the CRS
    pub fn validate(&self) -> Result<()> {
        match self.crs_type {
            CrsType::Name => {
                if self.properties.get("name").is_none() {
                    return Err(GeoJsonError::InvalidCrs {
                        message: "Named CRS must have 'name' property".to_string(),
                    });
                }
            }
            CrsType::Link => {
                if self.properties.get("href").is_none() {
                    return Err(GeoJsonError::InvalidCrs {
                        message: "Linked CRS must have 'href' property".to_string(),
                    });
                }
            }
        }
        Ok(())
    }
}

impl Default for Crs {
    fn default() -> Self {
        Self::wgs84()
    }
}

/// CRS validation utilities
pub mod validation {
    use super::*;

    /// Validates that coordinates are valid for WGS84
    #[allow(dead_code)] // Reserved for future strict validation mode
    pub fn validate_wgs84_coordinates(lon: f64, lat: f64) -> Result<()> {
        if !lon.is_finite() || !lat.is_finite() {
            return Err(GeoJsonError::invalid_coordinates(
                "Coordinates must be finite numbers",
            ));
        }

        if !(-180.0..=180.0).contains(&lon) {
            return Err(GeoJsonError::invalid_coordinates(format!(
                "Longitude out of range [-180, 180]: {lon}"
            )));
        }

        if !(-90.0..=90.0).contains(&lat) {
            return Err(GeoJsonError::invalid_coordinates(format!(
                "Latitude out of range [-90, 90]: {lat}"
            )));
        }

        Ok(())
    }

    /// Validates that coordinates are valid for Web Mercator
    #[allow(dead_code)] // Reserved for future strict validation mode
    pub fn validate_web_mercator_coordinates(x: f64, y: f64) -> Result<()> {
        const MAX_EXTENT: f64 = 20_037_508.342_789_244;

        if !x.is_finite() || !y.is_finite() {
            return Err(GeoJsonError::invalid_coordinates(
                "Coordinates must be finite numbers",
            ));
        }

        if !(-MAX_EXTENT..=MAX_EXTENT).contains(&x) {
            return Err(GeoJsonError::invalid_coordinates(format!(
                "X coordinate out of range [{}, {}]: {x}",
                -MAX_EXTENT, MAX_EXTENT
            )));
        }

        if !(-MAX_EXTENT..=MAX_EXTENT).contains(&y) {
            return Err(GeoJsonError::invalid_coordinates(format!(
                "Y coordinate out of range [{}, {}]: {y}",
                -MAX_EXTENT, MAX_EXTENT
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_named_crs() {
        let crs = NamedCrs::wgs84();
        assert!(crs.is_wgs84());
        assert_eq!(crs.name, "urn:ogc:def:crs:OGC:1.3:CRS84");

        let epsg = NamedCrs::from_epsg(4326);
        assert!(epsg.is_wgs84());
        assert_eq!(epsg.parse_epsg(), Some(4326));

        let mercator = NamedCrs::web_mercator();
        assert!(mercator.is_web_mercator());
        assert_eq!(mercator.parse_epsg(), Some(3857));
    }

    #[test]
    fn test_linked_crs() {
        let link = LinkedCrs::new("http://example.com/crs.json");
        assert_eq!(link.href, "http://example.com/crs.json");
        assert!(link.link_type.is_none());

        let link_typed = LinkedCrs::with_type("http://example.com/crs.json", "proj4");
        assert_eq!(link_typed.link_type, Some("proj4".to_string()));
    }

    #[test]
    fn test_crs_creation() {
        let crs = Crs::wgs84();
        assert!(crs.is_named());
        assert!(crs.is_wgs84());

        let epsg_crs = Crs::from_epsg(3857);
        assert_eq!(epsg_crs.epsg_code(), Some(3857));
    }

    #[test]
    fn test_crs_validation() {
        let crs = Crs::wgs84();
        assert!(crs.validate().is_ok());

        let named_crs = Crs::named(NamedCrs::new("EPSG:4326"));
        assert!(named_crs.validate().is_ok());
    }

    #[test]
    fn test_wgs84_validation() {
        use validation::validate_wgs84_coordinates;

        assert!(validate_wgs84_coordinates(0.0, 0.0).is_ok());
        assert!(validate_wgs84_coordinates(-180.0, -90.0).is_ok());
        assert!(validate_wgs84_coordinates(180.0, 90.0).is_ok());

        assert!(validate_wgs84_coordinates(181.0, 0.0).is_err());
        assert!(validate_wgs84_coordinates(0.0, 91.0).is_err());
        assert!(validate_wgs84_coordinates(f64::NAN, 0.0).is_err());
    }

    #[test]
    fn test_web_mercator_validation() {
        use validation::validate_web_mercator_coordinates;

        assert!(validate_web_mercator_coordinates(0.0, 0.0).is_ok());
        assert!(validate_web_mercator_coordinates(20_037_508.0, 20_037_508.0).is_ok());

        assert!(validate_web_mercator_coordinates(20_037_509.0, 0.0).is_err());
    }

    #[test]
    fn test_crs_serialization() {
        let crs = Crs::from_epsg(4326);
        let json = serde_json::to_string(&crs).ok();
        assert!(json.is_some());

        let deserialized: std::result::Result<Crs, _> =
            serde_json::from_str(&json.expect("valid json"));
        assert!(deserialized.is_ok());
    }
}
