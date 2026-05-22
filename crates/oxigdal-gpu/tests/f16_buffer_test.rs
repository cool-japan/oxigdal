//! CPU-only tests for f16 buffer support in oxigdal-gpu.
//!
//! None of these tests require an actual GPU device.  They verify:
//! - `BufferElementType` metadata correctness
//! - f16 ↔ f32 conversion round-trips
//! - WGSL shader source generation (enable directive, type tokens)

use half::f16;
use oxigdal_gpu::BufferElementType;

// ---------------------------------------------------------------------------
// BufferElementType metadata
// ---------------------------------------------------------------------------

#[test]
fn test_buffer_element_type_byte_sizes() {
    assert_eq!(BufferElementType::F32.byte_size(), 4);
    assert_eq!(BufferElementType::F16.byte_size(), 2);
    assert_eq!(BufferElementType::U8.byte_size(), 1);
    assert_eq!(BufferElementType::U16.byte_size(), 2);
    assert_eq!(BufferElementType::U32.byte_size(), 4);
    assert_eq!(BufferElementType::I32.byte_size(), 4);
}

#[test]
fn test_buffer_element_type_wgsl_type_strings() {
    assert_eq!(BufferElementType::F32.wgsl_type(), "f32");
    assert_eq!(BufferElementType::F16.wgsl_type(), "f16");
    assert_eq!(BufferElementType::U32.wgsl_type(), "u32");
    assert_eq!(BufferElementType::I32.wgsl_type(), "i32");
    // u8 and u16 degrade to u32 in WGSL
    assert_eq!(BufferElementType::U8.wgsl_type(), "u32");
    assert_eq!(BufferElementType::U16.wgsl_type(), "u32");
}

// ---------------------------------------------------------------------------
// f16 ↔ f32 round-trip conversions
// ---------------------------------------------------------------------------

#[test]
fn test_f16_round_trip_within_1bit_lsb_via_widening() {
    use oxigdal_gpu::{f16_to_f32_slice, f32_to_f16_slice};
    let original: Vec<f16> = vec![f16::from_f32(1.5), f16::from_f32(2.25), f16::from_f32(-0.5)];
    let widened = f16_to_f32_slice(&original);
    let narrowed = f32_to_f16_slice(&widened);
    for (a, b) in original.iter().zip(narrowed.iter()) {
        assert_eq!(
            a.to_bits(),
            b.to_bits(),
            "bits must be exactly equal after round-trip for value {:?}",
            a
        );
    }
}

#[test]
fn test_f16_fallback_widens_to_f32_conversion_is_correct() {
    use oxigdal_gpu::f16_to_f32_slice;
    // Use 1.1 — not exactly representable in f16 but well within range.
    // f16 has ~3.3 decimal digits of precision, so 1.1 should round-trip
    // with an absolute error less than 0.01.
    let input_val = 1.1_f32;
    let halfs = vec![f16::from_f32(input_val)];
    let f32s = f16_to_f32_slice(&halfs);
    assert!(
        (f32s[0] - input_val).abs() < 0.01,
        "widened value {} is too far from {}",
        f32s[0],
        input_val
    );
}

#[test]
fn test_f16_to_f32_to_f16_idempotent_for_representable_values() {
    use oxigdal_gpu::{f16_to_f32_slice, f32_to_f16_slice};
    // Values that are exactly representable in f16
    let representable = vec![0.0_f32, 1.0, 2.0, -4.0, 0.5, 0.25];
    let as_f16 = f32_to_f16_slice(&representable);
    let as_f32 = f16_to_f32_slice(&as_f16);
    let back = f32_to_f16_slice(&as_f32);
    for (a, b) in as_f16.iter().zip(back.iter()) {
        assert_eq!(
            a.to_bits(),
            b.to_bits(),
            "idempotent round-trip failed for {:?}",
            a
        );
    }
}

// ---------------------------------------------------------------------------
// WGSL shader source generation
// ---------------------------------------------------------------------------

#[test]
fn test_f16_kernel_source_contains_enable_directive_when_f16() {
    use oxigdal_gpu::make_element_wise_shader_source;
    let src = make_element_wise_shader_source(BufferElementType::F16);
    assert!(
        src.contains("enable f16"),
        "WGSL source must contain 'enable f16'; got:\n{src}"
    );
    assert!(
        src.contains("array<f16>"),
        "WGSL source must reference array<f16>; got:\n{src}"
    );
}

#[test]
fn test_f32_kernel_source_does_not_contain_enable_f16() {
    use oxigdal_gpu::make_element_wise_shader_source;
    let src = make_element_wise_shader_source(BufferElementType::F32);
    assert!(
        !src.contains("enable f16"),
        "F32 shader must not have 'enable f16'; got:\n{src}"
    );
    assert!(
        src.contains("array<f32>"),
        "F32 shader must reference array<f32>; got:\n{src}"
    );
}

#[test]
fn test_u32_kernel_source_does_not_contain_enable_f16() {
    use oxigdal_gpu::make_element_wise_shader_source;
    let src = make_element_wise_shader_source(BufferElementType::U32);
    assert!(
        !src.contains("enable f16"),
        "U32 shader must not have 'enable f16'; got:\n{src}"
    );
    assert!(src.contains("array<u32>"));
}
