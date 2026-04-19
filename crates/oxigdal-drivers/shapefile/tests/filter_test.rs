//! Integration tests for Shapefile attribute filtering.
//!
//! These tests write small Shapefiles and verify that the filtering APIs
//! return exactly the expected subsets.
#![allow(clippy::panic, clippy::expect_used)]

use oxigdal_core::vector::{FieldValue, Geometry, Point as CorePoint};
use oxigdal_shapefile::shp::shapes::ShapeType;
use oxigdal_shapefile::{
    FieldFilter, FieldFilterOp, FilterValue, ShapefileFeature, ShapefileReader,
    ShapefileSchemaBuilder, ShapefileWriter,
};
use std::collections::HashMap;
use std::env;

// ── Helper: build and write a point shapefile with 5 features ─────────────────

fn write_filter_fixture(base_path: &std::path::Path) {
    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("NAME", 50)
        .expect("NAME field")
        .add_numeric_field("VALUE", 10, 2)
        .expect("VALUE field")
        .add_logical_field("ACTIVE")
        .expect("ACTIVE field")
        .build();

    let mut features = Vec::new();
    for i in 0..5u32 {
        let mut attributes = HashMap::new();
        attributes.insert(
            "NAME".to_string(),
            FieldValue::String(format!("Point {}", i + 1)),
        );
        // Values: 1.0, 3.0, 5.0, 7.0, 9.0
        attributes.insert(
            "VALUE".to_string(),
            FieldValue::Float(1.0 + (i as f64) * 2.0),
        );
        // ACTIVE: true for even i (0, 2, 4) → records 1, 3, 5
        attributes.insert("ACTIVE".to_string(), FieldValue::Bool(i % 2 == 0));

        let geometry = Some(Geometry::Point(CorePoint::new(
            i as f64 * 10.0,
            i as f64 * 5.0,
        )));
        features.push(ShapefileFeature::new((i + 1) as i32, geometry, attributes));
    }

    let mut writer =
        ShapefileWriter::new(base_path, ShapeType::Point, schema).expect("create writer");
    writer.write_features(&features).expect("write features");
}

fn cleanup(base_path: &std::path::Path) {
    let _ = std::fs::remove_file(base_path.with_extension("shp"));
    let _ = std::fs::remove_file(base_path.with_extension("dbf"));
    let _ = std::fs::remove_file(base_path.with_extension("shx"));
}

// ── 1. test_filter_string_eq ──────────────────────────────────────────────────

#[test]
fn test_filter_string_eq() {
    let temp_dir = env::temp_dir();
    let base_path = temp_dir.join("filter_string_eq");
    write_filter_fixture(&base_path);

    let reader = ShapefileReader::open(&base_path).expect("open shapefile");

    let filter = FieldFilter {
        field: "NAME".to_string(),
        op: FieldFilterOp::Eq,
        value: FilterValue::String("Point 2".to_string()),
    };
    let results = reader.read_features_filtered(&filter).expect("filter read");
    assert_eq!(
        results.len(),
        1,
        "exactly one feature should match 'Point 2'"
    );
    assert_eq!(
        results[0].attributes.get("NAME"),
        Some(&FieldValue::String("Point 2".to_string()))
    );

    cleanup(&base_path);
}

// ── 2. test_filter_numeric_gt ─────────────────────────────────────────────────

#[test]
fn test_filter_numeric_gt() {
    let temp_dir = env::temp_dir();
    let base_path = temp_dir.join("filter_numeric_gt");
    write_filter_fixture(&base_path);

    let reader = ShapefileReader::open(&base_path).expect("open shapefile");

    // Values are 1.0, 3.0, 5.0, 7.0, 9.0 → >5.0 means 7.0 and 9.0
    let filter = FieldFilter {
        field: "VALUE".to_string(),
        op: FieldFilterOp::Gt,
        value: FilterValue::Float(5.0),
    };
    let results = reader.read_features_filtered(&filter).expect("filter read");
    assert_eq!(results.len(), 2, "only 7.0 and 9.0 are > 5.0");
    for f in &results {
        if let Some(FieldValue::Float(v)) = f.attributes.get("VALUE") {
            assert!(*v > 5.0, "all returned features must have VALUE > 5.0");
        } else {
            panic!("expected Float VALUE attribute");
        }
    }

    cleanup(&base_path);
}

// ── 3. test_filter_string_contains ────────────────────────────────────────────

#[test]
fn test_filter_string_contains() {
    let temp_dir = env::temp_dir();
    let base_path = temp_dir.join("filter_string_contains");
    write_filter_fixture(&base_path);

    let reader = ShapefileReader::open(&base_path).expect("open shapefile");

    let filter = FieldFilter {
        field: "NAME".to_string(),
        op: FieldFilterOp::Contains,
        value: FilterValue::String("Point".to_string()),
    };
    let results = reader.read_features_filtered(&filter).expect("filter read");
    assert_eq!(results.len(), 5, "all 5 features contain 'Point' in NAME");

    cleanup(&base_path);
}

// ── 4. test_filter_missing_field ──────────────────────────────────────────────

#[test]
fn test_filter_missing_field() {
    let temp_dir = env::temp_dir();
    let base_path = temp_dir.join("filter_missing_field");
    write_filter_fixture(&base_path);

    let reader = ShapefileReader::open(&base_path).expect("open shapefile");

    let filter = FieldFilter {
        field: "NONEXISTENT_FIELD".to_string(),
        op: FieldFilterOp::Eq,
        value: FilterValue::String("anything".to_string()),
    };
    let results = reader
        .read_features_filtered(&filter)
        .expect("filter read should not error");
    assert!(
        results.is_empty(),
        "filtering on a non-existent field must return empty results"
    );

    cleanup(&base_path);
}

// ── 5. test_filter_predicate_closure ─────────────────────────────────────────

#[test]
fn test_filter_predicate_closure() {
    let temp_dir = env::temp_dir();
    let base_path = temp_dir.join("filter_predicate_closure");
    write_filter_fixture(&base_path);

    let reader = ShapefileReader::open(&base_path).expect("open shapefile");

    // record_number values are 1..=5; even ones are 2, 4
    let results = reader
        .read_features_where(|f| f.record_number % 2 == 0)
        .expect("filter read");
    assert_eq!(results.len(), 2, "records 2 and 4 have even record_number");
    for f in &results {
        assert_eq!(
            f.record_number % 2,
            0,
            "all returned features must have even record_number"
        );
    }

    cleanup(&base_path);
}

// ── 6. test_filter_bool_eq ────────────────────────────────────────────────────

#[test]
fn test_filter_bool_eq() {
    let temp_dir = env::temp_dir();
    let base_path = temp_dir.join("filter_bool_eq");
    write_filter_fixture(&base_path);

    let reader = ShapefileReader::open(&base_path).expect("open shapefile");

    // ACTIVE is true for i=0,2,4 → records 1,3,5
    let filter = FieldFilter {
        field: "ACTIVE".to_string(),
        op: FieldFilterOp::Eq,
        value: FilterValue::Bool(true),
    };
    let results = reader.read_features_filtered(&filter).expect("filter read");
    assert_eq!(results.len(), 3, "records 1, 3, 5 are ACTIVE=true");
    for f in &results {
        assert_eq!(
            f.attributes.get("ACTIVE"),
            Some(&FieldValue::Bool(true)),
            "all returned features must be ACTIVE"
        );
    }

    cleanup(&base_path);
}
