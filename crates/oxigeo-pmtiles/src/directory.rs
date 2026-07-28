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

    // Guard against a malicious/corrupt entry count causing an immediate OOM
    // abort before any of the `n` per-entry varints have actually been read
    // and validated. Every entry contributes a minimum of 4 bytes (one
    // 1-byte varint each for tile-id delta, run length, tile length and
    // offset), so `n` can never legitimately exceed the number of remaining
    // bytes divided by 4. We additionally cap at a fixed sane maximum so
    // that a directory claiming to (barely) fit a huge `n` in a huge buffer
    // still can't force a multi-gigabyte allocation up front.
    const MAX_DIRECTORY_ENTRIES: usize = 50_000_000;
    let remaining = data.len().saturating_sub(pos);
    let max_possible_entries = remaining / 4;
    if n_entries_u64 > max_possible_entries as u64 {
        return Err(PmTilesError::InvalidFormat(format!(
            "Directory claims {n_entries_u64} entries but only {remaining} bytes remain \
             (each entry requires at least 4 bytes)"
        )));
    }
    if n_entries_u64 > MAX_DIRECTORY_ENTRIES as u64 {
        return Err(PmTilesError::InvalidFormat(format!(
            "Directory entry count {n_entries_u64} exceeds maximum allowed {MAX_DIRECTORY_ENTRIES}"
        )));
    }
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
        let (raw_val, c) = decode_varint(&data[pos..])?;
        pos += c;
        if raw_val == 0 {
            // Clustered shorthand: this tile immediately follows the previous one.
            last_offset = last_offset.saturating_add(u64::from(lengths[i.saturating_sub(1)]));
        } else {
            // Absolute offset encoded as `offset + 1` (so 0 is reserved for clustered).
            last_offset = raw_val.saturating_sub(1);
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// Regression test for a real defect: `decode_directory` used to derive
    /// four separate `Vec::with_capacity(n)` allocations directly from an
    /// unvalidated, attacker-controlled varint before reading (or validating)
    /// any of the `n` per-entry varints. A 5-byte malformed input
    /// (`ff ff ff ff 05`, matching the committed fuzz artifact
    /// `fuzz/artifacts/fuzz_pmtiles_header/oom-4ebba2269e3f4f7e8473fcf48ca5425d66d0cb3a`)
    /// decodes to an entry count in the billions, which previously caused an
    /// immediate out-of-memory abort. It must now be rejected with a typed
    /// error instead.
    #[test]
    fn decode_directory_rejects_oom_varint() {
        let data = [0xff, 0xff, 0xff, 0xff, 0x05];
        let result = decode_directory(&data);
        assert!(
            result.is_err(),
            "malformed directory with huge entry count must be rejected, not allocated"
        );
    }

    #[test]
    fn decode_directory_rejects_count_exceeding_remaining_bytes() {
        // Claims 1000 entries but supplies far fewer bytes than the minimum
        // 4 bytes/entry required.
        let mut data = vec![];
        // varint for 1000 (0xE8 0x07)
        data.push(0xE8);
        data.push(0x07);
        data.extend_from_slice(&[0u8; 10]);
        let result = decode_directory(&data);
        assert!(result.is_err());
    }

    #[test]
    fn decode_directory_accepts_valid_empty() {
        let data: [u8; 0] = [];
        let entries = decode_directory(&data).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn decode_directory_round_trip_single_entry() {
        // n=1, tile_id delta=5, run_length=1, length=100, offset raw=1 (=> 0)
        let data = [
            0x01, // n_entries = 1
            0x05, // tile id delta = 5
            0x01, // run_length = 1
            0x64, // length = 100
            0x01, // offset raw = 1 -> absolute offset 0
        ];
        let entries = decode_directory(&data).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].tile_id, 5);
        assert_eq!(entries[0].run_length, 1);
        assert_eq!(entries[0].length, 100);
        assert_eq!(entries[0].offset, 0);
        assert!(entries[0].is_tile());
    }
}
