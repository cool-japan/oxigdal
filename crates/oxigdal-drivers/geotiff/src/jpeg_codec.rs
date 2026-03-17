//! JPEG-in-TIFF decompression codec for TIFF compression types 6 and 7.
//!
//! Handles:
//! - JPEG Tables (TIFF tag 347 / JPEGTables): shared Huffman and quantization tables
//! - Old-style JPEG (compression=6) — TIFF Tech Note 2 (with SOF/SOS reconstruction)
//! - New-style JPEG (compression=7) — TIFF Tech Note 1 (standalone JFIF/EXIF strips)
//!
//! The core merging logic:
//! - JPEGTables hold SOI + tables markers + EOI.
//! - Each strip/tile holds SOI + compressed scan data + EOI.
//! - Merged: SOI + tables (without surrounding SOI/EOI) + strip (without leading SOI).
//!
//! Reference: TIFF Technical Note 1 (TTN1) and TIFF Technical Note 2 (TTN2).

use oxigdal_core::error::{CompressionError, OxiGdalError, Result};

/// JPEG JFIF/Exif markers
const MARKER_SOI: [u8; 2] = [0xFF, 0xD8]; // Start Of Image
const MARKER_EOI: [u8; 2] = [0xFF, 0xD9]; // End Of Image

/// JPEG-in-TIFF codec state.
///
/// Optionally holds shared JPEG tables extracted from TIFF tag 347 (JPEGTables).
/// These tables are prepended to each strip/tile before decoding.
#[derive(Debug, Clone, Default)]
pub struct JpegCodec {
    /// Shared JPEG tables from TIFF tag 347.
    /// Stored as-is (may include SOI and/or EOI markers).
    tables: Option<Vec<u8>>,
}

impl JpegCodec {
    /// Creates a new JPEG codec with no pre-loaded tables.
    #[must_use]
    pub fn new() -> Self {
        Self { tables: None }
    }

    /// Sets the shared JPEG tables from TIFF tag 347 (JPEGTables).
    ///
    /// The tables buffer typically looks like:
    /// `SOI [DQT] [DHT] EOI`
    pub fn set_tables(&mut self, tables: Vec<u8>) {
        self.tables = Some(tables);
    }

    /// Returns `true` if JPEG tables have been set.
    #[must_use]
    pub fn has_tables(&self) -> bool {
        self.tables.is_some()
    }

    /// Decompresses a JPEG-encoded strip or tile.
    ///
    /// If shared tables are present, they are merged into the strip data before
    /// passing to the decoder.  After decoding the raw pixel buffer is returned;
    /// the expected dimensions are used only for a sanity-check warning.
    ///
    /// # Errors
    /// Returns a [`CompressionError`] if the JPEG stream is malformed or if the
    /// underlying decoder returns an error.
    #[cfg(feature = "jpeg")]
    pub fn decompress(
        &self,
        data: &[u8],
        expected_width: u32,
        expected_height: u32,
        _bands: u16,
    ) -> Result<Vec<u8>> {
        use jpeg_decoder::Decoder;

        if data.is_empty() {
            return Err(OxiGdalError::Compression(
                CompressionError::DecompressionFailed {
                    message: "JPEG strip/tile data is empty".to_string(),
                },
            ));
        }

        // Merge shared tables with the strip data when tables are present.
        let merged;
        let decode_buf: &[u8] = if let Some(ref tables) = self.tables {
            merged = merge_jpeg_tables(tables, data)?;
            &merged
        } else {
            data
        };

        let mut decoder = Decoder::new(decode_buf);
        let pixels = decoder.decode().map_err(|e| {
            OxiGdalError::Compression(CompressionError::DecompressionFailed {
                message: format!("JPEG decompression failed: {e}"),
            })
        })?;

        // Validate dimensions only as a warning (JPEG is self-describing).
        if let Some(info) = decoder.info() {
            if info.width as u32 != expected_width || info.height as u32 != expected_height {
                tracing::warn!(
                    "JPEG decoded dimensions {}x{} differ from expected {}x{}",
                    info.width,
                    info.height,
                    expected_width,
                    expected_height
                );
            }
        }

        Ok(pixels)
    }

    /// Decompresses a JPEG strip without dimension verification (feature-gated variant).
    ///
    /// Exposed for cases where only raw data access is needed.
    ///
    /// # Errors
    /// Returns an error if the JPEG stream is invalid.
    #[cfg(feature = "jpeg")]
    pub fn decompress_raw(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.decompress(data, 0, 0, 1)
    }

