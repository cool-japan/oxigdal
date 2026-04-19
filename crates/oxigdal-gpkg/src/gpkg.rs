//! GeoPackage schema layer.
//!
//! Provides typed representations of the core GeoPackage tables defined by the
//! OGC GeoPackage Encoding Standard v1.3.1.

use crate::btree::{self, CellValue, MasterEntry};
use crate::error::GpkgError;
use crate::sqlite_reader::SqliteReader;

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
