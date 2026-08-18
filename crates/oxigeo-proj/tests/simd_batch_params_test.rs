//! Regression tests for **projection-parameter fidelity** of the SIMD batch
//! fast path (`Transformer::transform_batch`).
//!
//! # Why this file exists
//!
//! `simd_batch_test.rs` checks that batching itself is sound: it compares
//! `transform_batch(&coords)` against `transform_batch(&[c])`, i.e. the kernel
//! against *itself*.  That is a real property, but it is blind to a whole class
//! of defect — a kernel that ignores one of the projection's parameters is
//! perfectly self-consistent while being catastrophically wrong.
//!
//! Exactly that shipped: the Transverse Mercator kernel hardcoded
//! `m0 = meridional_arc(0.0)`, silently assuming `+lat_0=0`.  UTM has
//! `lat_0 = 0`, so every UTM test passed.  The Japan Plane Rectangular CS does
//! not (`lat_0 = 33°/36°/…`), so every JPR northing came out **3 985 144 m**
//! too far north — enough to place a raster outside its own grid entirely.
//!
//! The tests below therefore compare the batch fast path against the two things
//! that can actually catch a dropped parameter:
//!
//! 1. `Transformer::transform` — the scalar OxiProj pipeline, a genuinely
//!    independent implementation (Poder/Engsager exact transverse Mercator);
//! 2. absolute coordinates verified against **PROJ 9.5.1**.
//!
//! # Ground-truth provenance
//!
//! The `xy ↔ lon/lat` pairs embedded below were produced by PROJ 9.5.1 (via
//! GDAL's `osr`/`proj` bindings) during a separate verification pass over the
//! EPSG codes used by the downstream OxiGeo playground, five points per CRS
//! (centroid plus the four corners of the CRS's area of use).  They are
//! transcribed here so the tests are self-contained.
//!
//! # Datum handling
//!
//! Every CRS below is built with [`Crs::from_proj`] from an explicit PROJ
//! string rather than `from_epsg`, so these tests exercise the transform
//! kernels alone and do not depend on the embedded EPSG registry.

#![allow(clippy::expect_used)]

use oxigeo_proj::crs::Crs;
use oxigeo_proj::transform::{Coordinate, Transformer};

/// Batch-vs-scalar and batch-vs-PROJ tolerance, in metres.
///
/// Measured worst case across the full designed extent of the JPR zones
/// (±1.1° of the central meridian, 30°–37.5° N) is 6.0e-8 m, so this is a
/// ~17× margin.  See `test_tmerc_batch_accuracy_degrades_off_meridian` for the
/// characterisation of what sets this number.
const METRE_TOL: f64 = 1e-6;

/// Tolerance for inverse (projected → geographic) results, in degrees.
///
/// 1e-9° is about 1.1e-4 m on the ground.  The inverse direction has no SIMD
/// kernel at all — `try_simd_batch` only accepts geographic → projected — so
/// `test_jpr_inverse_batch_matches_scalar_and_ground_truth` additionally
/// asserts bit-identity with the scalar path; this bound only governs the
/// comparison against PROJ's published lon/lat.
const DEGREE_TOL: f64 = 1e-9;

/// WGS-84 geographic, the source CRS for every forward case.
const WGS84: &str = "+proj=longlat +datum=WGS84 +no_defs";

/// A Japan Plane Rectangular CS zone: EPSG code, PROJ string, and five
/// PROJ 9.5.1-verified `(lon, lat) → (easting, northing)` pairs.
struct JprZone {
    epsg: u32,
    name: &'static str,
    proj: &'static str,
    /// `((lon_deg, lat_deg), (easting_m, northing_m))`, verified with PROJ 9.5.1.
    points: [((f64, f64), (f64, f64)); 5],
}

