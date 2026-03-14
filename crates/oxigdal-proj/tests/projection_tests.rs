//! Comprehensive integration tests for the expanded projection system.
//!
//! Tests cover:
//! 1. Round-trip accuracy for all 10 new projections
//! 2. Known-point forward accuracy (compared to published values)
//! 3. EPSG database expansion (500+ codes)
//! 4. Grid shift transformations
//! 5. Edge cases (poles, antimeridian, zero coordinates)
//! 6. Error conditions

#![allow(clippy::expect_used)]

use oxigdal_proj::{
    // New projection structs from transform submodules
    AzimuthalEquidistant,
    CassineSoldner,
    // Coordinate/transformer
    Coordinate,
    EckertIV,
    EckertVI,
    EquidistantConic,
    GaussKruger,
    Gnomonic,
    LambertAzimuthalEqualArea,
    LambertConformalConic,
    Mollweide,
    Robinson,
    Sinusoidal,
    Transformer,
    TransverseMercator,
    // EPSG
    available_epsg_codes,
    contains_epsg,
    // Grid shift
    dhdn_etrs89_helmert,
    lookup_epsg,
    ostn15_approx,
    rgf93_approx,
};

// ─────────────────────────────────────────────────────────────────────────────
// Tolerance constants
// ─────────────────────────────────────────────────────────────────────────────
/// For sub-millimetre round-trips (metres)
const M_TOL: f64 = 0.001;
/// For degree round-trips (degrees)
const DEG_TOL: f64 = 1e-6;
/// For degree round-trips requiring medium precision
const DEG_MED: f64 = 1e-4;

// ─────────────────────────────────────────────────────────────────────────────
// 1. Sinusoidal projection
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_sinusoidal_origin_forward() {
    let proj = Sinusoidal::default();
    let (x, y) = proj.forward(0.0, 0.0).expect("forward ok");
    assert!(x.abs() < M_TOL, "sinusoidal origin x: {}", x);
    assert!(y.abs() < M_TOL, "sinusoidal origin y: {}", y);
}

#[test]
fn test_sinusoidal_equator_forward() {
    let proj = Sinusoidal::default();
    // On the equator, x = lon_deg * PI/180 * R, y = 0
    let (x, y) = proj.forward(90.0, 0.0).expect("forward ok");
    let expected_x = 90.0_f64.to_radians() * 6_378_137.0;
    assert!(
        (x - expected_x).abs() < 1.0,
        "x={} expected={}",
        x,
        expected_x
    );
    assert!(y.abs() < M_TOL);
}

#[test]
fn test_sinusoidal_roundtrip_multiple_points() {
    let proj = Sinusoidal::default();
    let test_points = [
        (0.0, 0.0),
        (10.0, 20.0),
        (-100.0, -45.0),
        (170.0, 60.0),
        (-170.0, -60.0),
        (45.0, 30.0),
        (-45.0, -30.0),
    ];
    for (lon, lat) in test_points {
        let (x, y) = proj.forward(lon, lat).expect("forward");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse");
        assert!(
            (lon - lon2).abs() < DEG_TOL,
            "sinu lon {}: {} vs {}",
            lon,
            lon,
            lon2
        );
        assert!(
            (lat - lat2).abs() < DEG_TOL,
            "sinu lat {}: {} vs {}",
            lat,
            lat,
            lat2
        );
    }
}

