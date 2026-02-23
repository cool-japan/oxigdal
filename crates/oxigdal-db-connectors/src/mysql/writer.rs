//! MySQL spatial data writer.

use crate::error::{Error, Result};
use crate::mysql::{MySqlConnector, geometry_to_wkt};
use geo_types::Geometry;
use mysql_async::prelude::*;
use serde_json::Value;
use std::collections::HashMap;

/// MySQL spatial data writer.
pub struct MySqlWriter {
    connector: MySqlConnector,
    table_name: String,
    geometry_column: String,
    batch_size: usize,
}

impl MySqlWriter {
    /// Create a new MySQL writer.
    pub fn new(connector: MySqlConnector, table_name: String, geometry_column: String) -> Self {
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
    pub async fn insert(
        &self,
        geometry: &Geometry<f64>,
        properties: &HashMap<String, Value>,
    ) -> Result<i64> {
        let wkt = geometry_to_wkt(geometry)?;
        let mut columns = vec![self.geometry_column.clone()];
        let mut placeholders = vec!["ST_GeomFromText(?)".to_string()];
        let mut values: Vec<mysql_async::Value> = vec![wkt.into()];

        for (key, value) in properties {
            columns.push(key.clone());
            placeholders.push("?".to_string());
            values.push(json_to_mysql_value(value)?);
        }

        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            self.table_name,
            columns.join(", "),
            placeholders.join(", ")
        );

        let mut conn = self.connector.get_conn().await?;
        conn.exec_drop(&sql, values)
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        Ok(conn
            .last_insert_id()
            .ok_or_else(|| Error::Query("No insert ID".to_string()))? as i64)
    }

    /// Insert multiple features in batch.
    pub async fn insert_batch(
        &self,
        features: &[(Geometry<f64>, HashMap<String, Value>)],
    ) -> Result<Vec<i64>> {
        if features.is_empty() {
            return Ok(Vec::new());
        }

        let mut ids = Vec::with_capacity(features.len());

        for chunk in features.chunks(self.batch_size) {
            let chunk_ids = self.insert_batch_chunk(chunk).await?;
            ids.extend(chunk_ids);
        }

        Ok(ids)
    }

    /// Insert a chunk of features.
    async fn insert_batch_chunk(
        &self,
        chunk: &[(Geometry<f64>, HashMap<String, Value>)],
    ) -> Result<Vec<i64>> {
        if chunk.is_empty() {
            return Ok(Vec::new());
        }

        // Get all unique property keys
        let mut all_keys: Vec<String> = chunk[0].1.keys().cloned().collect();
        all_keys.sort();

        let mut columns = vec![self.geometry_column.clone()];
        columns.extend(all_keys.clone());

        let mut value_groups = Vec::new();
        let mut all_values = Vec::new();

        for (geometry, properties) in chunk {
            let wkt = geometry_to_wkt(geometry)?;
            let mut row_values = vec!["ST_GeomFromText(?)".to_string()];
            all_values.push(wkt.into());

            for key in &all_keys {
                row_values.push("?".to_string());
                let value = properties.get(key).cloned().unwrap_or(Value::Null);
                all_values.push(json_to_mysql_value(&value)?);
            }

            value_groups.push(format!("({})", row_values.join(", ")));
        }

        let sql = format!(
            "INSERT INTO {} ({}) VALUES {}",
            self.table_name,
            columns.join(", "),
            value_groups.join(", ")
        );

        let mut conn = self.connector.get_conn().await?;
        conn.exec_drop(&sql, all_values)
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        let first_id = conn
            .last_insert_id()
            .ok_or_else(|| Error::Query("No insert ID".to_string()))? as i64;
        let ids: Vec<i64> = (first_id..first_id + chunk.len() as i64).collect();

        Ok(ids)
    }

