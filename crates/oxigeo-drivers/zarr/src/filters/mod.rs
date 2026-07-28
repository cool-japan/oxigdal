//! Data filters for Zarr arrays
//!
//! This module provides filters that transform data before/after compression.
//! Beyond the always-available [`NullFilter`], three real, numcodecs-compatible
//! array-to-array filters are provided behind cargo features:
//!
//! * [`ShuffleFilter`] (feature `shuffle`) -- byte shuffle by element size.
//! * [`DeltaFilter`] (feature `delta`) -- successive-difference encoding.
//! * [`ScaleOffsetFilter`] (feature `scale-offset`) -- fixed scale/offset
//!   quantisation (numcodecs `FixedScaleOffset`).
//!
//! A filter needs the element/dtype context that the bytes-only [`Filter`]
//! trait does not carry, so each filter stores that context at construction
//! time (mirroring how a Zarr v2 filter's JSON config declares `elementsize`,
//! `dtype`, `astype`, etc.).

use crate::error::Result;

#[cfg(any(feature = "shuffle", feature = "delta", feature = "scale-offset"))]
use crate::error::{FilterError, ZarrError};

/// Trait for data filters
pub trait Filter: Send + Sync {
    /// Returns the filter identifier
    fn id(&self) -> &str;

    /// Encodes (applies filter to) data
    ///
    /// # Errors
    /// Returns error if encoding fails
    fn encode(&self, data: &[u8]) -> Result<Vec<u8>>;

    /// Decodes (reverses filter on) data
    ///
    /// # Errors
    /// Returns error if decoding fails
    fn decode(&self, data: &[u8]) -> Result<Vec<u8>>;

    /// Clones the filter
    fn clone_box(&self) -> Box<dyn Filter>;
}

/// Null filter (no-op)
#[derive(Debug, Clone)]
pub struct NullFilter;

impl Filter for NullFilter {
    fn id(&self) -> &str {
        "null"
    }

    fn encode(&self, data: &[u8]) -> Result<Vec<u8>> {
        Ok(data.to_vec())
    }

    fn decode(&self, data: &[u8]) -> Result<Vec<u8>> {
        Ok(data.to_vec())
    }

    fn clone_box(&self) -> Box<dyn Filter> {
        Box::new(self.clone())
    }
}

/// Byte-shuffle filter (numcodecs `Shuffle`).
///
/// Splits each `element_size`-byte element into planes so that all the first
/// bytes of every element are stored together, then all the second bytes, and
/// so on. This groups similar byte magnitudes, improving downstream
/// compression. It is fully reversible and lossless.
#[cfg(feature = "shuffle")]
#[derive(Debug, Clone)]
pub struct ShuffleFilter {
    element_size: usize,
}

#[cfg(feature = "shuffle")]
impl ShuffleFilter {
    /// Creates a shuffle filter for the given element size in bytes.
    ///
    /// # Errors
    /// Returns [`FilterError::InvalidElementSize`] if `element_size` is zero.
    pub fn new(element_size: usize) -> Result<Self> {
        if element_size == 0 {
            return Err(ZarrError::Filter(FilterError::InvalidElementSize {
                size: element_size,
            }));
        }
        Ok(Self { element_size })
    }
}

#[cfg(feature = "shuffle")]
impl Filter for ShuffleFilter {
    fn id(&self) -> &str {
        "shuffle"
    }

    fn encode(&self, data: &[u8]) -> Result<Vec<u8>> {
        let e = self.element_size;
        if e == 1 {
            return Ok(data.to_vec());
        }
        if !data.len().is_multiple_of(e) {
            return Err(ZarrError::Filter(FilterError::EncodeFailed {
                message: format!(
                    "shuffle: data length {} is not a multiple of element size {e}",
                    data.len()
                ),
            }));
        }
        let count = data.len() / e;
        let mut out = vec![0u8; data.len()];
        for i in 0..e {
            for j in 0..count {
                out[i * count + j] = data[j * e + i];
            }
        }
        Ok(out)
    }

    fn decode(&self, data: &[u8]) -> Result<Vec<u8>> {
        let e = self.element_size;
        if e == 1 {
            return Ok(data.to_vec());
        }
        if !data.len().is_multiple_of(e) {
            return Err(ZarrError::Filter(FilterError::DecodeFailed {
                message: format!(
                    "shuffle: data length {} is not a multiple of element size {e}",
                    data.len()
                ),
            }));
        }
        let count = data.len() / e;
        let mut out = vec![0u8; data.len()];
        for i in 0..e {
            for j in 0..count {
                out[j * e + i] = data[i * count + j];
            }
        }
        Ok(out)
    }

