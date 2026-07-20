//! Integration tests for FlatGeoBuf export of GeoPackage feature tables.

#![cfg(feature = "flatgeobuf-export")]
#![allow(clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::io::Cursor;

use oxigeo_gpkg::flatgeobuf_export::FlatGeoBufExporter;
use oxigeo_gpkg::{FeatureRow, FeatureTable, FieldDefinition, FieldType, FieldValue, GpkgGeometry};

// ─────────────────────────────────────────────────────────────────────────────
// Builder helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Build a minimal empty feature table (no features, no schema columns).
fn empty_point_table(name: &str) -> FeatureTable {
    FeatureTable::new(name, "geom")
}

/// Build a feature table with `n` simple 2-D point features.
fn point_table_with_n(name: &str, n: usize) -> FeatureTable {
    let mut table = FeatureTable::new(name, "geom");
    for i in 0..n {
        let x = i as f64;
        let y = i as f64 * 2.0;
        let row = FeatureRow {
            fid: i as i64 + 1,
            geometry: Some(GpkgGeometry::Point { x, y }),
            fields: HashMap::new(),
        };
        table.add_feature(row);
    }
    table
}

/// Build a feature table with a linestring feature.
fn linestring_table() -> FeatureTable {
    let mut table = FeatureTable::new("linestrings", "geom");
    let row = FeatureRow {
        fid: 1,
        geometry: Some(GpkgGeometry::LineString {
            coords: vec![(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)],
        }),
        fields: HashMap::new(),
    };
    table.add_feature(row);
    table
}

/// Build a feature table with a polygon-with-hole feature.
fn polygon_with_hole_table() -> FeatureTable {
    let mut table = FeatureTable::new("polygons", "geom");
    let exterior = vec![
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 10.0),
        (0.0, 10.0),
        (0.0, 0.0),
    ];
    let hole = vec![(2.0, 2.0), (8.0, 2.0), (8.0, 8.0), (2.0, 8.0), (2.0, 2.0)];
    let row = FeatureRow {
        fid: 1,
        geometry: Some(GpkgGeometry::Polygon {
            rings: vec![exterior, hole],
        }),
        fields: HashMap::new(),
    };
    table.add_feature(row);
    table
}

/// Build a feature table with a multipolygon feature.
fn multipolygon_table() -> FeatureTable {
    let mut table = FeatureTable::new("multipolygons", "geom");
    let ring_a = vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)];
    let ring_b = vec![(5.0, 5.0), (6.0, 5.0), (6.0, 6.0), (5.0, 6.0), (5.0, 5.0)];
    let row = FeatureRow {
        fid: 1,
        geometry: Some(GpkgGeometry::MultiPolygon {
            polygons: vec![vec![ring_a], vec![ring_b]],
        }),
        fields: HashMap::new(),
    };
    table.add_feature(row);
    table
}

/// Build a feature table that has string and integer attribute columns.
fn table_with_string_and_int_columns() -> FeatureTable {
    let mut table = FeatureTable::new("attrs", "geom");
    table.schema.push(FieldDefinition {
        name: "name".into(),
        field_type: FieldType::Text,
        not_null: false,
        primary_key: false,
        default_value: None,
    });
    table.schema.push(FieldDefinition {
        name: "count".into(),
        field_type: FieldType::Integer,
        not_null: false,
        primary_key: false,
        default_value: None,
    });

    let mut fields = HashMap::new();
    fields.insert("name".into(), FieldValue::Text("Tokyo".into()));
    fields.insert("count".into(), FieldValue::Integer(13_960_000));

    let row = FeatureRow {
        fid: 1,
        geometry: Some(GpkgGeometry::Point { x: 139.7, y: 35.7 }),
        fields,
    };
    table.add_feature(row);
    table
}

/// Build a feature table with a single PointZ feature.
fn pointz_table() -> FeatureTable {
    let mut table = FeatureTable::new("pointz", "geom");
    let row = FeatureRow {
        fid: 1,
        geometry: Some(GpkgGeometry::PointZ {
            x: 1.0,
            y: 2.0,
            z: 100.0,
        }),
        fields: HashMap::new(),
    };
    table.add_feature(row);
    table
}

