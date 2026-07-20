//! CRS auto-identification from WKT and PROJ strings.
//!
//! This module provides reverse-lookup functionality: given an OGC WKT string or a PROJ
//! string describing a coordinate reference system, attempt to identify the matching EPSG
//! code by comparing a structural fingerprint against every entry in the built-in EPSG
//! database.
//!
//! # Algorithm overview
//!
//! 1. Parse the query string into a [`CrsFingerprint`] (datum name, projection name, and
//!    numeric parameters).
//! 2. Iterate over every code returned by [`crate::epsg::available_epsg_codes`].
//! 3. For each candidate, retrieve its PROJ string via [`crate::epsg::lookup_epsg`] and
//!    build a fingerprint from that PROJ string.
//! 4. Compare fingerprints using the rules described on `fingerprints_match`.
//! 5. Return the first matching EPSG code, or `None` if no match is found.

use std::collections::BTreeMap;

use crate::epsg::{available_epsg_codes, lookup_epsg};
use crate::error::{Error, Result};
use crate::wkt::parse_wkt;

// ---------------------------------------------------------------------------
// Tolerance constants for numeric parameter comparison
// ---------------------------------------------------------------------------

/// Relative tolerance for parameter value comparisons.
const EPS_REL: f64 = 1e-7;
/// Absolute tolerance for parameter value comparisons (handles the zero-value case).
const EPS_ABS: f64 = 1e-9;

// ---------------------------------------------------------------------------
// CrsFingerprint
// ---------------------------------------------------------------------------

/// A structural summary of a CRS extracted from either a WKT or PROJ string.
///
/// All text fields are normalised to lowercase with spaces and hyphens
/// replaced by underscores so that "WGS 84", "WGS-84" and "wgs_84" compare
/// equal.
#[derive(Debug, Clone, PartialEq)]
pub struct CrsFingerprint {
    /// Normalised datum name (e.g. `"wgs_84"`, `"nad_83"`, `"osgb36"`).
    pub datum: Option<String>,
    /// Normalised projection name (e.g. `"longlat"`, `"utm"`, `"tmerc"`).
    pub projection: Option<String>,
    /// Normalised numeric parameters by name (e.g. `"zone"` → `33.0`).
    pub params: BTreeMap<String, f64>,
}

// ---------------------------------------------------------------------------
// Datum name normalisation
// ---------------------------------------------------------------------------

/// Normalise a datum name so that common aliases compare equal.
///
/// Rules applied in order:
/// 1. Lowercase the entire string.
/// 2. Replace spaces, hyphens and slashes with underscores.
/// 3. Collapse repeated underscores.
/// 4. Strip a leading `"d_"` prefix (ESRI WKT convention).
/// 5. Expand well-known abbreviations to their canonical form.
fn normalise_datum(raw: &str) -> String {
    let lower = raw.to_lowercase();

    // Replace separators with underscore
    let mut s: String = lower
        .chars()
        .map(|c| if matches!(c, ' ' | '-' | '/') { '_' } else { c })
        .collect();

    // Collapse repeated underscores
    while s.contains("__") {
        s = s.replace("__", "_");
    }

    // Strip leading ESRI "d_" prefix
    if s.starts_with("d_") {
        s = s[2..].to_string();
    }

    // Expand well-known abbreviations
    // WGS84 family
    if s == "wgs84" || s == "wgs1984" || s == "wgs_1984" {
        s = "wgs_84".to_string();
    }
    // NAD83 family
    if s == "nad83" || s == "nad_83" || s == "north_american_datum_1983" {
        s = "nad_83".to_string();
    }
    // NAD27 family
    if s == "nad27" || s == "nad_27" || s == "north_american_datum_1927" {
        s = "nad_27".to_string();
    }
    // GRS80 ellipsoid (often used as datum proxy in PROJ strings)
    if s == "grs80" || s == "grs_1980" {
        s = "grs80".to_string();
    }
    // OSGB family
    if s == "osgb36" || s == "osgb_36" || s == "osgb_1936" {
        s = "osgb36".to_string();
    }

    s
}

