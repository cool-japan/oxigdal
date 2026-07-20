//! Tile-set diff between two PMTiles archives.
//!
//! Compares two in-memory PMTiles v3 archives and reports per-tile
//! changes — tiles added, removed, or content-changed — using a 64-bit
//! [FNV-1a](https://en.wikipedia.org/wiki/Fowler%E2%80%93Noll%E2%80%93Vo_hash_function)
//! non-cryptographic hash for cheap content comparison.
//!
//! The diff is purely structural: tiles are identified by their PMTiles v3
//! Hilbert-curve tile ID, and content-changed tiles are detected by
//! comparing their decompressed-as-stored (raw) byte payload's FNV-1a hash.
//! Two tiles with identical bytes are treated as unchanged even when the
//! source archives differ in unrelated layout (offsets, dedup runs, …).
//!
//! # Output stability
//!
//! Each `Vec` field of [`DiffReport`] is sorted by ascending `tile_id` so
//! that the output is deterministic and suitable for direct comparison
//! across runs.
//!
//! # Example
//!
//! ```no_run
//! use oxigeo_pmtiles::{PmTilesBuilder, TileType, diff_archives};
//!
//! let mut old = PmTilesBuilder::new(TileType::Png, 0, 0);
//! old.add_tile(0, 0, 0, b"v1").unwrap();
//! let old_bytes = old.build().unwrap();
//!
//! let mut new = PmTilesBuilder::new(TileType::Png, 0, 0);
//! new.add_tile(0, 0, 0, b"v2").unwrap();
//! let new_bytes = new.build().unwrap();
//!
//! let report = diff_archives(&old_bytes, &new_bytes).unwrap();
//! assert_eq!(report.added.len(), 0);
//! assert_eq!(report.removed.len(), 0);
//! assert_eq!(report.changed.len(), 1);
//! ```

use std::collections::HashMap;

use crate::error::PmTilesError;
use crate::pmtiles::PmTilesReader;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single per-tile change reported by [`diff_archives`].
///
/// Every variant carries the PMTiles v3 `tile_id` along with the resolved
/// `(z, x, y)` coordinates and the relevant byte size(s).  This is enough to
/// drive most downstream tooling (CDN purge lists, sync utilities, audit
/// logs) without requiring access to the original archives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TileChange {
    /// Tile present in the new archive but absent in the old archive.
    Added {
        /// PMTiles v3 Hilbert-curve tile ID.
        tile_id: u64,
        /// Zoom level resolved from the tile ID.
        z: u8,
        /// Tile column resolved from the tile ID.
        x: u32,
        /// Tile row resolved from the tile ID.
        y: u32,
        /// Byte length of the tile payload in the new archive.
        new_bytes: usize,
    },
    /// Tile present in the old archive but absent in the new archive.
    Removed {
        /// PMTiles v3 Hilbert-curve tile ID.
        tile_id: u64,
        /// Zoom level resolved from the tile ID.
        z: u8,
        /// Tile column resolved from the tile ID.
        x: u32,
        /// Tile row resolved from the tile ID.
        y: u32,
        /// Byte length of the tile payload in the old archive.
        old_bytes: usize,
    },
    /// Tile present in both archives with different content.
    Changed {
        /// PMTiles v3 Hilbert-curve tile ID.
        tile_id: u64,
        /// Zoom level resolved from the tile ID.
        z: u8,
        /// Tile column resolved from the tile ID.
        x: u32,
        /// Tile row resolved from the tile ID.
        y: u32,
        /// Byte length of the tile payload in the old archive.
        old_bytes: usize,
        /// Byte length of the tile payload in the new archive.
        new_bytes: usize,
    },
}

impl TileChange {
    /// PMTiles v3 tile ID of the changed tile, regardless of variant.
    pub fn tile_id(&self) -> u64 {
        match self {
            Self::Added { tile_id, .. }
            | Self::Removed { tile_id, .. }
            | Self::Changed { tile_id, .. } => *tile_id,
        }
    }

    /// Resolved zoom level of the changed tile.
    pub fn zoom(&self) -> u8 {
        match self {
            Self::Added { z, .. } | Self::Removed { z, .. } | Self::Changed { z, .. } => *z,
        }
    }

    /// Resolved `(x, y)` tile column/row.
    pub fn xy(&self) -> (u32, u32) {
        match self {
            Self::Added { x, y, .. } | Self::Removed { x, y, .. } | Self::Changed { x, y, .. } => {
                (*x, *y)
            }
        }
    }
}

/// Full per-tile diff between two PMTiles archives.
///
/// Each `Vec` is sorted by ascending `tile_id`.  `unchanged_count` counts
/// tiles present in both archives whose FNV-1a content hashes match.
#[derive(Debug, Clone, Default)]
pub struct DiffReport {
    /// Tiles present in the new archive but absent in the old archive.
    pub added: Vec<TileChange>,
    /// Tiles present in the old archive but absent in the new archive.
    pub removed: Vec<TileChange>,
    /// Tiles present in both with differing FNV-1a content hashes.
    pub changed: Vec<TileChange>,
    /// Tiles present in both with identical FNV-1a content hashes.
    pub unchanged_count: u64,
}

