//! Regression tests for GitHub issue #14 — reading a band straight into a
//! caller-owned typed buffer (`ndarray::Array2<f64>`, `Vec<f64>`, …) without an
//! extra full-size allocation.
//!
//! Covers the `oxigeo-core` buffer layer: the [`RasterElement`] trait, the bulk
//! conversion entry points, and the rewritten `convert_to` / `from_typed_vec`
//! fast paths.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use oxigeo_core::buffer::{
    FloatToIntRounding, RasterBuffer, RasterElement, RasterElementKind, convert_raw_bytes,
    convert_raw_into, convert_raw_into_with, elements_as_bytes,
};
use oxigeo_core::types::{NoDataValue, RasterDataType};

/// Every scalar raster data type, in declaration order.
const SCALAR_TYPES: [RasterDataType; 10] = [
    RasterDataType::UInt8,
    RasterDataType::Int8,
    RasterDataType::UInt16,
    RasterDataType::Int16,
    RasterDataType::UInt32,
    RasterDataType::Int32,
    RasterDataType::UInt64,
    RasterDataType::Int64,
    RasterDataType::Float32,
    RasterDataType::Float64,
];

/// Encodes a logical value into one native-endian sample of `data_type`.
///
/// Written independently of the production code: it deliberately mirrors the
/// legacy `set_pixel` behaviour (truncating `as` casts) so the expectations
/// below are not derived from the code under test.
fn encode_sample(data_type: RasterDataType, value: f64) -> Vec<u8> {
    match data_type {
        RasterDataType::UInt8 => vec![value as u8],
        RasterDataType::Int8 => vec![(value as i8) as u8],
        RasterDataType::UInt16 => (value as u16).to_ne_bytes().to_vec(),
        RasterDataType::Int16 => (value as i16).to_ne_bytes().to_vec(),
        RasterDataType::UInt32 => (value as u32).to_ne_bytes().to_vec(),
        RasterDataType::Int32 => (value as i32).to_ne_bytes().to_vec(),
        RasterDataType::UInt64 => (value as u64).to_ne_bytes().to_vec(),
        RasterDataType::Int64 => (value as i64).to_ne_bytes().to_vec(),
        RasterDataType::Float32 => (value as f32).to_ne_bytes().to_vec(),
        RasterDataType::Float64 => value.to_ne_bytes().to_vec(),
        RasterDataType::CFloat32 => {
            let mut out = (value as f32).to_ne_bytes().to_vec();
            out.extend_from_slice(&0f32.to_ne_bytes());
            out
        }
        RasterDataType::CFloat64 => {
            let mut out = value.to_ne_bytes().to_vec();
            out.extend_from_slice(&0f64.to_ne_bytes());
            out
        }
    }
}

/// Reference reimplementation of the *legacy* `convert_to` inner loop
/// (`get_pixel` → `set_pixel`), used to prove the rewritten bulk path produces
/// identical results.
fn legacy_convert(buffer: &RasterBuffer, target: RasterDataType) -> RasterBuffer {
    let mut out = RasterBuffer::nodata_filled(
        buffer.width(),
        buffer.height(),
        target,
        NoDataValue::None, // filled below; nodata is copied verbatim by convert_to
    );
    // `nodata_filled` with `None` behaves exactly like `zeros`.
    for y in 0..buffer.height() {
        for x in 0..buffer.width() {
            let value = buffer.get_pixel(x, y).expect("get_pixel");
            out.set_pixel(x, y, value).expect("set_pixel");
        }
    }
    out
}

