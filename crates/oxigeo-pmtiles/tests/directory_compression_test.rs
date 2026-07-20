//! Integration tests for PMTiles directory + metadata internal compression.
//!
//! Slice 15 / W3 — verify that when [`PmTilesBuilder::set_internal_compression`]
//! is called with a non-`None` algorithm, the writer actually compresses the
//! root directory, every leaf directory, and the JSON metadata block before
//! emitting them into the archive; and that [`PmTilesReader`] transparently
//! decompresses each region on load.
//!
//! Every test in this file is gated behind the `compression` cargo feature
//! because the feature is what wires the OxiARC codec dispatch into both
//! sides of the write/read pipeline.

#![cfg(feature = "compression")]
#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use oxigeo_pmtiles::{Compression, PmTilesBuilder, PmTilesHeader, PmTilesReader, TileType};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a small archive with `tile_count` distinct tiles, all at zoom 0..=7,
/// using the supplied `internal_compression`.
///
/// Tiles are placed in Hilbert order along the z=7 row at y=0 so that the
/// tile-ID column is densely populated.  Each tile payload is 64 bytes of a
/// constant byte derived from the tile index to keep dedup from collapsing
/// the directory to a single run-length entry.
fn build_archive_with_compression(c: Compression, tile_count: u32) -> Vec<u8> {
    let mut builder = PmTilesBuilder::new(TileType::Mvt, 0, 7);
    builder.set_internal_compression(c);
    // Distinct payloads so the directory has `tile_count` real entries.
    for i in 0..tile_count {
        let payload = vec![(i & 0xff) as u8; 64];
        builder
            .add_tile_by_id(u64::from(i), &payload)
            .expect("add_tile_by_id");
    }
    builder.build().expect("build")
}

/// Build a large archive deliberately big enough to force the writer's
/// leaf-directory split logic (root encoding exceeds ~16 kB).
///
/// We use unique 32-byte payloads per tile so the directory has one entry per
/// tile (no run-length collapsing), and the dedup map is largely useless.
fn build_archive_forcing_leaves(c: Compression, tile_count: u32) -> Vec<u8> {
    let mut builder = PmTilesBuilder::new(TileType::Mvt, 0, 14);
    builder.set_internal_compression(c);
    for i in 0..tile_count {
        // 4-byte unique header + 28 zero bytes — keeps payload size small but
        // ensures the FNV-1a hash differs per tile so no dedup occurs.
        let mut payload = vec![0u8; 32];
        payload[0..4].copy_from_slice(&i.to_le_bytes());
        builder
            .add_tile_by_id(u64::from(i), &payload)
            .expect("add_tile_by_id");
    }
    builder.build().expect("build")
}

/// Extract the raw bytes of the root directory section from a serialised
/// archive using the parsed header offsets.
fn root_dir_slice<'a>(archive: &'a [u8], header: &PmTilesHeader) -> &'a [u8] {
    let start = header.root_dir_offset as usize;
    let end = start + header.root_dir_length as usize;
    &archive[start..end]
}

/// Extract the raw bytes of the metadata section from a serialised archive.
fn metadata_slice<'a>(archive: &'a [u8], header: &PmTilesHeader) -> &'a [u8] {
    let start = header.metadata_offset as usize;
    let end = start + header.metadata_length as usize;
    &archive[start..end]
}

// ---------------------------------------------------------------------------
// Test 1 — gzip writer sets header byte 97 and gzip-magic bytes appear
// ---------------------------------------------------------------------------

#[test]
fn test_writer_emits_compressed_root_dir_when_gzip_selected() {
    let archive = build_archive_with_compression(Compression::Gzip, 16);

    // Header byte 97 must record Gzip (raw value 2).
    assert_eq!(
        archive[97], 2,
        "header byte 97 must report Gzip after set_internal_compression(Gzip)"
    );

    let header = PmTilesHeader::parse(&archive).expect("parse header");
    assert_eq!(header.internal_compression, Compression::Gzip);

    // The root directory bytes must start with the gzip magic (0x1f, 0x8b).
    let root = root_dir_slice(&archive, &header);
    assert!(
        root.len() >= 2,
        "root directory should not be empty for 16 tiles"
    );
    assert_eq!(
        &root[0..2],
        &[0x1f, 0x8b],
        "root directory must start with gzip magic; first 4 bytes were {:02x?}",
        &root[..root.len().min(4)]
    );
}

