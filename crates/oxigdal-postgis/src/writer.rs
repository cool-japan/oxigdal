//! PostGIS writer for inserting features into database
//!
//! This module provides functionality to write features to PostGIS tables.

use crate::connection::ConnectionPool;
use crate::copy_binary::{CopyBinaryEncoder, ewkb_from_wkb};
use crate::error::{QueryError, Result};
use crate::sql::{ColumnName, TableName};
use crate::types::to_postgis;
use crate::wkb::WkbEncoder;
use futures::SinkExt;
use oxigdal_core::vector::feature::Feature;
use tracing::{debug, info, warn};

/// PostGIS feature writer
pub struct PostGisWriter {
    pool: ConnectionPool,
    table_name: String,
    geometry_column: String,
    srid: Option<i32>,
    create_table: bool,
    batch: Vec<Feature>,
    batch_size: usize,
}

impl PostGisWriter {
    /// Creates a new PostGIS writer
    pub fn new(pool: ConnectionPool, table_name: impl Into<String>) -> Self {
        Self {
            pool,
            table_name: table_name.into(),
            geometry_column: "geom".to_string(),
            srid: Some(4326),
            create_table: false,
            batch: Vec::new(),
            batch_size: 1000,
        }
    }

    /// Sets the geometry column name
    pub fn geometry_column(mut self, column: impl Into<String>) -> Self {
        self.geometry_column = column.into();
        self
    }

    /// Sets the SRID
    pub const fn srid(mut self, srid: i32) -> Self {
        self.srid = Some(srid);
        self
    }

    /// Enables automatic table creation
    pub const fn create_table(mut self, create: bool) -> Self {
        self.create_table = create;
        self
    }

    /// Sets the batch size for batch insertions
    pub const fn batch_size(mut self, size: usize) -> Self {
        self.batch_size = size;
        self
    }

    /// Creates the table if it doesn't exist
    pub async fn ensure_table(&self) -> Result<()> {
        let client = self.pool.get().await?;

        let table = TableName::new(&self.table_name)?;
        let geom_col = ColumnName::new(&self.geometry_column)?;

        // Create table
        let create_sql = format!(
            "CREATE TABLE IF NOT EXISTS {} (id SERIAL PRIMARY KEY, {} geometry, properties jsonb)",
            table.qualified(),
            geom_col.quoted()
        );

        debug!("Creating table: {create_sql}");

        client
            .execute(&create_sql, &[])
            .await
            .map_err(|e| QueryError::ExecutionFailed {
                message: e.to_string(),
            })?;

        // Add spatial index
        let index_sql = format!(
            "CREATE INDEX IF NOT EXISTS {}_{}_gist ON {} USING GIST ({})",
            self.table_name,
            self.geometry_column,
            table.qualified(),
            geom_col.quoted()
        );

        client
            .execute(&index_sql, &[])
            .await
            .map_err(|e| QueryError::ExecutionFailed {
                message: e.to_string(),
            })?;

        info!("Table {} created successfully", self.table_name);

        Ok(())
    }

    /// Inserts a single feature
    pub async fn insert(&mut self, feature: &Feature) -> Result<i64> {
        if self.create_table {
            self.ensure_table().await?;
        }

        let client = self.pool.get().await?;

        let geometry = feature
            .geometry
            .as_ref()
            .ok_or_else(|| QueryError::ExecutionFailed {
                message: "Feature has no geometry".to_string(),
            })?;

        let postgis_geom = to_postgis(geometry.clone(), self.srid);
        let properties =
            serde_json::to_value(&feature.properties).map_err(|e| QueryError::ExecutionFailed {
                message: e.to_string(),
            })?;

        let table = TableName::new(&self.table_name)?;
        let geom_col = ColumnName::new(&self.geometry_column)?;

        let sql = format!(
            "INSERT INTO {} ({}, properties) VALUES ($1, $2) RETURNING id",
            table.qualified(),
            geom_col.quoted()
        );

        let row = client
            .query_one(&sql, &[&postgis_geom, &properties])
            .await
            .map_err(|e| QueryError::ExecutionFailed {
                message: e.to_string(),
            })?;

        let id: i64 = row.get(0);
        Ok(id)
    }

    /// Adds a feature to the batch
    pub fn add_to_batch(&mut self, feature: Feature) {
        self.batch.push(feature);
    }

