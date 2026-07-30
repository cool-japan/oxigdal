//! LERC (Limited Error Raster Compression) codec for GeoTIFF
//!
//! Pure Rust LERC2 support with two decode paths:
//!
//! 1. **Real Esri/GDAL LERC2 blobs** — the standard on-disk format: a versioned
//!    header, a run-length-encoded valid/invalid mask, and per-micro-block data
//!    (either raw *one-sweep* values or quantized values packed with the LERC
//!    `BitStuffer2` variable-bit-width bit-stuffer, including its lookup-table
//!    variant). [`LercCodec::decode`] decodes these for LERC2 versions 2–4 (v1–6
//!    headers parse) covering float and integer rasters. See the `lerc2`
//!    submodule for the bit-level implementation.
//! 2. **The crate's own raw-value payload** — a simple lossless layout written by
//!    [`LercCodec::encode`] for Float32/Float64/Int16/Int32/UInt8/UInt16, used
//!    for round-trip encode/decode within this crate.
//!
//! The two formats are disambiguated by the header version field: a real LERC2
//! blob stores a 32-bit version in `0..=6`, whereas the crate's raw format stores
//! `nDim=1` in the same position, pushing the field out of range.
//!
//! # Unsupported sub-variants (fail loud, never silent-wrong)
//!
//! The LERC2 Huffman-coded byte-tile mode and the v6 delta-delta Huffman float
//! mode are **not** decoded. Blobs using them return an explicit
//! [`CompressionError::DecompressionFailed`] rather than fabricating output.
//!
//! Reference: <https://github.com/Esri/lerc>

mod lerc2;

use oxigeo_core::error::{CompressionError, OxiGeoError, Result};

/// LERC2 magic bytes (6 bytes).
const LERC2_MAGIC: &[u8] = b"Lerc2 ";

/// Minimum valid LERC2 header size in bytes.
const LERC2_MIN_HEADER: usize = 30;

/// LERC data type codes (matches LERC spec table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LercDataType {
    /// Signed 8-bit integer
    Char,
    /// Unsigned 8-bit integer
    Byte,
    /// Signed 16-bit integer
    Short,
    /// Unsigned 16-bit integer
    UShort,
    /// Signed 32-bit integer
    Int,
    /// Unsigned 32-bit integer
    UInt,
    /// 32-bit float
    Float,
    /// 64-bit float
    Double,
}

impl LercDataType {
    /// Returns the LERC data type byte code.
    #[must_use]
    pub const fn code(&self) -> u8 {
        match self {
            Self::Char => 0,
            Self::Byte => 1,
            Self::Short => 2,
            Self::UShort => 3,
            Self::Int => 4,
            Self::UInt => 5,
            Self::Float => 6,
            Self::Double => 7,
        }
    }

    /// Creates a `LercDataType` from a byte code.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Char),
            1 => Some(Self::Byte),
            2 => Some(Self::Short),
            3 => Some(Self::UShort),
            4 => Some(Self::Int),
            5 => Some(Self::UInt),
            6 => Some(Self::Float),
            7 => Some(Self::Double),
            _ => None,
        }
    }

    /// Returns the size of this data type in bytes.
    #[must_use]
    pub const fn byte_size(&self) -> usize {
        match self {
            Self::Char | Self::Byte => 1,
            Self::Short | Self::UShort => 2,
            Self::Int | Self::UInt | Self::Float => 4,
            Self::Double => 8,
        }
    }

    /// Returns a human-readable name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Char => "Char (i8)",
            Self::Byte => "Byte (u8)",
            Self::Short => "Short (i16)",
            Self::UShort => "UShort (u16)",
            Self::Int => "Int (i32)",
            Self::UInt => "UInt (u32)",
            Self::Float => "Float (f32)",
            Self::Double => "Double (f64)",
        }
    }
}

/// LERC compression parameters.
#[derive(Debug, Clone)]
pub struct LercParams {
    /// Maximum allowed per-pixel error. 0.0 = lossless.
    pub max_z_error: f64,
    /// Data type to encode as.
    pub data_type: LercDataType,
}

impl Default for LercParams {
    fn default() -> Self {
        Self {
            max_z_error: 0.0,
            data_type: LercDataType::Float,
        }
    }
}