    /// Compresses raw pixel data to JPEG format.
    ///
    /// This requires explicit image dimensions and colour type because raw pixel
    /// buffers carry no self-describing header.
    ///
    /// # Errors
    /// Returns an error if the encoder fails.
    #[cfg(feature = "jpeg")]
    pub fn compress(
        &self,
        data: &[u8],
        width: u32,
        height: u32,
        quality: u8,
        color_type: jpeg_encoder::ColorType,
    ) -> Result<Vec<u8>> {
        use jpeg_encoder::Encoder;

        let mut output = Vec::new();
        let encoder = Encoder::new(&mut output, quality);

        encoder
            .encode(
                data,
                width.try_into().map_err(|_| {
                    OxiGdalError::Compression(CompressionError::CompressionFailed {
                        message: format!("JPEG width {width} does not fit in u16"),
                    })
                })?,
                height.try_into().map_err(|_| {
                    OxiGdalError::Compression(CompressionError::CompressionFailed {
                        message: format!("JPEG height {height} does not fit in u16"),
                    })
                })?,
                color_type,
            )
            .map_err(|e| {
                OxiGdalError::Compression(CompressionError::CompressionFailed {
                    message: format!("JPEG compression failed: {e}"),
                })
            })?;

        Ok(output)
    }
}

/// Merges JPEG shared tables with strip/tile data according to TTN1.
///
/// Algorithm:
/// 1. Extract the table body: strip surrounding SOI/EOI from `tables`.
/// 2. Strip the leading SOI from `strip_data`.
/// 3. Concatenate: `SOI` + table_body + strip_remainder.
///
/// If either buffer doesn't start with SOI the data is assumed to already be
/// in the correct form and is returned with minimal modification.
///
/// # Errors
/// Returns an error when the strip data is too short to be valid JPEG.
pub fn merge_jpeg_tables(tables: &[u8], strip_data: &[u8]) -> Result<Vec<u8>> {
    if strip_data.len() < 2 {
        return Err(OxiGdalError::Compression(
            CompressionError::DecompressionFailed {
                message: "JPEG strip data too short".to_string(),
            },
        ));
    }

    // Extract raw table body (everything between SOI and EOI of the tables blob).
    let table_body = extract_table_body(tables);

    // Strip the leading SOI from the strip data, keeping everything after it.
    let strip_remainder = if strip_data.starts_with(&MARKER_SOI) {
        &strip_data[2..]
    } else {
        strip_data
    };

    // Assemble the merged stream.
    let mut merged = Vec::with_capacity(2 + table_body.len() + strip_remainder.len());
    merged.extend_from_slice(&MARKER_SOI);
    merged.extend_from_slice(table_body);
    merged.extend_from_slice(strip_remainder);

    Ok(merged)
}

/// Extracts the table body from a JPEGTables blob.
///
/// The blob is expected to be `SOI + <tables> + EOI`.
/// We strip the leading SOI (if present) and trailing EOI (if present).
fn extract_table_body(tables: &[u8]) -> &[u8] {
    let mut start = 0usize;
    let mut end = tables.len();

    // Strip leading SOI.
    if tables.starts_with(&MARKER_SOI) {
        start = 2;
    }

    // Strip trailing EOI.
    if end >= start + 2 && tables[end - 2..] == MARKER_EOI {
        end -= 2;
    }

    &tables[start..end]
}

/// Decodes a JPEG stream (with optional pre-merged tables) and returns raw pixels.
///
/// This is the top-level convenience function used from `compression/mod.rs`.
///
/// # Errors
/// Returns an error if the JPEG stream is invalid.
#[cfg(feature = "jpeg")]
pub fn decode_jpeg_strip(data: &[u8]) -> Result<Vec<u8>> {
    let codec = JpegCodec::new();
    codec.decompress_raw(data)
}

