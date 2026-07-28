//! [`GeoPackageBuilder`] — the top-level GeoPackage writer.
//!
//! Produces a byte-perfect SQLite file that contains the mandatory GeoPackage
//! system tables and zero or more point feature tables, optionally with a
//! real R-tree spatial index (see [`GeoPackageBuilder::add_rtree_index`]).
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
    rtree_writer::{self, DDL_GPKG_EXTENSIONS, RTREE_EXTENSION_DEFINITION, RTreeIndexEntry},
    schema_emitter::{
        CustomSrs, DDL_GPKG_CONTENTS, DDL_GPKG_GEOMETRY_COLUMNS, DDL_GPKG_SPATIAL_REF_SYS,
        MANDATORY_SRS_IDS, ddl_feature_table_with_extra_columns, default_srs_rows,
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
    /// Custom (non-default) SRS rows registered via [`Self::add_custom_srs`].
    custom_srs: Vec<CustomSrs>,
    /// Names of feature tables for which [`Self::build`] must emit a real
    /// R-tree spatial index (`gpkg_rtree_index` extension) over the primary
    /// `geom` column, registered via [`Self::add_rtree_index`].
    rtree_indexes: Vec<String>,
}

impl GeoPackageBuilder {
    /// Create a new builder targeting the given spatial reference system.
    ///
    /// `srs_id` must be one of the three default SRS IDs (`-1`, `0`, `4326`)
    /// or a custom EPSG code registered via [`Self::add_custom_srs`] before
    /// calling [`Self::build`] — otherwise `build()` returns
    /// [`GpkgError::UnknownSrsId`]. The builder always writes the three
    /// OGC-mandated default SRS rows regardless.
    pub fn new(srs_id: i32) -> Self {
        Self {
            srs_id,
            feature_tables: Vec::new(),
            extra_geometry_columns: Vec::new(),
            custom_srs: Vec::new(),
            rtree_indexes: Vec::new(),
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
            custom_srs: self.custom_srs.clone(),
            rtree_indexes: self.rtree_indexes.clone(),
        };
        cloned.build()
    }

    /// Register a real R-tree spatial index (`gpkg_rtree_index` extension,
    /// OGC 12-128r19 Annex F) over the primary `geom` column of an
    /// already-declared feature table.
    ///
    /// [`Self::build`] emits the three SQLite `rtree` module shadow tables
    /// (`rtree_<table>_geom_node` / `_rowid` / `_parent`), the virtual-table
    /// entry itself, and a `gpkg_extensions` row — all readable by both
    /// [`crate::rtree::GpkgRTreeReader`] and a real SQLite `rtree` module.
    ///
    /// Calling this twice for the same table is idempotent (the second call
    /// is a no-op) rather than producing a duplicate shadow-table set.
    ///
    /// # Errors
    /// Returns [`GpkgError::TableNotFound`] when `table_name` has not been
    /// registered with [`Self::add_feature_table`].
    pub fn add_rtree_index(&mut self, table_name: &str) -> Result<&mut Self, GpkgError> {
        let table_known = self.feature_tables.iter().any(|t| t.name == table_name);
        if !table_known {
            return Err(GpkgError::TableNotFound(table_name.to_owned()));
        }
        if !self.rtree_indexes.iter().any(|n| n == table_name) {
            self.rtree_indexes.push(table_name.to_owned());
        }
        Ok(self)
    }

