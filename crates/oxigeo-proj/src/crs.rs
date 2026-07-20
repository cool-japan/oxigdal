//! Coordinate Reference System (CRS) definitions and operations.
//!
//! This module provides structures and methods for working with coordinate reference systems,
//! including EPSG codes, PROJ strings, and WKT representations.

use crate::area_of_use::{AreaOfUse, area_of_use_for_epsg};
#[cfg(not(feature = "std"))]
use crate::epsg::CrsType;
#[cfg(feature = "std")]
use crate::epsg::{CrsType, lookup_epsg};
use crate::error::{Error, Result};
use crate::wkt::WktParser;
#[cfg(not(feature = "std"))]
use alloc::string::{String, ToString};
use core::fmt;
use serde::{Deserialize, Serialize};

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
    /// Compound CRS combining a horizontal (geographic or projected) and a vertical CRS.
    Compound {
        /// Horizontal component (geographic or projected)
        horizontal: Box<Crs>,
        /// Vertical component (heights/depths)
        vertical: Box<Crs>,
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
    #[cfg(feature = "std")]
    pub fn from_epsg(code: u32) -> Result<Self> {
        let def = lookup_epsg(code)?;
        Ok(Self::from_epsg_definition(def))
    }

    /// Creates a CRS from an EPSG code (no_std version — always returns error).
    #[cfg(not(feature = "std"))]
    pub fn from_epsg(code: u32) -> Result<Self> {
        Err(Error::unsupported_crs(alloc::format!(
            "EPSG:{} lookup requires std feature",
            code
        )))
    }

    /// Creates a CRS from an EPSG definition.
    #[cfg(feature = "std")]
    fn from_epsg_definition(def: &crate::epsg::EpsgDefinition) -> Self {
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

        // WKT should start with a CRS type keyword (WKT1 or WKT2)
        let trimmed = wkt.trim();
        let upper = trimmed.to_uppercase();
        let valid_start =
            // WKT1 keywords
            upper.starts_with("GEOGCS[") ||
            upper.starts_with("PROJCS[") ||
            upper.starts_with("GEOCCS[") ||
            upper.starts_with("VERT_CS[") ||
            upper.starts_with("COMPD_CS[") ||
            // WKT2:2019 keywords
            upper.starts_with("GEOGCRS[") ||
            upper.starts_with("PROJCRS[") ||
            upper.starts_with("GEODCRS[") ||
            upper.starts_with("VERTCRS[") ||
            upper.starts_with("ENGCRS[") ||
            upper.starts_with("COMPOUNDCRS[") ||
            upper.starts_with("BOUNDCRS[");

        if !valid_start {
            return Err(Error::invalid_wkt(
                "WKT must start with a CRS keyword (GEOGCS, PROJCS, GEOGCRS, PROJCRS, etc.)",
            ));
        }

        // For compound CRS, parse the horizontal and vertical sub-CRS recursively.
        #[cfg(feature = "std")]
        if upper.starts_with("COMPOUNDCRS[") || upper.starts_with("COMPD_CS[") {
            return Self::from_compound_wkt(&wkt);
        }

        // Extract metadata from the WKT string
        let name = WktParser::extract_name(&wkt);
        let crs_type = Self::crs_type_from_wkt_keyword(&upper);
        let unit = WktParser::extract_unit(&wkt).map(|(u, _)| u);
        let datum = Self::extract_datum_from_wkt(&wkt);
        let epsg = WktParser::extract_epsg(&wkt);
        let authority = epsg.map(|_| "EPSG".to_string());

        Ok(Self {
            source: CrsSource::Wkt(wkt),
            name,
            crs_type,
            unit,
            datum,
            authority,
        })
    }

    /// Parses a compound WKT string and returns a `Crs` with a `CrsSource::Compound` source.
    ///
    /// Searches the immediate children of the parsed WKT node for a horizontal CRS
    /// (GEOGCRS / GEOGCS / PROJCRS / PROJCS / GEODCRS) and a vertical CRS
    /// (VERTCRS / VERT_CS), then recurses via `Crs::from_wkt` for each.
    #[cfg(feature = "std")]
    fn from_compound_wkt(wkt: &str) -> Result<Self> {
        use crate::wkt::parse_wkt;

        let node = parse_wkt(wkt)
            .map_err(|e| Error::invalid_wkt(format!("Failed to parse compound WKT: {}", e)))?;

        // Horizontal CRS keywords (WKT1 and WKT2).
        const HORIZ_KEYWORDS: &[&str] = &[
            "GEOGCRS", "PROJCRS", "GEODCRS", "GEOGCS", "PROJCS", "GEOCCS",
        ];
        // Vertical CRS keywords.
        const VERT_KEYWORDS: &[&str] = &["VERTCRS", "VERT_CS"];

        let horiz_child = node.find_child_any(HORIZ_KEYWORDS).ok_or_else(|| {
            Error::invalid_wkt(
                "COMPOUNDCRS does not contain a horizontal (GEOGCRS/PROJCRS) component",
            )
        })?;

        let vert_child = node.find_child_any(VERT_KEYWORDS).ok_or_else(|| {
            Error::invalid_wkt("COMPOUNDCRS does not contain a vertical (VERTCRS) component")
        })?;

        // Re-serialise each child back to a WKT string and recurse.
        let h_wkt = horiz_child.to_string_repr();
        let v_wkt = vert_child.to_string_repr();

        let horizontal = Self::from_wkt_simple(&h_wkt)?;
        let vertical = Self::from_wkt_simple(&v_wkt)?;

        Self::compound(horizontal, vertical)
    }

    /// Like `from_wkt` but never triggers compound recursion (used internally).
    #[cfg(feature = "std")]
    fn from_wkt_simple(wkt: &str) -> Result<Self> {
        if wkt.trim().is_empty() {
            return Err(Error::invalid_wkt("WKT string is empty"));
        }
        let upper = wkt.trim().to_uppercase();
        let name = WktParser::extract_name(wkt);
        let crs_type = Self::crs_type_from_wkt_keyword(&upper);
        let unit = WktParser::extract_unit(wkt).map(|(u, _)| u);
        let datum = Self::extract_datum_from_wkt(wkt);
        let epsg = WktParser::extract_epsg(wkt);
        let authority = epsg.map(|_| "EPSG".to_string());
        Ok(Self {
            source: CrsSource::Wkt(wkt.to_string()),
            name,
            crs_type,
            unit,
            datum,
            authority,
        })
    }

    /// Determines the `CrsType` from the leading WKT keyword.
    fn crs_type_from_wkt_keyword(upper: &str) -> Option<CrsType> {
        if upper.starts_with("PROJCRS[") || upper.starts_with("PROJCS[") {
            Some(CrsType::Projected)
        } else if upper.starts_with("GEOGCRS[") || upper.starts_with("GEOGCS[") {
            Some(CrsType::Geographic)
        } else if upper.starts_with("GEODCRS[") || upper.starts_with("GEOCCS[") {
            Some(CrsType::Geocentric)
        } else if upper.starts_with("VERTCRS[") || upper.starts_with("VERT_CS[") {
            Some(CrsType::Vertical)
        } else if upper.starts_with("COMPOUNDCRS[") || upper.starts_with("COMPD_CS[") {
            Some(CrsType::Compound)
        } else if upper.starts_with("ENGCRS[") {
            Some(CrsType::Engineering)
        } else {
            None
        }
    }

    /// Extract datum name from a WKT string (supports both WKT1 and WKT2).
    fn extract_datum_from_wkt(wkt: &str) -> Option<String> {
        // WKT1: DATUM["name" ...]
        // WKT2: DATUM["name" ...] (same keyword)
        for keyword in &["DATUM[\"", "DATUM [\""] {
            if let Some(idx) = wkt.find(keyword) {
                let after = &wkt[idx + keyword.len()..];
                let end = after.find('"')?;
                return Some(after[..end].to_string());
            }
        }
        None
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

    /// Creates a compound CRS from a horizontal and a vertical CRS.
    ///
    /// The horizontal component must be a [`CrsType::Geographic`] or
    /// [`CrsType::Projected`] CRS; the vertical component must be a
    /// [`CrsType::Vertical`] CRS.  Any other combination is rejected with
    /// [`Error::InvalidCompoundCrs`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCompoundCrs`] if the type constraints are not met.
    #[cfg(feature = "std")]
    pub fn compound(horizontal: Crs, vertical: Crs) -> Result<Crs> {
        // Validate horizontal component type.
        match horizontal.crs_type() {
            Some(CrsType::Geographic) | Some(CrsType::Projected) => {}
            other => {
                return Err(Error::invalid_compound_crs(format!(
                    "horizontal CRS must be Geographic or Projected, got {:?}",
                    other
                )));
            }
        }
        // Validate vertical component type.
        if !matches!(vertical.crs_type(), Some(CrsType::Vertical)) {
            return Err(Error::invalid_compound_crs(format!(
                "vertical CRS must be Vertical, got {:?}",
                vertical.crs_type()
            )));
        }
        let name = format!("{} + {}", horizontal, vertical);
        Ok(Crs {
            source: CrsSource::Compound {
                horizontal: Box::new(horizontal),
                vertical: Box::new(vertical),
            },
            name: Some(name),
            crs_type: Some(CrsType::Compound),
            unit: None,
            datum: None,
            authority: None,
        })
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
            CrsSource::Compound { .. } => None,
            _ => None,
        }
    }

    /// Returns the area of use for this CRS, if available.
    ///
    /// For EPSG-code-based CRS definitions, this returns the declared geographic
    /// bounds.  For CRS created from a PROJ string, WKT, or custom definition,
    /// no area-of-use information is embedded and `None` is returned.
    /// For compound CRS, delegates to the horizontal component.
    pub fn area_of_use(&self) -> Option<AreaOfUse> {
        match &self.source {
            CrsSource::Epsg(code) => area_of_use_for_epsg(*code),
            CrsSource::Compound { horizontal, .. } => horizontal.area_of_use(),
            _ => None,
        }
    }

    /// Converts the CRS to a PROJ string.
    ///
    /// # Errors
    ///
    /// Returns an error if the conversion fails.
    #[cfg(feature = "std")]
    pub fn to_proj_string(&self) -> Result<String> {
        match &self.source {
            CrsSource::Epsg(code) => {
                let def = lookup_epsg(*code)?;
                Ok(def.proj_string.clone())
            }
            CrsSource::Proj(proj_string) => Ok(proj_string.clone()),
            CrsSource::Wkt(wkt) => crate::wkt_to_proj::wkt_to_proj_string(wkt),
            CrsSource::Custom { definition, .. } => Ok(definition.clone()),
            CrsSource::Compound { horizontal, .. } => horizontal.to_proj_string(),
        }
    }

    /// Converts the CRS to a PROJ string (no_std version).
    #[cfg(not(feature = "std"))]
    pub fn to_proj_string(&self) -> Result<String> {
        match &self.source {
            CrsSource::Proj(proj_string) => Ok(proj_string.clone()),
            CrsSource::Custom { definition, .. } => Ok(definition.clone()),
            CrsSource::Compound { horizontal, .. } => horizontal.to_proj_string(),
            _ => Err(Error::unsupported_crs(
                "EPSG/WKT lookup requires std feature",
            )),
        }
    }

    /// Converts the CRS to a WKT string.
    ///
    /// # Errors
    ///
    /// Returns an error if the conversion fails.
    #[cfg(feature = "std")]
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
            CrsSource::Compound {
                horizontal,
                vertical,
            } => {
                let name = self.name.as_deref().unwrap_or("Compound CRS");
                let h_wkt = horizontal.to_wkt()?;
                let v_wkt = vertical.to_wkt()?;
                Ok(format!("COMPOUNDCRS[\"{}\",{},{}]", name, h_wkt, v_wkt))
            }
        }
    }

    /// Converts the CRS to a WKT string (no_std version).
    #[cfg(not(feature = "std"))]
    pub fn to_wkt(&self) -> Result<String> {
        match &self.source {
            CrsSource::Wkt(wkt) => Ok(wkt.clone()),
            CrsSource::Compound { .. } => Err(Error::unsupported_crs(
                "Compound CRS to WKT requires std feature",
            )),
            _ => Err(Error::unsupported_crs("WKT lookup requires std feature")),
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

    /// Checks if this is an Engineering (local) CRS.
    ///
    /// Engineering CRS represents local coordinate systems (e.g. ship-board,
    /// construction-site grids) with no geodetic datum.  No spatial conversion
    /// to or from a geodetic CRS is possible without additional parameters.
    pub fn is_engineering(&self) -> bool {
        matches!(self.crs_type, Some(CrsType::Engineering))
    }

    /// Checks if two CRS are equivalent.
    ///
    /// This performs a basic comparison. For more sophisticated comparison,
    /// use proper projection transformation libraries.
    pub fn is_equivalent(&self, other: &Crs) -> bool {
        // Compound-to-compound: both horizontal and vertical must match.
        if let (
            CrsSource::Compound {
                horizontal: sh,
                vertical: sv,
            },
            CrsSource::Compound {
                horizontal: oh,
                vertical: ov,
            },
        ) = (&self.source, &other.source)
        {
            return sh.is_equivalent(oh) && sv.is_equivalent(ov);
        }

        // Compound vs non-compound — not equivalent.
        if matches!(&self.source, CrsSource::Compound { .. })
            || matches!(&other.source, CrsSource::Compound { .. })
        {
            return false;
        }

        // Simple case: same EPSG code
        if let (Some(code1), Some(code2)) = (self.epsg_code(), other.epsg_code()) {
            return code1 == code2;
        }

        // WKT-to-WKT: byte-identical WKT strings are equivalent.
        if let (CrsSource::Wkt(w1), CrsSource::Wkt(w2)) = (&self.source, &other.source) {
            return w1 == w2;
        }

        // Otherwise, compare PROJ strings if possible
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
            CrsSource::Compound {
                horizontal,
                vertical,
            } => {
                if let Some(name) = &self.name {
                    write!(f, "{}", name)
                } else {
                    write!(f, "{} + {}", horizontal, vertical)
                }
            }
        }
    }
}