/// Three JPR zones, all with **non-zero `+lat_0`** — the parameter the batch
/// kernel used to drop.  Zone I is on `lat_0=33`, zones VI and IX on `lat_0=36`,
/// and the three sit on widely separated central meridians (129.5°, 136°,
/// 139.83°) so a mis-plumbed `lon_0` cannot hide either.
const JPR_ZONES: [JprZone; 3] = [
    JprZone {
        epsg: 6669,
        name: "JGD2011 / Japan Plane Rectangular CS I",
        proj: "+proj=tmerc +lat_0=33 +lon_0=129.5 +k=0.9999 +x_0=0 +y_0=0 \
               +ellps=GRS80 +units=m +no_defs",
        points: [
            ((129.315, 30.85), (-17694.12918193351, -238365.4573224007)),
            ((128.399, 27.738), (-108550.17603418912, -582796.6454517887)),
            ((130.231, 27.738), (72069.59109307396, -583068.1193481671)),
            ((130.231, 33.962), (67557.27385632659, 106928.45206855252)),
            ((128.399, 33.962), (-101753.117103151, 107233.88786149953)),
        ],
    },
    JprZone {
        epsg: 6674,
        name: "JGD2011 / Japan Plane Rectangular CS VI",
        proj: "+proj=tmerc +lat_0=36 +lon_0=136 +k=0.9999 +x_0=0 +y_0=0 \
               +ellps=GRS80 +units=m +no_defs",
        points: [
            (
                (135.925, 34.864999999999995),
                (-6857.153430803463, -125911.42130509262),
            ),
            ((135.073, 33.693), (-85940.41197993823, -255522.26689838083)),
            (
                (136.77700000000002, 33.693),
                (72033.83402766303, -255636.99282300583),
            ),
            (
                (136.77700000000002, 36.037),
                (70018.1099196868, 4384.4030259409055),
            ),
            ((135.073, 36.037), (-83535.45627841566, 4502.665091046598)),
        ],
    },
    JprZone {
        epsg: 6677,
        name: "JGD2011 / Japan Plane Rectangular CS IX",
        proj: "+proj=tmerc +lat_0=36 +lon_0=139.833333333333 +k=0.9999 +x_0=0 +y_0=0 \
               +ellps=GRS80 +units=m +no_defs",
        points: [
            (
                (139.755, 33.644999999999996),
                (-7266.047747636759, -261228.66136330654),
            ),
            ((138.671, 30.177), (-111942.29663465684, -645172.7300304073)),
            ((140.839, 30.177), (96853.17078966452, -645316.2489232976)),
            ((140.839, 37.113), (89375.7509938237, 123969.85501445037)),
            ((138.671, 37.113), (-103299.54222004942, 124128.82461783037)),
        ],
    },
];

/// Builds a `WGS84 → proj` transformer.
fn forward_to(proj: &str) -> Transformer {
    let src = Crs::from_proj(WGS84).expect("WGS84 source CRS");
    let dst = Crs::from_proj(proj).expect("target CRS from PROJ string");
    Transformer::new(src, dst)
        .expect("transformer")
        .with_strict(false)
}

/// Builds a `proj → WGS84` transformer.
fn inverse_from(proj: &str) -> Transformer {
    let src = Crs::from_proj(proj).expect("source CRS from PROJ string");
    let dst = Crs::from_proj(WGS84).expect("WGS84 target CRS");
    Transformer::new(src, dst)
        .expect("transformer")
        .with_strict(false)
}

// =============================================================================
// 1 — Forward: batch must agree with the scalar pipeline for non-zero `+lat_0`
//
// THE REGRESSION TEST.  Before the fix the batch northing was offset by
// `meridional_arc(lat_0)` — 3 985 144 m for `lat_0=36`, 3 653 481 m for
// `lat_0=33` — so this assertion failed by ~4e6 m on every point.
// =============================================================================

