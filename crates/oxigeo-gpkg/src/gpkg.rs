//! GeoPackage schema layer.
//!
//! Provides typed representations of the core GeoPackage tables defined by the
//! OGC GeoPackage Encoding Standard v1.3.1.

use crate::btree::{self, CellValue, MasterEntry};
use crate::error::GpkgError;
use crate::filter;
use crate::metadata::{GpkgMetadata, GpkgMetadataReference, MetadataScope, ReferenceScope};
use crate::sqlite_reader::SqliteReader;
use crate::vector::types::FieldType;

/// A full-table scan result: one `(rowid, column_values)` tuple per row.
pub type TableScanRows = Vec<(i64, Vec<CellValue>)>;

/// The content type stored in a GeoPackage table.
#[derive(Debug, Clone, PartialEq)]
pub enum GpkgDataType {
    /// OGC Simple Features vector data.
    Features,
    /// Raster tile pyramid (imagery or elevation).
    Tiles,
    /// Non-spatial attribute data.
    Attributes,
}

impl GpkgDataType {
    /// Parse the `data_type` column value from `gpkg_contents`.
    ///
    /// Unknown strings fall back to [`GpkgDataType::Features`].
    pub fn parse_type(s: &str) -> Self {
        match s {
            "features" => Self::Features,
            "tiles" => Self::Tiles,
            "attributes" => Self::Attributes,
            _ => Self::Features,
        }
    }

    /// Return the canonical string used in `gpkg_contents`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Features => "features",
            Self::Tiles => "tiles",
            Self::Attributes => "attributes",
        }
    }
}

/// A row from the `gpkg_contents` table.
#[derive(Debug, Clone)]
pub struct GpkgContents {
    /// Name of the user-data table.
    pub table_name: String,
    /// Logical content type.
    pub data_type: GpkgDataType,
    /// Human-readable identifier (may be the same as `table_name`).
    pub identifier: Option<String>,
    /// Human-readable description.
    pub description: Option<String>,
    /// Bounding box — western longitude.
    pub min_x: f64,
    /// Bounding box — southern latitude.
    pub min_y: f64,
    /// Bounding box — eastern longitude.
    pub max_x: f64,
    /// Bounding box — northern latitude.
    pub max_y: f64,
    /// Spatial reference system ID (references `gpkg_spatial_ref_sys`).
    pub srs_id: i32,
}

/// A row from the `gpkg_geometry_columns` table.
#[derive(Debug, Clone)]
pub struct GpkgGeometryColumn {
    /// User-data table that owns this geometry column.
    pub table_name: String,
    /// Name of the geometry column in `table_name`.
    pub column_name: String,
    /// OGC geometry type name, e.g. `"POINT"`, `"MULTIPOLYGON"`.
    pub geometry_type_name: String,
    /// Spatial reference system ID.
    pub srs_id: i32,
    /// Z coordinate rule: 0 = prohibited, 1 = mandatory, 2 = optional.
    pub z: u8,
    /// M coordinate rule: 0 = prohibited, 1 = mandatory, 2 = optional.
    pub m: u8,
}

/// A row from the `gpkg_spatial_ref_sys` table.
#[derive(Debug, Clone)]
pub struct GpkgSrs {
    /// Human-readable name of the SRS.
    pub srs_name: String,
    /// Numeric SRS identifier (primary key).
    pub srs_id: i32,
    /// Defining organisation (e.g. `"EPSG"`).
    pub organization: String,
    /// Organisation-assigned CRS code.
    pub organization_coordsys_id: i32,
    /// WKT definition of the SRS.
    pub definition: String,
    /// Optional human-readable description.
    pub description: Option<String>,
}

/// A parsed GeoPackage file.
///
/// Wraps the underlying [`SqliteReader`] and exposes GeoPackage-specific
/// metadata discovered from the standard system tables.
pub struct GeoPackage {
    /// Low-level SQLite reader.
    pub reader: SqliteReader,
    /// Rows from `gpkg_contents` populated during construction.
    pub contents: Vec<GpkgContents>,
}

impl GeoPackage {
    /// Open a GeoPackage from its raw file bytes.
    ///
    /// # Errors
    /// Returns an error when the bytes do not represent a valid SQLite file.
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, GpkgError> {
        let reader = SqliteReader::from_bytes(data)?;
        Ok(Self {
            reader,
            contents: Vec::new(),
        })
    }

