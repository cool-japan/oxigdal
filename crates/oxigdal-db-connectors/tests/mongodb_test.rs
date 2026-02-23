//! MongoDB connector tests.
#![allow(clippy::panic)]

use geo_types::{Geometry, point, polygon};
use mongodb::bson::doc;
use oxigdal_db_connectors::mongodb::{MongoDbConfig, geojson_to_geometry, geometry_to_geojson};

#[test]
fn test_mongodb_config_default() {
    let config = MongoDbConfig::default();
    assert_eq!(config.database, "gis");
}

#[test]
fn test_point_to_geojson() {
    let point = Geometry::Point(point!(x: 1.0, y: 2.0));
    let doc = geometry_to_geojson(&point).expect("Failed to convert");

    assert_eq!(doc.get_str("type").ok(), Some("Point"));
    let coords = doc.get_array("coordinates").expect("No coordinates");
    assert_eq!(coords.len(), 2);
}

#[test]
fn test_geojson_to_point() {
    let doc = doc! {
        "type": "Point",
        "coordinates": [1.0, 2.0]
    };

    let geom = geojson_to_geometry(&doc).expect("Failed to parse");
    match geom {
        Geometry::Point(p) => {
            assert_eq!(p.x(), 1.0);
            assert_eq!(p.y(), 2.0);
        }
        _ => panic!("Expected Point geometry"),
    }
}

#[test]
fn test_polygon_to_geojson() {
    let poly = polygon![
        (x: 0.0, y: 0.0),
        (x: 1.0, y: 0.0),
        (x: 1.0, y: 1.0),
        (x: 0.0, y: 1.0),
        (x: 0.0, y: 0.0),
    ];

    let geom = Geometry::Polygon(poly);
    let doc = geometry_to_geojson(&geom).expect("Failed to convert");

    assert_eq!(doc.get_str("type").ok(), Some("Polygon"));
    assert!(doc.get_array("coordinates").is_ok());
}

#[test]
fn test_geojson_to_polygon() {
    let doc = doc! {
        "type": "Polygon",
        "coordinates": [
            [
                [0.0, 0.0],
                [1.0, 0.0],
                [1.0, 1.0],
                [0.0, 1.0],
                [0.0, 0.0]
            ]
        ]
    };

    let geom = geojson_to_geometry(&doc).expect("Failed to parse");
    match geom {
        Geometry::Polygon(poly) => {
            assert_eq!(poly.exterior().coords().count(), 5);
        }
        _ => panic!("Expected Polygon geometry"),
    }
}

// Note: Integration tests with actual MongoDB connection would require a running MongoDB instance