#[test]
fn test_jpr_forward_batch_matches_scalar() {
    for zone in &JPR_ZONES {
        let t = forward_to(zone.proj);
        let coords: Vec<Coordinate> = zone
            .points
            .iter()
            .map(|((lon, lat), _)| Coordinate::from_lon_lat(*lon, *lat))
            .collect();

        let batch = t.transform_batch(&coords).expect("batch forward");
        assert_eq!(batch.len(), 5, "EPSG:{} result length", zone.epsg);

        for (i, c) in coords.iter().enumerate() {
            let scalar = t.transform(c).expect("scalar forward");
            let dx = (batch[i].x - scalar.x).abs();
            let dy = (batch[i].y - scalar.y).abs();
            assert!(
                dx < METRE_TOL && dy < METRE_TOL,
                "EPSG:{} ({}) point {i} at ({}, {}): batch=({:.6}, {:.6}) \
                 scalar=({:.6}, {:.6}) delta=({dx:.3e}, {dy:.3e}) m",
                zone.epsg,
                zone.name,
                c.x,
                c.y,
                batch[i].x,
                batch[i].y,
                scalar.x,
                scalar.y,
            );
        }
    }
}

// =============================================================================
// 2 — Forward: batch must match absolute PROJ 9.5.1 coordinates
//
// Batch-vs-scalar alone can be satisfied by both paths being wrong the same
// way.  This pins the batch path to externally verified numbers.
// =============================================================================

#[test]
fn test_jpr_forward_batch_matches_proj_ground_truth() {
    for zone in &JPR_ZONES {
        let t = forward_to(zone.proj);
        let coords: Vec<Coordinate> = zone
            .points
            .iter()
            .map(|((lon, lat), _)| Coordinate::from_lon_lat(*lon, *lat))
            .collect();

        let batch = t.transform_batch(&coords).expect("batch forward");

        for (i, ((lon, lat), (want_x, want_y))) in zone.points.iter().enumerate() {
            let dx = (batch[i].x - want_x).abs();
            let dy = (batch[i].y - want_y).abs();
            assert!(
                dx < METRE_TOL && dy < METRE_TOL,
                "EPSG:{} ({}) point {i} at ({lon}, {lat}): batch=({:.6}, {:.6}) \
                 PROJ 9.5.1=({want_x:.6}, {want_y:.6}) delta=({dx:.3e}, {dy:.3e}) m",
                zone.epsg,
                zone.name,
                batch[i].x,
                batch[i].y,
            );
        }
    }
}

// =============================================================================
// 3 — Inverse: `transform_batch` in the projected → geographic direction
//
// There is no SIMD kernel for the inverse: `try_simd_batch` requires
// `source.is_geographic() && target.is_projected()`, so this direction falls
// back to the per-point scalar loop.  The test pins that contract — batch and
// scalar must be *bit-identical* here, and both must reproduce the PROJ
// ground-truth longitudes/latitudes.
// =============================================================================

#[test]
fn test_jpr_inverse_batch_matches_scalar_and_ground_truth() {
    for zone in &JPR_ZONES {
        let t = inverse_from(zone.proj);
        let coords: Vec<Coordinate> = zone
            .points
            .iter()
            .map(|(_, (x, y))| Coordinate::new(*x, *y))
            .collect();

        let batch = t.transform_batch(&coords).expect("batch inverse");
        assert_eq!(batch.len(), 5, "EPSG:{} inverse length", zone.epsg);

        for (i, ((want_lon, want_lat), _)) in zone.points.iter().enumerate() {
            let scalar = t.transform(&coords[i]).expect("scalar inverse");
            assert_eq!(
                (batch[i].x, batch[i].y),
                (scalar.x, scalar.y),
                "EPSG:{} inverse point {i}: batch and scalar must be identical \
                 (no SIMD kernel exists for this direction)",
                zone.epsg,
            );

            let dlon = (batch[i].x - want_lon).abs();
            let dlat = (batch[i].y - want_lat).abs();
            assert!(
                dlon < DEGREE_TOL && dlat < DEGREE_TOL,
                "EPSG:{} ({}) inverse point {i}: got=({:.10}, {:.10}) \
                 PROJ 9.5.1=({want_lon:.10}, {want_lat:.10}) delta=({dlon:.3e}, {dlat:.3e})°",
                zone.epsg,
                zone.name,
                batch[i].x,
                batch[i].y,
            );
        }
    }
}

