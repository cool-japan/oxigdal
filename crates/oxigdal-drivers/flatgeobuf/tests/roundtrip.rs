//! Round-trip tests for FlatGeobuf driver
//!
//! Tests writing and reading FlatGeobuf files to ensure data integrity.
#![allow(clippy::panic, clippy::while_let_on_iterator)]

use oxigdal_core::vector::{
    Coordinate, Feature, Geometry, LineString, MultiPoint, Point, Polygon, PropertyValue,
};
use oxigdal_flatgeobuf::{
    Column, ColumnType, CrsInfo, FlatGeobufReader, FlatGeobufWriter, FlatGeobufWriterBuilder,
    GeometryType, Header,
};
use std::io::Cursor;

#[test]
fn test_roundtrip_simple_points() {
    // Create a writer with Point geometry
    let header = Header::new(GeometryType::Point);
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    // Add some point features
    for i in 0..10 {
        let point = Point::new(i as f64, i as f64 * 2.0);
        let feature = Feature::new(Geometry::Point(point));
        writer.add_feature(&feature).expect("Failed to add feature");
    }

    // Finish writing
    let cursor = writer.finish().expect("Failed to finish writing");

    // Read back
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    // Verify header
    assert_eq!(reader.header().geometry_type, GeometryType::Point);

    // Read all features
    let mut count = 0;
    let mut features_iter = reader.features().expect("Failed to get features iterator");
    while let Some(result) = features_iter.next() {
        let feature = result.expect("Failed to read feature");
        assert!(feature.has_geometry());

        if let Some(Geometry::Point(point)) = &feature.geometry {
            assert_eq!(point.coord.x, count as f64);
            assert_eq!(point.coord.y, count as f64 * 2.0);
        } else {
            panic!("Expected Point geometry");
        }

        count += 1;
    }

    assert_eq!(count, 10);
}

#[test]
fn test_roundtrip_points_with_properties() {
    // Create header with columns
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("name", ColumnType::String));
    header.add_column(Column::new("value", ColumnType::Int));
    header.add_column(Column::new("score", ColumnType::Double));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    // Add features with properties
    for i in 0..5 {
        let point = Point::new(i as f64, i as f64);
        let mut feature = Feature::new(Geometry::Point(point));
        feature.set_property("name", PropertyValue::String(format!("Point {i}")));
        feature.set_property("value", PropertyValue::Integer(i));
        feature.set_property("score", PropertyValue::Float(i as f64 * 1.5));
        writer.add_feature(&feature).expect("Failed to add feature");
    }

    let cursor = writer.finish().expect("Failed to finish writing");

    // Read back
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    // Verify columns
    assert_eq!(reader.header().columns.len(), 3);
    assert_eq!(reader.header().columns[0].name, "name");
    assert_eq!(reader.header().columns[1].name, "value");
    assert_eq!(reader.header().columns[2].name, "score");

    // Read and verify features
    let mut count = 0;
    let mut features_iter = reader.features().expect("Failed to get features iterator");
    while let Some(result) = features_iter.next() {
        let feature = result.expect("Failed to read feature");

        // Verify properties
        let name = feature.get_property("name").expect("Missing name property");
        let value = feature
            .get_property("value")
            .expect("Missing value property");
        let score = feature
            .get_property("score")
            .expect("Missing score property");

        if let Some(s) = name.as_string() {
            assert_eq!(s, &format!("Point {count}"));
        } else {
            panic!("Expected string value for name");
        }
        assert_eq!(value.as_i64(), Some(count));
        assert_eq!(score.as_f64(), Some(count as f64 * 1.5));

        count += 1;
    }

    assert_eq!(count, 5);
}

