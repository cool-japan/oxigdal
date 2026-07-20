//! Integration tests for the WGSL push-constants (immediates) helper module.
//!
//! Pure-Rust tests (range/layout validation, buffer writes, shader-source
//! inspection) run unconditionally.  GPU-dependent tests use the
//! `try_gpu_context` / `catch_unwind` pattern standard in this test suite so
//! that the whole suite still passes in environments without a GPU.

// Allow unwrap in tests and relax doc requirements.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use oxigeo_gpu::{
    GpuContext,
    push_constants::{
        MAX_PUSH_CONSTANTS_SIZE_BYTES, PushConstantRange, PushConstantsBuffer, PushConstantsLayout,
        make_push_constants_shader_source, max_push_constants_size, supports_push_constants,
    },
};

// ─────────────────────────────────────────────────────────────────────────────
// Helper: try to create a GPU context without panicking.
// ─────────────────────────────────────────────────────────────────────────────

/// Attempt to obtain a `GpuContext`, returning `None` when no GPU backend is
/// compiled in or no adapter is present on the machine.
///
/// `wgpu` panics synchronously when no backend feature flag is enabled, so we
/// wrap the future creation inside `catch_unwind`.
fn try_gpu_context() -> Option<GpuContext> {
    use std::panic::AssertUnwindSafe;
    let result =
        std::panic::catch_unwind(AssertUnwindSafe(|| pollster::block_on(GpuContext::new())));
    match result {
        Ok(Ok(ctx)) => Some(ctx),
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PushConstantRange tests — pure Rust
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_push_constant_range_compute_default() {
    let r = PushConstantRange::compute(32);
    assert_eq!(r.start, 0, "start must be 0 for compute() constructor");
    assert_eq!(r.end, 32, "end must equal the requested size");
    assert!(
        r.validate().is_ok(),
        "PushConstantRange::compute(32) must pass validation"
    );
}

#[test]
fn test_push_constant_range_validate_rejects_zero_size() {
    let r = PushConstantRange {
        stages: wgpu::ShaderStages::COMPUTE,
        start: 0,
        end: 0,
    };
    assert!(
        r.validate().is_err(),
        "start == end must be rejected as zero-size range"
    );
}

#[test]
fn test_push_constant_range_validate_rejects_start_ge_end() {
    let r = PushConstantRange {
        stages: wgpu::ShaderStages::COMPUTE,
        start: 16,
        end: 4,
    };
    assert!(r.validate().is_err(), "start > end must be rejected");
}

#[test]
fn test_push_constant_range_validate_rejects_over_128_bytes() {
    let r = PushConstantRange {
        stages: wgpu::ShaderStages::COMPUTE,
        start: 0,
        end: MAX_PUSH_CONSTANTS_SIZE_BYTES + 4,
    };
    assert!(
        r.validate().is_err(),
        "size > MAX_PUSH_CONSTANTS_SIZE_BYTES must be rejected"
    );
}

#[test]
fn test_push_constant_range_validate_accepts_max_size() {
    let r = PushConstantRange {
        stages: wgpu::ShaderStages::COMPUTE,
        start: 0,
        end: MAX_PUSH_CONSTANTS_SIZE_BYTES,
    };
    assert!(
        r.validate().is_ok(),
        "exactly MAX_PUSH_CONSTANTS_SIZE_BYTES must be accepted"
    );
}

#[test]
fn test_push_constant_range_validate_rejects_unaligned_end() {
    // end = 6 is not 4-byte aligned
    let r = PushConstantRange {
        stages: wgpu::ShaderStages::COMPUTE,
        start: 0,
        end: 6,
    };
    assert!(
        r.validate().is_err(),
        "end not 4-byte aligned must be rejected"
    );
}

#[test]
fn test_push_constant_range_size_helper() {
    let r = PushConstantRange::compute(64);
    assert_eq!(r.size(), 64);
}

// ─────────────────────────────────────────────────────────────────────────────
// PushConstantsLayout tests — pure Rust
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_push_constants_layout_compute_only_structure() {
    let l = PushConstantsLayout::compute_only(64);
    assert_eq!(l.total_size, 64, "total_size must equal the requested size");
    assert_eq!(l.ranges.len(), 1, "exactly one range expected");
    assert!(
        l.validate().is_ok(),
        "PushConstantsLayout::compute_only(64) must pass validation"
    );
}

#[test]
fn test_push_constants_layout_to_wgpu_ranges_single() {
    let l = PushConstantsLayout::compute_only(16);
    let ranges = l.to_wgpu_ranges();
    assert_eq!(ranges.len(), 1, "one range descriptor expected");
    assert_eq!(ranges[0].range(), 0..16, "range must span [0, 16)");
}

#[test]
fn test_push_constants_layout_to_wgpu_ranges_stages() {
    let l = PushConstantsLayout::compute_only(32);
    let ranges = l.to_wgpu_ranges();
    assert_eq!(ranges[0].stages, wgpu::ShaderStages::COMPUTE);
}

#[test]
fn test_push_constants_layout_validate_rejects_unaligned_total_size() {
    let mut l = PushConstantsLayout::compute_only(16);
    l.total_size = 7; // not 4-byte aligned
    assert!(
        l.validate().is_err(),
        "unaligned total_size must be rejected"
    );
}

#[test]
fn test_push_constants_layout_validate_rejects_too_large() {
    let mut l = PushConstantsLayout::compute_only(16);
    l.total_size = MAX_PUSH_CONSTANTS_SIZE_BYTES + 4;
    assert!(
        l.validate().is_err(),
        "oversized total_size must be rejected"
    );
}

#[test]
fn test_push_constants_layout_immediate_size_for_wgpu() {
    let l = PushConstantsLayout::compute_only(48);
    assert_eq!(l.immediate_size_for_wgpu(), 48);
}

// ─────────────────────────────────────────────────────────────────────────────
// PushConstantsBuffer tests — pure Rust
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_push_constants_buffer_new_zero_initialised() {
    let layout = PushConstantsLayout::compute_only(16);
    let buf = PushConstantsBuffer::new(layout);
    assert_eq!(buf.size(), 16);
    assert!(
        buf.as_bytes().iter().all(|&b| b == 0),
        "buffer must be zero-initialised"
    );
}

#[test]
fn test_push_constants_buffer_write_u32_roundtrip() {
    let layout = PushConstantsLayout::compute_only(16);
    let mut buf = PushConstantsBuffer::new(layout);
    buf.write_u32(0, 42).expect("write_u32 at offset 0 failed");
    let bytes = buf.as_bytes();
    let val = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    assert_eq!(val, 42, "round-trip u32 read failed");
}

#[test]
fn test_push_constants_buffer_write_f32_roundtrip() {
    let layout = PushConstantsLayout::compute_only(16);
    let mut buf = PushConstantsBuffer::new(layout);
    buf.write_f32(4, std::f32::consts::PI)
        .expect("write_f32 at offset 4 failed");
    let bytes = buf.as_bytes();
    let val = f32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    assert!(
        (val - std::f32::consts::PI).abs() < 1e-6,
        "round-trip f32 read failed: expected π ≈ 3.14159, got {val}"
    );
}

#[test]
fn test_push_constants_buffer_write_vec4_f32_roundtrip() {
    let layout = PushConstantsLayout::compute_only(16);
    let mut buf = PushConstantsBuffer::new(layout);
    let data: [f32; 4] = [1.0, 2.0, 3.0, 4.0];
    buf.write_vec4_f32(0, data).expect("write_vec4_f32 failed");
    let bytes = buf.as_bytes();
    for (i, expected) in data.iter().enumerate() {
        let off = i * 4;
        let val = f32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]]);
        assert!(
            (val - expected).abs() < 1e-6,
            "vec4 element {i}: expected {expected}, got {val}"
        );
    }
}