// ---------------------------------------------------------------------------
// Test 2 — brotli writer sets header byte 97 to 3 and emits non-gzip bytes
// ---------------------------------------------------------------------------

#[test]
fn test_writer_emits_compressed_root_dir_when_brotli_selected() {
    let archive = build_archive_with_compression(Compression::Brotli, 16);

    assert_eq!(archive[97], 3, "header byte 97 must report Brotli");
    let header = PmTilesHeader::parse(&archive).expect("parse header");
    assert_eq!(header.internal_compression, Compression::Brotli);

    let root = root_dir_slice(&archive, &header);
    assert!(!root.is_empty());
    // Brotli has no fixed magic; instead verify the bytes do NOT match the
    // PMTiles varint-encoded directory of 16 entries.  Specifically the first
    // byte of an uncompressed directory of N entries is the varint of N
    // (=16=0x10), which is exactly what brotli output should not look like.
    // We assert the output round-trips via the reader (see test 6) — here we
    // just sanity-check that the bytes differ from the uncompressed encoding.
    let uncompressed = build_archive_with_compression(Compression::None, 16);
    let unc_header = PmTilesHeader::parse(&uncompressed).expect("parse none header");
    let unc_root = root_dir_slice(&uncompressed, &unc_header);
    assert_ne!(
        root, unc_root,
        "Brotli root directory bytes must differ from the uncompressed encoding"
    );
}

// ---------------------------------------------------------------------------
// Test 3 — zstd writer sets header byte 97 to 4 and emits zstd magic
// ---------------------------------------------------------------------------

#[test]
fn test_writer_emits_compressed_root_dir_when_zstd_selected() {
    let archive = build_archive_with_compression(Compression::Zstd, 16);

    assert_eq!(archive[97], 4, "header byte 97 must report Zstd");
    let header = PmTilesHeader::parse(&archive).expect("parse header");
    assert_eq!(header.internal_compression, Compression::Zstd);

    let root = root_dir_slice(&archive, &header);
    assert!(root.len() >= 4, "root directory should have a zstd frame");
    // Zstd magic number: 0x28 0xB5 0x2F 0xFD (little-endian on disk).
    assert_eq!(
        &root[0..4],
        &[0x28, 0xB5, 0x2F, 0xFD],
        "root directory must start with zstd magic; got {:02x?}",
        &root[..4]
    );
}

// ---------------------------------------------------------------------------
// Test 4 — Compression::None still produces an uncompressed directory
// ---------------------------------------------------------------------------

#[test]
fn test_writer_emits_uncompressed_root_dir_when_none_selected() {
    let archive = build_archive_with_compression(Compression::None, 16);

    // Header byte 97 is the spec value `1` for Compression::None.
    assert_eq!(archive[97], 1, "header byte 97 must report None");
    let header = PmTilesHeader::parse(&archive).expect("parse header");
    assert_eq!(header.internal_compression, Compression::None);

    let root = root_dir_slice(&archive, &header);
    // The first varint of an uncompressed directory is the entry count.
    // For 16 distinct tile IDs the count varint is 0x10 (16 < 128 so one byte).
    assert_eq!(
        root[0], 0x10,
        "uncompressed root directory must start with the entry-count varint (0x10)"
    );
    // It must NOT look like any of the compressed framings.
    assert_ne!(&root[0..2], &[0x1f, 0x8b], "must not be gzip");
    assert_ne!(
        &root.get(0..4).unwrap_or(&[0, 0, 0, 0]),
        &[0x28, 0xB5, 0x2F, 0xFD],
        "must not be zstd"
    );
}

// ---------------------------------------------------------------------------
// Test 5 — metadata JSON is also compressed when internal compression is on
// ---------------------------------------------------------------------------

