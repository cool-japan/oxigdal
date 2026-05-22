//! Pure-Rust LASzip chunk decompression for LAS Point Formats 0 and 1.
//!
//! This module implements the *minimum* required to read a `.copc.laz` file
//! whose point data uses LAS PF0 (20-byte legacy) or PF1 (PF0 + GPS time).
//! Formats 6, 7, 8 (the COPC mandate) are explicitly **deferred** to a
//! follow-up slice and return [`crate::error::CopcError::UnsupportedLazFormat`].
//!
//! Wire-format references:
//! - LASzip specification 3.4 (LASzip authors, isenburg/laszip).
//! - PDAL LAS reader source, `pdal/io/LasReader.cpp`.
//! - The reference C++ implementation in `LASzip/src/`.

pub mod arithmetic;
pub mod chunk_table;
pub mod format_v1;
pub mod predictors;

use crate::copc_vlr::Vlr;
use crate::error::CopcError;

/// Canonical user_id string of the LASzip VLR (16 bytes, null-padded).
pub const LASZIP_VLR_USER_ID: &str = "laszip encoded";
/// LASzip VLR record_id (22204 = 0x56BC).
pub const LASZIP_VLR_RECORD_ID: u16 = 22204;

/// Parsed body of the LASzip VLR.
///
/// The wire-level structure is documented in LASzip Spec 3.4 §4.
/// We surface the same field names the C++ reader uses so the mapping is
/// obvious to anyone porting code.
#[derive(Debug, Clone)]
pub struct LazVlrInfo {
    /// Compressor type: 1 = pointwise, 2 = pointwise-chunked, 3-4 = layered.
    pub compressor: u16,
    /// Coder type: 0 = arithmetic (only supported value today).
    pub coder: u16,
    /// LASzip writer major version.
    pub version_major: u8,
    /// LASzip writer minor version.
    pub version_minor: u8,
    /// Reserved option flags (currently unused but parsed).
    pub options: u32,
    /// Maximum number of points per chunk.  `u32::MAX` means "variable size".
    pub chunk_size: u32,
    /// Total number of points in the file (signed for legacy reasons).
    pub num_points: i64,
    /// Total number of compressed bytes (informational, signed).
    pub num_bytes: i64,
    /// Per-item codec descriptors.
    pub items: Vec<LazItem>,
}

/// One LASzip "item" descriptor.
///
/// Each LAS point record is decomposed into a sequence of items (e.g. PF0 has
/// a single `POINT10` item, PF1 has `POINT10` + `GPSTIME11`, …).  The VLR
/// lists these in order.
#[derive(Debug, Clone, Copy)]
pub struct LazItem {
    /// Item type tag — see the LASzip spec table.
    pub item_type: u16,
    /// Item byte size on the wire (uncompressed).
    pub size: u16,
    /// Item codec version (1, 2, 3, …).
    pub version: u16,
}

/// Default chunk size used by laszip when the writer does not specify one.
pub const DEFAULT_LAZ_CHUNK_SIZE: u32 = 50_000;

/// Find the LASzip VLR among `vlrs`.
///
/// Returns `None` when no VLR with `user_id = "laszip encoded"` and
/// `record_id = 22204` is present, which is the unambiguous signal that the
/// point data is uncompressed.
pub fn detect_laszip_vlr(vlrs: &[Vlr]) -> Option<&Vlr> {
    vlrs.iter().find(|v| {
        v.key.user_id.trim_end_matches('\0').trim_end() == LASZIP_VLR_USER_ID
            && v.key.record_id == LASZIP_VLR_RECORD_ID
    })
}

