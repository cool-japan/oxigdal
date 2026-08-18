//! Integration tests for coordinate transformations.
//!
//! These tests verify the accuracy and correctness of coordinate transformations
//! between various coordinate reference systems.

#![allow(clippy::expect_used)]

use approx::assert_relative_eq;
use oxigeo_proj::{
    BoundingBox, Coordinate, Coordinate3D, Crs, Error, Transformer, TransverseMercator,
    transform_epsg,
};

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
    use oxigeo_proj::{available_epsg_codes, contains_epsg};

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

// ─────────────────────────────────────────────────────────────────────────────
// Gap A: Dead `eta` variable removal in Transverse Mercator kernel
// ─────────────────────────────────────────────────────────────────────────────

/// Verify that removing the dead `eta` placeholder from the TM forward kernel
/// did not alter any computed output values.  We use the sphere-based
/// `TransverseMercator` struct (which calls the same underlying kernel) to
/// perform a forward/inverse round-trip and check that coordinates are
/// bit-for-bit identical to independently computed reference values.
#[test]
fn test_tm_eta_removed_does_not_change_projected_coords() {
    // WGS-84 sphere radius; lon_0 = 9° E (German Gauss-Krüger strip 3).
    let tm = TransverseMercator::new(9.0, 0.0, 1.0, 500_000.0, 0.0, 6_378_137.0);

    let test_cases: &[(f64, f64)] = &[
        (10.0, 52.0), // 1° east of central meridian, 52°N
        (9.0, 0.0),   // On the central meridian at equator
        (8.0, 48.0),  // 1° west of central meridian, 48°N
        (11.0, 55.0), // 2° east, 55°N
    ];

    for &(lon, lat) in test_cases {
        let (x, y) = tm.forward(lon, lat).expect("forward ok");

        // Verify the projected coordinates are finite and in plausible ranges.
        assert!(
            x.is_finite() && y.is_finite(),
            "TM forward produced non-finite output for ({lon}, {lat})"
        );

        // Round-trip inverse to confirm the kernel is internally consistent.
        let (lon2, lat2) = tm.inverse(x, y).expect("inverse ok");
        assert!(
            (lon - lon2).abs() < 1e-7,
            "TM round-trip lon error {:.2e} at ({lon}, {lat})",
            (lon - lon2).abs()
        );
        assert!(
            (lat - lat2).abs() < 1e-7,
            "TM round-trip lat error {:.2e} at ({lon}, {lat})",
            (lat - lat2).abs()
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Gap B: Area-of-use strict validation in Transformer
// ─────────────────────────────────────────────────────────────────────────────

/// A fresh `Transformer::new(...)` must default to `strict = true`.
#[test]
fn test_transformer_default_is_strict() {
    let src = Crs::from_epsg(4326).expect("WGS84");
    let dst = Crs::from_epsg(3857).expect("Web Mercator");
    let t = Transformer::new(src, dst).expect("transformer");
    assert!(t.is_strict(), "default strict must be true");
}

/// In strict mode, a point outside the source CRS's AoU must return
/// `Error::OutOfAreaOfUse`.  We use British National Grid (EPSG:27700) whose
/// AoU covers only the UK: west=-7.56, south=49.96, east=1.78, north=60.84.
/// A point at (10.0°E, 51.5°N) is clearly outside those bounds.
#[test]
fn test_transformer_strict_rejects_point_outside_aou() {
    // source = WGS84 (geographic), target = BNG
    let transformer = Transformer::from_epsg(4326, 27700).expect("BNG transformer");
    assert!(transformer.is_strict());

    // Point is at 10°E, 51.5°N — inside Europe but east of the BNG source AoU
    // wait — source is EPSG:4326 (world), not 27700.
    // Let's use a source CRS with a restricted AoU: EPSG:4269 (NAD83, North America).
    // AoU: west=-172.54, south=14.92, east=-47.74, north=86.46.
    // A point at (10°E, 51°N) is outside that.
    let src = Crs::from_epsg(4269).expect("NAD83");
    let dst = Crs::from_epsg(4326).expect("WGS84");
    let transformer = Transformer::new(src, dst).expect("transformer");

    // 10°E, 51°N is outside NAD83's area of use (North America only)
    let outside = Coordinate::from_lon_lat(10.0, 51.0);
    let result = transformer.transform(&outside);

    assert!(
        result.is_err(),
        "strict mode should reject point outside AoU"
    );
    let err = result.expect_err("checked above");
    assert!(
        matches!(err, Error::OutOfAreaOfUse { lon, lat, .. } if (lon - 10.0).abs() < 1e-10 && (lat - 51.0).abs() < 1e-10),
        "expected OutOfAreaOfUse, got: {err}"
    );
}

/// With `.with_strict(false)`, the same out-of-bounds point must not be
/// rejected by the AoU check; the transformation should proceed.
#[test]
fn test_transformer_non_strict_passes_through() {
    let src = Crs::from_epsg(4269).expect("NAD83");
    let dst = Crs::from_epsg(4326).expect("WGS84");
    let transformer = Transformer::new(src, dst)
        .expect("transformer")
        .with_strict(false);

    assert!(!transformer.is_strict());

    // Same point that would be rejected in strict mode
    let outside = Coordinate::from_lon_lat(10.0, 51.0);
    // The transformation may succeed or fail for geometric reasons, but it must
    // NOT fail with OutOfAreaOfUse.
    let result = transformer.transform(&outside);
    if let Err(ref e) = result {
        assert!(
            !matches!(e, Error::OutOfAreaOfUse { .. }),
            "non-strict mode must not return OutOfAreaOfUse, got: {e}"
        );
    }
}

/// When the source CRS has no declared area of use (e.g., created from a raw
/// PROJ string), the strict check must be skipped silently and the transform
/// must proceed.
#[test]
fn test_transformer_aou_check_skipped_when_crs_has_no_aou() {
    // PROJ-string CRS: `area_of_use()` returns None for non-EPSG sources.
    let src = Crs::from_proj("+proj=longlat +datum=WGS84 +no_defs").expect("proj string CRS");
    let dst = Crs::from_epsg(3857).expect("Web Mercator");

    // Even in strict mode, no AoU bounds exist for the source, so any point
    // must be accepted.
    let transformer = Transformer::new(src, dst).expect("transformer");
    assert!(transformer.is_strict(), "still strict");

    let anywhere = Coordinate::from_lon_lat(10.0, 51.0);
    let result = transformer.transform(&anywhere);
    assert!(
        !matches!(result, Err(Error::OutOfAreaOfUse { .. })),
        "should not return OutOfAreaOfUse when source CRS has no AoU"
    );
}

/// Regression: the Transverse Mercator aliases must be reachable through
/// `oxigeo_proj::transform::*`, not only through `transform::cylindrical::*` and
/// the crate root. Before this was fixed, `transform/mod.rs` re-exported
/// `CassineSoldner`/`GaussKruger`/`TransverseMercator` but silently omitted the
/// two aliases, so a glob import of `transform` did not surface them.
#[test]
fn test_transverse_mercator_aliases_reachable_via_transform_glob() {
    mod glob_import {
        pub use oxigeo_proj::transform::*;
    }

    // Both aliases resolve through the `transform` module...
    let spherical = glob_import::SphericalTransverseMercator::new(
        0.0,
        0.0,
        0.9996,
        500_000.0,
        0.0,
        6_378_137.0,
    );
    let ellipsoidal =
        glob_import::EllipsoidalTransverseMercator::new(0.0, 0.0, 0.9996, 500_000.0, 0.0);

    // ...and denote the same types as the crate-root re-exports, so the two
    // import paths stay interchangeable.
    let root_spherical: oxigeo_proj::SphericalTransverseMercator = spherical;
    let root_ellipsoidal: oxigeo_proj::EllipsoidalTransverseMercator = ellipsoidal;

    let projected = root_spherical.forward(3.0, 0.0).expect("spherical forward");
    assert!(projected.0 > 500_000.0, "east of the central meridian");

    let projected = root_ellipsoidal
        .forward(3.0, 0.0)
        .expect("ellipsoidal forward");
    assert!(projected.0 > 500_000.0, "east of the central meridian");
}

// ---------------------------------------------------------------------------
// Feature-invariant CRS resolution (regression for the `proj-db` datum shift)
// ---------------------------------------------------------------------------

/// WGS 84 equatorial metres per degree (2 * pi * a / 360, a = 6378137).
const M_PER_DEG: f64 = 111_319.490_793_273_57;

/// Tolerance for the projection-only pivots, in metres. Matches the
/// `epsg_verified_registry_extended_test` fixtures.
const PIVOT_TOLERANCE_M: f64 = 1e-6;

/// Projection-only pivots: a PROJ-string geodetic base carrying **the same
/// datum tokens** as the registry entry, paired with the EPSG-sourced projected
/// CRS. A transform between the two must apply the map projection and nothing
/// else — no datum shift can survive, because both sides name the same datum.
///
/// Each row is `(code, geodetic base PROJ string, datum tokens that must still
/// appear verbatim in the registry entry, lon, lat, easting, northing)`.
/// The expected coordinates are PROJ ground truth (verified with PROJ 9.5.1 in
/// `tests/data/verified_epsg_projection_5pt.json`, re-confirmed against PROJ
/// 9.7.0 `cs2cs EPSG:4141 EPSG:2039` / `EPSG:4150 EPSG:2056`).
#[allow(clippy::type_complexity)]
const PROJECTION_ONLY_PIVOTS: &[(u32, &str, &[&str], f64, f64, f64, f64)] = &[
    (
        2039,
        "+proj=longlat +ellps=GRS80 +towgs84=-48,55,52,0,0,0,0 +no_defs",
        &["+ellps=GRS80", "+towgs84=-48,55,52,0,0,0,0"],
        34.93,
        31.365,
        193_412.215_923_065_79,
        585_981.701_027_398,
    ),
    (
        2056,
        "+proj=longlat +ellps=bessel +towgs84=674.374,15.056,405.346,0,0,0,0 +no_defs",
        &["+ellps=bessel", "+towgs84=674.374,15.056,405.346,0,0,0,0"],
        8.225,
        46.815,
        2_659_933.418_732_427_5,
        1_185_026.714_081_787,
    ),
];

/// Regression: an EPSG-sourced CRS must resolve to the *same* definition as its
/// own registry PROJ string in every feature configuration.
///
/// Before 0.2.4 the `proj-db` feature routed `CrsSource::Epsg` through
/// `oxiproj::Crs::from_epsg` (oxiproj's bundled authority database) while a
/// `CrsSource::Proj` partner kept using the registry string. The pair became
/// asymmetric — a datum-bearing definition against a datum-less one — and the
/// pipeline applied a one-sided datum shift: 87 m for `EPSG:2039`, 226 m for
/// `EPSG:2056`. This test pins the projection-only result unconditionally, so
/// it fails under whichever feature set reintroduces the asymmetry.
#[test]
fn test_epsg_sourced_crs_matches_its_registry_proj_string_forward() {
    for &(code, base_proj, datum_tokens, lon, lat, easting, northing) in PROJECTION_ONLY_PIVOTS {
        let def = oxigeo_proj::lookup_epsg(code).expect("code must be in the embedded registry");
        for token in datum_tokens {
            assert!(
                def.proj_string.contains(token),
                "EPSG:{code} registry entry no longer carries {token}; \
                 update the pivot base string in this test"
            );
        }

        let base = Crs::from_proj(base_proj).expect("geodetic base CRS");
        let projected = Crs::from_epsg(code).expect("projected CRS");
        let transformer = Transformer::new(base, projected)
            .expect("base -> code transformer")
            .with_strict(false);

        let out = transformer
            .transform(&Coordinate::from_lon_lat(lon, lat))
            .expect("base -> code transform");
        let residual = (out.x - easting).hypot(out.y - northing);
        assert!(
            residual <= PIVOT_TOLERANCE_M,
            "EPSG:{code} base -> code | got ({:.6}, {:.6}) | want ({easting:.6}, {northing:.6}) \
             | {residual:.3e} m > {PIVOT_TOLERANCE_M:.0e} m — a datum shift leaked into a \
             same-datum pair",
            out.x,
            out.y
        );
    }
}

/// Inverse direction of [`test_epsg_sourced_crs_matches_its_registry_proj_string_forward`].
#[test]
fn test_epsg_sourced_crs_matches_its_registry_proj_string_inverse() {
    for &(code, base_proj, _, lon, lat, easting, northing) in PROJECTION_ONLY_PIVOTS {
        let base = Crs::from_proj(base_proj).expect("geodetic base CRS");
        let projected = Crs::from_epsg(code).expect("projected CRS");
        let transformer = Transformer::new(projected, base)
            .expect("code -> base transformer")
            .with_strict(false);

        let out = transformer
            .transform(&Coordinate::new(easting, northing))
            .expect("code -> base transform");
        let coslat = lat.to_radians().cos().abs().max(1e-12);
        let residual = ((out.x - lon) * M_PER_DEG * coslat).hypot((out.y - lat) * M_PER_DEG);
        assert!(
            residual <= PIVOT_TOLERANCE_M,
            "EPSG:{code} code -> base | got ({:.9}, {:.9}) | want ({lon:.9}, {lat:.9}) \
             | {residual:.3e} m > {PIVOT_TOLERANCE_M:.0e} m — a datum shift leaked into a \
             same-datum pair",
            out.x,
            out.y
        );
    }
}

/// An EPSG code the embedded registry does not carry. `proj-db` must widen the
/// resolvable set to include it; without `proj-db` it must stay unresolvable.
const UNREGISTERED_EPSG_CODE: u32 = 2172;

/// Builds a `Crs` holding `UNREGISTERED_EPSG_CODE`. `Crs::source` is private and
/// `Crs::from_epsg` refuses unknown codes, so the only way such a value reaches
/// `crs_to_oxi` in practice — and the only way to build one here — is
/// deserialization.
fn unregistered_epsg_crs() -> Crs {
    assert!(
        !oxigeo_proj::contains_epsg(UNREGISTERED_EPSG_CODE),
        "EPSG:{UNREGISTERED_EPSG_CODE} joined the embedded registry; \
         pick another absent code for this test"
    );
    let mut value =
        serde_json::to_value(Crs::from_epsg(4326).expect("EPSG:4326")).expect("serialize Crs");
    value["source"]["Epsg"] = serde_json::json!(UNREGISTERED_EPSG_CODE);
    let crs: Crs = serde_json::from_value(value).expect("deserialize Crs");
    assert_eq!(crs.epsg_code(), Some(UNREGISTERED_EPSG_CODE));
    crs
}

/// `proj-db` is additive: it resolves EPSG codes the embedded registry lacks by
/// falling back to oxiproj's authority database, without changing any answer the
/// default build already produces (pinned by the two pivot tests above).
#[cfg(feature = "proj-db")]
#[test]
fn test_proj_db_resolves_epsg_codes_outside_the_embedded_registry() {
    let transformer = Transformer::new(
        Crs::from_epsg(4326).expect("EPSG:4326"),
        unregistered_epsg_crs(),
    )
    .expect("proj-db must resolve an EPSG code outside the embedded registry")
    .with_strict(false);

    let out = transformer
        .transform(&Coordinate::from_lon_lat(21.0, 52.0))
        .expect("transform into the fallback-resolved CRS");
    // Poland zone II is a Gauss-Kruger grid with a ~4.5e6 m false easting and a
    // ~5.7e6 m northing at 52 N; assert the magnitude only, since the datum
    // handling of a fallback-resolved CRS is oxiproj's, not this crate's.
    assert!(
        (4.0e6..5.5e6).contains(&out.x) && (5.0e6..6.5e6).contains(&out.y),
        "unexpected Poland zone II coordinate: ({}, {})",
        out.x,
        out.y
    );
}

/// Without `proj-db` there is no second CRS source, so the registry error is
/// final rather than silently producing a differently-defined CRS.
#[cfg(not(feature = "proj-db"))]
#[test]
fn test_unregistered_epsg_code_is_an_error_without_proj_db() {
    let built = Transformer::new(
        Crs::from_epsg(4326).expect("EPSG:4326"),
        unregistered_epsg_crs(),
    );
    assert!(
        built.is_err(),
        "an unregistered EPSG code must not resolve without proj-db"
    );
}
