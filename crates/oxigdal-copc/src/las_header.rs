//! ASPRS LAS 1.x file header parser.
//!
//! Reference: ASPRS LAS Specification 1.4 R15 (November 2019).

use crate::error::CopcError;

/// The four-byte magic that must appear at the start of every LAS file.
pub const LAS_MAGIC: &[u8] = b"LASF";

/// LAS format version.
#[derive(Debug, Clone, PartialEq)]
pub enum LasVersion {
    /// LAS 1.0
    V10,
    /// LAS 1.1
    V11,
    /// LAS 1.2
    V12,
    /// LAS 1.3
    V13,
    /// LAS 1.4
    V14,
}

impl LasVersion {
    /// Construct from major/minor version bytes.
    pub fn from_bytes(major: u8, minor: u8) -> Option<Self> {
        match (major, minor) {
            (1, 0) => Some(Self::V10),
            (1, 1) => Some(Self::V11),
            (1, 2) => Some(Self::V12),
            (1, 3) => Some(Self::V13),
            (1, 4) => Some(Self::V14),
            _ => None,
        }
    }
}

/// Parsed LAS public header block.
///
/// Field offsets and sizes are taken from the LAS 1.4 specification.
#[derive(Debug, Clone)]
pub struct LasHeader {
    /// LAS format version.
    pub version: LasVersion,
    /// System identifier (32 bytes, null-padded ASCII).
    pub system_id: [u8; 32],
    /// Generating software string (32 bytes, null-padded ASCII).
    pub generating_software: [u8; 32],
    /// File creation day-of-year.
    pub file_creation_day: u16,
    /// File creation year.
    pub file_creation_year: u16,
    /// Size of the public header block in bytes.
    pub header_size: u16,
    /// Byte offset from the start of the file to point data.
    pub offset_to_point_data: u32,
    /// Number of variable length records.
    pub number_of_vlrs: u32,
    /// Point data format ID (0–10).
    pub point_data_format_id: u8,
    /// Size of a single point record in bytes.
    pub point_data_record_length: u16,
    /// Total number of point records in the file.
    pub number_of_point_records: u64,
    /// Scale factor applied to raw X integer coordinates.
    pub scale_x: f64,
    /// Scale factor applied to raw Y integer coordinates.
    pub scale_y: f64,
    /// Scale factor applied to raw Z integer coordinates.
    pub scale_z: f64,
    /// X coordinate offset.
    pub offset_x: f64,
    /// Y coordinate offset.
    pub offset_y: f64,
    /// Z coordinate offset.
    pub offset_z: f64,
    /// Maximum X value.
    pub max_x: f64,
    /// Minimum X value.
    pub min_x: f64,
    /// Maximum Y value.
    pub max_y: f64,
    /// Minimum Y value.
    pub min_y: f64,
    /// Maximum Z value.
    pub max_z: f64,
    /// Minimum Z value.
    pub min_z: f64,
}

impl LasHeader {
    /// Parse a LAS public header from a byte slice.
    ///
    /// The slice must contain at least 227 bytes (the LAS 1.0–1.3 header size).
    ///
    /// # Errors
    /// Returns [`CopcError::InvalidFormat`] when the data is too short or the
    /// magic is wrong, and [`CopcError::UnsupportedVersion`] for unknown version
    /// bytes.
    pub fn parse(data: &[u8]) -> Result<Self, CopcError> {
        if data.len() < 227 {
            return Err(CopcError::InvalidFormat(format!(
                "LAS data too short: {} bytes (need ≥ 227)",
                data.len()
            )));
        }
        if !data.starts_with(LAS_MAGIC) {
            return Err(CopcError::InvalidFormat(
                "Not a LAS file (bad magic)".into(),
            ));
        }

        let major = data[24];
        let minor = data[25];
        let version = LasVersion::from_bytes(major, minor)
            .ok_or(CopcError::UnsupportedVersion(major, minor))?;

        let mut system_id = [0u8; 32];
        system_id.copy_from_slice(&data[26..58]);
        let mut generating_software = [0u8; 32];
        generating_software.copy_from_slice(&data[58..90]);

        // Helper: read a little-endian f64 at byte offset `o`.
        let f64_le = |o: usize| -> f64 {
            f64::from_le_bytes([
                data[o],
                data[o + 1],
                data[o + 2],
                data[o + 3],
                data[o + 4],
                data[o + 5],
                data[o + 6],
                data[o + 7],
            ])
        };

        let header_size = u16::from_le_bytes([data[94], data[95]]);
        let offset_to_point_data = u32::from_le_bytes([data[96], data[97], data[98], data[99]]);
        let number_of_vlrs = u32::from_le_bytes([data[100], data[101], data[102], data[103]]);
        let point_data_format_id = data[104];
        let point_data_record_length = u16::from_le_bytes([data[105], data[106]]);

        // LAS 1.4 stores the 64-bit point count at offset 247 (header ≥ 375 bytes).
        // For earlier versions we read the 32-bit legacy field at offset 107.
        let number_of_point_records = if matches!(version, LasVersion::V14) && data.len() >= 255 {
            u64::from_le_bytes([
                data[247], data[248], data[249], data[250], data[251], data[252], data[253],
                data[254],
            ])
        } else {
            u32::from_le_bytes([data[107], data[108], data[109], data[110]]) as u64
        };

        Ok(Self {
            version,
            system_id,
            generating_software,
            file_creation_day: u16::from_le_bytes([data[90], data[91]]),
            file_creation_year: u16::from_le_bytes([data[92], data[93]]),
            header_size,
            offset_to_point_data,
            number_of_vlrs,
            point_data_format_id,
            point_data_record_length,
            number_of_point_records,
            scale_x: f64_le(131),
            scale_y: f64_le(139),
            scale_z: f64_le(147),
            offset_x: f64_le(155),
            offset_y: f64_le(163),
            offset_z: f64_le(171),
            max_x: f64_le(179),
            min_x: f64_le(187),
            max_y: f64_le(195),
            min_y: f64_le(203),
            max_z: f64_le(211),
            min_z: f64_le(219),
        })
    }

    /// Return the bounding box as `(min, max)` in (X, Y, Z) order.
    pub fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        (
            [self.min_x, self.min_y, self.min_z],
            [self.max_x, self.max_y, self.max_z],
        )
    }
}
