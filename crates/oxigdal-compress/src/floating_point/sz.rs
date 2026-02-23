//! SZ-style floating-point compression
//!
//! SZ is an error-bounded lossy compression algorithm for scientific data.
//! This provides a simplified implementation with configurable error bounds.

use super::FpMode;
use crate::error::{CompressionError, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;

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
        let mut output = Vec::new();
        output.write_u8(1)?; // Mode marker
        output.write_f32::<LittleEndian>(error_bound as f32)?;

        let quant_factor = 1.0 / (2.0 * error_bound as f32);

        for &val in input {
            let quantized = (val * quant_factor).round() as i32;
            output.write_i32::<LittleEndian>(quantized)?;
        }

        Ok(output)
    }

    fn decompress_f32_absolute(&self, input: &[u8], len: usize) -> Result<Vec<f32>> {
        let mut cursor = Cursor::new(input);
        cursor.read_u8()?; // Skip mode marker

        let error_bound = cursor.read_f32::<LittleEndian>()?;
        let dequant_factor = 2.0 * error_bound;

        let mut output = Vec::with_capacity(len);

        for _ in 0..len {
            let quantized = cursor.read_i32::<LittleEndian>()?;
            output.push(quantized as f32 * dequant_factor);
        }

        Ok(output)
    }

    // Relative error bound compression for f32
    fn compress_f32_relative(&self, input: &[f32], error_bound: f64) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        output.write_u8(2)?; // Mode marker
        output.write_f32::<LittleEndian>(error_bound as f32)?;

        let max_abs = input.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        let quant_factor = 1.0 / (2.0 * error_bound as f32 * max_abs);

        output.write_f32::<LittleEndian>(max_abs)?;

        for &val in input {
            let quantized = (val * quant_factor).round() as i32;
            output.write_i32::<LittleEndian>(quantized)?;
        }

        Ok(output)
    }

    fn decompress_f32_relative(&self, input: &[u8], len: usize) -> Result<Vec<f32>> {
        let mut cursor = Cursor::new(input);
        cursor.read_u8()?; // Skip mode marker

        let error_bound = cursor.read_f32::<LittleEndian>()?;
        let max_abs = cursor.read_f32::<LittleEndian>()?;
        let dequant_factor = 2.0 * error_bound * max_abs;

        let mut output = Vec::with_capacity(len);

        for _ in 0..len {
            let quantized = cursor.read_i32::<LittleEndian>()?;
            output.push(quantized as f32 * dequant_factor);
        }

        Ok(output)
    }

    // Point-wise error bound compression for f32
    fn compress_f32_pointwise(&self, input: &[f32], error_bound: f64) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        output.write_u8(3)?; // Mode marker
        output.write_f32::<LittleEndian>(error_bound as f32)?;

        for &val in input {
            let quant_factor = 1.0 / (2.0 * error_bound as f32 * val.abs().max(1e-10));
            let quantized = (val * quant_factor).round() as i32;
            output.write_i32::<LittleEndian>(quantized)?;
        }

        Ok(output)
    }

    fn decompress_f32_pointwise(&self, input: &[u8], len: usize) -> Result<Vec<f32>> {
        let mut cursor = Cursor::new(input);
        cursor.read_u8()?; // Skip mode marker

        let error_bound = cursor.read_f32::<LittleEndian>()?;

        let mut output = Vec::with_capacity(len);

        for _ in 0..len {
            let quantized = cursor.read_i32::<LittleEndian>()?;
            let val_approx = quantized as f32;
            let dequant_factor = 2.0 * error_bound * val_approx.abs().max(1e-10);
            output.push(val_approx * dequant_factor);
        }

        Ok(output)
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
        let mut output = Vec::new();
        output.write_u8(1)?;
        output.write_f64::<LittleEndian>(error_bound)?;

        let quant_factor = 1.0 / (2.0 * error_bound);

        for &val in input {
            let quantized = (val * quant_factor).round() as i64;
            output.write_i64::<LittleEndian>(quantized)?;
        }

        Ok(output)
    }

    fn decompress_f64_absolute(&self, input: &[u8], len: usize) -> Result<Vec<f64>> {
        let mut cursor = Cursor::new(input);
        cursor.read_u8()?;

        let error_bound = cursor.read_f64::<LittleEndian>()?;
        let dequant_factor = 2.0 * error_bound;

        let mut output = Vec::with_capacity(len);

        for _ in 0..len {
            let quantized = cursor.read_i64::<LittleEndian>()?;
            output.push(quantized as f64 * dequant_factor);
        }

        Ok(output)
    }

    fn compress_f64_relative(&self, input: &[f64], error_bound: f64) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        output.write_u8(2)?;
        output.write_f64::<LittleEndian>(error_bound)?;

        let max_abs = input.iter().map(|v| v.abs()).fold(0.0f64, f64::max);
        let quant_factor = 1.0 / (2.0 * error_bound * max_abs);

        output.write_f64::<LittleEndian>(max_abs)?;

        for &val in input {
            let quantized = (val * quant_factor).round() as i64;
            output.write_i64::<LittleEndian>(quantized)?;
        }

        Ok(output)
    }

    fn decompress_f64_relative(&self, input: &[u8], len: usize) -> Result<Vec<f64>> {
        let mut cursor = Cursor::new(input);
        cursor.read_u8()?;

        let error_bound = cursor.read_f64::<LittleEndian>()?;
        let max_abs = cursor.read_f64::<LittleEndian>()?;
        let dequant_factor = 2.0 * error_bound * max_abs;

        let mut output = Vec::with_capacity(len);

        for _ in 0..len {
            let quantized = cursor.read_i64::<LittleEndian>()?;
            output.push(quantized as f64 * dequant_factor);
        }

        Ok(output)
    }

    fn compress_f64_pointwise(&self, input: &[f64], error_bound: f64) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        output.write_u8(3)?;
        output.write_f64::<LittleEndian>(error_bound)?;

        for &val in input {
            let quant_factor = 1.0 / (2.0 * error_bound * val.abs().max(1e-10));
            let quantized = (val * quant_factor).round() as i64;
            output.write_i64::<LittleEndian>(quantized)?;
        }

        Ok(output)
    }

    fn decompress_f64_pointwise(&self, input: &[u8], len: usize) -> Result<Vec<f64>> {
        let mut cursor = Cursor::new(input);
        cursor.read_u8()?;

        let error_bound = cursor.read_f64::<LittleEndian>()?;

        let mut output = Vec::with_capacity(len);

        for _ in 0..len {
            let quantized = cursor.read_i64::<LittleEndian>()?;
            let val_approx = quantized as f64;
            let dequant_factor = 2.0 * error_bound * val_approx.abs().max(1e-10);
            output.push(val_approx * dequant_factor);
        }

        Ok(output)
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
}
