//! COPC Variable Length Record (VLR) types.
//!
//! Reference: <https://copc.io/copc-specification-1.0.pdf>

use crate::error::CopcError;

/// The body of the COPC info VLR (`user_id = "copc"`, `record_id = 1`).
///
/// This 160-byte record describes the octree root extent and where to find
/// the hierarchy page for the root node.
#[derive(Debug, Clone)]
pub struct CopcInfo {
    /// X coordinate of the octree root centre.
    pub center_x: f64,
    /// Y coordinate of the octree root centre.
    pub center_y: f64,
    /// Z coordinate of the octree root centre.
    pub center_z: f64,
    /// Half-size (radius) of the root octree cube.
    pub halfsize: f64,
    /// Spacing between points at the root level.
    pub spacing: f64,
    /// Byte offset (from file start) of the root hierarchy page.
    pub root_hier_offset: u64,
    /// Byte length of the root hierarchy page.
    pub root_hier_size: u64,
    /// Minimum GPS time value in the file.
    pub gpstime_minimum: f64,
    /// Maximum GPS time value in the file.
    pub gpstime_maximum: f64,
}

impl CopcInfo {
    /// Parse the 160-byte COPC info VLR body.
    ///
    /// # Errors
    /// Returns [`CopcError::InvalidFormat`] when `data` is shorter than 160 bytes.
    pub fn parse(data: &[u8]) -> Result<Self, CopcError> {
        if data.len() < 160 {
            return Err(CopcError::InvalidFormat(format!(
                "COPC info VLR too short: {} bytes (need 160)",
                data.len()
            )));
        }

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
        let u64_le = |o: usize| -> u64 {
            u64::from_le_bytes([
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

        Ok(Self {
            center_x: f64_le(0),
            center_y: f64_le(8),
            center_z: f64_le(16),
            halfsize: f64_le(24),
            spacing: f64_le(32),
            root_hier_offset: u64_le(40),
            root_hier_size: u64_le(48),
            gpstime_minimum: f64_le(56),
            gpstime_maximum: f64_le(64),
        })
    }

    /// Return the bounding box of the root octree node as `(min, max)`.
    pub fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        let c = [self.center_x, self.center_y, self.center_z];
        let h = self.halfsize;
        (
            [c[0] - h, c[1] - h, c[2] - h],
            [c[0] + h, c[1] + h, c[2] + h],
        )
    }
}

/// Composite key that uniquely identifies a VLR.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VlrKey {
    /// User identifier string (up to 16 bytes, null-padded).
    pub user_id: String,
    /// Record ID.
    pub record_id: u16,
}

/// A parsed Variable Length Record (VLR).
#[derive(Debug, Clone)]
pub struct Vlr {
    /// Composite key for this VLR.
    pub key: VlrKey,
    /// Human-readable description (up to 32 bytes).
    pub description: String,
    /// Raw VLR payload bytes.
    pub data: Vec<u8>,
}

impl Vlr {
    /// Parse a single VLR starting at `offset` within `data`.
    ///
    /// Returns the parsed VLR and the byte offset immediately after it.
    ///
    /// # VLR wire layout (LAS spec)
    /// | Bytes | Field |
    /// |-------|-------|
    /// | 2 | Reserved (ignored) |
    /// | 16 | User ID (ASCII, null-padded) |
    /// | 2 | Record ID (little-endian) |
    /// | 2 | Record length after header (little-endian) |
    /// | 32 | Description (ASCII, null-padded) |
    /// | N | Data |
    ///
    /// # Errors
    /// Returns [`CopcError::InvalidFormat`] when the slice is too short.
    pub fn parse(data: &[u8], offset: usize) -> Result<(Self, usize), CopcError> {
        // Minimum VLR header is 54 bytes (2+16+2+2+32).
        if offset + 54 > data.len() {
            return Err(CopcError::InvalidFormat(format!(
                "VLR header truncated at offset {offset} (need ≥ {} bytes)",
                offset + 54
            )));
        }

        // Bytes 0-1: reserved, skip.
        let user_id_bytes = &data[offset + 2..offset + 18];
        let user_id = String::from_utf8_lossy(user_id_bytes)
            .trim_end_matches('\0')
            .to_string();

        let record_id = u16::from_le_bytes([data[offset + 18], data[offset + 19]]);
        let record_len = u16::from_le_bytes([data[offset + 20], data[offset + 21]]) as usize;

        let desc_bytes = &data[offset + 22..offset + 54];
        let description = String::from_utf8_lossy(desc_bytes)
            .trim_end_matches('\0')
            .to_string();

        let data_start = offset + 54;
        let data_end = data_start + record_len;
        if data_end > data.len() {
            return Err(CopcError::InvalidFormat(format!(
                "VLR data truncated: need {data_end} bytes but only {} available",
                data.len()
            )));
        }

        Ok((
            Vlr {
                key: VlrKey { user_id, record_id },
                description,
                data: data[data_start..data_end].to_vec(),
            },
            data_end,
        ))
    }
}
