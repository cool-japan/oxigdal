//! Integration tests for GeoPackage → MBTiles export.
//!
//! These tests build minimal in-memory GeoPackage files using `rusqlite`,
//! then exercise the full `GpkgMbTilesExporter` pipeline.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::needless_borrows_for_generic_args
)]

#[cfg(feature = "mbtiles-export")]
mod tests {
    use std::env;

    use rusqlite::Connection;

    use oxigdal_gpkg::mbtiles_export::{create_schema, detect_format_from_blob, xyz_to_tms_row};
    use oxigdal_gpkg::{GeoPackage, GpkgMbTilesExporter};

    // ─────────────────────────────────────────────────────────────────────────
    // Helpers
    // ─────────────────────────────────────────────────────────────────────────

    /// Generate a unique-ish suffix to avoid temp-file collisions.
    fn unique_suffix() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        format!("{}{}", t.as_secs(), t.subsec_nanos())
    }

    /// Build a minimal GeoPackage SQLite file in a temp path and return its bytes.
    ///
    /// `tiles` is a slice of `(zoom_level, tile_column, tile_row, tile_data)`.
    /// The schema includes:
    /// - `gpkg_spatial_ref_sys` with WGS 84 + two undefined SRS rows
    /// - `gpkg_contents` with one "tiles" row pointing at `"test_tiles"`
    /// - `gpkg_tile_matrix_set` with one row
    /// - `gpkg_tile_matrix` with zoom 0 (1×1) and zoom 1 (2×2)
    /// - `test_tiles` with the given tile rows
    fn make_test_gpkg(tiles: &[(u32, u32, u32, Vec<u8>)]) -> Vec<u8> {
        let dir = env::temp_dir();
        let path = dir.join(format!("test_gpkg_{}.gpkg", unique_suffix()));

        let conn = Connection::open(&path).unwrap();

        conn.execute_batch(
            "
            PRAGMA application_id = 1196444487;
            PRAGMA user_version = 10200;

            CREATE TABLE gpkg_spatial_ref_sys (
                srs_name                 TEXT    NOT NULL,
                srs_id                   INTEGER NOT NULL PRIMARY KEY,
                organization             TEXT    NOT NULL,
                organization_coordsys_id INTEGER NOT NULL,
                definition               TEXT    NOT NULL,
                description              TEXT
            );
            INSERT INTO gpkg_spatial_ref_sys VALUES
                ('WGS 84 geodetic', 4326, 'EPSG', 4326, 'undefined', NULL),
                ('undefined geographic CRS', 0, 'NONE', 0, 'undefined', NULL),
                ('undefined Cartesian CRS', -1, 'NONE', -1, 'undefined', NULL);

            CREATE TABLE gpkg_contents (
                table_name  TEXT     NOT NULL PRIMARY KEY,
                data_type   TEXT     NOT NULL,
                identifier  TEXT,
                description TEXT     DEFAULT '',
                last_change DATETIME NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S.000fZ','now')),
                min_x       REAL,
                min_y       REAL,
                max_x       REAL,
                max_y       REAL,
                srs_id      INTEGER
            );
            INSERT INTO gpkg_contents
                VALUES ('test_tiles','tiles','test_tiles','','2024-01-01T00:00:00.000fZ',
                        -180.0,-90.0,180.0,90.0,4326);

            CREATE TABLE gpkg_tile_matrix_set (
                table_name TEXT    NOT NULL PRIMARY KEY,
                srs_id     INTEGER NOT NULL,
                min_x      REAL    NOT NULL,
                min_y      REAL    NOT NULL,
                max_x      REAL    NOT NULL,
                max_y      REAL    NOT NULL
            );
            INSERT INTO gpkg_tile_matrix_set
                VALUES ('test_tiles', 4326, -180.0, -90.0, 180.0, 90.0);

            CREATE TABLE gpkg_tile_matrix (
                table_name   TEXT    NOT NULL,
                zoom_level   INTEGER NOT NULL,
                matrix_width INTEGER NOT NULL,
                matrix_height INTEGER NOT NULL,
                tile_width   INTEGER NOT NULL DEFAULT 256,
                tile_height  INTEGER NOT NULL DEFAULT 256,
                pixel_x_size REAL    NOT NULL,
                pixel_y_size REAL    NOT NULL,
                PRIMARY KEY (table_name, zoom_level)
            );
            INSERT INTO gpkg_tile_matrix VALUES
                ('test_tiles', 0, 1, 1, 256, 256, 0.703125,   0.703125),
                ('test_tiles', 1, 2, 2, 256, 256, 0.3515625,  0.3515625);

            CREATE TABLE test_tiles (
                zoom_level  INTEGER NOT NULL,
                tile_column INTEGER NOT NULL,
                tile_row    INTEGER NOT NULL,
                tile_data   BLOB    NOT NULL,
                PRIMARY KEY (zoom_level, tile_column, tile_row)
            );
            ",
        )
        .unwrap();

        for (z, col, row, blob) in tiles {
            conn.execute(
                "INSERT INTO test_tiles (zoom_level, tile_column, tile_row, tile_data) \
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![z, col, row, blob.as_slice()],
            )
            .unwrap();
        }

        drop(conn);
        std::fs::read(&path).unwrap()
    }

    /// A fake PNG blob (starts with `\x89PNG`).
    fn fake_png() -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
        v.extend_from_slice(&[0u8; 16]);
        v
    }

    /// A fake JPEG blob (starts with `\xff\xd8\xff`).
    fn fake_jpeg() -> Vec<u8> {
        let mut v = b"\xff\xd8\xff\xe0".to_vec();
        v.extend_from_slice(&[0u8; 16]);
        v
    }

    /// A fake WebP blob (`RIFF....WEBP`).
    fn fake_webp() -> Vec<u8> {
        let mut v = b"RIFF\x00\x00\x00\x00WEBP".to_vec();
        v.extend_from_slice(&[0u8; 16]);
        v
    }

    // ─────────────────────────────────────────────────────────────────────────
    // xyz_to_tms_row
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_xyz_to_tms_row_conversion_zoom_0() {
        // At zoom 0, single tile row 0 maps to TMS row 0.
        assert_eq!(xyz_to_tms_row(0, 0), 0);
    }

    #[test]
    fn test_xyz_to_tms_row_conversion_zoom_10() {
        // tms(10, 500) = 2^10 - 1 - 500 = 1023 - 500 = 523
        assert_eq!(xyz_to_tms_row(10, 500), 523);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // detect_format_from_blob
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_detect_format_png_magic() {
        assert_eq!(detect_format_from_blob(&fake_png()), "png");
    }

    #[test]
    fn test_detect_format_jpeg_magic() {
        assert_eq!(detect_format_from_blob(&fake_jpeg()), "jpg");
    }

    #[test]
    fn test_detect_format_webp_magic() {
        assert_eq!(detect_format_from_blob(&fake_webp()), "webp");
    }

    #[test]
    fn test_detect_format_default_png() {
        // Unknown magic bytes should default to "png".
        assert_eq!(detect_format_from_blob(&[0x00, 0x01, 0x02, 0x03]), "png");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // create_schema
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_create_schema_creates_tiles_and_metadata_tables() {
        let conn = Connection::open_in_memory().unwrap();
        create_schema(&conn).expect("schema creation must not fail");

        // tiles table must exist
        let tile_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tiles", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tile_count, 0);

        // metadata table must exist
        let meta_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM metadata", [], |r| r.get(0))
            .unwrap();
        assert_eq!(meta_count, 0);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // export_to_connection — empty pyramid
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_export_empty_pyramid_writes_no_tiles() {
        // Build a GeoPackage with the tile matrix defined but no actual tile rows.
        let gpkg_bytes = make_test_gpkg(&[]);
        let gpkg = GeoPackage::from_bytes(gpkg_bytes).expect("valid gpkg");

        let exporter =
            GpkgMbTilesExporter::new(&gpkg, "test_tiles").expect("exporter construction");

        let mbtiles_conn = Connection::open_in_memory().unwrap();
        let stats = exporter
            .export_to_connection(&mbtiles_conn)
            .expect("export must succeed");

        assert_eq!(stats.tiles_written, 0, "empty pyramid => 0 tiles written");
        assert_eq!(stats.bytes_written, 0);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // export_to_connection — with tile data
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_export_with_tiles_writes_rows() {
        let png = fake_png();
        let tiles = vec![(0u32, 0u32, 0u32, png.clone())];
        let gpkg_bytes = make_test_gpkg(&tiles);
        let gpkg = GeoPackage::from_bytes(gpkg_bytes).expect("valid gpkg");

        let exporter =
            GpkgMbTilesExporter::new(&gpkg, "test_tiles").expect("exporter construction");

        let mbtiles_conn = Connection::open_in_memory().unwrap();
        let stats = exporter
            .export_to_connection(&mbtiles_conn)
            .expect("export must succeed");

        assert_eq!(stats.tiles_written, 1);
        assert_eq!(stats.bytes_written, png.len() as u64);

        // Verify the tile row exists in the SQLite output
        let count: i64 = mbtiles_conn
            .query_row("SELECT COUNT(*) FROM tiles", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // export metadata — required keys
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_export_metadata_includes_required_fields() {
        let png = fake_png();
        let tiles = vec![(0u32, 0u32, 0u32, png)];
        let gpkg_bytes = make_test_gpkg(&tiles);
        let gpkg = GeoPackage::from_bytes(gpkg_bytes).expect("valid gpkg");

        let exporter =
            GpkgMbTilesExporter::new(&gpkg, "test_tiles").expect("exporter construction");

        let mbtiles_conn = Connection::open_in_memory().unwrap();
        let stats = exporter
            .export_to_connection(&mbtiles_conn)
            .expect("export must succeed");

        // At least 7 metadata rows: name, type, version, description, format, minzoom, maxzoom
        assert!(
            stats.metadata_keys >= 7,
            "expected >= 7 metadata keys, got {}",
            stats.metadata_keys
        );

        // Verify specific keys exist
        for key in &["name", "format", "minzoom", "maxzoom"] {
            let found: i64 = mbtiles_conn
                .query_row(
                    "SELECT COUNT(*) FROM metadata WHERE name = ?1",
                    rusqlite::params![key],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(found, 1, "metadata key '{key}' must be present");
        }

        // Format must equal "png" (detected from fake_png magic bytes)
        let format: String = mbtiles_conn
            .query_row("SELECT value FROM metadata WHERE name='format'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(format, "png");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // export_to_path
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_export_to_path_creates_valid_sqlite_file() {
        let png = fake_png();
        let tiles = vec![(0u32, 0u32, 0u32, png)];
        let gpkg_bytes = make_test_gpkg(&tiles);
        let gpkg = GeoPackage::from_bytes(gpkg_bytes).expect("valid gpkg");

        let exporter =
            GpkgMbTilesExporter::new(&gpkg, "test_tiles").expect("exporter construction");

        let out_path = env::temp_dir().join(format!("out_{}.mbtiles", unique_suffix()));
        let stats = exporter
            .export_to_path(&out_path)
            .expect("export_to_path must succeed");

        assert_eq!(stats.tiles_written, 1);
        assert!(out_path.exists(), "output file must be created");

        // Open the produced file and verify it is a valid SQLite MBTiles.
        let verify_conn = Connection::open(&out_path).unwrap();
        let count: i64 = verify_conn
            .query_row("SELECT COUNT(*) FROM tiles", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);

        // Clean up
        let _ = std::fs::remove_file(&out_path);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Round-trip: tile_data bytes must be byte-exact
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_export_round_trips_tile_data_byte_exact() {
        // Use a synthetic blob with known content — not a real image.
        let payload: Vec<u8> = (0u8..=127u8).collect();
        // Prefix with PNG magic so format detection works cleanly
        let mut blob = b"\x89PNG\r\n\x1a\n".to_vec();
        blob.extend_from_slice(&payload);

        let tiles = vec![(0u32, 0u32, 0u32, blob.clone())];
        let gpkg_bytes = make_test_gpkg(&tiles);
        let gpkg = GeoPackage::from_bytes(gpkg_bytes).expect("valid gpkg");

        let exporter =
            GpkgMbTilesExporter::new(&gpkg, "test_tiles").expect("exporter construction");

        let mbtiles_conn = Connection::open_in_memory().unwrap();
        exporter
            .export_to_connection(&mbtiles_conn)
            .expect("export must succeed");

        // TMS row = xyz_to_tms_row(0, 0) = 0
        let retrieved: Vec<u8> = mbtiles_conn
            .query_row(
                "SELECT tile_data FROM tiles \
                 WHERE zoom_level=0 AND tile_column=0 AND tile_row=0",
                [],
                |r| r.get(0),
            )
            .unwrap();

        assert_eq!(
            retrieved, blob,
            "tile_data must round-trip byte-for-byte through MBTiles"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // XYZ → TMS row conversion inside the export
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_export_applies_tms_row_conversion() {
        // Zoom 1: gpkg_row=0 → tms_row = 2^1 - 1 - 0 = 1
        //         gpkg_row=1 → tms_row = 2^1 - 1 - 1 = 0
        let png = fake_png();
        let tiles = vec![
            (1u32, 0u32, 0u32, png.clone()), // XYZ (1,0,0) → TMS (1,0,1)
            (1u32, 0u32, 1u32, png.clone()), // XYZ (1,0,1) → TMS (1,0,0)
        ];
        let gpkg_bytes = make_test_gpkg(&tiles);
        let gpkg = GeoPackage::from_bytes(gpkg_bytes).expect("valid gpkg");

        let exporter =
            GpkgMbTilesExporter::new(&gpkg, "test_tiles").expect("exporter construction");

        let mbtiles_conn = Connection::open_in_memory().unwrap();
        exporter
            .export_to_connection(&mbtiles_conn)
            .expect("export must succeed");

        // Verify TMS row 1 exists (from gpkg_row=0)
        let exists_row1: i64 = mbtiles_conn
            .query_row(
                "SELECT COUNT(*) FROM tiles WHERE zoom_level=1 AND tile_column=0 AND tile_row=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            exists_row1, 1,
            "TMS row 1 must exist for gpkg_row=0 at zoom 1"
        );

        // Verify TMS row 0 exists (from gpkg_row=1)
        let exists_row0: i64 = mbtiles_conn
            .query_row(
                "SELECT COUNT(*) FROM tiles WHERE zoom_level=1 AND tile_column=0 AND tile_row=0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            exists_row0, 1,
            "TMS row 0 must exist for gpkg_row=1 at zoom 1"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // new() with invalid table name
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_new_returns_error_for_unknown_table() {
        let gpkg_bytes = make_test_gpkg(&[]);
        let gpkg = GeoPackage::from_bytes(gpkg_bytes).expect("valid gpkg");
        let result = GpkgMbTilesExporter::new(&gpkg, "nonexistent_table");
        assert!(result.is_err(), "Unknown table must produce an error");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // stats: min/max zoom
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_export_stats_min_max_zoom() {
        let png = fake_png();
        // Insert tiles at zoom 0 and zoom 1 to exercise the full pyramid.
        let tiles = vec![
            (0u32, 0u32, 0u32, png.clone()),
            (1u32, 0u32, 0u32, png.clone()),
        ];
        let gpkg_bytes = make_test_gpkg(&tiles);
        let gpkg = GeoPackage::from_bytes(gpkg_bytes).expect("valid gpkg");

        let exporter =
            GpkgMbTilesExporter::new(&gpkg, "test_tiles").expect("exporter construction");

        let mbtiles_conn = Connection::open_in_memory().unwrap();
        let stats = exporter
            .export_to_connection(&mbtiles_conn)
            .expect("export must succeed");

        assert_eq!(stats.min_zoom, 0);
        assert_eq!(stats.max_zoom, 1);
    }
}
