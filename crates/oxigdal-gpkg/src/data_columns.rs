//! GeoPackage `gpkg_data_columns` extension reader.
//!
//! Implements the binding side of the OGC GeoPackage Encoding Standard §F.5
//! Schema extension.  The `gpkg_data_columns` table associates user-data columns
//! with optional human-readable metadata and with constraint names that serve as
//! foreign keys into `gpkg_data_column_constraints`.
//!
//! This module covers:
//!
//! * Parsing the `gpkg_data_columns` rows from a [`GeoPackage`].
//! * Building a [`DataColumnsCatalog`] that provides indexed look-ups by table
//!   name and by constraint name.
//!
//! The spec column layout of `gpkg_data_columns` (0-based B-tree positions) is:
//!
//! | # | column            | SQL type | semantics                     |
//! |---|-------------------|----------|-------------------------------|
//! | 0 | `table_name`      | TEXT     | NOT NULL — user-data table    |
//! | 1 | `column_name`     | TEXT     | NOT NULL — user-data column   |
//! | 2 | `name`            | TEXT     | optional alt-identifier       |
//! | 3 | `title`           | TEXT     | human-readable label          |
//! | 4 | `description`     | TEXT     | free-text annotation          |
//! | 5 | `mime_type`       | TEXT     | RFC 2046 mime, if BLOB col    |
//! | 6 | `constraint_name` | TEXT     | FK to constraint_name         |
//!
//! The relationship between this module and [`crate::schema_constraints`]:
//! `gpkg_data_columns` names the constraint (column 6) while
//! `gpkg_data_column_constraints` defines the constraint rules.

use std::collections::HashMap;

use crate::btree::CellValue;
use crate::error::GpkgError;
use crate::gpkg::GeoPackage;

// ─────────────────────────────────────────────────────────────────────────────
// DataColumn — one parsed row of gpkg_data_columns
// ─────────────────────────────────────────────────────────────────────────────

/// A typed representation of one `gpkg_data_columns` row.
///
/// Field optionality mirrors the SQL nullability of each column: `table_name`
/// and `column_name` are NOT NULL and are always `String`; every other field
/// is optional and maps to `Option<String>` when the stored value is NULL or
/// absent.
#[derive(Debug, Clone, PartialEq)]
pub struct DataColumn {
    /// Name of the user-data table that owns this column entry (NOT NULL).
    pub table_name: String,
    /// Name of the user-data column being annotated (NOT NULL).
    pub column_name: String,
    /// Optional alternative identifier for the column (may differ from
    /// `column_name` and is intended for machine consumption).
    pub name: Option<String>,
    /// Human-readable label for display in user interfaces.
    pub title: Option<String>,
    /// Free-text description or annotation of the column's semantics.
    pub description: Option<String>,
    /// RFC 2046 MIME type for BLOB columns (e.g. `"image/png"`).
    ///
    /// Only meaningful when the user-data column stores binary content;
    /// NULL for text or numeric columns.
    pub mime_type: Option<String>,
    /// Foreign key into `gpkg_data_column_constraints.constraint_name`.
    ///
    /// When set, the values stored in `column_name` of `table_name` are
    /// subject to the corresponding constraint rule(s).
    pub constraint_name: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// DataColumnsCatalog — indexed collection of DataColumn entries
// ─────────────────────────────────────────────────────────────────────────────

/// A catalog of all [`DataColumn`] entries loaded from `gpkg_data_columns`.
///
/// The catalog maintains two secondary indices:
///
/// * `by_table` — maps a table name to the indices of its column entries,
///   enabling efficient look-up of all annotated columns for a given table.
/// * `by_constraint` — maps a constraint name to the indices of all column
///   entries that reference it, enabling the reverse query "which columns use
///   this constraint?".
///
/// Both indices are built at [`Self::load`] time from the flat `entries` list.
/// All look-ups return borrowed slices or iterators into `entries`, so no
/// secondary heap allocation occurs at query time beyond the initial index
/// construction.
#[derive(Debug, Clone)]
pub struct DataColumnsCatalog {
    /// Flat list of all parsed rows, in the order returned by the B-tree scan.
    entries: Vec<DataColumn>,
    /// Secondary index: table name → indices into `entries`.
    by_table: HashMap<String, Vec<usize>>,
    /// Secondary index: constraint name → indices into `entries`.
    by_constraint: HashMap<String, Vec<usize>>,
}

impl DataColumnsCatalog {
    /// Load the `gpkg_data_columns` table from `gpkg` and build the catalog.
    ///
    /// If the table is absent from the GeoPackage (the Schema extension is
    /// optional), the returned catalog is empty — this is **not** an error.
    ///
    /// # Errors
    /// Returns an error only when the underlying SQLite B-tree traversal fails
    /// for a reason other than a missing table (e.g. malformed page data).
    pub fn load(gpkg: &GeoPackage) -> Result<Self, GpkgError> {
        let rows = read_data_columns_rows(gpkg).unwrap_or_default();

        let entry_count = rows.len();
        let mut by_table: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_constraint: HashMap<String, Vec<usize>> = HashMap::new();

        for (idx, entry) in rows.iter().enumerate() {
            by_table
                .entry(entry.table_name.clone())
                .or_default()
                .push(idx);
            if let Some(ref cn) = entry.constraint_name {
                by_constraint.entry(cn.clone()).or_default().push(idx);
            }
        }

        // Pre-size the catalog so the capacity comment in DataColumn is honest.
        let mut catalog = Self {
            entries: Vec::with_capacity(entry_count),
            by_table,
            by_constraint,
        };
        catalog.entries = rows;
        Ok(catalog)
    }

