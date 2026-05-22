//! Integration tests for PMTiles directory layout strategy auto-selection.

#![allow(clippy::expect_used)]

use oxigdal_pmtiles::{
    LayoutStrategy, PmTilesBuilder, PmTilesReader, TileType, analyze_tile_ordering, choose_strategy,
};

// Header byte offset of the `clustered` flag (PMTiles v3 spec).
const CLUSTERED_BYTE_OFFSET: usize = 96;

// ── analyze_tile_ordering ─────────────────────────────────────────────────────

#[test]
fn test_analyze_tile_ordering_empty_returns_zero_stats() {
    let analysis = analyze_tile_ordering(&[]);
    assert_eq!(analysis.tile_count, 0);
    assert_eq!(analysis.unique_data_count, 0);
    assert_eq!(analysis.dedup_ratio, 0.0);
    assert!(
        analysis.is_clustered,
        "empty manifest is trivially clustered"
    );
    assert_eq!(analysis.max_gap, 0);
    assert_eq!(analysis.mean_gap, 0.0);
}

#[test]
fn test_analyze_tile_ordering_monotonic_offsets_clustered_true() {
    // (tile_id, data_offset, data_length) with non-decreasing offsets.
    let entries = [(0, 0, 100), (1, 100, 100), (2, 200, 50), (3, 250, 50)];
    let analysis = analyze_tile_ordering(&entries);
    assert!(analysis.is_clustered);
    assert_eq!(analysis.tile_count, 4);
    assert_eq!(analysis.unique_data_count, 4);
    assert_eq!(analysis.dedup_ratio, 0.0);
}

#[test]
fn test_analyze_tile_ordering_unsorted_offsets_clustered_false() {
    // In ascending tile_id order the offsets jump down (100 → 50), so the
    // archive is not clustered. The manifest is intentionally unsorted on input
    // to exercise the internal sort-by-tile_id.
    let entries = [(2, 50, 10), (0, 0, 10), (1, 100, 10)];
    let analysis = analyze_tile_ordering(&entries);
    assert!(!analysis.is_clustered);
}

#[test]
fn test_analyze_tile_ordering_dedup_ratio_computed() {
    // Four tiles, two share data_offset 0 → 3 distinct offsets {0, 20, 40}.
    let entries = [(0, 0, 10), (1, 0, 10), (2, 20, 10), (3, 40, 10)];
    let analysis = analyze_tile_ordering(&entries);
    assert_eq!(analysis.tile_count, 4);
    assert_eq!(analysis.unique_data_count, 3);
    // 1 - 3/4 = 0.25
    assert!((analysis.dedup_ratio - 0.25).abs() < 1e-12);
}

// ── choose_strategy ───────────────────────────────────────────────────────────

#[test]
fn test_choose_strategy_auto_small_picks_clustered() {
    // 100 unique tiles, no dedup → Clustered.
    let entries: Vec<(u64, u64, u64)> = (0..100u64).map(|i| (i, i * 10, 10)).collect();
    let analysis = analyze_tile_ordering(&entries);
    assert_eq!(analysis.dedup_ratio, 0.0);
    assert_eq!(
        choose_strategy(&analysis, LayoutStrategy::Auto),
        LayoutStrategy::Clustered
    );
}

#[test]
fn test_choose_strategy_auto_large_picks_leaf_optimized() {
    // Exactly 16_384 unique tiles, 0 dedup → LeafOptimized (threshold is >=).
    let entries: Vec<(u64, u64, u64)> = (0..16_384u64).map(|i| (i, i * 10, 10)).collect();
    let analysis = analyze_tile_ordering(&entries);
    assert_eq!(analysis.dedup_ratio, 0.0);
    assert_eq!(analysis.tile_count, 16_384);
    assert_eq!(
        choose_strategy(&analysis, LayoutStrategy::Auto),
        LayoutStrategy::LeafOptimized
    );

    // One fewer (16_383) stays Clustered.
    let smaller: Vec<(u64, u64, u64)> = (0..16_383u64).map(|i| (i, i * 10, 10)).collect();
    let smaller_analysis = analyze_tile_ordering(&smaller);
    assert_eq!(
        choose_strategy(&smaller_analysis, LayoutStrategy::Auto),
        LayoutStrategy::Clustered
    );
}

#[test]
fn test_choose_strategy_auto_dedup_heavy_picks_compact() {
    // 10 tiles but only 2 distinct offsets → dedup_ratio = 1 - 2/10 = 0.8 > 0.5.
    let mut entries: Vec<(u64, u64, u64)> = Vec::new();
    for i in 0..5u64 {
        entries.push((i, 0, 10));
    }
    for i in 5..10u64 {
        entries.push((i, 100, 10));
    }
    let analysis = analyze_tile_ordering(&entries);
    assert!(analysis.dedup_ratio > 0.5);
    assert_eq!(
        choose_strategy(&analysis, LayoutStrategy::Auto),
        LayoutStrategy::Compact
    );
}