    fn clone_box(&self) -> Box<dyn Filter> {
        Box::new(self.clone())
    }
}

/// Element data type for the [`DeltaFilter`] / [`ScaleOffsetFilter`].
#[cfg(any(feature = "delta", feature = "scale-offset"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericDtype {
    /// Signed 8-bit.
    I8,
    /// Unsigned 8-bit.
    U8,
    /// Signed 16-bit.
    I16,
    /// Unsigned 16-bit.
    U16,
    /// Signed 32-bit.
    I32,
    /// Unsigned 32-bit.
    U32,
    /// Signed 64-bit.
    I64,
    /// Unsigned 64-bit.
    U64,
    /// 32-bit float.
    F32,
    /// 64-bit float.
    F64,
}

#[cfg(any(feature = "delta", feature = "scale-offset"))]
impl NumericDtype {
    /// Parses a numpy-style or plain dtype string.
    ///
    /// # Errors
    /// Returns [`FilterError::InvalidDeltaDtype`] for unsupported dtypes.
    pub fn parse(dtype: &str) -> Result<Self> {
        let d = dtype.trim();
        let d = d.strip_prefix(['<', '>', '|']).unwrap_or(d);
        let parsed = match d {
            "i1" | "int8" => Self::I8,
            "u1" | "uint8" => Self::U8,
            "i2" | "int16" => Self::I16,
            "u2" | "uint16" => Self::U16,
            "i4" | "int32" => Self::I32,
            "u4" | "uint32" => Self::U32,
            "i8" | "int64" => Self::I64,
            "u8" | "uint64" => Self::U64,
            "f4" | "float32" => Self::F32,
            "f8" | "float64" => Self::F64,
            _ => {
                return Err(ZarrError::Filter(FilterError::InvalidDeltaDtype {
                    dtype: dtype.to_string(),
                }));
            }
        };
        Ok(parsed)
    }

    /// Element size in bytes.
    #[must_use]
    pub const fn size(self) -> usize {
        match self {
            Self::I8 | Self::U8 => 1,
            Self::I16 | Self::U16 => 2,
            Self::I32 | Self::U32 | Self::F32 => 4,
            Self::I64 | Self::U64 | Self::F64 => 8,
        }
    }
}

/// Delta (successive-difference) filter (numcodecs `Delta`).
///
/// Replaces each element with the difference from its predecessor along the
/// flattened array. Integer types use wrapping arithmetic (exactly reversible);
/// float types use ordinary subtraction. Little-endian element encoding.
#[cfg(feature = "delta")]
#[derive(Debug, Clone)]
pub struct DeltaFilter {
    dtype: NumericDtype,
}

#[cfg(feature = "delta")]
impl DeltaFilter {
    /// Creates a delta filter for the given dtype string.
    ///
    /// # Errors
    /// Returns [`FilterError::InvalidDeltaDtype`] for unsupported dtypes.
    pub fn new(dtype: &str) -> Result<Self> {
        Ok(Self {
            dtype: NumericDtype::parse(dtype)?,
        })
    }

    /// Creates a delta filter directly from a [`NumericDtype`].
    #[must_use]
    pub const fn from_dtype(dtype: NumericDtype) -> Self {
        Self { dtype }
    }

    fn check_len(&self, len: usize, encoding: bool) -> Result<usize> {
        let size = self.dtype.size();
        if !len.is_multiple_of(size) {
            let message = format!("delta: data length {len} not a multiple of dtype size {size}");
            return Err(if encoding {
                ZarrError::Filter(FilterError::EncodeFailed { message })
            } else {
                ZarrError::Filter(FilterError::DecodeFailed { message })
            });
        }
        Ok(len / size)
    }
}

