//! Integration tests for the bytecode compiler + VM (W2, Slice 21).
//!
//! These tests exercise the public API: `RasterCalculator::evaluate_bytecode`,
//! `eval_bytecode` with a `CompiledProgram`, and `estimate_stack_depth`.
//!
//! Tests that require direct `Expr` construction (tests 1–5 and 13) live as
//! inline `#[cfg(test)]` tests inside `bytecode.rs` because `Expr` is private
//! to the `calculator` module family.

#![allow(clippy::panic)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use oxigdal_algorithms::{
    CompiledProgram, OpCode, RasterCalculator, estimate_stack_depth, eval_bytecode,
};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Build a 1×1 raster filled with a single value.
fn single_pixel_band(val: f64) -> RasterBuffer {
    let mut b = RasterBuffer::zeros(1, 1, RasterDataType::Float32);
    b.set_pixel(0, 0, val).expect("set_pixel should not fail");
    b
}

/// Build a 1×N raster from a slice of values (width=1).
fn column_band(vals: &[f64]) -> RasterBuffer {
    let n = vals.len() as u64;
    let mut b = RasterBuffer::zeros(1, n, RasterDataType::Float32);
    for (i, &v) in vals.iter().enumerate() {
        b.set_pixel(0, i as u64, v)
            .expect("set_pixel should not fail");
    }
    b
}