/// Representative samples per source type: bounds, negatives, zero, fractions.
fn samples_for(data_type: RasterDataType) -> Vec<f64> {
    match data_type {
        RasterDataType::UInt8 => vec![0.0, 1.0, 127.0, 200.0, 255.0],
        RasterDataType::Int8 => vec![-128.0, -1.0, 0.0, 1.0, 127.0],
        RasterDataType::UInt16 => vec![0.0, 1.0, 255.0, 40_000.0, 65_535.0],
        RasterDataType::Int16 => vec![-32_768.0, -1.0, 0.0, 300.0, 32_767.0],
        RasterDataType::UInt32 => vec![0.0, 1.0, 65_535.0, 3_000_000_000.0, 4_294_967_295.0],
        RasterDataType::Int32 => vec![-2_147_483_648.0, -1.0, 0.0, 70_000.0, 2_147_483_647.0],
        // Kept below 2^53 so the legacy f64 bridge is exact and the comparison
        // is meaningful; the >2^53 behaviour is asserted separately.
        RasterDataType::UInt64 => vec![0.0, 1.0, 65_535.0, 9_007_199_254_740_992.0],
        RasterDataType::Int64 => vec![-9_007_199_254_740_992.0, -1.0, 0.0, 9_007_199_254_740_992.0],
        RasterDataType::Float32 => vec![-1e30, -2.5, -0.5, 0.0, 0.5, 2.5, 1e30],
        RasterDataType::Float64 => vec![-1e300, -2.5, -0.5, 0.0, 0.5, 2.5, 1e300],
        RasterDataType::CFloat32 => vec![-2.5, 0.0, 2.5],
        RasterDataType::CFloat64 => vec![-2.5, 0.0, 2.5],
    }
}

fn buffer_from_samples(data_type: RasterDataType, samples: &[f64]) -> RasterBuffer {
    let mut data = Vec::new();
    for value in samples {
        data.extend_from_slice(&encode_sample(data_type, *value));
    }
    RasterBuffer::new(data, samples.len() as u64, 1, data_type, NoDataValue::None).expect("buffer")
}

// ─── T2: bulk conversion into a caller-owned slice ───────────────────────────

#[test]
fn test_issue_14_read_band_into_caller_owned_f64_slice() {
    // The exact shape the issue asks for: driver bytes -> user's Array2<f64>
    // backing slice, no intermediate Vec, no `.mapv(|v| v as f64)` pass.
    let raw: Vec<u8> = [10u16, 20, 30, 40, 50, 60]
        .iter()
        .flat_map(|v| v.to_ne_bytes())
        .collect();

    let mut destination = vec![f64::NAN; 6];
    convert_raw_into(&raw, RasterDataType::UInt16, &mut destination).expect("convert");

    assert_eq!(destination, vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0]);
}

#[test]
fn test_issue_14_copy_to_slice_matches_get_pixel() {
    let mut buffer = RasterBuffer::zeros(7, 5, RasterDataType::Int16);
    for y in 0..5 {
        for x in 0..7 {
            buffer
                .set_pixel(x, y, (x as f64) * 13.0 - (y as f64) * 7.0)
                .expect("set_pixel");
        }
    }

    let mut destination = vec![0.0f64; buffer.pixel_count() as usize];
    buffer.copy_to_slice(&mut destination).expect("copy");

    for y in 0..5 {
        for x in 0..7 {
            let expected = buffer.get_pixel(x, y).expect("get_pixel");
            assert_eq!(destination[(y * 7 + x) as usize], expected, "pixel {x},{y}");
        }
    }
}

#[test]
fn test_issue_14_copy_to_slice_memcpy_fast_path() {
    // Same-type conversion must be a byte-exact memcpy, including NaN payloads
    // and signed zeros that a value-level round trip could normalise.
    let values = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.0, 1.25];
    let buffer = RasterBuffer::from_element_slice(values.len() as u64, 1, &values).expect("buffer");

    let mut destination = vec![0.0f32; values.len()];
    buffer.copy_to_slice(&mut destination).expect("copy");

    assert!(destination[0].is_nan());
    assert_eq!(destination[1], f32::INFINITY);
    assert_eq!(destination[2], f32::NEG_INFINITY);
    assert_eq!(destination[3].to_bits(), (-0.0f32).to_bits());
    assert_eq!(destination[4], 1.25);
    assert_eq!(elements_as_bytes(&destination), buffer.as_bytes());
}

#[test]
fn test_issue_14_length_mismatch_is_a_typed_error_not_a_panic() {
    let buffer = RasterBuffer::zeros(4, 3, RasterDataType::UInt8);

    let mut too_small = vec![0.0f64; 11];
    let err = buffer.copy_to_slice(&mut too_small).expect_err("must fail");
    assert!(
        format!("{err:?}").contains("Destination length mismatch"),
        "unexpected error: {err:?}"
    );

    let mut too_large = vec![0.0f64; 13];
    assert!(buffer.copy_to_slice(&mut too_large).is_err());

    let mut exact = vec![0.0f64; 12];
    assert!(buffer.copy_to_slice(&mut exact).is_ok());

    // Ragged source: 5 bytes cannot be a whole number of UInt16 samples.
    let mut dst = vec![0.0f64; 2];
    assert!(convert_raw_into(&[0u8; 5], RasterDataType::UInt16, &mut dst).is_err());
}