/// Applies `op` element-wise for a fixed-width integer delta transform.
#[cfg(feature = "delta")]
macro_rules! delta_int {
    ($data:expr, $ty:ty, $count:expr, $encode:expr) => {{
        const W: usize = core::mem::size_of::<$ty>();
        let mut out = Vec::with_capacity($data.len());
        let mut prev: $ty = 0;
        for j in 0..$count {
            let mut buf = [0u8; W];
            buf.copy_from_slice(&$data[j * W..j * W + W]);
            let cur = <$ty>::from_le_bytes(buf);
            if $encode {
                // encode: store the difference from the previous raw value.
                out.extend_from_slice(&cur.wrapping_sub(prev).to_le_bytes());
                prev = cur;
            } else {
                // decode: running prefix sum reconstructs the raw values.
                let value = prev.wrapping_add(cur);
                out.extend_from_slice(&value.to_le_bytes());
                prev = value;
            }
        }
        out
    }};
}

/// Applies element-wise float delta transform.
#[cfg(feature = "delta")]
macro_rules! delta_float {
    ($data:expr, $ty:ty, $count:expr, $encode:expr) => {{
        const W: usize = core::mem::size_of::<$ty>();
        let mut out = Vec::with_capacity($data.len());
        let mut prev: $ty = 0.0;
        for j in 0..$count {
            let mut buf = [0u8; W];
            buf.copy_from_slice(&$data[j * W..j * W + W]);
            let cur = <$ty>::from_le_bytes(buf);
            if $encode {
                out.extend_from_slice(&(cur - prev).to_le_bytes());
                prev = cur;
            } else {
                let value = prev + cur;
                out.extend_from_slice(&value.to_le_bytes());
                prev = value;
            }
        }
        out
    }};
}

#[cfg(feature = "delta")]
impl DeltaFilter {
    fn transform(&self, data: &[u8], encode: bool) -> Result<Vec<u8>> {
        let count = self.check_len(data.len(), encode)?;
        let out = match self.dtype {
            NumericDtype::I8 => delta_int!(data, i8, count, encode),
            NumericDtype::U8 => delta_int!(data, u8, count, encode),
            NumericDtype::I16 => delta_int!(data, i16, count, encode),
            NumericDtype::U16 => delta_int!(data, u16, count, encode),
            NumericDtype::I32 => delta_int!(data, i32, count, encode),
            NumericDtype::U32 => delta_int!(data, u32, count, encode),
            NumericDtype::I64 => delta_int!(data, i64, count, encode),
            NumericDtype::U64 => delta_int!(data, u64, count, encode),
            NumericDtype::F32 => delta_float!(data, f32, count, encode),
            NumericDtype::F64 => delta_float!(data, f64, count, encode),
        };
        Ok(out)
    }
}

#[cfg(feature = "delta")]
impl Filter for DeltaFilter {
    fn id(&self) -> &str {
        "delta"
    }

    fn encode(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.transform(data, true)
    }

    fn decode(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.transform(data, false)
    }

    fn clone_box(&self) -> Box<dyn Filter> {
        Box::new(self.clone())
    }
}

/// Fixed scale/offset quantisation filter (numcodecs `FixedScaleOffset`).
///
/// Encodes each source float `x` as `round((x - offset) * scale)` stored in a
/// (typically smaller) integer `astype`, and decodes it back as
/// `enc / scale + offset`. This is lossy; the reconstruction error is bounded
/// by `1 / (2 * scale)`. Out-of-range values are rejected with a typed error
/// instead of silently wrapping.
#[cfg(feature = "scale-offset")]
#[derive(Debug, Clone)]
pub struct ScaleOffsetFilter {
    offset: f64,
    scale: f64,
    src: NumericDtype,
    astype: NumericDtype,
}

#[cfg(feature = "scale-offset")]
impl ScaleOffsetFilter {
    /// Creates a fixed scale/offset filter.
    ///
    /// # Errors
    /// Returns an error for unsupported dtypes, a non-positive/ non-finite
    /// scale, or a floating-point `astype` (the encoded type must be integer).
    pub fn new(offset: f64, scale: f64, src_dtype: &str, astype: &str) -> Result<Self> {
        if !scale.is_finite() || scale <= 0.0 {
            return Err(ZarrError::Filter(FilterError::InvalidConfiguration {
                filter: "scale-offset".to_string(),
                message: format!("scale must be finite and positive, got {scale}"),
            }));
        }
        let src = NumericDtype::parse(src_dtype)?;
        let astype = NumericDtype::parse(astype)?;
        if matches!(astype, NumericDtype::F32 | NumericDtype::F64) {
            return Err(ZarrError::Filter(FilterError::InvalidConfiguration {
                filter: "scale-offset".to_string(),
                message: "astype must be an integer type".to_string(),
            }));
        }
        Ok(Self {
            offset,
            scale,
            src,
            astype,
        })
    }

