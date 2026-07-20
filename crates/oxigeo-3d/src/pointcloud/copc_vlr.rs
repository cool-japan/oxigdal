//! COPC VLR + Hierarchy page parsing.
//!
//! Implements the parsing primitives defined by the COPC 1.0 specification
//! (<https://copc.io/>, 2022-02). This module is intentionally kept
//! self-contained so that the reader in [`crate::pointcloud::copc`] can call
//! into it without pulling in the `las` crate's higher-level COPC types
//! (which are partially `pub(crate)` and not directly usable here).
//!
//! Three primitives are exposed:
//!
//! * [`parse_copc_info`] — decodes the 160-byte LE-encoded COPC info VLR
//!   payload.
//! * [`parse_hierarchy_page`] — decodes a hierarchy page consisting of one or
//!   more 32-byte entries.
//! * [`find_copc_info_vlr`] — locates the COPC info VLR
//!   (`user_id == "copc"`, `record_id == 1`) inside a `las::Header`.
//!
//! See `src/pointcloud/copc.rs` for the `CopcReader` plumbing that consumes
//! these primitives.
//!
//! # Wire format
//!
//! COPC info VLR payload (offset / size / field):
//!
//! ```text
//! 0   f64 center_x
//! 8   f64 center_y
//! 16  f64 center_z
//! 24  f64 halfsize
//! 32  f64 spacing
//! 40  u64 root_hier_offset
//! 48  u64 root_hier_size
//! 56  f64 gps_time_min
//! 64  f64 gps_time_max
//! 72..160  reserved (zero)
//! ```
//!
//! Hierarchy page entry (32 bytes, repeated):
//!
//! ```text
//! 0   i32 level
//! 4   i32 x
//! 8   i32 y
//! 12  i32 z
//! 16  u64 offset
//! 24  i32 byte_size  (negative ⇒ this entry points at a child hierarchy page)
//! 28  i32 point_count
//! ```

use crate::error::Error;

/// User ID assigned to all COPC-defined VLRs.
pub const COPC_USER_ID: &str = "copc";

/// Record ID of the COPC info VLR.
pub const COPC_INFO_RECORD_ID: u16 = 1;

/// Record ID of COPC hierarchy EVLRs (informational; pages are referenced by
/// offset via the info VLR rather than looked up by record id).
pub const COPC_HIERARCHY_RECORD_ID: u16 = 1000;

/// Required length, in bytes, of the COPC info VLR payload.
pub const COPC_INFO_PAYLOAD_LEN: usize = 160;

/// Required length, in bytes, of a single COPC hierarchy entry.
pub const COPC_HIERARCHY_ENTRY_LEN: usize = 32;

/// Maximum hierarchy traversal depth, used by the reader to abort runaway
/// recursion in (potentially malformed) files.
pub const COPC_MAX_HIERARCHY_DEPTH: usize = 32;

/// Decoded 160-byte COPC info VLR payload (LE-encoded on disk).
#[derive(Debug, Clone, PartialEq)]
pub struct CopcInfoVlrPayload {
    /// Unscaled X coordinate of the octree center.
    pub center_x: f64,
    /// Unscaled Y coordinate of the octree center.
    pub center_y: f64,
    /// Unscaled Z coordinate of the octree center.
    pub center_z: f64,
    /// Perpendicular distance from the center to any side of the root node.
    pub halfsize: f64,
    /// Spacing between points at the root node (halved at each octree level).
    pub spacing: f64,
    /// File offset of the root hierarchy page.
    pub root_hier_offset: u64,
    /// Size, in bytes, of the root hierarchy page.
    pub root_hier_size: u64,
    /// Minimum GPS time observed across the file (0.0 if absent).
    pub gps_time_min: f64,
    /// Maximum GPS time observed across the file (0.0 if absent).
    pub gps_time_max: f64,
}

/// Voxel key in the COPC octree (level 0 = root).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VoxelKey {
    /// Octree depth (0 = root, increasing toward leaves).
    pub level: i32,
    /// Voxel X index at `level`.
    pub x: i32,
    /// Voxel Y index at `level`.
    pub y: i32,
    /// Voxel Z index at `level`.
    pub z: i32,
}