#[test]
fn test_issue_14_unaligned_source_bytes() {
    // Bytes arriving from a decoded tile are not aligned for f64/u32/…; the
    // bulk converter must read them without an unaligned dereference.
    let mut raw = vec![0xAAu8; 3];
    for value in [1.5f64, -2.5, 3.75, 4.0] {
        raw.extend_from_slice(&value.to_ne_bytes());
    }
    let unaligned = &raw[3..];
    assert_ne!(unaligned.as_ptr() as usize % 8, 0);

    let mut destination = vec![0.0f32; 4];
    convert_raw_into(unaligned, RasterDataType::Float64, &mut destination).expect("convert");
    assert_eq!(destination, vec![1.5f32, -2.5, 3.75, 4.0]);

    // Same-type (memcpy) path on an unaligned source too.
    let mut same = vec![0.0f64; 4];
    convert_raw_into(unaligned, RasterDataType::Float64, &mut same).expect("convert");
    assert_eq!(same, vec![1.5f64, -2.5, 3.75, 4.0]);
}

#[test]
fn test_issue_14_to_typed_vec_and_from_element_slice_round_trip() {
    let original: Vec<i32> = (-5..5).collect();
    let buffer = RasterBuffer::from_element_slice(5, 2, &original).expect("from_element_slice");
    assert_eq!(buffer.data_type(), RasterDataType::Int32);
    assert_eq!(buffer.width(), 5);
    assert_eq!(buffer.height(), 2);

    assert_eq!(buffer.to_typed_vec::<i32>().expect("typed vec"), original);

    let widened = buffer.to_typed_vec::<f64>().expect("typed vec");
    assert_eq!(
        widened,
        original.iter().map(|v| f64::from(*v)).collect::<Vec<_>>()
    );

    // Dimension mismatch is rejected.
    assert!(RasterBuffer::from_element_slice(4, 2, &original).is_err());
}

#[test]
fn test_issue_14_empty_buffer_is_not_an_error() {
    let buffer = RasterBuffer::zeros(0, 0, RasterDataType::Float64);
    assert!(buffer.to_typed_vec::<f64>().expect("typed vec").is_empty());

    let mut empty: [f64; 0] = [];
    assert!(buffer.copy_to_slice(&mut empty).is_ok());
}

// ─── T1: conversion semantics ────────────────────────────────────────────────

#[test]
fn test_issue_14_float_to_int_saturates_and_rounds_to_nearest() {
    let raw: Vec<u8> = [2.5f32, -2.5, 2.4, -2.4, 1e30, -1e30, f32::NAN]
        .iter()
        .flat_map(|v| v.to_ne_bytes())
        .collect();

    let mut nearest = vec![0i16; 7];
    convert_raw_into(&raw, RasterDataType::Float32, &mut nearest).expect("convert");
    assert_eq!(
        nearest,
        vec![3, -3, 2, -2, i16::MAX, i16::MIN, 0],
        "halves must go away from zero, overflow must saturate, NaN must be 0"
    );

    let mut truncating = vec![0i16; 7];
    convert_raw_into_with(
        &raw,
        RasterDataType::Float32,
        &mut truncating,
        FloatToIntRounding::Truncate,
    )
    .expect("convert");
    assert_eq!(truncating, vec![2, -2, 2, -2, i16::MAX, i16::MIN, 0]);
}

