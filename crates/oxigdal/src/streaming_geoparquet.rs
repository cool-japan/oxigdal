//! GeoParquet feature streaming implementation.
//!
//! Reads all rows from a `.parquet` file and yields each as a
//! [`StreamingFeature`] with raw WKB geometry bytes and JSON properties.
//!
//! The implementation uses the `GeoParquetReader` pushdown API introduced in
//! OxiGDAL 0.1.5 Item 1.  It eagerly collects all `RecordBatch` rows into
//! a `Vec<StreamingFeature>` (matching the GeoJSON and Shapefile sibling
//! pattern) and returns a [`FeatureStream::from_vec`].  True lazy streaming
//! is deferred to a future refactoring pass once the streaming trait grows
//! an async cursor abstraction.

use std::collections::HashMap;

use arrow_array::Array;
use oxigdal_core::error::{IoError, OxiGdalError};
use serde_json::Value as JsonValue;

use crate::streaming::{FeatureStream, StreamingFeature};
use crate::{DatasetInfo, Result};

/// Stream features from a GeoParquet file specified by `info.path`.
///
/// When the `geoparquet` feature is enabled and `info.path` points to a valid
/// GeoParquet file, this returns a [`FeatureStream`] over all rows.
///
/// Falls back to an empty stream when:
/// - `info.path` is `None` (programmatic dataset)
/// - the `geoparquet` feature is disabled
/// - the file cannot be read or is not a valid GeoParquet file
pub(crate) fn stream_geoparquet_features(info: &DatasetInfo) -> Result<FeatureStream> {
    let path = match &info.path {
        Some(p) => p.clone(),
        None => return Ok(FeatureStream::empty()),
    };

    use oxigdal_geoparquet::{GeoParquetReader, geometry::WkbWriter};

    let reader = GeoParquetReader::open(&path).map_err(|e| {
        OxiGdalError::Io(IoError::Read {
            message: format!("cannot open GeoParquet for streaming '{path}': {e}"),
        })
    })?;

    let geom_col = reader.geometry_column_name().to_string();

    // read_all() returns a GeoParquetBatchReader; collect all batches.
    let mut batch_reader = reader.read_all().map_err(|e| OxiGdalError::Internal {
        message: format!("cannot create GeoParquet batch reader for '{path}': {e}"),
    })?;

    let mut all_features: Vec<StreamingFeature> = Vec::new();

    while let Some(batch) = batch_reader
        .next_batch()
        .map_err(|e| OxiGdalError::Internal {
            message: format!("error reading GeoParquet batch from '{path}': {e}"),
        })?
    {
        let schema = batch.schema();
        let num_rows = batch.num_rows();

        // Find the geometry column index.
        let geom_col_idx = schema.index_of(&geom_col).ok();

        for row in 0..num_rows {
            // Decode WKB geometry bytes for this row.
            let geometry: Option<Vec<u8>> = geom_col_idx.and_then(|gi| {
                let col = batch.column(gi);
                let binary = col.as_any().downcast_ref::<arrow_array::BinaryArray>()?;
                if binary.is_null(row) {
                    return None;
                }
                let wkb_bytes = binary.value(row);
                // Re-encode via internal WKB reader→writer to normalise to
                // little-endian ISO WKB (the format expected by FeatureStream consumers).
                let mut wkb_reader = oxigdal_geoparquet::geometry::WkbReader::new(wkb_bytes);
                let geom = wkb_reader.read_geometry().ok()?;
                let mut writer = WkbWriter::new(true); // little-endian
                writer.write_geometry(&geom).ok()
            });

            // Build properties from all non-geometry columns.
            let properties: HashMap<String, JsonValue> = schema
                .fields()
                .iter()
                .enumerate()
                .filter(|(idx, field)| {
                    // Skip the geometry column.
                    Some(*idx) != geom_col_idx && field.name() != &geom_col
                })
                .map(|(idx, field)| {
                    let col = batch.column(idx);
                    let jv = arrow_col_value_to_json(col, row);
                    (field.name().clone(), jv)
                })
                .collect();

            all_features.push(StreamingFeature::new(geometry, properties));
        }
    }

    Ok(FeatureStream::from_vec(all_features))
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Convert a single row value from an Arrow array column to a JSON value.
///
/// Handles the most common Arrow data types encountered in GeoParquet files.
/// Unrecognised column types are represented as `null`.
fn arrow_col_value_to_json(col: &dyn arrow_array::Array, row: usize) -> JsonValue {
    use arrow_schema::DataType;

    if col.is_null(row) {
        return JsonValue::Null;
    }

    match col.data_type() {
        DataType::Boolean => {
            if let Some(a) = col.as_any().downcast_ref::<arrow_array::BooleanArray>() {
                return JsonValue::Bool(a.value(row));
            }
        }
        DataType::Int8 => {
            if let Some(a) = col.as_any().downcast_ref::<arrow_array::Int8Array>() {
                return JsonValue::Number(a.value(row).into());
            }
        }
        DataType::Int16 => {
            if let Some(a) = col.as_any().downcast_ref::<arrow_array::Int16Array>() {
                return JsonValue::Number(a.value(row).into());
            }
        }
        DataType::Int32 => {
            if let Some(a) = col.as_any().downcast_ref::<arrow_array::Int32Array>() {
                return JsonValue::Number(a.value(row).into());
            }
        }
        DataType::Int64 => {
            if let Some(a) = col.as_any().downcast_ref::<arrow_array::Int64Array>() {
                return JsonValue::Number(a.value(row).into());
            }
        }
        DataType::UInt8 => {
            if let Some(a) = col.as_any().downcast_ref::<arrow_array::UInt8Array>() {
                return JsonValue::Number(a.value(row).into());
            }
        }
        DataType::UInt16 => {
            if let Some(a) = col.as_any().downcast_ref::<arrow_array::UInt16Array>() {
                return JsonValue::Number(a.value(row).into());
            }
        }
        DataType::UInt32 => {
            if let Some(a) = col.as_any().downcast_ref::<arrow_array::UInt32Array>() {
                return JsonValue::Number(a.value(row).into());
            }
        }
        DataType::UInt64 => {
            if let Some(a) = col.as_any().downcast_ref::<arrow_array::UInt64Array>() {
                return JsonValue::Number(a.value(row).into());
            }
        }
        DataType::Float32 => {
            if let Some(a) = col.as_any().downcast_ref::<arrow_array::Float32Array>() {
                let v = a.value(row) as f64;
                if let Some(n) = serde_json::Number::from_f64(v) {
                    return JsonValue::Number(n);
                }
                return JsonValue::Null;
            }
        }
        DataType::Float64 => {
            if let Some(a) = col.as_any().downcast_ref::<arrow_array::Float64Array>() {
                let v = a.value(row);
                if let Some(n) = serde_json::Number::from_f64(v) {
                    return JsonValue::Number(n);
                }
                return JsonValue::Null;
            }
        }
        DataType::Utf8 => {
            if let Some(a) = col.as_any().downcast_ref::<arrow_array::StringArray>() {
                return JsonValue::String(a.value(row).to_string());
            }
        }
        DataType::LargeUtf8 => {
            if let Some(a) = col.as_any().downcast_ref::<arrow_array::LargeStringArray>() {
                return JsonValue::String(a.value(row).to_string());
            }
        }
        DataType::Binary | DataType::LargeBinary => {
            // Encode binary as hex for JSON portability.
            if let Some(a) = col.as_any().downcast_ref::<arrow_array::BinaryArray>() {
                let hex: String = a.value(row).iter().map(|b| format!("{b:02x}")).collect();
                return JsonValue::String(format!("0x{hex}"));
            }
        }
        _ => {} // Fall through to Null for unrecognised types.
    }
    JsonValue::Null
}
