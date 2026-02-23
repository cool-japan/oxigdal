//! ZFP-style floating-point compression
//!
//! ZFP is a compressed format for arrays of floating-point data.
//! This implementation provides similar functionality with configurable
//! compression modes.

use super::FpMode;
use crate::error::Result;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;

/// ZFP compression mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZfpMode {
    /// Fixed rate mode (bits per value)
    FixedRate(usize),
    /// Fixed precision mode (bit planes)
    FixedPrecision(usize),
    /// Fixed accuracy mode (error bound)
    FixedAccuracy(f64),
    /// Reversible mode (lossless)
    Reversible,
}

impl From<FpMode> for ZfpMode {
    fn from(mode: FpMode) -> Self {
        match mode {
            FpMode::FixedRate(rate) => ZfpMode::FixedRate(rate),
            FpMode::FixedPrecision(prec) => ZfpMode::FixedPrecision(prec),
            FpMode::FixedAccuracy(acc) => ZfpMode::FixedAccuracy(acc),
            FpMode::Reversible => ZfpMode::Reversible,
        }
    }
}

/// ZFP codec configuration
#[derive(Debug, Clone)]
pub struct ZfpConfig {
    /// Compression mode
    pub mode: ZfpMode,

    /// Block size (must be power of 2)
    pub block_size: usize,
}

impl Default for ZfpConfig {
    fn default() -> Self {
        Self {
            mode: ZfpMode::FixedRate(16),
            block_size: 4,
        }
    }
}

impl ZfpConfig {
    /// Create configuration with mode
    pub fn with_mode(mode: ZfpMode) -> Self {
        Self {
            mode,
            ..Default::default()
        }
    }

    /// Set block size
    pub fn with_block_size(mut self, size: usize) -> Self {
        self.block_size = size;
        self
    }
}

/// ZFP compression codec
pub struct ZfpCodec {
    config: ZfpConfig,
}

impl ZfpCodec {
    /// Create a new ZFP codec with default configuration
    pub fn new() -> Self {
        Self {
            config: ZfpConfig::default(),
        }
    }

    /// Create a new ZFP codec with custom configuration
    pub fn with_config(config: ZfpConfig) -> Self {
        Self { config }
    }

    /// Compress f32 array
    pub fn compress_f32(&self, input: &[f32]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        match self.config.mode {
            ZfpMode::Reversible => self.compress_f32_reversible(input),
            ZfpMode::FixedRate(rate) => self.compress_f32_fixed_rate(input, rate),
            ZfpMode::FixedPrecision(prec) => self.compress_f32_fixed_precision(input, prec),
            ZfpMode::FixedAccuracy(acc) => self.compress_f32_fixed_accuracy(input, acc),
        }
    }

