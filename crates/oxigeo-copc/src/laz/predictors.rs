//! LASzip Item Compressor v1 predictors.
//!
//! The Item Compressor v1 family decomposes a LAS point record into typed
//! fields (XYZ, intensity, classification, …) and codes each one with a
//! field-specific predictor.  Most predictors are simple: keep the previous
//! decoded value and let the [`crate::laz::arithmetic::IntegerCompressor`]
//! encode the corrector.  XYZ uses a 2-step linear extrapolation: the next
//! value is predicted as `last + last_delta`.
//!
//! All predictors here are 1:1 with the reference C++ implementation in
//! `LASzip/src/lasreaditemcompressed_v1.{hpp,cpp}` and their encoder twins.

#[cfg(any(test, feature = "laz-encoder"))]
use crate::laz::arithmetic::ArithmeticEncoder;
use crate::laz::arithmetic::{ArithmeticDecoder, BitModel, IntegerCompressor, SymbolModel};

/// Rolling state carried by the XYZ predictor.
///
/// The predictor models each component independently with the same 2-step
/// linear extrapolation: `predicted = last + last_delta`.
#[derive(Debug, Clone, Default)]
pub struct PointXYZContext {
    /// Most recently decoded X value.
    pub last_x: i32,
    /// Most recently decoded Y value.
    pub last_y: i32,
    /// Most recently decoded Z value.
    pub last_z: i32,
    /// Delta `last_x - prev_x` used for next-point extrapolation.
    pub last_delta_x: i32,
    /// Delta `last_y - prev_y` used for next-point extrapolation.
    pub last_delta_y: i32,
    /// Delta `last_z - prev_z` used for next-point extrapolation.
    pub last_delta_z: i32,
}

impl PointXYZContext {
    /// Construct a context primed with the given seed point.  Used for the
    /// first point of a chunk, which the encoder emits raw.
    pub fn with_seed(x: i32, y: i32, z: i32) -> Self {
        Self {
            last_x: x,
            last_y: y,
            last_z: z,
            last_delta_x: 0,
            last_delta_y: 0,
            last_delta_z: 0,
        }
    }
}

/// Decode the next XYZ triple using the predictor + integer compressor.
///
/// Mutates `ctx` to reflect the new point and returns the decoded `(x, y, z)`.
pub fn decode_xyz_v1(
    ic: &mut IntegerCompressor,
    decoder: &mut ArithmeticDecoder<'_>,
    ctx: &mut PointXYZContext,
) -> (i32, i32, i32) {
    let pred_x = ctx.last_x.wrapping_add(ctx.last_delta_x);
    let pred_y = ctx.last_y.wrapping_add(ctx.last_delta_y);
    let pred_z = ctx.last_z.wrapping_add(ctx.last_delta_z);

    let x = ic.decompress(decoder, pred_x, 0);
    let y = ic.decompress(decoder, pred_y, 1);
    let z = ic.decompress(decoder, pred_z, 2);

    ctx.last_delta_x = x.wrapping_sub(ctx.last_x);
    ctx.last_delta_y = y.wrapping_sub(ctx.last_y);
    ctx.last_delta_z = z.wrapping_sub(ctx.last_z);
    ctx.last_x = x;
    ctx.last_y = y;
    ctx.last_z = z;

    (x, y, z)
}

/// Encoder companion of [`decode_xyz_v1`] (test-only).
#[cfg(any(test, feature = "laz-encoder"))]
pub fn encode_xyz_v1(
    ic: &mut IntegerCompressor,
    encoder: &mut ArithmeticEncoder,
    ctx: &mut PointXYZContext,
    x: i32,
    y: i32,
    z: i32,
) {
    let pred_x = ctx.last_x.wrapping_add(ctx.last_delta_x);
    let pred_y = ctx.last_y.wrapping_add(ctx.last_delta_y);
    let pred_z = ctx.last_z.wrapping_add(ctx.last_delta_z);

    ic.compress(encoder, pred_x, x, 0);
    ic.compress(encoder, pred_y, y, 1);
    ic.compress(encoder, pred_z, z, 2);

    ctx.last_delta_x = x.wrapping_sub(ctx.last_x);
    ctx.last_delta_y = y.wrapping_sub(ctx.last_y);
    ctx.last_delta_z = z.wrapping_sub(ctx.last_z);
    ctx.last_x = x;
    ctx.last_y = y;
    ctx.last_z = z;
}

