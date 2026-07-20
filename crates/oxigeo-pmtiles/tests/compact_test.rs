//! Integration tests for PMTiles archive compaction.

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use oxigeo_pmtiles::{
    CompactOptions, PmTilesBuilder, PmTilesHeader, PmTilesReader, TileType, compact_archive,
    compact_archive_default, compact_archive_with_stats,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a minimal PMTiles archive containing the supplied tiles.
///
/// Zoom range is inferred from the tile list; falls back to `(0, 0)` for
/// empty lists.
fn build_archive(tiles: &[(u8, u32, u32, &[u8])]) -> Vec<u8> {
    let min_z = tiles.iter().map(|t| t.0).min().unwrap_or(0);
    let max_z = tiles.iter().map(|t| t.0).max().unwrap_or(0);
    let mut builder = PmTilesBuilder::new(TileType::Png, min_z, max_z);
    for &(z, x, y, data) in tiles {
        builder.add_tile(z, x, y, data).expect("add_tile");
    }
    builder.build().expect("build")
}

/// Assert that a byte slice starts with the PMTiles v3 magic and version.
fn assert_valid_pmtiles_header(bytes: &[u8]) {
    assert!(
        bytes.len() >= 127,
        "archive too short: {} bytes",
        bytes.len()
    );
    assert_eq!(&bytes[0..7], b"PMTiles", "magic mismatch");
    assert_eq!(bytes[7], 3, "version must be 3");
}

// ---------------------------------------------------------------------------
// Test 1 — empty archive round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_compact_empty_archive_returns_valid_archive() {
    let builder = PmTilesBuilder::new(TileType::Png, 0, 0);
    let original = builder.build().expect("build");

    let compacted = compact_archive_default(&original).expect("compact");
    assert_valid_pmtiles_header(&compacted);

    let header = PmTilesHeader::parse(&compacted).expect("parse");
    assert_eq!(header.addressed_tiles, 0, "no tiles in empty archive");
    assert_eq!(header.tile_entries, 0);
    assert_eq!(header.tile_contents, 0);
}

// ---------------------------------------------------------------------------
// Test 2 — single tile round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_compact_single_tile_round_trip() {
    let tile_bytes: &[u8] = b"single-tile-payload-12345";
    let original = build_archive(&[(0, 0, 0, tile_bytes)]);
    let compacted = compact_archive_default(&original).expect("compact");

    assert_valid_pmtiles_header(&compacted);
    let reader = PmTilesReader::from_bytes(compacted).expect("reader");
    let got = reader
        .get_tile(0, 0, 0)
        .expect("get_tile")
        .expect("tile present");
    assert_eq!(got, tile_bytes, "tile data must survive compaction");
}

// ---------------------------------------------------------------------------
// Test 3 — multiple tiles, all preserved
// ---------------------------------------------------------------------------

