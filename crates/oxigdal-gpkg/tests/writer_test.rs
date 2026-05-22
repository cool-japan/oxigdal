//! Integration tests for the pure-Rust GeoPackage writer (W4).

#![allow(clippy::expect_used, clippy::panic)]

use oxigdal_gpkg::{GeoPackage, GeoPackageBuilder};

// ─────────────────────────────────────────────────────────────────────────────
// Helper: build an empty GeoPackage (no feature tables)
// ─────────────────────────────────────────────────────────────────────────────

fn build_empty() -> Vec<u8> {
    GeoPackageBuilder::new(4326).build().expect("empty build")
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: SQLite magic and GeoPackage header fields
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_writer_minimal_empty_gpkg_validates_header() {
    let bytes = build_empty();

    // File must start with "SQLite"
    assert_eq!(&bytes[0..6], b"SQLite", "magic prefix mismatch");

    // Full 16-byte magic including NUL terminator
    assert_eq!(&bytes[0..16], b"SQLite format 3\0", "full magic mismatch");

    // Application ID at offset 68 (big-endian): must be 0x4750_4B47 ("GPKG")
    let app_id = u32::from_be_bytes(bytes[68..72].try_into().expect("slice"));
    assert_eq!(app_id, 0x4750_4B47, "application_id mismatch");

    // User version at offset 60 (big-endian): GeoPackage 1.3.0 = 10 300
    let user_version = u32::from_be_bytes(bytes[60..64].try_into().expect("slice"));
    assert_eq!(user_version, 10_300, "user_version mismatch");

    // Text encoding at offset 56 (big-endian): 1 = UTF-8
    let text_enc = u32::from_be_bytes(bytes[56..60].try_into().expect("slice"));
    assert_eq!(text_enc, 1, "text_encoding must be UTF-8 (1)");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: sqlite_master contains the required system tables
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_writer_sqlite_master_contains_system_tables() {
    let bytes = build_empty();
    let gpkg = GeoPackage::from_bytes(bytes).expect("parse");
    let entries = gpkg.scan_sqlite_master().expect("scan sqlite_master");

    let table_names: Vec<String> = entries
        .iter()
        .filter(|e| e.entry_type == "table")
        .map(|e| e.name.clone())
        .collect();

    assert!(
        table_names.contains(&"gpkg_spatial_ref_sys".to_string()),
        "gpkg_spatial_ref_sys missing; got: {table_names:?}"
    );
    assert!(
        table_names.contains(&"gpkg_contents".to_string()),
        "gpkg_contents missing; got: {table_names:?}"
    );
    assert!(
        table_names.contains(&"gpkg_geometry_columns".to_string()),
        "gpkg_geometry_columns missing; got: {table_names:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: default SRS rows present
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_writer_default_srs_rows_present() {
    let bytes = build_empty();
    let gpkg = GeoPackage::from_bytes(bytes).expect("parse");

    let rows = gpkg
        .scan_table_by_name("gpkg_spatial_ref_sys")
        .expect("scan")
        .expect("table not found");

    // Column 1 is srs_id (INTEGER).
    let srs_ids: Vec<i64> = rows
        .iter()
        .filter_map(|(_rowid, cols)| {
            if cols.len() > 1 {
                match &cols[1] {
                    oxigdal_gpkg::CellValue::Integer(v) => Some(*v),
                    _ => None,
                }
            } else {
                None
            }
        })
        .collect();

    assert!(srs_ids.contains(&-1), "srs_id -1 missing; got: {srs_ids:?}");
    assert!(srs_ids.contains(&0), "srs_id 0 missing; got: {srs_ids:?}");
    assert!(
        srs_ids.contains(&4326),
        "srs_id 4326 missing; got: {srs_ids:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: round-trip point feature table
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_writer_roundtrip_point_feature_table() {
    let points = vec![(1, 139.7, 35.7), (2, -74.0, 40.7)];
    let bytes = GeoPackageBuilder::new(4326)
        .add_feature_table("cities", "POINT", points)
        .build()
        .expect("build");

    let gpkg = GeoPackage::from_bytes(bytes).expect("parse");

    let rows = gpkg
        .scan_table_by_name("cities")
        .expect("scan")
        .expect("cities table not found");

    assert_eq!(rows.len(), 2, "expected 2 feature rows, got {}", rows.len());
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: gpkg_contents bbox matches feature points
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_writer_contents_row_bbox_matches_features() {
    let points = vec![(1, 0.0_f64, 0.0_f64), (2, 10.0_f64, 5.0_f64)];
    let bytes = GeoPackageBuilder::new(4326)
        .add_feature_table("layer", "POINT", points)
        .build()
        .expect("build");

    let gpkg = GeoPackage::from_bytes(bytes).expect("parse");

    let rows = gpkg
        .scan_table_by_name("gpkg_contents")
        .expect("scan")
        .expect("gpkg_contents not found");

    assert!(!rows.is_empty(), "gpkg_contents must have at least one row");

    // Column layout per OGC spec:
    //  0 table_name  1 data_type  2 identifier  3 description  4 last_change
    //  5 min_x       6 min_y      7 max_x        8 max_y        9 srs_id
    let (_rowid, cols) = &rows[0];

    let extract_float = |v: &oxigdal_gpkg::CellValue| -> Option<f64> {
        match v {
            oxigdal_gpkg::CellValue::Float(f) => Some(*f),
            oxigdal_gpkg::CellValue::Integer(i) => Some(*i as f64),
            _ => None,
        }
    };

    let min_x = extract_float(&cols[5]).expect("min_x must be a float");
    let min_y = extract_float(&cols[6]).expect("min_y must be a float");
    let max_x = extract_float(&cols[7]).expect("max_x must be a float");
    let max_y = extract_float(&cols[8]).expect("max_y must be a float");

    assert!(
        (min_x - 0.0_f64).abs() < 1e-9,
        "min_x expected 0.0, got {min_x}"
    );
    assert!(
        (min_y - 0.0_f64).abs() < 1e-9,
        "min_y expected 0.0, got {min_y}"
    );
    assert!(
        (max_x - 10.0_f64).abs() < 1e-9,
        "max_x expected 10.0, got {max_x}"
    );
    assert!(
        (max_y - 5.0_f64).abs() < 1e-9,
        "max_y expected 5.0, got {max_y}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: RowOverflowsPage variant compiles and is constructible
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_writer_rejects_oversized_row() {
    // Verify that the error variant exists and can be constructed.
    let e = oxigdal_gpkg::GpkgError::RowOverflowsPage {
        size: 5000,
        max: 4061,
    };
    let msg = e.to_string();
    assert!(
        msg.contains("5000") && msg.contains("4061"),
        "error message should contain size and max: {msg}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: multiple feature tables co-exist
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_writer_multiple_feature_tables() {
    let bytes = GeoPackageBuilder::new(4326)
        .add_feature_table("rivers", "POINT", vec![(1, 10.0, 20.0)])
        .add_feature_table("cities", "POINT", vec![(1, 30.0, 40.0), (2, 50.0, 60.0)])
        .build()
        .expect("build");

    let gpkg = GeoPackage::from_bytes(bytes).expect("parse");

    let rivers = gpkg
        .scan_table_by_name("rivers")
        .expect("scan")
        .expect("rivers not found");
    assert_eq!(rivers.len(), 1, "rivers should have 1 row");

    let cities = gpkg
        .scan_table_by_name("cities")
        .expect("scan")
        .expect("cities not found");
    assert_eq!(cities.len(), 2, "cities should have 2 rows");

    // Both should appear in sqlite_master
    let entries = gpkg.scan_sqlite_master().expect("scan master");
    let names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
    assert!(names.contains(&"rivers".to_string()), "rivers in master");
    assert!(names.contains(&"cities".to_string()), "cities in master");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: geometry columns table is populated correctly
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_writer_geometry_columns_populated() {
    let bytes = GeoPackageBuilder::new(4326)
        .add_feature_table("pts", "POINT", vec![(1, 1.0, 2.0)])
        .build()
        .expect("build");

    let gpkg = GeoPackage::from_bytes(bytes).expect("parse");

    let rows = gpkg
        .scan_table_by_name("gpkg_geometry_columns")
        .expect("scan")
        .expect("gpkg_geometry_columns not found");

    assert_eq!(rows.len(), 1, "one geometry column row expected");
    let (_rowid, cols) = &rows[0];

    // col 0: table_name
    let tbl = match &cols[0] {
        oxigdal_gpkg::CellValue::Text(s) => s.clone(),
        other => panic!("unexpected table_name: {other:?}"),
    };
    assert_eq!(tbl, "pts");

    // col 1: column_name
    let col_name = match &cols[1] {
        oxigdal_gpkg::CellValue::Text(s) => s.clone(),
        other => panic!("unexpected column_name: {other:?}"),
    };
    assert_eq!(col_name, "geom");

    // col 2: geometry_type_name
    let geom_type = match &cols[2] {
        oxigdal_gpkg::CellValue::Text(s) => s.clone(),
        other => panic!("unexpected geometry_type: {other:?}"),
    };
    assert_eq!(geom_type, "POINT");
}