    /// Decompress f32 array
    pub fn decompress_f32(&self, input: &[u8], len: usize) -> Result<Vec<f32>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        match self.config.mode {
            ZfpMode::Reversible => self.decompress_f32_reversible(input, len),
            ZfpMode::FixedRate(rate) => self.decompress_f32_fixed_rate(input, len, rate),
            ZfpMode::FixedPrecision(prec) => self.decompress_f32_fixed_precision(input, len, prec),
            ZfpMode::FixedAccuracy(acc) => self.decompress_f32_fixed_accuracy(input, len, acc),
        }
    }

    /// Compress f64 array
    pub fn compress_f64(&self, input: &[f64]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        match self.config.mode {
            ZfpMode::Reversible => self.compress_f64_reversible(input),
            ZfpMode::FixedRate(rate) => self.compress_f64_fixed_rate(input, rate),
            ZfpMode::FixedPrecision(prec) => self.compress_f64_fixed_precision(input, prec),
            ZfpMode::FixedAccuracy(acc) => self.compress_f64_fixed_accuracy(input, acc),
        }
    }

    /// Decompress f64 array
    pub fn decompress_f64(&self, input: &[u8], len: usize) -> Result<Vec<f64>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        match self.config.mode {
            ZfpMode::Reversible => self.decompress_f64_reversible(input, len),
            ZfpMode::FixedRate(rate) => self.decompress_f64_fixed_rate(input, len, rate),
            ZfpMode::FixedPrecision(prec) => self.decompress_f64_fixed_precision(input, len, prec),
            ZfpMode::FixedAccuracy(acc) => self.decompress_f64_fixed_accuracy(input, len, acc),
        }
    }

    // Reversible (lossless) compression for f32
    fn compress_f32_reversible(&self, input: &[f32]) -> Result<Vec<u8>> {
        let mut output = Vec::with_capacity(input.len() * 4);

        for &val in input {
            output.write_u32::<LittleEndian>(val.to_bits())?;
        }

        Ok(output)
    }

    fn decompress_f32_reversible(&self, input: &[u8], len: usize) -> Result<Vec<f32>> {
        let mut cursor = Cursor::new(input);
        let mut output = Vec::with_capacity(len);

        for _ in 0..len {
            let bits = cursor.read_u32::<LittleEndian>()?;
            output.push(f32::from_bits(bits));
        }

        Ok(output)
    }

    // Fixed-rate compression for f32
    fn compress_f32_fixed_rate(&self, input: &[f32], bits_per_value: usize) -> Result<Vec<u8>> {
        let mut output = Vec::new();

        // Store metadata
        output.write_u32::<LittleEndian>(input.len() as u32)?;
        output.write_u32::<LittleEndian>(bits_per_value as u32)?;

        // Simple quantization based on bit budget
        let range = Self::compute_range_f32(input);
        let levels = 1u64 << bits_per_value;
        let scale = (levels - 1) as f32 / range.1;

        output.write_f32::<LittleEndian>(range.0)?; // min
        output.write_f32::<LittleEndian>(scale)?; // scale

        // Quantize and encode
        for &val in input {
            let quantized = ((val - range.0) * scale) as u32;
            output.write_u32::<LittleEndian>(quantized)?;
        }

        Ok(output)
    }

    fn decompress_f32_fixed_rate(
        &self,
        input: &[u8],
        len: usize,
        _bits_per_value: usize,
    ) -> Result<Vec<f32>> {
        let mut cursor = Cursor::new(input);

        let _stored_len = cursor.read_u32::<LittleEndian>()?;
        let _stored_bits = cursor.read_u32::<LittleEndian>()?;

        let min = cursor.read_f32::<LittleEndian>()?;
        let scale = cursor.read_f32::<LittleEndian>()?;

        let mut output = Vec::with_capacity(len);

        for _ in 0..len {
            let quantized = cursor.read_u32::<LittleEndian>()?;
            let val = min + (quantized as f32 / scale);
            output.push(val);
        }

        Ok(output)
    }

    // Fixed-precision compression for f32 (simplified)
    fn compress_f32_fixed_precision(&self, input: &[f32], precision: usize) -> Result<Vec<u8>> {
        // Use fixed-rate with precision-based bit count
        let bits = (precision + 8).min(32);
        self.compress_f32_fixed_rate(input, bits)
    }

    fn decompress_f32_fixed_precision(
        &self,
        input: &[u8],
        len: usize,
        precision: usize,
    ) -> Result<Vec<f32>> {
        let bits = (precision + 8).min(32);
        self.decompress_f32_fixed_rate(input, len, bits)
    }

    // Fixed-accuracy compression for f32 (simplified)
    fn compress_f32_fixed_accuracy(&self, input: &[f32], accuracy: f64) -> Result<Vec<u8>> {
        let range = Self::compute_range_f32(input);
        let levels = (range.1 / accuracy as f32).ceil() as u64;
        let bits = (levels as f64).log2().ceil() as usize;
        let bits = bits.clamp(4, 32);
        self.compress_f32_fixed_rate(input, bits)
    }

    fn decompress_f32_fixed_accuracy(
        &self,
        input: &[u8],
        len: usize,
        _accuracy: f64,
    ) -> Result<Vec<f32>> {
        // Determine bits from stored metadata
        let mut cursor = Cursor::new(input);
        let _stored_len = cursor.read_u32::<LittleEndian>()?;
        let bits = cursor.read_u32::<LittleEndian>()? as usize;

        self.decompress_f32_fixed_rate(input, len, bits)
    }

    // f64 versions (similar implementations)
    fn compress_f64_reversible(&self, input: &[f64]) -> Result<Vec<u8>> {
        let mut output = Vec::with_capacity(input.len() * 8);

        for &val in input {
            output.write_u64::<LittleEndian>(val.to_bits())?;
        }

        Ok(output)
    }

    fn decompress_f64_reversible(&self, input: &[u8], len: usize) -> Result<Vec<f64>> {
        let mut cursor = Cursor::new(input);
        let mut output = Vec::with_capacity(len);

        for _ in 0..len {
            let bits = cursor.read_u64::<LittleEndian>()?;
            output.push(f64::from_bits(bits));
        }

        Ok(output)
    }

    fn compress_f64_fixed_rate(&self, input: &[f64], bits_per_value: usize) -> Result<Vec<u8>> {
        let mut output = Vec::new();

        output.write_u32::<LittleEndian>(input.len() as u32)?;
        output.write_u32::<LittleEndian>(bits_per_value as u32)?;

        let range = Self::compute_range_f64(input);
        let levels = 1u64 << bits_per_value.min(63);
        let scale = (levels - 1) as f64 / range.1;

        output.write_f64::<LittleEndian>(range.0)?;
        output.write_f64::<LittleEndian>(scale)?;

        for &val in input {
            let quantized = ((val - range.0) * scale) as u64;
            output.write_u64::<LittleEndian>(quantized)?;
        }

        Ok(output)
    }

    fn decompress_f64_fixed_rate(
        &self,
        input: &[u8],
        len: usize,
        _bits_per_value: usize,
    ) -> Result<Vec<f64>> {
        let mut cursor = Cursor::new(input);

        let _stored_len = cursor.read_u32::<LittleEndian>()?;
        let _stored_bits = cursor.read_u32::<LittleEndian>()?;

        let min = cursor.read_f64::<LittleEndian>()?;
        let scale = cursor.read_f64::<LittleEndian>()?;

        let mut output = Vec::with_capacity(len);

        for _ in 0..len {
            let quantized = cursor.read_u64::<LittleEndian>()?;
            let val = min + (quantized as f64 / scale);
            output.push(val);
        }

        Ok(output)
    }

    fn compress_f64_fixed_precision(&self, input: &[f64], precision: usize) -> Result<Vec<u8>> {
        let bits = (precision + 11).min(64);
        self.compress_f64_fixed_rate(input, bits)
    }

    fn decompress_f64_fixed_precision(
        &self,
        input: &[u8],
        len: usize,
        precision: usize,
    ) -> Result<Vec<f64>> {
        let bits = (precision + 11).min(64);
        self.decompress_f64_fixed_rate(input, len, bits)
    }

    fn compress_f64_fixed_accuracy(&self, input: &[f64], accuracy: f64) -> Result<Vec<u8>> {
        let range = Self::compute_range_f64(input);
        let levels = (range.1 / accuracy).ceil() as u64;
        let bits = (levels as f64).log2().ceil() as usize;
        let bits = bits.clamp(8, 64);
        self.compress_f64_fixed_rate(input, bits)
    }

    fn decompress_f64_fixed_accuracy(
        &self,
        input: &[u8],
        len: usize,
        _accuracy: f64,
    ) -> Result<Vec<f64>> {
        let mut cursor = Cursor::new(input);
        let _stored_len = cursor.read_u32::<LittleEndian>()?;
        let bits = cursor.read_u32::<LittleEndian>()? as usize;

        self.decompress_f64_fixed_rate(input, len, bits)
    }

    // Helper: compute data range for f32
    fn compute_range_f32(data: &[f32]) -> (f32, f32) {
        if data.is_empty() {
            return (0.0, 0.0);
        }

        let mut min = data[0];
        let mut max = data[0];

        for &val in data {
            min = min.min(val);
            max = max.max(val);
        }

        let range = max - min;
        (min, range)
    }

    // Helper: compute data range for f64
    fn compute_range_f64(data: &[f64]) -> (f64, f64) {
        if data.is_empty() {
            return (0.0, 0.0);
        }

        let mut min = data[0];
        let mut max = data[0];

        for &val in data {
            min = min.min(val);
            max = max.max(val);
        }

        let range = max - min;
        (min, range)
    }
}

impl Default for ZfpCodec {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zfp_reversible_f32() {
        let config = ZfpConfig::with_mode(ZfpMode::Reversible);
        let codec = ZfpCodec::with_config(config);

        let data: Vec<f32> = (0..100).map(|i| i as f32 * 0.1).collect();

        let compressed = codec.compress_f32(&data).expect("Compression failed");
        let decompressed = codec
            .decompress_f32(&compressed, data.len())
            .expect("Decompression failed");

        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_zfp_fixed_rate_f32() {
        let config = ZfpConfig::with_mode(ZfpMode::FixedRate(16));
        let codec = ZfpCodec::with_config(config);

        let data: Vec<f32> = (0..100).map(|i| i as f32 * 0.1).collect();

        let compressed = codec.compress_f32(&data).expect("Compression failed");
        let decompressed = codec
            .decompress_f32(&compressed, data.len())
            .expect("Decompression failed");

        assert_eq!(decompressed.len(), data.len());
    }
}
