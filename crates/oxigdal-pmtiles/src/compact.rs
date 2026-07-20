//! PMTiles v3 archive compaction.
//!
//! Compaction rebuilds a PMTiles archive from scratch, removing all gaps that
//! accumulate from deleted or replaced tiles.  The process reads every logical
//! tile from the source archive, re-packs the tile data into a fresh
//! [`PmTilesBuilder`], and re-encodes the directory.
//!
//! # Gap sources
//! - **Deleted tiles** — directory entries removed in a previous write pass
//!   leave their payload bytes unreachable in the data section.
//! - **Replaced tiles** — when a tile is overwritten, the old payload at the
//!   original offset is no longer referenced, fragmenting the data section.
//! - **Fragmented run-length entries** — large run-length blocks that are
//!   partially superseded leave gaps between the surviving logical tiles.
//!
//! # Algorithm
//! 1. Parse the source archive via [`PmTilesReader`].
//! 2. Call [`PmTilesReader::enumerate_tiles`] to obtain one [`TileInfo`] per
//!    logical tile in tile-ID order.
//! 3. For each tile, slice `bytes[tile_data_offset + data_offset .. + data_length]`
//!    to obtain the raw payload.
//! 4. When [`CompactOptions::deduplicate`] is `true`, track already-seen
//!    content via a `HashMap<Vec<u8>, u64>` (bytes → first tile_id) and note
//!    duplicates for statistics; the builder's own dedup logic (an FNV-1a
//!    hash pre-filter verified by a full byte-for-byte comparison) ensures
//!    the physical payload is shared in the output regardless, with no risk
//!    of a hash collision merging distinct content.
//! 5. Feed all tiles into a fresh [`PmTilesBuilder`] via `add_tile_by_id`.
//! 6. Propagate header fields (tile type, zoom, bounds, centre) from the source
//!    when [`CompactOptions::preserve_metadata`] is `true`.
//! 7. Call `builder.build()` to produce the compacted archive.

use std::collections::HashMap;

use crate::error::PmTilesError;
use crate::header::{PmTilesHeader, TileType};
use crate::pmtiles::{PmTilesReader, TileInfo};
use crate::writer::PmTilesBuilder;

// ---------------------------------------------------------------------------
// CompactOptions
// ---------------------------------------------------------------------------

/// Options controlling how [`compact_archive`] and
/// [`compact_archive_with_stats`] operate.
#[derive(Debug, Clone)]
pub struct CompactOptions {
    /// When `true`, tiles with identical byte content share a single physical
    /// payload in the output archive.  The builder already deduplicates
    /// (an FNV-1a hash pre-filter followed by a mandatory byte-for-byte
    /// comparison, so a hash collision can never merge distinct content);
    /// this flag additionally tracks duplicate dispatches so that
    /// [`CompactStats::tiles_deduplicated`] is accurate.
    ///
    /// When `false`, every tile is dispatched to the builder individually,
    /// but the builder still deduplicates by verified content equality
    /// unless disabled.
    ///
    /// Default: `true`.
    pub deduplicate: bool,

    /// When `true`, tiles are fed to the builder in ascending tile-ID order
    /// (Hilbert-curve order), matching the PMTiles v3 clustered layout
    /// recommendation.  [`PmTilesReader::enumerate_tiles`] already returns
    /// tiles sorted by tile ID, so enabling this flag is a no-op in practice;
    /// it exists to document *intent* and allow future optimisations.
    ///
    /// Default: `true`.
    pub sort_by_tile_id: bool,

    /// When `true`, the following header fields from the source archive are
    /// propagated to the output archive:
    /// - tile type
    /// - zoom range (`min_zoom`, `max_zoom`)
    /// - geographic bounding box (min/max lon/lat)
    /// - centre longitude, latitude, and zoom
    ///
    /// When `false`, the builder uses its own defaults (tile type `Unknown`,
    /// zoom range 0–0, world bounding box).
    ///
    /// Default: `true`.
    pub preserve_metadata: bool,
}

impl Default for CompactOptions {
    fn default() -> Self {
        Self {
            deduplicate: true,
            sort_by_tile_id: true,
            preserve_metadata: true,
        }
    }
}

// ---------------------------------------------------------------------------
// CompactStats
// ---------------------------------------------------------------------------

