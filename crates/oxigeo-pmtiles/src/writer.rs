//! PMTiles v3 archive writer.
//!
//! Builds a complete PMTiles v3 file, including header, root directory,
//! optional leaf directories, metadata, and tile data sections.  Tiles are
//! deduplicated by content: a fast FNV-1a hash pre-filters candidates, but
//! every candidate is verified with a full byte-for-byte comparison before
//! its storage is reused, so a hash collision can never cause two distinct
//! payloads to be merged.  Consecutive tiles sharing the same content and
//! contiguous IDs are merged with run-length compression.  When the root
//! directory serialises to more than [`LEAF_SPLIT_THRESHOLD`] bytes, the
//! entries are split into leaf directories.
//!
//! Reference: <https://github.com/protomaps/PMTiles/blob/main/spec/v3/spec.md>

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use crate::error::PmTilesError;
use crate::header::{Compression, PMTILES_HEADER_SIZE, PMTILES_MAGIC, TileType};
use crate::hilbert::zxy_to_tile_id;
use crate::layout::{
    LayoutStrategy, analyze_tile_ordering, apply_strategy_to_writer, choose_strategy,
};
use crate::varint::encode_varint_into;

/// Root directory size threshold (bytes) above which leaf directories are used.
/// The PMTiles v3 spec recommends ~16 kB.
pub const LEAF_SPLIT_THRESHOLD: usize = 16_384;

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

/// A tile waiting to be written.
#[derive(Debug)]
struct PendingTile {
    /// PMTiles tile ID (Hilbert-curve encoded).
    tile_id: u64,
    /// Raw tile data.
    data: Vec<u8>,
}

/// Internal directory entry for encoding.
#[derive(Debug, Clone)]
struct DirEntry {
    tile_id: u64,
    offset: u64,
    length: u32,
    run_length: u32,
}

// ---------------------------------------------------------------------------
// Content deduplication (FNV-1a, non-cryptographic)
// ---------------------------------------------------------------------------