/// Decode an intensity value modeled as `last + corrector`.
///
/// `model` is a SymbolModel sized to 256 bins; the corrector is reduced to a
/// signed byte and added with wrapping semantics.
pub fn decode_intensity_v1(
    decoder: &mut ArithmeticDecoder<'_>,
    model: &mut SymbolModel,
    last: u16,
) -> u16 {
    let corr_byte = decoder.decode_symbol(model) as i8;
    let corr = i32::from(corr_byte);
    ((i32::from(last)).wrapping_add(corr) & 0xFFFF) as u16
}

/// Encoder companion of [`decode_intensity_v1`] (test-only).
#[cfg(any(test, feature = "laz-encoder"))]
pub fn encode_intensity_v1(
    encoder: &mut ArithmeticEncoder,
    model: &mut SymbolModel,
    last: u16,
    actual: u16,
) {
    let delta = (i32::from(actual)).wrapping_sub(i32::from(last));
    let clamped = delta.clamp(-128, 127) as i8;
    encoder.encode_symbol(model, (clamped as u8) as u32);
}

/// Decode a classification byte.
///
/// LASzip v1 uses 256 independent symbol models, one per "previous
/// classification" value.  Each model emits the new classification directly.
pub fn decode_classification_v1(
    decoder: &mut ArithmeticDecoder<'_>,
    models: &mut [SymbolModel],
    last: u8,
) -> u8 {
    let model = &mut models[last as usize];
    decoder.decode_symbol(model) as u8
}

/// Encoder companion of [`decode_classification_v1`] (test-only).
#[cfg(any(test, feature = "laz-encoder"))]
pub fn encode_classification_v1(
    encoder: &mut ArithmeticEncoder,
    models: &mut [SymbolModel],
    last: u8,
    actual: u8,
) {
    let model = &mut models[last as usize];
    encoder.encode_symbol(model, u32::from(actual));
}

/// Allocate the 256 classification context models used by the v1 codec.
///
/// `SymbolModel` is not `Copy`, so the natural `[SymbolModel; 256]` literal
/// would be rejected by the compiler.  This helper returns a `Vec` of the
/// correct length instead.
pub fn make_classification_models() -> Vec<SymbolModel> {
    (0..256).map(|_| SymbolModel::new(256)).collect()
}

/// Decode a user_data byte modeled as `last + corrector`.
pub fn decode_user_data_v1(
    decoder: &mut ArithmeticDecoder<'_>,
    model: &mut SymbolModel,
    last: u8,
) -> u8 {
    let corr_byte = decoder.decode_symbol(model) as i8;
    let corr = i32::from(corr_byte);
    ((i32::from(last)).wrapping_add(corr) & 0xFF) as u8
}

/// Encoder companion of [`decode_user_data_v1`] (test-only).
#[cfg(any(test, feature = "laz-encoder"))]
pub fn encode_user_data_v1(
    encoder: &mut ArithmeticEncoder,
    model: &mut SymbolModel,
    last: u8,
    actual: u8,
) {
    let delta = (i32::from(actual)).wrapping_sub(i32::from(last));
    let clamped = delta.clamp(-128, 127) as i8;
    encoder.encode_symbol(model, (clamped as u8) as u32);
}

/// Decode a point_source_id u16.
pub fn decode_point_source_id_v1(
    decoder: &mut ArithmeticDecoder<'_>,
    model: &mut SymbolModel,
    last: u16,
) -> u16 {
    let corr_byte = decoder.decode_symbol(model) as i8;
    let corr = i32::from(corr_byte);
    ((i32::from(last)).wrapping_add(corr) & 0xFFFF) as u16
}

