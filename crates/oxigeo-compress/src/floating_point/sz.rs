//! SZ-style floating-point compression
//!
//! SZ is an error-bounded lossy compression algorithm for scientific data.
//! This implementation follows the core SZ design: a configurable-order 1-D
//! Lorenzo predictor produces residuals, those residuals are quantized against
//! the error bound, and the quantization codes are ZigZag + LEB128 varint coded.
//! Because smooth/near-constant data predicts well, its residual codes collapse
//! to one byte each (or an "unpredictable" fallback for the rare outlier), so a
//! wider error bound genuinely shrinks the output below the 4-bytes/value
//! lossless footprint — the predictor order actually drives that trade-off.

use super::FpMode;
use crate::error::{CompressionError, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;

/// Half-range of the quantization index. A predicted residual whose quantization
/// code falls outside `[-RADIUS, RADIUS]` (or is non-finite) is stored verbatim
/// as an "unpredictable" value instead, keeping the varint code stream compact
/// and the reconstruction always within the error bound.
const SZ_RADIUS: i64 = 32767;

/// Minimum magnitude used when deriving a point-wise error bound from a predicted
/// value, avoiding a division-by-zero / infinite quantization code at magnitude 0.
const SZ_POINTWISE_MIN_MAG_F32: f32 = 1e-20;
/// f64 counterpart of [`SZ_POINTWISE_MIN_MAG_F32`].
const SZ_POINTWISE_MIN_MAG_F64: f64 = 1e-300;

/// ZigZag-encode a signed integer into an unsigned one so small-magnitude values
/// (the common case for predicted residuals) map to small unsigned values that
/// a LEB128 varint stores in one byte.
#[inline]
fn zigzag_encode(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

/// Inverse of [`zigzag_encode`].
#[inline]
fn zigzag_decode(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

/// Append an unsigned LEB128 varint.
fn write_uvarint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

/// Read an unsigned LEB128 varint from `cursor`.
fn read_uvarint(cursor: &mut Cursor<&[u8]>) -> Result<u64> {
    let mut result: u64 = 0;
    let mut shift: u32 = 0;
    loop {
        let byte = cursor.read_u8()?;
        if shift >= 64 {
            return Err(CompressionError::FloatingPointError(
                "SZ varint overflow: stream is corrupt".to_string(),
            ));
        }
        result |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
    }
    Ok(result)
}

/// How the per-value error bound is derived inside the predictive coder (f32).
#[derive(Clone, Copy)]
enum EbSpec32 {
    /// A single absolute bound applied to every value (absolute & relative modes).
    Constant(f32),
    /// A point-wise relative bound: `fraction * |value|`.
    Pointwise(f32),
}

/// f64 counterpart of [`EbSpec32`].
#[derive(Clone, Copy)]
enum EbSpec64 {
    Constant(f64),
    Pointwise(f64),
}

/// 1-D Lorenzo predictor over already-reconstructed f32 samples.
///
/// Order 0 predicts 0, order 1 the previous sample, order 2 a linear
/// extrapolation, order 3 a quadratic one. Near the start of the stream the
/// effective order is clamped to how many reconstructed samples exist, so a
/// higher configured order genuinely changes what is predicted (and therefore the
/// residual magnitudes and the compressed size).
fn sz_predict_f32(recon: &[f32], order: usize) -> f32 {
    let n = recon.len();
    match order.min(n).min(3) {
        1 => recon[n - 1],
        2 => 2.0 * recon[n - 1] - recon[n - 2],
        3 => 3.0 * recon[n - 1] - 3.0 * recon[n - 2] + recon[n - 3],
        _ => 0.0,
    }
}

/// f64 counterpart of [`sz_predict_f32`].
fn sz_predict_f64(recon: &[f64], order: usize) -> f64 {
    let n = recon.len();
    match order.min(n).min(3) {
        1 => recon[n - 1],
        2 => 2.0 * recon[n - 1] - recon[n - 2],
        3 => 3.0 * recon[n - 1] - 3.0 * recon[n - 2] + recon[n - 3],
        _ => 0.0,
    }
}

/// Predictive-quantization body encoder (f32).
///
/// For each value it predicts from the reconstructed history, quantizes the
/// residual against the (possibly point-wise) error bound, and emits either a
/// compact varint quantization code or — when the code would overflow, the value
/// is non-finite, or the point-wise bound is violated — an "unpredictable" marker
/// followed by the raw 32-bit value. The reconstructed value (never the original)
/// feeds subsequent predictions, keeping encoder and decoder in lock-step.
fn sz_encode_body_f32(out: &mut Vec<u8>, input: &[f32], order: usize, eb_spec: EbSpec32) {
    let mut recon: Vec<f32> = Vec::with_capacity(input.len());
    for &val in input {
        let pred = sz_predict_f32(&recon, order);
        let eb_i = match eb_spec {
            EbSpec32::Constant(e) => e,
            EbSpec32::Pointwise(frac) => frac * pred.abs().max(SZ_POINTWISE_MIN_MAG_F32),
        };

        let mut emit_raw = true;
        if val.is_finite() && pred.is_finite() && eb_i > 0.0 {
            let d = (val - pred) / (2.0 * eb_i);
            if d.is_finite() {
                let qf = d.round();
                if qf.abs() <= SZ_RADIUS as f32 {
                    let q = qf as i64;
                    let recon_val = pred + (q as f32) * (2.0 * eb_i);
                    let within = match eb_spec {
                        EbSpec32::Constant(_) => true,
                        EbSpec32::Pointwise(frac) => (val - recon_val).abs() <= frac * val.abs(),
                    };
                    if within {
                        write_uvarint(out, (zigzag_encode(q) << 1) | 1);
                        recon.push(recon_val);
                        emit_raw = false;
                    }
                }
            }
        }

        if emit_raw {
            write_uvarint(out, 0);
            out.extend_from_slice(&val.to_bits().to_le_bytes());
            recon.push(val);
        }
    }
}

/// Predictive-quantization body decoder (f32); inverse of [`sz_encode_body_f32`].
fn sz_decode_body_f32(
    cursor: &mut Cursor<&[u8]>,
    len: usize,
    order: usize,
    eb_spec: EbSpec32,
) -> Result<Vec<f32>> {
    let mut recon: Vec<f32> = Vec::with_capacity(len);
    for _ in 0..len {
        let pred = sz_predict_f32(&recon, order);
        let token = read_uvarint(cursor)?;
        if token & 1 == 0 {
            // Unpredictable: raw 32-bit value follows.
            let bits = cursor.read_u32::<LittleEndian>()?;
            recon.push(f32::from_bits(bits));
        } else {
            let q = zigzag_decode(token >> 1);
            let eb_i = match eb_spec {
                EbSpec32::Constant(e) => e,
                EbSpec32::Pointwise(frac) => frac * pred.abs().max(SZ_POINTWISE_MIN_MAG_F32),
            };
            recon.push(pred + (q as f32) * (2.0 * eb_i));
        }
    }
    Ok(recon)
}

/// Predictive-quantization body encoder (f64). See [`sz_encode_body_f32`].
fn sz_encode_body_f64(out: &mut Vec<u8>, input: &[f64], order: usize, eb_spec: EbSpec64) {
    let mut recon: Vec<f64> = Vec::with_capacity(input.len());
    for &val in input {
        let pred = sz_predict_f64(&recon, order);
        let eb_i = match eb_spec {
            EbSpec64::Constant(e) => e,
            EbSpec64::Pointwise(frac) => frac * pred.abs().max(SZ_POINTWISE_MIN_MAG_F64),
        };

        let mut emit_raw = true;
        if val.is_finite() && pred.is_finite() && eb_i > 0.0 {
            let d = (val - pred) / (2.0 * eb_i);
            if d.is_finite() {
                let qf = d.round();
                if qf.abs() <= SZ_RADIUS as f64 {
                    let q = qf as i64;
                    let recon_val = pred + (q as f64) * (2.0 * eb_i);
                    let within = match eb_spec {
                        EbSpec64::Constant(_) => true,
                        EbSpec64::Pointwise(frac) => (val - recon_val).abs() <= frac * val.abs(),
                    };
                    if within {
                        write_uvarint(out, (zigzag_encode(q) << 1) | 1);
                        recon.push(recon_val);
                        emit_raw = false;
                    }
                }
            }
        }

        if emit_raw {
            write_uvarint(out, 0);
            out.extend_from_slice(&val.to_bits().to_le_bytes());
            recon.push(val);
        }
    }
}

/// Predictive-quantization body decoder (f64); inverse of [`sz_encode_body_f64`].
fn sz_decode_body_f64(
    cursor: &mut Cursor<&[u8]>,
    len: usize,
    order: usize,
    eb_spec: EbSpec64,
) -> Result<Vec<f64>> {
    let mut recon: Vec<f64> = Vec::with_capacity(len);
    for _ in 0..len {
        let pred = sz_predict_f64(&recon, order);
        let token = read_uvarint(cursor)?;
        if token & 1 == 0 {
            let bits = cursor.read_u64::<LittleEndian>()?;
            recon.push(f64::from_bits(bits));
        } else {
            let q = zigzag_decode(token >> 1);
            let eb_i = match eb_spec {
                EbSpec64::Constant(e) => e,
                EbSpec64::Pointwise(frac) => frac * pred.abs().max(SZ_POINTWISE_MIN_MAG_F64),
            };
            recon.push(pred + (q as f64) * (2.0 * eb_i));
        }
    }
    Ok(recon)
}

/// SZ compression mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SzMode {
    /// Absolute error bound
    Absolute(f64),
    /// Relative error bound (as fraction)
    Relative(f64),
    /// Point-wise relative error bound
    PointWise(f64),
    /// Reversible (lossless)
    Reversible,
}

impl From<FpMode> for SzMode {
    fn from(mode: FpMode) -> Self {
        match mode {
            FpMode::FixedAccuracy(acc) => SzMode::Absolute(acc),
            FpMode::Reversible => SzMode::Reversible,
            _ => SzMode::Absolute(1e-6),
        }
    }
}

/// SZ codec configuration
#[derive(Debug, Clone)]
pub struct SzConfig {
    /// Compression mode
    pub mode: SzMode,

    /// Predictor order (0-3)
    pub predictor_order: usize,
}

impl Default for SzConfig {
    fn default() -> Self {
        Self {
            mode: SzMode::Absolute(1e-6),
            predictor_order: 1,
        }
    }
}

impl SzConfig {
    /// Create configuration with mode
    pub fn with_mode(mode: SzMode) -> Self {
        Self {
            mode,
            ..Default::default()
        }
    }

    /// Set predictor order
    pub fn with_predictor_order(mut self, order: usize) -> Self {
        self.predictor_order = order.min(3);
        self
    }
}

/// SZ compression codec
pub struct SzCodec {
    config: SzConfig,
}

impl SzCodec {
    /// Create a new SZ codec with default configuration
    pub fn new() -> Self {
        Self {
            config: SzConfig::default(),
        }
    }

    /// Create a new SZ codec with custom configuration
    pub fn with_config(config: SzConfig) -> Self {
        Self { config }
    }

    /// Compress f32 array
    pub fn compress_f32(&self, input: &[f32]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        match self.config.mode {
            SzMode::Reversible => self.compress_f32_reversible(input),
            SzMode::Absolute(err) => self.compress_f32_absolute(input, err),
            SzMode::Relative(err) => self.compress_f32_relative(input, err),
            SzMode::PointWise(err) => self.compress_f32_pointwise(input, err),
        }
    }

    /// Decompress f32 array
    pub fn decompress_f32(&self, input: &[u8], len: usize) -> Result<Vec<f32>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        let mut cursor = Cursor::new(input);
        let mode_byte = cursor.read_u8()?;

        match mode_byte {
            0 => self.decompress_f32_reversible(input, len),
            1 => self.decompress_f32_absolute(input, len),
            2 => self.decompress_f32_relative(input, len),
            3 => self.decompress_f32_pointwise(input, len),
            _ => Err(CompressionError::FloatingPointError(format!(
                "Unknown SZ mode: {}",
                mode_byte
            ))),
        }
    }

    /// Compress f64 array
    pub fn compress_f64(&self, input: &[f64]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        match self.config.mode {
            SzMode::Reversible => self.compress_f64_reversible(input),
            SzMode::Absolute(err) => self.compress_f64_absolute(input, err),
            SzMode::Relative(err) => self.compress_f64_relative(input, err),
            SzMode::PointWise(err) => self.compress_f64_pointwise(input, err),
        }
    }

    /// Decompress f64 array
    pub fn decompress_f64(&self, input: &[u8], len: usize) -> Result<Vec<f64>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        let mut cursor = Cursor::new(input);
        let mode_byte = cursor.read_u8()?;

        match mode_byte {
            0 => self.decompress_f64_reversible(input, len),
            1 => self.decompress_f64_absolute(input, len),
            2 => self.decompress_f64_relative(input, len),
            3 => self.decompress_f64_pointwise(input, len),
            _ => Err(CompressionError::FloatingPointError(format!(
                "Unknown SZ mode: {}",
                mode_byte
            ))),
        }
    }

    // Reversible compression for f32
    fn compress_f32_reversible(&self, input: &[f32]) -> Result<Vec<u8>> {
        let mut output = Vec::with_capacity(1 + input.len() * 4);
        output.write_u8(0)?; // Mode marker

        for &val in input {
            output.write_u32::<LittleEndian>(val.to_bits())?;
        }

        Ok(output)
    }

    fn decompress_f32_reversible(&self, input: &[u8], len: usize) -> Result<Vec<f32>> {
        let mut cursor = Cursor::new(input);
        cursor.read_u8()?; // Skip mode marker

        let mut output = Vec::with_capacity(len);

        for _ in 0..len {
            let bits = cursor.read_u32::<LittleEndian>()?;
            output.push(f32::from_bits(bits));
        }

        Ok(output)
    }

    // Absolute error bound compression for f32
    fn compress_f32_absolute(&self, input: &[f32], error_bound: f64) -> Result<Vec<u8>> {
        let order = self.config.predictor_order;
        let eb = error_bound as f32;

        let mut output = Vec::with_capacity(3 + input.len());
        output.write_u8(1)?; // Mode marker
        output.write_u8(order as u8)?; // Predictor order (drives prediction)
        output.write_f32::<LittleEndian>(eb)?;

        sz_encode_body_f32(&mut output, input, order, EbSpec32::Constant(eb));
        Ok(output)
    }

    fn decompress_f32_absolute(&self, input: &[u8], len: usize) -> Result<Vec<f32>> {
        let mut cursor = Cursor::new(input);
        cursor.read_u8()?; // Skip mode marker
        let order = cursor.read_u8()? as usize;
        let eb = cursor.read_f32::<LittleEndian>()?;

        sz_decode_body_f32(&mut cursor, len, order, EbSpec32::Constant(eb))
    }

    // Relative error bound compression for f32
    fn compress_f32_relative(&self, input: &[f32], error_bound: f64) -> Result<Vec<u8>> {
        let order = self.config.predictor_order;
        let eb = error_bound as f32;
        let max_abs = input.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        let eb_abs = eb * max_abs;

        let mut output = Vec::with_capacity(3 + input.len());
        output.write_u8(2)?; // Mode marker
        output.write_u8(order as u8)?;
        output.write_f32::<LittleEndian>(eb)?;
        output.write_f32::<LittleEndian>(max_abs)?;

        sz_encode_body_f32(&mut output, input, order, EbSpec32::Constant(eb_abs));
        Ok(output)
    }

    fn decompress_f32_relative(&self, input: &[u8], len: usize) -> Result<Vec<f32>> {
        let mut cursor = Cursor::new(input);
        cursor.read_u8()?; // Skip mode marker
        let order = cursor.read_u8()? as usize;
        let eb = cursor.read_f32::<LittleEndian>()?;
        let max_abs = cursor.read_f32::<LittleEndian>()?;

        sz_decode_body_f32(&mut cursor, len, order, EbSpec32::Constant(eb * max_abs))
    }

    // Point-wise error bound compression for f32
    fn compress_f32_pointwise(&self, input: &[f32], error_bound: f64) -> Result<Vec<u8>> {
        let order = self.config.predictor_order;
        let eb = error_bound as f32;

        let mut output = Vec::with_capacity(3 + input.len());
        output.write_u8(3)?; // Mode marker
        output.write_u8(order as u8)?;
        output.write_f32::<LittleEndian>(eb)?;

        sz_encode_body_f32(&mut output, input, order, EbSpec32::Pointwise(eb));
        Ok(output)
    }

    fn decompress_f32_pointwise(&self, input: &[u8], len: usize) -> Result<Vec<f32>> {
        let mut cursor = Cursor::new(input);
        cursor.read_u8()?; // Skip mode marker
        let order = cursor.read_u8()? as usize;
        let eb = cursor.read_f32::<LittleEndian>()?;

        sz_decode_body_f32(&mut cursor, len, order, EbSpec32::Pointwise(eb))
    }

    // f64 versions (similar implementations)
    fn compress_f64_reversible(&self, input: &[f64]) -> Result<Vec<u8>> {
        let mut output = Vec::with_capacity(1 + input.len() * 8);
        output.write_u8(0)?;

        for &val in input {
            output.write_u64::<LittleEndian>(val.to_bits())?;
        }

        Ok(output)
    }

    fn decompress_f64_reversible(&self, input: &[u8], len: usize) -> Result<Vec<f64>> {
        let mut cursor = Cursor::new(input);
        cursor.read_u8()?;

        let mut output = Vec::with_capacity(len);

        for _ in 0..len {
            let bits = cursor.read_u64::<LittleEndian>()?;
            output.push(f64::from_bits(bits));
        }

        Ok(output)
    }

    fn compress_f64_absolute(&self, input: &[f64], error_bound: f64) -> Result<Vec<u8>> {
        let order = self.config.predictor_order;

        let mut output = Vec::with_capacity(10 + input.len());
        output.write_u8(1)?;
        output.write_u8(order as u8)?;
        output.write_f64::<LittleEndian>(error_bound)?;

        sz_encode_body_f64(&mut output, input, order, EbSpec64::Constant(error_bound));
        Ok(output)
    }

    fn decompress_f64_absolute(&self, input: &[u8], len: usize) -> Result<Vec<f64>> {
        let mut cursor = Cursor::new(input);
        cursor.read_u8()?;
        let order = cursor.read_u8()? as usize;
        let error_bound = cursor.read_f64::<LittleEndian>()?;

        sz_decode_body_f64(&mut cursor, len, order, EbSpec64::Constant(error_bound))
    }

    fn compress_f64_relative(&self, input: &[f64], error_bound: f64) -> Result<Vec<u8>> {
        let order = self.config.predictor_order;
        let max_abs = input.iter().map(|v| v.abs()).fold(0.0f64, f64::max);
        let eb_abs = error_bound * max_abs;

        let mut output = Vec::with_capacity(18 + input.len());
        output.write_u8(2)?;
        output.write_u8(order as u8)?;
        output.write_f64::<LittleEndian>(error_bound)?;
        output.write_f64::<LittleEndian>(max_abs)?;

        sz_encode_body_f64(&mut output, input, order, EbSpec64::Constant(eb_abs));
        Ok(output)
    }

    fn decompress_f64_relative(&self, input: &[u8], len: usize) -> Result<Vec<f64>> {
        let mut cursor = Cursor::new(input);
        cursor.read_u8()?;
        let order = cursor.read_u8()? as usize;
        let error_bound = cursor.read_f64::<LittleEndian>()?;
        let max_abs = cursor.read_f64::<LittleEndian>()?;

        sz_decode_body_f64(
            &mut cursor,
            len,
            order,
            EbSpec64::Constant(error_bound * max_abs),
        )
    }

    fn compress_f64_pointwise(&self, input: &[f64], error_bound: f64) -> Result<Vec<u8>> {
        let order = self.config.predictor_order;

        let mut output = Vec::with_capacity(10 + input.len());
        output.write_u8(3)?;
        output.write_u8(order as u8)?;
        output.write_f64::<LittleEndian>(error_bound)?;

        sz_encode_body_f64(&mut output, input, order, EbSpec64::Pointwise(error_bound));
        Ok(output)
    }

    fn decompress_f64_pointwise(&self, input: &[u8], len: usize) -> Result<Vec<f64>> {
        let mut cursor = Cursor::new(input);
        cursor.read_u8()?;
        let order = cursor.read_u8()? as usize;
        let error_bound = cursor.read_f64::<LittleEndian>()?;

        sz_decode_body_f64(&mut cursor, len, order, EbSpec64::Pointwise(error_bound))
    }
}

