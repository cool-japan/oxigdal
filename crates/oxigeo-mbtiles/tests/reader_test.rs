//! Integration tests for the SQLite-backed [`oxigeo_mbtiles::MBTilesReader`].
//!
//! All tests in this file require the `sqlite` cargo feature.  The test
//! harness builds a real on-disk SQLite database in a per-test temporary file
//! (via [`std::env::temp_dir`]), exercises the reader, then cleans up.
//!
//! Test fixtures are created using the same OxiSQL engine as the reader itself
//! (no C FFI in this test file).
//!
//! `unwrap_used`, `expect_used` and `panic` are allowed here because this is
//! a tests file — a panic IS the intended outcome of a failed assertion.

#![cfg(feature = "sqlite")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use oxigeo_mbtiles::{MBTilesReader, MbTilesError, TileCoord, TileFormat};

// ── Fixture helpers ───────────────────────────────────────────────────────────

/// Generate a unique temp file path for the test fixture database.
fn unique_tmp(label: &str) -> std::path::PathBuf {
    static CTR: AtomicU64 = AtomicU64::new(0);
    let n = CTR.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let mut p = std::env::temp_dir();
    p.push(format!("oxigeo_mbtiles_test_{label}_{pid}_{n}.sqlite"));
    p
}

/// Execute SQL via a one-shot OxiSQL connection and immediately close it.
///
/// Panics on any error — this is a test-only helper.
fn exec_fixture_sql(path: &Path, sql: &str) {
    use oxisql_core::Connection;
    use oxisql_sqlite_compat::SqliteConnection;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("rt");
    let path_str = path.to_string_lossy().into_owned();
    let conn = rt
        .block_on(SqliteConnection::open(&path_str))
        .expect("open");
    rt.block_on(conn.execute_batch(sql)).expect("execute_batch");
}

/// Build the standard test archive matching the W1 spec:
///
/// * metadata: name, format=pbf, bounds, minzoom=0, maxzoom=14
/// * tiles: (0,0,0)=deadbeef, (1,0,0)=cafebabe, (1,0,1)=00010203
fn build_test_mbtiles(path: &Path) {
    use oxisql_core::{Connection, ToSqlValue};
    use oxisql_sqlite_compat::SqliteConnection;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("rt");
    let path_str = path.to_string_lossy().into_owned();
    let conn = rt
        .block_on(SqliteConnection::open(&path_str))
        .expect("open");

    rt.block_on(conn.execute_batch(
        "CREATE TABLE metadata (name TEXT, value TEXT);
         CREATE TABLE tiles (
             zoom_level  INTEGER,
             tile_column INTEGER,
             tile_row    INTEGER,
             tile_data   BLOB
         );",
    ))
    .expect("create schema");

    for (name, value) in &[
        ("name", "test"),
        ("format", "pbf"),
        ("bounds", "-180,-85,180,85"),
        ("minzoom", "0"),
        ("maxzoom", "14"),
    ] {
        rt.block_on(conn.execute(
            "INSERT INTO metadata (name, value) VALUES ($1, $2)",
            &[name as &dyn ToSqlValue, value as &dyn ToSqlValue],
        ))
        .expect("insert metadata");
    }

    let tiles: &[(i64, i64, i64, Vec<u8>)] = &[
        (0, 0, 0, vec![0xde, 0xad, 0xbe, 0xef]),
        (1, 0, 0, vec![0xca, 0xfe, 0xba, 0xbe]),
        (1, 0, 1, vec![0x00, 0x01, 0x02, 0x03]),
    ];
    for (z, col, row, blob) in tiles {
        let blob_owned: Vec<u8> = blob.clone();
        rt.block_on(conn.execute(
            "INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) VALUES ($1, $2, $3, $4)",
            &[z as &dyn ToSqlValue, col, row, &blob_owned],
        ))
        .expect("insert tile");
    }

    // Force a WAL checkpoint so the on-disk file is self-contained.
    // Without this, std::fs::read may capture only the WAL header and miss the
    // committed pages — which breaks open_in_memory (byte-buffer round-trip).
    rt.block_on(conn.execute_batch("PRAGMA wal_checkpoint"))
        .expect("wal_checkpoint");
}

