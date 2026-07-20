//! Regression tests for the height-aware ("3-D") pipeline execution path.
//!
//! These tests specifically cover the fix for the `+proj=cart` /
//! `+proj=helmert` steps hard-coding Z=0 and dropping the `rx`/`ry` Helmert
//! rotation terms in the 2-D [`Pipeline::transform`]. [`Pipeline::transform_3d`]
//! threads the real height/Z through both steps.

#![allow(clippy::expect_used)]

use oxigeo_proj::{
    Coordinate3D, EllipsoidParams, HelmertConvention, HelmertParams, HelmertRateParams, Pipeline,
    StepKind, Unit,
};

// ---------------------------------------------------------------------------
// Helmert 3-D — rx/ry rotation terms are no longer discarded
// ---------------------------------------------------------------------------

#[test]
fn test_transform_3d_helmert_identity_zero_params() {
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

    let coord = Coordinate3D::new(3_000_000.0, 1_000_000.0, 5_000_000.0);
    let result = pipeline
        .transform_3d(&coord)
        .expect("identity transform_3d ok");

    assert!((result.x - coord.x).abs() < 1e-6);
    assert!((result.y - coord.y).abs() < 1e-6);
    assert!((result.z - coord.z).abs() < 1e-6);
}

#[test]
fn test_transform_3d_helmert_rz_only_matches_2d_result() {
    // With rx=ry=0, the 3-D result's (X, Y) must exactly match the existing
    // 2-D `transform` result (both reduce to the same rz-only formula).
    let params = HelmertParams {
        tx: 10.0,
        ty: -20.0,
        tz: 30.0,
        rx: 0.0,
        ry: 0.0,
        rz: 4.0,
        s: 2.0,
        convention: HelmertConvention::PositionVector,
    };

    let pipeline_2d = Pipeline::new().step(StepKind::Helmert {
        params: params.clone(),
    });
    let pipeline_3d = Pipeline::new().step(StepKind::Helmert { params });

    let coord_2d = oxigeo_proj::Coordinate::new(4_000_000.0, 1_500_000.0);
    let coord_3d = Coordinate3D::new(4_000_000.0, 1_500_000.0, 0.0);

    let result_2d = pipeline_2d.transform(&coord_2d).expect("2d transform ok");
    let result_3d = pipeline_3d
        .transform_3d(&coord_3d)
        .expect("3d transform ok");

    assert!(
        (result_2d.x - result_3d.x).abs() < 1e-9,
        "x mismatch: 2d={} 3d={}",
        result_2d.x,
        result_3d.x
    );
    assert!(
        (result_2d.y - result_3d.y).abs() < 1e-9,
        "y mismatch: 2d={} 3d={}",
        result_2d.y,
        result_3d.y
    );
}

#[test]
fn test_transform_3d_helmert_rx_ry_affect_output_when_z_nonzero() {
    // With Z != 0, non-zero rx/ry must change the output — this is exactly
    // the behavior the 2-D-only implementation silently drops.
    let base_params = HelmertParams {
        tx: 0.0,
        ty: 0.0,
        tz: 0.0,
        rx: 0.0,
        ry: 0.0,
        rz: 0.0,
        s: 0.0,
        convention: HelmertConvention::PositionVector,
    };
    let rotated_params = HelmertParams {
        rx: 5.0,
        ry: -3.0,
        ..base_params.clone()
    };

    let coord = Coordinate3D::new(4_000_000.0, 1_500_000.0, 4_500_000.0);

    let base_pipeline = Pipeline::new().step(StepKind::Helmert {
        params: base_params,
    });
    let rotated_pipeline = Pipeline::new().step(StepKind::Helmert {
        params: rotated_params,
    });

    let base_out = base_pipeline
        .transform_3d(&coord)
        .expect("base transform_3d ok");
    let rotated_out = rotated_pipeline
        .transform_3d(&coord)
        .expect("rotated transform_3d ok");

    // rx/ry rotation terms only act on non-zero Z, so with Z=4_500_000 the
    // outputs must differ measurably in X, Y, and Z.
    assert!(
        (base_out.x - rotated_out.x).abs() > 1.0,
        "expected rx/ry to move X measurably: base={} rotated={}",
        base_out.x,
        rotated_out.x
    );
    assert!(
        (base_out.y - rotated_out.y).abs() > 1.0,
        "expected rx/ry to move Y measurably: base={} rotated={}",
        base_out.y,
        rotated_out.y
    );
    assert!(
        (base_out.z - rotated_out.z).abs() > 1.0,
        "expected rx/ry to move Z measurably: base={} rotated={}",
        base_out.z,
        rotated_out.z
    );
}