/// One 32-byte entry in a COPC hierarchy page.
#[derive(Debug, Clone, PartialEq)]
pub struct HierarchyEntry {
    /// Voxel addressed by this entry.
    pub key: VoxelKey,
    /// File offset of the LAZ chunk (or child hierarchy page when
    /// `byte_size < 0`).
    pub offset: u64,
    /// Byte size: positive ⇒ LAZ chunk size; negative ⇒ child page size
    /// (negated); zero ⇒ leaf with no point data.
    pub byte_size: i32,
    /// Number of points stored in the addressed chunk.
    pub point_count: i32,
}

/// Read a `f64` from `payload` at `offset`, little-endian.
///
/// This helper avoids `.unwrap()` on the slice conversion by manually copying
/// into a fixed-size buffer. Callers must have already bounds-checked.
#[inline]
fn read_f64_le(payload: &[u8], offset: usize) -> f64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&payload[offset..offset + 8]);
    f64::from_le_bytes(bytes)
}

/// Read a `u64` from `payload` at `offset`, little-endian.
#[inline]
fn read_u64_le(payload: &[u8], offset: usize) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&payload[offset..offset + 8]);
    u64::from_le_bytes(bytes)
}

/// Read an `i32` from `payload` at `offset`, little-endian.
#[inline]
fn read_i32_le(payload: &[u8], offset: usize) -> i32 {
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&payload[offset..offset + 4]);
    i32::from_le_bytes(bytes)
}

/// Parse the 160-byte COPC info VLR payload.
///
/// Accepts payloads with trailing padding; only the first 160 bytes are
/// inspected. Bytes `72..160` are reserved per the spec and ignored.
///
/// # Errors
///
/// Returns [`Error::Copc`] if `payload.len() < 160`.
pub fn parse_copc_info(payload: &[u8]) -> Result<CopcInfoVlrPayload, Error> {
    if payload.len() < COPC_INFO_PAYLOAD_LEN {
        return Err(Error::malformed_copc_info(format!(
            "payload too short: {} < {}",
            payload.len(),
            COPC_INFO_PAYLOAD_LEN
        )));
    }
    Ok(CopcInfoVlrPayload {
        center_x: read_f64_le(payload, 0),
        center_y: read_f64_le(payload, 8),
        center_z: read_f64_le(payload, 16),
        halfsize: read_f64_le(payload, 24),
        spacing: read_f64_le(payload, 32),
        root_hier_offset: read_u64_le(payload, 40),
        root_hier_size: read_u64_le(payload, 48),
        gps_time_min: read_f64_le(payload, 56),
        gps_time_max: read_f64_le(payload, 64),
        // bytes 72..160 are reserved per the spec.
    })
}

/// Parse a hierarchy page into a vector of [`HierarchyEntry`] (32 bytes each).
///
/// # Errors
///
/// Returns [`Error::Copc`] if `bytes.len()` is not a multiple of 32.
pub fn parse_hierarchy_page(bytes: &[u8]) -> Result<Vec<HierarchyEntry>, Error> {
    if !bytes.len().is_multiple_of(COPC_HIERARCHY_ENTRY_LEN) {
        return Err(Error::malformed_copc_info(format!(
            "hierarchy page bytes {} not a multiple of {}",
            bytes.len(),
            COPC_HIERARCHY_ENTRY_LEN
        )));
    }
    let n = bytes.len() / COPC_HIERARCHY_ENTRY_LEN;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let base = i * COPC_HIERARCHY_ENTRY_LEN;
        let level = read_i32_le(bytes, base);
        let kx = read_i32_le(bytes, base + 4);
        let ky = read_i32_le(bytes, base + 8);
        let kz = read_i32_le(bytes, base + 12);
        let offset = read_u64_le(bytes, base + 16);
        let byte_size = read_i32_le(bytes, base + 24);
        let point_count = read_i32_le(bytes, base + 28);
        out.push(HierarchyEntry {
            key: VoxelKey {
                level,
                x: kx,
                y: ky,
                z: kz,
            },
            offset,
            byte_size,
            point_count,
        });
    }
    Ok(out)
}

