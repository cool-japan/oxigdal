//! Integration tests for the extended PROJ pipeline grammar (Slice 16 W1 + Slice 17 W1).
//!
//! Tests cover the step kinds:
//!   - `+proj=helmert`          — 7-parameter Helmert similarity transform
//!   - `+proj=cart`             — geographic ↔ ECEF (Cartesian) conversion
//!   - `+proj=vgridshift`       — vertical grid shift (stubbed)
//!   - `+proj=hgridshift`       — horizontal NTv2 grid shift (stubbed)
//!   - `+proj=helmert_temporal` — time-dependent Helmert (Slice 17 W1)

#![allow(clippy::expect_used)]

use oxigeo_proj::{
    Coordinate, EllipsoidParams, HelmertConvention, HelmertParams, HelmertRateParams, Pipeline,
    StepKind, parse_pipeline,
};

// ---------------------------------------------------------------------------
// Helmert — parse tests
// ---------------------------------------------------------------------------

#[test]
fn test_parse_helmert_step_position_vector() {
    let s = "+proj=pipeline \
             +step +proj=helmert \
             +x=100.0 +y=200.0 +z=300.0 \
             +rx=1.0 +ry=2.0 +rz=3.0 +s=5.0";

    let parsed = parse_pipeline(s).expect("parse_pipeline ok");
    assert_eq!(parsed.len(), 1);
    let (params, inv) = &parsed[0];
    assert!(!inv, "step should not be inverse");
    assert_eq!(
        params.get("proj").and_then(|v| v.as_deref()),
        Some("helmert")
    );
    assert_eq!(params.get("x").and_then(|v| v.as_deref()), Some("100.0"));
    assert_eq!(params.get("y").and_then(|v| v.as_deref()), Some("200.0"));
    assert_eq!(params.get("z").and_then(|v| v.as_deref()), Some("300.0"));
    assert_eq!(params.get("rx").and_then(|v| v.as_deref()), Some("1.0"));
    assert_eq!(params.get("ry").and_then(|v| v.as_deref()), Some("2.0"));
    assert_eq!(params.get("rz").and_then(|v| v.as_deref()), Some("3.0"));
    assert_eq!(params.get("s").and_then(|v| v.as_deref()), Some("5.0"));

    // Also verify the pipeline builds without error.
    let pipeline = Pipeline::from_proj_string(s).expect("should build helmert pipeline");
    assert_eq!(pipeline.len(), 1);
}

#[test]
fn test_parse_helmert_step_coordinate_frame() {
    let s = "+proj=pipeline \
             +step +proj=helmert \
             +x=10.0 +y=20.0 +z=30.0 \
             +rx=0.5 +ry=0.5 +rz=0.5 +s=1.0 \
             +convention=coordinate_frame";

    let pipeline = Pipeline::from_proj_string(s).expect("should parse helmert pipeline");
    assert_eq!(pipeline.len(), 1);

    // Apply with a zero input to verify the translation is applied correctly
    // regardless of convention (convention only affects rotation signs).
    let coord = Coordinate::new(0.0, 0.0);
    let result = pipeline
        .transform(&coord)
        .expect("transform should succeed");

    // tx=10, ty=20; all input coords = 0, so output ≈ (10, 20) despite rotations/scale.
    assert!(
        (result.x - 10.0).abs() < 1e-9,
        "expected x≈10.0, got {}",
        result.x
    );
    assert!(
        (result.y - 20.0).abs() < 1e-9,
        "expected y≈20.0, got {}",
        result.y
    );
}

// ---------------------------------------------------------------------------
// Helmert — apply tests
// ---------------------------------------------------------------------------