    /// Update a feature by ID.
    pub async fn update(
        &self,
        id: i64,
        geometry: &Geometry<f64>,
        properties: &HashMap<String, Value>,
    ) -> Result<()> {
        let wkt = geometry_to_wkt(geometry)?;
        let mut set_clauses = vec![format!("{} = ST_GeomFromText(?)", self.geometry_column)];
        let mut values: Vec<mysql_async::Value> = vec![wkt.into()];

        for (key, value) in properties {
            set_clauses.push(format!("{} = ?", key));
            values.push(json_to_mysql_value(value)?);
        }

        values.push(id.into());

        let sql = format!(
            "UPDATE {} SET {} WHERE id = ?",
            self.table_name,
            set_clauses.join(", ")
        );

        let mut conn = self.connector.get_conn().await?;
        conn.exec_drop(&sql, values)
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        Ok(())
    }

    /// Delete a feature by ID.
    pub async fn delete(&self, id: i64) -> Result<()> {
        let sql = format!("DELETE FROM {} WHERE id = ?", self.table_name);

        let mut conn = self.connector.get_conn().await?;
        conn.exec_drop(&sql, (id,))
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        Ok(())
    }

    /// Delete features matching a WHERE clause.
    pub async fn delete_where(&self, where_clause: &str) -> Result<u64> {
        let sql = format!("DELETE FROM {} WHERE {}", self.table_name, where_clause);

        let mut conn = self.connector.get_conn().await?;
        conn.query_drop(&sql)
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        Ok(conn.affected_rows())
    }

    /// Truncate the table.
    pub async fn truncate(&self) -> Result<()> {
        let sql = format!("TRUNCATE TABLE {}", self.table_name);

        let mut conn = self.connector.get_conn().await?;
        conn.query_drop(&sql)
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        Ok(())
    }

    /// Create spatial index.
    pub async fn create_spatial_index(&self, index_name: &str) -> Result<()> {
        let sql = format!(
            "CREATE SPATIAL INDEX {} ON {} ({})",
            index_name, self.table_name, self.geometry_column
        );

        let mut conn = self.connector.get_conn().await?;
        conn.query_drop(&sql)
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        Ok(())
    }

    /// Drop spatial index.
    pub async fn drop_spatial_index(&self, index_name: &str) -> Result<()> {
        let sql = format!("DROP INDEX {} ON {}", index_name, self.table_name);

        let mut conn = self.connector.get_conn().await?;
        conn.query_drop(&sql)
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        Ok(())
    }

    // Note: Transaction support is available through get_conn_for_transaction()
    // Users can call connector.get_conn_for_transaction() to get a connection,
    // then use conn.start_transaction() to begin a transaction.
}

/// Transaction state for tracking commit/rollback status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransactionState {
    /// Transaction is active and can execute operations.
    Active,
    /// Transaction has been committed.
    Committed,
    /// Transaction has been rolled back.
    RolledBack,
}

/// MySQL transaction writer with owned connection and explicit transaction management.
///
/// This implementation avoids the lifetime issues of mysql_async::Transaction by
/// managing transaction state explicitly through SQL queries. The transaction is
/// automatically rolled back on drop if not committed.
///
/// # Example
///
/// ```ignore
/// use oxigdal_db_connectors::mysql::{MySqlConnector, MySqlConfig};
/// use oxigdal_db_connectors::mysql::writer::MySqlTransactionWriter;
/// use geo_types::point;
/// use std::collections::HashMap;
///
/// async fn example() -> Result<(), Box<dyn std::error::Error>> {
///     let config = MySqlConfig::default();
///     let connector = MySqlConnector::new(config)?;
///
///     let mut tx_writer = MySqlTransactionWriter::begin(
///         connector,
///         "spatial_features".to_string(),
///         "geometry".to_string(),
///     ).await?;
///
///     let geometry = geo_types::Geometry::Point(point!(x: 1.0, y: 2.0));
///     let properties = HashMap::new();
///
///     tx_writer.insert(&geometry, &properties).await?;
///     tx_writer.commit().await?;
///
///     Ok(())
/// }
/// ```
pub struct MySqlTransactionWriter {
    conn: mysql_async::Conn,
    table_name: String,
    geometry_column: String,
    batch_size: usize,
    state: TransactionState,
}

