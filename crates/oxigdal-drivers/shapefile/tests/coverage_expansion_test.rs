//! Expanded integration tests for Shapefile driver - Core Driver Test Coverage
//!
//! This module adds 20+ additional tests covering:
//! - Point, MultiPoint, LineString, MultiLineString, Polygon, MultiPolygon geometries
//! - Attribute table (DBF) round-trip with various field types
//! - Empty geometry collections
//! - Large feature counts
//! - Special characters in attribute values
//! - Error handling (missing .shx, missing .dbf)

#![allow(clippy::panic, clippy::unnecessary_cast)]

use oxigdal_core::vector::{
    Coordinate, Geometry, LineString as CoreLineString, MultiLineString as CoreMultiLineString,
    MultiPoint as CoreMultiPoint, Point as CorePoint, Polygon as CorePolygon, PropertyValue,
};
use oxigdal_shapefile::shp::shapes::ShapeType;
use oxigdal_shapefile::{
    ShapefileFeature, ShapefileReader, ShapefileSchemaBuilder, ShapefileWriter,
};
use std::collections::HashMap;
use std::env;

/// Helper to create temp base path with unique name
fn temp_base_path(name: &str) -> std::path::PathBuf {
    let temp_dir = env::temp_dir();
    temp_dir.join(format!("oxigdal_shp_cov_{}", name))
}

/// Helper to clean up shapefile triple
fn cleanup_shapefile(base_path: &std::path::Path) {
    let _ = std::fs::remove_file(base_path.with_extension("shp"));
    let _ = std::fs::remove_file(base_path.with_extension("dbf"));
    let _ = std::fs::remove_file(base_path.with_extension("shx"));
}

// ============================================================
// Geometry type tests
// ============================================================

/// Test 1: Point geometry round-trip
#[test]
fn test_cov_point_geometry_roundtrip() {
    let base_path = temp_base_path("point_rt");
    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("NAME", 30)
        .expect("Failed to add NAME field for point roundtrip")
        .build();

    let mut features = Vec::new();
    for i in 0..5 {
        let mut attributes = HashMap::new();
        attributes.insert(
            "NAME".to_string(),
            PropertyValue::String(format!("Pt{}", i)),
        );
        let geometry = Some(Geometry::Point(CorePoint::new(
            i as f64 * 10.0 + 0.5,
            i as f64 * 5.0 + 0.25,
        )));
        features.push(ShapefileFeature::new((i + 1) as i32, geometry, attributes));
    }

    {
        let mut writer = ShapefileWriter::new(&base_path, ShapeType::Point, schema.clone())
            .expect("Failed to create writer for point roundtrip");
        writer
            .write_features(&features)
            .expect("Failed to write point features");
    }

    {
        let reader = ShapefileReader::open(&base_path).expect("Failed to open point shapefile");
        assert_eq!(reader.header().shape_type, ShapeType::Point);
        let read_features = reader
            .read_features()
            .expect("Failed to read point features");
        assert_eq!(read_features.len(), 5, "Should have 5 point features");

        for (i, feat) in read_features.iter().enumerate() {
            if let Some(Geometry::Point(point)) = &feat.geometry {
                assert!(
                    (point.coord.x - (i as f64 * 10.0 + 0.5)).abs() < 1e-6,
                    "Point {} X should match",
                    i
                );
                assert!(
                    (point.coord.y - (i as f64 * 5.0 + 0.25)).abs() < 1e-6,
                    "Point {} Y should match",
                    i
                );
            } else {
                panic!("Feature {} should be a Point geometry", i);
            }
        }
    }

    cleanup_shapefile(&base_path);
}