#[test]
fn test_issue_14_float_infinities_and_nan_per_destination() {
    let raw: Vec<u8> = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY]
        .iter()
        .flat_map(|v| v.to_ne_bytes())
        .collect();

    let mut unsigned = vec![0u8; 3];
    convert_raw_into(&raw, RasterDataType::Float64, &mut unsigned).expect("convert");
    assert_eq!(unsigned, vec![0, u8::MAX, u8::MIN]);

    let mut signed = vec![0i64; 3];
    convert_raw_into(&raw, RasterDataType::Float64, &mut signed).expect("convert");
    assert_eq!(signed, vec![0, i64::MAX, i64::MIN]);

    // Float destinations keep the special values instead of clamping them.
    let mut floats = vec![0.0f32; 3];
    convert_raw_into(&raw, RasterDataType::Float64, &mut floats).expect("convert");
    assert!(floats[0].is_nan());
    assert_eq!(floats[1], f32::INFINITY);
    assert_eq!(floats[2], f32::NEG_INFINITY);
}

#[test]
fn test_issue_14_int_to_int_saturates_at_bounds() {
    let raw: Vec<u8> = [-40_000i32, -1, 0, 300, 40_000, 2_147_483_647]
        .iter()
        .flat_map(|v| v.to_ne_bytes())
        .collect();

    let mut to_u8 = vec![0u8; 6];
    convert_raw_into(&raw, RasterDataType::Int32, &mut to_u8).expect("convert");
    assert_eq!(to_u8, vec![0, 0, 0, 255, 255, 255]);

    let mut to_i16 = vec![0i16; 6];
    convert_raw_into(&raw, RasterDataType::Int32, &mut to_i16).expect("convert");
    assert_eq!(
        to_i16,
        vec![i16::MIN, -1, 0, 300, i16::MAX, i16::MAX],
        "no wrapping is allowed"
    );

    let mut to_i8 = vec![0i8; 6];
    convert_raw_into(&raw, RasterDataType::Int32, &mut to_i8).expect("convert");
    assert_eq!(to_i8, vec![i8::MIN, -1, 0, i8::MAX, i8::MAX, i8::MAX]);
}

#[test]
fn test_issue_14_int_to_int_is_exact_beyond_2_pow_53() {
    // The whole point of the i128 bridge: these values are NOT representable in
    // f64, so the historical get_pixel/set_pixel round trip corrupted them.
    let values = [
        u64::MAX,
        u64::MAX - 1,
        (1u64 << 53) + 1,
        9_223_372_036_854_775_807,
    ];
    let raw: Vec<u8> = values.iter().flat_map(|v| v.to_ne_bytes()).collect();

    let mut same = vec![0u64; values.len()];
    convert_raw_into(&raw, RasterDataType::UInt64, &mut same).expect("convert");
    assert_eq!(same, values.to_vec());

    let mut signed = vec![0i64; values.len()];
    convert_raw_into(&raw, RasterDataType::UInt64, &mut signed).expect("convert");
    assert_eq!(
        signed,
        vec![i64::MAX, i64::MAX, (1i64 << 53) + 1, i64::MAX],
        "u64 -> i64 must saturate exactly, not drift through f64"
    );

    // The lossy f64 bridge is only used when the destination is a float, where
    // the loss is inherent to the destination type.
    let mut floats = vec![0.0f64; values.len()];
    convert_raw_into(&raw, RasterDataType::UInt64, &mut floats).expect("convert");
    assert_eq!(floats[2], 9_007_199_254_740_992.0);
}

#[test]
fn test_issue_14_negative_to_unsigned_clamps_to_zero() {
    let raw: Vec<u8> = [-1i8, -128, 0, 127]
        .iter()
        .flat_map(|v| v.to_ne_bytes())
        .collect();

    for_all_unsigned(&raw);

    fn for_all_unsigned(raw: &[u8]) {
        let mut u8s = vec![9u8; 4];
        convert_raw_into(raw, RasterDataType::Int8, &mut u8s).expect("convert");
        assert_eq!(u8s, vec![0, 0, 0, 127]);

        let mut u16s = vec![9u16; 4];
        convert_raw_into(raw, RasterDataType::Int8, &mut u16s).expect("convert");
        assert_eq!(u16s, vec![0, 0, 0, 127]);

        let mut u32s = vec![9u32; 4];
        convert_raw_into(raw, RasterDataType::Int8, &mut u32s).expect("convert");
        assert_eq!(u32s, vec![0, 0, 0, 127]);

        let mut u64s = vec![9u64; 4];
        convert_raw_into(raw, RasterDataType::Int8, &mut u64s).expect("convert");
        assert_eq!(u64s, vec![0, 0, 0, 127]);
    }
}

