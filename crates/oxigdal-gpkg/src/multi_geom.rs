//! Multiple-geometry-column support for GeoPackage feature tables.
//!
//! The OGC GeoPackage specification allows a feature table to expose more than
//! one geometry column, each registered as a separate row in
//! `gpkg_geometry_columns`.  This module provides types and functions to
//! discover, inspect, and (via the writer extension) register those additional
//! columns without touching the existing single-column fast path.

use crate::error::GpkgError;
use crate::gpkg::GeoPackage;

// ─────────────────────────────────────────────────────────────────────────────
// GeometryColumnDef
// ─────────────────────────────────────────────────────────────────────────────

/// Describes a single geometry column registered in `gpkg_geometry_columns`.
///
/// This mirrors one row of that system table, with the z/m flags decoded into
/// `bool` fields for ergonomic use.
#[derive(Debug, Clone, PartialEq)]
pub struct GeometryColumnDef {
    /// Name of the feature table that owns this geometry column.
    pub table_name: String,
    /// Name of the column inside that feature table (e.g. `"geom"`).
    pub column_name: String,
    /// OGC geometry type name stored in `gpkg_geometry_columns`
    /// (e.g. `"POINT"`, `"MULTIPOLYGON"`).
    pub geometry_type_name: String,
    /// Spatial reference system identifier.
    pub srs_id: i32,
    /// `true` when the z coordinate rule is 1 (mandatory) or 2 (optional).
    pub has_z: bool,
    /// `true` when the m coordinate rule is 1 (mandatory) or 2 (optional).
    pub has_m: bool,
}

impl GeometryColumnDef {
    /// Construct a new [`GeometryColumnDef`] from raw field values.
    ///
    /// `z_flag` and `m_flag` are the raw integer values from the
    /// `gpkg_geometry_columns` table: 0 = prohibited, 1 = mandatory, 2 = optional.
    pub fn from_raw(
        table_name: impl Into<String>,
        column_name: impl Into<String>,
        geometry_type_name: impl Into<String>,
        srs_id: i32,
        z_flag: u8,
        m_flag: u8,
    ) -> Self {
        Self {
            table_name: table_name.into(),
            column_name: column_name.into(),
            geometry_type_name: geometry_type_name.into(),
            srs_id,
            has_z: z_flag > 0,
            has_m: m_flag > 0,
        }
    }

    /// Return the raw z flag value suitable for writing to `gpkg_geometry_columns`.
    ///
    /// Maps `has_z = true` → 2 (optional) and `has_z = false` → 0 (prohibited).
    pub fn z_flag(&self) -> u8 {
        if self.has_z { 2 } else { 0 }
    }