    /// Register a custom (non-default) spatial reference system.
    ///
    /// Appends a row to the `gpkg_spatial_ref_sys` table emitted by
    /// [`Self::build`], allowing `srs_id` values other than the three
    /// OGC-mandated defaults (`-1`, `0`, `4326`) to be used as the builder's
    /// [`Self::new`] `srs_id` (or as a [`crate::multi_geom::GeometryColumnDef`]
    /// srs_id) without producing a dangling foreign-key reference.
    ///
    /// # Errors
    /// Returns [`GpkgError::DuplicateSrsId`] when `srs.srs_id` collides with
    /// one of the three mandatory default SRS ids or with a previously
    /// registered custom SRS id.
    pub fn add_custom_srs(&mut self, srs: CustomSrs) -> Result<&mut Self, GpkgError> {
        if MANDATORY_SRS_IDS.contains(&srs.srs_id) {
            return Err(GpkgError::DuplicateSrsId(srs.srs_id));
        }
        if self.custom_srs.iter().any(|s| s.srs_id == srs.srs_id) {
            return Err(GpkgError::DuplicateSrsId(srs.srs_id));
        }
        self.custom_srs.push(srs);
        Ok(self)
    }

    /// Register an additional geometry column for an already-declared feature
    /// table.
    ///
    /// Writes a new row to `gpkg_geometry_columns` in addition to the primary
    /// column that is automatically created by [`add_feature_table`], **and**
    /// [`Self::build`] extends that table's own `CREATE TABLE` DDL (as stored
    /// in `sqlite_master.sql`) to declare a matching `BLOB` column of the same
    /// name — so the emitted GeoPackage never has `gpkg_geometry_columns`
    /// metadata that outruns the table's actual schema.
    ///
    /// Every row in the table stores `NULL` for this extra column: there is
    /// currently no per-row value API for secondary geometry columns (only
    /// the primary `geom` column set via [`add_feature_table`] carries real
    /// point data). Populate the column's values afterward with any
    /// SQLite-compatible `UPDATE` if needed.
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
    /// * [`GpkgError::UnknownSrsId`] — this builder's `srs_id` (from
    ///   [`Self::new`]) is neither a default SRS id nor a custom SRS
    ///   registered via [`Self::add_custom_srs`].
    /// * [`GpkgError::RowOverflowsPage`] — a record is too large to fit on a
    ///   single 4096-byte leaf page.
    pub fn build(self) -> Result<Vec<u8>, GpkgError> {
        self.validate_srs_id()?;

        let mut allocator = PageAllocator::new();

        // ── Page 1: sqlite_master (reserve now, write last) ──────────────────
        // Allocate immediately so it gets page number 1.
        let sqlite_master_page = allocator.alloc();
        debug_assert_eq!(sqlite_master_page, 1);

        // ── gpkg_spatial_ref_sys ─────────────────────────────────────────────
        let srs_rows = build_srs_rows(&self.custom_srs);
        let srs_root = write_table(&mut allocator, &srs_rows, 0)?;

        // ── Feature tables ───────────────────────────────────────────────────
        let mut feature_root_pages: Vec<FeatureTableEntry> =
            Vec::with_capacity(self.feature_tables.len());

        for spec in &self.feature_tables {
            let extra_for_table: Vec<&GeometryColumnDef> = self
                .extra_geometry_columns
                .iter()
                .filter(|c| c.table_name == spec.name)
                .collect();
            let feat_rows = build_feature_rows(&spec.points, self.srs_id, extra_for_table.len());
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

        // ── R-tree spatial indexes (gpkg_rtree_index extension) ──────────────
        // Opt-in per Self::add_rtree_index; emits real shadow tables plus a
        // gpkg_extensions registration row for each requested table.
        let mut rtree_master_rows: Vec<MasterRowSpec> = Vec::new();
        let mut extension_rows: Vec<(String, String)> = Vec::new(); // (table_name, column_name)

        for table_name in &self.rtree_indexes {
            let (_, _, _, points) = feature_root_pages
                .iter()
                .find(|(name, ..)| name == table_name)
                .ok_or_else(|| GpkgError::TableNotFound(table_name.clone()))?;

            let entries: Vec<RTreeIndexEntry> = points
                .iter()
                .map(|&(fid, x, y)| (fid, x as f32, x as f32, y as f32, y as f32))
                .collect();

            let roots = rtree_writer::build_rtree_index(&mut allocator, &entries)?;

            let vt_name = rtree_writer::virtual_table_name(table_name, "geom");
            rtree_master_rows.push((
                0, // rowid: assigned below in sequence
                vec![],
                vt_name.clone(),
                rtree_writer::ddl_rtree_virtual_table(table_name, "geom"),
                0, // virtual tables have rootpage = 0
            ));
            rtree_master_rows.push((
                0,
                vec![],
                rtree_writer::node_table_name(table_name, "geom"),
                rtree_writer::ddl_rtree_node_table(table_name, "geom"),
                roots.node_root,
            ));
            rtree_master_rows.push((
                0,
                vec![],
                rtree_writer::rowid_table_name(table_name, "geom"),
                rtree_writer::ddl_rtree_rowid_table(table_name, "geom"),
                roots.rowid_root,
            ));
            rtree_master_rows.push((
                0,
                vec![],
                rtree_writer::parent_table_name(table_name, "geom"),
                rtree_writer::ddl_rtree_parent_table(table_name, "geom"),
                roots.parent_root,
            ));

            extension_rows.push((table_name.clone(), "geom".to_string()));
        }

        // ── gpkg_extensions (only emitted when at least one extension is used) ─
        let mut extensions_master_row: Option<MasterRowSpec> = None;
        if !extension_rows.is_empty() {
            let rows: Vec<(i64, Vec<u8>)> = extension_rows
                .iter()
                .enumerate()
                .map(|(idx, (table_name, column_name))| {
                    let rowid = (idx + 1) as i64;
                    let payload = encode_record(&[
                        RecordValue::Text(table_name),
                        RecordValue::Text(column_name),
                        RecordValue::Text("gpkg_rtree_index"),
                        RecordValue::Text(RTREE_EXTENSION_DEFINITION),
                        RecordValue::Text("write-only"),
                    ]);
                    (rowid, payload)
                })
                .collect();
            let extensions_root = write_table(&mut allocator, &rows, 0)?;
            extensions_master_row = Some((
                0,
                vec![],
                "gpkg_extensions".to_string(),
                DDL_GPKG_EXTENSIONS.to_string(),
                extensions_root,
            ));
        }

        // ── sqlite_master (page 1, header_offset = 100) ───────────────────────
        let mut master_rows = build_sqlite_master_rows(
            srs_root,
            contents_root,
            geom_col_root,
            &feature_root_pages,
            &self.extra_geometry_columns,
        );
        if let Some(row) = extensions_master_row {
            master_rows.push(row);
        }
        master_rows.extend(rtree_master_rows);
        // Assign final sequential rowids to every row (base rows already have
        // correct ones from build_sqlite_master_rows; extension/rtree rows
        // were pushed with a placeholder 0 above).
        let mut next_rowid = master_rows.iter().map(|r| r.0).max().unwrap_or(0) + 1;
        for row in &mut master_rows {
            if row.0 == 0 {
                row.0 = next_rowid;
                next_rowid += 1;
            }
        }
        // We already allocated page 1; emit it directly and store.
        let master_page = emit_master_page(&master_rows)?;
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

    /// Confirm `self.srs_id` refers to a row that will actually be present in
    /// the emitted `gpkg_spatial_ref_sys` table: one of the three mandatory
    /// default ids, or a custom SRS registered via [`Self::add_custom_srs`].
    fn validate_srs_id(&self) -> Result<(), GpkgError> {
        let is_default = MANDATORY_SRS_IDS.contains(&self.srs_id);
        let is_custom = self.custom_srs.iter().any(|s| s.srs_id == self.srs_id);
        if is_default || is_custom {
            Ok(())
        } else {
            Err(GpkgError::UnknownSrsId(self.srs_id))
        }
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
/// Rowids are assigned 1, 2, 3 for the three mandatory rows, followed by one
/// row per entry in `custom` (in registration order).
fn build_srs_rows(custom: &[CustomSrs]) -> Vec<(i64, Vec<u8>)> {
    let mut rows: Vec<(i64, Vec<u8>)> = default_srs_rows()
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
        .collect();

    let base_rowid = rows.len() as i64 + 1;
    for (idx, srs) in custom.iter().enumerate() {
        let rowid = base_rowid + idx as i64;
        let payload = encode_record(&[
            RecordValue::Text(&srs.srs_name),
            RecordValue::Int(i64::from(srs.srs_id)),
            RecordValue::Text(&srs.organization),
            RecordValue::Int(srs.organization_coordsys_id),
            RecordValue::Text(&srs.definition),
            RecordValue::Text(&srs.description),
        ]);
        rows.push((rowid, payload));
    }

    rows
}

/// Build encoded rows for a single feature table (one row per point).
///
/// Columns: `fid INTEGER, geom BLOB`, followed by `extra_column_count` extra
/// `BLOB` columns (declared in the table's DDL by
/// [`crate::writer::schema_emitter::ddl_feature_table_with_extra_columns`] to
/// keep `gpkg_geometry_columns` metadata consistent with the actual schema).
/// Extra columns have no per-row value API yet, so every row writes them as
/// `NULL` — a real absent-value marker, not a fabricated one.
fn build_feature_rows(
    points: &[(i64, f64, f64)],
    srs_id: i32,
    extra_column_count: usize,
) -> Vec<(i64, Vec<u8>)> {
    points
        .iter()
        .map(|(fid, x, y)| {
            let gpb = encode_gpkg_point(*x, *y, srs_id);
            let mut cols = vec![RecordValue::Int(*fid), RecordValue::Blob(&gpb)];
            for _ in 0..extra_column_count {
                cols.push(RecordValue::Null);
            }
            let payload = encode_record(&cols);
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
    extra_geometry_columns: &[GeometryColumnDef],
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

    // Feature tables — DDL includes any extra geometry columns registered for
    // this table so gpkg_geometry_columns metadata never outruns the actual
    // schema (see ddl_feature_table_with_extra_columns).
    for (name, _geom_type, root, _points) in feature_tables {
        let extra_names: Vec<&str> = extra_geometry_columns
            .iter()
            .filter(|c| &c.table_name == name)
            .map(|c| c.column_name.as_str())
            .collect();
        let ddl = ddl_feature_table_with_extra_columns(name, &extra_names);
        rows.push((rowid, vec![], name.clone(), ddl, *root));
        rowid += 1;
    }

    rows
}

/// Emit the sqlite_master leaf page (page 1, header_offset = 100).
///
/// # Errors
/// Returns [`GpkgError::RowOverflowsPage`] when any individual
/// `sqlite_master` row (table catalog entry) is too large to fit on page 1,
/// or when the accumulated set of catalog rows overflows the single 4096-byte
/// page — the same real error every other system/feature table page write
/// propagates via [`write_table`]. This used to be a `debug_assert!` that
/// compiled out in release builds, silently dropping the row instead.
fn emit_master_page(rows: &[MasterRowSpec]) -> Result<Vec<u8>, GpkgError> {
    use crate::sqlite_writer::btree_writer::{LeafPageBuilder, MAX_PAYLOAD_INLINE};

    let mut builder = LeafPageBuilder::new();
    for (rowid, _unused, name, sql, rootpage) in rows {
        let payload = encode_record(&[
            RecordValue::Text("table"),
            RecordValue::Text(name),
            RecordValue::Text(name),
            RecordValue::Int(*rootpage as i64),
            RecordValue::Text(sql),
        ]);
        if payload.len() > MAX_PAYLOAD_INLINE {
            return Err(GpkgError::RowOverflowsPage {
                size: payload.len(),
                max: MAX_PAYLOAD_INLINE,
            });
        }
        // Page 1 has header_offset = 100; try_add uses worst-case internally.
        let payload_len = payload.len();
        if !builder.try_add(*rowid, payload) {
            return Err(GpkgError::RowOverflowsPage {
                size: payload_len,
                max: MAX_PAYLOAD_INLINE,
            });
        }
    }
    Ok(builder.emit(100))
}