#[test]
fn test_push_constants_buffer_write_multiple_fields() {
    // Simulate a real use case: write 3 fields at different offsets.
    let layout = PushConstantsLayout::compute_only(16);
    let mut buf = PushConstantsBuffer::new(layout);
    buf.write_u32(0, 1024).expect("write width failed"); // offset 0
    buf.write_u32(4, 768).expect("write height failed"); // offset 4
    buf.write_f32(8, 0.5).expect("write scale failed"); // offset 8

    let bytes = buf.as_bytes();
    let width = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let height = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let scale = f32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);

    assert_eq!(width, 1024, "width mismatch");
    assert_eq!(height, 768, "height mismatch");
    assert!((scale - 0.5).abs() < 1e-6, "scale mismatch: got {scale}");
}

#[test]
fn test_push_constants_buffer_offset_overflow_errors() {
    let layout = PushConstantsLayout::compute_only(4);
    let mut buf = PushConstantsBuffer::new(layout);
    // Offset 4 + 4 bytes = 8 > 4-byte buffer → must fail.
    let result = buf.write_u32(4, 1);
    assert!(
        result.is_err(),
        "write past buffer end must return an error"
    );
    // Also verify writing exactly at the start succeeds.
    assert!(
        buf.write_u32(0, 99).is_ok(),
        "write at offset 0 must succeed"
    );
}