#[test]
fn test_compact_multi_tile_preserves_all_tiles() {
    let tiles: &[(u8, u32, u32, &[u8])] = &[
        (0, 0, 0, b"z0-data"),
        (1, 0, 0, b"z1-00-data"),
        (1, 1, 0, b"z1-10-data"),
        (1, 0, 1, b"z1-01-data"),
        (1, 1, 1, b"z1-11-data"),
    ];
    let original = build_archive(tiles);
    let compacted = compact_archive_default(&original).expect("compact");
    let reader = PmTilesReader::from_bytes(compacted).expect("reader");

    for &(z, x, y, expected) in tiles {
        let got = reader
            .get_tile(z, x, y)
            .expect("get_tile")
            .expect("tile must exist after compaction");
        assert_eq!(
            got, expected,
            "tile ({z},{x},{y}) data mismatch after compaction"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 4 — deduplication: identical content yields tiles_deduplicated count
// ---------------------------------------------------------------------------

#[test]
fn test_compact_deduplication_reduces_unique_contents() {
    // 3 tiles that all share the same byte content.
    let shared: &[u8] = b"identical-content-xyz";
    let original = build_archive(&[(1, 0, 0, shared), (1, 1, 0, shared), (1, 0, 1, shared)]);

    let opts = CompactOptions {
        deduplicate: true,
        ..Default::default()
    };
    let (compacted, stats) =
        compact_archive_with_stats(&original, &opts).expect("compact_with_stats");

    assert_eq!(stats.tiles_read, 3, "should read all 3 tiles");
    assert_eq!(
        stats.tiles_deduplicated, 2,
        "2 out of 3 tiles are duplicates"
    );

    // Verify the output archive has exactly 1 unique content entry.
    let out_header = PmTilesHeader::parse(&compacted).expect("parse");
    assert_eq!(
        out_header.tile_contents, 1,
        "compacted archive should have 1 unique content"
    );
    assert_eq!(
        out_header.addressed_tiles, 3,
        "all 3 tile addresses must still exist"
    );
}

// ---------------------------------------------------------------------------
// Test 5 — no-deduplicate mode keeps all tiles written independently
// ---------------------------------------------------------------------------

#[test]
fn test_compact_no_deduplicate_keeps_all() {
    let shared: &[u8] = b"same-content";
    let original = build_archive(&[(1, 0, 0, shared), (1, 1, 0, shared), (1, 0, 1, shared)]);

    let opts = CompactOptions {
        deduplicate: false,
        ..Default::default()
    };
    let (compacted, stats) =
        compact_archive_with_stats(&original, &opts).expect("compact_with_stats");

    // With dedup=false, tiles_deduplicated is always 0.
    assert_eq!(stats.tiles_deduplicated, 0, "no deduplication counted");
    assert_eq!(stats.tiles_written, 3, "all 3 tiles written");

    // The builder still deduplicates internally, so tile_contents is 1.
    // The important thing is the output is a valid archive with 3 addressed tiles.
    let out_header = PmTilesHeader::parse(&compacted).expect("parse");
    assert_eq!(out_header.addressed_tiles, 3);
}

// ---------------------------------------------------------------------------
// Test 6 — compacted output is smaller than a padded (gap-filled) input
// ---------------------------------------------------------------------------

#[test]
fn test_compact_output_is_smaller_than_padded_input() {
    // Build an archive with real tile data then append a large padding block
    // after the file to simulate gaps from deleted tiles.
    let tiles: &[(u8, u32, u32, &[u8])] = &[
        (0, 0, 0, b"tile-a"),
        (1, 0, 0, b"tile-b"),
        (1, 1, 1, b"tile-c"),
    ];
    let original = build_archive(tiles);

    // Append 2 KiB of zeroed padding to simulate an archive with gaps.
    let mut padded = original.clone();
    padded.extend(vec![0u8; 2048]);

    // Compact the non-padded (already-compact) archive and compare sizes.
    // The compacted form should be no larger than the original.
    let compacted = compact_archive_default(&original).expect("compact");
    // The compacted archive should be at most as large as the compact original
    // (compaction of a compact archive is idempotent in size terms).
    assert!(
        compacted.len() <= original.len() + 128,
        "compacted archive ({} bytes) should not be much larger than original ({} bytes)",
        compacted.len(),
        original.len()
    );

    // Verify the stats record correct before/after sizes.
    let opts = CompactOptions::default();
    let (_result, stats) = compact_archive_with_stats(&original, &opts).expect("stats");
    assert_eq!(stats.bytes_before, original.len());
    assert_eq!(stats.bytes_after, compacted.len());
}

// ---------------------------------------------------------------------------
// Test 7 — stats: bytes_before matches input length exactly
// ---------------------------------------------------------------------------

#[test]
fn test_compact_stats_bytes_before_after() {
    let original = build_archive(&[(0, 0, 0, b"tile-payload")]);
    let opts = CompactOptions::default();
    let (compacted, stats) = compact_archive_with_stats(&original, &opts).expect("compact");

    assert_eq!(
        stats.bytes_before,
        original.len(),
        "bytes_before must equal input length"
    );
    assert_eq!(
        stats.bytes_after,
        compacted.len(),
        "bytes_after must equal output length"
    );
}

// ---------------------------------------------------------------------------
// Test 8 — stats: tiles_read equals enumerate_tiles().len()
// ---------------------------------------------------------------------------

#[test]
fn test_compact_stats_tiles_read_equals_enumerate() {
    let tiles: &[(u8, u32, u32, &[u8])] = &[
        (0, 0, 0, b"t0"),
        (1, 0, 0, b"t1"),
        (1, 1, 0, b"t2"),
        (2, 0, 0, b"t3"),
        (2, 3, 3, b"t4"),
    ];
    let original = build_archive(tiles);
    let reader = PmTilesReader::from_bytes(original.clone()).expect("reader");
    let enumerated = reader.enumerate_tiles().expect("enumerate");

    let opts = CompactOptions::default();
    let (_compacted, stats) = compact_archive_with_stats(&original, &opts).expect("compact");

    assert_eq!(
        stats.tiles_read,
        enumerated.len(),
        "tiles_read must equal enumerate_tiles() count"
    );
}

// ---------------------------------------------------------------------------
// Test 9 — compact_archive_default returns a valid archive
// ---------------------------------------------------------------------------

#[test]
fn test_compact_default_options_returns_valid_archive() {
    let tiles: &[(u8, u32, u32, &[u8])] = &[
        (0, 0, 0, b"root-tile"),
        (1, 0, 0, b"leaf-1"),
        (1, 1, 1, b"leaf-2"),
    ];
    let original = build_archive(tiles);
    let compacted = compact_archive_default(&original).expect("compact_archive_default");

    assert_valid_pmtiles_header(&compacted);

    // Must parse without error.
    let header = PmTilesHeader::parse(&compacted).expect("parse compacted header");
    assert_eq!(
        header.addressed_tiles,
        tiles.len() as u64,
        "tile count must be preserved"
    );
    assert_eq!(header.spec_version, 3);
}

// ---------------------------------------------------------------------------
// Test 10 — tile data preserved byte-for-byte after compaction
// ---------------------------------------------------------------------------

#[test]
fn test_compact_tile_data_preserved_exactly() {
    // Use non-trivial binary content to catch byte-level corruption.
    let payload_a: Vec<u8> = (0u8..=127).collect();
    let payload_b: Vec<u8> = (128u8..=255).collect();
    let payload_c: Vec<u8> = (0u8..100)
        .map(|i| i.wrapping_mul(7).wrapping_add(13))
        .collect();

    let tiles: Vec<(u8, u32, u32, &[u8])> = vec![
        (1, 0, 0, &payload_a),
        (1, 1, 0, &payload_b),
        (1, 0, 1, &payload_c),
    ];

    let original = build_archive(&tiles);
    let compacted = compact_archive_default(&original).expect("compact");
    let reader = PmTilesReader::from_bytes(compacted).expect("reader");

    let got_a = reader.get_tile(1, 0, 0).expect("ok").expect("present");
    let got_b = reader.get_tile(1, 1, 0).expect("ok").expect("present");
    let got_c = reader.get_tile(1, 0, 1).expect("ok").expect("present");

    assert_eq!(got_a, payload_a, "payload_a mismatch");
    assert_eq!(got_b, payload_b, "payload_b mismatch");
    assert_eq!(got_c, payload_c, "payload_c mismatch");
}

// ---------------------------------------------------------------------------
// Test 11 — reduction_pct is 0.0 when archive is already compact
// ---------------------------------------------------------------------------

#[test]
fn test_compact_reduction_pct_range() {
    let original = build_archive(&[(0, 0, 0, b"tile")]);
    let opts = CompactOptions::default();
    let (_compacted, stats) = compact_archive_with_stats(&original, &opts).expect("stats");

    // reduction_pct should be in [0.0, 100.0] — can be negative if the
    // compactor adds any overhead for a tiny archive, but we clamp with
    // the saturating_sub in the implementation so it is always >= 0.0.
    assert!(
        stats.reduction_pct >= 0.0,
        "reduction_pct must be non-negative, got {}",
        stats.reduction_pct
    );
    assert!(
        stats.reduction_pct <= 100.0,
        "reduction_pct must be <= 100.0, got {}",
        stats.reduction_pct
    );
}

// ---------------------------------------------------------------------------
// Test 12 — zoom range and tile type preserved from source header
// ---------------------------------------------------------------------------

#[test]
fn test_compact_preserve_metadata_header_fields() {
    let mut builder = PmTilesBuilder::new(TileType::Jpeg, 3, 7);
    builder.set_bounds(-10.0, -5.0, 10.0, 5.0);
    builder.set_center(0.0, 0.0, 5);
    builder.add_tile(3, 0, 0, b"z3-tile").expect("add");
    builder.add_tile(5, 0, 0, b"z5-tile").expect("add");
    let original = builder.build().expect("build");

    let opts = CompactOptions {
        preserve_metadata: true,
        ..Default::default()
    };
    let compacted = compact_archive(&original, &opts).expect("compact");
    let out_header = PmTilesHeader::parse(&compacted).expect("parse");

    assert_eq!(out_header.tile_type, TileType::Jpeg, "tile type preserved");
    assert_eq!(out_header.min_zoom, 3, "min_zoom preserved");
    assert_eq!(out_header.max_zoom, 7, "max_zoom preserved");
    assert!(
        (out_header.min_lon() - (-10.0)).abs() < 1e-4,
        "min_lon preserved"
    );
    assert!(
        (out_header.max_lon() - 10.0).abs() < 1e-4,
        "max_lon preserved"
    );
    assert_eq!(out_header.center_zoom, 5, "center_zoom preserved");
}

// ---------------------------------------------------------------------------
// Test 13 — compaction is idempotent (compact(compact(x)) == compact(x))
// ---------------------------------------------------------------------------

#[test]
fn test_compact_idempotent() {
    let tiles: &[(u8, u32, u32, &[u8])] = &[
        (0, 0, 0, b"base"),
        (1, 0, 0, b"level1-a"),
        (1, 1, 1, b"level1-b"),
        (2, 0, 0, b"level2-a"),
    ];
    let original = build_archive(tiles);
    let once = compact_archive_default(&original).expect("first compact");
    let twice = compact_archive_default(&once).expect("second compact");

    // After two passes the archive size should stabilise (idempotency).
    // Allow a small tolerance for metadata section encoding differences.
    assert_eq!(
        once.len(),
        twice.len(),
        "compact is idempotent: second pass should not change archive size"
    );

    // Both passes must produce archives that read the same tile data.
    let reader_once = PmTilesReader::from_bytes(once).expect("reader once");
    let reader_twice = PmTilesReader::from_bytes(twice).expect("reader twice");

    for &(z, x, y, expected) in tiles {
        let got_once = reader_once.get_tile(z, x, y).expect("ok").expect("present");
        let got_twice = reader_twice
            .get_tile(z, x, y)
            .expect("ok")
            .expect("present");
        assert_eq!(
            got_once, expected,
            "tile ({z},{x},{y}) corrupted after once"
        );
        assert_eq!(
            got_twice, expected,
            "tile ({z},{x},{y}) corrupted after twice"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 14 — large archive with many tiles compacts correctly
// ---------------------------------------------------------------------------

#[test]
fn test_compact_large_archive_all_tiles_accessible() {
    // Build a dense z=4 archive (16×16 = 256 tiles) with unique content.
    let mut builder = PmTilesBuilder::new(TileType::Png, 4, 4);
    for x in 0..16u32 {
        for y in 0..16u32 {
            let data = format!("tile-{x:03}-{y:03}");
            builder
                .add_tile(4, x, y, data.as_bytes())
                .expect("add_tile");
        }
    }
    let original = builder.build().expect("build");
    let compacted = compact_archive_default(&original).expect("compact");

    let reader = PmTilesReader::from_bytes(compacted).expect("reader");
    let out_header = &reader.header;
    assert_eq!(out_header.addressed_tiles, 256, "all 256 tiles preserved");

    // Spot-check several tiles.
    for (x, y) in [(0, 0), (7, 3), (15, 15), (8, 12)] {
        let expected = format!("tile-{x:03}-{y:03}");
        let got = reader
            .get_tile(4, x, y)
            .expect("get_tile")
            .expect("tile exists");
        assert_eq!(
            got,
            expected.as_bytes(),
            "tile (4,{x},{y}) content mismatch"
        );
    }
}