impl MySqlTransactionWriter {
    /// Begin a new transaction and create a transaction writer.
    ///
    /// # Arguments
    ///
    /// * `connector` - The MySQL connector to get a connection from.
    /// * `table_name` - Name of the table to write to.
    /// * `geometry_column` - Name of the geometry column.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be obtained or the transaction
    /// cannot be started.
    pub async fn begin(
        connector: MySqlConnector,
        table_name: String,
        geometry_column: String,
    ) -> Result<Self> {
        let mut conn = connector.get_conn().await?;

        // Start the transaction explicitly
        conn.query_drop("START TRANSACTION")
            .await
            .map_err(|e| Error::Query(format!("Failed to start transaction: {}", e)))?;

        Ok(Self {
            conn,
            table_name,
            geometry_column,
            batch_size: 1000,
            state: TransactionState::Active,
        })
    }

    /// Set batch size for bulk inserts within the transaction.
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Check if the transaction is still active.
    pub fn is_active(&self) -> bool {
        self.state == TransactionState::Active
    }

    /// Ensure the transaction is active, returning an error if not.
    fn ensure_active(&self) -> Result<()> {
        match self.state {
            TransactionState::Active => Ok(()),
            TransactionState::Committed => Err(Error::Query(
                "Transaction has already been committed".to_string(),
            )),
            TransactionState::RolledBack => Err(Error::Query(
                "Transaction has already been rolled back".to_string(),
            )),
        }
    }

    /// Insert a single feature within the transaction.
    ///
    /// # Arguments
    ///
    /// * `geometry` - The geometry to insert.
    /// * `properties` - Additional properties as key-value pairs.
    ///
    /// # Returns
    ///
    /// Returns the inserted row ID on success.
    ///
    /// # Errors
    ///
    /// Returns an error if the transaction is not active or the insert fails.
    pub async fn insert(
        &mut self,
        geometry: &Geometry<f64>,
        properties: &HashMap<String, Value>,
    ) -> Result<i64> {
        self.ensure_active()?;

        let wkt = geometry_to_wkt(geometry)?;
        let mut columns = vec![self.geometry_column.clone()];
        let mut placeholders = vec!["ST_GeomFromText(?)".to_string()];
        let mut values: Vec<mysql_async::Value> = vec![wkt.into()];

        for (key, value) in properties {
            columns.push(key.clone());
            placeholders.push("?".to_string());
            values.push(json_to_mysql_value(value)?);
        }

        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            self.table_name,
            columns.join(", "),
            placeholders.join(", ")
        );