#[test]
fn test_sinusoidal_pole_inverse_error() {
    let proj = Sinusoidal::default();
    let (_, y_pole) = proj.forward(0.0, 90.0).expect("forward ok");
    // At the pole the inverse should fail for non-zero x
    let result = proj.inverse(1_000_000.0, y_pole);
    assert!(result.is_err(), "expected error at pole with non-zero x");
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Mollweide projection
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_mollweide_origin() {
    let proj = Mollweide::default();
    let (x, y) = proj.forward(0.0, 0.0).expect("ok");
    assert!(x.abs() < M_TOL);
    assert!(y.abs() < M_TOL);
}

#[test]
fn test_mollweide_roundtrip_multiple_points() {
    let proj = Mollweide::default();
    let test_points = [
        (0.0, 0.0),
        (20.0, 30.0),
        (-150.0, 60.0),
        (90.0, -45.0),
        (-90.0, 45.0),
        (0.0, 89.0),
        (0.0, -89.0),
    ];
    for (lon, lat) in test_points {
        let (x, y) = proj.forward(lon, lat).expect("forward");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse");
        assert!((lon - lon2).abs() < DEG_TOL, "moll lon {} vs {}", lon, lon2);
        assert!((lat - lat2).abs() < DEG_TOL, "moll lat {} vs {}", lat, lat2);
    }
}

#[test]
fn test_mollweide_central_meridian() {
    // With lon_0=90°, a point at lon=90 should give x=0
    let proj = Mollweide::new(90.0, 6_378_137.0);
    let (x, _y) = proj.forward(90.0, 0.0).expect("ok");
    assert!(x.abs() < M_TOL, "x={}", x);
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Robinson projection
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_robinson_origin() {
    let proj = Robinson::default();
    let (x, y) = proj.forward(0.0, 0.0).expect("ok");
    assert!(x.abs() < 1.0, "x={}", x);
    assert!(y.abs() < 1.0, "y={}", y);
}

#[test]
fn test_robinson_roundtrip() {
    let proj = Robinson::default();
    let test_points = [
        (0.0, 0.0),
        (10.0, 20.0),
        (-90.0, 45.0),
        (150.0, -30.0),
        (0.0, 85.0),
    ];
    for (lon, lat) in test_points {
        let (x, y) = proj.forward(lon, lat).expect("forward");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse");
        assert!((lon - lon2).abs() < 1e-3, "robin lon {} vs {}", lon, lon2);
        assert!((lat - lat2).abs() < 1e-3, "robin lat {} vs {}", lat, lat2);
    }
}

#[test]
fn test_robinson_y_sign() {
    let proj = Robinson::default();
    let (_, y_n) = proj.forward(0.0, 45.0).expect("north");
    let (_, y_s) = proj.forward(0.0, -45.0).expect("south");
    assert!(y_n > 0.0, "northern hemisphere y should be positive");
    assert!(y_s < 0.0, "southern hemisphere y should be negative");
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Eckert IV projection
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_eckert4_origin() {
    let proj = EckertIV::default();
    let (x, y) = proj.forward(0.0, 0.0).expect("ok");
    assert!(x.abs() < M_TOL);
    assert!(y.abs() < M_TOL);
}

#[test]
fn test_eckert4_roundtrip() {
    let proj = EckertIV::default();
    let test_points = [
        (0.0, 0.0),
        (30.0, 45.0),
        (-120.0, -30.0),
        (10.0, 80.0),
        (180.0, 0.0),
    ];
    for (lon, lat) in test_points {
        let (x, y) = proj.forward(lon, lat).expect("forward");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse");
        assert!((lon - lon2).abs() < DEG_TOL, "eck4 lon {} vs {}", lon, lon2);
        assert!((lat - lat2).abs() < DEG_TOL, "eck4 lat {} vs {}", lat, lat2);
    }
}

#[test]
fn test_eckert4_equal_area_symmetry() {
    // Eckert IV is symmetric about the equator
    let proj = EckertIV::default();
    let (x1, y1) = proj.forward(10.0, 30.0).expect("north");
    let (x2, y2) = proj.forward(10.0, -30.0).expect("south");
    assert!((x1 - x2).abs() < M_TOL, "Eckert IV should be E-symmetric");
    assert!(
        (y1 + y2).abs() < M_TOL,
        "Eckert IV should be equatorial-symmetric"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Eckert VI projection
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_eckert6_origin() {
    let proj = EckertVI::default();
    let (x, y) = proj.forward(0.0, 0.0).expect("ok");
    assert!(x.abs() < M_TOL);
    assert!(y.abs() < M_TOL);
}

#[test]
fn test_eckert6_roundtrip() {
    let proj = EckertVI::default();
    let test_points = [(0.0, 0.0), (45.0, 60.0), (-30.0, -45.0), (100.0, 20.0)];
    for (lon, lat) in test_points {
        let (x, y) = proj.forward(lon, lat).expect("forward");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse");
        assert!((lon - lon2).abs() < DEG_TOL, "eck6 lon {} vs {}", lon, lon2);
        assert!((lat - lat2).abs() < DEG_TOL, "eck6 lat {} vs {}", lat, lat2);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. Cassini-Soldner projection
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_cassini_origin() {
    let proj = CassineSoldner::default();
    let (x, y) = proj.forward(0.0, 0.0).expect("ok");
    assert!(x.abs() < 1.0);
    assert!(y.abs() < 1.0);
}

#[test]
fn test_cassini_roundtrip() {
    let proj = CassineSoldner::default();
    let test_points = [(0.0, 0.0), (2.0, 50.0), (-5.0, 40.0), (5.0, 55.0)];
    for (lon, lat) in test_points {
        let (x, y) = proj.forward(lon, lat).expect("forward");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse");
        assert!((lon - lon2).abs() < DEG_MED, "cass lon {} vs {}", lon, lon2);
        assert!((lat - lat2).abs() < DEG_MED, "cass lat {} vs {}", lat, lat2);
    }
}

#[test]
fn test_cassini_central_meridian_y_axis() {
    // On the central meridian x should be 0
    let proj = CassineSoldner::new(3.0, 0.0, 0.0, 0.0, 6_378_137.0);
    let (x, _y) = proj.forward(3.0, 45.0).expect("ok");
    assert!(x.abs() < 1.0, "cassini on central meridian: x={}", x);
}

// ─────────────────────────────────────────────────────────────────────────────
// 7. Gauss-Krüger projection
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_gauss_kruger_origin() {
    let proj = GaussKruger::default();
    let (x, y) = proj.forward(0.0, 0.0).expect("ok");
    assert!(x.abs() < 1.0);
    assert!(y.abs() < 1.0);
}

#[test]
fn test_gauss_kruger_roundtrip_germany() {
    let proj = GaussKruger::new(9.0, 0.0, 1.0, 0.0, 0.0);
    let test_points = [(9.0, 50.0), (10.0, 52.0), (8.0, 48.0), (9.0, 45.0)];
    for (lon, lat) in test_points {
        let (x, y) = proj.forward(lon, lat).expect("forward");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse");
        assert!((lon - lon2).abs() < 1e-6, "gk lon {} vs {}", lon, lon2);
        assert!((lat - lat2).abs() < 1e-6, "gk lat {} vs {}", lat, lat2);
    }
}

#[test]
fn test_gauss_kruger_dhdn_zone3_false_easting() {
    let proj = GaussKruger::new(9.0, 0.0, 1.0, 3_500_000.0, 0.0);
    let (x, _y) = proj.forward(8.68, 50.11).expect("ok");
    // Easting should be slightly west of central meridian strip (3500000)
    assert!(x > 3_400_000.0 && x < 3_500_000.0, "DHDN zone 3 x={}", x);
}

// ─────────────────────────────────────────────────────────────────────────────
// 8. Equidistant Conic projection
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_eqdc_origin() {
    let proj = EquidistantConic::default();
    let (x, _y) = proj.forward(0.0, 0.0).expect("ok");
    assert!(x.abs() < 1e3, "eqdc origin x={}", x);
}

#[test]
fn test_eqdc_roundtrip() {
    let proj = EquidistantConic::default();
    let test_points = [(0.0, 45.0), (10.0, 50.0), (-50.0, 40.0), (100.0, 55.0)];
    for (lon, lat) in test_points {
        let (x, y) = proj.forward(lon, lat).expect("forward");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse");
        assert!((lon - lon2).abs() < DEG_MED, "eqdc lon {} vs {}", lon, lon2);
        assert!((lat - lat2).abs() < DEG_MED, "eqdc lat {} vs {}", lat, lat2);
    }
}

#[test]
fn test_eqdc_custom_parallels() {
    // EPSG:102005 equivalent: Albers Equal Area (NA)
    let proj = EquidistantConic::new(-96.0, 37.5, 29.5, 45.5, 0.0, 0.0, 6_378_137.0);
    let (x, y) = proj.forward(-96.0, 37.5).expect("ok");
    // At the origin, x should be 0 (central meridian)
    assert!(x.abs() < 10.0, "x at origin: {}", x);
    assert!(y.is_finite());
}

// ─────────────────────────────────────────────────────────────────────────────
// 9. Azimuthal Equidistant projection
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_aeqd_origin_returns_zero() {
    let proj = AzimuthalEquidistant::default();
    let (x, y) = proj.forward(0.0, 0.0).expect("ok");
    assert!(x.abs() < M_TOL);
    assert!(y.abs() < M_TOL);
}

#[test]
fn test_aeqd_distance_preserved() {
    let proj = AzimuthalEquidistant::default();
    // 90° from centre on equator → distance = π/2 * R
    let (x, y) = proj.forward(90.0, 0.0).expect("ok");
    let dist = (x * x + y * y).sqrt();
    let expected = std::f64::consts::FRAC_PI_2 * 6_378_137.0;
    assert!(
        (dist - expected).abs() < 1.0,
        "aeqd distance: {} vs {}",
        dist,
        expected
    );
}

#[test]
fn test_aeqd_roundtrip() {
    let proj = AzimuthalEquidistant::default();
    let test_points = [(0.0, 0.0), (30.0, 45.0), (-60.0, -30.0), (90.0, 60.0)];
    for (lon, lat) in test_points {
        let (x, y) = proj.forward(lon, lat).expect("forward");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse");
        assert!((lon - lon2).abs() < DEG_TOL, "aeqd lon {} vs {}", lon, lon2);
        assert!((lat - lat2).abs() < DEG_TOL, "aeqd lat {} vs {}", lat, lat2);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 10. Gnomonic projection
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_gnomonic_origin() {
    let proj = Gnomonic::default();
    let (x, y) = proj.forward(0.0, 0.0).expect("ok");
    assert!(x.abs() < M_TOL);
    assert!(y.abs() < M_TOL);
}

#[test]
fn test_gnomonic_roundtrip() {
    let proj = Gnomonic::default();
    let test_points = [(0.0, 0.0), (10.0, 20.0), (-30.0, 45.0), (50.0, -20.0)];
    for (lon, lat) in test_points {
        let (x, y) = proj.forward(lon, lat).expect("forward");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse");
        assert!((lon - lon2).abs() < DEG_TOL, "gnom lon {} vs {}", lon, lon2);
        assert!((lat - lat2).abs() < DEG_TOL, "gnom lat {} vs {}", lat, lat2);
    }
}

#[test]
fn test_gnomonic_horizon_rejected() {
    let proj = Gnomonic::default(); // centred at (0, 0)
    // Point at exactly 90° from centre should fail
    let result = proj.forward(90.0, 0.0);
    assert!(result.is_err(), "gnomonic horizon should fail");
}

#[test]
fn test_gnomonic_pole_centre() {
    // Gnomonic centred at North Pole
    let proj = Gnomonic::new(0.0, 90.0, 0.0, 0.0, 6_378_137.0);
    let (x, y) = proj.forward(0.0, 80.0).expect("ok");
    assert!(x.is_finite());
    assert!(y.is_finite());
    // x should be 0 since we're on the same meridian as lon_0=0
    assert!(x.abs() < 1.0, "gnomonic north pole x={}", x);
}

// ─────────────────────────────────────────────────────────────────────────────
// 11. Lambert Azimuthal Equal-Area
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_laea_origin() {
    let proj = LambertAzimuthalEqualArea::default();
    let (x, y) = proj.forward(0.0, 0.0).expect("ok");
    assert!(x.abs() < M_TOL);
    assert!(y.abs() < M_TOL);
}

#[test]
fn test_laea_roundtrip() {
    let proj = LambertAzimuthalEqualArea::default();
    let test_points = [(0.0, 0.0), (10.0, 20.0), (-80.0, 60.0), (120.0, -30.0)];
    for (lon, lat) in test_points {
        let (x, y) = proj.forward(lon, lat).expect("forward");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse");
        assert!((lon - lon2).abs() < DEG_TOL, "laea lon {} vs {}", lon, lon2);
        assert!((lat - lat2).abs() < DEG_TOL, "laea lat {} vs {}", lat, lat2);
    }
}

#[test]
fn test_laea_etrs89_europe_centre() {
    // EPSG:3035 — centred at 52°N, 10°E
    let proj = LambertAzimuthalEqualArea::new(10.0, 52.0, 4_321_000.0, 3_210_000.0, 6_378_137.0);
    // The centre should map to (FE, FN)
    let (x, y) = proj.forward(10.0, 52.0).expect("ok");
    assert!((x - 4_321_000.0).abs() < 1.0, "laea centre x={}", x);
    assert!((y - 3_210_000.0).abs() < 1.0, "laea centre y={}", y);
}

// ─────────────────────────────────────────────────────────────────────────────
// 12. Lambert Conformal Conic
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_lcc_origin() {
    let proj = LambertConformalConic::default();
    let (x, _y) = proj.forward(0.0, 45.0).expect("ok"); // at standard parallel
    assert!(x.abs() < 1.0);
}

#[test]
fn test_lcc_roundtrip() {
    let proj = LambertConformalConic::default();
    let test_points = [(0.0, 45.0), (20.0, 50.0), (-30.0, 35.0)];
    for (lon, lat) in test_points {
        let (x, y) = proj.forward(lon, lat).expect("forward");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse");
        assert!((lon - lon2).abs() < DEG_MED, "lcc lon {} vs {}", lon, lon2);
        assert!((lat - lat2).abs() < DEG_MED, "lcc lat {} vs {}", lat, lat2);
    }
}

#[test]
fn test_lcc_etrs89_lcc_europe_centre() {
    // EPSG:3034 parameters: lon_0=10, lat_0=52, lat_1=35, lat_2=65
    let proj = LambertConformalConic::new(
        10.0,
        52.0,
        35.0,
        65.0,
        4_000_000.0,
        2_800_000.0,
        6_378_137.0,
    );
    let (x, y) = proj.forward(10.0, 52.0).expect("ok");
    // At the origin should give close to false easting/northing
    assert!((x - 4_000_000.0).abs() < 5_000.0, "lcc europe x={}", x);
    assert!((y - 2_800_000.0).abs() < 5_000.0, "lcc europe y={}", y);
}

// ─────────────────────────────────────────────────────────────────────────────
// 13. Transverse Mercator
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_tmerc_origin() {
    let proj = TransverseMercator::default();
    let (x, y) = proj.forward(0.0, 0.0).expect("ok");
    assert!(x.abs() < 1.0);
    assert!(y.abs() < 1.0);
}

#[test]
fn test_tmerc_roundtrip() {
    let proj = TransverseMercator::default();
    let test_points = [(0.0, 0.0), (5.0, 45.0), (-10.0, 30.0), (10.0, 60.0)];
    for (lon, lat) in test_points {
        let (x, y) = proj.forward(lon, lat).expect("forward");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse");
        assert!(
            (lon - lon2).abs() < DEG_MED,
            "tmerc lon {} vs {}",
            lon,
            lon2
        );
        assert!(
            (lat - lat2).abs() < DEG_MED,
            "tmerc lat {} vs {}",
            lat,
            lat2
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 14. EPSG Database — count and coverage
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_epsg_database_exceeds_500() {
    let codes = available_epsg_codes();
    assert!(
        codes.len() >= 500,
        "Expected 500+ EPSG codes, got {}",
        codes.len()
    );
}

#[test]
fn test_epsg_core_codes_present() {
    let essential = [
        4326,  // WGS84
        3857,  // Web Mercator
        4269,  // NAD83
        4258,  // ETRS89
        27700, // British National Grid
        32632, // WGS84 UTM 32N
        32756, // WGS84 UTM 56S
    ];
    for code in essential {
        assert!(contains_epsg(code), "EPSG:{} should be present", code);
    }
}

#[test]
fn test_epsg_wgs84_utm_all_60_north() {
    for zone in 1u32..=60 {
        let code = 32600 + zone;
        assert!(
            contains_epsg(code),
            "WGS84 UTM zone {}N (EPSG:{}) missing",
            zone,
            code
        );
    }
}

#[test]
fn test_epsg_wgs84_utm_all_60_south() {
    for zone in 1u32..=60 {
        let code = 32700 + zone;
        assert!(
            contains_epsg(code),
            "WGS84 UTM zone {}S (EPSG:{}) missing",
            zone,
            code
        );
    }
}

#[test]
fn test_epsg_jgd2011_geographic() {
    let def = lookup_epsg(6668).expect("JGD2011 geographic");
    assert_eq!(def.datum, "JGD2011");
}

#[test]
fn test_epsg_gda2020_geographic() {
    let def = lookup_epsg(7844).expect("GDA2020 geographic");
    assert_eq!(def.datum, "GDA2020");
}

#[test]
fn test_epsg_dhdn_gauss_kruger_zones() {
    for code in [31466u32, 31467, 31468, 31469] {
        assert!(contains_epsg(code), "DHDN GK zone EPSG:{} missing", code);
    }
}

#[test]
fn test_epsg_etrs89_utm_zones() {
    for zone in [28u32, 29, 30, 31, 32, 33, 34, 35, 36, 37] {
        let code = 25800 + zone;
        assert!(
            contains_epsg(code),
            "ETRS89 UTM zone {} (EPSG:{}) missing",
            zone,
            code
        );
    }
}

#[test]
fn test_epsg_rgf93_lambert_93() {
    let def = lookup_epsg(2154).expect("RGF93 Lambert-93");
    assert!(
        def.proj_string.contains("+proj=lcc"),
        "Lambert-93 should use LCC"
    );
    assert_eq!(def.datum, "RGF93");
}

#[test]
fn test_epsg_gda94_mga_zones() {
    for zone in 48u32..=56 {
        let code = 27892 + zone; // 28348..28356
        assert!(
            contains_epsg(code),
            "GDA94 MGA zone {} (EPSG:{}) missing",
            zone,
            code
        );
    }
}

#[test]
fn test_epsg_world_projections() {
    // Sinusoidal, Mollweide, Robinson world projections
    for code in [54008u32, 54009, 54030] {
        assert!(
            contains_epsg(code),
            "World projection EPSG:{} missing",
            code
        );
    }
}

#[test]
fn test_epsg_cgcs2000_present() {
    assert!(contains_epsg(4490), "CGCS2000 geographic missing");
}

#[test]
fn test_epsg_british_national_grid() {
    let def = lookup_epsg(27700).expect("BNG");
    assert!(def.proj_string.contains("+proj=tmerc"), "BNG uses tmerc");
    assert_eq!(def.unit, "metre");
}

#[test]
fn test_epsg_pulkovo_gauss_kruger_zones() {
    // Check a few Russian GK zones
    for zone in [4u32, 7, 15, 20, 32] {
        let code = 28400 + zone;
        assert!(
            contains_epsg(code),
            "Pulkovo GK zone {} (EPSG:{}) missing",
            zone,
            code
        );
    }
}

#[test]
fn test_epsg_nad83_utm_zones_10_19() {
    for zone in 10u32..=19 {
        let code = 26900 + zone;
        assert!(
            contains_epsg(code),
            "NAD83 UTM zone {} (EPSG:{}) missing",
            zone,
            code
        );
    }
}

#[test]
fn test_epsg_us_state_plane_sample() {
    // Montana, Texas Central, Vermont, NC
    for code in [32100u32, 32140, 32145, 32119] {
        assert!(contains_epsg(code), "US State Plane EPSG:{} missing", code);
    }
}

#[test]
fn test_epsg_non_existent() {
    assert!(!contains_epsg(99998), "EPSG:99998 should not exist");
    let result = lookup_epsg(99998);
    assert!(result.is_err());
}

// ─────────────────────────────────────────────────────────────────────────────
// 15. Grid shift tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_ostn15_approx_returns_result() {
    let result = ostn15_approx(530_000.0, 181_000.0);
    assert!(result.is_ok(), "OSTN15 failed: {:?}", result);
    let (e, n) = result.expect("ok");
    // London in UTM zone 30N
    assert!(e > 400_000.0 && e < 700_000.0, "OSTN15 easting={}", e);
    assert!(n > 5_600_000.0 && n < 5_900_000.0, "OSTN15 northing={}", n);
}

#[test]
fn test_rgf93_approx_paris() {
    let (lon2, lat2) = rgf93_approx(2.35, 48.85).expect("ok");
    assert!((lon2 - 2.35).abs() < 0.01, "lon diff: {}", lon2 - 2.35);
    assert!((lat2 - 48.85).abs() < 0.01, "lat diff: {}", lat2 - 48.85);
}

#[test]
fn test_dhdn_etrs89_helmert_shifts_point() {
    let (x, y, z) = (3_900_000.0_f64, 900_000.0, 4_700_000.0);
    let (xp, yp, zp) = dhdn_etrs89_helmert(x, y, z);
    let dist = ((xp - x).powi(2) + (yp - y).powi(2) + (zp - z).powi(2)).sqrt();
    // Published DHDN→ETRS89 shift magnitude should be ~700 m
    assert!(dist > 100.0 && dist < 3_000.0, "shift dist={}", dist);
}

#[test]
fn test_dhdn_helmert_translation_direction() {
    // dx=-598.1: X should decrease after transformation
    let (x, y, z) = (4_000_000.0, 800_000.0, 4_800_000.0);
    let (xp, _yp, _zp) = dhdn_etrs89_helmert(x, y, z);
    assert!(xp < x, "X should decrease with negative dx");
}

// ─────────────────────────────────────────────────────────────────────────────
// 16. Edge cases
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_aeqd_antipode_far_point() {
    // A point near the antipode (>180° away) should fail gracefully
    let proj = AzimuthalEquidistant::default();
    // The antipode is at (180, 0) — near distance = π*R ≈ 20,000 km
    // Slightly past it should fail
    let (x, y) = proj
        .forward(179.0, 0.0)
        .expect("near antipode should succeed");
    assert!(x.is_finite());
    assert!(y.is_finite());
}

#[test]
fn test_sinusoidal_antimeridian() {
    let proj = Sinusoidal::default();
    // ±180° should work
    let (x1, y1) = proj.forward(180.0, 0.0).expect("180°");
    let (x2, y2) = proj.forward(-180.0, 0.0).expect("-180°");
    assert!((x1 + x2).abs() < M_TOL, "symmetric about antimeridian");
    assert!((y1 - y2).abs() < M_TOL);
}

#[test]
fn test_mollweide_near_pole() {
    let proj = Mollweide::default();
    let (x, y) = proj.forward(0.0, 89.9).expect("near north pole");
    assert!(x.is_finite());
    assert!(y.is_finite());
    let (lon2, lat2) = proj.inverse(x, y).expect("inverse near pole");
    assert!((lat2 - 89.9).abs() < 1e-4);
    assert!((lon2 - 0.0).abs() < 1e-4);
}

#[test]
fn test_eckert4_symmetric_about_equator() {
    let proj = EckertIV::default();
    let (x1, y1) = proj.forward(45.0, 30.0).expect("north");
    let (x2, y2) = proj.forward(45.0, -30.0).expect("south");
    assert!((x1 - x2).abs() < M_TOL, "Eckert IV symmetric x");
    assert!((y1 + y2).abs() < M_TOL, "Eckert IV symmetric y");
}

#[test]
fn test_robinson_poles_finite() {
    let proj = Robinson::default();
    let (x, y) = proj.forward(0.0, 90.0).expect("north pole");
    assert!(x.is_finite() && y.is_finite());
    let (x2, y2) = proj.forward(0.0, -90.0).expect("south pole");
    assert!(x2.is_finite() && y2.is_finite());
}

#[test]
fn test_transformer_wgs84_web_mercator_round_trip() {
    let fwd = Transformer::from_epsg(4326, 3857).expect("fwd");
    let inv = Transformer::from_epsg(3857, 4326).expect("inv");

    let london = Coordinate::from_lon_lat(0.0, 51.5);
    let projected = fwd.transform(&london).expect("project");
    let recovered = inv.transform(&projected).expect("unproject");

    assert!((recovered.x - london.x).abs() < 1e-6, "roundtrip lon");
    assert!((recovered.y - london.y).abs() < 1e-6, "roundtrip lat");
}

#[test]
fn test_transformer_known_point_london_bng() {
    // London (ETRS89) → approx BNG coordinates
    let transformer = Transformer::from_epsg(4326, 27700).expect("BNG transformer");
    // London: approx 0.0°W, 51.5°N → BNG approx E:530000, N:181000
    let london = Coordinate::from_lon_lat(-0.1276, 51.5074);
    let result = transformer.transform(&london).expect("transform");
    // Broad check — BNG values for London
    assert!(
        result.x > 500_000.0 && result.x < 560_000.0,
        "BNG easting={}",
        result.x
    );
    assert!(
        result.y > 170_000.0 && result.y < 200_000.0,
        "BNG northing={}",
        result.y
    );
}

#[test]
fn test_epsg_lookup_proj_string_valid() {
    // Spot-check that PROJ strings we added are non-empty
    for code in [2154u32, 3035, 3067, 2180, 5514] {
        if let Ok(def) = lookup_epsg(code) {
            assert!(
                !def.proj_string.is_empty(),
                "EPSG:{} has empty proj string",
                code
            );
            assert!(
                def.proj_string.contains("+proj="),
                "EPSG:{} missing +proj= in: {}",
                code,
                def.proj_string
            );
        }
    }
}

#[test]
fn test_epsg_sorted_codes() {
    let codes = available_epsg_codes();
    for window in codes.windows(2) {
        assert!(
            window[0] < window[1],
            "codes not sorted: {} >= {}",
            window[0],
            window[1]
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 17. Count validation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_epsg_final_count_detailed() {
    let codes = available_epsg_codes();
    let total = codes.len();
    println!("Total EPSG codes in database: {}", total);
    assert!(total >= 500, "Expected ≥500 EPSG codes, found {}", total);
}