    /// Open a GeoPackage from a main database file and an optional WAL file.
    pub fn from_files(main: Vec<u8>, wal: Option<Vec<u8>>) -> Result<Self, GpkgError> {
        let data = if let Some(wal_bytes) = wal {
            crate::wal::overlay_wal(&main, &wal_bytes)?
        } else {
            main
        };
        Self::from_bytes(data)
    }

    /// Return `true` when the file appears to be a well-formed GeoPackage.
    ///
    /// Accepts files whose application_id matches `"GPKG"` *or* whose SQLite
    /// structure is valid, to accommodate pre-1.2 files.
    pub fn is_valid_gpkg(&self) -> bool {
        self.reader.header.is_geopackage() || self.reader.is_valid()
    }

    /// Return the page size of the underlying SQLite database.
    pub fn page_size(&self) -> u32 {
        self.reader.header.page_size
    }

    /// Return the total page count of the underlying SQLite database.
    pub fn page_count(&self) -> u32 {
        self.reader.page_count()
    }

    /// Return `true` if the `application_id` field equals the GeoPackage magic.
    pub fn has_gpkg_application_id(&self) -> bool {
        self.reader.header.application_id == 0x4750_4B47
    }

    // ── B-tree / SQLite master scans ────────────────────────────────────────

    /// Scan the `sqlite_master` system table (always rooted at page 1) and
    /// return every entry — tables, indices, views, and triggers.
    ///
    /// # Errors
    /// Returns an error if the underlying SQLite B-tree pages are malformed or
    /// truncated.
    pub fn scan_sqlite_master(&self) -> Result<Vec<MasterEntry>, GpkgError> {
        btree::scan_sqlite_master(
            self.reader.raw_data(),
            self.reader.header.page_size as usize,
        )
    }

    /// Perform a full scan of a table B-tree starting at `root_page`.
    ///
    /// Returns `(rowid, column_values)` for every row, traversing any number of
    /// interior pages to reach leaf pages.
    ///
    /// # Errors
    /// Returns an error if `root_page` is out of range or any B-tree page is
    /// malformed.
    pub fn scan_table(&self, root_page: u32) -> Result<TableScanRows, GpkgError> {
        btree::scan_table(
            self.reader.raw_data(),
            root_page,
            self.reader.header.page_size as usize,
        )
    }

    /// Look up a user table's root page number by name, then scan it.
    ///
    /// Returns `Ok(None)` when no `sqlite_master` entry matches `table_name`.
    ///
    /// # Errors
    /// Returns an error if the `sqlite_master` scan or the subsequent table
    /// scan fails.
    pub fn scan_table_by_name(&self, table_name: &str) -> Result<Option<TableScanRows>, GpkgError> {
        let master = self.scan_sqlite_master()?;
        for entry in master {
            if entry.entry_type == "table" && entry.name == table_name {
                if entry.rootpage == 0 {
                    return Ok(Some(Vec::new()));
                }
                return Ok(Some(self.scan_table(entry.rootpage)?));
            }
        }
        Ok(None)
    }

