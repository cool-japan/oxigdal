//! Coordinate Reference System (CRS) definitions and operations.
//!
//! This module provides structures and methods for working with coordinate reference systems,
//! including EPSG codes, PROJ strings, and WKT representations.

use crate::epsg::{CrsType, EpsgDefinition, lookup_epsg};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Coordinate Reference System.
///
/// A CRS defines how coordinates relate to positions on the Earth's surface.
/// This structure supports multiple ways of defining a CRS: EPSG codes, PROJ strings, and WKT.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Crs {
    /// The source of the CRS definition
    source: CrsSource,
    /// Human-readable name (if available)
    name: Option<String>,
    /// CRS type (geographic, projected, etc.)
    crs_type: Option<CrsType>,
    /// Unit of measurement
    unit: Option<String>,
    /// Datum name
    datum: Option<String>,
    /// Authority (e.g., "EPSG")
    authority: Option<String>,
}

/// Source of CRS definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CrsSource {
    /// EPSG code
    Epsg(u32),
    /// PROJ string
    Proj(String),
    /// Well-Known Text (WKT)
    Wkt(String),
    /// Custom definition
    Custom {
        /// Name of the custom CRS
        name: String,
        /// Definition string
        definition: String,
    },
}

impl Crs {
    /// Creates a CRS from an EPSG code.
    ///
    /// # Arguments
    ///
    /// * `code` - EPSG code
    ///
    /// # Errors
    ///
    /// Returns an error if the EPSG code is not found in the database.
    pub fn from_epsg(code: u32) -> Result<Self> {
        let def = lookup_epsg(code)?;
        Ok(Self::from_epsg_definition(def))
    }

    /// Creates a CRS from an EPSG definition.
    fn from_epsg_definition(def: &EpsgDefinition) -> Self {
        Self {
            source: CrsSource::Epsg(def.code),
            name: Some(def.name.clone()),
            crs_type: Some(def.crs_type),
            unit: Some(def.unit.clone()),
            datum: Some(def.datum.clone()),
            authority: Some("EPSG".to_string()),
        }
    }

    /// Creates a CRS from a PROJ string.
    ///
    /// # Arguments
    ///
    /// * `proj_string` - PROJ string definition
    ///
    /// # Errors
    ///
    /// Returns an error if the PROJ string is invalid.
    pub fn from_proj<S: Into<String>>(proj_string: S) -> Result<Self> {
        let proj_string = proj_string.into();

        // Basic validation
        if proj_string.trim().is_empty() {
            return Err(Error::invalid_proj_string("PROJ string is empty"));
        }

        if !proj_string.contains("+proj=") && !proj_string.starts_with("proj=") {
            return Err(Error::invalid_proj_string(
                "PROJ string must contain +proj= parameter",
            ));
        }

        // Try to extract projection type and other parameters
        let (crs_type, unit) = Self::parse_proj_string(&proj_string);

        Ok(Self {
            source: CrsSource::Proj(proj_string),
            name: None,
            crs_type,
            unit,
            datum: None,
            authority: None,
        })
    }

    /// Creates a CRS from a WKT (Well-Known Text) string.
    ///
    /// # Arguments
    ///
    /// * `wkt` - WKT string definition
    ///
    /// # Errors
    ///
    /// Returns an error if the WKT string is invalid.
    pub fn from_wkt<S: Into<String>>(wkt: S) -> Result<Self> {
        let wkt = wkt.into();

        // Basic validation
        if wkt.trim().is_empty() {
            return Err(Error::invalid_wkt("WKT string is empty"));
        }

        // WKT should start with a CRS type keyword
        let trimmed = wkt.trim();
        let valid_start = trimmed.starts_with("GEOGCS[")
            || trimmed.starts_with("PROJCS[")
            || trimmed.starts_with("GEOCCS[")
            || trimmed.starts_with("VERT_CS[")
            || trimmed.starts_with("COMPD_CS[");

        if !valid_start {
            return Err(Error::invalid_wkt(
                "WKT must start with GEOGCS, PROJCS, GEOCCS, VERT_CS, or COMPD_CS",
            ));
        }

        Ok(Self {
            source: CrsSource::Wkt(wkt),
            name: None,
            crs_type: None,
            unit: None,
            datum: None,
            authority: None,
        })
    }

