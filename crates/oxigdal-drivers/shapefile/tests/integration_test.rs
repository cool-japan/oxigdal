//! Integration tests for Shapefile driver
//!
//! Tests round-trip reading and writing, format compliance, and error handling.
#![allow(clippy::panic, clippy::unnecessary_cast)]

use oxigdal_core::vector::{FieldValue, Geometry, Point as CorePoint};
use oxigdal_shapefile::dbf::{FieldType, FieldValue as DbfFieldValue};
use oxigdal_shapefile::shp::Shape;
use oxigdal_shapefile::shp::shapes::{Point, ShapeType};
use oxigdal_shapefile::{
    ShapefileFeature, ShapefileReader, ShapefileSchemaBuilder, ShapefileWriter,
};
use std::collections::HashMap;
use std::env;

#[test]
fn test_point_shapefile_round_trip() {
    let temp_dir = env::temp_dir();
    let base_path = temp_dir.join("test_points");

    // Create schema
    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("NAME", 50)
        .expect("Failed to add NAME character field")
        .add_numeric_field("VALUE", 10, 2)
        .expect("Failed to add VALUE numeric field")
        .add_logical_field("ACTIVE")
        .expect("Failed to add ACTIVE logical field")
        .build();

    // Create features
    let mut features = Vec::new();

    for i in 0..10 {
        let mut attributes = HashMap::new();
        attributes.insert(
            "NAME".to_string(),
            FieldValue::String(format!("Point {}", i)),
        );
        attributes.insert("VALUE".to_string(), FieldValue::Float(i as f64 * 1.5));
        attributes.insert("ACTIVE".to_string(), FieldValue::Bool(i % 2 == 0));

        let geometry = Some(Geometry::Point(CorePoint::new(
            i as f64 * 10.0,
            i as f64 * 5.0,
        )));

        features.push(ShapefileFeature::new((i + 1) as i32, geometry, attributes));
    }

    // Write
    {
        let mut writer = ShapefileWriter::new(&base_path, ShapeType::Point, schema.clone())
            .expect("Failed to create shapefile writer");
        writer
            .write_features(&features)
            .expect("Failed to write features");
    }

    // Read
    {
        let reader = ShapefileReader::open(&base_path).expect("Failed to open shapefile");
        assert_eq!(reader.header().shape_type, ShapeType::Point);
        assert_eq!(reader.field_descriptors().len(), 3);

        let read_features = reader.read_features().expect("Failed to read features");
        assert_eq!(read_features.len(), 10);

        // Verify first feature
        let first = &read_features[0];
        assert_eq!(first.record_number, 1);
        assert!(first.geometry.is_some());

        if let Some(Geometry::Point(point)) = &first.geometry {
            assert_eq!(point.coord.x, 0.0);
            assert_eq!(point.coord.y, 0.0);
        } else {
            panic!("Expected Point geometry");
        }

        assert_eq!(
            first.attributes.get("NAME"),
            Some(&FieldValue::String("Point 0".to_string()))
        );
    }

    // Cleanup
    let _ = std::fs::remove_file(base_path.with_extension("shp"));
    let _ = std::fs::remove_file(base_path.with_extension("dbf"));
    let _ = std::fs::remove_file(base_path.with_extension("shx"));
}

#[test]
fn test_empty_feature_error() {
    let temp_dir = env::temp_dir();
    let base_path = temp_dir.join("test_empty");

    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("NAME", 50)
        .expect("Failed to add NAME field")
        .build();

    let mut writer = ShapefileWriter::new(&base_path, ShapeType::Point, schema)
        .expect("Failed to create writer for empty test");

    let result = writer.write_features(&[]);
    assert!(result.is_err());
}

