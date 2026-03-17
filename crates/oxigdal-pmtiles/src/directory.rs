//! PMTiles v3 directory decoder.
//!
//! Directories map tile IDs to byte ranges within the tile-data section.
//! All integers are encoded as unsigned LEB-128 varints.

use crate::error::PmTilesError;

/// A single entry in a PMTiles directory.
#[derive(Debug, Clone)]
pub struct DirectoryEntry {
    /// Tile ID (Hilbert curve index).
    pub tile_id: u64,
    /// Byte offset of the tile data from the start of the tile-data section.
    pub offset: u64,
    /// Byte length of the tile data.
    pub length: u32,
    /// For run-length-encoded tiles: the number of consecutive tile IDs that
    /// share this entry.  A value of 0 means this entry points to a leaf
    /// directory rather than tile data.
    pub run_length: u32,
}

impl DirectoryEntry {
    /// Return `true` when this entry points to tile data.
    pub fn is_tile(&self) -> bool {
        self.run_length > 0
    }

    /// Return `true` when this entry points to a leaf directory page.
    pub fn is_leaf_directory(&self) -> bool {
        self.run_length == 0
    }
}

/// Decode a single unsigned LEB-128 varint from `data`.
///
/// Returns `(value, bytes_consumed)`.
///
/// # Errors
/// - [`PmTilesError::InvalidFormat`] when the varint exceeds 64 bits or the
///   slice is truncated.
pub fn decode_varint(data: &[u8]) -> Result<(u64, usize), PmTilesError> {
    let mut result = 0u64;
    let mut shift = 0u32;
    for (i, &byte) in data.iter().enumerate() {
        if shift >= 64 {
            return Err(PmTilesError::InvalidFormat(
                "Varint overflow (>64 bits)".into(),
            ));
        }
        result |= u64::from(byte & 0x7F) << shift;
        if byte & 0x80 == 0 {
            return Ok((result, i + 1));
        }
        shift += 7;
    }
    Err(PmTilesError::InvalidFormat("Truncated varint".into()))
}

/// Decode a PMTiles v3 directory from its (already-decompressed) byte slice.
///
/// The directory wire format is:
/// 1. `num_entries` (varint)
/// 2. `num_entries` delta-encoded tile IDs (varints, cumulative sum)
/// 3. `num_entries` run lengths (varints)
/// 4. `num_entries` tile-data lengths (varints)
/// 5. `num_entries` offsets (varints):
///    - index 0: absolute byte offset
///    - index i > 0: delta > 0 means `prev_offset + delta`;
///      delta == 0 means `prev_offset + prev_length` (clustered)
///
/// # Errors
/// Returns [`PmTilesError::InvalidFormat`] on malformed input.
pub fn decode_directory(data: &[u8]) -> Result<Vec<DirectoryEntry>, PmTilesError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }

    let mut pos = 0usize;

    let (n_entries_u64, consumed) = decode_varint(data)?;
    pos += consumed;
    let n = n_entries_u64 as usize;

    let mut tile_ids = Vec::with_capacity(n);
    let mut last_id: u64 = 0;
    for _ in 0..n {
        let (delta, c) = decode_varint(&data[pos..])?;
        pos += c;
        last_id = last_id.saturating_add(delta);
        tile_ids.push(last_id);
    }

    let mut run_lengths = Vec::with_capacity(n);
    for _ in 0..n {
        let (v, c) = decode_varint(&data[pos..])?;
        pos += c;
        run_lengths.push(v as u32);
    }

    let mut lengths = Vec::with_capacity(n);
    for _ in 0..n {
        let (v, c) = decode_varint(&data[pos..])?;
        pos += c;
        lengths.push(v as u32);
    }

    let mut offsets = Vec::with_capacity(n);
    let mut last_offset: u64 = 0;
    for i in 0..n {
        let (delta, c) = decode_varint(&data[pos..])?;
        pos += c;
        if i == 0 {
            // First offset is always absolute.
            last_offset = delta;
        } else if delta == 0 {
            // delta == 0 means "immediately follows the previous tile" (clustered).
            last_offset = last_offset.saturating_add(u64::from(lengths[i - 1]));
        } else {
            last_offset = last_offset.saturating_add(delta);
        }
        offsets.push(last_offset);
    }

    let entries = (0..n)
        .map(|i| DirectoryEntry {
            tile_id: tile_ids[i],
            offset: offsets[i],
            length: lengths[i],
            run_length: run_lengths[i],
        })
        .collect();

    Ok(entries)
}
