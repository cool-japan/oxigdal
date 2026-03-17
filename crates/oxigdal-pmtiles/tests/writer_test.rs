//! Integration tests for the PMTiles writer.

#![allow(clippy::expect_used)]

use oxigdal_pmtiles::{
    PmTilesBuilder, PmTilesHeader, PmTilesReader, TileType, tile_id_to_zxy, zxy_to_tile_id,
};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Build a simple archive and return its bytes.
fn build_simple_archive(tiles: &[(u8, u32, u32, &[u8])]) -> Vec<u8> {
    let min_z = tiles.iter().map(|t| t.0).min().unwrap_or(0);
    let max_z = tiles.iter().map(|t| t.0).max().unwrap_or(0);
    let mut builder = PmTilesBuilder::new(TileType::Png, min_z, max_z);
    for &(z, x, y, data) in tiles {
        builder.add_tile(z, x, y, data).expect("add_tile");
    }
    builder.build().expect("build")
}

// ── Basic builder tests ──────────────────────────────────────────────────────

#[test]
fn test_builder_new_defaults() {
    let builder = PmTilesBuilder::new(TileType::Mvt, 0, 14);
    assert_eq!(builder.tile_count(), 0);
}

#[test]
fn test_builder_add_tile_increments_count() {
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 5);
    builder.add_tile(0, 0, 0, b"tile0").expect("ok");
    assert_eq!(builder.tile_count(), 1);
    builder.add_tile(1, 0, 0, b"tile1").expect("ok");
    assert_eq!(builder.tile_count(), 2);
}

#[test]
fn test_builder_add_tile_zoom_below_min_error() {
    let mut builder = PmTilesBuilder::new(TileType::Png, 2, 5);
    assert!(builder.add_tile(1, 0, 0, b"data").is_err());
}

#[test]
fn test_builder_add_tile_zoom_above_max_error() {
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 3);
    assert!(builder.add_tile(4, 0, 0, b"data").is_err());
}

#[test]
fn test_builder_add_tile_out_of_range_coords() {
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 3);
    // At z=1, max coord is 1
    assert!(builder.add_tile(1, 2, 0, b"data").is_err());
}

// ── Build output tests ──────────────────────────────────────────────────────

#[test]
fn test_build_empty_archive() {
    let builder = PmTilesBuilder::new(TileType::Png, 0, 5);
    let archive = builder.build().expect("build ok");
    // Should have at least the 127-byte header
    assert!(archive.len() >= 127);
    let header = PmTilesHeader::parse(&archive).expect("parse ok");
    assert_eq!(header.tile_type, TileType::Png);
    assert_eq!(header.addressed_tiles, 0);
}

#[test]
fn test_build_single_tile() {
    let archive = build_simple_archive(&[(0, 0, 0, b"hello-tile")]);
    assert!(archive.len() > 127);
    let header = PmTilesHeader::parse(&archive).expect("parse ok");
    assert_eq!(header.addressed_tiles, 1);
    assert_eq!(header.tile_entries, 1);
    assert_eq!(header.tile_contents, 1);
}

#[test]
fn test_build_header_magic() {
    let archive = build_simple_archive(&[(0, 0, 0, b"data")]);
    assert_eq!(&archive[0..7], b"PMTiles");
    assert_eq!(archive[7], 3);
}

#[test]
fn test_build_tile_type_preserved() {
    let mut builder = PmTilesBuilder::new(TileType::Jpeg, 0, 0);
    builder.add_tile(0, 0, 0, b"jpeg-data").expect("ok");
    let archive = builder.build().expect("build");
    let header = PmTilesHeader::parse(&archive).expect("parse");
    assert_eq!(header.tile_type, TileType::Jpeg);
}

#[test]
fn test_build_zoom_range_preserved() {
    let mut builder = PmTilesBuilder::new(TileType::Png, 3, 12);
    builder.add_tile(5, 0, 0, b"data").expect("ok");
    let archive = builder.build().expect("build");
    let header = PmTilesHeader::parse(&archive).expect("parse");
    assert_eq!(header.min_zoom, 3);
    assert_eq!(header.max_zoom, 12);
}

