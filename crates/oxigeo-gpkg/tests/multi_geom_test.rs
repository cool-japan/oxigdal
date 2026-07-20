//! Integration tests for multiple-geometry-column support (W4).
//!
//! Uses `GeoPackageBuilder` to produce in-memory GeoPackage fixtures, then
//! verifies the `multi_geom` API against those fixtures.

#![allow(clippy::expect_used, clippy::panic)]

use oxigeo_gpkg::{
    GeoPackage, GeoPackageBuilder, GeometryColumnDef, MultiGeomColumnSet,
    has_multiple_geometry_columns, load_all_geometry_columns, load_geometry_columns_for_table,
};

// ─────────────────────────────────────────────────────────────────────────────
// Test helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Build a GeoPackage with a single feature table `"pts"` (POINT, srs 4326).
fn build_single_table() -> Vec<u8> {
    GeoPackageBuilder::new(4326)
        .add_feature_table("pts", "POINT", vec![(1, 139.7, 35.7)])
        .build()
        .expect("build single-table gpkg")
}

/// Build a GeoPackage with two feature tables: `"rivers"` and `"cities"`.
fn build_two_tables() -> Vec<u8> {
    GeoPackageBuilder::new(4326)
        .add_feature_table("rivers", "LINESTRING", vec![(1, 0.0, 0.0)])
        .add_feature_table("cities", "POINT", vec![(1, 10.0, 10.0)])
        .build()
        .expect("build two-table gpkg")
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. In-memory construction: find_by_name
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_geometry_column_def_find_by_name_returns_correct() {
    let col_a = GeometryColumnDef::from_raw("layer", "geom", "POINT", 4326, 0, 0);
    let col_b = GeometryColumnDef::from_raw("layer", "geom2", "POLYGON", 4326, 1, 0);

    let set = MultiGeomColumnSet {
        table_name: "layer".to_string(),
        columns: vec![col_a, col_b],
    };

    let found = set.find_by_name("geom2").expect("should find geom2");
    assert_eq!(found.column_name, "geom2");
    assert_eq!(found.geometry_type_name, "POLYGON");
    assert!(found.has_z, "z=1 means has_z=true");
    assert!(!found.has_m);

    assert!(
        set.find_by_name("nonexistent").is_none(),
        "unknown column should return None"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. In-memory construction: primary() returns first
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_geometry_column_def_primary_returns_first() {
    let col_a = GeometryColumnDef::from_raw("tbl", "geom", "POINT", 4326, 0, 0);
    let col_b = GeometryColumnDef::from_raw("tbl", "outline", "POLYGON", 4326, 0, 0);

    let set = MultiGeomColumnSet {
        table_name: "tbl".to_string(),
        columns: vec![col_a.clone(), col_b],
    };

    let primary = set.primary().expect("primary must exist");
    assert_eq!(
        primary.column_name, "geom",
        "primary should be the first element"
    );
    assert_eq!(primary.geometry_type_name, "POINT");

    let empty = MultiGeomColumnSet::new("empty_tbl");
    assert!(empty.primary().is_none(), "empty set has no primary");
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Round-trip: single column → MultiGeomColumnSet with 1 entry
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_load_geometry_columns_for_table_single_column() {
    let bytes = build_single_table();
    let gpkg = GeoPackage::from_bytes(bytes).expect("parse");

    let result = load_geometry_columns_for_table(&gpkg, "pts").expect("scan should not error");

    let set = result.expect("pts should have geometry columns");
    assert_eq!(set.table_name, "pts");
    assert_eq!(
        set.column_count(),
        1,
        "single feature table → exactly one geometry column"
    );

    let primary = set.primary().expect("primary must be present");
    assert_eq!(primary.table_name, "pts");
    assert!(!set.has_multiple());
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Unknown table returns Ok(None)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_load_geometry_columns_for_table_returns_none_for_unknown_table() {
    let bytes = build_single_table();
    let gpkg = GeoPackage::from_bytes(bytes).expect("parse");

    let result =
        load_geometry_columns_for_table(&gpkg, "nonexistent_table").expect("scan should not error");

    assert!(
        result.is_none(),
        "unknown table should return Ok(None), got: {result:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Two feature tables → two MultiGeomColumnSets
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_load_all_geometry_columns_returns_all_tables() {
    let bytes = build_two_tables();
    let gpkg = GeoPackage::from_bytes(bytes).expect("parse");

    let all = load_all_geometry_columns(&gpkg).expect("scan");
    assert_eq!(
        all.len(),
        2,
        "two feature tables should produce two column sets; got {all:?}"
    );

    let table_names: Vec<&str> = all.iter().map(|s| s.table_name.as_str()).collect();
    assert!(
        table_names.contains(&"rivers"),
        "rivers missing from results: {table_names:?}"
    );
    assert!(
        table_names.contains(&"cities"),
        "cities missing from results: {table_names:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. Single-column table → has_multiple returns false
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_has_multiple_geometry_columns_single_returns_false() {
    let bytes = build_single_table();
    let gpkg = GeoPackage::from_bytes(bytes).expect("parse");

    let multi = has_multiple_geometry_columns(&gpkg, "pts").expect("scan");
    assert!(!multi, "a table with one geometry column is not multi-geom");
}

// ─────────────────────────────────────────────────────────────────────────────
// 7. has_z decoded from z flag
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_geometry_column_def_has_z_from_z_flag() {
    // z = 0 → has_z = false
    let c0 = GeometryColumnDef::from_raw("t", "g", "POINTZ", 4326, 0, 0);
    assert!(!c0.has_z, "z=0 must yield has_z=false");

    // z = 1 → has_z = true (mandatory)
    let c1 = GeometryColumnDef::from_raw("t", "g", "POINTZ", 4326, 1, 0);
    assert!(c1.has_z, "z=1 must yield has_z=true");

    // z = 2 → has_z = true (optional)
    let c2 = GeometryColumnDef::from_raw("t", "g", "POINTZ", 4326, 2, 0);
    assert!(c2.has_z, "z=2 must yield has_z=true");

    // Round-trip: z_flag() restores a compatible flag value (non-zero).
    assert_ne!(c1.z_flag(), 0, "has_z=true round-trips to non-zero z_flag");
    assert_eq!(c0.z_flag(), 0, "has_z=false round-trips to z_flag=0");
}

// ─────────────────────────────────────────────────────────────────────────────
// 8. has_m decoded from m flag
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_geometry_column_def_has_m_from_m_flag() {
    // m = 0 → has_m = false
    let c0 = GeometryColumnDef::from_raw("t", "g", "POINTM", 4326, 0, 0);
    assert!(!c0.has_m, "m=0 must yield has_m=false");

    // m = 1 → has_m = true
    let c1 = GeometryColumnDef::from_raw("t", "g", "POINTM", 4326, 0, 1);
    assert!(c1.has_m, "m=1 must yield has_m=true");

    // m = 2 → has_m = true
    let c2 = GeometryColumnDef::from_raw("t", "g", "POINTM", 4326, 0, 2);
    assert!(c2.has_m, "m=2 must yield has_m=true");

    // Round-trip via m_flag().
    assert_ne!(c1.m_flag(), 0, "has_m=true round-trips to non-zero m_flag");
    assert_eq!(c0.m_flag(), 0, "has_m=false round-trips to m_flag=0");
}

// ─────────────────────────────────────────────────────────────────────────────
// 9. column_name field matches what the builder writes
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_load_geometry_columns_column_name_matches_table() {
    let bytes = build_single_table();
    let gpkg = GeoPackage::from_bytes(bytes).expect("parse");

    let set = load_geometry_columns_for_table(&gpkg, "pts")
        .expect("scan")
        .expect("pts must have geometry columns");

    let primary = set.primary().expect("primary");
    // The builder always writes the column name "geom" for the primary column.
    assert_eq!(
        primary.column_name, "geom",
        "primary geometry column name must be 'geom'"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 10. srs_id matches what the builder wrote
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_load_geometry_columns_srs_id_matches_builder() {
    // Build with srs_id = 4326.
    let bytes = GeoPackageBuilder::new(4326)
        .add_feature_table("layer", "POINT", vec![(1, 0.0, 0.0)])
        .build()
        .expect("build");
    let gpkg = GeoPackage::from_bytes(bytes).expect("parse");

    let set = load_geometry_columns_for_table(&gpkg, "layer")
        .expect("scan")
        .expect("layer must have geometry columns");

    let primary = set.primary().expect("primary");
    assert_eq!(
        primary.srs_id, 4326,
        "srs_id must match the value passed to GeoPackageBuilder::new"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 11. add_geometry_column_def writes extra row (writer extension)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_add_geometry_column_def_writes_extra_row() {
    let extra = GeometryColumnDef::from_raw("pts", "outline", "POLYGON", 4326, 0, 0);

    let mut builder = GeoPackageBuilder::new(4326);
    builder.add_feature_table_mut("pts", "POINT", vec![(1, 0.0, 0.0)]);
    builder
        .add_geometry_column_def("pts", &extra)
        .expect("add_geometry_column_def should succeed for known table");

    let bytes = builder.build_from_ref().expect("build");
    let gpkg = GeoPackage::from_bytes(bytes).expect("parse");

    let set = load_geometry_columns_for_table(&gpkg, "pts")
        .expect("scan")
        .expect("pts must have geometry columns");

    assert_eq!(
        set.column_count(),
        2,
        "pts should have 2 geometry columns (primary + extra)"
    );
    assert!(set.has_multiple(), "has_multiple() must be true");

    let outline = set
        .find_by_name("outline")
        .expect("outline column must exist");
    assert_eq!(outline.geometry_type_name, "POLYGON");
    assert_eq!(outline.srs_id, 4326);
}

// ─────────────────────────────────────────────────────────────────────────────
// 12. add_geometry_column_def rejects unknown table
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_add_geometry_column_def_rejects_unknown_table() {
    let extra = GeometryColumnDef::from_raw("ghost", "geom2", "POLYGON", 4326, 0, 0);
    let mut builder = GeoPackageBuilder::new(4326);
    // Do NOT register "ghost" as a feature table.
    let result = builder.add_geometry_column_def("ghost", &extra);
    assert!(
        result.is_err(),
        "add_geometry_column_def should fail for an unregistered table"
    );
}