#[test]
fn test_roundtrip_linestrings() {
    let header = Header::new(GeometryType::LineString);
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    // Create linestring
    let coords = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(1.0, 1.0),
        Coordinate::new_2d(2.0, 0.0),
    ];
    let linestring = LineString::new(coords).expect("Failed to create linestring");
    let feature = Feature::new(Geometry::LineString(linestring));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish writing");

    // Read back
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get features iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    if let Some(Geometry::LineString(ls)) = &feature.geometry {
        assert_eq!(ls.len(), 3);
        assert_eq!(ls.coords[0].x, 0.0);
        assert_eq!(ls.coords[1].x, 1.0);
        assert_eq!(ls.coords[2].x, 2.0);
    } else {
        panic!("Expected LineString geometry");
    }
}

#[test]
fn test_roundtrip_polygons() {
    let header = Header::new(GeometryType::Polygon);
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    // Create polygon with hole
    let exterior = LineString::new(vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(10.0, 0.0),
        Coordinate::new_2d(10.0, 10.0),
        Coordinate::new_2d(0.0, 10.0),
        Coordinate::new_2d(0.0, 0.0),
    ])
    .expect("Failed to create exterior");

    let hole = LineString::new(vec![
        Coordinate::new_2d(2.0, 2.0),
        Coordinate::new_2d(8.0, 2.0),
        Coordinate::new_2d(8.0, 8.0),
        Coordinate::new_2d(2.0, 8.0),
        Coordinate::new_2d(2.0, 2.0),
    ])
    .expect("Failed to create hole");

    let polygon = Polygon::new(exterior, vec![hole]).expect("Failed to create polygon");
    let feature = Feature::new(Geometry::Polygon(polygon));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish writing");

    // Read back
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get features iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    if let Some(Geometry::Polygon(poly)) = &feature.geometry {
        assert_eq!(poly.exterior.len(), 5);
        assert_eq!(poly.interiors.len(), 1);
        assert_eq!(poly.interiors[0].len(), 5);
    } else {
        panic!("Expected Polygon geometry");
    }
}

#[test]
fn test_roundtrip_multipoint() {
    let header = Header::new(GeometryType::MultiPoint);
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let points = vec![
        Point::new(0.0, 0.0),
        Point::new(1.0, 1.0),
        Point::new(2.0, 2.0),
    ];
    let multipoint = MultiPoint::new(points);
    let feature = Feature::new(Geometry::MultiPoint(multipoint));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish writing");

    // Read back
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get features iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    if let Some(Geometry::MultiPoint(mp)) = &feature.geometry {
        assert_eq!(mp.points.len(), 3);
    } else {
        panic!("Expected MultiPoint geometry");
    }
}

#[test]
fn test_roundtrip_3d_points() {
    let header = Header::new(GeometryType::Point).with_z();
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let point = Point::new_3d(1.0, 2.0, 3.0);
    let feature = Feature::new(Geometry::Point(point));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish writing");

    // Read back
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    assert!(reader.header().has_z);

    let mut features_iter = reader.features().expect("Failed to get features iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    if let Some(Geometry::Point(p)) = &feature.geometry {
        assert_eq!(p.coord.x, 1.0);
        assert_eq!(p.coord.y, 2.0);
        assert_eq!(p.coord.z, Some(3.0));
    } else {
        panic!("Expected Point geometry");
    }
}

#[test]
fn test_roundtrip_with_index() {
    let header = Header::new(GeometryType::Point).with_index(true);
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    // Add features in different spatial locations
    for i in 0..20 {
        let x = (i % 5) as f64;
        let y = (i / 5) as f64;
        let point = Point::new(x, y);
        let feature = Feature::new(Geometry::Point(point));
        writer.add_feature(&feature).expect("Failed to add feature");
    }

    let cursor = writer.finish().expect("Failed to finish writing");

    // Read back
    let cursor = Cursor::new(cursor.into_inner());
    let reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    // Verify index exists
    assert!(reader.header().has_index);
    assert!(reader.index().is_some());
}

#[test]
fn test_roundtrip_with_crs() {
    let header = Header::new(GeometryType::Point).with_crs(CrsInfo::from_epsg(4326));
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let point = Point::new(-122.4, 37.8);
    let feature = Feature::new(Geometry::Point(point));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish writing");

    // Read back
    let cursor = Cursor::new(cursor.into_inner());
    let reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    // Verify CRS
    assert!(reader.header().crs.is_some());
    let crs = reader.header().crs.as_ref().expect("Missing CRS");
    assert_eq!(crs.organization, Some("EPSG".to_string()));
    assert_eq!(crs.organization_code, Some(4326));
}

