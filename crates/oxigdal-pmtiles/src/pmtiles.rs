//! High-level PMTiles reader.

use crate::directory::{DirectoryEntry, decode_directory};
use crate::error::PmTilesError;
use crate::header::{Compression, PmTilesHeader};
use crate::hilbert::{tile_id_to_zxy, zxy_to_tile_id};
use crate::layout::{LayoutAnalysis, analyze_tile_ordering};
use crate::metadata::PmTilesMetadata;

/// Metadata about a single logical tile in a PMTiles archive.
///
/// A directory entry with `run_length > 1` represents a run of consecutive
/// tile IDs that all share the same compressed data payload.  [`enumerate_tiles`]
/// expands those entries into one [`TileInfo`] per logical tile.
///
/// [`enumerate_tiles`]: PmTilesReader::enumerate_tiles
#[derive(Debug, Clone, PartialEq)]
pub struct TileInfo {
    /// PMTiles v3 tile ID (Hilbert-curve index).
    pub tile_id: u64,
    /// Zoom level.
    pub z: u8,
    /// Tile column.
    pub x: u32,
    /// Tile row.
    pub y: u32,
    /// Byte offset of the tile payload measured from the start of the
    /// tile-data section (i.e. relative to `header.tile_data_offset`).
    pub data_offset: u64,
    /// Compressed byte length of the tile payload.
    pub data_length: u32,
}

/// A PMTiles v3 archive reader backed by an in-memory byte buffer.
pub struct PmTilesReader {
    /// Parsed header.
    pub header: PmTilesHeader,
    data: Vec<u8>,
}

impl PmTilesReader {
    /// Construct a reader from the raw bytes of a PMTiles file.
    ///
    /// # Errors
    /// Propagates any error from [`PmTilesHeader::parse`].
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, PmTilesError> {
        let header = PmTilesHeader::parse(&data)?;
        Ok(Self { header, data })
    }

    /// Return the raw (possibly compressed) bytes of the root directory.
    ///
    /// # Errors
    /// Returns [`PmTilesError::InvalidFormat`] when the directory region falls
    /// outside the file.
    pub fn raw_root_directory(&self) -> Result<&[u8], PmTilesError> {
        let start = self.header.root_dir_offset as usize;
        let end = start + self.header.root_dir_length as usize;
        if end > self.data.len() {
            return Err(PmTilesError::InvalidFormat(format!(
                "Root directory [{start}..{end}) out of bounds (file is {} bytes)",
                self.data.len()
            )));
        }
        Ok(&self.data[start..end])
    }

    /// Decode and return the entries of the root directory.
    ///
    /// If `internal_compression` is set, the root directory is decompressed
    /// before decoding.  For uncompressed archives (as in test/synthetic files),
    /// the directory is decoded directly.
    ///
    /// # Errors
    /// Propagates errors from [`Self::raw_root_directory`], decompression, or
    /// [`decode_directory`].
    pub fn root_directory(&self) -> Result<Vec<DirectoryEntry>, PmTilesError> {
        let raw = self.raw_root_directory()?;
        let decompressed = decompress_data(raw, &self.header.internal_compression)?;
        decode_directory(&decompressed)
    }

    /// Return whether the archive's header marks it as clustered.
    ///
    /// A clustered archive stores tile data in ascending `tile_id` order with
    /// monotonically non-decreasing offsets (see [`crate::layout`]).
    pub fn is_clustered(&self) -> bool {
        self.header.clustered
    }

    /// Analyse the actual tile ordering of the root directory.
    ///
    /// Decodes the root directory, keeps only tile (non-leaf) entries, maps
    /// them to `(tile_id, data_offset, data_length)` triples, and runs
    /// [`analyze_tile_ordering`].  This re-derives clustering, deduplication,
    /// and gap statistics directly from the stored directory rather than the
    /// header flag.
    ///
    /// # Errors
    /// Propagates errors from [`Self::root_directory`] when the root directory
    /// cannot be decoded.
    pub fn detected_layout(&self) -> Result<LayoutAnalysis, PmTilesError> {
        let entries = self.root_directory()?;
        let manifest: Vec<(u64, u64, u64)> = entries
            .iter()
            .filter(|e| e.is_tile())
            .map(|e| (e.tile_id, e.offset, u64::from(e.length)))
            .collect();
        Ok(analyze_tile_ordering(&manifest))
    }

