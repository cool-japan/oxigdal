//! Property-based tests for GpuAlgebra raster algebra operations.
//!
//! Invariants tested:
//! 1. Add is commutative: A+B == B+A
//! 2. Multiply is commutative: A*B == B*A
//! 3. Subtract is anti-commutative: A-B == -(B-A)
//! 4. Clamp output is always within [min, max]
//! 5. Normalize maps range to [dst_min, dst_max]
//! 6. Sqrt of non-negative inputs gives non-negative outputs
//! 7. Abs always gives non-negative outputs
//! 8. Output length always equals input length

#![allow(clippy::expect_used)]

use oxigdal_gpu::algebra::{AlgebraOp, GpuAlgebra};
use proptest::prelude::*;

// ── Strategies ────────────────────────────────────────────────────────────────

prop_compose! {
    fn finite_f32_vec(max_len: usize)(
        v in prop::collection::vec(-1000.0f32..1000.0f32, 1..max_len)
    ) -> Vec<f32> { v }
}

prop_compose! {
    fn nonneg_f32_vec(max_len: usize)(
        v in prop::collection::vec(0.0f32..10000.0f32, 1..max_len)
    ) -> Vec<f32> { v }
}

prop_compose! {
    fn same_len_vecs()(
        len in 1usize..50usize,
        a in prop::collection::vec(-1000.0f32..1000.0f32, 1..50),
        b in prop::collection::vec(-1000.0f32..1000.0f32, 1..50),
    ) -> (Vec<f32>, Vec<f32>) {
        let min_len = len.min(a.len()).min(b.len());
        (a[..min_len].to_vec(), b[..min_len].to_vec())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn all_finite(v: &[f32]) -> bool {
    v.iter().all(|x| x.is_finite())
}

fn vecs_approx_eq(a: &[f32], b: &[f32], tol: f32) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .all(|(x, y)| (x - y).abs() <= tol || (x.is_nan() && y.is_nan()))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

proptest! {
    /// Add is commutative: A+B == B+A
    #[test]
    fn prop_add_commutative((a, b) in same_len_vecs()) {
        prop_assume!(all_finite(&a) && all_finite(&b));

        let ab = GpuAlgebra::execute(&a, Some(&b), AlgebraOp::Add, None)
            .expect("add A+B should succeed");
        let ba = GpuAlgebra::execute(&b, Some(&a), AlgebraOp::Add, None)
            .expect("add B+A should succeed");

        prop_assert_eq!(ab.len(), ba.len());
        prop_assert!(
            vecs_approx_eq(&ab, &ba, f32::EPSILON * 4.0),
            "A+B != B+A at some element"
        );
    }

    /// Multiply is commutative: A*B == B*A
    #[test]
    fn prop_mul_commutative((a, b) in same_len_vecs()) {
        prop_assume!(all_finite(&a) && all_finite(&b));

        let ab = GpuAlgebra::execute(&a, Some(&b), AlgebraOp::Multiply, None)
            .expect("mul A*B should succeed");
        let ba = GpuAlgebra::execute(&b, Some(&a), AlgebraOp::Multiply, None)
            .expect("mul B*A should succeed");

        prop_assert_eq!(ab.len(), ba.len());
        prop_assert!(
            vecs_approx_eq(&ab, &ba, f32::EPSILON * 4.0),
            "A*B != B*A at some element"
        );
    }

    /// Subtract is anti-commutative: A-B == -(B-A)
    #[test]
    fn prop_sub_anticommutative((a, b) in same_len_vecs()) {
        prop_assume!(all_finite(&a) && all_finite(&b));

        let ab = GpuAlgebra::execute(&a, Some(&b), AlgebraOp::Subtract, None)
            .expect("sub A-B should succeed");
        let ba = GpuAlgebra::execute(&b, Some(&a), AlgebraOp::Subtract, None)
            .expect("sub B-A should succeed");

        prop_assert_eq!(ab.len(), ba.len());

        for (i, (v_ab, v_ba)) in ab.iter().zip(ba.iter()).enumerate() {
            prop_assert!(
                (v_ab + v_ba).abs() < f32::EPSILON * 16.0,
                "anti-commutativity failed at index {}: {} + {} = {}",
                i, v_ab, v_ba, v_ab + v_ba
            );
        }
    }

    /// Clamp output is always within [lo, hi].
    #[test]
    fn prop_clamp_bounded(
        data in finite_f32_vec(200),
        lo in -500.0f32..0.0f32,
        hi in 0.0f32..500.0f32,
    ) {
        prop_assume!(!data.is_empty());
        prop_assume!(lo < hi);

        let result = GpuAlgebra::execute(
            &data,
            None,
            AlgebraOp::Clamp { min: lo, max: hi },
            None,
        )
        .expect("clamp should succeed");

        prop_assert_eq!(result.len(), data.len(), "output length mismatch");

        for (i, &v) in result.iter().enumerate() {
            prop_assert!(
                v >= lo - f32::EPSILON && v <= hi + f32::EPSILON,
                "clamp output[{}]={} outside [{}, {}]",
                i, v, lo, hi
            );
        }
    }

    /// Normalize maps [src_min, src_max] → [dst_min, dst_max].
    /// After normalizing [0,100] → [0,1], all outputs should be in [0,1].
    #[test]
    fn prop_normalize_range(
        data in prop::collection::vec(0.0f32..100.0f32, 2..100)
    ) {
        let result = GpuAlgebra::execute(
            &data,
            None,
            AlgebraOp::Normalize {
                src_min: 0.0,
                src_max: 100.0,
                dst_min: 0.0,
                dst_max: 1.0,
            },
            None,
        )
        .expect("normalize should succeed");

        prop_assert_eq!(result.len(), data.len());

        for (i, &v) in result.iter().enumerate() {
            prop_assert!(
                v >= 0.0 - f32::EPSILON * 2.0 && v <= 1.0 + f32::EPSILON * 2.0,
                "normalize output[{}]={} outside [0, 1]",
                i, v
            );
        }
    }

    /// Sqrt of non-negative inputs gives non-negative, finite outputs.
    #[test]
    fn prop_sqrt_nonnegative(
        data in nonneg_f32_vec(200)
    ) {
        let result = GpuAlgebra::execute(&data, None, AlgebraOp::Sqrt, None)
            .expect("sqrt should succeed");

        prop_assert_eq!(result.len(), data.len());

        for (i, &v) in result.iter().enumerate() {
            prop_assert!(
                v >= 0.0,
                "sqrt output[{}]={} is negative",
                i, v
            );
            prop_assert!(
                v.is_finite(),
                "sqrt output[{}]={} is not finite",
                i, v
            );
        }
    }

    /// Abs always gives non-negative outputs.
    #[test]
    fn prop_abs_nonnegative(
        data in finite_f32_vec(200)
    ) {
        let result = GpuAlgebra::execute(&data, None, AlgebraOp::Abs, None)
            .expect("abs should succeed");

        prop_assert_eq!(result.len(), data.len());

        for (i, &v) in result.iter().enumerate() {
            prop_assert!(
                v >= 0.0,
                "abs output[{}]={} is negative",
                i, v
            );
        }
    }

    /// Output length equals input length for all operations.
    #[test]
    fn prop_output_length_matches_input(
        data in finite_f32_vec(100)
    ) {
        let ops = [
            AlgebraOp::Sqrt,
            AlgebraOp::Abs,
            AlgebraOp::Clamp { min: -1.0, max: 1.0 },
            AlgebraOp::Normalize {
                src_min: -1000.0,
                src_max: 1000.0,
                dst_min: 0.0,
                dst_max: 1.0,
            },
        ];

        for op in ops {
            let result = GpuAlgebra::execute(&data, None, op, None)
                .expect("op should succeed");
            prop_assert_eq!(
                result.len(),
                data.len(),
                "output length mismatch"
            );
        }
    }

    /// Min(A, B) ≤ each element of A and B.
    #[test]
    fn prop_min_leq_inputs((a, b) in same_len_vecs()) {
        prop_assume!(all_finite(&a) && all_finite(&b));

        let result = GpuAlgebra::execute(&a, Some(&b), AlgebraOp::Min, None)
            .expect("min should succeed");

        for (i, (&ra, &rb, &rv)) in a.iter().zip(b.iter()).zip(result.iter())
            .map(|((ra, rb), rv)| (ra, rb, rv))
            .enumerate()
        {
            prop_assert!(
                rv <= ra + f32::EPSILON && rv <= rb + f32::EPSILON,
                "min[{}]={} is not <= a={} or b={}",
                i, rv, ra, rb
            );
        }
    }

    /// Max(A, B) ≥ each element of A and B.
    #[test]
    fn prop_max_geq_inputs((a, b) in same_len_vecs()) {
        prop_assume!(all_finite(&a) && all_finite(&b));

        let result = GpuAlgebra::execute(&a, Some(&b), AlgebraOp::Max, None)
            .expect("max should succeed");

        for (i, (&ra, &rb, &rv)) in a.iter().zip(b.iter()).zip(result.iter())
            .map(|((ra, rb), rv)| (ra, rb, rv))
            .enumerate()
        {
            prop_assert!(
                rv >= ra - f32::EPSILON && rv >= rb - f32::EPSILON,
                "max[{}]={} is not >= a={} or b={}",
                i, rv, ra, rb
            );
        }
    }
}
