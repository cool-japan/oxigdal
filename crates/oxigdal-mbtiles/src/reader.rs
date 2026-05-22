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
//! root for the wiring.  The implementation:
//!
//! 1. Opens the SQLite file **read-only** (`SQLITE_OPEN_READ_ONLY`).
//! 2. Verifies that both required tables (`metadata`, `tiles`) exist.
//! 3. Eagerly loads the `metadata` rows via [`MBTilesMetadata::from_map_strict`]
//!    so malformed `bounds` / `center` / `minzoom` / `maxzoom` values fail
//!    fast with a typed [`MbTilesError::InvalidMetadata`].
//! 4. Exposes lazy accessors for tile data; the database stays open for the
//!    lifetime of the [`MBTilesReader`].
//!
//! ## In-memory archives
//!
//! [`MBTilesReader::open_in_memory`] accepts a `&[u8]` containing an entire
//! SQLite database image.  rusqlite supports `:memory:` databases natively
//! but cannot deserialize a byte buffer into one without the `serialize`
//! feature; to stay on the workspace-pinned dependency set we instead spill
//! the bytes to a temporary file inside [`std::env::temp_dir`] and open it
//! read-only.  The temp file is deleted as soon as the reader is dropped.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use crate::error::MbTilesError;
use crate::mbtiles::{MBTiles, MBTilesMetadata};
use crate::tile_coords::TileCoord;

/// Read-only handle to an on-disk MBTiles SQLite database.
///
/// The connection is opened with `SQLITE_OPEN_READ_ONLY`, so the underlying
/// file is never mutated.  Metadata is loaded eagerly at construction time;
/// tile bytes are fetched lazily on every [`Self::get_tile`] call.
#[derive(Debug)]
pub struct MBTilesReader {
    conn: Connection,
    metadata: MBTilesMetadata,
    /// Path to a temp file created by [`Self::open_in_memory`], if any.
    /// Deleted on drop.
    owned_temp_path: Option<PathBuf>,
}

