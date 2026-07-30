//! SQLite-backed writer for on-disk `.mbtiles` tile archives.
//!
//! Persists an in-memory [`MBTilesData`] (produced by [`MBTilesWriter::build`])
//! to a real SQLite `.mbtiles` file conforming to the MBTiles 1.3 schema:
//!
//! ```sql
//! CREATE TABLE metadata (name TEXT, value TEXT);
//! CREATE TABLE tiles (
//!     zoom_level  INTEGER,
//!     tile_column INTEGER,
//!     tile_row    INTEGER,
//!     tile_data   BLOB
//! );
//! CREATE UNIQUE INDEX tile_index ON tiles (zoom_level, tile_column, tile_row);
//! ```
//!
//! This module is gated behind the `sqlite` cargo feature (mirrors
//! [`crate::reader`]) and uses the Pure-Rust
//! [`oxisql_sqlite_compat::SqliteConnection`] engine — no C/FFI, no
//! `libsqlite3`.
//!
//! [`MBTilesWriter::build`]: crate::writer::MBTilesWriter::build

use std::path::Path;

use oxisql_core::{Connection, ToSqlValue};
use oxisql_sqlite_compat::SqliteConnection;

use crate::error::MbTilesError;
use crate::writer::{MBTilesData, MBTilesWriter};

fn sqlite_err(e: impl std::fmt::Display) -> MbTilesError {
    MbTilesError::Sqlite(e.to_string())
}

fn build_runtime() -> Result<tokio::runtime::Runtime, MbTilesError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| sqlite_err(format!("tokio runtime build failed: {e}")))
}

const CREATE_METADATA_TABLE: &str = "CREATE TABLE metadata (name TEXT, value TEXT)";
const CREATE_TILES_TABLE: &str = "CREATE TABLE tiles ( \
    zoom_level INTEGER, \
    tile_column INTEGER, \
    tile_row INTEGER, \
    tile_data BLOB \
)";
const CREATE_TILE_INDEX: &str =
    "CREATE UNIQUE INDEX tile_index ON tiles (zoom_level, tile_column, tile_row)";

impl MBTilesData {
    /// Persist this archive to a real on-disk SQLite `.mbtiles` file at `path`.
    ///
    /// Any existing file at `path` is removed first so the write always
    /// starts from a clean schema (MBTiles has no "append" semantics for a
    /// whole-archive write). All rows — `metadata` and every stored tile —
    /// are inserted inside a single transaction, so a failure partway through
    /// never leaves a half-written archive committed to disk.
    ///
    /// # Errors
    ///
    /// * [`MbTilesError::Io`] — the existing file at `path` could not be
    ///   removed, or the new file could not be created.
    /// * [`MbTilesError::Sqlite`] — schema creation or row insertion failed
    ///   (e.g. disk full, permission denied).
    pub fn write_to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), MbTilesError> {
        let path_ref = path.as_ref();
        if path_ref.exists() {
            std::fs::remove_file(path_ref)?;
        }

        let runtime = build_runtime()?;
        let path_str = path_ref.to_string_lossy().into_owned();
        let conn = runtime
            .block_on(SqliteConnection::open(&path_str))
            .map_err(sqlite_err)?;

        runtime
            .block_on(write_archive(&conn, self))
            .map_err(sqlite_err)
    }
}

impl MBTilesWriter {
    /// Consume the writer, build the in-memory [`MBTilesData`] snapshot, and
    /// immediately persist it to a real on-disk SQLite `.mbtiles` file.
    ///
    /// Equivalent to `self.build().write_to_file(path)`.
    ///
    /// # Errors
    ///
    /// See [`MBTilesData::write_to_file`].
    pub fn write_to_file<P: AsRef<Path>>(self, path: P) -> Result<(), MbTilesError> {
        self.build().write_to_file(path)
    }
}

