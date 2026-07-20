//! Parallel tile compression pipeline for PMTiles v3 archives.
//!
//! This module provides a data-parallel encoding path that compresses raw
//! (uncompressed) tile payloads using [`rayon`] before handing the compressed
//! bytes to a [`PmTilesBuilder`].
//!
//! # Feature gate
//! This entire module is gated behind the `parallel` feature, which implies
//! `compression`.  All public functions also carry an explicit
//! `#[cfg(feature = "compression")]` guard so that the API surface remains
//! consistent even if the feature graph changes in future.
//!
//! # Design
//! The pipeline operates in three distinct phases:
//!
//! 1. **Parallel compression** — [`compress_tiles_parallel`] fans out each
//!    [`RawTile`] to rayon's global thread pool (or an explicitly-sized pool
//!    configured via [`ParallelEncodeConfig::threads`]).  Each tile is
//!    compressed independently; results are collected as
//!    `Vec<(tile_id, compressed_bytes)>`.
//!
//! 2. **Sort by tile ID** — The compressed results are sorted by `tile_id` so
//!    that [`PmTilesBuilder`] can deduplicate and run-length encode entries
//!    efficiently (the builder itself also sorts, but pre-sorting here avoids
//!    O(n log n) work inside the builder).
//!
//! 3. **Archive assembly** — [`build_pmtiles_parallel`] feeds the sorted,
//!    compressed tiles into a caller-provided [`PmTilesBuilder`] and calls
//!    [`PmTilesBuilder::build`] to produce the final byte buffer together with
//!    a [`ParallelBuildStats`] summary.
//!
//! # Thread pool customisation
//! Setting [`ParallelEncodeConfig::threads`] to `Some(n)` installs a local
//! rayon `ThreadPool` that is scoped to the compression phase only; the global
//! pool is not affected.  This is useful for benchmarks, tests that need
//! determinism, and environments where rayon's default thread count is
//! inappropriate (e.g. cloud Lambda functions with limited vCPUs).

#![cfg(feature = "parallel")]

use rayon::prelude::*;

use crate::error::PmTilesError;
use crate::header::Compression;
use crate::writer::PmTilesBuilder;

// ---------------------------------------------------------------------------
// Public data types
// ---------------------------------------------------------------------------

/// A single raw (uncompressed) tile payload together with its PMTiles tile ID.
///
/// Create one of these for every tile you want to include in the archive, then
/// pass a slice of them to [`compress_tiles_parallel`] or
/// [`build_pmtiles_parallel`].
///
/// # Tile IDs
/// Tile IDs must be valid Hilbert-curve-encoded values as produced by
/// [`crate::hilbert::zxy_to_tile_id`].  Invalid or duplicate tile IDs will
/// propagate through to [`PmTilesBuilder::add_tile_by_id`], which will
/// deduplicate them silently via FNV-1a content hashing.
#[derive(Debug, Clone)]
pub struct RawTile {
    /// PMTiles Hilbert-curve tile ID (as produced by [`crate::hilbert::zxy_to_tile_id`]).
    pub tile_id: u64,
    /// Uncompressed tile payload bytes.
    pub raw_data: Vec<u8>,
}

/// Configuration for the parallel compression pipeline.
///
/// All fields have sensible defaults via [`Default`]; only override what you
/// need.
#[derive(Debug, Clone)]
pub struct ParallelEncodeConfig {
    /// Number of tiles per rayon work chunk when using
    /// [`compress_tiles_parallel`].
    ///
    /// Larger chunks reduce scheduling overhead but may cause load imbalance
    /// when tile sizes vary greatly.  The default of 64 is a practical
    /// compromise for typical web-map tile archives.
    ///
    /// Default: `64`.
    pub chunk_size: usize,

    /// Override the number of threads in the rayon pool used for compression.
    ///
    /// When `Some(n)`, a local [`rayon::ThreadPool`] with exactly `n` threads
    /// is created and used only for the compression phase; the global pool is
    /// not modified.  When `None`, the global pool (whose size is determined by
    /// rayon's heuristics, typically the number of logical CPUs) is used.
    ///
    /// Default: `None` (use global pool).
    pub threads: Option<usize>,
}

impl Default for ParallelEncodeConfig {
    fn default() -> Self {
        Self {
            chunk_size: 64,
            threads: None,
        }
    }
}