impl DiffReport {
    /// Total number of changes (added + removed + changed).
    pub fn total_changes(&self) -> usize {
        self.added.len() + self.removed.len() + self.changed.len()
    }

    /// Sum of byte lengths of all tiles in the [`Self::added`] list.
    pub fn total_added_bytes(&self) -> usize {
        self.added
            .iter()
            .map(|c| match c {
                TileChange::Added { new_bytes, .. } => *new_bytes,
                _ => 0,
            })
            .sum()
    }

    /// Sum of byte lengths of all tiles in the [`Self::removed`] list.
    pub fn total_removed_bytes(&self) -> usize {
        self.removed
            .iter()
            .map(|c| match c {
                TileChange::Removed { old_bytes, .. } => *old_bytes,
                _ => 0,
            })
            .sum()
    }

    /// Net byte delta of changed tiles (new total minus old total).
    ///
    /// A positive value means changed tiles grew on average; a negative
    /// value means they shrank.
    pub fn changed_byte_delta(&self) -> i64 {
        self.changed
            .iter()
            .map(|c| match c {
                TileChange::Changed {
                    old_bytes,
                    new_bytes,
                    ..
                } => *new_bytes as i64 - *old_bytes as i64,
                _ => 0,
            })
            .sum()
    }

    /// Returns `true` when there are no added, removed, or changed tiles.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

/// Compact summary of a [`DiffReport`] (counts only).
///
/// Useful when callers only need a tally of changes without the per-tile
/// detail (e.g. for periodic monitoring or alerting).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiffSummary {
    /// Number of tiles in the new archive that are absent from the old.
    pub added: u64,
    /// Number of tiles in the old archive that are absent from the new.
    pub removed: u64,
    /// Number of tiles present in both with differing content.
    pub changed: u64,
    /// Number of tiles present in both with identical content.
    pub unchanged: u64,
}

impl DiffSummary {
    /// Total number of changes (added + removed + changed).
    pub fn total_changes(&self) -> u64 {
        self.added + self.removed + self.changed
    }

    /// Total number of tiles considered (changes + unchanged).
    pub fn total_tiles(&self) -> u64 {
        self.total_changes() + self.unchanged
    }
}

// ---------------------------------------------------------------------------
// FNV-1a 64-bit content hash
// ---------------------------------------------------------------------------

