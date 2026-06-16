//! Integration tests for the `mbtiles` feature — PMTiles → MBTiles export.
//!
//! All tests are gated on `#[cfg(feature = "mbtiles")]` so that the test
//! suite compiles cleanly without the feature enabled.
//!
//! Database verification queries use [`MbTilesConn`]'s helper methods
//! (`query_count`, `query_text`, `query_blob`) backed by the Pure-Rust
//! OxiSQL engine — no C/FFI, no `libsqlite3`.

#![cfg(feature = "mbtiles")]
#![allow(clippy::expect_used)]

use oxigdal_pmtiles::writer::PmTilesBuilder;
use oxigdal_pmtiles::{MbTilesConn, MbTilesExportStats, MbTilesExporter, PmTilesReader, TileType};

// ---------------------------------------------------------------------------
// Helper builders
// ---------------------------------------------------------------------------

/// Build an empty PMTiles archive with the given tile type and zoom range.
fn empty_archive(tile_type: TileType, min_zoom: u8, max_zoom: u8) -> PmTilesReader {
    let builder = PmTilesBuilder::new(tile_type, min_zoom, max_zoom);
    let bytes = builder.build().expect("build empty archive");
    PmTilesReader::from_bytes(bytes).expect("parse empty archive")
}

/// Build a PMTiles archive with three distinct tiles at z=0..=2.
fn three_tile_archive() -> PmTilesReader {
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 2);
    builder.add_tile(0, 0, 0, b"tile-z0").expect("add z0");
    builder.add_tile(1, 0, 0, b"tile-z1-00").expect("add z1");
    builder.add_tile(2, 0, 0, b"tile-z2-00").expect("add z2");
    let bytes = builder.build().expect("build three-tile archive");
    PmTilesReader::from_bytes(bytes).expect("parse three-tile archive")
}

// ---------------------------------------------------------------------------
// Test 1: export to in-memory connection creates schema
// ---------------------------------------------------------------------------

#[test]
fn test_export_to_memory_creates_schema() {
    let reader = empty_archive(TileType::Png, 0, 2);
    let exporter = MbTilesExporter::new(&reader);
    let db = MbTilesConn::open_memory().expect("open in-memory db");
    exporter
        .export_to_connection(&db)
        .expect("export should succeed");

    // Verify `metadata` table exists and has at least the required keys.
    let metadata_count = db
        .query_count("SELECT COUNT(*) FROM metadata", &[])
        .expect("query metadata count");
    assert!(
        metadata_count >= 5,
        "expected at least 5 metadata rows (name, format, bounds, minzoom, maxzoom), got {metadata_count}"
    );

    // Verify `tiles` table exists (query returns 0 rows for empty archive).
    let tile_count = db
        .query_count("SELECT COUNT(*) FROM tiles", &[])
        .expect("query tile count");
    assert_eq!(tile_count, 0, "empty archive should produce 0 tile rows");
}

// ---------------------------------------------------------------------------
// Test 2: export empty archive writes no tiles
// ---------------------------------------------------------------------------

#[test]
fn test_export_empty_archive_writes_no_tiles() {
    let reader = empty_archive(TileType::Png, 0, 4);
    let exporter = MbTilesExporter::new(&reader);
    let db = MbTilesConn::open_memory().expect("open in-memory db");
    let stats: MbTilesExportStats = exporter
        .export_to_connection(&db)
        .expect("export should succeed");

    assert_eq!(
        stats.tiles_written, 0,
        "empty archive: tiles_written should be 0"
    );
    assert_eq!(
        stats.bytes_written, 0,
        "empty archive: bytes_written should be 0"
    );
    // Zoom levels default to 0/0 when no tiles are written.
    assert_eq!(stats.min_zoom, 0);
    assert_eq!(stats.max_zoom, 0);
}

// ---------------------------------------------------------------------------
// Test 3: export three tiles writes three rows
// ---------------------------------------------------------------------------

#[test]
fn test_export_three_tiles_writes_three_rows() {
    let reader = three_tile_archive();
    let exporter = MbTilesExporter::new(&reader);
    let db = MbTilesConn::open_memory().expect("open in-memory db");
    let stats = exporter
        .export_to_connection(&db)
        .expect("export should succeed");

    assert_eq!(stats.tiles_written, 3, "expected 3 tile rows written");

    let db_count = db
        .query_count("SELECT COUNT(*) FROM tiles", &[])
        .expect("query tile count");
    assert_eq!(db_count, 3, "database should contain exactly 3 tile rows");
}