// ── Round-trip write→read ───────────────────────────────────────────────────

#[test]
fn test_round_trip_single_tile_directory() {
    let archive = build_simple_archive(&[(0, 0, 0, b"tile-data-z0")]);
    let reader = PmTilesReader::from_bytes(archive).expect("reader ok");
    let entries = reader.root_directory().expect("dir ok");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].tile_id, zxy_to_tile_id(0, 0, 0).expect("ok"));
}

#[test]
fn test_round_trip_multiple_tiles_directory() {
    let tiles: Vec<(u8, u32, u32, &[u8])> = vec![
        (0, 0, 0, b"z0-tile"),
        (1, 0, 0, b"z1-00"),
        (1, 1, 0, b"z1-10"),
        (1, 0, 1, b"z1-01"),
        (1, 1, 1, b"z1-11"),
    ];
    let archive = build_simple_archive(&tiles);
    let reader = PmTilesReader::from_bytes(archive).expect("reader ok");
    let entries = reader.root_directory().expect("dir ok");
    assert_eq!(entries.len(), 5);

    // Verify entries are sorted by tile_id
    for i in 1..entries.len() {
        assert!(entries[i].tile_id >= entries[i - 1].tile_id);
    }
}

#[test]
fn test_round_trip_tile_data_readable() {
    let tile_data = b"PNG-fake-tile-payload-12345";
    let archive = build_simple_archive(&[(0, 0, 0, tile_data)]);
    let reader = PmTilesReader::from_bytes(archive.clone()).expect("reader");
    let entries = reader.root_directory().expect("dir");
    assert_eq!(entries.len(), 1);

    let entry = &entries[0];
    let data_start = reader.header.tile_data_offset as usize + entry.offset as usize;
    let data_end = data_start + entry.length as usize;
    let recovered = &archive[data_start..data_end];
    assert_eq!(recovered, tile_data);
}

#[test]
fn test_round_trip_multiple_tile_data() {
    let tiles: Vec<(u8, u32, u32, Vec<u8>)> = vec![
        (0, 0, 0, b"alpha".to_vec()),
        (1, 0, 0, b"bravo".to_vec()),
        (1, 1, 0, b"charlie".to_vec()),
    ];

    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 1);
    for (z, x, y, data) in &tiles {
        builder.add_tile(*z, *x, *y, data).expect("ok");
    }
    let archive = builder.build().expect("build");
    let reader = PmTilesReader::from_bytes(archive.clone()).expect("reader");
    let entries = reader.root_directory().expect("dir");

    // Read each tile's data back
    for entry in &entries {
        let (z, x, y) = tile_id_to_zxy(entry.tile_id).expect("valid");
        let data_start = reader.header.tile_data_offset as usize + entry.offset as usize;
        let data_end = data_start + entry.length as usize;
        let recovered = &archive[data_start..data_end];

        // Find the original data
        let original = tiles.iter().find(|t| t.0 == z && t.1 == x && t.2 == y);
        assert!(
            original.is_some(),
            "tile ({z},{x},{y}) not found in originals"
        );
        assert_eq!(recovered, original.expect("checked above").3.as_slice());
    }
}

// ── Deduplication ───────────────────────────────────────────────────────────

#[test]
fn test_deduplication_identical_tiles() {
    let same_data = b"identical-tile-content";
    let mut builder = PmTilesBuilder::new(TileType::Png, 1, 1);
    for x in 0..2u32 {
        for y in 0..2u32 {
            builder.add_tile(1, x, y, same_data).expect("ok");
        }
    }
    let archive = builder.build().expect("build");
    let header = PmTilesHeader::parse(&archive).expect("parse");

    // 4 addressed tiles but only 1 unique content
    assert_eq!(header.addressed_tiles, 4);
    assert_eq!(header.tile_contents, 1);
}

#[test]
fn test_deduplication_mixed_content() {
    let mut builder = PmTilesBuilder::new(TileType::Png, 1, 1);
    builder.add_tile(1, 0, 0, b"data-A").expect("ok");
    builder.add_tile(1, 1, 0, b"data-B").expect("ok");
    builder.add_tile(1, 0, 1, b"data-A").expect("ok"); // duplicate
    builder.add_tile(1, 1, 1, b"data-B").expect("ok"); // duplicate
    let archive = builder.build().expect("build");
    let header = PmTilesHeader::parse(&archive).expect("parse");

    assert_eq!(header.addressed_tiles, 4);
    assert_eq!(header.tile_contents, 2);
}