#[test]
fn test_issue_14_element_trait_metadata() {
    assert_eq!(u8::DATA_TYPE, RasterDataType::UInt8);
    assert_eq!(i8::DATA_TYPE, RasterDataType::Int8);
    assert_eq!(u16::DATA_TYPE, RasterDataType::UInt16);
    assert_eq!(i16::DATA_TYPE, RasterDataType::Int16);
    assert_eq!(u32::DATA_TYPE, RasterDataType::UInt32);
    assert_eq!(i32::DATA_TYPE, RasterDataType::Int32);
    assert_eq!(u64::DATA_TYPE, RasterDataType::UInt64);
    assert_eq!(i64::DATA_TYPE, RasterDataType::Int64);
    assert_eq!(f32::DATA_TYPE, RasterDataType::Float32);
    assert_eq!(f64::DATA_TYPE, RasterDataType::Float64);

    assert_eq!(u8::KIND, RasterElementKind::Integer);
    assert_eq!(i64::KIND, RasterElementKind::Integer);
    assert_eq!(f32::KIND, RasterElementKind::Float);
    assert_eq!(f64::KIND, RasterElementKind::Float);

    for data_type in SCALAR_TYPES {
        // Every scalar type is reachable through the trait with a matching size.
        let size = data_type.size_bytes();
        assert!(size == 1 || size == 2 || size == 4 || size == 8);
    }

    assert_eq!(f64::SIZE, 8);
    assert_eq!(u16::SIZE, 2);
    assert_eq!(f64::from_ne_bytes(1.5f64.to_ne_bytes()), 1.5);
    assert_eq!(u16::to_ne_bytes(513), 513u16.to_ne_bytes());
    assert_eq!(i32::from_raster_f64(-2.5), -3);
    assert_eq!(i32::from_raster_f64_truncating(-2.5), -2);
    assert_eq!(i32::from_raster_i128(i128::MAX), i32::MAX);
    assert_eq!(u8::to_raster_i128(7), 7);
    assert_eq!(f32::to_raster_f64(0.5), 0.5);
}

// ─── T2: exhaustive 10 × 10 conversion matrix ────────────────────────────────

/// Independent expectation for a single sample, derived from the documented
/// semantics with concrete Rust casts (not from the production dispatch).
fn expected_value(src: RasterDataType, dst: RasterDataType, sample: f64) -> f64 {
    // Decode what the source type actually stores for this sample.
    let stored = decode_sample(src, sample);
    match dst {
        RasterDataType::UInt8 => f64::from(saturating_u8(src, stored)),
        RasterDataType::Int8 => f64::from(saturating_i8(src, stored)),
        RasterDataType::UInt16 => f64::from(saturating_u16(src, stored)),
        RasterDataType::Int16 => f64::from(saturating_i16(src, stored)),
        RasterDataType::UInt32 => f64::from(saturating_u32(src, stored)),
        RasterDataType::Int32 => f64::from(saturating_i32(src, stored)),
        RasterDataType::UInt64 => saturating_u64(src, stored) as f64,
        RasterDataType::Int64 => saturating_i64(src, stored) as f64,
        RasterDataType::Float32 | RasterDataType::CFloat32 => f64::from(stored as f32),
        RasterDataType::Float64 | RasterDataType::CFloat64 => stored,
    }
}

/// Value a source type actually holds after encoding `sample`.
fn decode_sample(src: RasterDataType, sample: f64) -> f64 {
    match src {
        RasterDataType::Float32 | RasterDataType::CFloat32 => f64::from(sample as f32),
        _ => sample,
    }
}

macro_rules! saturating_fn {
    ($name:ident, $ty:ty) => {
        fn $name(src: RasterDataType, value: f64) -> $ty {
            if matches!(
                src,
                RasterDataType::Float32
                    | RasterDataType::Float64
                    | RasterDataType::CFloat32
                    | RasterDataType::CFloat64
            ) {
                // float -> int: round half away from zero, then saturate.
                let rounded = if value.is_sign_negative() {
                    -((-value).round())
                } else {
                    value.round()
                };
                rounded as $ty
            } else {
                // int -> int: exact saturation via i128.
                (value as i128).clamp(<$ty>::MIN as i128, <$ty>::MAX as i128) as $ty
            }
        }
    };
}

