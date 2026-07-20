//! [`GeoPackageBuilder`] — the top-level GeoPackage writer.
//!
//! Produces a byte-perfect SQLite file that contains the mandatory GeoPackage
//! system tables and zero or more point feature tables.
//!
//! # Limitations
//! Records exceeding `PAGE_SIZE - 35` bytes will return [`GpkgError::RowOverflowsPage`].
//! Interior page chains (for large tables) are not yet supported.
//! Only 2-D point features are accepted; other geometry types require a
//! custom GPB blob (see [`crate::writer::feature_writer::encode_gpkg_point`]).

use crate::error::GpkgError;
use crate::multi_geom::GeometryColumnDef;
use crate::sqlite_writer::{
    PAGE_SIZE,
    btree_writer::write_table,
    file_header::make_gpkg_header,
    page_allocator::PageAllocator,
    record::{RecordValue, encode_record},
};
use crate::writer::{
    feature_writer::{compute_bbox, encode_gpkg_point},
    schema_emitter::{
        DDL_GPKG_CONTENTS, DDL_GPKG_GEOMETRY_COLUMNS, DDL_GPKG_SPATIAL_REF_SYS, ddl_feature_table,
        default_srs_rows,
    },
};

// ─────────────────────────────────────────────────────────────────────────────
// Type alias for the internal feature-table metadata tuple
// ─────────────────────────────────────────────────────────────────────────────

/// Internal representation of a registered feature table after page allocation.
///
/// Fields: `(table_name, geometry_type_name, root_page_num, point_features)`.
type FeatureTableEntry = (String, String, u32, Vec<(i64, f64, f64)>);

// ─────────────────────────────────────────────────────────────────────────────
// FeatureTableSpec
// ─────────────────────────────────────────────────────────────────────────────

/// Specification for a single point feature table.
pub struct FeatureTableSpec {
    /// Name of the user-data table.
    pub name: String,
    /// OGC geometry type name (e.g. `"POINT"`).
    pub geometry_type: String,
    /// Point features: `(fid, x, y)`.
    pub points: Vec<(i64, f64, f64)>,
}

// ─────────────────────────────────────────────────────────────────────────────
// GeoPackageBuilder
// ─────────────────────────────────────────────────────────────────────────────

/// Builder for creating GeoPackage files from scratch.
///
/// Assembles the mandatory system tables (`gpkg_spatial_ref_sys`,
/// `gpkg_contents`, `gpkg_geometry_columns`) and any user-provided feature
/// tables into a single binary buffer that conforms to the SQLite file format
/// and the OGC GeoPackage 1.3.0 specification.
///
/// # Example
/// ```no_run
/// use oxigeo_gpkg::GeoPackageBuilder;
///
/// let bytes = GeoPackageBuilder::new(4326)
///     .add_feature_table("cities", "POINT", vec![(1, 139.7, 35.7), (2, -74.0, 40.7)])
///     .build()
///     .expect("build");
/// ```
pub struct GeoPackageBuilder {
    /// SRS identifier used as the default CRS for the whole file.
    srs_id: i32,
    /// Ordered list of feature tables to write.
    feature_tables: Vec<FeatureTableSpec>,
    /// Additional geometry-column metadata rows to append to
    /// `gpkg_geometry_columns` beyond the primary column of each feature table.
    extra_geometry_columns: Vec<GeometryColumnDef>,
}

impl GeoPackageBuilder {
    /// Create a new builder targeting the given spatial reference system.
    ///
    /// `srs_id` must be one of the three default SRS IDs (`-1`, `0`, `4326`)
    /// or a custom EPSG code added separately.  The builder always writes the
    /// three OGC-mandated default SRS rows regardless.
    pub fn new(srs_id: i32) -> Self {
        Self {
            srs_id,
            feature_tables: Vec::new(),
            extra_geometry_columns: Vec::new(),
        }
    }

    /// Register a point feature table to be included in the GeoPackage.
    ///
    /// * `name` — SQL table name (must be a valid SQLite identifier).
    /// * `geometry_type` — OGC geometry type string stored in
    ///   `gpkg_geometry_columns` (e.g. `"POINT"`).
    /// * `points` — `(fid, x, y)` tuples for each feature.
    pub fn add_feature_table(
        mut self,
        name: impl Into<String>,
        geometry_type: impl Into<String>,
        points: Vec<(i64, f64, f64)>,
    ) -> Self {
        self.feature_tables.push(FeatureTableSpec {
            name: name.into(),
            geometry_type: geometry_type.into(),
            points,
        });
        self
    }