/// Statistics produced by [`build_pmtiles_parallel`].
#[derive(Debug, Clone)]
pub struct ParallelBuildStats {
    /// Total number of tiles that were encoded (compressed and inserted).
    pub tiles_encoded: usize,
    /// Number of rayon work chunks that were dispatched.
    pub chunks_processed: usize,
    /// Sum of all input (uncompressed) tile payload sizes in bytes.
    pub total_bytes_raw: u64,
    /// Sum of all output (compressed) tile payload sizes in bytes.
    pub total_bytes_compressed: u64,
    /// `total_bytes_compressed / total_bytes_raw`.
    ///
    /// Returns `1.0` when `total_bytes_raw == 0` to avoid division by zero.
    pub compression_ratio: f64,
}

// ---------------------------------------------------------------------------
// Low-level single-tile compression
// ---------------------------------------------------------------------------

/// Compress a single tile payload using the given [`Compression`] codec.
///
/// This is the fundamental building block of the parallel pipeline.  It is
/// exposed publicly so that callers can benchmark individual codecs or
/// pre-compress tiles outside of the parallel pipeline.
///
/// # Behaviour by codec
/// * [`Compression::None`] / [`Compression::Unknown`] — the input bytes are
///   returned as a new `Vec<u8>` without any transformation (pass-through).
/// * [`Compression::Gzip`] — RFC 1952 gzip at level 6 via
///   [`oxiarc_archive::gzip::compress`].
/// * [`Compression::Brotli`] — Brotli at quality 6 via
///   [`oxiarc_archive::brotli::compress_with_quality`].
/// * [`Compression::Zstd`] — Zstandard at the codec default level via
///   [`oxiarc_archive::zstd::compress`].
///
/// # Errors
/// Returns [`PmTilesError::Decompression`] (reused for compression errors
/// throughout the crate) when the underlying OxiARC codec reports a failure.
#[cfg(feature = "compression")]
pub fn compress_tile(data: &[u8], compression: Compression) -> Result<Vec<u8>, PmTilesError> {
    match compression {
        Compression::None | Compression::Unknown => Ok(data.to_vec()),
        Compression::Gzip => oxiarc_archive::gzip::compress(data, 6)
            .map_err(|e| PmTilesError::Decompression(format!("Gzip compression failed: {e}"))),
        Compression::Brotli => oxiarc_archive::brotli::compress_with_quality(data, 6)
            .map_err(|e| PmTilesError::Decompression(format!("Brotli compression failed: {e}"))),
        Compression::Zstd => oxiarc_archive::zstd::compress(data)
            .map_err(|e| PmTilesError::Decompression(format!("Zstd compression failed: {e}"))),
    }
}

// ---------------------------------------------------------------------------
// Parallel compression of a tile slice
// ---------------------------------------------------------------------------

/// Compress all tiles in parallel using rayon, preserving tile IDs.
///
/// Fans out compression work across rayon's thread pool (or a local pool when
/// [`ParallelEncodeConfig::threads`] is `Some`).  Tiles within each chunk are
/// processed sequentially; chunks themselves run in parallel.
///
/// # Return value
/// Returns a `Vec<(tile_id, compressed_bytes)>` whose order is **not
/// guaranteed** to match the input order (rayon may reorder chunks).  Call
/// `.sort_by_key(|(id, _)| *id)` before inserting into a
/// [`PmTilesBuilder`] if ordering is required.
///
/// # Empty input
/// An empty `raw_tiles` slice returns an empty `Vec` without spawning any
/// tasks.
///
/// # Errors
/// Returns the first [`PmTilesError`] encountered by any compression task.
/// Remaining tasks may or may not have completed at the point of the error
/// (rayon's `collect::<Result<…>>()` semantics).
#[cfg(feature = "compression")]
pub fn compress_tiles_parallel(
    raw_tiles: &[RawTile],
    compression: Compression,
    config: &ParallelEncodeConfig,
) -> Result<Vec<(u64, Vec<u8>)>, PmTilesError> {
    if raw_tiles.is_empty() {
        return Ok(Vec::new());
    }

    // Helper closure that performs the actual per-tile compression.
    // `compression` is cloned once per tile since the enum is not `Copy`.
    let compress_one = |tile: &RawTile| -> Result<(u64, Vec<u8>), PmTilesError> {
        let compressed = compress_tile(&tile.raw_data, compression.clone())?;
        Ok((tile.tile_id, compressed))
    };

    let chunk_size = config.chunk_size.max(1);

    // When an explicit thread count is requested, spin up a scoped local pool
    // so we do not permanently alter the global rayon configuration.
    if let Some(n_threads) = config.threads {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(n_threads)
            .build()
            .map_err(|e| {
                PmTilesError::InvalidFormat(format!(
                    "Failed to create rayon thread pool with {n_threads} threads: {e}"
                ))
            })?;

        pool.install(|| {
            raw_tiles
                .par_chunks(chunk_size)
                .map(|chunk| {
                    chunk
                        .iter()
                        .map(compress_one)
                        .collect::<Result<Vec<_>, PmTilesError>>()
                })
                .collect::<Result<Vec<Vec<_>>, PmTilesError>>()
                .map(|nested| nested.into_iter().flatten().collect())
        })
    } else {
        raw_tiles
            .par_chunks(chunk_size)
            .map(|chunk| {
                chunk
                    .iter()
                    .map(compress_one)
                    .collect::<Result<Vec<_>, PmTilesError>>()
            })
            .collect::<Result<Vec<Vec<_>>, PmTilesError>>()
            .map(|nested| nested.into_iter().flatten().collect())
    }
}