// ---------------------------------------------------------------------------
// Test 4: required metadata fields are present
// ---------------------------------------------------------------------------

#[test]
fn test_export_metadata_includes_required_fields() {
    let reader = empty_archive(TileType::Png, 2, 8);
    let exporter = MbTilesExporter::new(&reader);
    let db = MbTilesConn::open_memory().expect("open in-memory db");
    exporter
        .export_to_connection(&db)
        .expect("export should succeed");

    let required_keys = ["name", "format", "bounds", "minzoom", "maxzoom"];
    for key in &required_keys {
        let key_str: &str = key;
        let found = db
            .query_count("SELECT COUNT(*) FROM metadata WHERE name = $1", &[&key_str])
            .expect("query metadata key");
        assert_eq!(
            found, 1,
            "required metadata key '{key}' should be present exactly once"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 5: format = "png" when tile type is Png
// ---------------------------------------------------------------------------

#[test]
fn test_export_metadata_format_png_when_tile_type_png() {
    let reader = empty_archive(TileType::Png, 0, 2);
    let exporter = MbTilesExporter::new(&reader);
    let db = MbTilesConn::open_memory().expect("open in-memory db");
    exporter
        .export_to_connection(&db)
        .expect("export should succeed");

    let format = db
        .query_text("SELECT value FROM metadata WHERE name = 'format'", &[])
        .expect("query format metadata")
        .expect("format metadata should be present");
    assert_eq!(format, "png", "PNG tile type should produce format = 'png'");
}

// ---------------------------------------------------------------------------
// Test 6: format = "pbf" when tile type is Mvt
// ---------------------------------------------------------------------------

#[test]
fn test_export_metadata_format_pbf_when_tile_type_mvt() {
    let reader = empty_archive(TileType::Mvt, 0, 14);
    let exporter = MbTilesExporter::new(&reader);
    let db = MbTilesConn::open_memory().expect("open in-memory db");
    exporter
        .export_to_connection(&db)
        .expect("export should succeed");

    let format = db
        .query_text("SELECT value FROM metadata WHERE name = 'format'", &[])
        .expect("query format metadata")
        .expect("format metadata should be present");
    assert_eq!(format, "pbf", "MVT tile type should produce format = 'pbf'");
}

// ---------------------------------------------------------------------------
// Tests 7–9: TMS row conversion (pure arithmetic — no archive needed)
// ---------------------------------------------------------------------------

/// Local mirror of the inline TMS conversion used in `export_to_connection`.
fn xyz_to_tms(z: u8, y: u32) -> u32 {
    (1u32 << z).wrapping_sub(1).wrapping_sub(y)
}

#[test]
fn test_tms_row_conversion_zoom_0_y_0_returns_0() {
    // z=0: 2^0 - 1 - 0 = 0.  The single tile at zoom-0 has TMS row 0.
    assert_eq!(xyz_to_tms(0, 0), 0);
}

#[test]
fn test_tms_row_conversion_zoom_2_y_3_returns_0() {
    // z=2: 2^2 - 1 - 3 = 4 - 1 - 3 = 0.  Bottom row of a 4×4 grid.
    assert_eq!(xyz_to_tms(2, 3), 0);
}

#[test]
fn test_tms_row_conversion_zoom_10_y_500_returns_523() {
    // z=10: 2^10 - 1 - 500 = 1024 - 1 - 500 = 523.
    assert_eq!(xyz_to_tms(10, 500), 523);
}

// ---------------------------------------------------------------------------
// Test 10: export to file creates a valid SQLite database
// ---------------------------------------------------------------------------

#[test]
fn test_export_to_file_creates_valid_sqlite() {
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 1);
    builder
        .add_tile(0, 0, 0, &[1u8, 2, 3, 4])
        .expect("add tile");
    builder
        .add_tile(1, 0, 0, &[5u8, 6, 7, 8])
        .expect("add tile");
    let bytes = builder.build().expect("build archive");
    let reader = PmTilesReader::from_bytes(bytes).expect("parse archive");

    let tmp_path = std::env::temp_dir().join("oxigdal_mbtiles_export_test_file.mbtiles");
    let exporter = MbTilesExporter::new(&reader);
    let stats = exporter
        .export_to_path(&tmp_path)
        .expect("export to file should succeed");

    assert_eq!(stats.tiles_written, 2);

    // Re-open the file with a new connection and verify the tile count.
    let verify_db = MbTilesConn::open(&tmp_path).expect("re-open exported file");
    let db_count = verify_db
        .query_count("SELECT COUNT(*) FROM tiles", &[])
        .expect("query tile count");
    assert_eq!(db_count, 2, "exported file should contain 2 tile rows");

    let _ = std::fs::remove_file(&tmp_path);
}

// ---------------------------------------------------------------------------
// Test 11: round-trip — tile data bytes survive export → query unchanged
// ---------------------------------------------------------------------------

#[test]
fn test_export_round_trip_tile_data_byte_exact() {
    // PNG magic bytes: \x89PNG\r\n\x1a\n
    let png_magic: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 0);
    builder.add_tile(0, 0, 0, png_magic).expect("add tile");
    let bytes = builder.build().expect("build archive");
    let reader = PmTilesReader::from_bytes(bytes).expect("parse archive");

    let exporter = MbTilesExporter::new(&reader);
    let db = MbTilesConn::open_memory().expect("open in-memory db");
    let stats = exporter
        .export_to_connection(&db)
        .expect("export should succeed");

    assert_eq!(stats.tiles_written, 1);
    assert_eq!(stats.bytes_written, png_magic.len() as u64);

    // TMS row for z=0, y=0 is 0.
    let stored = db
        .query_blob(
            "SELECT tile_data FROM tiles \
             WHERE zoom_level = 0 AND tile_column = 0 AND tile_row = 0",
            &[],
        )
        .expect("query tile data")
        .expect("tile data should be present");

    assert_eq!(
        stored, png_magic,
        "stored tile bytes must match the original payload byte-for-byte"
    );
}

// ---------------------------------------------------------------------------
// Test 12: export to same path twice — second export succeeds (overwrite)
// ---------------------------------------------------------------------------

#[test]
fn test_export_to_path_overwrites_existing() {
    let tmp_path = std::env::temp_dir().join("oxigdal_mbtiles_export_test_overwrite.mbtiles");

    let reader1 = empty_archive(TileType::Png, 0, 2);
    let exporter1 = MbTilesExporter::new(&reader1);
    exporter1
        .export_to_path(&tmp_path)
        .expect("first export should succeed");

    assert!(tmp_path.exists(), "file should exist after first export");

    // Second export to the same path must succeed without error.
    let mut builder2 = PmTilesBuilder::new(TileType::Mvt, 0, 5);
    builder2
        .add_tile(0, 0, 0, b"second-tile")
        .expect("add tile");
    let bytes2 = builder2.build().expect("build second archive");
    let reader2 = PmTilesReader::from_bytes(bytes2).expect("parse second archive");
    let exporter2 = MbTilesExporter::new(&reader2);
    let stats2 = exporter2
        .export_to_path(&tmp_path)
        .expect("second export (overwrite) should succeed");

    assert_eq!(
        stats2.tiles_written, 1,
        "second export should have written exactly 1 tile"
    );

    // Verify the overwritten file reflects the second export's content.
    let verify_db = MbTilesConn::open(&tmp_path).expect("open overwritten file");
    let format = verify_db
        .query_text("SELECT value FROM metadata WHERE name = 'format'", &[])
        .expect("query format from overwritten db")
        .expect("format metadata should be present");
    assert_eq!(
        format, "pbf",
        "overwritten file should reflect second archive's tile type (MVT → pbf)"
    );

    let _ = std::fs::remove_file(&tmp_path);
}

// ---------------------------------------------------------------------------
// Test 13: zoom range in stats reflects actual tile zoom levels
// ---------------------------------------------------------------------------

#[test]
fn test_export_stats_zoom_range_reflects_tiles() {
    let mut builder = PmTilesBuilder::new(TileType::Png, 0, 3);
    builder.add_tile(1, 0, 0, b"z1").expect("add z1");
    builder.add_tile(3, 0, 0, b"z3").expect("add z3");
    let bytes = builder.build().expect("build");
    let reader = PmTilesReader::from_bytes(bytes).expect("parse");

    let exporter = MbTilesExporter::new(&reader);
    let db = MbTilesConn::open_memory().expect("open in-memory db");
    let stats = exporter.export_to_connection(&db).expect("export");

    assert_eq!(stats.tiles_written, 2);
    assert_eq!(stats.min_zoom, 1, "min_zoom should be 1");
    assert_eq!(stats.max_zoom, 3, "max_zoom should be 3");
}
