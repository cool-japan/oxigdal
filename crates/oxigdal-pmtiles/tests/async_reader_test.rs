//! Integration tests for the async PMTiles reader.
//!
//! All tests are gated behind the `async` Cargo feature and use
//! `tokio::io::BufReader<std::io::Cursor<Vec<u8>>>` as the async source so
//! that no filesystem access is required.

#![cfg(feature = "async")]
#![allow(clippy::expect_used)]

use oxigdal_pmtiles::{AsyncPmTilesReader, PmTilesBuilder, TileType};
use tokio::io::BufReader;

// ─────────────────────────────────────────────────────────────────────────────
// Test helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Build a minimal single-tile archive and return its bytes.
fn build_one_tile_archive(data: &[u8]) -> Vec<u8> {
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 0);
    builder.add_tile(0, 0, 0, data).expect("add tile");
    builder.build().expect("build archive")
}

/// Build an archive with a small set of `(z, x, y)` tiles.
fn build_multi_tile_archive(tiles: &[(u8, u32, u32, &[u8])]) -> Vec<u8> {
    let min_z = tiles.iter().map(|t| t.0).min().unwrap_or(0);
    let max_z = tiles.iter().map(|t| t.0).max().unwrap_or(0);
    let mut builder = PmTilesBuilder::new(TileType::Png, min_z, max_z);
    for &(z, x, y, d) in tiles {
        builder.add_tile(z, x, y, d).expect("add tile");
    }
    builder.build().expect("build archive")
}

