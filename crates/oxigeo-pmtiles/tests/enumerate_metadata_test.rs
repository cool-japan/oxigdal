//! Integration tests for tile enumeration (`enumerate_tiles`) and
//! metadata JSON parsing (`metadata` / `PmTilesMetadata::from_bytes`).
//!
//! Every test builds a synthetic PMTiles v3 archive using
//! [`PmTilesBuilder`], then exercises the new API.

#![allow(clippy::expect_used)]

use oxigeo_pmtiles::{
    Compression, PmTilesBuilder, PmTilesMetadata, PmTilesReader, TileInfo, TileType,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a PMTiles archive from the builder and return a `PmTilesReader`.
fn reader_from_builder(builder: PmTilesBuilder) -> PmTilesReader {
    let bytes = builder.build().expect("build ok");
    PmTilesReader::from_bytes(bytes).expect("reader ok")
}

// ---------------------------------------------------------------------------
// Test 1: Empty archive → enumerate_tiles returns an empty Vec
// ---------------------------------------------------------------------------

#[test]
fn test_enumerate_empty_archive_returns_empty_vec() {
    let builder = PmTilesBuilder::new(TileType::Png, 0, 5);
    let reader = reader_from_builder(builder);

    let tiles = reader.enumerate_tiles().expect("enumerate ok");
    assert!(
        tiles.is_empty(),
        "Expected no tiles in an empty archive, got {}",
        tiles.len()
    );
}

// ---------------------------------------------------------------------------
// Test 2: Single tile (z=0, x=0, y=0) → enumerate returns exactly one entry
// ---------------------------------------------------------------------------

#[test]
fn test_enumerate_single_tile_returns_one_entry() {
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 0);
    builder.add_tile(0, 0, 0, b"z0-tile").expect("add ok");
    let reader = reader_from_builder(builder);

    let tiles = reader.enumerate_tiles().expect("enumerate ok");
    assert_eq!(tiles.len(), 1, "Expected exactly 1 tile");
    let info = &tiles[0];
    // tile_id 0 corresponds to z=0, x=0, y=0.
    assert_eq!(info.tile_id, 0);
    assert_eq!(info.z, 0);
    assert_eq!(info.x, 0);
    assert_eq!(info.y, 0);
}

