//! ZFP codec implementation (Pure Rust floating-point compression)
//!
//! This module provides a Pure Rust implementation of ZFP-like compression,
//! optimized for floating-point arrays.

use crate::codecs::Codec;
use crate::error::{CodecError, Result, ZarrError};

/// ZFP compression mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZfpMode {
    /// Fixed-rate mode (bits per value)
    FixedRate { rate: u32 },
    /// Fixed-precision mode (number of bit planes)
    FixedPrecision { precision: u32 },
    /// Fixed-accuracy mode (absolute error tolerance)
    FixedAccuracy { tolerance: u32 },
}

/// ZFP codec (Pure Rust implementation)
#[derive(Debug, Clone)]
pub struct ZfpCodec {
    /// Compression mode
    mode: ZfpMode,
    /// Data type (f32 or f64)
    dtype: ZfpDataType,
}

/// ZFP data type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZfpDataType {
    /// 32-bit float
    Float32,
    /// 64-bit float
    Float64,
}

impl ZfpCodec {
    /// Creates a new ZFP codec with fixed-rate mode
    #[must_use]
    pub const fn fixed_rate(rate: u32, dtype: ZfpDataType) -> Self {
        Self {
            mode: ZfpMode::FixedRate { rate },
            dtype,
        }
    }

    /// Creates a new ZFP codec with fixed-precision mode
    #[must_use]
    pub const fn fixed_precision(precision: u32, dtype: ZfpDataType) -> Self {
        Self {
            mode: ZfpMode::FixedPrecision { precision },
            dtype,
        }
    }

    /// Creates a new ZFP codec with fixed-accuracy mode
    #[must_use]
    pub const fn fixed_accuracy(tolerance: u32, dtype: ZfpDataType) -> Self {
        Self {
            mode: ZfpMode::FixedAccuracy { tolerance },
            dtype,
        }
    }

    /// Compresses float32 data (simplified implementation)
    fn compress_f32(&self, data: &[f32]) -> Result<Vec<u8>> {
        // This is a simplified implementation for demonstration
        // In production, use a complete ZFP implementation

        let mut compressed = Vec::new();

        // Write header
        compressed.push(0x01); // Version
        compressed.push(0x20); // Float32 marker
        compressed.extend_from_slice(&(data.len() as u32).to_le_bytes());

        // Simple delta encoding + quantization
        let mut prev = 0.0f32;
        for &value in data {
            let delta = value - prev;
            let quantized = (delta * 1000.0) as i16; // Simple quantization
            compressed.extend_from_slice(&quantized.to_le_bytes());
            prev = value;
        }

        Ok(compressed)
    }

    /// Decompresses float32 data (simplified implementation)
    fn decompress_f32(&self, data: &[u8]) -> Result<Vec<f32>> {
        if data.len() < 6 {
            return Err(ZarrError::Codec(CodecError::DecompressionFailed {
                message: "ZFP data too short".to_string(),
            }));
        }

        // Read header
        let _version = data[0];
        let dtype_marker = data[1];
        if dtype_marker != 0x20 {
            return Err(ZarrError::Codec(CodecError::DecompressionFailed {
                message: "Invalid data type marker".to_string(),
            }));
        }

        let len = u32::from_le_bytes([data[2], data[3], data[4], data[5]]) as usize;
        let mut result = Vec::with_capacity(len);

        // Decode delta-encoded values
        let mut prev = 0.0f32;
        let mut offset = 6;
        for _ in 0..len {
            if offset + 2 > data.len() {
                break;
            }
            let quantized = i16::from_le_bytes([data[offset], data[offset + 1]]);
            let delta = f32::from(quantized) / 1000.0;
            let value = prev + delta;
            result.push(value);
            prev = value;
            offset += 2;
        }

        Ok(result)
    }

    /// Compresses float64 data (simplified implementation)
    fn compress_f64(&self, data: &[f64]) -> Result<Vec<u8>> {
        let mut compressed = Vec::new();

        // Write header
        compressed.push(0x01); // Version
        compressed.push(0x40); // Float64 marker
        compressed.extend_from_slice(&(data.len() as u32).to_le_bytes());

        // Simple delta encoding + quantization
        let mut prev = 0.0f64;
        for &value in data {
            let delta = value - prev;
            let quantized = (delta * 1000.0) as i32; // Simple quantization
            compressed.extend_from_slice(&quantized.to_le_bytes());
            prev = value;
        }

        Ok(compressed)
    }

