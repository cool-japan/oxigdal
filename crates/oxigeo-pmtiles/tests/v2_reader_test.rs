//! Integration tests for the PMTiles v2 backward-compatibility reader.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use oxigeo_pmtiles::header::TileType;
use oxigeo_pmtiles::writer::PmTilesBuilder;
use oxigeo_pmtiles::{
    PMTILES_V2_MAGIC, PmTilesReader, PmTilesV2Reader, detect_pmtiles_version,
    hilbert::zxy_to_tile_id, parse_v2_header, read_v2_entry, zxy_to_v2_tile_id,
};

// ---------------------------------------------------------------------------
// Helper: build a synthetic PMTiles v2 archive in memory
// ---------------------------------------------------------------------------

/// Build a minimal, well-formed PMTiles v2 byte buffer containing the given
/// tiles.
///
/// Layout produced:
/// ```text
/// [magic 3B] [meta_len u16 LE] [metadata UTF-8]
/// [entry₀ 17B] … [entryN 17B]
/// [tile_data_blob₀] … [tile_data_blobN]
/// ```
///
/// Offsets stored in each entry point into the section that follows the
/// directory.
fn make_synthetic_v2(tiles: &[(u8, u32, u32, Vec<u8>)]) -> Vec<u8> {
    let meta = r#"{"format":"png","attribution":""}"#;
    let meta_bytes = meta.as_bytes();
    let meta_len = meta_bytes.len() as u16;

    let mut buf = Vec::new();

    // Header: magic + metadata length + metadata
    buf.extend_from_slice(b"PM\x02");
    buf.extend_from_slice(&meta_len.to_le_bytes());
    buf.extend_from_slice(meta_bytes);

    // The directory entries immediately follow the metadata, before tile data.
    // Compute where tile data will start (after all 17-byte entries).
    let entries_start = buf.len() + tiles.len() * 17;

    // Accumulate tile data blobs and record offsets.
    let mut tile_offsets: Vec<u64> = Vec::with_capacity(tiles.len());
    let mut tile_data: Vec<u8> = Vec::new();

    for (_, _, _, blob) in tiles {
        tile_offsets.push((entries_start + tile_data.len()) as u64);
        tile_data.extend_from_slice(blob);
    }

    // Write directory entries (each 17 bytes).
    for (i, (z, x, y, blob)) in tiles.iter().enumerate() {
        // z (1 byte)
        buf.push(*z);

        // x as u24 LE (3 bytes)
        let xb = x.to_le_bytes();
        buf.extend_from_slice(&xb[0..3]);

        // y as u24 LE (3 bytes)
        let yb = y.to_le_bytes();
        buf.extend_from_slice(&yb[0..3]);

        // offset as u48 LE (6 bytes)
        let off = tile_offsets[i].to_le_bytes();
        buf.extend_from_slice(&off[0..6]);

        // length as u32 LE (4 bytes)
        buf.extend_from_slice(&(blob.len() as u32).to_le_bytes());
    }

    // Append tile data after the directory.
    buf.extend_from_slice(&tile_data);

    buf
}

// ---------------------------------------------------------------------------
// detect_pmtiles_version
// ---------------------------------------------------------------------------

#[test]
fn test_detect_pmtiles_version_v2_magic() {
    // Any data starting with b"PM\x02" should be detected as version 2.
    let data = b"PM\x02\x00\x00some extra bytes";
    let version = detect_pmtiles_version(data).expect("detect ok");
    assert_eq!(version, 2);
}

#[test]
fn test_detect_pmtiles_version_v3_archive() {
    // Build a real v3 archive and verify detection.
    let builder = PmTilesBuilder::new(TileType::Png, 0, 0);
    let archive = builder.build().expect("build ok");
    let version = detect_pmtiles_version(&archive).expect("detect ok");
    assert_eq!(version, 3);
}

#[test]
fn test_detect_pmtiles_version_unknown_errors() {
    let result = detect_pmtiles_version(b"garbage_data_xyz");
    assert!(result.is_err(), "expected Err for unrecognised magic");
}

#[test]
fn test_detect_pmtiles_version_too_short_errors() {
    let result = detect_pmtiles_version(b"PM");
    assert!(result.is_err(), "expected Err when data is too short");
}

// ---------------------------------------------------------------------------
// parse_v2_header
// ---------------------------------------------------------------------------

