//! Re-compression of PMTiles archives between gzip / brotli / zstd formats.
//!
//! Currently the [`crate::pmtiles::PmTilesReader`] transparently decompresses
//! tile payloads via OxiARC, and the [`crate::writer::PmTilesBuilder`] can be
//! configured to advertise any [`Compression`] in the header.  This module
//! ties those pieces together and provides a one-shot API for converting an
//! existing PMTiles archive whose tile payloads are compressed with one
//! algorithm into a new archive whose payloads use a different algorithm.
//!
//! # Algorithm
//! 1. Parse the source archive via [`PmTilesReader`].
//! 2. Resolve the actual *source* compression — when the caller passes
//!    [`Compression::Unknown`] (the "auto" sentinel), use the header's
//!    `tile_compression` field.
//! 3. Reject [`Compression::Unknown`] as a target compression.
//! 4. Enumerate every logical tile via [`PmTilesReader::enumerate_tiles`].
//! 5. For each tile, slice the raw (still-compressed) bytes out of the source
//!    buffer, decompress with the source codec, recompress with the target
//!    codec at the requested level, and dispatch to a fresh
//!    [`PmTilesBuilder`] via `add_tile_by_id`.
//! 6. Mirror header metadata (tile type, zoom range, bounds, centre, JSON
//!    metadata) onto the builder so that the output archive remains
//!    semantically equivalent.
//! 7. Set the builder's `tile_compression` header byte to the target
//!    algorithm so that consumers know how to decode the payloads.
//! 8. `builder.build()` to assemble the new archive.
//!
//! # Identity transcode
//! When the resolved source and target compression are identical, the raw
//! tile bytes are copied verbatim without a decompress / recompress round
//! trip, and counted under [`TranscodeStats::tiles_skipped_identity`].
//! This is useful for "force a known compression byte without touching the
//! data" and for unit-testing the identity path.

use crate::error::PmTilesError;
use crate::header::Compression;
use crate::pmtiles::{PmTilesReader, TileInfo};
use crate::writer::PmTilesBuilder;

// ---------------------------------------------------------------------------
// TranscodeOptions
// ---------------------------------------------------------------------------

/// Options controlling [`transcode_archive`] and
/// [`transcode_archive_with_stats`].
#[derive(Debug, Clone)]
pub struct TranscodeOptions {
    /// Source compression algorithm.  When set to [`Compression::Unknown`]
    /// the actual algorithm is auto-detected from the source archive's
    /// `tile_compression` header field.
    ///
    /// Default: [`Compression::Unknown`] (auto-detect).
    pub from: Compression,

    /// Target compression algorithm.  Must NOT be [`Compression::Unknown`];
    /// the transcoder returns [`PmTilesError::UnsupportedCompression`] if it
    /// is.
    ///
    /// Default: [`Compression::Gzip`].
    pub to: Compression,

    /// Codec-specific compression level (lower = faster / larger, higher =
    /// slower / smaller).  Interpreted by the target codec:
    ///
    /// * Gzip: 0–9 (default 6 when `None`)
    /// * Brotli: 0–11 (default 6 when `None`)
    /// * Zstd: ignored — the OxiARC zstd encoder does not currently expose
    ///   a level knob; the default is always used.
    ///
    /// Default: `None`.
    pub level: Option<i32>,
}

impl Default for TranscodeOptions {
    fn default() -> Self {
        Self {
            from: Compression::Unknown,
            to: Compression::Gzip,
            level: None,
        }
    }
}

// ---------------------------------------------------------------------------
// TranscodeStats
// ---------------------------------------------------------------------------

/// Quantitative summary of a single transcode run.
#[derive(Debug, Clone, Copy, Default)]
pub struct TranscodeStats {
    /// Number of tiles that were actually decompressed and recompressed.
    pub tiles_transcoded: u64,

    /// Number of tiles whose payload was passed through verbatim because the
    /// resolved source and target compression were identical.
    pub tiles_skipped_identity: u64,

    /// Sum of compressed-on-input tile byte lengths.
    pub bytes_before: u64,

    /// Sum of compressed-on-output tile byte lengths.
    pub bytes_after: u64,
}

impl TranscodeStats {
    /// Output/input byte ratio.  Returns `1.0` when `bytes_before == 0`.
    pub fn ratio(&self) -> f64 {
        if self.bytes_before == 0 {
            1.0
        } else {
            self.bytes_after as f64 / self.bytes_before as f64
        }
    }
}

// ---------------------------------------------------------------------------
// Low-level codec dispatch
// ---------------------------------------------------------------------------

