//! PMTiles v2 backward-compatible reader and upgrade utility.
//!
//! Parses archives written in the PMTiles v2 format (magic `b"PM\x02"`) and
//! exposes tile retrieval, full enumeration, and an upgrade path to v3 via
//! [`PmTilesV2Reader::upgrade_to_v3`].
//!
//! Reference: <https://github.com/protomaps/PMTiles/blob/master/spec/v2/v2.md>

use std::path::Path;

use crate::error::PmTilesError;
use crate::header::TileType;
use crate::header_v2::{
    PmTilesV2Entry, PmTilesV2Header, V2_ENTRY_SIZE, parse_v2_header, read_v2_entry,
};
use crate::writer::PmTilesBuilder;

/// A decoded tile from a PMTiles v2 archive: `(z, x, y, raw_blob)`.
pub type V2Tile = (u8, u32, u32, Vec<u8>);

// ---------------------------------------------------------------------------
// Version detection
// ---------------------------------------------------------------------------

/// Detect the PMTiles spec version encoded in the first few bytes of `data`.
///
/// Returns `Ok(2)` for v2 archives (magic `b"PM\x02"`), `Ok(3)` for v3
/// archives (magic `b"PMTiles\x03"`), or an error for anything else.
///
/// # Errors
///
/// Returns [`PmTilesError::InvalidFormat`] when `data` is too short or does
/// not match either known format.
pub fn detect_pmtiles_version(data: &[u8]) -> Result<u8, PmTilesError> {
    if data.len() < 3 {
        return Err(PmTilesError::InvalidFormat(
            "too short to detect PMTiles version".into(),
        ));
    }

    // v2: 2-byte magic 'PM' + version byte 2
    if data[0] == b'P' && data[1] == b'M' && data.get(2) == Some(&2) {
        return Ok(2);
    }

    // v3: 7-byte magic 'PMTiles' + version byte 3 at index 7
    if data.starts_with(b"PMTiles") && data.get(7) == Some(&3) {
        return Ok(3);
    }

    Err(PmTilesError::InvalidFormat(
        "not a PMTiles file (unrecognised magic / version)".into(),
    ))
}

// ---------------------------------------------------------------------------
// PmTilesV2Reader
// ---------------------------------------------------------------------------

/// A PMTiles v2 archive reader backed by an in-memory byte buffer.
///
/// Provides tile retrieval by `(z, x, y)`, full tile enumeration, and
/// conversion to a PMTiles v3 [`PmTilesBuilder`] via [`upgrade_to_v3`].
///
/// [`upgrade_to_v3`]: PmTilesV2Reader::upgrade_to_v3
pub struct PmTilesV2Reader {
    /// The raw bytes of the archive.
    data: Vec<u8>,
    /// The parsed v2 header (including root directory entries).
    header: PmTilesV2Header,
}

impl PmTilesV2Reader {
    /// Construct a reader from the raw bytes of a PMTiles v2 archive.
    ///
    /// # Errors
    ///
    /// Returns an error when `data` is not a valid PMTiles v2 archive.
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, PmTilesError> {
        let header = parse_v2_header(&data)?;
        Ok(Self { data, header })
    }

    /// Construct a reader by reading the entire file at `path` into memory.
    ///
    /// # Errors
    ///
    /// Propagates I/O errors from [`std::fs::read`] and parsing errors from
    /// [`Self::from_bytes`].
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, PmTilesError> {
        let data = std::fs::read(path)?;
        Self::from_bytes(data)
    }

    /// Return a reference to the parsed PMTiles v2 header.
    pub fn header(&self) -> &PmTilesV2Header {
        &self.header
    }

    /// Return the raw JSON metadata string embedded in the archive.
    pub fn metadata_json(&self) -> &str {
        &self.header.metadata
    }

    // -----------------------------------------------------------------------
    // Tile retrieval
    // -----------------------------------------------------------------------

    /// Retrieve a tile by `(z, x, y)` coordinates.
    ///
    /// Searches the root directory linearly.  When a leaf-directory pointer
    /// matches the requested coordinates, the sub-entries are read from the
    /// archive buffer at the recorded offset.
    ///
    /// Returns `Ok(Some(bytes))` when the tile exists, `Ok(None)` when it is
    /// not present in the archive.
    ///
    /// # Errors
    ///
    /// Returns [`PmTilesError::InvalidFormat`] when a leaf-directory offset /
    /// length falls outside the archive buffer.
    pub fn get_tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Vec<u8>>, PmTilesError> {
        // First look in root entries.
        if let Some(blob) = self.find_in_entries(&self.header.root_entries, z, x, y)? {
            return Ok(Some(blob));
        }

        // Walk leaf directories pointed to by root entries with is_dir == true.
        for root_entry in &self.header.root_entries {
            if !root_entry.is_dir {
                continue;
            }
            let leaf_entries = self.read_leaf_entries(root_entry)?;
            if let Some(blob) = self.find_in_entries(&leaf_entries, z, x, y)? {
                return Ok(Some(blob));
            }
        }

        Ok(None)
    }

