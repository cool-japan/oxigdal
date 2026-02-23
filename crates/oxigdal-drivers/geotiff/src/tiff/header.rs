//! TIFF/BigTIFF header parsing
//!
//! This module handles parsing of TIFF and BigTIFF file headers,
//! determining byte order and TIFF variant.

use byteorder::{BigEndian, ByteOrder, LittleEndian};
use oxigdal_core::error::{OxiGdalError, Result};

/// TIFF magic number for little-endian
pub const TIFF_MAGIC_LE: [u8; 2] = [0x49, 0x49]; // "II"

/// TIFF magic number for big-endian
pub const TIFF_MAGIC_BE: [u8; 2] = [0x4D, 0x4D]; // "MM"

/// Classic TIFF version number
pub const TIFF_VERSION_CLASSIC: u16 = 42;

/// BigTIFF version number
pub const TIFF_VERSION_BIGTIFF: u16 = 43;

/// Byte order of the TIFF file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrderType {
    /// Little-endian (Intel)
    LittleEndian,
    /// Big-endian (Motorola)
    BigEndian,
}

impl ByteOrderType {
    /// Reads a u16 value with this byte order
    #[must_use]
    pub fn read_u16(self, bytes: &[u8]) -> u16 {
        match self {
            Self::LittleEndian => LittleEndian::read_u16(bytes),
            Self::BigEndian => BigEndian::read_u16(bytes),
        }
    }

    /// Reads a u32 value with this byte order
    #[must_use]
    pub fn read_u32(self, bytes: &[u8]) -> u32 {
        match self {
            Self::LittleEndian => LittleEndian::read_u32(bytes),
            Self::BigEndian => BigEndian::read_u32(bytes),
        }
    }

    /// Reads a u64 value with this byte order
    #[must_use]
    pub fn read_u64(self, bytes: &[u8]) -> u64 {
        match self {
            Self::LittleEndian => LittleEndian::read_u64(bytes),
            Self::BigEndian => BigEndian::read_u64(bytes),
        }
    }

    /// Reads an i16 value with this byte order
    #[must_use]
    pub fn read_i16(self, bytes: &[u8]) -> i16 {
        match self {
            Self::LittleEndian => LittleEndian::read_i16(bytes),
            Self::BigEndian => BigEndian::read_i16(bytes),
        }
    }

    /// Reads an i32 value with this byte order
    #[must_use]
    pub fn read_i32(self, bytes: &[u8]) -> i32 {
        match self {
            Self::LittleEndian => LittleEndian::read_i32(bytes),
            Self::BigEndian => BigEndian::read_i32(bytes),
        }
    }

    /// Reads an i64 value with this byte order
    #[must_use]
    pub fn read_i64(self, bytes: &[u8]) -> i64 {
        match self {
            Self::LittleEndian => LittleEndian::read_i64(bytes),
            Self::BigEndian => BigEndian::read_i64(bytes),
        }
    }

    /// Reads an f32 value with this byte order
    #[must_use]
    pub fn read_f32(self, bytes: &[u8]) -> f32 {
        match self {
            Self::LittleEndian => LittleEndian::read_f32(bytes),
            Self::BigEndian => BigEndian::read_f32(bytes),
        }
    }

    /// Reads an f64 value with this byte order
    #[must_use]
    pub fn read_f64(self, bytes: &[u8]) -> f64 {
        match self {
            Self::LittleEndian => LittleEndian::read_f64(bytes),
            Self::BigEndian => BigEndian::read_f64(bytes),
        }
    }

    /// Writes a u16 value with this byte order
    pub fn write_u16(self, buf: &mut [u8], value: u16) {
        match self {
            Self::LittleEndian => LittleEndian::write_u16(buf, value),
            Self::BigEndian => BigEndian::write_u16(buf, value),
        }
    }

    /// Writes a u32 value with this byte order
    pub fn write_u32(self, buf: &mut [u8], value: u32) {
        match self {
            Self::LittleEndian => LittleEndian::write_u32(buf, value),
            Self::BigEndian => BigEndian::write_u32(buf, value),
        }
    }

    /// Writes a u64 value with this byte order
    pub fn write_u64(self, buf: &mut [u8], value: u64) {
        match self {
            Self::LittleEndian => LittleEndian::write_u64(buf, value),
            Self::BigEndian => BigEndian::write_u64(buf, value),
        }
    }

    /// Writes an f64 value with this byte order
    pub fn write_f64(self, buf: &mut [u8], value: f64) {
        match self {
            Self::LittleEndian => LittleEndian::write_f64(buf, value),
            Self::BigEndian => BigEndian::write_f64(buf, value),
        }
    }
}

/// TIFF variant (classic or BigTIFF)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TiffVariant {
    /// Classic TIFF (32-bit offsets, max 4GB)
    Classic,
    /// BigTIFF (64-bit offsets, unlimited size)
    BigTiff,
}

