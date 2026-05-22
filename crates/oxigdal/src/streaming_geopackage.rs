//! GeoPackage feature streaming implementation.
//!
//! Reads all feature tables from a `.gpkg` file and yields each row as a
//! [`StreamingFeature`] with WKB geometry and JSON properties.
//!
//! The implementation is **eager**: all rows from all feature tables are loaded
//! into memory at construction time (matching the pattern of the sibling
//! GeoJSON and FlatGeobuf implementations).  This is acceptable because the
//! GeoPackage driver exposes only a `Vec`-returning API.  True lazy streaming
//! is deferred to a future refactoring pass.

use std::collections::HashMap;

use oxigdal_core::error::{IoError, OxiGdalError};
use serde_json::Value as JsonValue;

use crate::streaming::{FeatureStream, StreamingFeature};
use crate::{DatasetInfo, Result};

/// Stream features from a GeoPackage file specified by `info.path`.
///
/// When the `gpkg` feature is enabled and `info.path` points to a valid
/// GeoPackage file, this returns a [`FeatureStream`] over all feature rows
/// in all feature-type tables inside the package.
///
/// Falls back to an empty stream when:
/// - `info.path` is `None` (programmatic dataset)
/// - the `gpkg` feature is disabled
/// - the file cannot be read or parsed
pub(crate) fn stream_geopackage_features(info: &DatasetInfo) -> Result<FeatureStream> {
    let path = match &info.path {
        Some(p) => p.clone(),
        None => return Ok(FeatureStream::empty()),
    };

    let data = std::fs::read(&path).map_err(|e| {
        OxiGdalError::Io(IoError::Read {
            message: format!("cannot read GeoPackage for streaming '{path}': {e}"),
        })
    })?;

    use oxigdal_gpkg::{GeoPackage, GpkgBinaryParser, GpkgDataType};

    let mut gpkg = GeoPackage::from_bytes(data).map_err(|e| OxiGdalError::Internal {
        message: format!("cannot parse GeoPackage '{path}': {e}"),
    })?;

    // load_contents may fail on minimal/empty GPKGs that lack the system tables.
    // Treat this as an empty file (no features) rather than an error, to be
    // consistent with the sibling drivers that fall back to empty on parse failures.
    if gpkg.load_contents().is_err() {
        return Ok(FeatureStream::empty());
    }

    // Collect the names of all feature tables.
    let feature_table_names: Vec<String> = gpkg
        .contents
        .iter()
        .filter(|c| c.data_type == GpkgDataType::Features)
        .map(|c| c.table_name.clone())
        .collect();

    let mut all_features: Vec<StreamingFeature> = Vec::new();

    for table_name in &feature_table_names {
        // Scan the feature table by name.  `scan_table_by_name` returns
        // `Vec<(rowid, Vec<CellValue>)>` — there is no schema-aware API,
        // so we derive column names from `sqlite_master`.
        let rows =
            match gpkg
                .scan_table_by_name(table_name)
                .map_err(|e| OxiGdalError::Internal {
                    message: format!("cannot scan table '{table_name}' in '{path}': {e}"),
                })? {
                Some(r) => r,
                None => continue,
            };

        // Determine the geometry column name from gpkg_geometry_columns.
        // We look up the geometry column table entry for this feature table.
        let geom_col_name =
            gpkg_geometry_column_name(&gpkg, table_name).unwrap_or_else(|| "geom".to_string());

        // Derive column names by inspecting sqlite_master for this table's
        // CREATE TABLE statement, falling back to positional names.
        let col_names = column_names_from_master(&gpkg, table_name);

        for (_rowid, cell_values) in rows {
            use oxigdal_gpkg::CellValue;

            // Find geometry column index by name (or default to first Blob).
            let geom_idx = col_names
                .iter()
                .position(|n| n.eq_ignore_ascii_case(&geom_col_name));

            // Decode geometry.
            let geometry: Option<Vec<u8>> = geom_idx
                .and_then(|idx| cell_values.get(idx))
                .and_then(|cv| match cv {
                    CellValue::Blob(b) => Some(b.clone()),
                    _ => None,
                })
                .and_then(|blob| GpkgBinaryParser::parse(&blob).ok())
                .map(|g| GpkgBinaryParser::to_wkb(&g));

            // Build properties from non-geometry columns.
            let properties: HashMap<String, JsonValue> = col_names
                .iter()
                .enumerate()
                .filter(|(idx, name)| {
                    // Skip the geometry column.
                    !name.eq_ignore_ascii_case(&geom_col_name) && (geom_idx != Some(*idx))
                })
                .filter_map(|(idx, name)| {
                    cell_values.get(idx).map(|cv| {
                        let jv = cell_value_to_json(cv);
                        (name.clone(), jv)
                    })
                })
                .collect();

            all_features.push(StreamingFeature::new(geometry, properties));
        }
    }

    Ok(FeatureStream::from_vec(all_features))
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Look up the geometry column name for `table_name` in `gpkg_geometry_columns`.
///
/// Returns `None` when the metadata table cannot be read or has no entry for
/// the given table.
fn gpkg_geometry_column_name(gpkg: &oxigdal_gpkg::GeoPackage, table_name: &str) -> Option<String> {
    // Scan gpkg_geometry_columns: (table_name, column_name, geometry_type, srs_id, z, m)
    let rows = gpkg.scan_table_by_name("gpkg_geometry_columns").ok()??;
    for (_rowid, values) in rows {
        use oxigdal_gpkg::CellValue;
        if let (Some(CellValue::Text(tbl)), Some(CellValue::Text(col))) =
            (values.first(), values.get(1))
        {
            if tbl.eq_ignore_ascii_case(table_name) {
                return Some(col.clone());
            }
        }
    }
    None
}

/// Derive column names for `table_name` from `sqlite_master.sql`.
///
/// Parses `CREATE TABLE t (col1 type, col2 type, ...)` to extract column
/// names in declaration order.  Returns positional names (`col0`, `col1`, …)
/// when parsing fails.
fn column_names_from_master(gpkg: &oxigdal_gpkg::GeoPackage, table_name: &str) -> Vec<String> {
    let fallback = |n: usize| -> Vec<String> { (0..n).map(|i| format!("col{i}")).collect() };

    let master = match gpkg.scan_sqlite_master() {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };

    for entry in &master {
        if entry.entry_type == "table" && entry.name.eq_ignore_ascii_case(table_name) {
            if let Some(names) = parse_create_table_columns(&entry.sql) {
                return names;
            }
            // Couldn't parse; fall back to a scan to count columns.
            if let Ok(Some(rows)) = gpkg.scan_table_by_name(table_name) {
                if let Some((_rid, cols)) = rows.first() {
                    return fallback(cols.len());
                }
            }
            break;
        }
    }
    Vec::new()
}

/// Minimal `CREATE TABLE t (col1 type1, col2 type2, …)` column-name parser.
///
/// Returns `None` when the SQL cannot be parsed (e.g. expressions, CHECK
/// constraints, etc.).  Only handles the simple common case.
fn parse_create_table_columns(sql: &str) -> Option<Vec<String>> {
    // Find the content between the outer parentheses.
    let open = sql.find('(')?;
    let close = sql.rfind(')')?;
    if close <= open {
        return None;
    }
    let inner = &sql[open + 1..close];

    let mut names = Vec::new();
    for part in inner.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Skip table-level constraints: PRIMARY KEY, UNIQUE, CHECK, FOREIGN KEY.
        let upper = trimmed.to_ascii_uppercase();
        if upper.starts_with("PRIMARY")
            || upper.starts_with("UNIQUE")
            || upper.starts_with("CHECK")
            || upper.starts_with("FOREIGN")
        {
            continue;
        }
        // The column name is the first token (possibly quoted).
        if let Some(name) = parse_column_name_token(trimmed) {
            names.push(name);
        }
    }

    if names.is_empty() { None } else { Some(names) }
}

