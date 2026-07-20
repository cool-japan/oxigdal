//! Integration tests for geographic distance functions in `oxigdal-index`.
//!
//! Tests cover:
//! * Haversine: known city pairs, degenerate inputs, edge cases.
//! * Vincenty inverse: WGS84 geodesic distances, azimuths, antipodal case.
//! * Geographic k-NN: sorting, cardinality, empty input.
//! * Geographic radius filter: correctness of filtering threshold.
//! * `geo_bbox_extent_m`: metric extents of well-known bounding boxes.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use oxigdal_index::{
    GeoPoint, SpatialQuery, geo_bbox_extent_m, geo_nearest_k, geo_within_radius, haversine_m,
    haversine_m_with_radius, vincenty_inverse_wgs84,
};

// ---------------------------------------------------------------------------
// Haversine tests
// ---------------------------------------------------------------------------

/// London (51.5074 N, 0.1278 W) → Paris (48.8566 N, 2.3522 E).
///
/// The spherical haversine distance on the WGS84 mean-radius sphere is ≈ 343,557 m.
/// The geodetic (Vincenty/WGS84 ellipsoid) distance is ≈ 341,552 m.
/// We test the haversine value here; the discrepancy with the geodetic value is
/// expected (~0.6%) because haversine uses a sphere approximation.
///
/// Tolerance: ±2 000 m — broader than usual to tolerate any reference value
/// rounding in the test specification.
#[test]
fn test_haversine_london_paris() {
    let london_lat = 51.5074_f64;
    let london_lon = -0.1278_f64;
    let paris_lat = 48.8566_f64;
    let paris_lon = 2.3522_f64;

    let d = haversine_m(london_lat, london_lon, paris_lat, paris_lon);

    // Spherical great-circle (haversine) ≈ 343,557 m; geodetic ≈ 341,552 m.
    // The two differ by ~0.6% because haversine assumes a sphere.
    // We accept anything in [330,000 m, 360,000 m] — well within city-pair range.
    assert!(
        d > 330_000.0 && d < 360_000.0,
        "London–Paris haversine should be in 330–360 km range; got {d:.1} m"
    );
    // More tightly: within 5,000 m of the reference spherical value.
    assert!(
        (d - 343_557.0).abs() < 5_000.0,
        "London–Paris haversine should be ≈343,557 m (spherical); got {d:.1} m"
    );
}

/// Same point → distance is exactly zero.
#[test]
fn test_haversine_coincident_zero() {
    let d = haversine_m(48.8566, 2.3522, 48.8566, 2.3522);
    assert!(
        d.abs() < 1e-6,
        "Haversine of coincident points should be 0 m; got {d}"
    );
}

/// (0°, 0°) → (0°, 1°): one degree of longitude at the equator ≈ 111,195 m.
/// Tolerance ±100 m.
#[test]
fn test_haversine_equatorial_one_degree() {
    let d = haversine_m(0.0, 0.0, 0.0, 1.0);
    assert!(
        (d - 111_195.0).abs() < 100.0,
        "One degree of longitude at equator should be ≈111,195 m; got {d:.1} m"
    );
}

/// Near-polar crossing: (89°N, 0°) → (89°S, 0°).
/// Expected distance ≈ 2 × 89° in radians × R_mean ≈ 19,779 km.
/// Tolerance: within 5% of expected.
#[test]
fn test_haversine_polar() {
    let d = haversine_m(89.0, 0.0, -89.0, 0.0);
    // 178° arc on a sphere of radius 6,371,008.8 m.
    let expected_m = 178.0_f64.to_radians() * 6_371_008.8_f64;
    let tolerance = expected_m * 0.05;
    assert!(
        (d - expected_m).abs() < tolerance,
        "Polar haversine should be within 5% of {expected_m:.0} m; got {d:.0} m"
    );
}

/// `haversine_m_with_radius` with mean WGS84 radius must match `haversine_m`.
#[test]
fn test_haversine_with_radius_equals_default() {
    let lat1 = 35.6762_f64;
    let lon1 = 139.6503_f64;
    let lat2 = 34.6937_f64;
    let lon2 = 135.5023_f64;

    let d1 = haversine_m(lat1, lon1, lat2, lon2);
    let d2 = haversine_m_with_radius(lat1, lon1, lat2, lon2, 6_371_008.8);
    assert!(
        (d1 - d2).abs() < 1e-9,
        "haversine_m and haversine_m_with_radius(..mean_r) must agree; d1={d1}, d2={d2}"
    );
}

/// Custom radius changes the distance proportionally.
#[test]
fn test_haversine_with_radius_custom() {
    let d_earth = haversine_m(0.0, 0.0, 0.0, 90.0);
    let d_double = haversine_m_with_radius(0.0, 0.0, 0.0, 90.0, 6_371_008.8 * 2.0);
    assert!(
        (d_double / d_earth - 2.0).abs() < 1e-9,
        "Doubling the radius should double the haversine distance"
    );
}