impl TiffVariant {
    /// Returns the size of an offset in bytes
    #[must_use]
    pub const fn offset_size(self) -> usize {
        match self {
            Self::Classic => 4,
            Self::BigTiff => 8,
        }
    }

    /// Returns the header size in bytes
    #[must_use]
    pub const fn header_size(self) -> usize {
        match self {
            Self::Classic => 8,
            Self::BigTiff => 16,
        }
    }

    /// Returns the IFD entry size in bytes
    #[must_use]
    pub const fn ifd_entry_size(self) -> usize {
        match self {
            Self::Classic => 12,
            Self::BigTiff => 20,
        }
    }
}

/// Parsed TIFF header
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TiffHeader {
    /// Byte order
    pub byte_order: ByteOrderType,
    /// TIFF variant
    pub variant: TiffVariant,
    /// Offset to first IFD
    pub first_ifd_offset: u64,
}

impl TiffHeader {
    /// Minimum header size needed to determine variant
    pub const MIN_HEADER_SIZE: usize = 8;

    /// BigTIFF header size
    pub const BIGTIFF_HEADER_SIZE: usize = 16;

    /// Parses a TIFF header from bytes
    ///
    /// # Arguments
    /// * `data` - At least 8 bytes for classic TIFF, 16 for BigTIFF
    ///
    /// # Errors
    /// Returns an error if the header is invalid
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < Self::MIN_HEADER_SIZE {
            return Err(OxiGdalError::io_error_builder("TIFF header too small")
                .with_operation("parse_tiff_header")
                .with_parameter("actual_size", data.len().to_string())
                .with_parameter("min_size", Self::MIN_HEADER_SIZE.to_string())
                .with_suggestion("File may be truncated or not a valid TIFF file")
                .build());
        }

        // Parse byte order
        let byte_order = match (data[0], data[1]) {
            (0x49, 0x49) => ByteOrderType::LittleEndian,
            (0x4D, 0x4D) => ByteOrderType::BigEndian,
            _ => {
                return Err(
                    OxiGdalError::io_error_builder("Invalid TIFF byte order marker")
                        .with_operation("parse_tiff_header")
                        .with_parameter("actual_bytes", format!("0x{:02X}{:02X}", data[0], data[1]))
                        .with_parameter("expected", "0x4949 (II) or 0x4D4D (MM)")
                        .with_suggestion("File is not a valid TIFF. Verify file format")
                        .build(),
                );
            }
        };

        // Parse version
        let version = byte_order.read_u16(&data[2..4]);

        let (variant, first_ifd_offset) = match version {
            TIFF_VERSION_CLASSIC => {
                let offset = u64::from(byte_order.read_u32(&data[4..8]));
                (TiffVariant::Classic, offset)
            }
            TIFF_VERSION_BIGTIFF => {
                if data.len() < Self::BIGTIFF_HEADER_SIZE {
                    return Err(OxiGdalError::io_error_builder("BigTIFF header too small")
                        .with_operation("parse_bigtiff_header")
                        .with_parameter("actual_size", data.len().to_string())
                        .with_parameter("required_size", Self::BIGTIFF_HEADER_SIZE.to_string())
                        .with_suggestion("File may be truncated. BigTIFF requires 16-byte header")
                        .build());
                }

                // Verify offset byte size (should be 8)
                let offset_byte_size = byte_order.read_u16(&data[4..6]);
                if offset_byte_size != 8 {
                    return Err(
                        OxiGdalError::io_error_builder("Invalid BigTIFF offset byte size")
                            .with_operation("parse_bigtiff_header")
                            .with_parameter("actual", offset_byte_size.to_string())
                            .with_parameter("expected", "8")
                            .with_suggestion(
                                "File header is corrupted. BigTIFF requires 8-byte offsets",
                            )
                            .build(),
                    );
                }

                // Verify constant (should be 0)
                let constant = byte_order.read_u16(&data[6..8]);
                if constant != 0 {
                    return Err(
                        OxiGdalError::io_error_builder("Invalid BigTIFF constant field")
                            .with_operation("parse_bigtiff_header")
                            .with_parameter("actual", constant.to_string())
                            .with_parameter("expected", "0")
                            .with_suggestion("File header is corrupted or non-standard")
                            .build(),
                    );
                }

                let offset = byte_order.read_u64(&data[8..16]);
                (TiffVariant::BigTiff, offset)
            }
            _ => {
                return Err(OxiGdalError::not_supported_builder(format!(
                    "TIFF version {}",
                    version
                ))
                .with_operation("parse_tiff_header")
                .with_parameter("version", version.to_string())
                .with_parameter("supported_versions", "42 (Classic TIFF), 43 (BigTIFF)")
                .with_suggestion("File uses unsupported TIFF version or may be corrupted")
                .build());
            }
        };

