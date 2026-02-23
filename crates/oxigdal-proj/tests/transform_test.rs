//! Integration tests for coordinate transformations.
//!
//! These tests verify the accuracy and correctness of coordinate transformations
//! between various coordinate reference systems.

#![allow(clippy::expect_used)]

use approx::assert_relative_eq;
use oxigdal_proj::{BoundingBox, Coordinate, Coordinate3D, Crs, Transformer, transform_epsg};

/// Tolerance for coordinate comparisons (meters or degrees depending on CRS)
const TOLERANCE: f64 = 0.1;

/// Tolerance for Web Mercator coordinates (meters)
const WEB_MERCATOR_TOLERANCE: f64 = 1.0;

#[test]
fn test_identity_transform_wgs84() {
    let wgs84 = Crs::wgs84();
    let transformer = Transformer::new(wgs84.clone(), wgs84).expect("should create transformer");

    let coords = vec![
        Coordinate::from_lon_lat(0.0, 0.0),
        Coordinate::from_lon_lat(-122.4194, 37.7749), // San Francisco
        Coordinate::from_lon_lat(139.6917, 35.6895),  // Tokyo
        Coordinate::from_lon_lat(-0.1276, 51.5074),   // London
        Coordinate::from_lon_lat(-46.6333, -23.5505), // São Paulo
    ];

    for coord in coords {
        let result = transformer.transform(&coord).expect("should transform");
        assert_relative_eq!(result.x, coord.x, epsilon = 1e-10);
        assert_relative_eq!(result.y, coord.y, epsilon = 1e-10);
    }
}

#[test]
fn test_wgs84_to_web_mercator_equator() {
    let transformer = Transformer::from_epsg(4326, 3857).expect("should create");

    // Point on equator at prime meridian
    let coord = Coordinate::from_lon_lat(0.0, 0.0);
    let result = transformer.transform(&coord).expect("should transform");

    assert_relative_eq!(result.x, 0.0, epsilon = WEB_MERCATOR_TOLERANCE);
    assert_relative_eq!(result.y, 0.0, epsilon = WEB_MERCATOR_TOLERANCE);
}

#[test]
fn test_wgs84_to_web_mercator_london() {
    let transformer = Transformer::from_epsg(4326, 3857).expect("should create");

    // London: 0°, 51.5°N
    let london = Coordinate::from_lon_lat(0.0, 51.5);
    let result = transformer.transform(&london).expect("should transform");

    // X should be near 0 (prime meridian)
    assert_relative_eq!(result.x, 0.0, epsilon = WEB_MERCATOR_TOLERANCE);

    // Y should be approximately 6,692,000 meters (51.5°N in Web Mercator)
    // This is a well-known value
    assert!(result.y > 6_600_000.0 && result.y < 6_800_000.0);
}

#[test]
fn test_wgs84_to_web_mercator_san_francisco() {
    let transformer = Transformer::from_epsg(4326, 3857).expect("should create");

    // San Francisco: -122.4194°, 37.7749°N
    let sf = Coordinate::from_lon_lat(-122.4194, 37.7749);
    let result = transformer.transform(&sf).expect("should transform");

    // X should be negative (west of prime meridian)
    assert!(result.x < -13_600_000.0 && result.x > -13_700_000.0);

    // Y should be positive (northern hemisphere)
    assert!(result.y > 4_500_000.0 && result.y < 4_600_000.0);
}

#[test]
fn test_web_mercator_to_wgs84_roundtrip() {
    let wgs84 = Crs::from_epsg(4326).expect("should create");
    let web_mercator = Crs::from_epsg(3857).expect("should create");

    let forward = Transformer::new(wgs84.clone(), web_mercator.clone()).expect("should create");
    let backward = Transformer::new(web_mercator, wgs84).expect("should create");

    let test_points = vec![
        Coordinate::from_lon_lat(0.0, 0.0),
        Coordinate::from_lon_lat(10.0, 20.0),
        Coordinate::from_lon_lat(-50.0, 30.0),
        Coordinate::from_lon_lat(100.0, -45.0),
    ];

    for original in test_points {
        let transformed = forward.transform(&original).expect("should transform");
        let roundtrip = backward
            .transform(&transformed)
            .expect("should transform back");

        assert_relative_eq!(roundtrip.x, original.x, epsilon = TOLERANCE);
        assert_relative_eq!(roundtrip.y, original.y, epsilon = TOLERANCE);
    }
}