// Common CRS constants for convenience
impl Crs {
    /// Returns the ITRF reference frame name if this CRS is based on an ITRF realization.
    ///
    /// Recognises ITRF frames by their EPSG code or by pattern-matching the CRS name
    /// (for CRS created from PROJ strings or WKT).  Returns `None` for non-ITRF CRS.
    ///
    /// # Recognised EPSG codes
    ///
    /// | Code  | Frame     |
    /// |-------|-----------|
    /// | 7900  | ITRF97    |
    /// | 7901  | ITRF2000  |
    /// | 7902  | ITRF2005  |
    /// | 7930  | ITRF2008  |
    /// | 7789  | ITRF2014  |
    /// | 9000  | ITRF2020  |
    pub fn itrf_name(&self) -> Option<String> {
        match &self.source {
            CrsSource::Epsg(code) => match code {
                7900 => Some("ITRF97".to_string()),
                7901 => Some("ITRF2000".to_string()),
                7902 => Some("ITRF2005".to_string()),
                7930 => Some("ITRF2008".to_string()),
                7789 => Some("ITRF2014".to_string()),
                9000 => Some("ITRF2020".to_string()),
                _ => None,
            },
            // For PROJ/WKT/Custom CRS: check datum name or CRS name.
            _ => {
                let candidate = self.datum.as_deref().or(self.name.as_deref()).unwrap_or("");
                // Match well-known ITRF name strings (case-insensitive prefix check).
                for frame in &[
                    "ITRF2020", "ITRF2014", "ITRF2008", "ITRF2005", "ITRF2000", "ITRF97", "ITRF96",
                ] {
                    if candidate.to_uppercase().contains(frame) {
                        return Some((*frame).to_string());
                    }
                }
                None
            }
        }
    }

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

