//! SQLite spatial data writer.

use crate::error::{Error, Result};
use crate::sqlite::{SqliteConnector, geometry_to_wkb};
use geo_types::Geometry;
use oxisql_core::ToSqlValue;
use serde_json::Value as JsonValue;
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
        properties: &HashMap<String, JsonValue>,
    ) -> Result<i64> {
        let wkb = geometry_to_wkb(geometry)?;
        let mut columns = vec![self.geometry_column.clone()];
        let mut prop_keys: Vec<String> = properties.keys().cloned().collect();
        prop_keys.sort(); // deterministic order

        for key in &prop_keys {
            columns.push(key.clone());
        }

        let placeholders: Vec<String> = (1..=columns.len()).map(|i| format!("${}", i)).collect();

        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            self.table_name,
            columns.join(", "),
            placeholders.join(", ")
        );

        // Build values as owned Vec<OxiSqlParam>
        let mut params: Vec<OxiSqlParam> = vec![OxiSqlParam::Blob(wkb)];
        for key in &prop_keys {
            if let Some(val) = properties.get(key) {
                params.push(json_to_param(val)?);
            }
        }

        let param_refs: Vec<&dyn ToSqlValue> =
            params.iter().map(|p| p as &dyn ToSqlValue).collect();

        // The `INSERT` and the subsequent `SELECT last_insert_rowid()` must be
        // atomic with respect to any other write on this shared connection —
        // otherwise a concurrent insert could complete between the two
        // statements and we would return the wrong rowid. Hold the connector's
        // write lock across both.
        let _guard = self.connector.lock_writes();

        self.connector
            .blocking_conn()
            .execute(&sql, &param_refs)
            .map_err(|e| Error::Query(e.to_string()))?;

        // Get last inserted rowid
        let rows = self
            .connector
            .blocking_conn()
            .query("SELECT last_insert_rowid()", &[])
            .map_err(|e| Error::Query(e.to_string()))?;

        rows.first()
            .and_then(|row| row.get_by_index(0))
            .and_then(|v| {
                if let oxisql_core::Value::I64(n) = v {
                    Some(*n)
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                Error::Query("failed to read last_insert_rowid() after INSERT".to_string())
            })
    }

    /// Insert multiple features in batch using transaction SQL statements.
    pub fn insert_batch(
        &self,
        features: &[(Geometry<f64>, HashMap<String, JsonValue>)],
    ) -> Result<Vec<i64>> {
        if features.is_empty() {
            return Ok(Vec::new());
        }

        let mut ids = Vec::with_capacity(features.len());
        self.connector.begin_transaction()?;

        let result = (|| -> std::result::Result<(), Error> {
            for chunk in features.chunks(self.batch_size) {
                for (geometry, properties) in chunk {
                    let id = self.insert(geometry, properties)?;
                    ids.push(id);
                }
            }
            Ok(())
        })();

        match result {
            Ok(()) => {
                self.connector.commit_transaction()?;
                Ok(ids)
            }
            Err(e) => {
                let _ = self.connector.rollback_transaction();
                Err(e)
            }
        }
    }

    /// Update a feature by ID.
    pub fn update(
        &self,
        id: i64,
        geometry: &Geometry<f64>,
        properties: &HashMap<String, JsonValue>,
    ) -> Result<()> {
        let wkb = geometry_to_wkb(geometry)?;

        let mut set_clauses = vec![format!("{} = $1", self.geometry_column)];
        let mut params: Vec<OxiSqlParam> = vec![OxiSqlParam::Blob(wkb)];

        let mut prop_keys: Vec<String> = properties.keys().cloned().collect();
        prop_keys.sort();

        for (i, key) in prop_keys.iter().enumerate() {
            set_clauses.push(format!("{} = ${}", key, i + 2));
            if let Some(val) = properties.get(key) {
                params.push(json_to_param(val)?);
            }
        }

        // id is the last parameter
        let id_param_idx = params.len() + 1;
        params.push(OxiSqlParam::I64(id));

        let sql = format!(
            "UPDATE {} SET {} WHERE id = ${}",
            self.table_name,
            set_clauses.join(", "),
            id_param_idx
        );

        let param_refs: Vec<&dyn ToSqlValue> =
            params.iter().map(|p| p as &dyn ToSqlValue).collect();

        self.connector
            .blocking_conn()
            .execute(&sql, &param_refs)
            .map_err(|e| Error::Query(e.to_string()))?;

        Ok(())
    }

    /// Delete a feature by ID.
    pub fn delete(&self, id: i64) -> Result<()> {
        let sql = format!("DELETE FROM {} WHERE id = $1", self.table_name);
        let id_ref: &i64 = &id;
        self.connector
            .blocking_conn()
            .execute(&sql, &[id_ref as &dyn ToSqlValue])
            .map_err(|e| Error::Query(e.to_string()))?;
        Ok(())
    }

    /// Delete features matching a WHERE clause.
    pub fn delete_where(&self, where_clause: &str) -> Result<usize> {
        let sql = format!("DELETE FROM {} WHERE {}", self.table_name, where_clause);
        let affected = self
            .connector
            .blocking_conn()
            .execute(&sql, &[])
            .map_err(|e| Error::Query(e.to_string()))?;
        Ok(affected as usize)
    }

    /// Truncate the table.
    pub fn truncate(&self) -> Result<()> {
        let sql = format!("DELETE FROM {}", self.table_name);
        self.connector
            .blocking_conn()
            .execute(&sql, &[])
            .map_err(|e| Error::Query(e.to_string()))?;
        Ok(())
    }
}

/// An owned parameter value for dynamic SQL binding.
enum OxiSqlParam {
    Null,
    I64(i64),
    F64(f64),
    Text(String),
    Blob(Vec<u8>),
    Bool(bool),
}

impl ToSqlValue for OxiSqlParam {
    fn to_value(&self) -> oxisql_core::Value {
        match self {
            OxiSqlParam::Null => oxisql_core::Value::Null,
            OxiSqlParam::I64(n) => oxisql_core::Value::I64(*n),
            OxiSqlParam::F64(f) => oxisql_core::Value::F64(*f),
            OxiSqlParam::Text(s) => oxisql_core::Value::Text(s.clone()),
            OxiSqlParam::Blob(b) => oxisql_core::Value::Blob(b.clone()),
            OxiSqlParam::Bool(b) => oxisql_core::Value::Bool(*b),
        }
    }
}

/// Convert JSON value to an owned OxiSqlParam.
fn json_to_param(value: &JsonValue) -> Result<OxiSqlParam> {
    match value {
        JsonValue::Null => Ok(OxiSqlParam::Null),
        JsonValue::Bool(b) => Ok(OxiSqlParam::Bool(*b)),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(OxiSqlParam::I64(i))
            } else if let Some(f) = n.as_f64() {
                Ok(OxiSqlParam::F64(f))
            } else {
                Err(Error::TypeConversion("Invalid number".to_string()))
            }
        }
        JsonValue::String(s) => Ok(OxiSqlParam::Text(s.clone())),
        JsonValue::Array(_) | JsonValue::Object(_) => {
            let json_str = serde_json::to_string(value)?;
            Ok(OxiSqlParam::Text(json_str))
        }
    }
}
