//! BigTIFF write support — 64-bit offsets, >4GB output
//!
//! This module provides helpers for deciding when to use BigTIFF format
//! and for writing BigTIFF-compliant headers and IFD entries.
//!
//! # Format summary
//! A BigTIFF file starts with a 16-byte header:
//! - bytes 0–1: byte-order mark (`II` = little-endian, `MM` = big-endian)
//! - bytes 2–3: magic `0x002B` (43)
//! - bytes 4–5: offset-byte size = `8`
//! - bytes 6–7: constant `0`
//! - bytes 8–15: first IFD offset (u64)
//!
//! Each IFD entry is 20 bytes:
//! - 2 bytes: tag
//! - 2 bytes: field type
//! - 8 bytes: count (u64)
//! - 8 bytes: value or offset (u64)

use std::io::{Seek, SeekFrom, Write};

use oxigeo_core::error::{OxiGeoError, Result};

use crate::tiff::ByteOrderType;

/// BigTIFF version magic number (43 = 0x002B).
const TIFF_VERSION_BIGTIFF: u16 = 43;

/// Classic TIFF maximum addressable size (4 GiB).
///
/// Files larger than this cannot be expressed using 32-bit offsets and
/// require BigTIFF format.
pub const CLASSIC_TIFF_LIMIT: u64 = 4_294_967_296;

/// BigTIFF IFD entry size in bytes (tag 2 + type 2 + count 8 + value/offset 8).
pub const BIGTIFF_IFD_ENTRY_SIZE: usize = 20;

/// BigTIFF header size in bytes.
pub const BIGTIFF_HEADER_SIZE: usize = 16;

/// Controls when BigTIFF format is used during write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BigTiffMode {
    /// Use BigTIFF only when the projected file size exceeds [`CLASSIC_TIFF_LIMIT`].
    ///
    /// This is the default; small files are written as classic TIFF.
    #[default]
    Auto,
    /// Always use BigTIFF, regardless of file size.
    Force,
    /// Never use BigTIFF.
    ///
    /// Returns an error if the projected file size would exceed [`CLASSIC_TIFF_LIMIT`].
    Disable,
}

/// A BigTIFF file header ready to be serialized.
///
/// The header layout is fixed at 16 bytes.  Use [`BigTiffHeader::write_to`] to
/// write it to any [`Write + Seek`] target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigTiffHeader {
    /// Byte order used for all multi-byte values in the file.
    pub byte_order: ByteOrderType,
    /// Offset (from start of file) of the first Image File Directory.
    pub first_ifd_offset: u64,
}

impl BigTiffHeader {
    /// Creates a new `BigTiffHeader`.
    #[must_use]
    pub const fn new(byte_order: ByteOrderType, first_ifd_offset: u64) -> Self {
        Self {
            byte_order,
            first_ifd_offset,
        }
    }

    /// Serialises the header and writes it to `w`, seeking to position 0 first.
    ///
    /// After the call the write cursor is positioned at byte 16 (end of header).
    ///
    /// # Errors
    /// Returns an error if any I/O operation fails.
    pub fn write_to<W: Write + Seek>(&self, w: &mut W) -> Result<()> {
        w.seek(SeekFrom::Start(0))
            .map_err(|e| OxiGeoError::Io(e.into()))?;

        let bytes = self.to_bytes();
        w.write_all(&bytes).map_err(|e| OxiGeoError::Io(e.into()))
    }

    /// Serialises the header to a fixed 16-byte array.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; BIGTIFF_HEADER_SIZE] {
        let mut buf = [0u8; BIGTIFF_HEADER_SIZE];

        // Byte-order mark
        match self.byte_order {
            ByteOrderType::LittleEndian => {
                buf[0] = 0x49; // 'I'
                buf[1] = 0x49; // 'I'
            }
            ByteOrderType::BigEndian => {
                buf[0] = 0x4D; // 'M'
                buf[1] = 0x4D; // 'M'
            }
        }

        // BigTIFF magic (43)
        self.byte_order
            .write_u16(&mut buf[2..4], TIFF_VERSION_BIGTIFF);

        // Offset-byte size = 8
        self.byte_order.write_u16(&mut buf[4..6], 8);

        // Constant = 0 (already zero-initialised)

        // First IFD offset
        self.byte_order
            .write_u64(&mut buf[8..16], self.first_ifd_offset);

        buf
    }
}

/// A single BigTIFF IFD entry (20 bytes).
///
/// In BigTIFF the tag and type fields are the same width as classic TIFF,
/// but the count and value/offset fields are 8 bytes each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigTiffIfdEntry {
    /// Tag identifier.
    pub tag: u16,
    /// TIFF field type (e.g. 4 = LONG, 16 = LONG8).
    pub type_: u16,
    /// Number of values.
    pub count: u64,
    /// Inline value or file offset to the value data.
    pub value_or_offset: u64,
}