        Ok(Self {
            byte_order,
            variant,
            first_ifd_offset,
        })
    }

    /// Creates a classic TIFF header
    #[must_use]
    pub const fn classic(byte_order: ByteOrderType, first_ifd_offset: u32) -> Self {
        Self {
            byte_order,
            variant: TiffVariant::Classic,
            first_ifd_offset: first_ifd_offset as u64,
        }
    }

    /// Creates a BigTIFF header
    #[must_use]
    pub const fn bigtiff(byte_order: ByteOrderType, first_ifd_offset: u64) -> Self {
        Self {
            byte_order,
            variant: TiffVariant::BigTiff,
            first_ifd_offset,
        }
    }

    /// Serializes the header to bytes
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = match self.variant {
            TiffVariant::Classic => vec![0u8; 8],
            TiffVariant::BigTiff => vec![0u8; 16],
        };

        // Write byte order mark
        match self.byte_order {
            ByteOrderType::LittleEndian => {
                bytes[0] = 0x49;
                bytes[1] = 0x49;
            }
            ByteOrderType::BigEndian => {
                bytes[0] = 0x4D;
                bytes[1] = 0x4D;
            }
        }

        match self.variant {
            TiffVariant::Classic => {
                self.byte_order
                    .write_u16(&mut bytes[2..4], TIFF_VERSION_CLASSIC);
                self.byte_order
                    .write_u32(&mut bytes[4..8], self.first_ifd_offset as u32);
            }
            TiffVariant::BigTiff => {
                self.byte_order
                    .write_u16(&mut bytes[2..4], TIFF_VERSION_BIGTIFF);
                self.byte_order.write_u16(&mut bytes[4..6], 8); // Offset byte size
                self.byte_order.write_u16(&mut bytes[6..8], 0); // Constant
                self.byte_order
                    .write_u64(&mut bytes[8..16], self.first_ifd_offset);
            }
        }

        bytes
    }

    /// Returns true if this is a BigTIFF
    #[must_use]
    pub const fn is_bigtiff(&self) -> bool {
        matches!(self.variant, TiffVariant::BigTiff)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn test_parse_classic_le() {
        let data = [
            0x49, 0x49, // Little-endian
            0x2A, 0x00, // Version 42
            0x08, 0x00, 0x00, 0x00, // First IFD at offset 8
        ];

        let header = TiffHeader::parse(&data).expect("should parse");
        assert_eq!(header.byte_order, ByteOrderType::LittleEndian);
        assert_eq!(header.variant, TiffVariant::Classic);
        assert_eq!(header.first_ifd_offset, 8);
    }

    #[test]
    fn test_parse_classic_be() {
        let data = [
            0x4D, 0x4D, // Big-endian
            0x00, 0x2A, // Version 42
            0x00, 0x00, 0x00, 0x08, // First IFD at offset 8
        ];

        let header = TiffHeader::parse(&data).expect("should parse");
        assert_eq!(header.byte_order, ByteOrderType::BigEndian);
        assert_eq!(header.variant, TiffVariant::Classic);
        assert_eq!(header.first_ifd_offset, 8);
    }

    #[test]
    fn test_parse_bigtiff_le() {
        let data = [
            0x49, 0x49, // Little-endian
            0x2B, 0x00, // Version 43 (BigTIFF)
            0x08, 0x00, // Offset byte size = 8
            0x00, 0x00, // Constant = 0
            0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // First IFD at offset 16
        ];

        let header = TiffHeader::parse(&data).expect("should parse");
        assert_eq!(header.byte_order, ByteOrderType::LittleEndian);
        assert_eq!(header.variant, TiffVariant::BigTiff);
        assert_eq!(header.first_ifd_offset, 16);
        assert!(header.is_bigtiff());
    }

    #[test]
    fn test_invalid_magic() {
        let data = [0x00, 0x00, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
        assert!(TiffHeader::parse(&data).is_err());
    }

    #[test]
    fn test_invalid_version() {
        let data = [0x49, 0x49, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00];
        assert!(TiffHeader::parse(&data).is_err());
    }

    #[test]
    fn test_header_roundtrip() {
        let original = TiffHeader::classic(ByteOrderType::LittleEndian, 1024);
        let bytes = original.to_bytes();
        let parsed = TiffHeader::parse(&bytes).expect("should parse");
        assert_eq!(original, parsed);

        let original_big = TiffHeader::bigtiff(ByteOrderType::BigEndian, 0x1234_5678_9ABC_DEF0);
        let bytes_big = original_big.to_bytes();
        let parsed_big = TiffHeader::parse(&bytes_big).expect("should parse");
        assert_eq!(original_big, parsed_big);
    }
}
