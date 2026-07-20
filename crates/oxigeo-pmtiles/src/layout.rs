//! PMTiles directory layout strategy analysis and auto-selection.
//!
//! The PMTiles v3 header carries a single `clustered` flag (byte offset 96)
//! that signals whether tile payloads are stored in ascending `tile_id` order
//! with monotonically non-decreasing data offsets.  A clustered archive can be
//! streamed and supports delta-encoded offsets in directories.  Beyond that
//! single flag, real-world archives benefit from picking *how* the directory
//! is laid out: compact for small or deduplication-heavy data, leaf-split for
//! very large tile counts, and plain clustered for the common case.
//!
//! This module provides a lightweight analysis of a tile-ordering manifest
//! (`(tile_id, data_offset, data_length)` triples) and a deterministic
//! strategy selector.  The writer ([`crate::writer::PmTilesBuilder`]) consumes
//! these to resolve [`LayoutStrategy::Auto`] before serialising the header, and
//! the reader ([`crate::pmtiles::PmTilesReader`]) re-derives the analysis from a
//! decoded directory.
//!
//! Reference: <https://github.com/protomaps/PMTiles/blob/main/spec/v3/spec.md>

use crate::writer::PmTilesBuilder;

/// Tile-count threshold at and above which [`LayoutStrategy::Auto`] selects
/// [`LayoutStrategy::LeafOptimized`].
///
/// At `16_384` tiles the root directory approaches the PMTiles-recommended
/// ~16 kB ceiling, so splitting into leaf directories keeps the root small and
/// range requests cheap.  A count of `16_383` resolves to
/// [`LayoutStrategy::Clustered`]; `16_384` resolves to
/// [`LayoutStrategy::LeafOptimized`].
pub const LEAF_OPTIMIZED_TILE_THRESHOLD: usize = 16_384;

/// Deduplication-ratio threshold above which [`LayoutStrategy::Auto`] selects
/// [`LayoutStrategy::Compact`].
///
/// A ratio strictly greater than `0.5` means more than half of the addressed
/// tiles share content with another tile, so a compact directory (favouring
/// run-length sharing over streamability) minimises archive size.
pub const COMPACT_DEDUP_RATIO_THRESHOLD: f64 = 0.5;

/// Directory layout strategy for a PMTiles archive.
///
/// The variant selected drives the header `clustered` flag and (where the
/// writer supports it) whether the root directory is split into leaf
/// directories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayoutStrategy {
    /// Pick a concrete strategy automatically from a [`LayoutAnalysis`].
    ///
    /// Resolution rules (see [`choose_strategy`]): deduplication-heavy archives
    /// become [`Compact`](LayoutStrategy::Compact); very large tile counts
    /// become [`LeafOptimized`](LayoutStrategy::LeafOptimized); everything else
    /// becomes [`Clustered`](LayoutStrategy::Clustered).
    #[default]
    Auto,
    /// Tiles ordered by `tile_id` with monotonically non-decreasing data
    /// offsets.  The archive is streamable and the header `clustered` flag is
    /// set.  Best for the common, moderate-sized, low-deduplication case.
    Clustered,
    /// Split the directory early into leaf directories.  Keeps the root
    /// directory small for large archives so initial range requests stay cheap.
    /// The header `clustered` flag remains set (the data is still ordered).
    LeafOptimized,
    /// Minimise directory size, favouring run-length / content sharing over
    /// strict streamable ordering.  Best for small or deduplication-heavy
    /// archives.  The header `clustered` flag is cleared.
    Compact,
}