saturating_fn!(saturating_u8, u8);
saturating_fn!(saturating_i8, i8);
saturating_fn!(saturating_u16, u16);
saturating_fn!(saturating_i16, i16);
saturating_fn!(saturating_u32, u32);
saturating_fn!(saturating_i32, i32);
saturating_fn!(saturating_u64, u64);
saturating_fn!(saturating_i64, i64);

/// Reads one converted sample back as f64 through the (already independently
/// tested) `get_pixel` accessor.
fn converted_values(buffer: &RasterBuffer) -> Vec<f64> {
    (0..buffer.width())
        .map(|x| buffer.get_pixel(x, 0).expect("get_pixel"))
        .collect()
}

#[test]
fn test_issue_14_conversion_matrix_all_scalar_pairs() {
    for src in SCALAR_TYPES {
        let samples = samples_for(src);
        let buffer = buffer_from_samples(src, &samples);

        for dst in SCALAR_TYPES {
            // Typed destination path (`convert_raw_into`), checked through a
            // dtype-specific helper so every one of the 100 pairs runs.
            let actual = typed_convert_to_f64(buffer.as_bytes(), src, dst, samples.len());
            for (index, sample) in samples.iter().enumerate() {
                let expected = expected_value(src, dst, *sample);
                assert_eq!(
                    actual[index], expected,
                    "typed {src:?} -> {dst:?} sample {sample} (index {index})"
                );
            }

            // Byte destination path (`convert_raw_bytes`, used by convert_to).
            let mut bytes = vec![0u8; samples.len() * dst.size_bytes()];
            convert_raw_bytes(
                buffer.as_bytes(),
                src,
                &mut bytes,
                dst,
                FloatToIntRounding::Nearest,
            )
            .expect("convert_raw_bytes");
            let round_tripped =
                RasterBuffer::new(bytes, samples.len() as u64, 1, dst, NoDataValue::None)
                    .expect("buffer");
            for (index, sample) in samples.iter().enumerate() {
                let expected = expected_value(src, dst, *sample);
                assert_eq!(
                    converted_values(&round_tripped)[index],
                    expected,
                    "bytes {src:?} -> {dst:?} sample {sample} (index {index})"
                );
            }
        }
    }
}

/// Runs `convert_raw_into::<T>` for the `T` matching `dst` and widens the result
/// back to `f64` for comparison.
fn typed_convert_to_f64(
    raw: &[u8],
    src: RasterDataType,
    dst: RasterDataType,
    count: usize,
) -> Vec<f64> {
    macro_rules! run {
        ($ty:ty) => {{
            let mut out = vec![<$ty>::default(); count];
            convert_raw_into(raw, src, &mut out).expect("convert_raw_into");
            out.into_iter().map(|v| v as f64).collect()
        }};
    }
    match dst {
        RasterDataType::UInt8 => run!(u8),
        RasterDataType::Int8 => run!(i8),
        RasterDataType::UInt16 => run!(u16),
        RasterDataType::Int16 => run!(i16),
        RasterDataType::UInt32 => run!(u32),
        RasterDataType::Int32 => run!(i32),
        RasterDataType::UInt64 => run!(u64),
        RasterDataType::Int64 => run!(i64),
        RasterDataType::Float32 => run!(f32),
        RasterDataType::Float64 => run!(f64),
        RasterDataType::CFloat32 | RasterDataType::CFloat64 => {
            panic!("complex destinations are only reachable through convert_raw_bytes")
        }
    }
}

// ─── T3: convert_to / from_typed_vec keep their observable behaviour ─────────

#[test]
fn test_issue_14_convert_to_matches_legacy_per_pixel_loop() {
    let mut all_types = SCALAR_TYPES.to_vec();
    all_types.push(RasterDataType::CFloat32);
    all_types.push(RasterDataType::CFloat64);

    for src in all_types.iter().copied() {
        let samples = samples_for(src);
        let buffer = buffer_from_samples(src, &samples);

        for dst in all_types.iter().copied() {
            let converted = buffer.convert_to(dst).expect("convert_to");
            let legacy = legacy_convert(&buffer, dst);

            assert_eq!(
                converted.as_bytes(),
                legacy.as_bytes(),
                "convert_to {src:?} -> {dst:?} diverged from the legacy loop"
            );
            assert_eq!(converted.width(), buffer.width());
            assert_eq!(converted.height(), buffer.height());
            assert_eq!(converted.data_type(), dst);
        }
    }
}