    // =========================================================================
    // WKT2:2019 acceptance tests
    // =========================================================================

    #[test]
    fn test_from_wkt2_geogcrs() {
        let wkt = r#"GEOGCRS["WGS 84",DATUM["World Geodetic System 1984",ELLIPSOID["WGS 84",6378137,298.257223563]],ID["EPSG",4326]]"#;
        let crs = Crs::from_wkt(wkt).expect("should accept GEOGCRS");
        assert_eq!(crs.name(), Some("WGS 84"));
        assert_eq!(crs.crs_type(), Some(CrsType::Geographic));
        assert_eq!(crs.datum(), Some("World Geodetic System 1984"));
        assert!(crs.is_geographic());
        assert!(!crs.is_projected());
    }

    #[test]
    fn test_from_wkt2_projcrs() {
        let wkt = r#"PROJCRS["WGS 84 / UTM zone 33N",BASEGEOGCRS["WGS 84",DATUM["World Geodetic System 1984",ELLIPSOID["WGS 84",6378137,298.257223563]]],LENGTHUNIT["metre",1],ID["EPSG",32633]]"#;
        let crs = Crs::from_wkt(wkt).expect("should accept PROJCRS");
        assert_eq!(crs.name(), Some("WGS 84 / UTM zone 33N"));
        assert_eq!(crs.crs_type(), Some(CrsType::Projected));
        assert!(crs.is_projected());
        assert_eq!(crs.unit(), Some("metre"));
    }