impl LercParams {
    /// Creates a new `LercParams` with given max Z error and data type.
    #[must_use]
    pub fn new(max_z_error: f64, data_type: LercDataType) -> Self {
        Self {
            max_z_error,
            data_type,
        }
    }

    /// Returns true if parameters specify lossless encoding.
    #[must_use]
    pub fn is_lossless(&self) -> bool {
        self.max_z_error == 0.0
    }
}

/// Parsed LERC2 image info header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LercImageInfo {
    /// LERC format version (2 or 3).
    pub version: u16,
    /// Data type code.
    pub data_type: u8,
    /// Number of dimensions per pixel (usually 1).
    pub n_dim: u32,
    /// Image width (columns).
    pub n_cols: u32,
    /// Image height (rows).
    pub n_rows: u32,
    /// Number of bands.
    pub n_bands: u32,
}

impl LercImageInfo {
    /// Parse `LercImageInfo` from the raw header bytes starting at offset 0.
    ///
    /// Layout: magic(6) + version(2) + dt(1) + nDim(4) + nCols(4) + nRows(4) + nBands(4)
    fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < LERC2_MIN_HEADER {
            return Err(OxiGeoError::Compression(
                CompressionError::DecompressionFailed {
                    message: format!(
                        "LERC2 data too short: {} < {} bytes",
                        data.len(),
                        LERC2_MIN_HEADER
                    ),
                },
            ));
        }

        if !data.starts_with(LERC2_MAGIC) {
            return Err(OxiGeoError::Compression(
                CompressionError::DecompressionFailed {
                    message: format!(
                        "Not LERC2 format: expected magic {:?}, got {:?}",
                        LERC2_MAGIC,
                        &data[..6]
                    ),
                },
            ));
        }

        let version = u16::from_le_bytes([data[6], data[7]]);
        let data_type = data[8];
        let n_dim = u32::from_le_bytes([data[9], data[10], data[11], data[12]]);
        let n_cols = u32::from_le_bytes([data[13], data[14], data[15], data[16]]);
        let n_rows = u32::from_le_bytes([data[17], data[18], data[19], data[20]]);
        let n_bands = u32::from_le_bytes([data[21], data[22], data[23], data[24]]);

        if n_cols == 0 || n_rows == 0 || n_bands == 0 {
            return Err(OxiGeoError::Compression(
                CompressionError::DecompressionFailed {
                    message: format!(
                        "LERC2 header has zero dimension: cols={n_cols}, rows={n_rows}, bands={n_bands}"
                    ),
                },
            ));
        }

        Ok(Self {
            version,
            data_type,
            n_dim,
            n_cols,
            n_rows,
            n_bands,
        })
    }

    /// Total pixel count (cols * rows * bands).
    #[must_use]
    pub fn pixel_count(&self) -> usize {
        (self.n_cols as usize)
            .saturating_mul(self.n_rows as usize)
            .saturating_mul(self.n_bands as usize)
    }
}

/// A fully decoded LERC raster: dequantized values plus geometry and the
/// declared element type. Values are in row-major, depth/band-interleaved order
/// (`values[(row * cols + col) * bands + band]`); pixels marked invalid by the
/// LERC mask are left at `0.0`.
#[derive(Debug, Clone)]
pub struct LercDecoded {
    /// Dequantized pixel values, widened to `f64`.
    pub values: Vec<f64>,
    /// Image width (columns).
    pub n_cols: u32,
    /// Image height (rows).
    pub n_rows: u32,
    /// Number of bands / per-pixel depth values.
    pub n_bands: u32,
    /// Declared LERC element data type (used to serialize native bytes).
    pub data_type: LercDataType,
}

/// LERC codec: encode and decode LERC-compressed raster data.
pub struct LercCodec;

impl LercCodec {
    /// Decode LERC-compressed data.
    ///
    /// Returns `(decoded_values_as_f64, width, height, n_bands)`.
    ///
    /// # Errors
    /// Returns an error if the data is not valid LERC2 format or is truncated.
    pub fn decode(data: &[u8]) -> Result<(Vec<f64>, u32, u32, u32)> {
        let d = Self::decode_full(data)?;
        Ok((d.values, d.n_cols, d.n_rows, d.n_bands))
    }

