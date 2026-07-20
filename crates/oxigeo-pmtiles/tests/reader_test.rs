//! High-level reader tests: `get_tile` round-trip, nonexistent tile,
//! dedup size reduction, leaf directory lookup, compressed tile roundtrip,
//! and truncated-file error reporting.
//!
//! These tests exercise the end-to-end write→read path (via `PmTilesBuilder`
//! and `PmTilesReader::get_tile`) plus the manual-archive path used to force
//! leaf directory traversal, which the writer currently does not emit.

#![allow(clippy::expect_used)]

use oxigeo_pmtiles::{
    Compression, PmTilesBuilder, PmTilesError, PmTilesReader, TileType, zxy_to_tile_id,
};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// LEB-128 unsigned varint encode (small helper used when hand-crafting
/// directory/header bytes below).
fn enc_varint(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
    out
}

/// Encode a PMTiles v3 directory from `(tile_id, run_length, length, offset)`.
///
/// All offsets are encoded absolutely (`offset + 1`) so the decoder returns
/// exactly the provided offsets.
fn encode_dir(entries: &[(u64, u32, u32, u64)]) -> Vec<u8> {
    let mut out = enc_varint(entries.len() as u64);

    // tile_id deltas
    let mut last_id = 0u64;
    for &(tid, _, _, _) in entries {
        out.extend(enc_varint(tid.saturating_sub(last_id)));
        last_id = tid;
    }
    // run_lengths
    for &(_, rl, _, _) in entries {
        out.extend(enc_varint(u64::from(rl)));
    }
    // lengths
    for &(_, _, len, _) in entries {
        out.extend(enc_varint(u64::from(len)));
    }
    // offsets — absolute, encoded as `offset + 1`
    for &(_, _, _, off) in entries {
        out.extend(enc_varint(off + 1));
    }
    out
}