// ---------------------------------------------------------------------------
// Test 3: Tile at z=3, x=2, y=1 — verify TileInfo.z/x/y matches round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_enumerate_tile_id_to_zxy_correct() {
    let z: u8 = 3;
    let x: u32 = 2;
    let y: u32 = 1;

    let mut builder = PmTilesBuilder::new(TileType::Mvt, 0, 3);
    builder
        .add_tile(z, x, y, b"vector-tile-data")
        .expect("add ok");
    let reader = reader_from_builder(builder);

    let tiles = reader.enumerate_tiles().expect("enumerate ok");
    // We added one tile; it might be deduplicated but the logical tile count
    // depends on whether any run-length merging happened with just one tile —
    // it should still produce exactly one TileInfo.
    let found = tiles.iter().find(|t| t.z == z && t.x == x && t.y == y);
    assert!(
        found.is_some(),
        "Expected to find tile z={z} x={x} y={y} in enumeration, got {tiles:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 4: Run-length expansion — identical tiles deduplicated by builder
//         produce individual TileInfo entries, one per logical tile ID.
// ---------------------------------------------------------------------------

#[test]
fn test_enumerate_run_length_expanded() {
    // z=0 tile_id=0, z=1 tiles cover IDs 1..4.
    // Use identical data for all z=1 tiles so the builder produces a
    // run-length entry (they share the same content hash and are
    // consecutive if sorted by tile_id).
    let shared_data = b"shared-tile-content-run-length-test";

    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 1);
    // At z=1 there are 4 tiles: (0,0),(0,1),(1,0),(1,1).
    builder
        .add_tile(1, 0, 0, shared_data)
        .expect("add 1/0/0 ok");
    builder
        .add_tile(1, 0, 1, shared_data)
        .expect("add 1/0/1 ok");
    builder
        .add_tile(1, 1, 0, shared_data)
        .expect("add 1/1/0 ok");
    builder
        .add_tile(1, 1, 1, shared_data)
        .expect("add 1/1/1 ok");
    let reader = reader_from_builder(builder);

    let tiles = reader.enumerate_tiles().expect("enumerate ok");
    // We added 4 logical tiles; enumerate_tiles must expand any run-length
    // entries so the result has exactly 4 TileInfo structs.
    assert_eq!(
        tiles.len(),
        4,
        "Expected 4 TileInfo entries (one per logical tile), got {}",
        tiles.len()
    );

    // All must be at zoom 1.
    for ti in &tiles {
        assert_eq!(ti.z, 1, "Expected z=1, got z={}", ti.z);
    }

    // All share the same data_offset and data_length (single deduplicated blob).
    let first_offset = tiles[0].data_offset;
    let first_length = tiles[0].data_length;
    for ti in &tiles {
        assert_eq!(
            ti.data_offset, first_offset,
            "All run-length tiles should share data_offset"
        );
        assert_eq!(
            ti.data_length, first_length,
            "All run-length tiles should share data_length"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 5: Multiple tiles — result is sorted by tile_id
// ---------------------------------------------------------------------------

#[test]
fn test_enumerate_multiple_tiles_sorted_by_tile_id() {
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 2);
    // Add tiles in arbitrary order; the builder sorts them, and enumerate_tiles
    // must also return them in tile-ID order.
    let tiles_to_add: &[(u8, u32, u32, &[u8])] = &[
        (2, 3, 1, b"d1"),
        (1, 1, 0, b"d2"),
        (0, 0, 0, b"d3"),
        (2, 0, 2, b"d4"),
        (1, 0, 1, b"d5"),
    ];
    for &(z, x, y, data) in tiles_to_add {
        builder.add_tile(z, x, y, data).expect("add ok");
    }
    let reader = reader_from_builder(builder);

    let tiles = reader.enumerate_tiles().expect("enumerate ok");
    assert_eq!(tiles.len(), tiles_to_add.len());

    // Verify strictly ascending tile_id order.
    for window in tiles.windows(2) {
        assert!(
            window[0].tile_id < window[1].tile_id,
            "tile_ids not sorted: {} >= {}",
            window[0].tile_id,
            window[1].tile_id
        );
    }
}

// ---------------------------------------------------------------------------
// Test 6: Empty metadata section → PmTilesMetadata::from_bytes with empty
//         slice produces all-None metadata.
// ---------------------------------------------------------------------------

#[test]
fn test_metadata_empty_json_parses() {
    // Build an archive without calling set_metadata (builder defaults to "{}").
    let builder = PmTilesBuilder::new(TileType::Png, 0, 0);
    let reader = reader_from_builder(builder);

    let meta = reader.metadata().expect("metadata ok");
    assert!(
        meta.name.is_none(),
        "Expected name to be None, got {:?}",
        meta.name
    );
    assert!(
        meta.description.is_none(),
        "Expected description to be None"
    );
    assert!(meta.format.is_none(), "Expected format to be None");
    assert!(meta.bounds.is_none(), "Expected bounds to be None");
    assert!(meta.center.is_none(), "Expected center to be None");
    assert!(meta.minzoom.is_none(), "Expected minzoom to be None");
    assert!(meta.maxzoom.is_none(), "Expected maxzoom to be None");
    assert!(
        meta.attribution.is_none(),
        "Expected attribution to be None"
    );
    // PmTilesMetadata::from_bytes on "{}" yields empty extra.
    assert!(
        meta.extra.is_empty(),
        "Expected no extra fields for default metadata, got {:?}",
        meta.extra
    );
}

// ---------------------------------------------------------------------------
// Test 7: Builder's set_metadata injects JSON → name and format parsed.
// ---------------------------------------------------------------------------

#[test]
fn test_metadata_name_and_format_parsed() {
    let mut builder = PmTilesBuilder::new(TileType::Mvt, 0, 14);
    builder.add_tile(0, 0, 0, b"\x00\x01\x02").expect("add ok");
    builder.set_metadata(
        r#"{"name":"test-tileset","format":"pbf","minzoom":0,"maxzoom":14}"#.to_string(),
    );
    let reader = reader_from_builder(builder);

    let meta = reader.metadata().expect("metadata ok");
    assert_eq!(
        meta.name.as_deref(),
        Some("test-tileset"),
        "name mismatch: {:?}",
        meta.name
    );
    assert_eq!(
        meta.format.as_deref(),
        Some("pbf"),
        "format mismatch: {:?}",
        meta.format
    );
    assert_eq!(meta.minzoom, Some(0u8), "minzoom mismatch");
    assert_eq!(meta.maxzoom, Some(14u8), "maxzoom mismatch");
}

// ---------------------------------------------------------------------------
// Test 8: Extra / unknown fields are captured in `extra` map.
// ---------------------------------------------------------------------------

#[test]
fn test_metadata_extra_fields_captured() {
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 5);
    builder.set_metadata(
        r#"{"name":"x","custom_numeric":42,"custom_string":"hello","nested":{"a":1,"b":2}}"#
            .to_string(),
    );
    let reader = reader_from_builder(builder);

    let meta = reader.metadata().expect("metadata ok");
    assert_eq!(meta.name.as_deref(), Some("x"));

    // `custom_numeric` should be in extra as a JSON number.
    let custom_val = meta
        .extra
        .get("custom_numeric")
        .expect("custom_numeric key missing");
    assert_eq!(
        custom_val,
        &serde_json::Value::Number(42.into()),
        "custom_numeric mismatch: {custom_val:?}"
    );

    // `custom_string` should be in extra as a JSON string.
    let custom_str = meta
        .extra
        .get("custom_string")
        .expect("custom_string key missing");
    assert_eq!(
        custom_str,
        &serde_json::Value::String("hello".to_string()),
        "custom_string mismatch: {custom_str:?}"
    );

    // `nested` should be present as an object.
    let nested = meta.extra.get("nested").expect("nested key missing");
    assert!(nested.is_object(), "nested should be an object: {nested:?}");
}

// ---------------------------------------------------------------------------
// Test 9: PmTilesMetadata::from_bytes with a raw JSON slice (unit).
// ---------------------------------------------------------------------------

#[test]
fn test_metadata_from_bytes_unit_bounds_and_center() {
    let json = br#"{"name":"geo","bounds":[-10.0,-5.0,10.0,5.0],"center":[0.0,0.0,4]}"#;
    let meta = PmTilesMetadata::from_bytes(json, Compression::None).expect("parse ok");

    assert_eq!(meta.name.as_deref(), Some("geo"));
    let bounds = meta.bounds.expect("bounds present");
    assert!((bounds[0] - -10.0_f64).abs() < 1e-9);
    assert!((bounds[1] - -5.0_f64).abs() < 1e-9);
    assert!((bounds[2] - 10.0_f64).abs() < 1e-9);
    assert!((bounds[3] - 5.0_f64).abs() < 1e-9);

    let center = meta.center.expect("center present");
    assert!((center[0] - 0.0_f64).abs() < 1e-9);
    assert!((center[1] - 0.0_f64).abs() < 1e-9);
    assert!((center[2] - 4.0_f64).abs() < 1e-9);
}

// ---------------------------------------------------------------------------
// Test 10: TileInfo data_offset and data_length point to valid tile data.
// ---------------------------------------------------------------------------

#[test]
fn test_enumerate_tile_info_offsets_are_valid() {
    let tile_data = b"hello-tile-payload";

    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 1);
    builder.add_tile(1, 0, 0, tile_data).expect("add ok");
    builder.add_tile(1, 1, 1, b"other-tile").expect("add ok");

    let archive_bytes = builder.build().expect("build ok");
    let reader = PmTilesReader::from_bytes(archive_bytes.clone()).expect("reader ok");

    let tiles = reader.enumerate_tiles().expect("enumerate ok");
    assert_eq!(tiles.len(), 2);

    // Find the TileInfo for (1,0,0) and verify offset/length against actual archive bytes.
    let info = tiles
        .iter()
        .find(|t| t.z == 1 && t.x == 0 && t.y == 0)
        .expect("tile (1,0,0) must be in enumeration");

    let tile_data_start = reader.header.tile_data_offset as usize;
    let payload_start = tile_data_start + info.data_offset as usize;
    let payload_end = payload_start + info.data_length as usize;

    assert!(
        payload_end <= archive_bytes.len(),
        "Tile payload [{payload_start}..{payload_end}) is out of bounds (archive is {} bytes)",
        archive_bytes.len()
    );

    let extracted = &archive_bytes[payload_start..payload_end];
    assert_eq!(
        extracted, tile_data,
        "Extracted payload does not match original tile data"
    );
}