/// Extract all pixel values from a raster in row-major order.
fn extract_pixels(r: &RasterBuffer) -> Vec<f64> {
    let mut out = Vec::new();
    for y in 0..r.height() {
        for x in 0..r.width() {
            out.push(r.get_pixel(x, y).expect("get_pixel should not fail"));
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests 6–8: eval_bytecode with hand-built CompiledProgram
// ─────────────────────────────────────────────────────────────────────────────

/// Test 6: Program that loads const 1.0 evaluates to 1.0.
#[test]
fn test_eval_bytecode_constant_one() {
    let prog = CompiledProgram {
        ops: vec![OpCode::LoadConst(0)],
        constants: vec![1.0],
        required_bands: 0,
        cache_slot_count: 0,
        estimated_stack_depth: 1,
    };
    let mut cache = Vec::new();
    let mut stack = Vec::with_capacity(4);
    let result = eval_bytecode(&prog, &[], 0, &mut cache, &mut stack).expect("eval should succeed");
    assert!(
        (result - 1.0).abs() < f64::EPSILON,
        "expected 1.0, got {result}"
    );
}

/// Test 7: (b0 + b1) at pixel 0 → 4.0; at pixel 1 → 6.0.
///
/// bands[0] = [1.0, 2.0]   (B1, 1-indexed → LoadBand(1))
/// bands[1] = [3.0, 4.0]   (B2, 1-indexed → LoadBand(2))
#[test]
fn test_eval_bytecode_band_add() {
    let prog = CompiledProgram {
        ops: vec![OpCode::LoadBand(1), OpCode::LoadBand(2), OpCode::Add],
        constants: vec![],
        required_bands: 2,
        cache_slot_count: 0,
        estimated_stack_depth: 2,
    };
    let b0: &[f64] = &[1.0, 2.0];
    let b1: &[f64] = &[3.0, 4.0];
    let bands: &[&[f64]] = &[b0, b1];

    let mut cache = Vec::new();
    let mut stack = Vec::with_capacity(4);

    let pixel0 = eval_bytecode(&prog, bands, 0, &mut cache, &mut stack)
        .expect("eval pixel 0 should succeed");
    assert!(
        (pixel0 - 4.0).abs() < f64::EPSILON,
        "expected 4.0, got {pixel0}"
    );

    let pixel1 = eval_bytecode(&prog, bands, 1, &mut cache, &mut stack)
        .expect("eval pixel 1 should succeed");
    assert!(
        (pixel1 - 6.0).abs() < f64::EPSILON,
        "expected 6.0, got {pixel1}"
    );
}

/// Test 8: sqrt(b0) at pixel 0 (value 4.0) → 2.0; at pixel 1 (value 9.0) → 3.0.
#[test]
fn test_eval_bytecode_sqrt() {
    let prog = CompiledProgram {
        ops: vec![OpCode::LoadBand(1), OpCode::Sqrt],
        constants: vec![],
        required_bands: 1,
        cache_slot_count: 0,
        estimated_stack_depth: 1,
    };
    let band: &[f64] = &[4.0, 9.0];
    let bands: &[&[f64]] = &[band];

    let mut cache = Vec::new();
    let mut stack = Vec::with_capacity(4);

    let pixel0 = eval_bytecode(&prog, bands, 0, &mut cache, &mut stack)
        .expect("eval pixel 0 should succeed");
    assert!(
        (pixel0 - 2.0).abs() < f64::EPSILON,
        "sqrt(4)=2, got {pixel0}"
    );

    let pixel1 = eval_bytecode(&prog, bands, 1, &mut cache, &mut stack)
        .expect("eval pixel 1 should succeed");
    assert!(
        (pixel1 - 3.0).abs() < f64::EPSILON,
        "sqrt(9)=3, got {pixel1}"
    );
}

/// Test 9: Division by zero → NaN (not an error).
#[test]
fn test_eval_bytecode_division_by_zero() {
    // 1.0 / 0.0 → NaN
    let prog = CompiledProgram {
        ops: vec![OpCode::LoadConst(0), OpCode::LoadConst(1), OpCode::Div],
        constants: vec![1.0, 0.0],
        required_bands: 0,
        cache_slot_count: 0,
        estimated_stack_depth: 2,
    };
    let mut cache = Vec::new();
    let mut stack = Vec::with_capacity(4);
    let result = eval_bytecode(&prog, &[], 0, &mut cache, &mut stack)
        .expect("division by zero should not be an error");
    assert!(result.is_nan(), "1.0/0.0 should be NaN, got {result}");
}

/// Test 10: Cond with true condition (cond=1.0) → then_val.
#[test]
fn test_eval_bytecode_cond_true_branch() {
    // Stack before Cond: [cond=1.0, then=42.0, else=99.0]
    let prog = CompiledProgram {
        ops: vec![
            OpCode::LoadConst(0), // cond = 1.0
            OpCode::LoadConst(1), // then_val = 42.0
            OpCode::LoadConst(2), // else_val = 99.0
            OpCode::Cond,
        ],
        constants: vec![1.0, 42.0, 99.0],
        required_bands: 0,
        cache_slot_count: 0,
        estimated_stack_depth: 3,
    };
    let mut cache = Vec::new();
    let mut stack = Vec::with_capacity(4);
    let result = eval_bytecode(&prog, &[], 0, &mut cache, &mut stack)
        .expect("cond true branch should succeed");
    assert!(
        (result - 42.0).abs() < f64::EPSILON,
        "cond=1.0 should pick then_val=42.0, got {result}"
    );
}

/// Test 11: Cond with false condition (cond=0.0) → else_val.
#[test]
fn test_eval_bytecode_cond_false_branch() {
    let prog = CompiledProgram {
        ops: vec![
            OpCode::LoadConst(0), // cond = 0.0
            OpCode::LoadConst(1), // then_val = 42.0
            OpCode::LoadConst(2), // else_val = 99.0
            OpCode::Cond,
        ],
        constants: vec![0.0, 42.0, 99.0],
        required_bands: 0,
        cache_slot_count: 0,
        estimated_stack_depth: 3,
    };
    let mut cache = Vec::new();
    let mut stack = Vec::with_capacity(4);
    let result = eval_bytecode(&prog, &[], 0, &mut cache, &mut stack)
        .expect("cond false branch should succeed");
    assert!(
        (result - 99.0).abs() < f64::EPSILON,
        "cond=0.0 should pick else_val=99.0, got {result}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 12: evaluate_bytecode matches tree-walk for NDVI
// ─────────────────────────────────────────────────────────────────────────────

/// Test 12: NDVI via string-parse must match the tree-walking evaluator exactly.
///
/// NDVI = (B1 - B2) / (B1 + B2)  where B1 = NIR, B2 = Red.
#[test]
fn test_evaluate_bytecode_matches_tree_eval_for_ndvi() {
    // Use deterministic pixel values (simulating random data).
    let nir_vals: [f64; 8] = [100.0, 120.0, 80.0, 200.0, 50.0, 300.0, 90.0, 150.0];
    let red_vals: [f64; 8] = [50.0, 40.0, 60.0, 100.0, 30.0, 200.0, 45.0, 75.0];

    let n = nir_vals.len() as u64;
    let mut nir = RasterBuffer::zeros(1, n, RasterDataType::Float32);
    let mut red = RasterBuffer::zeros(1, n, RasterDataType::Float32);
    for i in 0..nir_vals.len() {
        nir.set_pixel(0, i as u64, nir_vals[i]).expect("set_pixel");
        red.set_pixel(0, i as u64, red_vals[i]).expect("set_pixel");
    }

    let expr = "(B1 - B2) / (B1 + B2)";

    let tree_result = RasterCalculator::evaluate(expr, &[nir.clone(), red.clone()])
        .expect("tree evaluate should succeed");
    let byte_result = RasterCalculator::evaluate_bytecode(expr, &[nir.clone(), red.clone()])
        .expect("bytecode evaluate should succeed");

    let tree_px = extract_pixels(&tree_result);
    let byte_px = extract_pixels(&byte_result);

    assert_eq!(tree_px.len(), byte_px.len(), "pixel count must match");
    for (i, (t, b)) in tree_px.iter().zip(byte_px.iter()).enumerate() {
        if t.is_nan() {
            assert!(b.is_nan(), "pixel {i}: tree=NaN but bytecode={b}");
        } else {
            assert!(
                (t - b).abs() < 1e-9,
                "pixel {i}: tree={t} bytecode={b} differ by {}",
                (t - b).abs()
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Additional evaluate_bytecode smoke tests
// ─────────────────────────────────────────────────────────────────────────────

/// Simple arithmetic: B1 + B2 via string API.
#[test]
fn test_evaluate_bytecode_simple_add() {
    let b1 = single_pixel_band(10.0);
    let b2 = single_pixel_band(5.0);
    let result = RasterCalculator::evaluate_bytecode("B1 + B2", &[b1, b2])
        .expect("evaluate_bytecode should succeed");
    let val = result.get_pixel(0, 0).expect("get_pixel");
    assert!(
        (val - 15.0).abs() < f64::EPSILON,
        "expected 15.0, got {val}"
    );
}

/// Math function: sqrt(B1) via string API.
#[test]
fn test_evaluate_bytecode_sqrt_string() {
    let b1 = single_pixel_band(16.0);
    let result = RasterCalculator::evaluate_bytecode("sqrt(B1)", &[b1])
        .expect("evaluate_bytecode should succeed");
    let val = result.get_pixel(0, 0).expect("get_pixel");
    assert!((val - 4.0).abs() < f64::EPSILON, "sqrt(16)=4, got {val}");
}

/// Conditional expression: if B1 > 20 then 1 else 0.
#[test]
fn test_evaluate_bytecode_conditional() {
    // B1 = 30 → condition true → 1.0
    let b_true = single_pixel_band(30.0);
    let r = RasterCalculator::evaluate_bytecode("if B1 > 20 then 1 else 0", &[b_true])
        .expect("evaluate_bytecode should succeed");
    let v = r.get_pixel(0, 0).expect("get_pixel");
    assert!((v - 1.0).abs() < f64::EPSILON, "30 > 20 → 1.0, got {v}");

    // B1 = 10 → condition false → 0.0
    let b_false = single_pixel_band(10.0);
    let r2 = RasterCalculator::evaluate_bytecode("if B1 > 20 then 1 else 0", &[b_false])
        .expect("evaluate_bytecode should succeed");
    let v2 = r2.get_pixel(0, 0).expect("get_pixel");
    assert!((v2 - 0.0).abs() < f64::EPSILON, "10 > 20 → 0.0, got {v2}");
}

/// Division by zero via string API → NaN pixel.
#[test]
fn test_evaluate_bytecode_div_by_zero_string() {
    let b1 = single_pixel_band(10.0);
    let b2 = single_pixel_band(0.0);
    let result = RasterCalculator::evaluate_bytecode("B1 / B2", &[b1, b2])
        .expect("evaluate_bytecode should succeed even for div-by-zero");
    let val = result.get_pixel(0, 0).expect("get_pixel");
    assert!(val.is_nan(), "B1/0.0 should be NaN, got {val}");
}

/// estimate_stack_depth for a program with a Cond (depth should be >= 3).
#[test]
fn test_estimate_stack_depth_cond() {
    let ops = vec![
        OpCode::LoadConst(0), // +1 → 1
        OpCode::LoadConst(1), // +1 → 2
        OpCode::LoadConst(2), // +1 → 3
        OpCode::Cond,         // -2 → 1
    ];
    let depth = estimate_stack_depth(&ops);
    assert_eq!(depth, 3, "Cond needs 3-deep stack, got {depth}");
}

/// Empty bands returns EmptyInput error.
#[test]
fn test_evaluate_bytecode_empty_bands_error() {
    let result = RasterCalculator::evaluate_bytecode("B1 + 1", &[]);
    assert!(result.is_err(), "empty bands should be an error");
}

/// Mismatched band dimensions returns InvalidDimensions error.
#[test]
fn test_evaluate_bytecode_mismatched_dimensions() {
    let b1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
    let b2 = RasterBuffer::zeros(3, 3, RasterDataType::Float32);
    let result = RasterCalculator::evaluate_bytecode("B1 + B2", &[b1, b2]);
    assert!(result.is_err(), "mismatched dimensions should be an error");
}

/// Multi-pixel: verify each pixel evaluated correctly for a larger raster.
#[test]
fn test_evaluate_bytecode_multi_pixel_column() {
    // B1 = [1, 4, 9, 16], expression = sqrt(B1)
    let vals = vec![1.0_f64, 4.0, 9.0, 16.0];
    let b = column_band(&vals);
    let result = RasterCalculator::evaluate_bytecode("sqrt(B1)", &[b])
        .expect("evaluate_bytecode should succeed");
    let pxs = extract_pixels(&result);
    let expected = [1.0_f64, 2.0, 3.0, 4.0];
    for (i, (&actual, &exp)) in pxs.iter().zip(expected.iter()).enumerate() {
        assert!(
            (actual - exp).abs() < 1e-9,
            "pixel {i}: expected {exp}, got {actual}"
        );
    }
}
