//! SQLite-backed reader for on-disk `.mbtiles` tile archives.
//!
//! MBTiles 1.3 (<https://github.com/mapbox/mbtiles-spec>) prescribes a
//! plain SQLite database as the container format, with the following
//! minimum schema:
//!
//! ```sql
//! CREATE TABLE metadata (name TEXT, value TEXT);
//! CREATE TABLE tiles (
//!     zoom_level  INTEGER,
//!     tile_column INTEGER,
//!     tile_row    INTEGER,
//!     tile_data   BLOB
//! );
//! ```
//!
//! `tile_row` follows the **TMS** convention (`y = 0` at south); callers that
//! need XYZ coordinates can use [`crate::tms_to_xyz`].
//!
//! This module is gated behind the `sqlite` cargo feature — see the crate
//! root for the wiring.  The implementation uses the Pure-Rust
//! [`oxisql_sqlite_compat::SqliteConnection`] engine (no C/FFI, no `libsqlite3`).
//!
//! ## In-memory archives
//!
//! [`MBTilesReader::open_in_memory`] accepts a `&[u8]` containing an entire
//! SQLite database image.  The bytes are spilled to a temporary file inside
//! [`std::env::temp_dir`] and opened read-write (the engine doesn't have an
//! OpenFlags API, but only SELECTs are issued so the file is never mutated in
//! practice).  The temp file is deleted as soon as the reader is dropped.
//!
//! ## Sync ↔ async bridge
//!
//! The OxiSQL engine is async-only.  Each [`MBTilesReader`] therefore owns a
//! dedicated current-thread Tokio runtime and drives every database operation
//! through `runtime.block_on(...)`.
//!
//! ## Parameter placeholders
//!
//! OxiSQL uses `$1`, `$2`, … positional placeholders.  All SQL in this module
//! uses the `$N` form.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use oxisql_core::{Connection, Value};
use oxisql_sqlite_compat::SqliteConnection;

use crate::error::MbTilesError;
use crate::mbtiles::{MBTiles, MBTilesMetadata};
use crate::tile_coords::TileCoord;

// ---------------------------------------------------------------------------
// Error-mapping helper
// ---------------------------------------------------------------------------

fn sqlite_err(e: impl std::fmt::Display) -> MbTilesError {
    MbTilesError::Sqlite(e.to_string())
}

// ---------------------------------------------------------------------------
// MBTilesReader
// ---------------------------------------------------------------------------

/// Read-only handle to an on-disk MBTiles SQLite database.
///
/// The OxiSQL engine opens files read-write but only SELECTs are issued.
/// Metadata is loaded eagerly at construction time; tile bytes are fetched
/// lazily on every [`Self::get_tile`] call.
pub struct MBTilesReader {
    conn: SqliteConnection,
    runtime: tokio::runtime::Runtime,
    metadata: MBTilesMetadata,
    /// Path to a temp file created by [`Self::open_in_memory`], if any.
    /// Deleted on drop.
    owned_temp_path: Option<PathBuf>,
}

impl std::fmt::Debug for MBTilesReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MBTilesReader")
            .field("owned_temp_path", &self.owned_temp_path)
            .finish()
    }
}