    /// Flushes the batch to the database.
    ///
    /// The fast path streams the whole batch through the PostgreSQL binary COPY
    /// protocol (`COPY ... FROM STDIN WITH (FORMAT binary)`), which avoids one
    /// round-trip per feature. If the server rejects the binary COPY for any
    /// reason the writer transparently degrades to the per-row INSERT path via
    /// `flush_via_inserts`, guaranteeing no data
    /// loss. On success the batch is cleared.
    pub async fn flush(&mut self) -> Result<usize> {
        if self.batch.is_empty() {
            return Ok(0);
        }

        if self.create_table {
            self.ensure_table().await?;
        }

        debug!("Flushing batch of {} features", self.batch.len());

        // Fast path: stream the batch through the binary COPY protocol.
        match self.flush_via_binary_copy().await {
            Ok(count) => {
                self.batch.clear();
                Ok(count)
            }
            Err(e) => {
                // Graceful degradation: a server that rejects binary COPY must
                // not cause data loss — replay the batch with per-row INSERTs.
                warn!("binary COPY failed ({e}); falling back to per-row INSERT");
                let count = self.flush_via_inserts().await?;
                self.batch.clear();
                Ok(count)
            }
        }
    }

    /// Streams the current batch to the database using the PostgreSQL binary
    /// COPY protocol.
    ///
    /// Builds a single binary-COPY payload for every feature that carries a
    /// geometry — field 1 is the geometry as EWKB (WKB with the SRID flag set),
    /// field 2 is the feature's properties serialised as a JSON document. The
    /// payload is sent to a `COPY ... FROM STDIN WITH (FORMAT binary)` sink.
    async fn flush_via_binary_copy(&self) -> Result<usize> {
        let client = self.pool.get().await?;

        let table = TableName::new(&self.table_name)?;
        let geom_col = ColumnName::new(&self.geometry_column)?;

        let copy_sql = format!(
            "COPY {} ({}, properties) FROM STDIN WITH (FORMAT binary)",
            table.qualified(),
            geom_col.quoted()
        );

        // Build the full binary-COPY payload for every feature that has a
        // geometry. PostGIS accepts EWKB for a `geometry` column and accepts
        // the UTF-8 text bytes of a JSON document for a `jsonb` column.
        let mut encoder = CopyBinaryEncoder::new();
        let mut count = 0usize;
        for feature in &self.batch {
            let Some(ref geometry) = feature.geometry else {
                continue;
            };

            // `WkbEncoder::new()` emits plain WKB; promote it to EWKB so the
            // SRID travels with the geometry. When `self.srid` is `None` we
            // still emit plain WKB (PostGIS will assume SRID 0).
            let mut wkb_encoder = WkbEncoder::new();
            let wkb = wkb_encoder.encode(geometry)?;
            let geom_field = match self.srid {
                Some(srid) => ewkb_from_wkb(&wkb, srid)?,
                None => wkb,
            };

            let properties = serde_json::to_value(&feature.properties).map_err(|e| {
                QueryError::ExecutionFailed {
                    message: e.to_string(),
                }
            })?;
            let properties_bytes =
                serde_json::to_vec(&properties).map_err(|e| QueryError::ExecutionFailed {
                    message: e.to_string(),
                })?;

            encoder.begin_row(2);
            encoder.write_field_bytes(&geom_field);
            encoder.write_field_bytes(&properties_bytes);
            count += 1;
        }

        let payload = encoder.finish();

        // `CopyInSink` is `!Unpin` — pin it on the stack before driving the
        // `Sink`. `bytes::Bytes` implements `Buf`, the bound `copy_in`
        // requires for the streamed item type.
        let sink = client
            .copy_in::<str, bytes::Bytes>(&copy_sql)
            .await
            .map_err(|e| QueryError::ExecutionFailed {
                message: e.to_string(),
            })?;
        tokio::pin!(sink);

        sink.as_mut()
            .send(bytes::Bytes::from(payload))
            .await
            .map_err(|e| QueryError::ExecutionFailed {
                message: e.to_string(),
            })?;

        sink.finish()
            .await
            .map_err(|e| QueryError::ExecutionFailed {
                message: e.to_string(),
            })?;

        Ok(count)
    }