/// Test 2: LineString (PolyLine) geometry round-trip
#[test]
fn test_cov_linestring_geometry_roundtrip() {
    let base_path = temp_base_path("linestring_rt");
    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("NAME", 30)
        .expect("Failed to add NAME field for linestring")
        .build();

    let coords = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(10.0, 10.0),
        Coordinate::new_2d(20.0, 5.0),
        Coordinate::new_2d(30.0, 15.0),
    ];
    let linestring =
        CoreLineString::new(coords).expect("Failed to create linestring for roundtrip");
    let geometry = Some(Geometry::LineString(linestring));

    let mut attributes = HashMap::new();
    attributes.insert(
        "NAME".to_string(),
        PropertyValue::String("TestLine".to_string()),
    );
    let features = vec![ShapefileFeature::new(1, geometry, attributes)];

    {
        let mut writer = ShapefileWriter::new(&base_path, ShapeType::PolyLine, schema)
            .expect("Failed to create writer for linestring roundtrip");
        writer
            .write_features(&features)
            .expect("Failed to write linestring features");
    }

    {
        let reader =
            ShapefileReader::open(&base_path).expect("Failed to open linestring shapefile");
        assert_eq!(reader.header().shape_type, ShapeType::PolyLine);
        let read_features = reader
            .read_features()
            .expect("Failed to read linestring features");
        assert_eq!(read_features.len(), 1, "Should have 1 linestring feature");

        if let Some(Geometry::LineString(ls)) = &read_features[0].geometry {
            assert_eq!(ls.coords.len(), 4, "LineString should have 4 coordinates");
            assert!(
                (ls.coords[0].x - 0.0).abs() < 1e-6,
                "First coord X should be 0.0"
            );
            assert!(
                (ls.coords[3].x - 30.0).abs() < 1e-6,
                "Last coord X should be 30.0"
            );
        } else {
            panic!("Expected LineString geometry");
        }
    }

    cleanup_shapefile(&base_path);
}

/// Test 3: Polygon geometry round-trip
#[test]
fn test_cov_polygon_geometry_roundtrip() {
    let base_path = temp_base_path("polygon_rt");
    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("NAME", 30)
        .expect("Failed to add NAME field for polygon roundtrip")
        .build();

    let exterior_coords = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(10.0, 0.0),
        Coordinate::new_2d(10.0, 10.0),
        Coordinate::new_2d(0.0, 10.0),
        Coordinate::new_2d(0.0, 0.0),
    ];
    let exterior =
        CoreLineString::new(exterior_coords).expect("Failed to create exterior ring for polygon");
    let polygon =
        CorePolygon::new(exterior, vec![]).expect("Failed to create polygon for polygon roundtrip");
    let geometry = Some(Geometry::Polygon(polygon));

    let mut attributes = HashMap::new();
    attributes.insert(
        "NAME".to_string(),
        PropertyValue::String("TestPoly".to_string()),
    );
    let features = vec![ShapefileFeature::new(1, geometry, attributes)];

    {
        let mut writer = ShapefileWriter::new(&base_path, ShapeType::Polygon, schema)
            .expect("Failed to create writer for polygon roundtrip");
        writer
            .write_features(&features)
            .expect("Failed to write polygon features");
    }

    {
        let reader = ShapefileReader::open(&base_path).expect("Failed to open polygon shapefile");
        assert_eq!(reader.header().shape_type, ShapeType::Polygon);
        let read_features = reader
            .read_features()
            .expect("Failed to read polygon features");
        assert_eq!(read_features.len(), 1, "Should have 1 polygon feature");

        if let Some(Geometry::Polygon(poly)) = &read_features[0].geometry {
            assert!(
                poly.exterior.coords.len() >= 4,
                "Polygon should have at least 4 exterior coordinates"
            );
        } else {
            panic!("Expected Polygon geometry");
        }
    }

    cleanup_shapefile(&base_path);
}

/// Test 4: MultiPoint geometry round-trip
#[test]
fn test_cov_multipoint_geometry_roundtrip() {
    let base_path = temp_base_path("multipoint_rt");
    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("NAME", 30)
        .expect("Failed to add NAME field for multipoint roundtrip")
        .build();

    let points = vec![
        CorePoint::new(1.0, 2.0),
        CorePoint::new(3.0, 4.0),
        CorePoint::new(5.0, 6.0),
    ];
    let multipoint = CoreMultiPoint::new(points);
    let geometry = Some(Geometry::MultiPoint(multipoint));

    let mut attributes = HashMap::new();
    attributes.insert(
        "NAME".to_string(),
        PropertyValue::String("TestMP".to_string()),
    );
    let features = vec![ShapefileFeature::new(1, geometry, attributes)];

    {
        let mut writer = ShapefileWriter::new(&base_path, ShapeType::MultiPoint, schema)
            .expect("Failed to create writer for multipoint roundtrip");
        writer
            .write_features(&features)
            .expect("Failed to write multipoint features");
    }

    {
        let reader =
            ShapefileReader::open(&base_path).expect("Failed to open multipoint shapefile");
        assert_eq!(reader.header().shape_type, ShapeType::MultiPoint);
        let read_features = reader
            .read_features()
            .expect("Failed to read multipoint features");
        assert_eq!(read_features.len(), 1, "Should have 1 multipoint feature");

        if let Some(Geometry::MultiPoint(mp)) = &read_features[0].geometry {
            assert_eq!(mp.points.len(), 3, "MultiPoint should have 3 points");
        } else {
            panic!("Expected MultiPoint geometry");
        }
    }

    cleanup_shapefile(&base_path);
}

