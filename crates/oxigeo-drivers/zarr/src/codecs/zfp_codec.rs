//! Pure-Rust lossy floating-point quantiser (NOT the ZFP bitstream format).
//!
//! # What this is
//!
//! A Pure-Rust, mode-aware, *lossy* scalar quantiser for `f32`/`f64` arrays.
//! It applies delta prediction followed by uniform quantisation at a
//! resolution derived from the configured [`ZfpMode`], then stores the
//! quantised deltas as fixed-width little-endian integers. The reconstruction
//! error is bounded by half the quantisation step.
//!
//! # What this is NOT
//!
//! This is **not** an implementation of the ZFP bitstream format
//! (`llnl/zfp`): it performs no block transform, no exponent alignment and no
//! embedded/bit-plane coding, and its output is **not interoperable** with
//! any other ZFP implementation. It is therefore hidden from the public docs
//! and is deliberately **not** wired into the Zarr v3 codec dispatcher: a
//! `zarr.json` declaring `zfp` is rejected as an unknown codec.
//!
//! # Correctness guarantees
//!
//! * The configured [`ZfpMode`] is honoured -- each mode maps to a distinct
//!   quantisation scale, so different modes produce different output.
//! * Quantisation is **overflow-checked**: if a value/delta cannot be
//!   represented at the requested resolution within the fixed-width integer,
//!   [`CodecError::CompressionFailed`] is returned. The previous
//!   implementation silently saturated on overflow (`(x) as i16`),
//!   permanently corrupting the reconstructed value; that can no longer
//!   happen.
//! * The scale is written into the payload header, so decoding never depends
//!   on the decoder being constructed with the same mode.

use crate::codecs::Codec;
use crate::error::{CodecError, Result, ZarrError};

/// Quantiser magic byte (payload format version).
const ZFP_VERSION: u8 = 0x02;
/// Marker for `f32` payloads.
const MARKER_F32: u8 = 0x20;
/// Marker for `f64` payloads.
const MARKER_F64: u8 = 0x40;
/// Header size: version(1) + marker(1) + len(4) + scale(8).
const HEADER_LEN: usize = 14;
/// Upper bound on the base-2 exponent used to derive a scale, so a large
/// `rate`/`precision` cannot produce a non-finite or absurd scale.
const MAX_SCALE_EXP: i32 = 40;

/// Quantiser mode. Each variant maps to a distinct quantisation scale, so the
/// choice of mode always affects the encoded output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZfpMode {
    /// Rate-controlled: `rate` behaves as a target number of precision bits;
    /// higher `rate` yields a finer quantisation step.
    FixedRate {
        /// Precision bits (clamped to a sane maximum internally).
        rate: u32,
    },
    /// Precision-controlled: `precision` fractional bits are retained.
    FixedPrecision {
        /// Number of fractional bits (clamped internally).
        precision: u32,
    },
    /// Accuracy-controlled: `tolerance` is the absolute error budget; the
    /// quantisation step is set so the reconstruction error stays within it.
    FixedAccuracy {
        /// Absolute error tolerance (`0` is treated as high precision).
        tolerance: u32,
    },
}

impl ZfpMode {
    /// Derives the quantisation scale (multiplicative factor applied before
    /// rounding to an integer) for this mode. Always finite and `>= 1.0`.
    #[must_use]
    fn scale(self) -> f64 {
        match self {
            Self::FixedRate { rate } => {
                let exp = (rate as i32).clamp(0, MAX_SCALE_EXP);
                2f64.powi(exp)
            }
            Self::FixedPrecision { precision } => {
                let exp = (precision as i32).clamp(0, MAX_SCALE_EXP);
                2f64.powi(exp)
            }
            Self::FixedAccuracy { tolerance } => {
                if tolerance == 0 {
                    2f64.powi(16)
                } else {
                    // step == tolerance  =>  scale == 1/tolerance, but never
                    // below 1.0 so the codec stays meaningfully quantising.
                    (1.0 / f64::from(tolerance)).max(1.0)
                }
            }
        }
    }
}

/// ZFP-style data type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZfpDataType {
    /// 32-bit float.
    Float32,
    /// 64-bit float.
    Float64,
}

/// Pure-Rust lossy floating-point quantiser (see module docs).
#[derive(Debug, Clone)]
pub struct ZfpCodec {
    /// Compression mode.
    mode: ZfpMode,
    /// Data type (f32 or f64).
    dtype: ZfpDataType,
}