#[test]
fn test_deduplication_all_unique() {
    let mut builder = PmTilesBuilder::new(TileType::Png, 1, 1);
    builder.add_tile(1, 0, 0, b"data-1").expect("ok");
    builder.add_tile(1, 1, 0, b"data-2").expect("ok");
    builder.add_tile(1, 0, 1, b"data-3").expect("ok");
    builder.add_tile(1, 1, 1, b"data-4").expect("ok");
    let archive = builder.build().expect("build");
    let header = PmTilesHeader::parse(&archive).expect("parse");

    assert_eq!(header.addressed_tiles, 4);
    assert_eq!(header.tile_contents, 4);
}

#[test]
fn test_deduplication_data_shared_content() {
    let same_data = b"shared-tile-payload";
    let mut builder = PmTilesBuilder::new(TileType::Png, 1, 1);
    builder.add_tile(1, 0, 0, same_data).expect("ok");
    builder.add_tile(1, 1, 0, same_data).expect("ok");
    let archive = builder.build().expect("build");
    let reader = PmTilesReader::from_bytes(archive.clone()).expect("reader");
    let entries = reader.root_directory().expect("dir");

    // Both entries should have same length
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].length, entries[1].length);
    assert_eq!(entries[0].length, same_data.len() as u32);

    // Verify actual tile data is correct for both entries (clustered layout)
    let base = reader.header.tile_data_offset as usize;
    let d0 = &archive[base + entries[0].offset as usize
        ..base + entries[0].offset as usize + entries[0].length as usize];
    let d1 = &archive[base + entries[1].offset as usize
        ..base + entries[1].offset as usize + entries[1].length as usize];
    assert_eq!(d0, same_data);
    assert_eq!(d1, same_data);
}

// ── Metadata ────────────────────────────────────────────────────────────────

#[test]
fn test_metadata_json_round_trip() {
    let json_str = r#"{"name":"test","description":"test archive"}"#;
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 0);
    builder.set_metadata(json_str.to_string());
    builder.add_tile(0, 0, 0, b"tile").expect("ok");
    let archive = builder.build().expect("build");
    let header = PmTilesHeader::parse(&archive).expect("parse");

    let meta_start = header.metadata_offset as usize;
    let meta_end = meta_start + header.metadata_length as usize;
    let recovered = std::str::from_utf8(&archive[meta_start..meta_end]).expect("utf8");
    assert_eq!(recovered, json_str);
}

#[test]
fn test_metadata_default_empty_json() {
    let builder = PmTilesBuilder::new(TileType::Png, 0, 0);
    let archive = builder.build().expect("build");
    let header = PmTilesHeader::parse(&archive).expect("parse");

    let meta_start = header.metadata_offset as usize;
    let meta_end = meta_start + header.metadata_length as usize;
    let recovered = std::str::from_utf8(&archive[meta_start..meta_end]).expect("utf8");
    assert_eq!(recovered, "{}");
}

// ── Bounds and center ───────────────────────────────────────────────────────

#[test]
fn test_bounds_round_trip() {
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 0);
    builder.set_bounds(-122.5, 37.5, -122.0, 38.0);
    builder.add_tile(0, 0, 0, b"tile").expect("ok");
    let archive = builder.build().expect("build");
    let header = PmTilesHeader::parse(&archive).expect("parse");
    let b = header.bounds();
    assert!((b[0] - (-122.5)).abs() < 1e-4);
    assert!((b[1] - 37.5).abs() < 1e-4);
    assert!((b[2] - (-122.0)).abs() < 1e-4);
    assert!((b[3] - 38.0).abs() < 1e-4);
}