/// Decompress `data` using the algorithm `c`.
///
/// * [`Compression::None`] / [`Compression::Unknown`] — returns a copy of
///   `data` unmodified (so transcodes can chain through "unknown ⇒ x" without
///   special-casing).
///
/// # Errors
/// Returns [`PmTilesError::Decompression`] when the underlying OxiARC codec
/// reports a failure.
fn decompress_with(data: &[u8], c: Compression) -> Result<Vec<u8>, PmTilesError> {
    match c {
        Compression::None | Compression::Unknown => Ok(data.to_vec()),
        Compression::Gzip => {
            let mut reader = std::io::Cursor::new(data);
            oxiarc_archive::gzip::decompress(&mut reader)
                .map_err(|e| PmTilesError::Decompression(format!("Gzip decompression failed: {e}")))
        }
        Compression::Brotli => oxiarc_archive::brotli::decompress(data)
            .map_err(|e| PmTilesError::Decompression(format!("Brotli decompression failed: {e}"))),
        Compression::Zstd => oxiarc_archive::zstd::decompress(data)
            .map_err(|e| PmTilesError::Decompression(format!("Zstd decompression failed: {e}"))),
    }
}

/// Compress `data` using algorithm `c` at the optional `level`.
///
/// * [`Compression::None`] — returns a copy of `data`.
/// * [`Compression::Unknown`] — returns [`PmTilesError::UnsupportedCompression`].
///
/// # Errors
/// Returns [`PmTilesError::Decompression`] when the underlying OxiARC codec
/// reports a failure, or [`PmTilesError::UnsupportedCompression`] for
/// [`Compression::Unknown`].
fn compress_with(data: &[u8], c: Compression, level: Option<i32>) -> Result<Vec<u8>, PmTilesError> {
    match c {
        Compression::None => Ok(data.to_vec()),
        Compression::Gzip => {
            // Clamp level into the valid gzip range [0, 9]; default 6.
            let lvl = level.unwrap_or(6).clamp(0, 9) as u8;
            oxiarc_archive::gzip::compress(data, lvl)
                .map_err(|e| PmTilesError::Decompression(format!("Gzip compression failed: {e}")))
        }
        Compression::Brotli => {
            // Brotli quality range is [0, 11].  Default to 6 (NORMAL).
            let lvl = level.unwrap_or(6).clamp(0, 11) as u32;
            oxiarc_archive::brotli::compress_with_quality(data, lvl)
                .map_err(|e| PmTilesError::Decompression(format!("Brotli compression failed: {e}")))
        }
        Compression::Zstd => {
            // OxiARC's zstd encoder does not expose a level today; the
            // `level` argument is accepted but currently ignored.
            oxiarc_archive::zstd::compress(data)
                .map_err(|e| PmTilesError::Decompression(format!("Zstd compression failed: {e}")))
        }
        Compression::Unknown => Err(PmTilesError::UnsupportedCompression),
    }
}

// ---------------------------------------------------------------------------
// Tile-level transcoding
// ---------------------------------------------------------------------------

/// Transcode a single tile payload from one compression algorithm to another.
///
/// * When `from == to`, the input is returned unmodified (a clone).
/// * Otherwise the payload is decompressed via `from` and recompressed via
///   `to` at the requested `level`.
///
/// # Errors
/// * [`PmTilesError::UnsupportedCompression`] when `to` is
///   [`Compression::Unknown`].
/// * [`PmTilesError::Decompression`] on codec failure.
pub fn transcode_tile(
    data: &[u8],
    from: Compression,
    to: Compression,
    level: Option<i32>,
) -> Result<Vec<u8>, PmTilesError> {
    if to == Compression::Unknown {
        return Err(PmTilesError::UnsupportedCompression);
    }
    if from == to {
        return Ok(data.to_vec());
    }
    let raw = decompress_with(data, from)?;
    compress_with(&raw, to, level)
}

// ---------------------------------------------------------------------------
// Internal helpers (shared with crate::compact)
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

// ---------------------------------------------------------------------------
// Archive-level transcoding
// ---------------------------------------------------------------------------

/// Re-compress every tile payload in a PMTiles v3 archive and return the
/// resulting archive bytes.
///
/// See [`transcode_archive_with_stats`] for the variant that also returns
/// statistics.
///
/// # Errors
/// Propagates errors from [`transcode_archive_with_stats`].
pub fn transcode_archive(bytes: &[u8], opts: &TranscodeOptions) -> Result<Vec<u8>, PmTilesError> {
    let (out, _stats) = transcode_archive_with_stats(bytes, opts)?;
    Ok(out)
}