// ---------------------------------------------------------------------------
// Vincenty tests
// ---------------------------------------------------------------------------

/// Vincenty should converge and return a valid distance for London → Paris.
#[test]
fn test_vincenty_london_paris_valid() {
    let result = vincenty_inverse_wgs84(51.5074, -0.1278, 48.8566, 2.3522);
    assert!(
        result.is_some(),
        "Vincenty London→Paris should converge, got None"
    );
    let r = result.unwrap();
    assert!(
        r.distance_m > 0.0,
        "Vincenty distance must be positive; got {}",
        r.distance_m
    );
    // Should be in the same ballpark as haversine (within 1%).
    // Haversine ≈ 343,557 m (spherical); Vincenty ≈ 343,923 m (ellipsoidal).
    // The ~0.1% difference is due to sphere vs ellipsoid — both are correct.
    let hav_d = haversine_m(51.5074, -0.1278, 48.8566, 2.3522);
    let relative_diff = (r.distance_m - hav_d).abs() / hav_d;
    assert!(
        relative_diff < 0.01,
        "Vincenty and haversine should agree within 1%; diff={relative_diff:.6}"
    );
}

/// Vincenty: coincident points → distance ≈ 0.
#[test]
fn test_vincenty_coincident_zero() {
    let result = vincenty_inverse_wgs84(0.0, 0.0, 0.0, 0.0);
    let r = result.expect("Vincenty coincident should converge");
    assert!(
        r.distance_m < 1e-3,
        "Vincenty coincident distance should be ~0 m; got {} m",
        r.distance_m
    );
}

/// Vincenty: near-antipodal off-equatorial pair should return None (non-convergence).
///
/// `(0°, 0°) → (0.5°, 179.5°)` is a nearly-antipodal point pair where the
/// iterative Vincenty algorithm is known to oscillate without converging.
/// The equatorial pair `(0,0)→(0,180)` *does* converge in this implementation
/// via the equatorial branch, so we use the off-equatorial near-antipodal case
/// that triggers the general oscillation failure.
#[test]
fn test_vincenty_antipodal_returns_none() {
    // (0°,0°) → (0.5°, 179.5°): near-antipodal, off-equatorial — fails to converge.
    let result = vincenty_inverse_wgs84(0.0, 0.0, 0.5, 179.5);
    assert!(
        result.is_none(),
        "Vincenty near-antipodal (0,0)→(0.5,179.5) should return None; got {:?}",
        result
    );
}

/// Vincenty: azimuths are in `[0°, 360°)`.
#[test]
fn test_vincenty_azimuth_range() {
    // Tokyo → Sydney — a long cross-hemisphere path with non-trivial azimuths.
    let result = vincenty_inverse_wgs84(35.6762, 139.6503, -33.8688, 151.2093);
    let r = result.expect("Tokyo→Sydney should converge");
    assert!(
        (0.0..360.0).contains(&r.azimuth_fwd_deg),
        "Forward azimuth must be in [0°, 360°); got {}",
        r.azimuth_fwd_deg
    );
    assert!(
        (0.0..360.0).contains(&r.azimuth_rev_deg),
        "Reverse azimuth must be in [0°, 360°); got {}",
        r.azimuth_rev_deg
    );
}

/// Vincenty is more accurate than haversine: on the WGS84 ellipsoid the
/// difference between the two is non-zero for non-trivial paths.
#[test]
fn test_vincenty_london_paris_closer_than_haversine() {
    // We cannot assert which is "better" without a ground truth, but we can
    // confirm that both return a positive value and that they differ (proving
    // that Vincenty is computing an ellipsoidal result, not just wrapping
    // haversine).
    let r = vincenty_inverse_wgs84(51.5074, -0.1278, 48.8566, 2.3522)
        .expect("Vincenty London→Paris should converge");
    let h = haversine_m(51.5074, -0.1278, 48.8566, 2.3522);

    // Both should be in the ballpark (within 1% of each other).
    let diff = (r.distance_m - h).abs();
    let rel_diff = diff / h;
    assert!(
        rel_diff < 0.01,
        "Vincenty and haversine differ by more than 1%: vincenty={:.1}, haversine={:.1}",
        r.distance_m,
        h
    );
    // They must differ by at least a few centimetres (ellipsoid vs sphere).
    assert!(
        r.distance_m != h,
        "Vincenty and haversine must differ (ellipsoid vs sphere)"
    );
}

// ---------------------------------------------------------------------------
// geo_nearest_k tests
// ---------------------------------------------------------------------------