#[test]
fn test_transform_3d_helmert_inverse_round_trip_with_rotation() {
    // Full 7-parameter Helmert (rx, ry, rz all non-zero) with a non-zero Z
    // input must round-trip exactly through forward + inverse via the 3x3
    // linear solve.
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
    let inv_pipeline = Pipeline::new().step_inv(StepKind::Helmert { params });

    let original = Coordinate3D::new(4_000_000.0, 1_500_000.0, 4_200_000.0);
    let transformed = fwd_pipeline
        .transform_3d(&original)
        .expect("forward helmert 3d ok");
    let recovered = inv_pipeline
        .transform_3d(&transformed)
        .expect("inverse helmert 3d ok");

    assert!(
        (recovered.x - original.x).abs() < 1e-4,
        "round-trip x error: expected {}, got {}",
        original.x,
        recovered.x
    );
    assert!(
        (recovered.y - original.y).abs() < 1e-4,
        "round-trip y error: expected {}, got {}",
        original.y,
        recovered.y
    );
    assert!(
        (recovered.z - original.z).abs() < 1e-4,
        "round-trip z error: expected {}, got {}",
        original.z,
        recovered.z
    );
}

#[test]
fn test_transform_3d_helmert_coordinate_frame_convention_round_trip() {
    let params = HelmertParams {
        tx: -50.0,
        ty: 25.0,
        tz: 10.0,
        rx: 0.7,
        ry: 0.9,
        rz: -0.4,
        s: 1.2,
        convention: HelmertConvention::CoordinateFrame,
    };

    let fwd_pipeline = Pipeline::new().step(StepKind::Helmert {
        params: params.clone(),
    });
    let inv_pipeline = Pipeline::new().step_inv(StepKind::Helmert { params });

    let original = Coordinate3D::new(6_378_137.0, 0.0, 3_000_000.0);
    let transformed = fwd_pipeline
        .transform_3d(&original)
        .expect("forward helmert 3d ok");
    let recovered = inv_pipeline
        .transform_3d(&transformed)
        .expect("inverse helmert 3d ok");

    assert!((recovered.x - original.x).abs() < 1e-4);
    assert!((recovered.y - original.y).abs() < 1e-4);
    assert!((recovered.z - original.z).abs() < 1e-4);
}

#[test]
fn test_transform_3d_helmert_temporal_applies_rotation() {
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
        dtx: 0.0,
        dty: 0.0,
        dtz: 0.0,
        drx: 10_000.0, // mas/yr — large so the effect is unmistakable at dt=1
        dry: 0.0,
        drz: 0.0,
        ds: 0.0,
        ref_epoch: 2000.0,
    };

    let pipeline = Pipeline::new().step(StepKind::HelmertTemporal {
        params,
        rates,
        epoch: 2001.0,
    });

    let coord = Coordinate3D::new(1_000_000.0, 1_000_000.0, 5_000_000.0);
    let result = pipeline
        .transform_3d(&coord)
        .expect("helmert_temporal transform_3d ok");

    // drx accumulates to rx = 10.0 arcsec after 1 year, which (with Z != 0)
    // must move Y and Z away from the identity result.
    assert!((result.y - coord.y).abs() > 1.0, "y={}", result.y);
    assert!((result.z - coord.z).abs() > 1.0, "z={}", result.z);
}

// ---------------------------------------------------------------------------
// Cart 3-D — real height threaded through, not assumed zero
// ---------------------------------------------------------------------------

#[test]
fn test_transform_3d_cart_forward_uses_real_height() {
    let a_wgs84 = 6_378_137.0_f64;
    let ell = EllipsoidParams {
        a: a_wgs84,
        f: 1.0 / 298.257_223_563,
    };
    let pipeline = Pipeline::new().step(StepKind::Cart { ellipsoid: ell });

    // At the equator/prime-meridian, X_ecef should be (a + h), not just a.
    let h = 1_000.0;
    let coord = Coordinate3D::new(0.0, 0.0, h);
    let result = pipeline.transform_3d(&coord).expect("cart forward 3d ok");

    assert!(
        (result.x - (a_wgs84 + h)).abs() < 1e-3,
        "expected X ≈ a+h = {}, got {}",
        a_wgs84 + h,
        result.x
    );
    assert!(result.y.abs() < 1e-3);
    assert!(
        result.z.abs() < 1e-3,
        "expected Z ≈ 0 at equator, got {}",
        result.z
    );
}