// =============================================================================
// 4 — Round trip through both batch directions
// =============================================================================

#[test]
fn test_jpr_batch_round_trip() {
    for zone in &JPR_ZONES {
        let fwd = forward_to(zone.proj);
        let inv = inverse_from(zone.proj);

        let coords: Vec<Coordinate> = zone
            .points
            .iter()
            .map(|((lon, lat), _)| Coordinate::from_lon_lat(*lon, *lat))
            .collect();

        let projected = fwd.transform_batch(&coords).expect("forward");
        let back = inv.transform_batch(&projected).expect("inverse");

        for (i, c) in coords.iter().enumerate() {
            assert!(
                (back[i].x - c.x).abs() < DEGREE_TOL && (back[i].y - c.y).abs() < DEGREE_TOL,
                "EPSG:{} round-trip point {i}: ({}, {}) -> ({:.10}, {:.10})",
                zone.epsg,
                c.x,
                c.y,
                back[i].x,
                back[i].y,
            );
        }
    }
}

// =============================================================================
// 5 — Batch size independence: 1, 4, 5 and 8 points must give the same answer
//
// Exercises the 4-lane unrolled body and the 0–3 point remainder tail with a
// non-zero `+lat_0`, which the pre-fix kernel never saw.
// =============================================================================

#[test]
fn test_jpr_batch_lane_and_tail_consistency() {
    let zone = &JPR_ZONES[2]; // CS IX, lat_0 = 36
    let t = forward_to(zone.proj);

    // 9 points spanning the zone: 9 = 2 full lanes + 1 tail point.
    let coords: Vec<Coordinate> = (0..9)
        .map(|i| Coordinate::from_lon_lat(139.0 + i as f64 * 0.2, 34.0 + i as f64 * 0.3))
        .collect();

    let all = t.transform_batch(&coords).expect("batch of 9");

    for (i, c) in coords.iter().enumerate() {
        let single = t.transform_batch(&[*c]).expect("batch of 1");
        assert_eq!(
            (all[i].x, all[i].y),
            (single[0].x, single[0].y),
            "point {i} differs between a 9-point batch and a 1-point batch",
        );
        let scalar = t.transform(c).expect("scalar");
        assert!(
            (all[i].x - scalar.x).abs() < METRE_TOL && (all[i].y - scalar.y).abs() < METRE_TOL,
            "point {i}: batch=({:.6}, {:.6}) scalar=({:.6}, {:.6})",
            all[i].x,
            all[i].y,
            scalar.x,
            scalar.y,
        );
    }
}

// =============================================================================
// 6 — Mercator: `+x_0` / `+y_0` / `+lat_ts` must reach the kernel
//
// Same family of defect as the `+lat_0` bug: `merc_point_fwd` had no false
// easting/northing parameters at all, and `+lat_ts` was never parsed.  For
// SIRGAS 2000 / Brazil Mercator that dropped 5 000 000 m of easting,
// 10 000 000 m of northing, and a 0.061 % scale factor.
// =============================================================================

#[test]
fn test_merc_false_origin_and_lat_ts() {
    // EPSG:5641 — SIRGAS 2000 / Brazil Mercator.
    let proj = "+proj=merc +lon_0=-43 +lat_ts=-2 +x_0=5000000 +y_0=10000000 \
                +ellps=GRS80 +units=m +no_defs";
    let t = forward_to(proj);

    let coords: Vec<Coordinate> = [
        (-43.0, -2.0),
        (-40.0, -10.0),
        (-50.0, -20.0),
        (-35.0, -5.0),
        (-45.0, 0.0),
        (-60.0, -15.0),
    ]
    .iter()
    .map(|(lon, lat)| Coordinate::from_lon_lat(*lon, *lat))
    .collect();

    let batch = t.transform_batch(&coords).expect("merc batch");

    for (i, c) in coords.iter().enumerate() {
        let scalar = t.transform(c).expect("merc scalar");
        assert!(
            (batch[i].x - scalar.x).abs() < METRE_TOL && (batch[i].y - scalar.y).abs() < METRE_TOL,
            "merc point {i} at ({}, {}): batch=({:.6}, {:.6}) scalar=({:.6}, {:.6})",
            c.x,
            c.y,
            batch[i].x,
            batch[i].y,
            scalar.x,
            scalar.y,
        );
    }

    // The false origin must actually be present: at the projection origin the
    // result is exactly (x_0, y_0) only if both offsets were applied.
    let origin = t
        .transform_batch(&[Coordinate::from_lon_lat(-43.0, 0.0)])
        .expect("origin");
    assert!(
        (origin[0].x - 5000000.0).abs() < METRE_TOL,
        "false easting dropped: x = {}",
        origin[0].x
    );
    assert!(
        (origin[0].y - 10000000.0).abs() < METRE_TOL,
        "false northing dropped: y = {}",
        origin[0].y
    );
}

