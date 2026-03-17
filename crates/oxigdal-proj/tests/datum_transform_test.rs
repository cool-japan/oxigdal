//! Tests for `oxigdal_proj::datum_transform`.
//!
//! Covers ellipsoid parameters, ECEF conversions, Molodensky, Bursa-Wolf,
//! ITRF epoch-aware transforms, and the unified `DatumTransformer` API.

use oxigdal_proj::datum_transform::{
    BursaWolfParams, DatumTransformer, Ellipsoid, EpochTransformArgs, ItRfFrame, ItrfEpoch,
    ItrfTransformParams, MolodenskyParams, TransformMethod, ecef_to_geodetic, geodetic_to_ecef,
};

use core::f64::consts::{FRAC_PI_2, PI};

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Assert two `f64` values are within `tol` of each other.
fn assert_close(a: f64, b: f64, tol: f64, label: &str) {
    assert!(
        (a - b).abs() < tol,
        "{label}: expected {b:.12}, got {a:.12}, diff = {:.3e}",
        (a - b).abs()
    );
}

/// Convert degrees to radians for concise test expressions.
fn rad(deg: f64) -> f64 {
    deg.to_radians()
}

// ═════════════════════════════════════════════════════════════════════════════
// Ellipsoid tests
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_wgs84_semi_major_axis() {
    assert_eq!(Ellipsoid::WGS84.a, 6_378_137.0);
}

#[test]
fn test_wgs84_inv_f() {
    assert_close(Ellipsoid::WGS84.inv_f, 298.257_223_563, 1e-9, "WGS84 inv_f");
}

#[test]
fn test_wgs84_flattening() {
    let expected = 1.0 / 298.257_223_563;
    assert_close(Ellipsoid::WGS84.f(), expected, 1e-15, "WGS84 f");
}

#[test]
fn test_wgs84_semi_minor_axis() {
    // WGS84 b = 6 356 752.314 140 35 m (NIMA TR8350.2)
    assert_close(Ellipsoid::WGS84.b(), 6_356_752.314_140_35, 1e-3, "WGS84 b");
}

#[test]
fn test_wgs84_first_eccentricity_squared() {
    // WGS84 e² = 0.006 694 379 990 14
    assert_close(
        Ellipsoid::WGS84.e2(),
        0.006_694_379_990_14,
        1e-12,
        "WGS84 e²",
    );
}

#[test]
fn test_wgs84_second_eccentricity_squared() {
    let e2 = Ellipsoid::WGS84.e2();
    let expected = e2 / (1.0 - e2);
    assert_close(Ellipsoid::WGS84.e_prime2(), expected, 1e-15, "WGS84 e'²");
}

#[test]
fn test_grs80_parameters() {
    assert_eq!(Ellipsoid::GRS80.a, 6_378_137.0);
    assert_close(Ellipsoid::GRS80.inv_f, 298.257_222_101, 1e-9, "GRS80 inv_f");
}

#[test]
fn test_international_1924() {
    assert_eq!(Ellipsoid::INTERNATIONAL.a, 6_378_388.0);
    assert_close(
        Ellipsoid::INTERNATIONAL.inv_f,
        297.0,
        1e-12,
        "INTERNATIONAL inv_f",
    );
}

#[test]
fn test_bessel_1841() {
    assert_close(Ellipsoid::BESSEL.a, 6_377_397.155, 1e-3, "BESSEL a");
}

#[test]
fn test_airy_1830() {
    assert_close(Ellipsoid::AIRY.a, 6_377_563.396, 1e-3, "AIRY a");
}

#[test]
fn test_sphere_flattening_zero() {
    let sphere = Ellipsoid::new(6_371_000.0, 0.0);
    assert_eq!(sphere.f(), 0.0);
    assert_eq!(sphere.b(), sphere.a);
    assert_eq!(sphere.e2(), 0.0);
}

#[test]
fn test_n_radius_at_equator() {
    // At the equator the prime vertical radius of curvature equals a
    let n = Ellipsoid::WGS84.n_radius(0.0);
    assert_close(n, Ellipsoid::WGS84.a, 1e-6, "N at equator");
}

#[test]
fn test_n_radius_at_pole() {
    // At the pole N = a / sqrt(1 - e²) = a²/b
    let e = Ellipsoid::WGS84;
    let expected = e.a * e.a / e.b();
    let n = e.n_radius(FRAC_PI_2);
    assert_close(n, expected, 1e-3, "N at pole");
}

#[test]
fn test_m_radius_less_than_n() {
    // M ≤ N always; equality at poles
    let lat = rad(45.0);
    let m = Ellipsoid::WGS84.m_radius(lat);
    let n = Ellipsoid::WGS84.n_radius(lat);
    assert!(m < n, "M should be less than N at mid-latitudes");
}