    /// Decode LERC-compressed data, returning the full [`LercDecoded`] result
    /// including the declared element data type.
    ///
    /// Real Esri/GDAL LERC2 blobs are decoded via the bit-stuffed block decoder;
    /// otherwise the crate's own raw-value payload format is used.
    ///
    /// # Errors
    /// Returns an error if the data is not a valid LERC2 stream or is truncated,
    /// or if it uses an unsupported Huffman sub-variant.
    pub fn decode_full(data: &[u8]) -> Result<LercDecoded> {
        if let Some(res) = lerc2::try_decode(data) {
            return res;
        }
        Self::decode_raw_format(data)
    }

    /// Encode raster data to LERC2 format.
    ///
    /// # Arguments
    /// * `values` - Pixel values in f64, in row-major band-interleaved order
    /// * `width` - Image width
    /// * `height` - Image height
    /// * `n_bands` - Number of bands
    /// * `params` - Encoding parameters
    ///
    /// # Errors
    /// Returns an error if the input dimensions are inconsistent.
    pub fn encode(
        values: &[f64],
        width: u32,
        height: u32,
        n_bands: u32,
        params: &LercParams,
    ) -> Result<Vec<u8>> {
        Self::encode_lerc2(values, width, height, n_bands, params)
    }

    /// Returns true if the byte slice appears to be LERC-encoded data.
    #[must_use]
    pub fn is_lerc(data: &[u8]) -> bool {
        data.starts_with(LERC2_MAGIC) || data.starts_with(b"Lerc1")
    }

    /// Returns the LERC version embedded in the data, or `None` if not LERC2.
    #[must_use]
    pub fn version(data: &[u8]) -> Option<u16> {
        if data.starts_with(LERC2_MAGIC) && data.len() >= 8 {
            Some(u16::from_le_bytes([data[6], data[7]]))
        } else {
            None
        }
    }

    /// Parse only the image info header without decoding the pixel data.
    ///
    /// # Errors
    /// Returns an error if the header is invalid.
    pub fn parse_header(data: &[u8]) -> Result<LercImageInfo> {
        LercImageInfo::parse(data)
    }

    // -----------------------------------------------------------------------
    // Private implementation
    // -----------------------------------------------------------------------

    /// Decodes the crate's own raw-value payload format (see [`Self::encode`]).
    ///
    /// This path is taken only when the blob is *not* a real Esri/GDAL LERC2
    /// stream. If the payload is shorter than the raw layout (i.e. it looks like
    /// an external bit-stuffed blob whose header we could not recognise as real
    /// LERC2), it fails explicitly rather than fabricating an all-zero raster.
    fn decode_raw_format(data: &[u8]) -> Result<LercDecoded> {
        let info = LercImageInfo::parse(data)?;
        let pixel_count = info.pixel_count();

        // Raw-value payload starts at byte 25 (after the crate's header).
        const HDR_SIZE: usize = 25;

        let dt = LercDataType::from_code(info.data_type).ok_or_else(|| {
            OxiGeoError::Compression(CompressionError::DecompressionFailed {
                message: format!("Unknown LERC2 data type code: {}", info.data_type),
            })
        })?;

        let expected_raw = HDR_SIZE + pixel_count * dt.byte_size();

        if data.len() < expected_raw {
            // Not our raw format and not a recognised real LERC2 blob: refuse to
            // fabricate output (silent data loss).
            return Err(OxiGeoError::Compression(
                CompressionError::DecompressionFailed {
                    message: format!(
                        "LERC2 payload is neither the crate's raw-value format nor a recognised \
                         Esri/GDAL bit-stuffed blob (data type {}, {}x{}x{} pixels); refusing to \
                         fabricate output",
                        dt.name(),
                        info.n_cols,
                        info.n_rows,
                        info.n_bands
                    ),
                },
            ));
        }

        let values = Self::decode_raw_payload(&data[HDR_SIZE..], pixel_count, &dt)?;

        Ok(LercDecoded {
            values,
            n_cols: info.n_cols,
            n_rows: info.n_rows,
            n_bands: info.n_bands,
            data_type: dt,
        })
    }