/// Test 5: MultiLineString geometry round-trip
#[test]
fn test_cov_multilinestring_geometry_roundtrip() {
    let base_path = temp_base_path("multilinestring_rt");
    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("NAME", 30)
        .expect("Failed to add NAME field for multilinestring roundtrip")
        .build();

    let ls1 = CoreLineString::new(vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(5.0, 5.0),
    ])
    .expect("Failed to create line 1 for multilinestring");
    let ls2 = CoreLineString::new(vec![
        Coordinate::new_2d(10.0, 10.0),
        Coordinate::new_2d(15.0, 15.0),
        Coordinate::new_2d(20.0, 10.0),
    ])
    .expect("Failed to create line 2 for multilinestring");

    let mls = CoreMultiLineString::new(vec![ls1, ls2]);
    let geometry = Some(Geometry::MultiLineString(mls));

    let mut attributes = HashMap::new();
    attributes.insert(
        "NAME".to_string(),
        PropertyValue::String("TestMLS".to_string()),
    );
    let features = vec![ShapefileFeature::new(1, geometry, attributes)];

    {
        let mut writer = ShapefileWriter::new(&base_path, ShapeType::PolyLine, schema)
            .expect("Failed to create writer for multilinestring roundtrip");
        writer
            .write_features(&features)
            .expect("Failed to write multilinestring features");
    }

    {
        let reader =
            ShapefileReader::open(&base_path).expect("Failed to open multilinestring shapefile");
        let read_features = reader
            .read_features()
            .expect("Failed to read multilinestring features");
        assert_eq!(
            read_features.len(),
            1,
            "Should have 1 multilinestring feature"
        );

        match &read_features[0].geometry {
            Some(Geometry::MultiLineString(mls)) => {
                assert_eq!(
                    mls.line_strings.len(),
                    2,
                    "Should have 2 linestrings in MultiLineString"
                );
            }
            Some(Geometry::LineString(_)) => {
                // If reader returned single LineString, that's also acceptable for single-part
                // but we have 2 parts, so this should not happen
                panic!("Expected MultiLineString, got single LineString");
            }
            other => panic!("Expected MultiLineString geometry, got {:?}", other),
        }
    }

    cleanup_shapefile(&base_path);
}

/// Test 6: Polygon with hole (interior ring) round-trip
#[test]
fn test_cov_polygon_with_hole_roundtrip() {
    let base_path = temp_base_path("polygon_hole_rt");
    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("NAME", 30)
        .expect("Failed to add NAME field for polygon with hole")
        .build();

    let exterior = CoreLineString::new(vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(20.0, 0.0),
        Coordinate::new_2d(20.0, 20.0),
        Coordinate::new_2d(0.0, 20.0),
        Coordinate::new_2d(0.0, 0.0),
    ])
    .expect("Failed to create exterior for polygon with hole");

    let interior = CoreLineString::new(vec![
        Coordinate::new_2d(5.0, 5.0),
        Coordinate::new_2d(15.0, 5.0),
        Coordinate::new_2d(15.0, 15.0),
        Coordinate::new_2d(5.0, 15.0),
        Coordinate::new_2d(5.0, 5.0),
    ])
    .expect("Failed to create interior ring for polygon with hole");

    let polygon =
        CorePolygon::new(exterior, vec![interior]).expect("Failed to create polygon with hole");
    let geometry = Some(Geometry::Polygon(polygon));

    let mut attributes = HashMap::new();
    attributes.insert(
        "NAME".to_string(),
        PropertyValue::String("WithHole".to_string()),
    );
    let features = vec![ShapefileFeature::new(1, geometry, attributes)];

    {
        let mut writer = ShapefileWriter::new(&base_path, ShapeType::Polygon, schema)
            .expect("Failed to create writer for polygon with hole");
        writer
            .write_features(&features)
            .expect("Failed to write polygon with hole");
    }

    {
        let reader =
            ShapefileReader::open(&base_path).expect("Failed to open polygon-with-hole shapefile");
        let read_features = reader
            .read_features()
            .expect("Failed to read polygon with hole features");
        assert_eq!(
            read_features.len(),
            1,
            "Should have 1 polygon with hole feature"
        );

        if let Some(Geometry::Polygon(poly)) = &read_features[0].geometry {
            assert_eq!(
                poly.interiors.len(),
                1,
                "Polygon should have 1 interior ring"
            );
        } else {
            panic!("Expected Polygon geometry with hole");
        }
    }

    cleanup_shapefile(&base_path);
}