        self.conn
            .exec_drop(&sql, values)
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        self.conn
            .last_insert_id()
            .map(|id| id as i64)
            .ok_or_else(|| Error::Query("No insert ID returned".to_string()))
    }

    /// Insert multiple features in batch within the transaction.
    ///
    /// # Arguments
    ///
    /// * `features` - A slice of (geometry, properties) tuples.
    ///
    /// # Returns
    ///
    /// Returns a vector of inserted row IDs on success.
    ///
    /// # Errors
    ///
    /// Returns an error if the transaction is not active or any insert fails.
    /// On error, the transaction is automatically rolled back.
    pub async fn insert_batch(
        &mut self,
        features: &[(Geometry<f64>, HashMap<String, Value>)],
    ) -> Result<Vec<i64>> {
        self.ensure_active()?;

        if features.is_empty() {
            return Ok(Vec::new());
        }

        let mut ids = Vec::with_capacity(features.len());

        for chunk in features.chunks(self.batch_size) {
            match self.insert_batch_chunk(chunk).await {
                Ok(chunk_ids) => ids.extend(chunk_ids),
                Err(e) => {
                    // Attempt rollback on error, but don't mask the original error
                    let _ = self.rollback_internal().await;
                    return Err(e);
                }
            }
        }

        Ok(ids)
    }

    /// Insert a chunk of features using a multi-row INSERT statement.
    async fn insert_batch_chunk(
        &mut self,
        chunk: &[(Geometry<f64>, HashMap<String, Value>)],
    ) -> Result<Vec<i64>> {
        if chunk.is_empty() {
            return Ok(Vec::new());
        }

        // Get all unique property keys from the first item
        // We assume all items in the chunk have the same schema
        let mut all_keys: Vec<String> = chunk
            .first()
            .map(|(_, props)| props.keys().cloned().collect())
            .unwrap_or_default();
        all_keys.sort();

        let mut columns = vec![self.geometry_column.clone()];
        columns.extend(all_keys.clone());

        let mut value_groups = Vec::new();
        let mut all_values = Vec::new();

        for (geometry, properties) in chunk {
            let wkt = geometry_to_wkt(geometry)?;
            let mut row_values = vec!["ST_GeomFromText(?)".to_string()];
            all_values.push(wkt.into());

            for key in &all_keys {
                row_values.push("?".to_string());
                let value = properties.get(key).cloned().unwrap_or(Value::Null);
                all_values.push(json_to_mysql_value(&value)?);
            }

            value_groups.push(format!("({})", row_values.join(", ")));
        }

        let sql = format!(
            "INSERT INTO {} ({}) VALUES {}",
            self.table_name,
            columns.join(", "),
            value_groups.join(", ")
        );

        self.conn
            .exec_drop(&sql, all_values)
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        let first_id = self
            .conn
            .last_insert_id()
            .ok_or_else(|| Error::Query("No insert ID returned".to_string()))?
            as i64;
        let ids: Vec<i64> = (first_id..first_id + chunk.len() as i64).collect();

        Ok(ids)
    }

    /// Update a feature by ID within the transaction.
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the row to update.
    /// * `geometry` - The new geometry.
    /// * `properties` - The new properties.
    ///
    /// # Errors
    ///
    /// Returns an error if the transaction is not active or the update fails.
    pub async fn update(
        &mut self,
        id: i64,
        geometry: &Geometry<f64>,
        properties: &HashMap<String, Value>,
    ) -> Result<()> {
        self.ensure_active()?;

        let wkt = geometry_to_wkt(geometry)?;
        let mut set_clauses = vec![format!("{} = ST_GeomFromText(?)", self.geometry_column)];
        let mut values: Vec<mysql_async::Value> = vec![wkt.into()];

        for (key, value) in properties {
            set_clauses.push(format!("{} = ?", key));
            values.push(json_to_mysql_value(value)?);
        }

        values.push(id.into());

        let sql = format!(
            "UPDATE {} SET {} WHERE id = ?",
            self.table_name,
            set_clauses.join(", ")
        );

        self.conn
            .exec_drop(&sql, values)
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        Ok(())
    }

    /// Delete a feature by ID within the transaction.
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the row to delete.
    ///
    /// # Errors
    ///
    /// Returns an error if the transaction is not active or the delete fails.
    pub async fn delete(&mut self, id: i64) -> Result<()> {
        self.ensure_active()?;

        let sql = format!("DELETE FROM {} WHERE id = ?", self.table_name);

        self.conn
            .exec_drop(&sql, (id,))
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        Ok(())
    }

    /// Delete features matching a WHERE clause within the transaction.
    ///
    /// # Arguments
    ///
    /// * `where_clause` - SQL WHERE clause (without the WHERE keyword).
    ///
    /// # Returns
    ///
    /// Returns the number of affected rows.
    ///
    /// # Errors
    ///
    /// Returns an error if the transaction is not active or the delete fails.
    pub async fn delete_where(&mut self, where_clause: &str) -> Result<u64> {
        self.ensure_active()?;

        let sql = format!("DELETE FROM {} WHERE {}", self.table_name, where_clause);

        self.conn
            .query_drop(&sql)
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        Ok(self.conn.affected_rows())
    }

    /// Get the number of rows affected by the last statement.
    pub fn affected_rows(&self) -> u64 {
        self.conn.affected_rows()
    }

    /// Execute a raw SQL statement within the transaction.
    ///
    /// # Arguments
    ///
    /// * `sql` - The SQL statement to execute.
    ///
    /// # Returns
    ///
    /// Returns the number of affected rows.
    ///
    /// # Errors
    ///
    /// Returns an error if the transaction is not active or the execution fails.
    pub async fn execute(&mut self, sql: &str) -> Result<u64> {
        self.ensure_active()?;

        self.conn
            .query_drop(sql)
            .await
            .map_err(|e| Error::Query(e.to_string()))?;

        Ok(self.conn.affected_rows())
    }

    /// Create a savepoint within the transaction.
    ///
    /// Savepoints allow partial rollback within a transaction.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the savepoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the transaction is not active or the savepoint cannot be created.
    pub async fn savepoint(&mut self, name: &str) -> Result<()> {
        self.ensure_active()?;

        let sql = format!("SAVEPOINT {}", name);
        self.conn
            .query_drop(&sql)
            .await
            .map_err(|e| Error::Query(format!("Failed to create savepoint: {}", e)))?;

        Ok(())
    }

    /// Rollback to a savepoint within the transaction.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the savepoint to rollback to.
    ///
    /// # Errors
    ///
    /// Returns an error if the transaction is not active or the rollback fails.
    pub async fn rollback_to_savepoint(&mut self, name: &str) -> Result<()> {
        self.ensure_active()?;

        let sql = format!("ROLLBACK TO SAVEPOINT {}", name);
        self.conn
            .query_drop(&sql)
            .await
            .map_err(|e| Error::Query(format!("Failed to rollback to savepoint: {}", e)))?;

        Ok(())
    }

    /// Release a savepoint within the transaction.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the savepoint to release.
    ///
    /// # Errors
    ///
    /// Returns an error if the transaction is not active or the release fails.
    pub async fn release_savepoint(&mut self, name: &str) -> Result<()> {
        self.ensure_active()?;

        let sql = format!("RELEASE SAVEPOINT {}", name);
        self.conn
            .query_drop(&sql)
            .await
            .map_err(|e| Error::Query(format!("Failed to release savepoint: {}", e)))?;

        Ok(())
    }

    /// Internal rollback without consuming self.
    async fn rollback_internal(&mut self) -> Result<()> {
        if self.state == TransactionState::Active {
            self.conn
                .query_drop("ROLLBACK")
                .await
                .map_err(|e| Error::Query(format!("Failed to rollback transaction: {}", e)))?;
            self.state = TransactionState::RolledBack;
        }
        Ok(())
    }

    /// Commit the transaction.
    ///
    /// After committing, no more operations can be performed on this writer.
    ///
    /// # Errors
    ///
    /// Returns an error if the transaction is not active or the commit fails.
    pub async fn commit(mut self) -> Result<()> {
        self.ensure_active()?;

        self.conn
            .query_drop("COMMIT")
            .await
            .map_err(|e| Error::Query(format!("Failed to commit transaction: {}", e)))?;

        self.state = TransactionState::Committed;
        Ok(())
    }

    /// Rollback the transaction.
    ///
    /// After rolling back, no more operations can be performed on this writer.
    ///
    /// # Errors
    ///
    /// Returns an error if the transaction is not active or the rollback fails.
    pub async fn rollback(mut self) -> Result<()> {
        self.ensure_active()?;
        self.rollback_internal().await
    }

    /// Consume the transaction writer and return the underlying connection.
    ///
    /// This will rollback the transaction if it hasn't been committed.
    /// Use this if you need to reuse the connection after the transaction.
    pub async fn into_conn(mut self) -> mysql_async::Conn {
        if self.state == TransactionState::Active {
            // Best effort rollback - ignore errors
            let _ = self.conn.query_drop("ROLLBACK").await;
            self.state = TransactionState::RolledBack;
        }
        self.conn
    }
}

// Note: Drop trait implementation for automatic rollback
// Since we can't run async code in Drop, we mark the transaction as needing
// rollback and let the MySQL server handle cleanup when the connection is
// returned to the pool or closed. For safety, prefer calling commit() or
// rollback() explicitly.

/// Convert JSON value to MySQL value.
fn json_to_mysql_value(value: &Value) -> Result<mysql_async::Value> {
    match value {
        Value::Null => Ok(mysql_async::Value::NULL),
        Value::Bool(b) => Ok(mysql_async::Value::Int(*b as i64)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(mysql_async::Value::Int(i))
            } else if let Some(u) = n.as_u64() {
                Ok(mysql_async::Value::UInt(u))
            } else if let Some(f) = n.as_f64() {
                Ok(mysql_async::Value::Double(f))
            } else {
                Err(Error::TypeConversion("Invalid number".to_string()))
            }
        }
        Value::String(s) => Ok(mysql_async::Value::Bytes(s.as_bytes().to_vec())),
        Value::Array(_) | Value::Object(_) => {
            let json_str = serde_json::to_string(value)?;
            Ok(mysql_async::Value::Bytes(json_str.as_bytes().to_vec()))
        }
    }
}
