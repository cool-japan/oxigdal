//! GeoPackage tile pyramid reader.
//!
//! Provides [`TilePyramidReader`], a type that reads tile blobs from a
//! GeoPackage tile pyramid table by `(zoom_level, tile_column, tile_row)`.
//!
//! Reference: OGC GeoPackage Encoding Standard v1.3.1, §2.2 (Tiles).

use std::collections::BTreeMap;
use std::fmt;

use crate::btree::CellValue;
use crate::error::GpkgError;
use crate::gpkg::GeoPackage;
use crate::tile_matrix::TileMatrix;

// ─────────────────────────────────────────────────────────────────────────────
// TilePyramidReader
// ─────────────────────────────────────────────────────────────────────────────

/// A reader for a single GeoPackage tile pyramid content table.
///
/// Constructed via [`TilePyramidReader::open`], which validates that the given
/// table is registered in `gpkg_tile_matrix_set` and loads all
/// `gpkg_tile_matrix` rows for the table into an in-memory index.
///
/// Individual tile blobs are retrieved on demand via [`TilePyramidReader::get_tile`].
pub struct TilePyramidReader<'a> {
    /// Reference to the parent GeoPackage.
    gpkg: &'a GeoPackage,
    /// Name of the user-data tile pyramid table.
    table_name: String,
    /// Tile matrix metadata keyed by zoom level.
    tile_matrices: BTreeMap<u32, TileMatrix>,
}

impl<'a> fmt::Debug for TilePyramidReader<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TilePyramidReader")
            .field("table_name", &self.table_name)
            .field("zoom_levels", &self.zoom_levels())
            .finish()
    }
}

impl<'a> TilePyramidReader<'a> {
    /// Open a tile pyramid for the table named `table_name`.
    ///
    /// Validates that `table_name` is present in `gpkg_tile_matrix_set`, then
    /// loads all matching rows from `gpkg_tile_matrix` into an in-memory index.
    ///
    /// # Errors
    /// - [`GpkgError::TileSetNotFound`] when `table_name` has no entry in
    ///   `gpkg_tile_matrix_set`.
    /// - [`GpkgError::TableNotFound`] when `gpkg_tile_matrix_set` or
    ///   `gpkg_tile_matrix` tables are absent from `sqlite_master`.
    /// - [`GpkgError::InvalidFormat`] on malformed rows.
    /// - Propagates any lower-level [`GpkgError`] from the B-tree scan.
    pub fn open(gpkg: &'a GeoPackage, table_name: &str) -> Result<Self, GpkgError> {
        // ── 1. Validate tile_matrix_set registration ──────────────────────────
        let tms_rows = gpkg
            .scan_table_by_name("gpkg_tile_matrix_set")?
            .ok_or_else(|| GpkgError::TableNotFound("gpkg_tile_matrix_set".to_string()))?;

        // gpkg_tile_matrix_set column layout (OGC GeoPackage §2.2.7):
        //   0: table_name  TEXT
        //   1: srs_id      INTEGER
        //   2: min_x       REAL
        //   3: min_y       REAL
        //   4: max_x       REAL
        //   5: max_y       REAL
        let registered = tms_rows.iter().any(|(_rowid, cols)| {
            cols.first()
                .map(|v| cell_as_str(v) == table_name)
                .unwrap_or(false)
        });
        if !registered {
            return Err(GpkgError::TileSetNotFound(table_name.to_string()));
        }

        // ── 2. Load tile matrix rows ──────────────────────────────────────────
        let tm_rows = gpkg
            .scan_table_by_name("gpkg_tile_matrix")?
            .ok_or_else(|| GpkgError::TableNotFound("gpkg_tile_matrix".to_string()))?;

        // gpkg_tile_matrix column layout (OGC GeoPackage §2.2.6):
        //   0: table_name   TEXT
        //   1: zoom_level   INTEGER
        //   2: matrix_width INTEGER
        //   3: matrix_height INTEGER
        //   4: tile_width    INTEGER
        //   5: tile_height   INTEGER
        //   6: pixel_x_size  REAL
        //   7: pixel_y_size  REAL
        let mut tile_matrices: BTreeMap<u32, TileMatrix> = BTreeMap::new();
        for (_rowid, cols) in &tm_rows {
            if cols.len() < 8 {
                // Malformed row — skip rather than abort so that partially-written
                // tiles tables do not prevent reading well-formed rows.
                continue;
            }
            let row_table_name = cell_as_str(&cols[0]);
            if row_table_name != table_name {
                continue;
            }
            let zoom_level = cell_as_u32(&cols[1]).ok_or_else(|| {
                GpkgError::InvalidFormat(format!(
                    "gpkg_tile_matrix row has non-integer zoom_level for table '{table_name}'"
                ))
            })?;
            let matrix_width = cell_as_u32(&cols[2]).ok_or_else(|| {
                GpkgError::InvalidFormat(format!(
                    "gpkg_tile_matrix row has non-integer matrix_width (zoom {zoom_level})"
                ))
            })?;
            let matrix_height = cell_as_u32(&cols[3]).ok_or_else(|| {
                GpkgError::InvalidFormat(format!(
                    "gpkg_tile_matrix row has non-integer matrix_height (zoom {zoom_level})"
                ))
            })?;
            let tile_width = cell_as_u32(&cols[4]).ok_or_else(|| {
                GpkgError::InvalidFormat(format!(
                    "gpkg_tile_matrix row has non-integer tile_width (zoom {zoom_level})"
                ))
            })?;
            let tile_height = cell_as_u32(&cols[5]).ok_or_else(|| {
                GpkgError::InvalidFormat(format!(
                    "gpkg_tile_matrix row has non-integer tile_height (zoom {zoom_level})"
                ))
            })?;
            let pixel_x_size = cell_as_f64(&cols[6]);
            let pixel_y_size = cell_as_f64(&cols[7]);

            let matrix = TileMatrix {
                table_name: row_table_name,
                zoom_level,
                matrix_width,
                matrix_height,
                tile_width,
                tile_height,
                pixel_x_size,
                pixel_y_size,
            };
            tile_matrices.insert(zoom_level, matrix);
        }

        Ok(Self {
            gpkg,
            table_name: table_name.to_string(),
            tile_matrices,
        })
    }