/// Locate the COPC info VLR (`user_id == "copc"`, `record_id == 1`) in a LAS
/// header.
///
/// Matches against the `user_id` after trimming trailing NUL/whitespace,
/// because the `las` crate retains the raw 16-byte field as a `String` and
/// some encoders leave padding behind.
pub fn find_copc_info_vlr(header: &las::Header) -> Option<&las::Vlr> {
    header
        .vlrs()
        .iter()
        .find(|v| is_copc_info_vlr(v))
        .or_else(|| header.evlrs().iter().find(|v| is_copc_info_vlr(v)))
}

#[inline]
fn is_copc_info_vlr(vlr: &las::Vlr) -> bool {
    let trimmed = vlr.user_id.trim_end_matches('\0').trim();
    trimmed == COPC_USER_ID && vlr.record_id == COPC_INFO_RECORD_ID
}

// ---------------------------------------------------------------------------
// Test fixtures
// ---------------------------------------------------------------------------

/// Test-only encoder for [`CopcInfoVlrPayload`]. Produces an exactly
/// 160-byte LE-encoded buffer suitable for round-trip tests and as VLR payload
/// inside synthetic LAS headers.
#[cfg(any(test, feature = "copc"))]
#[doc(hidden)]
pub fn encode_copc_info_for_test(payload: &CopcInfoVlrPayload) -> Vec<u8> {
    let mut out = Vec::with_capacity(COPC_INFO_PAYLOAD_LEN);
    out.extend_from_slice(&payload.center_x.to_le_bytes());
    out.extend_from_slice(&payload.center_y.to_le_bytes());
    out.extend_from_slice(&payload.center_z.to_le_bytes());
    out.extend_from_slice(&payload.halfsize.to_le_bytes());
    out.extend_from_slice(&payload.spacing.to_le_bytes());
    out.extend_from_slice(&payload.root_hier_offset.to_le_bytes());
    out.extend_from_slice(&payload.root_hier_size.to_le_bytes());
    out.extend_from_slice(&payload.gps_time_min.to_le_bytes());
    out.extend_from_slice(&payload.gps_time_max.to_le_bytes());
    // 88 reserved bytes (11 * u64 zero per the spec).
    out.resize(COPC_INFO_PAYLOAD_LEN, 0);
    out
}

/// Test-only encoder for a single [`HierarchyEntry`] (32 bytes).
#[cfg(any(test, feature = "copc"))]
#[doc(hidden)]
pub fn encode_hierarchy_entry_for_test(entry: &HierarchyEntry) -> [u8; COPC_HIERARCHY_ENTRY_LEN] {
    let mut buf = [0u8; COPC_HIERARCHY_ENTRY_LEN];
    buf[0..4].copy_from_slice(&entry.key.level.to_le_bytes());
    buf[4..8].copy_from_slice(&entry.key.x.to_le_bytes());
    buf[8..12].copy_from_slice(&entry.key.y.to_le_bytes());
    buf[12..16].copy_from_slice(&entry.key.z.to_le_bytes());
    buf[16..24].copy_from_slice(&entry.offset.to_le_bytes());
    buf[24..28].copy_from_slice(&entry.byte_size.to_le_bytes());
    buf[28..32].copy_from_slice(&entry.point_count.to_le_bytes());
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_payload() -> CopcInfoVlrPayload {
        CopcInfoVlrPayload {
            center_x: 1.5,
            center_y: -2.25,
            center_z: 3.125,
            halfsize: 1024.0,
            spacing: 0.5,
            root_hier_offset: 4096,
            root_hier_size: 96,
            gps_time_min: 100.0,
            gps_time_max: 200.0,
        }
    }

    #[test]
    fn unit_round_trip() {
        let original = sample_payload();
        let bytes = encode_copc_info_for_test(&original);
        assert_eq!(bytes.len(), COPC_INFO_PAYLOAD_LEN);
        let parsed = parse_copc_info(&bytes)
            .expect("encode/decode of a canonical payload should never fail in unit tests");
        assert_eq!(parsed, original);
    }

    #[test]
    fn unit_hierarchy_entry_round_trip() {
        let e = HierarchyEntry {
            key: VoxelKey {
                level: 2,
                x: 5,
                y: -1,
                z: 0,
            },
            offset: 1 << 20,
            byte_size: 256,
            point_count: 42,
        };
        let bytes = encode_hierarchy_entry_for_test(&e);
        let decoded = parse_hierarchy_page(&bytes)
            .expect("a 32-byte buffer must decode to exactly one entry");
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0], e);
    }
}