    fn read_src(&self, bytes: &[u8]) -> f64 {
        match self.src {
            NumericDtype::F32 => {
                f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64
            }
            NumericDtype::F64 => f64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            // Integer sources are read as their integer value.
            _ => read_int_le(bytes, self.src),
        }
    }

    fn astype_range(&self) -> (f64, f64) {
        match self.astype {
            NumericDtype::I8 => (f64::from(i8::MIN), f64::from(i8::MAX)),
            NumericDtype::U8 => (0.0, f64::from(u8::MAX)),
            NumericDtype::I16 => (f64::from(i16::MIN), f64::from(i16::MAX)),
            NumericDtype::U16 => (0.0, f64::from(u16::MAX)),
            NumericDtype::I32 => (f64::from(i32::MIN), f64::from(i32::MAX)),
            NumericDtype::U32 => (0.0, f64::from(u32::MAX)),
            NumericDtype::I64 => (i64::MIN as f64, i64::MAX as f64),
            NumericDtype::U64 => (0.0, u64::MAX as f64),
            NumericDtype::F32 | NumericDtype::F64 => (f64::MIN, f64::MAX),
        }
    }
}

/// Reads a little-endian integer of `dtype` from `bytes` as f64.
#[cfg(feature = "scale-offset")]
fn read_int_le(bytes: &[u8], dtype: NumericDtype) -> f64 {
    match dtype {
        NumericDtype::I8 => f64::from(bytes[0] as i8),
        NumericDtype::U8 => f64::from(bytes[0]),
        NumericDtype::I16 => f64::from(i16::from_le_bytes([bytes[0], bytes[1]])),
        NumericDtype::U16 => f64::from(u16::from_le_bytes([bytes[0], bytes[1]])),
        NumericDtype::I32 => {
            f64::from(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        }
        NumericDtype::U32 => {
            f64::from(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        }
        NumericDtype::I64 => i64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]) as f64,
        NumericDtype::U64 => u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]) as f64,
        NumericDtype::F32 | NumericDtype::F64 => 0.0,
    }
}

/// Writes an integer value (already validated in range) as little-endian
/// `dtype` bytes.
#[cfg(feature = "scale-offset")]
fn write_int_le(value: f64, dtype: NumericDtype, out: &mut Vec<u8>) {
    match dtype {
        NumericDtype::I8 => out.extend_from_slice(&(value as i8).to_le_bytes()),
        NumericDtype::U8 => out.extend_from_slice(&(value as u8).to_le_bytes()),
        NumericDtype::I16 => out.extend_from_slice(&(value as i16).to_le_bytes()),
        NumericDtype::U16 => out.extend_from_slice(&(value as u16).to_le_bytes()),
        NumericDtype::I32 => out.extend_from_slice(&(value as i32).to_le_bytes()),
        NumericDtype::U32 => out.extend_from_slice(&(value as u32).to_le_bytes()),
        NumericDtype::I64 => out.extend_from_slice(&(value as i64).to_le_bytes()),
        NumericDtype::U64 => out.extend_from_slice(&(value as u64).to_le_bytes()),
        NumericDtype::F32 | NumericDtype::F64 => {}
    }
}

#[cfg(feature = "scale-offset")]
impl Filter for ScaleOffsetFilter {
    fn id(&self) -> &str {
        "fixedscaleoffset"
    }

    fn encode(&self, data: &[u8]) -> Result<Vec<u8>> {
        let src_size = self.src.size();
        if !data.len().is_multiple_of(src_size) {
            return Err(ZarrError::Filter(FilterError::EncodeFailed {
                message: format!(
                    "scale-offset: data length {} not a multiple of src size {src_size}",
                    data.len()
                ),
            }));
        }
        let (lo, hi) = self.astype_range();
        let mut out = Vec::with_capacity(data.len() / src_size * self.astype.size());
        for chunk in data.chunks_exact(src_size) {
            let x = self.read_src(chunk);
            let enc = ((x - self.offset) * self.scale).round();
            if !enc.is_finite() || enc < lo || enc > hi {
                return Err(ZarrError::Filter(FilterError::EncodeFailed {
                    message: format!(
                        "scale-offset: encoded value {enc} for input {x} is outside the \
                         astype range [{lo}, {hi}]"
                    ),
                }));
            }
            write_int_le(enc, self.astype, &mut out);
        }
        Ok(out)
    }

