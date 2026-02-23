//! Delta encoding codec
//!
//! Delta encoding stores differences between consecutive values rather than the
//! values themselves. This is particularly effective for coordinate data and
//! time series where consecutive values are often similar.

use crate::error::{CompressionError, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;

/// Delta codec data type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeltaDataType {
    /// Signed 8-bit integers
    I8,
    /// Signed 16-bit integers
    I16,
    /// Signed 32-bit integers
    I32,
    /// Signed 64-bit integers
    I64,
    /// Unsigned 8-bit integers
    U8,
    /// Unsigned 16-bit integers
    U16,
    /// Unsigned 32-bit integers
    U32,
    /// Unsigned 64-bit integers
    U64,
    /// 32-bit floating point
    F32,
    /// 64-bit floating point
    F64,
}

impl DeltaDataType {
    /// Get the size of the data type in bytes
    pub fn size(&self) -> usize {
        match self {
            DeltaDataType::I8 | DeltaDataType::U8 => 1,
            DeltaDataType::I16 | DeltaDataType::U16 => 2,
            DeltaDataType::I32 | DeltaDataType::U32 | DeltaDataType::F32 => 4,
            DeltaDataType::I64 | DeltaDataType::U64 | DeltaDataType::F64 => 8,
        }
    }
}

/// Delta codec configuration
#[derive(Debug, Clone)]
pub struct DeltaConfig {
    /// Data type
    pub data_type: DeltaDataType,

    /// Order of delta encoding (1 = first-order, 2 = second-order, etc.)
    pub order: usize,
}

impl Default for DeltaConfig {
    fn default() -> Self {
        Self {
            data_type: DeltaDataType::I32,
            order: 1,
        }
    }
}

impl DeltaConfig {
    /// Create new configuration with specified data type
    pub fn with_data_type(data_type: DeltaDataType) -> Self {
        Self {
            data_type,
            ..Default::default()
        }
    }

    /// Set delta order
    pub fn with_order(mut self, order: usize) -> Self {
        self.order = order;
        self
    }
}

/// Delta compression codec
pub struct DeltaCodec {
    config: DeltaConfig,
}

impl DeltaCodec {
    /// Create a new Delta codec with default configuration
    pub fn new() -> Self {
        Self {
            config: DeltaConfig::default(),
        }
    }

    /// Create a new Delta codec with custom configuration
    pub fn with_config(config: DeltaConfig) -> Self {
        Self { config }
    }

    /// Compress data using delta encoding
    pub fn compress(&self, input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        match self.config.data_type {
            DeltaDataType::I32 => self.compress_i32(input),
            DeltaDataType::I64 => self.compress_i64(input),
            DeltaDataType::F32 => self.compress_f32(input),
            DeltaDataType::F64 => self.compress_f64(input),
            _ => Err(CompressionError::UnsupportedDataType(format!(
                "Delta encoding not implemented for {:?}",
                self.config.data_type
            ))),
        }
    }

    /// Decompress delta encoded data
    pub fn decompress(&self, input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        match self.config.data_type {
            DeltaDataType::I32 => self.decompress_i32(input),
            DeltaDataType::I64 => self.decompress_i64(input),
            DeltaDataType::F32 => self.decompress_f32(input),
            DeltaDataType::F64 => self.decompress_f64(input),
            _ => Err(CompressionError::UnsupportedDataType(format!(
                "Delta decoding not implemented for {:?}",
                self.config.data_type
            ))),
        }
    }

    /// Compress i32 values
    fn compress_i32(&self, input: &[u8]) -> Result<Vec<u8>> {
        let mut cursor = Cursor::new(input);
        let mut values = Vec::new();

        while let Ok(value) = cursor.read_i32::<LittleEndian>() {
            values.push(value);
        }

        if values.is_empty() {
            return Ok(Vec::new());
        }

        let mut output = Vec::with_capacity(values.len() * 4);

        // Write first value
        output.write_i32::<LittleEndian>(values[0])?;

        // Write deltas
        for i in 1..values.len() {
            let delta = values[i].wrapping_sub(values[i - 1]);
            output.write_i32::<LittleEndian>(delta)?;
        }

        Ok(output)
    }

    /// Decompress i32 values
    fn decompress_i32(&self, input: &[u8]) -> Result<Vec<u8>> {
        let mut cursor = Cursor::new(input);
        let mut values = Vec::new();

        // Read first value
        let first = cursor.read_i32::<LittleEndian>()?;
        values.push(first);

        // Read and apply deltas
        while let Ok(delta) = cursor.read_i32::<LittleEndian>() {
            let last = *values
                .last()
                .ok_or_else(|| CompressionError::DeltaError("Empty values vector".to_string()))?;
            values.push(last.wrapping_add(delta));
        }

        let mut output = Vec::with_capacity(values.len() * 4);
        for value in values {
            output.write_i32::<LittleEndian>(value)?;
        }

        Ok(output)
    }