#[test]
fn test_writer_emits_compressed_metadata_when_internal_compression_set() {
    // A larger metadata payload makes the size difference unambiguous.
    let big_json = format!(
        r#"{{"name":"slice15-w3","description":"{}","format":"pbf","extra":"{}"}}"#,
        "A".repeat(512),
        "B".repeat(512)
    );

    let mut builder_gz = PmTilesBuilder::new(TileType::Mvt, 0, 0);
    builder_gz.set_internal_compression(Compression::Gzip);
    builder_gz.set_metadata(big_json.clone());
    builder_gz
        .add_tile_by_id(0, b"tile-0")
        .expect("add_tile_by_id");
    let archive_gz = builder_gz.build().expect("build gz");
    let header_gz = PmTilesHeader::parse(&archive_gz).expect("parse gz header");

    // Metadata bytes must start with the gzip magic.
    let meta_gz = metadata_slice(&archive_gz, &header_gz);
    assert!(meta_gz.len() >= 2);
    assert_eq!(
        &meta_gz[0..2],
        &[0x1f, 0x8b],
        "metadata block must be gzip-compressed when internal compression is Gzip"
    );

    // The compressed metadata must be smaller than the JSON source (the JSON
    // contains long runs of 'A' and 'B' that gzip handles trivially).
    assert!(
        meta_gz.len() < big_json.len(),
        "compressed metadata ({} bytes) must be smaller than JSON source ({} bytes)",
        meta_gz.len(),
        big_json.len()
    );

    // The reader must transparently decode it back to the original JSON.
    let reader = PmTilesReader::from_bytes(archive_gz).expect("reader");
    let parsed = reader.metadata().expect("parse metadata");
    assert_eq!(parsed.name.as_deref(), Some("slice15-w3"));
    assert_eq!(parsed.format.as_deref(), Some("pbf"));
}

// ---------------------------------------------------------------------------
// Test 6 — reader decompresses gzip root dir and finds every tile back
// ---------------------------------------------------------------------------

#[test]
fn test_reader_decompresses_root_dir_when_header_says_gzip() {
    let tile_count: u32 = 32;
    let archive = build_archive_with_compression(Compression::Gzip, tile_count);

    let reader = PmTilesReader::from_bytes(archive).expect("reader");
    assert_eq!(reader.header.internal_compression, Compression::Gzip);

    // root_directory() must decompress and decode without error.
    let root_entries = reader.root_directory().expect("decode root");
    assert!(
        !root_entries.is_empty(),
        "decoded root directory must contain at least one entry"
    );

    // Every tile we wrote must be retrievable; the bytes must match exactly.
    let tiles = reader.enumerate_tiles().expect("enumerate");
    assert_eq!(tiles.len() as u32, tile_count);
    for info in &tiles {
        let raw = reader
            .get_tile(info.z, info.x, info.y)
            .expect("get_tile")
            .expect("tile must exist");
        let expected = vec![(info.tile_id & 0xff) as u8; 64];
        assert_eq!(raw, expected, "tile {} payload mismatch", info.tile_id);
    }
}

// ---------------------------------------------------------------------------
// Test 7 — reader decompresses zstd root dir and finds every tile back
// ---------------------------------------------------------------------------

#[test]
fn test_reader_decompresses_root_dir_when_header_says_zstd() {
    let tile_count: u32 = 24;
    let archive = build_archive_with_compression(Compression::Zstd, tile_count);

    let reader = PmTilesReader::from_bytes(archive).expect("reader");
    assert_eq!(reader.header.internal_compression, Compression::Zstd);

    let tiles = reader.enumerate_tiles().expect("enumerate");
    assert_eq!(tiles.len() as u32, tile_count);
    for info in &tiles {
        let raw = reader
            .get_tile(info.z, info.x, info.y)
            .expect("get_tile")
            .expect("tile must exist");
        let expected = vec![(info.tile_id & 0xff) as u8; 64];
        assert_eq!(raw, expected, "tile {} payload mismatch", info.tile_id);
    }
}

// ---------------------------------------------------------------------------
// Test 8 — full round trip of 100 tiles with gzip internal compression
// ---------------------------------------------------------------------------