    /// Register a point feature table using a mutable reference.
    ///
    /// Equivalent to [`add_feature_table`] but operates on `&mut self` so that
    /// it can be combined with [`add_geometry_column_def`] in the same builder
    /// chain without transferring ownership.
    ///
    /// [`add_feature_table`]: GeoPackageBuilder::add_feature_table
    /// [`add_geometry_column_def`]: GeoPackageBuilder::add_geometry_column_def
    pub fn add_feature_table_mut(
        &mut self,
        name: impl Into<String>,
        geometry_type: impl Into<String>,
        points: Vec<(i64, f64, f64)>,
    ) -> &mut Self {
        self.feature_tables.push(FeatureTableSpec {
            name: name.into(),
            geometry_type: geometry_type.into(),
            points,
        });
        self
    }

    /// Assemble all pages and return a complete GeoPackage file as raw bytes.
    ///
    /// This is a by-reference variant of [`build`] that clones the internal
    /// state, which allows the builder to be reused after construction.
    ///
    /// [`build`]: GeoPackageBuilder::build
    pub fn build_from_ref(&self) -> Result<Vec<u8>, GpkgError> {
        let cloned = GeoPackageBuilder {
            srs_id: self.srs_id,
            feature_tables: self
                .feature_tables
                .iter()
                .map(|s| FeatureTableSpec {
                    name: s.name.clone(),
                    geometry_type: s.geometry_type.clone(),
                    points: s.points.clone(),
                })
                .collect(),
            extra_geometry_columns: self.extra_geometry_columns.clone(),
        };
        cloned.build()
    }

    /// Register an additional geometry column for an already-declared feature
    /// table.
    ///
    /// Writes a new row to `gpkg_geometry_columns` in addition to the primary
    /// column that is automatically created by [`add_feature_table`].  Use this
    /// to record secondary geometry columns whose SQL column must be part of the
    /// original table DDL (advanced use).
    ///
    /// Returns `GpkgError::TableNotFound` when `table_name` has not been
    /// registered with [`add_feature_table`] yet.
    ///
    /// [`add_feature_table`]: GeoPackageBuilder::add_feature_table
    pub fn add_geometry_column_def(
        &mut self,
        table_name: &str,
        column_def: &GeometryColumnDef,
    ) -> Result<&mut Self, GpkgError> {
        // Validate that the table was declared first.
        let table_known = self.feature_tables.iter().any(|t| t.name == table_name);
        if !table_known {
            return Err(GpkgError::TableNotFound(table_name.to_owned()));
        }

        // Clone the def, ensuring the table_name field matches exactly.
        let mut def = column_def.clone();
        def.table_name = table_name.to_owned();
        self.extra_geometry_columns.push(def);
        Ok(self)
    }