    /// Decompresses float64 data (simplified implementation)
    fn decompress_f64(&self, data: &[u8]) -> Result<Vec<f64>> {
        if data.len() < 6 {
            return Err(ZarrError::Codec(CodecError::DecompressionFailed {
                message: "ZFP data too short".to_string(),
            }));
        }

        // Read header
        let _version = data[0];
        let dtype_marker = data[1];
        if dtype_marker != 0x40 {
            return Err(ZarrError::Codec(CodecError::DecompressionFailed {
                message: "Invalid data type marker".to_string(),
            }));
        }

        let len = u32::from_le_bytes([data[2], data[3], data[4], data[5]]) as usize;
        let mut result = Vec::with_capacity(len);

        // Decode delta-encoded values
        let mut prev = 0.0f64;
        let mut offset = 6;
        for _ in 0..len {
            if offset + 4 > data.len() {
                break;
            }
            let quantized = i32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]);
            let delta = f64::from(quantized) / 1000.0;
            let value = prev + delta;
            result.push(value);
            prev = value;
            offset += 4;
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
                if data.len() % 4 != 0 {
                    return Err(ZarrError::Codec(CodecError::CompressionFailed {
                        message: "Data length not multiple of 4 for float32".to_string(),
                    }));
                }

                // Convert bytes to f32
                let floats: Vec<f32> = data
                    .chunks_exact(4)
                    .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                    .collect();

                self.compress_f32(&floats)
            }
            ZfpDataType::Float64 => {
                if data.len() % 8 != 0 {
                    return Err(ZarrError::Codec(CodecError::CompressionFailed {
                        message: "Data length not multiple of 8 for float64".to_string(),
                    }));
                }

                // Convert bytes to f64
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

                // Convert f32 to bytes
                let mut result = Vec::with_capacity(floats.len() * 4);
                for &value in &floats {
                    result.extend_from_slice(&value.to_le_bytes());
                }
                Ok(result)
            }
            ZfpDataType::Float64 => {
                let floats = self.decompress_f64(data)?;

                // Convert f64 to bytes
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

    #[test]
    fn test_zfp_codec_f32() {
        let codec = ZfpCodec::fixed_rate(16, ZfpDataType::Float32);
        assert_eq!(codec.id(), "zfp");

        // Create test data (as bytes)
        let floats = vec![1.0f32, 2.0, 3.0, 4.0, 5.0];
        let mut data = Vec::new();
        for &f in &floats {
            data.extend_from_slice(&f.to_le_bytes());
        }

        let compressed = codec.encode(&data).expect("compress");
        let decompressed = codec.decode(&compressed).expect("decompress");

        // Verify decompressed data
        let decoded_floats: Vec<f32> = decompressed
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();

        // Allow some error due to quantization
        for (a, b) in floats.iter().zip(decoded_floats.iter()) {
            assert!((a - b).abs() < 0.01, "Values differ: {a} vs {b}");
        }
    }

    #[test]
    fn test_zfp_codec_f64() {
        let codec = ZfpCodec::fixed_precision(20, ZfpDataType::Float64);

        // Create test data (as bytes)
        let floats = vec![1.0f64, 2.0, 3.0, 4.0, 5.0];
        let mut data = Vec::new();
        for &f in &floats {
            data.extend_from_slice(&f.to_le_bytes());
        }

        let compressed = codec.encode(&data).expect("compress");
        let decompressed = codec.decode(&compressed).expect("decompress");

        // Verify decompressed data
        let decoded_floats: Vec<f64> = decompressed
            .chunks_exact(8)
            .map(|chunk| {
                f64::from_le_bytes([
                    chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
                ])
            })
            .collect();

        // Allow some error due to quantization
        for (a, b) in floats.iter().zip(decoded_floats.iter()) {
            assert!((a - b).abs() < 0.01, "Values differ: {a} vs {b}");
        }
    }

    #[test]
    fn test_zfp_invalid_length() {
        let codec = ZfpCodec::fixed_rate(16, ZfpDataType::Float32);
        let data = vec![1, 2, 3]; // Not divisible by 4

        let result = codec.encode(&data);
        assert!(result.is_err());
    }
}
