//! Spatial reference (CRS) types for OxiGeo.
//!
//! This module provides [`SpatialReference`] — a parsed, format-aware wrapper
//! around any string-form coordinate reference system identifier (WKT, EPSG
//! code string, PROJ string, or unknown).

#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use crate::compat::*;
use core::fmt;

/// The detected encoding format of a CRS string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrsFormat {
    /// OGC WKT (WKT1 or WKT2) — starts with `GEOGCS[`, `PROJCS[`, etc.
    Wkt,
    /// EPSG authority code — e.g. `"EPSG:4326"` or bare `"4326"`.
    EpsgCode,
    /// PROJ string — starts with `+proj=` or `proj=`.
    ProjString,
    /// Format could not be detected.
    Unknown,
}

/// A parsed, format-aware representation of a Coordinate Reference System.
///
/// Wraps a CRS string in any of the common geospatial encodings (WKT, EPSG
/// code, PROJ string) and exposes typed accessors so callers can query the
/// authority, numeric EPSG code, human-readable name, and projection class
/// without reaching for an external PROJ/GDAL library.
///
/// # Examples
///
/// ```
/// use oxigeo_core::types::SpatialReference;
///
/// let sr = SpatialReference::from_epsg(4326);
/// assert_eq!(sr.epsg(), Some(4326));
/// assert!(sr.is_geographic());
/// assert_eq!(sr.to_epsg_string(), Some("EPSG:4326".to_string()));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpatialReference {
    /// The canonical string representation (raw input).
    raw: String,
    /// The detected format of `raw`.
    format: CrsFormat,
    /// EPSG numeric code if known (e.g. 4326 for WGS84).
    epsg: Option<u32>,
    /// Short name/authority identifier (e.g. "WGS 84", "GDA2020").
    name: Option<String>,
}

// ── Constructors ─────────────────────────────────────────────────────────────

impl SpatialReference {
    /// Parse a CRS from any string representation.
    ///
    /// Detects the format automatically:
    /// - `EPSG:NNNN` / `epsg:NNNN` → [`CrsFormat::EpsgCode`]
    /// - `+proj=…` / `proj=…` → [`CrsFormat::ProjString`]
    /// - `GEOGCS[…`, `PROJCS[…`, `PROJCRS[…`, `GEOGCRS[…`, `COMPOUNDCRS[…`
    ///   → [`CrsFormat::Wkt`]
    /// - anything else → [`CrsFormat::Unknown`]
    ///
    /// # Examples
    ///
    /// ```
    /// use oxigeo_core::types::{SpatialReference, CrsFormat};
    ///
    /// let sr = SpatialReference::from_str("EPSG:4326");
    /// assert_eq!(sr.format(), CrsFormat::EpsgCode);
    /// assert_eq!(sr.epsg(), Some(4326));
    /// ```
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        let trimmed = s.trim();
        let upper = trimmed.to_uppercase();

        if upper.starts_with("EPSG:") {
            let code_str = &trimmed[5..];
            let epsg = code_str.trim().parse::<u32>().ok();
            let name = epsg.and_then(well_known_epsg_name).map(String::from);
            return Self {
                raw: trimmed.to_string(),
                format: CrsFormat::EpsgCode,
                epsg,
                name,
            };
        }

        if upper.starts_with("+PROJ=") || upper.starts_with("PROJ=") {
            let name = extract_proj_hint(trimmed).map(String::from);
            return Self {
                raw: trimmed.to_string(),
                format: CrsFormat::ProjString,
                epsg: None,
                name,
            };
        }

        for prefix in &["GEOGCS[", "PROJCS[", "PROJCRS[", "GEOGCRS[", "COMPOUNDCRS["] {
            if upper.starts_with(prefix) {
                let name = extract_wkt_name(trimmed).map(String::from);
                return Self {
                    raw: trimmed.to_string(),
                    format: CrsFormat::Wkt,
                    epsg: None,
                    name,
                };
            }
        }

