//! Integration tests for the coordinate operation pipeline.

use oxigeo_proj::{Coordinate, Pipeline, StepKind, Unit};

// ---------------------------------------------------------------------------
// AxisSwap tests
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_single_step_axisswap_order_2_1_swaps_xy() {
    let coord = Coordinate::new(10.0, 20.0);
    let pipeline = Pipeline::new().step(StepKind::AxisSwap {
        order: [2, 1, 0, 0],
    });
    let result = pipeline
        .transform(&coord)
        .expect("transform should succeed");
    assert!(
        (result.x - 20.0).abs() < 1e-10,
        "expected x=20.0, got {}",
        result.x
    );
    assert!(
        (result.y - 10.0).abs() < 1e-10,
        "expected y=10.0, got {}",
        result.y
    );
}

// ---------------------------------------------------------------------------
// UnitConvert tests
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_unit_convert_m_to_km_divides_by_1000() {
    let coord = Coordinate::new(3000.0, 5000.0);
    let pipeline = Pipeline::new().step(StepKind::UnitConvert {
        from: Unit::M,
        to: Unit::Km,
    });
    let result = pipeline
        .transform(&coord)
        .expect("unit convert should succeed");
    assert!(
        (result.x - 3.0).abs() < 1e-10,
        "expected x=3.0, got {}",
        result.x
    );
    assert!(
        (result.y - 5.0).abs() < 1e-10,
        "expected y=5.0, got {}",
        result.y
    );
}

// ---------------------------------------------------------------------------
// Two-step pipeline test
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_two_step_axisswap_then_unit_convert() {
    // Step 1: swap (10, 20) → (20, 10)
    // Step 2: m → km  ⟹  (0.020, 0.010)
    let coord = Coordinate::new(10.0, 20.0);
    let pipeline = Pipeline::new()
        .step(StepKind::AxisSwap {
            order: [2, 1, 0, 0],
        })
        .step(StepKind::UnitConvert {
            from: Unit::M,
            to: Unit::Km,
        });
    let result = pipeline
        .transform(&coord)
        .expect("two-step transform should succeed");
    assert!(
        (result.x - 0.020).abs() < 1e-10,
        "expected x=0.020, got {}",
        result.x
    );
    assert!(
        (result.y - 0.010).abs() < 1e-10,
        "expected y=0.010, got {}",
        result.y
    );
}

// ---------------------------------------------------------------------------
// from_proj_string tests
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_from_proj_string_recognises_step_keyword() {
    let s = "+proj=pipeline +step +proj=axisswap +order=2,1 +step +proj=axisswap +order=2,1";
    let pipeline = Pipeline::from_proj_string(s).expect("should parse pipeline");

    // Double axisswap is identity.
    let coord = Coordinate::new(1.0, 2.0);
    let result = pipeline
        .transform(&coord)
        .expect("double swap should succeed");
    assert!(
        (result.x - 1.0).abs() < 1e-10,
        "expected x=1.0, got {}",
        result.x
    );
    assert!(
        (result.y - 2.0).abs() < 1e-10,
        "expected y=2.0, got {}",
        result.y
    );
}

// ---------------------------------------------------------------------------
// Global inverse tests
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_inv_global_reverses_all_steps() {
    // Forward pipeline: axisswap (swap x↔y), then m→km.
    // Inverse pipeline: first km→m (inverse of step 2), then axisswap (inverse of step 1).
    //
    // Starting coord (after forward): swap(1000, 2000) = (2000, 1000) → /1000 = (2.0, 1.0)
    // Inverse of that: multiply by 1000 → (2000, 1000) → swap → (1000, 2000). ✓

    let forward = Pipeline::new()
        .step(StepKind::AxisSwap {
            order: [2, 1, 0, 0],
        })
        .step(StepKind::UnitConvert {
            from: Unit::M,
            to: Unit::Km,
        });

    let backward = forward.clone().with_inverse(true);

    let original = Coordinate::new(1000.0, 2000.0);
    let fwd_result = forward
        .transform(&original)
        .expect("forward transform should succeed");
    let bwd_result = backward
        .transform(&fwd_result)
        .expect("backward transform should succeed");

    assert!(
        (bwd_result.x - original.x).abs() < 1e-8,
        "round-trip x: expected {}, got {}",
        original.x,
        bwd_result.x
    );
    assert!(
        (bwd_result.y - original.y).abs() < 1e-8,
        "round-trip y: expected {}, got {}",
        original.y,
        bwd_result.y
    );
}

// ---------------------------------------------------------------------------
// Per-step inverse test
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_inv_per_step_only_reverses_that_step() {
    // A pipeline with two steps:
    //   step 1 (forward):  UnitConvert M→Km          (÷1000)
    //   step 2 (inverse):  UnitConvert M→Km inverted (×1000, i.e. Km→M)
    //
    // Net effect is identity: ÷1000 then ×1000 = passthrough.
    let pipeline = Pipeline::new()
        .step(StepKind::UnitConvert {
            from: Unit::M,
            to: Unit::Km,
        })
        .step_inv(StepKind::UnitConvert {
            from: Unit::M,
            to: Unit::Km,
        });

    let coord = Coordinate::new(5000.0, 3000.0);
    let result = pipeline
        .transform(&coord)
        .expect("per-step inverse transform should succeed");

    assert!(
        (result.x - coord.x).abs() < 1e-8,
        "expected x={}, got {}",
        coord.x,
        result.x
    );
    assert!(
        (result.y - coord.y).abs() < 1e-8,
        "expected y={}, got {}",
        coord.y,
        result.y
    );
}

// ---------------------------------------------------------------------------
// Project step test
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_project_step_delegates_to_proj4rs() {
    // Just verify it doesn't panic on a valid proj string.
    // The result may succeed or fail depending on proj4rs support — that is OK.
    let s = "+proj=pipeline +step +proj=merc +R=6378137";
    let r = Pipeline::from_proj_string(s);
    // We don't assert success or failure — just no panic.
    let _ = r;
}

// ---------------------------------------------------------------------------
// Empty pipeline (passthrough)
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_empty_returns_passthrough() {
    let pipeline = Pipeline::new();
    let coord = Coordinate::new(1.0, 2.0);
    let result = pipeline
        .transform(&coord)
        .expect("empty pipeline should succeed");
    assert!(
        (result.x - 1.0).abs() < 1e-10,
        "expected x=1.0, got {}",
        result.x
    );
    assert!(
        (result.y - 2.0).abs() < 1e-10,
        "expected y=2.0, got {}",
        result.y
    );
}

// ---------------------------------------------------------------------------
// transform_many test
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_transform_many() {
    let pipeline = Pipeline::new().step(StepKind::AxisSwap {
        order: [2, 1, 0, 0],
    });
    let coords = vec![
        Coordinate::new(1.0, 10.0),
        Coordinate::new(2.0, 20.0),
        Coordinate::new(3.0, 30.0),
    ];
    let results = pipeline
        .transform_many(&coords)
        .expect("transform_many should succeed");
    assert_eq!(results.len(), 3);
    assert!((results[0].x - 10.0).abs() < 1e-10);
    assert!((results[0].y - 1.0).abs() < 1e-10);
    assert!((results[1].x - 20.0).abs() < 1e-10);
    assert!((results[2].x - 30.0).abs() < 1e-10);
}
