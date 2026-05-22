//! PMTiles → MBTiles export.
//!
//! Converts a [`PmTilesReader`] archive into an MBTiles SQLite database.
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

#![cfg(feature = "mbtiles")]

use std::path::Path;

use rusqlite::{Connection, params};

use crate::error::PmTilesError;
use crate::header::{PmTilesHeader, TileType};
use crate::pmtiles::PmTilesReader;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// High-level exporter that converts a [`PmTilesReader`] archive into an
/// MBTiles SQLite database.
///
/// # Example
/// ```no_run
/// use oxigdal_pmtiles::MbTilesExporter;
/// use oxigdal_pmtiles::PmTilesReader;
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
    /// # Errors
    /// Returns [`PmTilesError::Io`] on filesystem errors and
    /// [`PmTilesError::SqliteError`] on database errors.
    pub fn export_to_path<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<MbTilesExportStats, PmTilesError> {
        let path = path.as_ref();
        // Remove any pre-existing file so that `Connection::open` creates a
        // completely fresh database.  The MBTiles spec defines the format as
        // a single self-contained SQLite file; overwriting is the correct
        // behaviour for a batch export operation.
        if path.exists() {
            std::fs::remove_file(path).map_err(PmTilesError::Io)?;
        }
        let conn = Connection::open(path).map_err(|e| PmTilesError::SqliteError(e.to_string()))?;
        self.export_to_connection(&conn)
    }

    /// Export the archive into an existing (possibly in-memory) SQLite
    /// [`Connection`].
    ///
    /// The schema is created inside the provided connection, so this method
    /// can be called with `Connection::open_in_memory()` for testing.
    ///
    /// WAL journalling is intentionally disabled (`journal_mode=OFF`) and
    /// fsync is suppressed (`synchronous=OFF`) to maximise bulk-insert
    /// throughput.  These pragmas are only appropriate for initial, batch
    /// writes; do not use an exported database as an operational write-ahead
    /// store.
    ///
    /// # Errors
    /// Returns [`PmTilesError::SqliteError`] on any SQLite-level failure and
    /// propagates tile-retrieval errors from the underlying [`PmTilesReader`].
    pub fn export_to_connection(
        &self,
        conn: &Connection,
    ) -> Result<MbTilesExportStats, PmTilesError> {
        // ------------------------------------------------------------------
        // Step 1: Create the MBTiles schema.
        // ------------------------------------------------------------------
        create_schema(conn)?;

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
        let metadata_keys = write_metadata(conn, header, metadata_json_str.as_deref())?;

        // ------------------------------------------------------------------
        // Step 3: Bulk-insert tiles.
        //
        // Performance pragmas: disabling the journal and fsync is safe here
        // because we are writing a *new* database from scratch.  Any crash
        // mid-write simply leaves a corrupt file that can be regenerated by
        // re-running the export.
        // ------------------------------------------------------------------
        conn.execute_batch("PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;")
            .map_err(|e| PmTilesError::SqliteError(e.to_string()))?;

        let tile_infos = self.reader.enumerate_tiles()?;

        let mut tiles_written: u64 = 0;
        let mut bytes_written: u64 = 0;
        let mut min_zoom: u8 = u8::MAX;
        let mut max_zoom: u8 = 0;

        // Prepare the INSERT statement once and reuse it for every tile.
        let mut stmt = conn
            .prepare(
                "INSERT OR REPLACE INTO tiles \
                 (zoom_level, tile_column, tile_row, tile_data) \
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .map_err(|e| PmTilesError::SqliteError(e.to_string()))?;

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

            stmt.execute(params![
                tile_info.z as i64,
                tile_info.x as i64,
                tms_row as i64,
                tile_data,
            ])
            .map_err(|e| PmTilesError::SqliteError(e.to_string()))?;

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

/// Create the canonical MBTiles schema inside `conn`.
///
/// Uses `IF NOT EXISTS` guards so that this is idempotent when called on a
/// connection that already has the schema (e.g. an in-memory connection reused
/// across multiple test assertions).
///
/// The `tiles` table is declared `WITHOUT ROWID` because (zoom, column, row)
/// is already the primary key and serves as the unique row identifier.
/// This avoids an implicit extra 64-bit integer rowid column and makes
/// point-lookups on the PK slightly more efficient.
fn create_schema(conn: &Connection) -> Result<(), PmTilesError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS metadata (
            name  TEXT NOT NULL,
            value TEXT
        );
        CREATE UNIQUE INDEX IF NOT EXISTS name ON metadata (name);

        CREATE TABLE IF NOT EXISTS tiles (
            zoom_level  INTEGER NOT NULL,
            tile_column INTEGER NOT NULL,
            tile_row    INTEGER NOT NULL,
            tile_data   BLOB,
            PRIMARY KEY (zoom_level, tile_column, tile_row)
        ) WITHOUT ROWID;
        ",
    )
    .map_err(|e| PmTilesError::SqliteError(e.to_string()))
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
    conn: &Connection,
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
        conn.execute(
            "INSERT OR REPLACE INTO metadata (name, value) VALUES (?1, ?2)",
            params![name, value],
        )
        .map_err(|e| PmTilesError::SqliteError(e.to_string()))?;
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