    /// Retrieve the raw tile blob for the given `(zoom, col, row)` triple.
    ///
    /// Performs a full scan of the tile pyramid table and filters by the three
    /// key columns, returning the `tile_data` blob on the first matching row.
    ///
    /// Returns `Ok(None)` when the tile is not present in the table (sparse
    /// tile pyramids commonly omit tiles for regions with no data).
    ///
    /// # Column layout of OGC tile pyramid tables (§2.2.5)
    /// | # | column       | type    |
    /// |---|--------------|---------|
    /// | 0 | zoom_level   | INTEGER |
    /// | 1 | tile_column  | INTEGER |
    /// | 2 | tile_row     | INTEGER |
    /// | 3 | tile_data    | BLOB    |
    ///
    /// # Errors
    /// - [`GpkgError::TableNotFound`] when the tile pyramid table is absent.
    /// - Propagates any lower-level [`GpkgError`] from the B-tree scan.
    pub fn get_tile(&self, zoom: u32, col: u32, row: u32) -> Result<Option<Vec<u8>>, GpkgError> {
        let rows = self
            .gpkg
            .scan_table_by_name(&self.table_name)?
            .ok_or_else(|| GpkgError::TableNotFound(self.table_name.clone()))?;

        for (_rowid, cols) in rows {
            // Require at least 4 columns: zoom_level, tile_column, tile_row, tile_data
            if cols.len() < 4 {
                continue;
            }
            let row_zoom = match cell_as_u32(&cols[0]) {
                Some(z) => z,
                None => continue,
            };
            if row_zoom != zoom {
                continue;
            }
            let row_col = match cell_as_u32(&cols[1]) {
                Some(c) => c,
                None => continue,
            };
            if row_col != col {
                continue;
            }
            let row_row = match cell_as_u32(&cols[2]) {
                Some(r) => r,
                None => continue,
            };
            if row_row != row {
                continue;
            }
            // Matching tile found — extract blob
            let blob = match &cols[3] {
                CellValue::Blob(bytes) => bytes.clone(),
                CellValue::Null => {
                    // Tile exists but has no data — return empty vec rather than None
                    // to distinguish "tile row present, no data" from "tile absent".
                    Vec::new()
                }
                other => {
                    return Err(GpkgError::InvalidFormat(format!(
                        "tile_data column has unexpected type for ({zoom},{col},{row}): {other:?}"
                    )));
                }
            };
            return Ok(Some(blob));
        }

        Ok(None)
    }