impl BigTiffIfdEntry {
    /// Creates a new `BigTiffIfdEntry`.
    #[must_use]
    pub const fn new(tag: u16, type_: u16, count: u64, value_or_offset: u64) -> Self {
        Self {
            tag,
            type_,
            count,
            value_or_offset,
        }
    }

    /// Writes the 20-byte IFD entry to `w` using the given byte order.
    ///
    /// # Errors
    /// Returns an error if any I/O operation fails.
    pub fn write_to<W: Write>(&self, w: &mut W, byte_order: ByteOrderType) -> Result<()> {
        let mut buf = [0u8; BIGTIFF_IFD_ENTRY_SIZE];

        byte_order.write_u16(&mut buf[0..2], self.tag);
        byte_order.write_u16(&mut buf[2..4], self.type_);
        byte_order.write_u64(&mut buf[4..12], self.count);
        byte_order.write_u64(&mut buf[12..20], self.value_or_offset);

        w.write_all(&buf).map_err(|e| OxiGeoError::Io(e.into()))
    }
}

/// Projects the raw pixel-data size for a raster image (no headers, no tiles).
///
/// This is a lower-bound estimate used to decide whether BigTIFF is required.
/// Actual files are always slightly larger (header, IFDs, tile padding).
#[must_use]
pub fn project_file_size(width: u64, height: u64, bands: u64, bytes_per_sample: u64) -> u64 {
    width
        .saturating_mul(height)
        .saturating_mul(bands)
        .saturating_mul(bytes_per_sample)
}

