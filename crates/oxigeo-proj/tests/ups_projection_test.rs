//! Integration tests for the Universal Polar Stereographic (UPS) projection.

#![allow(clippy::expect_used)]

use oxigeo_proj::{
    PolarStereographicParams, UpsCoordinate, UpsHemisphere, polar_stereo_w,
    polar_stereographic_forward, polar_stereographic_inverse, ups_from_geographic,
    ups_to_geographic, ups_zone_letter,
};

const M_TOL: f64 = 1.0;
const MM_TOL: f64 = 0.001;
const DEG_TOL: f64 = 1e-6;

// ─── Pole origin tests ────────────────────────────────────────────────────────

#[test]
fn test_ups_north_origin_at_pole_returns_fe_fn() {
    let params = PolarStereographicParams::wgs84_ups_north();
    let (e, n) = polar_stereographic_forward(&params, 0.0, 90.0);
    assert!(
        (e - 2_000_000.0).abs() < M_TOL,
        "north pole easting should be 2 000 000, got {}",
        e
    );
    assert!(
        (n - 2_000_000.0).abs() < M_TOL,
        "north pole northing should be 2 000 000, got {}",
        n
    );
}

#[test]
fn test_ups_south_origin_at_pole_returns_fe_fn() {
    let params = PolarStereographicParams::wgs84_ups_south();
    let (e, n) = polar_stereographic_forward(&params, 0.0, -90.0);
    assert!(
        (e - 2_000_000.0).abs() < M_TOL,
        "south pole easting should be 2 000 000, got {}",
        e
    );
    assert!(
        (n - 2_000_000.0).abs() < M_TOL,
        "south pole northing should be 2 000 000, got {}",
        n
    );
}

#[test]
fn test_ups_forward_at_north_pole_returns_2000000_2000000() {
    let coord = ups_from_geographic(0.0, 90.0).expect("north pole is valid UPS");
    assert!(
        (coord.easting - 2_000_000.0).abs() < M_TOL,
        "easting: {}",
        coord.easting
    );
    assert!(
        (coord.northing - 2_000_000.0).abs() < M_TOL,
        "northing: {}",
        coord.northing
    );
    assert_eq!(coord.hemisphere, UpsHemisphere::North);
}

// ─── Round-trip accuracy ──────────────────────────────────────────────────────

#[test]
fn test_ups_north_forward_inverse_round_trip_at_85n() {
    let (lon0, lat0) = (45.0_f64, 85.0_f64);
    let coord = ups_from_geographic(lon0, lat0).expect("85 N is valid UPS");
    let (lon1, lat1) = ups_to_geographic(&coord);
    assert!(
        (lon1 - lon0).abs() < DEG_TOL,
        "longitude round-trip error: {} deg",
        (lon1 - lon0).abs()
    );
    assert!(
        (lat1 - lat0).abs() < DEG_TOL,
        "latitude round-trip error: {} deg",
        (lat1 - lat0).abs()
    );
}

#[test]
fn test_ups_south_forward_inverse_round_trip_at_85s() {
    let (lon0, lat0) = (45.0_f64, -85.0_f64);
    let coord = ups_from_geographic(lon0, lat0).expect("85 S is valid UPS");
    let (lon1, lat1) = ups_to_geographic(&coord);
    assert!(
        (lon1 - lon0).abs() < DEG_TOL,
        "longitude round-trip error: {} deg",
        (lon1 - lon0).abs()
    );
    assert!(
        (lat1 - lat0).abs() < DEG_TOL,
        "latitude round-trip error: {} deg",
        (lat1 - lat0).abs()
    );
}

#[test]
fn test_round_trip_at_88n_within_1mm() {
    let params = PolarStereographicParams::wgs84_ups_north();
    let (lon0, lat0) = (120.0_f64, 88.0_f64);
    let (e, n) = polar_stereographic_forward(&params, lon0, lat0);
    let (lon1, lat1) = polar_stereographic_inverse(&params, e, n);

    // Convert degree error to approximate metres at 88 N
    // 1 deg lat ≈ 111 km; 1 deg lon at 88 N ≈ 111 km * cos(88°) ≈ 3.9 km
    let lat_err_m = (lat1 - lat0).abs() * 111_319.5;
    let lon_err_m = (lon1 - lon0).abs() * 111_319.5 * (88.0_f64.to_radians().cos());

    assert!(
        lat_err_m < MM_TOL,
        "latitude round-trip error {} m exceeds {} m",
        lat_err_m,
        MM_TOL
    );
    assert!(
        lon_err_m < MM_TOL,
        "longitude round-trip error {} m exceeds {} m",
        lon_err_m,
        MM_TOL
    );
}

// ─── Known-value tests ────────────────────────────────────────────────────────

#[test]
fn test_ups_forward_known_coord_matches_published_value() {
    // At lon = 180°, sin(λ) = 0, so E = FE = 2 000 000 exactly.
    let params = PolarStereographicParams::wgs84_ups_north();
    let (e, _n) = polar_stereographic_forward(&params, 180.0, 85.0);
    assert!(
        (e - 2_000_000.0).abs() < M_TOL,
        "easting at lon=180° should equal FE, got {}",
        e
    );
}