// ---------------------------------------------------------------------------
// Test 11: enumerate_tiles agrees with get_tile on all coordinates.
// ---------------------------------------------------------------------------

#[test]
fn test_enumerate_and_get_tile_agree() {
    let test_tiles: &[(u8, u32, u32, &[u8])] = &[
        (0, 0, 0, b"z0"),
        (1, 0, 0, b"z1-0-0"),
        (1, 1, 0, b"z1-1-0"),
        (1, 0, 1, b"z1-0-1"),
        (1, 1, 1, b"z1-1-1"),
        (2, 0, 0, b"z2-0-0"),
    ];

    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 2);
    for &(z, x, y, data) in test_tiles {
        builder.add_tile(z, x, y, data).expect("add ok");
    }
    let reader = reader_from_builder(builder);

    let infos = reader.enumerate_tiles().expect("enumerate ok");

    // Every tile returned by enumerate_tiles must be retrievable via get_tile.
    for info in &infos {
        let got = reader
            .get_tile(info.z, info.x, info.y)
            .expect("get_tile ok");
        assert!(
            got.is_some(),
            "get_tile(z={}, x={}, y={}) returned None but enumerate_tiles reported it",
            info.z,
            info.x,
            info.y
        );
    }

    // Every tile we added must appear in the enumeration.
    for &(z, x, y, _) in test_tiles {
        let found = infos
            .iter()
            .any(|t: &TileInfo| t.z == z && t.x == x && t.y == y);
        assert!(
            found,
            "Tile z={z} x={x} y={y} is missing from enumerate_tiles"
        );
    }
}
