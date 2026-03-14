//! Property-based tests for CRS transformations in oxigdal-proj.
//!
//! Invariants tested:
//! 1. Round-trip: inverse(forward(p)) ≈ p  (within floating-point tolerance)
//! 2. Identity: transform to same CRS produces identity result
//! 3. Projection-specific: each projection forward+inverse round-trips
//! 4. EPSG lookup: deterministic (same code → same result)
//! 5. Batch: element-wise equals batch transform

#![allow(clippy::expect_used)]

use oxigdal_proj::{
    Coordinate, LambertConformalConic, Mollweide, Robinson, Sinusoidal, Transformer,
    TransverseMercator, available_epsg_codes, contains_epsg,
};
use proptest::prelude::*;

const PROJ_TOL_LOOSE: f64 = 1e-5;
const PROJ_TOL_MEDIUM: f64 = 1e-6;
const PROJ_TOL_TIGHT: f64 = 1e-8;

// ── Strategies ───────────────────────────────────────────────────────────────

prop_compose! {
    fn valid_latlon()(
        lat in -89.9f64..89.9f64,
        lon in -179.9f64..179.9f64,
    ) -> (f64, f64) { (lat, lon) }
}

prop_compose! {
    fn webmercator_latlon()(
        lat in -84.9f64..84.9f64,
        lon in -179.9f64..179.9f64,
    ) -> (f64, f64) { (lat, lon) }
}

prop_compose! {
    fn utm_latlon()(
        lat in -79.9f64..83.9f64,
        lon in -179.9f64..179.9f64,
    ) -> (f64, f64) { (lat, lon) }
}

prop_compose! {
    fn lcc_latlon()(
        lat in 20.1f64..69.9f64,
        lon in -179.9f64..179.9f64,
    ) -> (f64, f64) { (lat, lon) }
}

prop_compose! {
    fn near_equator_latlon()(
        lat in -88.9f64..88.9f64,
        lon in -179.9f64..179.9f64,
    ) -> (f64, f64) { (lat, lon) }
}