/// Build an archive carrying every canonical MBTiles 1.3 metadata key plus a
/// handful of vendor extension keys.
fn build_full_metadata_mbtiles(path: &Path) {
    use oxisql_core::{Connection, ToSqlValue};
    use oxisql_sqlite_compat::SqliteConnection;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("rt");
    let path_str = path.to_string_lossy().into_owned();
    let conn = rt
        .block_on(SqliteConnection::open(&path_str))
        .expect("open");

    rt.block_on(conn.execute_batch(
        "CREATE TABLE metadata (name TEXT, value TEXT);
         CREATE TABLE tiles (
             zoom_level  INTEGER,
             tile_column INTEGER,
             tile_row    INTEGER,
             tile_data   BLOB
         );",
    ))
    .expect("create schema");

    for (name, value) in &[
        ("name", "full"),
        ("format", "png"),
        ("bounds", "-10.5,-20.25,30.75,40.125"),
        ("center", "5.0,10.0,7"),
        ("minzoom", "2"),
        ("maxzoom", "9"),
        ("attribution", "(c) OxiGeo"),
        ("description", "A complete metadata fixture"),
        ("type", "overlay"),
        ("version", "1.3.0"),
        ("json", "{\"vector_layers\":[]}"),
        ("vendor_a", "value-a"),
        ("vendor_b", "value-b"),
    ] {
        rt.block_on(conn.execute(
            "INSERT INTO metadata (name, value) VALUES ($1, $2)",
            &[name as &dyn ToSqlValue, value as &dyn ToSqlValue],
        ))
        .expect("insert metadata");
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

// Test 1: opens a minimal valid archive without error.
#[test]
fn test_reader_opens_minimal_valid_archive() {
    let path = unique_tmp("t01");
    build_test_mbtiles(&path);
    let reader = MBTilesReader::open(&path).expect("open reader");
    assert_eq!(reader.metadata().name.as_deref(), Some("test"));
    let _ = std::fs::remove_file(&path);
}

// Test 2: every canonical metadata key is parsed into the typed field.
#[test]
fn test_reader_load_metadata_canonical_keys() {
    let path = unique_tmp("t02");
    build_full_metadata_mbtiles(&path);
    let reader = MBTilesReader::open(&path).expect("open reader");
    let m = reader.metadata();

    assert_eq!(m.name.as_deref(), Some("full"));
    assert_eq!(m.format, Some(TileFormat::Png));
    assert_eq!(m.minzoom, Some(2));
    assert_eq!(m.maxzoom, Some(9));
    assert_eq!(m.attribution.as_deref(), Some("(c) OxiGeo"));
    assert_eq!(
        m.description.as_deref(),
        Some("A complete metadata fixture")
    );
    assert_eq!(m.tile_type.as_deref(), Some("overlay"));
    assert_eq!(m.version.as_deref(), Some("1.3.0"));
    assert_eq!(m.json.as_deref(), Some("{\"vector_layers\":[]}"));
    let _ = std::fs::remove_file(&path);
}

// Test 3: bounds CSV "minlon,minlat,maxlon,maxlat" parsed into [f64; 4].
#[test]
fn test_reader_load_metadata_bounds_parsed() {
    let path = unique_tmp("t03");
    build_full_metadata_mbtiles(&path);
    let reader = MBTilesReader::open(&path).expect("open reader");
    let bounds = reader.metadata().bounds.expect("bounds present");
    assert!((bounds[0] - -10.5).abs() < 1e-12);
    assert!((bounds[1] - -20.25).abs() < 1e-12);
    assert!((bounds[2] - 30.75).abs() < 1e-12);
    assert!((bounds[3] - 40.125).abs() < 1e-12);
    let _ = std::fs::remove_file(&path);
}

// Test 4: center CSV "lon,lat,zoom" parsed into [f64; 3].
#[test]
fn test_reader_load_metadata_center_parsed() {
    let path = unique_tmp("t04");
    build_full_metadata_mbtiles(&path);
    let reader = MBTilesReader::open(&path).expect("open reader");
    let center = reader.metadata().center.expect("center present");
    assert!((center[0] - 5.0).abs() < 1e-12);
    assert!((center[1] - 10.0).abs() < 1e-12);
    assert!((center[2] - 7.0).abs() < 1e-12);
    let _ = std::fs::remove_file(&path);
}

// Test 5: unknown metadata keys land in `extras`.
#[test]
fn test_reader_load_metadata_handles_extra_keys() {
    let path = unique_tmp("t05");
    build_full_metadata_mbtiles(&path);
    let reader = MBTilesReader::open(&path).expect("open reader");
    let extras = reader.metadata().extras();
    assert_eq!(extras.get("vendor_a").map(String::as_str), Some("value-a"));
    assert_eq!(extras.get("vendor_b").map(String::as_str), Some("value-b"));
    // Canonical keys must NOT leak into extras.
    assert!(!extras.contains_key("name"));
    assert!(!extras.contains_key("bounds"));
    assert!(!extras.contains_key("minzoom"));
    let _ = std::fs::remove_file(&path);
}

// Test 6: an archive lacking every optional metadata key is still accepted.
#[test]
fn test_reader_load_metadata_missing_optional_keys_ok() {
    let path = unique_tmp("t06");
    exec_fixture_sql(
        &path,
        "CREATE TABLE metadata (name TEXT, value TEXT);
         CREATE TABLE tiles (
             zoom_level  INTEGER,
             tile_column INTEGER,
             tile_row    INTEGER,
             tile_data   BLOB
         );",
    );
    let reader = MBTilesReader::open(&path).expect("open reader");
    let m = reader.metadata();
    assert!(m.name.is_none());
    assert!(m.format.is_none());
    assert!(m.bounds.is_none());
    assert!(m.center.is_none());
    assert!(m.minzoom.is_none());
    assert!(m.maxzoom.is_none());
    assert!(m.extras().is_empty());
    assert_eq!(reader.tile_count().expect("count"), 0);
    let _ = std::fs::remove_file(&path);
}

// Test 7: get_tile round-trip — bytes inserted via SQL come back identical.
#[test]
fn test_reader_get_tile_round_trip() {
    let path = unique_tmp("t07");
    build_test_mbtiles(&path);
    let reader = MBTilesReader::open(&path).expect("open reader");

    let tile_000 = reader
        .get_tile(&TileCoord { z: 0, x: 0, y: 0 })
        .expect("query (0,0,0)")
        .expect("tile present");
    assert_eq!(tile_000, vec![0xde, 0xad, 0xbe, 0xef]);

    let tile_100 = reader
        .get_tile(&TileCoord { z: 1, x: 0, y: 0 })
        .expect("query (1,0,0)")
        .expect("tile present");
    assert_eq!(tile_100, vec![0xca, 0xfe, 0xba, 0xbe]);

    let tile_101 = reader
        .get_tile(&TileCoord { z: 1, x: 0, y: 1 })
        .expect("query (1,0,1)")
        .expect("tile present");
    assert_eq!(tile_101, vec![0x00, 0x01, 0x02, 0x03]);
    let _ = std::fs::remove_file(&path);
}

// Test 8: requesting a non-existent tile returns Ok(None).
#[test]
fn test_reader_get_tile_missing_returns_none() {
    let path = unique_tmp("t08");
    build_test_mbtiles(&path);
    let reader = MBTilesReader::open(&path).expect("open reader");

    let missing = reader
        .get_tile(&TileCoord { z: 5, x: 17, y: 23 })
        .expect("query missing");
    assert!(missing.is_none());
    let _ = std::fs::remove_file(&path);
}

// Test 9: tile_count matches the number of rows inserted.
#[test]
fn test_reader_tile_count_matches_inserted() {
    let path = unique_tmp("t09");
    build_test_mbtiles(&path);
    let reader = MBTilesReader::open(&path).expect("open reader");
    assert_eq!(reader.tile_count().expect("count"), 3);
    let _ = std::fs::remove_file(&path);
}

// Test 10: zoom_levels returns DISTINCT values, sorted ascending.
#[test]
fn test_reader_zoom_levels_distinct_sorted() {
    use oxisql_core::{Connection, ToSqlValue};
    use oxisql_sqlite_compat::SqliteConnection;

    let path = unique_tmp("t10");
    {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("rt");
        let path_str = path.to_string_lossy().into_owned();
        let conn = rt
            .block_on(SqliteConnection::open(&path_str))
            .expect("open");

        rt.block_on(conn.execute_batch(
            "CREATE TABLE metadata (name TEXT, value TEXT);
             CREATE TABLE tiles (
                 zoom_level  INTEGER,
                 tile_column INTEGER,
                 tile_row    INTEGER,
                 tile_data   BLOB
             );",
        ))
        .expect("schema");

        let rows: &[(i64, i64, i64, Vec<u8>)] = &[
            (3, 0, 0, vec![0x00]),
            (1, 0, 0, vec![0x01]),
            (5, 2, 3, vec![0x02]),
            (1, 1, 0, vec![0x03]),
            (3, 1, 1, vec![0x04]),
            (0, 0, 0, vec![0x05]),
        ];
        for (z, col, row, blob) in rows {
            let b: Vec<u8> = blob.clone();
            rt.block_on(conn.execute(
                "INSERT INTO tiles VALUES ($1, $2, $3, $4)",
                &[z as &dyn ToSqlValue, col, row, &b],
            ))
            .expect("insert");
        }
    }

    let reader = MBTilesReader::open(&path).expect("open reader");
    let zooms = reader.zoom_levels().expect("zoom levels");
    assert_eq!(zooms, vec![0, 1, 3, 5]);
    let _ = std::fs::remove_file(&path);
}

// Test 11: list_tiles enumerates every row ordered by (z, x, y) ascending.
#[test]
fn test_reader_list_tiles_ordered_by_z_x_y() {
    use oxisql_core::{Connection, ToSqlValue};
    use oxisql_sqlite_compat::SqliteConnection;

    let path = unique_tmp("t11");
    {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("rt");
        let path_str = path.to_string_lossy().into_owned();
        let conn = rt
            .block_on(SqliteConnection::open(&path_str))
            .expect("open");

        rt.block_on(conn.execute_batch(
            "CREATE TABLE metadata (name TEXT, value TEXT);
             CREATE TABLE tiles (
                 zoom_level  INTEGER,
                 tile_column INTEGER,
                 tile_row    INTEGER,
                 tile_data   BLOB
             );",
        ))
        .expect("schema");

        let rows: &[(i64, i64, i64)] = &[(1, 1, 1), (1, 0, 1), (1, 0, 0), (1, 1, 0), (0, 0, 0)];
        let blob: Vec<u8> = vec![0x00];
        for (z, col, row) in rows {
            let b: Vec<u8> = blob.clone();
            rt.block_on(conn.execute(
                "INSERT INTO tiles VALUES ($1, $2, $3, $4)",
                &[z as &dyn ToSqlValue, col, row, &b],
            ))
            .expect("insert");
        }
    }

    let reader = MBTilesReader::open(&path).expect("open reader");
    let listed = reader.list_tiles().expect("list tiles");
    assert_eq!(
        listed,
        vec![
            TileCoord { z: 0, x: 0, y: 0 },
            TileCoord { z: 1, x: 0, y: 0 },
            TileCoord { z: 1, x: 0, y: 1 },
            TileCoord { z: 1, x: 1, y: 0 },
            TileCoord { z: 1, x: 1, y: 1 },
        ]
    );
    let _ = std::fs::remove_file(&path);
}

// Test 12: into_mbtiles eagerly materialises every tile, preserving bytes.
#[test]
fn test_reader_into_mbtiles_preserves_all_tiles() {
    let path = unique_tmp("t12");
    build_test_mbtiles(&path);
    let reader = MBTilesReader::open(&path).expect("open reader");

    let store = reader.into_mbtiles().expect("materialise");
    assert_eq!(store.tile_count(), 3);

    let t000 = store
        .get_tile(&TileCoord { z: 0, x: 0, y: 0 })
        .expect("tile (0,0,0) present");
    assert_eq!(t000.as_slice(), &[0xde, 0xad, 0xbe, 0xef]);

    let t100 = store
        .get_tile(&TileCoord { z: 1, x: 0, y: 0 })
        .expect("tile (1,0,0) present");
    assert_eq!(t100.as_slice(), &[0xca, 0xfe, 0xba, 0xbe]);

    let t101 = store
        .get_tile(&TileCoord { z: 1, x: 0, y: 1 })
        .expect("tile (1,0,1) present");
    assert_eq!(t101.as_slice(), &[0x00, 0x01, 0x02, 0x03]);

    // Metadata is preserved on the materialised store too.
    assert_eq!(store.metadata.name.as_deref(), Some("test"));
    assert_eq!(store.metadata.format, Some(TileFormat::Pbf));
    assert_eq!(store.metadata.maxzoom, Some(14));
    let _ = std::fs::remove_file(&path);
}

// Test 13: archives missing the `tiles` table are rejected.
#[test]
fn test_reader_rejects_missing_tiles_table() {
    let path = unique_tmp("t13");
    exec_fixture_sql(&path, "CREATE TABLE metadata (name TEXT, value TEXT);");

    let err = MBTilesReader::open(&path).unwrap_err();
    match err {
        MbTilesError::InvalidFormat(msg) => assert!(msg.contains("tiles"), "msg={msg}"),
        other => panic!("expected InvalidFormat, got {other:?}"),
    }
    let _ = std::fs::remove_file(&path);
}

// Test 14: archives missing the `metadata` table are rejected.
#[test]
fn test_reader_rejects_missing_metadata_table() {
    let path = unique_tmp("t14");
    exec_fixture_sql(
        &path,
        "CREATE TABLE tiles (
             zoom_level  INTEGER,
             tile_column INTEGER,
             tile_row    INTEGER,
             tile_data   BLOB
         );",
    );
    let err = MBTilesReader::open(&path).unwrap_err();
    match err {
        MbTilesError::InvalidFormat(msg) => assert!(msg.contains("metadata"), "msg={msg}"),
        other => panic!("expected InvalidFormat, got {other:?}"),
    }
    let _ = std::fs::remove_file(&path);
}

// Test 15: a malformed `bounds` CSV surfaces InvalidMetadata.
#[test]
fn test_reader_invalid_bounds_csv_returns_error() {
    use oxisql_core::{Connection, ToSqlValue};
    use oxisql_sqlite_compat::SqliteConnection;

    let path = unique_tmp("t15");
    {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("rt");
        let path_str = path.to_string_lossy().into_owned();
        let conn = rt
            .block_on(SqliteConnection::open(&path_str))
            .expect("open");
        rt.block_on(conn.execute_batch(
            "CREATE TABLE metadata (name TEXT, value TEXT);
             CREATE TABLE tiles (
                 zoom_level  INTEGER,
                 tile_column INTEGER,
                 tile_row    INTEGER,
                 tile_data   BLOB
             );",
        ))
        .expect("schema");
        let k: &str = "bounds";
        let v: &str = "not,a,valid,bbox";
        rt.block_on(conn.execute(
            "INSERT INTO metadata (name, value) VALUES ($1, $2)",
            &[&k as &dyn ToSqlValue, &v],
        ))
        .expect("insert");
    }

    let err = MBTilesReader::open(&path).unwrap_err();
    match err {
        MbTilesError::InvalidMetadata(msg) => assert!(msg.contains("bounds"), "msg={msg}"),
        other => panic!("expected InvalidMetadata, got {other:?}"),
    }
    let _ = std::fs::remove_file(&path);
}

// Bonus: in-memory byte buffer reader (covers Self::open_in_memory).
#[test]
fn test_reader_open_in_memory_round_trip() {
    // Build a real archive on disk first, slurp its bytes, then re-open via
    // the in-memory path and verify identical results.
    let path = unique_tmp("t_inmem");
    build_test_mbtiles(&path);
    let bytes = std::fs::read(&path).expect("read bytes");
    let _ = std::fs::remove_file(&path);

    let reader = MBTilesReader::open_in_memory(&bytes).expect("open in memory");
    assert_eq!(reader.tile_count().expect("count"), 3);
    assert_eq!(reader.metadata().format, Some(TileFormat::Pbf));
    let listed = reader.list_tiles().expect("list tiles");
    assert_eq!(listed.len(), 3);
}
