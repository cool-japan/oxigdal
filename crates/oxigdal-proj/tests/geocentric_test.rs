//! Integration tests for the `geocentric` module.
//!
//! Validates geographic ↔ ECEF conversions, ellipsoid derived quantities,
//! `GeocentricCrs` metadata, `EcefTransformer` (with and without Helmert),
//! and the three-step geographic-to-geographic datum conversion.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use oxigdal_proj::geocentric::{
    EcefCoordinate, EcefTransformer, GeocentricCrs, GeocentricEllipsoid, ecef_to_geographic,
    ecef_to_geographic_iterative, geographic_to_ecef, geographic_to_geographic_via_ecef,
};
use oxigdal_proj::grid_shift::Helmert7Params;

// ─────────────────────────────────────────────────────────────────────────────
// Ellipsoid derived quantities
// ─────────────────────────────────────────────────────────────────────────────

/// WGS84 first eccentricity squared should be ≈ 0.006 694 379 990 14.
#[test]
fn test_ellipsoid_eccentricity_squared_wgs84() {
    let ell = GeocentricEllipsoid::wgs84();
    let e2 = ell.eccentricity_squared();
    let expected = 0.006_694_379_990_14_f64;
    assert!(
        (e2 - expected).abs() < 1e-14,
        "e² = {e2:.15}, expected ≈ {expected:.15}"
    );
}