impl MBTilesReader {
    fn build_runtime() -> Result<tokio::runtime::Runtime, MbTilesError> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| sqlite_err(format!("tokio runtime build failed: {e}")))
    }

    /// Open an existing `.mbtiles` file at `path`.
    ///
    /// # Errors
    ///
    /// * [`MbTilesError::Sqlite`] — the file is not a valid SQLite database
    ///   or cannot be opened.
    /// * [`MbTilesError::InvalidFormat`] — one of the mandatory MBTiles tables
    ///   (`metadata`, `tiles`) is missing.
    /// * [`MbTilesError::InvalidMetadata`] — a canonical metadata field is malformed.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, MbTilesError> {
        let runtime = Self::build_runtime()?;
        let path_str = path.as_ref().to_string_lossy().into_owned();
        let conn = runtime
            .block_on(SqliteConnection::open(&path_str))
            .map_err(sqlite_err)?;
        require_table_rt(&conn, &runtime, "metadata")?;
        require_table_rt(&conn, &runtime, "tiles")?;
        let metadata = load_metadata_rt(&conn, &runtime)?;
        Ok(Self {
            conn,
            runtime,
            metadata,
            owned_temp_path: None,
        })
    }

    /// Open a `.mbtiles` archive supplied as an in-memory byte buffer.
    ///
    /// Spills `bytes` to a uniquely named temp file inside
    /// [`std::env::temp_dir`] and opens it.  The temp file is removed when
    /// the returned [`MBTilesReader`] is dropped.
    ///
    /// # Errors
    ///
    /// Same as [`Self::open`], plus [`MbTilesError::Io`] if the temp file
    /// cannot be written.
    pub fn open_in_memory(bytes: &[u8]) -> Result<Self, MbTilesError> {
        let temp_path = unique_temp_path();
        fs::write(&temp_path, bytes)?;

        let runtime = match Self::build_runtime() {
            Ok(rt) => rt,
            Err(e) => {
                let _ = fs::remove_file(&temp_path);
                return Err(e);
            }
        };

        let path_str = temp_path.to_string_lossy().into_owned();
        let conn = match runtime.block_on(SqliteConnection::open(&path_str)) {
            Ok(c) => c,
            Err(e) => {
                let _ = fs::remove_file(&temp_path);
                return Err(sqlite_err(e));
            }
        };

        let validate = (|| -> Result<MBTilesMetadata, MbTilesError> {
            require_table_rt(&conn, &runtime, "metadata")?;
            require_table_rt(&conn, &runtime, "tiles")?;
            load_metadata_rt(&conn, &runtime)
        })();

        match validate {
            Ok(metadata) => Ok(Self {
                conn,
                runtime,
                metadata,
                owned_temp_path: Some(temp_path),
            }),
            Err(e) => {
                // conn will be dropped here; temp file cleanup follows
                let _ = fs::remove_file(&temp_path);
                Err(e)
            }
        }
    }

    /// Read-only access to the loaded tileset metadata.
    pub fn metadata(&self) -> &MBTilesMetadata {
        &self.metadata
    }

    /// Enumerate every `(zoom_level, tile_column, tile_row)` triple present
    /// in the archive, ordered by `(z, x, y)` ascending.
    ///
    /// `tile_row` is returned **as stored** (TMS convention).
    pub fn list_tiles(&self) -> Result<Vec<TileCoord>, MbTilesError> {
        let rows = self
            .runtime
            .block_on(self.conn.query(
                "SELECT zoom_level, tile_column, tile_row
                 FROM tiles
                 ORDER BY zoom_level ASC, tile_column ASC, tile_row ASC",
                &[],
            ))
            .map_err(sqlite_err)?;

        let mut coords = Vec::with_capacity(rows.len());
        for row in &rows {
            let z = get_i64(row, 0, "zoom_level")?;
            let x = get_i64(row, 1, "tile_column")?;
            let y = get_i64(row, 2, "tile_row")?;
            coords.push(coord_from_row(z, x, y)?);
        }
        Ok(coords)
    }

    /// Fetch the raw bytes for a single tile.
    ///
    /// Returns `Ok(None)` when no row matches the requested coordinate.
    /// `coord.y` is interpreted in the **TMS** convention to match the
    /// on-disk layout.
    pub fn get_tile(&self, coord: &TileCoord) -> Result<Option<Vec<u8>>, MbTilesError> {
        let z = coord.z as i64;
        let x = coord.x as i64;
        let y = coord.y as i64;

        let rows = self
            .runtime
            .block_on(self.conn.query(
                "SELECT tile_data
                 FROM tiles
                 WHERE zoom_level = $1 AND tile_column = $2 AND tile_row = $3
                 LIMIT 1",
                &[&z, &x, &y],
            ))
            .map_err(sqlite_err)?;

        match rows.first() {
            None => Ok(None),
            Some(row) => {
                let blob = match row.get_by_index(0) {
                    Some(Value::Blob(b)) => b.clone(),
                    Some(Value::Null) | None => return Ok(None),
                    Some(other) => {
                        return Err(MbTilesError::Sqlite(format!(
                            "tile_data: expected BLOB, got {}",
                            other.type_name()
                        )));
                    }
                };
                Ok(Some(blob))
            }
        }
    }

    /// Total number of tile rows in the archive.
    pub fn tile_count(&self) -> Result<usize, MbTilesError> {
        let rows = self
            .runtime
            .block_on(self.conn.query("SELECT COUNT(*) FROM tiles", &[]))
            .map_err(sqlite_err)?;
        let count = match rows.first().and_then(|r| r.get_by_index(0)) {
            Some(Value::I64(n)) => *n,
            _ => 0i64,
        };
        usize::try_from(count).map_err(|_| {
            MbTilesError::InvalidFormat(format!("tile count out of usize range: {count}"))
        })
    }

    /// Distinct zoom levels present in the archive, sorted ascending.
    pub fn zoom_levels(&self) -> Result<Vec<u8>, MbTilesError> {
        let rows = self
            .runtime
            .block_on(self.conn.query(
                "SELECT DISTINCT zoom_level FROM tiles ORDER BY zoom_level ASC",
                &[],
            ))
            .map_err(sqlite_err)?;

        let mut zooms = Vec::with_capacity(rows.len());
        for row in &rows {
            let z = get_i64(row, 0, "zoom_level")?;
            let z_u8 = u8::try_from(z).map_err(|_| {
                MbTilesError::InvalidFormat(format!("zoom level {z} out of u8 range"))
            })?;
            zooms.push(z_u8);
        }
        Ok(zooms)
    }

    /// Eagerly load every tile into a new in-memory [`MBTiles`] store.
    pub fn into_mbtiles(self) -> Result<MBTiles, MbTilesError> {
        let mut store = MBTiles::new(self.metadata.clone());
        let rows = self
            .runtime
            .block_on(self.conn.query(
                "SELECT zoom_level, tile_column, tile_row, tile_data
                 FROM tiles
                 ORDER BY zoom_level ASC, tile_column ASC, tile_row ASC",
                &[],
            ))
            .map_err(sqlite_err)?;

        for row in &rows {
            let z = get_i64(row, 0, "zoom_level")?;
            let x = get_i64(row, 1, "tile_column")?;
            let y = get_i64(row, 2, "tile_row")?;
            let data = match row.get_by_index(3) {
                Some(Value::Blob(b)) => b.clone(),
                _ => Vec::new(),
            };
            let coord = coord_from_row(z, x, y)?;
            store.insert_tile(coord, data);
        }
        Ok(store)
    }
}