#[test]
fn test_field_types() {
    use oxigdal_shapefile::dbf::{FieldDescriptor, FieldType};

    // Character field
    let field = FieldDescriptor::new("NAME".to_string(), FieldType::Character, 50, 0)
        .expect("Failed to create character field descriptor");
    assert_eq!(field.name, "NAME");
    assert_eq!(field.field_type, FieldType::Character);
    assert_eq!(field.length, 50);

    // Numeric field
    let field = FieldDescriptor::new("VALUE".to_string(), FieldType::Number, 10, 2)
        .expect("Failed to create numeric field descriptor");
    assert_eq!(field.field_type, FieldType::Number);
    assert_eq!(field.decimal_count, 2);

    // Field name too long
    let result = FieldDescriptor::new("VERYLONGNAME".to_string(), FieldType::Character, 50, 0);
    assert!(result.is_err());
}

#[test]
fn test_field_value_parsing() {
    // String
    let value = DbfFieldValue::parse(b"  test  ", FieldType::Character, 0)
        .expect("Failed to parse string value");
    assert_eq!(value, DbfFieldValue::String("test".to_string()));

    // Integer
    let value = DbfFieldValue::parse(b"  123  ", FieldType::Number, 0)
        .expect("Failed to parse integer value");
    assert_eq!(value, DbfFieldValue::Integer(123));

    // Float
    let value = DbfFieldValue::parse(b" 12.34 ", FieldType::Number, 2)
        .expect("Failed to parse float value");
    assert_eq!(value, DbfFieldValue::Float(12.34));

    // Boolean true
    let value =
        DbfFieldValue::parse(b"T", FieldType::Logical, 0).expect("Failed to parse boolean true");
    assert_eq!(value, DbfFieldValue::Boolean(true));

    // Boolean false
    let value =
        DbfFieldValue::parse(b"F", FieldType::Logical, 0).expect("Failed to parse boolean false");
    assert_eq!(value, DbfFieldValue::Boolean(false));

    // Date
    let value =
        DbfFieldValue::parse(b"20240125", FieldType::Date, 0).expect("Failed to parse date value");
    assert_eq!(value, DbfFieldValue::Date("20240125".to_string()));

    // Null (empty string)
    let value =
        DbfFieldValue::parse(b"   ", FieldType::Character, 0).expect("Failed to parse null value");
    assert_eq!(value, DbfFieldValue::Null);
}

#[test]
fn test_shape_type_conversion() {
    assert_eq!(
        ShapeType::from_code(0).expect("Failed to parse Null type"),
        ShapeType::Null
    );
    assert_eq!(
        ShapeType::from_code(1).expect("Failed to parse Point type"),
        ShapeType::Point
    );
    assert_eq!(
        ShapeType::from_code(3).expect("Failed to parse PolyLine type"),
        ShapeType::PolyLine
    );
    assert_eq!(
        ShapeType::from_code(5).expect("Failed to parse Polygon type"),
        ShapeType::Polygon
    );
    assert_eq!(
        ShapeType::from_code(8).expect("Failed to parse MultiPoint type"),
        ShapeType::MultiPoint
    );
    assert_eq!(
        ShapeType::from_code(11).expect("Failed to parse PointZ type"),
        ShapeType::PointZ
    );
    assert_eq!(
        ShapeType::from_code(13).expect("Failed to parse PolyLineZ type"),
        ShapeType::PolyLineZ
    );
    assert_eq!(
        ShapeType::from_code(15).expect("Failed to parse PolygonZ type"),
        ShapeType::PolygonZ
    );
    assert_eq!(
        ShapeType::from_code(21).expect("Failed to parse PointM type"),
        ShapeType::PointM
    );
    assert_eq!(
        ShapeType::from_code(23).expect("Failed to parse PolyLineM type"),
        ShapeType::PolyLineM
    );
    assert_eq!(
        ShapeType::from_code(25).expect("Failed to parse PolygonM type"),
        ShapeType::PolygonM
    );
    assert_eq!(
        ShapeType::from_code(31).expect("Failed to parse MultiPatch type"),
        ShapeType::MultiPatch
    );

    // Invalid code
    let result = ShapeType::from_code(999);
    assert!(result.is_err());
}