    /// Decode a raw (non-bit-stuffed) pixel payload.
    fn decode_raw_payload(
        payload: &[u8],
        pixel_count: usize,
        dt: &LercDataType,
    ) -> Result<Vec<f64>> {
        let mut values = Vec::with_capacity(pixel_count);
        let byte_size = dt.byte_size();

        if payload.len() < pixel_count * byte_size {
            return Err(OxiGeoError::Compression(
                CompressionError::DecompressionFailed {
                    message: format!(
                        "LERC2 payload truncated: expected {} bytes, got {}",
                        pixel_count * byte_size,
                        payload.len()
                    ),
                },
            ));
        }

        for i in 0..pixel_count {
            let off = i * byte_size;
            let v = match dt {
                LercDataType::Char => payload[off] as i8 as f64,
                LercDataType::Byte => payload[off] as f64,
                LercDataType::Short => i16::from_le_bytes([payload[off], payload[off + 1]]) as f64,
                LercDataType::UShort => u16::from_le_bytes([payload[off], payload[off + 1]]) as f64,
                LercDataType::Int => i32::from_le_bytes([
                    payload[off],
                    payload[off + 1],
                    payload[off + 2],
                    payload[off + 3],
                ]) as f64,
                LercDataType::UInt => u32::from_le_bytes([
                    payload[off],
                    payload[off + 1],
                    payload[off + 2],
                    payload[off + 3],
                ]) as f64,
                LercDataType::Float => f32::from_le_bytes([
                    payload[off],
                    payload[off + 1],
                    payload[off + 2],
                    payload[off + 3],
                ]) as f64,
                LercDataType::Double => f64::from_le_bytes([
                    payload[off],
                    payload[off + 1],
                    payload[off + 2],
                    payload[off + 3],
                    payload[off + 4],
                    payload[off + 5],
                    payload[off + 6],
                    payload[off + 7],
                ]),
            };
            values.push(v);
        }
        Ok(values)
    }

    fn encode_lerc2(
        values: &[f64],
        width: u32,
        height: u32,
        n_bands: u32,
        params: &LercParams,
    ) -> Result<Vec<u8>> {
        let expected = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(n_bands as usize);

        if values.len() != expected {
            return Err(OxiGeoError::Compression(
                CompressionError::CompressionFailed {
                    message: format!(
                        "LERC2 encode: expected {} values ({width}x{height}x{n_bands}), got {}",
                        expected,
                        values.len()
                    ),
                },
            ));
        }

        let dt_code = params.data_type.code();

        // Serialize header (25 bytes)
        let mut buf: Vec<u8> = Vec::with_capacity(25 + expected * params.data_type.byte_size());
        buf.extend_from_slice(LERC2_MAGIC); // bytes 0-5
        buf.extend_from_slice(&2u16.to_le_bytes()); // bytes 6-7: version
        buf.push(dt_code); // byte 8: data type
        buf.extend_from_slice(&1u32.to_le_bytes()); // bytes 9-12: nDim=1
        buf.extend_from_slice(&width.to_le_bytes()); // bytes 13-16
        buf.extend_from_slice(&height.to_le_bytes()); // bytes 17-20
        buf.extend_from_slice(&n_bands.to_le_bytes()); // bytes 21-24

        // Write pixel data as raw little-endian values (lossless raw encoding)
        let _ = params.max_z_error; // used in full bit-stuffed block encoding
        for &v in values {
            match params.data_type {
                LercDataType::Char => buf.push((v as i8) as u8),
                LercDataType::Byte => buf.push(v as u8),
                LercDataType::Short => buf.extend_from_slice(&(v as i16).to_le_bytes()),
                LercDataType::UShort => buf.extend_from_slice(&(v as u16).to_le_bytes()),
                LercDataType::Int => buf.extend_from_slice(&(v as i32).to_le_bytes()),
                LercDataType::UInt => buf.extend_from_slice(&(v as u32).to_le_bytes()),
                LercDataType::Float => buf.extend_from_slice(&(v as f32).to_le_bytes()),
                LercDataType::Double => buf.extend_from_slice(&v.to_le_bytes()),
            }
        }

        Ok(buf)
    }
}

// ---------------------------------------------------------------------------
// Wire into the GeoTIFF compression dispatch
// ---------------------------------------------------------------------------