#[test]
fn test_apply_helmert_identity_zero_params() {
    // All-zero Helmert is identity.
    let pipeline = Pipeline::new().step(StepKind::Helmert {
        params: HelmertParams {
            tx: 0.0,
            ty: 0.0,
            tz: 0.0,
            rx: 0.0,
            ry: 0.0,
            rz: 0.0,
            s: 0.0,
            convention: HelmertConvention::PositionVector,
        },
    });

    let coord = Coordinate::new(3_000_000.0, 1_000_000.0);
    let result = pipeline.transform(&coord).expect("identity transform ok");

    assert!(
        (result.x - coord.x).abs() < 1e-6,
        "identity x mismatch: got {}",
        result.x
    );
    assert!(
        (result.y - coord.y).abs() < 1e-6,
        "identity y mismatch: got {}",
        result.y
    );
}

#[test]
fn test_apply_helmert_inverse_round_trip() {
    // Forward pipeline applies the Helmert transform.
    // Inverse pipeline uses step_inv to engage the analytic closed-form inverse.
    let params = HelmertParams {
        tx: 100.0,
        ty: 200.0,
        tz: 300.0,
        rx: 1.5,
        ry: -0.8,
        rz: 2.3,
        s: 3.0,
        convention: HelmertConvention::PositionVector,
    };

    let fwd_pipeline = Pipeline::new().step(StepKind::Helmert {
        params: params.clone(),
    });
    // step_inv invokes apply_helmert with inverse=true, using the exact 2×2 inverse.
    let inv_pipeline = Pipeline::new().step_inv(StepKind::Helmert { params });

    let original = Coordinate::new(4_000_000.0, 1_500_000.0);
    let transformed = fwd_pipeline
        .transform(&original)
        .expect("forward helmert ok");
    let recovered = inv_pipeline
        .transform(&transformed)
        .expect("inverse helmert ok");

    assert!(
        (recovered.x - original.x).abs() < 1e-6,
        "round-trip x error: expected {}, got {}",
        original.x,
        recovered.x
    );
    assert!(
        (recovered.y - original.y).abs() < 1e-6,
        "round-trip y error: expected {}, got {}",
        original.y,
        recovered.y
    );
}

// ---------------------------------------------------------------------------
// Cart — parse tests
// ---------------------------------------------------------------------------

#[test]
fn test_parse_cart_step_wgs84() {
    let s = "+proj=pipeline +step +proj=cart +ellps=WGS84";
    let pipeline = Pipeline::from_proj_string(s).expect("should parse cart pipeline");
    assert_eq!(pipeline.len(), 1);

    // lon=0°, lat=0° → X = a_WGS84, Y ≈ 0.
    let a_wgs84 = 6_378_137.0_f64;
    let coord = Coordinate::new(0.0, 0.0);
    let result = pipeline.transform(&coord).expect("cart forward ok");

    assert!(
        (result.x - a_wgs84).abs() < 1.0,
        "expected X ≈ a_WGS84 = {}, got {}",
        a_wgs84,
        result.x
    );
    assert!(result.y.abs() < 1.0, "expected Y ≈ 0, got {}", result.y);
}

#[test]
fn test_parse_ellipsoid_grs80_lookup() {
    // Verify EllipsoidParams constants.
    let ell = EllipsoidParams {
        a: 6_378_137.0,
        f: 1.0 / 298.257_222_101,
    };
    assert!(
        (ell.a - 6_378_137.0).abs() < 1e-3,
        "GRS80 a mismatch: {}",
        ell.a
    );
    let expected_f = 1.0 / 298.257_222_101;
    assert!(
        (ell.f - expected_f).abs() < 1e-12,
        "GRS80 f mismatch: {}",
        ell.f
    );

    // Parse from a pipeline string to exercise the lookup path.
    let s = "+proj=pipeline +step +proj=cart +ellps=GRS80";
    let pipeline = Pipeline::from_proj_string(s).expect("should parse cart GRS80 pipeline");
    assert_eq!(pipeline.len(), 1);

    let coord = Coordinate::new(0.0, 0.0);
    let result = pipeline.transform(&coord).expect("cart GRS80 forward ok");
    assert!(
        (result.x - 6_378_137.0).abs() < 1.0,
        "GRS80 equator X mismatch: {}",
        result.x
    );
}