#[test]
fn test_shape_type_properties() {
    // Has Z
    assert!(ShapeType::PointZ.has_z());
    assert!(ShapeType::PolyLineZ.has_z());
    assert!(ShapeType::PolygonZ.has_z());
    assert!(ShapeType::MultiPointZ.has_z());
    assert!(ShapeType::MultiPatch.has_z());
    assert!(!ShapeType::Point.has_z());
    assert!(!ShapeType::PolyLine.has_z());

    // Has M
    assert!(ShapeType::PointM.has_m());
    assert!(ShapeType::PolyLineM.has_m());
    assert!(ShapeType::PolygonM.has_m());
    assert!(ShapeType::MultiPointM.has_m());
    assert!(ShapeType::PointZ.has_m()); // Z types also have M
    assert!(ShapeType::PolyLineZ.has_m());
    assert!(!ShapeType::Point.has_m());
}

#[test]
fn test_point_creation() {
    let point = Point::new(10.5, 20.3);
    assert_eq!(point.x, 10.5);
    assert_eq!(point.y, 20.3);

    // Test with Shape enum
    let shape = Shape::Point(point);
    assert_eq!(shape.shape_type(), ShapeType::Point);
}

#[test]
fn test_bounding_box() {
    use oxigdal_shapefile::shp::header::BoundingBox;

    // Valid 2D bbox
    let bbox =
        BoundingBox::new_2d(-180.0, -90.0, 180.0, 90.0).expect("Failed to create 2D bounding box");
    assert_eq!(bbox.x_min, -180.0);
    assert_eq!(bbox.x_max, 180.0);
    assert!(bbox.z_min.is_none());

    // Valid 3D bbox
    let bbox = BoundingBox::new_3d(-180.0, -90.0, 180.0, 90.0, -100.0, 8848.0)
        .expect("Failed to create 3D bounding box");
    assert_eq!(bbox.z_min, Some(-100.0));
    assert_eq!(bbox.z_max, Some(8848.0));

    // Invalid bbox (min > max)
    let result = BoundingBox::new_2d(180.0, -90.0, -180.0, 90.0);
    assert!(result.is_err());

    let result = BoundingBox::new_3d(-180.0, -90.0, 180.0, 90.0, 100.0, -100.0);
    assert!(result.is_err());
}

#[test]
fn test_schema_builder() {
    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("NAME", 50)
        .expect("Failed to add NAME field")
        .add_numeric_field("POPULATION", 10, 0)
        .expect("Failed to add POPULATION field")
        .add_numeric_field("AREA", 15, 3)
        .expect("Failed to add AREA field")
        .add_logical_field("CAPITAL")
        .expect("Failed to add CAPITAL field")
        .add_date_field("FOUNDED")
        .expect("Failed to add FOUNDED field")
        .build();

    assert_eq!(schema.len(), 5);
    assert_eq!(schema[0].name, "NAME");
    assert_eq!(schema[1].name, "POPULATION");
    assert_eq!(schema[2].name, "AREA");
    assert_eq!(schema[2].decimal_count, 3);
    assert_eq!(schema[3].name, "CAPITAL");
    assert_eq!(schema[4].name, "FOUNDED");
}

#[test]
fn test_missing_files() {
    let temp_dir = env::temp_dir();
    let base_path = temp_dir.join("nonexistent_shapefile");

    let result = ShapefileReader::open(&base_path);
    assert!(result.is_err());

    if let Err(e) = result {
        assert!(e.to_string().contains("Missing required file"));
    }
}