#[test]
fn test_clarke1866_parameters() {
    assert_close(Ellipsoid::CLARKE1866.a, 6_378_206.4, 0.1, "Clarke a");
    assert_close(
        Ellipsoid::CLARKE1866.inv_f,
        294.978_698_2,
        1e-6,
        "Clarke inv_f",
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// geodetic_to_ecef tests
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_geodetic_to_ecef_equator_origin() {
    // lat=0, lon=0, h=0 → (a, 0, 0)
    let (x, y, z) = geodetic_to_ecef(0.0, 0.0, 0.0, &Ellipsoid::WGS84);
    assert_close(x, Ellipsoid::WGS84.a, 1e-4, "equator X");
    assert_close(y, 0.0, 1e-4, "equator Y");
    assert_close(z, 0.0, 1e-4, "equator Z");
}

#[test]
fn test_geodetic_to_ecef_equator_east() {
    // lat=0, lon=90°, h=0 → (0, a, 0)
    let (x, y, z) = geodetic_to_ecef(0.0, rad(90.0), 0.0, &Ellipsoid::WGS84);
    assert_close(x, 0.0, 1e-4, "lon90 X");
    assert_close(y, Ellipsoid::WGS84.a, 1e-4, "lon90 Y");
    assert_close(z, 0.0, 1e-4, "lon90 Z");
}

#[test]
fn test_geodetic_to_ecef_north_pole() {
    // lat=90°, lon=0, h=0 → (0, 0, b)
    let (x, y, z) = geodetic_to_ecef(FRAC_PI_2, 0.0, 0.0, &Ellipsoid::WGS84);
    assert_close(x, 0.0, 1e-4, "pole X");
    assert_close(y, 0.0, 1e-4, "pole Y");
    assert_close(z, Ellipsoid::WGS84.b(), 1e-3, "pole Z");
}

#[test]
fn test_geodetic_to_ecef_south_pole() {
    let (x, y, z) = geodetic_to_ecef(-FRAC_PI_2, 0.0, 0.0, &Ellipsoid::WGS84);
    assert_close(x, 0.0, 1e-4, "south pole X");
    assert_close(y, 0.0, 1e-4, "south pole Y");
    assert_close(z, -Ellipsoid::WGS84.b(), 1e-3, "south pole Z");
}

#[test]
fn test_geodetic_to_ecef_with_height() {
    // At equator with h=1000 m, X should be a+1000
    let (x, _y, _z) = geodetic_to_ecef(0.0, 0.0, 1000.0, &Ellipsoid::WGS84);
    assert_close(x, Ellipsoid::WGS84.a + 1000.0, 1e-4, "equator h=1000 X");
}

#[test]
fn test_geodetic_to_ecef_london() {
    // London (51.5°N, 0°E) — cross-check with known values
    let (x, y, z) = geodetic_to_ecef(rad(51.5), 0.0, 0.0, &Ellipsoid::WGS84);
    // X ~ 3978924 m, Y ~ 0 m, Z ~ 4968040 m
    assert!(x > 3_900_000.0 && x < 4_100_000.0, "London X={x}");
    assert_close(y, 0.0, 1e-1, "London Y");
    assert!(z > 4_900_000.0 && z < 5_100_000.0, "London Z={z}");
}

// ═════════════════════════════════════════════════════════════════════════════
// ecef_to_geodetic tests
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_ecef_to_geodetic_round_trip_equator() {
    let lat_in = 0.0_f64;
    let lon_in = 0.0_f64;
    let h_in = 0.0_f64;
    let (x, y, z) = geodetic_to_ecef(lat_in, lon_in, h_in, &Ellipsoid::WGS84);
    let (lat_out, lon_out, h_out) = ecef_to_geodetic(x, y, z, &Ellipsoid::WGS84);
    assert_close(lat_out, lat_in, 1e-10, "round-trip lat equator");
    assert_close(lon_out, lon_in, 1e-10, "round-trip lon equator");
    assert_close(h_out, h_in, 1e-4, "round-trip h equator");
}

#[test]
fn test_ecef_to_geodetic_round_trip_london() {
    let lat_in = rad(51.5);
    let lon_in = rad(-0.1);
    let h_in = 50.0;
    let (x, y, z) = geodetic_to_ecef(lat_in, lon_in, h_in, &Ellipsoid::WGS84);
    let (lat_out, lon_out, h_out) = ecef_to_geodetic(x, y, z, &Ellipsoid::WGS84);
    assert_close(lat_out, lat_in, 1e-9, "round-trip lat London");
    assert_close(lon_out, lon_in, 1e-9, "round-trip lon London");
    assert_close(h_out, h_in, 1e-3, "round-trip h London");
}

#[test]
fn test_ecef_to_geodetic_round_trip_tokyo() {
    let lat_in = rad(35.68);
    let lon_in = rad(139.69);
    let h_in = 40.0;
    let (x, y, z) = geodetic_to_ecef(lat_in, lon_in, h_in, &Ellipsoid::WGS84);
    let (lat_out, lon_out, h_out) = ecef_to_geodetic(x, y, z, &Ellipsoid::WGS84);
    assert_close(lat_out, lat_in, 1e-9, "round-trip lat Tokyo");
    assert_close(lon_out, lon_in, 1e-9, "round-trip lon Tokyo");
    assert_close(h_out, h_in, 1e-3, "round-trip h Tokyo");
}

#[test]
fn test_ecef_to_geodetic_round_trip_south_america() {
    let lat_in = rad(-23.5);
    let lon_in = rad(-46.6);
    let h_in = 760.0;
    let (x, y, z) = geodetic_to_ecef(lat_in, lon_in, h_in, &Ellipsoid::WGS84);
    let (lat_out, lon_out, h_out) = ecef_to_geodetic(x, y, z, &Ellipsoid::WGS84);
    assert_close(lat_out, lat_in, 1e-9, "round-trip lat Sao Paulo");
    assert_close(lon_out, lon_in, 1e-9, "round-trip lon Sao Paulo");
    assert_close(h_out, h_in, 1e-3, "round-trip h Sao Paulo");
}

#[test]
fn test_ecef_to_geodetic_high_altitude() {
    let lat_in = rad(45.0);
    let lon_in = rad(10.0);
    let h_in = 10_000.0; // 10 km altitude
    let (x, y, z) = geodetic_to_ecef(lat_in, lon_in, h_in, &Ellipsoid::WGS84);
    let (lat_out, lon_out, h_out) = ecef_to_geodetic(x, y, z, &Ellipsoid::WGS84);
    assert_close(lat_out, lat_in, 1e-9, "round-trip lat high alt");
    assert_close(lon_out, lon_in, 1e-9, "round-trip lon high alt");
    assert_close(h_out, h_in, 1e-3, "round-trip h high alt");
}

#[test]
fn test_ecef_to_geodetic_longitude_range() {
    // Test that longitudes spanning the full range are recovered correctly
    for lon_deg in [-179.0_f64, -90.0, 0.0, 90.0, 179.9] {
        let lat_in = rad(20.0);
        let lon_in = rad(lon_deg);
        let (x, y, z) = geodetic_to_ecef(lat_in, lon_in, 0.0, &Ellipsoid::WGS84);
        let (_, lon_out, _) = ecef_to_geodetic(x, y, z, &Ellipsoid::WGS84);
        assert_close(lon_out, lon_in, 1e-9, &format!("lon={lon_deg}°"));
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// MolodenskyParams tests
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_molodensky_new_computes_da_df() {
    let p = MolodenskyParams::new(0.0, 0.0, 0.0, &Ellipsoid::WGS84, &Ellipsoid::INTERNATIONAL);
    let expected_da = Ellipsoid::INTERNATIONAL.a - Ellipsoid::WGS84.a;
    let expected_df = Ellipsoid::INTERNATIONAL.f() - Ellipsoid::WGS84.f();
    assert_close(p.da, expected_da, 0.001, "Molodensky da");
    assert_close(p.df, expected_df, 1e-12, "Molodensky df");
}

#[test]
fn test_molodensky_identity_zero_shift() {
    // With all zeros and same source/target ellipsoid, output == input
    let p = MolodenskyParams {
        dx: 0.0,
        dy: 0.0,
        dz: 0.0,
        da: 0.0,
        df: 0.0,
    };
    let lat = rad(51.0);
    let lon = rad(-1.0);
    let h = 100.0;
    let (lat_out, lon_out, h_out) = p.transform(lat, lon, h, &Ellipsoid::WGS84);
    assert_close(lat_out, lat, 1e-10, "identity lat");
    assert_close(lon_out, lon, 1e-10, "identity lon");
    assert_close(h_out, h, 1e-6, "identity h");
}

#[test]
fn test_molodensky_abridged_identity_zero_shift() {
    let p = MolodenskyParams {
        dx: 0.0,
        dy: 0.0,
        dz: 0.0,
        da: 0.0,
        df: 0.0,
    };
    let lat = rad(40.0);
    let lon = rad(20.0);
    let h = 200.0;
    let (lat_out, lon_out, h_out) = p.transform_abridged(lat, lon, h, &Ellipsoid::WGS84);
    assert_close(lat_out, lat, 1e-10, "abridged identity lat");
    assert_close(lon_out, lon, 1e-10, "abridged identity lon");
    assert_close(h_out, h, 1e-6, "abridged identity h");
}

#[test]
fn test_molodensky_wgs84_to_ed50_nontrivial() {
    let (params, src, tgt) = MolodenskyParams::wgs84_to_ed50();
    let lat = rad(48.0); // Paris area
    let lon = rad(2.3);
    let h = 100.0;
    let (lat_out, lon_out, h_out) = params.transform(lat, lon, h, src);
    // ED50 should differ from WGS84 by a detectable amount
    assert!(
        (lat_out - lat).abs() > 1e-6,
        "ED50 lat shift should be non-trivial"
    );
    assert!(
        (lon_out - lon).abs() > 1e-6,
        "ED50 lon shift should be non-trivial"
    );
    // Suppress unused warning
    let _ = tgt;
    let _ = h_out;
}

#[test]
fn test_molodensky_abridged_close_to_full() {
    // Abridged and full Molodensky should agree within ~10 m for typical shifts
    let (params, src, _) = MolodenskyParams::wgs84_to_ed50();
    let lat = rad(48.0);
    let lon = rad(2.3);
    let h = 100.0;
    let (lat_f, lon_f, h_f) = params.transform(lat, lon, h, src);
    let (lat_a, lon_a, h_a) = params.transform_abridged(lat, lon, h, src);
    // Should agree to sub-arcsecond (< 1e-5 rad ~ 30 m at the equator)
    assert!((lat_f - lat_a).abs() < 1e-5, "full vs abridged lat diff");
    assert!((lon_f - lon_a).abs() < 1e-5, "full vs abridged lon diff");
    assert!((h_f - h_a).abs() < 20.0, "full vs abridged h diff");
}

#[test]
fn test_molodensky_wgs84_to_tokyo_nontrivial() {
    let (params, src, _tgt) = MolodenskyParams::wgs84_to_tokyo();
    let lat = rad(35.6);
    let lon = rad(139.7);
    let h = 50.0;
    let (lat_out, lon_out, _) = params.transform(lat, lon, h, src);
    // Tokyo datum shift from WGS84 should be significant (several arc-seconds)
    assert!((lat_out - lat).abs() > 1e-5, "Tokyo lat shift");
    assert!((lon_out - lon).abs() > 1e-5, "Tokyo lon shift");
}

// ═════════════════════════════════════════════════════════════════════════════
// BursaWolfParams tests
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_bursa_wolf_identity_zero_params() {
    let bw = BursaWolfParams::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let (x, y, z) = (1_000_000.0, 2_000_000.0, 3_000_000.0);
    let (xo, yo, zo) = bw.transform_ecef(x, y, z);
    assert_close(xo, x, 1e-6, "BW identity X");
    assert_close(yo, y, 1e-6, "BW identity Y");
    assert_close(zo, z, 1e-6, "BW identity Z");
}

#[test]
fn test_bursa_wolf_pure_translation() {
    let bw = BursaWolfParams::new(100.0, 200.0, 300.0, 0.0, 0.0, 0.0, 0.0);
    let (xo, yo, zo) = bw.transform_ecef(0.0, 0.0, 0.0);
    assert_close(xo, 100.0, 1e-10, "pure tx");
    assert_close(yo, 200.0, 1e-10, "pure ty");
    assert_close(zo, 300.0, 1e-10, "pure tz");
}

#[test]
fn test_bursa_wolf_scale_only() {
    let bw = BursaWolfParams::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0); // +1 ppm
    let (xo, yo, zo) = bw.transform_ecef(1_000_000.0, 0.0, 0.0);
    // scale = 1 + 1e-6, so X_out = 1_000_000 * (1 + 1e-6) = 1_000_001
    assert_close(xo, 1_000_001.0, 1e-3, "scale-only X");
    assert_close(yo, 0.0, 1e-10, "scale-only Y");
    assert_close(zo, 0.0, 1e-10, "scale-only Z");
}

#[test]
fn test_bursa_wolf_inverse_is_near_identity() {
    let bw = BursaWolfParams::osgb36_to_wgs84();
    let bw_inv = bw.inverse();
    let (x, y, z) = (3_900_000.0, -50_000.0, 5_000_000.0);
    let (xp, yp, zp) = bw.transform_ecef(x, y, z);
    let (xr, yr, zr) = bw_inv.transform_ecef(xp, yp, zp);
    // After forward + inverse should be near original (within 1 cm for sub-metre params)
    assert_close(xr, x, 0.1, "OSGB36 forward+inverse X");
    assert_close(yr, y, 0.1, "OSGB36 forward+inverse Y");
    assert_close(zr, z, 0.1, "OSGB36 forward+inverse Z");
}

#[test]
fn test_bursa_wolf_osgb36_to_wgs84_shifts_uk_coords() {
    // Greenwich Observatory: approximately 51.4778°N, 0.0°E
    let bw = BursaWolfParams::osgb36_to_wgs84();
    let (lat_out, lon_out, _) = bw.transform_geodetic(
        rad(51.4778),
        rad(0.0),
        0.0,
        &Ellipsoid::AIRY,
        &Ellipsoid::WGS84,
    );
    // WGS84 result should be in a plausible UK range
    let lat_d = lat_out.to_degrees();
    let lon_d = lon_out.to_degrees();
    assert!(
        lat_d > 49.0 && lat_d < 62.0,
        "OSGB36 lat in UK range: {lat_d}"
    );
    assert!(
        lon_d > -9.0 && lon_d < 3.0,
        "OSGB36 lon in UK range: {lon_d}"
    );
}

#[test]
fn test_bursa_wolf_transform_geodetic_round_trip() {
    // Forward then inverse should return near the original point
    let bw = BursaWolfParams::osgb36_to_wgs84();
    let bw_inv = bw.inverse();
    let lat = rad(52.2);
    let lon = rad(-1.5);
    let h = 100.0;
    let (lat_t, lon_t, h_t) =
        bw.transform_geodetic(lat, lon, h, &Ellipsoid::AIRY, &Ellipsoid::WGS84);
    let (lat_r, lon_r, h_r) =
        bw_inv.transform_geodetic(lat_t, lon_t, h_t, &Ellipsoid::WGS84, &Ellipsoid::AIRY);
    assert_close(lat_r, lat, 1e-6, "geodetic round-trip lat");
    assert_close(lon_r, lon, 1e-6, "geodetic round-trip lon");
    assert_close(h_r, h, 1.0, "geodetic round-trip h");
}

#[test]
fn test_bursa_wolf_ed50_to_wgs84_constructed() {
    let bw = BursaWolfParams::ed50_to_wgs84();
    assert_close(bw.tx, -89.5, 1e-10, "ED50 tx");
    assert_close(bw.ty, -93.8, 1e-10, "ED50 ty");
    assert_close(bw.tz, -123.1, 1e-10, "ED50 tz");
}

#[test]
fn test_bursa_wolf_tokyo_to_wgs84_constructed() {
    let bw = BursaWolfParams::tokyo_to_wgs84();
    assert_close(bw.tx, -146.414, 1e-3, "Tokyo tx");
    assert_close(bw.ty, 507.337, 1e-3, "Tokyo ty");
    assert_close(bw.tz, 680.507, 1e-3, "Tokyo tz");
    assert_eq!(bw.rx, 0.0);
    assert_eq!(bw.ry, 0.0);
    assert_eq!(bw.rz, 0.0);
}

#[test]
fn test_bursa_wolf_nad27_conus_constructed() {
    let bw = BursaWolfParams::nad27_to_wgs84_conus();
    assert_close(bw.tx, -8.0, 1e-10, "NAD27 tx");
    assert_close(bw.ty, 160.0, 1e-10, "NAD27 ty");
    assert_close(bw.tz, 176.0, 1e-10, "NAD27 tz");
}

#[test]
fn test_bursa_wolf_osgb36_parameter_signs() {
    let bw = BursaWolfParams::osgb36_to_wgs84();
    // Published EPSG:1314 parameters
    assert!(bw.tx > 400.0 && bw.tx < 500.0, "OSGB36 tx={}", bw.tx);
    assert!(bw.ty < -100.0, "OSGB36 ty={}", bw.ty);
    assert!(bw.tz > 500.0, "OSGB36 tz={}", bw.tz);
    assert!(bw.ds < -15.0, "OSGB36 ds={}", bw.ds);
}

// ═════════════════════════════════════════════════════════════════════════════
// ItRfFrame tests
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_itrf_frame_names() {
    assert_eq!(ItRfFrame::Itrf2020.name(), "ITRF2020");
    assert_eq!(ItRfFrame::Itrf2014.name(), "ITRF2014");
    assert_eq!(ItRfFrame::Itrf2008.name(), "ITRF2008");
    assert_eq!(ItRfFrame::Itrf2005.name(), "ITRF2005");
    assert_eq!(ItRfFrame::Itrf2000.name(), "ITRF2000");
    assert_eq!(ItRfFrame::Itrf97.name(), "ITRF97");
    assert_eq!(ItRfFrame::Itrf96.name(), "ITRF96");
    assert_eq!(ItRfFrame::Gda2020.name(), "GDA2020");
    assert_eq!(ItRfFrame::Gda94.name(), "GDA94");
}

#[test]
fn test_itrf_frame_reference_epochs() {
    assert_close(
        ItRfFrame::Itrf2020.reference_epoch(),
        2015.0,
        1e-10,
        "ITRF2020 epoch",
    );
    assert_close(
        ItRfFrame::Itrf2014.reference_epoch(),
        2010.0,
        1e-10,
        "ITRF2014 epoch",
    );
    assert_close(
        ItRfFrame::Itrf2008.reference_epoch(),
        2005.0,
        1e-10,
        "ITRF2008 epoch",
    );
    assert_close(
        ItRfFrame::Itrf2005.reference_epoch(),
        2000.0,
        1e-10,
        "ITRF2005 epoch",
    );
    assert_close(
        ItRfFrame::Itrf2000.reference_epoch(),
        1997.0,
        1e-10,
        "ITRF2000 epoch",
    );
    assert_close(
        ItRfFrame::Gda2020.reference_epoch(),
        2020.0,
        1e-10,
        "GDA2020 epoch",
    );
    assert_close(
        ItRfFrame::Gda94.reference_epoch(),
        1994.0,
        1e-10,
        "GDA94 epoch",
    );
}

#[test]
fn test_itrf_epoch_creation() {
    let ep = ItrfEpoch::new(ItRfFrame::Itrf2014, 2022.5);
    assert_eq!(ep.frame, ItRfFrame::Itrf2014);
    assert_close(ep.epoch, 2022.5, 1e-10, "epoch value");
}

// ═════════════════════════════════════════════════════════════════════════════
// ItrfTransformParams tests
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_itrf_params_at_reference_epoch_equals_base() {
    let p = ItrfTransformParams::itrf2014_to_itrf2008();
    let ref_epoch = ItRfFrame::Itrf2014.reference_epoch();
    let bw = p.params_at_epoch(ref_epoch, ref_epoch);
    assert_close(bw.tx, p.bursa_wolf.tx, 1e-15, "params_at_ref tx");
    assert_close(bw.ty, p.bursa_wolf.ty, 1e-15, "params_at_ref ty");
    assert_close(bw.tz, p.bursa_wolf.tz, 1e-15, "params_at_ref tz");
}

#[test]
fn test_itrf_zero_rates_gives_same_at_any_epoch() {
    // If all rates are zero, transform at any epoch should equal transform at reference epoch
    let bw = BursaWolfParams::new(0.001, 0.002, 0.003, 0.0, 0.0, 0.0, 0.0);
    let p = ItrfTransformParams::new(bw, [0.0; 7]);
    let lat = rad(45.0);
    let lon = rad(10.0);
    let h = 100.0;
    let (lat_a, lon_a, h_a) = p.transform_at_epoch(EpochTransformArgs::new(
        lat,
        lon,
        h,
        &Ellipsoid::WGS84,
        &Ellipsoid::WGS84,
        2010.0,
        2010.0,
    ));
    let (lat_b, lon_b, h_b) = p.transform_at_epoch(EpochTransformArgs::new(
        lat,
        lon,
        h,
        &Ellipsoid::WGS84,
        &Ellipsoid::WGS84,
        2010.0,
        2025.0,
    ));
    assert_close(lat_a, lat_b, 1e-12, "zero-rates lat independence");
    assert_close(lon_a, lon_b, 1e-12, "zero-rates lon independence");
    assert_close(h_a, h_b, 1e-8, "zero-rates h independence");
}

#[test]
fn test_itrf2014_to_itrf2008_params_small() {
    let p = ItrfTransformParams::itrf2014_to_itrf2008();
    // Translation parameters should be very small (millimetre level)
    assert!(p.bursa_wolf.tx.abs() < 0.01, "ITRF2014→2008 tx < 1 cm");
    assert!(p.bursa_wolf.ty.abs() < 0.01, "ITRF2014→2008 ty < 1 cm");
    assert!(p.bursa_wolf.tz.abs() < 0.01, "ITRF2014→2008 tz < 1 cm");
}

#[test]
fn test_itrf2008_to_itrf2005_constructed() {
    let p = ItrfTransformParams::itrf2008_to_itrf2005();
    // tx should be negative (frame shift direction)
    assert!(p.bursa_wolf.tx < 0.0, "ITRF2008→2005 tx<0");
}

#[test]
fn test_itrf_transform_at_epoch_nontrivial_with_rates() {
    // A transform with non-zero rates should produce different results at different epochs
    let bw = BursaWolfParams::new(0.01, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let rates = [0.001, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]; // 1 mm/yr in x
    let p = ItrfTransformParams::new(bw, rates);
    let lat = rad(45.0);
    let lon = rad(0.0);
    let h = 0.0;
    let (lat_2010, lon_2010, _) = p.transform_at_epoch(EpochTransformArgs::new(
        lat,
        lon,
        h,
        &Ellipsoid::WGS84,
        &Ellipsoid::WGS84,
        2010.0,
        2010.0,
    ));
    let (lat_2030, lon_2030, _) = p.transform_at_epoch(EpochTransformArgs::new(
        lat,
        lon,
        h,
        &Ellipsoid::WGS84,
        &Ellipsoid::WGS84,
        2010.0,
        2030.0,
    ));
    // 20-year accumulation at 1 mm/yr = 2 cm — should produce a detectable difference
    assert!(
        (lat_2030 - lat_2010).abs() + (lon_2030 - lon_2010).abs() > 1e-10,
        "nonzero rates produce change over 20 years"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// DatumTransformer tests
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn test_datum_transformer_identity() {
    let t = DatumTransformer::new(
        TransformMethod::Identity,
        Ellipsoid::WGS84,
        Ellipsoid::WGS84,
    );
    let (lat_out, lon_out, h_out) = t.transform_degrees(51.5, -0.1, 100.0);
    assert_close(lat_out, 51.5, 1e-10, "identity lat deg");
    assert_close(lon_out, -0.1, 1e-10, "identity lon deg");
    assert_close(h_out, 100.0, 1e-10, "identity h");
}

#[test]
fn test_datum_transformer_identity_accuracy() {
    let t = DatumTransformer::new(
        TransformMethod::Identity,
        Ellipsoid::WGS84,
        Ellipsoid::WGS84,
    );
    assert_close(t.accuracy_meters(), 0.0, 1e-10, "identity accuracy");
}

#[test]
fn test_datum_transformer_molodensky_accuracy() {
    let params = MolodenskyParams {
        dx: 0.0,
        dy: 0.0,
        dz: 0.0,
        da: 0.0,
        df: 0.0,
    };
    let t = DatumTransformer::new(
        TransformMethod::Molodensky(params),
        Ellipsoid::WGS84,
        Ellipsoid::INTERNATIONAL,
    );
    assert_close(
        t.accuracy_meters(),
        1.0,
        1e-10,
        "Molodensky accuracy estimate",
    );
}

#[test]
fn test_datum_transformer_bursa_wolf_accuracy() {
    let bw = BursaWolfParams::osgb36_to_wgs84();
    let t = DatumTransformer::new(
        TransformMethod::BursaWolf(bw),
        Ellipsoid::AIRY,
        Ellipsoid::WGS84,
    );
    assert_close(
        t.accuracy_meters(),
        0.5,
        1e-10,
        "BursaWolf accuracy estimate",
    );
}

#[test]
fn test_datum_transformer_itrf_accuracy() {
    let p = ItrfTransformParams::itrf2014_to_itrf2008();
    let t = DatumTransformer::new(
        TransformMethod::Itrf(p, 2020.0),
        Ellipsoid::WGS84,
        Ellipsoid::GRS80,
    );
    assert_close(t.accuracy_meters(), 0.01, 1e-10, "ITRF accuracy estimate");
}

#[test]
fn test_datum_transformer_transform_degrees_uses_degrees() {
    // Verify degrees ↔ radians conversion is wired correctly
    let bw = BursaWolfParams::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0); // identity shift
    let t = DatumTransformer::new(
        TransformMethod::BursaWolf(bw),
        Ellipsoid::WGS84,
        Ellipsoid::WGS84,
    );
    let (lat_out, lon_out, h_out) = t.transform_degrees(45.0, 10.0, 500.0);
    // Identity Helmert should return the same point (within ECEF round-trip error)
    assert_close(lat_out, 45.0, 1e-6, "BW identity lat deg");
    assert_close(lon_out, 10.0, 1e-6, "BW identity lon deg");
    assert_close(h_out, 500.0, 1e-3, "BW identity h");
}

#[test]
fn test_datum_transformer_batch_processing() {
    let t = DatumTransformer::new(
        TransformMethod::Identity,
        Ellipsoid::WGS84,
        Ellipsoid::WGS84,
    );
    let points = vec![
        (51.5, -0.1, 0.0),
        (35.68, 139.69, 40.0),
        (-23.5, -46.6, 760.0),
    ];
    let out = t.transform_batch(&points);
    assert_eq!(out.len(), 3, "batch returns same count");
    for (i, (&(lat_in, lon_in, h_in), &(lat_out, lon_out, h_out))) in
        points.iter().zip(out.iter()).enumerate()
    {
        assert_close(lat_out, lat_in, 1e-10, &format!("batch lat [{i}]"));
        assert_close(lon_out, lon_in, 1e-10, &format!("batch lon [{i}]"));
        assert_close(h_out, h_in, 1e-10, &format!("batch h [{i}]"));
    }
}

#[test]
fn test_datum_transformer_batch_empty_input() {
    let t = DatumTransformer::new(
        TransformMethod::Identity,
        Ellipsoid::WGS84,
        Ellipsoid::WGS84,
    );
    let out = t.transform_batch(&[]);
    assert!(out.is_empty(), "empty batch returns empty vec");
}

#[test]
fn test_datum_transformer_osgb36_greenwich() {
    // Greenwich Observatory in OSGB36 is approximately (51.4778°N, 0°E)
    // After OSGB36 → WGS84 the longitude should shift very slightly westward
    // and the latitude should remain in a plausible range.
    let bw = BursaWolfParams::osgb36_to_wgs84();
    let t = DatumTransformer::new(
        TransformMethod::BursaWolf(bw),
        Ellipsoid::AIRY,
        Ellipsoid::WGS84,
    );
    let (lat_wgs, lon_wgs, _) = t.transform_degrees(51.4778, 0.0, 0.0);
    assert!(
        lat_wgs > 51.4 && lat_wgs < 51.6,
        "Greenwich lat in range: {lat_wgs}"
    );
    // Longitude shift is a few arc-seconds westward
    assert!(
        lon_wgs > -0.01 && lon_wgs < 0.01,
        "Greenwich lon shift small: {lon_wgs}"
    );
}

#[test]
fn test_datum_transformer_molodensky_produces_shift() {
    let (params, _src, _tgt) = MolodenskyParams::wgs84_to_ed50();
    let t = DatumTransformer::new(
        TransformMethod::Molodensky(params),
        Ellipsoid::WGS84,
        Ellipsoid::INTERNATIONAL,
    );
    let (lat_out, lon_out, _) = t.transform_degrees(48.0, 2.3, 100.0);
    // Non-trivial shift expected
    assert!(
        (lat_out - 48.0).abs() > 1e-4,
        "Molodensky produces lat shift"
    );
    assert!(
        (lon_out - 2.3).abs() > 1e-4,
        "Molodensky produces lon shift"
    );
}

#[test]
fn test_datum_transformer_itrf_epoch_shift() {
    let p = ItrfTransformParams::itrf2014_to_itrf2008();
    let t = DatumTransformer::new(
        TransformMethod::Itrf(p, 2020.0),
        Ellipsoid::GRS80,
        Ellipsoid::GRS80,
    );
    // Should not panic and should produce a plausible result
    let (lat_out, lon_out, h_out) = t.transform_degrees(45.0, 10.0, 100.0);
    assert!(
        lat_out > 44.0 && lat_out < 46.0,
        "ITRF lat plausible: {lat_out}"
    );
    assert!(
        lon_out > 9.0 && lon_out < 11.0,
        "ITRF lon plausible: {lon_out}"
    );
    assert!(h_out > 99.0 && h_out < 101.0, "ITRF h plausible: {h_out}");
}

#[test]
fn test_bursa_wolf_params_new_roundtrip() {
    let bw = BursaWolfParams::new(1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0);
    assert_close(bw.tx, 1.0, 1e-15, "new tx");
    assert_close(bw.ty, 2.0, 1e-15, "new ty");
    assert_close(bw.tz, 3.0, 1e-15, "new tz");
    assert_close(bw.rx, 4.0, 1e-15, "new rx");
    assert_close(bw.ry, 5.0, 1e-15, "new ry");
    assert_close(bw.rz, 6.0, 1e-15, "new rz");
    assert_close(bw.ds, 7.0, 1e-15, "new ds");
}

#[test]
fn test_bursa_wolf_rotation_only() {
    // A small Z rotation should mix X and Y components
    let bw = BursaWolfParams::new(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0); // 1 arcsec Rz
    let (xo, yo, zo) = bw.transform_ecef(1_000_000.0, 0.0, 0.0);
    // rz * y term: y=0 so no change; rz * x term mixes into y
    // xo ≈ x (no Y-contribution), yo ≈ -rz*x (small negative), zo ≈ z
    assert_close(xo, 1_000_000.0, 1.0, "Rz rotation X approx unchanged");
    assert!(yo.abs() > 0.001, "Rz rotation mixes into Y: yo={yo}");
    assert_close(zo, 0.0, 1e-10, "Rz rotation Z unchanged");
}

#[test]
fn test_ellipsoid_b_relationship() {
    // b = a * (1 - f) must hold for all predefined ellipsoids
    for e in [
        &Ellipsoid::WGS84,
        &Ellipsoid::GRS80,
        &Ellipsoid::INTERNATIONAL,
        &Ellipsoid::BESSEL,
        &Ellipsoid::AIRY,
    ] {
        let b_computed = e.a * (1.0 - e.f());
        assert_close(
            e.b(),
            b_computed,
            1e-6,
            &format!("{} b relationship", e.name),
        );
    }
}

#[test]
fn test_ellipsoid_e2_formula() {
    // e² = 2f - f² must hold for WGS84
    let f = Ellipsoid::WGS84.f();
    let e2_manual = 2.0 * f - f * f;
    assert_close(
        Ellipsoid::WGS84.e2(),
        e2_manual,
        1e-15,
        "WGS84 e2 formula check",
    );
}

#[test]
fn test_geodetic_ecef_multiple_round_trips() {
    // Exhaustive round-trip check at a grid of lat/lon/h values
    let lats = [-80.0_f64, -45.0, 0.0, 45.0, 80.0];
    let lons = [-170.0_f64, -90.0, 0.0, 90.0, 170.0];
    let heights = [0.0_f64, 1000.0, 10_000.0];
    for &lat_d in &lats {
        for &lon_d in &lons {
            for &h in &heights {
                let lat_r = rad(lat_d);
                let lon_r = rad(lon_d);
                let (x, y, z) = geodetic_to_ecef(lat_r, lon_r, h, &Ellipsoid::WGS84);
                let (lat_o, lon_o, h_o) = ecef_to_geodetic(x, y, z, &Ellipsoid::WGS84);
                assert_close(
                    lat_o,
                    lat_r,
                    1e-9,
                    &format!("grid lat={lat_d} lon={lon_d} h={h}"),
                );
                assert_close(
                    lon_o,
                    lon_r,
                    1e-9,
                    &format!("grid lon lat={lat_d} lon={lon_d} h={h}"),
                );
                assert_close(
                    h_o,
                    h,
                    1e-3,
                    &format!("grid h lat={lat_d} lon={lon_d} h={h}"),
                );
            }
        }
    }
}

#[test]
fn test_datum_transformer_clone() {
    let bw = BursaWolfParams::osgb36_to_wgs84();
    let t = DatumTransformer::new(
        TransformMethod::BursaWolf(bw),
        Ellipsoid::AIRY,
        Ellipsoid::WGS84,
    );
    let t2 = t.clone();
    let (la, lo, _ha) = t.transform_degrees(52.0, -1.0, 0.0);
    let (lb, lob, _hb) = t2.transform_degrees(52.0, -1.0, 0.0);
    assert_close(la, lb, 1e-12, "clone lat");
    assert_close(lo, lob, 1e-12, "clone lon");
}

#[test]
fn test_itrf_params_rates_length() {
    let p = ItrfTransformParams::itrf2014_to_itrf2008();
    assert_eq!(p.rates.len(), 7, "rates has 7 elements");
}

#[test]
fn test_molodensky_geographic_coords_remain_in_range() {
    let (params, src, _) = MolodenskyParams::wgs84_to_ed50();
    for lat_d in [-60.0_f64, 0.0, 60.0] {
        for lon_d in [-180.0_f64, 0.0, 180.0] {
            let (lat_out, lon_out, _) = params.transform(rad(lat_d), rad(lon_d), 0.0, src);
            assert!(
                lat_out.abs() <= PI / 2.0 + 0.01,
                "lat stays near valid range: {lat_out}"
            );
            assert!(
                lon_out.abs() <= PI + 0.1,
                "lon stays near valid range: {lon_out}"
            );
        }
    }
}