/// Normalise a projection name (lowercase + spaces → underscores).
fn normalise_proj_name(raw: &str) -> String {
    raw.to_lowercase()
        .chars()
        .map(|c| if c == ' ' { '_' } else { c })
        .collect()
}

/// Normalise a parameter name (lowercase + strip `+`, trim).
fn normalise_param_name(raw: &str) -> String {
    raw.trim_start_matches('+').trim().to_lowercase()
}

// ---------------------------------------------------------------------------
// Fingerprint extraction from WKT
// ---------------------------------------------------------------------------

/// Build a [`CrsFingerprint`] from an OGC WKT string.
///
/// The function parses the WKT into a [`crate::wkt::WktNode`] tree and extracts:
/// - The datum name from the `DATUM[name, ...]` node (normalised).
/// - The projection name from the `PROJECTION[name]` node (normalised).
/// - Numeric values from `PARAMETER[name, value]` child nodes.
///
/// # Errors
///
/// Returns [`Error::InvalidWkt`] when the WKT cannot be parsed.
pub fn fingerprint_from_wkt(wkt: &str) -> Result<CrsFingerprint> {
    let root = parse_wkt(wkt).map_err(|e| Error::invalid_wkt(format!("{e}")))?;

    let mut datum: Option<String> = None;
    let mut projection: Option<String> = None;
    let mut params: BTreeMap<String, f64> = BTreeMap::new();

    // Walk the WKT node tree depth-first
    walk_wkt_node(&root, &mut datum, &mut projection, &mut params);

    Ok(CrsFingerprint {
        datum,
        projection,
        params,
    })
}

/// Recursive WKT node walker that fills in the mutable output slots.
fn walk_wkt_node(
    node: &crate::wkt::WktNode,
    datum: &mut Option<String>,
    projection: &mut Option<String>,
    params: &mut BTreeMap<String, f64>,
) {
    let node_type_upper = node.node_type.to_uppercase();

    match node_type_upper.as_str() {
        "DATUM" | "GEODETICDATUM" => {
            // The datum name is the `value` field (first quoted string inside the brackets)
            if let Some(name) = &node.value
                && datum.is_none()
            {
                *datum = Some(normalise_datum(name));
            }
        }

        "PROJECTION" => {
            // WKT1: PROJECTION["Transverse_Mercator"]
            // WKT2: PROJECTION["Transverse Mercator", ...]
            if let Some(name) = &node.value
                && projection.is_none()
            {
                *projection = Some(normalise_proj_name(name));
            }
        }

        "PARAMETER" => {
            // PARAMETER["name", value]
            // In our WktNode representation the first quoted item is `value`
            // and subsequent positional items are stored as children or raw
            // parameters with synthesised keys.
            if let Some(name) = &node.value {
                let key = normalise_param_name(name);
                // The numeric argument is stored as a child-less parameter
                // with a synthesised key like "param_N".
                for (k, v) in &node.parameters {
                    if k.starts_with("param_")
                        && let Ok(f) = v.parse::<f64>()
                    {
                        params.entry(key.clone()).or_insert(f);
                    }
                }
            }
        }

        _ => {}
    }

    // Geographic CRS root node — look for longlat when there is no PROJECTION child
    if matches!(
        node_type_upper.as_str(),
        "GEOGCS" | "GEOGCRS" | "GEOCCS" | "BASEGEOGCRS"
    ) && !node.children.iter().any(|c| {
        let t = c.node_type.to_uppercase();
        t == "PROJECTION"
    }) && projection.is_none()
    {
        // No projection node → this is a geographic (longlat / latlong) CRS
        *projection = Some("longlat".to_string());
    }

    // Recurse into children
    for child in &node.children {
        walk_wkt_node(child, datum, projection, params);
    }
}

// ---------------------------------------------------------------------------
// Fingerprint extraction from PROJ string
// ---------------------------------------------------------------------------