// =============================================================================
// 7 — Lambert Conformal Conic must use the ellipsoidal formulae
//
// The kernel was sphere-based while every real LCC CRS is ellipsoidal.  For
// EPSG:2154 (RGF93 / Lambert-93) that was a 214 m error only 1.5° from the
// projection origin, growing with distance.
// =============================================================================

#[test]
fn test_lcc_ellipsoidal_matches_scalar() {
    // EPSG:2154 — RGF93 v1 / Lambert-93.
    let proj = "+proj=lcc +lat_0=46.5 +lon_0=3 +lat_1=49 +lat_2=44 \
                +x_0=700000 +y_0=6600000 +ellps=GRS80 +units=m +no_defs";
    let t = forward_to(proj);

    // The five verified corners/centroid of the EPSG:2154 area of use.
    let coords: Vec<Coordinate> = [
        (0.25999999999999973, 46.35500000000003),
        (-7.835999999999993, 42.19100000000002),
        (8.356000000000002, 42.19100000000002),
        (8.356, 50.51900000000003),
        (-7.835999999999997, 50.51900000000003),
    ]
    .iter()
    .map(|(lon, lat)| Coordinate::from_lon_lat(*lon, *lat))
    .collect();

    let batch = t.transform_batch(&coords).expect("lcc batch");

    for (i, c) in coords.iter().enumerate() {
        let scalar = t.transform(c).expect("lcc scalar");
        let dx = (batch[i].x - scalar.x).abs();
        let dy = (batch[i].y - scalar.y).abs();
        assert!(
            dx < METRE_TOL && dy < METRE_TOL,
            "lcc point {i} at ({}, {}): batch=({:.6}, {:.6}) scalar=({:.6}, {:.6}) \
             delta=({dx:.3e}, {dy:.3e}) m — a sphere-based kernel fails here by ~1e2 m",
            c.x,
            c.y,
            batch[i].x,
            batch[i].y,
            scalar.x,
            scalar.y,
        );
    }

    // At the projection origin the answer is exactly the false origin.
    let origin = t
        .transform_batch(&[Coordinate::from_lon_lat(3.0, 46.5)])
        .expect("origin");
    assert!(
        (origin[0].x - 700000.0).abs() < METRE_TOL && (origin[0].y - 6600000.0).abs() < METRE_TOL,
        "lcc false origin: got ({}, {})",
        origin[0].x,
        origin[0].y
    );
}

// =============================================================================
// 8 — Non-metric CRS: `+units=us-ft` must be honoured
//
// The kernels compute metres.  A State Plane CRS in US survey feet would come
// back 3.28× too large if the linear unit were ignored.
// =============================================================================