/// Parse a LASzip VLR data block.
///
/// The wire layout is fixed-size up to the items array:
///
/// | Bytes | Field |
/// |-------|-------|
/// | 2 | compressor (u16 LE) |
/// | 2 | coder (u16 LE) |
/// | 1 | version_major |
/// | 1 | version_minor |
/// | 2 | version_revision |
/// | 4 | options (u32 LE) |
/// | 4 | chunk_size (u32 LE) |
/// | 8 | num_points (i64 LE) |
/// | 8 | num_bytes (i64 LE) |
/// | 2 | num_items (u16 LE) |
/// | N × 6 | per-item: item_type (u16), size (u16), version (u16) |
///
/// # Errors
/// Returns [`CopcError::InvalidFormat`] when `data` is shorter than the
/// fixed header (34 bytes) or the items array is truncated.
pub fn parse_laszip_vlr_data(data: &[u8]) -> Result<LazVlrInfo, CopcError> {
    if data.len() < 34 {
        return Err(CopcError::InvalidFormat(format!(
            "LASzip VLR too short: {} bytes (need >= 34)",
            data.len()
        )));
    }

    let compressor = u16::from_le_bytes([data[0], data[1]]);
    let coder = u16::from_le_bytes([data[2], data[3]]);
    let version_major = data[4];
    let version_minor = data[5];
    // bytes 6..8 = version_revision (ignored)
    let options = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let chunk_size = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let num_points = i64::from_le_bytes([
        data[16], data[17], data[18], data[19], data[20], data[21], data[22], data[23],
    ]);
    let num_bytes = i64::from_le_bytes([
        data[24], data[25], data[26], data[27], data[28], data[29], data[30], data[31],
    ]);
    let num_items = u16::from_le_bytes([data[32], data[33]]) as usize;

    let items_start = 34;
    let items_byte_len = num_items.checked_mul(6).ok_or_else(|| {
        CopcError::InvalidFormat(format!("LASzip num_items {num_items} overflows"))
    })?;
    if items_start + items_byte_len > data.len() {
        return Err(CopcError::InvalidFormat(format!(
            "LASzip VLR items truncated: need {} bytes from offset {items_start}, have {}",
            items_byte_len,
            data.len() - items_start
        )));
    }

    let mut items = Vec::with_capacity(num_items);
    for i in 0..num_items {
        let off = items_start + i * 6;
        let item_type = u16::from_le_bytes([data[off], data[off + 1]]);
        let size = u16::from_le_bytes([data[off + 2], data[off + 3]]);
        let version = u16::from_le_bytes([data[off + 4], data[off + 5]]);
        items.push(LazItem {
            item_type,
            size,
            version,
        });
    }

    Ok(LazVlrInfo {
        compressor,
        coder,
        version_major,
        version_minor,
        options,
        chunk_size,
        num_points,
        num_bytes,
        items,
    })
}

/// Encode a [`LazVlrInfo`] back into bytes (test-only helper).
///
/// Used to build synthetic VLRs in tests.
#[cfg(any(test, feature = "laz-encoder"))]
pub fn serialize_laszip_vlr_data(info: &LazVlrInfo) -> Vec<u8> {
    let mut out = Vec::with_capacity(34 + info.items.len() * 6);
    out.extend_from_slice(&info.compressor.to_le_bytes());
    out.extend_from_slice(&info.coder.to_le_bytes());
    out.push(info.version_major);
    out.push(info.version_minor);
    out.extend_from_slice(&0u16.to_le_bytes()); // version_revision
    out.extend_from_slice(&info.options.to_le_bytes());
    out.extend_from_slice(&info.chunk_size.to_le_bytes());
    out.extend_from_slice(&info.num_points.to_le_bytes());
    out.extend_from_slice(&info.num_bytes.to_le_bytes());
    out.extend_from_slice(&(info.items.len() as u16).to_le_bytes());
    for item in &info.items {
        out.extend_from_slice(&item.item_type.to_le_bytes());
        out.extend_from_slice(&item.size.to_le_bytes());
        out.extend_from_slice(&item.version.to_le_bytes());
    }
    out
}