#[test]
fn test_batch_transformation() {
    let transformer = Transformer::from_epsg(4326, 4326).expect("should create");

    let coords = vec![
        Coordinate::from_lon_lat(0.0, 0.0),
        Coordinate::from_lon_lat(10.0, 20.0),
        Coordinate::from_lon_lat(-30.0, 40.0),
        Coordinate::from_lon_lat(50.0, -60.0),
    ];

    let results = transformer
        .transform_batch(&coords)
        .expect("should transform");

    assert_eq!(results.len(), coords.len());
    for (original, result) in coords.iter().zip(results.iter()) {
        assert_relative_eq!(result.x, original.x, epsilon = 1e-10);
        assert_relative_eq!(result.y, original.y, epsilon = 1e-10);
    }
}

#[test]
fn test_bounding_box_transformation() {
    let transformer = Transformer::from_epsg(4326, 3857).expect("should create");

    // Small bounding box around London
    let bbox = BoundingBox::new(-1.0, 51.0, 1.0, 52.0).expect("valid bbox");

    let transformed = transformer.transform_bbox(&bbox).expect("should transform");

    // Verify that the transformed bbox contains reasonable values
    assert!(transformed.min_x < 0.0);
    assert!(transformed.max_x > 0.0);
    assert!(transformed.min_y > 6_000_000.0);
    assert!(transformed.max_y > transformed.min_y);
}

#[test]
fn test_bounding_box_corner_transformation() {
    let wgs84 = Crs::from_epsg(4326).expect("should create");
    let transformer = Transformer::new(wgs84.clone(), wgs84).expect("should create");

    let bbox = BoundingBox::new(0.0, 0.0, 10.0, 10.0).expect("valid bbox");
    let corners = bbox.corners();

    assert_eq!(corners.len(), 4);
    assert_eq!(corners[0], Coordinate::new(0.0, 0.0));
    assert_eq!(corners[1], Coordinate::new(10.0, 0.0));
    assert_eq!(corners[2], Coordinate::new(10.0, 10.0));
    assert_eq!(corners[3], Coordinate::new(0.0, 10.0));

    // Transform corners
    for corner in &corners {
        let result = transformer.transform(corner).expect("should transform");
        assert_relative_eq!(result.x, corner.x, epsilon = 1e-10);
        assert_relative_eq!(result.y, corner.y, epsilon = 1e-10);
    }
}

#[test]
fn test_3d_coordinate_transformation() {
    let wgs84 = Crs::from_epsg(4326).expect("should create");
    let transformer = Transformer::new(wgs84.clone(), wgs84).expect("should create");

    let coord_3d = Coordinate3D::new(10.0, 20.0, 100.0); // 100m elevation
    let result = transformer
        .transform_3d(&coord_3d)
        .expect("should transform");

    assert_relative_eq!(result.x, 10.0, epsilon = 1e-10);
    assert_relative_eq!(result.y, 20.0, epsilon = 1e-10);
    assert_relative_eq!(result.z, 100.0, epsilon = 1e-10);
}

#[test]
fn test_utm_zone_transformation() {
    // Transform from WGS84 to UTM zone 33N (covers parts of Europe)
    let transformer = Transformer::from_epsg(4326, 32633).expect("should create");

    // Point in the middle of UTM zone 33N (around 15°E, 50°N)
    let coord = Coordinate::from_lon_lat(15.0, 50.0);
    let result = transformer.transform(&coord).expect("should transform");

    // UTM coordinates should be in meters
    // X (Easting) should be around 500,000 (central meridian)
    assert!(result.x > 450_000.0 && result.x < 550_000.0);

    // Y (Northing) should be around 5,500,000 for 50°N
    assert!(result.y > 5_400_000.0 && result.y < 5_600_000.0);
}

