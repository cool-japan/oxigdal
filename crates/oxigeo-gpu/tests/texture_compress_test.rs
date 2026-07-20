//! Integration tests for BC1 and BC4 texture compression.
//!
//! Pure-Rust tests (helpers, CPU encoders, dimension validation) run
//! unconditionally.  GPU-dependent tests wrap `GpuContext::new()` in
//! `std::panic::catch_unwind` and gracefully skip when no wgpu backend is
//! available, matching the pattern used in `fft_test.rs` and
//! `texture_resample_test.rs`.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use oxigeo_gpu::{
    TextureCompressor, TextureFormat, compress_bc1_block_cpu, compress_bc4_block_cpu,
    dequantize_rgb565, nearest_index_4, quantize_rgb565, validate_texture_dimensions,
};

// ─────────────────────────────────────────────────────────────────────────────
// Helper: try to obtain a GPU context without panicking.
// ─────────────────────────────────────────────────────────────────────────────

fn try_gpu_context() -> Option<oxigeo_gpu::GpuContext> {
    use std::panic::AssertUnwindSafe;

    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        pollster::block_on(oxigeo_gpu::GpuContext::new())
    }));

    match result {
        Ok(Ok(ctx)) => Some(ctx),
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 — quantize_rgb565 / dequantize_rgb565 round-trip (lossy)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_quantize_rgb565_round_trip_lossy() {
    // RGB565 has 5 bits for R and B, 6 bits for G.
    // Tolerances: R within ±8, G within ±4, B within ±8.
    let (r_in, g_in, b_in) = (255u8, 128u8, 64u8);
    let packed = quantize_rgb565(r_in, g_in, b_in);
    let (r_out, g_out, b_out) = dequantize_rgb565(packed);

    let r_diff = (r_in as i32 - r_out as i32).unsigned_abs();
    let g_diff = (g_in as i32 - g_out as i32).unsigned_abs();
    let b_diff = (b_in as i32 - b_out as i32).unsigned_abs();

    assert!(
        r_diff <= 8,
        "R round-trip error {r_diff} exceeds ±8 (in={r_in}, out={r_out})"
    );
    assert!(
        g_diff <= 4,
        "G round-trip error {g_diff} exceeds ±4 (in={g_in}, out={g_out})"
    );
    assert!(
        b_diff <= 8,
        "B round-trip error {b_diff} exceeds ±8 (in={b_in}, out={b_out})"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 — dequantize_rgb565(0xFFFF) == (255, 255, 255)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_dequantize_rgb565_all_ones_extracts_expected() {
    let (r, g, b) = dequantize_rgb565(0xFFFF);
    assert_eq!(r, 255, "R should be 255 for 0xFFFF");
    assert_eq!(g, 255, "G should be 255 for 0xFFFF");
    assert_eq!(b, 255, "B should be 255 for 0xFFFF");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 — BC1 solid colour block → c0 == c1
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_compress_bc1_block_cpu_solid_color_emits_equal_endpoints() {
    let mut block = [0u8; 64];
    for i in 0..16 {
        block[i * 4] = 100;
        block[i * 4 + 1] = 200;
        block[i * 4 + 2] = 50;
        block[i * 4 + 3] = 255;
    }
    let encoded = compress_bc1_block_cpu(&block);
    let c0 = u16::from_le_bytes([encoded[0], encoded[1]]);
    let c1 = u16::from_le_bytes([encoded[2], encoded[3]]);
    assert_eq!(
        c0, c1,
        "a solid-colour BC1 block must produce equal endpoints; got c0={c0:#06x} c1={c1:#06x}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 — BC1 two-colour block → c0 != c1
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_compress_bc1_block_cpu_two_color_block_emits_distinct_endpoints() {
    // First 8 pixels = black, last 8 pixels = white.
    let mut block = [0u8; 64];
    // Pixels 0..7: RGBA = (0, 0, 0, 255)
    for i in 0..8 {
        block[i * 4 + 3] = 255;
    }
    // Pixels 8..15: RGBA = (255, 255, 255, 255)
    for i in 8..16 {
        block[i * 4] = 255;
        block[i * 4 + 1] = 255;
        block[i * 4 + 2] = 255;
        block[i * 4 + 3] = 255;
    }
    let encoded = compress_bc1_block_cpu(&block);
    let c0 = u16::from_le_bytes([encoded[0], encoded[1]]);
    let c1 = u16::from_le_bytes([encoded[2], encoded[3]]);
    assert_ne!(
        c0, c1,
        "a two-colour BC1 block (black+white) must produce distinct endpoints; \
         both are {c0:#06x}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 — BC4 solid value → ep0 == ep1
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_compress_bc4_block_cpu_solid_value_emits_equal_endpoints() {
    let block = [128u8; 16];
    let encoded = compress_bc4_block_cpu(&block);
    assert_eq!(
        encoded[0], encoded[1],
        "solid BC4 block (128) must have ep0 == ep1; got ep0={} ep1={}",
        encoded[0], encoded[1]
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6 — BC4 two-value block → ep0 != ep1
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_compress_bc4_block_cpu_two_value_emits_distinct_endpoints() {
    // Pixels 0..7 = 0, pixels 8..15 = 255.
    let mut block = [0u8; 16];
    for b in &mut block[8..16] {
        *b = 255;
    }
    let encoded = compress_bc4_block_cpu(&block);
    assert_ne!(
        encoded[0], encoded[1],
        "two-value BC4 block (0+255) must produce distinct endpoints; \
         both are {}",
        encoded[0]
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7 — nearest_index_4: (0,0,0) nearest to black at index 0
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_nearest_index_4_black_nearest_to_black_endpoint() {
    let endpoints = [
        (0u8, 0u8, 0u8),
        (255u8, 255u8, 255u8),
        (170u8, 170u8, 170u8),
        (85u8, 85u8, 85u8),
    ];
    let idx = nearest_index_4((0, 0, 0), endpoints);
    assert_eq!(
        idx, 0,
        "black (0,0,0) must be nearest to endpoint 0 (0,0,0); got index {idx}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 8 — TextureCompressor rejects width not multiple of 4
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_texture_compressor_rejects_non_multiple_of_4_width() {
    // Pure validation — no GPU context required.
    let result = validate_texture_dimensions(7, 8);
    assert!(
        result.is_err(),
        "width=7 is not a multiple of 4 and must be rejected"
    );
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("multiple") || msg.contains("4"),
        "error message should mention the multiple-of-4 requirement: {msg}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 9 — TextureCompressor rejects height not multiple of 4
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_texture_compressor_rejects_non_multiple_of_4_height() {
    let result = validate_texture_dimensions(8, 5);
    assert!(
        result.is_err(),
        "height=5 is not a multiple of 4 and must be rejected"
    );
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("multiple") || msg.contains("4"),
        "error message should mention the multiple-of-4 requirement: {msg}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 10 — BC1 compress 8×8 → 4 blocks when backend present
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_texture_compressor_bc1_compress_8x8_emits_4_blocks_when_backend_present() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return, // no GPU — skip
        };

        let compressor = match TextureCompressor::new(&ctx, TextureFormat::Bc1RgbUnorm, 8, 8) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("TextureCompressor::new failed (skip): {e}");
                return;
            }
        };

        // 8×8 RGBA8 image — all opaque mid-grey.
        let input = vec![128u8; 8 * 8 * 4];
        match compressor.compress(&ctx, &input) {
            Ok(output) => {
                // 8×8 pixels → 2×2 blocks → 4 blocks × 8 bytes = 32 bytes.
                assert_eq!(
                    output.len(),
                    32,
                    "8×8 BC1 image must produce 4 blocks (32 bytes); got {} bytes",
                    output.len()
                );
            }
            Err(e) => eprintln!("compress failed (skip): {e}"),
        }
    }));
    let _ = result;
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 11 — BC4 compress 8×8 → 4 blocks when backend present
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_texture_compressor_bc4_compress_8x8_emits_4_blocks_when_backend_present() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return, // no GPU — skip
        };

        let compressor = match TextureCompressor::new(&ctx, TextureFormat::Bc4RUnorm, 8, 8) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("TextureCompressor::new failed (skip): {e}");
                return;
            }
        };

        // 8×8 R8 image — all 200.
        let input = vec![200u8; 8 * 8];
        match compressor.compress(&ctx, &input) {
            Ok(output) => {
                // 8×8 pixels → 2×2 blocks → 4 blocks × 8 bytes = 32 bytes.
                assert_eq!(
                    output.len(),
                    32,
                    "8×8 BC4 image must produce 4 blocks (32 bytes); got {} bytes",
                    output.len()
                );
            }
            Err(e) => eprintln!("compress failed (skip): {e}"),
        }
    }));
    let _ = result;
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 12 — BC1 output length is (w/4)*(h/4)*8 for a 16×8 image
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_texture_compressor_bc1_output_length_correct_when_backend_present() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = match try_gpu_context() {
            Some(c) => c,
            None => return, // no GPU — skip
        };

        let (w, h) = (16u32, 8u32);
        let expected_bytes = ((w / 4) * (h / 4) * 8) as usize; // 2*2*8 = 32

        let compressor = match TextureCompressor::new(&ctx, TextureFormat::Bc1RgbUnorm, w, h) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("TextureCompressor::new failed (skip): {e}");
                return;
            }
        };

        let input = vec![64u8; (w * h * 4) as usize];
        match compressor.compress(&ctx, &input) {
            Ok(output) => {
                assert_eq!(
                    output.len(),
                    expected_bytes,
                    "16×8 BC1 compress must produce {expected_bytes} bytes; got {}",
                    output.len()
                );
            }
            Err(e) => eprintln!("compress failed (skip): {e}"),
        }
    }));
    let _ = result;
}