// ---------------------------------------------------------------------------
// Cart — apply tests
// ---------------------------------------------------------------------------

#[test]
fn test_apply_cart_forward_origin_yields_x_a() {
    // lon=0°, lat=0°, h=0 → X=a, Y=0, Z=0 (Z dropped in 2-D pipeline).
    let a_wgs84 = 6_378_137.0_f64;
    let ell = EllipsoidParams {
        a: a_wgs84,
        f: 1.0 / 298.257_223_563,
    };

    let pipeline = Pipeline::new().step(StepKind::Cart { ellipsoid: ell });

    let coord = Coordinate::new(0.0, 0.0);
    let result = pipeline.transform(&coord).expect("cart forward ok");

    assert!(
        (result.x - a_wgs84).abs() < 1e-4,
        "expected X ≈ {}, got {}",
        a_wgs84,
        result.x
    );
    assert!(result.y.abs() < 1e-4, "expected Y ≈ 0, got {}", result.y);
}

#[test]
fn test_apply_cart_round_trip_bowring() {
    // Forward then inverse should recover (lon°, lat°) within 1e-6.
    // We use (0°, 0°) so that the implicit Z=0 in the 2-D pipeline is exact.
    let ell = EllipsoidParams {
        a: 6_378_137.0,
        f: 1.0 / 298.257_223_563,
    };

    let fwd_pipeline = Pipeline::new().step(StepKind::Cart {
        ellipsoid: ell.clone(),
    });
    // step_inv marks the cart step as inverse (ECEF → geographic).
    let inv_pipeline = Pipeline::new().step_inv(StepKind::Cart { ellipsoid: ell });

    let original = Coordinate::new(0.0, 0.0);
    let ecef = fwd_pipeline.transform(&original).expect("cart fwd");
    let recovered = inv_pipeline.transform(&ecef).expect("cart inv");

    assert!(
        (recovered.x - original.x).abs() < 1e-6,
        "round-trip lon error: expected {}, got {}",
        original.x,
        recovered.x
    );
    assert!(
        (recovered.y - original.y).abs() < 1e-6,
        "round-trip lat error: expected {}, got {}",
        original.y,
        recovered.y
    );
}

// ---------------------------------------------------------------------------
// vgridshift / hgridshift — parse and stub-error tests
// ---------------------------------------------------------------------------

#[test]
fn test_parse_vgridshift_step_unknown_grid_yields_not_implemented_error() {
    let s = "+proj=pipeline +step +proj=vgridshift +grids=egm96_15.gtx +direction=forward";
    let pipeline = Pipeline::from_proj_string(s).expect("should parse vgridshift pipeline");
    assert_eq!(pipeline.len(), 1);

    let coord = Coordinate::new(0.0, 0.0);
    let err = pipeline
        .transform(&coord)
        .expect_err("vgridshift should fail");
    let msg = format!("{}", err);
    assert!(
        msg.contains("egm96_15.gtx") || msg.contains("not loaded"),
        "error message should mention grid: {}",
        msg
    );
}

#[test]
fn test_parse_hgridshift_step_unknown_grid_yields_not_implemented_error() {
    let s = "+proj=pipeline +step +proj=hgridshift +grids=ntv2_0.gsb +direction=forward";
    let pipeline = Pipeline::from_proj_string(s).expect("should parse hgridshift pipeline");
    assert_eq!(pipeline.len(), 1);

    let coord = Coordinate::new(0.0, 0.0);
    let err = pipeline
        .transform(&coord)
        .expect_err("hgridshift should fail");
    let msg = format!("{}", err);
    assert!(
        msg.contains("ntv2_0.gsb") || msg.contains("not loaded"),
        "error message should mention grid: {}",
        msg
    );
}