#[test]
fn test_southern_hemisphere_utm() {
    // Transform from WGS84 to UTM zone 56S (covers parts of Australia)
    let transformer = Transformer::from_epsg(4326, 32756).expect("should create");

    // Point in Australia (around 150°E, -30°S)
    let coord = Coordinate::from_lon_lat(150.0, -30.0);
    let result = transformer.transform(&coord).expect("should transform");

    // UTM coordinates should be in meters
    assert!(result.x > 200_000.0 && result.x < 800_000.0);
    assert!(result.y > 6_000_000.0 && result.y < 7_000_000.0);
}

#[test]
fn test_convenience_transform_function() {
    let coord = Coordinate::from_lon_lat(0.0, 0.0);

    // Identity transformation
    let result = transform_epsg(&coord, 4326, 4326).expect("should transform");
    assert_relative_eq!(result.x, 0.0, epsilon = 1e-10);
    assert_relative_eq!(result.y, 0.0, epsilon = 1e-10);
}

#[test]
fn test_invalid_coordinate_transformation() {
    let transformer = Transformer::from_epsg(4326, 3857).expect("should create");

    // NaN coordinate
    let invalid = Coordinate::new(f64::NAN, 0.0);
    let result = transformer.transform(&invalid);
    assert!(result.is_err());

    // Infinite coordinate
    let infinite = Coordinate::new(f64::INFINITY, 0.0);
    let result = transformer.transform(&infinite);
    assert!(result.is_err());
}

#[test]
fn test_crs_equivalence() {
    let wgs84_1 = Crs::from_epsg(4326).expect("should create");
    let wgs84_2 = Crs::from_epsg(4326).expect("should create");
    let web_mercator = Crs::from_epsg(3857).expect("should create");

    assert!(wgs84_1.is_equivalent(&wgs84_2));
    assert!(!wgs84_1.is_equivalent(&web_mercator));
}

#[test]
fn test_multiple_crs_types() {
    // Geographic CRS
    let wgs84 = Crs::wgs84();
    assert!(wgs84.is_geographic());
    assert!(!wgs84.is_projected());

    // Projected CRS
    let web_mercator = Crs::web_mercator();
    assert!(!web_mercator.is_geographic());
    assert!(web_mercator.is_projected());

    // NAD83
    let nad83 = Crs::nad83();
    assert!(nad83.is_geographic());

    // ETRS89
    let etrs89 = Crs::etrs89();
    assert!(etrs89.is_geographic());
}

#[test]
fn test_proj_string_conversion() {
    let wgs84 = Crs::wgs84();
    let proj_string = wgs84.to_proj_string().expect("should convert");

    assert!(proj_string.contains("+proj=longlat"));
    assert!(proj_string.contains("+datum=WGS84"));
}

#[test]
fn test_coordinate_validation() {
    // Valid geographic coordinate
    let valid = Coordinate::from_lon_lat(10.0, 20.0);
    assert!(valid.validate_geographic().is_ok());

    // Invalid longitude (out of range)
    let invalid_lon = Coordinate::from_lon_lat(200.0, 0.0);
    assert!(invalid_lon.validate_geographic().is_err());

    // Invalid latitude (out of range)
    let invalid_lat = Coordinate::from_lon_lat(0.0, 100.0);
    assert!(invalid_lat.validate_geographic().is_err());

    // Edge cases (valid)
    let edge_lon = Coordinate::from_lon_lat(180.0, 0.0);
    assert!(edge_lon.validate_geographic().is_ok());

    let edge_lat = Coordinate::from_lon_lat(0.0, 90.0);
    assert!(edge_lat.validate_geographic().is_ok());
}