impl Default for SzCodec {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sz_reversible_f32() {
        let config = SzConfig::with_mode(SzMode::Reversible);
        let codec = SzCodec::with_config(config);

        let data: Vec<f32> = (0..100).map(|i| i as f32 * 0.1).collect();

        let compressed = codec.compress_f32(&data).expect("Compression failed");
        let decompressed = codec
            .decompress_f32(&compressed, data.len())
            .expect("Decompression failed");

        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_sz_absolute_f32() {
        let config = SzConfig::with_mode(SzMode::Absolute(0.01));
        let codec = SzCodec::with_config(config);

        let data: Vec<f32> = (0..100).map(|i| i as f32 * 0.1).collect();

        let compressed = codec.compress_f32(&data).expect("Compression failed");
        let decompressed = codec
            .decompress_f32(&compressed, data.len())
            .expect("Decompression failed");

        assert_eq!(decompressed.len(), data.len());

        // Check error bounds
        for (orig, decomp) in data.iter().zip(decompressed.iter()) {
            assert!((orig - decomp).abs() <= 0.02);
        }
    }

    #[test]
    fn test_zigzag_varint_round_trip() {
        for v in [0i64, 1, -1, 5, -5, 32767, -32768, i32::MAX as i64] {
            assert_eq!(zigzag_decode(zigzag_encode(v)), v);
            let mut buf = Vec::new();
            write_uvarint(&mut buf, zigzag_encode(v));
            let mut cur = Cursor::new(buf.as_slice());
            assert_eq!(zigzag_decode(read_uvarint(&mut cur).expect("read")), v);
        }
    }