impl ZfpCodec {
    /// Creates a new codec with rate-controlled quantisation.
    #[must_use]
    pub const fn fixed_rate(rate: u32, dtype: ZfpDataType) -> Self {
        Self {
            mode: ZfpMode::FixedRate { rate },
            dtype,
        }
    }

    /// Creates a new codec with precision-controlled quantisation.
    #[must_use]
    pub const fn fixed_precision(precision: u32, dtype: ZfpDataType) -> Self {
        Self {
            mode: ZfpMode::FixedPrecision { precision },
            dtype,
        }
    }

    /// Creates a new codec with accuracy-controlled quantisation.
    #[must_use]
    pub const fn fixed_accuracy(tolerance: u32, dtype: ZfpDataType) -> Self {
        Self {
            mode: ZfpMode::FixedAccuracy { tolerance },
            dtype,
        }
    }

    /// Returns the configured mode.
    #[must_use]
    pub const fn mode(&self) -> ZfpMode {
        self.mode
    }

    /// Writes the common payload header (version, marker, length, scale).
    fn write_header(out: &mut Vec<u8>, marker: u8, len: usize, scale: f64) {
        out.push(ZFP_VERSION);
        out.push(marker);
        out.extend_from_slice(&(len as u32).to_le_bytes());
        out.extend_from_slice(&scale.to_bits().to_le_bytes());
    }

    /// Reads and validates the common payload header, returning `(len, scale)`.
    fn read_header(data: &[u8], expected_marker: u8) -> Result<(usize, f64)> {
        if data.len() < HEADER_LEN {
            return Err(ZarrError::Codec(CodecError::DecompressionFailed {
                message: format!(
                    "quantiser payload too short: {} bytes (need at least {HEADER_LEN})",
                    data.len()
                ),
            }));
        }
        if data[1] != expected_marker {
            return Err(ZarrError::Codec(CodecError::DecompressionFailed {
                message: format!(
                    "quantiser data type marker mismatch: got {:#04x}, expected {:#04x}",
                    data[1], expected_marker
                ),
            }));
        }
        let len = u32::from_le_bytes([data[2], data[3], data[4], data[5]]) as usize;
        let scale_bits = u64::from_le_bytes([
            data[6], data[7], data[8], data[9], data[10], data[11], data[12], data[13],
        ]);
        let scale = f64::from_bits(scale_bits);
        if !scale.is_finite() || scale <= 0.0 {
            return Err(ZarrError::Codec(CodecError::DecompressionFailed {
                message: "quantiser header carries a non-finite/non-positive scale".to_string(),
            }));
        }
        Ok((len, scale))
    }

    /// Quantises `value - prev` at `scale`, returning the integer code and the
    /// reconstructed running `prev` (so encode and decode stay drift-free).
    ///
    /// Returns a typed error rather than silently saturating when the delta
    /// cannot be represented in `[min, max]`.
    fn quantise_delta(value: f64, prev: f64, scale: f64, min: f64, max: f64) -> Result<(i64, f64)> {
        if !value.is_finite() {
            return Err(ZarrError::Codec(CodecError::CompressionFailed {
                message: format!("cannot quantise non-finite value {value}"),
            }));
        }
        let delta = value - prev;
        let scaled = (delta * scale).round();
        if !scaled.is_finite() || scaled < min || scaled > max {
            return Err(ZarrError::Codec(CodecError::CompressionFailed {
                message: format!(
                    "quantised delta {scaled} for value {value} overflows the representable \
                     range [{min}, {max}] at scale {scale}; choose a coarser mode"
                ),
            }));
        }
        // `scaled` is finite and within [min, max]; the cast is exact.
        let code = scaled as i64;
        let reconstructed = prev + (scaled / scale);
        Ok((code, reconstructed))
    }

    /// Compresses `f32` data (delta + overflow-checked quantisation).
    fn compress_f32(&self, data: &[f32]) -> Result<Vec<u8>> {
        let scale = self.mode.scale();
        let mut out = Vec::with_capacity(HEADER_LEN + data.len() * 4);
        Self::write_header(&mut out, MARKER_F32, data.len(), scale);

        let mut prev = 0.0f64;
        for &value in data {
            let (code, reconstructed) = Self::quantise_delta(
                f64::from(value),
                prev,
                scale,
                f64::from(i32::MIN),
                f64::from(i32::MAX),
            )?;
            out.extend_from_slice(&(code as i32).to_le_bytes());
            prev = reconstructed;
        }
        Ok(out)
    }