// ============================================================
// Attribute (DBF) tests
// ============================================================

/// Test 7: Various DBF field types round-trip
#[test]
fn test_cov_dbf_field_types_roundtrip() {
    let base_path = temp_base_path("dbf_types_rt");
    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("CHARFLD", 50)
        .expect("Failed to add CHARFLD for field type roundtrip")
        .add_numeric_field("INTFLD", 10, 0)
        .expect("Failed to add INTFLD for field type roundtrip")
        .add_numeric_field("FLTFLD", 15, 6)
        .expect("Failed to add FLTFLD for field type roundtrip")
        .add_logical_field("BOOLFLD")
        .expect("Failed to add BOOLFLD for field type roundtrip")
        .add_date_field("DATEFLD")
        .expect("Failed to add DATEFLD for field type roundtrip")
        .build();

    let mut attributes = HashMap::new();
    attributes.insert(
        "CHARFLD".to_string(),
        PropertyValue::String("Hello World".to_string()),
    );
    attributes.insert("INTFLD".to_string(), PropertyValue::Integer(42));
    #[allow(clippy::approx_constant)]
    let flt_val = 3.141_593f64;
    attributes.insert("FLTFLD".to_string(), PropertyValue::Float(flt_val));
    attributes.insert("BOOLFLD".to_string(), PropertyValue::Bool(true));
    attributes.insert(
        "DATEFLD".to_string(),
        PropertyValue::String("20260310".to_string()),
    );

    let geometry = Some(Geometry::Point(CorePoint::new(1.0, 2.0)));
    let features = vec![ShapefileFeature::new(1, geometry, attributes)];

    {
        let mut writer = ShapefileWriter::new(&base_path, ShapeType::Point, schema)
            .expect("Failed to create writer for DBF type roundtrip");
        writer
            .write_features(&features)
            .expect("Failed to write DBF type roundtrip features");
    }

    {
        let reader = ShapefileReader::open(&base_path).expect("Failed to open DBF type roundtrip");
        assert_eq!(
            reader.field_descriptors().len(),
            5,
            "Should have 5 field descriptors"
        );
        let read_features = reader
            .read_features()
            .expect("Failed to read DBF type roundtrip features");
        assert_eq!(read_features.len(), 1, "Should have 1 feature");

        let attrs = &read_features[0].attributes;
        assert_eq!(
            attrs.get("CHARFLD"),
            Some(&PropertyValue::String("Hello World".to_string())),
            "CHARFLD should match"
        );
        if let Some(PropertyValue::Integer(val)) = attrs.get("INTFLD") {
            assert_eq!(*val, 42, "INTFLD should be 42");
        } else {
            panic!("INTFLD should be Integer");
        }
        if let Some(PropertyValue::Float(val)) = attrs.get("FLTFLD") {
            #[allow(clippy::approx_constant)]
            let expected_flt = 3.141_593f64;
            assert!(
                (*val - expected_flt).abs() < 0.001,
                "FLTFLD should be approximately 3.141593"
            );
        } else {
            panic!("FLTFLD should be Float");
        }
        assert_eq!(
            attrs.get("BOOLFLD"),
            Some(&PropertyValue::Bool(true)),
            "BOOLFLD should be true"
        );
    }

    cleanup_shapefile(&base_path);
}