impl MBTilesReader {
    /// Open an existing `.mbtiles` file at `path` for read-only access.
    ///
    /// # Errors
    ///
    /// * [`MbTilesError::Sqlite`] — the file is not a valid SQLite database
    ///   or cannot be opened for reading.
    /// * [`MbTilesError::InvalidFormat`] — one of the mandatory MBTiles tables
    ///   (`metadata`, `tiles`) is missing.
    /// * [`MbTilesError::InvalidMetadata`] — a canonical metadata field
    ///   (`bounds`, `center`, `minzoom`, `maxzoom`) is malformed.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, MbTilesError> {
        let conn = Connection::open_with_flags(
            path.as_ref(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        require_table(&conn, "metadata")?;
        require_table(&conn, "tiles")?;
        let metadata = load_metadata(&conn)?;
        Ok(Self {
            conn,
            metadata,
            owned_temp_path: None,
        })
    }

    /// Open a `.mbtiles` archive supplied as an in-memory byte buffer.
    ///
    /// Spills `bytes` to a uniquely named temp file inside
    /// [`std::env::temp_dir`] and opens it read-only.  The temp file is
    /// removed when the returned [`MBTilesReader`] is dropped.
    ///
    /// # Errors
    ///
    /// Same as [`Self::open`], plus [`MbTilesError::Io`] if the temp file
    /// cannot be written.
    pub fn open_in_memory(bytes: &[u8]) -> Result<Self, MbTilesError> {
        let temp_path = unique_temp_path();
        fs::write(&temp_path, bytes)?;

        // Open via the standard read-only path; on success take ownership of
        // the temp file so Drop can clean up.  On any error we delete the
        // temp file ourselves and propagate.
        let conn_result = Connection::open_with_flags(
            &temp_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        );
        let conn = match conn_result {
            Ok(c) => c,
            Err(e) => {
                let _ = fs::remove_file(&temp_path);
                return Err(MbTilesError::from(e));
            }
        };

        let validate = (|| -> Result<MBTilesMetadata, MbTilesError> {
            require_table(&conn, "metadata")?;
            require_table(&conn, "tiles")?;
            load_metadata(&conn)
        })();

        match validate {
            Ok(metadata) => Ok(Self {
                conn,
                metadata,
                owned_temp_path: Some(temp_path),
            }),
            Err(e) => {
                drop(conn);
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
        let mut stmt = self.conn.prepare(
            "SELECT zoom_level, tile_column, tile_row
             FROM tiles
             ORDER BY zoom_level ASC, tile_column ASC, tile_row ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let z: i64 = row.get(0)?;
            let x: i64 = row.get(1)?;
            let y: i64 = row.get(2)?;
            Ok((z, x, y))
        })?;
        let mut coords = Vec::new();
        for row in rows {
            let (z, x, y) = row?;
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
        let mut stmt = self.conn.prepare(
            "SELECT tile_data
             FROM tiles
             WHERE zoom_level = ?1 AND tile_column = ?2 AND tile_row = ?3
             LIMIT 1",
        )?;
        let mut rows = stmt.query(rusqlite::params![
            coord.z as i64,
            coord.x as i64,
            coord.y as i64,
        ])?;
        match rows.next()? {
            Some(row) => {
                let blob: Vec<u8> = row.get(0)?;
                Ok(Some(blob))
            }
            None => Ok(None),
        }
    }

    /// Total number of tile rows in the archive.
    pub fn tile_count(&self) -> Result<usize, MbTilesError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM tiles", [], |row| row.get(0))?;
        usize::try_from(count).map_err(|_| {
            MbTilesError::InvalidFormat(format!("tile count out of usize range: {count}"))
        })
    }

    /// Distinct zoom levels present in the archive, sorted ascending.
    pub fn zoom_levels(&self) -> Result<Vec<u8>, MbTilesError> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT zoom_level FROM tiles ORDER BY zoom_level ASC")?;
        let rows = stmt.query_map([], |row| {
            let z: i64 = row.get(0)?;
            Ok(z)
        })?;
        let mut zooms = Vec::new();
        for row in rows {
            let z = row?;
            let z_u8 = u8::try_from(z).map_err(|_| {
                MbTilesError::InvalidFormat(format!("zoom level {z} out of u8 range"))
            })?;
            zooms.push(z_u8);
        }
        Ok(zooms)
    }

    /// Eagerly load every tile into a new in-memory [`MBTiles`] store.
    ///
    /// Convenient for small archives or tests; for large pyramids prefer the
    /// lazy [`Self::get_tile`] / [`Self::list_tiles`] accessors.
    pub fn into_mbtiles(self) -> Result<MBTiles, MbTilesError> {
        let mut store = MBTiles::new(self.metadata.clone());
        let mut stmt = self.conn.prepare(
            "SELECT zoom_level, tile_column, tile_row, tile_data
             FROM tiles
             ORDER BY zoom_level ASC, tile_column ASC, tile_row ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let z: i64 = row.get(0)?;
            let x: i64 = row.get(1)?;
            let y: i64 = row.get(2)?;
            let data: Vec<u8> = row.get(3)?;
            Ok((z, x, y, data))
        })?;
        for row in rows {
            let (z, x, y, data) = row?;
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

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Confirm that a table with `name` exists in the schema, otherwise return
/// [`MbTilesError::InvalidFormat`].
fn require_table(conn: &Connection, name: &str) -> Result<(), MbTilesError> {
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |row| row.get(0),
    )?;
    if exists == 0 {
        return Err(MbTilesError::InvalidFormat(format!(
            "missing mandatory MBTiles table: {name}"
        )));
    }
    Ok(())
}

/// Read every `(name, value)` row from the `metadata` table and feed it
/// through [`MBTilesMetadata::from_map_strict`].
fn load_metadata(conn: &Connection) -> Result<MBTilesMetadata, MbTilesError> {
    let mut stmt = conn.prepare("SELECT name, value FROM metadata")?;
    let rows = stmt.query_map([], |row| {
        let key: String = row.get(0)?;
        let value: String = row.get(1)?;
        Ok((key, value))
    })?;
    let mut map: HashMap<String, String> = HashMap::new();
    for row in rows {
        let (k, v) = row?;
        map.insert(k, v);
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
