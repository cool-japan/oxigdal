//! Integration tests for Shapefile driver
//!
//! Tests round-trip reading and writing, format compliance, and error handling.
#![allow(clippy::panic, clippy::unnecessary_cast)]

use oxigdal_core::vector::{Geometry, Point as CorePoint, PropertyValue};
use oxigdal_shapefile::dbf::{FieldType, FieldValue};
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
            PropertyValue::String(format!("Point {}", i)),
        );
        attributes.insert("VALUE".to_string(), PropertyValue::Float(i as f64 * 1.5));
        attributes.insert("ACTIVE".to_string(), PropertyValue::Bool(i % 2 == 0));

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
            Some(&PropertyValue::String("Point 0".to_string()))
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
    let value = FieldValue::parse(b"  test  ", FieldType::Character, 0)
        .expect("Failed to parse string value");
    assert_eq!(value, FieldValue::String("test".to_string()));

    // Integer
    let value =
        FieldValue::parse(b"  123  ", FieldType::Number, 0).expect("Failed to parse integer value");
    assert_eq!(value, FieldValue::Integer(123));

    // Float
    let value =
        FieldValue::parse(b" 12.34 ", FieldType::Number, 2).expect("Failed to parse float value");
    assert_eq!(value, FieldValue::Float(12.34));

    // Boolean true
    let value =
        FieldValue::parse(b"T", FieldType::Logical, 0).expect("Failed to parse boolean true");
    assert_eq!(value, FieldValue::Boolean(true));

    // Boolean false
    let value =
        FieldValue::parse(b"F", FieldType::Logical, 0).expect("Failed to parse boolean false");
    assert_eq!(value, FieldValue::Boolean(false));

    // Date
    let value =
        FieldValue::parse(b"20240125", FieldType::Date, 0).expect("Failed to parse date value");
    assert_eq!(value, FieldValue::Date("20240125".to_string()));

    // Null (empty string)
    let value =
        FieldValue::parse(b"   ", FieldType::Character, 0).expect("Failed to parse null value");
    assert_eq!(value, FieldValue::Null);
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
        attributes.insert("ID".to_string(), PropertyValue::String(format!("ID{}", i)));
        attributes.insert("VALUE".to_string(), PropertyValue::Float(i as f64 * 0.5));

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