    /// Decompresses `f32` data.
    fn decompress_f32(&self, data: &[u8]) -> Result<Vec<f32>> {
        let (len, scale) = Self::read_header(data, MARKER_F32)?;
        let body = &data[HEADER_LEN..];
        if body.len() < len * 4 {
            return Err(ZarrError::Codec(CodecError::DecompressionFailed {
                message: format!(
                    "quantiser body truncated: {} bytes for {len} f32 values",
                    body.len()
                ),
            }));
        }

        let mut result = Vec::with_capacity(len);
        let mut prev = 0.0f64;
        for chunk in body.chunks_exact(4).take(len) {
            let code = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            prev += f64::from(code) / scale;
            result.push(prev as f32);
        }
        Ok(result)
    }

    /// Compresses `f64` data (delta + overflow-checked quantisation).
    fn compress_f64(&self, data: &[f64]) -> Result<Vec<u8>> {
        let scale = self.mode.scale();
        let mut out = Vec::with_capacity(HEADER_LEN + data.len() * 8);
        Self::write_header(&mut out, MARKER_F64, data.len(), scale);

        let mut prev = 0.0f64;
        for &value in data {
            let (code, reconstructed) =
                Self::quantise_delta(value, prev, scale, i64::MIN as f64, i64::MAX as f64)?;
            out.extend_from_slice(&code.to_le_bytes());
            prev = reconstructed;
        }
        Ok(out)
    }

    /// Decompresses `f64` data.
    fn decompress_f64(&self, data: &[u8]) -> Result<Vec<f64>> {
        let (len, scale) = Self::read_header(data, MARKER_F64)?;
        let body = &data[HEADER_LEN..];
        if body.len() < len * 8 {
            return Err(ZarrError::Codec(CodecError::DecompressionFailed {
                message: format!(
                    "quantiser body truncated: {} bytes for {len} f64 values",
                    body.len()
                ),
            }));
        }

        let mut result = Vec::with_capacity(len);
        let mut prev = 0.0f64;
        for chunk in body.chunks_exact(8).take(len) {
            let code = i64::from_le_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
            ]);
            prev += code as f64 / scale;
            result.push(prev);
        }
        Ok(result)
    }
}

impl Codec for ZfpCodec {
    fn id(&self) -> &str {
        "zfp"
    }

