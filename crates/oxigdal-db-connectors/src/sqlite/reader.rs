//! SQLite spatial data reader.

use crate::error::{Error, Result};
use crate::sqlite::{SqliteConnector, wkb_to_geometry};
use geo_types::Geometry;
use rusqlite::Row;
use serde_json::Value;
use std::collections::HashMap;

/// Feature read from SQLite.
#[derive(Debug, Clone)]
pub struct SqliteFeature {
    /// Feature ID.
    pub id: i64,
    /// Geometry.
    pub geometry: Geometry<f64>,
    /// Properties.
    pub properties: HashMap<String, Value>,
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
        self.read_with_query(&sql, &[])
    }

    /// Read features with a WHERE clause.
    pub fn read_where(&self, where_clause: &str) -> Result<Vec<SqliteFeature>> {
        let sql = format!("SELECT * FROM {} WHERE {}", self.table_name, where_clause);
        self.read_with_query(&sql, &[])
    }

    /// Read features within a bounding box.
    pub fn read_bbox(
        &self,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
    ) -> Result<Vec<SqliteFeature>> {
        let conn = self.connector.conn();

        if self.connector.has_spatialite()? {
            let sql = format!(
                "SELECT * FROM {} WHERE MbrIntersects({}, BuildMbr(?, ?, ?, ?))",
                self.table_name, self.geometry_column
            );

            let mut stmt = conn.prepare(&sql)?;
            let mut rows = stmt.query([min_x, min_y, max_x, max_y])?;

            let mut features = Vec::new();
            while let Some(row) = rows.next()? {
                if let Ok(feature) = self.row_to_feature(row) {
                    features.push(feature);
                }
            }

            Ok(features)
        } else {
            // Fallback without SpatiaLite
            self.read_all()
        }
    }

    /// Count features in the table.
    pub fn count(&self) -> Result<i64> {
        let conn = self.connector.conn();
        let sql = format!("SELECT COUNT(*) FROM {}", self.table_name);
        let count: i64 = conn.query_row(&sql, [], |row| row.get(0))?;
        Ok(count)
    }

    /// Count features matching a WHERE clause.
    pub fn count_where(&self, where_clause: &str) -> Result<i64> {
        let conn = self.connector.conn();
        let sql = format!(
            "SELECT COUNT(*) FROM {} WHERE {}",
            self.table_name, where_clause
        );
        let count: i64 = conn.query_row(&sql, [], |row| row.get(0))?;
        Ok(count)
    }

    /// Read features with custom query.
    fn read_with_query(
        &self,
        sql: &str,
        params: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<SqliteFeature>> {
        let conn = self.connector.conn();
        let mut stmt = conn.prepare(sql)?;
        let mut rows = stmt.query(params)?;

        let mut features = Vec::new();
        while let Some(row) = rows.next()? {
            if let Ok(feature) = self.row_to_feature(row) {
                features.push(feature);
            }
        }

        Ok(features)
    }

    /// Convert row to feature.
    fn row_to_feature(&self, row: &Row) -> Result<SqliteFeature> {
        let id: i64 = row.get("id")?;

        let geom_data: Vec<u8> = row.get(self.geometry_column.as_str())?;
        let geometry = wkb_to_geometry(&geom_data)?;

        let mut properties = HashMap::new();
        let column_count = row.as_ref().column_count();

        for i in 0..column_count {
            let column_name = row
                .as_ref()
                .column_name(i)
                .map_err(|e| Error::SQLite(e.to_string()))?;

            if column_name != "id" && column_name != self.geometry_column {
                let value = sqlite_value_to_json(row, i)?;
                properties.insert(column_name.to_string(), value);
            }
        }

        Ok(SqliteFeature {
            id,
            geometry,
            properties,
        })
    }
}

/// Convert SQLite value to JSON value.
fn sqlite_value_to_json(row: &Row, index: usize) -> Result<Value> {
    use rusqlite::types::ValueRef;

    match row.get_ref(index)? {
        ValueRef::Null => Ok(Value::Null),
        ValueRef::Integer(i) => Ok(Value::Number(i.into())),
        ValueRef::Real(f) => serde_json::Number::from_f64(f)
            .map(Value::Number)
            .ok_or_else(|| Error::TypeConversion("Invalid float value".to_string())),
        ValueRef::Text(bytes) => {
            let text =
                std::str::from_utf8(bytes).map_err(|e| Error::TypeConversion(e.to_string()))?;
            Ok(Value::String(text.to_string()))
        }
        ValueRef::Blob(bytes) => {
            // Try to parse as JSON, otherwise return as base64
            if let Ok(json) = serde_json::from_slice::<Value>(bytes) {
                Ok(json)
            } else {
                Ok(Value::String(base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    bytes,
                )))
            }
        }
    }
}