#[test]
fn test_us_survey_foot_units() {
    // Maryland-style SPCS tmerc in US survey feet, and California zone 1 lcc.
    for (label, proj, pts) in [
        (
            "tmerc us-ft",
            "+proj=tmerc +lat_0=38.83333333333334 +lon_0=-77 +k=0.9999 \
             +x_0=399999.9998983998 +y_0=0 +datum=NAD83 +units=us-ft +no_defs",
            [(-77.0, 39.0), (-77.5, 38.5), (-76.5, 39.5)],
        ),
        (
            "lcc us-ft",
            "+proj=lcc +lat_0=39.3333333333333 +lon_0=-122 +lat_1=41.6666666666667 \
             +lat_2=40 +x_0=2000000.0001016 +y_0=500000.0001016 +datum=NAD83 \
             +units=us-ft +no_defs",
            [(-122.0, 40.5), (-121.0, 41.0), (-123.0, 40.0)],
        ),
    ] {
        let t = forward_to(proj);
        let coords: Vec<Coordinate> = pts
            .iter()
            .map(|(lon, lat)| Coordinate::from_lon_lat(*lon, *lat))
            .collect();
        let batch = t.transform_batch(&coords).expect("us-ft batch");

        for (i, c) in coords.iter().enumerate() {
            let scalar = t.transform(c).expect("us-ft scalar");
            assert!(
                (batch[i].x - scalar.x).abs() < METRE_TOL
                    && (batch[i].y - scalar.y).abs() < METRE_TOL,
                "{label} point {i}: batch=({:.6}, {:.6}) scalar=({:.6}, {:.6}) \
                 — a 3.28× unit error would show here",
                batch[i].x,
                batch[i].y,
                scalar.x,
                scalar.y,
            );
        }
    }
}

// =============================================================================
// 9 — Datum shifts must make the fast path decline
//
// The kernels apply no Helmert transformation.  When the source and target
// differ by a real datum shift, `try_simd_batch` must return `None` so the
// scalar pipeline handles it; otherwise the batch silently omits a shift of
// hundreds of metres.
// =============================================================================

#[test]
fn test_datum_shift_falls_back_to_scalar() {
    for (label, proj) in [
        (
            // Tokyo-datum JPR IX: Bessel ellipsoid + a ~600 m Helmert shift.
            "Tokyo datum JPR IX",
            "+proj=tmerc +lat_0=36 +lon_0=139.833333333333 +k=0.9999 +x_0=0 +y_0=0 \
             +ellps=bessel +towgs84=-146.414,507.337,680.507,0,0,0,0 +units=m +no_defs",
        ),
        (
            // OSGB36 / British National Grid: Airy ellipsoid + 7-parameter shift.
            "OSGB36 British National Grid",
            "+proj=tmerc +lat_0=49 +lon_0=-2 +k=0.9996012717 +x_0=400000 +y_0=-100000 \
             +ellps=airy +towgs84=446.448,-125.157,542.06,0.15,0.247,0.842,-20.489 \
             +units=m +no_defs",
        ),
    ] {
        let t = forward_to(proj);
        let coords: Vec<Coordinate> = [(139.755, 33.645), (-2.0, 52.0), (0.0, 51.0)]
            .iter()
            .map(|(lon, lat)| Coordinate::from_lon_lat(*lon, *lat))
            .collect();

        let batch = t.transform_batch(&coords).expect("batch");

        for (i, c) in coords.iter().enumerate() {
            let scalar = t.transform(c).expect("scalar");
            // Declining means the batch *is* the scalar loop: bit-identical.
            assert_eq!(
                (batch[i].x, batch[i].y),
                (scalar.x, scalar.y),
                "{label} point {i}: the SIMD fast path must decline when a datum \
                 shift applies, so batch and scalar must be bit-identical",
            );
        }
    }
}

// =============================================================================
// 9b — LCC `+k_0` (LCC_1SP) must reach the kernel
//
// tmerc and merc both read `+k` / `+k_0`; the LCC path did not, so any
// one-standard-parallel LCC came out scaled wrong (≈3.9e3 m at 45° N for
// k_0 = 0.99).  Same family of defect as the `+lat_0` bug.
// =============================================================================