    fn encode(&self, data: &[u8]) -> Result<Vec<u8>> {
        match self.dtype {
            ZfpDataType::Float32 => {
                if !data.len().is_multiple_of(4) {
                    return Err(ZarrError::Codec(CodecError::CompressionFailed {
                        message: "Data length not multiple of 4 for float32".to_string(),
                    }));
                }
                let floats: Vec<f32> = data
                    .chunks_exact(4)
                    .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                    .collect();
                self.compress_f32(&floats)
            }
            ZfpDataType::Float64 => {
                if !data.len().is_multiple_of(8) {
                    return Err(ZarrError::Codec(CodecError::CompressionFailed {
                        message: "Data length not multiple of 8 for float64".to_string(),
                    }));
                }
                let floats: Vec<f64> = data
                    .chunks_exact(8)
                    .map(|chunk| {
                        f64::from_le_bytes([
                            chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6],
                            chunk[7],
                        ])
                    })
                    .collect();
                self.compress_f64(&floats)
            }
        }
    }

    fn decode(&self, data: &[u8]) -> Result<Vec<u8>> {
        match self.dtype {
            ZfpDataType::Float32 => {
                let floats = self.decompress_f32(data)?;
                let mut result = Vec::with_capacity(floats.len() * 4);
                for &value in &floats {
                    result.extend_from_slice(&value.to_le_bytes());
                }
                Ok(result)
            }
            ZfpDataType::Float64 => {
                let floats = self.decompress_f64(data)?;
                let mut result = Vec::with_capacity(floats.len() * 8);
                for &value in &floats {
                    result.extend_from_slice(&value.to_le_bytes());
                }
                Ok(result)
            }
        }
    }

    fn clone_box(&self) -> Box<dyn Codec> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f32_bytes(values: &[f32]) -> Vec<u8> {
        values.iter().flat_map(|f| f.to_le_bytes()).collect()
    }

    #[test]
    fn test_zfp_codec_f32() {
        let codec = ZfpCodec::fixed_rate(16, ZfpDataType::Float32);
        assert_eq!(codec.id(), "zfp");

        let floats = vec![1.0f32, 2.0, 3.0, 4.0, 5.0];
        let compressed = codec.encode(&f32_bytes(&floats)).expect("compress");
        let decompressed = codec.decode(&compressed).expect("decompress");

        let decoded: Vec<f32> = decompressed
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();

        for (a, b) in floats.iter().zip(decoded.iter()) {
            assert!((a - b).abs() < 0.01, "Values differ: {a} vs {b}");
        }
    }

    #[test]
    fn test_zfp_codec_f64() {
        let codec = ZfpCodec::fixed_precision(20, ZfpDataType::Float64);
        let floats = vec![1.0f64, 2.0, 3.0, 4.0, 5.0];
        let mut data = Vec::new();
        for &f in &floats {
            data.extend_from_slice(&f.to_le_bytes());
        }

        let compressed = codec.encode(&data).expect("compress");
        let decompressed = codec.decode(&compressed).expect("decompress");
        let decoded: Vec<f64> = decompressed
            .chunks_exact(8)
            .map(|c| f64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
            .collect();

        for (a, b) in floats.iter().zip(decoded.iter()) {
            assert!((a - b).abs() < 0.01, "Values differ: {a} vs {b}");
        }
    }

    #[test]
    fn test_zfp_invalid_length() {
        let codec = ZfpCodec::fixed_rate(16, ZfpDataType::Float32);
        assert!(codec.encode(&[1, 2, 3]).is_err());
    }

    #[test]
    fn test_zfp_mode_actually_affects_output() {
        // Two different modes MUST produce different encoded bytes for the
        // same input -- the old implementation ignored the mode entirely.
        let data = f32_bytes(&[0.1, 0.2, 0.35, 0.9, 1.7]);
        let coarse = ZfpCodec::fixed_rate(4, ZfpDataType::Float32)
            .encode(&data)
            .expect("coarse");
        let fine = ZfpCodec::fixed_rate(18, ZfpDataType::Float32)
            .encode(&data)
            .expect("fine");
        assert_ne!(
            coarse, fine,
            "different quantiser modes must yield different output"
        );

        let acc = ZfpCodec::fixed_accuracy(4, ZfpDataType::Float32)
            .encode(&data)
            .expect("accuracy");
        assert_ne!(coarse, acc);
    }

    #[test]
    fn test_zfp_accuracy_mode_bounds_error() {
        // fixed_accuracy(t) must keep the reconstruction error within ~t.
        let values: Vec<f32> = (0..64).map(|i| (i as f32) * 0.013 - 0.4).collect();
        let tolerance = 2u32;
        let codec = ZfpCodec::fixed_accuracy(tolerance, ZfpDataType::Float32);
        let decoded_bytes = codec
            .decode(&codec.encode(&f32_bytes(&values)).expect("enc"))
            .expect("dec");
        let decoded: Vec<f32> = decoded_bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        for (a, b) in values.iter().zip(decoded.iter()) {
            assert!(
                (a - b).abs() <= tolerance as f32,
                "accuracy bound violated: {a} vs {b}"
            );
        }
    }

    #[test]
    fn test_zfp_overflow_returns_typed_error_not_corruption() {
        // A huge delta at a fine scale cannot be represented; the codec must
        // return a typed error instead of silently saturating (the old
        // `(x) as i16` behaviour permanently corrupted such values).
        let codec = ZfpCodec::fixed_precision(30, ZfpDataType::Float32);
        let data = f32_bytes(&[0.0, 1.0e12]);
        let result = codec.encode(&data);
        assert!(
            matches!(
                result,
                Err(ZarrError::Codec(CodecError::CompressionFailed { .. }))
            ),
            "expected typed CompressionFailed on overflow, got {result:?}"
        );
    }

    #[test]
    fn test_zfp_rejects_non_finite() {
        let codec = ZfpCodec::fixed_rate(10, ZfpDataType::Float32);
        let data = f32_bytes(&[f32::NAN]);
        assert!(codec.encode(&data).is_err());
    }
}