/// Decides whether BigTIFF should be used for the given raster dimensions and mode.
///
/// | `mode`          | projected size ≤ 4 GiB | projected size > 4 GiB |
/// |-----------------|------------------------|------------------------|
/// | `Auto`          | `Ok(false)`            | `Ok(true)`             |
/// | `Force`         | `Ok(true)`             | `Ok(true)`             |
/// | `Disable`       | `Ok(false)`            | `Err(...)`             |
///
/// # Errors
/// Returns an error when `mode` is [`BigTiffMode::Disable`] and the projected
/// size exceeds [`CLASSIC_TIFF_LIMIT`].
pub fn needs_bigtiff(
    width: u64,
    height: u64,
    bands: u64,
    bytes_per_sample: u64,
    mode: BigTiffMode,
) -> Result<bool> {
    let projected = project_file_size(width, height, bands, bytes_per_sample);
    let exceeds_limit = projected > CLASSIC_TIFF_LIMIT;

    match mode {
        BigTiffMode::Auto => Ok(exceeds_limit),
        BigTiffMode::Force => Ok(true),
        BigTiffMode::Disable => {
            if exceeds_limit {
                Err(OxiGeoError::invalid_parameter_builder(
                    "bigtiff_mode",
                    "Projected file size exceeds 4 GiB classic-TIFF limit but BigTIFF is disabled",
                )
                .with_operation("needs_bigtiff")
                .with_parameter("projected_bytes", projected.to_string())
                .with_parameter("limit_bytes", CLASSIC_TIFF_LIMIT.to_string())
                .with_parameter("width", width.to_string())
                .with_parameter("height", height.to_string())
                .with_parameter("bands", bands.to_string())
                .with_parameter("bytes_per_sample", bytes_per_sample.to_string())
                .with_suggestion(
                    "Use BigTiffMode::Auto or BigTiffMode::Force to enable BigTIFF output",
                )
                .build())
            } else {
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicU64, Ordering};

    /// An RAII fixture path inside [`std::env::temp_dir`].
    ///
    /// The leaf name embeds the process id and a monotonic counter, so no two
    /// test binaries — nor two concurrent runs of this one — can ever land on
    /// the same file.  Dropping the guard removes the fixture, so a panicking
    /// test leaks nothing.
    struct TempPath(std::path::PathBuf);

    impl TempPath {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!(
                "oxigeo_geotiff_bigtiff_{}_{seq}_{name}",
                std::process::id()
            )))
        }
    }

    impl AsRef<std::path::Path> for TempPath {
        fn as_ref(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    // -----------------------------------------------------------------------
    // needs_bigtiff — pure logic tests (no I/O)
    // -----------------------------------------------------------------------

    #[test]
    fn test_bigtiff_auto_mode_triggers_above_threshold() {
        assert!(
            needs_bigtiff(100_000, 100_000, 4, 4, BigTiffMode::Auto)
                .expect("Auto mode should not error")
        );
    }

    #[test]
    fn test_bigtiff_auto_mode_no_trigger_small() {
        assert!(
            !needs_bigtiff(100, 100, 1, 1, BigTiffMode::Auto)
                .expect("Auto mode should not error for small file")
        );
    }

    #[test]
    fn test_bigtiff_force_mode_small_file() {
        assert!(
            needs_bigtiff(10, 10, 1, 1, BigTiffMode::Force).expect("Force mode should not error")
        );
    }

    #[test]
    fn test_bigtiff_disable_mode_errors_on_oversize() {
        assert!(needs_bigtiff(100_000, 100_000, 4, 4, BigTiffMode::Disable).is_err());
    }

    #[test]
    fn test_bigtiff_disable_mode_ok_for_small_file() {
        assert!(
            !needs_bigtiff(100, 100, 1, 1, BigTiffMode::Disable)
                .expect("Disable mode should not error for small file")
        );
    }

    // -----------------------------------------------------------------------
    // project_file_size
    // -----------------------------------------------------------------------

    #[test]
    fn test_project_file_size_basic() {
        // 1024 * 1024 * 1 * 1 = 1 048 576 bytes
        assert_eq!(project_file_size(1024, 1024, 1, 1), 1_048_576);
    }

    #[test]
    fn test_project_file_size_exceeds_limit() {
        // 100 000 * 100 000 * 4 * 4 = 160 000 000 000 bytes > 4 GiB
        let size = project_file_size(100_000, 100_000, 4, 4);
        assert!(size > CLASSIC_TIFF_LIMIT);
    }

    #[test]
    fn test_project_file_size_saturation() {
        // Should not panic on extreme values
        let _ = project_file_size(u64::MAX, u64::MAX, u64::MAX, u64::MAX);
    }

    // -----------------------------------------------------------------------
    // BigTiffHeader byte layout
    // -----------------------------------------------------------------------

    #[test]
    fn test_bigtiff_header_magic_le() {
        let header = BigTiffHeader::new(ByteOrderType::LittleEndian, 16);
        let bytes = header.to_bytes();

        // byte-order mark
        assert_eq!(bytes[0], 0x49); // 'I'
        assert_eq!(bytes[1], 0x49); // 'I'
        // BigTIFF magic 0x002B (little-endian)
        assert_eq!(bytes[2], 0x2B);
        assert_eq!(bytes[3], 0x00);
        // offset-byte size = 8 (little-endian)
        assert_eq!(bytes[4], 0x08);
        assert_eq!(bytes[5], 0x00);
        // constant = 0
        assert_eq!(bytes[6], 0x00);
        assert_eq!(bytes[7], 0x00);
        // first IFD offset = 16 (little-endian u64)
        assert_eq!(bytes[8], 0x10);
        assert_eq!(bytes[9], 0x00);
    }

    #[test]
    fn test_bigtiff_header_magic_be() {
        let header = BigTiffHeader::new(ByteOrderType::BigEndian, 16);
        let bytes = header.to_bytes();

        assert_eq!(bytes[0], 0x4D); // 'M'
        assert_eq!(bytes[1], 0x4D); // 'M'
        // BigTIFF magic 0x002B (big-endian)
        assert_eq!(bytes[2], 0x00);
        assert_eq!(bytes[3], 0x2B);
    }

    #[test]
    fn test_bigtiff_header_roundtrip_via_tiff_header() {
        use crate::tiff::TiffHeader;

        let header = BigTiffHeader::new(ByteOrderType::LittleEndian, 16);
        let bytes = header.to_bytes();

        // The existing TiffHeader parser must accept what we produce.
        let parsed = TiffHeader::parse(&bytes).expect("BigTiffHeader bytes must be parseable");
        assert!(parsed.is_bigtiff());
        assert_eq!(parsed.first_ifd_offset, 16);
        assert_eq!(parsed.byte_order, ByteOrderType::LittleEndian);
    }

    // -----------------------------------------------------------------------
    // BigTiffIfdEntry byte layout
    // -----------------------------------------------------------------------

    #[test]
    fn test_bigtiff_ifd_entry_layout() {
        use std::io::Cursor;

        let entry = BigTiffIfdEntry::new(256, 4, 1, 1024);
        let mut buf = Vec::new();
        entry
            .write_to(&mut buf, ByteOrderType::LittleEndian)
            .expect("write should succeed");

        assert_eq!(buf.len(), BIGTIFF_IFD_ENTRY_SIZE);

        // tag = 256 (LE)
        assert_eq!(u16::from_le_bytes([buf[0], buf[1]]), 256);
        // type = 4 (LE)
        assert_eq!(u16::from_le_bytes([buf[2], buf[3]]), 4);
        // count = 1 (LE u64)
        assert_eq!(
            u64::from_le_bytes(
                buf[4..12]
                    .try_into()
                    .expect("slice must be exactly 8 bytes")
            ),
            1
        );
        // value_or_offset = 1024 (LE u64)
        assert_eq!(
            u64::from_le_bytes(
                buf[12..20]
                    .try_into()
                    .expect("slice must be exactly 8 bytes")
            ),
            1024
        );

        // Ensure write_to<W: Write> also works with Cursor
        let mut cursor = Cursor::new(Vec::new());
        entry
            .write_to(&mut cursor, ByteOrderType::LittleEndian)
            .expect("write to cursor should succeed");
        assert_eq!(cursor.into_inner().len(), BIGTIFF_IFD_ENTRY_SIZE);
    }

    // -----------------------------------------------------------------------
    // End-to-end: write a Force-mode GeoTIFF and verify BigTIFF bytes
    // -----------------------------------------------------------------------

    #[test]
    fn test_bigtiff_header_magic_0x002b() {
        use std::io::{Read, Seek, SeekFrom};

        use crate::tiff::Compression;
        use crate::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
        use oxigeo_core::types::RasterDataType;

        let tmp = TempPath::new("magic.tif");

        let config = WriterConfig::new(16, 16, 1, RasterDataType::UInt8)
            .with_compression(Compression::None)
            .with_tile_size(16, 16)
            .with_bigtiff(true);

        let options = GeoTiffWriterOptions {
            bigtiff_mode: BigTiffMode::Force,
            ..Default::default()
        };

        let mut writer =
            GeoTiffWriter::create(&tmp, config, options).expect("writer creation should succeed");

        let data = vec![0u8; 16 * 16];
        writer.write(&data).expect("write should succeed");

        // Verify BigTIFF magic bytes in the output file
        let mut f = std::fs::File::open(&tmp).expect("output file should exist");
        let mut buf = [0u8; 8];
        f.seek(SeekFrom::Start(0)).expect("seek should succeed");
        f.read_exact(&mut buf).expect("read_exact should succeed");

        // bytes 0-1: 'II' (little-endian)
        assert_eq!(buf[0], 0x49);
        assert_eq!(buf[1], 0x49);
        // bytes 2-3: 0x002B (BigTIFF magic, little-endian)
        assert_eq!(buf[2], 0x2B);
        assert_eq!(buf[3], 0x00);
        // bytes 4-5: offset-byte size = 8
        assert_eq!(buf[4], 0x08);
        assert_eq!(buf[5], 0x00);
        // bytes 6-7: constant = 0
        assert_eq!(buf[6], 0x00);
        assert_eq!(buf[7], 0x00);

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_bigtiff_force_mode_header_bytes() {
        use std::io::{Read, Seek, SeekFrom};

        use crate::tiff::Compression;
        use crate::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
        use oxigeo_core::types::RasterDataType;

        let tmp = TempPath::new("force_header.tif");

        let config = WriterConfig::new(32, 32, 1, RasterDataType::UInt8)
            .with_compression(Compression::None)
            .with_tile_size(32, 32)
            .with_bigtiff(true);

        let options = GeoTiffWriterOptions {
            bigtiff_mode: BigTiffMode::Force,
            ..Default::default()
        };

        let mut writer =
            GeoTiffWriter::create(&tmp, config, options).expect("writer creation should succeed");

        let data = vec![42u8; 32 * 32];
        writer.write(&data).expect("write should succeed");

        let mut f = std::fs::File::open(&tmp).expect("output file should exist");
        let mut header_buf = [0u8; 16];
        f.seek(SeekFrom::Start(0)).expect("seek should succeed");
        f.read_exact(&mut header_buf)
            .expect("read_exact should succeed");

        // bytes 0-1: II (little-endian byte order)
        assert_eq!(&header_buf[0..2], b"II");
        // bytes 2-3: 0x002B (BigTIFF magic, LE)
        assert_eq!(header_buf[2], 0x2B);
        assert_eq!(header_buf[3], 0x00);
        // bytes 4-5: offset-byte size = 8
        assert_eq!(header_buf[4], 0x08);
        assert_eq!(header_buf[5], 0x00);
        // bytes 6-7: constant = 0
        assert_eq!(header_buf[6], 0x00);
        assert_eq!(header_buf[7], 0x00);
        // bytes 8-15: first IFD offset as u64 (must be >= 16)
        let ifd_offset = u64::from_le_bytes(
            header_buf[8..16]
                .try_into()
                .expect("slice must be exactly 8 bytes"),
        );
        assert!(ifd_offset >= 16, "first IFD offset must be >= header size");

        let _ = std::fs::remove_file(&tmp);
    }
}