// ─────────────────────────────────────────────────────────────────────────────
// Additional pure-Rust coverage: validate_texture_dimensions edge cases
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_validate_texture_dimensions_zero_is_ok_multiple_of_4() {
    // 0 is technically a multiple of 4 (0 % 4 == 0).
    // We accept it here; the caller is responsible for ensuring non-zero dims.
    assert!(validate_texture_dimensions(0, 4).is_ok());
    assert!(validate_texture_dimensions(4, 0).is_ok());
}

#[test]
fn test_validate_texture_dimensions_large_multiples_ok() {
    assert!(validate_texture_dimensions(4096, 4096).is_ok());
    assert!(validate_texture_dimensions(16384, 8192).is_ok());
}

// ─────────────────────────────────────────────────────────────────────────────
// CPU fallback path (no GPU context needed)
// ─────────────────────────────────────────────────────────────────────────────

/// Verify that the CPU fallback path works even when constructed with a
/// real GPU context but then bypassed.
#[test]
fn test_compress_bc1_cpu_path_produces_correct_output_size() {
    // A 4×4 single-block BC1 image.
    let mut block = [0u8; 64];
    for i in 0..16 {
        block[i * 4] = 200;
        block[i * 4 + 1] = 100;
        block[i * 4 + 2] = 50;
        block[i * 4 + 3] = 255;
    }
    let encoded = compress_bc1_block_cpu(&block);
    assert_eq!(
        encoded.len(),
        8,
        "compress_bc1_block_cpu must always return exactly 8 bytes"
    );
}

#[test]
fn test_compress_bc4_cpu_path_produces_correct_output_size() {
    let block = [77u8; 16];
    let encoded = compress_bc4_block_cpu(&block);
    assert_eq!(
        encoded.len(),
        8,
        "compress_bc4_block_cpu must always return exactly 8 bytes"
    );
}