#[test]
fn test_shape_content_length() {
    // Null shape
    let shape = Shape::Null;
    assert_eq!(shape.content_length(), 0);

    // Point (2 doubles = 16 bytes = 8 words)
    let shape = Shape::Point(Point::new(10.0, 20.0));
    assert_eq!(shape.content_length(), 8);

    // PointZ (4 doubles = 32 bytes = 16 words)
    let shape = Shape::PointZ(oxigdal_shapefile::PointZ::new(10.0, 20.0, 30.0));
    assert_eq!(shape.content_length(), 16);

    // PointM (3 doubles = 24 bytes = 12 words)
    let shape = Shape::PointM(oxigdal_shapefile::PointM::new(10.0, 20.0, 100.0));
    assert_eq!(shape.content_length(), 12);
}

#[test]
fn test_large_dataset() {
    let temp_dir = env::temp_dir();
    let base_path = temp_dir.join("test_large");

    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("ID", 10)
        .expect("Failed to add ID field")
        .add_numeric_field("VALUE", 10, 2)
        .expect("Failed to add VALUE field")
        .build();

    // Create 200 features (sufficient to test large dataset handling)
    let mut features = Vec::new();
    for i in 0..200 {
        let mut attributes = HashMap::new();
        attributes.insert("ID".to_string(), FieldValue::String(format!("ID{}", i)));
        attributes.insert("VALUE".to_string(), FieldValue::Float(i as f64 * 0.5));

        let geometry = Some(Geometry::Point(CorePoint::new(
            (i % 100) as f64,
            (i / 100) as f64,
        )));

        features.push(ShapefileFeature::new((i + 1) as i32, geometry, attributes));
    }

    // Write
    {
        let mut writer = ShapefileWriter::new(&base_path, ShapeType::Point, schema)
            .expect("Failed to create writer for large dataset");
        writer
            .write_features(&features)
            .expect("Failed to write features for large dataset");
    }

    // Read
    {
        let reader = ShapefileReader::open(&base_path).expect("Failed to open large dataset");
        let read_features = reader
            .read_features()
            .expect("Failed to read features from large dataset");
        assert_eq!(read_features.len(), 200);
    }

    // Cleanup
    let _ = std::fs::remove_file(base_path.with_extension("shp"));
    let _ = std::fs::remove_file(base_path.with_extension("dbf"));
    let _ = std::fs::remove_file(base_path.with_extension("shx"));
}

// ---------------------------------------------------------------------------
// .prj CRS support
// ---------------------------------------------------------------------------

#[test]
fn test_prj_roundtrip() {
    let temp_dir = env::temp_dir();
    let base_path = temp_dir.join("test_prj_roundtrip");

    let wkt = r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]]"#;

    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("NAME", 50)
        .expect("Failed to add NAME field")
        .build();

    let features = vec![ShapefileFeature::new(
        1,
        Some(Geometry::Point(CorePoint::new(10.0, 20.0))),
        HashMap::new(),
    )];

    // Write with CRS set
    {
        let mut writer = ShapefileWriter::new(&base_path, ShapeType::Point, schema)
            .expect("Failed to create writer");
        writer.set_crs(wkt);
        writer
            .write_features(&features)
            .expect("Failed to write features");
    }

    // Read back and verify CRS
    {
        let reader = ShapefileReader::open(&base_path).expect("Failed to open shapefile");
        assert!(
            reader.crs().is_some(),
            "Expected CRS to be present after round-trip"
        );
        assert_eq!(reader.crs(), Some(wkt));
    }

    // Cleanup
    let _ = std::fs::remove_file(base_path.with_extension("shp"));
    let _ = std::fs::remove_file(base_path.with_extension("dbf"));
    let _ = std::fs::remove_file(base_path.with_extension("shx"));
    let _ = std::fs::remove_file(base_path.with_extension("prj"));
}