    /// Return the raw m flag value suitable for writing to `gpkg_geometry_columns`.
    ///
    /// Maps `has_m = true` → 2 (optional) and `has_m = false` → 0 (prohibited).
    pub fn m_flag(&self) -> u8 {
        if self.has_m { 2 } else { 0 }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MultiGeomColumnSet
// ─────────────────────────────────────────────────────────────────────────────

/// All geometry columns registered for a single feature table.
///
/// The first element of `columns` is treated as the *primary* geometry column
/// (the one used by readers that only support a single geometry column per
/// table).  Additional elements are secondary columns.
#[derive(Debug, Clone)]
pub struct MultiGeomColumnSet {
    /// Name of the feature table (matches `gpkg_contents.table_name`).
    pub table_name: String,
    /// Ordered list of geometry columns; the first is the primary column.
    pub columns: Vec<GeometryColumnDef>,
}

impl MultiGeomColumnSet {
    /// Create a new, empty column set for `table_name`.
    pub fn new(table_name: impl Into<String>) -> Self {
        Self {
            table_name: table_name.into(),
            columns: Vec::new(),
        }
    }

    /// Return a reference to the primary (first) geometry column, or `None`
    /// when no columns have been registered yet.
    pub fn primary(&self) -> Option<&GeometryColumnDef> {
        self.columns.first()
    }

    /// Search for a geometry column by its column name.
    ///
    /// Returns `None` when no column with that name exists in this set.
    pub fn find_by_name(&self, column_name: &str) -> Option<&GeometryColumnDef> {
        self.columns.iter().find(|c| c.column_name == column_name)
    }

    /// Return the number of registered geometry columns.
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// Return `true` when this table has more than one geometry column.
    pub fn has_multiple(&self) -> bool {
        self.columns.len() > 1
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Load all geometry columns registered for `table_name` in this GeoPackage.
///
/// Scans `gpkg_geometry_columns` and returns a [`MultiGeomColumnSet`]
/// containing every row whose `table_name` column matches the requested name.
///
/// Returns `Ok(None)` when no matching rows are found (the table either does
/// not exist in `gpkg_contents` or has no geometry column registered).
///
/// # Errors
/// Propagates any SQLite B-tree scan errors.
pub fn load_geometry_columns_for_table(
    reader: &GeoPackage,
    table_name: &str,
) -> Result<Option<MultiGeomColumnSet>, GpkgError> {
    let all_rows = match reader.scan_table_by_name("gpkg_geometry_columns")? {
        Some(r) => r,
        None => return Ok(None),
    };

    let mut column_set = MultiGeomColumnSet::new(table_name);

    for (_rowid, values) in &all_rows {
        if values.len() < 6 {
            continue; // malformed row — skip
        }

        let row_table_name = cell_to_string(&values[0]);
        if row_table_name != table_name {
            continue;
        }

        let def = decode_geometry_column_row(values);
        column_set.columns.push(def);
    }

    if column_set.columns.is_empty() {
        Ok(None)
    } else {
        Ok(Some(column_set))
    }
}

/// Load geometry columns for all feature tables present in the GeoPackage.
///
/// Returns one [`MultiGeomColumnSet`] per distinct `table_name` value found
/// in `gpkg_geometry_columns`.  Tables with no registered geometry column are
/// omitted.  The order of the returned vec follows the B-tree row order of
/// `gpkg_geometry_columns` (rowid ascending).
///
/// # Errors
/// Propagates any SQLite B-tree scan errors.
pub fn load_all_geometry_columns(
    reader: &GeoPackage,
) -> Result<Vec<MultiGeomColumnSet>, GpkgError> {
    let all_rows = match reader.scan_table_by_name("gpkg_geometry_columns")? {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };

    // Preserve insertion order while deduplicating by table name.
    let mut table_order: Vec<String> = Vec::new();
    let mut table_map: std::collections::HashMap<String, MultiGeomColumnSet> =
        std::collections::HashMap::new();

    for (_rowid, values) in &all_rows {
        if values.len() < 6 {
            continue;
        }

        let row_table_name = cell_to_string(&values[0]);
        let def = decode_geometry_column_row(values);

        if let Some(set) = table_map.get_mut(&row_table_name) {
            set.columns.push(def);
        } else {
            table_order.push(row_table_name.clone());
            let mut set = MultiGeomColumnSet::new(&row_table_name);
            set.columns.push(def);
            table_map.insert(row_table_name, set);
        }
    }

    Ok(table_order
        .into_iter()
        .filter_map(|name| table_map.remove(&name))
        .collect())
}

/// Return `true` when the named feature table has more than one geometry column
/// registered in `gpkg_geometry_columns`.
///
/// Returns `false` for tables with exactly one geometry column or for tables
/// that are not found in `gpkg_geometry_columns`.
///
/// # Errors
/// Propagates any SQLite B-tree scan errors.
pub fn has_multiple_geometry_columns(
    reader: &GeoPackage,
    table_name: &str,
) -> Result<bool, GpkgError> {
    match load_geometry_columns_for_table(reader, table_name)? {
        Some(set) => Ok(set.has_multiple()),
        None => Ok(false),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Private helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Decode one row from `gpkg_geometry_columns` into a [`GeometryColumnDef`].
///
/// Expected column layout (0-based):
///
/// | # | column               | type    |
/// |---|----------------------|---------|
/// | 0 | `table_name`         | TEXT    |
/// | 1 | `column_name`        | TEXT    |
/// | 2 | `geometry_type_name` | TEXT    |
/// | 3 | `srs_id`             | INTEGER |
/// | 4 | `z`                  | INTEGER |
/// | 5 | `m`                  | INTEGER |
///
/// Caller must ensure `values.len() >= 6` before calling this function.
fn decode_geometry_column_row(values: &[crate::btree::CellValue]) -> GeometryColumnDef {
    let table_name = cell_to_string(&values[0]);
    let column_name = cell_to_string(&values[1]);
    let geometry_type_name = cell_to_string(&values[2]);
    let srs_id = cell_to_i32(&values[3]);
    let z_flag = cell_to_u8(&values[4]);
    let m_flag = cell_to_u8(&values[5]);

    GeometryColumnDef::from_raw(
        table_name,
        column_name,
        geometry_type_name,
        srs_id,
        z_flag,
        m_flag,
    )
}

// ── Cell-value coercion helpers (local copies to avoid exposing pub(crate)) ──

fn cell_to_string(v: &crate::btree::CellValue) -> String {
    use crate::btree::CellValue;
    match v {
        CellValue::Text(s) => s.clone(),
        CellValue::Integer(i) => i.to_string(),
        CellValue::Float(f) => f.to_string(),
        CellValue::Blob(b) => String::from_utf8_lossy(b).into_owned(),
        CellValue::Null => String::new(),
    }
}

fn cell_to_i32(v: &crate::btree::CellValue) -> i32 {
    use crate::btree::CellValue;
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
        CellValue::Float(f) => *f as i32,
        _ => 0,
    }
}

fn cell_to_u8(v: &crate::btree::CellValue) -> u8 {
    use crate::btree::CellValue;
    match v {
        CellValue::Integer(i) => {
            if *i < 0 {
                0
            } else if *i > u8::MAX as i64 {
                u8::MAX
            } else {
                *i as u8
            }
        }
        CellValue::Float(f) => {
            let i = *f as i64;
            if i < 0 {
                0
            } else if i > u8::MAX as i64 {
                u8::MAX
            } else {
                i as u8
            }
        }
        _ => 0,
    }
}