    /// Return the [`TileMatrix`] for the given zoom level, if present.
    pub fn tile_matrix(&self, zoom: u32) -> Option<&TileMatrix> {
        self.tile_matrices.get(&zoom)
    }

    /// Return a sorted list of all zoom levels for which a `TileMatrix` row exists.
    ///
    /// The list is in ascending order because the internal storage is a [`BTreeMap`].
    pub fn zoom_levels(&self) -> Vec<u32> {
        self.tile_matrices.keys().copied().collect()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Cell-value coercion helpers (private)
// ─────────────────────────────────────────────────────────────────────────────

/// Extract the string representation of a `CellValue`, returning an empty
/// string for `Null`.
fn cell_as_str(v: &CellValue) -> String {
    match v {
        CellValue::Text(s) => s.clone(),
        CellValue::Integer(i) => i.to_string(),
        CellValue::Float(f) => f.to_string(),
        CellValue::Blob(b) => String::from_utf8_lossy(b).into_owned(),
        CellValue::Null => String::new(),
    }
}

/// Extract a `u32` from a `CellValue::Integer`, returning `None` for
/// non-integer variants or out-of-range values.
fn cell_as_u32(v: &CellValue) -> Option<u32> {
    match v {
        CellValue::Integer(i) if *i >= 0 && *i <= u32::MAX as i64 => Some(*i as u32),
        _ => None,
    }
}

/// Extract an `f64` from a `CellValue`, coercing integers where needed.
fn cell_as_f64(v: &CellValue) -> f64 {
    match v {
        CellValue::Float(f) => *f,
        CellValue::Integer(i) => *i as f64,
        _ => 0.0,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::btree::encode_sqlite_varint;

    // ── Low-level page builders (copied from integration test helpers) ─────────

    /// Build a leaf table B-tree page with the given rowid-payload cells.
    ///
    /// `header_offset` is `100` for page 1 (which shares the SQLite file header)
    /// and `0` for any other page.
    fn build_leaf_table_page(
        page_size: usize,
        cells: &[(i64, &[u8])],
        header_offset: usize,
    ) -> Vec<u8> {
        let mut page = vec![0u8; page_size];
        let cell_count = cells.len();

        let mut content_end = page_size;
        let mut cell_offsets: Vec<usize> = Vec::with_capacity(cell_count);

        for (rowid, payload) in cells {
            let pl_varint = encode_sqlite_varint(payload.len() as u64);
            let rid_varint = encode_sqlite_varint(*rowid as u64);
            let cell_size = pl_varint.len() + rid_varint.len() + payload.len();

            content_end -= cell_size;
            let start = content_end;
            cell_offsets.push(start);

            let mut pos = start;
            page[pos..pos + pl_varint.len()].copy_from_slice(&pl_varint);
            pos += pl_varint.len();
            page[pos..pos + rid_varint.len()].copy_from_slice(&rid_varint);
            pos += rid_varint.len();
            page[pos..pos + payload.len()].copy_from_slice(payload);
        }

        let hdr = header_offset;
        page[hdr] = 13; // leaf table page type
        page[hdr + 1] = 0;
        page[hdr + 2] = 0;
        page[hdr + 3] = ((cell_count >> 8) & 0xFF) as u8;
        page[hdr + 4] = (cell_count & 0xFF) as u8;
        let content_start = content_end as u16;
        page[hdr + 5] = ((content_start >> 8) & 0xFF) as u8;
        page[hdr + 6] = (content_start & 0xFF) as u8;
        page[hdr + 7] = 0;

        let ptr_start = hdr + 8;
        for (i, offset) in cell_offsets.iter().enumerate() {
            let o = *offset as u16;
            page[ptr_start + i * 2] = ((o >> 8) & 0xFF) as u8;
            page[ptr_start + i * 2 + 1] = (o & 0xFF) as u8;
        }

        page
    }

    /// Encode a SQLite record payload from `(serial_type, value_bytes)` pairs.
    fn encode_record(fields: &[(u64, &[u8])]) -> Vec<u8> {
        let serial_type_varints: Vec<Vec<u8>> = fields
            .iter()
            .map(|(st, _)| encode_sqlite_varint(*st))
            .collect();
        let st_bytes: usize = serial_type_varints.iter().map(|v| v.len()).sum();

        let mut hdr_len = st_bytes + 1;
        let hdr_varint = encode_sqlite_varint(hdr_len as u64);
        if hdr_varint.len() != 1 {
            hdr_len = st_bytes + hdr_varint.len();
        }
        let hdr_varint = encode_sqlite_varint(hdr_len as u64);

        let mut out = Vec::new();
        out.extend_from_slice(&hdr_varint);
        for v in &serial_type_varints {
            out.extend_from_slice(v);
        }
        for (_, bytes) in fields {
            out.extend_from_slice(bytes);
        }
        out
    }

    /// Write the minimal SQLite file header into the first 100 bytes of `data`.
    fn write_sqlite_header(data: &mut [u8], page_size: u16, db_size_pages: u32) {
        data[..16].copy_from_slice(b"SQLite format 3\x00");
        data[16..18].copy_from_slice(&page_size.to_be_bytes());
        data[28..32].copy_from_slice(&db_size_pages.to_be_bytes());
        data[56..60].copy_from_slice(&1u32.to_be_bytes()); // UTF-8
    }

    // ── sqlite_master record builder ───────────────────────────────────────────

    /// Encode a single `sqlite_master` row record for a leaf table that lives at
    /// the given root page.
    fn master_row_record(name: &str, root_page: u8) -> Vec<u8> {
        let entry_type = b"table";
        let name_bytes = name.as_bytes();
        let tbl_name_bytes = name.as_bytes();
        let sql = format!("CREATE TABLE {name}(id INTEGER)").into_bytes();
        let rootpage_bytes = [root_page];

        let st_type = (entry_type.len() as u64) * 2 + 13;
        let st_name = (name_bytes.len() as u64) * 2 + 13;
        let st_tbl = (tbl_name_bytes.len() as u64) * 2 + 13;
        let st_root = 1u64; // i8
        let st_sql = (sql.len() as u64) * 2 + 13;

        encode_record(&[
            (st_type, entry_type as &[u8]),
            (st_name, name_bytes),
            (st_tbl, tbl_name_bytes),
            (st_root, &rootpage_bytes),
            (st_sql, &sql),
        ])
    }

    // ── Text cell helper ───────────────────────────────────────────────────────

    /// Encode a TEXT serial-type and value bytes for use in `encode_record`.
    fn text_st(s: &str) -> (u64, Vec<u8>) {
        let bytes = s.as_bytes().to_vec();
        let st = (bytes.len() as u64) * 2 + 13;
        (st, bytes)
    }

    /// Encode a non-negative i8-range integer (serial type 1, 1 byte).
    fn int1_st(v: u8) -> (u64, Vec<u8>) {
        (1u64, vec![v])
    }

    /// Encode an IEEE-754 float (serial type 7, 8 bytes big-endian).
    fn float_st(v: f64) -> (u64, Vec<u8>) {
        (7u64, v.to_be_bytes().to_vec())
    }

    // ── Multi-table GeoPackage builder ─────────────────────────────────────────

    /// A table specification: `(name, rows)` where each row is `(rowid, record_payload)`.
    type TableSpec<'t> = (&'t str, Vec<(i64, Vec<u8>)>);

    /// Build a minimal in-memory GeoPackage with:
    ///
    /// - Page 1 : `sqlite_master` (leaf, header_offset=100) — one or more rows
    /// - Page 2+: user data tables
    ///
    /// The `tables` slice is a list of `(name, page_cells)` pairs, where
    /// `page_cells` are the `(rowid, record_payload)` tuples for that table's
    /// leaf page.  Tables are placed starting at page 2 in the given order.
    ///
    /// The `sqlite_master` leaf page is built last (page 1) referencing all table
    /// root pages.
    fn build_gpkg_with_tables(page_size: usize, tables: &[TableSpec<'_>]) -> Vec<u8> {
        let n_pages = 1 + tables.len();
        let mut file = vec![0u8; page_size * n_pages];

        // Write data table pages starting at page 2.
        for (i, (_name, rows)) in tables.iter().enumerate() {
            let page_idx = i + 1; // 0-based; page 2 is index 1
            let rows_ref: Vec<(i64, &[u8])> = rows
                .iter()
                .map(|(rowid, payload)| (*rowid, payload.as_slice()))
                .collect();
            let page_bytes = build_leaf_table_page(page_size, &rows_ref, 0);
            file[page_idx * page_size..(page_idx + 1) * page_size].copy_from_slice(&page_bytes);
        }

        // Build sqlite_master on page 1 — one row per table, referencing page 2+.
        let master_rows: Vec<(i64, Vec<u8>)> = tables
            .iter()
            .enumerate()
            .map(|(i, (name, _))| {
                let root_page = (i + 2) as u8; // pages 2, 3, 4, …
                let record = master_row_record(name, root_page);
                ((i + 1) as i64, record)
            })
            .collect();
        let master_refs: Vec<(i64, &[u8])> = master_rows
            .iter()
            .map(|(rowid, payload)| (*rowid, payload.as_slice()))
            .collect();
        let master_page = build_leaf_table_page(page_size, &master_refs, 100);
        file[..page_size].copy_from_slice(&master_page);

        write_sqlite_header(&mut file, page_size as u16, n_pages as u32);
        file
    }

    // ── gpkg_tile_matrix_set row record ───────────────────────────────────────

    /// Encode a `gpkg_tile_matrix_set` row.
    ///
    /// Column layout (§2.2.7):
    ///   0: table_name  TEXT
    ///   1: srs_id      INTEGER (i8)
    ///   2: min_x       REAL
    ///   3: min_y       REAL
    ///   4: max_x       REAL
    ///   5: max_y       REAL
    fn tile_matrix_set_row(table_name: &str) -> Vec<u8> {
        let (st_name, name_bytes) = text_st(table_name);
        let (st_srs, srs_bytes) = int1_st(4); // srs_id = 4326 doesn't fit i8; use 4 as placeholder
        let (st_minx, minx_bytes) = float_st(-180.0);
        let (st_miny, miny_bytes) = float_st(-90.0);
        let (st_maxx, maxx_bytes) = float_st(180.0);
        let (st_maxy, maxy_bytes) = float_st(90.0);

        encode_record(&[
            (st_name, &name_bytes),
            (st_srs, &srs_bytes),
            (st_minx, &minx_bytes),
            (st_miny, &miny_bytes),
            (st_maxx, &maxx_bytes),
            (st_maxy, &maxy_bytes),
        ])
    }

    // ── gpkg_tile_matrix row record ────────────────────────────────────────────

    /// Encode a `gpkg_tile_matrix` row.
    ///
    /// Column layout (§2.2.6):
    ///   0: table_name    TEXT
    ///   1: zoom_level    INTEGER (i8)
    ///   2: matrix_width  INTEGER (i16, serial type 2 — values can exceed 255)
    ///   3: matrix_height INTEGER (i16, serial type 2)
    ///   4: tile_width    INTEGER (i16, serial type 2 — standard tile is 256)
    ///   5: tile_height   INTEGER (i16, serial type 2)
    ///   6: pixel_x_size  REAL
    ///   7: pixel_y_size  REAL
    fn tile_matrix_row(
        table_name: &str,
        zoom: u8,
        matrix_w: u16,
        matrix_h: u16,
        pixel_x: f64,
        pixel_y: f64,
    ) -> Vec<u8> {
        let (st_name, name_bytes) = text_st(table_name);
        let (st_zoom, zoom_bytes) = int1_st(zoom);
        // Use serial type 2 (signed 16-bit big-endian) for all integer dimension values.
        let mw_bytes = (matrix_w as i16).to_be_bytes();
        let mh_bytes = (matrix_h as i16).to_be_bytes();
        let tile_w_bytes = 256i16.to_be_bytes();
        let tile_h_bytes = 256i16.to_be_bytes();
        let (st_px, px_bytes) = float_st(pixel_x);
        let (st_py, py_bytes) = float_st(pixel_y);

        encode_record(&[
            (st_name, &name_bytes),
            (st_zoom, &zoom_bytes),
            (2u64, &mw_bytes),     // matrix_width  — serial type 2 (i16)
            (2u64, &mh_bytes),     // matrix_height — serial type 2 (i16)
            (2u64, &tile_w_bytes), // tile_width    — serial type 2 (i16)
            (2u64, &tile_h_bytes), // tile_height   — serial type 2 (i16)
            (st_px, &px_bytes),
            (st_py, &py_bytes),
        ])
    }

    // ── Tile data row record ───────────────────────────────────────────────────

    /// Encode a tile pyramid table row `(zoom_level, tile_column, tile_row, tile_data)`.
    ///
    /// Column layout (§2.2.5):
    ///   0: zoom_level  INTEGER (i8)
    ///   1: tile_column INTEGER (i8)
    ///   2: tile_row    INTEGER (i8)
    ///   3: tile_data   BLOB
    fn tile_row_record(zoom: u8, col: u8, row: u8, data: &[u8]) -> Vec<u8> {
        let (st_zoom, zoom_bytes) = int1_st(zoom);
        let (st_col, col_bytes) = int1_st(col);
        let (st_row, row_bytes) = int1_st(row);
        // Blob serial type: len*2 + 12
        let blob_st = (data.len() as u64) * 2 + 12;

        encode_record(&[
            (st_zoom, &zoom_bytes),
            (st_col, &col_bytes),
            (st_row, &row_bytes),
            (blob_st, data),
        ])
    }

    // ── Test: open returns error for unknown table ─────────────────────────────

    #[test]
    fn test_tile_pyramid_open_returns_error_for_unknown_table() {
        // Build a GeoPackage with gpkg_tile_matrix_set containing one entry for
        // "my_tiles", then try to open "other_tiles" — must return an error.
        let page_size = 4096usize;
        let tms_row = tile_matrix_set_row("my_tiles");
        let tm_row = tile_matrix_row("my_tiles", 0, 1, 1, 0.703125, 0.703125);

        let gpkg_bytes = build_gpkg_with_tables(
            page_size,
            &[
                ("gpkg_tile_matrix_set", vec![(1, tms_row)]),
                ("gpkg_tile_matrix", vec![(1, tm_row)]),
            ],
        );
        let gpkg = GeoPackage::from_bytes(gpkg_bytes).expect("valid gpkg");
        let result = TilePyramidReader::open(&gpkg, "other_tiles");
        assert!(
            result.is_err(),
            "Expected error for unregistered table name"
        );
        let err = result.expect_err("must be an error");
        assert!(
            matches!(err, GpkgError::TileSetNotFound(ref n) if n == "other_tiles"),
            "Expected TileSetNotFound(\"other_tiles\"), got {err:?}"
        );
    }

    // ── Test: zoom_levels() returns sorted ascending list ─────────────────────

    #[test]
    fn test_tile_pyramid_zoom_levels_sorted() {
        // Insert three tile_matrix rows (zoom 5, 2, 8) in that order.
        // zoom_levels() must return [2, 5, 8].
        let page_size = 4096usize;

        let tms_row = tile_matrix_set_row("imagery");
        let tm5 = tile_matrix_row("imagery", 5, 32, 16, 0.02197, 0.02197);
        let tm2 = tile_matrix_row("imagery", 2, 4, 2, 0.17578, 0.17578);
        let tm8 = tile_matrix_row("imagery", 8, 256, 128, 0.00274, 0.00274);

        let gpkg_bytes = build_gpkg_with_tables(
            page_size,
            &[
                ("gpkg_tile_matrix_set", vec![(1, tms_row)]),
                // All three matrix rows in a single gpkg_tile_matrix table page.
                ("gpkg_tile_matrix", vec![(1, tm5), (2, tm2), (3, tm8)]),
            ],
        );
        let gpkg = GeoPackage::from_bytes(gpkg_bytes).expect("valid gpkg");
        let reader = TilePyramidReader::open(&gpkg, "imagery").expect("open ok");
        let zooms = reader.zoom_levels();
        assert_eq!(zooms, vec![2u32, 5u32, 8u32]);
    }

    // ── Test: tile_matrix() accessor ──────────────────────────────────────────

    #[test]
    fn test_tile_pyramid_tile_matrix_accessor() {
        let page_size = 4096usize;

        let tms_row = tile_matrix_set_row("dem");
        let tm5 = tile_matrix_row("dem", 5, 32, 16, 0.02197265625, 0.02197265625);

        let gpkg_bytes = build_gpkg_with_tables(
            page_size,
            &[
                ("gpkg_tile_matrix_set", vec![(1, tms_row)]),
                ("gpkg_tile_matrix", vec![(1, tm5)]),
            ],
        );
        let gpkg = GeoPackage::from_bytes(gpkg_bytes).expect("valid gpkg");
        let reader = TilePyramidReader::open(&gpkg, "dem").expect("open ok");

        let m = reader.tile_matrix(5).expect("zoom 5 matrix must exist");
        assert_eq!(m.zoom_level, 5);
        assert_eq!(m.matrix_width, 32);
        assert_eq!(m.matrix_height, 16);
        assert_eq!(m.tile_width, 256);
        assert_eq!(m.tile_height, 256);
        assert!((m.pixel_x_size - 0.02197265625).abs() < 1e-12);
        assert!((m.pixel_y_size - 0.02197265625).abs() < 1e-12);

        // Non-existent zoom level must return None.
        assert!(reader.tile_matrix(99).is_none());
    }

    // ── Test: get_tile returns None when tile is missing ──────────────────────

    #[test]
    fn test_tile_pyramid_get_tile_returns_none_when_missing() {
        // Build a GeoPackage whose tile table has one tile (0,0,0) but we query (0,1,0).
        let page_size = 4096usize;

        let tms_row = tile_matrix_set_row("tiles");
        let tm0 = tile_matrix_row("tiles", 0, 1, 1, 0.703125, 0.703125);
        let tile_record = tile_row_record(0, 0, 0, &[0xDE, 0xAD, 0xBE, 0xEF]);

        let gpkg_bytes = build_gpkg_with_tables(
            page_size,
            &[
                ("gpkg_tile_matrix_set", vec![(1, tms_row)]),
                ("gpkg_tile_matrix", vec![(1, tm0)]),
                ("tiles", vec![(1, tile_record)]),
            ],
        );
        let gpkg = GeoPackage::from_bytes(gpkg_bytes).expect("valid gpkg");
        let reader = TilePyramidReader::open(&gpkg, "tiles").expect("open ok");

        // Tile (0, 1, 0) does not exist — expect Ok(None)
        let result = reader.get_tile(0, 1, 0).expect("no error expected");
        assert!(result.is_none(), "Expected None for missing tile");
    }

    // ── Test: get_tile returns the blob when tile exists ──────────────────────

    #[test]
    fn test_tile_pyramid_get_tile_returns_blob() {
        let page_size = 4096usize;

        let tms_row = tile_matrix_set_row("tiles");
        let tm0 = tile_matrix_row("tiles", 0, 1, 1, 0.703125, 0.703125);
        let expected_blob: Vec<u8> = vec![0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE];
        let tile_record = tile_row_record(0, 0, 0, &expected_blob);

        let gpkg_bytes = build_gpkg_with_tables(
            page_size,
            &[
                ("gpkg_tile_matrix_set", vec![(1, tms_row)]),
                ("gpkg_tile_matrix", vec![(1, tm0)]),
                ("tiles", vec![(1, tile_record)]),
            ],
        );
        let gpkg = GeoPackage::from_bytes(gpkg_bytes).expect("valid gpkg");
        let reader = TilePyramidReader::open(&gpkg, "tiles").expect("open ok");

        let result = reader.get_tile(0, 0, 0).expect("no error expected");
        assert!(result.is_some(), "Expected Some blob for existing tile");
        assert_eq!(
            result.expect("blob is Some"),
            expected_blob,
            "Tile blob bytes must match"
        );
    }
}
