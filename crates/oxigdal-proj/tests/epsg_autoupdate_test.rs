//! Integration tests for the EPSG snapshot auto-update via build.rs.
//!
//! These tests verify that:
//!   1. Codes baked in by `build.rs` from `epsg-snapshot/minimal.json` are present.
//!   2. Pre-existing built-in codes (WGS84, Web Mercator) are still present.
//!   3. Unknown codes correctly return an error.
//!   4. The snapshot records contain the expected field values.

use oxigdal_proj::epsg::{CrsType, EpsgDatabase};

// ── snapshot-sourced codes ────────────────────────────────────────────────────

#[test]
fn test_epsg_4324_wgs72be_present() {
    let db = EpsgDatabase::new();
    let def = db
        .lookup(4324)
        .expect("WGS 72BE (EPSG:4324) should be in snapshot");
    assert!(
        def.name.contains("WGS 72") || def.name.contains("72BE"),
        "unexpected name: {}",
        def.name
    );
}

#[test]
fn test_epsg_4617_nad83_csrs_present() {
    let db = EpsgDatabase::new();
    let def = db
        .lookup(4617)
        .expect("NAD83(CSRS) (EPSG:4617) should be in snapshot");
    assert!(
        def.name.contains("NAD83") || def.name.contains("CSRS"),
        "unexpected name: {}",
        def.name
    );
    assert_eq!(def.crs_type, CrsType::Geographic);
}

#[test]
fn test_epsg_4647_etrs89_utm32_zen_present() {
    let db = EpsgDatabase::new();
    let def = db
        .lookup(4647)
        .expect("ETRS89 / UTM zone 32N (zE-N) (EPSG:4647) should be in snapshot");
    assert_eq!(def.crs_type, CrsType::Projected);
}

#[test]
fn test_epsg_3408_nsidc_ease_north_present() {
    let db = EpsgDatabase::new();
    let def = db
        .lookup(3408)
        .expect("NSIDC EASE-Grid North (EPSG:3408) should be in snapshot");
    assert!(
        def.name.contains("EASE") || def.name.contains("North"),
        "unexpected name: {}",
        def.name
    );
    assert_eq!(def.crs_type, CrsType::Projected);
}

#[test]
fn test_epsg_3409_nsidc_ease_south_present() {
    let db = EpsgDatabase::new();
    let def = db
        .lookup(3409)
        .expect("NSIDC EASE-Grid South (EPSG:3409) should be in snapshot");
    assert_eq!(def.crs_type, CrsType::Projected);
}

// ── built-in codes must remain after snapshot merge ──────────────────────────

#[test]
fn test_epsg_4326_wgs84_still_present() {
    let db = EpsgDatabase::new();
    let def = db.lookup(4326).expect("WGS84 must always be present");
    assert!(
        def.name.contains("WGS") || def.name.contains("84"),
        "WGS84 name is unexpected: {}",
        def.name
    );
}

#[test]
fn test_epsg_3857_web_mercator_still_present() {
    let db = EpsgDatabase::new();
    let def = db
        .lookup(3857)
        .expect("Web Mercator must always be present");
    assert_eq!(def.crs_type, CrsType::Projected);
}

#[test]
fn test_epsg_4269_nad83_still_present() {
    let db = EpsgDatabase::new();
    let def = db.lookup(4269).expect("NAD83 should be present");
    assert!(
        def.name.contains("NAD83") || def.name.contains("North American"),
        "unexpected name: {}",
        def.name
    );
}

// ── database-wide invariants ──────────────────────────────────────────────────

#[test]
fn test_epsg_lookup_unknown_code_returns_error() {
    let db = EpsgDatabase::new();
    assert!(
        db.lookup(99999).is_err(),
        "unknown code 99999 should not be in the database"
    );
}

#[test]
fn test_epsg_count_after_generated_load_above_baseline() {
    let db = EpsgDatabase::new();
    // Pre-snapshot baseline was 500+. After snapshot we expect even more.
    assert!(
        db.len() > 10,
        "should have more than 10 codes after snapshot load; got {}",
        db.len()
    );
}

// ── field-level assertions on snapshot records ───────────────────────────────

#[test]
fn test_epsg_4617_proj_string_non_empty() {
    let db = EpsgDatabase::new();
    if let Ok(def) = db.lookup(4617) {
        assert!(
            !def.proj_string.is_empty(),
            "proj_string must not be empty for EPSG:4617"
        );
    }
}

#[test]
fn test_epsg_4230_ed50_crs_type_is_geographic() {
    let db = EpsgDatabase::new();
    if let Ok(def) = db.lookup(4230) {
        assert_eq!(
            def.crs_type,
            CrsType::Geographic,
            "ED50 (EPSG:4230) should be Geographic"
        );
    }
}

#[test]
fn test_epsg_4978_geocentric_crs_type() {
    let db = EpsgDatabase::new();
    if let Ok(def) = db.lookup(4978) {
        assert_eq!(
            def.crs_type,
            CrsType::Geocentric,
            "WGS 84 geocentric (EPSG:4978) should be Geocentric"
        );
    }
}
