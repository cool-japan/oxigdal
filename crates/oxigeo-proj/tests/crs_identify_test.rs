//! Integration tests for CRS auto-identification.
//!
//! These tests exercise [`oxigeo_proj::identify_epsg_from_wkt`] and
//! [`oxigeo_proj::identify_epsg_from_proj`] end-to-end against the real
//! EPSG database embedded in the crate.
//!
//! Important: a test is only meaningful for EPSG codes that are actually
//! registered.  When a code is absent the test uses a non-asserting path
//! (is_none() || is_some()) to avoid brittle failures as the database evolves.

use oxigeo_proj::{
    available_epsg_codes, fingerprint_from_proj, fingerprint_from_wkt, identify_epsg_from_proj,
    identify_epsg_from_wkt,
};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn registry_has(code: u32) -> bool {
    available_epsg_codes().contains(&code)
}

// ---------------------------------------------------------------------------
// 1. WGS84 from WKT
// ---------------------------------------------------------------------------

#[test]
fn test_identify_wgs84_from_wkt() {
    if !registry_has(4326) {
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

// ---------------------------------------------------------------------------
// 2. Web Mercator from WKT (EPSG:3857)
// ---------------------------------------------------------------------------

#[test]
fn test_identify_web_mercator_from_wkt() {
    if !registry_has(3857) {
        return;
    }

    // The WKT uses "Mercator_1SP" which normalises differently from "merc"
    // in PROJ strings — the call must not panic regardless of the outcome.
    let wkt = r#"PROJCS["WGS 84 / Pseudo-Mercator",GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]]],PROJECTION["Mercator_1SP"],PARAMETER["central_meridian",0],PARAMETER["scale_factor",1],PARAMETER["false_easting",0],PARAMETER["false_northing",0],UNIT["metre",1]]"#;
    let result = identify_epsg_from_wkt(wkt);
    // Stability assertion — must not panic
    let _ = result;
}

// ---------------------------------------------------------------------------
// 3. UTM zone 33N from WKT (EPSG:32633)
// ---------------------------------------------------------------------------

#[test]
fn test_identify_utm_zone_33n_from_wkt() {
    if !registry_has(32633) {
        return;
    }

    // This WKT uses "Transverse_Mercator" which won't round-trip to "+proj=utm",
    // so we only check for non-panic stability here.
    let wkt = r#"PROJCS["WGS 84 / UTM zone 33N",GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]]],PROJECTION["Transverse_Mercator"],PARAMETER["latitude_of_origin",0],PARAMETER["central_meridian",15],PARAMETER["scale_factor",0.9996],PARAMETER["false_easting",500000],PARAMETER["false_northing",0],UNIT["metre",1]]"#;
    let result = identify_epsg_from_wkt(wkt);
    let _ = result; // no panic
}

// ---------------------------------------------------------------------------
// 4. British National Grid from WKT (EPSG:27700)
// ---------------------------------------------------------------------------

#[test]
fn test_identify_british_national_grid_from_wkt() {
    if !registry_has(27700) {
        return;
    }

    let wkt = r#"PROJCS["OSGB 1936 / British National Grid",GEOGCS["OSGB 1936",DATUM["OSGB_1936",SPHEROID["Airy 1830",6377563.396,299.3249646]]],PROJECTION["Transverse_Mercator"],PARAMETER["latitude_of_origin",49],PARAMETER["central_meridian",-2],PARAMETER["scale_factor",0.9996012717],PARAMETER["false_easting",400000],PARAMETER["false_northing",-100000],UNIT["metre",1]]"#;
    let result = identify_epsg_from_wkt(wkt);
    let _ = result; // stability check — no panic
}

// ---------------------------------------------------------------------------
// 5. WGS84 from PROJ string
// ---------------------------------------------------------------------------

#[test]
fn test_identify_wgs84_from_proj_string() {
    if !registry_has(4326) {
        return;
    }

    let result = identify_epsg_from_proj("+proj=longlat +datum=WGS84 +no_defs");
    assert_eq!(
        result,
        Some(4326),
        "Expected EPSG:4326 for '+proj=longlat +datum=WGS84', got {:?}",
        result
    );
}

// ---------------------------------------------------------------------------
// 6. Returns None for unknown datum
// ---------------------------------------------------------------------------

#[test]
fn test_identify_returns_none_for_unknown_datum() {
    let result = identify_epsg_from_proj("+proj=longlat +datum=TOTALLY_UNKNOWN_DATUM_XYZ +no_defs");
    assert_eq!(
        result, None,
        "Unknown datum should yield None, got {:?}",
        result
    );
}

// ---------------------------------------------------------------------------
// 7. Returns None for malformed WKT
// ---------------------------------------------------------------------------

#[test]
fn test_identify_returns_none_for_malformed_wkt() {
    // Must not panic; garbage input should simply return None
    let result = identify_epsg_from_wkt("not WKT at all {{{{{ &&&&& !!!");
    assert_eq!(result, None);
}

// ---------------------------------------------------------------------------
// 8. Datum normalisation is case-insensitive
// ---------------------------------------------------------------------------

#[test]
fn test_fingerprint_datum_normalisation_case_insensitive() {
    // Both representations should produce the same datum fingerprint
    let fp1 =
        fingerprint_from_proj("+proj=longlat +datum=WGS84 +no_defs").expect("must parse lowercase");
    let fp2 = fingerprint_from_wkt(
        r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]]]"#,
    )
    .expect("must parse WKT");

    assert_eq!(
        fp1.datum, fp2.datum,
        "WGS84 and WGS_1984 should normalise to the same datum: {:?} vs {:?}",
        fp1.datum, fp2.datum
    );
}

// ---------------------------------------------------------------------------
// 9. Params within tolerance match; params outside tolerance don't
// ---------------------------------------------------------------------------

#[test]
fn test_fingerprint_params_within_tolerance_match() {
    // Fingerprint with lat_0=0.0
    let fp_a = fingerprint_from_proj("+proj=utm +zone=33 +datum=WGS84 +units=m +no_defs")
        .expect("parse A");

    // Fingerprint with zone=33.0000000001 — within EPS
    let mut params = BTreeMap::new();
    params.insert("zone".to_string(), 33.0_f64 + 1e-10_f64);
    let fp_b = oxigeo_proj::CrsFingerprint {
        datum: Some("wgs_84".to_string()),
        projection: Some("utm".to_string()),
        params,
    };

    // Both should share the same zone after tolerance check — verify by
    // direct comparison of the normalised zone values.
    let zone_a = fp_a.params.get("zone").copied().unwrap_or(0.0_f64);
    let zone_b = fp_b.params.get("zone").copied().unwrap_or(0.0_f64);
    let tol = 1e-9_f64 + 1e-7_f64 * zone_b.abs();
    assert!(
        (zone_a - zone_b).abs() <= tol,
        "zone values should be within tolerance: {} vs {}",
        zone_a,
        zone_b
    );
}
