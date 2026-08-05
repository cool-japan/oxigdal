//! Shared GeoPackage table-schema helpers.
//!
//! The GeoPackage driver ([`oxigeo_gpkg`]) exposes a row scanner that returns
//! `(rowid, Vec<CellValue>)` tuples — positional values with no column names
//! attached.  Turning those rows into named attributes requires the table's
//! `CREATE TABLE` statement from `sqlite_master`, which is what this module
//! parses.
//!
//! Both consumers of GeoPackage rows in this crate use it:
//!
//! - [`crate::layer`] — builds [`oxigeo_core::vector::Feature`] values for the
//!   [`Dataset::layers`](crate::Dataset::layers) API.
//! - [`crate::streaming_geopackage`] — builds
//!   [`StreamingFeature`](crate::streaming::StreamingFeature) values with JSON
//!   properties.
//!
//! Keeping the parsing here means a schema fix (rowid aliasing, quoted
//! identifiers, table-level constraints) lands in both paths at once.

use oxigeo_gpkg::{CellValue, GeoPackage};

/// One column of a GeoPackage user table, as declared by its DDL.
#[derive(Debug, Clone)]
pub(crate) struct ColumnDef {
    /// Column name, unquoted.
    pub(crate) name: String,
    /// `true` when the column is declared `INTEGER PRIMARY KEY`, which in
    /// SQLite makes it an alias for the row's 64-bit rowid.
    ///
    /// The value of such a column is **not** stored in the record payload —
    /// SQLite writes `NULL` there and keeps the real value in the cell's rowid
    /// field.  Readers must substitute the rowid, otherwise every `fid` reads
    /// back as null (see [`ColumnValue::resolve`]).
    pub(crate) rowid_alias: bool,
}

/// Column layout of a GeoPackage user table.
#[derive(Debug, Clone, Default)]
pub(crate) struct TableSchema {
    columns: Vec<ColumnDef>,
}

impl TableSchema {
    /// Recover the column layout of `table_name` from `sqlite_master`.
    ///
    /// Falls back to positional names (`col0`, `col1`, …) when the DDL cannot
    /// be parsed, so callers always get a usable — if less descriptive —
    /// schema.  Returns an empty schema when the table does not exist.
    pub(crate) fn load(gpkg: &GeoPackage, table_name: &str) -> Self {
        let master = match gpkg.scan_sqlite_master() {
            Ok(entries) => entries,
            Err(_) => return Self::default(),
        };

        for entry in &master {
            if entry.entry_type != "table" || !entry.name.eq_ignore_ascii_case(table_name) {
                continue;
            }
            if let Some(columns) = parse_create_table_columns(&entry.sql) {
                return Self { columns };
            }
            // DDL present but unparseable: fall back to positional names sized
            // from the first row of the table.
            if let Ok(Some(rows)) = gpkg.scan_table_by_name(table_name)
                && let Some((_rowid, cells)) = rows.first()
            {
                return Self {
                    columns: (0..cells.len())
                        .map(|i| ColumnDef {
                            name: format!("col{i}"),
                            rowid_alias: false,
                        })
                        .collect(),
                };
            }
            break;
        }

        Self::default()
    }

    /// Column definitions in declaration order.
    pub(crate) fn columns(&self) -> &[ColumnDef] {
        &self.columns
    }

    /// Position of the column named `name` (ASCII case-insensitive).
    pub(crate) fn index_of(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
    }

    /// Resolve the value of column `index` for a scanned row.
    ///
    /// `rowid` is the cell's rowid as returned by
    /// [`GeoPackage::scan_table_by_name`]; it is substituted for `INTEGER
    /// PRIMARY KEY` columns whose stored payload is `NULL`, which is how SQLite
    /// records a rowid alias.
    pub(crate) fn resolve<'a>(
        &self,
        index: usize,
        cells: &'a [CellValue],
        rowid: i64,
    ) -> ColumnValue<'a> {
        match cells.get(index) {
            Some(CellValue::Null) | None
                if self.columns.get(index).is_some_and(|c| c.rowid_alias) =>
            {
                ColumnValue::RowId(rowid)
            }
            Some(value) => ColumnValue::Cell(value),
            None => ColumnValue::Missing,
        }
    }
}