        Self {
            raw: trimmed.to_string(),
            format: CrsFormat::Unknown,
            epsg: None,
            name: None,
        }
    }

    /// Create from a known EPSG numeric code.
    ///
    /// The canonical `raw` string is set to `"EPSG:{code}"`.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxigeo_core::types::SpatialReference;
    ///
    /// let sr = SpatialReference::from_epsg(32632);
    /// assert!(sr.is_projected());
    /// ```
    pub fn from_epsg(code: u32) -> Self {
        let raw = format!("EPSG:{code}");
        let name = well_known_epsg_name(code).map(String::from);
        Self {
            raw,
            format: CrsFormat::EpsgCode,
            epsg: Some(code),
            name,
        }
    }

    /// Create from a WKT string (WKT1 or WKT2).
    ///
    /// The format is forced to [`CrsFormat::Wkt`] regardless of content.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxigeo_core::types::{SpatialReference, CrsFormat};
    ///
    /// let sr = SpatialReference::from_wkt("GEOGCS[\"WGS 84\",...]");
    /// assert_eq!(sr.format(), CrsFormat::Wkt);
    /// ```
    pub fn from_wkt(wkt: &str) -> Self {
        let name = extract_wkt_name(wkt).map(String::from);
        Self {
            raw: wkt.to_string(),
            format: CrsFormat::Wkt,
            epsg: None,
            name,
        }
    }

    /// Create from a PROJ string (starts with `+proj=` or `proj=`).
    ///
    /// The format is forced to [`CrsFormat::ProjString`].
    ///
    /// # Examples
    ///
    /// ```
    /// use oxigeo_core::types::{SpatialReference, CrsFormat};
    ///
    /// let sr = SpatialReference::from_proj_string("+proj=utm +zone=32 +datum=WGS84");
    /// assert_eq!(sr.format(), CrsFormat::ProjString);
    /// ```
    pub fn from_proj_string(proj: &str) -> Self {
        let name = extract_proj_hint(proj).map(String::from);
        Self {
            raw: proj.to_string(),
            format: CrsFormat::ProjString,
            epsg: None,
            name,
        }
    }
}

// ── Accessors ─────────────────────────────────────────────────────────────────

impl SpatialReference {
    /// Returns the raw CRS string as provided (or as constructed from an EPSG code).
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Returns the detected format of the CRS string.
    #[must_use]
    pub fn format(&self) -> CrsFormat {
        self.format
    }

    /// Returns the EPSG numeric code if known.
    #[must_use]
    pub fn epsg(&self) -> Option<u32> {
        self.epsg
    }

    /// Returns a human-readable short name if available
    /// (e.g. `"WGS 84"`, `"Web Mercator / Pseudo-Mercator"`).
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Returns `true` if this CRS is a geographic (lat/lon) system.
    ///
    /// Heuristic rules:
    /// - EPSG 4000–4999 → geographic
    /// - WKT starting with `GEOGCS[` or `GEOGCRS[` → geographic
    /// - PROJ string containing `+proj=longlat` or `+proj=latlong` → geographic
    #[must_use]
    pub fn is_geographic(&self) -> bool {
        match self.format {
            CrsFormat::EpsgCode => self
                .epsg
                .map(|c| (4000..=4999).contains(&c))
                .unwrap_or(false),
            CrsFormat::Wkt => {
                let u = self.raw.to_uppercase();
                u.starts_with("GEOGCS[") || u.starts_with("GEOGCRS[")
            }
            CrsFormat::ProjString => {
                let u = self.raw.to_uppercase();
                u.contains("+PROJ=LONGLAT")
                    || u.contains("+PROJ=LATLONG")
                    || u.contains("PROJ=LONGLAT")
                    || u.contains("PROJ=LATLONG")
            }
            CrsFormat::Unknown => false,
        }
    }

    /// Returns `true` if this CRS is a projected (metric) system.
    ///
    /// Heuristic rules:
    /// - EPSG 25000–32999 → projected UTM / similar
    /// - WKT starting with `PROJCS[` or `PROJCRS[` → projected
    /// - PROJ string not containing `+proj=longlat` / `+proj=latlong` → projected
    #[must_use]
    pub fn is_projected(&self) -> bool {
        match self.format {
            CrsFormat::EpsgCode => self
                .epsg
                .map(|c| (25000..=32999).contains(&c))
                .unwrap_or(false),
            CrsFormat::Wkt => {
                let u = self.raw.to_uppercase();
                u.starts_with("PROJCS[") || u.starts_with("PROJCRS[")
            }
            CrsFormat::ProjString => !self.is_geographic(),
            CrsFormat::Unknown => false,
        }
    }

