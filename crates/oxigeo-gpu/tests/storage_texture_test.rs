//! Integration tests for the storage-texture output kernel.
//!
//! Tests that do not require a GPU backend run unconditionally.
//! Tests that require a live device use a helper that wraps `GpuContext::new()`
//! in `std::panic::catch_unwind` to gracefully handle environments where wgpu
//! panics because no backend feature is compiled in.

// Permit unwrap() in tests and relax doc requirements.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use oxigeo_gpu::{
    GpuBuffer, GpuContext,
    storage_texture::{
        build_storage_texture_kernel, make_storage_texture_shader_source, new_storage_texture,
        read_texture_to_vec_f32,
    },
};
use wgpu::{BufferUsages, TextureFormat};

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
// Tests 1–2: shader source generation (pure-Rust, no GPU)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_make_storage_texture_shader_source_rgba32float_contains_format_directive() {
    let src = make_storage_texture_shader_source(TextureFormat::Rgba32Float);
    assert!(
        src.contains("rgba32float"),
        "shader source must embed the 'rgba32float' format directive"
    );
    assert!(
        src.contains("texture_storage_2d"),
        "shader source must declare a texture_storage_2d binding"
    );
    assert!(
        src.contains("@compute"),
        "shader source must contain a compute entry point"
    );
}

#[test]
fn test_make_storage_texture_shader_source_rgba8unorm_contains_format_directive() {
    let src = make_storage_texture_shader_source(TextureFormat::Rgba8Unorm);
    assert!(
        src.contains("rgba8unorm"),
        "shader source must embed the 'rgba8unorm' format directive"
    );
    assert!(
        src.contains("texture_storage_2d"),
        "shader source must declare a texture_storage_2d binding"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3: format validation — string-based, no GPU required
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_storage_texture_format_validation_rejects_unsupported() {
    use oxigeo_gpu::storage_texture::is_supported_storage_format;

    assert!(
        !is_supported_storage_format(TextureFormat::Depth32Float),
        "Depth32Float must not be accepted as a storage texture format"
    );
    assert!(
        !is_supported_storage_format(TextureFormat::Rgba16Float),
        "Rgba16Float must not be accepted as a storage texture format"
    );
    assert!(
        is_supported_storage_format(TextureFormat::Rgba32Float),
        "Rgba32Float must be accepted"
    );
    assert!(
        is_supported_storage_format(TextureFormat::Rgba8Unorm),
        "Rgba8Unorm must be accepted"
    );
    assert!(
        is_supported_storage_format(TextureFormat::R32Float),
        "R32Float must be accepted"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4: StorageTextureBinding struct fields — backend required
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_storage_texture_binding_struct_fields_set_correctly() {
    let ctx = match try_gpu_context() {
        Some(c) => c,
        None => return, // no GPU backend — skip gracefully
    };

    let width = 64u32;
    let height = 32u32;
    let format = TextureFormat::Rgba32Float;

    let binding = new_storage_texture(&ctx, width, height, format)
        .expect("new_storage_texture must succeed for Rgba32Float");

    assert_eq!(
        binding.width, width,
        "width field must match requested width"
    );
    assert_eq!(
        binding.height, height,
        "height field must match requested height"
    );
    assert_eq!(
        binding.format, format,
        "format field must match the requested texture format"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5: StorageTextureKernel can be constructed — backend required
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_storage_texture_kernel_constructible_when_backend_present() {
    let ctx = match try_gpu_context() {
        Some(c) => c,
        None => return, // no GPU backend — skip gracefully
    };

    let format = TextureFormat::Rgba32Float;
    let wgsl = make_storage_texture_shader_source(format);

    let kernel = build_storage_texture_kernel(&ctx, &wgsl, "main", format);
    assert!(
        kernel.is_ok(),
        "StorageTextureKernel construction must succeed when a GPU backend is present; \
         error: {:?}",
        kernel.err()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6: dispatch writes expected pixel values — backend required
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_dispatch_to_texture_writes_expected_pixels_when_backend_present() {
    let ctx = match try_gpu_context() {
        Some(c) => c,
        None => return, // no GPU backend — skip gracefully
    };

    let width = 16u32;
    let height = 16u32;
    let format = TextureFormat::Rgba32Float;

    // Fill input with a constant value.
    let fill_value = 0.75f32;
    let input_data: Vec<f32> = vec![fill_value; (width * height) as usize];

    let input_buffer = GpuBuffer::from_data(
        &ctx,
        &input_data,
        BufferUsages::STORAGE | BufferUsages::COPY_SRC,
    )
    .expect("input GpuBuffer creation must succeed");

    let texture = new_storage_texture(&ctx, width, height, format)
        .expect("storage texture creation must succeed");

    let wgsl = make_storage_texture_shader_source(format);
    let kernel = build_storage_texture_kernel(&ctx, &wgsl, "main", format)
        .expect("kernel construction must succeed");

    kernel
        .dispatch_to_texture(&ctx, &input_buffer, &texture)
        .expect("dispatch_to_texture must succeed");

    // Read back and verify the R channel of every pixel.
    let result =
        read_texture_to_vec_f32(&ctx, &texture).expect("read_texture_to_vec_f32 must succeed");

    // Rgba32Float → 4 floats per pixel.
    assert_eq!(
        result.len(),
        (width * height * 4) as usize,
        "result length must match width × height × 4 for Rgba32Float"
    );

    for (i, chunk) in result.chunks(4).enumerate() {
        let r = chunk[0];
        let a = chunk[3];
        assert!(
            (r - fill_value).abs() < 1e-4,
            "pixel {i} R channel expected {fill_value}, got {r}"
        );
        assert!(
            (a - 1.0f32).abs() < 1e-4,
            "pixel {i} A channel expected 1.0, got {a}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7: round-trip write → read with a pattern — backend required
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_read_texture_round_trips_when_backend_present() {
    let ctx = match try_gpu_context() {
        Some(c) => c,
        None => return, // no GPU backend — skip gracefully
    };

    let width = 8u32;
    let height = 8u32;
    let format = TextureFormat::Rgba32Float;

    // Use a linearly increasing pattern: pixel i gets value i / total.
    let total = (width * height) as usize;
    let input_data: Vec<f32> = (0..total).map(|i| i as f32 / total as f32).collect();

    let input_buffer = GpuBuffer::from_data(
        &ctx,
        &input_data,
        BufferUsages::STORAGE | BufferUsages::COPY_SRC,
    )
    .expect("input GpuBuffer creation must succeed");

    let texture = new_storage_texture(&ctx, width, height, format)
        .expect("storage texture creation must succeed");

    let wgsl = make_storage_texture_shader_source(format);
    let kernel = build_storage_texture_kernel(&ctx, &wgsl, "main", format)
        .expect("kernel construction must succeed");

    kernel
        .dispatch_to_texture(&ctx, &input_buffer, &texture)
        .expect("dispatch_to_texture must succeed");

    let result =
        read_texture_to_vec_f32(&ctx, &texture).expect("read_texture_to_vec_f32 must succeed");

    // Rgba32Float → 4 floats per pixel; the shader copies input[idx] to R, G, B.
    assert_eq!(
        result.len(),
        total * 4,
        "result length must be width × height × 4"
    );

    for (i, chunk) in result.chunks(4).enumerate() {
        let expected = input_data[i];
        let r = chunk[0];
        assert!(
            (r - expected).abs() < 1e-4,
            "pixel {i} R channel: expected {expected:.6}, got {r:.6}"
        );
    }
}