/// A column value resolved against a table schema.
///
/// Borrows the scanned cell where one exists, so resolving a row costs no
/// allocation even when the payload is a large geometry blob.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ColumnValue<'a> {
    /// The value stored in the record payload.
    Cell(&'a CellValue),
    /// An `INTEGER PRIMARY KEY` column, whose value is the row's rowid.
    RowId(i64),
    /// The row has fewer values than the schema has columns.
    Missing,
}

/// Look up the geometry column name of `table_name` in `gpkg_geometry_columns`.
///
/// Returns `None` when the metadata table cannot be read or holds no entry for
/// the given table.
pub(crate) fn geometry_column_name(gpkg: &GeoPackage, table_name: &str) -> Option<String> {
    // gpkg_geometry_columns: (table_name, column_name, geometry_type_name,
    //                         srs_id, z, m)
    let rows = gpkg.scan_table_by_name("gpkg_geometry_columns").ok()??;
    for (_rowid, values) in rows {
        if let (Some(CellValue::Text(table)), Some(CellValue::Text(column))) =
            (values.first(), values.get(1))
            && table.eq_ignore_ascii_case(table_name)
        {
            return Some(column.clone());
        }
    }
    None
}

/// Look up the declared OGC geometry type of `table_name`, e.g. `"POINT"`.
pub(crate) fn geometry_type_name(gpkg: &GeoPackage, table_name: &str) -> Option<String> {
    let rows = gpkg.scan_table_by_name("gpkg_geometry_columns").ok()??;
    for (_rowid, values) in rows {
        if let (Some(CellValue::Text(table)), Some(CellValue::Text(type_name))) =
            (values.first(), values.get(2))
            && table.eq_ignore_ascii_case(table_name)
        {
            return Some(type_name.clone());
        }
    }
    None
}

// ─── CREATE TABLE parsing ────────────────────────────────────────────────────

/// Parse the column list of a `CREATE TABLE t (…)` statement.
///
/// Splits on **top-level** commas only, so that constraint clauses carrying
/// their own parenthesised lists (`PRIMARY KEY (a, b)`) do not fragment into
/// bogus columns, and skips table-level constraints.  Returns `None` when the
/// statement has no parenthesised body or yields no columns.
fn parse_create_table_columns(sql: &str) -> Option<Vec<ColumnDef>> {
    let open = sql.find('(')?;
    let close = sql.rfind(')')?;
    if close <= open {
        return None;
    }

    let mut columns = Vec::new();
    for part in split_top_level(&sql[open + 1..close]) {
        let trimmed = part.trim();
        if trimmed.is_empty() || is_table_constraint(trimmed) {
            continue;
        }
        if let Some(name) = parse_column_name_token(trimmed) {
            columns.push(ColumnDef {
                rowid_alias: is_integer_primary_key(trimmed),
                name,
            });
        }
    }

    if columns.is_empty() {
        None
    } else {
        Some(columns)
    }
}

/// Split a `CREATE TABLE` body on commas that are not nested inside
/// parentheses or inside a quoted identifier / string literal.
fn split_top_level(body: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut quote: Option<char> = None;

    for ch in body.chars() {
        match quote {
            Some(q) => {
                current.push(ch);
                if ch == q || (q == '[' && ch == ']') {
                    quote = None;
                }
            }
            None => match ch {
                '"' | '\'' | '`' | '[' => {
                    quote = Some(ch);
                    current.push(ch);
                }
                '(' => {
                    depth += 1;
                    current.push(ch);
                }
                ')' => {
                    depth = depth.saturating_sub(1);
                    current.push(ch);
                }
                ',' if depth == 0 => {
                    parts.push(core::mem::take(&mut current));
                }
                _ => current.push(ch),
            },
        }
    }
    parts.push(current);
    parts
}