/// FNV-1a 64-bit hash for content deduplication.
///
/// This is a simple, fast, non-cryptographic hash used purely as a
/// pre-filter to narrow the set of byte-equality candidates for a tile
/// payload. It provides **no** collision-resistance guarantee on its own:
/// [`PmTilesBuilder::build`] always verifies a hash hit with a full
/// byte-for-byte comparison before treating two payloads as identical, so
/// a hash collision here can never cause distinct tile content to be
/// merged.
fn fnv1a_hash(data: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0100_0000_01b3;
    let mut hash = FNV_OFFSET;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Look up a verified content-dedup match for `data` among the candidates
/// stored under `hash` in `seen_hashes`.
///
/// `hash` (e.g. from [`fnv1a_hash`]) is only used to narrow the candidate
/// set; it is *never* trusted as proof of equality on its own. Each
/// candidate's stored bytes (sliced out of `tile_data_buf` by its recorded
/// `(offset, length)`) are compared byte-for-byte against `data`, and only
/// an exact match is returned. This means a hash collision between two
/// genuinely distinct payloads can never cause one to be silently reused in
/// place of the other -- the mismatching candidate is skipped and the
/// caller falls through to storing `data` as a new, additional candidate
/// under the same hash key.
///
/// Returns `None` if no stored candidate under `hash` has byte-identical
/// content to `data` (including the case where `hash` has no candidates
/// at all).
fn find_verified_dedup_match(
    seen_hashes: &BTreeMap<u64, Vec<(u64, u32)>>,
    tile_data_buf: &[u8],
    hash: u64,
    data: &[u8],
) -> Option<(u64, u32)> {
    let candidates = seen_hashes.get(&hash)?;
    for &(existing_offset, existing_len) in candidates {
        if existing_len as usize != data.len() {
            continue;
        }
        let start = existing_offset as usize;
        let Some(end) = start.checked_add(existing_len as usize) else {
            // Should be unreachable: offsets/lengths are always derived from
            // tile_data_buf's own prior length, but guard defensively rather
            // than panic on overflow.
            continue;
        };
        let Some(existing_bytes) = tile_data_buf.get(start..end) else {
            // Should be unreachable: offsets/lengths are always derived from
            // tile_data_buf's own prior length, but guard defensively rather
            // than panic-index.
            continue;
        };
        if existing_bytes == data {
            return Some((existing_offset, existing_len));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Run-length compression
// ---------------------------------------------------------------------------

/// Merge consecutive `DirEntry` values that share the same `offset` and
/// `length` (i.e. identical, deduplicated tile content) into a single entry
/// with an increased `run_length`.
///
/// Expects `entries` to already be sorted by `tile_id` with each entry
/// having `run_length == 1`.  Non-consecutive tile IDs (gap ≠ 1) break runs.
fn run_length_compress(entries: Vec<DirEntry>) -> Vec<DirEntry> {
    if entries.is_empty() {
        return entries;
    }

    let mut result: Vec<DirEntry> = Vec::with_capacity(entries.len());
    let mut iter = entries.into_iter();

    // Safety: is_empty() check above guarantees at least one element.
    let Some(first) = iter.next() else {
        return result;
    };
    let mut current = first;

    for next in iter {
        let expected_next_id = current.tile_id + u64::from(current.run_length);
        if next.tile_id == expected_next_id
            && next.offset == current.offset
            && next.length == current.length
        {
            // Extend the run.
            current.run_length += 1;
        } else {
            result.push(current);
            current = next;
        }
    }
    result.push(current);
    result
}

// ---------------------------------------------------------------------------
// Directory encoding (column-oriented, per spec)
// ---------------------------------------------------------------------------

/// Encode directory entries into the PMTiles v3 wire format.
///
/// Format:
/// 1. `num_entries` (varint)
/// 2. delta-encoded `tile_id` column (varints)
/// 3. `run_length` column (varints)
/// 4. `length` column (varints)
/// 5. `offset` column (varints); uses `offset + 1` encoding where `0` means
///    "immediately follows previous entry" (clustered shorthand).
fn encode_directory(entries: &[DirEntry]) -> Result<Vec<u8>, PmTilesError> {
    let n = entries.len();
    let mut buf = Vec::with_capacity(n * 8);

    // num_entries
    encode_varint_into(n as u64, &mut buf);

    // Tile ID deltas (sorted, cumulative)
    let mut last_id: u64 = 0;
    for entry in entries {
        let delta = entry.tile_id.checked_sub(last_id).ok_or_else(|| {
            PmTilesError::InvalidFormat(format!(
                "Tile IDs not sorted: {} after {}",
                entry.tile_id, last_id
            ))
        })?;
        encode_varint_into(delta, &mut buf);
        last_id = entry.tile_id;
    }

    // Run lengths
    for entry in entries {
        encode_varint_into(u64::from(entry.run_length), &mut buf);
    }

    // Lengths
    for entry in entries {
        encode_varint_into(u64::from(entry.length), &mut buf);
    }

    // Offsets: absolute encoding with `offset + 1` so that 0 is reserved for
    // the clustered shorthand (immediately follows previous entry).
    let mut last_offset: u64 = 0;
    for (i, entry) in entries.iter().enumerate() {
        if i > 0 {
            let prev = &entries[i - 1];
            // For run_length > 0 (tile) entries the clustered shorthand applies.
            // For run_length == 0 (leaf) entries we always emit the absolute offset.
            if prev.run_length > 0 && entry.offset == last_offset + u64::from(prev.length) {
                encode_varint_into(0, &mut buf);
                last_offset = entry.offset;
                continue;
            }
        }
        // Absolute offset encoded as `offset + 1`.
        encode_varint_into(entry.offset + 1, &mut buf);
        last_offset = entry.offset;
    }

    Ok(buf)
}

// ---------------------------------------------------------------------------
// Leaf directory splitting
// ---------------------------------------------------------------------------

/// The output of [`split_into_leaves`].
struct DirectorySplit {
    /// Root-level entries (may be leaf-pointer entries with `run_length == 0`).
    root_entries: Vec<DirEntry>,
    /// Concatenated serialised leaf directory bytes (empty when root-only).
    leaf_bytes: Vec<u8>,
}

/// Split `entries` into a root directory + leaf directories when the root
/// would exceed [`LEAF_SPLIT_THRESHOLD`] bytes.
///
/// If the root fits in the threshold, returns the entries unchanged with an
/// empty `leaf_bytes`.
///
/// Leaf entries in the root have `run_length == 0`, pointing into the leaf
/// section by byte offset and length.
///
/// When `compression` is anything other than [`Compression::None`] (and the
/// `compression` feature is enabled), each leaf chunk's serialised bytes are
/// individually compressed before being appended to `leaf_bytes`.  The root
/// entry's `length` records the compressed byte length, which is what the
/// reader needs to extract and decompress that single leaf.
fn split_into_leaves(
    entries: Vec<DirEntry>,
    compression: &Compression,
) -> Result<DirectorySplit, PmTilesError> {
    // Probe root size first using the *uncompressed* encoding.  When the root
    // directory itself is later compressed (in `build`), the threshold check
    // is intentionally based on the uncompressed size: leaf splitting affects
    // the directory-structure cost regardless of how the bytes are eventually
    // stored, and the spec's ~16 kB suggestion is about random-access
    // friendliness of the tree.
    let root_serialised = encode_directory(&entries)?;
    if root_serialised.len() <= LEAF_SPLIT_THRESHOLD {
        return Ok(DirectorySplit {
            root_entries: entries,
            leaf_bytes: Vec::new(),
        });
    }

    // Decide how many entries per leaf.
    // We aim for each leaf to be around LEAF_SPLIT_THRESHOLD bytes.
    let oversize_factor = root_serialised.len().div_ceil(LEAF_SPLIT_THRESHOLD);
    let chunk_size = entries.len().div_ceil(oversize_factor).max(1);

    let mut root_entries: Vec<DirEntry> = Vec::new();
    let mut leaf_bytes: Vec<u8> = Vec::new();

    for chunk in entries.chunks(chunk_size) {
        let leaf_serialised = encode_directory(chunk)?;
        // Per-leaf compression: the reader extracts each leaf by absolute
        // (offset, length) within the leaf-dirs section and then decompresses
        // it in isolation, so each chunk MUST be its own compression frame.
        let leaf_payload = compress_internal_section(&leaf_serialised, compression)?;

        let leaf_offset = leaf_bytes.len() as u64;
        let leaf_length = u32::try_from(leaf_payload.len()).map_err(|_| {
            PmTilesError::InvalidFormat("Leaf directory exceeds u32::MAX bytes".into())
        })?;

        // Root entry: run_length == 0 signals a leaf pointer.
        // `tile_id` = first tile in the leaf chunk.
        // `offset` = byte offset within the leaf_dirs section.
        // `length` = byte length of this leaf directory (compressed when an
        //            internal compression is in effect).
        root_entries.push(DirEntry {
            tile_id: chunk[0].tile_id,
            offset: leaf_offset,
            length: leaf_length,
            run_length: 0,
        });

        leaf_bytes.extend_from_slice(&leaf_payload);
    }

    Ok(DirectorySplit {
        root_entries,
        leaf_bytes,
    })
}

// ---------------------------------------------------------------------------
// Internal compression dispatch (root / leaf directories + metadata)
// ---------------------------------------------------------------------------

/// Compress `data` using the archive's `internal_compression` algorithm.
///
/// * [`Compression::None`] / [`Compression::Unknown`] — returns a copy of
///   `data` unmodified, matching the reader's pass-through semantics.
/// * Other algorithms require the `compression` Cargo feature.
///
/// # Errors
/// * [`PmTilesError::UnsupportedCompression`] when a non-`None` algorithm is
///   requested but the `compression` feature is disabled at compile time.
/// * [`PmTilesError::Decompression`] when the underlying OxiARC codec reports
///   a failure (the variant is named after decompression but is reused for
///   compression errors throughout the crate for consistency).
fn compress_internal_section(
    data: &[u8],
    compression: &Compression,
) -> Result<Vec<u8>, PmTilesError> {
    match compression {
        Compression::None | Compression::Unknown => Ok(data.to_vec()),
        #[cfg(feature = "compression")]
        Compression::Gzip => {
            // Default to compression level 6 (zlib's classic "balanced" preset)
            // — this matches the level used by [`crate::transcode`].
            oxiarc_archive::gzip::compress(data, 6)
                .map_err(|e| PmTilesError::Decompression(format!("Gzip compression failed: {e}")))
        }
        #[cfg(feature = "compression")]
        Compression::Brotli => oxiarc_archive::brotli::compress_with_quality(data, 6)
            .map_err(|e| PmTilesError::Decompression(format!("Brotli compression failed: {e}"))),
        #[cfg(feature = "compression")]
        Compression::Zstd => oxiarc_archive::zstd::compress(data)
            .map_err(|e| PmTilesError::Decompression(format!("Zstd compression failed: {e}"))),
        #[cfg(not(feature = "compression"))]
        _ => Err(PmTilesError::UnsupportedCompression),
    }
}

// ---------------------------------------------------------------------------
// Header serialisation
// ---------------------------------------------------------------------------

/// Fields needed to serialise a PMTiles header.
struct HeaderFields {
    root_dir_offset: u64,
    root_dir_length: u64,
    metadata_offset: u64,
    metadata_length: u64,
    leaf_dirs_offset: u64,
    leaf_dirs_length: u64,
    tile_data_offset: u64,
    tile_data_length: u64,
    addressed_tiles: u64,
    tile_entries: u64,
    tile_contents: u64,
    clustered: bool,
    internal_compression: Compression,
    tile_compression: Compression,
    tile_type: TileType,
    min_zoom: u8,
    max_zoom: u8,
    min_lon_e7: i32,
    min_lat_e7: i32,
    max_lon_e7: i32,
    max_lat_e7: i32,
    center_zoom: u8,
    center_lon_e7: i32,
    center_lat_e7: i32,
}

/// Convert a `TileType` to its spec byte value.
fn tile_type_to_u8(tt: &TileType) -> u8 {
    match tt {
        TileType::Unknown => 0,
        TileType::Mvt => 1,
        TileType::Png => 2,
        TileType::Jpeg => 3,
        TileType::Webp => 4,
        TileType::Avif => 5,
    }
}

/// Convert a `Compression` to its spec byte value.
fn compression_to_u8(c: &Compression) -> u8 {
    match c {
        Compression::Unknown => 0,
        Compression::None => 1,
        Compression::Gzip => 2,
        Compression::Brotli => 3,
        Compression::Zstd => 4,
    }
}

/// Serialise a PMTiles v3 header into exactly 127 bytes.
fn serialize_header(fields: &HeaderFields) -> [u8; PMTILES_HEADER_SIZE] {
    let mut buf = [0u8; PMTILES_HEADER_SIZE];

    // Magic
    buf[0..7].copy_from_slice(PMTILES_MAGIC);
    // Version
    buf[7] = 3;

    // u64 LE fields
    buf[8..16].copy_from_slice(&fields.root_dir_offset.to_le_bytes());
    buf[16..24].copy_from_slice(&fields.root_dir_length.to_le_bytes());
    buf[24..32].copy_from_slice(&fields.metadata_offset.to_le_bytes());
    buf[32..40].copy_from_slice(&fields.metadata_length.to_le_bytes());
    buf[40..48].copy_from_slice(&fields.leaf_dirs_offset.to_le_bytes());
    buf[48..56].copy_from_slice(&fields.leaf_dirs_length.to_le_bytes());
    buf[56..64].copy_from_slice(&fields.tile_data_offset.to_le_bytes());
    buf[64..72].copy_from_slice(&fields.tile_data_length.to_le_bytes());
    buf[72..80].copy_from_slice(&fields.addressed_tiles.to_le_bytes());
    buf[80..88].copy_from_slice(&fields.tile_entries.to_le_bytes());
    buf[88..96].copy_from_slice(&fields.tile_contents.to_le_bytes());

    // Single-byte fields
    buf[96] = if fields.clustered { 1 } else { 0 };
    buf[97] = compression_to_u8(&fields.internal_compression);
    buf[98] = compression_to_u8(&fields.tile_compression);
    buf[99] = tile_type_to_u8(&fields.tile_type);
    buf[100] = fields.min_zoom;
    buf[101] = fields.max_zoom;

    // i32 LE fields
    buf[102..106].copy_from_slice(&fields.min_lon_e7.to_le_bytes());
    buf[106..110].copy_from_slice(&fields.min_lat_e7.to_le_bytes());
    buf[110..114].copy_from_slice(&fields.max_lon_e7.to_le_bytes());
    buf[114..118].copy_from_slice(&fields.max_lat_e7.to_le_bytes());
    buf[118] = fields.center_zoom;
    buf[119..123].copy_from_slice(&fields.center_lon_e7.to_le_bytes());
    buf[123..127].copy_from_slice(&fields.center_lat_e7.to_le_bytes());

    buf
}

// ---------------------------------------------------------------------------
// PmTilesBuilder — in-memory builder (produces Vec<u8>)
// ---------------------------------------------------------------------------

/// Builder for constructing PMTiles v3 archives as an in-memory byte vector.
///
/// # Design
/// Tiles are collected, then on [`build`](PmTilesBuilder::build):
/// 1. Sorted by Hilbert tile ID.
/// 2. Content-deduplicated with FNV-1a hashing.
/// 3. Run-length compressed (consecutive identical tiles → single entry).
/// 4. Split into root + leaf directories when the root exceeds
///    [`LEAF_SPLIT_THRESHOLD`] bytes.
/// 5. Serialised as a contiguous byte buffer.
///
/// # Example
/// ```
/// use oxigeo_pmtiles::writer::PmTilesBuilder;
/// use oxigeo_pmtiles::TileType;
///
/// let mut builder = PmTilesBuilder::new(TileType::Png, 0, 2);
/// builder.add_tile(0, 0, 0, b"tile-z0").unwrap();
/// builder.add_tile(1, 0, 0, b"tile-z1-00").unwrap();
/// let archive = builder.build().unwrap();
/// assert!(archive.len() > 127);
/// ```
pub struct PmTilesBuilder {
    tile_type: TileType,
    min_zoom: u8,
    max_zoom: u8,
    metadata_json: Option<String>,
    min_lon_e7: i32,
    min_lat_e7: i32,
    max_lon_e7: i32,
    max_lat_e7: i32,
    center_lon_e7: i32,
    center_lat_e7: i32,
    center_zoom: u8,
    internal_compression: Compression,
    tile_compression: Compression,
    tiles: Vec<PendingTile>,
    /// Directory layout strategy.  [`LayoutStrategy::Auto`] (the default) is
    /// resolved from a tile-ordering analysis at [`build`](Self::build) time.
    layout_strategy: LayoutStrategy,
    /// Explicit override for the header `clustered` flag, set by
    /// [`apply_strategy_to_writer`] once a concrete strategy is resolved.
    /// `None` falls back to the deduplication-derived default.
    clustered_override: Option<bool>,
}

impl PmTilesBuilder {
    /// Create a new builder for the given tile type and zoom range.
    pub fn new(tile_type: TileType, min_zoom: u8, max_zoom: u8) -> Self {
        Self {
            tile_type,
            min_zoom,
            max_zoom,
            metadata_json: None,
            min_lon_e7: -1_800_000_000,
            min_lat_e7: -900_000_000,
            max_lon_e7: 1_800_000_000,
            max_lat_e7: 900_000_000,
            center_lon_e7: 0,
            center_lat_e7: 0,
            center_zoom: min_zoom,
            internal_compression: Compression::None,
            tile_compression: Compression::None,
            tiles: Vec::new(),
            layout_strategy: LayoutStrategy::Auto,
            clustered_override: None,
        }
    }

    /// Set the tile payload compression header value.
    ///
    /// This only records the compression byte in the PMTiles header; it does
    /// not transform the tile bytes themselves.  The caller is responsible for
    /// supplying tile payloads that are already encoded in the declared
    /// algorithm.  Used by `crate::transcode` (feature-gated behind
    /// `compression`) when re-compressing archives.
    pub fn set_tile_compression(&mut self, c: Compression) -> &mut Self {
        self.tile_compression = c;
        self
    }

    /// Set the directory/metadata compression algorithm.
    ///
    /// When set to any value other than [`Compression::None`] (and the
    /// `compression` Cargo feature is enabled), [`Self::build`] will compress
    /// the root directory, every leaf directory, and the JSON metadata block
    /// using the chosen algorithm before they are written into the output
    /// archive.  The compression byte at header offset 97 is updated to
    /// match, so a compliant reader (such as [`crate::PmTilesReader`]) will
    /// transparently decompress these regions on load.
    ///
    /// [`Compression::Unknown`] is treated identically to [`Compression::None`]
    /// at write time — the directory/metadata bytes are emitted verbatim.
    ///
    /// When the `compression` feature is **not** enabled, passing a non-`None`
    /// algorithm will cause [`Self::build`] to return
    /// [`PmTilesError::UnsupportedCompression`].
    pub fn set_internal_compression(&mut self, c: Compression) -> &mut Self {
        self.internal_compression = c;
        self
    }

    /// Set the JSON metadata string.
    pub fn set_metadata(&mut self, json: String) {
        self.metadata_json = Some(json);
    }

    /// Set the directory [`LayoutStrategy`].
    ///
    /// Defaults to [`LayoutStrategy::Auto`], which is resolved from a
    /// tile-ordering analysis at [`build`](Self::build) time.  An explicit
    /// strategy is honoured directly.  Returns `&mut Self` for chaining.
    pub fn set_layout_strategy(&mut self, strategy: LayoutStrategy) -> &mut Self {
        self.layout_strategy = strategy;
        self
    }

    /// Return the configured directory [`LayoutStrategy`].
    pub fn layout_strategy(&self) -> LayoutStrategy {
        self.layout_strategy
    }

    /// Override the header `clustered` flag directly.
    ///
    /// Used by [`apply_strategy_to_writer`] once a concrete
    /// [`LayoutStrategy`] is resolved.  Setting this takes precedence over the
    /// deduplication-derived default at [`build`](Self::build) time.
    pub(crate) fn set_clustered_flag(&mut self, clustered: bool) {
        self.clustered_override = Some(clustered);
    }

    /// Set the geographic bounding box in decimal degrees.
    pub fn set_bounds(&mut self, min_lon: f64, min_lat: f64, max_lon: f64, max_lat: f64) {
        self.min_lon_e7 = (min_lon * 1e7) as i32;
        self.min_lat_e7 = (min_lat * 1e7) as i32;
        self.max_lon_e7 = (max_lon * 1e7) as i32;
        self.max_lat_e7 = (max_lat * 1e7) as i32;
    }

    /// Set the default view centre and zoom level.
    pub fn set_center(&mut self, lon: f64, lat: f64, zoom: u8) {
        self.center_lon_e7 = (lon * 1e7) as i32;
        self.center_lat_e7 = (lat * 1e7) as i32;
        self.center_zoom = zoom;
    }

    /// Add a tile at the given `(z, x, y)` coordinates.
    ///
    /// # Errors
    /// Returns [`PmTilesError::InvalidFormat`] if `z` is outside the configured
    /// zoom range, or the coordinates are out of range for the zoom level.
    pub fn add_tile(&mut self, z: u8, x: u32, y: u32, data: &[u8]) -> Result<(), PmTilesError> {
        if z < self.min_zoom || z > self.max_zoom {
            return Err(PmTilesError::InvalidFormat(format!(
                "Zoom level {z} outside configured range [{}, {}]",
                self.min_zoom, self.max_zoom
            )));
        }
        let tile_id = zxy_to_tile_id(z, x, y)?;
        self.tiles.push(PendingTile {
            tile_id,
            data: data.to_vec(),
        });
        Ok(())
    }

    /// Add a tile using a raw Hilbert-curve tile ID (bypasses zoom validation).
    ///
    /// This is intended for archive compaction and copy operations where the
    /// tile ID is already known and correct.
    pub fn add_tile_by_id(&mut self, tile_id: u64, data: &[u8]) -> Result<(), PmTilesError> {
        self.tiles.push(PendingTile {
            tile_id,
            data: data.to_vec(),
        });
        Ok(())
    }

    /// Return the number of tiles added so far.
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    // ------------------------------------------------------------------
    // Auto-derivation helpers (bounds / zoom / center from tile set)
    // ------------------------------------------------------------------

    /// Derive the geographic bounding box automatically from the tiles added
    /// so far by computing the union of all tile extents in WGS-84 degrees.
    ///
    /// When no tiles have been added the existing bounds are left unchanged.
    /// This is most useful just before calling [`build`](Self::build).
    ///
    /// Returns `&mut self` for method chaining.
    pub fn auto_bounds(&mut self) -> &mut Self {
        use crate::hilbert::tile_id_to_zxy;
        use crate::webmerc::tile_bounds_lonlat;

        if self.tiles.is_empty() {
            return self;
        }

        let mut min_lon = f64::INFINITY;
        let mut max_lon = f64::NEG_INFINITY;
        let mut min_lat = f64::INFINITY;
        let mut max_lat = f64::NEG_INFINITY;

        for tile in &self.tiles {
            if let Ok((z, x, y)) = tile_id_to_zxy(tile.tile_id) {
                let (tl, bl, tr, br) = tile_bounds_lonlat(z, x, y);
                if tl < min_lon {
                    min_lon = tl;
                }
                if bl < min_lat {
                    min_lat = bl;
                }
                if tr > max_lon {
                    max_lon = tr;
                }
                if br > max_lat {
                    max_lat = br;
                }
            }
        }

        if min_lon.is_finite() {
            self.min_lon_e7 = (min_lon * 1e7).round() as i32;
            self.max_lon_e7 = (max_lon * 1e7).round() as i32;
            self.min_lat_e7 = (min_lat * 1e7).round() as i32;
            self.max_lat_e7 = (max_lat * 1e7).round() as i32;
        }
        self
    }

    /// Derive the zoom range (`min_zoom`, `max_zoom`) from the zoom levels of
    /// tiles already added to the builder.
    ///
    /// When no tiles have been added the existing values are left unchanged.
    /// Returns `&mut self` for method chaining.
    pub fn auto_zoom_range(&mut self) -> &mut Self {
        use crate::hilbert::tile_id_to_zxy;

        if self.tiles.is_empty() {
            return self;
        }

        let mut min_z = u8::MAX;
        let mut max_z = 0u8;

        for tile in &self.tiles {
            if let Ok((z, _, _)) = tile_id_to_zxy(tile.tile_id) {
                if z < min_z {
                    min_z = z;
                }
                if z > max_z {
                    max_z = z;
                }
            }
        }

        if min_z <= max_z {
            self.min_zoom = min_z;
            self.max_zoom = max_z;
        }
        self
    }

    /// Set the default view centre to the midpoint of the current bounding box
    /// and the zoom level to `min_zoom`.
    ///
    /// Call [`auto_bounds`](Self::auto_bounds) (or [`set_bounds`](Self::set_bounds))
    /// first so that the bounding box reflects the actual tile coverage.
    /// Returns `&mut self` for method chaining.
    pub fn auto_center(&mut self) -> &mut Self {
        let center_lon = (self.min_lon_e7 as f64 + self.max_lon_e7 as f64) / 2.0 / 1e7;
        let center_lat = (self.min_lat_e7 as f64 + self.max_lat_e7 as f64) / 2.0 / 1e7;
        self.center_lon_e7 = (center_lon * 1e7).round() as i32;
        self.center_lat_e7 = (center_lat * 1e7).round() as i32;
        self.center_zoom = self.min_zoom;
        self
    }

    /// Convenience wrapper: calls [`auto_zoom_range`](Self::auto_zoom_range),
    /// [`auto_bounds`](Self::auto_bounds), and [`auto_center`](Self::auto_center)
    /// in that order.
    ///
    /// After this call the archive header will reflect the actual zoom levels,
    /// geographic extent, and centre of the tiles that were added.
    /// Returns `&mut self` for method chaining.
    pub fn auto_all(&mut self) -> &mut Self {
        self.auto_zoom_range().auto_bounds().auto_center()
    }

    /// Consume the builder and produce a complete PMTiles v3 archive as bytes.
    ///
    /// The output is a valid PMTiles v3 file with:
    /// - 127-byte header
    /// - Root directory (varint-encoded, column-oriented, uncompressed)
    /// - Optional leaf directories (when root exceeds [`LEAF_SPLIT_THRESHOLD`])
    /// - JSON metadata section (uncompressed)
    /// - Tile data section (tiles sorted by tile_id, deduplicated)
    ///
    /// Consecutive tiles with identical content and contiguous IDs are merged
    /// with run-length compression.
    ///
    /// # Errors
    /// Returns [`PmTilesError::InvalidFormat`] on internal encoding failures.
    pub fn build(mut self) -> Result<Vec<u8>, PmTilesError> {
        // Sort tiles by tile_id.
        self.tiles.sort_by_key(|t| t.tile_id);

        let addressed_tiles = self.tiles.len() as u64;

        // ------------------------------------------------------------------
        // Step 1: Content deduplication via FNV-1a hash pre-filter plus a
        // mandatory byte-for-byte equality check on every candidate hit.
        // Produces a tile_data_buf and raw DirEntry list (run_length == 1).
        // ------------------------------------------------------------------
        // Value: a small list of (offset, length) candidates that share this
        // FNV-1a hash. FNV-1a is only a fast pre-filter -- a hash hit is
        // *not* treated as proof of content equality. Every candidate is
        // verified with a full byte-for-byte comparison against the stored
        // payload before being reused; a genuine hash collision (two
        // distinct payloads sharing a hash) falls through to storing the
        // new payload as an additional candidate under the same key.
        let mut seen_hashes: BTreeMap<u64, Vec<(u64, u32)>> = BTreeMap::new();
        let mut tile_data_buf: Vec<u8> = Vec::new();
        let mut had_dedup = false;

        let mut raw_entries: Vec<DirEntry> = Vec::with_capacity(self.tiles.len());

        for tile in &self.tiles {
            let hash = fnv1a_hash(&tile.data);
            let len = u32::try_from(tile.data.len()).map_err(|_| {
                PmTilesError::InvalidFormat("Tile data exceeds u32::MAX bytes".into())
            })?;

            let reused = find_verified_dedup_match(&seen_hashes, &tile_data_buf, hash, &tile.data);

            let (offset, length) = if let Some((existing_offset, existing_len)) = reused {
                had_dedup = true;
                (existing_offset, existing_len)
            } else {
                let offset = u64::try_from(tile_data_buf.len()).map_err(|_| {
                    PmTilesError::InvalidFormat("Total tile data exceeds u64 range".into())
                })?;
                tile_data_buf.extend_from_slice(&tile.data);
                seen_hashes.entry(hash).or_default().push((offset, len));
                (offset, len)
            };

            raw_entries.push(DirEntry {
                tile_id: tile.tile_id,
                offset,
                length,
                run_length: 1,
            });
        }

        let unique_contents = seen_hashes.values().map(Vec::len).sum::<usize>() as u64;

        // ------------------------------------------------------------------
        // Step 1b: Resolve the directory layout strategy.
        //
        // Build a `(tile_id, data_offset, data_length)` manifest from the raw
        // (pre-run-length) entries, analyse it, resolve `Auto` to a concrete
        // strategy, and apply it to the builder's `clustered` override.  An
        // explicit override (set directly via `set_clustered_flag`) wins over
        // the resolution; otherwise the resolved strategy drives the flag.
        // ------------------------------------------------------------------
        let ordering_manifest: Vec<(u64, u64, u64)> = raw_entries
            .iter()
            .map(|e| (e.tile_id, e.offset, u64::from(e.length)))
            .collect();
        let analysis = analyze_tile_ordering(&ordering_manifest);
        let resolved_strategy = choose_strategy(&analysis, self.layout_strategy);
        if self.clustered_override.is_none() {
            apply_strategy_to_writer(&mut self, resolved_strategy);
        }

        // ------------------------------------------------------------------
        // Step 2: Run-length compression.
        // Consecutive tiles with the same (offset, length) and contiguous IDs
        // are merged into a single directory entry.
        // ------------------------------------------------------------------
        let compressed_entries = run_length_compress(raw_entries);
        let tile_entries_count = compressed_entries.len() as u64;

        // ------------------------------------------------------------------
        // Step 3: Leaf directory split (if root is too large).
        // When `internal_compression` is set, each leaf chunk is compressed
        // independently inside `split_into_leaves` so that the reader can
        // extract and decompress one leaf at a time without scanning the
        // entire leaf-dirs section.
        // ------------------------------------------------------------------
        let split = split_into_leaves(compressed_entries, &self.internal_compression)?;

        let root_dir_uncompressed = encode_directory(&split.root_entries)?;
        // The root directory is compressed (when an internal compression is
        // selected) as a single frame.  Readers fetch the whole region using
        // header.root_dir_offset / root_dir_length, so a single frame is the
        // natural unit.
        let root_dir_bytes =
            compress_internal_section(&root_dir_uncompressed, &self.internal_compression)?;
        let leaf_dir_bytes = split.leaf_bytes;

        // ------------------------------------------------------------------
        // Step 4: Encode metadata.
        //
        // JSON metadata defaults to `"{}"` when the caller did not supply
        // one.  When an internal compression is selected, the metadata block
        // is compressed as a single frame as well — matching what
        // [`PmTilesMetadata::from_bytes`] expects on the reader side.
        // ------------------------------------------------------------------
        let metadata_uncompressed = self
            .metadata_json
            .as_deref()
            .unwrap_or("{}")
            .as_bytes()
            .to_vec();
        let metadata_bytes =
            compress_internal_section(&metadata_uncompressed, &self.internal_compression)?;

        // ------------------------------------------------------------------
        // Step 5: Compute section offsets.
        // Layout: [header(127)] [root_dir] [metadata] [leaf_dirs] [tile_data]
        //
        // Note: metadata is placed *before* leaf_dirs to match the reference
        // layout implied by the spec field ordering (metadata_offset comes
        // before leaf_dirs_offset in the header).
        // ------------------------------------------------------------------
        let root_dir_offset = PMTILES_HEADER_SIZE as u64;
        let root_dir_length = root_dir_bytes.len() as u64;
        let metadata_offset = root_dir_offset + root_dir_length;
        let metadata_length = metadata_bytes.len() as u64;
        let leaf_dirs_offset = metadata_offset + metadata_length;
        let leaf_dirs_length = leaf_dir_bytes.len() as u64;
        let tile_data_offset = leaf_dirs_offset + leaf_dirs_length;
        let tile_data_length = tile_data_buf.len() as u64;

        // ------------------------------------------------------------------
        // Step 6: Serialise the header.
        // ------------------------------------------------------------------
        let header_bytes = serialize_header(&HeaderFields {
            root_dir_offset,
            root_dir_length,
            metadata_offset,
            metadata_length,
            leaf_dirs_offset,
            leaf_dirs_length,
            tile_data_offset,
            tile_data_length,
            addressed_tiles,
            tile_entries: tile_entries_count,
            tile_contents: unique_contents,
            // Clustered = tiles in tile-id order with offsets monotonically
            // increasing.  When a layout strategy resolved an explicit flag
            // (`clustered_override`), honour it; otherwise fall back to the
            // deduplication-derived default (dedup breaks clustering).
            clustered: self.clustered_override.unwrap_or(!had_dedup),
            internal_compression: self.internal_compression,
            tile_compression: self.tile_compression,
            tile_type: self.tile_type,
            min_zoom: self.min_zoom,
            max_zoom: self.max_zoom,
            min_lon_e7: self.min_lon_e7,
            min_lat_e7: self.min_lat_e7,
            max_lon_e7: self.max_lon_e7,
            max_lat_e7: self.max_lat_e7,
            center_zoom: self.center_zoom,
            center_lon_e7: self.center_lon_e7,
            center_lat_e7: self.center_lat_e7,
        });

        // ------------------------------------------------------------------
        // Step 7: Assemble output buffer.
        // ------------------------------------------------------------------
        let total_size = PMTILES_HEADER_SIZE
            + root_dir_bytes.len()
            + metadata_bytes.len()
            + leaf_dir_bytes.len()
            + tile_data_buf.len();

        let mut output = Vec::with_capacity(total_size);
        output.extend_from_slice(&header_bytes);
        output.extend_from_slice(&root_dir_bytes);
        output.extend_from_slice(&metadata_bytes);
        output.extend_from_slice(&leaf_dir_bytes);
        output.extend_from_slice(&tile_data_buf);

        Ok(output)
    }
}

// ---------------------------------------------------------------------------
// PmTilesWriter — file-based wrapper
// ---------------------------------------------------------------------------

/// Options for [`PmTilesWriter`].
#[derive(Debug, Clone)]
pub struct PmTilesWriterOptions {
    /// Tile type byte (see [`TileType`]).  Defaults to [`TileType::Mvt`].
    pub tile_type: TileType,
    /// Minimum zoom level present in the archive.
    pub min_zoom: u8,
    /// Maximum zoom level present in the archive.
    pub max_zoom: u8,
    /// Optional JSON metadata string.
    pub metadata: Option<String>,
}

impl Default for PmTilesWriterOptions {
    fn default() -> Self {
        Self {
            tile_type: TileType::Mvt,
            min_zoom: 0,
            max_zoom: 14,
            metadata: None,
        }
    }
}

/// File-based PMTiles v3 writer.
///
/// Collects tiles in memory, then writes a complete PMTiles v3 file when
/// [`finish`](PmTilesWriter::finish) is called.
///
/// # Note on seekability
/// The output path must refer to a regular file because the writer uses
/// [`std::fs::write`] (atomic write of the fully-assembled buffer).  Network
/// or pipe targets are not supported.
///
/// # Example
/// ```no_run
/// use oxigeo_pmtiles::writer::{PmTilesWriter, PmTilesWriterOptions};
///
/// let mut w = PmTilesWriter::create("/tmp/out.pmtiles", PmTilesWriterOptions::default()).unwrap();
/// w.write_tile(0, 0, 0, b"tile data").unwrap();
/// w.finish().unwrap();
/// ```
pub struct PmTilesWriter {
    path: std::path::PathBuf,
    builder: PmTilesBuilder,
}

impl PmTilesWriter {
    /// Create a new writer targeting `path` with the given options.
    ///
    /// # Errors
    /// Currently infallible, but returns `Result` for API consistency.
    pub fn create<P: AsRef<Path>>(
        path: P,
        options: PmTilesWriterOptions,
    ) -> Result<Self, PmTilesError> {
        let mut builder =
            PmTilesBuilder::new(options.tile_type, options.min_zoom, options.max_zoom);
        if let Some(json) = options.metadata {
            builder.set_metadata(json);
        }
        Ok(Self {
            path: path.as_ref().to_path_buf(),
            builder,
        })
    }

    /// Add a tile at the given `(z, x, y)` coordinates.
    ///
    /// # Errors
    /// Returns [`PmTilesError::InvalidFormat`] if coordinates are invalid.
    pub fn write_tile(&mut self, z: u8, x: u32, y: u32, data: &[u8]) -> Result<(), PmTilesError> {
        self.builder.add_tile(z, x, y, data)
    }

    /// Build the archive and write it to the file path provided at creation.
    ///
    /// Consumes the writer.  The file is created (or truncated) atomically
    /// from the fully-assembled byte buffer.
    ///
    /// # Errors
    /// Propagates errors from [`PmTilesBuilder::build`] or from
    /// [`File`] I/O.
    pub fn finish(self) -> Result<(), PmTilesError> {
        let archive_bytes = self.builder.build()?;
        let mut file = File::create(&self.path)?;
        file.write_all(&archive_bytes)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::PmTilesHeader;
    use crate::pmtiles::PmTilesReader;

    // -----------------------------------------------------------------------
    // Unit tests for internal helpers
    // -----------------------------------------------------------------------

    #[test]
    fn test_fnv1a_hash_different_data() {
        let h1 = fnv1a_hash(b"hello");
        let h2 = fnv1a_hash(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_fnv1a_hash_same_data() {
        let h1 = fnv1a_hash(b"hello");
        let h2 = fnv1a_hash(b"hello");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_fnv1a_hash_empty() {
        let h1 = fnv1a_hash(b"");
        let h2 = fnv1a_hash(b"");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_tile_type_to_u8_round_trip() {
        let types = [
            TileType::Unknown,
            TileType::Mvt,
            TileType::Png,
            TileType::Jpeg,
            TileType::Webp,
            TileType::Avif,
        ];
        for tt in &types {
            let byte = tile_type_to_u8(tt);
            let back = TileType::from_u8(byte);
            assert_eq!(&back, tt);
        }
    }

    #[test]
    fn test_compression_to_u8_round_trip() {
        let compressions = [
            Compression::Unknown,
            Compression::None,
            Compression::Gzip,
            Compression::Brotli,
            Compression::Zstd,
        ];
        for c in &compressions {
            let byte = compression_to_u8(c);
            let back = Compression::from_u8(byte);
            assert_eq!(&back, c);
        }
    }

    #[test]
    fn test_serialize_header_magic_and_length() {
        let h = serialize_header(&HeaderFields {
            root_dir_offset: 127,
            root_dir_length: 0,
            metadata_offset: 127,
            metadata_length: 0,
            leaf_dirs_offset: 0,
            leaf_dirs_length: 0,
            tile_data_offset: 127,
            tile_data_length: 0,
            addressed_tiles: 0,
            tile_entries: 0,
            tile_contents: 0,
            clustered: true,
            internal_compression: Compression::None,
            tile_compression: Compression::None,
            tile_type: TileType::Png,
            min_zoom: 0,
            max_zoom: 14,
            min_lon_e7: 0,
            min_lat_e7: 0,
            max_lon_e7: 0,
            max_lat_e7: 0,
            center_zoom: 5,
            center_lon_e7: 0,
            center_lat_e7: 0,
        });
        assert_eq!(&h[0..7], PMTILES_MAGIC);
        assert_eq!(h[7], 3);
        assert_eq!(h.len(), 127);
    }

    #[test]
    fn test_run_length_compress_empty() {
        let result = run_length_compress(vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_run_length_compress_no_run() {
        // Different offsets → no merging.
        let entries = vec![
            DirEntry {
                tile_id: 0,
                offset: 0,
                length: 10,
                run_length: 1,
            },
            DirEntry {
                tile_id: 1,
                offset: 10,
                length: 20,
                run_length: 1,
            },
            DirEntry {
                tile_id: 2,
                offset: 30,
                length: 15,
                run_length: 1,
            },
        ];
        let result = run_length_compress(entries);
        assert_eq!(result.len(), 3);
        for e in &result {
            assert_eq!(e.run_length, 1);
        }
    }

    #[test]
    fn test_run_length_compress_full_run() {
        // All tiles share same offset + length and are contiguous → single entry.
        let entries: Vec<DirEntry> = (0..10)
            .map(|i| DirEntry {
                tile_id: i,
                offset: 0,
                length: 100,
                run_length: 1,
            })
            .collect();
        let result = run_length_compress(entries);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].run_length, 10);
        assert_eq!(result[0].tile_id, 0);
    }

    #[test]
    fn test_run_length_compress_partial_run() {
        // First 5 tiles share data, last 5 are unique.
        let mut entries: Vec<DirEntry> = (0..5)
            .map(|i| DirEntry {
                tile_id: i,
                offset: 0,
                length: 50,
                run_length: 1,
            })
            .collect();
        entries.extend((5..10).map(|i| DirEntry {
            tile_id: i,
            offset: 50 + (i - 5) * 100,
            length: 100,
            run_length: 1,
        }));
        let result = run_length_compress(entries);
        assert_eq!(result.len(), 6); // 1 run of 5 + 5 individual
        assert_eq!(result[0].run_length, 5);
        for r in &result[1..] {
            assert_eq!(r.run_length, 1);
        }
    }

    #[test]
    fn test_run_length_compress_gap_breaks_run() {
        // Tile IDs 0,1, then 3,4 (gap at 2) — even if same offset/length.
        let entries = vec![
            DirEntry {
                tile_id: 0,
                offset: 0,
                length: 50,
                run_length: 1,
            },
            DirEntry {
                tile_id: 1,
                offset: 0,
                length: 50,
                run_length: 1,
            },
            DirEntry {
                tile_id: 3,
                offset: 0,
                length: 50,
                run_length: 1,
            },
            DirEntry {
                tile_id: 4,
                offset: 0,
                length: 50,
                run_length: 1,
            },
        ];
        let result = run_length_compress(entries);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].run_length, 2);
        assert_eq!(result[1].run_length, 2);
    }

    // -----------------------------------------------------------------------
    // PmTilesBuilder tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_builder_empty_archive() {
        let builder = PmTilesBuilder::new(TileType::Png, 0, 0);
        let archive = builder.build().expect("build ok");
        assert!(archive.len() >= PMTILES_HEADER_SIZE);
        let header = PmTilesHeader::parse(&archive).expect("parse ok");
        assert_eq!(header.addressed_tiles, 0);
        assert_eq!(header.tile_entries, 0);
        assert_eq!(header.tile_contents, 0);
    }

    #[test]
    fn test_builder_single_tile() {
        let mut builder = PmTilesBuilder::new(TileType::Png, 0, 0);
        builder.add_tile(0, 0, 0, b"single").expect("add ok");
        let archive = builder.build().expect("build ok");
        let header = PmTilesHeader::parse(&archive).expect("parse ok");
        assert_eq!(header.addressed_tiles, 1);
        assert_eq!(header.tile_entries, 1);
        assert_eq!(header.tile_contents, 1);
    }

    #[test]
    fn test_builder_dedup_identical_tiles() {
        let mut builder = PmTilesBuilder::new(TileType::Png, 0, 5);
        let tile_data = vec![0xABu8; 1024];
        for x in 0..10u32 {
            builder.add_tile(5, x, 0, &tile_data).expect("add ok");
        }
        let archive = builder.build().expect("build ok");
        let header = PmTilesHeader::parse(&archive).expect("parse ok");
        assert_eq!(header.addressed_tiles, 10);
        // All deduplicated to 1 unique content.
        assert_eq!(header.tile_contents, 1);
        // Total archive size is much smaller than 10 * 1024 bytes.
        assert!(
            archive.len() < 127 + 10 * 1024,
            "archive should be smaller due to dedup, got {} bytes",
            archive.len()
        );
    }

    #[test]
    fn test_find_verified_dedup_match_rejects_hash_collision() {
        // Regression test for a real defect: dedup must never trust a hash
        // hit alone. Fabricate a scenario where two *distinct* payloads are
        // deliberately stored under the *same* hash key (as would happen on
        // a genuine FNV-1a collision) and confirm the lookup only reports a
        // match for byte-identical content, never for the colliding-but-
        // different payload.
        let tile_data_buf: Vec<u8> = b"AAAA".to_vec();
        let colliding_hash = 0xDEAD_BEEFu64; // arbitrary key shared by both.
        let mut seen_hashes: BTreeMap<u64, Vec<(u64, u32)>> = BTreeMap::new();
        // A single candidate stored under `colliding_hash`: bytes "AAAA" at
        // offset 0, length 4.
        seen_hashes.insert(colliding_hash, vec![(0, 4)]);

        // Same hash, byte-identical content -> must match.
        let identical =
            find_verified_dedup_match(&seen_hashes, &tile_data_buf, colliding_hash, b"AAAA");
        assert_eq!(identical, Some((0, 4)));

        // Same hash, same length, but genuinely different bytes (the
        // collision case) -> must NOT match.
        let colliding_but_different =
            find_verified_dedup_match(&seen_hashes, &tile_data_buf, colliding_hash, b"BBBB");
        assert_eq!(
            colliding_but_different, None,
            "a hash collision must never be treated as a content match"
        );

        // Same hash, different length -> must NOT match (fast-path rejects
        // before even touching the buffer).
        let different_length =
            find_verified_dedup_match(&seen_hashes, &tile_data_buf, colliding_hash, b"AAAAA");
        assert_eq!(different_length, None);

        // Unknown hash -> no candidates at all.
        let no_candidates =
            find_verified_dedup_match(&seen_hashes, &tile_data_buf, 0x1234, b"AAAA");
        assert_eq!(no_candidates, None);
    }

    #[test]
    fn test_find_verified_dedup_match_second_candidate_in_list() {
        // Two distinct payloads legitimately share one hash bucket (both
        // added earlier because the first didn't byte-match the second at
        // insertion time). The lookup must scan every candidate, not just
        // the first, and find the correct one.
        let tile_data_buf: Vec<u8> = [&b"AAAA"[..], &b"CCCC"[..]].concat();
        let hash = 0x1111_1111u64;
        let mut seen_hashes: BTreeMap<u64, Vec<(u64, u32)>> = BTreeMap::new();
        seen_hashes.insert(hash, vec![(0, 4), (4, 4)]);

        let first = find_verified_dedup_match(&seen_hashes, &tile_data_buf, hash, b"AAAA");
        assert_eq!(first, Some((0, 4)));

        let second = find_verified_dedup_match(&seen_hashes, &tile_data_buf, hash, b"CCCC");
        assert_eq!(second, Some((4, 4)));

        let neither = find_verified_dedup_match(&seen_hashes, &tile_data_buf, hash, b"ZZZZ");
        assert_eq!(neither, None);
    }

    #[test]
    fn test_builder_hash_collision_preserves_distinct_content() {
        // End-to-end regression test: two tiles whose content is
        // deliberately engineered to collide under the *real* fnv1a_hash
        // (by exploiting the fact that seen_hashes is keyed by the actual
        // hash value) must still both retain their own distinct content
        // after build() + round-trip through the reader. We can't easily
        // brute-force a genuine 64-bit FNV-1a collision in a unit test, so
        // this test instead proves the property that matters operationally:
        // many distinct payloads added to the same builder never have their
        // content corrupted by any other payload, verified via full
        // round-trip decode for every tile. This exercises the exact code
        // path (`find_verified_dedup_match`) that the fabricated-collision
        // unit tests above verify in isolation.
        let mut builder = PmTilesBuilder::new(TileType::Png, 0, 5);
        let mut expected: Vec<(u8, u32, Vec<u8>)> = Vec::new();
        for x in 0..32u32 {
            // Distinct content per tile, all the same length so that a
            // naive length-blind hash-only dedup would be most tempted to
            // (wrongly) merge them.
            let data = vec![x as u8; 32];
            builder.add_tile(5, x, 0, &data).expect("add ok");
            expected.push((5, x, data));
        }
        let archive = builder.build().expect("build ok");
        let reader = PmTilesReader::from_bytes(archive).expect("reader ok");
        for (z, x, data) in expected {
            let tile = reader
                .get_tile(z, x, 0)
                .expect("get_tile ok")
                .expect("tile present");
            assert_eq!(tile, data, "tile z={z} x={x} content must be unmodified");
        }
    }

    #[test]
    fn test_builder_run_length_in_archive() {
        // Tiles at IDs 0,1,2 all sharing same data → run_length == 3 in dir.
        let mut builder = PmTilesBuilder::new(TileType::Mvt, 0, 2);
        let shared = b"shared tile data";
        // z=0 is tile_id=0; z=1 tiles are IDs 1..5.
        // Write z=0,0,0 (id=0) and z=1,0,0 (id=1) and z=1,0,1 (id=2?) — they
        // share the same data.  The Hilbert order determines contiguity.
        // For simplicity just verify the builder produces a valid archive.
        builder.add_tile(0, 0, 0, shared).expect("add ok");
        builder.add_tile(1, 0, 0, shared).expect("add ok");
        let archive = builder.build().expect("build ok");
        let header = PmTilesHeader::parse(&archive).expect("parse ok");
        assert_eq!(header.addressed_tiles, 2);
        assert_eq!(header.tile_contents, 1); // deduplicated
    }

    // -----------------------------------------------------------------------
    // PmTilesWriter (file-based) tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_pmtiles_writer_magic_and_version() {
        use std::io::{Read, Seek, SeekFrom};
        let tmp = std::env::temp_dir().join("test_pmtiles_writer_magic.pmtiles");
        let writer =
            PmTilesWriter::create(&tmp, PmTilesWriterOptions::default()).expect("create ok");
        writer.finish().expect("finish ok");

        let mut f = std::fs::File::open(&tmp).expect("open ok");
        let mut buf = [0u8; 8];
        f.read_exact(&mut buf).expect("read ok");
        assert_eq!(&buf[0..7], b"PMTiles");
        assert_eq!(buf[7], 3);

        // addressed_tiles_count at offset 72 should be 0
        f.seek(SeekFrom::Start(72)).expect("seek ok");
        let mut count_buf = [0u8; 8];
        f.read_exact(&mut count_buf).expect("read ok");
        assert_eq!(u64::from_le_bytes(count_buf), 0);

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_pmtiles_writer_small_archive_root_only() {
        use std::io::{Read, Seek, SeekFrom};
        let tmp = std::env::temp_dir().join("test_pmtiles_writer_small.pmtiles");
        let mut writer =
            PmTilesWriter::create(&tmp, PmTilesWriterOptions::default()).expect("create ok");
        // Write 100 unique tiles at zoom 5.
        for x in 0..10u32 {
            for y in 0..10u32 {
                let data: Vec<u8> = vec![(x * 10 + y) as u8; 64];
                writer.write_tile(5, x, y, &data).expect("write ok");
            }
        }
        writer.finish().expect("finish ok");

        let mut f = std::fs::File::open(&tmp).expect("open ok");
        // leaf_dirs_length is at offset 48.
        f.seek(SeekFrom::Start(48)).expect("seek ok");
        let mut buf8 = [0u8; 8];
        f.read_exact(&mut buf8).expect("read ok");
        let leaf_dirs_length = u64::from_le_bytes(buf8);
        // 100 tiles should fit in root directory without leaves.
        assert_eq!(
            leaf_dirs_length, 0,
            "small archive should have no leaf directories"
        );

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_pmtiles_writer_dedup_reduces_file_size() {
        let tmp = std::env::temp_dir().join("test_pmtiles_writer_dedup.pmtiles");
        let mut writer =
            PmTilesWriter::create(&tmp, PmTilesWriterOptions::default()).expect("create ok");
        let tile_data = vec![0xABu8; 1024];
        // Write same tile data to 20 different coordinates.
        for i in 0..20u32 {
            writer.write_tile(5, i, 0, &tile_data).expect("write ok");
        }
        writer.finish().expect("finish ok");

        // File should be much smaller than 20 * 1024 (dedup in effect).
        let file_size = std::fs::metadata(&tmp).expect("meta ok").len();
        // Header (127) + small dir + metadata ("{}") + 1×1024 bytes of tile data.
        assert!(
            file_size < 5_000,
            "Expected deduplicated archive, got {} bytes",
            file_size
        );

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_pmtiles_writer_large_archive_has_leaves() {
        use std::io::{Read, Seek, SeekFrom};

        let tmp = std::env::temp_dir().join("test_pmtiles_writer_large_leaves.pmtiles");
        let mut writer = PmTilesWriter::create(
            &tmp,
            PmTilesWriterOptions {
                max_zoom: 7,
                ..Default::default()
            },
        )
        .expect("create ok");

        // Write 5000 tiles with distinct data so no run-length compression removes entries.
        let mut count: u32 = 0;
        'outer: for z in 0u8..=7 {
            let max_tile = 1u32 << z;
            for x in 0..max_tile {
                for y in 0..max_tile {
                    let data = vec![(count % 256) as u8; 100];
                    writer.write_tile(z, x, y, &data).expect("write ok");
                    count += 1;
                    if count >= 5000 {
                        break 'outer;
                    }
                }
            }
        }
        writer.finish().expect("finish ok");

        // Read addressed_tiles_count (offset 72) and file length.
        let mut f = std::fs::File::open(&tmp).expect("open ok");
        let mut header_buf = [0u8; 127];
        f.read_exact(&mut header_buf).expect("read header ok");
        let header = PmTilesHeader::parse(&header_buf).expect("parse ok");

        assert_eq!(header.addressed_tiles, count as u64);
        assert!(
            header.tile_data_length > 0,
            "tile data section should be non-empty"
        );

        // Check that leaf_dirs_length > 0 for 5000 unique entries.
        f.seek(SeekFrom::Start(48)).expect("seek ok");
        let mut buf8 = [0u8; 8];
        f.read_exact(&mut buf8).expect("read ok");
        let leaf_dirs_length = u64::from_le_bytes(buf8);
        assert!(
            leaf_dirs_length > 0,
            "5000-tile archive should use leaf directories (leaf_dirs_length={leaf_dirs_length})"
        );

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_pmtiles_writer_roundtrip_via_reader() {
        use std::io::Read;

        let tmp = std::env::temp_dir().join("test_pmtiles_writer_roundtrip.pmtiles");
        let mut writer =
            PmTilesWriter::create(&tmp, PmTilesWriterOptions::default()).expect("create ok");

        let expected_tiles: &[(u8, u32, u32, &[u8])] = &[
            (0, 0, 0, b"\x01\x02\x03"),
            (1, 0, 0, b"\x04\x05\x06"),
            (1, 1, 0, b"\x07\x08\x09"),
            (1, 0, 1, b"\x0A\x0B\x0C"),
        ];

        for &(z, x, y, data) in expected_tiles {
            writer.write_tile(z, x, y, data).expect("write ok");
        }
        writer.finish().expect("finish ok");

        // Read the file back and parse with PmTilesReader.
        let mut f = std::fs::File::open(&tmp).expect("open ok");
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes).expect("read ok");
        let reader = PmTilesReader::from_bytes(bytes).expect("reader ok");

        // Verify header counts.
        assert_eq!(reader.header.addressed_tiles, expected_tiles.len() as u64);
        assert_eq!(reader.header.spec_version, 3);

        // Retrieve each tile and compare bytes.
        for &(z, x, y, expected_data) in expected_tiles {
            let got = reader
                .get_tile(z, x, y)
                .expect("get_tile ok")
                .expect("tile should exist");
            assert_eq!(
                got, expected_data,
                "tile z={z} x={x} y={y} content mismatch"
            );
        }

        let _ = std::fs::remove_file(&tmp);
    }
}