    /// Retrieve a tile by `(z, x, y)` coordinates.
    ///
    /// Returns `Ok(Some(data))` if the tile exists, `Ok(None)` if not found.
    /// The returned bytes are the raw tile payload; if `tile_compression` is not
    /// `None`, call [`decompress_tile`](`Self::decompress_tile`) or use
    /// [`get_tile_decompressed`](`Self::get_tile_decompressed`) instead.
    ///
    /// # Errors
    /// Propagates errors from coordinate conversion, directory decoding, or
    /// decompression of leaf directories.
    pub fn get_tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Vec<u8>>, PmTilesError> {
        let tile_id = zxy_to_tile_id(z, x, y)?;
        let root_entries = self.root_directory()?;
        self.find_tile_in_entries(&root_entries, tile_id)
    }

    /// Retrieve and decompress a tile by `(z, x, y)` coordinates.
    ///
    /// Transparently decompresses the tile payload based on
    /// `header.tile_compression`.
    ///
    /// Returns `Ok(Some(data))` if the tile exists, `Ok(None)` if not found.
    ///
    /// # Errors
    /// Propagates errors from tile retrieval or decompression.
    pub fn get_tile_decompressed(
        &self,
        z: u8,
        x: u32,
        y: u32,
    ) -> Result<Option<Vec<u8>>, PmTilesError> {
        let raw_tile = self.get_tile(z, x, y)?;
        match raw_tile {
            Some(data) => {
                let decompressed = decompress_data(&data, &self.header.tile_compression)?;
                Ok(Some(decompressed))
            }
            None => Ok(None),
        }
    }

    /// Decompress a raw tile payload using the archive's `tile_compression`.
    ///
    /// # Errors
    /// Returns [`PmTilesError::Decompression`] on failure.
    pub fn decompress_tile(&self, raw_data: &[u8]) -> Result<Vec<u8>, PmTilesError> {
        decompress_data(raw_data, &self.header.tile_compression)
    }

    /// Enumerate every logical tile stored in the archive, in tile-ID order.
    ///
    /// Directory entries with `run_length > 1` are expanded: each of the
    /// `run_length` consecutive tile IDs starting at `entry.tile_id` is
    /// yielded as an individual [`TileInfo`] that shares the same
    /// `data_offset` and `data_length` (content-deduplicated payload).
    ///
    /// The algorithm performs a two-level traversal matching the PMTiles v3
    /// directory structure (root → optional leaf pages).  Leaf pages are
    /// followed whenever a root entry has `run_length == 0`.
    ///
    /// The returned `Vec` is sorted by `tile_id` because directories are
    /// required to be stored in ascending tile-ID order.
    ///
    /// # Errors
    /// Propagates errors from directory decoding or decompression.
    pub fn enumerate_tiles(&self) -> Result<Vec<TileInfo>, PmTilesError> {
        let root_entries = self.root_directory()?;
        let mut infos: Vec<TileInfo> = Vec::new();

        for entry in &root_entries {
            if entry.is_leaf_directory() {
                // Follow the leaf pointer.
                let leaf_raw = self.read_leaf_directory(entry.offset, entry.length)?;
                let leaf_decompressed =
                    decompress_data(&leaf_raw, &self.header.internal_compression)?;
                let leaf_entries = decode_directory(&leaf_decompressed)?;
                for leaf_entry in &leaf_entries {
                    if leaf_entry.is_tile() {
                        expand_entry_into(leaf_entry, &mut infos)?;
                    }
                    // Nested leaf directories are not supported in PMTiles v3.
                }
            } else {
                // Tile data entry — expand run length.
                expand_entry_into(entry, &mut infos)?;
            }
        }

        // Directories are required to be sorted; preserve that order.
        infos.sort_by_key(|t| t.tile_id);

        Ok(infos)
    }

