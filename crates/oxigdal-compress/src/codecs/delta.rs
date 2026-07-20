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

    /// Order of delta encoding (0 = raw pass-through, 1 = first-order,
    /// 2 = second-order, etc.).
    ///
    /// The compressed stream is self-describing: the *effective* order
    /// actually applied is written as a one-byte header, so `decompress`
    /// never needs to be given a matching `DeltaConfig` to round-trip
    /// correctly. The effective order is `order` clamped to
    /// `min(order, value_count.saturating_sub(1), 255)` — an order that is
    /// meaningless for the amount of data present (e.g. `order >=
    /// value_count`) or that would not fit in the one-byte header is
    /// silently reduced to the largest order that is still well-defined,
    /// rather than being rejected as an error.
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

    /// Set delta order. See [`DeltaConfig::order`] for how out-of-range
    /// values are handled.
    pub fn with_order(mut self, order: usize) -> Self {
        self.order = order;
        self
    }
}

/// Clamp a requested delta order to the largest order that is well-defined
/// for `value_count` values and representable in the one-byte stream
/// header.
///
/// - Fewer than 2 values: no differencing is meaningful, order is 0.
/// - Otherwise: `order` is capped at `value_count - 1` (an Nth-order
///   difference needs at least `N + 1` points) and at `u8::MAX` (the header
///   field width).
fn effective_order(order: usize, value_count: usize) -> usize {
    if value_count <= 1 {
        0
    } else {
        order.min(value_count - 1).min(u8::MAX as usize)
    }
}

/// Apply `passes` iterations of in-place first-order differencing to `i32`
/// values, i.e. `values[i] -= values[i - 1]` for `i` from the end down to
/// `1`. `values[0]` is always left untouched and serves as the anchor for
/// [`cumsum_pass_i32`] to invert the transform.
fn diff_pass_i32(values: &mut [i32], passes: usize) {
    for _ in 0..passes {
        for i in (1..values.len()).rev() {
            values[i] = values[i].wrapping_sub(values[i - 1]);
        }
    }
}

/// Inverse of [`diff_pass_i32`]: applies `passes` iterations of in-place
/// running-sum reconstruction, `values[i] += values[i - 1]` for `i` from `1`
/// up to the end.
fn cumsum_pass_i32(values: &mut [i32], passes: usize) {
    for _ in 0..passes {
        for i in 1..values.len() {
            values[i] = values[i].wrapping_add(values[i - 1]);
        }
    }
}

/// `i64` counterpart of [`diff_pass_i32`], used for `I64` and (as bit
/// patterns) `F64` data.
fn diff_pass_i64(values: &mut [i64], passes: usize) {
    for _ in 0..passes {
        for i in (1..values.len()).rev() {
            values[i] = values[i].wrapping_sub(values[i - 1]);
        }
    }
}