/// Encoder companion of [`decode_point_source_id_v1`] (test-only).
#[cfg(any(test, feature = "laz-encoder"))]
pub fn encode_point_source_id_v1(
    encoder: &mut ArithmeticEncoder,
    model: &mut SymbolModel,
    last: u16,
    actual: u16,
) {
    let delta = (i32::from(actual)).wrapping_sub(i32::from(last));
    let clamped = delta.clamp(-128, 127) as i8;
    encoder.encode_symbol(model, (clamped as u8) as u32);
}

/// Decode the scan-angle-rank byte (i8, modeled the same as user_data).
pub fn decode_scan_angle_rank_v1(
    decoder: &mut ArithmeticDecoder<'_>,
    model: &mut SymbolModel,
    last: i8,
) -> i8 {
    let corr_byte = decoder.decode_symbol(model) as i8;
    let corr = i32::from(corr_byte);
    (i32::from(last).wrapping_add(corr)).clamp(-128, 127) as i8
}

/// Encoder companion of [`decode_scan_angle_rank_v1`] (test-only).
#[cfg(any(test, feature = "laz-encoder"))]
pub fn encode_scan_angle_rank_v1(
    encoder: &mut ArithmeticEncoder,
    model: &mut SymbolModel,
    last: i8,
    actual: i8,
) {
    let delta = (i32::from(actual)).wrapping_sub(i32::from(last));
    let clamped = delta.clamp(-128, 127) as i8;
    encoder.encode_symbol(model, (clamped as u8) as u32);
}

/// Decode the bit-packed return_number / number_of_returns / scan_direction /
/// edge_of_flight_line byte for legacy formats (0-5).
///
/// The reference codec models this byte as a single 256-bin distribution.
pub fn decode_return_flags_v1(decoder: &mut ArithmeticDecoder<'_>, model: &mut SymbolModel) -> u8 {
    decoder.decode_symbol(model) as u8
}

/// Encoder companion (test-only).
#[cfg(any(test, feature = "laz-encoder"))]
pub fn encode_return_flags_v1(encoder: &mut ArithmeticEncoder, model: &mut SymbolModel, value: u8) {
    encoder.encode_symbol(model, u32::from(value));
}

/// Decode the f64 GPS time.  In v1 the GPS time delta is encoded with a
/// dedicated [`IntegerCompressor`] over the i64 bit pattern.
pub fn decode_gps_time_v1(
    ic: &mut IntegerCompressor,
    decoder: &mut ArithmeticDecoder<'_>,
    last_bits: &mut i64,
) -> f64 {
    let pred = *last_bits as i32;
    let lo = ic.decompress(decoder, pred, 0);
    let hi_pred = (*last_bits >> 32) as i32;
    let hi = ic.decompress(decoder, hi_pred, 1);
    let new_bits = ((hi as i64) << 32) | (lo as i64 & 0xFFFF_FFFF);
    *last_bits = new_bits;
    f64::from_bits(new_bits as u64)
}

/// Encoder companion of [`decode_gps_time_v1`] (test-only).
#[cfg(any(test, feature = "laz-encoder"))]
pub fn encode_gps_time_v1(
    ic: &mut IntegerCompressor,
    encoder: &mut ArithmeticEncoder,
    last_bits: &mut i64,
    actual: f64,
) {
    let new_bits = actual.to_bits() as i64;
    let lo_pred = *last_bits as i32;
    let lo_actual = new_bits as i32;
    ic.compress(encoder, lo_pred, lo_actual, 0);
    let hi_pred = (*last_bits >> 32) as i32;
    let hi_actual = (new_bits >> 32) as i32;
    ic.compress(encoder, hi_pred, hi_actual, 1);
    *last_bits = new_bits;
}