// ---------------------------------------------------------------------------
// Multi-step chain tests
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_with_helmert_then_unitconvert_chains() {
    // A two-step pipeline: Helmert translation + unit-convert (m→m, identity).
    let s = "+proj=pipeline \
             +step +proj=helmert +x=100.0 +y=200.0 +z=0.0 \
             +rx=0.0 +ry=0.0 +rz=0.0 +s=0.0 \
             +step +proj=unitconvert +xy_in=m +xy_out=m";

    let pipeline = Pipeline::from_proj_string(s).expect("should parse two-step pipeline");
    assert_eq!(pipeline.len(), 2);

    let coord = Coordinate::new(0.0, 0.0);
    let result = pipeline.transform(&coord).expect("two-step chain ok");

    // Helmert tx=100, ty=200; unitconvert m→m is identity.
    assert!(
        (result.x - 100.0).abs() < 1e-9,
        "expected x≈100.0, got {}",
        result.x
    );
    assert!(
        (result.y - 200.0).abs() < 1e-9,
        "expected y≈200.0, got {}",
        result.y
    );
}

#[test]
fn test_pipeline_with_cart_inverse_returns_geographic() {
    // Apply the cart step in inverse mode to known ECEF coordinates.
    // ECEF at (lon=0°, lat=0°, h=0): X=a_WGS84, Y=0, Z=0.
    // 2-D pipeline takes (X, Y) with Z=0; inverse must recover (0°, 0°).
    let a_wgs84 = 6_378_137.0_f64;
    let ell = EllipsoidParams {
        a: a_wgs84,
        f: 1.0 / 298.257_223_563,
    };

    let pipeline = Pipeline::new().step_inv(StepKind::Cart { ellipsoid: ell });

    let ecef_coord = Coordinate::new(a_wgs84, 0.0);
    let geo = pipeline.transform(&ecef_coord).expect("cart inverse ok");

    assert!(geo.x.abs() < 1e-9, "expected lon ≈ 0°, got {}", geo.x);
    assert!(geo.y.abs() < 1e-9, "expected lat ≈ 0°, got {}", geo.y);
}

// ===========================================================================
// Slice 17 W1 Part A — HelmertTemporal step kind
// ===========================================================================

// ---------------------------------------------------------------------------
// HelmertTemporal — zero-rate equivalence with static Helmert
// ---------------------------------------------------------------------------

/// All rates = 0 → HelmertTemporal must produce exactly the same output as
/// the static Helmert step with the same parameters, regardless of the epoch
/// difference.
#[test]
fn test_helmert_temporal_identity_rates_matches_static() {
    let params = HelmertParams {
        tx: 150.0,
        ty: -250.0,
        tz: 300.0,
        rx: 1.5,
        ry: -0.8,
        rz: 2.3,
        s: 3.0,
        convention: HelmertConvention::PositionVector,
    };

    let rates = HelmertRateParams {
        dtx: 0.0,
        dty: 0.0,
        dtz: 0.0,
        drx: 0.0,
        dry: 0.0,
        drz: 0.0,
        ds: 0.0,
        ref_epoch: 2000.0,
    };

    // Large epoch difference: rates=0 → no drift at all.
    let temporal_pipeline = Pipeline::new().step(StepKind::HelmertTemporal {
        params: params.clone(),
        rates,
        epoch: 2050.0,
    });
    let static_pipeline = Pipeline::new().step(StepKind::Helmert {
        params: params.clone(),
    });

    let coord = Coordinate::new(4_000_000.0, 1_500_000.0);
    let temporal_result = temporal_pipeline.transform(&coord).expect("temporal ok");
    let static_result = static_pipeline.transform(&coord).expect("static ok");

    assert!(
        (temporal_result.x - static_result.x).abs() < 1e-10,
        "x mismatch: temporal={}, static={}",
        temporal_result.x,
        static_result.x
    );
    assert!(
        (temporal_result.y - static_result.y).abs() < 1e-10,
        "y mismatch: temporal={}, static={}",
        temporal_result.y,
        static_result.y
    );
}

// ---------------------------------------------------------------------------
// HelmertTemporal — epoch equals ref_epoch → dt = 0, effective == params
// ---------------------------------------------------------------------------

