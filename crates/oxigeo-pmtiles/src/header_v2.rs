//! PMTiles v2 file header and directory entry parsing.
//!
//! Reference: <https://github.com/protomaps/PMTiles/blob/master/spec/v2/v2.md>
//!
//! Binary layout:
//! - bytes 0-1: magic `b"PM"` (2 bytes)
//! - byte 2: version = 2
//! - bytes 3-4: metadata_length as u16 LE
//! - bytes 5 ... 5+metadata_length-1: JSON metadata (UTF-8)
//! - bytes 5+metadata_length onwards: root directory entries, each 17 bytes
//!
//! Per-entry layout (17 bytes):
//! - byte 0: z (zoom, u8)
//! - bytes 1-3: x as u24 LE (3 bytes)
//! - bytes 4-6: y as u24 LE (3 bytes)
//! - bytes 7-12: tile data offset as u48 LE (6 bytes)
//! - bytes 13-16: tile data length as u32 LE (4 bytes)
//! - if length == 0 → leaf directory pointer (offset points to more entries)

use crate::error::PmTilesError;

/// Three-byte magic sequence that identifies a PMTiles v2 file.
pub const PMTILES_V2_MAGIC: &[u8] = b"PM\x02";

/// Size of a single v2 directory entry in bytes.
pub const V2_ENTRY_SIZE: usize = 17;

/// Minimum number of bytes required to begin parsing a v2 header.
pub const V2_HEADER_MIN_SIZE: usize = 5;

/// Maximum size of the root region (header + directory) in a v2 archive.
///
/// PMTiles v2 packs the entire header and root directory into the first
/// 512 bytes of the file, so at most `(512 - 5) / 17 = 29` entries can
/// fit when there is zero metadata.  We cap entry parsing here to avoid
/// reading into the tile-data section.
pub const V2_ROOT_BLOCK_SIZE: usize = 512;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single PMTiles v2 directory entry.
///
/// Entries with `length == 0` (equivalently, `is_dir == true`) are leaf
/// directory pointers.  In that case `offset` points to a sub-sequence of
/// 17-byte entries stored elsewhere in the archive.
#[derive(Debug, Clone, PartialEq)]
pub struct PmTilesV2Entry {
    /// Zoom level.
    pub z: u8,
    /// Tile column.
    pub x: u32,
    /// Tile row.
    pub y: u32,
    /// Byte offset of tile data (or sub-directory entries) within the archive.
    pub offset: u64,
    /// Byte length of tile data; 0 means this entry is a leaf-directory pointer.
    pub length: u32,
    /// `true` when `length == 0`, i.e., this entry is a leaf-directory pointer.
    pub is_dir: bool,
}

/// A parsed PMTiles v2 header together with its root directory entries.
#[derive(Debug)]
pub struct PmTilesV2Header {
    /// Spec version (always 2 for a valid v2 archive).
    pub version: u8,
    /// The raw JSON metadata string embedded in the archive.
    pub metadata: String,
    /// Root directory entries extracted from the header region.
    pub root_entries: Vec<PmTilesV2Entry>,
}

// ---------------------------------------------------------------------------
// Parsing functions
// ---------------------------------------------------------------------------