#[test]
fn test_shapefile_without_prj_still_opens() {
    let temp_dir = env::temp_dir();
    let base_path = temp_dir.join("test_no_prj");

    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("NAME", 50)
        .expect("Failed to add NAME field")
        .build();

    let features = vec![ShapefileFeature::new(
        1,
        Some(Geometry::Point(CorePoint::new(5.0, 5.0))),
        HashMap::new(),
    )];

    // Write without CRS
    {
        let mut writer = ShapefileWriter::new(&base_path, ShapeType::Point, schema)
            .expect("Failed to create writer");
        writer
            .write_features(&features)
            .expect("Failed to write features");
    }

    // Ensure no .prj is present (should not have been created)
    let prj_path = base_path.with_extension("prj");
    if prj_path.exists() {
        std::fs::remove_file(&prj_path).expect("Cleanup .prj");
    }

    // Reader must open fine and return None for crs()
    let reader = ShapefileReader::open(&base_path).expect("Shapefile should open without .prj");
    assert_eq!(reader.crs(), None, "Expected no CRS when .prj is absent");

    // Cleanup
    let _ = std::fs::remove_file(base_path.with_extension("shp"));
    let _ = std::fs::remove_file(base_path.with_extension("dbf"));
    let _ = std::fs::remove_file(base_path.with_extension("shx"));
}

// ---------------------------------------------------------------------------
// .cpg encoding support
// ---------------------------------------------------------------------------

#[test]
fn test_cpg_encoding_read() {
    let temp_dir = env::temp_dir();
    let base_path = temp_dir.join("test_cpg_encoding");

    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("NAME", 50)
        .expect("Failed to add NAME field")
        .build();

    let features = vec![ShapefileFeature::new(
        1,
        Some(Geometry::Point(CorePoint::new(1.0, 2.0))),
        {
            let mut m = HashMap::new();
            m.insert("NAME".to_string(), FieldValue::String("Test".to_string()));
            m
        },
    )];

    // Write shapefile
    {
        let mut writer = ShapefileWriter::new(&base_path, ShapeType::Point, schema)
            .expect("Failed to create writer");
        writer
            .write_features(&features)
            .expect("Failed to write features");
    }

    // Manually write a .cpg file alongside it
    let cpg_path = base_path.with_extension("cpg");
    std::fs::write(&cpg_path, "UTF-8").expect("Failed to write .cpg file");

    // Read and verify encoding is reported
    let reader = ShapefileReader::open(&base_path).expect("Failed to open shapefile");
    assert_eq!(
        reader.encoding(),
        Some("UTF-8"),
        "Expected encoding to be 'UTF-8'"
    );

    // Cleanup
    let _ = std::fs::remove_file(base_path.with_extension("shp"));
    let _ = std::fs::remove_file(base_path.with_extension("dbf"));
    let _ = std::fs::remove_file(base_path.with_extension("shx"));
    let _ = std::fs::remove_file(&cpg_path);
}

// ---------------------------------------------------------------------------
// Spatial index / bbox filtering
// ---------------------------------------------------------------------------

#[test]
fn test_spatial_filter_bbox() {
    let temp_dir = env::temp_dir();
    let base_path = temp_dir.join("test_spatial_bbox");

    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("ID", 10)
        .expect("Failed to add ID field")
        .build();

    // Create 10 points: (10,10), (20,20), ..., (100,100)
    let features: Vec<ShapefileFeature> = (1..=10)
        .map(|i| {
            let x = (i * 10) as f64;
            let y = (i * 10) as f64;
            let mut attrs = HashMap::new();
            attrs.insert("ID".to_string(), FieldValue::String(format!("pt{}", i)));
            ShapefileFeature::new(i, Some(Geometry::Point(CorePoint::new(x, y))), attrs)
        })
        .collect();

    // Write
    {
        let mut writer = ShapefileWriter::new(&base_path, ShapeType::Point, schema)
            .expect("Failed to create writer");
        writer
            .write_features(&features)
            .expect("Failed to write features");
    }

    // Query [0,0,50,50] — should match points at (10,10),(20,20),(30,30),(40,40),(50,50)
    {
        let mut reader = ShapefileReader::open(&base_path).expect("Failed to open shapefile");
        let results = reader
            .features_in_bbox(0.0, 0.0, 50.0, 50.0)
            .expect("Failed spatial filter");
        assert_eq!(
            results.len(),
            5,
            "Expected 5 features in lower-left quadrant (inclusive edges)"
        );

        // Verify all returned features have x,y <= 50
        for feat in &results {
            if let Some(Geometry::Point(p)) = &feat.geometry {
                assert!(
                    p.coord.x <= 50.0 && p.coord.y <= 50.0,
                    "Unexpected point ({}, {}) outside query bbox",
                    p.coord.x,
                    p.coord.y
                );
            } else {
                panic!("Expected Point geometry in spatial filter result");
            }
        }
    }

    // Cleanup
    let _ = std::fs::remove_file(base_path.with_extension("shp"));
    let _ = std::fs::remove_file(base_path.with_extension("dbf"));
    let _ = std::fs::remove_file(base_path.with_extension("shx"));
}