/// Result of analysing a tile-ordering manifest.
///
/// Produced by [`analyze_tile_ordering`] and consumed by [`choose_strategy`].
#[derive(Debug, Clone)]
pub struct LayoutAnalysis {
    /// Total number of tile entries analysed.
    pub tile_count: usize,
    /// Number of distinct `data_offset` values.  Identical offsets indicate
    /// deduplicated tiles that share a single payload, so this is a proxy for
    /// the count of unique payloads.
    pub unique_data_count: usize,
    /// Fraction of tiles that are deduplicated, in `[0.0, 1.0)`.
    ///
    /// Computed as `1.0 - unique_data_count / tile_count`.  `0.0` when every
    /// tile is unique (or the manifest is empty).
    pub dedup_ratio: f64,
    /// `true` when, taken in ascending `tile_id` order, the data offsets are
    /// monotonically non-decreasing (i.e. the archive is clustered).  Empty and
    /// single-entry manifests are trivially clustered.
    pub is_clustered: bool,
    /// Largest gap, in bytes, between the end of one tile's payload and the
    /// start of the next (in `tile_id` order).  `0` when there are no positive
    /// gaps or fewer than two entries.
    pub max_gap: u64,
    /// Mean of all positive inter-tile gaps in bytes.  `0.0` when there are no
    /// positive gaps or fewer than two entries.
    pub mean_gap: f64,
}

/// Analyse a tile-ordering manifest of `(tile_id, data_offset, data_length)`
/// triples.
///
/// The manifest does not need to be pre-sorted; a copy is sorted by `tile_id`
/// internally before checking clustering and computing gaps.  Deduplication is
/// inferred from repeated `data_offset` values (deduplicated tiles point at the
/// same payload offset).
///
/// See [`LayoutAnalysis`] for the meaning of each computed field.
pub fn analyze_tile_ordering(entries: &[(u64, u64, u64)]) -> LayoutAnalysis {
    let tile_count = entries.len();

    // Empty manifest: trivially clustered, zero everything.
    if tile_count == 0 {
        return LayoutAnalysis {
            tile_count: 0,
            unique_data_count: 0,
            dedup_ratio: 0.0,
            is_clustered: true,
            max_gap: 0,
            mean_gap: 0.0,
        };
    }

    // Count distinct data offsets (proxy for unique, non-deduplicated payloads).
    let mut distinct_offsets: Vec<u64> = entries.iter().map(|&(_, offset, _)| offset).collect();
    distinct_offsets.sort_unstable();
    distinct_offsets.dedup();
    let unique_data_count = distinct_offsets.len();

    // Deduplication ratio: fraction of tiles sharing a payload with another.
    let dedup_ratio = 1.0 - (unique_data_count as f64 / tile_count as f64);

    // Sort a copy by tile_id to evaluate clustering and gaps in tile order.
    let mut sorted: Vec<(u64, u64, u64)> = entries.to_vec();
    sorted.sort_by_key(|&(tile_id, _, _)| tile_id);

    let mut is_clustered = true;
    let mut max_gap: u64 = 0;
    let mut gap_sum: u64 = 0;
    let mut gap_count: u64 = 0;

    for window in sorted.windows(2) {
        let (_, prev_offset, prev_len) = window[0];
        let (_, next_offset, _) = window[1];

        // Monotonic non-decreasing offsets ⇒ clustered.
        if next_offset < prev_offset {
            is_clustered = false;
        }

        // Gap only when the next payload starts at or after the end of the
        // previous payload.  Overlapping / deduplicated tiles (next_offset
        // before prev end) contribute no positive gap.
        let prev_end = prev_offset.saturating_add(prev_len);
        if next_offset >= prev_end {
            let gap = next_offset - prev_end;
            if gap > max_gap {
                max_gap = gap;
            }
            gap_sum = gap_sum.saturating_add(gap);
            gap_count += 1;
        }
    }

    let mean_gap = if gap_count == 0 {
        0.0
    } else {
        gap_sum as f64 / gap_count as f64
    };

    LayoutAnalysis {
        tile_count,
        unique_data_count,
        dedup_ratio,
        is_clustered,
        max_gap,
        mean_gap,
    }
}

