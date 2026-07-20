//! PMTiles → MBTiles export.
//!
//! Converts a [`PmTilesReader`] archive into an MBTiles SQLite database
//! backed by the Pure-Rust [`oxisql_sqlite_compat::SqliteConnection`] engine
//! (no C/FFI, no `libsqlite3`).
//!
//! MBTiles spec: <https://github.com/mapbox/mbtiles-spec>
//!
//! Key MBTiles conventions implemented here:
//! * Schema: `metadata(name TEXT, value TEXT)` with unique index on `name`.
//! * Schema: `tiles(zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_data BLOB)`.
//! * Tile row uses **TMS** convention: `tms_row = (2^z - 1) - xyz_y`.
//! * Tile payloads are stored **decompressed** (native format, e.g. PNG bytes
//!   or raw MVT protobuf), so consumers can serve them directly without an
//!   extra decompression step.
//! * Required metadata keys: `name`, `format`, `bounds`, `minzoom`, `maxzoom`.
//! * Optional metadata keys: `center`, `json` (passthrough of PMTiles JSON metadata).
//!
//! # Sync ↔ async bridge
//!
//! The OxiSQL engine is async-only.  Each [`MbTilesConn`] therefore owns a
//! dedicated current-thread Tokio runtime and drives every database operation
//! through `runtime.block_on(...)`.  This keeps the connection self-contained:
//! it works in plain synchronous test functions and on worker threads alike,
//! without requiring an ambient runtime.
//!
//! # Parameter placeholders
//!
//! OxiSQL uses `$1`, `$2`, … positional placeholders.  All SQL in this module
//! uses the `$N` form.
//!
//! # WAL sidecar
//!
//! OxiSQL writes in WAL mode.  Tile archives handed off to external MBTiles
//! consumers must be checkpointed so that the WAL sidecar is merged back into
//! the main database file.  [`MbTilesExporter::export_to_path`] issues
//! `PRAGMA wal_checkpoint` automatically after all writes complete.

#![cfg(feature = "mbtiles")]

use std::path::Path;

use oxisql_core::{Connection, ToSqlValue, Value};
use oxisql_sqlite_compat::SqliteConnection;
use tokio::runtime::Runtime;

use crate::error::PmTilesError;
use crate::header::{PmTilesHeader, TileType};
use crate::pmtiles::PmTilesReader;

// ---------------------------------------------------------------------------
// Error mapping helpers
// ---------------------------------------------------------------------------

/// Map any `Display` error to [`PmTilesError::SqliteError`].
fn sqlite_err(e: impl std::fmt::Display) -> PmTilesError {
    PmTilesError::SqliteError(e.to_string())
}

// ---------------------------------------------------------------------------
// MbTilesConn — sync wrapper around the async OxiSQL connection
// ---------------------------------------------------------------------------

/// A synchronous handle to an OxiSQL-backed MBTiles SQLite database.
///
/// Owns both the async connection and the Tokio runtime needed to drive it
/// from synchronous callers.  Use [`MbTilesConn::open`] for file-backed
/// databases and [`MbTilesConn::open_memory`] for in-memory databases
/// (useful in tests).
pub struct MbTilesConn {
    pub(crate) conn: SqliteConnection,
    pub(crate) runtime: Runtime,
}