#[test]
fn test_roundtrip_null_properties() {
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("nullable_field", ColumnType::String));
    header.add_column(Column::new("required_field", ColumnType::Int));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    // Add feature with null property
    let point = Point::new(0.0, 0.0);
    let mut feature = Feature::new(Geometry::Point(point));
    feature.set_property("nullable_field", PropertyValue::Null);
    feature.set_property("required_field", PropertyValue::Integer(42));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish writing");

    // Read back
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get features iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    let nullable = feature
        .get_property("nullable_field")
        .expect("Missing nullable_field");
    let required = feature
        .get_property("required_field")
        .expect("Missing required_field");

    assert!(nullable.is_null());
    assert_eq!(required.as_i64(), Some(42));
}

#[test]
fn test_roundtrip_empty_collection() {
    let header = Header::new(GeometryType::Point);
    let cursor = Cursor::new(Vec::new());
    let writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    // Finish without adding any features
    let cursor = writer.finish().expect("Failed to finish writing");

    // Read back
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get features iterator");
    assert!(features_iter.next().is_none());
}

#[test]
fn test_roundtrip_mixed_property_types() {
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("byte_field", ColumnType::Byte));
    header.add_column(Column::new("bool_field", ColumnType::Bool));
    header.add_column(Column::new("short_field", ColumnType::Short));
    header.add_column(Column::new("int_field", ColumnType::Int));
    header.add_column(Column::new("long_field", ColumnType::Long));
    header.add_column(Column::new("float_field", ColumnType::Float));
    header.add_column(Column::new("double_field", ColumnType::Double));
    header.add_column(Column::new("string_field", ColumnType::String));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let point = Point::new(0.0, 0.0);
    let mut feature = Feature::new(Geometry::Point(point));
    feature.set_property("byte_field", PropertyValue::Integer(127));
    feature.set_property("bool_field", PropertyValue::Bool(true));
    feature.set_property("short_field", PropertyValue::Integer(32000));
    feature.set_property("int_field", PropertyValue::Integer(2_000_000_000));
    feature.set_property(
        "long_field",
        PropertyValue::Integer(9_000_000_000_000_000_000),
    );
    feature.set_property("float_field", PropertyValue::Float(std::f64::consts::PI));
    feature.set_property("double_field", PropertyValue::Float(std::f64::consts::E));
    feature.set_property(
        "string_field",
        PropertyValue::String("Hello, FlatGeobuf!".to_string()),
    );

    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish writing");

    // Read back
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get features iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    assert_eq!(
        feature.get_property("byte_field").and_then(|v| v.as_i64()),
        Some(127)
    );
    assert_eq!(
        feature.get_property("bool_field").and_then(|v| v.as_bool()),
        Some(true)
    );
    if let Some(s) = feature
        .get_property("string_field")
        .and_then(|v| v.as_string())
    {
        assert_eq!(s, "Hello, FlatGeobuf!");
    } else {
        panic!("Expected string value");
    }
}

#[test]
fn test_builder_api() {
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriterBuilder::new(GeometryType::Point)
        .with_z()
        .with_index()
        .with_crs(CrsInfo::from_epsg(4326))
        .with_column(Column::new("name", ColumnType::String))
        .build(cursor)
        .expect("Failed to build writer");

    let point = Point::new_3d(1.0, 2.0, 3.0);
    let mut feature = Feature::new(Geometry::Point(point));
    feature.set_property("name", PropertyValue::String("Test".to_string()));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish writing");

    // Read back
    let cursor = Cursor::new(cursor.into_inner());
    let reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    assert!(reader.header().has_z);
    assert!(reader.header().has_index);
    assert!(reader.header().crs.is_some());
    assert_eq!(reader.header().columns.len(), 1);
}