/// Resolve a (possibly [`Auto`](LayoutStrategy::Auto)) strategy into a concrete
/// one using a [`LayoutAnalysis`].
///
/// An explicit strategy is returned unchanged (pass-through).  For
/// [`LayoutStrategy::Auto`] the rules are, in order:
/// 1. `dedup_ratio` > [`COMPACT_DEDUP_RATIO_THRESHOLD`] ⇒
///    [`LayoutStrategy::Compact`].
/// 2. `tile_count` ≥ [`LEAF_OPTIMIZED_TILE_THRESHOLD`] ⇒
///    [`LayoutStrategy::LeafOptimized`].
/// 3. otherwise ⇒ [`LayoutStrategy::Clustered`].
pub fn choose_strategy(analysis: &LayoutAnalysis, strategy: LayoutStrategy) -> LayoutStrategy {
    match strategy {
        LayoutStrategy::Auto => {
            if analysis.dedup_ratio > COMPACT_DEDUP_RATIO_THRESHOLD {
                LayoutStrategy::Compact
            } else if analysis.tile_count >= LEAF_OPTIMIZED_TILE_THRESHOLD {
                LayoutStrategy::LeafOptimized
            } else {
                LayoutStrategy::Clustered
            }
        }
        explicit => explicit,
    }
}

/// Apply a *concrete* strategy to a [`PmTilesBuilder`], setting its `clustered`
/// flag accordingly.
///
/// [`Clustered`](LayoutStrategy::Clustered) and
/// [`LeafOptimized`](LayoutStrategy::LeafOptimized) set the flag (`true`);
/// [`Compact`](LayoutStrategy::Compact) clears it (`false`).
///
/// [`LayoutStrategy::Auto`] is expected to have been resolved via
/// [`choose_strategy`] beforehand; if it reaches here it is treated defensively
/// as [`Clustered`](LayoutStrategy::Clustered) (flag set).
pub fn apply_strategy_to_writer(builder: &mut PmTilesBuilder, strategy: LayoutStrategy) {
    let clustered = match strategy {
        LayoutStrategy::Compact => false,
        LayoutStrategy::Clustered | LayoutStrategy::LeafOptimized | LayoutStrategy::Auto => true,
    };
    builder.set_clustered_flag(clustered);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_strategy_is_auto() {
        assert_eq!(LayoutStrategy::default(), LayoutStrategy::Auto);
    }

    #[test]
    fn test_analyze_empty() {
        let analysis = analyze_tile_ordering(&[]);
        assert_eq!(analysis.tile_count, 0);
        assert_eq!(analysis.unique_data_count, 0);
        assert_eq!(analysis.dedup_ratio, 0.0);
        assert!(analysis.is_clustered);
        assert_eq!(analysis.max_gap, 0);
        assert_eq!(analysis.mean_gap, 0.0);
    }

    #[test]
    fn test_analyze_monotonic_clustered() {
        let entries = [(0, 0, 10), (1, 10, 10), (2, 20, 5)];
        let analysis = analyze_tile_ordering(&entries);
        assert!(analysis.is_clustered);
        assert_eq!(analysis.unique_data_count, 3);
        assert_eq!(analysis.dedup_ratio, 0.0);
    }

    #[test]
    fn test_analyze_decreasing_not_clustered() {
        // In tile_id order the offsets go 0, 100, 50 → not monotonic.
        let entries = [(0, 0, 10), (1, 100, 10), (2, 50, 10)];
        let analysis = analyze_tile_ordering(&entries);
        assert!(!analysis.is_clustered);
    }

    #[test]
    fn test_analyze_gaps() {
        // Offsets: 0 (len 10) → end 10, next 30 → gap 20; 30 (len 5) → end 35,
        // next 35 → gap 0.
        let entries = [(0, 0, 10), (1, 30, 5), (2, 35, 5)];
        let analysis = analyze_tile_ordering(&entries);
        assert_eq!(analysis.max_gap, 20);
        // gaps are 20 and 0 → mean 10.
        assert_eq!(analysis.mean_gap, 10.0);
    }

    #[test]
    fn test_choose_auto_small_clustered() {
        let analysis = analyze_tile_ordering(&[(0, 0, 10), (1, 10, 10)]);
        assert_eq!(
            choose_strategy(&analysis, LayoutStrategy::Auto),
            LayoutStrategy::Clustered
        );
    }

    #[test]
    fn test_choose_explicit_passthrough() {
        let analysis = analyze_tile_ordering(&[(0, 0, 10)]);
        for s in [
            LayoutStrategy::Clustered,
            LayoutStrategy::Compact,
            LayoutStrategy::LeafOptimized,
        ] {
            assert_eq!(choose_strategy(&analysis, s), s);
        }
    }
}