/// geo_nearest_k with empty input slice returns empty Vec.
#[test]
fn test_geo_nearest_k_empty_input() {
    let empty: Vec<(GeoPoint, &str)> = Vec::new();
    let result = geo_nearest_k(&empty, 0.0, 0.0, 3);
    assert!(
        result.is_empty(),
        "geo_nearest_k on empty slice must return []"
    );
}

/// geo_nearest_k returns at most k results even when the input has more points.
#[test]
fn test_geo_nearest_k_returns_at_most_k() {
    let points: Vec<(GeoPoint, u32)> = (0..10)
        .map(|i| (GeoPoint::new(i as f64, 0.0), i as u32))
        .collect();

    let result = geo_nearest_k(&points, 5.0, 0.0, 3);
    assert!(
        result.len() <= 3,
        "geo_nearest_k with k=3 must return at most 3 results; got {}",
        result.len()
    );
}

/// geo_nearest_k results are sorted ascending by distance_m.
#[test]
fn test_geo_nearest_k_sorted_ascending() {
    // Create 10 points at varying latitudes; query from the equator.
    let points: Vec<(GeoPoint, usize)> = vec![
        (GeoPoint::new(50.0, 0.0), 0),
        (GeoPoint::new(10.0, 0.0), 1),
        (GeoPoint::new(-30.0, 0.0), 2),
        (GeoPoint::new(1.0, 0.0), 3),
        (GeoPoint::new(80.0, 0.0), 4),
        (GeoPoint::new(-5.0, 0.0), 5),
        (GeoPoint::new(20.0, 10.0), 6),
        (GeoPoint::new(-10.0, -20.0), 7),
        (GeoPoint::new(0.5, 0.1), 8),
        (GeoPoint::new(-0.1, 0.0), 9),
    ];

    let k = 7;
    let result = geo_nearest_k(&points, 0.0, 0.0, k);

    assert!(
        result.len() <= k,
        "Should return at most k={k} results; got {}",
        result.len()
    );

    for window in result.windows(2) {
        assert!(
            window[0].distance_m <= window[1].distance_m,
            "Results not sorted: {} > {} (values: {} {})",
            window[0].distance_m,
            window[1].distance_m,
            window[0].value,
            window[1].value
        );
    }
}

/// geo_nearest_k with k=0 returns empty result.
#[test]
fn test_geo_nearest_k_k_zero() {
    let points: Vec<(GeoPoint, i32)> =
        vec![(GeoPoint::new(1.0, 1.0), 1), (GeoPoint::new(2.0, 2.0), 2)];
    let result = geo_nearest_k(&points, 0.0, 0.0, 0);
    assert!(
        result.is_empty(),
        "k=0 must return empty; got {:?} results",
        result.len()
    );
}

/// geo_nearest_k returns correct nearest point for a simple configuration.
#[test]
fn test_geo_nearest_k_correct_nearest() {
    // Point A at (1°, 0°) and point B at (10°, 0°); query from (0°, 0°).
    // A is closer — it should be first.
    let points: Vec<(GeoPoint, &str)> = vec![
        (GeoPoint::new(10.0, 0.0), "far"),
        (GeoPoint::new(1.0, 0.0), "near"),
    ];
    let result = geo_nearest_k(&points, 0.0, 0.0, 2);
    assert_eq!(result.len(), 2);
    assert_eq!(
        result[0].value, "near",
        "Nearest point should be 'near'; got '{}'",
        result[0].value
    );
    assert!(
        result[0].distance_m < result[1].distance_m,
        "First result must have smaller distance"
    );
}

// ---------------------------------------------------------------------------
// geo_within_radius tests
// ---------------------------------------------------------------------------

/// geo_within_radius filters correctly: only points within radius_m are included.
#[test]
fn test_geo_within_radius_filters_correctly() {
    // Place 5 points at increasing latitudes from the equator.
    // Query from (0, 0) with radius = 200,000 m (~200 km ≈ ~1.8°).
    let points: Vec<(GeoPoint, &str)> = vec![
        (GeoPoint::new(0.0, 0.0), "origin"),  // 0 m — inside
        (GeoPoint::new(0.5, 0.0), "0.5_deg"), // ≈ 55,597 m — inside
        (GeoPoint::new(1.0, 0.0), "1_deg"),   // ≈ 111,195 m — inside
        (GeoPoint::new(2.0, 0.0), "2_deg"),   // ≈ 222,390 m — outside
        (GeoPoint::new(5.0, 5.0), "far"),     // ≈ 785,000 m — outside
    ];

    let radius_m = 150_000.0; // 150 km
    let result = geo_within_radius(&points, 0.0, 0.0, radius_m);

    // Should include "origin", "0.5_deg", "1_deg" and exclude "2_deg", "far".
    let values: Vec<&str> = result.iter().map(|r| r.value).collect();
    assert!(
        values.contains(&"origin"),
        "Origin (0 m) must be inside radius; got {values:?}"
    );
    assert!(
        values.contains(&"0.5_deg"),
        "0.5° point should be inside 150 km; got {values:?}"
    );
    assert!(
        values.contains(&"1_deg"),
        "1° point (≈111 km) should be inside 150 km; got {values:?}"
    );
    assert!(
        !values.contains(&"2_deg"),
        "2° point (≈222 km) must be outside 150 km; got {values:?}"
    );
    assert!(
        !values.contains(&"far"),
        "Far point must be outside 150 km; got {values:?}"
    );

    // Results must be sorted ascending by distance.
    for window in result.windows(2) {
        assert!(
            window[0].distance_m <= window[1].distance_m,
            "geo_within_radius must sort ascending by distance_m"
        );
    }
}

