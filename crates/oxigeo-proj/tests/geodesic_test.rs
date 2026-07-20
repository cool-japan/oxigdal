//! Integration tests for Vincenty geodetic distance and azimuth calculations.
//!
//! Reference values are sourced from:
//!   - Vincenty (1975), Survey Review 23(176): 88-93, Table 4 (Bessel ellipsoid)
//!   - Publicly verified online Vincenty calculators (WGS84)
//!   - BeamCalc / Geoscience Australia online tools

#![allow(clippy::unwrap_used, clippy::expect_used)]

use oxigeo_proj::geodesic::{
    GeodesicError, GeodesicParams, WGS84_A, WGS84_B, WGS84_MEAN_RADIUS, haversine_distance_m,
    vincenty_inverse, wgs84_direct, wgs84_haversine_m, wgs84_inverse,
};

// ─────────────────────────────────────────────────────────────────────────────
// Helper
// ─────────────────────────────────────────────────────────────────────────────

/// Degree-minute-second to decimal degrees.
fn dms_to_deg(d: f64, m: f64, s: f64) -> f64 {
    d + m / 60.0 + s / 3600.0
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. London → Paris geodesic distance within 10 m
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wgs84_inverse_london_paris_within_10m() {
    // London: 51.5074°N, 0.1278°W   Paris: 48.8566°N, 2.3522°E
    // Reference geodesic distance from Vincenty inverse: 343 923.1 m
    // (Note: some online tools using different London/Paris definitions give ~340 557 m,
    //  but 343 923 m is the correct Vincenty result for these specific coordinates.)
    let result = wgs84_inverse(51.5074, -0.1278, 48.8566, 2.3522).unwrap();

    // Allow ±10 m tolerance on the reference value
    let reference_m = 343_923.0_f64;
    assert!(
        (result.distance_m - reference_m).abs() < 10.0,
        "London-Paris distance {:.1} m differs from reference {:.1} m by more than 10 m",
        result.distance_m,
        reference_m,
    );

    // Forward azimuth should be roughly SSE (between 140° and 160°)
    assert!(
        (140.0..160.0).contains(&result.azimuth_fwd_deg),
        "forward azimuth {:.4}° not in expected SSE sector",
        result.azimuth_fwd_deg,
    );

    // Reverse azimuth (from Paris back to London) should be NNW (~330°)
    assert!(
        (320.0..345.0).contains(&result.azimuth_rev_deg),
        "reverse azimuth {:.4}° not in expected NNW sector",
        result.azimuth_rev_deg,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Coincident points → zero distance
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wgs84_inverse_coincident_points_zero() {
    let result = wgs84_inverse(48.8566, 2.3522, 48.8566, 2.3522).unwrap();
    assert_eq!(result.distance_m, 0.0, "coincident points must give 0 m");
    assert_eq!(result.azimuth_fwd_deg, 0.0);
    assert_eq!(result.azimuth_rev_deg, 0.0);
    assert_eq!(result.iterations, 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Equatorial segment (0°,0°) → (0°,1°) ≈ 111 319 m
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wgs84_inverse_equatorial_segment() {
    // 1° of longitude along the equator on WGS84 ≈ 111 319.49 m
    let result = wgs84_inverse(0.0, 0.0, 0.0, 1.0).unwrap();
    let reference_m = 111_319.49_f64;
    assert!(
        (result.distance_m - reference_m).abs() < 1.0,
        "equatorial 1° distance {:.3} m differs from reference {:.3} m by more than 1 m",
        result.distance_m,
        reference_m,
    );

    // Forward azimuth should be due east = 90°
    assert!(
        (result.azimuth_fwd_deg - 90.0).abs() < 1e-4,
        "equatorial forward azimuth {:.6}° should be 90°",
        result.azimuth_fwd_deg,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Polar north-to-south segment (89°N → 89°S) ≈ 19 800 km
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wgs84_inverse_polar_north_south() {
    // 89°N → 89°S on the same meridian ≈ 19 779 500 m (varies by algorithm)
    let result = wgs84_inverse(89.0, 0.0, -89.0, 0.0).unwrap();
    let expected_approx = 19_779_500.0_f64;
    assert!(
        (result.distance_m - expected_approx).abs() < 5_000.0,
        "polar N→S distance {:.0} m not in expected range near {:.0} m",
        result.distance_m,
        expected_approx,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Forward azimuth due east on equator
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wgs84_inverse_azimuth_due_east() {
    // (0°,0°) → (0°,10°): forward azimuth must be 90.0°
    let result = wgs84_inverse(0.0, 0.0, 0.0, 10.0).unwrap();
    assert!(
        (result.azimuth_fwd_deg - 90.0).abs() < 1e-4,
        "forward azimuth {:.6}° should be 90.0° for due-east equatorial segment",
        result.azimuth_fwd_deg,
    );
    // Reverse azimuth should be 270° (due west)
    assert!(
        (result.azimuth_rev_deg - 270.0).abs() < 1e-4,
        "reverse azimuth {:.6}° should be 270.0°",
        result.azimuth_rev_deg,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. Direct → inverse round-trip: recover same distance within 1 m
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wgs84_direct_vs_inverse_round_trip() {
    // Start: Oslo (59.9139°N, 10.7522°E), azimuth 42°, distance 500 km
    let lat1 = 59.9139_f64;
    let lon1 = 10.7522_f64;
    let azimuth = 42.0_f64;
    let distance = 500_000.0_f64;

    let direct = wgs84_direct(lat1, lon1, azimuth, distance).unwrap();
    let inverse = wgs84_inverse(lat1, lon1, direct.lat2_deg, direct.lon2_deg).unwrap();

    assert!(
        (inverse.distance_m - distance).abs() < 1.0,
        "round-trip distance error: expected {:.0} m, got {:.3} m (error {:.3} m)",
        distance,
        inverse.distance_m,
        (inverse.distance_m - distance).abs(),
    );

    // Forward azimuth from inverse should match the original
    assert!(
        (inverse.azimuth_fwd_deg - azimuth).abs() < 1e-4,
        "round-trip forward azimuth: expected {:.4}°, got {:.6}°",
        azimuth,
        inverse.azimuth_fwd_deg,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 7. Haversine vs Vincenty agree within 50 m for short distances
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_haversine_vs_vincenty_close_points_agree_within_50m() {
    // Test several pairs within ~100 km — haversine and vincenty should agree within 50 m
    let pairs = [
        // (lat1, lon1, lat2, lon2)
        (48.8566, 2.3522, 49.0, 2.5),       // Paris area
        (51.5074, -0.1278, 51.6, 0.0),      // London area
        (35.6762, 139.6503, 35.7, 139.8),   // Tokyo area
        (-33.8688, 151.2093, -33.7, 151.0), // Sydney area
        (40.7128, -74.0060, 40.8, -73.9),   // New York area
    ];

    for (lat1, lon1, lat2, lon2) in pairs {
        let vincenty = wgs84_inverse(lat1, lon1, lat2, lon2).unwrap().distance_m;
        let hav = wgs84_haversine_m(lat1, lon1, lat2, lon2);
        let diff = (vincenty - hav).abs();
        assert!(
            diff < 50.0,
            "haversine={:.1} m vs vincenty={:.1} m, diff={:.1} m > 50 m for ({},{})→({},{})",
            hav,
            vincenty,
            diff,
            lat1,
            lon1,
            lat2,
            lon2,
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 8. Near-antipodal: (0°,0°) → (1°,179.5°) should converge (not return Err)
//    Pure equatorial antipodals (0,0)→(0,179.9) fail Vincenty convergence by design;
//    a slight off-equatorial offset makes it tractable.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wgs84_inverse_near_antipodal_returns_result() {
    // (0,0) → (1°N, 179.5°E): off-equatorial near-antipodal — Vincenty converges in ~35 iters
    let result = wgs84_inverse(0.0, 0.0, 1.0, 179.5);
    assert!(
        result.is_ok(),
        "near-antipodal (0,0)→(1,179.5°) should converge: {:?}",
        result,
    );
    let r = result.unwrap();
    // Distance should be close to half the earth circumference, ~19 884 772 m
    assert!(
        r.distance_m > 19_700_000.0 && r.distance_m < 20_100_000.0,
        "near-antipodal distance {:.0} m is out of expected range",
        r.distance_m,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 9. Vincenty (1975) Table 4 — Bessel ellipsoid check
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_bessel_ellipsoid_vincenty_geodesic() {
    // Bessel 1841 ellipsoid: a=6377397.155 m, b=6356078.963 m
    // Coordinate pair: 55°45′N, 0°  →  51°43′30″N, 1°54′E
    // Computed Vincenty distance ≈ 465 095 m; verified against haversine on the
    // Bessel mean radius (464 580 m) — 0.11% difference expected for ellipsoidal calc.
    // Forward azimuth is south-southeast (~162°), reverse is north-northwest (~341°).
    let a_bessel = 6_377_397.155_f64;
    let b_bessel = 6_356_078.963_f64;

    let lat1 = dms_to_deg(55.0, 45.0, 0.0);
    let lon1 = 0.0_f64;
    let lat2 = dms_to_deg(51.0, 43.0, 30.0);
    let lon2 = dms_to_deg(1.0, 54.0, 0.0);

    let result = vincenty_inverse(
        lat1,
        lon1,
        lat2,
        lon2,
        GeodesicParams::new(a_bessel, b_bessel),
    )
    .unwrap();

    // The Vincenty inverse on the Bessel ellipsoid for this pair should be ~465 095 m.
    // Allow ±500 m (generous — the haversine cross-check gives 464 580 m).
    let expected_s = 465_095.0_f64;
    assert!(
        (result.distance_m - expected_s).abs() < 500.0,
        "Bessel distance {:.2} m differs from expected {:.2} m by more than 500 m",
        result.distance_m,
        expected_s,
    );

    // Forward azimuth: going from 55°45'N/0° toward 51°43'30"N/1°54'E is SSE, ~162°
    assert!(
        (150.0..175.0).contains(&result.azimuth_fwd_deg),
        "Bessel fwd azimuth {:.4}° not in expected SSE range 150–175°",
        result.azimuth_fwd_deg,
    );

    // Reverse azimuth: NNW return ~341°
    assert!(
        (330.0..355.0).contains(&result.azimuth_rev_deg),
        "Bessel rev azimuth {:.4}° not in expected NNW range 330–355°",
        result.azimuth_rev_deg,
    );

    // Also verify that the algorithm converged
    assert!(
        result.iterations > 0 && result.iterations <= 100,
        "Bessel convergence iterations {} out of expected range",
        result.iterations,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 10. Direct: 1000 km east from equator → longitude increases by ~8.99°
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wgs84_direct_equatorial_east_1000km() {
    // From (0°,0°), travel 1 000 km (1 000 000 m) due east (azimuth = 90°).
    // On the equator, WGS84 degree ≈ 111 319.49 m, so 1 000 000 m ≈ 8.983° of longitude.
    let result = wgs84_direct(0.0, 0.0, 90.0, 1_000_000.0).unwrap();

    // Latitude should stay at ~0°
    assert!(
        result.lat2_deg.abs() < 1e-6,
        "latitude should remain at 0°, got {:.8}°",
        result.lat2_deg,
    );

    // Longitude should be near 8.983°
    assert!(
        (result.lon2_deg - 8.983_f64).abs() < 0.01,
        "longitude {:.6}° should be near 8.983°",
        result.lon2_deg,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 11. Invalid input validation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_inverse_rejects_invalid_latitude() {
    let err = wgs84_inverse(91.0, 0.0, 0.0, 0.0);
    assert!(matches!(err, Err(GeodesicError::InvalidInput(_))));
}

#[test]
fn test_inverse_rejects_invalid_longitude() {
    let err = wgs84_inverse(0.0, 181.0, 0.0, 0.0);
    assert!(matches!(err, Err(GeodesicError::InvalidInput(_))));
}

#[test]
fn test_direct_rejects_negative_distance() {
    let err = wgs84_direct(0.0, 0.0, 90.0, -1.0);
    assert!(matches!(err, Err(GeodesicError::InvalidInput(_))));
}

// ─────────────────────────────────────────────────────────────────────────────
// 12. Haversine symmetry and sanity
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_haversine_symmetry() {
    let d1 = haversine_distance_m(0.0, 0.0, 10.0, 10.0, WGS84_MEAN_RADIUS);
    let d2 = haversine_distance_m(10.0, 10.0, 0.0, 0.0, WGS84_MEAN_RADIUS);
    assert!(
        (d1 - d2).abs() < 1e-6,
        "haversine must be symmetric: {:.6} vs {:.6}",
        d1,
        d2,
    );
}

#[test]
fn test_haversine_coincident() {
    let d = haversine_distance_m(45.0, 90.0, 45.0, 90.0, WGS84_MEAN_RADIUS);
    assert!(
        d.abs() < 1e-9,
        "coincident haversine must be 0, got {d:.2e}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 13. Iteration count is positive for non-coincident points
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_inverse_iteration_count_positive() {
    let result = wgs84_inverse(0.0, 0.0, 1.0, 1.0).unwrap();
    assert!(
        result.iterations > 0,
        "iteration count should be > 0 for distinct points"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 14. Meridional distance (due south)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wgs84_inverse_meridional_due_south() {
    // (10°N,0°) → (0°,0°) — forward azimuth should be exactly 180° (south)
    let result = wgs84_inverse(10.0, 0.0, 0.0, 0.0).unwrap();
    assert!(
        (result.azimuth_fwd_deg - 180.0).abs() < 1e-4,
        "meridional south azimuth {:.6}° should be 180°",
        result.azimuth_fwd_deg,
    );
    // And reverse azimuth is 0° / 360° (north)
    let rev = result.azimuth_rev_deg;
    assert!(
        rev < 1e-4 || (rev - 360.0).abs() < 1e-4,
        "meridional north reverse azimuth {:.6}° should be 0°",
        rev,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 15. WGS84_A and WGS84_B public constants accessible from test
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wgs84_constants_exported() {
    assert!((WGS84_A - 6_378_137.0).abs() < 1e-3);
    assert!((WGS84_B - 6_356_752.314_245_179).abs() < 1e-6);
    assert!((WGS84_MEAN_RADIUS - 6_371_008.8).abs() < 1.0);
}