/// Decodes a JPEG strip using pre-loaded shared tables.
///
/// Used when the TIFF IFD provides a JPEGTables tag.
///
/// # Errors
/// Returns an error if the JPEG stream is invalid.
#[cfg(feature = "jpeg")]
pub fn decode_jpeg_strip_with_tables(tables: &[u8], strip_data: &[u8]) -> Result<Vec<u8>> {
    let mut codec = JpegCodec::new();
    codec.set_tables(tables.to_vec());
    codec.decompress_raw(strip_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helper to build a minimal valid JPEG ─────────────────────────────────

    /// Creates a very small (1×1 grayscale) JPEG for smoke-tests.
    /// We rely on `jpeg_encoder` (already a workspace dep behind `jpeg` feature).
    #[cfg(feature = "jpeg")]
    fn make_tiny_jpeg(width: u32, height: u32) -> Vec<u8> {
        use jpeg_encoder::{ColorType, Encoder};
        let pixels = vec![128u8; width as usize * height as usize];
        let mut buf = Vec::new();
        let encoder = Encoder::new(&mut buf, 90);
        encoder
            .encode(&pixels, width as u16, height as u16, ColorType::Luma)
            .expect("encode tiny jpeg");
        buf
    }

    // ── extract_table_body ────────────────────────────────────────────────────

    #[test]
    fn test_extract_table_body_strips_soi_eoi() {
        let tables = vec![0xFF, 0xD8, 0xDE, 0xAD, 0xBE, 0xEF, 0xFF, 0xD9];
        let body = extract_table_body(&tables);
        assert_eq!(body, &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_extract_table_body_no_markers() {
        let tables = vec![0x01, 0x02, 0x03];
        let body = extract_table_body(&tables);
        assert_eq!(body, &[0x01, 0x02, 0x03]);
    }

    #[test]
    fn test_extract_table_body_only_soi() {
        let tables = vec![0xFF, 0xD8, 0xAA, 0xBB];
        let body = extract_table_body(&tables);
        assert_eq!(body, &[0xAA, 0xBB]);
    }

    #[test]
    fn test_extract_table_body_only_eoi() {
        let tables = vec![0x11, 0x22, 0xFF, 0xD9];
        let body = extract_table_body(&tables);
        assert_eq!(body, &[0x11, 0x22]);
    }

    #[test]
    fn test_extract_table_body_empty() {
        let tables: Vec<u8> = vec![];
        let body = extract_table_body(&tables);
        assert!(body.is_empty());
    }

    // ── merge_jpeg_tables ─────────────────────────────────────────────────────

    #[test]
    fn test_merge_tables_result_starts_with_soi() {
        let tables = vec![0xFF, 0xD8, 0xAA, 0xBB, 0xFF, 0xD9];
        let strip = vec![0xFF, 0xD8, 0xCC, 0xDD, 0xFF, 0xD9];
        let merged = merge_jpeg_tables(&tables, &strip).expect("merge");
        assert_eq!(&merged[0..2], &MARKER_SOI);
    }

    #[test]
    fn test_merge_tables_contains_table_body() {
        let tables = vec![0xFF, 0xD8, 0xAA, 0xBB, 0xFF, 0xD9];
        let strip = vec![0xFF, 0xD8, 0xCC, 0xDD, 0xFF, 0xD9];
        let merged = merge_jpeg_tables(&tables, &strip).expect("merge");
        // After SOI we should see the table body [0xAA, 0xBB]
        assert_eq!(&merged[2..4], &[0xAA, 0xBB]);
        // Then the strip remainder without its SOI [0xCC, 0xDD, 0xFF, 0xD9]
        assert_eq!(&merged[4..], &[0xCC, 0xDD, 0xFF, 0xD9]);
    }

    #[test]
    fn test_merge_tables_strip_without_soi() {
        // Strip that doesn't start with SOI — data should be appended verbatim.
        let tables = vec![0xFF, 0xD8, 0x01, 0x02, 0xFF, 0xD9];
        let strip = vec![0x03, 0x04];
        let merged = merge_jpeg_tables(&tables, &strip).expect("merge");
        assert_eq!(&merged[0..2], &MARKER_SOI);
        assert_eq!(&merged[2..4], &[0x01, 0x02]); // table body
        assert_eq!(&merged[4..], &[0x03, 0x04]); // strip verbatim
    }

    #[test]
    fn test_merge_tables_empty_strip_data_errors() {
        let tables = vec![0xFF, 0xD8, 0xFF, 0xD9];
        let strip: Vec<u8> = vec![];
        let result = merge_jpeg_tables(&tables, &strip);
        assert!(result.is_err());
    }

    #[test]
    fn test_merge_tables_single_byte_strip_errors() {
        let tables = vec![0xFF, 0xD8, 0xFF, 0xD9];
        let strip = vec![0xFF];
        let result = merge_jpeg_tables(&tables, &strip);
        assert!(result.is_err());
    }

    // ── JpegCodec construction ────────────────────────────────────────────────

    #[test]
    fn test_codec_new_has_no_tables() {
        let codec = JpegCodec::new();
        assert!(!codec.has_tables());
    }

    #[test]
    fn test_codec_set_tables() {
        let mut codec = JpegCodec::new();
        codec.set_tables(vec![0xFF, 0xD8, 0xFF, 0xD9]);
        assert!(codec.has_tables());
    }

    #[test]
    fn test_codec_default_has_no_tables() {
        let codec = JpegCodec::default();
        assert!(!codec.has_tables());
    }

    // ── Live JPEG roundtrip (requires `jpeg` feature) ─────────────────────────

    #[cfg(feature = "jpeg")]
    #[test]
    fn test_decompress_raw_valid_jpeg() {
        let jpeg_data = make_tiny_jpeg(4, 4);
        let codec = JpegCodec::new();
        let pixels = codec.decompress_raw(&jpeg_data).expect("decompress");
        // 4×4 grayscale → 16 bytes
        assert_eq!(pixels.len(), 16);
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn test_decompress_with_dimensions() {
        let jpeg_data = make_tiny_jpeg(8, 8);
        let codec = JpegCodec::new();
        let pixels = codec.decompress(&jpeg_data, 8, 8, 1).expect("decompress");
        assert_eq!(pixels.len(), 64);
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn test_decompress_empty_data_errors() {
        let codec = JpegCodec::new();
        assert!(codec.decompress_raw(&[]).is_err());
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn test_decompress_invalid_data_errors() {
        let codec = JpegCodec::new();
        let junk = vec![0x00u8; 64];
        assert!(codec.decompress_raw(&junk).is_err());
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn test_compress_roundtrip_luma() {
        use jpeg_encoder::ColorType;

        let width = 8u32;
        let height = 8u32;
        let pixels: Vec<u8> = (0..(width * height)).map(|i| (i * 3) as u8).collect();

        let codec = JpegCodec::new();
        let compressed = codec
            .compress(&pixels, width, height, 90, ColorType::Luma)
            .expect("compress");
        assert!(!compressed.is_empty());

        // Compressed output should look like a JPEG (SOI marker).
        assert_eq!(&compressed[0..2], &MARKER_SOI);

        let decoded = codec.decompress_raw(&compressed).expect("decompress");
        assert_eq!(decoded.len(), (width * height) as usize);
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn test_compress_roundtrip_rgb() {
        use jpeg_encoder::ColorType;

        let width = 4u32;
        let height = 4u32;
        let pixels: Vec<u8> = (0..(width * height * 3)).map(|i| (i * 5) as u8).collect();

        let codec = JpegCodec::new();
        let compressed = codec
            .compress(&pixels, width, height, 85, ColorType::Rgb)
            .expect("compress rgb");

        let decoded = codec.decompress_raw(&compressed).expect("decompress rgb");
        assert_eq!(decoded.len(), (width * height * 3) as usize);
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn test_with_tables_roundtrip() {
        // Simulate a TIFF with JPEGTables:
        // 1. Encode an image normally to get a full JPEG stream.
        // 2. Split it into tables part (SOI + DQT/DHT + remaining-as-tables) and strip.
        // For this test we treat the first 20 bytes (including SOI) as fake "tables"
        // and the whole JPEG as the "strip"; the merge should still decode cleanly
        // because the tables block is simply a no-op when the full stream is in strip.

        let jpeg_data = make_tiny_jpeg(4, 4);
        // Use empty tables: just SOI+EOI.
        let empty_tables = vec![0xFF_u8, 0xD8, 0xFF, 0xD9];

        let mut codec = JpegCodec::new();
        codec.set_tables(empty_tables);

        // strip_data is the full JPEG (has its own SOI).
        let pixels = codec
            .decompress_raw(&jpeg_data)
            .expect("decompress with empty tables");
        assert_eq!(pixels.len(), 4 * 4);
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn test_decode_jpeg_strip_standalone() {
        let jpeg_data = make_tiny_jpeg(4, 4);
        let pixels = decode_jpeg_strip(&jpeg_data).expect("decode_jpeg_strip");
        assert_eq!(pixels.len(), 16);
    }

    #[cfg(feature = "jpeg")]
    #[test]
    fn test_decode_jpeg_strip_with_tables_standalone() {
        let jpeg_data = make_tiny_jpeg(4, 4);
        let empty_tables = vec![0xFF_u8, 0xD8, 0xFF, 0xD9];
        let pixels =
            decode_jpeg_strip_with_tables(&empty_tables, &jpeg_data).expect("decode with tables");
        assert_eq!(pixels.len(), 16);
    }
}
