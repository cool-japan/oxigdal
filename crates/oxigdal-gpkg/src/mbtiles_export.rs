//! GeoPackage → MBTiles tile-table extraction.
//!
//! Provides [`GpkgMbTilesExporter`] which reads a tile pyramid from a
//! GeoPackage (via [`TilePyramidReader`]) and writes every tile blob into
//! an MBTiles SQLite file.  The output follows the MBTiles 1.3 specification:
//! - `tiles` table with `(zoom_level, tile_column, tile_row, tile_data)`
//! - `metadata` key-value table
//!
//! GeoPackage stores tiles in XYZ order (Y=0 at top); MBTiles stores tiles in
//! TMS order (Y=0 at bottom).  This module converts automatically using
//! [`xyz_to_tms_row`].
//!
//! All code in this module is compiled only when the `mbtiles-export` Cargo
//! feature is enabled.

#![cfg(feature = "mbtiles-export")]

use rusqlite::{Connection, params};
use std::path::Path;

use crate::error::GpkgError;
use crate::gpkg::GeoPackage;
use crate::tile_matrix::TileMatrix;
use crate::tile_pyramid::TilePyramidReader;

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// Exports a single GeoPackage tile pyramid table into the MBTiles format.
///
/// Tile data blobs are copied verbatim.  The Y-axis convention is converted
/// from GeoPackage/XYZ (top-origin) to MBTiles/TMS (bottom-origin) via
/// `tms_row = 2^zoom - 1 - gpkg_row`.
///
/// # Example
/// ```ignore
/// use oxigdal_gpkg::{GeoPackage, GpkgMbTilesExporter};
///
/// let gpkg_bytes = std::fs::read("imagery.gpkg").unwrap();
/// let gpkg = GeoPackage::from_bytes(gpkg_bytes).unwrap();
/// let exporter = GpkgMbTilesExporter::new(&gpkg, "imagery_tiles").unwrap();
/// let stats = exporter.export_to_path("/tmp/out.mbtiles").unwrap();
/// println!("Wrote {} tiles", stats.tiles_written);
/// ```
pub struct GpkgMbTilesExporter<'a> {
    gpkg: &'a GeoPackage,
    table_name: String,
}

/// Summary statistics produced by a completed MBTiles export.
#[derive(Debug, Clone)]
pub struct GpkgMbTilesStats {
    /// Number of tile rows written (skips None / empty tiles).
    pub tiles_written: u64,
    /// Lowest zoom level present in the source tile pyramid.
    pub min_zoom: u32,
    /// Highest zoom level present in the source tile pyramid.
    pub max_zoom: u32,
    /// Total uncompressed bytes of tile data written.
    pub bytes_written: u64,
    /// Number of `metadata` key-value pairs inserted.
    pub metadata_keys: usize,
    /// Detected tile format: `"png"`, `"jpg"`, or `"webp"`.
    pub format: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// GpkgMbTilesExporter implementation
// ─────────────────────────────────────────────────────────────────────────────

impl<'a> GpkgMbTilesExporter<'a> {
    /// Construct an exporter for the named tile table in `gpkg`.
    ///
    /// Validates that `table_name` is registered in `gpkg_tile_matrix_set` by
    /// opening a [`TilePyramidReader`].
    ///
    /// # Errors
    /// - [`GpkgError::TileSetNotFound`] if `table_name` is not a valid tile
    ///   pyramid in the given GeoPackage.
    /// - Propagates any lower-level [`GpkgError`] from the B-tree scan.
    pub fn new(gpkg: &'a GeoPackage, table_name: &str) -> Result<Self, GpkgError> {
        // Validate by opening the reader (returns TileSetNotFound on bad name)
        let _reader = TilePyramidReader::open(gpkg, table_name)?;
        Ok(Self {
            gpkg,
            table_name: table_name.to_string(),
        })
    }

    /// Export to an MBTiles file at `path`.
    ///
    /// Creates or overwrites the file at `path`.  The file is a valid SQLite
    /// database with `tiles` and `metadata` tables conforming to MBTiles 1.3.
    ///
    /// # Errors
    /// - [`GpkgError::MbTilesExportError`] if the SQLite file cannot be
    ///   opened or if any write fails.
    /// - Propagates any [`GpkgError`] from reading the source GeoPackage.
    pub fn export_to_path<P: AsRef<Path>>(&self, path: P) -> Result<GpkgMbTilesStats, GpkgError> {
        let conn = Connection::open(path.as_ref())
            .map_err(|e| GpkgError::MbTilesExportError(e.to_string()))?;
        self.export_to_connection(&conn)
    }