/// `i64` counterpart of [`cumsum_pass_i32`].
fn cumsum_pass_i64(values: &mut [i64], passes: usize) {
    for _ in 0..passes {
        for i in 1..values.len() {
            values[i] = values[i].wrapping_add(values[i - 1]);
        }
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

        let order = effective_order(self.config.order, values.len());
        diff_pass_i32(&mut values, order);

        let mut output = Vec::with_capacity(values.len() * 4 + 1);

        // Self-describing header: the effective order actually applied.
        output.push(order as u8);

        for value in &values {
            output.write_i32::<LittleEndian>(*value)?;
        }

        Ok(output)
    }

    /// Decompress i32 values
    fn decompress_i32(&self, input: &[u8]) -> Result<Vec<u8>> {
        let mut cursor = Cursor::new(input);
        let order = cursor.read_u8()? as usize;

        let first = cursor.read_i32::<LittleEndian>()?;
        let mut values = vec![first];

        while let Ok(value) = cursor.read_i32::<LittleEndian>() {
            values.push(value);
        }

        cumsum_pass_i32(&mut values, order);

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

        let order = effective_order(self.config.order, values.len());
        diff_pass_i64(&mut values, order);

        let mut output = Vec::with_capacity(values.len() * 8 + 1);

        output.push(order as u8);

        for value in &values {
            output.write_i64::<LittleEndian>(*value)?;
        }

        Ok(output)
    }

    /// Decompress i64 values
    fn decompress_i64(&self, input: &[u8]) -> Result<Vec<u8>> {
        let mut cursor = Cursor::new(input);
        let order = cursor.read_u8()? as usize;

        let first = cursor.read_i64::<LittleEndian>()?;
        let mut values = vec![first];

        while let Ok(value) = cursor.read_i64::<LittleEndian>() {
            values.push(value);
        }

        cumsum_pass_i64(&mut values, order);

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

        let order = effective_order(self.config.order, values.len());
        diff_pass_i32(&mut values, order);

        let mut output = Vec::with_capacity(values.len() * 4 + 1);

        output.push(order as u8);

        for value in &values {
            output.write_i32::<LittleEndian>(*value)?;
        }

        Ok(output)
    }

    /// Decompress f32 values
    fn decompress_f32(&self, input: &[u8]) -> Result<Vec<u8>> {
        let mut cursor = Cursor::new(input);
        let order = cursor.read_u8()? as usize;

        let first = cursor.read_i32::<LittleEndian>()?;
        let mut values = vec![first];

        while let Ok(value) = cursor.read_i32::<LittleEndian>() {
            values.push(value);
        }

        cumsum_pass_i32(&mut values, order);

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

        let order = effective_order(self.config.order, values.len());
        diff_pass_i64(&mut values, order);

        let mut output = Vec::with_capacity(values.len() * 8 + 1);

        output.push(order as u8);

        for value in &values {
            output.write_i64::<LittleEndian>(*value)?;
        }

        Ok(output)
    }

    /// Decompress f64 values
    fn decompress_f64(&self, input: &[u8]) -> Result<Vec<u8>> {
        let mut cursor = Cursor::new(input);
        let order = cursor.read_u8()? as usize;

        let first = cursor.read_i64::<LittleEndian>()?;
        let mut values = vec![first];

        while let Ok(value) = cursor.read_i64::<LittleEndian>() {
            values.push(value);
        }

        cumsum_pass_i64(&mut values, order);

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

    /// Regression test for the `DeltaConfig::order` field being silently
    /// ignored: round-trips for orders 1, 2 and 3 on i32 data, using a
    /// quadratic sequence so higher orders actually change the encoding.
    #[test]
    fn test_delta_i32_multi_order_roundtrip() {
        let mut data = Vec::new();
        for i in 0..64i32 {
            // Quadratic sequence: higher-order differencing should compact
            // this better than first-order.
            data.write_i32::<LittleEndian>(i * i).ok();
        }

        for order in [1usize, 2, 3] {
            let config = DeltaConfig::with_data_type(DeltaDataType::I32).with_order(order);
            let codec = DeltaCodec::with_config(config);

            let compressed = codec
                .compress(&data)
                .unwrap_or_else(|e| panic!("compression failed for order {order}: {e}"));
            let decompressed = codec
                .decompress(&compressed)
                .unwrap_or_else(|e| panic!("decompression failed for order {order}: {e}"));

            assert_eq!(decompressed, data, "round-trip mismatch at order {order}");
        }
    }

    /// Same multi-order round-trip coverage for f64 data.
    #[test]
    fn test_delta_f64_multi_order_roundtrip() {
        let mut data = Vec::new();
        for i in 0..64i64 {
            data.write_f64::<LittleEndian>((i * i) as f64 * 0.5).ok();
        }

        for order in [1usize, 2, 3] {
            let config = DeltaConfig::with_data_type(DeltaDataType::F64).with_order(order);
            let codec = DeltaCodec::with_config(config);

            let compressed = codec
                .compress(&data)
                .unwrap_or_else(|e| panic!("compression failed for order {order}: {e}"));
            let decompressed = codec
                .decompress(&compressed)
                .unwrap_or_else(|e| panic!("decompression failed for order {order}: {e}"));

            assert_eq!(decompressed, data, "round-trip mismatch at order {order}");
        }
    }

    /// Proves `with_order` is actually exercised end-to-end: for a
    /// quadratic input, order=2 must encode to different bytes than
    /// order=1 (previously `order` was read nowhere and both produced
    /// byte-identical first-order output).
    #[test]
    fn test_delta_order_changes_encoded_bytes() {
        let mut data = Vec::new();
        for i in 0..32i32 {
            data.write_i32::<LittleEndian>(i * i).ok();
        }

        let codec_order1 =
            DeltaCodec::with_config(DeltaConfig::with_data_type(DeltaDataType::I32).with_order(1));
        let codec_order2 =
            DeltaCodec::with_config(DeltaConfig::with_data_type(DeltaDataType::I32).with_order(2));

        let compressed1 = codec_order1
            .compress(&data)
            .expect("order=1 compression failed");
        let compressed2 = codec_order2
            .compress(&data)
            .expect("order=2 compression failed");

        assert_ne!(
            compressed1, compressed2,
            "order=1 and order=2 must produce different encoded bytes for quadratic input"
        );

        // Both must still round-trip on their own (self-describing header).
        let decompressed1 = codec_order1
            .decompress(&compressed1)
            .expect("order=1 decompression failed");
        let decompressed2 = codec_order2
            .decompress(&compressed2)
            .expect("order=2 decompression failed");
        assert_eq!(decompressed1, data);
        assert_eq!(decompressed2, data);
    }

    /// The compressed stream is self-describing (order is stored in a
    /// header byte), so a decoder using a *different* `DeltaConfig` than
    /// the encoder still round-trips correctly.
    #[test]
    fn test_delta_decompress_self_describing_ignores_decoder_config_order() {
        let mut data = Vec::new();
        for i in 0..40i32 {
            data.write_i32::<LittleEndian>(i * i * i).ok();
        }

        let encoder =
            DeltaCodec::with_config(DeltaConfig::with_data_type(DeltaDataType::I32).with_order(3));
        let compressed = encoder.compress(&data).expect("compression failed");

        // Decoder configured with a mismatched order (0) -- the header in
        // the stream, not this config, drives inversion.
        let decoder =
            DeltaCodec::with_config(DeltaConfig::with_data_type(DeltaDataType::I32).with_order(0));
        let decompressed = decoder
            .decompress(&compressed)
            .expect("decompression failed");

        assert_eq!(decompressed, data);
    }

    /// A requested order that is not meaningful for the amount of data
    /// present (order >= value_count) or exceeds the one-byte header width
    /// is clamped rather than rejected, and still round-trips exactly.
    #[test]
    fn test_delta_order_clamped_for_small_and_oversized_requests() {
        // Single value: any order collapses to a raw copy (order 0).
        let mut single = Vec::new();
        single.write_i32::<LittleEndian>(42).ok();
        let codec =
            DeltaCodec::with_config(DeltaConfig::with_data_type(DeltaDataType::I32).with_order(9));
        let compressed = codec.compress(&single).expect("compression failed");
        assert_eq!(compressed[0], 0, "order must clamp to 0 for a single value");
        let decompressed = codec.decompress(&compressed).expect("decompression failed");
        assert_eq!(decompressed, single);

        // order requested far larger than the number of values (10 values,
        // order 1000): must clamp to value_count - 1 = 9, not panic or
        // truncate silently in a way that breaks the round-trip.
        let mut data = Vec::new();
        for i in 0..10i32 {
            data.write_i32::<LittleEndian>(i).ok();
        }
        let codec = DeltaCodec::with_config(
            DeltaConfig::with_data_type(DeltaDataType::I32).with_order(1000),
        );
        let compressed = codec.compress(&data).expect("compression failed");
        assert_eq!(compressed[0], 9, "order must clamp to value_count - 1");
        let decompressed = codec.decompress(&compressed).expect("decompression failed");
        assert_eq!(decompressed, data);
    }

    /// order=0 is an explicit pass-through: no differencing is applied.
    #[test]
    fn test_delta_order_zero_is_pass_through() {
        let mut data = Vec::new();
        for i in 0..20i32 {
            data.write_i32::<LittleEndian>(i * 7 + 3).ok();
        }

        let codec =
            DeltaCodec::with_config(DeltaConfig::with_data_type(DeltaDataType::I32).with_order(0));
        let compressed = codec.compress(&data).expect("compression failed");
        assert_eq!(compressed[0], 0);

        // With order 0, the values after the header must equal the raw
        // input values verbatim (no differencing at all).
        let mut expected = vec![0u8]; // header
        expected.extend_from_slice(&data);
        assert_eq!(compressed, expected);

        let decompressed = codec.decompress(&compressed).expect("decompression failed");
        assert_eq!(decompressed, data);
    }
}