/// Serializes one decoded value into `out` as **host**-order bytes of `dt`.
///
/// The name is load-bearing: `crate::decoded_needs_native_swap` excludes
/// `Compression::Lerc` from the byte-order normalisation precisely because this
/// function is the one that puts LERC samples in host order, so it must actually
/// do so. It used to write `to_le_bytes` unconditionally, which agrees with the
/// host on every little-endian target — every target this crate is tested on —
/// and silently byte-reversed every LERC sample on a big-endian one, with the
/// swap that would have fixed it deliberately switched off.
///
/// This is about the *decoder's output*, not the wire format: a LERC blob is
/// little-endian on disk by definition (see [`LercCodec::encode`]), whatever the
/// enclosing TIFF's `II`/`MM` header says.
fn serialize_native(out: &mut Vec<u8>, v: f64, dt: LercDataType) {
    match dt {
        LercDataType::Char => out.push((v as i8) as u8),
        LercDataType::Byte => out.push(v as u8),
        LercDataType::Short => out.extend_from_slice(&(v as i16).to_ne_bytes()),
        LercDataType::UShort => out.extend_from_slice(&(v as u16).to_ne_bytes()),
        LercDataType::Int => out.extend_from_slice(&(v as i32).to_ne_bytes()),
        LercDataType::UInt => out.extend_from_slice(&(v as u32).to_ne_bytes()),
        LercDataType::Float => out.extend_from_slice(&(v as f32).to_ne_bytes()),
        LercDataType::Double => out.extend_from_slice(&v.to_ne_bytes()),
    }
}

/// Decode a LERC-compressed TIFF tile/strip into **host**-order sample bytes
/// matching the blob's declared LERC data type.
///
/// The returned buffer holds `cols * rows * bands` samples of
/// [`LercDataType::byte_size`] bytes each, in row-major, band-interleaved order.
///
/// # Errors
/// Returns an error if the data is not valid LERC2 or uses an unsupported
/// sub-variant.
pub fn decompress_lerc(data: &[u8], _expected_size: usize) -> Result<Vec<u8>> {
    let decoded = LercCodec::decode_full(data)?;

    let mut out = Vec::with_capacity(decoded.values.len() * decoded.data_type.byte_size());
    for &v in &decoded.values {
        serialize_native(&mut out, v, decoded.data_type);
    }
    Ok(out)
}