#[test]
fn test_parse_v2_header_minimal_archive() {
    let archive = make_synthetic_v2(&[]);
    let header = parse_v2_header(&archive).expect("parse ok");
    assert_eq!(header.version, 2);
    assert!(header.metadata.contains("format"));
    assert!(header.root_entries.is_empty());
}

#[test]
fn test_parse_v2_header_rejects_short_data() {
    let result = parse_v2_header(&[1u8]);
    assert!(result.is_err(), "expected Err for 1-byte input");
}

#[test]
fn test_parse_v2_header_rejects_wrong_magic() {
    // Correct length, but magic is 'PM\x03' (version 3 byte, not 2).
    let mut data = b"PM\x03\x00\x00".to_vec();
    data.extend_from_slice(b"{}");
    let result = parse_v2_header(&data);
    assert!(result.is_err(), "expected Err for wrong magic/version");
}

#[test]
fn test_parse_v2_header_metadata_roundtrip() {
    let meta = r#"{"format":"png","minzoom":0,"maxzoom":14}"#;
    let mut buf = Vec::new();
    buf.extend_from_slice(b"PM\x02");
    buf.extend_from_slice(&(meta.len() as u16).to_le_bytes());
    buf.extend_from_slice(meta.as_bytes());
    let header = parse_v2_header(&buf).expect("parse ok");
    assert_eq!(header.metadata, meta);
}

// ---------------------------------------------------------------------------
// read_v2_entry
// ---------------------------------------------------------------------------

#[test]
fn test_parse_v2_entry_unpacks_17_bytes() {
    // Craft a 17-byte entry:
    //   z = 7
    //   x = 0x00_01_23 (little-endian u24) = 0x000123
    //   y = 0x00_04_56 (little-endian u24) = 0x000456
    //   offset = 0x0000_DEAD_BEEF_1234 (u48 LE)
    //   length = 0x0000_07D0 = 2000 (u32 LE)
    let mut entry_bytes = [0u8; 17];
    entry_bytes[0] = 7; // z

    // x = 0x000123 → [0x23, 0x01, 0x00]
    entry_bytes[1] = 0x23;
    entry_bytes[2] = 0x01;
    entry_bytes[3] = 0x00;

    // y = 0x000456 → [0x56, 0x04, 0x00]
    entry_bytes[4] = 0x56;
    entry_bytes[5] = 0x04;
    entry_bytes[6] = 0x00;

    // offset = 0x0000_DEAD_BEEF_1234
    // Bytes: 0x34, 0x12, 0xEF, 0xBE, 0xAD, 0xDE
    entry_bytes[7] = 0x34;
    entry_bytes[8] = 0x12;
    entry_bytes[9] = 0xEF;
    entry_bytes[10] = 0xBE;
    entry_bytes[11] = 0xAD;
    entry_bytes[12] = 0xDE;

    // length = 2000 = 0x000007D0 → [0xD0, 0x07, 0x00, 0x00]
    entry_bytes[13] = 0xD0;
    entry_bytes[14] = 0x07;
    entry_bytes[15] = 0x00;
    entry_bytes[16] = 0x00;

    let entry = read_v2_entry(&entry_bytes, 0).expect("read ok");
    assert_eq!(entry.z, 7);
    assert_eq!(entry.x, 0x000123);
    assert_eq!(entry.y, 0x000456);
    assert_eq!(entry.offset, 0x0000_DEAD_BEEF_1234);
    assert_eq!(entry.length, 2000);
    assert!(!entry.is_dir);
}

#[test]
fn test_parse_v2_entry_is_dir_when_length_zero() {
    // length == 0 → is_dir must be true.
    let entry_bytes = [0u8; 17];
    let entry = read_v2_entry(&entry_bytes, 0).expect("read ok");
    assert!(entry.is_dir);
    assert_eq!(entry.length, 0);
}

// ---------------------------------------------------------------------------
// zxy_to_v2_tile_id matches v3 Hilbert
// ---------------------------------------------------------------------------

#[test]
fn test_zxy_to_v2_tile_id_matches_v3_hilbert() {
    // The v2 tile-ID helper delegates to the same Hilbert implementation as v3.
    let cases: &[(u8, u32, u32)] = &[
        (0, 0, 0),
        (1, 0, 0),
        (1, 1, 0),
        (1, 0, 1),
        (2, 0, 0),
        (2, 3, 3),
        (5, 10, 10),
    ];
    for &(z, x, y) in cases {
        let v2_id = zxy_to_v2_tile_id(z, x, y).expect("v2 ok");
        let v3_id = zxy_to_tile_id(z, x, y).expect("v3 ok");
        assert_eq!(
            v2_id, v3_id,
            "mismatch for z={z} x={x} y={y}: v2={v2_id} v3={v3_id}"
        );
    }
}