/// Holder for the no-op binary model used by the unused-but-spec'd bit fields.
///
/// Defined to keep the v1 encode/decode call sites symmetrical with the
/// reference implementation.
#[derive(Debug, Default, Clone)]
pub struct UnusedBitContext {
    /// Per-format reserved bit model.
    pub model: BitModel,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_predictor_xyz_v1_recovers_constant_stream() {
        // Encode the same XYZ value 16 times, decode, expect bit-for-bit match.
        let mut ic_enc = IntegerCompressor::new(32, 3);
        let mut enc = ArithmeticEncoder::new();
        let mut ctx_enc = PointXYZContext::with_seed(1_000, 2_000, 50);
        for _ in 0..16 {
            encode_xyz_v1(&mut ic_enc, &mut enc, &mut ctx_enc, 1_000, 2_000, 50);
        }
        let bytes = enc.done();
        let mut ic_dec = IntegerCompressor::new(32, 3);
        let mut dec = ArithmeticDecoder::new(&bytes).expect("decoder init");
        let mut ctx_dec = PointXYZContext::with_seed(1_000, 2_000, 50);
        for _ in 0..16 {
            let (x, y, z) = decode_xyz_v1(&mut ic_dec, &mut dec, &mut ctx_dec);
            assert_eq!((x, y, z), (1_000, 2_000, 50));
        }
    }

    #[test]
    fn test_predictor_xyz_v1_recovers_linear_stream() {
        // Encode points that advance by +10 each step.
        let mut ic_enc = IntegerCompressor::new(32, 3);
        let mut enc = ArithmeticEncoder::new();
        let mut ctx_enc = PointXYZContext::with_seed(0, 0, 0);
        for i in 0..16 {
            let x = (i + 1) * 10;
            let y = (i + 1) * 20;
            let z = (i + 1) * 5;
            encode_xyz_v1(&mut ic_enc, &mut enc, &mut ctx_enc, x, y, z);
        }
        let bytes = enc.done();
        let mut ic_dec = IntegerCompressor::new(32, 3);
        let mut dec = ArithmeticDecoder::new(&bytes).expect("decoder init");
        let mut ctx_dec = PointXYZContext::with_seed(0, 0, 0);
        for i in 0..16 {
            let (x, y, z) = decode_xyz_v1(&mut ic_dec, &mut dec, &mut ctx_dec);
            assert_eq!(x, (i + 1) * 10);
            assert_eq!(y, (i + 1) * 20);
            assert_eq!(z, (i + 1) * 5);
        }
    }

    #[test]
    fn test_predictor_intensity_v1_recovers_repeated_values() {
        let mut model_enc = SymbolModel::new(256);
        let mut enc = ArithmeticEncoder::new();
        let mut last_enc: u16 = 100;
        for _ in 0..32 {
            encode_intensity_v1(&mut enc, &mut model_enc, last_enc, 100);
            last_enc = 100;
        }
        let bytes = enc.done();
        let mut model_dec = SymbolModel::new(256);
        let mut dec = ArithmeticDecoder::new(&bytes).expect("decoder init");
        let mut last_dec: u16 = 100;
        for _ in 0..32 {
            let v = decode_intensity_v1(&mut dec, &mut model_dec, last_dec);
            assert_eq!(v, 100);
            last_dec = v;
        }
    }

    #[test]
    fn test_predictor_classification_v1_256_contexts_independent() {
        // Encode 256 transitions through 256 distinct previous classes.
        let mut models_enc = make_classification_models();
        let mut enc = ArithmeticEncoder::new();
        let pattern: Vec<(u8, u8)> = (0..256).map(|i| (i as u8, ((i + 1) % 256) as u8)).collect();
        for (last, actual) in &pattern {
            encode_classification_v1(&mut enc, &mut models_enc, *last, *actual);
        }
        let bytes = enc.done();
        let mut models_dec = make_classification_models();
        let mut dec = ArithmeticDecoder::new(&bytes).expect("decoder init");
        for (last, expected) in &pattern {
            let got = decode_classification_v1(&mut dec, &mut models_dec, *last);
            assert_eq!(got, *expected, "last={last}");
        }
    }
}