/// Encode a TIFF tile/strip with LERC compression.
///
/// # Errors
/// Returns an error on dimension mismatch.
pub fn compress_lerc(data: &[u8], width: u32, height: u32, n_bands: u32) -> Result<Vec<u8>> {
    // Treat incoming bytes as f32 pixels in the **host's** byte order, which is
    // what every raster buffer in the workspace holds (see the crate-level
    // *Byte order of decoded samples* section); the LERC blob this produces is
    // little-endian on the wire regardless.
    if !data.len().is_multiple_of(4) {
        return Err(OxiGeoError::Compression(
            CompressionError::CompressionFailed {
                message: format!(
                    "LERC compress: data length {} is not a multiple of 4 (expected f32 input)",
                    data.len()
                ),
            },
        ));
    }

    let values: Vec<f64> = data
        .chunks_exact(4)
        .map(|c| f32::from_ne_bytes([c[0], c[1], c[2], c[3]]) as f64)
        .collect();

    LercCodec::encode(&values, width, height, n_bands, &LercParams::default())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    // -- LercDataType --

    #[test]
    fn test_lerc_data_type_codes_roundtrip() {
        let types = [
            LercDataType::Char,
            LercDataType::Byte,
            LercDataType::Short,
            LercDataType::UShort,
            LercDataType::Int,
            LercDataType::UInt,
            LercDataType::Float,
            LercDataType::Double,
        ];
        for dt in &types {
            let code = dt.code();
            let back = LercDataType::from_code(code).expect("roundtrip failed");
            assert_eq!(dt, &back, "roundtrip failed for {:?}", dt);
        }
    }

    #[test]
    fn test_lerc_data_type_from_code_invalid() {
        assert!(LercDataType::from_code(8).is_none());
        assert!(LercDataType::from_code(255).is_none());
    }

    #[test]
    fn test_lerc_data_type_byte_sizes() {
        assert_eq!(LercDataType::Char.byte_size(), 1);
        assert_eq!(LercDataType::Byte.byte_size(), 1);
        assert_eq!(LercDataType::Short.byte_size(), 2);
        assert_eq!(LercDataType::UShort.byte_size(), 2);
        assert_eq!(LercDataType::Int.byte_size(), 4);
        assert_eq!(LercDataType::UInt.byte_size(), 4);
        assert_eq!(LercDataType::Float.byte_size(), 4);
        assert_eq!(LercDataType::Double.byte_size(), 8);
    }

    #[test]
    fn test_lerc_data_type_names_non_empty() {
        let types = [
            LercDataType::Char,
            LercDataType::Byte,
            LercDataType::Short,
            LercDataType::UShort,
            LercDataType::Int,
            LercDataType::UInt,
            LercDataType::Float,
            LercDataType::Double,
        ];
        for dt in &types {
            assert!(!dt.name().is_empty());
        }
    }

    // -- LercParams --

    #[test]
    fn test_lerc_params_default_is_lossless() {
        let p = LercParams::default();
        assert!(p.is_lossless());
        assert_eq!(p.max_z_error, 0.0);
        assert_eq!(p.data_type, LercDataType::Float);
    }

    #[test]
    fn test_lerc_params_lossy() {
        let p = LercParams::new(0.5, LercDataType::Float);
        assert!(!p.is_lossless());
    }

    // -- is_lerc / version --

    #[test]
    fn test_is_lerc_positive() {
        let mut data = vec![0u8; 32];
        data[..6].copy_from_slice(LERC2_MAGIC);
        assert!(LercCodec::is_lerc(&data));
    }

    #[test]
    fn test_is_lerc_negative() {
        assert!(!LercCodec::is_lerc(b"PNG\x89\x50\x4E"));
        assert!(!LercCodec::is_lerc(b""));
    }

    #[test]
    fn test_version_extraction() {
        let mut data = vec![0u8; 32];
        data[..6].copy_from_slice(LERC2_MAGIC);
        data[6] = 2;
        data[7] = 0;
        assert_eq!(LercCodec::version(&data), Some(2));
    }

    #[test]
    fn test_version_none_for_non_lerc() {
        assert!(LercCodec::version(b"notlerc").is_none());
    }

    // -- Header parsing --

    #[test]
    fn test_parse_header_too_short() {
        let result = LercCodec::parse_header(b"Lerc2 \x02\x00\x06");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_header_wrong_magic() {
        let data = vec![0u8; 32];
        let result = LercCodec::parse_header(&data);
        assert!(result.is_err());
    }

    /// A crate-format header whose payload is shorter than the raw-value layout
    /// (i.e. it is not our raw format and its version field is not a recognised
    /// real LERC2 version) must return an explicit error instead of silently
    /// decoding to an all-zero raster of the correct shape.
    #[test]
    fn test_decode_bit_stuffed_payload_errors_not_zeros() {
        // Header: magic(6) + version(2) + dt=Float(1) + nDim=1(4) + nCols=2(4)
        //         + nRows=2(4) + nBands=1(4) = 25 bytes, then a few filler bytes
        // to clear LERC2_MIN_HEADER (30) while staying well below the 41-byte
        // raw-value payload size (25 + 4 pixels * 4 bytes), forcing the
        // bit-stuffed branch.
        let mut data = Vec::new();
        data.extend_from_slice(LERC2_MAGIC);
        data.extend_from_slice(&2u16.to_le_bytes()); // version
        data.push(LercDataType::Float.code()); // data type
        data.extend_from_slice(&1u32.to_le_bytes()); // nDim
        data.extend_from_slice(&2u32.to_le_bytes()); // nCols
        data.extend_from_slice(&2u32.to_le_bytes()); // nRows
        data.extend_from_slice(&1u32.to_le_bytes()); // nBands
        data.extend_from_slice(&[0u8; 6]); // filler to exceed LERC2_MIN_HEADER

        let err = LercCodec::decode(&data).expect_err("short payload must error");
        let msg = format!("{err}");
        assert!(
            msg.contains("refusing to fabricate output"),
            "expected an explicit fail-loud error, got: {msg}"
        );
    }

    // -- Encode/decode roundtrip: Float --

    #[test]
    fn test_encode_decode_roundtrip_float() {
        let values: Vec<f64> = (0..12).map(|i| i as f64 * 1.5).collect();
        let params = LercParams {
            max_z_error: 0.0,
            data_type: LercDataType::Float,
        };
        let encoded = LercCodec::encode(&values, 4, 3, 1, &params).expect("encode");
        let (decoded, w, h, b) = LercCodec::decode(&encoded).expect("decode");

        assert_eq!(w, 4);
        assert_eq!(h, 3);
        assert_eq!(b, 1);
        assert_eq!(decoded.len(), 12);
        for (orig, dec) in values.iter().zip(decoded.iter()) {
            assert!((orig - dec).abs() < 1e-4, "mismatch: {orig} vs {dec}");
        }
    }

    // -- Encode/decode roundtrip: Double --

    #[test]
    fn test_encode_decode_roundtrip_double() {
        let values: Vec<f64> = (0..6).map(|i| i as f64 * std::f64::consts::PI).collect();
        let params = LercParams {
            max_z_error: 0.0,
            data_type: LercDataType::Double,
        };
        let encoded = LercCodec::encode(&values, 2, 3, 1, &params).expect("encode");
        let (decoded, w, h, b) = LercCodec::decode(&encoded).expect("decode");

        assert_eq!((w, h, b), (2, 3, 1));
        for (o, d) in values.iter().zip(decoded.iter()) {
            assert!((o - d).abs() < 1e-10);
        }
    }

    // -- Encode/decode roundtrip: Short --

    #[test]
    fn test_encode_decode_roundtrip_short() {
        let values: Vec<f64> = vec![-100.0, 0.0, 100.0, 200.0];
        let params = LercParams {
            max_z_error: 0.0,
            data_type: LercDataType::Short,
        };
        let encoded = LercCodec::encode(&values, 2, 2, 1, &params).expect("encode");
        let (decoded, ..) = LercCodec::decode(&encoded).expect("decode");
        for (o, d) in values.iter().zip(decoded.iter()) {
            assert!((o - d).abs() < 1.0);
        }
    }

    // -- Multi-band encode/decode --

    #[test]
    fn test_encode_decode_multiband() {
        let values: Vec<f64> = (0..24).map(|i| i as f64).collect(); // 4x3x2
        let params = LercParams::default();
        let encoded = LercCodec::encode(&values, 4, 3, 2, &params).expect("encode");
        let (decoded, w, h, b) = LercCodec::decode(&encoded).expect("decode");
        assert_eq!((w, h, b), (4, 3, 2));
        assert_eq!(decoded.len(), 24);
    }

    // -- Wrong size error --

    #[test]
    fn test_encode_wrong_size_error() {
        let values = vec![1.0f64; 10]; // wrong: should be 4*3*1=12
        let result = LercCodec::encode(&values, 4, 3, 1, &LercParams::default());
        assert!(result.is_err());
    }

    // -- parse_header success --

    #[test]
    fn test_parse_header_roundtrip() {
        let values: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0];
        let params = LercParams::default();
        let encoded = LercCodec::encode(&values, 2, 2, 1, &params).expect("encode");
        let info = LercCodec::parse_header(&encoded).expect("parse_header");
        assert_eq!(info.n_cols, 2);
        assert_eq!(info.n_rows, 2);
        assert_eq!(info.n_bands, 1);
        assert_eq!(info.version, 2);
        assert_eq!(info.pixel_count(), 4);
    }

    // -- decompress_lerc / compress_lerc wrappers --

    #[test]
    fn test_decompress_lerc_wrapper() {
        let values: Vec<f64> = (0..4).map(|i| i as f64).collect();
        let encoded = LercCodec::encode(&values, 2, 2, 1, &LercParams::default()).expect("encode");
        let out = decompress_lerc(&encoded, 16).expect("decompress_lerc");
        // Returns 4 f32 values = 16 bytes
        assert_eq!(out.len(), 16);
    }

    #[test]
    fn test_compress_lerc_invalid_non_multiple_of_4() {
        let result = compress_lerc(&[0u8, 1u8, 2u8], 1, 1, 1);
        assert!(result.is_err());
    }
}