#[test]
fn test_center_round_trip() {
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 5);
    builder.set_center(139.6917, 35.6895, 10);
    builder.add_tile(0, 0, 0, b"tile").expect("ok");
    let archive = builder.build().expect("build");
    let header = PmTilesHeader::parse(&archive).expect("parse");
    assert!((header.center_lon() - 139.6917).abs() < 1e-4);
    assert!((header.center_lat() - 35.6895).abs() < 1e-4);
    assert_eq!(header.center_zoom, 10);
}

// ── Multiple zoom levels ────────────────────────────────────────────────────

#[test]
fn test_multiple_zoom_levels() {
    let tiles: Vec<(u8, u32, u32, &[u8])> = vec![
        (0, 0, 0, b"zoom0"),
        (1, 0, 0, b"zoom1-a"),
        (1, 1, 1, b"zoom1-b"),
        (2, 0, 0, b"zoom2-a"),
        (2, 3, 3, b"zoom2-b"),
    ];
    let archive = build_simple_archive(&tiles);
    let reader = PmTilesReader::from_bytes(archive).expect("reader");
    let entries = reader.root_directory().expect("dir");
    assert_eq!(entries.len(), 5);

    // Verify each tile_id can be decoded back
    for entry in &entries {
        let result = tile_id_to_zxy(entry.tile_id);
        assert!(result.is_ok(), "tile_id {} is invalid", entry.tile_id);
    }
}

#[test]
fn test_zoom_range_5_to_8() {
    let mut builder = PmTilesBuilder::new(TileType::Mvt, 5, 8);
    builder.add_tile(5, 0, 0, b"z5").expect("ok");
    builder.add_tile(6, 0, 0, b"z6").expect("ok");
    builder.add_tile(7, 0, 0, b"z7").expect("ok");
    builder.add_tile(8, 0, 0, b"z8").expect("ok");
    let archive = builder.build().expect("build");
    let header = PmTilesHeader::parse(&archive).expect("parse");
    assert_eq!(header.min_zoom, 5);
    assert_eq!(header.max_zoom, 8);
    assert_eq!(header.tile_type, TileType::Mvt);
}

// ── Header field validation ─────────────────────────────────────────────────

#[test]
fn test_header_clustered_flag() {
    let archive = build_simple_archive(&[(0, 0, 0, b"data")]);
    let header = PmTilesHeader::parse(&archive).expect("parse");
    assert!(header.clustered);
}

#[test]
fn test_header_internal_compression_none() {
    let archive = build_simple_archive(&[(0, 0, 0, b"data")]);
    let header = PmTilesHeader::parse(&archive).expect("parse");
    assert_eq!(
        header.internal_compression,
        oxigdal_pmtiles::Compression::None
    );
    assert_eq!(header.tile_compression, oxigdal_pmtiles::Compression::None);
}

#[test]
fn test_header_offsets_consistent() {
    let archive = build_simple_archive(&[(0, 0, 0, b"tile-0"), (1, 0, 0, b"tile-1")]);
    let header = PmTilesHeader::parse(&archive).expect("parse");

    // root_dir should start at 127
    assert_eq!(header.root_dir_offset, 127);
    // metadata should follow root_dir
    assert_eq!(
        header.metadata_offset,
        header.root_dir_offset + header.root_dir_length
    );
    // tile_data should follow metadata
    assert_eq!(
        header.tile_data_offset,
        header.metadata_offset + header.metadata_length
    );
    // total file size should match
    assert_eq!(
        archive.len() as u64,
        header.tile_data_offset + header.tile_data_length
    );
}

// ── Edge cases ──────────────────────────────────────────────────────────────

#[test]
fn test_empty_tile_data() {
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 0);
    builder.add_tile(0, 0, 0, b"").expect("ok");
    let archive = builder.build().expect("build");
    let reader = PmTilesReader::from_bytes(archive).expect("reader");
    let entries = reader.root_directory().expect("dir");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].length, 0);
}

#[test]
fn test_large_tile_data() {
    let big_tile = vec![0xABu8; 65536]; // 64KB
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 0);
    builder.add_tile(0, 0, 0, &big_tile).expect("ok");
    let archive = builder.build().expect("build");
    let reader = PmTilesReader::from_bytes(archive.clone()).expect("reader");
    let entries = reader.root_directory().expect("dir");
    assert_eq!(entries[0].length, 65536);

    let data_start = reader.header.tile_data_offset as usize + entries[0].offset as usize;
    let data_end = data_start + entries[0].length as usize;
    assert_eq!(&archive[data_start..data_end], big_tile.as_slice());
}