    /// Look up a user table's root page number by name, scan it, and restore
    /// SQLite's [REAL type affinity](https://www.sqlite.org/datatype3.html#type_affinity)
    /// for columns declared with a REAL-affinity SQL type (`REAL`, `DOUBLE`,
    /// `DOUBLE PRECISION`, `FLOAT`, `NUMERIC`, or `DECIMAL` — the exact set
    /// [`FieldType::from_sql_type`] maps to [`FieldType::Real`]; NUMERIC and
    /// DECIMAL are included here because that classification is shared with
    /// every other affinity-aware consumer in this crate, even though
    /// strict SQLite NUMERIC-affinity semantics would otherwise preserve an
    /// inserted integer as an integer).
    ///
    /// As a storage optimisation, real SQLite writes an integral value
    /// destined for a REAL column using a smaller INTEGER *serial type*
    /// rather than always using the 8-byte float serial type. A raw B-tree
    /// scan therefore returns a column that mixes [`CellValue::Integer`] and
    /// [`CellValue::Float`] cells for what is, semantically, a single REAL
    /// column (e.g. `40` alongside `40.5`, both written to the same column).
    /// [`Self::scan_table_by_name`] returns that mixed, serial-type-literal
    /// result; this method additionally parses the table's declaration from
    /// `sqlite_master.sql` and promotes every `Integer` cell in a
    /// REAL-affinity column to the equivalent `Float`, so both `40` and
    /// `40.5` come back as [`CellValue::Float`].
    ///
    /// The `CREATE TABLE` text is parsed with a best-effort, quote-aware
    /// scanner (quoted/backtick/bracket identifiers, multi-word types,
    /// table-level constraint clauses). If a row's decoded value count does
    /// not match the number of columns parsed from the SQL text — e.g. the
    /// DDL uses a construct the scanner cannot follow — that row's values are
    /// left completely untouched rather than restored against a
    /// misaligned guess.
    ///
    /// This method never changes the behavior of [`Self::scan_table_by_name`]:
    /// the two are independent, and calling this one is purely additive.
    ///
    /// Returns `Ok(None)` when no `sqlite_master` entry matches `table_name`.
    ///
    /// # Errors
    /// Returns an error if the `sqlite_master` scan or the subsequent table
    /// scan fails.
    pub fn scan_table_by_name_typed(
        &self,
        table_name: &str,
    ) -> Result<Option<TableScanRows>, GpkgError> {
        let master = self.scan_sqlite_master()?;
        let Some(entry) = master
            .iter()
            .find(|e| e.entry_type == "table" && e.name == table_name)
        else {
            return Ok(None);
        };

        if entry.rootpage == 0 {
            return Ok(Some(Vec::new()));
        }

        let mut rows = self.scan_table(entry.rootpage)?;
        let declared_types = btree::declared_column_types(&entry.sql);
        restore_real_affinity(&mut rows, &declared_types);
        Ok(Some(rows))
    }