#[test]
fn test_issue_14_convert_to_preserves_nodata_verbatim() {
    let mut buffer =
        RasterBuffer::nodata_filled(3, 2, RasterDataType::Float32, NoDataValue::Float(-9999.0));
    buffer.set_pixel(0, 0, 12.5).expect("set_pixel");

    let converted = buffer
        .convert_to(RasterDataType::Int16)
        .expect("convert_to");
    // The nodata value is carried over unchanged (not re-encoded), as before.
    assert_eq!(converted.nodata(), NoDataValue::Float(-9999.0));
    // And truncation (not rounding) is preserved for convert_to.
    assert_eq!(converted.get_pixel(0, 0).expect("get_pixel"), 12.0);
    assert_eq!(converted.get_pixel(1, 0).expect("get_pixel"), -9999.0);
}

#[test]
fn test_issue_14_convert_to_same_type_is_a_clone() {
    let buffer =
        RasterBuffer::nodata_filled(2, 2, RasterDataType::Float64, NoDataValue::Float(f64::NAN));
    let converted = buffer
        .convert_to(RasterDataType::Float64)
        .expect("convert_to");
    assert_eq!(converted.as_bytes(), buffer.as_bytes());
    assert!(converted.nodata().as_f64().expect("nodata").is_nan());
}

#[test]
fn test_issue_14_convert_to_is_exact_for_large_integers() {
    // Documented improvement over the legacy f64 bridge: no precision loss.
    let values = [(1u64 << 53) + 1, u64::MAX - 1];
    let buffer = RasterBuffer::from_element_slice(2, 1, &values).expect("buffer");

    let converted = buffer
        .convert_to(RasterDataType::Int64)
        .expect("convert_to");
    assert_eq!(
        converted.to_typed_vec::<i64>().expect("typed vec"),
        vec![(1i64 << 53) + 1, i64::MAX]
    );

    // The legacy loop would have rounded through f64 and produced 2^53 here.
    let legacy = legacy_convert(&buffer, RasterDataType::Int64);
    assert_eq!(
        legacy.to_typed_vec::<i64>().expect("typed vec")[0],
        1i64 << 53,
        "sanity: the legacy path really was lossy for this value"
    );
}

#[test]
fn test_issue_14_from_typed_vec_bulk_copy_is_byte_identical() {
    let values: Vec<f32> = (0..64).map(|v| v as f32 * 0.5).collect();
    let bulk = RasterBuffer::from_typed_vec(8, 8, values.clone(), RasterDataType::Float32)
        .expect("from_typed_vec");

    let expected: Vec<u8> = values.iter().flat_map(|v| v.to_ne_bytes()).collect();
    assert_eq!(bulk.as_bytes(), expected.as_slice());

    // Still validates its arguments.
    assert!(RasterBuffer::from_typed_vec(8, 9, values.clone(), RasterDataType::Float32).is_err());
    assert!(RasterBuffer::from_typed_vec(8, 8, values, RasterDataType::Float64).is_err());
}

// ─── T5: as_slice alignment ──────────────────────────────────────────────────

#[test]
fn test_issue_14_as_slice_zero_sized_buffer_is_safe() {
    // Previously `from_raw_parts` was called with the `Vec<u8>` dangling pointer
    // (address 1), which is undefined behaviour even for a zero-length slice.
    let buffer = RasterBuffer::zeros(0, 0, RasterDataType::Float64);
    assert!(buffer.as_slice::<f64>().expect("as_slice").is_empty());

    let mut buffer = RasterBuffer::zeros(0, 4, RasterDataType::Float64);
    assert!(
        buffer
            .as_slice_mut::<f64>()
            .expect("as_slice_mut")
            .is_empty()
    );
    assert!(buffer.row_slice::<f64>(0).expect("row_slice").is_empty());
}