/// Test 8: Numeric attribute precision
#[test]
fn test_cov_numeric_precision() {
    let base_path = temp_base_path("num_precision");
    let schema = ShapefileSchemaBuilder::new()
        .add_numeric_field("PRECISE", 20, 8)
        .expect("Failed to add PRECISE field")
        .build();

    let mut attributes = HashMap::new();
    attributes.insert("PRECISE".to_string(), PropertyValue::Float(123456.78901234));

    let geometry = Some(Geometry::Point(CorePoint::new(0.0, 0.0)));
    let features = vec![ShapefileFeature::new(1, geometry, attributes)];

    {
        let mut writer = ShapefileWriter::new(&base_path, ShapeType::Point, schema)
            .expect("Failed to create writer for numeric precision");
        writer
            .write_features(&features)
            .expect("Failed to write numeric precision features");
    }

    {
        let reader =
            ShapefileReader::open(&base_path).expect("Failed to open numeric precision shapefile");
        let read_features = reader
            .read_features()
            .expect("Failed to read numeric precision features");

        if let Some(PropertyValue::Float(val)) = read_features[0].attributes.get("PRECISE") {
            assert!(
                (*val - 123456.78901234).abs() < 0.01,
                "Precise numeric value should be approximately correct"
            );
        } else {
            panic!("PRECISE should be Float");
        }
    }

    cleanup_shapefile(&base_path);
}

/// Test 9: Special characters in attribute values
#[test]
fn test_cov_special_characters_in_attributes() {
    let base_path = temp_base_path("special_chars");
    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("TEXT", 100)
        .expect("Failed to add TEXT field for special chars")
        .build();

    let test_strings: &[&str] = &[
        "Hello, World!",
        "Line1 Line2",
        "Tab\there",
        "Quotes'and\"stuff",
        "Numbers123",
        "Symbols@#$%^&*",
        "  Leading spaces",
        "Trailing spaces  ",
    ];

    let mut features = Vec::new();
    for (i, text) in test_strings.iter().enumerate() {
        let mut attributes = HashMap::new();
        attributes.insert("TEXT".to_string(), PropertyValue::String(text.to_string()));
        let geometry = Some(Geometry::Point(CorePoint::new(i as f64, 0.0)));
        features.push(ShapefileFeature::new((i + 1) as i32, geometry, attributes));
    }

    {
        let mut writer = ShapefileWriter::new(&base_path, ShapeType::Point, schema)
            .expect("Failed to create writer for special chars");
        writer
            .write_features(&features)
            .expect("Failed to write special chars features");
    }

    {
        let reader =
            ShapefileReader::open(&base_path).expect("Failed to open special chars shapefile");
        let read_features = reader
            .read_features()
            .expect("Failed to read special chars features");
        assert_eq!(
            read_features.len(),
            test_strings.len(),
            "Should have all special char features"
        );

        // Verify first feature text is preserved
        if let Some(PropertyValue::String(val)) = read_features[0].attributes.get("TEXT") {
            assert_eq!(val.trim(), "Hello, World!", "First text should match");
        }
    }

    cleanup_shapefile(&base_path);
}

/// Test 10: Null attribute values
#[test]
fn test_cov_null_attributes() {
    let base_path = temp_base_path("null_attrs");
    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("NAME", 30)
        .expect("Failed to add NAME field for null attrs")
        .add_numeric_field("VALUE", 10, 2)
        .expect("Failed to add VALUE field for null attrs")
        .build();

    let mut attributes = HashMap::new();
    attributes.insert("NAME".to_string(), PropertyValue::Null);
    attributes.insert("VALUE".to_string(), PropertyValue::Null);

    let geometry = Some(Geometry::Point(CorePoint::new(0.0, 0.0)));
    let features = vec![ShapefileFeature::new(1, geometry, attributes)];

    {
        let mut writer = ShapefileWriter::new(&base_path, ShapeType::Point, schema)
            .expect("Failed to create writer for null attrs");
        writer
            .write_features(&features)
            .expect("Failed to write null attr features");
    }

    {
        let reader = ShapefileReader::open(&base_path).expect("Failed to open null attr shapefile");
        let read_features = reader
            .read_features()
            .expect("Failed to read null attr features");
        assert_eq!(read_features.len(), 1, "Should have 1 null attr feature");

        // Null values should come back as Null
        let name_val = read_features[0].attributes.get("NAME");
        assert!(
            name_val == Some(&PropertyValue::Null)
                || matches!(name_val, Some(PropertyValue::String(s)) if s.trim().is_empty()),
            "NAME should be null or empty"
        );
    }

    cleanup_shapefile(&base_path);
}

// ============================================================
// Large feature count tests
// ============================================================

