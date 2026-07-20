//! Integration tests for the texture-based hardware-sampler resampling path.
//!
//! Pure-Rust tests (shader-source inspection, filter-mode mapping, struct
//! traits) run unconditionally.  Tests that require an actual wgpu device wrap
//! `GpuContext::new()` in `std::panic::catch_unwind` so the suite is robust to
//! environments where no GPU backend is compiled in.

// Permit unwrap() in tests and relax doc requirements.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use oxigdal_gpu::{
    GpuContext,
    kernels::resampling::ResamplingMethod,
    texture_resample::{
        TextureFilterMethod, TextureResampler, make_texture_resample_shader_source,
        new_input_texture_r32float, texture_filter_for_resampling,
    },
};
use wgpu::TextureFormat;

// ─────────────────────────────────────────────────────────────────────────────
// Helper: try to create a GPU context, returning None if unavailable.
// ─────────────────────────────────────────────────────────────────────────────

/// Attempt to obtain a `GpuContext`, returning `None` when no GPU backend is
/// compiled in or no adapter is present on the machine.
///
/// `wgpu` panics synchronously when no backend feature flag is enabled, so we
/// must wrap the future creation inside `catch_unwind`.
fn try_gpu_context() -> Option<GpuContext> {
    use std::panic::AssertUnwindSafe;

    let result =
        std::panic::catch_unwind(AssertUnwindSafe(|| pollster::block_on(GpuContext::new())));

    match result {
        Ok(Ok(ctx)) => Some(ctx),
        _ => None, // no GPU backend compiled / no adapter present / any other failure
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure-Rust tests: shader source generation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_make_texture_resample_shader_source_contains_sampler() {
    let src = make_texture_resample_shader_source(
        TextureFilterMethod::Linear,
        TextureFormat::Rgba32Float,
    );
    assert!(
        src.contains("sampler"),
        "shader source must declare a sampler binding"
    );
}

#[test]
fn test_make_texture_resample_shader_source_contains_texture_sample_level() {
    let src = make_texture_resample_shader_source(
        TextureFilterMethod::Linear,
        TextureFormat::Rgba32Float,
    );
    assert!(
        src.contains("textureSampleLevel"),
        "shader source must use textureSampleLevel for hardware-sampled filtering"
    );
}

#[test]
fn test_make_texture_resample_shader_source_dst_format_rgba32float() {
    let src = make_texture_resample_shader_source(
        TextureFilterMethod::Linear,
        TextureFormat::Rgba32Float,
    );
    assert!(
        src.contains("rgba32float"),
        "shader source must embed the destination storage-texture format directive"
    );
}

#[test]
fn test_make_texture_resample_shader_source_dst_format_r32float() {
    let src =
        make_texture_resample_shader_source(TextureFilterMethod::Nearest, TextureFormat::R32Float);
    assert!(
        src.contains("r32float"),
        "shader source must embed the r32float storage-texture format directive"
    );
}

#[test]
fn test_make_texture_resample_shader_source_has_compute_entry_point() {
    let src = make_texture_resample_shader_source(
        TextureFilterMethod::Linear,
        TextureFormat::Rgba32Float,
    );
    assert!(
        src.contains("@compute"),
        "shader source must declare a @compute entry point"
    );
    assert!(
        src.contains("@workgroup_size(16, 16)"),
        "shader source must use a 16x16 workgroup size"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure-Rust tests: ResamplingMethod → TextureFilterMethod mapping
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_texture_filter_for_resampling_nearest() {
    assert_eq!(
        texture_filter_for_resampling(ResamplingMethod::NearestNeighbor),
        Some(TextureFilterMethod::Nearest)
    );
}

#[test]
fn test_texture_filter_for_resampling_bilinear() {
    assert_eq!(
        texture_filter_for_resampling(ResamplingMethod::Bilinear),
        Some(TextureFilterMethod::Linear)
    );
}

#[test]
fn test_texture_filter_for_resampling_bicubic_falls_back_linear() {
    assert_eq!(
        texture_filter_for_resampling(ResamplingMethod::Bicubic),
        Some(TextureFilterMethod::Linear),
        "Bicubic must fall back to Linear filtering for the texture path"
    );
}

#[test]
fn test_texture_filter_for_resampling_lanczos_returns_none() {
    assert_eq!(
        texture_filter_for_resampling(ResamplingMethod::Lanczos { a: 2 }),
        None,
        "Lanczos must not map to a hardware sampler — caller must use the compute-buffer path"
    );
    assert_eq!(
        texture_filter_for_resampling(ResamplingMethod::Lanczos { a: 3 }),
        None
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure-Rust tests: TextureFilterMethod trait behaviour
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_texture_filter_method_debug_clone_eq() {
    let nearest = TextureFilterMethod::Nearest;
    let linear = TextureFilterMethod::Linear;

    // Debug formatting should not panic and should produce a non-empty string.
    assert!(!format!("{nearest:?}").is_empty());
    assert!(!format!("{linear:?}").is_empty());

    // Clone semantics — Copy/Clone derive correctness.
    let nearest_copy = nearest;
    let linear_copy = linear;
    assert_eq!(nearest, nearest_copy);
    assert_eq!(linear, linear_copy);

    // Equality.
    assert_eq!(TextureFilterMethod::Nearest, TextureFilterMethod::Nearest);
    assert_eq!(TextureFilterMethod::Linear, TextureFilterMethod::Linear);
    assert_ne!(TextureFilterMethod::Nearest, TextureFilterMethod::Linear);
}

#[test]
fn test_texture_filter_method_wgpu_filter_round_trip() {
    assert_eq!(
        TextureFilterMethod::Nearest.wgpu_filter(),
        wgpu::FilterMode::Nearest
    );
    assert_eq!(
        TextureFilterMethod::Linear.wgpu_filter(),
        wgpu::FilterMode::Linear
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// GPU-required tests: backend-conditional
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_texture_resampler_new_when_backend_present() {
    let ctx = match try_gpu_context() {
        Some(c) => c,
        None => return, // no GPU backend — skip gracefully
    };

    let resampler = TextureResampler::new(
        &ctx,
        TextureFilterMethod::Linear,
        TextureFormat::Rgba32Float,
    );
    assert!(
        resampler.is_ok(),
        "TextureResampler::new must succeed when a GPU backend is present; error: {:?}",
        resampler.err()
    );

    let resampler = match resampler {
        Ok(r) => r,
        Err(_) => return,
    };
    assert_eq!(resampler.method(), TextureFilterMethod::Linear);
    assert_eq!(resampler.dst_format(), TextureFormat::Rgba32Float);
}

#[test]
fn test_texture_resampler_new_rejects_unsupported_dst_format() {
    let ctx = match try_gpu_context() {
        Some(c) => c,
        None => return, // no GPU backend — skip gracefully
    };

    let result = TextureResampler::new(
        &ctx,
        TextureFilterMethod::Linear,
        TextureFormat::Depth32Float,
    );
    assert!(
        result.is_err(),
        "TextureResampler::new must reject Depth32Float as a destination format"
    );
}

#[test]
fn test_new_input_texture_r32float_size_mismatch_errors() {
    let ctx = match try_gpu_context() {
        Some(c) => c,
        None => return, // no GPU backend — skip gracefully
    };

    // width × height = 4, but data.len() = 3 — must return an error.
    let result = new_input_texture_r32float(&ctx, 2, 2, &[1.0_f32, 2.0, 3.0]);
    assert!(
        result.is_err(),
        "new_input_texture_r32float must reject data whose length != width × height"
    );
}