impl Drop for MBTilesReader {
    fn drop(&mut self) {
        if let Some(path) = self.owned_temp_path.take() {
            let _ = fs::remove_file(path);
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract an `i64` value from column `idx` of `row`.
fn get_i64(row: &oxisql_core::Row, idx: usize, col_name: &str) -> Result<i64, MbTilesError> {
    match row.get_by_index(idx) {
        Some(Value::I64(n)) => Ok(*n),
        other => Err(MbTilesError::InvalidFormat(format!(
            "column {col_name}: expected I64, got {:?}",
            other
        ))),
    }
}

/// Confirm that a table with `name` exists in the schema via a runtime-owned
/// connection, otherwise return [`MbTilesError::InvalidFormat`].
fn require_table_rt(
    conn: &SqliteConnection,
    rt: &tokio::runtime::Runtime,
    name: &str,
) -> Result<(), MbTilesError> {
    let name_ref: &str = name;
    let rows = rt
        .block_on(conn.query(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = $1",
            &[&name_ref],
        ))
        .map_err(sqlite_err)?;

    let count = match rows.first().and_then(|r| r.get_by_index(0)) {
        Some(Value::I64(n)) => *n,
        _ => 0i64,
    };

    if count == 0 {
        return Err(MbTilesError::InvalidFormat(format!(
            "missing mandatory MBTiles table: {name}"
        )));
    }
    Ok(())
}

/// Read every `(name, value)` row from the `metadata` table and feed it
/// through [`MBTilesMetadata::from_map_strict`].
fn load_metadata_rt(
    conn: &SqliteConnection,
    rt: &tokio::runtime::Runtime,
) -> Result<MBTilesMetadata, MbTilesError> {
    let rows = rt
        .block_on(conn.query("SELECT name, value FROM metadata", &[]))
        .map_err(sqlite_err)?;

    let mut map: HashMap<String, String> = HashMap::new();
    for row in &rows {
        let key = match row.get_by_index(0) {
            Some(Value::Text(s)) => s.clone(),
            _ => continue,
        };
        let value = match row.get_by_index(1) {
            Some(Value::Text(s)) => s.clone(),
            Some(Value::Null) | None => String::new(),
            _ => continue,
        };
        map.insert(key, value);
    }
    MBTilesMetadata::from_map_strict(map)
}

/// Convert a raw `(z, x, y)` row triple into a [`TileCoord`], range-checking
/// each component.
fn coord_from_row(z: i64, x: i64, y: i64) -> Result<TileCoord, MbTilesError> {
    let z_u8 = u8::try_from(z)
        .map_err(|_| MbTilesError::InvalidFormat(format!("zoom_level {z} out of u8 range")))?;
    let x_u32 = u32::try_from(x)
        .map_err(|_| MbTilesError::InvalidFormat(format!("tile_column {x} out of u32 range")))?;
    let y_u32 = u32::try_from(y)
        .map_err(|_| MbTilesError::InvalidFormat(format!("tile_row {y} out of u32 range")))?;
    Ok(TileCoord {
        z: z_u8,
        x: x_u32,
        y: y_u32,
    })
}

/// Generate a unique path inside [`std::env::temp_dir`] for the in-memory
/// reader spill file.  Uses process id + a monotonic counter for uniqueness;
/// the file itself is created later by `fs::write`.
fn unique_temp_path() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let mut p = std::env::temp_dir();
    p.push(format!("oxigdal-mbtiles-{pid}-{seq}.sqlite"));
    p
}