/// `as_slice` must either hand back a correct zero-copy view or refuse with the
/// documented alignment error — never undefined behaviour.
///
/// A `Vec<u8>` is only guaranteed to be 1-byte aligned. Every production
/// allocator (glibc/macOS `malloc`, jemalloc, mimalloc) over-aligns to 8/16
/// bytes so the zero-copy path succeeds, but Miri's allocator honours the
/// requested alignment exactly and does hand back odd addresses — which is
/// precisely the case the check exists for. Both outcomes are accepted here.
fn assert_slice_or_alignment_error<T: Copy + 'static>(
    result: oxigeo_core::error::Result<&[T]>,
    expected_len: usize,
) {
    match result {
        Ok(slice) => assert_eq!(slice.len(), expected_len),
        Err(err) => {
            let text = format!("{err:?}");
            assert!(text.contains("misaligned"), "unexpected error: {text}");
            assert!(
                text.contains("copy_to_slice"),
                "the error must point at the alignment-free alternative: {text}"
            );
        }
    }
}

#[test]
fn test_issue_14_as_slice_still_works_for_every_scalar_type() {
    macro_rules! check {
        ($ty:ty, $variant:ident) => {{
            let buffer = RasterBuffer::zeros(16, 4, RasterDataType::$variant);
            assert_slice_or_alignment_error(buffer.as_slice::<$ty>(), 64);
            assert_slice_or_alignment_error(buffer.row_slice::<$ty>(3), 16);
            // The copying path always works, whatever the storage alignment.
            assert_eq!(buffer.to_typed_vec::<$ty>().expect("typed vec").len(), 64);
        }};
    }
    check!(u8, UInt8);
    check!(i8, Int8);
    check!(u16, UInt16);
    check!(i16, Int16);
    check!(u32, UInt32);
    check!(i32, Int32);
    check!(u64, UInt64);
    check!(i64, Int64);
    check!(f32, Float32);
    check!(f64, Float64);
}

#[test]
fn test_issue_14_as_slice_type_size_mismatch_still_errors() {
    let buffer = RasterBuffer::zeros(4, 4, RasterDataType::UInt8);
    assert!(buffer.as_slice::<f64>().is_err());
    assert!(buffer.row_slice::<u32>(0).is_err());
    assert!(buffer.row_slice::<u8>(4).is_err());
}

#[test]
fn test_issue_14_copy_to_slice_is_the_alignment_free_alternative() {
    // Whatever the storage alignment, the typed copy always works — this is the
    // documented fallback when `as_slice` reports a misaligned buffer.
    let buffer = RasterBuffer::from_element_slice(4, 1, &[1.0f64, 2.0, 3.0, 4.0]).expect("buffer");
    let mut destination = vec![0.0f64; 4];
    buffer.copy_to_slice(&mut destination).expect("copy");
    assert_eq!(destination, vec![1.0, 2.0, 3.0, 4.0]);
    assert_eq!(
        buffer.to_typed_vec::<f64>().expect("typed vec"),
        destination
    );

    // The zero-copy view agrees whenever the allocator aligned the storage.
    if let Ok(view) = buffer.as_slice::<f64>() {
        assert_eq!(view.to_vec(), destination);
    }
}

// ─── Complex source handling ─────────────────────────────────────────────────

#[test]
fn test_issue_14_complex_sources_use_the_real_component() {
    let mut raw = Vec::new();
    for (real, imaginary) in [(1.5f64, 100.0f64), (-2.5, -100.0)] {
        raw.extend_from_slice(&real.to_ne_bytes());
        raw.extend_from_slice(&imaginary.to_ne_bytes());
    }

    let mut destination = vec![0.0f64; 2];
    convert_raw_into(&raw, RasterDataType::CFloat64, &mut destination).expect("convert");
    assert_eq!(destination, vec![1.5, -2.5]);

    let buffer =
        RasterBuffer::new(raw, 2, 1, RasterDataType::CFloat64, NoDataValue::None).expect("buffer");
    assert_eq!(
        buffer.to_typed_vec::<f64>().expect("typed vec"),
        vec![1.5, -2.5]
    );
    assert_eq!(
        buffer.to_typed_vec::<i32>().expect("typed vec"),
        vec![2, -3],
        "complex -> int rounds half away from zero like any float source"
    );
}