    #[test]
    fn test_from_wkt2_geodcrs() {
        let wkt = r#"GEODCRS["WGS 84",DATUM["WGS 1984"]]"#;
        let crs = Crs::from_wkt(wkt).expect("should accept GEODCRS");
        assert_eq!(crs.crs_type(), Some(CrsType::Geocentric));
    }

    #[test]
    fn test_from_wkt2_vertcrs() {
        let wkt = r#"VERTCRS["EGM96 height",UNIT["metre",1]]"#;
        let crs = Crs::from_wkt(wkt).expect("should accept VERTCRS");
        assert_eq!(crs.crs_type(), Some(CrsType::Vertical));
        assert_eq!(crs.name(), Some("EGM96 height"));
    }

    #[test]
    fn test_from_wkt2_engcrs() {
        let wkt = r#"ENGCRS["Local Engineering"]"#;
        let crs = Crs::from_wkt(wkt).expect("should accept ENGCRS");
        assert_eq!(crs.crs_type(), Some(CrsType::Engineering));
    }

    #[test]
    fn test_from_wkt2_compoundcrs() {
        let wkt =
            r#"COMPOUNDCRS["WGS 84 + EGM96 height",GEOGCRS["WGS 84"],VERTCRS["EGM96 height"]]"#;
        let crs = Crs::from_wkt(wkt).expect("should accept COMPOUNDCRS");
        assert_eq!(crs.crs_type(), Some(CrsType::Compound));
    }