// ─── Inverse at grid origin ───────────────────────────────────────────────────

#[test]
fn test_ups_inverse_at_origin_returns_pole() {
    let params_n = PolarStereographicParams::wgs84_ups_north();
    let (_lon, lat) = polar_stereographic_inverse(&params_n, 2_000_000.0, 2_000_000.0);
    assert!(
        (lat - 90.0).abs() < DEG_TOL,
        "inverse of N grid origin should be N pole, got lat={}",
        lat
    );

    let params_s = PolarStereographicParams::wgs84_ups_south();
    let (_lon, lat) = polar_stereographic_inverse(&params_s, 2_000_000.0, 2_000_000.0);
    assert!(
        (lat - (-90.0)).abs() < DEG_TOL,
        "inverse of S grid origin should be S pole, got lat={}",
        lat
    );
}

// ─── Zone-letter tests ────────────────────────────────────────────────────────

#[test]
fn test_ups_zone_letter_north_y_z_partition() {
    assert_eq!(ups_zone_letter(-1.0, 84.0), Some('Y'));
    assert_eq!(ups_zone_letter(-90.0, 90.0), Some('Y'));
    assert_eq!(ups_zone_letter(0.0, 84.0), Some('Z'));
    assert_eq!(ups_zone_letter(90.0, 89.0), Some('Z'));
}

#[test]
fn test_ups_zone_letter_south_a_b_partition() {
    assert_eq!(ups_zone_letter(-1.0, -80.0), Some('A'));
    assert_eq!(ups_zone_letter(-90.0, -90.0), Some('A'));
    assert_eq!(ups_zone_letter(0.0, -80.0), Some('B'));
    assert_eq!(ups_zone_letter(90.0, -85.0), Some('B'));
}

#[test]
fn test_ups_zone_letter_returns_none_outside_band() {
    assert_eq!(ups_zone_letter(0.0, 45.0), None);
    assert_eq!(ups_zone_letter(10.0, 0.0), None);
    assert_eq!(ups_zone_letter(-45.0, 83.9), None);
    assert_eq!(ups_zone_letter(0.0, -79.9), None);
}

// ─── Input validation ─────────────────────────────────────────────────────────

#[test]
fn test_ups_from_geographic_rejects_low_latitude() {
    let result = ups_from_geographic(0.0, 45.0);
    assert!(result.is_err(), "lat=45° should be rejected by UPS");

    let result = ups_from_geographic(30.0, -50.0);
    assert!(result.is_err(), "lat=-50° should be rejected by UPS");

    let result = ups_from_geographic(0.0, 79.999);
    assert!(result.is_err(), "lat=79.999° should be rejected by UPS");
}

// ─── W constant ──────────────────────────────────────────────────────────────

#[test]
fn test_polar_stereo_w_wgs84_value() {
    let w = polar_stereo_w(0.081_819_190_842_621_5);
    assert!(
        (1.003..1.004).contains(&w),
        "W for WGS-84 should be in [1.003, 1.004), got {}",
        w
    );
}

// ─── Polar band corner consistency ───────────────────────────────────────────

#[test]
fn test_ups_corners_of_polar_band_consistency() {
    let corners: &[(f64, f64)] = &[(45.0, 80.0), (135.0, 80.0), (-45.0, 80.0), (-135.0, 80.0)];

    for &(lon0, lat0) in corners {
        let coord = ups_from_geographic(lon0, lat0).expect("corner point is inside UPS band");

        let (lon1, lat1) = ups_to_geographic(&coord);

        assert!(
            (lon1 - lon0).abs() < DEG_TOL,
            "corner ({},{}) longitude round-trip error: {}",
            lon0,
            lat0,
            (lon1 - lon0).abs()
        );
        assert!(
            (lat1 - lat0).abs() < DEG_TOL,
            "corner ({},{}) latitude round-trip error: {}",
            lon0,
            lat0,
            (lat1 - lat0).abs()
        );
    }
}

// ─── UpsCoordinate struct construction ───────────────────────────────────────

#[test]
fn test_ups_coordinate_hemisphere_assigned_correctly() {
    let north = ups_from_geographic(0.0, 85.0).expect("85 N");
    assert_eq!(north.hemisphere, UpsHemisphere::North);

    let south = ups_from_geographic(0.0, -85.0).expect("85 S");
    assert_eq!(south.hemisphere, UpsHemisphere::South);
}

#[test]
fn test_ups_coordinate_manual_inverse() {
    let coord = UpsCoordinate {
        easting: 2_000_000.0,
        northing: 2_000_000.0,
        hemisphere: UpsHemisphere::South,
    };
    let (_lon, lat) = ups_to_geographic(&coord);
    assert!(
        (lat - (-90.0)).abs() < DEG_TOL,
        "manual south-pole coordinate should invert to -90°, got {}",
        lat
    );
}