    fn decode(&self, data: &[u8]) -> Result<Vec<u8>> {
        let enc_size = self.astype.size();
        if !data.len().is_multiple_of(enc_size) {
            return Err(ZarrError::Filter(FilterError::DecodeFailed {
                message: format!(
                    "scale-offset: data length {} not a multiple of astype size {enc_size}",
                    data.len()
                ),
            }));
        }
        let mut out = Vec::with_capacity(data.len() / enc_size * self.src.size());
        for chunk in data.chunks_exact(enc_size) {
            let enc = read_int_le(chunk, self.astype);
            let x = enc / self.scale + self.offset;
            match self.src {
                NumericDtype::F32 => out.extend_from_slice(&(x as f32).to_le_bytes()),
                NumericDtype::F64 => out.extend_from_slice(&x.to_le_bytes()),
                other => write_int_le(x.round(), other, &mut out),
            }
        }
        Ok(out)
    }

    fn clone_box(&self) -> Box<dyn Filter> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[cfg(feature = "shuffle")]
    #[test]
    fn test_shuffle_filter_roundtrip() {
        let f = ShuffleFilter::new(4).expect("shuffle");
        let data: Vec<u8> = (0..40).collect();
        let enc = f.encode(&data).expect("enc");
        assert_ne!(enc, data, "shuffle must reorder bytes");
        assert_eq!(f.decode(&enc).expect("dec"), data);
    }

    #[cfg(feature = "shuffle")]
    #[test]
    fn test_shuffle_filter_rejects_zero_and_misaligned() {
        assert!(ShuffleFilter::new(0).is_err());
        let f = ShuffleFilter::new(4).expect("shuffle");
        assert!(f.encode(&[1, 2, 3]).is_err());
    }

    #[cfg(feature = "delta")]
    #[test]
    fn test_delta_filter_int_roundtrip() {
        let f = DeltaFilter::new("<i4").expect("delta");
        let values = [10i32, 12, 9, 100, -5, -5];
        let data: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let enc = f.encode(&data).expect("enc");
        let dec = f.decode(&enc).expect("dec");
        assert_eq!(dec, data);
        // First stored delta equals the first value (prev starts at 0).
        assert_eq!(&enc[0..4], &10i32.to_le_bytes());
    }

    #[cfg(feature = "delta")]
    #[test]
    fn test_delta_filter_float_roundtrip() {
        let f = DeltaFilter::new("float64").expect("delta");
        let values = [1.5f64, 2.5, 2.5, 10.0];
        let data: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let dec = f.decode(&f.encode(&data).expect("enc")).expect("dec");
        assert_eq!(dec, data);
    }

    #[cfg(feature = "scale-offset")]
    #[test]
    fn test_scale_offset_roundtrip_within_tolerance() {
        // Encode f64 temperatures to u16 at 0.01 resolution.
        let f = ScaleOffsetFilter::new(0.0, 100.0, "float64", "uint16").expect("filter");
        let values = [12.34f64, 55.55, 0.0, 100.0];
        let data: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let enc = f.encode(&data).expect("enc");
        // u16 encoding is 2 bytes per value vs 8 for f64.
        assert_eq!(enc.len(), values.len() * 2);
        let dec_bytes = f.decode(&enc).expect("dec");
        let dec: Vec<f64> = dec_bytes
            .chunks_exact(8)
            .map(|c| f64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
            .collect();
        for (a, b) in values.iter().zip(dec.iter()) {
            assert!((a - b).abs() <= 0.005 + 1e-9, "{a} vs {b}");
        }
    }

    #[cfg(feature = "scale-offset")]
    #[test]
    fn test_scale_offset_out_of_range_errors() {
        let f = ScaleOffsetFilter::new(0.0, 100.0, "float64", "uint8").expect("filter");
        // 1000 * 100 = 100000 > u8::MAX -> typed error, not a silent wrap.
        let data = 1000.0f64.to_le_bytes().to_vec();
        assert!(f.encode(&data).is_err());
    }

    #[cfg(feature = "scale-offset")]
    #[test]
    fn test_scale_offset_rejects_float_astype() {
        assert!(ScaleOffsetFilter::new(0.0, 1.0, "float64", "float32").is_err());
    }
}