/// When the evaluation epoch equals the reference epoch, dt = 0 so all rates
/// contribute zero correction.  The output must equal the static Helmert
/// applied with the same parameters.
#[test]
fn test_helmert_temporal_apply_at_ref_epoch_equals_static() {
    let params = HelmertParams {
        tx: 100.0,
        ty: 200.0,
        tz: 0.0,
        rx: 0.5,
        ry: -1.0,
        rz: 3.0,
        s: 2.5,
        convention: HelmertConvention::PositionVector,
    };

    let rates = HelmertRateParams {
        dtx: 10.0, // 10 mm/yr — would matter if dt != 0
        dty: -20.0,
        dtz: 5.0,
        drx: 100.0, // 100 mas/yr
        dry: -50.0,
        drz: 200.0,
        ds: 30.0, // 30 ppb/yr
        ref_epoch: 2015.0,
    };

    // epoch == ref_epoch → dt = 0 → zero correction from rates.
    let temporal_pipeline = Pipeline::new().step(StepKind::HelmertTemporal {
        params: params.clone(),
        rates,
        epoch: 2015.0, // same as ref_epoch
    });
    let static_pipeline = Pipeline::new().step(StepKind::Helmert {
        params: params.clone(),
    });

    let coord = Coordinate::new(3_500_000.0, 900_000.0);
    let t_res = temporal_pipeline.transform(&coord).expect("temporal ok");
    let s_res = static_pipeline.transform(&coord).expect("static ok");

    assert!(
        (t_res.x - s_res.x).abs() < 1e-10,
        "at ref_epoch: x should match static: temporal={} static={}",
        t_res.x,
        s_res.x
    );
    assert!(
        (t_res.y - s_res.y).abs() < 1e-10,
        "at ref_epoch: y should match static: temporal={} static={}",
        t_res.y,
        s_res.y
    );
}

// ---------------------------------------------------------------------------
// HelmertTemporal — translation rate correctly accumulated over 1 year
// ---------------------------------------------------------------------------

/// dtx = 100 mm/yr, all other rates/static params = 0.
/// After dt = 1 yr the effective tx must be 0.1 m (= 100 mm / 1000 * 1 yr).
#[test]
fn test_helmert_temporal_translation_rate_applied() {
    let params = HelmertParams {
        tx: 0.0,
        ty: 0.0,
        tz: 0.0,
        rx: 0.0,
        ry: 0.0,
        rz: 0.0,
        s: 0.0,
        convention: HelmertConvention::PositionVector,
    };

    let rates = HelmertRateParams {
        dtx: 100.0, // 100 mm/yr
        dty: 0.0,
        dtz: 0.0,
        drx: 0.0,
        dry: 0.0,
        drz: 0.0,
        ds: 0.0,
        ref_epoch: 2010.0,
    };

    // dt = 1 yr → effective tx = 0 + 100/1000*1 = 0.1 m
    let pipeline = Pipeline::new().step(StepKind::HelmertTemporal {
        params,
        rates,
        epoch: 2011.0,
    });

    // Input at origin → output should be (tx_eff, ty_eff) = (0.1, 0.0).
    let coord = Coordinate::new(0.0, 0.0);
    let result = pipeline.transform(&coord).expect("temporal translate ok");

    assert!(
        (result.x - 0.1).abs() < 1e-12,
        "expected x ≈ 0.1 m (from 100 mm/yr * 1 yr), got {}",
        result.x
    );
    assert!(result.y.abs() < 1e-12, "expected y ≈ 0, got {}", result.y);
}

// ---------------------------------------------------------------------------
// HelmertTemporal — rotation rate correctly accumulated over 1 year
// ---------------------------------------------------------------------------