    #[test]
    fn test_sz_absolute_f32_actually_compresses() {
        // Smooth data with a generous absolute error bound must produce output far
        // below the 4-bytes/value lossless footprint — the defect being fixed.
        let config = SzConfig::with_mode(SzMode::Absolute(0.05)).with_predictor_order(1);
        let codec = SzCodec::with_config(config);

        let data: Vec<f32> = (0..1000).map(|i| i as f32 * 0.1).collect();
        let compressed = codec.compress_f32(&data).expect("compress");

        assert!(
            compressed.len() < data.len() * 4,
            "SZ absolute must compress below the lossless width: {} vs {}",
            compressed.len(),
            data.len() * 4
        );

        let decompressed = codec
            .decompress_f32(&compressed, data.len())
            .expect("decompress");
        for (o, d) in data.iter().zip(decompressed.iter()) {
            assert!((o - d).abs() <= 0.05 + 1e-6);
        }
    }

    #[test]
    fn test_sz_predictor_order_changes_output_size() {
        // On a perfect linear ramp the order-2 Lorenzo predictor is exact, so
        // every residual is zero and the stream is maximally compact — strictly
        // smaller than order-0 (no prediction). This proves predictor_order is
        // actually read and affects the result.
        let data: Vec<f32> = (0..500).map(|i| 3.0 + i as f32 * 2.5).collect();

        let order0 = SzCodec::with_config(
            SzConfig::with_mode(SzMode::Absolute(0.001)).with_predictor_order(0),
        )
        .compress_f32(&data)
        .expect("order0");
        let order2 = SzCodec::with_config(
            SzConfig::with_mode(SzMode::Absolute(0.001)).with_predictor_order(2),
        )
        .compress_f32(&data)
        .expect("order2");

        assert!(
            order2.len() < order0.len(),
            "higher predictor order must shrink linear-ramp output: order2={} order0={}",
            order2.len(),
            order0.len()
        );
    }