/// Test 11: Large feature count (500 features)
#[test]
fn test_cov_large_feature_count() {
    let base_path = temp_base_path("large_count");
    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("ID", 10)
        .expect("Failed to add ID field for large count")
        .add_numeric_field("SEQ", 10, 0)
        .expect("Failed to add SEQ field for large count")
        .build();

    let count = 500;
    let mut features = Vec::with_capacity(count);
    for i in 0..count {
        let mut attributes = HashMap::new();
        attributes.insert(
            "ID".to_string(),
            PropertyValue::String(format!("F{:04}", i)),
        );
        attributes.insert("SEQ".to_string(), PropertyValue::Integer(i as i64));
        let geometry = Some(Geometry::Point(CorePoint::new(
            (i % 360) as f64 - 180.0,
            (i % 180) as f64 - 90.0,
        )));
        features.push(ShapefileFeature::new((i + 1) as i32, geometry, attributes));
    }

    {
        let mut writer = ShapefileWriter::new(&base_path, ShapeType::Point, schema)
            .expect("Failed to create writer for large feature count");
        writer
            .write_features(&features)
            .expect("Failed to write large feature count");
    }

    {
        let reader =
            ShapefileReader::open(&base_path).expect("Failed to open large count shapefile");
        let read_features = reader
            .read_features()
            .expect("Failed to read large count features");
        assert_eq!(read_features.len(), count, "Should have all 500 features");

        // Verify first and last
        assert_eq!(read_features[0].record_number, 1, "First record number");
        assert_eq!(
            read_features[count - 1].record_number,
            count as i32,
            "Last record number"
        );
    }

    cleanup_shapefile(&base_path);
}

/// Test 12: Multiple polygons with varying complexity
#[test]
fn test_cov_multiple_polygons() {
    let base_path = temp_base_path("multi_polys");
    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("NAME", 30)
        .expect("Failed to add NAME field for multi polygons")
        .add_numeric_field("AREA", 15, 3)
        .expect("Failed to add AREA field for multi polygons")
        .build();

    let mut features = Vec::new();

    // Triangle
    let exterior1 = CoreLineString::new(vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(10.0, 0.0),
        Coordinate::new_2d(5.0, 8.66),
        Coordinate::new_2d(0.0, 0.0),
    ])
    .expect("Failed to create triangle exterior");
    let poly1 = CorePolygon::new(exterior1, vec![]).expect("Failed to create triangle");

    let mut attrs1 = HashMap::new();
    attrs1.insert(
        "NAME".to_string(),
        PropertyValue::String("Triangle".to_string()),
    );
    attrs1.insert("AREA".to_string(), PropertyValue::Float(43.3));
    features.push(ShapefileFeature::new(
        1,
        Some(Geometry::Polygon(poly1)),
        attrs1,
    ));

    // Rectangle
    let exterior2 = CoreLineString::new(vec![
        Coordinate::new_2d(20.0, 0.0),
        Coordinate::new_2d(30.0, 0.0),
        Coordinate::new_2d(30.0, 5.0),
        Coordinate::new_2d(20.0, 5.0),
        Coordinate::new_2d(20.0, 0.0),
    ])
    .expect("Failed to create rectangle exterior");
    let poly2 = CorePolygon::new(exterior2, vec![]).expect("Failed to create rectangle");

    let mut attrs2 = HashMap::new();
    attrs2.insert(
        "NAME".to_string(),
        PropertyValue::String("Rectangle".to_string()),
    );
    attrs2.insert("AREA".to_string(), PropertyValue::Float(50.0));
    features.push(ShapefileFeature::new(
        2,
        Some(Geometry::Polygon(poly2)),
        attrs2,
    ));

    {
        let mut writer = ShapefileWriter::new(&base_path, ShapeType::Polygon, schema)
            .expect("Failed to create writer for multi polygons");
        writer
            .write_features(&features)
            .expect("Failed to write multi polygon features");
    }

    {
        let reader =
            ShapefileReader::open(&base_path).expect("Failed to open multi polygon shapefile");
        let read_features = reader
            .read_features()
            .expect("Failed to read multi polygon features");
        assert_eq!(read_features.len(), 2, "Should have 2 polygon features");

        for feat in &read_features {
            assert!(feat.geometry.is_some(), "Each feature should have geometry");
        }
    }

    cleanup_shapefile(&base_path);
}

// ============================================================
// Error handling tests
// ============================================================

/// Test 13: Missing .shp file
#[test]
fn test_cov_missing_shp_file() {
    let base_path = temp_base_path("missing_shp");
    let result = ShapefileReader::open(&base_path);
    assert!(result.is_err(), "Should fail when .shp file is missing");
}