    /// Parse and return the structured JSON metadata embedded in the archive.
    ///
    /// The metadata section may be compressed using the archive's
    /// `internal_compression`.  An empty metadata section (length 0) is
    /// treated as `{}` and returns an all-`None` [`PmTilesMetadata`] instance.
    ///
    /// # Errors
    /// - [`PmTilesError::InvalidArchive`] when the section falls outside the
    ///   file bounds.
    /// - [`PmTilesError::Decompression`] on decompression failure.
    /// - [`PmTilesError::JsonParse`] on malformed JSON.
    pub fn metadata(&self) -> Result<PmTilesMetadata, PmTilesError> {
        let offset = self.header.metadata_offset as usize;
        let length = self.header.metadata_length as usize;
        if offset + length > self.data.len() {
            return Err(PmTilesError::InvalidArchive(format!(
                "Metadata section [{offset}..{}) out of bounds (file is {} bytes)",
                offset + length,
                self.data.len()
            )));
        }
        let raw = &self.data[offset..offset + length];
        PmTilesMetadata::from_bytes(raw, self.header.internal_compression.clone())
    }

    /// Detect the tile data format of a specific tile from its raw content.
    ///
    /// Fetches the raw (possibly compressed) tile bytes and sniffs their
    /// leading bytes for well-known magic sequences.  Returns `None` when
    /// the tile does not exist in the archive.
    ///
    /// # Errors
    /// Propagates any error from [`Self::get_tile`].
    pub fn detect_tile_format(
        &self,
        z: u8,
        x: u32,
        y: u32,
    ) -> Result<Option<crate::tile_detect::DetectedTileFormat>, PmTilesError> {
        match self.get_tile(z, x, y)? {
            Some(data) => Ok(Some(crate::tile_detect::detect_tile_format(&data))),
            None => Ok(None),
        }
    }

    /// Sample up to `sample_size` tiles from the archive and return the most
    /// commonly detected format.
    ///
    /// Iterates through the tile directory in tile-ID order, reads each tile's
    /// raw bytes directly from the data buffer (without calling `get_tile` to
    /// avoid repeated directory traversal), and counts format occurrences using
    /// `crate::tile_detect::FormatCounts`.
    ///
    /// Returns [`crate::tile_detect::DetectedTileFormat::Unknown`] when the
    /// archive contains no tiles or when `sample_size` is zero.
    ///
    /// # Errors
    /// Propagates errors from [`Self::enumerate_tiles`].
    pub fn detect_dominant_format(
        &self,
        sample_size: usize,
    ) -> Result<crate::tile_detect::DetectedTileFormat, PmTilesError> {
        use crate::tile_detect::{DetectedTileFormat, FormatCounts, detect_tile_format};

        let tiles = self.enumerate_tiles()?;
        let mut counts = FormatCounts::default();

        for info in tiles.iter().take(sample_size) {
            let offset = (self.header.tile_data_offset + info.data_offset) as usize;
            let length = info.data_length as usize;
            // Skip entries that would read out-of-bounds — the archive may be
            // corrupt but we don't want to panic; we simply omit the sample.
            if offset.saturating_add(length) > self.data.len() {
                continue;
            }
            let raw = &self.data[offset..offset + length];
            counts.add(detect_tile_format(raw));
        }

        Ok(counts.dominant().unwrap_or(DetectedTileFormat::Unknown))
    }