/// Re-compress every tile payload in a PMTiles v3 archive and return both
/// the resulting archive bytes and a [`TranscodeStats`] summary.
///
/// # Errors
/// * [`PmTilesError::UnsupportedCompression`] when `opts.to` is
///   [`Compression::Unknown`].
/// * [`PmTilesError::InvalidFormat`] / [`PmTilesError::UnsupportedVersion`]
///   when the source archive is malformed.
/// * [`PmTilesError::Decompression`] on codec failure.
pub fn transcode_archive_with_stats(
    bytes: &[u8],
    opts: &TranscodeOptions,
) -> Result<(Vec<u8>, TranscodeStats), PmTilesError> {
    // Reject Unknown as target up front so the caller does not have to wait
    // for the per-tile codec dispatch to fail.
    if opts.to == Compression::Unknown {
        return Err(PmTilesError::UnsupportedCompression);
    }

    // -----------------------------------------------------------------------
    // Step 1: Parse the source archive.
    // -----------------------------------------------------------------------
    let reader = PmTilesReader::from_bytes(bytes.to_vec())?;
    let header = reader.header.clone();
    let tile_data_offset = header.tile_data_offset;

    // -----------------------------------------------------------------------
    // Step 2: Resolve the effective source compression.
    //
    // When the caller passes `Compression::Unknown` we trust the header's
    // `tile_compression` byte.  When the caller passes an explicit codec we
    // use that — this lets callers override a malformed header byte.
    // -----------------------------------------------------------------------
    let actual_from = if opts.from == Compression::Unknown {
        header.tile_compression.clone()
    } else {
        opts.from.clone()
    };

    // -----------------------------------------------------------------------
    // Step 3: Construct a fresh builder that mirrors the source header.
    // -----------------------------------------------------------------------
    let mut builder =
        PmTilesBuilder::new(header.tile_type.clone(), header.min_zoom, header.max_zoom);
    builder.set_bounds(
        header.min_lon(),
        header.min_lat(),
        header.max_lon(),
        header.max_lat(),
    );
    builder.set_center(header.center_lon(), header.center_lat(), header.center_zoom);
    builder.set_tile_compression(opts.to.clone());
    // The builder writes directory bytes uncompressed, so the output's
    // internal_compression must be `None` regardless of the source.
    builder.set_internal_compression(Compression::None);

    // Mirror the source's JSON metadata.  The reader decompresses it via the
    // source's `internal_compression`; we re-emit it uncompressed (matching
    // the builder's behaviour).
    if let Ok(metadata) = reader.metadata()
        && let Ok(json) = metadata.to_json()
    {
        builder.set_metadata(json);
    }

    // -----------------------------------------------------------------------
    // Step 4: Walk every logical tile, transcode, dispatch.
    // -----------------------------------------------------------------------
    let mut stats = TranscodeStats::default();
    let identity = actual_from == opts.to;

    let tile_infos = reader.enumerate_tiles()?;
    for info in &tile_infos {
        let raw_compressed = extract_tile_bytes(bytes, tile_data_offset, info)?;
        stats.bytes_before = stats
            .bytes_before
            .saturating_add(raw_compressed.len() as u64);

        let transcoded = if identity {
            raw_compressed.to_vec()
        } else {
            transcode_tile(
                raw_compressed,
                actual_from.clone(),
                opts.to.clone(),
                opts.level,
            )?
        };

        stats.bytes_after = stats.bytes_after.saturating_add(transcoded.len() as u64);
        if identity {
            stats.tiles_skipped_identity = stats.tiles_skipped_identity.saturating_add(1);
        } else {
            stats.tiles_transcoded = stats.tiles_transcoded.saturating_add(1);
        }

        builder.add_tile_by_id(info.tile_id, &transcoded)?;
    }

    // -----------------------------------------------------------------------
    // Step 5: Assemble the new archive.
    // -----------------------------------------------------------------------
    let out = builder.build()?;
    Ok((out, stats))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transcode_stats_default_ratio_is_one() {
        let s = TranscodeStats::default();
        assert!((s.ratio() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_transcode_stats_ratio_half() {
        let s = TranscodeStats {
            tiles_transcoded: 1,
            tiles_skipped_identity: 0,
            bytes_before: 100,
            bytes_after: 50,
        };
        assert!((s.ratio() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_transcode_options_default_values() {
        let opts = TranscodeOptions::default();
        assert_eq!(opts.from, Compression::Unknown);
        assert_eq!(opts.to, Compression::Gzip);
        assert!(opts.level.is_none());
    }

    #[test]
    fn test_transcode_tile_identity_none() {
        // None ⇒ None is a no-op clone.
        let raw = b"identity payload";
        let out =
            transcode_tile(raw, Compression::None, Compression::None, None).expect("transcode");
        assert_eq!(out.as_slice(), raw);
    }

    #[test]
    fn test_transcode_tile_unknown_target_errors() {
        let raw = b"x";
        let err = transcode_tile(raw, Compression::None, Compression::Unknown, None)
            .expect_err("must reject Unknown target");
        assert!(matches!(err, PmTilesError::UnsupportedCompression));
    }

    #[test]
    fn test_decompress_with_none_is_passthrough() {
        let data = b"raw";
        assert_eq!(decompress_with(data, Compression::None).expect("ok"), data);
        assert_eq!(
            decompress_with(data, Compression::Unknown).expect("ok"),
            data
        );
    }

    #[test]
    fn test_compress_with_none_is_passthrough() {
        let data = b"raw";
        assert_eq!(
            compress_with(data, Compression::None, None).expect("ok"),
            data
        );
    }

    #[test]
    fn test_compress_with_unknown_errors() {
        let data = b"raw";
        let err = compress_with(data, Compression::Unknown, None).expect_err("Unknown must error");
        assert!(matches!(err, PmTilesError::UnsupportedCompression));
    }
}