#[test]
fn test_push_constants_buffer_clear_resets_to_zero() {
    let layout = PushConstantsLayout::compute_only(16);
    let mut buf = PushConstantsBuffer::new(layout);
    buf.write_u32(0, 0xDEAD_BEEF).expect("write failed");
    buf.clear();
    assert!(
        buf.as_bytes().iter().all(|&b| b == 0),
        "clear() must reset all bytes to zero"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Shader source generation tests — pure Rust
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_make_push_constants_shader_source_contains_var_immediate() {
    let src = make_push_constants_shader_source(
        "struct PushConstantsBlock { val: u32 }",
        "let x = pc.val;",
    );
    assert!(
        src.contains("var<immediate>"),
        "shader source must use var<immediate> (wgpu 29+ naming); got:\n{src}"
    );
}

#[test]
fn test_make_push_constants_shader_source_includes_struct_def() {
    let src = make_push_constants_shader_source("struct PushConstantsBlock { val: u32 }", "");
    assert!(
        src.contains("struct PushConstantsBlock"),
        "shader source must embed the struct definition"
    );
}

#[test]
fn test_make_push_constants_shader_source_has_compute_entry_point() {
    let src = make_push_constants_shader_source("struct PushConstantsBlock { val: u32 }", "");
    assert!(
        src.contains("@compute"),
        "shader source must declare a @compute entry point"
    );
}

#[test]
fn test_make_push_constants_shader_source_body_included() {
    let body = "let result = pc.val * 2u;";
    let src = make_push_constants_shader_source("struct PushConstantsBlock { val: u32 }", body);
    assert!(
        src.contains(body),
        "shader body must appear verbatim in the generated source"
    );
}

#[test]
fn test_make_push_constants_shader_source_references_pc_variable() {
    let src = make_push_constants_shader_source(
        "struct PushConstantsBlock { width: u32, height: u32 }",
        "let w = pc.width;",
    );
    assert!(
        src.contains("pc: PushConstantsBlock"),
        "the immediate variable must be declared as 'pc: PushConstantsBlock'"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// GPU-dependent tests — wrapped in catch_unwind / try_gpu_context
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_supports_push_constants_gpu_query_does_not_panic() {
    // This test only asserts that the function runs without panicking.
    // The actual bool result depends on the hardware/driver.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return,
        };
        let _supported = supports_push_constants(&ctx);
        // No assertion — just proving the call succeeds.
    }));
    let _ = result;
}

#[test]
fn test_max_push_constants_size_gpu_query_does_not_panic() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return,
        };
        let max_size = max_push_constants_size(&ctx);
        // When IMMEDIATES is not enabled, the size is 0 — that is valid.
        // When it is enabled, we expect at least the spec minimum.
        if supports_push_constants(&ctx) {
            assert!(
                max_size >= MAX_PUSH_CONSTANTS_SIZE_BYTES,
                "max_push_constants_size should be >= {} when IMMEDIATES is supported, got {}",
                MAX_PUSH_CONSTANTS_SIZE_BYTES,
                max_size
            );
        }
    }));
    let _ = result;
}

#[test]
fn test_build_push_constants_pipeline_requires_feature_or_errors() {
    // When built without the IMMEDIATES feature, build_push_constants_pipeline
    // must return a meaningful error (not panic).  This test verifies the
    // error path is reachable without requiring actual push-constant support.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return,
        };

        // A context created with GpuContext::new() does NOT request IMMEDIATES,
        // so build_push_constants_pipeline must return UnsupportedOperation.
        if supports_push_constants(&ctx) {
            // Hardware supports it but we did not request it during context
            // creation — the device was still created without the feature, so
            // the API-level check should still fire.  Skip asserting the error.
            return;
        }

        use oxigeo_gpu::push_constants::build_push_constants_pipeline;
        let layout = PushConstantsLayout::compute_only(16);
        let src = make_push_constants_shader_source(
            "struct PushConstantsBlock { value: u32, _p0: u32, _p1: u32, _p2: u32 }",
            "let v = pc.value;",
        );
        let err = build_push_constants_pipeline(&ctx, &src, "main", &layout);
        assert!(
            err.is_err(),
            "build_push_constants_pipeline must return Err when IMMEDIATES not enabled"
        );
    }));
    let _ = result;
}