impl MbTilesConn {
    /// Open (or create) an MBTiles database at the given file path.
    ///
    /// # Errors
    /// Returns [`PmTilesError::SqliteError`] if the file cannot be opened or
    /// the Tokio runtime cannot be initialised.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, PmTilesError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| sqlite_err(format!("tokio runtime build failed: {e}")))?;
        let path_str = path.as_ref().to_string_lossy().into_owned();
        let conn = runtime
            .block_on(SqliteConnection::open(&path_str))
            .map_err(sqlite_err)?;
        Ok(Self { conn, runtime })
    }

    /// Open a fresh in-memory MBTiles database.
    ///
    /// Useful for testing: the database lives only in RAM and is discarded
    /// when the [`MbTilesConn`] is dropped.
    ///
    /// # Errors
    /// Returns [`PmTilesError::SqliteError`] if the engine cannot be
    /// initialised.
    pub fn open_memory() -> Result<Self, PmTilesError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| sqlite_err(format!("tokio runtime build failed: {e}")))?;
        let conn = runtime
            .block_on(SqliteConnection::open_memory())
            .map_err(sqlite_err)?;
        Ok(Self { conn, runtime })
    }

    // ---- Internal helpers -----------------------------------------------

    /// Execute a DML/DDL statement (no result rows needed).
    pub(crate) fn exec(&self, sql: &str, params: &[&dyn ToSqlValue]) -> Result<u64, PmTilesError> {
        self.runtime
            .block_on(self.conn.execute(sql, params))
            .map_err(sqlite_err)
    }

    /// Execute multiple semicolon-separated DDL statements.
    pub(crate) fn exec_batch(&self, sql: &str) -> Result<(), PmTilesError> {
        self.runtime
            .block_on(self.conn.execute_batch(sql))
            .map_err(sqlite_err)?;
        Ok(())
    }

    /// Execute a SELECT statement and return all result rows.
    pub(crate) fn query_rows(
        &self,
        sql: &str,
        params: &[&dyn ToSqlValue],
    ) -> Result<Vec<oxisql_core::Row>, PmTilesError> {
        self.runtime
            .block_on(self.conn.query(sql, params))
            .map_err(sqlite_err)
    }

    /// Count rows matching a query and return the count as `i64`.
    ///
    /// The query must return a single `COUNT(*)` column.  An empty result
    /// (aggregate over no rows) returns `0`.
    pub fn query_count(&self, sql: &str, params: &[&dyn ToSqlValue]) -> Result<i64, PmTilesError> {
        let rows = self.query_rows(sql, params)?;
        match rows.first().and_then(|r| r.get_by_index(0)) {
            Some(Value::I64(n)) => Ok(*n),
            Some(Value::Null) | None => Ok(0),
            Some(other) => Err(sqlite_err(format!(
                "query_count: unexpected value {}",
                other.type_name()
            ))),
        }
    }

    /// Fetch a single `TEXT` value from the first result row, first column.
    ///
    /// Returns `None` when the query produces no rows (or `NULL`).
    pub fn query_text(
        &self,
        sql: &str,
        params: &[&dyn ToSqlValue],
    ) -> Result<Option<String>, PmTilesError> {
        let rows = self.query_rows(sql, params)?;
        match rows.first().and_then(|r| r.get_by_index(0)) {
            Some(Value::Text(s)) => Ok(Some(s.clone())),
            Some(Value::Null) | None => Ok(None),
            Some(other) => Err(sqlite_err(format!(
                "query_text: unexpected value {}",
                other.type_name()
            ))),
        }
    }

    /// Fetch a single `BLOB` value from the first result row, first column.
    ///
    /// Returns `None` when the query produces no rows (or `NULL`).
    pub fn query_blob(
        &self,
        sql: &str,
        params: &[&dyn ToSqlValue],
    ) -> Result<Option<Vec<u8>>, PmTilesError> {
        let rows = self.query_rows(sql, params)?;
        match rows.first().and_then(|r| r.get_by_index(0)) {
            Some(Value::Blob(b)) => Ok(Some(b.clone())),
            Some(Value::Null) | None => Ok(None),
            Some(other) => Err(sqlite_err(format!(
                "query_blob: unexpected value {}",
                other.type_name()
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// High-level exporter that converts a [`PmTilesReader`] archive into an
/// MBTiles SQLite database.
///
/// # Example
/// ```no_run
/// use oxigeo_pmtiles::MbTilesExporter;
/// use oxigeo_pmtiles::PmTilesReader;
///
/// let data = std::fs::read("world.pmtiles").unwrap();
/// let reader = PmTilesReader::from_bytes(data).unwrap();
/// let exporter = MbTilesExporter::new(&reader);
/// let stats = exporter.export_to_path("/tmp/world.mbtiles").unwrap();
/// println!("Exported {} tiles", stats.tiles_written);
/// ```
pub struct MbTilesExporter<'a> {
    reader: &'a PmTilesReader,
}

/// Statistics returned after a successful MBTiles export.
#[derive(Debug, Clone)]
pub struct MbTilesExportStats {
    /// Total number of tile rows inserted into the `tiles` table.
    pub tiles_written: u64,
    /// Minimum zoom level encountered in the exported tile set.
    pub min_zoom: u8,
    /// Maximum zoom level encountered in the exported tile set.
    pub max_zoom: u8,
    /// Total uncompressed bytes stored as tile payloads.
    pub bytes_written: u64,
    /// Number of metadata key/value pairs written to the `metadata` table.
    pub metadata_keys: usize,
}

// ---------------------------------------------------------------------------
// MbTilesExporter implementation
// ---------------------------------------------------------------------------

impl<'a> MbTilesExporter<'a> {
    /// Construct an exporter backed by the given [`PmTilesReader`].
    pub fn new(reader: &'a PmTilesReader) -> Self {
        Self { reader }
    }

    /// Export the archive to an MBTiles file at `path`.
    ///
    /// If the file already exists it is deleted before the new database is
    /// created (per spec: the output is always a fresh, self-consistent
    /// MBTiles file).
    ///
    /// After all tiles are written a `PRAGMA wal_checkpoint` is issued so
    /// that the WAL sidecar is merged back into the main database file,
    /// making the output readable by external MBTiles consumers without the
    /// `-wal` sidecar.
    ///
    /// # Errors
    /// Returns [`PmTilesError::Io`] on filesystem errors and
    /// [`PmTilesError::SqliteError`] on database errors.
    pub fn export_to_path<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<MbTilesExportStats, PmTilesError> {
        let path = path.as_ref();
        // Remove any pre-existing file so that the engine creates a completely
        // fresh database.  The MBTiles spec defines the format as a single
        // self-contained SQLite file; overwriting is the correct behaviour for
        // a batch export operation.
        if path.exists() {
            std::fs::remove_file(path).map_err(PmTilesError::Io)?;
        }
        let db = MbTilesConn::open(path)?;
        let stats = self.export_to_connection(&db)?;
        // Checkpoint the WAL so that the output file is self-contained for
        // external MBTiles consumers (no `-wal` sidecar required).
        db.exec_batch("PRAGMA wal_checkpoint")?;
        Ok(stats)
    }

    /// Export the archive into an existing [`MbTilesConn`].
    ///
    /// The schema is created inside the provided connection, so this method
    /// can be called with `MbTilesConn::open_memory()` for testing.
    ///
    /// # Errors
    /// Returns [`PmTilesError::SqliteError`] on any SQLite-level failure and
    /// propagates tile-retrieval errors from the underlying [`PmTilesReader`].
    pub fn export_to_connection(
        &self,
        db: &MbTilesConn,
    ) -> Result<MbTilesExportStats, PmTilesError> {
        // ------------------------------------------------------------------
        // Step 1: Create the MBTiles schema.
        // ------------------------------------------------------------------
        create_schema(db)?;

        // ------------------------------------------------------------------
        // Step 2: Write metadata.
        //
        // The PMTiles JSON metadata blob (if present) is serialised back to
        // a JSON string and stored under the `json` key, which is the
        // standard mechanism for passing vector-tile layer descriptors
        // through MBTiles.
        // ------------------------------------------------------------------
        let header = &self.reader.header;
        let metadata_json_str = self
            .reader
            .metadata()
            .ok()
            .and_then(|m| serde_json::to_string(&m).ok());
        let metadata_keys = write_metadata(db, header, metadata_json_str.as_deref())?;

        // ------------------------------------------------------------------
        // Step 3: Bulk-insert tiles.
        //
        // The OxiSQL engine is WAL-only; there is no way to switch to
        // journal_mode=OFF or alter synchronous mode via PRAGMA.  Writes are
        // therefore issued as individual `INSERT OR REPLACE` statements.
        // Sequential individual executes provide equivalent correctness to a
        // transaction-batched approach because a crash mid-write simply
        // leaves a partial file that can be regenerated by re-running the
        // export.
        // ------------------------------------------------------------------
        let tile_infos = self.reader.enumerate_tiles()?;

        let mut tiles_written: u64 = 0;
        let mut bytes_written: u64 = 0;
        let mut min_zoom: u8 = u8::MAX;
        let mut max_zoom: u8 = 0;

        for tile_info in &tile_infos {
            // Retrieve the raw (possibly compressed) tile payload.
            let raw_opt = self
                .reader
                .get_tile(tile_info.z, tile_info.x, tile_info.y)?;

            let raw = match raw_opt {
                Some(r) => r,
                // Tile listed in the directory but not resolvable — skip.
                None => continue,
            };

            // Decompress the tile payload.  MBTiles consumers expect native,
            // uncompressed tile bytes (PNG, JPEG, or raw MVT protobuf).
            let tile_data = self.reader.decompress_tile(&raw)?;

            // Convert XYZ `y` to TMS `row`:
            //   tms_row = 2^z - 1 - xyz_y
            // At zoom 0 with y=0: tms_row = 0 (single tile, same address).
            // At zoom 2 with y=3: tms_row = 3 - 3 = 0 (bottom row → TMS row 0).
            let tms_row = (1u32 << tile_info.z)
                .wrapping_sub(1)
                .wrapping_sub(tile_info.y);

            bytes_written += tile_data.len() as u64;

            // `ToSqlValue` is not implemented for `&[u8]`, only for `Vec<u8>`.
            // Materialise a `Vec<u8>` so the blanket `&T: ToSqlValue` applies.
            let tile_blob: Vec<u8> = tile_data;
            let zoom_i64 = tile_info.z as i64;
            let col_i64 = tile_info.x as i64;
            let row_i64 = tms_row as i64;

            db.exec(
                "INSERT OR REPLACE INTO tiles \
                 (zoom_level, tile_column, tile_row, tile_data) \
                 VALUES ($1, $2, $3, $4)",
                &[&zoom_i64, &col_i64, &row_i64, &tile_blob],
            )?;

            tiles_written += 1;

            if tile_info.z < min_zoom {
                min_zoom = tile_info.z;
            }
            if tile_info.z > max_zoom {
                max_zoom = tile_info.z;
            }
        }

        // When there are no tiles reset zoom levels to sensible defaults.
        if tiles_written == 0 {
            min_zoom = 0;
            max_zoom = 0;
        }

        Ok(MbTilesExportStats {
            tiles_written,
            min_zoom,
            max_zoom,
            bytes_written,
            metadata_keys,
        })
    }
}

// ---------------------------------------------------------------------------
// Schema creation
// ---------------------------------------------------------------------------

/// Create the canonical MBTiles schema inside `db`.
///
/// Uses `IF NOT EXISTS` guards so that this is idempotent when called on a
/// connection that already has the schema (e.g. an in-memory connection reused
/// across multiple test assertions).
///
/// Note: `WITHOUT ROWID` is intentionally omitted — it is not yet supported
/// by the OxiSQL / Limbo engine.  The tiles table therefore has an implicit
/// rowid column alongside the composite primary key; this does not affect
/// MBTiles spec compliance.
fn create_schema(db: &MbTilesConn) -> Result<(), PmTilesError> {
    // Note: the UNIQUE constraint on `metadata.name` is expressed inline
    // (column-level `UNIQUE`) rather than as a separate `CREATE UNIQUE INDEX`
    // statement.  The OxiSQL / Limbo engine does not yet honour the
    // `IF NOT EXISTS` guard on `CREATE INDEX`, so a standalone index creation
    // would fail on the second call with "index already exists".  Declaring
    // the uniqueness inside `CREATE TABLE IF NOT EXISTS` is idempotent because
    // the whole statement is skipped when the table already exists.
    //
    // Note: `WITHOUT ROWID` is intentionally omitted — it is not yet supported
    // by the OxiSQL / Limbo engine.  The tiles table therefore has an implicit
    // rowid column alongside the composite primary key; this does not affect
    // MBTiles spec compliance.
    db.exec_batch(
        "
        CREATE TABLE IF NOT EXISTS metadata (
            name  TEXT NOT NULL UNIQUE,
            value TEXT
        );

        CREATE TABLE IF NOT EXISTS tiles (
            zoom_level  INTEGER NOT NULL,
            tile_column INTEGER NOT NULL,
            tile_row    INTEGER NOT NULL,
            tile_data   BLOB,
            PRIMARY KEY (zoom_level, tile_column, tile_row)
        );
        ",
    )
}

// ---------------------------------------------------------------------------
// Metadata writing
// ---------------------------------------------------------------------------

/// Populate the MBTiles `metadata` table from the archive [`PmTilesHeader`]
/// and optional JSON metadata string.
///
/// Required keys (per spec): `name`, `format`, `bounds`, `minzoom`, `maxzoom`.
/// Optional keys written when available: `center`, `json`.
///
/// Returns the total number of key/value pairs inserted.
///
/// # Errors
/// Returns [`PmTilesError::SqliteError`] on any database failure.
fn write_metadata(
    db: &MbTilesConn,
    header: &PmTilesHeader,
    metadata_json: Option<&str>,
) -> Result<usize, PmTilesError> {
    let format = format_from_tile_type(&header.tile_type);

    // Build the required-plus-optional entry list.  Each entry is a
    // (&'static str key, owned String value) pair to avoid lifetime
    // gymnastics with the optional entries.
    let mut entries: Vec<(&'static str, String)> = vec![
        ("name", "PMTiles Export".to_string()),
        ("format", format.to_string()),
        (
            "bounds",
            format!(
                "{},{},{},{}",
                header.min_lon_e7 as f64 / 1e7,
                header.min_lat_e7 as f64 / 1e7,
                header.max_lon_e7 as f64 / 1e7,
                header.max_lat_e7 as f64 / 1e7,
            ),
        ),
        ("minzoom", header.min_zoom.to_string()),
        ("maxzoom", header.max_zoom.to_string()),
        (
            "center",
            format!(
                "{},{},{}",
                header.center_lon_e7 as f64 / 1e7,
                header.center_lat_e7 as f64 / 1e7,
                header.center_zoom,
            ),
        ),
    ];

    // Include the raw JSON metadata blob under the `json` key so that
    // consumers (e.g. TileJSON parsers) can extract layer definitions.
    if let Some(json_str) = metadata_json {
        // Only insert a non-trivial metadata blob; the PmTilesReader returns
        // an all-None PmTilesMetadata for empty archives (serialised as "{}").
        // There is no harm in inserting "{}" — it is simply not very useful.
        entries.push(("json", json_str.to_string()));
    }

    let count = entries.len();
    for (name, value) in &entries {
        let name_str: &str = name;
        let value_str: &str = value.as_str();
        db.exec(
            "INSERT OR REPLACE INTO metadata (name, value) VALUES ($1, $2)",
            &[&name_str, &value_str],
        )?;
    }

    Ok(count)
}

// ---------------------------------------------------------------------------
// Tile-type → MBTiles format string
// ---------------------------------------------------------------------------

/// Map a [`TileType`] to the MBTiles `format` metadata value.
///
/// MBTiles spec recognised values: `"png"`, `"jpg"`, `"webp"`, `"pbf"`.
/// AVIF is not part of the original MBTiles spec but is stored as `"avif"`
/// for forward compatibility.  Unknown types fall back to `"pbf"` (vector).
fn format_from_tile_type(tile_type: &TileType) -> &'static str {
    match tile_type {
        TileType::Png => "png",
        TileType::Jpeg => "jpg",
        TileType::Webp => "webp",
        TileType::Avif => "avif",
        TileType::Mvt | TileType::Unknown => "pbf",
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::TileType;

    // Helper: convert xyz_y to TMS row directly (mirrors the inline formula in
    // export_to_connection).  Used for arithmetic-only tests (7–9) that do not
    // need a full archive round-trip.
    fn xyz_to_tms(z: u8, y: u32) -> u32 {
        (1u32 << z).wrapping_sub(1).wrapping_sub(y)
    }

    #[test]
    fn test_format_from_tile_type_png() {
        assert_eq!(format_from_tile_type(&TileType::Png), "png");
    }

    #[test]
    fn test_format_from_tile_type_jpeg() {
        assert_eq!(format_from_tile_type(&TileType::Jpeg), "jpg");
    }

    #[test]
    fn test_format_from_tile_type_webp() {
        assert_eq!(format_from_tile_type(&TileType::Webp), "webp");
    }

    #[test]
    fn test_format_from_tile_type_mvt() {
        assert_eq!(format_from_tile_type(&TileType::Mvt), "pbf");
    }

    #[test]
    fn test_format_from_tile_type_unknown() {
        assert_eq!(format_from_tile_type(&TileType::Unknown), "pbf");
    }

    #[test]
    fn test_tms_row_conversion_zoom_0_y_0_returns_0() {
        // z=0, y=0 → 2^0 - 1 - 0 = 0
        assert_eq!(xyz_to_tms(0, 0), 0);
    }

    #[test]
    fn test_tms_row_conversion_zoom_2_y_3_returns_0() {
        // z=2, y=3 → 2^2 - 1 - 3 = 4 - 1 - 3 = 0
        assert_eq!(xyz_to_tms(2, 3), 0);
    }

    #[test]
    fn test_tms_row_conversion_zoom_10_y_500_returns_523() {
        // z=10, y=500 → 2^10 - 1 - 500 = 1024 - 1 - 500 = 523
        assert_eq!(xyz_to_tms(10, 500), 523);
    }

    #[test]
    fn test_tms_row_conversion_zoom_1_y_0_returns_1() {
        // z=1, y=0 → 2^1 - 1 - 0 = 1
        assert_eq!(xyz_to_tms(1, 0), 1);
    }

    #[test]
    fn test_tms_row_conversion_zoom_1_y_1_returns_0() {
        // z=1, y=1 → 2^1 - 1 - 1 = 0
        assert_eq!(xyz_to_tms(1, 1), 0);
    }
}