    /// Creates a custom CRS.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the custom CRS
    /// * `definition` - Definition string (PROJ format)
    pub fn custom<S: Into<String>>(name: S, definition: S) -> Self {
        Self {
            source: CrsSource::Custom {
                name: name.into(),
                definition: definition.into(),
            },
            name: None,
            crs_type: None,
            unit: None,
            datum: None,
            authority: None,
        }
    }

    /// Returns the CRS source.
    pub fn source(&self) -> &CrsSource {
        &self.source
    }

    /// Returns the name of the CRS, if available.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Returns the CRS type, if known.
    pub fn crs_type(&self) -> Option<CrsType> {
        self.crs_type
    }

    /// Returns the unit of measurement, if known.
    pub fn unit(&self) -> Option<&str> {
        self.unit.as_deref()
    }

    /// Returns the datum, if known.
    pub fn datum(&self) -> Option<&str> {
        self.datum.as_deref()
    }

    /// Returns the authority (e.g., "EPSG"), if available.
    pub fn authority(&self) -> Option<&str> {
        self.authority.as_deref()
    }

    /// Returns the EPSG code, if this CRS was created from an EPSG code.
    pub fn epsg_code(&self) -> Option<u32> {
        match &self.source {
            CrsSource::Epsg(code) => Some(*code),
            _ => None,
        }
    }

    /// Converts the CRS to a PROJ string.
    ///
    /// # Errors
    ///
    /// Returns an error if the conversion fails.
    pub fn to_proj_string(&self) -> Result<String> {
        match &self.source {
            CrsSource::Epsg(code) => {
                let def = lookup_epsg(*code)?;
                Ok(def.proj_string.clone())
            }
            CrsSource::Proj(proj_string) => Ok(proj_string.clone()),
            CrsSource::Wkt(_) => Err(Error::unsupported_crs(
                "WKT to PROJ conversion not yet implemented",
            )),
            CrsSource::Custom { definition, .. } => Ok(definition.clone()),
        }
    }

    /// Converts the CRS to a WKT string.
    ///
    /// # Errors
    ///
    /// Returns an error if the conversion fails.
    pub fn to_wkt(&self) -> Result<String> {
        match &self.source {
            CrsSource::Epsg(code) => {
                let def = lookup_epsg(*code)?;
                def.wkt.clone().ok_or_else(|| {
                    Error::unsupported_crs(format!("No WKT available for EPSG:{}", code))
                })
            }
            CrsSource::Wkt(wkt) => Ok(wkt.clone()),
            CrsSource::Proj(_) => Err(Error::unsupported_crs(
                "PROJ to WKT conversion not yet implemented",
            )),
            CrsSource::Custom { .. } => Err(Error::unsupported_crs(
                "Custom CRS to WKT conversion not yet implemented",
            )),
        }
    }

    /// Checks if this is a geographic CRS (latitude/longitude).
    pub fn is_geographic(&self) -> bool {
        matches!(self.crs_type, Some(CrsType::Geographic))
    }

    /// Checks if this is a projected CRS (planar coordinates).
    pub fn is_projected(&self) -> bool {
        matches!(self.crs_type, Some(CrsType::Projected))
    }