#[test]
fn test_many_tiles_z4() {
    // z=4 has 256 tiles total
    let mut builder = PmTilesBuilder::new(TileType::Png, 4, 4);
    for x in 0..16u32 {
        for y in 0..16u32 {
            let data = format!("tile-{x}-{y}");
            builder.add_tile(4, x, y, data.as_bytes()).expect("ok");
        }
    }
    let archive = builder.build().expect("build");
    let reader = PmTilesReader::from_bytes(archive).expect("reader");
    let entries = reader.root_directory().expect("dir");
    assert_eq!(entries.len(), 256);
}

#[test]
fn test_tile_type_webp() {
    let mut builder = PmTilesBuilder::new(TileType::Webp, 0, 0);
    builder.add_tile(0, 0, 0, b"webp-data").expect("ok");
    let archive = builder.build().expect("build");
    let header = PmTilesHeader::parse(&archive).expect("parse");
    assert_eq!(header.tile_type, TileType::Webp);
}

#[test]
fn test_tile_type_avif() {
    let mut builder = PmTilesBuilder::new(TileType::Avif, 0, 0);
    builder.add_tile(0, 0, 0, b"avif-data").expect("ok");
    let archive = builder.build().expect("build");
    let header = PmTilesHeader::parse(&archive).expect("parse");
    assert_eq!(header.tile_type, TileType::Avif);
}

#[test]
fn test_directory_sorted_by_tile_id() {
    // Add tiles in reverse order — builder should sort them
    let mut builder = PmTilesBuilder::new(TileType::Png, 1, 2);
    builder.add_tile(2, 3, 3, b"last").expect("ok");
    builder.add_tile(1, 0, 0, b"first").expect("ok");
    builder.add_tile(2, 0, 0, b"mid").expect("ok");
    let archive = builder.build().expect("build");
    let reader = PmTilesReader::from_bytes(archive).expect("reader");
    let entries = reader.root_directory().expect("dir");

    for i in 1..entries.len() {
        assert!(
            entries[i].tile_id >= entries[i - 1].tile_id,
            "directory not sorted at index {i}"
        );
    }
}

#[test]
fn test_run_length_is_one() {
    let archive = build_simple_archive(&[(0, 0, 0, b"tile")]);
    let reader = PmTilesReader::from_bytes(archive).expect("reader");
    let entries = reader.root_directory().expect("dir");
    assert_eq!(entries[0].run_length, 1);
    assert!(entries[0].is_tile());
}

// ── Temp file round-trip ────────────────────────────────────────────────────

#[test]
fn test_write_to_temp_file_and_read_back() {
    use std::io::Write;

    let archive = build_simple_archive(&[
        (0, 0, 0, b"tile-0"),
        (1, 0, 0, b"tile-1-00"),
        (1, 1, 1, b"tile-1-11"),
    ]);

    let dir = std::env::temp_dir();
    let path = dir.join("test_pmtiles_writer.pmtiles");
    {
        let mut f = std::fs::File::create(&path).expect("create file");
        f.write_all(&archive).expect("write");
    }
    let read_back = std::fs::read(&path).expect("read file");
    let _ = std::fs::remove_file(&path);

    let reader = PmTilesReader::from_bytes(read_back).expect("reader");
    assert_eq!(reader.header.addressed_tiles, 3);
    let entries = reader.root_directory().expect("dir");
    assert_eq!(entries.len(), 3);
}

#[test]
fn test_builder_set_metadata_large() {
    let large_json = format!(
        r#"{{"tiles":[{}]}}"#,
        (0..100)
            .map(|i| format!("{i}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 0);
    builder.set_metadata(large_json.clone());
    builder.add_tile(0, 0, 0, b"tile").expect("ok");
    let archive = builder.build().expect("build");
    let header = PmTilesHeader::parse(&archive).expect("parse");
    assert_eq!(header.metadata_length, large_json.len() as u64);
}