/// FNV-1a 64-bit hash used for cheap tile-content comparison.
///
/// This matches the dedup hash used by [`crate::writer::PmTilesBuilder`] so
/// that round-tripping an archive through a writer does not falsely report
/// tiles as changed.
///
/// FNV-1a is non-cryptographic; collisions are theoretically possible but
/// astronomically unlikely for tile-sized payloads.  This is acceptable for
/// the diff use case (change detection, not security).
fn fnv1a_64(data: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0100_0000_01b3;
    let mut hash = FNV_OFFSET;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

// ---------------------------------------------------------------------------
// Index building
// ---------------------------------------------------------------------------

/// Compact per-tile record kept while building the diff index.
#[derive(Debug, Clone, Copy)]
struct IndexedTile {
    z: u8,
    x: u32,
    y: u32,
    content_hash: u64,
    byte_size: usize,
}

/// Build a `tile_id → IndexedTile` map from a parsed reader.
///
/// Tiles whose directory entry exists but whose payload cannot be retrieved
/// surface a [`PmTilesError::InvalidArchive`] so that diff results are not
/// silently incorrect.
fn build_index(reader: &PmTilesReader) -> Result<HashMap<u64, IndexedTile>, PmTilesError> {
    let infos = reader.enumerate_tiles()?;
    let mut map: HashMap<u64, IndexedTile> = HashMap::with_capacity(infos.len());

    for info in infos {
        let bytes = reader.get_tile(info.z, info.x, info.y)?.ok_or_else(|| {
            PmTilesError::InvalidArchive(format!(
                "tile_id {} (z={}, x={}, y={}) listed in directory but payload missing",
                info.tile_id, info.z, info.x, info.y
            ))
        })?;
        map.insert(
            info.tile_id,
            IndexedTile {
                z: info.z,
                x: info.x,
                y: info.y,
                content_hash: fnv1a_64(&bytes),
                byte_size: bytes.len(),
            },
        );
    }

    Ok(map)
}

// ---------------------------------------------------------------------------
// Public diff functions
// ---------------------------------------------------------------------------

/// Compare two PMTiles archives by raw bytes and return a full diff report.
///
/// Both inputs must be valid PMTiles v3 archives parsable by
/// [`PmTilesReader::from_bytes`].  Tile content is compared using the raw
/// (possibly compressed) payload bytes — this matches the byte stream the
/// underlying storage actually holds and is consistent with the writer's
/// dedup behaviour.
///
/// # Errors
/// Propagates any [`PmTilesError`] from header parsing, directory decoding,
/// or tile extraction.  An [`PmTilesError::InvalidArchive`] is returned when
/// a tile listed in the directory cannot be retrieved.
pub fn diff_archives(old_bytes: &[u8], new_bytes: &[u8]) -> Result<DiffReport, PmTilesError> {
    let old_reader = PmTilesReader::from_bytes(old_bytes.to_vec())?;
    let new_reader = PmTilesReader::from_bytes(new_bytes.to_vec())?;

    let old_index = build_index(&old_reader)?;
    let new_index = build_index(&new_reader)?;

    let mut report = DiffReport::default();

    // Removed: tiles in old but not in new.
    for (&tile_id, tile) in &old_index {
        if !new_index.contains_key(&tile_id) {
            report.removed.push(TileChange::Removed {
                tile_id,
                z: tile.z,
                x: tile.x,
                y: tile.y,
                old_bytes: tile.byte_size,
            });
        }
    }

    // Added + Changed + Unchanged: iterate new and look up old.
    for (&tile_id, new_tile) in &new_index {
        match old_index.get(&tile_id) {
            None => report.added.push(TileChange::Added {
                tile_id,
                z: new_tile.z,
                x: new_tile.x,
                y: new_tile.y,
                new_bytes: new_tile.byte_size,
            }),
            Some(old_tile) => {
                if old_tile.content_hash != new_tile.content_hash {
                    report.changed.push(TileChange::Changed {
                        tile_id,
                        z: new_tile.z,
                        x: new_tile.x,
                        y: new_tile.y,
                        old_bytes: old_tile.byte_size,
                        new_bytes: new_tile.byte_size,
                    });
                } else {
                    report.unchanged_count += 1;
                }
            }
        }
    }

    // Deterministic ordering for stable output and reproducible tests.
    report.added.sort_by_key(TileChange::tile_id);
    report.removed.sort_by_key(TileChange::tile_id);
    report.changed.sort_by_key(TileChange::tile_id);

    Ok(report)
}

/// Compare two PMTiles archives and return only the summary counts.
///
/// Convenience wrapper around [`diff_archives`] that drops the per-tile
/// detail.  Has the same error semantics as [`diff_archives`].
///
/// # Errors
/// Propagates errors from [`diff_archives`].
pub fn diff_archives_summary(
    old_bytes: &[u8],
    new_bytes: &[u8],
) -> Result<DiffSummary, PmTilesError> {
    let report = diff_archives(old_bytes, new_bytes)?;
    Ok(DiffSummary {
        added: report.added.len() as u64,
        removed: report.removed.len() as u64,
        changed: report.changed.len() as u64,
        unchanged: report.unchanged_count,
    })
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fnv1a_64_known_offset_for_empty_input() {
        // Standard FNV-1a 64 spec: empty input → FNV_OFFSET.
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn test_fnv1a_64_distinguishes_different_inputs() {
        assert_ne!(fnv1a_64(b"hello"), fnv1a_64(b"world"));
    }

    #[test]
    fn test_fnv1a_64_is_deterministic() {
        assert_eq!(fnv1a_64(b"oxigeo"), fnv1a_64(b"oxigeo"));
    }

    #[test]
    fn test_diff_report_default_is_empty() {
        let r = DiffReport::default();
        assert!(r.is_empty());
        assert_eq!(r.total_changes(), 0);
        assert_eq!(r.total_added_bytes(), 0);
        assert_eq!(r.total_removed_bytes(), 0);
        assert_eq!(r.changed_byte_delta(), 0);
    }

    #[test]
    fn test_diff_summary_aggregates_counts() {
        let s = DiffSummary {
            added: 1,
            removed: 2,
            changed: 3,
            unchanged: 4,
        };
        assert_eq!(s.total_changes(), 6);
        assert_eq!(s.total_tiles(), 10);
    }

    #[test]
    fn test_tile_change_accessors() {
        let added = TileChange::Added {
            tile_id: 7,
            z: 2,
            x: 1,
            y: 1,
            new_bytes: 32,
        };
        assert_eq!(added.tile_id(), 7);
        assert_eq!(added.zoom(), 2);
        assert_eq!(added.xy(), (1, 1));

        let removed = TileChange::Removed {
            tile_id: 11,
            z: 3,
            x: 4,
            y: 5,
            old_bytes: 100,
        };
        assert_eq!(removed.tile_id(), 11);
        assert_eq!(removed.zoom(), 3);
        assert_eq!(removed.xy(), (4, 5));

        let changed = TileChange::Changed {
            tile_id: 21,
            z: 4,
            x: 2,
            y: 3,
            old_bytes: 10,
            new_bytes: 20,
        };
        assert_eq!(changed.tile_id(), 21);
        assert_eq!(changed.zoom(), 4);
        assert_eq!(changed.xy(), (2, 3));
    }
}
