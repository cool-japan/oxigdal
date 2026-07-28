//! Integration tests for the pure-Rust GeoPackage writer (W4).

#![allow(clippy::expect_used, clippy::panic)]

use oxigeo_gpkg::{GeoPackage, GeoPackageBuilder};

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
                    oxigeo_gpkg::CellValue::Integer(v) => Some(*v),
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

    let extract_float = |v: &oxigeo_gpkg::CellValue| -> Option<f64> {
        match v {
            oxigeo_gpkg::CellValue::Float(f) => Some(*f),
            oxigeo_gpkg::CellValue::Integer(i) => Some(*i as f64),
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
    let e = oxigeo_gpkg::GpkgError::RowOverflowsPage {
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
        oxigeo_gpkg::CellValue::Text(s) => s.clone(),
        other => panic!("unexpected table_name: {other:?}"),
    };
    assert_eq!(tbl, "pts");

    // col 1: column_name
    let col_name = match &cols[1] {
        oxigeo_gpkg::CellValue::Text(s) => s.clone(),
        other => panic!("unexpected column_name: {other:?}"),
    };
    assert_eq!(col_name, "geom");

    // col 2: geometry_type_name
    let geom_type = match &cols[2] {
        oxigeo_gpkg::CellValue::Text(s) => s.clone(),
        other => panic!("unexpected geometry_type: {other:?}"),
    };
    assert_eq!(geom_type, "POINT");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: custom SRS registration and validation (regression for a real defect:
// GeoPackageBuilder::new(3857) used to silently produce a file whose
// gpkg_contents/gpkg_geometry_columns srs_id referenced no
// gpkg_spatial_ref_sys row at all).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_writer_non_default_srs_id_without_registration_is_rejected() {
    let err = GeoPackageBuilder::new(3857)
        .add_feature_table("pts", "POINT", vec![(1, 1.0, 2.0)])
        .build()
        .expect_err("build must reject an unregistered non-default srs_id");

    match err {
        oxigeo_gpkg::GpkgError::UnknownSrsId(id) => assert_eq!(id, 3857),
        other => panic!("expected UnknownSrsId, got {other:?}"),
    }
}

#[test]
fn test_writer_custom_srs_registered_and_written() {
    use oxigeo_gpkg::CustomSrs;

    let mut builder = GeoPackageBuilder::new(3857);
    builder
        .add_custom_srs(CustomSrs::epsg(
            3857,
            "WGS 84 / Pseudo-Mercator",
            "PROJCS[\"WGS 84 / Pseudo-Mercator\"]",
        ))
        .expect("add_custom_srs");
    builder.add_feature_table_mut("pts", "POINT", vec![(1, 100.0, 200.0)]);

    let bytes = builder.build().expect("build with registered custom SRS");
    let gpkg = GeoPackage::from_bytes(bytes).expect("parse");

    let rows = gpkg
        .scan_table_by_name("gpkg_spatial_ref_sys")
        .expect("scan")
        .expect("gpkg_spatial_ref_sys not found");

    // 3 mandatory rows + 1 custom row.
    assert_eq!(rows.len(), 4, "expected 3 default + 1 custom SRS row");

    let srs_ids: Vec<i64> = rows
        .iter()
        .filter_map(|(_rowid, cols)| match &cols[1] {
            oxigeo_gpkg::CellValue::Integer(v) => Some(*v),
            _ => None,
        })
        .collect();
    assert!(srs_ids.contains(&3857), "custom srs_id 3857 missing");
}

#[test]
fn test_writer_add_custom_srs_rejects_mandatory_id_collision() {
    use oxigeo_gpkg::CustomSrs;

    let mut builder = GeoPackageBuilder::new(4326);
    match builder.add_custom_srs(CustomSrs::epsg(4326, "dup", "def")) {
        Err(oxigeo_gpkg::GpkgError::DuplicateSrsId(id)) => assert_eq!(id, 4326),
        Err(other) => panic!("expected DuplicateSrsId, got {other:?}"),
        Ok(_) => panic!("must reject collision with mandatory srs_id 4326"),
    }
}

#[test]
fn test_writer_add_custom_srs_rejects_duplicate_custom_id() {
    use oxigeo_gpkg::CustomSrs;

    let mut builder = GeoPackageBuilder::new(3857);
    builder
        .add_custom_srs(CustomSrs::epsg(3857, "first", "def1"))
        .expect("first registration succeeds");

    match builder.add_custom_srs(CustomSrs::epsg(3857, "second", "def2")) {
        Err(oxigeo_gpkg::GpkgError::DuplicateSrsId(id)) => assert_eq!(id, 3857),
        Err(other) => panic!("expected DuplicateSrsId, got {other:?}"),
        Ok(_) => panic!("must reject duplicate custom srs_id"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: sqlite_master catalog overflow is a real, propagated error — not a
// silently-dropped row (regression for a real defect: `emit_master_page` used
// to only check via `debug_assert!`, which compiles out in release builds).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_writer_sqlite_master_overflow_returns_error_not_silent_success() {
    let mut builder = GeoPackageBuilder::new(4326);
    // Each feature table contributes one sqlite_master row whose DDL embeds
    // the table name twice (CREATE TABLE {name} ... ) plus fixed boilerplate.
    // Enough short-named tables will overflow the single 4096-byte page 1
    // that also holds the 3 mandatory system-table rows.
    for i in 0..200 {
        builder.add_feature_table_mut(format!("t{i}"), "POINT", vec![(1, 0.0, 0.0)]);
    }

    let result = builder.build();
    assert!(
        result.is_err(),
        "200 feature tables must overflow sqlite_master's single leaf page and \
         return an error rather than silently dropping catalog rows"
    );
    match result {
        Err(oxigeo_gpkg::GpkgError::RowOverflowsPage { .. }) => {}
        other => panic!("expected RowOverflowsPage, got {other:?}"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test: R-tree spatial index writer (regression for a real defect: the
// writer never emitted an R-tree, even though a reader existed to consume
// one). Full round trip: build with add_rtree_index -> parse the resulting
// bytes -> GpkgRTreeReader::open -> bbox query returns the right rowids.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_writer_rtree_index_end_to_end_round_trip() {
    use oxigeo_gpkg::GpkgRTreeReader;

    let mut builder = GeoPackageBuilder::new(4326);
    builder.add_feature_table_mut(
        "pts",
        "POINT",
        vec![(1, 0.0, 0.0), (2, 10.0, 10.0), (3, -5.0, -5.0)],
    );
    builder.add_rtree_index("pts").expect("add_rtree_index");

    let bytes = builder.build().expect("build with rtree index");
    let gpkg = GeoPackage::from_bytes(bytes).expect("parse");

    // 1. The shadow tables and virtual table must appear in sqlite_master.
    let master = gpkg.scan_sqlite_master().expect("scan sqlite_master");
    let names: Vec<&str> = master.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"rtree_pts_geom"), "virtual table missing");
    assert!(
        names.contains(&"rtree_pts_geom_node"),
        "_node table missing"
    );
    assert!(
        names.contains(&"rtree_pts_geom_rowid"),
        "_rowid table missing"
    );
    assert!(
        names.contains(&"rtree_pts_geom_parent"),
        "_parent table missing"
    );
    assert!(
        names.contains(&"gpkg_extensions"),
        "gpkg_extensions table missing"
    );

    // 2. gpkg_extensions must register the gpkg_rtree_index extension.
    let ext_rows = gpkg
        .scan_table_by_name("gpkg_extensions")
        .expect("scan")
        .expect("gpkg_extensions must exist");
    assert_eq!(ext_rows.len(), 1);
    let (_rowid, cols) = &ext_rows[0];
    match &cols[2] {
        oxigeo_gpkg::CellValue::Text(s) => assert_eq!(s, "gpkg_rtree_index"),
        other => panic!("unexpected extension_name: {other:?}"),
    }

    // 3. The R-tree must actually be queryable and return the right rowids.
    let reader = GpkgRTreeReader::open(&gpkg, "pts", "geom").expect("open rtree reader");
    assert_eq!(reader.len(), 1, "single-node tree: exactly 1 node total");
    assert_eq!(
        reader.all_entries().len(),
        3,
        "all 3 points must be indexed as leaf entries"
    );

    let hits = reader.search(-1.0, -1.0, 1.0, 1.0);
    assert_eq!(
        hits,
        vec![1],
        "bbox around origin should only match rowid 1"
    );

    let hits_all = reader.search(-100.0, -100.0, 100.0, 100.0);
    let mut sorted_hits = hits_all.clone();
    sorted_hits.sort_unstable();
    assert_eq!(
        sorted_hits,
        vec![1, 2, 3],
        "a bbox covering everything must match all 3 rowids"
    );
}

#[test]
fn test_writer_rtree_index_rejects_unknown_table() {
    let mut builder = GeoPackageBuilder::new(4326);
    let err = builder
        .add_rtree_index("ghost")
        .err()
        .expect("must reject rtree index for unregistered table");
    match err {
        oxigeo_gpkg::GpkgError::TableNotFound(name) => assert_eq!(name, "ghost"),
        other => panic!("expected TableNotFound, got {other:?}"),
    }
}

#[test]
fn test_writer_without_rtree_index_has_no_extensions_table() {
    // Default (opt-out) behavior must be unchanged: no rtree requested means
    // no gpkg_extensions / rtree_* tables at all.
    let bytes = GeoPackageBuilder::new(4326)
        .add_feature_table("pts", "POINT", vec![(1, 0.0, 0.0)])
        .build()
        .expect("build");
    let gpkg = GeoPackage::from_bytes(bytes).expect("parse");
    let master = gpkg.scan_sqlite_master().expect("scan sqlite_master");
    let names: Vec<&str> = master.iter().map(|e| e.name.as_str()).collect();
    assert!(
        !names.iter().any(|n| n.starts_with("rtree_")),
        "no rtree_* tables expected without add_rtree_index, got: {names:?}"
    );
    assert!(
        !names.contains(&"gpkg_extensions"),
        "no gpkg_extensions table expected without any extension in use"
    );
}