    /// Scan a named table with offset/limit pagination.
    ///
    /// Returns `(rowid, columns)` pairs in B-tree (rowid ascending) order.
    /// Returns `Ok(None)` when the table is not found in `sqlite_master`.
    ///
    /// # Errors
    /// Returns an error if the `sqlite_master` scan or the B-tree traversal fails.
    pub fn scan_table_paginated(
        &self,
        table_name: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Option<TableScanRows>, GpkgError> {
        let master = btree::scan_sqlite_master(
            self.reader.raw_data(),
            self.reader.header.page_size as usize,
        )?;
        for entry in master {
            if entry.entry_type == "table" && entry.name == table_name {
                if entry.rootpage == 0 || limit == 0 {
                    return Ok(Some(Vec::new()));
                }
                let rows = btree::scan_table_paginated(
                    self.reader.raw_data(),
                    entry.rootpage,
                    self.reader.header.page_size as usize,
                    offset,
                    limit,
                )?;
                return Ok(Some(rows));
            }
        }
        Ok(None)
    }

    /// Scan a named table, returning only rows that satisfy a [`filter::FilterExpr`].
    ///
    /// The filter is evaluated against each decoded row after the leaf-page is
    /// parsed; non-matching rows are discarded before being included in the
    /// result.  Rows are returned in B-tree (rowid ascending) order.
    ///
    /// Returns `Ok(None)` when the table is not found in `sqlite_master`.
    ///
    /// # Errors
    /// Returns an error if the `sqlite_master` scan or the B-tree traversal fails.
    pub fn scan_table_filtered(
        &self,
        table_name: &str,
        expr: &filter::FilterExpr,
    ) -> Result<Option<TableScanRows>, GpkgError> {
        let master = btree::scan_sqlite_master(
            self.reader.raw_data(),
            self.reader.header.page_size as usize,
        )?;
        for entry in master {
            if entry.entry_type == "table" && entry.name == table_name {
                if entry.rootpage == 0 {
                    return Ok(Some(Vec::new()));
                }
                let rows = btree::scan_table_filtered(
                    self.reader.raw_data(),
                    entry.rootpage,
                    self.reader.header.page_size as usize,
                    expr,
                )?;
                return Ok(Some(rows));
            }
        }
        Ok(None)
    }

    /// Scan a named table with a filter and post-filter offset/limit pagination.
    ///
    /// Rows are first matched against `expr`, then `offset` matching rows are
    /// skipped, and at most `limit` matching rows are returned.  Both `offset`
    /// and `limit` are relative to the post-filter row stream.
    ///
    /// Returns `Ok(None)` when the table is not found in `sqlite_master`.
    ///
    /// # Errors
    /// Returns an error if the `sqlite_master` scan or the B-tree traversal fails.
    pub fn scan_table_filtered_paginated(
        &self,
        table_name: &str,
        expr: &filter::FilterExpr,
        offset: usize,
        limit: usize,
    ) -> Result<Option<TableScanRows>, GpkgError> {
        let master = btree::scan_sqlite_master(
            self.reader.raw_data(),
            self.reader.header.page_size as usize,
        )?;
        for entry in master {
            if entry.entry_type == "table" && entry.name == table_name {
                if entry.rootpage == 0 || limit == 0 {
                    return Ok(Some(Vec::new()));
                }
                let rows = btree::scan_table_filtered_paginated(
                    self.reader.raw_data(),
                    entry.rootpage,
                    self.reader.header.page_size as usize,
                    expr,
                    offset,
                    limit,
                )?;
                return Ok(Some(rows));
            }
        }
        Ok(None)
    }

    /// Count total rows in a named table.
    ///
    /// Returns `Ok(None)` when the table is not found in `sqlite_master`.
    ///
    /// # Errors
    /// Returns an error if the `sqlite_master` scan or the B-tree traversal fails.
    pub fn count_table_rows(&self, table_name: &str) -> Result<Option<u64>, GpkgError> {
        let master = btree::scan_sqlite_master(
            self.reader.raw_data(),
            self.reader.header.page_size as usize,
        )?;
        for entry in master {
            if entry.entry_type == "table" && entry.name == table_name {
                if entry.rootpage == 0 {
                    return Ok(Some(0));
                }
                let count = btree::count_table_rows(
                    self.reader.raw_data(),
                    entry.rootpage,
                    self.reader.header.page_size as usize,
                )?;
                return Ok(Some(count));
            }
        }
        Ok(None)
    }

    /// Populate `self.contents` by scanning the `gpkg_contents` system table.
    ///
    /// The canonical column layout of `gpkg_contents` is:
    ///
    /// | # | column         | type      |
    /// |---|----------------|-----------|
    /// | 0 | `table_name`   | TEXT      |
    /// | 1 | `data_type`    | TEXT      |
    /// | 2 | `identifier`   | TEXT      |
    /// | 3 | `description`  | TEXT      |
    /// | 4 | `last_change`  | DATETIME  |
    /// | 5 | `min_x`        | REAL      |
    /// | 6 | `min_y`        | REAL      |
    /// | 7 | `max_x`        | REAL      |
    /// | 8 | `max_y`        | REAL      |
    /// | 9 | `srs_id`       | INTEGER   |
    ///
    /// Returns the number of rows loaded.
    ///
    /// # Errors
    /// Returns an error if the `sqlite_master` scan fails or the
    /// `gpkg_contents` table is not present and cannot be scanned.
    pub fn load_contents(&mut self) -> Result<usize, GpkgError> {
        let rows = self
            .scan_table_by_name("gpkg_contents")?
            .unwrap_or_default();

        let mut contents = Vec::with_capacity(rows.len());
        for (_rowid, values) in rows {
            if values.len() < 10 {
                continue; // skip malformed row
            }
            let table_name = cell_to_string(&values[0]);
            let data_type = GpkgDataType::parse_type(&cell_to_string(&values[1]));
            let identifier = cell_to_optional_string(&values[2]);
            let description = cell_to_optional_string(&values[3]);
            // values[4] is last_change (TEXT/datetime) — skipped
            let min_x = cell_to_f64(&values[5]);
            let min_y = cell_to_f64(&values[6]);
            let max_x = cell_to_f64(&values[7]);
            let max_y = cell_to_f64(&values[8]);
            let srs_id = cell_to_i32(&values[9]);

            contents.push(GpkgContents {
                table_name,
                data_type,
                identifier,
                description,
                min_x,
                min_y,
                max_x,
                max_y,
                srs_id,
            });
        }

        let count = contents.len();
        self.contents = contents;
        Ok(count)
    }

    /// Load all rows from the `gpkg_extensions` system table.
    ///
    /// Returns an empty vector when the table is absent.
    pub fn load_extensions(&mut self) -> Result<Vec<crate::extensions::GpkgExtension>, GpkgError> {
        let rows = match self.scan_table_by_name("gpkg_extensions")? {
            Some(r) => r,
            None => return Ok(Vec::new()),
        };

        let mut exts = Vec::with_capacity(rows.len());
        for (_rowid, values) in rows {
            if values.len() < 5 {
                continue;
            }
            let table_name = cell_to_optional_string(&values[0]);
            let column_name = cell_to_optional_string(&values[1]);
            let extension_name = cell_to_string(&values[2]);
            let definition = cell_to_string(&values[3]);
            let scope_str = cell_to_string(&values[4]);
            let scope = scope_str
                .parse::<crate::extensions::ExtensionScope>()
                .unwrap_or(crate::extensions::ExtensionScope::ReadWrite);
            exts.push(crate::extensions::GpkgExtension {
                table_name,
                column_name,
                extension_name,
                definition,
                scope,
            });
        }
        Ok(exts)
    }

    /// Load all rows from the `gpkg_metadata` system table (OGC §10.8, Table 16).
    ///
    /// Returns an empty vector when the `gpkg_metadata` table is absent from
    /// the GeoPackage (the table is optional per the OGC specification).
    ///
    /// Column layout expected in the B-tree (positions are 0-based):
    ///
    /// | # | Column            | SQLite type |
    /// |---|-------------------|-------------|
    /// | 0 | `id`              | INTEGER     |
    /// | 1 | `md_scope`        | TEXT        |
    /// | 2 | `md_standard_uri` | TEXT        |
    /// | 3 | `mime_type`       | TEXT        |
    /// | 4 | `metadata`        | TEXT        |
    ///
    /// Rows with fewer than 5 columns are silently skipped to defend against
    /// schema version mismatches.
    ///
    /// # Errors
    /// Returns an error if the `sqlite_master` scan or the B-tree traversal
    /// of `gpkg_metadata` fails for reasons other than a missing table.
    pub fn load_metadata(&self) -> Result<Vec<GpkgMetadata>, GpkgError> {
        let rows = match self.scan_table_by_name("gpkg_metadata")? {
            Some(r) => r,
            None => return Ok(Vec::new()),
        };

        let mut result = Vec::with_capacity(rows.len());
        for (_rowid, values) in rows {
            if values.len() < 5 {
                // Malformed or schema-version-mismatched row — skip gracefully.
                continue;
            }

            let id = cell_to_i64(&values[0]);
            let md_scope = cell_to_string(&values[1])
                .parse::<MetadataScope>()
                .unwrap_or(MetadataScope::Undefined);
            let md_standard_uri = cell_to_string(&values[2]);
            let mime_type = cell_to_string(&values[3]);
            let metadata = cell_to_string(&values[4]);

            result.push(GpkgMetadata {
                id,
                md_scope,
                md_standard_uri,
                mime_type,
                metadata,
            });
        }

        Ok(result)
    }

    /// Load all rows from `gpkg_relations` (OGC GPKG-RTE §2.4).
    ///
    /// Returns an empty vector when the `gpkg_relations` table is absent from
    /// the GeoPackage (the table is present only when the Related Tables
    /// Extension is in use).
    ///
    /// Column layout expected in the B-tree (positions are 0-based):
    ///
    /// | # | Column                   | SQLite type |
    /// |---|--------------------------|-------------|
    /// | 0 | `id`                     | INTEGER     |
    /// | 1 | `base_table_name`        | TEXT        |
    /// | 2 | `base_primary_column`    | TEXT        |
    /// | 3 | `related_table_name`     | TEXT        |
    /// | 4 | `related_primary_column` | TEXT        |
    /// | 5 | `relation_name`          | TEXT        |
    /// | 6 | `mapping_table_name`     | TEXT        |
    ///
    /// Rows with fewer than 6 columns are silently skipped to defend against
    /// schema version mismatches.
    ///
    /// # Errors
    /// Returns an error if the `sqlite_master` scan or the B-tree traversal
    /// of `gpkg_relations` fails for reasons other than a missing table.
    pub fn load_relations(&self) -> Result<Vec<crate::related_tables::GpkgRelation>, GpkgError> {
        let rows = match self.scan_table_by_name("gpkg_relations")? {
            Some(r) => r,
            None => return Ok(Vec::new()),
        };

        let mut out = Vec::with_capacity(rows.len());
        for (rowid, values) in rows {
            // We need at least 6 columns; with the INTEGER `id` as column 0
            // that means 7 values total.  With only 6 values the `id` column
            // must have been omitted (non-standard) so we fall back to the
            // rowid from the B-tree.
            let has_id_col = values.len() >= 7;

            // Need at least 6 meaningful text columns.
            if values.len() < 6 {
                continue;
            }

            let id = if has_id_col {
                cell_to_i64(&values[0])
            } else {
                rowid
            };
            let base_off: usize = if has_id_col { 1 } else { 0 };

            let base_table_name = cell_to_string(&values[base_off]);
            let base_primary_column = cell_to_string(&values[base_off + 1]);
            let related_table_name = cell_to_string(&values[base_off + 2]);
            let related_primary_column = cell_to_string(&values[base_off + 3]);
            // FromStr for RelationType is infallible (Err = Infallible).
            let relation_name = cell_to_string(&values[base_off + 4])
                .parse::<crate::related_tables::RelationType>()
                .unwrap_or_else(|e| match e {});
            let mapping_table_name = cell_to_string(&values[base_off + 5]);

            out.push(crate::related_tables::GpkgRelation {
                id,
                base_table_name,
                base_primary_column,
                related_table_name,
                related_primary_column,
                relation_name,
                mapping_table_name,
            });
        }
        Ok(out)
    }

    /// Load all rows from a GPKG-RTE mapping table.
    ///
    /// Mapping tables store `(base_id, related_id)` pairs that implement the
    /// many-to-many join described by a [`crate::related_tables::GpkgRelation`].
    /// The table name to pass is the `mapping_table_name` field of the relevant
    /// relation row.
    ///
    /// Returns an empty vector when the named table is absent from the
    /// GeoPackage.
    ///
    /// Expected column layout (0-based):
    ///
    /// | # | Column       | SQLite type |
    /// |---|--------------|-------------|
    /// | 0 | `id`         | INTEGER     |
    /// | 1 | `base_id`    | INTEGER     |
    /// | 2 | `related_id` | INTEGER     |
    ///
    /// When only two columns are found the B-tree rowid is used as the `id`,
    /// and the two columns are treated as `base_id` and `related_id`.
    /// Rows with fewer than 2 columns are silently skipped.
    ///
    /// # Errors
    /// Returns an error if the `sqlite_master` scan or B-tree traversal fails
    /// for reasons other than a missing table.
    pub fn load_mapping_table(
        &self,
        table_name: &str,
    ) -> Result<Vec<crate::related_tables::MappingRow>, GpkgError> {
        let rows = match self.scan_table_by_name(table_name)? {
            Some(r) => r,
            None => return Ok(Vec::new()),
        };

        let mut out = Vec::with_capacity(rows.len());
        for (rowid, values) in rows {
            let (id, base_id, related_id) = if values.len() >= 3 {
                (
                    cell_to_i64(&values[0]),
                    cell_to_i64(&values[1]),
                    cell_to_i64(&values[2]),
                )
            } else if values.len() == 2 {
                (rowid, cell_to_i64(&values[0]), cell_to_i64(&values[1]))
            } else {
                // Fewer than 2 value columns — cannot form a valid mapping row.
                continue;
            };
            out.push(crate::related_tables::MappingRow {
                id,
                base_id,
                related_id,
            });
        }
        Ok(out)
    }

    /// Load all rows from the `gpkg_metadata_reference` system table
    /// (OGC §10.8.5, Table 18).
    ///
    /// Returns an empty vector when the table is absent (the table is optional
    /// per the OGC specification).
    ///
    /// Column layout expected in the B-tree (positions are 0-based):
    ///
    /// | # | Column            | SQLite type |
    /// |---|-------------------|-------------|
    /// | 0 | `reference_scope` | TEXT        |
    /// | 1 | `table_name`      | TEXT NULL   |
    /// | 2 | `column_name`     | TEXT NULL   |
    /// | 3 | `row_id_value`    | INTEGER NULL|
    /// | 4 | `timestamp`       | TEXT        |
    /// | 5 | `md_file_id`      | INTEGER     |
    /// | 6 | `md_parent_id`    | INTEGER NULL|
    ///
    /// Rows with fewer than 7 columns are silently skipped.
    ///
    /// # Errors
    /// Returns an error if the `sqlite_master` scan or the B-tree traversal
    /// of `gpkg_metadata_reference` fails for reasons other than a missing table.
    pub fn load_metadata_references(&self) -> Result<Vec<GpkgMetadataReference>, GpkgError> {
        let rows = match self.scan_table_by_name("gpkg_metadata_reference")? {
            Some(r) => r,
            None => return Ok(Vec::new()),
        };

        let mut result = Vec::with_capacity(rows.len());
        for (_rowid, values) in rows {
            if values.len() < 7 {
                // Malformed or schema-version-mismatched row — skip gracefully.
                continue;
            }

            let reference_scope = cell_to_string(&values[0])
                .parse::<ReferenceScope>()
                .unwrap_or(ReferenceScope::GeoPackage);
            let table_name = cell_to_optional_string(&values[1]);
            let column_name = cell_to_optional_string(&values[2]);
            let row_id_value = cell_to_optional_i64(&values[3]);
            let timestamp = cell_to_string(&values[4]);
            let md_file_id = cell_to_i64(&values[5]);
            let md_parent_id = cell_to_optional_i64(&values[6]);

            result.push(GpkgMetadataReference {
                reference_scope,
                table_name,
                column_name,
                row_id_value,
                timestamp,
                md_file_id,
                md_parent_id,
            });
        }

        Ok(result)
    }
}

// ── Cell-value coercion helpers ─────────────────────────────────────────────

fn cell_to_string(v: &CellValue) -> String {
    match v {
        CellValue::Text(s) => s.clone(),
        CellValue::Integer(i) => i.to_string(),
        CellValue::Float(f) => f.to_string(),
        CellValue::Blob(b) => String::from_utf8_lossy(b).into_owned(),
        CellValue::Null => String::new(),
    }
}

fn cell_to_optional_string(v: &CellValue) -> Option<String> {
    match v {
        CellValue::Null => None,
        CellValue::Text(s) if s.is_empty() => None,
        other => Some(cell_to_string(other)),
    }
}

fn cell_to_f64(v: &CellValue) -> f64 {
    match v {
        CellValue::Float(f) => *f,
        CellValue::Integer(i) => *i as f64,
        _ => 0.0,
    }
}

fn cell_to_i32(v: &CellValue) -> i32 {
    match v {
        CellValue::Integer(i) => {
            if *i > i32::MAX as i64 {
                i32::MAX
            } else if *i < i32::MIN as i64 {
                i32::MIN
            } else {
                *i as i32
            }
        }
        _ => 0,
    }
}

/// Coerce a [`CellValue`] to `i64`, returning 0 for non-integer types.
pub(crate) fn cell_to_i64(v: &CellValue) -> i64 {
    match v {
        CellValue::Integer(i) => *i,
        CellValue::Float(f) => *f as i64,
        _ => 0,
    }
}

/// Coerce a [`CellValue`] to `Option<i64>`, returning `None` for SQL NULL.
pub(crate) fn cell_to_optional_i64(v: &CellValue) -> Option<i64> {
    match v {
        CellValue::Null => None,
        CellValue::Integer(i) => Some(*i),
        CellValue::Float(f) => Some(*f as i64),
        _ => None,
    }
}

/// Promote `Integer` cells to `Float` in columns whose declared SQL type has
/// REAL affinity (see [`GeoPackage::scan_table_by_name_typed`]), leaving
/// every other cell untouched.
///
/// A row whose value count does not match `declared_types` is left entirely
/// unmodified: a length mismatch means the declared-type positions cannot be
/// trusted to line up with this row's values (e.g. the `CREATE TABLE` text
/// used a construct the best-effort DDL scanner did not follow), and
/// restoring against a misaligned guess would silently corrupt an unrelated
/// column rather than simply missing the restoration.
fn restore_real_affinity(rows: &mut TableScanRows, declared_types: &[FieldType]) {
    for (_, values) in rows.iter_mut() {
        if values.len() != declared_types.len() {
            continue;
        }
        for (value, field_type) in values.iter_mut().zip(declared_types.iter()) {
            if *field_type == FieldType::Real
                && let CellValue::Integer(n) = *value
            {
                *value = CellValue::Float(n as f64);
            }
        }
    }
}