    /// Search directory entries (root or leaf) for the given tile ID.
    ///
    /// If a leaf directory pointer is found, it is followed by extracting
    /// and decompressing the leaf directory from the leaf_dirs section.
    fn find_tile_in_entries(
        &self,
        entries: &[DirectoryEntry],
        tile_id: u64,
    ) -> Result<Option<Vec<u8>>, PmTilesError> {
        let entry = binary_search_entries(entries, tile_id);

        match entry {
            None => Ok(None),
            Some(e) if e.is_leaf_directory() => {
                // Follow leaf directory pointer.
                let leaf_data = self.read_leaf_directory(e.offset, e.length)?;
                let decompressed = decompress_data(&leaf_data, &self.header.internal_compression)?;
                let leaf_entries = decode_directory(&decompressed)?;
                // Search the leaf directory (no further recursion — PMTiles v3
                // has at most 2 levels: root + leaf).
                let leaf_match = binary_search_entries(&leaf_entries, tile_id);
                match leaf_match {
                    None => Ok(None),
                    Some(le) if le.is_tile() => self.extract_tile_data(le),
                    Some(_) => {
                        // Nested leaf pointer — not supported in v3 spec.
                        Err(PmTilesError::InvalidFormat(
                            "Nested leaf directories are not supported in PMTiles v3".into(),
                        ))
                    }
                }
            }
            Some(e) if e.is_tile() => self.extract_tile_data(e),
            Some(_) => Ok(None),
        }
    }

    /// Extract raw tile bytes from the tile-data section.
    fn extract_tile_data(&self, entry: &DirectoryEntry) -> Result<Option<Vec<u8>>, PmTilesError> {
        let start = (self.header.tile_data_offset + entry.offset) as usize;
        let end = start + entry.length as usize;
        if end > self.data.len() {
            return Err(PmTilesError::InvalidFormat(format!(
                "Tile data [{start}..{end}) out of bounds (file is {} bytes)",
                self.data.len()
            )));
        }
        Ok(Some(self.data[start..end].to_vec()))
    }

    /// Read a leaf directory page from the leaf_dirs section.
    fn read_leaf_directory(&self, offset: u64, length: u32) -> Result<Vec<u8>, PmTilesError> {
        let start = (self.header.leaf_dirs_offset + offset) as usize;
        let end = start + length as usize;
        if end > self.data.len() {
            return Err(PmTilesError::InvalidFormat(format!(
                "Leaf directory [{start}..{end}) out of bounds (file is {} bytes)",
                self.data.len()
            )));
        }
        Ok(self.data[start..end].to_vec())
    }
}

/// Expand a tile [`DirectoryEntry`] (with `run_length >= 1`) into one or more
/// [`TileInfo`] records and append them to `out`.
///
/// A `run_length` of 1 yields a single entry.  A `run_length` of N yields N
/// entries with consecutive tile IDs (tile_id, tile_id+1, …, tile_id+N-1),
/// all sharing the same `data_offset` and `data_length`.
///
/// # Errors
/// Propagates errors from [`tile_id_to_zxy`].
fn expand_entry_into(entry: &DirectoryEntry, out: &mut Vec<TileInfo>) -> Result<(), PmTilesError> {
    for i in 0..u64::from(entry.run_length) {
        let current_id = entry.tile_id + i;
        let (z, x, y) = tile_id_to_zxy(current_id)?;
        out.push(TileInfo {
            tile_id: current_id,
            z,
            x,
            y,
            data_offset: entry.offset,
            data_length: entry.length,
        });
    }
    Ok(())
}

/// Binary search for a tile ID within sorted directory entries.
///
/// A tile entry matches if `entry.tile_id <= tile_id < entry.tile_id + entry.run_length`
/// (for tile entries with run_length > 0).
/// A leaf entry (run_length == 0) matches if `tile_id >= entry.tile_id` and either
/// this is the last entry or `tile_id < next_entry.tile_id`.
pub(crate) fn binary_search_entries(
    entries: &[DirectoryEntry],
    tile_id: u64,
) -> Option<&DirectoryEntry> {
    if entries.is_empty() {
        return None;
    }

    // Binary search for the rightmost entry whose tile_id <= tile_id.
    let idx = match entries.binary_search_by_key(&tile_id, |e| e.tile_id) {
        Ok(i) => i,
        Err(0) => return None, // tile_id < all entries
        Err(i) => i - 1,       // largest entry.tile_id <= tile_id
    };

    let entry = &entries[idx];

    if entry.is_leaf_directory() {
        // Leaf directory: check that tile_id falls within this leaf's range.
        // (tile_id >= entry.tile_id is guaranteed by binary search)
        Some(entry)
    } else if tile_id < entry.tile_id + u64::from(entry.run_length) {
        // Tile entry: tile_id is within the run.
        Some(entry)
    } else {
        None
    }
}