    #[test]
    fn test_sz_relative_f32_round_trip_within_bound() {
        let config = SzConfig::with_mode(SzMode::Relative(0.01)).with_predictor_order(2);
        let codec = SzCodec::with_config(config);

        let data: Vec<f32> = (0..300).map(|i| (i as f32 * 0.3).sin() * 100.0).collect();
        let compressed = codec.compress_f32(&data).expect("compress");
        let decompressed = codec
            .decompress_f32(&compressed, data.len())
            .expect("decompress");

        let max_abs = data.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        for (o, d) in data.iter().zip(decompressed.iter()) {
            assert!((o - d).abs() <= 0.01 * max_abs + 1e-4);
        }
    }

    #[test]
    fn test_sz_pointwise_f32_respects_relative_bound() {
        let config = SzConfig::with_mode(SzMode::PointWise(0.01)).with_predictor_order(1);
        let codec = SzCodec::with_config(config);

        // Values comfortably away from zero so the relative bound is meaningful.
        let data: Vec<f32> = (0..200).map(|i| 10.0 + i as f32 * 0.5).collect();
        let compressed = codec.compress_f32(&data).expect("compress");
        let decompressed = codec
            .decompress_f32(&compressed, data.len())
            .expect("decompress");

        for (o, d) in data.iter().zip(decompressed.iter()) {
            assert!(
                (o - d).abs() <= 0.01 * o.abs() + 1e-4,
                "pointwise relative bound violated: {} vs {}",
                o,
                d
            );
        }
    }