    /// Flushes the current batch with one parameterised `INSERT` per feature.
    ///
    /// This is the graceful-degradation fallback used by [`flush`](Self::flush)
    /// when the binary COPY path fails. It does not clear `self.batch`; the
    /// caller is responsible for that.
    async fn flush_via_inserts(&self) -> Result<usize> {
        let client = self.pool.get().await?;

        let table = TableName::new(&self.table_name)?;
        let geom_col = ColumnName::new(&self.geometry_column)?;

        let mut count = 0;
        for feature in &self.batch {
            if let Some(ref geometry) = feature.geometry {
                let postgis_geom = to_postgis(geometry.clone(), self.srid);
                let properties = serde_json::to_value(&feature.properties).map_err(|e| {
                    QueryError::ExecutionFailed {
                        message: e.to_string(),
                    }
                })?;

                let sql = format!(
                    "INSERT INTO {} ({}, properties) VALUES ($1, $2)",
                    table.qualified(),
                    geom_col.quoted()
                );

                client
                    .execute(&sql, &[&postgis_geom, &properties])
                    .await
                    .map_err(|e| QueryError::ExecutionFailed {
                        message: e.to_string(),
                    })?;

                count += 1;
            }
        }

        Ok(count)
    }

    /// Updates a feature by ID
    pub async fn update(&self, id: i64, feature: &Feature) -> Result<u64> {
        let client = self.pool.get().await?;

        let geometry = feature
            .geometry
            .as_ref()
            .ok_or_else(|| QueryError::ExecutionFailed {
                message: "Feature has no geometry".to_string(),
            })?;

        let postgis_geom = to_postgis(geometry.clone(), self.srid);
        let properties =
            serde_json::to_value(&feature.properties).map_err(|e| QueryError::ExecutionFailed {
                message: e.to_string(),
            })?;

        let table = TableName::new(&self.table_name)?;
        let geom_col = ColumnName::new(&self.geometry_column)?;

        let sql = format!(
            "UPDATE {} SET {} = $1, properties = $2 WHERE id = $3",
            table.qualified(),
            geom_col.quoted()
        );

        let rows_affected = client
            .execute(&sql, &[&postgis_geom, &properties, &id])
            .await
            .map_err(|e| QueryError::ExecutionFailed {
                message: e.to_string(),
            })?;

        Ok(rows_affected)
    }

    /// Deletes a feature by ID
    pub async fn delete(&self, id: i64) -> Result<u64> {
        let client = self.pool.get().await?;

        let table = TableName::new(&self.table_name)?;

        let sql = format!("DELETE FROM {} WHERE id = $1", table.qualified());

        let rows_affected =
            client
                .execute(&sql, &[&id])
                .await
                .map_err(|e| QueryError::ExecutionFailed {
                    message: e.to_string(),
                })?;

        Ok(rows_affected)
    }

    /// Truncates the table
    pub async fn truncate(&self) -> Result<()> {
        let client = self.pool.get().await?;

        let table = TableName::new(&self.table_name)?;

        let sql = format!("TRUNCATE TABLE {}", table.qualified());

        client
            .execute(&sql, &[])
            .await
            .map_err(|e| QueryError::ExecutionFailed {
                message: e.to_string(),
            })?;

        info!("Table {} truncated", self.table_name);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::ConnectionConfig;

    #[test]
    fn test_writer_creation() {
        let config = ConnectionConfig::default();
        let pool = ConnectionPool::new(config).ok();
        assert!(pool.is_some());

        let pool = pool.expect("pool creation failed");
        let writer = PostGisWriter::new(pool, "test_table");

        assert_eq!(writer.table_name, "test_table");
        assert_eq!(writer.geometry_column, "geom");
        assert_eq!(writer.srid, Some(4326));
    }

    #[test]
    fn test_writer_configuration() {
        let config = ConnectionConfig::default();
        let pool = ConnectionPool::new(config).ok();
        assert!(pool.is_some());

        let pool = pool.expect("pool creation failed");
        let writer = PostGisWriter::new(pool, "test_table")
            .geometry_column("the_geom")
            .srid(3857)
            .create_table(true)
            .batch_size(500);

        assert_eq!(writer.geometry_column, "the_geom");
        assert_eq!(writer.srid, Some(3857));
        assert!(writer.create_table);
        assert_eq!(writer.batch_size, 500);
    }
}