    // =========================================================================
    // Compound CRS constructor tests
    // =========================================================================

    #[test]
    fn test_crs_compound_constructor_validates_horizontal_must_be_geographic_or_projected() {
        // Geographic horizontal — should succeed.
        let horiz = Crs::wgs84(); // Geographic
        let vert_wkt = r#"VERTCRS["EGM96 height",VDATUM["EGM96 geoid"],CS[vertical,1],AXIS["gravity-related height (H)",up,LENGTHUNIT["metre",1]]]"#;
        let vert = Crs::from_wkt(vert_wkt).expect("should parse VERTCRS");
        let compound = Crs::compound(horiz, vert);
        assert!(compound.is_ok(), "geographic + vertical should succeed");

        let c = compound.expect("should be ok");
        assert_eq!(c.crs_type(), Some(CrsType::Compound));
        assert!(matches!(c.source(), CrsSource::Compound { .. }));
    }

    #[test]
    fn test_crs_compound_constructor_rejects_vertical_as_horizontal() {
        // Using a vertical CRS as the horizontal component must fail.
        let vert_wkt = r#"VERTCRS["EGM96 height",VDATUM["EGM96 geoid"],CS[vertical,1],AXIS["gravity-related height (H)",up,LENGTHUNIT["metre",1]]]"#;
        let vert1 = Crs::from_wkt(vert_wkt).expect("should parse VERTCRS");
        let vert2 = Crs::from_wkt(vert_wkt).expect("should parse VERTCRS");
        let result = Crs::compound(vert1, vert2);
        assert!(result.is_err(), "vertical-as-horizontal must be rejected");
        let err = result.expect_err("should error");
        assert!(
            matches!(err, crate::error::Error::InvalidCompoundCrs { .. }),
            "should be InvalidCompoundCrs, got: {:?}",
            err
        );
    }

