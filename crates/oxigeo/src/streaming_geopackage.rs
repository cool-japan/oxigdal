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

use oxigeo_core::error::{IoError, OxiGeoError};
use serde_json::Value as JsonValue;

use crate::gpkg_schema::{ColumnValue, TableSchema, geometry_column_name};
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
        OxiGeoError::Io(IoError::Read {
            message: format!("cannot read GeoPackage for streaming '{path}': {e}"),
        })
    })?;

    use oxigeo_gpkg::{GeoPackage, GpkgBinaryParser, GpkgDataType};

    let mut gpkg = GeoPackage::from_bytes(data).map_err(|e| OxiGeoError::Internal {
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
        // `Vec<(rowid, Vec<CellValue>)>` — positional values, so column names
        // and rowid aliasing come from the shared schema parser.
        let rows = match gpkg
            .scan_table_by_name(table_name)
            .map_err(|e| OxiGeoError::Internal {
                message: format!("cannot scan table '{table_name}' in '{path}': {e}"),
            })? {
            Some(r) => r,
            None => continue,
        };

        let schema = TableSchema::load(&gpkg, table_name);
        let geom_col_name =
            geometry_column_name(&gpkg, table_name).unwrap_or_else(|| "geom".to_string());
        let geom_idx = schema.index_of(&geom_col_name);

        for (rowid, cell_values) in rows {
            use oxigeo_gpkg::CellValue;

            // Decode geometry.
            let geometry: Option<Vec<u8>> = geom_idx
                .map(|idx| schema.resolve(idx, &cell_values, rowid))
                .and_then(|value| match value {
                    ColumnValue::Cell(CellValue::Blob(b)) => Some(b),
                    _ => None,
                })
                .and_then(|blob| GpkgBinaryParser::parse(blob).ok())
                .map(|g| GpkgBinaryParser::to_wkb(&g));

            // Build properties from non-geometry columns.
            let properties: HashMap<String, JsonValue> = schema
                .columns()
                .iter()
                .enumerate()
                .filter(|(idx, _)| geom_idx != Some(*idx))
                .map(|(idx, column)| {
                    let value = match schema.resolve(idx, &cell_values, rowid) {
                        ColumnValue::Cell(cell) => cell_value_to_json(cell),
                        // `INTEGER PRIMARY KEY` columns store NULL in the
                        // payload; their value is the row's rowid.
                        ColumnValue::RowId(id) => JsonValue::Number(id.into()),
                        ColumnValue::Missing => JsonValue::Null,
                    };
                    (column.name.clone(), value)
                })
                .collect();

            all_features.push(StreamingFeature::new(geometry, properties));
        }
    }

    Ok(FeatureStream::from_vec(all_features))
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Convert a [`CellValue`] to a [`serde_json::Value`].
fn cell_value_to_json(cv: &oxigeo_gpkg::CellValue) -> JsonValue {
    use oxigeo_gpkg::CellValue;
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