    // ── Basic accessors ───────────────────────────────────────────────────────

    /// Return a slice of all parsed [`DataColumn`] entries.
    #[must_use]
    pub fn entries(&self) -> &[DataColumn] {
        &self.entries
    }

    /// Return the total number of entries in the catalog.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return `true` when no entries are present in the catalog.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over every [`DataColumn`] in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &DataColumn> {
        self.entries.iter()
    }

    // ── Index-backed look-ups ─────────────────────────────────────────────────

    /// Return all [`DataColumn`] entries whose `table_name` matches `table`.
    ///
    /// Returns an empty vector when no annotations exist for the given table.
    /// The returned entries are in the original B-tree scan order (rowid
    /// ascending).
    #[must_use]
    pub fn for_table(&self, table: &str) -> Vec<&DataColumn> {
        self.by_table
            .get(table)
            .map(|indices| indices.iter().map(|&i| &self.entries[i]).collect())
            .unwrap_or_default()
    }

    /// Return the single [`DataColumn`] for (`table`, `column`), if present.
    ///
    /// The OGC spec makes (`table_name`, `column_name`) the primary key of
    /// `gpkg_data_columns`, so at most one entry should exist for any given
    /// pair.  When multiple rows are present due to producer non-conformance,
    /// the first matching entry in scan order is returned.
    ///
    /// Returns `None` when no annotation exists for the given column.
    #[must_use]
    pub fn for_column(&self, table: &str, column: &str) -> Option<&DataColumn> {
        self.for_table(table)
            .into_iter()
            .find(|dc| dc.column_name == column)
    }