#[test]
fn test_round_trip_archive_with_gzip_internal_compression_preserves_all_tiles() {
    let tile_count: u32 = 100;
    let archive = build_archive_with_compression(Compression::Gzip, tile_count);

    let reader = PmTilesReader::from_bytes(archive).expect("reader");
    assert_eq!(reader.header.internal_compression, Compression::Gzip);
    assert_eq!(reader.header.addressed_tiles, u64::from(tile_count));

    // Verify every tile round-trips losslessly.
    for i in 0..tile_count {
        let expected = vec![(i & 0xff) as u8; 64];
        let (z, x, y) = oxigeo_pmtiles::tile_id_to_zxy(u64::from(i)).expect("tile_id_to_zxy");
        let got = reader
            .get_tile(z, x, y)
            .expect("get_tile")
            .expect("tile must exist");
        assert_eq!(
            got, expected,
            "tile {i} (z={z} x={x} y={y}) payload mismatch"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 9 — large archive forces leaf creation; leaves must decompress too
// ---------------------------------------------------------------------------

#[test]
fn test_round_trip_archive_with_compressed_leaves_finds_tiles_at_high_zoom() {
    // A 16 384-tile distinct-payload archive serialises to a directory that
    // comfortably exceeds the 16 kB LEAF_SPLIT_THRESHOLD, forcing leaf
    // creation.  The encoder uses one varint each for tile-id delta, run
    // length, payload length and (clustered shorthand) offset — typically
    // 4 bytes per entry — so 16 384 entries occupy ~64 kB before any
    // compression and is guaranteed to overflow the 16 kB threshold even
    // when run-length collapsing is partially applied.
    let tile_count: u32 = 16_384;
    let archive = build_archive_forcing_leaves(Compression::Gzip, tile_count);

    let reader = PmTilesReader::from_bytes(archive).expect("reader");
    assert_eq!(reader.header.internal_compression, Compression::Gzip);
    assert!(
        reader.header.leaf_dirs_length > 0,
        "this test requires leaf directories to be present; got leaf_dirs_length=0"
    );

    // The reader must be able to traverse root → leaf → tile data.
    let tiles = reader.enumerate_tiles().expect("enumerate");
    assert_eq!(tiles.len() as u32, tile_count);

    // Spot-check the first, last, and a few middle tiles via the public
    // get_tile() path (which exercises leaf-directory decompression too).
    let sample_ids: [u32; 5] = [
        0,
        tile_count / 4,
        tile_count / 2,
        (tile_count / 4) * 3,
        tile_count - 1,
    ];
    for i in sample_ids {
        let (z, x, y) = oxigeo_pmtiles::tile_id_to_zxy(u64::from(i)).expect("tile_id_to_zxy");
        let got = reader
            .get_tile(z, x, y)
            .expect("get_tile")
            .expect("tile must exist");
        let mut expected = vec![0u8; 32];
        expected[0..4].copy_from_slice(&i.to_le_bytes());
        assert_eq!(got, expected, "tile {i} payload mismatch via leaf path");
    }
}

// ---------------------------------------------------------------------------
// Test 10 — gzip-compressed archive is smaller than uncompressed for a
//            repetitive (highly-compressible) tile layout.
// ---------------------------------------------------------------------------

#[test]
fn test_compressed_archive_smaller_than_uncompressed_for_repetitive_tile_layout() {
    // For the directory + metadata size to dominate the file size we need a
    // largeish directory of distinct tiles AND substantial JSON metadata.
    let big_json = format!(
        r#"{{"name":"repetitive","attribution":"{}","extra":"{}"}}"#,
        "X".repeat(2_048),
        "Y".repeat(2_048),
    );

    let tile_count: u32 = 4_000;

    let mut none_builder = PmTilesBuilder::new(TileType::Mvt, 0, 14);
    none_builder.set_internal_compression(Compression::None);
    none_builder.set_metadata(big_json.clone());
    for i in 0..tile_count {
        let mut payload = vec![0u8; 32];
        payload[0..4].copy_from_slice(&i.to_le_bytes());
        none_builder
            .add_tile_by_id(u64::from(i), &payload)
            .expect("add_tile_by_id");
    }
    let none_archive = none_builder.build().expect("build none");

    let mut gz_builder = PmTilesBuilder::new(TileType::Mvt, 0, 14);
    gz_builder.set_internal_compression(Compression::Gzip);
    gz_builder.set_metadata(big_json);
    for i in 0..tile_count {
        let mut payload = vec![0u8; 32];
        payload[0..4].copy_from_slice(&i.to_le_bytes());
        gz_builder
            .add_tile_by_id(u64::from(i), &payload)
            .expect("add_tile_by_id");
    }
    let gz_archive = gz_builder.build().expect("build gz");

    assert!(
        gz_archive.len() < none_archive.len(),
        "gzip archive ({} bytes) must be smaller than uncompressed ({} bytes)",
        gz_archive.len(),
        none_archive.len()
    );

    // And the gzip archive must round-trip just as well.
    let reader = PmTilesReader::from_bytes(gz_archive).expect("reader");
    let tiles = reader.enumerate_tiles().expect("enumerate");
    assert_eq!(tiles.len() as u32, tile_count);
}