/// Decompress a single LAZ chunk into raw LAS bytes.
///
/// # Parameters
/// - `compressed` — bytes of the chunk on the wire.
/// - `point_count` — number of records encoded in this chunk.
/// - `record_length` — declared LAS record length (used to validate the
///   seed-record layout).
/// - `format_id` — LAS point data format ID.
///
/// # Errors
/// - [`CopcError::UnsupportedLazFormat`] for `format_id >= 2`.
/// - [`CopcError::LazDecoderError`] on malformed compressed data.
pub fn decompress_chunk(
    compressed: &[u8],
    point_count: usize,
    record_length: usize,
    format_id: u8,
) -> Result<Vec<u8>, CopcError> {
    match format_id {
        0 => {
            if record_length != format_v1::PF0_RECORD_SIZE {
                return Err(CopcError::LazDecoderError(format!(
                    "PF0 record_length {record_length} does not match expected {} bytes",
                    format_v1::PF0_RECORD_SIZE
                )));
            }
            format_v1::decompress_format_0(compressed, point_count)
        }
        1 => {
            if record_length != format_v1::PF1_RECORD_SIZE {
                return Err(CopcError::LazDecoderError(format!(
                    "PF1 record_length {record_length} does not match expected {} bytes",
                    format_v1::PF1_RECORD_SIZE
                )));
            }
            format_v1::decompress_format_1(compressed, point_count)
        }
        other => Err(CopcError::UnsupportedLazFormat { format_id: other }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::copc_vlr::{Vlr, VlrKey};

    fn make_vlr(user_id: &str, record_id: u16, data: Vec<u8>) -> Vlr {
        Vlr {
            key: VlrKey {
                user_id: user_id.to_string(),
                record_id,
            },
            description: String::new(),
            data,
        }
    }

    #[test]
    fn test_laz_vlr_detection_returns_expected_record_id() {
        let vlrs = vec![
            make_vlr("copc", 1, vec![0u8; 160]),
            make_vlr(LASZIP_VLR_USER_ID, LASZIP_VLR_RECORD_ID, vec![0u8; 34]),
            make_vlr("other", 99, vec![0u8; 4]),
        ];
        let found = detect_laszip_vlr(&vlrs).expect("must find laszip VLR");
        assert_eq!(found.key.record_id, LASZIP_VLR_RECORD_ID);
        assert_eq!(found.key.user_id, LASZIP_VLR_USER_ID);

        let no_laz = vec![make_vlr("copc", 1, vec![0u8; 160])];
        assert!(detect_laszip_vlr(&no_laz).is_none());
    }

    #[test]
    fn test_laz_vlr_parse_canonical_items_field() {
        let info = LazVlrInfo {
            compressor: 2,
            coder: 0,
            version_major: 3,
            version_minor: 4,
            options: 0,
            chunk_size: 50_000,
            num_points: 1000,
            num_bytes: 2048,
            items: vec![
                LazItem {
                    item_type: 6, // POINT10
                    size: 20,
                    version: 1,
                },
                LazItem {
                    item_type: 7, // GPSTIME11
                    size: 8,
                    version: 1,
                },
            ],
        };
        let bytes = serialize_laszip_vlr_data(&info);
        let parsed = parse_laszip_vlr_data(&bytes).expect("parse");
        assert_eq!(parsed.compressor, 2);
        assert_eq!(parsed.chunk_size, 50_000);
        assert_eq!(parsed.num_points, 1000);
        assert_eq!(parsed.items.len(), 2);
        assert_eq!(parsed.items[0].item_type, 6);
        assert_eq!(parsed.items[0].size, 20);
        assert_eq!(parsed.items[1].item_type, 7);
        assert_eq!(parsed.items[1].size, 8);
    }

    #[test]
    fn test_decompress_unsupported_format_6_returns_typed_error() {
        let result = decompress_chunk(&[0u8; 64], 4, 30, 6);
        assert!(matches!(
            result,
            Err(CopcError::UnsupportedLazFormat { format_id: 6 })
        ));
        // PF7 also rejected.
        let result7 = decompress_chunk(&[0u8; 64], 4, 36, 7);
        assert!(matches!(
            result7,
            Err(CopcError::UnsupportedLazFormat { format_id: 7 })
        ));
        // PF8 also rejected.
        let result8 = decompress_chunk(&[0u8; 64], 4, 38, 8);
        assert!(matches!(
            result8,
            Err(CopcError::UnsupportedLazFormat { format_id: 8 })
        ));
    }
}
