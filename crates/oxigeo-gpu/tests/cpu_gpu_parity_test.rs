//! GPU-vs-CPU parity tests for the `cpu_fallback` module.
//!
//! `cpu_fallback::cpu::*` is meant to be a numerically-equivalent substitute for
//! the GPU kernels it replaces. These tests run each CPU function and its GPU
//! counterpart on identical inputs and assert the outputs agree within
//! tolerance, so a silent divergence (different epsilon, NaN handling, etc.)
//! would be caught rather than only comparing each path to hand-written numbers.
//!
//! Every test self-gates on `GpuContext::new()`: without GPU hardware the whole
//! test is a no-op; with hardware it performs the real comparison.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use oxigeo_gpu::{ComputePipeline, ElementWiseOp, GpuBuffer, GpuContext, ScalarOp, cpu};
use wgpu::BufferUsages;

fn assert_close(a: &[f32], b: &[f32], what: &str) {
    assert_eq!(a.len(), b.len(), "{what}: length mismatch");
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        assert!(
            (x - y).abs() < 1e-3,
            "{what}: element {i} diverges: gpu={x} cpu={y}"
        );
    }
}

async fn gpu_binary(ctx: &GpuContext, a: &[f32], b: &[f32], op: ElementWiseOp) -> Vec<f32> {
    let b_buf = GpuBuffer::from_data(
        ctx,
        b,
        BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
    )
    .expect("b buffer");
    let pipeline = ComputePipeline::from_data(ctx, a, a.len() as u32, 1).expect("pipeline");
    pipeline
        .element_wise(op, &b_buf)
        .expect("element_wise")
        .read()
        .await
        .expect("read")
}

#[tokio::test]
async fn test_binary_ops_gpu_matches_cpu() {
    if let Ok(ctx) = GpuContext::new().await {
        let a: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let b: Vec<f32> = vec![2.0, 4.0, 1.0, 8.0, 5.0, 3.0, 9.0, 2.0];

        assert_close(
            &gpu_binary(&ctx, &a, &b, ElementWiseOp::Add).await,
            &cpu::add_slices(&a, &b),
            "add",
        );
        assert_close(
            &gpu_binary(&ctx, &a, &b, ElementWiseOp::Subtract).await,
            &cpu::sub_slices(&a, &b),
            "sub",
        );
        assert_close(
            &gpu_binary(&ctx, &a, &b, ElementWiseOp::Multiply).await,
            &cpu::mul_slices(&a, &b),
            "mul",
        );
        // Division: all b non-zero, so both paths take the plain a/b branch.
        assert_close(
            &gpu_binary(&ctx, &a, &b, ElementWiseOp::Divide).await,
            &cpu::div_slices(&a, &b),
            "div",
        );
        assert_close(
            &gpu_binary(&ctx, &a, &b, ElementWiseOp::Min).await,
            &cpu::min_slices(&a, &b),
            "min",
        );
        assert_close(
            &gpu_binary(&ctx, &a, &b, ElementWiseOp::Max).await,
            &cpu::max_slices(&a, &b),
            "max",
        );
    }
}

#[tokio::test]
async fn test_unary_ops_gpu_matches_cpu() {
    if let Ok(ctx) = GpuContext::new().await {
        let data: Vec<f32> = vec![-4.0, -1.5, 0.0, 2.25, 9.0, 16.0, 3.0, 7.0];

        let gpu_abs = ComputePipeline::from_data(&ctx, &data, data.len() as u32, 1)
            .expect("pipeline")
            .abs()
            .expect("abs")
            .read()
            .await
            .expect("read");
        assert_close(&gpu_abs, &cpu::abs(&data), "abs");

        // sqrt: GPU kernel and cpu::sqrt must BOTH guard negatives (max(0, x)).
        // The input includes negatives so a divergence would be caught.
        let gpu_sqrt = ComputePipeline::from_data(&ctx, &data, data.len() as u32, 1)
            .expect("pipeline")
            .sqrt()
            .expect("sqrt")
            .read()
            .await
            .expect("read");
        assert_close(&gpu_sqrt, &cpu::sqrt(&data), "sqrt");
    }
}

#[tokio::test]
async fn test_divide_near_zero_denominator_parity() {
    // The GPU `divide` kernel returns 0.0 when |denominator| < 1e-10; cpu::div_slices
    // must do the same. Include an exact zero and a tiny denominator.
    if let Ok(ctx) = GpuContext::new().await {
        let a: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0];
        let b: Vec<f32> = vec![0.0, 1e-12, 2.0, 4.0];

        let gpu = gpu_binary(&ctx, &a, &b, ElementWiseOp::Divide).await;
        let cpu_out = cpu::div_slices(&a, &b);
        assert_close(&gpu, &cpu_out, "divide near-zero");
        // First two must be exactly 0.0 (guarded), not inf/NaN.
        assert_eq!(cpu_out[0], 0.0);
        assert_eq!(cpu_out[1], 0.0);
    }
}

#[tokio::test]
async fn test_scalar_ops_gpu_matches_cpu() {
    if let Ok(ctx) = GpuContext::new().await {
        let data: Vec<f32> = vec![-5.0, -1.0, 0.0, 1.0, 3.0, 6.0, 10.0, 12.0];

        let gpu_add = ComputePipeline::from_data(&ctx, &data, data.len() as u32, 1)
            .expect("pipeline")
            .add(3.5)
            .expect("add")
            .read()
            .await
            .expect("read");
        assert_close(&gpu_add, &cpu::add_scalar(&data, 3.5), "add_scalar");

        let gpu_mul = ComputePipeline::from_data(&ctx, &data, data.len() as u32, 1)
            .expect("pipeline")
            .multiply(2.0)
            .expect("mul")
            .read()
            .await
            .expect("read");
        assert_close(&gpu_mul, &cpu::mul_scalar(&data, 2.0), "mul_scalar");

        let gpu_clamp = ComputePipeline::from_data(&ctx, &data, data.len() as u32, 1)
            .expect("pipeline")
            .scalar(ScalarOp::Clamp { min: 0.0, max: 8.0 })
            .expect("clamp")
            .read()
            .await
            .expect("read");
        assert_close(&gpu_clamp, &cpu::clamp(&data, 0.0, 8.0), "clamp");
    }
}