// ---------------------------------------------------------------------------
// Full parallel build pipeline
// ---------------------------------------------------------------------------

/// Compress tiles in parallel, then assemble a complete PMTiles v3 archive.
///
/// This is the primary high-level API of this module.  It combines all three
/// pipeline phases described in the [module documentation](self):
///
/// 1. Record raw byte counts for statistics.
/// 2. Fan out compression via [`compress_tiles_parallel`].
/// 3. Sort compressed results by tile ID (ascending).
/// 4. Configure `builder` with the chosen [`Compression`] for the tile header
///    byte (the pre-compressed bytes are inserted as-is; the builder does NOT
///    re-compress them).
/// 5. Insert each tile via [`PmTilesBuilder::add_tile_by_id`].
/// 6. Finalise the archive with [`PmTilesBuilder::build`].
/// 7. Compute and return [`ParallelBuildStats`].
///
/// # Panics
/// Does not panic.  All error paths surface as [`PmTilesError`].
///
/// # Errors
/// * [`PmTilesError::Decompression`] — a codec failed during compression.
/// * [`PmTilesError::InvalidFormat`] — rayon thread pool creation failed
///   (only when [`ParallelEncodeConfig::threads`] is `Some`), or a tile ID
///   caused a directory encoding error inside the builder.
/// * Any error returned by [`PmTilesBuilder::build`].
#[cfg(feature = "compression")]
pub fn build_pmtiles_parallel(
    raw_tiles: Vec<RawTile>,
    compression: Compression,
    mut builder: PmTilesBuilder,
    config: &ParallelEncodeConfig,
) -> Result<(Vec<u8>, ParallelBuildStats), PmTilesError> {
    // Phase 1: Measure raw byte totals before we move `raw_tiles`.
    let tiles_encoded = raw_tiles.len();
    let total_bytes_raw: u64 = raw_tiles
        .iter()
        .map(|t| t.raw_data.len() as u64)
        .fold(0u64, u64::saturating_add);

    let chunks_processed = if tiles_encoded == 0 {
        0
    } else {
        let chunk_size = config.chunk_size.max(1);
        tiles_encoded.div_ceil(chunk_size)
    };

    // Phase 2: Compress all tiles in parallel.
    let mut compressed_pairs = compress_tiles_parallel(&raw_tiles, compression.clone(), config)?;

    // Phase 3: Sort by tile_id so the builder receives a clustered layout.
    // This ensures deterministic archive layout and optimal run-length
    // encoding even if rayon reordered chunks.
    compressed_pairs.sort_unstable_by_key(|(id, _)| *id);

    // Phase 4: Collect compressed byte totals and record compression on builder.
    let total_bytes_compressed: u64 = compressed_pairs
        .iter()
        .map(|(_, bytes)| bytes.len() as u64)
        .fold(0u64, u64::saturating_add);

    // Tell the builder which compression codec was applied to the tile payloads
    // so the header byte is written correctly.  The builder will not attempt to
    // re-compress the bytes; it stores them verbatim.
    builder.set_tile_compression(compression);

    // Phase 5: Feed compressed tiles into the builder.
    for (tile_id, compressed_bytes) in compressed_pairs {
        builder.add_tile_by_id(tile_id, &compressed_bytes)?;
    }

    // Phase 6: Assemble the archive.
    let archive = builder.build()?;

    // Phase 7: Compute derived stats.
    let compression_ratio = if total_bytes_raw == 0 {
        1.0
    } else {
        total_bytes_compressed as f64 / total_bytes_raw as f64
    };

    let stats = ParallelBuildStats {
        tiles_encoded,
        chunks_processed,
        total_bytes_raw,
        total_bytes_compressed,
        compression_ratio,
    };

    Ok((archive, stats))
}