/// Parse the header + root directory entries of a PMTiles v2 archive.
///
/// # Errors
///
/// - [`PmTilesError::InvalidV2Header`] when `data` is too short, the magic
///   bytes do not match, the version byte is not 2, or the metadata region
///   overflows the buffer.
/// - [`PmTilesError::InvalidV2Header`] when the metadata UTF-8 decoding fails.
pub fn parse_v2_header(data: &[u8]) -> Result<PmTilesV2Header, PmTilesError> {
    // At minimum we need the 2-byte magic + 1-byte version + 2-byte length.
    if data.len() < V2_HEADER_MIN_SIZE {
        return Err(PmTilesError::InvalidV2Header(format!(
            "data too short: {} bytes (need at least {})",
            data.len(),
            V2_HEADER_MIN_SIZE
        )));
    }

    // Validate magic + version.
    if data[0] != b'P' || data[1] != b'M' {
        return Err(PmTilesError::InvalidV2Header(
            "invalid magic bytes: expected 'PM'".into(),
        ));
    }
    if data[2] != 2 {
        return Err(PmTilesError::InvalidV2Header(format!(
            "expected version 2, found version {}",
            data[2]
        )));
    }

    // metadata_length is a u16 LE at bytes [3..5].
    let metadata_length = u16::from_le_bytes([data[3], data[4]]) as usize;

    let metadata_start = 5_usize;
    let metadata_end = metadata_start + metadata_length;

    if metadata_end > data.len() {
        return Err(PmTilesError::InvalidV2Header(format!(
            "metadata region [{metadata_start}..{metadata_end}) overflows buffer (len={})",
            data.len()
        )));
    }

    let metadata = std::str::from_utf8(&data[metadata_start..metadata_end])
        .map_err(|e| PmTilesError::InvalidV2Header(format!("metadata is not valid UTF-8: {e}")))?
        .to_owned();

    // Root directory entries start immediately after the metadata.
    //
    // PMTiles v2 packs the header and root directory into the first 512 bytes
    // of the archive.  We cap parsing at that boundary.
    let entries_start = metadata_end;
    let entries_end = V2_ROOT_BLOCK_SIZE.min(data.len());
    let entries_data = if entries_start <= entries_end {
        &data[entries_start..entries_end]
    } else {
        &data[entries_start..]
    };

    let candidate_count = entries_data.len() / V2_ENTRY_SIZE;

    // Determine the true directory boundary using two passes:
    //
    // Pass 1: collect all candidate entries and find the minimum tile-data
    // offset among non-directory entries.  That offset marks where tile data
    // begins, which in turn tells us where the directory ends.
    let mut candidates: Vec<PmTilesV2Entry> = Vec::with_capacity(candidate_count);
    for i in 0..candidate_count {
        match read_v2_entry(entries_data, i * V2_ENTRY_SIZE) {
            Ok(e) => candidates.push(e),
            Err(_) => break,
        }
    }

    // Compute the minimum tile-data offset.  If no data entries exist (e.g.
    // all entries are directory pointers, or the archive is empty), fall back
    // to treating all candidates as valid.
    let min_data_offset: Option<u64> = candidates
        .iter()
        .filter(|e| !e.is_dir)
        .map(|e| e.offset)
        .min();

    // Pass 2: Keep only entries whose buffer position is entirely within the
    // directory region (i.e. before tile data starts).
    //
    // Entry i occupies bytes [entries_start + i*17 .. entries_start + (i+1)*17)
    // in the full archive buffer.  If tile data starts at `min_data_offset`,
    // the entry must end no later than that offset.
    let mut root_entries = Vec::with_capacity(candidates.len());
    for (i, entry) in candidates.into_iter().enumerate() {
        let entry_end_in_archive = (entries_start + (i + 1) * V2_ENTRY_SIZE) as u64;
        if let Some(data_start) = min_data_offset {
            // The entry's bytes in the archive must not overlap tile data.
            if entry_end_in_archive > data_start {
                // We've reached the tile data region — stop.
                break;
            }
        }
        root_entries.push(entry);
    }

    Ok(PmTilesV2Header {
        version: 2,
        metadata,
        root_entries,
    })
}

/// Decode a single 17-byte PMTiles v2 directory entry from `data` at `offset`.
///
/// Byte layout relative to `offset`:
///
/// | Bytes   | Field  | Encoding   |
/// |---------|--------|------------|
/// | 0       | z      | u8         |
/// | 1–3     | x      | u24 LE     |
/// | 4–6     | y      | u24 LE     |
/// | 7–12    | offset | u48 LE     |
/// | 13–16   | length | u32 LE     |
///
/// # Errors
///
/// Returns [`PmTilesError::InvalidV2Header`] when `data[offset..offset+17]`
/// is out of range.
pub fn read_v2_entry(data: &[u8], offset: usize) -> Result<PmTilesV2Entry, PmTilesError> {
    let end = offset + V2_ENTRY_SIZE;
    if end > data.len() {
        return Err(PmTilesError::InvalidV2Header(format!(
            "entry at offset {offset} requires bytes [{offset}..{end}) but buffer is {} bytes",
            data.len()
        )));
    }

    let z = data[offset];

    // x: 3 bytes, little-endian (u24)
    let x = u32::from(data[offset + 1])
        | (u32::from(data[offset + 2]) << 8)
        | (u32::from(data[offset + 3]) << 16);

    // y: 3 bytes, little-endian (u24)
    let y = u32::from(data[offset + 4])
        | (u32::from(data[offset + 5]) << 8)
        | (u32::from(data[offset + 6]) << 16);

    // tile data offset: 6 bytes, little-endian (u48)
    let tile_offset = u64::from(data[offset + 7])
        | (u64::from(data[offset + 8]) << 8)
        | (u64::from(data[offset + 9]) << 16)
        | (u64::from(data[offset + 10]) << 24)
        | (u64::from(data[offset + 11]) << 32)
        | (u64::from(data[offset + 12]) << 40);

    // tile data length: 4 bytes, little-endian (u32)
    let length = u32::from_le_bytes([
        data[offset + 13],
        data[offset + 14],
        data[offset + 15],
        data[offset + 16],
    ]);

    let is_dir = length == 0;

    Ok(PmTilesV2Entry {
        z,
        x,
        y,
        offset: tile_offset,
        length,
        is_dir,
    })
}