/// Decompress data based on the given compression algorithm.
///
/// When `compression` is `None`, returns a copy of the input.
/// When the `compression` feature is not enabled, returns an error for
/// non-`None` compression types.
///
/// # Errors
/// Returns [`PmTilesError::Decompression`] on failure or unsupported compression.
pub fn decompress_data(data: &[u8], compression: &Compression) -> Result<Vec<u8>, PmTilesError> {
    match compression {
        Compression::None => Ok(data.to_vec()),
        Compression::Unknown => Ok(data.to_vec()),
        #[cfg(feature = "compression")]
        Compression::Gzip => {
            let mut reader = std::io::Cursor::new(data);
            oxiarc_archive::gzip::decompress(&mut reader)
                .map_err(|e| PmTilesError::Decompression(format!("Gzip decompression failed: {e}")))
        }
        #[cfg(feature = "compression")]
        Compression::Brotli => oxiarc_archive::brotli::decompress(data)
            .map_err(|e| PmTilesError::Decompression(format!("Brotli decompression failed: {e}"))),
        #[cfg(feature = "compression")]
        Compression::Zstd => oxiarc_archive::zstd::decompress(data)
            .map_err(|e| PmTilesError::Decompression(format!("Zstd decompression failed: {e}"))),
        #[cfg(not(feature = "compression"))]
        other => Err(PmTilesError::Decompression(format!(
            "{other:?} decompression requires the `compression` feature"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_search_entries_empty() {
        assert!(binary_search_entries(&[], 0).is_none());
    }

    #[test]
    fn test_binary_search_entries_exact_match() {
        let entries = vec![DirectoryEntry {
            tile_id: 5,
            offset: 0,
            length: 100,
            run_length: 1,
        }];
        let result = binary_search_entries(&entries, 5);
        assert!(result.is_some());
        assert_eq!(result.map(|e| e.tile_id), Some(5));
    }

    #[test]
    fn test_binary_search_entries_within_run() {
        let entries = vec![DirectoryEntry {
            tile_id: 10,
            offset: 0,
            length: 100,
            run_length: 5,
        }];
        // tile_id 12 is within run [10, 15)
        let result = binary_search_entries(&entries, 12);
        assert!(result.is_some());
        assert_eq!(result.map(|e| e.tile_id), Some(10));
    }

    #[test]
    fn test_binary_search_entries_outside_run() {
        let entries = vec![DirectoryEntry {
            tile_id: 10,
            offset: 0,
            length: 100,
            run_length: 2,
        }];
        // tile_id 12 is outside run [10, 12)
        assert!(binary_search_entries(&entries, 12).is_none());
    }

    #[test]
    fn test_binary_search_entries_before_all() {
        let entries = vec![DirectoryEntry {
            tile_id: 10,
            offset: 0,
            length: 100,
            run_length: 1,
        }];
        assert!(binary_search_entries(&entries, 5).is_none());
    }

    #[test]
    fn test_binary_search_entries_leaf_directory() {
        let entries = vec![DirectoryEntry {
            tile_id: 0,
            offset: 1024,
            length: 512,
            run_length: 0, // leaf
        }];
        let result = binary_search_entries(&entries, 5);
        assert!(result.is_some());
        assert!(result.is_some_and(|e| e.is_leaf_directory()));
    }

    #[test]
    fn test_decompress_data_none() {
        let data = b"hello world";
        let result = decompress_data(data, &Compression::None).expect("ok");
        assert_eq!(result, data);
    }

    #[test]
    fn test_decompress_data_unknown() {
        let data = b"raw bytes";
        let result = decompress_data(data, &Compression::Unknown).expect("ok");
        assert_eq!(result, data);
    }
}