#[test]
fn test_transform_3d_cart_round_trip_recovers_height() {
    let ell = EllipsoidParams {
        a: 6_378_137.0,
        f: 1.0 / 298.257_223_563,
    };
    let fwd_pipeline = Pipeline::new().step(StepKind::Cart {
        ellipsoid: ell.clone(),
    });
    let inv_pipeline = Pipeline::new().step_inv(StepKind::Cart { ellipsoid: ell });

    let original = Coordinate3D::new(139.767, 35.681, 1234.5);
    let ecef = fwd_pipeline
        .transform_3d(&original)
        .expect("cart forward 3d ok");
    let recovered = inv_pipeline
        .transform_3d(&ecef)
        .expect("cart inverse 3d ok");

    assert!(
        (recovered.x - original.x).abs() < 1e-6,
        "lon round-trip: expected {}, got {}",
        original.x,
        recovered.x
    );
    assert!(
        (recovered.y - original.y).abs() < 1e-6,
        "lat round-trip: expected {}, got {}",
        original.y,
        recovered.y
    );
    assert!(
        (recovered.z - original.z).abs() < 1e-3,
        "height round-trip: expected {}, got {}",
        original.z,
        recovered.z
    );
}

// ---------------------------------------------------------------------------
// Full cart → helmert → cart⁻¹ chain (the common real-world datum-shift
// pattern) — this is the scenario the Z=0/rx=ry=0 bug silently broke.
// ---------------------------------------------------------------------------

#[test]
fn test_transform_3d_cart_helmert_cart_chain_with_height_and_rotation() {
    let ell = EllipsoidParams {
        a: 6_378_137.0,
        f: 1.0 / 298.257_223_563,
    };
    let helmert_params = HelmertParams {
        tx: 1.0,
        ty: 2.0,
        tz: 3.0,
        rx: 0.5,
        ry: -0.3,
        rz: 0.2,
        s: 0.5,
        convention: HelmertConvention::PositionVector,
    };

    let pipeline = Pipeline::new()
        .step(StepKind::Cart {
            ellipsoid: ell.clone(),
        })
        .step(StepKind::Helmert {
            params: helmert_params,
        })
        .step_inv(StepKind::Cart { ellipsoid: ell });

    // A point at a non-zero height so the rx/ry terms are actually exercised.
    let input = Coordinate3D::new(139.767, 35.681, 500.0);
    let output = pipeline
        .transform_3d(&input)
        .expect("full 3d chain transform ok");

    // A real datum shift must move both the horizontal position and the
    // height; neither should equal the input exactly, and the result must
    // be finite.
    assert!(output.is_valid(), "output must be finite: {:?}", output);
    assert!((output.x - input.x).abs() > 1e-9 || (output.y - input.y).abs() > 1e-9);
    assert!(
        (output.z - input.z).abs() > 1e-6,
        "expected height to change under a real 3D Helmert shift, got {} vs {}",
        output.z,
        input.z
    );
}

#[test]
fn test_transform_3d_non_cart_helmert_steps_pass_z_through_unchanged() {
    // AxisSwap and UnitConvert are not (yet) Z-aware; Z must survive
    // unchanged through those step kinds.
    let pipeline = Pipeline::new()
        .step(StepKind::AxisSwap {
            order: [2, 1, 0, 0],
        })
        .step(StepKind::UnitConvert {
            from: Unit::M,
            to: Unit::Km,
        });

    let coord = Coordinate3D::new(1000.0, 2000.0, 42.0);
    let result = pipeline.transform_3d(&coord).expect("transform_3d ok");

    assert!((result.x - 2.0).abs() < 1e-10);
    assert!((result.y - 1.0).abs() < 1e-10);
    assert!(
        (result.z - 42.0).abs() < 1e-10,
        "Z must pass through AxisSwap/UnitConvert unchanged, got {}",
        result.z
    );
}

#[test]
fn test_transform_many_3d() {
    let pipeline = Pipeline::new().step(StepKind::Helmert {
        params: HelmertParams {
            tx: 10.0,
            ty: 20.0,
            tz: 30.0,
            rx: 0.0,
            ry: 0.0,
            rz: 0.0,
            s: 0.0,
            convention: HelmertConvention::PositionVector,
        },
    });

    let coords = vec![
        Coordinate3D::new(0.0, 0.0, 0.0),
        Coordinate3D::new(100.0, 200.0, 300.0),
    ];
    let results = pipeline
        .transform_many_3d(&coords)
        .expect("transform_many_3d ok");

    assert_eq!(results.len(), 2);
    assert!((results[0].x - 10.0).abs() < 1e-9);
    assert!((results[0].y - 20.0).abs() < 1e-9);
    assert!((results[0].z - 30.0).abs() < 1e-9);
    assert!((results[1].x - 110.0).abs() < 1e-9);
    assert!((results[1].y - 220.0).abs() < 1e-9);
    assert!((results[1].z - 330.0).abs() < 1e-9);
}