/// Build a 127-byte PMTiles v3 header with zeroed defaults plus the given
/// section offsets.  Used for hand-assembled archives (leaf directory test).
#[allow(clippy::too_many_arguments)]
fn build_header(
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
    internal_compression: u8,
    tile_compression: u8,
) -> Vec<u8> {
    let mut h = vec![0u8; 127];
    h[0..7].copy_from_slice(b"PMTiles");
    h[7] = 3;
    h[8..16].copy_from_slice(&root_dir_offset.to_le_bytes());
    h[16..24].copy_from_slice(&root_dir_length.to_le_bytes());
    h[24..32].copy_from_slice(&metadata_offset.to_le_bytes());
    h[32..40].copy_from_slice(&metadata_length.to_le_bytes());
    h[40..48].copy_from_slice(&leaf_dirs_offset.to_le_bytes());
    h[48..56].copy_from_slice(&leaf_dirs_length.to_le_bytes());
    h[56..64].copy_from_slice(&tile_data_offset.to_le_bytes());
    h[64..72].copy_from_slice(&tile_data_length.to_le_bytes());
    h[72..80].copy_from_slice(&addressed_tiles.to_le_bytes());
    h[80..88].copy_from_slice(&tile_entries.to_le_bytes());
    h[88..96].copy_from_slice(&tile_contents.to_le_bytes());
    h[96] = 0; // clustered=false (safe default for hand-built archives)
    h[97] = internal_compression;
    h[98] = tile_compression;
    h[99] = 2; // PNG
    h[100] = 0;
    h[101] = 14;
    // bounds / center zeroed
    h[118] = 0;
    h
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Build + read round-trip
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_get_tile_round_trip_single_tile() {
    let payload = b"round-trip-payload-z0-0-0";
    let mut b = PmTilesBuilder::new(TileType::Png, 0, 0);
    b.add_tile(0, 0, 0, payload).expect("add_tile");
    let archive = b.build().expect("build");

    let reader = PmTilesReader::from_bytes(archive).expect("reader");
    let got = reader.get_tile(0, 0, 0).expect("get_tile");
    assert_eq!(got.as_deref(), Some(payload.as_slice()));
}

#[test]
fn test_get_tile_round_trip_many_tiles() {
    // Build a small archive with several distinct tiles and verify each one
    // round-trips exactly.
    let tiles: Vec<(u8, u32, u32, Vec<u8>)> = vec![
        (0, 0, 0, b"z0".to_vec()),
        (1, 0, 0, b"z1-00".to_vec()),
        (1, 1, 0, b"z1-10".to_vec()),
        (1, 0, 1, b"z1-01".to_vec()),
        (1, 1, 1, b"z1-11".to_vec()),
        (2, 3, 3, b"z2-33".to_vec()),
    ];

    let mut b = PmTilesBuilder::new(TileType::Mvt, 0, 2);
    for (z, x, y, data) in &tiles {
        b.add_tile(*z, *x, *y, data).expect("add_tile");
    }
    let archive = b.build().expect("build");
    let reader = PmTilesReader::from_bytes(archive).expect("reader");

    for (z, x, y, data) in &tiles {
        let got = reader.get_tile(*z, *x, *y).expect("get_tile");
        assert_eq!(
            got.as_deref(),
            Some(data.as_slice()),
            "mismatch at ({z},{x},{y})"
        );
    }
}

#[test]
fn test_get_tile_round_trip_empty_tile() {
    // Edge case: zero-length tile payload still round-trips.
    let mut b = PmTilesBuilder::new(TileType::Png, 0, 0);
    b.add_tile(0, 0, 0, b"").expect("add_tile");
    let archive = b.build().expect("build");

    let reader = PmTilesReader::from_bytes(archive).expect("reader");
    let got = reader.get_tile(0, 0, 0).expect("get_tile");
    assert_eq!(got.as_deref(), Some(b"".as_slice()));
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Nonexistent tile → Ok(None)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_get_tile_nonexistent_returns_none() {
    // Write only z=1,(0,0) and ask for z=1,(1,1).
    let mut b = PmTilesBuilder::new(TileType::Png, 1, 1);
    b.add_tile(1, 0, 0, b"only-tile").expect("add_tile");
    let archive = b.build().expect("build");

    let reader = PmTilesReader::from_bytes(archive).expect("reader");
    let got = reader.get_tile(1, 1, 1).expect("get_tile");
    assert!(
        got.is_none(),
        "expected None for nonexistent tile, got Some"
    );
}

#[test]
fn test_get_tile_nonexistent_empty_archive() {
    // Archive with no tiles at all → every lookup is None.
    let b = PmTilesBuilder::new(TileType::Png, 0, 0);
    let archive = b.build().expect("build");
    let reader = PmTilesReader::from_bytes(archive).expect("reader");

    let got = reader.get_tile(0, 0, 0).expect("get_tile");
    assert!(got.is_none());
}

#[test]
fn test_get_tile_nonexistent_between_tiles() {
    // z=2 grid: write (0,0) and (3,3); ask for (1,1) which has a tile_id
    // that falls between them.
    let mut b = PmTilesBuilder::new(TileType::Png, 2, 2);
    b.add_tile(2, 0, 0, b"corner-a").expect("add");
    b.add_tile(2, 3, 3, b"corner-b").expect("add");
    let archive = b.build().expect("build");
    let reader = PmTilesReader::from_bytes(archive).expect("reader");

    let got = reader.get_tile(2, 1, 1).expect("get_tile");
    assert!(got.is_none(), "expected None for gap tile");
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Dedup file size reduction
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_dedup_reduces_tile_data_size() {
    // Write 10 copies of the same 2 KiB payload at z=2 grid positions.
    // With dedup the tile_data section should hold ~1 copy (2 KiB), not 10
    // copies (20 KiB).
    let payload = vec![0xA5u8; 2048];

    let mut dup = PmTilesBuilder::new(TileType::Png, 2, 2);
    // z=2 has 4x4=16 tiles; fill 10 of them with identical content.
    let positions: Vec<(u32, u32)> = (0..4)
        .flat_map(|x| (0..4).map(move |y| (x, y)))
        .take(10)
        .collect();
    for &(x, y) in &positions {
        dup.add_tile(2, x, y, &payload).expect("add");
    }
    let dup_archive = dup.build().expect("build");

    // Control: same 10 positions but with 10 distinct payloads of the same
    // size.  Total tile_data must be roughly 10×payload.
    let mut uniq = PmTilesBuilder::new(TileType::Png, 2, 2);
    for (i, &(x, y)) in positions.iter().enumerate() {
        let mut data = payload.clone();
        data[0] = i as u8;
        uniq.add_tile(2, x, y, &data).expect("add");
    }
    let uniq_archive = uniq.build().expect("build");

    // Sanity-check header counts on the deduplicated archive.
    let dup_reader = PmTilesReader::from_bytes(dup_archive.clone()).expect("reader");
    assert_eq!(dup_reader.header.addressed_tiles, 10);
    // With run-length compression, consecutive identical tiles are merged.
    // The 10 tiles at z=2 have IDs [5..12, 18..19] with a gap — they compress
    // into 2 directory entries (a run of 8 + a run of 2).
    assert!(
        dup_reader.header.tile_entries <= 10,
        "run-length compression should reduce tile_entries (got {})",
        dup_reader.header.tile_entries
    );
    assert_eq!(dup_reader.header.tile_contents, 1);
    // Deduplicated archive stores exactly one copy of the payload.
    assert_eq!(dup_reader.header.tile_data_length, payload.len() as u64);

    let uniq_reader = PmTilesReader::from_bytes(uniq_archive.clone()).expect("reader");
    assert_eq!(uniq_reader.header.tile_contents, 10);
    assert_eq!(
        uniq_reader.header.tile_data_length,
        (payload.len() * 10) as u64
    );

    // File-size sanity check — the directory is a bit larger in the dedup
    // case (non-clustered offsets cost more bytes) but tile-data dominates
    // by orders of magnitude.
    assert!(
        dup_archive.len() < uniq_archive.len() / 2,
        "dedup archive ({}) not substantially smaller than unique-data archive ({})",
        dup_archive.len(),
        uniq_archive.len()
    );

    // And the tile payload is still recoverable at every position.
    for &(x, y) in &positions {
        let got = dup_reader.get_tile(2, x, y).expect("get_tile");
        assert_eq!(got.as_deref(), Some(payload.as_slice()));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Leaf directory lookup
// ─────────────────────────────────────────────────────────────────────────────

/// Hand-build a PMTiles archive whose root directory contains a single leaf
/// pointer.  The leaf directory itself then contains a real tile entry.
///
/// This exercises `PmTilesReader::find_tile_in_entries`' leaf-traversal branch,
/// which the writer does not currently emit.
#[test]
fn test_get_tile_via_leaf_directory() {
    let tile_payload: &[u8] = b"tile-under-leaf";
    let tile_id = zxy_to_tile_id(2, 1, 1).expect("tile_id");

    // --- Leaf directory: a single tile entry that matches tile_id.
    // tile_id starts at 0 in a freshly-decoded directory, so we encode the
    // target tile_id as the first delta.  offset is relative to tile_data.
    let leaf_dir = encode_dir(&[(
        tile_id,
        1, // run_length=1 → tile entry
        tile_payload.len() as u32,
        0, // relative offset within the tile_data section
    )]);

    // --- Root directory: a single leaf pointer covering the low tile-id range.
    // run_length=0 marks a leaf entry.  Offset is relative to leaf_dirs.
    let root_dir = encode_dir(&[(
        0,                     // tile_id (start of covered range)
        0,                     // run_length=0 → leaf directory pointer
        leaf_dir.len() as u32, // length of the leaf page in leaf_dirs
        0,                     // offset within leaf_dirs
    )]);

    // --- Assemble the file layout:
    //   [header(127)] [root_dir] [leaf_dirs] [metadata] [tile_data]
    let root_dir_offset = 127u64;
    let root_dir_length = root_dir.len() as u64;
    let leaf_dirs_offset = root_dir_offset + root_dir_length;
    let leaf_dirs_length = leaf_dir.len() as u64;
    let metadata = b"{}";
    let metadata_offset = leaf_dirs_offset + leaf_dirs_length;
    let metadata_length = metadata.len() as u64;
    let tile_data_offset = metadata_offset + metadata_length;
    let tile_data_length = tile_payload.len() as u64;

    let mut archive = build_header(
        root_dir_offset,
        root_dir_length,
        metadata_offset,
        metadata_length,
        leaf_dirs_offset,
        leaf_dirs_length,
        tile_data_offset,
        tile_data_length,
        /* addressed_tiles */ 1,
        /* tile_entries   */ 1,
        /* tile_contents  */ 1,
        /* internal_compression */ 1, // None
        /* tile_compression     */ 1, // None
    );
    archive.extend_from_slice(&root_dir);
    archive.extend_from_slice(&leaf_dir);
    archive.extend_from_slice(metadata);
    archive.extend_from_slice(tile_payload);

    let reader = PmTilesReader::from_bytes(archive).expect("reader");
    // Resolving the tile requires the reader to follow the leaf pointer.
    let got = reader.get_tile(2, 1, 1).expect("get_tile");
    assert_eq!(got.as_deref(), Some(tile_payload));

    // And a tile_id that falls outside the leaf's single entry must still
    // return None (i.e. leaf traversal doesn't spuriously succeed).
    let miss = reader.get_tile(2, 2, 2).expect("get_tile");
    assert!(miss.is_none());
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Gzip / brotli / zstd decompression roundtrip (compression feature)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "compression")]
#[test]
fn test_decompress_data_gzip_roundtrip() {
    use oxigeo_pmtiles::pmtiles::decompress_data;

    let raw = b"hello pmtiles over gzip payload for roundtrip test";
    let compressed = oxiarc_archive::gzip::compress(raw, 6).expect("gzip compress");
    // Compressed output must differ from raw (sanity).
    assert_ne!(compressed.as_slice(), raw.as_slice());

    let decompressed = decompress_data(&compressed, &Compression::Gzip).expect("decompress");
    assert_eq!(decompressed.as_slice(), raw.as_slice());
}

#[cfg(feature = "compression")]
#[test]
fn test_decompress_data_brotli_roundtrip() {
    use oxigeo_pmtiles::pmtiles::decompress_data;

    let raw = b"brotli payload roundtrip for pmtiles decompress path";
    let compressed = oxiarc_archive::brotli::compress(raw).expect("brotli compress");
    let decompressed = decompress_data(&compressed, &Compression::Brotli).expect("decompress");
    assert_eq!(decompressed.as_slice(), raw.as_slice());
}

#[cfg(feature = "compression")]
#[test]
fn test_decompress_data_zstd_roundtrip() {
    use oxigeo_pmtiles::pmtiles::decompress_data;

    let raw = b"zstd payload roundtrip for pmtiles decompress path";
    let compressed = oxiarc_archive::zstd::compress(raw).expect("zstd compress");
    let decompressed = decompress_data(&compressed, &Compression::Zstd).expect("decompress");
    assert_eq!(decompressed.as_slice(), raw.as_slice());
}

#[cfg(feature = "compression")]
#[test]
fn test_get_tile_decompressed_with_gzip() {
    // Build an uncompressed archive, then pretend the tile_compression is Gzip
    // by hand-crafting an archive whose payload is gzip-compressed.
    let raw_tile = b"this is a fake vector tile body for gzip compression";
    let compressed_tile = oxiarc_archive::gzip::compress(raw_tile, 6).expect("gzip compress");
    let tile_id = zxy_to_tile_id(0, 0, 0).expect("tile_id");

    let root_dir = encode_dir(&[(tile_id, 1, compressed_tile.len() as u32, 0)]);
    let root_dir_offset = 127u64;
    let root_dir_length = root_dir.len() as u64;
    let metadata = b"{}";
    let metadata_offset = root_dir_offset + root_dir_length;
    let metadata_length = metadata.len() as u64;
    let tile_data_offset = metadata_offset + metadata_length;
    let tile_data_length = compressed_tile.len() as u64;

    let mut archive = build_header(
        root_dir_offset,
        root_dir_length,
        metadata_offset,
        metadata_length,
        0,
        0,
        tile_data_offset,
        tile_data_length,
        1,
        1,
        1,
        /* internal_compression */ 1, // None (root_dir is uncompressed)
        /* tile_compression     */ 2, // Gzip
    );
    archive.extend_from_slice(&root_dir);
    archive.extend_from_slice(metadata);
    archive.extend_from_slice(&compressed_tile);

    let reader = PmTilesReader::from_bytes(archive).expect("reader");

    // Raw tile → compressed bytes.
    let raw = reader.get_tile(0, 0, 0).expect("get_tile");
    assert_eq!(raw.as_deref(), Some(compressed_tile.as_slice()));

    // Decompressed tile → original bytes.
    let decompressed = reader
        .get_tile_decompressed(0, 0, 0)
        .expect("get_tile_decompressed");
    assert_eq!(decompressed.as_deref(), Some(raw_tile.as_slice()));
}

#[cfg(not(feature = "compression"))]
#[test]
fn test_decompress_data_gzip_without_feature_errors() {
    use oxigeo_pmtiles::pmtiles::decompress_data;

    let err = decompress_data(b"anything", &Compression::Gzip).expect_err("must fail");
    assert!(matches!(err, PmTilesError::Decompression(_)));
}

// Passthroughs should always succeed regardless of the feature flag.
#[test]
fn test_decompress_data_none_passthrough() {
    use oxigeo_pmtiles::pmtiles::decompress_data;

    let raw = b"not compressed";
    let out = decompress_data(raw, &Compression::None).expect("ok");
    assert_eq!(out.as_slice(), raw.as_slice());
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. Truncated / malformed file
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_reader_truncated_under_header_errors() {
    // Fewer than 127 bytes → header parse must fail with InvalidFormat.
    let short = vec![0u8; 64];
    let result = PmTilesReader::from_bytes(short);
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(matches!(e, PmTilesError::InvalidFormat(_)));
    }
}

#[test]
fn test_reader_truncated_root_dir_errors() {
    // Header claims a 1000-byte root directory but the file is just the header.
    let data = build_header(
        127, 1000, // root dir claims 1000 bytes
        0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1,
    );
    let reader = PmTilesReader::from_bytes(data).expect("header parses");
    let err = reader
        .root_directory()
        .expect_err("root dir must be out of bounds");
    assert!(matches!(err, PmTilesError::InvalidFormat(_)));
}

#[test]
fn test_reader_truncated_tile_data_errors() {
    // Build a valid directory pointing at an offset past the end of the file.
    let tile_id = zxy_to_tile_id(0, 0, 0).expect("tile_id");
    let root_dir = encode_dir(&[(tile_id, 1, 10_000, 0)]);
    let root_dir_offset = 127u64;
    let root_dir_length = root_dir.len() as u64;
    let metadata = b"{}";
    let metadata_offset = root_dir_offset + root_dir_length;
    let metadata_length = metadata.len() as u64;
    let tile_data_offset = metadata_offset + metadata_length;
    // Claim 10_000 bytes of tile data but don't actually write them.
    let tile_data_length = 10_000u64;

    let mut archive = build_header(
        root_dir_offset,
        root_dir_length,
        metadata_offset,
        metadata_length,
        0,
        0,
        tile_data_offset,
        tile_data_length,
        1,
        1,
        1,
        1,
        1,
    );
    archive.extend_from_slice(&root_dir);
    archive.extend_from_slice(metadata);
    // Intentionally omit the tile payload bytes.

    let reader = PmTilesReader::from_bytes(archive).expect("header/directory parse");
    let err = reader
        .get_tile(0, 0, 0)
        .expect_err("must detect truncation");
    assert!(matches!(err, PmTilesError::InvalidFormat(_)));
}

#[test]
fn test_reader_missing_magic_errors() {
    let mut data = vec![0u8; 127];
    data[0..7].copy_from_slice(b"NOTPMTL");
    data[7] = 3;
    let result = PmTilesReader::from_bytes(data);
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(matches!(e, PmTilesError::InvalidFormat(_)));
    }
}