/// Test 14: Missing .dbf file (create .shp but not .dbf)
#[test]
fn test_cov_missing_dbf_file() {
    let base_path = temp_base_path("missing_dbf");

    // Create just the .shp file to make it partially exist
    let shp_path = base_path.with_extension("shp");
    std::fs::write(&shp_path, b"fake shp content").expect("Should create fake .shp");

    let result = ShapefileReader::open(&base_path);
    assert!(result.is_err(), "Should fail when .dbf file is missing");

    let _ = std::fs::remove_file(shp_path);
}

/// Test 15: Empty features write error
#[test]
fn test_cov_empty_features_write_error() {
    let base_path = temp_base_path("empty_write_err");
    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("NAME", 30)
        .expect("Failed to add NAME field for empty write error")
        .build();

    let mut writer = ShapefileWriter::new(&base_path, ShapeType::Point, schema)
        .expect("Failed to create writer for empty write error");
    let result = writer.write_features(&[]);
    assert!(result.is_err(), "Should reject empty feature list");

    cleanup_shapefile(&base_path);
}

/// Test 16: Invalid shape type code
#[test]
fn test_cov_invalid_shape_type_code() {
    let result = ShapeType::from_code(999);
    assert!(result.is_err(), "Should reject invalid shape type code 999");

    let result = ShapeType::from_code(-1);
    assert!(result.is_err(), "Should reject negative shape type code");
}

/// Test 17: Field name too long
#[test]
fn test_cov_field_name_too_long() {
    use oxigdal_shapefile::dbf::{FieldDescriptor, FieldType};

    let result = FieldDescriptor::new("TOOLONGNAME1".to_string(), FieldType::Character, 50, 0);
    assert!(
        result.is_err(),
        "Should reject field names longer than 11 characters"
    );
}

// ============================================================
// Index and feature structure tests
// ============================================================

/// Test 18: Verify SHX index is consistent
#[test]
fn test_cov_shx_index_consistency() {
    let base_path = temp_base_path("shx_index");
    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("ID", 10)
        .expect("Failed to add ID field for SHX index test")
        .build();

    let count = 20;
    let mut features = Vec::new();
    for i in 0..count {
        let mut attributes = HashMap::new();
        attributes.insert("ID".to_string(), PropertyValue::String(format!("P{}", i)));
        let geometry = Some(Geometry::Point(CorePoint::new(i as f64, i as f64)));
        features.push(ShapefileFeature::new((i + 1) as i32, geometry, attributes));
    }

    {
        let mut writer = ShapefileWriter::new(&base_path, ShapeType::Point, schema)
            .expect("Failed to create writer for SHX index test");
        writer
            .write_features(&features)
            .expect("Failed to write SHX index test features");
    }

    {
        let reader = ShapefileReader::open(&base_path).expect("Failed to open SHX index shapefile");

        // Verify index entries exist
        let index_entries = reader.index_entries();
        assert!(
            index_entries.is_some(),
            "Should have index entries from .shx"
        );
        if let Some(entries) = index_entries {
            assert_eq!(entries.len(), count, "Should have {} index entries", count);

            // Verify offsets are monotonically increasing
            for i in 1..entries.len() {
                assert!(
                    entries[i].offset > entries[i - 1].offset,
                    "Index offsets should be monotonically increasing"
                );
            }
        }
    }

    cleanup_shapefile(&base_path);
}

/// Test 19: Feature to OxiGDAL conversion
#[test]
fn test_cov_feature_to_oxigdal_conversion() {
    let mut attributes = HashMap::new();
    attributes.insert(
        "name".to_string(),
        PropertyValue::String("Test".to_string()),
    );
    attributes.insert("value".to_string(), PropertyValue::Integer(42));

    let geometry = Some(Geometry::Point(CorePoint::new(10.0, 20.0)));
    let feature = ShapefileFeature::new(1, geometry, attributes);

    let oxigdal_feature = feature
        .to_oxigdal_feature()
        .expect("Should convert to OxiGDAL feature");

    assert!(
        matches!(oxigdal_feature.geometry, Some(Geometry::Point(_))),
        "Should have Point geometry after conversion"
    );
    assert_eq!(
        oxigdal_feature.properties.len(),
        2,
        "Should have 2 properties after conversion"
    );
}