    #[test]
    fn test_from_wkt_compound_wkt2_parses_horizontal_and_vertical_components() {
        // COMPOUNDCRS WKT2 string using constructs the existing WktParser handles
        // correctly (all direction keywords and CS type names quoted or omitted,
        // no ORDER / ENSEMBLEACCURACY / MEMBER sub-nodes that start with unquoted
        // identifiers not supported by the simple recursive-descent parser).
        let wkt = concat!(
            r#"COMPOUNDCRS["WGS 84 + EGM96","#,
            r#"GEOGCRS["WGS 84","#,
            r#"DATUM["World Geodetic System 1984","#,
            r#"ELLIPSOID["WGS 84",6378137,298.257223563,LENGTHUNIT["metre",1]]],"#,
            r#"PRIMEM["Greenwich",0,ANGLEUNIT["degree",0.0174532925199433]],"#,
            r#"UNIT["degree",0.0174532925199433],"#,
            r#"ID["EPSG",4326]],"#,
            r#"VERTCRS["EGM96 height","#,
            r#"VDATUM["EGM96 geoid"],"#,
            r#"UNIT["metre",1]]]"#
        );
        let crs = Crs::from_wkt(wkt).expect("should parse compound WKT2");
        assert_eq!(crs.crs_type(), Some(CrsType::Compound));

        // The source must be a true Compound variant (not Wkt).
        assert!(
            matches!(crs.source(), CrsSource::Compound { .. }),
            "source should be CrsSource::Compound, got {:?}",
            crs.source()
        );

        // Drill into components via pattern match.
        let CrsSource::Compound {
            horizontal,
            vertical,
        } = crs.source()
        else {
            unreachable!("source must be CrsSource::Compound at this point");
        };
        assert_eq!(
            horizontal.crs_type(),
            Some(CrsType::Geographic),
            "horizontal should be Geographic"
        );
        assert_eq!(
            vertical.crs_type(),
            Some(CrsType::Vertical),
            "vertical should be Vertical"
        );
        assert_eq!(horizontal.name(), Some("WGS 84"));
        assert_eq!(vertical.name(), Some("EGM96 height"));
    }

    #[test]
    fn test_from_wkt2_boundcrs() {
        let wkt = r#"BOUNDCRS["Bound WGS 84",GEOGCRS["WGS 84"]]"#;
        let crs = Crs::from_wkt(wkt).expect("should accept BOUNDCRS");
        // BOUNDCRS has no specific CrsType mapping — crs_type is None
        assert!(crs.name().is_some());
    }

    #[test]
    fn test_from_wkt2_invalid_keyword() {
        let result = Crs::from_wkt("FOOBAR[\"test\"]");
        assert!(result.is_err());
    }

    #[test]
    fn test_from_wkt_extracts_metadata_wkt1() {
        let wkt = r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],UNIT["degree",0.0174532925199433],AUTHORITY["EPSG","4326"]]"#;
        let crs = Crs::from_wkt(wkt).expect("should accept WKT1");
        assert_eq!(crs.name(), Some("WGS 84"));
        assert_eq!(crs.crs_type(), Some(CrsType::Geographic));
        assert_eq!(crs.datum(), Some("WGS_1984"));
        assert_eq!(crs.unit(), Some("degree"));
    }

    #[test]
    fn test_wkt2_roundtrip_name_epsg() {
        // Create from WKT2, get name and verify WKT is stored
        let wkt = r#"GEOGCRS["WGS 84",DATUM["World Geodetic System 1984",ELLIPSOID["WGS 84",6378137,298.257223563]],ID["EPSG",4326]]"#;
        let crs = Crs::from_wkt(wkt).expect("should parse");
        assert_eq!(crs.name(), Some("WGS 84"));
        // WKT roundtrip: to_wkt should return the original WKT
        let wkt_out = crs.to_wkt().expect("should return WKT");
        assert_eq!(wkt_out, wkt);
    }
}