    #[test]
    fn test_sz_absolute_f64_round_trip_and_size() {
        let config = SzConfig::with_mode(SzMode::Absolute(1e-3)).with_predictor_order(2);
        let codec = SzCodec::with_config(config);

        let data: Vec<f64> = (0..800).map(|i| i as f64 * 0.01).collect();
        let compressed = codec.compress_f64(&data).expect("compress");
        assert!(compressed.len() < data.len() * 8);

        let decompressed = codec
            .decompress_f64(&compressed, data.len())
            .expect("decompress");
        for (o, d) in data.iter().zip(decompressed.iter()) {
            assert!((o - d).abs() <= 1e-3 + 1e-9);
        }
    }

    #[test]
    fn test_sz_absolute_handles_non_finite_values() {
        // NaN / Inf must be stored verbatim (unpredictable path) and recovered
        // exactly, never corrupted by the quantizer.
        let config = SzConfig::with_mode(SzMode::Absolute(0.1));
        let codec = SzCodec::with_config(config);

        let data: Vec<f32> = vec![1.0, f32::NAN, 2.0, f32::INFINITY, -3.0, f32::NEG_INFINITY];
        let compressed = codec.compress_f32(&data).expect("compress");
        let decompressed = codec
            .decompress_f32(&compressed, data.len())
            .expect("decompress");

        assert!(decompressed[1].is_nan());
        assert_eq!(decompressed[3], f32::INFINITY);
        assert_eq!(decompressed[5], f32::NEG_INFINITY);
        assert!((data[0] - decompressed[0]).abs() <= 0.1);
        assert!((data[2] - decompressed[2]).abs() <= 0.1);
    }
}