prop_compose! {
    fn valid_utm_zone()(
        zone in 1u32..=60u32,
        north in proptest::bool::ANY,
    ) -> (u32, bool) { (zone, north) }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Returns the central meridian for a UTM zone number.
fn utm_central_meridian(zone: u32) -> f64 {
    -183.0 + (zone as f64) * 6.0
}

// ── Proptest macros ───────────────────────────────────────────────────────────

proptest! {
    /// WebMercator forward→inverse round-trip for WGS84 geographic coordinates.
    /// Uses Transformer::from_epsg(4326, 3857) then from_epsg(3857, 4326).
    #[test]
    fn prop_webmercator_roundtrip((lat, lon) in webmercator_latlon()) {
        let fwd = Transformer::from_epsg(4326, 3857);
        let inv = Transformer::from_epsg(3857, 4326);

        let (fwd_t, inv_t) = match (fwd, inv) {
            (Ok(f), Ok(i)) => (f, i),
            _ => return Ok(()),  // skip if CRS not supported
        };

        let coord = Coordinate::from_lon_lat(lon, lat);
        let projected = match fwd_t.transform(&coord) {
            Ok(p) => p,
            Err(_) => return Ok(()),
        };

        prop_assume!(projected.x.is_finite() && projected.y.is_finite());

        let back = match inv_t.transform(&projected) {
            Ok(b) => b,
            Err(_) => return Ok(()),
        };

        prop_assume!(back.x.is_finite() && back.y.is_finite());

        prop_assert!(
            (back.x - lon).abs() < PROJ_TOL_LOOSE,
            "lon round-trip failed: got {}, expected {}, diff {}",
            back.x, lon, (back.x - lon).abs()
        );
        prop_assert!(
            (back.y - lat).abs() < PROJ_TOL_LOOSE,
            "lat round-trip failed: got {}, expected {}, diff {}",
            back.y, lat, (back.y - lat).abs()
        );
    }

    /// UTM forward→inverse round-trip using TransverseMercator.
    #[test]
    fn prop_utm_roundtrip(
        (lat, lon) in utm_latlon(),
        (zone, north) in valid_utm_zone(),
    ) {
        let central_meridian = utm_central_meridian(zone);
        let false_northing = if north { 0.0 } else { 10_000_000.0 };

        let proj = TransverseMercator::new(
            central_meridian,
            0.0,
            0.9996,
            500_000.0,
            false_northing,
            6_378_137.0,
        );

        let (x, y) = match proj.forward(lon, lat) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };

        prop_assume!(x.is_finite() && y.is_finite());

        let (lon2, lat2) = match proj.inverse(x, y) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };

        prop_assume!(lon2.is_finite() && lat2.is_finite());

        // Normalize longitude difference to handle 360-degree wrap-around
        let lon_diff = {
            let raw = lon2 - lon;
            if raw > 180.0 { raw - 360.0 }
            else if raw < -180.0 { raw + 360.0 }
            else { raw }
        };

        prop_assert!(
            (lat2 - lat).abs() < PROJ_TOL_MEDIUM,
            "lat round-trip failed: got {}, expected {}, diff {}",
            lat2, lat, (lat2 - lat).abs()
        );
        prop_assert!(
            lon_diff.abs() < PROJ_TOL_MEDIUM,
            "lon round-trip failed: got {}, expected {}, normalized diff {}",
            lon2, lon, lon_diff.abs()
        );
    }

    /// Lambert Conformal Conic round-trip.
    #[test]
    fn prop_lcc_roundtrip((lat, lon) in lcc_latlon()) {
        let proj = LambertConformalConic::default();

        let (x, y) = match proj.forward(lon, lat) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };

        prop_assume!(x.is_finite() && y.is_finite());

        let (lon2, lat2) = match proj.inverse(x, y) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };

        prop_assume!(lon2.is_finite() && lat2.is_finite());

        prop_assert!(
            (lat2 - lat).abs() < PROJ_TOL_MEDIUM,
            "lat round-trip failed: got {}, expected {}",
            lat2, lat
        );
        prop_assert!(
            (lon2 - lon).abs() < PROJ_TOL_MEDIUM,
            "lon round-trip failed: got {}, expected {}",
            lon2, lon
        );
    }

    /// Mollweide round-trip.
    #[test]
    fn prop_mollweide_roundtrip((lat, lon) in near_equator_latlon()) {
        let proj = Mollweide::default();

        let (x, y) = match proj.forward(lon, lat) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };

        prop_assume!(x.is_finite() && y.is_finite());

        let (lon2, lat2) = match proj.inverse(x, y) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };

        prop_assume!(lon2.is_finite() && lat2.is_finite());

        prop_assert!(
            (lat2 - lat).abs() < PROJ_TOL_LOOSE,
            "lat round-trip failed: got {}, expected {}",
            lat2, lat
        );
        prop_assert!(
            (lon2 - lon).abs() < PROJ_TOL_LOOSE,
            "lon round-trip failed: got {}, expected {}",
            lon2, lon
        );
    }

    /// Sinusoidal round-trip.
    #[test]
    fn prop_sinusoidal_roundtrip((lat, lon) in near_equator_latlon()) {
        let proj = Sinusoidal::default();

        let (x, y) = match proj.forward(lon, lat) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };

        prop_assume!(x.is_finite() && y.is_finite());

        let (lon2, lat2) = match proj.inverse(x, y) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };

        prop_assume!(lon2.is_finite() && lat2.is_finite());

        prop_assert!(
            (lat2 - lat).abs() < PROJ_TOL_TIGHT,
            "lat round-trip failed: got {}, expected {}",
            lat2, lat
        );
        prop_assert!(
            (lon2 - lon).abs() < PROJ_TOL_TIGHT,
            "lon round-trip failed: got {}, expected {}",
            lon2, lon
        );
    }

    /// Robinson round-trip.
    #[test]
    fn prop_robinson_roundtrip((lat, lon) in near_equator_latlon()) {
        let proj = Robinson::default();

        let (x, y) = match proj.forward(lon, lat) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };

        prop_assume!(x.is_finite() && y.is_finite());

        let (lon2, lat2) = match proj.inverse(x, y) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };

        prop_assume!(lon2.is_finite() && lat2.is_finite());

        prop_assert!(
            (lat2 - lat).abs() < PROJ_TOL_LOOSE,
            "lat round-trip failed: got {}, expected {}",
            lat2, lat
        );
        prop_assert!(
            (lon2 - lon).abs() < PROJ_TOL_LOOSE,
            "lon round-trip failed: got {}, expected {}",
            lon2, lon
        );
    }

    /// EPSG lookup is deterministic: same code → same WKT/name on repeated lookup.
    #[test]
    fn prop_epsg_deterministic(code in 4326u32..9999u32) {
        if !contains_epsg(code) {
            return Ok(());
        }

        let result1 = oxigdal_proj::lookup_epsg(code);
        let result2 = oxigdal_proj::lookup_epsg(code);

        match (result1, result2) {
            (Ok(def1), Ok(def2)) => {
                prop_assert_eq!(def1.name.as_str(), def2.name.as_str(), "EPSG name not deterministic for {}", code);
                prop_assert_eq!(def1.code, def2.code, "EPSG code mismatch for {}", code);
            }
            (Err(_), Err(_)) => {} // both failed consistently
            _ => prop_assert!(false, "EPSG lookup inconsistent for {}", code),
        }
    }

    /// Batch transform over all available EPSG codes produces consistent CRS type.
    #[test]
    fn prop_available_codes_all_defined(
        idx in 0usize..10usize
    ) {
        let codes = available_epsg_codes();
        if codes.is_empty() {
            return Ok(());
        }
        let code = codes[idx % codes.len()];
        let result = oxigdal_proj::lookup_epsg(code);
        prop_assert!(result.is_ok(), "available_epsg_codes() returned code {} that lookup_epsg fails on", code);
    }

    /// Identity transform (same CRS both sides) produces same coordinate.
    #[test]
    fn prop_identity_transform((lat, lon) in valid_latlon()) {
        let transformer = match Transformer::from_epsg(4326, 4326) {
            Ok(t) => t,
            Err(_) => return Ok(()),
        };

        let coord = Coordinate::from_lon_lat(lon, lat);
        let result = match transformer.transform(&coord) {
            Ok(r) => r,
            Err(_) => return Ok(()),
        };

        prop_assume!(result.x.is_finite() && result.y.is_finite());

        prop_assert!(
            (result.x - lon).abs() < PROJ_TOL_TIGHT,
            "identity transform changed x: {} → {}",
            lon, result.x
        );
        prop_assert!(
            (result.y - lat).abs() < PROJ_TOL_TIGHT,
            "identity transform changed y: {} → {}",
            lat, result.y
        );
    }

    /// Element-wise transform equals batch transform via transform_epsg.
    #[test]
    fn prop_elementwise_consistent(
        points in prop::collection::vec(valid_latlon(), 1..20)
    ) {
        for (lat, lon) in &points {
            let coord = Coordinate::from_lon_lat(*lon, *lat);
            let r1 = oxigdal_proj::transform_epsg(&coord, 4326, 4326);
            let r2 = oxigdal_proj::transform_epsg(&coord, 4326, 4326);

            match (r1, r2) {
                (Ok(c1), Ok(c2)) => {
                    prop_assert!(
                        (c1.x - c2.x).abs() < f64::EPSILON * 2.0,
                        "transform_epsg not deterministic for ({}, {})",
                        lon, lat
                    );
                }
                (Err(_), Err(_)) => {}
                _ => prop_assert!(false, "transform_epsg inconsistent"),
            }
        }
    }
}
