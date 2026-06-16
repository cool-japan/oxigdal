//! Integration tests for GeoPackage → MBTiles export.
//!
//! These tests build minimal GeoPackage SQLite files using the Pure-Rust
//! OxiSQL engine (no C/FFI), then exercise the full `GpkgMbTilesExporter`
//! pipeline via `export_to_path`.
//!
//! Readback of the produced MBTiles file also uses OxiSQL.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::needless_borrows_for_generic_args
)]

#[cfg(feature = "mbtiles-export")]
mod tests {
    use std::env;
    use std::sync::atomic::{AtomicU64, Ordering};

    use oxisql_core::{Connection, ToSqlValue, Value};
    use oxisql_sqlite_compat::SqliteConnection;

    use oxigdal_gpkg::mbtiles_export::{detect_format_from_blob, xyz_to_tms_row};
    use oxigdal_gpkg::{GeoPackage, GpkgMbTilesExporter};

    // ─────────────────────────────────────────────────────────────────────────
    // Helpers
    // ─────────────────────────────────────────────────────────────────────────

    fn unique_suffix() -> String {
        static CTR: AtomicU64 = AtomicU64::new(0);
        let n = CTR.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        format!("{pid}_{n}")
    }

    fn make_rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio rt")
    }

    fn oxisql_open(rt: &tokio::runtime::Runtime, path: &std::path::Path) -> SqliteConnection {
        let p = path.to_string_lossy().into_owned();
        rt.block_on(SqliteConnection::open(&p)).expect("open")
    }

    fn oxisql_count(rt: &tokio::runtime::Runtime, conn: &SqliteConnection, sql: &str) -> i64 {
        let rows = rt.block_on(conn.query(sql, &[])).expect("query");
        match rows.first().and_then(|r| r.get_by_index(0)) {
            Some(Value::I64(n)) => *n,
            _ => 0,
        }
    }

    fn oxisql_text(
        rt: &tokio::runtime::Runtime,
        conn: &SqliteConnection,
        sql: &str,
        params: &[&dyn ToSqlValue],
    ) -> Option<String> {
        let rows = rt.block_on(conn.query(sql, params)).expect("query");
        match rows.first().and_then(|r| r.get_by_index(0)) {
            Some(Value::Text(s)) => Some(s.clone()),
            _ => None,
        }
    }

    fn oxisql_blob(
        rt: &tokio::runtime::Runtime,
        conn: &SqliteConnection,
        sql: &str,
    ) -> Option<Vec<u8>> {
        let rows = rt.block_on(conn.query(sql, &[])).expect("query");
        match rows.first().and_then(|r| r.get_by_index(0)) {
            Some(Value::Blob(b)) => Some(b.clone()),
            _ => None,
        }
    }

    /// Build a minimal GeoPackage SQLite file in a temp path and return its bytes.
    ///
    /// `tiles` is a slice of `(zoom_level, tile_column, tile_row, tile_data)`.
    fn make_test_gpkg(tiles: &[(u32, u32, u32, Vec<u8>)]) -> Vec<u8> {
        let path = env::temp_dir().join(format!("test_gpkg_{}.gpkg", unique_suffix()));
        let rt = make_rt();
        let conn = oxisql_open(&rt, &path);

        rt.block_on(conn.execute_batch(
            "
            PRAGMA application_id = 1196444487;
            PRAGMA user_version = 10200;

            CREATE TABLE gpkg_spatial_ref_sys (
                srs_name                 TEXT    NOT NULL,
                srs_id                   INTEGER PRIMARY KEY,
                organization             TEXT    NOT NULL,
                organization_coordsys_id INTEGER NOT NULL,
                definition               TEXT    NOT NULL,
                description              TEXT
            );

            CREATE TABLE gpkg_contents (
                table_name  TEXT     NOT NULL PRIMARY KEY,
                data_type   TEXT     NOT NULL,
                identifier  TEXT,
                description TEXT     DEFAULT '',
                last_change TEXT     NOT NULL DEFAULT '2024-01-01T00:00:00.000Z',
                min_x       REAL,
                min_y       REAL,
                max_x       REAL,
                max_y       REAL,
                srs_id      INTEGER
            );

            CREATE TABLE gpkg_tile_matrix_set (
                table_name TEXT    NOT NULL PRIMARY KEY,
                srs_id     INTEGER NOT NULL,
                min_x      REAL    NOT NULL,
                min_y      REAL    NOT NULL,
                max_x      REAL    NOT NULL,
                max_y      REAL    NOT NULL
            );

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

            CREATE TABLE test_tiles (
                zoom_level  INTEGER NOT NULL,
                tile_column INTEGER NOT NULL,
                tile_row    INTEGER NOT NULL,
                tile_data   BLOB    NOT NULL,
                PRIMARY KEY (zoom_level, tile_column, tile_row)
            );
            ",
        ))
        .unwrap();

        // Insert SRS rows one by one (execute_batch with multi-value INSERTs
        // may hit parse limits)
        let srs_rows: &[(&str, i64, &str, i64, &str)] = &[
            ("WGS 84 geodetic", 4326, "EPSG", 4326, "undefined"),
            ("undefined geographic CRS", 0, "NONE", 0, "undefined"),
            ("undefined Cartesian CRS", -1, "NONE", -1, "undefined"),
        ];
        for (name, srs_id, org, coordsys_id, defn) in srs_rows {
            let n: &str = name;
            let o: &str = org;
            let d: &str = defn;
            let sid: i64 = *srs_id;
            let cid: i64 = *coordsys_id;
            let empty: &str = "";
            // Insert all 6 columns (including nullable description) without
            // explicit column list to avoid limbo bugs with INTEGER PRIMARY KEY
            // in column-list INSERTs.
            rt.block_on(conn.execute(
                "INSERT INTO gpkg_spatial_ref_sys \
                 VALUES ($1, $2, $3, $4, $5, $6)",
                &[&n as &dyn ToSqlValue, &sid, &o, &cid, &d, &empty],
            ))
            .unwrap();
        }

        let tn: &str = "test_tiles";
        let dt: &str = "tiles";
        let lc: &str = "2024-01-01T00:00:00.000Z";
        let min_xy: f64 = -90.0;
        let max_xy: f64 = 90.0;
        let min_x: f64 = -180.0;
        let max_x: f64 = 180.0;
        let srs: i64 = 4326;

        rt.block_on(conn.execute(
            "INSERT INTO gpkg_contents \
             (table_name, data_type, identifier, description, last_change, \
              min_x, min_y, max_x, max_y, srs_id) \
             VALUES ($1, $2, $1, '', $3, $4, $5, $6, $7, $8)",
            &[
                &tn as &dyn ToSqlValue,
                &dt,
                &lc,
                &min_x,
                &min_xy,
                &max_x,
                &max_xy,
                &srs,
            ],
        ))
        .unwrap();

        rt.block_on(conn.execute(
            "INSERT INTO gpkg_tile_matrix_set \
             (table_name, srs_id, min_x, min_y, max_x, max_y) \
             VALUES ($1, $2, $3, $4, $5, $6)",
            &[
                &tn as &dyn ToSqlValue,
                &srs,
                &min_x,
                &min_xy,
                &max_x,
                &max_xy,
            ],
        ))
        .unwrap();

        // zoom 0: 1x1, zoom 1: 2x2
        let zoom_rows: &[(i64, i64, i64, f64, f64)] = &[
            (0, 1, 1, 0.703125, 0.703125),
            (1, 2, 2, 0.3515625, 0.3515625),
        ];
        for (zoom, mw, mh, px, py) in zoom_rows {
            let z: i64 = *zoom;
            let w: i64 = *mw;
            let h: i64 = *mh;
            let pxv: f64 = *px;
            let pyv: f64 = *py;
            let tw: i64 = 256;
            rt.block_on(conn.execute(
                "INSERT INTO gpkg_tile_matrix \
                 (table_name, zoom_level, matrix_width, matrix_height, \
                  tile_width, tile_height, pixel_x_size, pixel_y_size) \
                 VALUES ($1, $2, $3, $4, $5, $5, $6, $7)",
                &[&tn as &dyn ToSqlValue, &z, &w, &h, &tw, &pxv, &pyv],
            ))
            .unwrap();
        }

        for (z, col, row, blob) in tiles {
            let zi = *z as i64;
            let ci = *col as i64;
            let ri = *row as i64;
            let b: Vec<u8> = blob.clone();
            rt.block_on(conn.execute(
                "INSERT INTO test_tiles (zoom_level, tile_column, tile_row, tile_data) \
                 VALUES ($1, $2, $3, $4)",
                &[&zi as &dyn ToSqlValue, &ci, &ri, &b],
            ))
            .unwrap();
        }

        // Checkpoint WAL before reading bytes
        rt.block_on(conn.execute_batch("PRAGMA wal_checkpoint"))
            .unwrap();
        drop(conn);
        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        bytes
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
        assert_eq!(xyz_to_tms_row(0, 0), 0);
    }

    #[test]
    fn test_xyz_to_tms_row_conversion_zoom_10() {
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
        assert_eq!(detect_format_from_blob(&[0x00, 0x01, 0x02, 0x03]), "png");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // export_to_path — empty pyramid
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_export_empty_pyramid_writes_no_tiles() {
        let gpkg_bytes = make_test_gpkg(&[]);
        let gpkg = GeoPackage::from_bytes(gpkg_bytes).expect("valid gpkg");
        let exporter =
            GpkgMbTilesExporter::new(&gpkg, "test_tiles").expect("exporter construction");

        let out = env::temp_dir().join(format!("empty_{}.mbtiles", unique_suffix()));
        let stats = exporter.export_to_path(&out).expect("export must succeed");

        assert_eq!(stats.tiles_written, 0, "empty pyramid => 0 tiles written");
        assert_eq!(stats.bytes_written, 0);
        let _ = std::fs::remove_file(&out);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // export_to_path — with tile data
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_export_with_tiles_writes_rows() {
        let png = fake_png();
        let tiles = vec![(0u32, 0u32, 0u32, png.clone())];
        let gpkg_bytes = make_test_gpkg(&tiles);
        let gpkg = GeoPackage::from_bytes(gpkg_bytes).expect("valid gpkg");
        let exporter =
            GpkgMbTilesExporter::new(&gpkg, "test_tiles").expect("exporter construction");

        let out = env::temp_dir().join(format!("with_tiles_{}.mbtiles", unique_suffix()));
        let stats = exporter.export_to_path(&out).expect("export must succeed");

        assert_eq!(stats.tiles_written, 1);
        assert_eq!(stats.bytes_written, png.len() as u64);

        let rt = make_rt();
        let conn = oxisql_open(&rt, &out);
        let count = oxisql_count(&rt, &conn, "SELECT COUNT(*) FROM tiles");
        assert_eq!(count, 1);
        let _ = std::fs::remove_file(&out);
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

        let out = env::temp_dir().join(format!("meta_{}.mbtiles", unique_suffix()));
        let stats = exporter.export_to_path(&out).expect("export must succeed");

        assert!(
            stats.metadata_keys >= 7,
            "expected >= 7 metadata keys, got {}",
            stats.metadata_keys
        );

        let rt = make_rt();
        let conn = oxisql_open(&rt, &out);

        for key in &["name", "format", "minzoom", "maxzoom"] {
            let key_ref: &str = key;
            let found = oxisql_count(
                &rt,
                &conn,
                &format!("SELECT COUNT(*) FROM metadata WHERE name = '{key_ref}'"),
            );
            assert_eq!(found, 1, "metadata key '{key_ref}' must be present");
        }

        let format = oxisql_text(
            &rt,
            &conn,
            "SELECT value FROM metadata WHERE name='format'",
            &[],
        );
        assert_eq!(format.as_deref(), Some("png"));
        let _ = std::fs::remove_file(&out);
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

        let rt = make_rt();
        let conn = oxisql_open(&rt, &out_path);
        let count = oxisql_count(&rt, &conn, "SELECT COUNT(*) FROM tiles");
        assert_eq!(count, 1);

        let _ = std::fs::remove_file(&out_path);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Round-trip: tile_data bytes must be byte-exact
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_export_round_trips_tile_data_byte_exact() {
        let payload: Vec<u8> = (0u8..=127u8).collect();
        let mut blob = b"\x89PNG\r\n\x1a\n".to_vec();
        blob.extend_from_slice(&payload);

        let tiles = vec![(0u32, 0u32, 0u32, blob.clone())];
        let gpkg_bytes = make_test_gpkg(&tiles);
        let gpkg = GeoPackage::from_bytes(gpkg_bytes).expect("valid gpkg");
        let exporter =
            GpkgMbTilesExporter::new(&gpkg, "test_tiles").expect("exporter construction");

        let out = env::temp_dir().join(format!("rt_{}.mbtiles", unique_suffix()));
        exporter.export_to_path(&out).expect("export must succeed");

        let rt = make_rt();
        let conn = oxisql_open(&rt, &out);
        let retrieved = oxisql_blob(
            &rt,
            &conn,
            "SELECT tile_data FROM tiles \
             WHERE zoom_level=0 AND tile_column=0 AND tile_row=0",
        );

        assert_eq!(
            retrieved.as_deref(),
            Some(blob.as_slice()),
            "tile_data must round-trip byte-for-byte through MBTiles"
        );
        let _ = std::fs::remove_file(&out);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // XYZ → TMS row conversion inside the export
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_export_applies_tms_row_conversion() {
        let png = fake_png();
        let tiles = vec![
            (1u32, 0u32, 0u32, png.clone()),
            (1u32, 0u32, 1u32, png.clone()),
        ];
        let gpkg_bytes = make_test_gpkg(&tiles);
        let gpkg = GeoPackage::from_bytes(gpkg_bytes).expect("valid gpkg");
        let exporter =
            GpkgMbTilesExporter::new(&gpkg, "test_tiles").expect("exporter construction");

        let out = env::temp_dir().join(format!("tms_{}.mbtiles", unique_suffix()));
        exporter.export_to_path(&out).expect("export must succeed");

        let rt = make_rt();
        let conn = oxisql_open(&rt, &out);

        let exists_row1 = oxisql_count(
            &rt,
            &conn,
            "SELECT COUNT(*) FROM tiles WHERE zoom_level=1 AND tile_column=0 AND tile_row=1",
        );
        assert_eq!(
            exists_row1, 1,
            "TMS row 1 must exist for gpkg_row=0 at zoom 1"
        );

        let exists_row0 = oxisql_count(
            &rt,
            &conn,
            "SELECT COUNT(*) FROM tiles WHERE zoom_level=1 AND tile_column=0 AND tile_row=0",
        );
        assert_eq!(
            exists_row0, 1,
            "TMS row 0 must exist for gpkg_row=1 at zoom 1"
        );

        let _ = std::fs::remove_file(&out);
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
        let tiles = vec![
            (0u32, 0u32, 0u32, png.clone()),
            (1u32, 0u32, 0u32, png.clone()),
        ];
        let gpkg_bytes = make_test_gpkg(&tiles);
        let gpkg = GeoPackage::from_bytes(gpkg_bytes).expect("valid gpkg");
        let exporter =
            GpkgMbTilesExporter::new(&gpkg, "test_tiles").expect("exporter construction");

        let out = env::temp_dir().join(format!("zoom_{}.mbtiles", unique_suffix()));
        let stats = exporter.export_to_path(&out).expect("export must succeed");

        assert_eq!(stats.min_zoom, 0);
        assert_eq!(stats.max_zoom, 1);
        let _ = std::fs::remove_file(&out);
    }
}