    /// Returns the authority string (`"EPSG"`) if the EPSG code is set.
    #[must_use]
    pub fn authority(&self) -> Option<&str> {
        if self.epsg.is_some() {
            Some("EPSG")
        } else {
            None
        }
    }

    /// Returns the `"EPSG:NNNN"` string if the EPSG code is known.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxigeo_core::types::SpatialReference;
    ///
    /// assert_eq!(
    ///     SpatialReference::from_epsg(4326).to_epsg_string(),
    ///     Some("EPSG:4326".to_string())
    /// );
    /// ```
    #[must_use]
    pub fn to_epsg_string(&self) -> Option<String> {
        self.epsg.map(|c| format!("EPSG:{c}"))
    }
}

// ── Display / From ────────────────────────────────────────────────────────────

impl fmt::Display for SpatialReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.raw)
    }
}

impl From<&str> for SpatialReference {
    fn from(s: &str) -> Self {
        Self::from_str(s)
    }
}

impl From<String> for SpatialReference {
    fn from(s: String) -> Self {
        Self::from_str(&s)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the well-known name for a handful of common EPSG codes.
fn well_known_epsg_name(code: u32) -> Option<&'static str> {
    match code {
        4326 => Some("WGS 84"),
        3857 => Some("Web Mercator / Pseudo-Mercator"),
        4269 => Some("NAD83"),
        32632 => Some("WGS 84 / UTM zone 32N"),
        _ => None,
    }
}

/// Extract the first double-quoted string after the opening `[` in a WKT value.
///
/// For `GEOGCS["WGS 84", ...]` this returns `"WGS 84"`.
fn extract_wkt_name(wkt: &str) -> Option<&str> {
    let after_bracket = wkt.find('[').map(|i| &wkt[i + 1..])?;
    let start = after_bracket.find('"').map(|i| i + 1)?;
    let content = &after_bracket[start..];
    let end = content.find('"')?;
    Some(&content[..end])
}

/// Try to extract a hint string from a PROJ string via `+datum=` or `+ellps=`.
fn extract_proj_hint(proj: &str) -> Option<&str> {
    for key in &["+datum=", "datum=", "+ellps=", "ellps="] {
        if let Some(pos) = proj.find(key) {
            let after = &proj[pos + key.len()..];
            let end = after.find(' ').unwrap_or(after.len());
            let value = after[..end].trim();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_epsg_string() {
        let sr = SpatialReference::from_str("EPSG:4326");
        assert_eq!(sr.epsg(), Some(4326));
        assert_eq!(sr.format(), CrsFormat::EpsgCode);
    }

    #[test]
    fn test_from_epsg_lowercase() {
        let sr = SpatialReference::from_str("epsg:3857");
        assert_eq!(sr.epsg(), Some(3857));
        assert_eq!(sr.format(), CrsFormat::EpsgCode);
    }

    #[test]
    fn test_from_wkt_geogcs() {
        let wkt = r#"GEOGCS["WGS 84",DATUM["WGS_1984"],PRIMEM["Greenwich",0]]"#;
        let sr = SpatialReference::from_str(wkt);
        assert_eq!(sr.format(), CrsFormat::Wkt);
        assert_eq!(sr.name(), Some("WGS 84"));
    }

    #[test]
    fn test_from_proj_string() {
        let sr = SpatialReference::from_str("+proj=utm +zone=32 +datum=WGS84");
        assert_eq!(sr.format(), CrsFormat::ProjString);
        assert!(sr.is_projected());
    }

    #[test]
    fn test_is_geographic_epsg_4326() {
        let sr = SpatialReference::from_epsg(4326);
        assert!(sr.is_geographic());
        assert!(!sr.is_projected());
    }

    #[test]
    fn test_is_projected_epsg_32632() {
        let sr = SpatialReference::from_epsg(32632);
        assert!(sr.is_projected());
        assert!(!sr.is_geographic());
    }

    #[test]
    fn test_from_str_unknown() {
        let sr = SpatialReference::from_str("not a CRS");
        assert_eq!(sr.format(), CrsFormat::Unknown);
        assert_eq!(sr.epsg(), None);
    }

    #[test]
    fn test_display() {
        let sr = SpatialReference::from_epsg(4326);
        let s = format!("{sr}");
        assert!(
            s.contains("4326"),
            "Display should contain '4326', got: {s}"
        );
    }

    #[test]
    fn test_from_str_conversion() {
        let sr = SpatialReference::from("EPSG:4269");
        assert_eq!(sr.epsg(), Some(4269));
        assert_eq!(sr.format(), CrsFormat::EpsgCode);
    }

    #[test]
    fn test_to_epsg_string() {
        let sr = SpatialReference::from_epsg(4326);
        assert_eq!(sr.to_epsg_string(), Some("EPSG:4326".to_string()));
    }

    #[test]
    fn test_from_wkt_projcs() {
        let wkt = r#"PROJCS["WGS 84 / UTM zone 32N",GEOGCS["WGS 84"]]"#;
        let sr = SpatialReference::from_str(wkt);
        assert_eq!(sr.format(), CrsFormat::Wkt);
        assert!(sr.is_projected());
        assert_eq!(sr.name(), Some("WGS 84 / UTM zone 32N"));
    }

    #[test]
    fn test_authority_with_epsg() {
        let sr = SpatialReference::from_epsg(4326);
        assert_eq!(sr.authority(), Some("EPSG"));
    }

    #[test]
    fn test_authority_without_epsg() {
        let sr = SpatialReference::from_str("not a CRS");
        assert_eq!(sr.authority(), None);
    }

    #[test]
    fn test_well_known_names() {
        assert_eq!(SpatialReference::from_epsg(4326).name(), Some("WGS 84"));
        assert_eq!(
            SpatialReference::from_epsg(3857).name(),
            Some("Web Mercator / Pseudo-Mercator")
        );
        assert_eq!(SpatialReference::from_epsg(4269).name(), Some("NAD83"));
        assert_eq!(
            SpatialReference::from_epsg(32632).name(),
            Some("WGS 84 / UTM zone 32N")
        );
        assert_eq!(SpatialReference::from_epsg(99999).name(), None);
    }

    #[test]
    fn test_proj_geographic() {
        let sr = SpatialReference::from_proj_string("+proj=longlat +datum=WGS84 +no_defs");
        assert!(sr.is_geographic());
        assert!(!sr.is_projected());
    }

    #[test]
    fn test_from_wkt_constructor() {
        let wkt = r#"GEOGCS["NAD83",DATUM["North_American_Datum_1983"]]"#;
        let sr = SpatialReference::from_wkt(wkt);
        assert_eq!(sr.format(), CrsFormat::Wkt);
        assert_eq!(sr.name(), Some("NAD83"));
    }

    #[test]
    fn test_from_string_trait() {
        let sr = SpatialReference::from("EPSG:4326".to_string());
        assert_eq!(sr.epsg(), Some(4326));
    }

    #[test]
    fn test_raw_accessor() {
        let sr = SpatialReference::from_epsg(4326);
        assert_eq!(sr.raw(), "EPSG:4326");
    }

    #[test]
    fn test_epsg_unknown_code_no_name() {
        let sr = SpatialReference::from_epsg(1234);
        assert_eq!(sr.name(), None);
        assert_eq!(sr.epsg(), Some(1234));
    }

    #[test]
    fn test_geogcrs_wkt2_detected() {
        let wkt = r#"GEOGCRS["WGS 84",DATUM["World Geodetic System 1984"]]"#;
        let sr = SpatialReference::from_str(wkt);
        assert_eq!(sr.format(), CrsFormat::Wkt);
        assert!(sr.is_geographic());
    }

    #[test]
    fn test_projcrs_wkt2_detected() {
        let wkt = r#"PROJCRS["WGS 84 / UTM zone 33N",BASEGEOGCRS["WGS 84"]]"#;
        let sr = SpatialReference::from_str(wkt);
        assert_eq!(sr.format(), CrsFormat::Wkt);
        assert!(sr.is_projected());
    }
}