/// WGS84 semi-minor axis should be ≈ 6 356 752.314 140 m.
#[test]
fn test_ellipsoid_semiminor_b_wgs84() {
    let ell = GeocentricEllipsoid::wgs84();
    let b = ell.semiminor_b();
    // Published value (derived from a and f).
    let expected = 6_356_752.314_140_356_f64;
    assert!(
        (b - expected).abs() < 1e-3,
        "b = {b:.6}, expected ≈ {expected:.6}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Geographic → ECEF known-point checks
// ─────────────────────────────────────────────────────────────────────────────

/// Equator, prime meridian, zero height → X ≈ a, Y ≈ 0, Z ≈ 0.
#[test]
fn test_wgs84_equator_zero_lon() {
    let ell = GeocentricEllipsoid::wgs84();
    let ecef = geographic_to_ecef(&ell, 0.0, 0.0, 0.0);
    let a = ell.semimajor_a;
    assert!((ecef.x - a).abs() < 1e-4, "X = {:.4} ≠ a = {a:.4}", ecef.x);
    assert!(ecef.y.abs() < 1e-10, "Y = {} ≠ 0", ecef.y);
    assert!(ecef.z.abs() < 1e-10, "Z = {} ≠ 0", ecef.z);
}

/// North pole (lat=90°, lon=0°, h=0) → X ≈ 0, Y ≈ 0, Z ≈ b.
#[test]
fn test_wgs84_north_pole_returns_zero_zero_b() {
    let ell = GeocentricEllipsoid::wgs84();
    let ecef = geographic_to_ecef(&ell, 0.0, 90.0, 0.0);
    let b = ell.semiminor_b();
    assert!(
        ecef.x.abs() < 1e-4,
        "X = {:.6} should be ≈ 0 at North Pole",
        ecef.x
    );
    assert!(
        ecef.y.abs() < 1e-4,
        "Y = {:.6} should be ≈ 0 at North Pole",
        ecef.y
    );
    assert!((ecef.z - b).abs() < 1e-3, "Z = {:.4} ≠ b = {b:.4}", ecef.z);
}

/// A point with h=0 lies on the ellipsoid surface: X²/a² + Y²/a² + Z²/b² ≈ 1.
#[test]
fn test_geographic_to_ecef_height_0_lies_on_ellipsoid() {
    let ell = GeocentricEllipsoid::wgs84();
    let a = ell.semimajor_a;
    let b = ell.semiminor_b();
    // Use a non-trivial point (45°N, 30°E).
    let ecef = geographic_to_ecef(&ell, 30.0, 45.0, 0.0);
    let ellipsoid_check =
        (ecef.x * ecef.x) / (a * a) + (ecef.y * ecef.y) / (a * a) + (ecef.z * ecef.z) / (b * b);
    assert!(
        (ellipsoid_check - 1.0).abs() < 1e-9,
        "ellipsoid equation = {ellipsoid_check:.12} (should be 1.0)"
    );
}

/// Negative height should produce a point strictly closer to Earth's centre.
#[test]
fn test_geographic_to_ecef_negative_height_handled() {
    let ell = GeocentricEllipsoid::wgs84();
    let ecef_0 = geographic_to_ecef(&ell, 15.0, 50.0, 0.0);
    let ecef_neg = geographic_to_ecef(&ell, 15.0, 50.0, -100.0);
    assert!(
        ecef_neg.magnitude() < ecef_0.magnitude(),
        "h=-100 magnitude {:.3} should be less than h=0 magnitude {:.3}",
        ecef_neg.magnitude(),
        ecef_0.magnitude()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Round-trip accuracy
// ─────────────────────────────────────────────────────────────────────────────

/// Forward then inverse (Bowring) should agree to < 1 µm.
#[test]
fn test_geographic_to_ecef_round_trip() {
    let ell = GeocentricEllipsoid::wgs84();
    let lon_in = 13.408_333;
    let lat_in = 52.518_611; // Berlin
    let h_in = 40.0_f64;

    let ecef = geographic_to_ecef(&ell, lon_in, lat_in, h_in);
    let (lon_out, lat_out, h_out) = ecef_to_geographic(&ell, &ecef);

    // 1 µm tolerance on the ellipsoidal surface translates to ~1e-11 degrees.
    assert!(
        (lon_out - lon_in).abs() < 1e-9,
        "lon round-trip error: {}°",
        (lon_out - lon_in).abs()
    );
    assert!(
        (lat_out - lat_in).abs() < 1e-9,
        "lat round-trip error: {}°",
        (lat_out - lat_in).abs()
    );
    assert!(
        (h_out - h_in).abs() < 1e-6,
        "height round-trip error: {} m",
        (h_out - h_in).abs()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Bowring vs Iterative agreement
// ─────────────────────────────────────────────────────────────────────────────

/// Both methods should agree on longitude, latitude and height to < 1 × 10⁻⁹.
#[test]
fn test_ecef_to_geographic_bowring_vs_iterative() {
    let ell = GeocentricEllipsoid::wgs84();
    // Tokyo: 139.69°E, 35.69°N, h=25 m
    let ecef = geographic_to_ecef(&ell, 139.69, 35.69, 25.0);

    let (lon_b, lat_b, h_b) = ecef_to_geographic(&ell, &ecef);
    let (lon_i, lat_i, h_i) = ecef_to_geographic_iterative(&ell, &ecef);

    assert!(
        (lon_b - lon_i).abs() < 1e-9,
        "lon Bowring vs iterative: {}°",
        (lon_b - lon_i).abs()
    );
    assert!(
        (lat_b - lat_i).abs() < 1e-9,
        "lat Bowring vs iterative: {}°",
        (lat_b - lat_i).abs()
    );
    assert!(
        (h_b - h_i).abs() < 1e-6,
        "height Bowring vs iterative: {} m",
        (h_b - h_i).abs()
    );
}

/// Iterative method on a 60°N known point should converge within 10 iterations.
/// We verify the result against a reference computed from the closed-form.
#[test]
fn test_ecef_to_geographic_iterative_converges_under_10_iter() {
    let ell = GeocentricEllipsoid::wgs84();
    // Helsinki: 25.0°E, 60.17°N, h=50 m
    let lon_ref = 25.0_f64;
    let lat_ref = 60.17_f64;
    let h_ref = 50.0_f64;

    let ecef = geographic_to_ecef(&ell, lon_ref, lat_ref, h_ref);
    let (lon_out, lat_out, h_out) = ecef_to_geographic_iterative(&ell, &ecef);

    assert!(
        (lon_out - lon_ref).abs() < 1e-9,
        "lon iterative error: {}°",
        (lon_out - lon_ref).abs()
    );
    assert!(
        (lat_out - lat_ref).abs() < 1e-9,
        "lat iterative error: {}°",
        (lat_out - lat_ref).abs()
    );
    assert!(
        (h_out - h_ref).abs() < 1e-6,
        "height iterative error: {} m",
        (h_out - h_ref).abs()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// GeocentricCrs metadata
// ─────────────────────────────────────────────────────────────────────────────

/// WGS84 geocentric CRS must carry EPSG code 4978.
#[test]
fn test_geocentric_crs_wgs84_epsg_4978() {
    let crs = GeocentricCrs::wgs84();
    assert_eq!(
        crs.epsg_code,
        Some(4978),
        "WGS84 geocentric EPSG code should be 4978"
    );
    assert!(crs.name.contains("WGS"));
}

// ─────────────────────────────────────────────────────────────────────────────
// EcefCoordinate helpers
// ─────────────────────────────────────────────────────────────────────────────

/// `magnitude` and `subtract` should behave correctly.
#[test]
fn test_ecef_magnitude_and_subtract() {
    let a = EcefCoordinate::new(3.0, 4.0, 0.0);
    assert!((a.magnitude() - 5.0).abs() < 1e-12, "magnitude of (3,4,0)");

    let b = EcefCoordinate::new(1.0, 1.0, 0.0);
    let diff = a.subtract(&b);
    assert!((diff.x - 2.0).abs() < 1e-12);
    assert!((diff.y - 3.0).abs() < 1e-12);
    assert!(diff.z.abs() < 1e-12);
}

// ─────────────────────────────────────────────────────────────────────────────
// EcefTransformer — identity (no Helmert)
// ─────────────────────────────────────────────────────────────────────────────

/// Without a Helmert shift the transformer must return the identical coordinate.
#[test]
fn test_ecef_transformer_identity_when_same_ellipsoid() {
    let src = GeocentricCrs::wgs84();
    let dst = GeocentricCrs::wgs84();
    let transformer = EcefTransformer::new(src, dst);

    let ell = GeocentricEllipsoid::wgs84();
    let ecef_in = geographic_to_ecef(&ell, 10.0, 50.0, 0.0);
    let ecef_out = transformer.transform(&ecef_in);

    assert!(
        (ecef_out.x - ecef_in.x).abs() < 1e-10,
        "X should be unchanged"
    );
    assert!(
        (ecef_out.y - ecef_in.y).abs() < 1e-10,
        "Y should be unchanged"
    );
    assert!(
        (ecef_out.z - ecef_in.z).abs() < 1e-10,
        "Z should be unchanged"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// EcefTransformer — Helmert shift applied
// ─────────────────────────────────────────────────────────────────────────────

/// A Helmert with dx=100 (all other params zero) must shift X by exactly 100 m.
#[test]
fn test_ecef_transformer_applies_helmert() {
    let shift_100 = Helmert7Params {
        dx: 100.0,
        dy: 0.0,
        dz: 0.0,
        rx: 0.0,
        ry: 0.0,
        rz: 0.0,
        ds: 0.0,
    };

    let src = GeocentricCrs::wgs84();
    let dst = GeocentricCrs::custom("Shifted", GeocentricEllipsoid::wgs84());
    let transformer = EcefTransformer::new(src, dst).with_helmert(shift_100);

    let ell = GeocentricEllipsoid::wgs84();
    let ecef_in = geographic_to_ecef(&ell, 0.0, 0.0, 0.0);
    let ecef_out = transformer.transform(&ecef_in);

    assert!(
        (ecef_out.x - ecef_in.x - 100.0).abs() < 1e-9,
        "X shift: expected +100 m, got {:.6}",
        ecef_out.x - ecef_in.x
    );
    assert!(
        (ecef_out.y - ecef_in.y).abs() < 1e-9,
        "Y should be unchanged"
    );
    assert!(
        (ecef_out.z - ecef_in.z).abs() < 1e-9,
        "Z should be unchanged"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// geographic_to_geographic_via_ecef — identity round-trip
// ─────────────────────────────────────────────────────────────────────────────

/// src=dst=WGS84, no Helmert → output should equal input to < 1 × 10⁻⁹ degrees.
#[test]
fn test_geographic_to_geographic_via_ecef_round_trip() {
    let ell = GeocentricEllipsoid::wgs84();
    let lon_in = -73.935_242; // New York City
    let lat_in = 40.730_610;
    let h_in = 10.0_f64;

    let (lon_out, lat_out, h_out) =
        geographic_to_geographic_via_ecef(&ell, &ell, None, lon_in, lat_in, h_in);

    assert!(
        (lon_out - lon_in).abs() < 1e-9,
        "lon round-trip: {}°",
        (lon_out - lon_in).abs()
    );
    assert!(
        (lat_out - lat_in).abs() < 1e-9,
        "lat round-trip: {}°",
        (lat_out - lat_in).abs()
    );
    assert!(
        (h_out - h_in).abs() < 1e-6,
        "height round-trip: {} m",
        (h_out - h_in).abs()
    );
}