    /// Checks if two CRS are equivalent.
    ///
    /// This performs a basic comparison. For more sophisticated comparison,
    /// use proper projection transformation libraries.
    pub fn is_equivalent(&self, other: &Crs) -> bool {
        // Simple case: same EPSG code
        if let (Some(code1), Some(code2)) = (self.epsg_code(), other.epsg_code()) {
            return code1 == code2;
        }

        // Otherwise, compare PROJ strings
        if let (Ok(proj1), Ok(proj2)) = (self.to_proj_string(), other.to_proj_string()) {
            return proj1 == proj2;
        }

        false
    }

    /// Parses a PROJ string to extract CRS type and unit.
    fn parse_proj_string(proj_string: &str) -> (Option<CrsType>, Option<String>) {
        let mut crs_type = None;
        let mut unit = None;

        // Check for geographic projection (longlat)
        if proj_string.contains("+proj=longlat") || proj_string.contains("+proj=latlong") {
            crs_type = Some(CrsType::Geographic);
            unit = Some("degree".to_string());
        } else if proj_string.contains("+proj=") {
            // Projected CRS
            crs_type = Some(CrsType::Projected);

            // Try to extract unit
            if proj_string.contains("+units=m") {
                unit = Some("metre".to_string());
            } else if proj_string.contains("+units=km") {
                unit = Some("kilometre".to_string());
            } else if proj_string.contains("+units=ft") {
                unit = Some("foot".to_string());
            } else if proj_string.contains("+units=us-ft") {
                unit = Some("US survey foot".to_string());
            }
        }

        (crs_type, unit)
    }
}

impl fmt::Display for Crs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.source {
            CrsSource::Epsg(code) => {
                if let Some(name) = &self.name {
                    write!(f, "EPSG:{} ({})", code, name)
                } else {
                    write!(f, "EPSG:{}", code)
                }
            }
            CrsSource::Proj(proj_string) => {
                if let Some(name) = &self.name {
                    write!(f, "{}: {}", name, proj_string)
                } else {
                    write!(f, "{}", proj_string)
                }
            }
            CrsSource::Wkt(_) => {
                if let Some(name) = &self.name {
                    write!(f, "WKT: {}", name)
                } else {
                    write!(f, "WKT")
                }
            }
            CrsSource::Custom { name, .. } => write!(f, "Custom: {}", name),
        }
    }
}

// Common CRS constants for convenience
impl Crs {
    /// WGS84 geographic CRS (EPSG:4326).
    ///
    /// This method is guaranteed to succeed as WGS84 is always in the database.
    /// If it fails (which should never happen), it falls back to a custom CRS.
    pub fn wgs84() -> Self {
        Self::from_epsg(4326)
            .unwrap_or_else(|_| Self::custom("WGS84", "+proj=longlat +datum=WGS84 +no_defs"))
    }

    /// Web Mercator projected CRS (EPSG:3857).
    ///
    /// This method is guaranteed to succeed as Web Mercator is always in the database.
    /// If it fails (which should never happen), it falls back to a custom CRS.
    pub fn web_mercator() -> Self {
        Self::from_epsg(3857).unwrap_or_else(|_| {
            Self::custom("Web Mercator", "+proj=merc +a=6378137 +b=6378137 +lat_ts=0 +lon_0=0 +x_0=0 +y_0=0 +k=1 +units=m +nadgrids=@null +wktext +no_defs")
        })
    }

    /// NAD83 geographic CRS (EPSG:4269).
    ///
    /// This method is guaranteed to succeed as NAD83 is always in the database.
    /// If it fails (which should never happen), it falls back to a custom CRS.
    pub fn nad83() -> Self {
        Self::from_epsg(4269)
            .unwrap_or_else(|_| Self::custom("NAD83", "+proj=longlat +datum=NAD83 +no_defs"))
    }