    /// Enumerate every non-directory tile stored in the archive.
    ///
    /// Returns a `Vec` of `(z, x, y, blob)` tuples.  Leaf directories pointed
    /// to by root entries are followed recursively (one level deep, as
    /// specified by PMTiles v2).
    ///
    /// # Errors
    ///
    /// Propagates errors from leaf-directory reading or data extraction.
    pub fn enumerate_tiles(&self) -> Result<Vec<V2Tile>, PmTilesError> {
        let mut result: Vec<V2Tile> = Vec::new();

        for entry in &self.header.root_entries {
            if entry.is_dir {
                // Follow the leaf directory pointer.
                let leaf_entries = self.read_leaf_entries(entry)?;
                for leaf in &leaf_entries {
                    if !leaf.is_dir {
                        let blob = self.extract_blob(leaf)?;
                        result.push((leaf.z, leaf.x, leaf.y, blob));
                    }
                }
            } else {
                let blob = self.extract_blob(entry)?;
                result.push((entry.z, entry.x, entry.y, blob));
            }
        }

        Ok(result)
    }

    /// Upgrade this v2 archive into a PMTiles v3 [`PmTilesBuilder`].
    ///
    /// The builder is returned un-built; the caller should inspect or modify
    /// it and then call [`PmTilesBuilder::build`] to produce the final bytes.
    ///
    /// The tile type is detected from the `"format"` key in the embedded JSON
    /// metadata:
    ///
    /// | `"format"` value | [`TileType`]         |
    /// |------------------|----------------------|
    /// | `"png"`          | [`TileType::Png`]    |
    /// | `"jpg"` / `"jpeg"` | [`TileType::Jpeg`] |
    /// | `"webp"`         | [`TileType::Webp`]   |
    /// | `"avif"`         | [`TileType::Avif`]   |
    /// | `"pbf"` / `"mvt"` | [`TileType::Mvt`]  |
    /// | anything else    | [`TileType::Unknown`] |
    ///
    /// Zoom range and geographic bounds are derived automatically from the
    /// tiles present in the archive via
    /// [`PmTilesBuilder::auto_all`].
    ///
    /// # Errors
    ///
    /// Propagates errors from [`Self::enumerate_tiles`] or
    /// [`PmTilesBuilder::add_tile`].
    pub fn upgrade_to_v3(&self) -> Result<PmTilesBuilder, PmTilesError> {
        let tile_type = detect_tile_type_from_metadata(&self.header.metadata);

        // Enumerate all tiles first so we know the zoom range.
        let tiles = self.enumerate_tiles()?;

        let (min_z, max_z) = zoom_range_from_tiles(&tiles);

        let mut builder = PmTilesBuilder::new(tile_type, min_z, max_z);

        // Copy the raw metadata string into the v3 archive unchanged.
        builder.set_metadata(self.header.metadata.clone());

        for (z, x, y, blob) in tiles {
            builder.add_tile(z, x, y, &blob)?;
        }

        // Derive zoom range, bounds, and centre from the actual tile set.
        builder.auto_all();

        Ok(builder)
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Search `entries` linearly for a tile at `(z, x, y)`.
    ///
    /// PMTiles v2 does not require entries to be sorted, so we cannot safely
    /// use binary search.  The root directory is small (fits in 512 bytes
    /// minus the header), so a linear scan is acceptable.
    fn find_in_entries(
        &self,
        entries: &[PmTilesV2Entry],
        z: u8,
        x: u32,
        y: u32,
    ) -> Result<Option<Vec<u8>>, PmTilesError> {
        for entry in entries {
            if entry.is_dir {
                continue;
            }
            if entry.z == z && entry.x == x && entry.y == y {
                let blob = self.extract_blob(entry)?;
                return Ok(Some(blob));
            }
        }
        Ok(None)
    }

    /// Parse leaf directory entries referenced by a root directory pointer.
    ///
    /// A root entry with `is_dir == true` has `offset` pointing to a sequence
    /// of 17-byte entries within the archive buffer.  The v2 spec does not
    /// encode the sub-directory length explicitly, so we read all complete
    /// 17-byte entries from `offset` to the end of the buffer (or until we
    /// hit a non-entry region).
    ///
    /// In practice the v2 sub-directory length can be inferred by the spec
    /// as "all entries that fit in the block pointed to by offset".  Since
    /// the v2 format stores entries in a single 512-byte root region (minus
    /// the header), leaf directories are stored after the tile data section.
    /// We cap the read at 512 bytes (30 entries × 17 = 510 bytes) to match
    /// the spec limit.
    fn read_leaf_entries(
        &self,
        dir_entry: &PmTilesV2Entry,
    ) -> Result<Vec<PmTilesV2Entry>, PmTilesError> {
        let start = dir_entry.offset as usize;

        if start > self.data.len() {
            return Err(PmTilesError::InvalidFormat(format!(
                "leaf directory offset {start} exceeds archive length {}",
                self.data.len()
            )));
        }

        let available = self.data.len() - start;
        // Cap to 512 bytes as a safety measure (spec root limit).
        let readable = available.min(512);
        let leaf_data = &self.data[start..start + readable];

        let entry_count = leaf_data.len() / V2_ENTRY_SIZE;
        let mut entries = Vec::with_capacity(entry_count);

        for i in 0..entry_count {
            let entry = read_v2_entry(leaf_data, i * V2_ENTRY_SIZE)?;
            entries.push(entry);
        }

        Ok(entries)
    }

    /// Extract the raw tile blob referenced by a directory entry.
    ///
    /// # Errors
    ///
    /// Returns [`PmTilesError::InvalidFormat`] when the requested byte range
    /// `[entry.offset .. entry.offset + entry.length]` falls outside the
    /// archive buffer.
    fn extract_blob(&self, entry: &PmTilesV2Entry) -> Result<Vec<u8>, PmTilesError> {
        let start = entry.offset as usize;
        let end = start + entry.length as usize;

        if end > self.data.len() {
            return Err(PmTilesError::InvalidFormat(format!(
                "tile data [{start}..{end}) out of bounds (archive is {} bytes)",
                self.data.len()
            )));
        }

        Ok(self.data[start..end].to_vec())
    }
}

// ---------------------------------------------------------------------------
// Metadata helpers
// ---------------------------------------------------------------------------

/// Detect the [`TileType`] from the raw JSON metadata string.
///
/// Looks for a top-level `"format"` key and maps its value to the appropriate
/// [`TileType`] variant.  Falls back to [`TileType::Unknown`] for unrecognised
/// or absent values.
fn detect_tile_type_from_metadata(metadata_json: &str) -> TileType {
    // Use a minimal JSON parse via serde_json to extract the "format" field.
    // If serde_json is unavailable or the parse fails, default to Unknown.
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(metadata_json) {
        if let Some(format) = value.get("format").and_then(|v| v.as_str()) {
            return match format.to_ascii_lowercase().as_str() {
                "png" => TileType::Png,
                "jpg" | "jpeg" => TileType::Jpeg,
                "webp" => TileType::Webp,
                "avif" => TileType::Avif,
                "pbf" | "mvt" => TileType::Mvt,
                _ => TileType::Unknown,
            };
        }
    }
    TileType::Unknown
}

/// Compute the zoom range from a list of `(z, x, y, blob)` tiles.
///
/// Returns `(0, 0)` when the list is empty.
fn zoom_range_from_tiles(tiles: &[V2Tile]) -> (u8, u8) {
    if tiles.is_empty() {
        return (0, 0);
    }
    let mut min_z = u8::MAX;
    let mut max_z = 0u8;
    for &(z, _, _, _) in tiles {
        if z < min_z {
            min_z = z;
        }
        if z > max_z {
            max_z = z;
        }
    }
    (min_z, max_z)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_minimal_v2(metadata: &str) -> Vec<u8> {
        let meta_bytes = metadata.as_bytes();
        let mut buf = Vec::new();
        buf.extend_from_slice(b"PM\x02");
        buf.extend_from_slice(&(meta_bytes.len() as u16).to_le_bytes());
        buf.extend_from_slice(meta_bytes);
        buf
    }

    #[test]
    fn test_detect_version_v2() {
        let data = b"PM\x02\x00\x00";
        assert_eq!(detect_pmtiles_version(data).expect("ok"), 2);
    }

    #[test]
    fn test_detect_version_v3() {
        use crate::header::TileType;
        use crate::writer::PmTilesBuilder;
        let builder = PmTilesBuilder::new(TileType::Png, 0, 0);
        let archive = builder.build().expect("build ok");
        assert_eq!(detect_pmtiles_version(&archive).expect("ok"), 3);
    }

    #[test]
    fn test_detect_version_garbage() {
        assert!(detect_pmtiles_version(b"garbage").is_err());
    }

    #[test]
    fn test_detect_tile_type_png() {
        let tt = detect_tile_type_from_metadata(r#"{"format":"png"}"#);
        assert_eq!(tt, TileType::Png);
    }

    #[test]
    fn test_detect_tile_type_pbf() {
        let tt = detect_tile_type_from_metadata(r#"{"format":"pbf"}"#);
        assert_eq!(tt, TileType::Mvt);
    }

    #[test]
    fn test_detect_tile_type_unknown() {
        let tt = detect_tile_type_from_metadata(r#"{"format":"xyz"}"#);
        assert_eq!(tt, TileType::Unknown);
    }

    #[test]
    fn test_from_bytes_minimal() {
        let data = make_minimal_v2("{}");
        let reader = PmTilesV2Reader::from_bytes(data).expect("ok");
        assert_eq!(reader.metadata_json(), "{}");
        assert!(reader.header().root_entries.is_empty());
    }

    #[test]
    fn test_from_bytes_invalid() {
        assert!(PmTilesV2Reader::from_bytes(b"garbage".to_vec()).is_err());
    }
}