/// Build a feature table with one feature that has a null field value.
fn table_with_null_field() -> FeatureTable {
    let mut table = FeatureTable::new("nullable", "geom");
    table.schema.push(FieldDefinition {
        name: "description".into(),
        field_type: FieldType::Text,
        not_null: false,
        primary_key: false,
        default_value: None,
    });

    let mut fields = HashMap::new();
    fields.insert("description".into(), FieldValue::Null);

    let row = FeatureRow {
        fid: 1,
        geometry: Some(GpkgGeometry::Point { x: 0.0, y: 0.0 }),
        fields,
    };
    table.add_feature(row);
    table
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: empty feature table produces non-zero output (header written)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_export_empty_feature_table_writes_nonzero_bytes() {
    let table = empty_point_table("empty_pts");
    let exporter = FlatGeoBufExporter::new();
    let mut cursor = Cursor::new(Vec::new());
    let result = exporter.export_table(&table, &mut cursor);
    assert!(result.is_ok(), "export_table returned Err: {:?}", result);
    assert_eq!(result.expect("count"), 0);
    // FlatGeobuf header must have been written: expect at least the 8-byte
    // magic + some header bytes.
    assert!(
        cursor.get_ref().len() > 8,
        "output must be larger than magic bytes alone"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: point table with 3 features — export returns 3
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_export_point_table_feature_count_matches() {
    let table = point_table_with_n("pts3", 3);
    let exporter = FlatGeoBufExporter::new();
    let mut cursor = Cursor::new(Vec::new());
    let count = exporter
        .export_table(&table, &mut cursor)
        .expect("export failed");
    assert_eq!(count, 3);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3: linestring table
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_export_linestring_table_returns_ok() {
    let table = linestring_table();
    let exporter = FlatGeoBufExporter::new();
    let mut cursor = Cursor::new(Vec::new());
    let result = exporter.export_table(&table, &mut cursor);
    assert!(result.is_ok(), "linestring export failed: {:?}", result);
    assert_eq!(result.expect("count"), 1);
    assert!(!cursor.get_ref().is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4: polygon with hole
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_export_polygon_with_hole_returns_ok() {
    let table = polygon_with_hole_table();
    let exporter = FlatGeoBufExporter::new();
    let mut cursor = Cursor::new(Vec::new());
    let result = exporter.export_table(&table, &mut cursor);
    assert!(result.is_ok(), "polygon export failed: {:?}", result);
    assert_eq!(result.expect("count"), 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5: multipolygon
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_export_multipolygon_returns_ok() {
    let table = multipolygon_table();
    let exporter = FlatGeoBufExporter::new();
    let mut cursor = Cursor::new(Vec::new());
    let result = exporter.export_table(&table, &mut cursor);
    assert!(result.is_ok(), "multipolygon export failed: {:?}", result);
    assert_eq!(result.expect("count"), 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6: table with string and int columns
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_export_table_with_string_and_int_columns_returns_ok() {
    let table = table_with_string_and_int_columns();
    let exporter = FlatGeoBufExporter::new();
    let mut cursor = Cursor::new(Vec::new());
    let result = exporter.export_table(&table, &mut cursor);
    assert!(result.is_ok(), "string+int export failed: {:?}", result);
    assert_eq!(result.expect("count"), 1);
    assert!(!cursor.get_ref().is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7: nonexistent table in a batch-export scenario returns error
// (we test by exporting a valid FeatureTable and then calling
//  export_tables_to_dir with an invalid path to trigger I/O error)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_export_nonexistent_table_returns_error() {
    use oxigeo_gpkg::GpkgError;
    // Attempt to write to a directory that does not exist.
    let exporter = FlatGeoBufExporter::new();
    let table = point_table_with_n("pts", 1);
    let bad_dir = std::path::PathBuf::from("/nonexistent_dir_oxigeo_test");
    let result = exporter.export_tables_to_dir(&[table], &bad_dir);
    assert!(
        matches!(result, Err(GpkgError::FlatGeoBufExportError(_))),
        "expected FlatGeoBufExportError, got: {:?}",
        result
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 8: export_tables_to_dir creates one file per table
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_export_all_tables_creates_one_file_per_table() {
    let exporter = FlatGeoBufExporter::new();
    let tables = vec![
        point_table_with_n("alpha", 2),
        point_table_with_n("beta", 1),
        linestring_table(),
    ];

    let tmp_dir = std::env::temp_dir().join(format!("oxigeo_fgb_test_{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir).expect("create temp dir");

    let result = exporter.export_tables_to_dir(&tables, &tmp_dir);
    assert!(result.is_ok(), "export_tables_to_dir failed: {:?}", result);
    let counts = result.expect("counts map");

    // Verify one .fgb file per table
    assert_eq!(counts.len(), 3);
    for table in &tables {
        let fgb_path = tmp_dir.join(format!("{}.fgb", table.name));
        assert!(fgb_path.exists(), "{:?} not created", fgb_path);
        assert!(
            fgb_path.metadata().expect("metadata").len() > 0,
            "{:?} is empty",
            fgb_path
        );
    }

    assert_eq!(counts["alpha"], 2);
    assert_eq!(counts["beta"], 1);
    assert_eq!(counts["linestrings"], 1);

    // Clean up
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 9: PointZ table (Z coordinate) exports successfully
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_export_z_coordinate_table_returns_ok() {
    let table = pointz_table();
    let exporter = FlatGeoBufExporter::new();
    let mut cursor = Cursor::new(Vec::new());
    let result = exporter.export_table(&table, &mut cursor);
    assert!(result.is_ok(), "PointZ export failed: {:?}", result);
    assert_eq!(result.expect("count"), 1);
    // The output bytes should be non-empty
    assert!(!cursor.get_ref().is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 10: table with a null field value exports successfully
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_export_table_with_null_field_value_returns_ok() {
    let table = table_with_null_field();
    let exporter = FlatGeoBufExporter::new();
    let mut cursor = Cursor::new(Vec::new());
    let result = exporter.export_table(&table, &mut cursor);
    assert!(result.is_ok(), "null-field export failed: {:?}", result);
    assert_eq!(result.expect("count"), 1);
}