/// drz = 1000 mas/yr, dt = 1 yr → effective rz = 1000/1000 = 1 arcsec.
/// We verify the rotation effect by applying to a known Cartesian point.
///
/// For a point on the +X axis (x=R, y=0) with Position Vector convention:
///   X' = tx + scale*X + drz*Y = 0 + 1*R + 0 = R   (X unchanged at Y=0)
///   Y' = ty − drz*X + scale*Y = 0 − drz*R + 0 = −drz*R
///
/// drz in radians = 1 arcsec * π/648_000 ≈ 4.848e-6 rad
#[test]
fn test_helmert_temporal_rotation_rate_applied() {
    let params = HelmertParams {
        tx: 0.0,
        ty: 0.0,
        tz: 0.0,
        rx: 0.0,
        ry: 0.0,
        rz: 0.0, // zero at ref_epoch
        s: 0.0,
        convention: HelmertConvention::PositionVector,
    };

    // drz = 1000 mas/yr → after 1 yr → effective rz = 1 arcsec
    let rates = HelmertRateParams {
        dtx: 0.0,
        dty: 0.0,
        dtz: 0.0,
        drx: 0.0,
        dry: 0.0,
        drz: 1_000.0, // 1000 mas/yr = 1 arcsec/yr
        ds: 0.0,
        ref_epoch: 2000.0,
    };

    let pipeline = Pipeline::new().step(StepKind::HelmertTemporal {
        params,
        rates,
        epoch: 2001.0, // dt = 1 yr → rz_eff = 1 arcsec
    });

    // R = 1_000_000 m for numerical precision
    let r = 1_000_000.0_f64;
    let coord = Coordinate::new(r, 0.0);
    let result = pipeline.transform(&coord).expect("temporal rotation ok");

    // Expected: drz_rad = 1 * π/648_000
    let drz_rad = std::f64::consts::PI / 648_000.0;
    let expected_dy = -drz_rad * r;

    assert!(
        (result.x - r).abs() < 1e-6,
        "x should remain ≈ R: expected {}, got {}",
        r,
        result.x
    );
    assert!(
        (result.y - expected_dy).abs() < 1e-3,
        "y should be ≈ {}: got {}",
        expected_dy,
        result.y
    );
}

// ---------------------------------------------------------------------------
// HelmertTemporal — forward + inverse recovers input within 1e-6
// ---------------------------------------------------------------------------

/// Applying HelmertTemporal forward then inverse (using step_inv) must
/// recover the original coordinate to within 1e-6 m.
#[test]
fn test_helmert_temporal_round_trip_inverse() {
    let params = HelmertParams {
        tx: 50.0,
        ty: -30.0,
        tz: 20.0,
        rx: 0.3,
        ry: -0.5,
        rz: 1.2,
        s: 1.5,
        convention: HelmertConvention::PositionVector,
    };

    let rates = HelmertRateParams {
        dtx: 15.0,
        dty: -25.0,
        dtz: 10.0,
        drx: 50.0,
        dry: -30.0,
        drz: 80.0,
        ds: 5.0,
        ref_epoch: 2005.0,
    };

    let epoch = 2020.0;

    let fwd_pipeline = Pipeline::new().step(StepKind::HelmertTemporal {
        params: params.clone(),
        rates: rates.clone(),
        epoch,
    });
    let inv_pipeline = Pipeline::new().step_inv(StepKind::HelmertTemporal {
        params,
        rates,
        epoch,
    });

    let original = Coordinate::new(3_800_000.0, 1_200_000.0);
    let transformed = fwd_pipeline.transform(&original).expect("fwd ok");
    let recovered = inv_pipeline.transform(&transformed).expect("inv ok");

    assert!(
        (recovered.x - original.x).abs() < 1e-6,
        "round-trip x error: expected {}, got {}",
        original.x,
        recovered.x
    );
    assert!(
        (recovered.y - original.y).abs() < 1e-6,
        "round-trip y error: expected {}, got {}",
        original.y,
        recovered.y
    );
}

// ---------------------------------------------------------------------------
// HelmertTemporal — parse from full PROJ pipeline string
// ---------------------------------------------------------------------------