#[test]
fn test_lcc_scale_factor_k0() {
    let proj = "+proj=lcc +lat_1=45 +lat_0=45 +lon_0=0 +k_0=0.99 +x_0=0 +y_0=0 \
                +ellps=GRS80 +units=m +no_defs";
    let t = forward_to(proj);

    let coords: Vec<Coordinate> = [
        (5.0, 45.0),
        (0.0, 45.0),
        (-3.0, 43.0),
        (4.0, 47.0),
        (2.0, 44.0),
    ]
    .iter()
    .map(|(lon, lat)| Coordinate::from_lon_lat(*lon, *lat))
    .collect();

    let batch = t.transform_batch(&coords).expect("lcc k_0 batch");

    for (i, c) in coords.iter().enumerate() {
        let scalar = t.transform(c).expect("lcc k_0 scalar");
        let dx = (batch[i].x - scalar.x).abs();
        let dy = (batch[i].y - scalar.y).abs();
        assert!(
            dx < METRE_TOL && dy < METRE_TOL,
            "lcc k_0 point {i} at ({}, {}): batch=({:.6}, {:.6}) scalar=({:.6}, {:.6}) \
             delta=({dx:.3e}, {dy:.3e}) m — dropping +k_0 costs ~4e3 m here",
            c.x,
            c.y,
            batch[i].x,
            batch[i].y,
            scalar.x,
            scalar.y,
        );
    }
}

// =============================================================================
// 10 — Characterisation: where the batch/scalar agreement budget goes
//
// This is documentation-as-a-test.  The batch kernel is the Snyder truncated
// series; the scalar path is OxiProj's Poder/Engsager exact transverse
// Mercator.  They agree to ~1e-8 m near the central meridian and diverge as
// Δλ⁷.  JPR zones are ±1.3° wide, so they sit in the flat part; a UTM zone
// edge (±3°) is ~3e-5 m.  If this test starts failing, the kernel's accuracy
// profile changed and `METRE_TOL` needs revisiting.
// =============================================================================

#[test]
fn test_tmerc_batch_accuracy_degrades_off_meridian() {
    let t = forward_to(JPR_ZONES[2].proj); // CS IX, lon_0 = 139.8333…
    let lon0 = 139.833333333333;

    // Within the zone's designed extent the agreement is at the 1e-7 m level.
    let mut worst_in_zone: f64 = 0.0;
    for lat in [30.0f64, 32.0, 34.0, 36.0, 37.5] {
        for dlon in [-1.1f64, -0.5, 0.0, 0.5, 1.1] {
            let c = Coordinate::from_lon_lat(lon0 + dlon, lat);
            let b = t.transform_batch(&[c]).expect("batch")[0];
            let s = t.transform(&c).expect("scalar");
            worst_in_zone = worst_in_zone.max((b.x - s.x).abs()).max((b.y - s.y).abs());
        }
    }
    assert!(
        worst_in_zone < METRE_TOL,
        "worst batch-vs-scalar disagreement inside the JPR IX zone extent is \
         {worst_in_zone:.3e} m, above the {METRE_TOL:.0e} m budget"
    );

    // Far outside any sane zone the truncated series is visibly worse.  This is
    // a *known, documented* property of the Snyder kernel and it is pinned from
    // both sides so the accuracy profile cannot drift unnoticed in either
    // direction.  Measured at Δλ = 10°: ~1.4e-1 m.
    //
    // If this assertion fails high, the kernel got worse — investigate.
    // If it fails low, someone improved the kernel (e.g. ported the
    // Poder/Engsager Clenshaw summation).  That is welcome: update this bound
    // and the accuracy note on `METRE_TOL` to match the new profile.
    let far = Coordinate::from_lon_lat(lon0 + 10.0, 35.0);
    let b_far = t.transform_batch(&[far]).expect("batch")[0];
    let s_far = t.transform(&far).expect("scalar");
    let d_far = (b_far.x - s_far.x).abs().max((b_far.y - s_far.y).abs());
    assert!(
        (1e-3..1.0).contains(&d_far),
        "10° off the central meridian the Snyder series is expected to disagree \
         with the exact algorithm by ~1e-1 m; measured {d_far:.3e} m"
    );
}