#[test]
fn test_choose_strategy_explicit_passes_through() {
    // Even with dedup-heavy / large analysis, an explicit strategy is returned
    // unchanged.
    let entries: Vec<(u64, u64, u64)> = (0..20_000u64).map(|_| (0, 0, 10)).collect();
    let analysis = analyze_tile_ordering(&entries);
    for s in [
        LayoutStrategy::Clustered,
        LayoutStrategy::Compact,
        LayoutStrategy::LeafOptimized,
    ] {
        assert_eq!(choose_strategy(&analysis, s), s);
    }
}

// ── Builder strategy field ────────────────────────────────────────────────────

#[test]
fn test_layout_strategy_default_is_auto() {
    let builder = PmTilesBuilder::new(TileType::Png, 0, 2);
    assert_eq!(builder.layout_strategy(), LayoutStrategy::Auto);
    assert_eq!(LayoutStrategy::default(), LayoutStrategy::Auto);
}

#[test]
fn test_set_layout_strategy_overrides_default() {
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 2);
    builder.set_layout_strategy(LayoutStrategy::Compact);
    assert_eq!(builder.layout_strategy(), LayoutStrategy::Compact);
    // Chaining returns &mut Self.
    builder
        .set_layout_strategy(LayoutStrategy::LeafOptimized)
        .set_layout_strategy(LayoutStrategy::Clustered);
    assert_eq!(builder.layout_strategy(), LayoutStrategy::Clustered);
}

// ── Writer → header byte round-trip ───────────────────────────────────────────

#[test]
fn test_writer_clustered_strategy_emits_header_byte_one() {
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 2);
    builder.set_layout_strategy(LayoutStrategy::Clustered);
    builder.add_tile(0, 0, 0, b"tile-a").expect("add a");
    builder.add_tile(1, 0, 0, b"tile-b").expect("add b");
    builder.add_tile(1, 1, 0, b"tile-c").expect("add c");
    let archive = builder.build().expect("build");
    assert_eq!(
        archive[CLUSTERED_BYTE_OFFSET], 1,
        "explicit Clustered must set header byte 96 to 1"
    );
}

#[test]
fn test_writer_compact_strategy_emits_header_byte_zero() {
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 2);
    builder.set_layout_strategy(LayoutStrategy::Compact);
    builder.add_tile(0, 0, 0, b"tile-a").expect("add a");
    builder.add_tile(1, 0, 0, b"tile-b").expect("add b");
    builder.add_tile(1, 1, 0, b"tile-c").expect("add c");
    let archive = builder.build().expect("build");
    assert_eq!(
        archive[CLUSTERED_BYTE_OFFSET], 0,
        "explicit Compact must clear header byte 96 to 0"
    );
}

// ── Reader inspection ─────────────────────────────────────────────────────────

#[test]
fn test_reader_is_clustered_matches_header() {
    // Clustered archive.
    let mut clustered = PmTilesBuilder::new(TileType::Png, 0, 2);
    clustered.set_layout_strategy(LayoutStrategy::Clustered);
    clustered.add_tile(0, 0, 0, b"a").expect("add");
    clustered.add_tile(1, 0, 0, b"b").expect("add");
    let clustered_bytes = clustered.build().expect("build");
    let clustered_reader = PmTilesReader::from_bytes(clustered_bytes).expect("reader");
    assert!(clustered_reader.is_clustered());
    assert_eq!(
        clustered_reader.is_clustered(),
        clustered_reader.header.clustered
    );

    // Compact archive → clustered flag cleared.
    let mut compact = PmTilesBuilder::new(TileType::Png, 0, 2);
    compact.set_layout_strategy(LayoutStrategy::Compact);
    compact.add_tile(0, 0, 0, b"a").expect("add");
    compact.add_tile(1, 0, 0, b"b").expect("add");
    let compact_bytes = compact.build().expect("build");
    let compact_reader = PmTilesReader::from_bytes(compact_bytes).expect("reader");
    assert!(!compact_reader.is_clustered());
    assert_eq!(
        compact_reader.is_clustered(),
        compact_reader.header.clustered
    );
}

#[test]
fn test_reader_detected_layout_matches_writer_analysis() {
    // Write four distinct tiles, read back, and analyse the directory.
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 2);
    builder.add_tile(0, 0, 0, b"\x01\x02\x03").expect("add");
    builder.add_tile(1, 0, 0, b"\x04\x05\x06\x07").expect("add");
    builder.add_tile(1, 1, 0, b"\x08\x09").expect("add");
    builder
        .add_tile(1, 0, 1, b"\x0A\x0B\x0C\x0D\x0E")
        .expect("add");
    let archive = builder.build().expect("build");

    let reader = PmTilesReader::from_bytes(archive).expect("reader");
    let analysis = reader.detected_layout().expect("detected layout");

    // Four distinct tiles → four entries, all unique, fully clustered.
    assert_eq!(analysis.tile_count, 4);
    assert_eq!(analysis.unique_data_count, 4);
    assert_eq!(analysis.dedup_ratio, 0.0);
    assert!(analysis.is_clustered);
    // detected_layout's clustering must agree with the header flag.
    assert_eq!(analysis.is_clustered, reader.is_clustered());
}