    /// Assemble all pages and return a complete GeoPackage file as raw bytes.
    ///
    /// # Errors
    /// Returns [`GpkgError::RowOverflowsPage`] when any record is too large
    /// to fit on a single 4096-byte leaf page.
    pub fn build(self) -> Result<Vec<u8>, GpkgError> {
        let mut allocator = PageAllocator::new();

        // ── Page 1: sqlite_master (reserve now, write last) ──────────────────
        // Allocate immediately so it gets page number 1.
        let sqlite_master_page = allocator.alloc();
        debug_assert_eq!(sqlite_master_page, 1);

        // ── gpkg_spatial_ref_sys ─────────────────────────────────────────────
        let srs_rows = build_srs_rows();
        let srs_root = write_table(&mut allocator, &srs_rows, 0)?;

        // ── Feature tables ───────────────────────────────────────────────────
        let mut feature_root_pages: Vec<FeatureTableEntry> =
            Vec::with_capacity(self.feature_tables.len());

        for spec in &self.feature_tables {
            let feat_rows = build_feature_rows(&spec.points, self.srs_id);
            let root = write_table(&mut allocator, &feat_rows, 0)?;
            feature_root_pages.push((
                spec.name.clone(),
                spec.geometry_type.clone(),
                root,
                spec.points.clone(),
            ));
        }

        // ── gpkg_contents ────────────────────────────────────────────────────
        let contents_rows = build_contents_rows(&feature_root_pages, self.srs_id);
        let contents_root = write_table(&mut allocator, &contents_rows, 0)?;

        // ── gpkg_geometry_columns ────────────────────────────────────────────
        let geom_col_rows = build_geometry_columns_rows(
            &feature_root_pages,
            self.srs_id,
            &self.extra_geometry_columns,
        );
        let geom_col_root = write_table(&mut allocator, &geom_col_rows, 0)?;

        // ── sqlite_master (page 1, header_offset = 100) ───────────────────────
        let master_rows =
            build_sqlite_master_rows(srs_root, contents_root, geom_col_root, &feature_root_pages);
        // We already allocated page 1; emit it directly and store.
        let master_page = emit_master_page(&master_rows);
        allocator.write(sqlite_master_page, master_page);

        // ── Assemble raw pages ───────────────────────────────────────────────
        let page_count = allocator.page_count();
        let raw_pages = allocator.finalize();

        // ── Prepend the 100-byte SQLite file header ──────────────────────────
        // The file header occupies the first 100 bytes of page 1.
        // We must overwrite those bytes in `raw_pages`.
        let header_bytes = make_gpkg_header(page_count);
        let mut out = Vec::with_capacity(raw_pages.len());
        // First 100 bytes: file header
        out.extend_from_slice(&header_bytes);
        // Bytes 100 .. PAGE_SIZE: remainder of page 1 (after the leaf header + cells)
        out.extend_from_slice(&raw_pages[100..PAGE_SIZE]);
        // Pages 2..N: verbatim
        out.extend_from_slice(&raw_pages[PAGE_SIZE..]);
        Ok(out)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Row-building helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Build encoded rows for `gpkg_spatial_ref_sys`.
///
/// Columns: `srs_name TEXT, srs_id INTEGER, organization TEXT,
///           organization_coordsys_id INTEGER, definition TEXT, description TEXT`.
///
/// Rowids are assigned 1, 2, 3 for the three mandatory rows.
fn build_srs_rows() -> Vec<(i64, Vec<u8>)> {
    default_srs_rows()
        .iter()
        .enumerate()
        .map(|(idx, srs)| {
            let rowid = (idx + 1) as i64;
            let payload = encode_record(&[
                RecordValue::Text(srs.srs_name),
                RecordValue::Int(srs.srs_id),
                RecordValue::Text(srs.organization),
                RecordValue::Int(srs.organization_coordsys_id),
                RecordValue::Text(srs.definition),
                RecordValue::Text(srs.description),
            ]);
            (rowid, payload)
        })
        .collect()
}

/// Build encoded rows for a single feature table (one row per point).
///
/// Columns: `fid INTEGER, geom BLOB`.
fn build_feature_rows(points: &[(i64, f64, f64)], srs_id: i32) -> Vec<(i64, Vec<u8>)> {
    points
        .iter()
        .map(|(fid, x, y)| {
            let gpb = encode_gpkg_point(*x, *y, srs_id);
            let payload = encode_record(&[RecordValue::Int(*fid), RecordValue::Blob(&gpb)]);
            (*fid, payload)
        })
        .collect()
}

/// Build encoded rows for `gpkg_contents`.
///
/// Columns: `table_name TEXT, data_type TEXT, identifier TEXT,
///           description TEXT, last_change DATETIME,
///           min_x DOUBLE, min_y DOUBLE, max_x DOUBLE, max_y DOUBLE,
///           srs_id INTEGER`.
fn build_contents_rows(tables: &[FeatureTableEntry], srs_id: i32) -> Vec<(i64, Vec<u8>)> {
    tables
        .iter()
        .enumerate()
        .map(|(idx, (name, _geom_type, _root, points))| {
            let rowid = (idx + 1) as i64;
            let bbox = compute_bbox(points);
            let last_change = "2026-01-01T00:00:00.000Z";

            let payload = match bbox {
                Some((min_x, min_y, max_x, max_y)) => encode_record(&[
                    RecordValue::Text(name),
                    RecordValue::Text("features"),
                    RecordValue::Text(name), // identifier = table_name
                    RecordValue::Text(""),   // description
                    RecordValue::Text(last_change),
                    RecordValue::Float(min_x),
                    RecordValue::Float(min_y),
                    RecordValue::Float(max_x),
                    RecordValue::Float(max_y),
                    RecordValue::Int(srs_id as i64),
                ]),
                None => encode_record(&[
                    RecordValue::Text(name),
                    RecordValue::Text("features"),
                    RecordValue::Text(name),
                    RecordValue::Text(""),
                    RecordValue::Text(last_change),
                    RecordValue::Null,
                    RecordValue::Null,
                    RecordValue::Null,
                    RecordValue::Null,
                    RecordValue::Int(srs_id as i64),
                ]),
            };
            (rowid, payload)
        })
        .collect()
}

/// Build encoded rows for `gpkg_geometry_columns`.
///
/// Columns: `table_name TEXT, column_name TEXT, geometry_type_name TEXT,
///           srs_id INTEGER, z TINYINT, m TINYINT`.
///
/// Primary geometry columns are derived from `tables` (one per feature table).
/// Additional columns from `extra_cols` are appended in declaration order.
fn build_geometry_columns_rows(
    tables: &[FeatureTableEntry],
    srs_id: i32,
    extra_cols: &[GeometryColumnDef],
) -> Vec<(i64, Vec<u8>)> {
    let mut rows: Vec<(i64, Vec<u8>)> = Vec::with_capacity(tables.len() + extra_cols.len());

    // Primary columns — one per registered feature table.
    for (idx, (name, geom_type, _root, _points)) in tables.iter().enumerate() {
        let rowid = (idx + 1) as i64;
        let payload = encode_record(&[
            RecordValue::Text(name),
            RecordValue::Text("geom"),
            RecordValue::Text(geom_type),
            RecordValue::Int(srs_id as i64),
            RecordValue::Int(0), // z = prohibited
            RecordValue::Int(0), // m = prohibited
        ]);
        rows.push((rowid, payload));
    }

    // Extra columns registered via `add_geometry_column_def`.
    let base_rowid = (tables.len() + 1) as i64;
    for (extra_idx, col) in extra_cols.iter().enumerate() {
        let rowid = base_rowid + extra_idx as i64;
        let payload = encode_record(&[
            RecordValue::Text(&col.table_name),
            RecordValue::Text(&col.column_name),
            RecordValue::Text(&col.geometry_type_name),
            RecordValue::Int(col.srs_id as i64),
            RecordValue::Int(col.z_flag() as i64),
            RecordValue::Int(col.m_flag() as i64),
        ]);
        rows.push((rowid, payload));
    }

    rows
}

/// Internal representation of a sqlite_master row prior to encoding.
///
/// Fields: `(rowid, _unused_bytes, table_name, ddl_sql, root_page)`.
type MasterRowSpec = (i64, Vec<u8>, String, String, u32);

/// Build encoded rows for `sqlite_master`.
///
/// Columns: `type TEXT, name TEXT, tbl_name TEXT, rootpage INTEGER, sql TEXT`.
///
/// Order: gpkg_spatial_ref_sys, gpkg_contents, gpkg_geometry_columns,
///        then each feature table.
fn build_sqlite_master_rows(
    srs_root: u32,
    contents_root: u32,
    geom_col_root: u32,
    feature_tables: &[FeatureTableEntry],
) -> Vec<MasterRowSpec> {
    let mut rows: Vec<MasterRowSpec> = Vec::new();

    let mut rowid = 1i64;

    // gpkg_spatial_ref_sys
    rows.push((
        rowid,
        vec![],
        "gpkg_spatial_ref_sys".to_string(),
        DDL_GPKG_SPATIAL_REF_SYS.to_string(),
        srs_root,
    ));
    rowid += 1;

    // gpkg_contents
    rows.push((
        rowid,
        vec![],
        "gpkg_contents".to_string(),
        DDL_GPKG_CONTENTS.to_string(),
        contents_root,
    ));
    rowid += 1;

    // gpkg_geometry_columns
    rows.push((
        rowid,
        vec![],
        "gpkg_geometry_columns".to_string(),
        DDL_GPKG_GEOMETRY_COLUMNS.to_string(),
        geom_col_root,
    ));
    rowid += 1;

    // Feature tables
    for (name, _geom_type, root, _points) in feature_tables {
        let ddl = ddl_feature_table(name);
        rows.push((rowid, vec![], name.clone(), ddl, *root));
        rowid += 1;
    }

    rows
}

/// Emit the sqlite_master leaf page (page 1, header_offset = 100).
fn emit_master_page(rows: &[MasterRowSpec]) -> Vec<u8> {
    use crate::sqlite_writer::btree_writer::LeafPageBuilder;

    let mut builder = LeafPageBuilder::new();
    for (rowid, _unused, name, sql, rootpage) in rows {
        let payload = encode_record(&[
            RecordValue::Text("table"),
            RecordValue::Text(name),
            RecordValue::Text(name),
            RecordValue::Int(*rootpage as i64),
            RecordValue::Text(sql),
        ]);
        // Page 1 has header_offset = 100; try_add uses worst-case internally.
        let ok = builder.try_add(*rowid, payload);
        debug_assert!(ok, "sqlite_master row {rowid} did not fit on page 1");
    }
    builder.emit(100)
}