/// Compute the PMTiles v3-compatible Hilbert-curve tile ID for a v2 tile.
///
/// PMTiles v2 does not use Hilbert IDs internally, but this function is
/// provided so callers can look up v2 tiles using the same ID space as the
/// v3 reader (e.g. when upgrading an archive).
///
/// # Errors
///
/// Propagates [`PmTilesError::InvalidFormat`] from [`crate::hilbert::zxy_to_tile_id`]
/// when the coordinates are out of range.
pub fn zxy_to_v2_tile_id(z: u8, x: u32, y: u32) -> Result<u64, PmTilesError> {
    crate::hilbert::zxy_to_tile_id(z, x, y)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_v2_header_empty_entries() {
        let meta = b"{}";
        let mut data = Vec::new();
        data.extend_from_slice(b"PM\x02");
        data.extend_from_slice(&(meta.len() as u16).to_le_bytes());
        data.extend_from_slice(meta);
        let h = parse_v2_header(&data).expect("parse ok");
        assert_eq!(h.version, 2);
        assert_eq!(h.metadata, "{}");
        assert!(h.root_entries.is_empty());
    }

    #[test]
    fn test_parse_v2_header_rejects_too_short() {
        assert!(parse_v2_header(&[1, 2]).is_err());
    }

    #[test]
    fn test_parse_v2_header_rejects_wrong_magic() {
        // Wrong magic 'XX'
        let data = b"XX\x02\x00\x00";
        assert!(parse_v2_header(data).is_err());
    }

    #[test]
    fn test_parse_v2_header_rejects_wrong_version() {
        // Magic correct, version = 3
        let data = b"PM\x03\x00\x00";
        assert!(parse_v2_header(data).is_err());
    }

    #[test]
    fn test_read_v2_entry_known_values() {
        // Manually craft a 17-byte entry:
        // z=5, x=1, y=2, offset=0x102030, length=256
        let mut entry_bytes = [0u8; 17];
        entry_bytes[0] = 5; // z
        // x = 1 as u24 LE
        entry_bytes[1] = 1;
        entry_bytes[2] = 0;
        entry_bytes[3] = 0;
        // y = 2 as u24 LE
        entry_bytes[4] = 2;
        entry_bytes[5] = 0;
        entry_bytes[6] = 0;
        // offset = 0x102030 as u48 LE
        entry_bytes[7] = 0x30;
        entry_bytes[8] = 0x20;
        entry_bytes[9] = 0x10;
        entry_bytes[10] = 0;
        entry_bytes[11] = 0;
        entry_bytes[12] = 0;
        // length = 256 as u32 LE
        entry_bytes[13] = 0x00;
        entry_bytes[14] = 0x01;
        entry_bytes[15] = 0;
        entry_bytes[16] = 0;

        let entry = read_v2_entry(&entry_bytes, 0).expect("ok");
        assert_eq!(entry.z, 5);
        assert_eq!(entry.x, 1);
        assert_eq!(entry.y, 2);
        assert_eq!(entry.offset, 0x102030);
        assert_eq!(entry.length, 256);
        assert!(!entry.is_dir);
    }

    #[test]
    fn test_read_v2_entry_is_dir_when_length_zero() {
        let mut entry_bytes = [0u8; 17];
        // All fields zero → length=0 → is_dir=true
        let entry = read_v2_entry(&entry_bytes, 0).expect("ok");
        assert!(entry.is_dir);
        assert_eq!(entry.length, 0);
        // Zero out again properly
        entry_bytes[13] = 0;
        let entry2 = read_v2_entry(&entry_bytes, 0).expect("ok");
        assert!(entry2.is_dir);
    }
}