/// Extract the column name from a column definition token.
///
/// Handles plain identifiers and double-quoted identifiers.
fn parse_column_name_token(col_def: &str) -> Option<String> {
    let col_def = col_def.trim();
    if let Some(rest) = col_def.strip_prefix('"') {
        // Double-quoted identifier — find closing quote.
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    } else if let Some(rest) = col_def.strip_prefix('`') {
        let end = rest.find('`')?;
        Some(rest[..end].to_string())
    } else if let Some(rest) = col_def.strip_prefix('[') {
        let end = rest.find(']')?;
        Some(rest[..end].to_string())
    } else {
        // Plain identifier — first whitespace-separated token.
        Some(col_def.split_whitespace().next()?.to_string())
    }
}

/// Convert a [`CellValue`] to a [`serde_json::Value`].
fn cell_value_to_json(cv: &oxigdal_gpkg::CellValue) -> JsonValue {
    use oxigdal_gpkg::CellValue;
    match cv {
        CellValue::Integer(i) => JsonValue::Number((*i).into()),
        CellValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        CellValue::Text(s) => JsonValue::String(s.clone()),
        CellValue::Blob(b) => {
            // Encode binary as a hex-prefixed string for JSON portability.
            let hex: String = b.iter().map(|byte| format!("{byte:02x}")).collect();
            JsonValue::String(format!("0x{hex}"))
        }
        CellValue::Null => JsonValue::Null,
    }
}