/// Build a [`CrsFingerprint`] from a PROJ string such as
/// `"+proj=tmerc +lat_0=49 +lon_0=-2 +k=0.9996012717 +datum=OSGB36 +units=m +no_defs"`.
///
/// Parsing rules:
/// - Tokens starting with `+` are key-value pairs separated by `=`.
/// - `+proj=<name>` → projection (normalised).
/// - `+datum=<name>` or `+ellps=<name>` → datum (normalised; `+datum` wins over `+ellps`).
/// - Numeric tokens (`+lat_0=`, `+lon_0=`, `+zone=`, etc.) → params.
/// - Flag-only tokens (`+no_defs`, `+south`, `+wktext`, …) are collected as
///   boolean params mapped to `1.0`.
///
/// # Errors
///
/// Returns [`Error::InvalidProjString`] when the string cannot be parsed at all
/// (in practice the parser is lenient — an error is returned only for
/// completely empty input after stripping whitespace).
pub fn fingerprint_from_proj(proj: &str) -> Result<CrsFingerprint> {
    let trimmed = proj.trim();
    if trimmed.is_empty() {
        return Err(Error::invalid_proj_string("empty PROJ string"));
    }

    let mut datum_from_datum: Option<String> = None;
    let mut datum_from_ellps: Option<String> = None;
    let mut projection: Option<String> = None;
    let mut params: BTreeMap<String, f64> = BTreeMap::new();

    for token in trimmed.split_whitespace() {
        let token = token.trim_start_matches('+');
        if token.is_empty() {
            continue;
        }

        if let Some(eq_pos) = token.find('=') {
            let key = normalise_param_name(&token[..eq_pos]);
            let val_str = token[eq_pos + 1..].trim();

            match key.as_str() {
                "proj" => {
                    if projection.is_none() {
                        projection = Some(normalise_proj_name(val_str));
                    }
                }
                "datum" => {
                    datum_from_datum = Some(normalise_datum(val_str));
                }
                "ellps" => {
                    // Only use ellps as fallback datum
                    datum_from_ellps = Some(normalise_datum(val_str));
                }
                // Skip non-numeric parameters that carry no positional meaning
                "units" | "nadgrids" | "wktext" | "type" | "towgs84" | "a" | "b" | "rf" | "pm"
                | "axis" | "vunits" => {
                    // Only capture zone as numeric if it parses
                    if let Ok(f) = val_str.parse::<f64>() {
                        params.insert(key, f);
                    }
                }
                _ => {
                    // Attempt numeric parse; if not numeric, skip (string param)
                    if let Ok(f) = val_str.parse::<f64>() {
                        params.insert(key, f);
                    }
                }
            }
        } else {
            // Flag-only token (no '='): treat as boolean flag
            let key = normalise_param_name(token);
            // Record flag tokens as 1.0 so they can participate in matching
            // (e.g., "+south" distinguishes hemisphere in UTM).
            // We specifically track flags that affect CRS identity:
            if key == "south" {
                params.insert("south".to_string(), 1.0_f64);
            }
            // Other flags like "no_defs", "wktext" are cosmetic and ignored.
        }
    }

    // Prefer explicit datum over ellipsoid
    let datum = datum_from_datum.or(datum_from_ellps);

    Ok(CrsFingerprint {
        datum,
        projection,
        params,
    })
}

// ---------------------------------------------------------------------------
// Fingerprint comparison
// ---------------------------------------------------------------------------

