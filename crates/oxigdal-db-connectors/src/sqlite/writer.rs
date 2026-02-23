//! SQLite spatial data writer.

use crate::error::{Error, Result};
use crate::sqlite::{SqliteConnector, geometry_to_wkb};
use geo_types::Geometry;
use rusqlite::params_from_iter;
use serde_json::Value;
use std::collections::HashMap;

/// SQLite spatial data writer.
pub struct SqliteWriter {
    connector: SqliteConnector,
    table_name: String,
    geometry_column: String,
    batch_size: usize,
}

impl SqliteWriter {
    /// Create a new SQLite writer.
    pub fn new(connector: SqliteConnector, table_name: String, geometry_column: String) -> Self {
        Self {
            connector,
            table_name,
            geometry_column,
            batch_size: 1000,
        }
    }

    /// Set batch size for bulk inserts.
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Insert a single feature.
    pub fn insert(
        &self,
        geometry: &Geometry<f64>,
        properties: &HashMap<String, Value>,
    ) -> Result<i64> {
        let wkb = geometry_to_wkb(geometry)?;
        let conn = self.connector.conn();

        let mut columns = vec![self.geometry_column.clone()];
        let mut placeholders = vec!["?".to_string()];

        for key in properties.keys() {
            columns.push(key.clone());
            placeholders.push("?".to_string());
        }

        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            self.table_name,
            columns.join(", "),
            placeholders.join(", ")
        );

        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(wkb)];
        for value in properties.values() {
            params.push(json_to_sqlite_param(value)?);
        }

        conn.execute(&sql, params_from_iter(params.iter().map(|p| p.as_ref())))?;

        Ok(conn.last_insert_rowid())
    }

    /// Insert multiple features in batch.
    pub fn insert_batch(
        &self,
        features: &[(Geometry<f64>, HashMap<String, Value>)],
    ) -> Result<Vec<i64>> {
        if features.is_empty() {
            return Ok(Vec::new());
        }

        let mut ids = Vec::with_capacity(features.len());
        self.connector.begin_transaction()?;

        let result = (|| {
            for chunk in features.chunks(self.batch_size) {
                for (geometry, properties) in chunk {
                    let id = self.insert(geometry, properties)?;
                    ids.push(id);
                }
            }
            Ok::<_, Error>(())
        })();

        match result {
            Ok(()) => {
                self.connector.commit_transaction()?;
                Ok(ids)
            }
            Err(e) => {
                self.connector.rollback_transaction()?;
                Err(e)
            }
        }
    }

    /// Update a feature by ID.
    pub fn update(
        &self,
        id: i64,
        geometry: &Geometry<f64>,
        properties: &HashMap<String, Value>,
    ) -> Result<()> {
        let wkb = geometry_to_wkb(geometry)?;
        let conn = self.connector.conn();

        let mut set_clauses = vec![format!("{} = ?", self.geometry_column)];
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(wkb)];

        for (key, value) in properties {
            set_clauses.push(format!("{} = ?", key));
            params.push(json_to_sqlite_param(value)?);
        }

        params.push(Box::new(id));

        let sql = format!(
            "UPDATE {} SET {} WHERE id = ?",
            self.table_name,
            set_clauses.join(", ")
        );

        conn.execute(&sql, params_from_iter(params.iter().map(|p| p.as_ref())))?;

        Ok(())
    }

    /// Delete a feature by ID.
    pub fn delete(&self, id: i64) -> Result<()> {
        let conn = self.connector.conn();
        let sql = format!("DELETE FROM {} WHERE id = ?", self.table_name);
        conn.execute(&sql, [id])?;
        Ok(())
    }

    /// Delete features matching a WHERE clause.
    pub fn delete_where(&self, where_clause: &str) -> Result<usize> {
        let conn = self.connector.conn();
        let sql = format!("DELETE FROM {} WHERE {}", self.table_name, where_clause);
        let rows = conn.execute(&sql, [])?;
        Ok(rows)
    }

    /// Truncate the table.
    pub fn truncate(&self) -> Result<()> {
        let conn = self.connector.conn();
        let sql = format!("DELETE FROM {}", self.table_name);
        conn.execute(&sql, [])?;
        Ok(())
    }
}

/// Convert JSON value to SQLite parameter.
fn json_to_sqlite_param(value: &Value) -> Result<Box<dyn rusqlite::ToSql>> {
    match value {
        Value::Null => Ok(Box::new(rusqlite::types::Null)),
        Value::Bool(b) => Ok(Box::new(*b as i64)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Box::new(i))
            } else if let Some(f) = n.as_f64() {
                Ok(Box::new(f))
            } else {
                Err(Error::TypeConversion("Invalid number".to_string()))
            }
        }
        Value::String(s) => Ok(Box::new(s.clone())),
        Value::Array(_) | Value::Object(_) => {
            let json_str = serde_json::to_string(value)?;
            Ok(Box::new(json_str))
        }
    }
}
