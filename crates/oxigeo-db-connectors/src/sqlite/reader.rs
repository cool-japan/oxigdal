//! SQLite spatial data reader.

use crate::error::{Error, Result};
use crate::sqlite::{SqliteConnector, wkb_to_geometry};
use geo::BoundingRect;
use geo_types::Geometry;
use oxisql_core::Value;
use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// Feature read from SQLite.
#[derive(Debug, Clone)]
pub struct SqliteFeature {
    /// Feature ID.
    pub id: i64,
    /// Geometry.
    pub geometry: Geometry<f64>,
    /// Properties.
    pub properties: HashMap<String, JsonValue>,
}

/// SQLite spatial data reader.
pub struct SqliteReader {
    connector: SqliteConnector,
    table_name: String,
    geometry_column: String,
}

impl SqliteReader {
    /// Create a new SQLite reader.
    pub fn new(connector: SqliteConnector, table_name: String, geometry_column: String) -> Self {
        Self {
            connector,
            table_name,
            geometry_column,
        }
    }

    /// Read all features from the table.
    pub fn read_all(&self) -> Result<Vec<SqliteFeature>> {
        let sql = format!("SELECT * FROM {}", self.table_name);
        self.read_with_sql(&sql)
    }

    /// Read features with a WHERE clause.
    pub fn read_where(&self, where_clause: &str) -> Result<Vec<SqliteFeature>> {
        let sql = format!("SELECT * FROM {} WHERE {}", self.table_name, where_clause);
        self.read_with_sql(&sql)
    }

    /// Read features within a bounding box (pure-Rust fallback — loads all and filters in memory).
    pub fn read_bbox(
        &self,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
    ) -> Result<Vec<SqliteFeature>> {
        // No SpatiaLite in pure-Rust mode: load all and filter by bounding box.
        let all = self.read_all()?;
        let filtered = all
            .into_iter()
            .filter(|feat| geometry_intersects_bbox(&feat.geometry, min_x, min_y, max_x, max_y))
            .collect();
        Ok(filtered)
    }

    /// Count features in the table.
    pub fn count(&self) -> Result<i64> {
        let sql = format!("SELECT COUNT(*) FROM {}", self.table_name);
        let rows = self
            .connector
            .blocking_conn()
            .query(&sql, &[])
            .map_err(|e| Error::Query(e.to_string()))?;
        extract_count(&rows)
    }

    /// Count features matching a WHERE clause.
    pub fn count_where(&self, where_clause: &str) -> Result<i64> {
        let sql = format!(
            "SELECT COUNT(*) FROM {} WHERE {}",
            self.table_name, where_clause
        );
        let rows = self
            .connector
            .blocking_conn()
            .query(&sql, &[])
            .map_err(|e| Error::Query(e.to_string()))?;
        extract_count(&rows)
    }

    /// Read features with custom SQL.
    fn read_with_sql(&self, sql: &str) -> Result<Vec<SqliteFeature>> {
        let rows = self
            .connector
            .blocking_conn()
            .query(sql, &[])
            .map_err(|e| Error::Query(e.to_string()))?;

        let mut features = Vec::new();
        for row in &rows {
            match self.row_to_feature(row) {
                Ok(feature) => features.push(feature),
                Err(_) => continue,
            }
        }
        Ok(features)
    }

    /// Convert a Row to a SqliteFeature.
    fn row_to_feature(&self, row: &oxisql_core::Row) -> Result<SqliteFeature> {
        let id = row
            .try_get::<i64>("id")
            .map_err(|e| Error::Query(e.to_string()))?;

        let geom_blob = row
            .try_get::<Vec<u8>>(&self.geometry_column)
            .map_err(|e| Error::Query(e.to_string()))?;
        let geometry = wkb_to_geometry(&geom_blob)?;

        // Enumerate all columns and convert non-id, non-geometry columns to JSON properties
        let mut properties = HashMap::new();
        let col_names = row.columns();
        for (i, col_name) in col_names.iter().enumerate() {
            if col_name == "id" || col_name == &self.geometry_column {
                continue;
            }
            if let Some(val) = row.get_by_index(i) {
                properties.insert(col_name.clone(), oxisql_value_to_json(val));
            }
        }

        Ok(SqliteFeature {
            id,
            geometry,
            properties,
        })
    }
}

/// Extract i64 count from first row first column.
fn extract_count(rows: &[oxisql_core::Row]) -> Result<i64> {
    rows.first()
        .and_then(|row| row.get_by_index(0))
        .and_then(|v| match v {
            Value::I64(n) => Some(*n),
            _ => None,
        })
        .ok_or_else(|| Error::Query("COUNT query returned no result".to_string()))
}

/// Convert oxisql Value to JSON value.
fn oxisql_value_to_json(val: &Value) -> JsonValue {
    match val {
        Value::Null => JsonValue::Null,
        Value::I64(n) => JsonValue::Number((*n).into()),
        Value::F64(f) => serde_json::Number::from_f64(*f)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        Value::Text(s) => JsonValue::String(s.clone()),
        Value::Bool(b) => JsonValue::Bool(*b),
        Value::Blob(bytes) => {
            // Try to parse as JSON, otherwise base64-encode
            if let Ok(json) = serde_json::from_slice::<JsonValue>(bytes) {
                json
            } else {
                use base64::Engine;
                JsonValue::String(base64::engine::general_purpose::STANDARD.encode(bytes))
            }
        }
        Value::Timestamp(us) => JsonValue::Number((*us).into()),
        Value::Date(days) => JsonValue::Number((i64::from(*days)).into()),
        Value::Time(us) => JsonValue::Number((*us).into()),
        Value::Uuid(u) => JsonValue::String(format!("{:032x}", u)),
        Value::Json(s) => {
            serde_json::from_str::<JsonValue>(s).unwrap_or_else(|_| JsonValue::String(s.clone()))
        }
        Value::Decimal(s) => JsonValue::String(s.clone()),
        Value::Array(arr) => JsonValue::Array(arr.iter().map(oxisql_value_to_json).collect()),
        Value::TypedArray { values, .. } => {
            JsonValue::Array(values.iter().map(oxisql_value_to_json).collect())
        }
    }
}

/// Check if geometry bounding box intersects the given bbox (in-memory filter).
fn geometry_intersects_bbox(
    geom: &Geometry<f64>,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
) -> bool {
    if let Some(bbox) = geom.bounding_rect() {
        bbox.min().x <= max_x
            && bbox.max().x >= min_x
            && bbox.min().y <= max_y
            && bbox.max().y >= min_y
    } else {
        false
    }
}