    /// Return all [`DataColumn`] entries that reference `constraint_name`.
    ///
    /// This is the inverse direction of `for_table`: it answers the question
    /// "which annotated columns are subject to this constraint?".
    ///
    /// Returns an empty vector when no columns reference the given constraint.
    #[must_use]
    pub fn columns_using_constraint(&self, constraint_name: &str) -> Vec<&DataColumn> {
        self.by_constraint
            .get(constraint_name)
            .map(|indices| indices.iter().map(|&i| &self.entries[i]).collect())
            .unwrap_or_default()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Row reader — parse the gpkg_data_columns B-tree table
// ─────────────────────────────────────────────────────────────────────────────

/// Load every row from the `gpkg_data_columns` table in `gpkg`.
///
/// Returns an empty vector when the table is absent (the Schema extension is
/// optional and not all producers include it).
///
/// Rows with fewer than 7 value cells are silently skipped to defend against
/// schema-version mismatches.  Rows whose `table_name` or `column_name` columns
/// are empty are also skipped: the OGC spec marks both as NOT NULL, so an empty
/// string indicates a malformed or corrupt row.
///
/// # Errors
/// Returns an error if the underlying `sqlite_master` scan or the B-tree
/// traversal fails for reasons other than a missing table.
pub fn read_data_columns_rows(gpkg: &GeoPackage) -> Result<Vec<DataColumn>, GpkgError> {
    let rows = match gpkg.scan_table_by_name("gpkg_data_columns")? {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };

    let mut out = Vec::with_capacity(rows.len());
    for (_rowid, values) in rows {
        if values.len() < 7 {
            // Schema mismatch — skip the row defensively rather than erroring,
            // mirroring the behaviour of `load_data_column_constraints`.
            continue;
        }

        let table_name = cell_to_string(&values[0]);
        if table_name.is_empty() {
            // Per OGC the table_name column is NOT NULL; an empty value
            // indicates a malformed row that we silently ignore.
            continue;
        }

        let column_name = cell_to_string(&values[1]);
        if column_name.is_empty() {
            // Same rationale as table_name above.
            continue;
        }

        let name = cell_to_optional_string(&values[2]);
        let title = cell_to_optional_string(&values[3]);
        let description = cell_to_optional_string(&values[4]);
        let mime_type = cell_to_optional_string(&values[5]);
        let constraint_name = cell_to_optional_string(&values[6]);

        out.push(DataColumn {
            table_name,
            column_name,
            name,
            title,
            description,
            mime_type,
            constraint_name,
        });
    }
    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal cell-value coercion helpers (private to this module)
// ─────────────────────────────────────────────────────────────────────────────

/// Coerce a [`CellValue`] to an owned `String`, producing an empty string for
/// NULL values.  This is used only for NOT-NULL columns where the caller checks
/// for emptiness afterward.
fn cell_to_string(v: &CellValue) -> String {
    match v {
        CellValue::Text(s) => s.clone(),
        CellValue::Integer(i) => i.to_string(),
        CellValue::Float(f) => f.to_string(),
        CellValue::Blob(b) => String::from_utf8_lossy(b).into_owned(),
        CellValue::Null => String::new(),
    }
}

/// Coerce a [`CellValue`] to `Option<String>`, returning `None` for NULL and
/// for explicitly-empty text strings.
fn cell_to_optional_string(v: &CellValue) -> Option<String> {
    match v {
        CellValue::Null => None,
        CellValue::Text(s) if s.is_empty() => None,
        other => Some(cell_to_string(other)),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── DataColumn construction helpers ──────────────────────────────────────

    fn make_entry(table: &str, column: &str, constraint: Option<&str>) -> DataColumn {
        DataColumn {
            table_name: table.to_owned(),
            column_name: column.to_owned(),
            name: None,
            title: None,
            description: None,
            mime_type: None,
            constraint_name: constraint.map(str::to_owned),
        }
    }

    // ── DataColumnsCatalog (no GeoPackage needed for these) ──────────────────

    #[test]
    fn test_catalog_len_and_is_empty_in_memory() {
        let cat = DataColumnsCatalog {
            entries: Vec::new(),
            by_table: HashMap::new(),
            by_constraint: HashMap::new(),
        };
        assert!(cat.is_empty());
        assert_eq!(cat.len(), 0);
    }

    #[test]
    fn test_catalog_for_table_filters_in_memory() {
        let entries = vec![
            make_entry("t1", "c1", None),
            make_entry("t1", "c2", Some("r1")),
            make_entry("t2", "c3", Some("r1")),
        ];
        let mut by_table: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_constraint: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            by_table.entry(e.table_name.clone()).or_default().push(i);
            if let Some(ref cn) = e.constraint_name {
                by_constraint.entry(cn.clone()).or_default().push(i);
            }
        }
        let cat = DataColumnsCatalog {
            entries,
            by_table,
            by_constraint,
        };
        assert_eq!(cat.for_table("t1").len(), 2);
        assert_eq!(cat.for_table("t2").len(), 1);
        assert_eq!(cat.for_table("unknown").len(), 0);
    }

    #[test]
    fn test_catalog_for_column_in_memory() {
        let entries = vec![
            make_entry("t1", "c1", None),
            make_entry("t1", "c2", Some("r1")),
        ];
        let mut by_table: HashMap<String, Vec<usize>> = HashMap::new();
        let by_constraint: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            by_table.entry(e.table_name.clone()).or_default().push(i);
        }
        let cat = DataColumnsCatalog {
            entries,
            by_table,
            by_constraint,
        };
        assert!(cat.for_column("t1", "c1").is_some());
        assert!(cat.for_column("t1", "c2").is_some());
        assert!(cat.for_column("t1", "nonexistent").is_none());
    }

    #[test]
    fn test_catalog_columns_using_constraint_in_memory() {
        let entries = vec![
            make_entry("t1", "c1", Some("size_rule")),
            make_entry("t1", "c2", Some("size_rule")),
            make_entry("t2", "c3", None),
        ];
        let mut by_table: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_constraint: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            by_table.entry(e.table_name.clone()).or_default().push(i);
            if let Some(ref cn) = e.constraint_name {
                by_constraint.entry(cn.clone()).or_default().push(i);
            }
        }
        let cat = DataColumnsCatalog {
            entries,
            by_table,
            by_constraint,
        };
        assert_eq!(cat.columns_using_constraint("size_rule").len(), 2);
        assert_eq!(cat.columns_using_constraint("absent").len(), 0);
    }

    // ── cell_to_string / cell_to_optional_string ─────────────────────────────

    #[test]
    fn test_cell_to_string_variants() {
        assert_eq!(cell_to_string(&CellValue::Text("hello".into())), "hello");
        assert_eq!(cell_to_string(&CellValue::Integer(42)), "42");
        assert_eq!(cell_to_string(&CellValue::Null), "");
        assert_eq!(cell_to_string(&CellValue::Blob(b"hi".to_vec())), "hi");
    }

    #[test]
    fn test_cell_to_optional_string_null_is_none() {
        assert_eq!(cell_to_optional_string(&CellValue::Null), None);
    }

    #[test]
    fn test_cell_to_optional_string_empty_text_is_none() {
        assert_eq!(
            cell_to_optional_string(&CellValue::Text(String::new())),
            None
        );
    }

    #[test]
    fn test_cell_to_optional_string_nonempty_text() {
        assert_eq!(
            cell_to_optional_string(&CellValue::Text("image/png".into())),
            Some("image/png".to_owned())
        );
    }
}