/// Parsing a `+proj=helmert_temporal` step via `parse_pipeline` must produce
/// a map with all rate parameters correctly populated.
#[test]
fn test_parse_helmert_temporal_from_pipeline_string() {
    let s = "+proj=pipeline \
             +step +proj=helmert_temporal \
             +x=10.0 +y=20.0 +z=30.0 \
             +rx=0.5 +ry=-0.3 +rz=1.2 +s=2.0 \
             +dtx=15.0 +dty=-25.0 +dtz=10.0 \
             +drx=50.0 +dry=-30.0 +drz=80.0 \
             +ds=5.0 \
             +ref_epoch=2010.0 +t_epoch=2025.5";

    // Verify the raw parameter map is populated.
    let parsed = parse_pipeline(s).expect("parse_pipeline ok");
    assert_eq!(parsed.len(), 1, "should have exactly one step");

    let (step_params, inv) = &parsed[0];
    assert!(!inv, "step should not be inverse");
    assert_eq!(
        step_params.get("proj").and_then(|v| v.as_deref()),
        Some("helmert_temporal")
    );

    // Static params
    assert_eq!(
        step_params.get("x").and_then(|v| v.as_deref()),
        Some("10.0")
    );
    assert_eq!(
        step_params.get("y").and_then(|v| v.as_deref()),
        Some("20.0")
    );
    assert_eq!(
        step_params.get("z").and_then(|v| v.as_deref()),
        Some("30.0")
    );
    assert_eq!(
        step_params.get("rx").and_then(|v| v.as_deref()),
        Some("0.5")
    );
    assert_eq!(step_params.get("s").and_then(|v| v.as_deref()), Some("2.0"));

    // Rate params
    assert_eq!(
        step_params.get("dtx").and_then(|v| v.as_deref()),
        Some("15.0")
    );
    assert_eq!(
        step_params.get("dty").and_then(|v| v.as_deref()),
        Some("-25.0")
    );
    assert_eq!(
        step_params.get("dtz").and_then(|v| v.as_deref()),
        Some("10.0")
    );
    assert_eq!(
        step_params.get("drz").and_then(|v| v.as_deref()),
        Some("80.0")
    );
    assert_eq!(
        step_params.get("ds").and_then(|v| v.as_deref()),
        Some("5.0")
    );
    assert_eq!(
        step_params.get("ref_epoch").and_then(|v| v.as_deref()),
        Some("2010.0")
    );
    assert_eq!(
        step_params.get("t_epoch").and_then(|v| v.as_deref()),
        Some("2025.5")
    );

    // Build the full pipeline and verify it evaluates without error.
    let pipeline = Pipeline::from_proj_string(s).expect("should build helmert_temporal pipeline");
    assert_eq!(pipeline.len(), 1);

    let coord = Coordinate::new(4_000_000.0, 1_000_000.0);
    let result = pipeline
        .transform(&coord)
        .expect("helmert_temporal transform ok");
    assert!(
        result.x.is_finite() && result.y.is_finite(),
        "result must be finite"
    );

    // Verify epoch correctly affects the result: dt = 2025.5 - 2010.0 = 15.5 yr
    // effective tx = 10.0 + 15.0/1000 * 15.5 = 10.0 + 0.2325 = 10.2325
    // At origin (0, 0): output x ≈ tx_eff, output y ≈ ty_eff.
    let pipeline_origin = Pipeline::from_proj_string(s).expect("pipeline ok");
    let zero_coord = Coordinate::new(0.0, 0.0);
    let at_origin = pipeline_origin
        .transform(&zero_coord)
        .expect("origin transform ok");

    let expected_tx_eff = 10.0 + 15.0 / 1_000.0 * 15.5;
    let expected_ty_eff = 20.0 + (-25.0) / 1_000.0 * 15.5;
    assert!(
        (at_origin.x - expected_tx_eff).abs() < 1e-9,
        "expected effective tx={}, got {}",
        expected_tx_eff,
        at_origin.x
    );
    assert!(
        (at_origin.y - expected_ty_eff).abs() < 1e-9,
        "expected effective ty={}, got {}",
        expected_ty_eff,
        at_origin.y
    );
}