// ---------------------------------------------------------------------------
// PmTilesV2Reader::from_bytes
// ---------------------------------------------------------------------------

#[test]
fn test_v2_reader_from_bytes_synthetic_archive() {
    let tiles: Vec<(u8, u32, u32, Vec<u8>)> = vec![
        (0, 0, 0, b"tile-z0".to_vec()),
        (1, 0, 0, b"tile-z1-00".to_vec()),
    ];
    let archive = make_synthetic_v2(&tiles);
    let reader = PmTilesV2Reader::from_bytes(archive).expect("from_bytes ok");
    assert_eq!(reader.header().version, 2);
}

#[test]
fn test_v2_reader_from_bytes_garbage_fails() {
    let result = PmTilesV2Reader::from_bytes(b"not a pmtiles file".to_vec());
    assert!(result.is_err(), "expected Err for garbage input");
}

// ---------------------------------------------------------------------------
// metadata_json
// ---------------------------------------------------------------------------

#[test]
fn test_v2_reader_metadata_json_parsed() {
    let tiles: Vec<(u8, u32, u32, Vec<u8>)> = vec![(0, 0, 0, b"px".to_vec())];
    let archive = make_synthetic_v2(&tiles);
    let reader = PmTilesV2Reader::from_bytes(archive).expect("ok");
    let meta = reader.metadata_json();
    assert!(
        meta.contains("format"),
        "metadata should contain 'format', got: {meta}"
    );
}

// ---------------------------------------------------------------------------
// get_tile
// ---------------------------------------------------------------------------

#[test]
fn test_v2_reader_get_tile_returns_blob() {
    let blob = b"hello-tile-data".to_vec();
    let tiles: Vec<(u8, u32, u32, Vec<u8>)> = vec![(3, 5, 7, blob.clone())];
    let archive = make_synthetic_v2(&tiles);
    let reader = PmTilesV2Reader::from_bytes(archive).expect("ok");

    let result = reader.get_tile(3, 5, 7).expect("get_tile ok");
    assert_eq!(result, Some(blob), "tile data should match");
}

#[test]
fn test_v2_reader_get_tile_missing_returns_none() {
    let tiles: Vec<(u8, u32, u32, Vec<u8>)> = vec![(0, 0, 0, b"data".to_vec())];
    let archive = make_synthetic_v2(&tiles);
    let reader = PmTilesV2Reader::from_bytes(archive).expect("ok");

    // Request a tile that was not added.
    let result = reader.get_tile(5, 5, 5).expect("get_tile ok");
    assert_eq!(result, None);
}

#[test]
fn test_v2_reader_get_tile_multiple_tiles() {
    let blobs: Vec<Vec<u8>> = vec![
        b"alpha".to_vec(),
        b"beta_longer".to_vec(),
        b"gamma_even_longer_blob".to_vec(),
    ];
    let tiles: Vec<(u8, u32, u32, Vec<u8>)> = vec![
        (0, 0, 0, blobs[0].clone()),
        (1, 0, 0, blobs[1].clone()),
        (1, 1, 0, blobs[2].clone()),
    ];
    let archive = make_synthetic_v2(&tiles);
    let reader = PmTilesV2Reader::from_bytes(archive).expect("ok");

    assert_eq!(
        reader.get_tile(0, 0, 0).expect("ok"),
        Some(blobs[0].clone())
    );
    assert_eq!(
        reader.get_tile(1, 0, 0).expect("ok"),
        Some(blobs[1].clone())
    );
    assert_eq!(
        reader.get_tile(1, 1, 0).expect("ok"),
        Some(blobs[2].clone())
    );
}

// ---------------------------------------------------------------------------
// enumerate_tiles
// ---------------------------------------------------------------------------

#[test]
fn test_v2_reader_enumerate_tiles_walks_all() {
    let tiles: Vec<(u8, u32, u32, Vec<u8>)> = vec![
        (0, 0, 0, b"t0".to_vec()),
        (1, 0, 0, b"t1".to_vec()),
        (2, 1, 1, b"t2".to_vec()),
    ];
    let archive = make_synthetic_v2(&tiles);
    let reader = PmTilesV2Reader::from_bytes(archive).expect("ok");

    let enumerated = reader.enumerate_tiles().expect("enumerate ok");
    assert_eq!(enumerated.len(), 3, "should yield all 3 tiles");
}