    /// Compress i64 values
    fn compress_i64(&self, input: &[u8]) -> Result<Vec<u8>> {
        let mut cursor = Cursor::new(input);
        let mut values = Vec::new();

        while let Ok(value) = cursor.read_i64::<LittleEndian>() {
            values.push(value);
        }

        if values.is_empty() {
            return Ok(Vec::new());
        }

        let mut output = Vec::with_capacity(values.len() * 8);

        output.write_i64::<LittleEndian>(values[0])?;

        for i in 1..values.len() {
            let delta = values[i].wrapping_sub(values[i - 1]);
            output.write_i64::<LittleEndian>(delta)?;
        }

        Ok(output)
    }

    /// Decompress i64 values
    fn decompress_i64(&self, input: &[u8]) -> Result<Vec<u8>> {
        let mut cursor = Cursor::new(input);
        let mut values = Vec::new();

        let first = cursor.read_i64::<LittleEndian>()?;
        values.push(first);

        while let Ok(delta) = cursor.read_i64::<LittleEndian>() {
            let last = *values
                .last()
                .ok_or_else(|| CompressionError::DeltaError("Empty values vector".to_string()))?;
            values.push(last.wrapping_add(delta));
        }

        let mut output = Vec::with_capacity(values.len() * 8);
        for value in values {
            output.write_i64::<LittleEndian>(value)?;
        }

        Ok(output)
    }

    /// Compress f32 values (as bit patterns)
    fn compress_f32(&self, input: &[u8]) -> Result<Vec<u8>> {
        let mut cursor = Cursor::new(input);
        let mut values = Vec::new();

        while let Ok(value) = cursor.read_f32::<LittleEndian>() {
            values.push(value.to_bits() as i32);
        }

        if values.is_empty() {
            return Ok(Vec::new());
        }

        let mut output = Vec::with_capacity(values.len() * 4);

        output.write_i32::<LittleEndian>(values[0])?;

        for i in 1..values.len() {
            let delta = values[i].wrapping_sub(values[i - 1]);
            output.write_i32::<LittleEndian>(delta)?;
        }

        Ok(output)
    }

    /// Decompress f32 values
    fn decompress_f32(&self, input: &[u8]) -> Result<Vec<u8>> {
        let mut cursor = Cursor::new(input);
        let mut values = Vec::new();

        let first = cursor.read_i32::<LittleEndian>()?;
        values.push(first);

        while let Ok(delta) = cursor.read_i32::<LittleEndian>() {
            let last = *values
                .last()
                .ok_or_else(|| CompressionError::DeltaError("Empty values vector".to_string()))?;
            values.push(last.wrapping_add(delta));
        }

        let mut output = Vec::with_capacity(values.len() * 4);
        for value in values {
            output.write_f32::<LittleEndian>(f32::from_bits(value as u32))?;
        }

        Ok(output)
    }

    /// Compress f64 values (as bit patterns)
    fn compress_f64(&self, input: &[u8]) -> Result<Vec<u8>> {
        let mut cursor = Cursor::new(input);
        let mut values = Vec::new();

        while let Ok(value) = cursor.read_f64::<LittleEndian>() {
            values.push(value.to_bits() as i64);
        }

        if values.is_empty() {
            return Ok(Vec::new());
        }

        let mut output = Vec::with_capacity(values.len() * 8);

        output.write_i64::<LittleEndian>(values[0])?;

        for i in 1..values.len() {
            let delta = values[i].wrapping_sub(values[i - 1]);
            output.write_i64::<LittleEndian>(delta)?;
        }

        Ok(output)
    }

    /// Decompress f64 values
    fn decompress_f64(&self, input: &[u8]) -> Result<Vec<u8>> {
        let mut cursor = Cursor::new(input);
        let mut values = Vec::new();

        let first = cursor.read_i64::<LittleEndian>()?;
        values.push(first);

        while let Ok(delta) = cursor.read_i64::<LittleEndian>() {
            let last = *values
                .last()
                .ok_or_else(|| CompressionError::DeltaError("Empty values vector".to_string()))?;
            values.push(last.wrapping_add(delta));
        }

        let mut output = Vec::with_capacity(values.len() * 8);
        for value in values {
            output.write_f64::<LittleEndian>(f64::from_bits(value as u64))?;
        }

        Ok(output)
    }
}

impl Default for DeltaCodec {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delta_i32() {
        let config = DeltaConfig::with_data_type(DeltaDataType::I32);
        let codec = DeltaCodec::with_config(config);

        let mut data = Vec::new();
        for i in 0..100 {
            data.write_i32::<LittleEndian>(i * 10).ok();
        }

        let compressed = codec.compress(&data).expect("Compression failed");
        let decompressed = codec.decompress(&compressed).expect("Decompression failed");

        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_delta_f64() {
        let config = DeltaConfig::with_data_type(DeltaDataType::F64);
        let codec = DeltaCodec::with_config(config);

        let mut data = Vec::new();
        for i in 0..100 {
            data.write_f64::<LittleEndian>(i as f64 * 0.1).ok();
        }

        let compressed = codec.compress(&data).expect("Compression failed");
        let decompressed = codec.decompress(&compressed).expect("Decompression failed");

        assert_eq!(decompressed, data);
    }
}