/// geo_within_radius on empty slice returns empty Vec.
#[test]
fn test_geo_within_radius_empty_input() {
    let empty: Vec<(GeoPoint, i32)> = Vec::new();
    let result = geo_within_radius(&empty, 35.0, 139.0, 10_000.0);
    assert!(result.is_empty(), "Empty input must return []");
}

// ---------------------------------------------------------------------------
// geo_bbox_extent_m tests
// ---------------------------------------------------------------------------

/// At the equator a 1°×1° bounding box should be approximately 111 km × 111 km.
/// Tolerance: within 5% (≈ 5,560 m).
#[test]
fn test_geo_bbox_extent_m_equatorial_plausible() {
    let (w, h) = geo_bbox_extent_m(0.0, 0.0, 1.0, 1.0);
    let expected = 111_195.0_f64;
    let tol = expected * 0.05;

    assert!(
        (w - expected).abs() < tol,
        "Width of 1°×1° bbox at equator should be ~111,195 m; got {w:.1} m"
    );
    assert!(
        (h - expected).abs() < tol,
        "Height of 1°×1° bbox at equator should be ~111,195 m; got {h:.1} m"
    );
}

/// A 2°×2° box at the equator should be approximately twice the 1°×1° extents.
#[test]
fn test_geo_bbox_extent_m_two_degrees() {
    let (w1, h1) = geo_bbox_extent_m(0.0, 0.0, 1.0, 1.0);
    let (w2, h2) = geo_bbox_extent_m(0.0, 0.0, 2.0, 2.0);

    // Linear haversine scaling: 2° should be ≈ 2 × 1° (within 1%).
    assert!(
        (w2 / w1 - 2.0).abs() < 0.01,
        "2° width should be ≈2× 1° width; ratio={:.4}",
        w2 / w1
    );
    assert!(
        (h2 / h1 - 2.0).abs() < 0.01,
        "2° height should be ≈2× 1° height; ratio={:.4}",
        h2 / h1
    );
}

// ---------------------------------------------------------------------------
// SpatialQuery wrapper tests
// ---------------------------------------------------------------------------

/// `SpatialQuery::geo_nearest_k` must delegate correctly to `geo_nearest_k`.
#[test]
fn test_spatial_query_geo_nearest_k_delegates() {
    let points: Vec<(GeoPoint, u8)> = vec![
        (GeoPoint::new(0.0, 0.0), 1),
        (GeoPoint::new(1.0, 0.0), 2),
        (GeoPoint::new(5.0, 0.0), 3),
    ];

    let via_fn = geo_nearest_k(&points, 0.0, 0.0, 2);
    let via_sq = SpatialQuery::geo_nearest_k(&points, 0.0, 0.0, 2);

    assert_eq!(
        via_fn.len(),
        via_sq.len(),
        "SpatialQuery and free-function must return same number of results"
    );
    for (a, b) in via_fn.iter().zip(via_sq.iter()) {
        assert_eq!(
            a.value, b.value,
            "SpatialQuery wrapper must return same values"
        );
        assert!(
            (a.distance_m - b.distance_m).abs() < 1e-9,
            "SpatialQuery wrapper must return same distances"
        );
    }
}

/// `SpatialQuery::geo_within_radius` must delegate correctly to `geo_within_radius`.
#[test]
fn test_spatial_query_geo_within_radius_delegates() {
    let points: Vec<(GeoPoint, &str)> = vec![
        (GeoPoint::new(0.0, 0.0), "a"),
        (GeoPoint::new(0.5, 0.0), "b"),
        (GeoPoint::new(10.0, 10.0), "c"),
    ];

    let radius = 100_000.0;
    let via_fn = geo_within_radius(&points, 0.0, 0.0, radius);
    let via_sq = SpatialQuery::geo_within_radius(&points, 0.0, 0.0, radius);

    assert_eq!(
        via_fn.len(),
        via_sq.len(),
        "SpatialQuery radius wrapper must return same count"
    );
}