async fn write_archive(
    conn: &SqliteConnection,
    data: &MBTilesData,
) -> Result<(), oxisql_core::OxiSqlError> {
    conn.execute(CREATE_METADATA_TABLE, &[]).await?;
    conn.execute(CREATE_TILES_TABLE, &[]).await?;
    conn.execute(CREATE_TILE_INDEX, &[]).await?;

    let mut txn = conn.transaction().await?;

    for (name, value) in data.metadata.to_rows() {
        let name_val: &dyn ToSqlValue = &name;
        let value_val: &dyn ToSqlValue = &value;
        txn.execute(
            "INSERT INTO metadata (name, value) VALUES ($1, $2)",
            &[name_val, value_val],
        )
        .await?;
    }

    for (coord, bytes) in &data.tiles {
        let z = i64::from(coord.z);
        let x = i64::from(coord.x);
        let y = i64::from(coord.y);
        let z_val: &dyn ToSqlValue = &z;
        let x_val: &dyn ToSqlValue = &x;
        let y_val: &dyn ToSqlValue = &y;
        let data_val: &dyn ToSqlValue = bytes;
        txn.execute(
            "INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data) \
             VALUES ($1, $2, $3, $4)",
            &[z_val, x_val, y_val, data_val],
        )
        .await?;
    }

    txn.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::mbtiles::MBTilesMetadata;
    use crate::reader::MBTilesReader;
    use crate::tile_coords::TileFormat;

    /// Per-test scratch SQLite fixture inside the system temp dir (house
    /// policy: no hardcoded absolute paths).
    ///
    /// The leaf name embeds the process id and a monotonic counter, so no two
    /// test binaries — nor two concurrent runs of this one — can ever land on
    /// the same database.  Dropping the guard removes the fixture *and its
    /// SQLite companions*, so a panicking test leaks nothing.
    struct TempPath(std::path::PathBuf);

    impl TempPath {
        fn new(tag: &str) -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!(
                "oxigeo-mbtiles-writer-test-{tag}-{}-{seq}.mbtiles",
                std::process::id()
            )))
        }
    }

    impl std::ops::Deref for TempPath {
        type Target = std::path::Path;

        fn deref(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl AsRef<std::path::Path> for TempPath {
        fn as_ref(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempPath {
        fn drop(&mut self) {
            // SQLite writes `-wal` / `-shm` / `-journal` companions next to
            // the database; removing only the main file would leak them.
            for suffix in ["", "-wal", "-shm", "-journal"] {
                let mut sidecar = self.0.clone().into_os_string();
                sidecar.push(suffix);
                let _ = std::fs::remove_file(std::path::PathBuf::from(sidecar));
            }
        }
    }

    fn unique_temp_path(tag: &str) -> TempPath {
        TempPath::new(tag)
    }

    /// End-to-end: build an archive in memory, write it to a real on-disk
    /// SQLite file, then read it back through `MBTilesReader` and confirm
    /// every tile and metadata field round-trips, including the TMS y-flip.
    #[test]
    fn write_then_read_round_trip() {
        let path = unique_temp_path("roundtrip");

        let mut writer = MBTilesWriter::new("test-tileset", TileFormat::Png);
        writer = writer.with_metadata(MBTilesMetadata {
            name: Some("test-tileset".to_string()),
            format: Some(TileFormat::Png),
            bounds: Some([-180.0, -85.0511, 180.0, 85.0511]),
            center: Some([0.0, 0.0, 2.0]),
            minzoom: Some(0),
            maxzoom: Some(2),
            attribution: Some("(c) OxiGeo".to_string()),
            description: Some("A test tileset".to_string()),
            tile_type: Some("baselayer".to_string()),
            version: Some("1.3".to_string()),
            json: None,
            extras: std::collections::HashMap::new(),
        });

        // add_tile uses TMS convention directly.
        writer.add_tile(0, 0, 0, vec![0x89, b'P', b'N', b'G']);
        // add_tile_xyz(z=1,x=0,y=0) (north-west in XYZ) should flip to TMS y=1.
        writer.add_tile_xyz(1, 0, 0, vec![1, 2, 3, 4]);
        writer.add_tile_xyz(1, 1, 1, vec![5, 6, 7, 8]);

        writer.write_to_file(&path).unwrap();

        assert!(path.exists(), "write_to_file must create a real file");

        let reader = MBTilesReader::open(&path).unwrap();
        assert_eq!(reader.tile_count().unwrap(), 3);

        let meta = reader.metadata();
        assert_eq!(meta.name.as_deref(), Some("test-tileset"));
        assert_eq!(meta.format, Some(TileFormat::Png));
        assert_eq!(meta.minzoom, Some(0));
        assert_eq!(meta.maxzoom, Some(2));
        assert_eq!(meta.bounds, Some([-180.0, -85.0511, 180.0, 85.0511]));
        assert_eq!(meta.tile_type.as_deref(), Some("baselayer"));

        // Tile at TMS (0,0,0).
        let t000 = reader
            .get_tile(&crate::tile_coords::TileCoord { z: 0, x: 0, y: 0 })
            .unwrap();
        assert_eq!(t000, Some(vec![0x89, b'P', b'N', b'G']));

        // add_tile_xyz(1, 0, 0) -> TMS y = 2^1 - 1 - 0 = 1.
        let flipped = reader
            .get_tile(&crate::tile_coords::TileCoord { z: 1, x: 0, y: 1 })
            .unwrap();
        assert_eq!(flipped, Some(vec![1, 2, 3, 4]));

        // add_tile_xyz(1, 1, 1) -> TMS y = 2^1 - 1 - 1 = 0.
        let flipped2 = reader
            .get_tile(&crate::tile_coords::TileCoord { z: 1, x: 1, y: 0 })
            .unwrap();
        assert_eq!(flipped2, Some(vec![5, 6, 7, 8]));
    }

    /// A byte-for-byte comparison isn't meaningful (SQLite headers/page
    /// layout vary), but the on-disk file must at minimum start with the
    /// standard SQLite magic header, proving `write_to_file` produced a real
    /// SQLite database image rather than a custom/fake format.
    #[test]
    fn write_to_file_produces_real_sqlite_file() {
        let path = unique_temp_path("magic");
        let mut writer = MBTilesWriter::new("magic-test", TileFormat::Pbf);
        writer.add_tile(0, 0, 0, vec![1, 2, 3]);
        writer.write_to_file(&path).unwrap();

        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.len() >= 16);
        assert_eq!(&bytes[0..16], b"SQLite format 3\0");
    }

    /// Writing to a path that already contains a file must overwrite it
    /// cleanly rather than erroring or corrupting the old content.
    #[test]
    fn write_to_file_overwrites_existing_file() {
        let path = unique_temp_path("overwrite");
        std::fs::write(&path, b"not a real mbtiles file").unwrap();

        let mut writer = MBTilesWriter::new("overwrite-test", TileFormat::Png);
        writer.add_tile(3, 1, 1, vec![9, 9, 9]);
        writer.write_to_file(&path).unwrap();

        let reader = MBTilesReader::open(&path).unwrap();
        assert_eq!(reader.tile_count().unwrap(), 1);
    }

    /// Non-canonical metadata keys must survive a write/read round trip.
    #[test]
    fn write_to_file_preserves_extras() {
        let path = unique_temp_path("extras");
        let mut extras = std::collections::HashMap::new();
        extras.insert("vendor_key".to_string(), "vendor_value".to_string());

        let mut writer = MBTilesWriter::new("extras-test", TileFormat::Png);
        writer = writer.with_metadata(MBTilesMetadata {
            name: Some("extras-test".to_string()),
            format: Some(TileFormat::Png),
            extras,
            ..Default::default()
        });
        writer.add_tile(0, 0, 0, vec![1]);
        writer.write_to_file(&path).unwrap();

        let reader = MBTilesReader::open(&path).unwrap();
        assert_eq!(
            reader
                .metadata()
                .extras()
                .get("vendor_key")
                .map(|s| s.as_str()),
            Some("vendor_value")
        );
    }
}