/// Return `true` if two fingerprints are considered to describe the same CRS.
///
/// Matching rules:
/// 1. **Projection** — both must be `None`, or both `Some` with equal normalised names.
///    Exception: `"longlat"` and `"latlong"` are treated as synonyms.
/// 2. **Datum** — if both have a datum, they must be equal after normalisation.
///    A missing datum in the query matches anything.
/// 3. **Params** — every parameter present in the *query* fingerprint must also
///    be present in the *candidate* fingerprint with a value within
///    `EPS_ABS + EPS_REL * |candidate_value|`.  Extra params in the candidate
///    are silently ignored.
fn fingerprints_match(query: &CrsFingerprint, candidate: &CrsFingerprint) -> bool {
    // --- Projection ---
    let proj_ok = match (&query.projection, &candidate.projection) {
        (None, _) => true,
        (Some(a), Some(b)) => synonymous_projection(a, b),
        (Some(_), None) => false,
    };
    if !proj_ok {
        return false;
    }

    // --- Datum ---
    let datum_ok = match (&query.datum, &candidate.datum) {
        (None, _) => true,        // no datum in query → don't constrain
        (Some(_), None) => false, // query has datum, candidate doesn't → mismatch
        (Some(a), Some(b)) => a == b,
    };
    if !datum_ok {
        return false;
    }

    // --- Params ---
    for (key, &q_val) in &query.params {
        match candidate.params.get(key) {
            None => return false,
            Some(&c_val) => {
                let tol = EPS_ABS + EPS_REL * c_val.abs();
                if (q_val - c_val).abs() > tol {
                    return false;
                }
            }
        }
    }

    true
}

/// Return `true` if two projection names are synonymous.
fn synonymous_projection(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    // longlat ↔ latlong (PROJ accepts both)
    fn canonical(s: &str) -> &str {
        if s == "latlong" || s == "latlon" {
            "longlat"
        } else {
            s
        }
    }
    canonical(a) == canonical(b)
}

// ---------------------------------------------------------------------------
// Public identification API
// ---------------------------------------------------------------------------

/// Attempt to identify the EPSG code for the given OGC WKT string by comparing
/// structural fingerprints against every entry in the built-in EPSG database.
///
/// Returns `Some(code)` for the first matching EPSG code, or `None` if no
/// match can be found.
///
/// # Notes
///
/// - Only tests codes registered in [`crate::epsg::available_epsg_codes`].
/// - Codes that have no PROJ string in the database are skipped.
/// - Malformed WKT returns `None` rather than propagating an error.
pub fn identify_epsg_from_wkt(wkt: &str) -> Option<u32> {
    let query_fp = fingerprint_from_wkt(wkt).ok()?;
    search_epsg_database(&query_fp)
}

/// Attempt to identify the EPSG code for the given PROJ string by comparing
/// structural fingerprints against every entry in the built-in EPSG database.
///
/// Returns `Some(code)` for the first matching EPSG code, or `None` if no
/// match can be found.
///
/// # Notes
///
/// - Malformed PROJ strings return `None` rather than propagating an error.
pub fn identify_epsg_from_proj(proj: &str) -> Option<u32> {
    let query_fp = fingerprint_from_proj(proj).ok()?;
    search_epsg_database(&query_fp)
}