/// `true` when a body item is a table-level constraint rather than a column.
fn is_table_constraint(item: &str) -> bool {
    let upper = item.to_ascii_uppercase();
    upper.starts_with("PRIMARY")
        || upper.starts_with("UNIQUE")
        || upper.starts_with("CHECK")
        || upper.starts_with("FOREIGN")
        || upper.starts_with("CONSTRAINT")
}

/// `true` when a column definition declares `INTEGER PRIMARY KEY` — SQLite's
/// rowid alias.
fn is_integer_primary_key(col_def: &str) -> bool {
    let upper = col_def.to_ascii_uppercase();
    if !upper.contains("PRIMARY KEY") {
        return false;
    }
    // The declared type is whatever sits between the column name and the
    // constraint keywords; a rowid alias requires it to be exactly `INTEGER`.
    let mut tokens = upper.split_whitespace();
    let _name = tokens.next();
    tokens
        .next()
        .is_some_and(|type_token| type_token == "INTEGER")
}

/// Extract the column name from a column definition, unquoting the identifier.
fn parse_column_name_token(col_def: &str) -> Option<String> {
    let col_def = col_def.trim();
    if let Some(rest) = col_def.strip_prefix('"') {
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    } else if let Some(rest) = col_def.strip_prefix('`') {
        let end = rest.find('`')?;
        Some(rest[..end].to_string())
    } else if let Some(rest) = col_def.strip_prefix('[') {
        let end = rest.find(']')?;
        Some(rest[..end].to_string())
    } else {
        Some(col_def.split_whitespace().next()?.to_string())
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_feature_table() {
        let columns = parse_create_table_columns(
            "CREATE TABLE cities (\n  fid INTEGER PRIMARY KEY AUTOINCREMENT,\n  geom BLOB,\n  name TEXT\n)",
        )
        .expect("parse");
        let names: Vec<&str> = columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["fid", "geom", "name"]);
        assert!(columns[0].rowid_alias, "fid is an INTEGER PRIMARY KEY");
        assert!(!columns[1].rowid_alias);
    }

    #[test]
    fn skips_table_level_constraints() {
        let columns = parse_create_table_columns(
            "CREATE TABLE gpkg_geometry_columns (\n  table_name TEXT NOT NULL,\n  column_name TEXT NOT NULL,\n  CONSTRAINT pk_geom_cols PRIMARY KEY (table_name, column_name),\n  CONSTRAINT uk_gc_table_name UNIQUE (table_name)\n)",
        )
        .expect("parse");
        let names: Vec<&str> = columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            ["table_name", "column_name"],
            "named table constraints must not be mistaken for columns"
        );
    }

    #[test]
    fn handles_quoted_identifiers() {
        let columns = parse_create_table_columns(
            "CREATE TABLE \"my layer\" (\"fid\" INTEGER PRIMARY KEY, \"geom\" POINT, \"name, alt\" TEXT)",
        )
        .expect("parse");
        let names: Vec<&str> = columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["fid", "geom", "name, alt"]);
        assert!(columns[0].rowid_alias);
    }

    #[test]
    fn text_primary_key_is_not_a_rowid_alias() {
        let columns =
            parse_create_table_columns("CREATE TABLE t (code TEXT PRIMARY KEY, n INTEGER)")
                .expect("parse");
        assert!(!columns[0].rowid_alias, "TEXT PRIMARY KEY is not a rowid");
    }

    #[test]
    fn resolves_rowid_alias_from_null_payload() {
        let schema = TableSchema {
            columns: vec![
                ColumnDef {
                    name: "fid".to_string(),
                    rowid_alias: true,
                },
                ColumnDef {
                    name: "name".to_string(),
                    rowid_alias: false,
                },
            ],
        };
        let cells = vec![CellValue::Null, CellValue::Text("Kyoto".to_string())];
        match schema.resolve(0, &cells, 42) {
            ColumnValue::RowId(id) => assert_eq!(id, 42),
            other => panic!("expected RowId, got {other:?}"),
        }
        match schema.resolve(1, &cells, 42) {
            ColumnValue::Cell(CellValue::Text(t)) => assert_eq!(t, "Kyoto"),
            other => panic!("expected Text cell, got {other:?}"),
        }
    }
}