/// Wrap raw bytes in the async adapter used by all tests.
fn make_reader(bytes: Vec<u8>) -> BufReader<std::io::Cursor<Vec<u8>>> {
    BufReader::new(std::io::Cursor::new(bytes))
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 — open parses the header and spec_version is 3
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_async_reader_open_parses_header() {
    let archive = build_one_tile_archive(b"tile_data");
    let reader = AsyncPmTilesReader::open(make_reader(archive))
        .await
        .expect("open");
    assert_eq!(reader.header().spec_version, 3);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 — random bytes return an Err (bad magic)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_async_reader_open_invalid_magic_returns_error() {
    // 127 bytes that do not start with "PMTiles"
    let garbage = vec![0xFFu8; 200];
    let result = AsyncPmTilesReader::open(make_reader(garbage)).await;
    assert!(
        result.is_err(),
        "expected an error for non-PMTiles bytes, got Ok"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 — get_tile returns Some for a tile that was added
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_async_reader_get_tile_returns_some_for_present_tile() {
    let archive = build_one_tile_archive(b"present");
    let mut reader = AsyncPmTilesReader::open(make_reader(archive))
        .await
        .expect("open");
    let result = reader.get_tile(0, 0, 0).await.expect("get_tile");
    assert!(result.is_some(), "expected Some for z=0/x=0/y=0");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 — get_tile returns None for a tile not in the archive
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_async_reader_get_tile_returns_none_for_absent_tile() {
    // Archive has only z=0/x=0/y=0.
    let archive = build_one_tile_archive(b"only_one");
    let mut reader = AsyncPmTilesReader::open(make_reader(archive))
        .await
        .expect("open");
    // z=5/x=1/y=1 is guaranteed absent.
    let result = reader.get_tile(5, 1, 1).await.expect("get_tile");
    assert!(result.is_none(), "expected None for an absent tile");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 — tile data round-trips exactly
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_async_reader_get_tile_data_matches_original() {
    let original = b"exact-payload-12345";
    let archive = build_one_tile_archive(original);
    let mut reader = AsyncPmTilesReader::open(make_reader(archive))
        .await
        .expect("open");
    let data = reader
        .get_tile(0, 0, 0)
        .await
        .expect("get_tile")
        .expect("some");
    assert_eq!(data.as_slice(), original as &[u8]);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6 — get_tile_by_id and get_tile return the same bytes
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_async_reader_get_tile_by_id_matches_get_tile() {
    use oxigdal_pmtiles::zxy_to_tile_id;

    let payload = b"by-id-and-zxy";
    let archive = build_multi_tile_archive(&[(2, 1, 1, payload)]);

    let cursor_a = std::io::Cursor::new(archive.clone());
    let cursor_b = std::io::Cursor::new(archive);

    let mut reader_a = AsyncPmTilesReader::open(BufReader::new(cursor_a))
        .await
        .expect("open a");
    let mut reader_b = AsyncPmTilesReader::open(BufReader::new(cursor_b))
        .await
        .expect("open b");

    let via_zxy = reader_a
        .get_tile(2, 1, 1)
        .await
        .expect("get_tile")
        .expect("some");
    let tile_id = zxy_to_tile_id(2, 1, 1).expect("tile_id");
    let via_id = reader_b
        .get_tile_by_id(tile_id)
        .await
        .expect("get_tile_by_id")
        .expect("some");

    assert_eq!(via_zxy, via_id);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7 — root_directory is non-empty after open (archive has tiles)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_async_reader_root_directory_non_empty_after_open() {
    let archive = build_one_tile_archive(b"has-tile");
    let reader = AsyncPmTilesReader::open(make_reader(archive))
        .await
        .expect("open");
    assert!(
        !reader.root_directory().is_empty(),
        "root directory should be non-empty"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 8 — leaf cache grows when a leaf directory is traversed
//
// We use an archive large enough to force leaf-directory splitting (>16 kB of
// root directory varint entries).  The PMTiles builder splits the root into
// leaves once the root exceeds LEAF_SPLIT_THRESHOLD (16 384 bytes).  With one
// byte of tile data and ~10 bytes of directory overhead per entry we need
// roughly 2 000 distinct tiles to reliably exceed the threshold.  We add
// 10 000 tiles at z=8 (max coord 0..255) to guarantee leaf directories are
// produced.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_async_reader_leaf_cache_grows_on_leaf_traversal() {
    // Build a large archive that definitely needs leaf directories.
    // z=8 allows x/y in 0..255; we use min_zoom=max_zoom=8.
    let mut builder = PmTilesBuilder::new(TileType::Mvt, 8, 8);
    let mut count = 0usize;
    'outer: for x in 0..255u32 {
        for y in 0..255u32 {
            builder
                .add_tile(8, x, y, format!("tile-{x}-{y}").as_bytes())
                .expect("add tile");
            count += 1;
            if count >= 10_000 {
                break 'outer;
            }
        }
    }
    let archive = builder.build().expect("build");

    let mut reader = AsyncPmTilesReader::open(make_reader(archive))
        .await
        .expect("open");

    // Before any lookup the leaf cache is empty.
    assert_eq!(reader.cached_leaf_count(), 0);

    // A lookup that traverses a leaf should populate the cache.
    // Tile z=8/x=5/y=5 is in the middle of the grid and should be present.
    let _tile = reader.get_tile(8, 5, 5).await.expect("get_tile");

    // The archive has leaf directories; fetching a tile should have populated
    // at least one leaf cache entry.
    assert!(
        reader.cached_leaf_count() > 0,
        "leaf cache should have grown after traversing a leaf"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 9 — fetching the same tile twice does not grow the leaf cache further
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_async_reader_leaf_cache_stable_on_repeat_fetch() {
    // Same large archive as test 8.
    let mut builder = PmTilesBuilder::new(TileType::Mvt, 8, 8);
    let mut count = 0usize;
    'outer: for x in 0..255u32 {
        for y in 0..255u32 {
            builder
                .add_tile(8, x, y, format!("d-{x}-{y}").as_bytes())
                .expect("add");
            count += 1;
            if count >= 10_000 {
                break 'outer;
            }
        }
    }
    let archive = builder.build().expect("build");
    let mut reader = AsyncPmTilesReader::open(make_reader(archive))
        .await
        .expect("open");

    // First fetch — may traverse a leaf.
    let _ = reader.get_tile(8, 5, 5).await.expect("get_tile 1");
    let after_first = reader.cached_leaf_count();

    // Second fetch of the exact same tile — cache count must not grow.
    let _ = reader.get_tile(8, 5, 5).await.expect("get_tile 2");
    let after_second = reader.cached_leaf_count();

    assert_eq!(
        after_first, after_second,
        "leaf cache should not grow on repeated fetch of the same tile"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 10 — header accessor exposes a meaningful root_dir_offset
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_async_reader_header_accessor_returns_parsed_header() {
    let archive = build_one_tile_archive(b"header-test");
    let reader = AsyncPmTilesReader::open(make_reader(archive))
        .await
        .expect("open");

    // The root directory is placed immediately after the 127-byte header.
    assert!(
        reader.header().root_dir_offset > 0,
        "root_dir_offset should be > 0 (comes after the 127-byte header)"
    );
    assert_eq!(reader.header().spec_version, 3);
}