#[test]
fn test_bounding_box_operations() {
    let mut bbox = BoundingBox::new(0.0, 0.0, 10.0, 10.0).expect("valid bbox");

    // Test contains
    assert!(bbox.contains(&Coordinate::new(5.0, 5.0)));
    assert!(bbox.contains(&Coordinate::new(0.0, 0.0)));
    assert!(bbox.contains(&Coordinate::new(10.0, 10.0)));
    assert!(!bbox.contains(&Coordinate::new(-1.0, 5.0)));
    assert!(!bbox.contains(&Coordinate::new(11.0, 5.0)));

    // Test expand
    bbox.expand_to_include(&Coordinate::new(15.0, 5.0));
    assert_eq!(bbox.max_x, 15.0);
    assert!(bbox.contains(&Coordinate::new(15.0, 5.0)));

    bbox.expand_to_include(&Coordinate::new(5.0, -5.0));
    assert_eq!(bbox.min_y, -5.0);
    assert!(bbox.contains(&Coordinate::new(5.0, -5.0)));

    // Test dimensions
    assert_eq!(bbox.width(), 15.0);
    assert_eq!(bbox.height(), 15.0);

    // Test center
    let center = bbox.center();
    assert_eq!(center.x, 7.5);
    assert_eq!(center.y, 2.5);
}

#[test]
fn test_epsg_database_coverage() {
    use oxigdal_proj::{available_epsg_codes, contains_epsg};

    let codes = available_epsg_codes();

    // Should have at least 100 codes
    assert!(codes.len() >= 100);

    // Check for essential codes
    assert!(contains_epsg(4326)); // WGS84
    assert!(contains_epsg(3857)); // Web Mercator
    assert!(contains_epsg(4269)); // NAD83
    assert!(contains_epsg(4258)); // ETRS89

    // Check UTM zones
    assert!(contains_epsg(32601)); // UTM 1N
    assert!(contains_epsg(32660)); // UTM 60N
    assert!(contains_epsg(32701)); // UTM 1S
    assert!(contains_epsg(32760)); // UTM 60S
}

#[test]
fn test_world_coverage() {
    // Test that we can transform coordinates from different parts of the world
    let transformer = Transformer::from_epsg(4326, 3857).expect("should create");

    let world_cities = vec![
        ("New York", -74.0060, 40.7128),
        ("London", -0.1276, 51.5074),
        ("Tokyo", 139.6917, 35.6895),
        ("Sydney", 151.2093, -33.8688),
        ("São Paulo", -46.6333, -23.5505),
        ("Paris", 2.3522, 48.8566),
        ("Mumbai", 72.8777, 19.0760),
        ("Moscow", 37.6173, 55.7558),
    ];

    for (name, lon, lat) in world_cities {
        let coord = Coordinate::from_lon_lat(lon, lat);
        let result = transformer.transform(&coord);

        assert!(
            result.is_ok(),
            "Failed to transform {} ({}, {})",
            name,
            lon,
            lat
        );

        let result = result.expect("checked above");
        assert!(
            result.is_valid(),
            "{} produced invalid result: ({}, {})",
            name,
            result.x,
            result.y
        );
    }
}

#[test]
fn test_polar_regions() {
    // Web Mercator is not defined at the poles, but should work near them
    let transformer = Transformer::from_epsg(4326, 3857).expect("should create");

    // Point near North Pole (but within Web Mercator limits: < 85.06°)
    let near_north = Coordinate::from_lon_lat(0.0, 80.0);
    let result = transformer.transform(&near_north);
    assert!(result.is_ok());

    // Point near South Pole (but within Web Mercator limits: > -85.06°)
    let near_south = Coordinate::from_lon_lat(0.0, -80.0);
    let result = transformer.transform(&near_south);
    assert!(result.is_ok());
}

#[test]
fn test_antimeridian_handling() {
    let transformer = Transformer::from_epsg(4326, 4326).expect("should create");

    // Points near the antimeridian (180°/-180°)
    let west = Coordinate::from_lon_lat(179.9, 0.0);
    let east = Coordinate::from_lon_lat(-179.9, 0.0);

    let result_west = transformer.transform(&west).expect("should transform");
    let result_east = transformer.transform(&east).expect("should transform");

    assert_relative_eq!(result_west.x, 179.9, epsilon = TOLERANCE);
    assert_relative_eq!(result_east.x, -179.9, epsilon = TOLERANCE);
}