    /// Export to an already-open rusqlite [`Connection`].
    ///
    /// Creates the `tiles` and `metadata` tables (if they do not exist), then
    /// iterates over every `(zoom, col, row)` slot defined in
    /// `gpkg_tile_matrix`, reads each tile blob, converts the row index from
    /// XYZ to TMS convention, and inserts it.
    ///
    /// Tiles absent from the GeoPackage (sparse pyramids) are silently skipped.
    ///
    /// # Errors
    /// - [`GpkgError::MbTilesExportError`] if any SQLite operation fails.
    /// - Propagates any [`GpkgError`] from reading the source GeoPackage.
    pub fn export_to_connection(&self, conn: &Connection) -> Result<GpkgMbTilesStats, GpkgError> {
        // ── 1. Create MBTiles schema ──────────────────────────────────────────
        create_schema(conn)?;

        // ── 2. Open tile pyramid reader ───────────────────────────────────────
        let reader = TilePyramidReader::open(self.gpkg, &self.table_name)?;
        let zoom_levels = reader.zoom_levels();

        if zoom_levels.is_empty() {
            // No tile matrix rows — return zero-filled stats.
            let metadata_keys = write_metadata(conn, &self.table_name, 0, 0, "png")?;
            return Ok(GpkgMbTilesStats {
                tiles_written: 0,
                min_zoom: 0,
                max_zoom: 0,
                bytes_written: 0,
                metadata_keys,
                format: "png".to_string(),
            });
        }

        // SAFETY: zoom_levels is non-empty — checked immediately above.
        let min_zoom = zoom_levels[0];
        let max_zoom = zoom_levels[zoom_levels.len() - 1];

        // ── 3. Prepare tile insertion statement ───────────────────────────────
        let mut insert_stmt = conn
            .prepare(
                "INSERT OR REPLACE INTO tiles \
                 (zoom_level, tile_column, tile_row, tile_data) \
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .map_err(|e| GpkgError::MbTilesExportError(e.to_string()))?;

        let mut tiles_written: u64 = 0;
        let mut bytes_written: u64 = 0;
        let mut detected_format: Option<String> = None;

        // ── 4. Iterate all (zoom, col, row) slots defined in tile_matrix ──────
        for zoom in &zoom_levels {
            let matrix: &TileMatrix = match reader.tile_matrix(*zoom) {
                Some(m) => m,
                None => continue,
            };

            let cols = matrix.matrix_width;
            let rows = matrix.matrix_height;

            for col in 0..cols {
                for gpkg_row in 0..rows {
                    let blob = match reader.get_tile(*zoom, col, gpkg_row)? {
                        Some(b) if !b.is_empty() => b,
                        _ => continue, // absent or empty tile — sparse pyramid
                    };

                    // Detect format from first tile's magic bytes
                    if detected_format.is_none() {
                        detected_format = Some(detect_format_from_blob(&blob).to_string());
                    }

                    let tms_row = xyz_to_tms_row(*zoom, gpkg_row);
                    bytes_written += blob.len() as u64;

                    insert_stmt
                        .execute(params![*zoom, col, tms_row, blob.as_slice()])
                        .map_err(|e| GpkgError::MbTilesExportError(e.to_string()))?;

                    tiles_written += 1;
                }
            }
        }

        drop(insert_stmt);

        // ── 5. Write metadata ─────────────────────────────────────────────────
        let format = detected_format.unwrap_or_else(|| "png".to_string());
        let metadata_keys = write_metadata(conn, &self.table_name, min_zoom, max_zoom, &format)?;

        Ok(GpkgMbTilesStats {
            tiles_written,
            min_zoom,
            max_zoom,
            bytes_written,
            metadata_keys,
            format,
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Schema creation
// ─────────────────────────────────────────────────────────────────────────────

/// Create the MBTiles 1.3 schema in `conn`.
///
/// Uses `PRAGMA journal_mode=OFF` and `PRAGMA synchronous=OFF` to maximise
/// write throughput during bulk export.  `tiles` is a `WITHOUT ROWID` table
/// with a composite primary key for efficient lookup after export.
pub fn create_schema(conn: &Connection) -> Result<(), GpkgError> {
    conn.execute_batch(
        "
        PRAGMA journal_mode=OFF;
        PRAGMA synchronous=OFF;
        CREATE TABLE IF NOT EXISTS metadata (
            name  TEXT NOT NULL,
            value TEXT
        );
        CREATE TABLE IF NOT EXISTS tiles (
            zoom_level  INTEGER NOT NULL,
            tile_column INTEGER NOT NULL,
            tile_row    INTEGER NOT NULL,
            tile_data   BLOB    NOT NULL,
            PRIMARY KEY (zoom_level, tile_column, tile_row)
        ) WITHOUT ROWID;
        ",
    )
    .map_err(|e| GpkgError::MbTilesExportError(e.to_string()))
}

// ─────────────────────────────────────────────────────────────────────────────
// Metadata insertion
// ─────────────────────────────────────────────────────────────────────────────

/// Insert required MBTiles metadata key-value rows.
///
/// Writes `name`, `type`, `version`, `description`, `format`, `minzoom`, and
/// `maxzoom` — seven rows in total per MBTiles 1.3 §2.
///
/// Returns the count of rows inserted.
fn write_metadata(
    conn: &Connection,
    table_name: &str,
    min_zoom: u32,
    max_zoom: u32,
    format: &str,
) -> Result<usize, GpkgError> {
    let rows: &[(&str, String)] = &[
        ("name", table_name.to_string()),
        ("type", "overlay".to_string()),
        ("version", "1".to_string()),
        (
            "description",
            format!("Exported from GeoPackage table '{table_name}'"),
        ),
        ("format", format.to_string()),
        ("minzoom", min_zoom.to_string()),
        ("maxzoom", max_zoom.to_string()),
    ];

    for (k, v) in rows {
        conn.execute(
            "INSERT INTO metadata (name, value) VALUES (?1, ?2)",
            params![k, v],
        )
        .map_err(|e| GpkgError::MbTilesExportError(e.to_string()))?;
    }

    Ok(rows.len())
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper: tile format detection from magic bytes
// ─────────────────────────────────────────────────────────────────────────────

/// Detect the image format of a tile blob from its leading magic bytes.
///
/// Recognises PNG (`\x89PNG`), JPEG (`\xff\xd8\xff`), and WebP (`RIFF…WEBP`).
/// Falls back to `"png"` for unrecognised byte sequences.
pub fn detect_format_from_blob(blob: &[u8]) -> &'static str {
    if blob.starts_with(b"\x89PNG") {
        "png"
    } else if blob.starts_with(b"\xff\xd8\xff") {
        "jpg"
    } else if blob.starts_with(b"RIFF") && blob.get(8..12) == Some(b"WEBP") {
        "webp"
    } else {
        "png"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper: XYZ → TMS row conversion
// ─────────────────────────────────────────────────────────────────────────────

/// Convert a GeoPackage (XYZ, top-origin) tile row to a TMS (bottom-origin)
/// row index as required by MBTiles.
///
/// The conversion is:
/// ```text
/// tms_row = 2^zoom - 1 - xyz_row
/// ```
///
/// At zoom 0 the single tile maps to itself (`tms_row = 0`).  At higher zooms
/// the rows are mirrored around the vertical midpoint.
pub fn xyz_to_tms_row(zoom: u32, xyz_row: u32) -> u32 {
    (1u32 << zoom).saturating_sub(1).saturating_sub(xyz_row)
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests (private helpers)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod unit_tests {
    use super::{detect_format_from_blob, xyz_to_tms_row};

    #[test]
    fn test_xyz_to_tms_row_zoom0() {
        // At zoom 0 there is one tile; row 0 maps to TMS row 0.
        assert_eq!(xyz_to_tms_row(0, 0), 0);
    }

    #[test]
    fn test_xyz_to_tms_row_zoom1() {
        // At zoom 1: 2^1 - 1 = 1
        assert_eq!(xyz_to_tms_row(1, 0), 1);
        assert_eq!(xyz_to_tms_row(1, 1), 0);
    }

    #[test]
    fn test_xyz_to_tms_row_zoom10() {
        // 2^10 - 1 - 500 = 1023 - 500 = 523
        assert_eq!(xyz_to_tms_row(10, 500), 523);
    }

    #[test]
    fn test_detect_format_png() {
        let blob = b"\x89PNG\r\n\x1a\n";
        assert_eq!(detect_format_from_blob(blob), "png");
    }

    #[test]
    fn test_detect_format_jpeg() {
        let blob = b"\xff\xd8\xff\xe0";
        assert_eq!(detect_format_from_blob(blob), "jpg");
    }

    #[test]
    fn test_detect_format_webp() {
        let blob = b"RIFF\x00\x00\x00\x00WEBP";
        assert_eq!(detect_format_from_blob(blob), "webp");
    }

    #[test]
    fn test_detect_format_unknown_falls_back_to_png() {
        let blob = b"\x00\x01\x02\x03";
        assert_eq!(detect_format_from_blob(blob), "png");
    }
}