/// Test 20: Feature without geometry conversion should fail
#[test]
fn test_cov_feature_no_geometry_conversion() {
    let attributes = HashMap::new();
    let feature = ShapefileFeature::new(1, None, attributes);

    let result = feature.to_oxigdal_feature();
    assert!(
        result.is_err(),
        "Should fail to convert feature without geometry"
    );
}

/// Test 21: Shape content length calculation
#[test]
fn test_cov_shape_content_length() {
    use oxigdal_shapefile::shp::Shape;
    use oxigdal_shapefile::shp::shapes::Point;

    let null_shape = Shape::Null;
    assert_eq!(null_shape.content_length(), 0, "Null shape content length");

    let point = Shape::Point(Point::new(10.0, 20.0));
    assert_eq!(
        point.content_length(),
        8,
        "Point content length (8 words = 16 bytes)"
    );

    let pointz = Shape::PointZ(oxigdal_shapefile::PointZ::new(10.0, 20.0, 30.0));
    assert_eq!(
        pointz.content_length(),
        16,
        "PointZ content length (16 words = 32 bytes)"
    );

    let pointm = Shape::PointM(oxigdal_shapefile::PointM::new(10.0, 20.0, 100.0));
    assert_eq!(
        pointm.content_length(),
        12,
        "PointM content length (12 words = 24 bytes)"
    );
}

/// Test 22: Schema builder with all field types
#[test]
fn test_cov_schema_builder_comprehensive() {
    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("CHAR", 50)
        .expect("Failed to add CHAR field")
        .add_numeric_field("INT", 10, 0)
        .expect("Failed to add INT field")
        .add_numeric_field("FLOAT", 15, 4)
        .expect("Failed to add FLOAT field")
        .add_logical_field("BOOL")
        .expect("Failed to add BOOL field")
        .add_date_field("DATE")
        .expect("Failed to add DATE field")
        .build();

    assert_eq!(schema.len(), 5, "Schema should have 5 fields");
    assert_eq!(schema[0].name, "CHAR");
    assert_eq!(schema[0].length, 50);
    assert_eq!(schema[1].name, "INT");
    assert_eq!(schema[1].decimal_count, 0);
    assert_eq!(schema[2].name, "FLOAT");
    assert_eq!(schema[2].decimal_count, 4);
    assert_eq!(schema[3].name, "BOOL");
    assert_eq!(schema[3].length, 1);
    assert_eq!(schema[4].name, "DATE");
    assert_eq!(schema[4].length, 8);
}

/// Test 23: Shape type properties
#[test]
fn test_cov_shape_type_properties_comprehensive() {
    // has_z checks
    assert!(!ShapeType::Point.has_z(), "Point should not have Z");
    assert!(!ShapeType::PolyLine.has_z(), "PolyLine should not have Z");
    assert!(!ShapeType::Polygon.has_z(), "Polygon should not have Z");
    assert!(ShapeType::PointZ.has_z(), "PointZ should have Z");
    assert!(ShapeType::PolyLineZ.has_z(), "PolyLineZ should have Z");
    assert!(ShapeType::PolygonZ.has_z(), "PolygonZ should have Z");
    assert!(ShapeType::MultiPointZ.has_z(), "MultiPointZ should have Z");
    assert!(ShapeType::MultiPatch.has_z(), "MultiPatch should have Z");

    // has_m checks
    assert!(!ShapeType::Point.has_m(), "Point should not have M");
    assert!(ShapeType::PointM.has_m(), "PointM should have M");
    assert!(ShapeType::PolyLineM.has_m(), "PolyLineM should have M");
    assert!(ShapeType::PolygonM.has_m(), "PolygonM should have M");
    assert!(ShapeType::PointZ.has_m(), "PointZ should also have M");

    // name checks
    assert_eq!(ShapeType::Point.name(), "Point");
    assert_eq!(ShapeType::PolyLine.name(), "PolyLine");
    assert_eq!(ShapeType::Polygon.name(), "Polygon");
    assert_eq!(ShapeType::MultiPoint.name(), "MultiPoint");
    assert_eq!(ShapeType::Null.name(), "Null");

    // code round-trip
    for code in &[0, 1, 3, 5, 8, 11, 13, 15, 18, 21, 23, 25, 28, 31] {
        let shape_type = ShapeType::from_code(*code)
            .unwrap_or_else(|_| panic!("Should parse shape type code {}", code));
        assert_eq!(
            shape_type.to_code(),
            *code,
            "Shape type code should round-trip for {}",
            code
        );
    }
}