/// Quantitative summary of a single compaction run.
#[derive(Debug, Clone)]
pub struct CompactStats {
    /// Number of logical tiles read from the source archive (after run-length
    /// expansion).  Equals [`PmTilesReader::enumerate_tiles`] length.
    pub tiles_read: usize,

    /// Number of tiles dispatched to the builder.  When deduplication is
    /// enabled, tiles whose content was already seen are still dispatched
    /// (the builder deduplicates the physical payload) but are counted under
    /// `tiles_deduplicated`.
    pub tiles_written: usize,

    /// Number of tiles whose byte content was identical to a previously-seen
    /// tile.  Only meaningful when [`CompactOptions::deduplicate`] is `true`;
    /// otherwise always `0`.
    pub tiles_deduplicated: usize,

    /// Total byte length of the source archive.
    pub bytes_before: usize,

    /// Total byte length of the compacted output archive.
    pub bytes_after: usize,

    /// Percentage reduction in archive size:
    /// `(bytes_before - bytes_after) / bytes_before * 100.0`.
    /// Returns `0.0` when `bytes_before == 0`.
    pub reduction_pct: f64,
}

impl CompactStats {
    fn new(
        tiles_read: usize,
        tiles_written: usize,
        tiles_deduplicated: usize,
        bytes_before: usize,
        bytes_after: usize,
    ) -> Self {
        let reduction_pct = if bytes_before == 0 {
            0.0
        } else {
            let saved = bytes_before.saturating_sub(bytes_after) as f64;
            saved / bytes_before as f64 * 100.0
        };
        Self {
            tiles_read,
            tiles_written,
            tiles_deduplicated,
            bytes_before,
            bytes_after,
            reduction_pct,
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Extract the raw tile payload slice from the source archive bytes.
///
/// `tile_data_offset` is the absolute byte position of the tile-data section
/// within `archive_bytes`; `info.data_offset` is relative to that section.
///
/// # Errors
/// Returns [`PmTilesError::InvalidFormat`] when the computed range falls
/// outside the archive bounds.
fn extract_tile_bytes<'a>(
    archive_bytes: &'a [u8],
    tile_data_offset: u64,
    info: &TileInfo,
) -> Result<&'a [u8], PmTilesError> {
    let abs_start = (tile_data_offset + info.data_offset) as usize;
    let abs_end = abs_start + info.data_length as usize;
    if abs_end > archive_bytes.len() {
        return Err(PmTilesError::InvalidFormat(format!(
            "Tile data for tile_id={} at [{abs_start}..{abs_end}) is out of bounds \
             (archive is {} bytes)",
            info.tile_id,
            archive_bytes.len()
        )));
    }
    Ok(&archive_bytes[abs_start..abs_end])
}

/// Copy geographic and centre metadata from `src` header onto `builder`.
fn apply_source_metadata(builder: &mut PmTilesBuilder, src: &PmTilesHeader) {
    builder.set_bounds(src.min_lon(), src.min_lat(), src.max_lon(), src.max_lat());
    builder.set_center(src.center_lon(), src.center_lat(), src.center_zoom);
}

/// Compute the zoom range actually present in `tiles`.
///
/// Returns `(0, 0)` for an empty tile set.
fn zoom_range_of(tiles: &[TileInfo]) -> (u8, u8) {
    if tiles.is_empty() {
        return (0, 0);
    }
    let mn = tiles.iter().map(|t| t.z).min().unwrap_or(0);
    let mx = tiles.iter().map(|t| t.z).max().unwrap_or(0);
    (mn, mx)
}

/// Core compaction logic shared by all public entry points.
fn compact_inner(
    bytes: &[u8],
    options: &CompactOptions,
) -> Result<(Vec<u8>, CompactStats), PmTilesError> {
    let bytes_before = bytes.len();

    // -----------------------------------------------------------------------
    // Step 1: Parse source archive and enumerate all logical tiles.
    // `enumerate_tiles` expands run-length entries and returns tiles sorted
    // by tile_id.
    // -----------------------------------------------------------------------
    let reader = PmTilesReader::from_bytes(bytes.to_vec())?;
    let tiles = reader.enumerate_tiles()?;
    let tiles_read = tiles.len();
    let tile_data_offset = reader.header.tile_data_offset;
    let src_header = &reader.header;

    // -----------------------------------------------------------------------
    // Step 2: Determine zoom range and tile type for the output builder.
    // -----------------------------------------------------------------------
    let (effective_min_zoom, effective_max_zoom) = if options.preserve_metadata {
        (src_header.min_zoom, src_header.max_zoom)
    } else {
        zoom_range_of(&tiles)
    };

    let tile_type = if options.preserve_metadata {
        src_header.tile_type.clone()
    } else {
        TileType::Unknown
    };

    // -----------------------------------------------------------------------
    // Step 3: Construct a fresh builder.
    // -----------------------------------------------------------------------
    let mut builder = PmTilesBuilder::new(tile_type, effective_min_zoom, effective_max_zoom);
    if options.preserve_metadata {
        apply_source_metadata(&mut builder, src_header);
    }

    // -----------------------------------------------------------------------
    // Step 4: Feed tiles into the builder.
    //
    // `content_map` tracks already-seen raw bytes (keyed by the byte vector
    // itself, so no hash-collision ambiguity is possible here) purely for
    // statistics when `deduplicate == true`.  The builder always deduplicates
    // (FNV-1a pre-filter plus a mandatory byte-for-byte comparison) regardless
    // of this flag, but `content_map` lets us count how many tiles were
    // "duplicate" from a logical perspective.
    //
    // All tiles are dispatched to the builder (we never skip a tile_id from
    // the directory), so that every tile_id remains addressable in the output.
    // The builder's deduplication means duplicate payloads share one physical
    // copy in the tile-data section.
    // -----------------------------------------------------------------------
    let mut content_map: HashMap<Vec<u8>, u64> = HashMap::new();
    let mut tiles_deduplicated = 0usize;

    for info in &tiles {
        let raw = extract_tile_bytes(bytes, tile_data_offset, info)?;

        if options.deduplicate {
            if content_map.contains_key(raw) {
                tiles_deduplicated += 1;
            } else {
                content_map.insert(raw.to_vec(), info.tile_id);
            }
        }

        // Always add the tile to the builder so every tile_id is represented.
        builder.add_tile_by_id(info.tile_id, raw)?;
    }

    let tiles_written = tiles_read;

    // -----------------------------------------------------------------------
    // Step 5: Build the compacted archive.
    // -----------------------------------------------------------------------
    let compacted = builder.build()?;
    let bytes_after = compacted.len();

    let stats = CompactStats::new(
        tiles_read,
        tiles_written,
        tiles_deduplicated,
        bytes_before,
        bytes_after,
    );

    Ok((compacted, stats))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Compact a PMTiles v3 archive in memory.
///
/// Reads all logical tiles from `bytes`, rebuilds the archive with fresh
/// contiguous offsets, removes all unreachable gaps in the tile-data section,
/// and re-encodes the directory.  The output is a valid PMTiles v3 archive.
///
/// # Gap removal
/// Gaps arise from:
/// - Deleted tiles (directory entries pointing to stale payload bytes).
/// - Replaced tiles (new data written at different offsets; old bytes orphaned).
/// - Fragmented run-length entries after partial updates.
///
/// After compaction the tile-data section contains only reachable payloads;
/// the archive is typically smaller than or equal to the source.
///
/// # Errors
/// - [`PmTilesError::InvalidFormat`] when the source archive is malformed.
/// - [`PmTilesError::UnsupportedVersion`] when the source is not PMTiles v3.
pub fn compact_archive(bytes: &[u8], options: &CompactOptions) -> Result<Vec<u8>, PmTilesError> {
    let (compacted, _stats) = compact_inner(bytes, options)?;
    Ok(compacted)
}

/// Compact with default options: deduplicate, sort by tile ID, preserve metadata.
///
/// Equivalent to `compact_archive(bytes, &CompactOptions::default())`.
///
/// # Errors
/// Propagates errors from [`compact_archive`].
pub fn compact_archive_default(bytes: &[u8]) -> Result<Vec<u8>, PmTilesError> {
    compact_archive(bytes, &CompactOptions::default())
}

/// Compact a PMTiles v3 archive and return both the compacted bytes and run
/// statistics.
///
/// The [`CompactStats`] includes byte counts before and after, tile counts,
/// deduplication counts, and a percentage reduction.  This is useful for
/// monitoring, logging, or deciding whether to replace the original archive.
///
/// # Errors
/// Propagates errors from [`compact_archive`].
pub fn compact_archive_with_stats(
    bytes: &[u8],
    options: &CompactOptions,
) -> Result<(Vec<u8>, CompactStats), PmTilesError> {
    compact_inner(bytes, options)
}