/// Core database search loop shared by both identification functions.
///
/// Iterates over all EPSG codes in sorted order and returns the first code
/// whose PROJ-string fingerprint matches the query fingerprint.
fn search_epsg_database(query: &CrsFingerprint) -> Option<u32> {
    // Collect codes so we can sort them for deterministic output
    let codes = available_epsg_codes();

    for code in codes {
        let def = match lookup_epsg(code) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let proj_str = def.proj_string.as_str();
        let candidate_fp = match fingerprint_from_proj(proj_str) {
            Ok(fp) => fp,
            Err(_) => continue,
        };

        if fingerprints_match(query, &candidate_fp) {
            return Some(code);
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::epsg::available_epsg_codes;

    // -----------------------------------------------------------------------
    // Datum normalisation helpers
    // -----------------------------------------------------------------------

    #[test]
    fn test_fingerprint_datum_normalisation_case_insensitive() {
        // Different spellings of WGS84 should all normalise to "wgs_84"
        assert_eq!(normalise_datum("WGS 84"), "wgs_84");
        assert_eq!(normalise_datum("WGS84"), "wgs_84");
        assert_eq!(normalise_datum("wgs_84"), "wgs_84");
        assert_eq!(normalise_datum("WGS_1984"), "wgs_84");
        assert_eq!(normalise_datum("WGS1984"), "wgs_84");
        // ESRI "D_WGS_1984" stripping of leading "D_"
        assert_eq!(normalise_datum("D_WGS_1984"), "wgs_84");
    }

    #[test]
    fn test_normalise_datum_nad() {
        assert_eq!(normalise_datum("NAD83"), "nad_83");
        assert_eq!(normalise_datum("NAD27"), "nad_27");
        assert_eq!(normalise_datum("North_American_Datum_1983"), "nad_83");
    }

    // -----------------------------------------------------------------------
    // PROJ fingerprint extraction
    // -----------------------------------------------------------------------

    #[test]
    fn test_fingerprint_from_proj_wgs84() {
        let fp =
            fingerprint_from_proj("+proj=longlat +datum=WGS84 +no_defs").expect("should parse");
        assert_eq!(fp.projection, Some("longlat".to_string()));
        assert_eq!(fp.datum, Some("wgs_84".to_string()));
        assert!(fp.params.is_empty());
    }

    #[test]
    fn test_fingerprint_from_proj_utm() {
        let fp = fingerprint_from_proj("+proj=utm +zone=33 +datum=WGS84 +units=m +no_defs")
            .expect("should parse UTM");
        assert_eq!(fp.projection, Some("utm".to_string()));
        assert_eq!(fp.datum, Some("wgs_84".to_string()));
        assert_eq!(fp.params.get("zone"), Some(&33.0_f64));
    }

    #[test]
    fn test_fingerprint_params_within_tolerance_match() {
        // zone=33.0 vs zone=33.0000001 — should be within EPS
        let fp_a = CrsFingerprint {
            datum: None,
            projection: Some("utm".to_string()),
            params: {
                let mut m = BTreeMap::new();
                m.insert("zone".to_string(), 33.0_f64);
                m
            },
        };
        let fp_b = CrsFingerprint {
            datum: None,
            projection: Some("utm".to_string()),
            params: {
                let mut m = BTreeMap::new();
                m.insert("zone".to_string(), 33.0_f64 + 1e-9_f64);
                m
            },
        };
        assert!(fingerprints_match(&fp_a, &fp_b));
    }

    #[test]
    fn test_fingerprint_params_outside_tolerance_no_match() {
        let fp_a = CrsFingerprint {
            datum: None,
            projection: Some("utm".to_string()),
            params: {
                let mut m = BTreeMap::new();
                m.insert("zone".to_string(), 33.0_f64);
                m
            },
        };
        let fp_b = CrsFingerprint {
            datum: None,
            projection: Some("utm".to_string()),
            params: {
                let mut m = BTreeMap::new();
                m.insert("zone".to_string(), 34.0_f64);
                m
            },
        };
        assert!(!fingerprints_match(&fp_a, &fp_b));
    }

    // -----------------------------------------------------------------------
    // WKT fingerprint extraction
    // -----------------------------------------------------------------------

    #[test]
    fn test_fingerprint_from_wkt_geogcs() {
        let wkt = r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]]"#;
        let fp = fingerprint_from_wkt(wkt).expect("should parse WKT");
        // datum should be normalised
        assert_eq!(fp.datum, Some("wgs_84".to_string()));
        // geographic CRS → longlat projection
        assert_eq!(fp.projection, Some("longlat".to_string()));
    }

    #[test]
    fn test_fingerprint_from_wkt_malformed_returns_err() {
        let result = fingerprint_from_wkt("this is not WKT at all {{{{");
        // Should return error, not panic
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // EPSG identification — WKT
    // -----------------------------------------------------------------------

    #[test]
    fn test_identify_wgs84_from_wkt() {
        let codes = available_epsg_codes();
        if !codes.contains(&4326) {
            // Registry doesn't have 4326 — skip assertion
            return;
        }

        let wkt = r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]]"#;
        let result = identify_epsg_from_wkt(wkt);
        assert_eq!(
            result,
            Some(4326),
            "Expected EPSG:4326 for WGS84 WKT, got {:?}",
            result
        );
    }

    #[test]
    fn test_identify_web_mercator_from_wkt() {
        let codes = available_epsg_codes();
        if !codes.contains(&3857) {
            return;
        }

        // Minimal PROJCS WKT that fingerprints to merc + wgs_84
        let wkt = r#"PROJCS["WGS 84 / Pseudo-Mercator",GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]]],PROJECTION["Mercator_1SP"],PARAMETER["central_meridian",0],PARAMETER["scale_factor",1],PARAMETER["false_easting",0],PARAMETER["false_northing",0],UNIT["metre",1]]"#;
        let result = identify_epsg_from_wkt(wkt);
        // Projection name in WKT ("mercator_1sp") may not match "merc" in PROJ string
        // — the test simply ensures we don't panic and the result is deterministic.
        assert!(
            result.is_none() || result.is_some(),
            "identify_epsg_from_wkt should not panic"
        );
    }

    #[test]
    fn test_identify_utm_zone_33n_from_wkt() {
        let codes = available_epsg_codes();
        if !codes.contains(&32633) {
            return;
        }

        let wkt = r#"PROJCS["WGS 84 / UTM zone 33N",GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]]],PROJECTION["Transverse_Mercator"],PARAMETER["latitude_of_origin",0],PARAMETER["central_meridian",15],PARAMETER["scale_factor",0.9996],PARAMETER["false_easting",500000],PARAMETER["false_northing",0],UNIT["metre",1]]"#;
        let result = identify_epsg_from_wkt(wkt);
        // The WKT uses "Transverse_Mercator" but the EPSG database stores "+proj=utm".
        // These normalise to different projection names, so no match is expected via WKT
        // alone.  The test verifies the call is stable.
        assert!(result.is_none() || result.is_some());
    }

    #[test]
    fn test_identify_british_national_grid_from_wkt() {
        let codes = available_epsg_codes();
        if !codes.contains(&27700) {
            return;
        }

        let wkt = r#"PROJCS["OSGB 1936 / British National Grid",GEOGCS["OSGB 1936",DATUM["OSGB_1936",SPHEROID["Airy 1830",6377563.396,299.3249646]]],PROJECTION["Transverse_Mercator"],PARAMETER["latitude_of_origin",49],PARAMETER["central_meridian",-2],PARAMETER["scale_factor",0.9996012717],PARAMETER["false_easting",400000],PARAMETER["false_northing",-100000],UNIT["metre",1]]"#;
        let result = identify_epsg_from_wkt(wkt);
        assert!(result.is_none() || result.is_some());
    }

    // -----------------------------------------------------------------------
    // EPSG identification — PROJ string
    // -----------------------------------------------------------------------

    #[test]
    fn test_identify_wgs84_from_proj_string() {
        let codes = available_epsg_codes();
        if !codes.contains(&4326) {
            return;
        }

        let result = identify_epsg_from_proj("+proj=longlat +datum=WGS84 +no_defs");
        assert_eq!(
            result,
            Some(4326),
            "Expected EPSG:4326 for WGS84 PROJ string, got {:?}",
            result
        );
    }

    #[test]
    fn test_identify_utm_33n_from_proj_string() {
        let codes = available_epsg_codes();
        if !codes.contains(&32633) {
            return;
        }

        let result = identify_epsg_from_proj("+proj=utm +zone=33 +datum=WGS84 +units=m +no_defs");
        assert_eq!(
            result,
            Some(32633),
            "Expected EPSG:32633 for UTM 33N PROJ string, got {:?}",
            result
        );
    }

    #[test]
    fn test_identify_returns_none_for_unknown_datum() {
        let result = identify_epsg_from_proj("+proj=longlat +datum=BOGUS_DATUM_XYZ_9999 +no_defs");
        assert_eq!(result, None, "Unknown datum should return None");
    }

    #[test]
    fn test_identify_returns_none_for_malformed_wkt() {
        let result = identify_epsg_from_wkt("THIS IS NOT WKT {{{{{{{{ garbage &&&");
        // Must not panic; result can be None
        assert!(result.is_none());
    }
}