#[test]
fn test_spatial_filter_no_matches() {
    let temp_dir = env::temp_dir();
    let base_path = temp_dir.join("test_spatial_no_match");

    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("ID", 10)
        .expect("Failed to add ID field")
        .build();

    // Points at (60,60),(70,70),(80,80) — all outside query bbox
    let features: Vec<ShapefileFeature> = [60.0_f64, 70.0, 80.0]
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            ShapefileFeature::new(
                (i + 1) as i32,
                Some(Geometry::Point(CorePoint::new(v, v))),
                HashMap::new(),
            )
        })
        .collect();

    // Write
    {
        let mut writer = ShapefileWriter::new(&base_path, ShapeType::Point, schema)
            .expect("Failed to create writer");
        writer
            .write_features(&features)
            .expect("Failed to write features");
    }

    // Query bbox [0,0,50,50] should return no results
    {
        let mut reader = ShapefileReader::open(&base_path).expect("Failed to open shapefile");
        let results = reader
            .features_in_bbox(0.0, 0.0, 50.0, 50.0)
            .expect("Failed spatial filter (no matches)");
        assert!(
            results.is_empty(),
            "Expected empty result when query bbox doesn't intersect any feature"
        );
    }

    // Cleanup
    let _ = std::fs::remove_file(base_path.with_extension("shp"));
    let _ = std::fs::remove_file(base_path.with_extension("dbf"));
    let _ = std::fs::remove_file(base_path.with_extension("shx"));
}

// ---------------------------------------------------------------------------
// Streaming iterator tests
// ---------------------------------------------------------------------------

/// Helper: write N point features and return the base path.
#[allow(clippy::expect_used)]
fn write_n_point_features(name: &str, n: usize) -> std::path::PathBuf {
    let temp_dir = env::temp_dir();
    let base_path = temp_dir.join(name);

    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("ID", 10)
        .expect("add ID field")
        .add_numeric_field("VAL", 10, 2)
        .expect("add VAL field")
        .build();

    let features: Vec<ShapefileFeature> = (0..n)
        .map(|i| {
            let mut attrs = HashMap::new();
            attrs.insert("ID".to_string(), FieldValue::String(format!("id{}", i)));
            attrs.insert("VAL".to_string(), FieldValue::Float(i as f64 * 1.1));
            ShapefileFeature::new(
                (i + 1) as i32,
                Some(Geometry::Point(CorePoint::new(i as f64, i as f64 * 2.0))),
                attrs,
            )
        })
        .collect();

    let mut writer =
        ShapefileWriter::new(&base_path, ShapeType::Point, schema).expect("create writer");
    writer.write_features(&features).expect("write features");

    base_path
}

fn cleanup_base(base: &std::path::Path) {
    let _ = std::fs::remove_file(base.with_extension("shp"));
    let _ = std::fs::remove_file(base.with_extension("dbf"));
    let _ = std::fs::remove_file(base.with_extension("shx"));
}