#[test]
fn test_v2_reader_enumerate_tiles_empty_archive() {
    let archive = make_synthetic_v2(&[]);
    let reader = PmTilesV2Reader::from_bytes(archive).expect("ok");
    let enumerated = reader.enumerate_tiles().expect("enumerate ok");
    assert!(enumerated.is_empty());
}

#[test]
fn test_v2_reader_enumerate_tiles_blob_contents() {
    let blob = b"unique_blob_content".to_vec();
    let tiles: Vec<(u8, u32, u32, Vec<u8>)> = vec![(4, 8, 9, blob.clone())];
    let archive = make_synthetic_v2(&tiles);
    let reader = PmTilesV2Reader::from_bytes(archive).expect("ok");

    let enumerated = reader.enumerate_tiles().expect("enumerate ok");
    assert_eq!(enumerated.len(), 1);
    let (z, x, y, data) = &enumerated[0];
    assert_eq!((*z, *x, *y), (4u8, 8u32, 9u32));
    assert_eq!(data, &blob);
}

// ---------------------------------------------------------------------------
// upgrade_to_v3
// ---------------------------------------------------------------------------

#[test]
fn test_v2_reader_upgrade_to_v3_produces_valid_archive() {
    let tiles: Vec<(u8, u32, u32, Vec<u8>)> = vec![
        (0, 0, 0, b"z0-tile".to_vec()),
        (1, 0, 0, b"z1-00-tile".to_vec()),
        (1, 1, 1, b"z1-11-tile".to_vec()),
    ];
    let archive_v2 = make_synthetic_v2(&tiles);
    let reader_v2 = PmTilesV2Reader::from_bytes(archive_v2).expect("v2 reader ok");

    let builder = reader_v2.upgrade_to_v3().expect("upgrade ok");
    let archive_v3 = builder.build().expect("build ok");

    // The v3 archive must be parseable by PmTilesReader.
    let reader_v3 = PmTilesReader::from_bytes(archive_v3).expect("v3 reader ok");
    assert_eq!(reader_v3.header.spec_version, 3);
    assert_eq!(
        reader_v3.header.addressed_tiles,
        tiles.len() as u64,
        "all tiles should be present in the upgraded archive"
    );

    // Verify tile content round-trip.
    for (z, x, y, expected_blob) in &tiles {
        let got = reader_v3
            .get_tile(*z, *x, *y)
            .expect("get_tile ok")
            .expect("tile should exist after upgrade");
        assert_eq!(
            &got, expected_blob,
            "tile z={z} x={x} y={y} content mismatch after upgrade"
        );
    }
}

#[test]
fn test_v2_reader_upgrade_to_v3_empty_archive() {
    let archive_v2 = make_synthetic_v2(&[]);
    let reader_v2 = PmTilesV2Reader::from_bytes(archive_v2).expect("v2 reader ok");

    let builder = reader_v2.upgrade_to_v3().expect("upgrade ok");
    let archive_v3 = builder.build().expect("build ok");

    let reader_v3 = PmTilesReader::from_bytes(archive_v3).expect("v3 reader ok");
    assert_eq!(reader_v3.header.addressed_tiles, 0);
}

#[test]
fn test_v2_reader_upgrade_detects_png_tile_type() {
    // The synthetic archive uses format "png" in metadata.
    let archive_v2 = make_synthetic_v2(&[(0, 0, 0, b"px".to_vec())]);
    let reader_v2 = PmTilesV2Reader::from_bytes(archive_v2).expect("ok");
    let builder = reader_v2.upgrade_to_v3().expect("upgrade ok");
    let archive_v3 = builder.build().expect("build ok");
    let reader_v3 = PmTilesReader::from_bytes(archive_v3).expect("v3 ok");
    // TileType::Png == byte value 2 in the v3 header.
    use oxigeo_pmtiles::header::TileType;
    assert_eq!(reader_v3.header.tile_type, TileType::Png);
}

// ---------------------------------------------------------------------------
// PMTILES_V2_MAGIC constant
// ---------------------------------------------------------------------------

#[test]
fn test_v2_magic_constant() {
    assert_eq!(PMTILES_V2_MAGIC, b"PM\x02");
    assert_eq!(PMTILES_V2_MAGIC.len(), 3);
}