    /// ETRS89 geographic CRS (EPSG:4258).
    ///
    /// This method is guaranteed to succeed as ETRS89 is always in the database.
    /// If it fails (which should never happen), it falls back to a custom CRS.
    pub fn etrs89() -> Self {
        Self::from_epsg(4258).unwrap_or_else(|_| {
            Self::custom(
                "ETRS89",
                "+proj=longlat +ellps=GRS80 +towgs84=0,0,0,0,0,0,0 +no_defs",
            )
        })
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_from_epsg() {
        let wgs84 = Crs::from_epsg(4326);
        assert!(wgs84.is_ok());
        let wgs84 = wgs84.expect("WGS84 should exist");
        assert_eq!(wgs84.epsg_code(), Some(4326));
        assert_eq!(wgs84.name(), Some("WGS 84"));
        assert!(wgs84.is_geographic());
        assert!(!wgs84.is_projected());
    }

    #[test]
    fn test_from_epsg_invalid() {
        let result = Crs::from_epsg(99999);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_proj() {
        let proj_string = "+proj=longlat +datum=WGS84 +no_defs";
        let crs = Crs::from_proj(proj_string);
        assert!(crs.is_ok());
        let crs = crs.expect("PROJ string should be valid");
        assert!(crs.is_geographic());
    }

    #[test]
    fn test_from_proj_invalid() {
        let result = Crs::from_proj("");
        assert!(result.is_err());

        let result = Crs::from_proj("invalid proj string");
        assert!(result.is_err());
    }

    #[test]
    fn test_from_wkt() {
        let wkt = r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]]]"#;
        let crs = Crs::from_wkt(wkt);
        assert!(crs.is_ok());
    }

    #[test]
    fn test_from_wkt_invalid() {
        let result = Crs::from_wkt("");
        assert!(result.is_err());

        let result = Crs::from_wkt("invalid wkt");
        assert!(result.is_err());
    }

    #[test]
    fn test_custom_crs() {
        let crs = Crs::custom("My CRS", "+proj=longlat +datum=WGS84 +no_defs");
        assert!(matches!(crs.source(), CrsSource::Custom { .. }));
    }

    #[test]
    fn test_to_proj_string() {
        let wgs84 = Crs::from_epsg(4326).expect("WGS84 should exist");
        let proj_string = wgs84.to_proj_string();
        assert!(proj_string.is_ok());
        assert!(
            proj_string
                .expect("should have proj string")
                .contains("+proj=longlat")
        );
    }

    #[test]
    fn test_is_equivalent() {
        let wgs84_1 = Crs::from_epsg(4326).expect("WGS84 should exist");
        let wgs84_2 = Crs::from_epsg(4326).expect("WGS84 should exist");
        let web_merc = Crs::from_epsg(3857).expect("Web Mercator should exist");

        assert!(wgs84_1.is_equivalent(&wgs84_2));
        assert!(!wgs84_1.is_equivalent(&web_merc));
    }

    #[test]
    fn test_common_crs_constants() {
        let wgs84 = Crs::wgs84();
        assert_eq!(wgs84.epsg_code(), Some(4326));

        let web_merc = Crs::web_mercator();
        assert_eq!(web_merc.epsg_code(), Some(3857));

        let nad83 = Crs::nad83();
        assert_eq!(nad83.epsg_code(), Some(4269));

        let etrs89 = Crs::etrs89();
        assert_eq!(etrs89.epsg_code(), Some(4258));
    }

    #[test]
    fn test_display() {
        let wgs84 = Crs::wgs84();
        let display = format!("{}", wgs84);
        assert!(display.contains("EPSG:4326"));
        assert!(display.contains("WGS 84"));
    }

    #[test]
    fn test_parse_proj_string() {
        let (crs_type, unit) = Crs::parse_proj_string("+proj=longlat +datum=WGS84 +no_defs");
        assert_eq!(crs_type, Some(CrsType::Geographic));
        assert_eq!(unit, Some("degree".to_string()));

        let (crs_type, unit) = Crs::parse_proj_string("+proj=merc +units=m +no_defs");
        assert_eq!(crs_type, Some(CrsType::Projected));
        assert_eq!(unit, Some("metre".to_string()));
    }
}