/// iter_features() visits every feature exactly once (count == 10)
#[test]
fn test_iter_features_count() {
    let base = write_n_point_features("iter_count_10", 10);

    let reader = ShapefileReader::open(&base).expect("open shapefile");
    let count = reader.iter_features().expect("create iter").count();

    assert_eq!(count, 10, "expected 10 features from iter_features");

    cleanup_base(&base);
}

/// Collecting via iter_features() yields the same data as read_features()
#[test]
fn test_iter_features_same_as_read_features() {
    let base = write_n_point_features("iter_same_as_read", 5);

    let reader = ShapefileReader::open(&base).expect("open shapefile");

    // Collect all features via the iterator
    let iter_features: Vec<ShapefileFeature> = reader
        .iter_features()
        .expect("create iter")
        .map(|r| r.expect("iter record ok"))
        .collect();

    // Read all features the traditional way
    let bulk_features = reader.read_features().expect("read_features");

    assert_eq!(
        iter_features.len(),
        bulk_features.len(),
        "feature counts differ"
    );

    // Compare by record_number and geometry (order is deterministic — sequential file read)
    for (iter_feat, bulk_feat) in iter_features.iter().zip(bulk_features.iter()) {
        assert_eq!(
            iter_feat.record_number, bulk_feat.record_number,
            "record_number mismatch at record {}",
            bulk_feat.record_number
        );
        assert_eq!(
            iter_feat.geometry, bulk_feat.geometry,
            "geometry mismatch at record {}",
            bulk_feat.record_number
        );
        // Compare attribute keys
        let iter_keys: std::collections::BTreeSet<_> = iter_feat.attributes.keys().collect();
        let bulk_keys: std::collections::BTreeSet<_> = bulk_feat.attributes.keys().collect();
        assert_eq!(
            iter_keys, bulk_keys,
            "attribute keys differ at record {}",
            bulk_feat.record_number
        );
    }

    cleanup_base(&base);
}

/// .take(3) reads exactly 3 records, not all 10
#[test]
fn test_iter_features_early_termination() {
    let base = write_n_point_features("iter_early_term", 10);

    let reader = ShapefileReader::open(&base).expect("open shapefile");
    let taken: Vec<ShapefileFeature> = reader
        .iter_features()
        .expect("create iter")
        .take(3)
        .map(|r| r.expect("record ok"))
        .collect();

    assert_eq!(taken.len(), 3, "expected exactly 3 features after .take(3)");

    // Verify the record numbers are 1, 2, 3 (first three sequential records)
    assert_eq!(taken[0].record_number, 1);
    assert_eq!(taken[1].record_number, 2);
    assert_eq!(taken[2].record_number, 3);

    cleanup_base(&base);
}

/// Write 500 features and iterate without collecting — verifies that no
/// internal buffering of all records is required.
#[test]
fn test_iter_large_dataset_low_memory() {
    let base = write_n_point_features("iter_large_500", 500);

    let reader = ShapefileReader::open(&base).expect("open shapefile");

    // Use a fold to accumulate a sum without storing any feature — this would
    // fail if the iterator secretly buffers all records into a Vec first.
    let (count, coord_sum) = reader
        .iter_features()
        .expect("create iter")
        .map(|r| r.expect("record ok"))
        .fold((0usize, 0.0_f64), |(cnt, acc), feat| {
            let x = if let Some(Geometry::Point(ref p)) = feat.geometry {
                p.coord.x
            } else {
                0.0
            };
            (cnt + 1, acc + x)
        });

    assert_eq!(count, 500, "expected 500 features");
    // x-coordinates are 0.0, 1.0, ..., 499.0 → sum = 499*500/2 = 124750
    let expected_sum = (0..500_u64).map(|i| i as f64).sum::<f64>();
    assert!(
        (coord_sum - expected_sum).abs() < 1e-6,
        "coordinate sum mismatch: got {coord_sum}, expected {expected_sum}"
    );

    cleanup_base(&base);
}
