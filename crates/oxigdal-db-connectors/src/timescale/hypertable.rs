//! TimescaleDB hypertable operations.

use crate::error::{Error, Result};
use crate::timescale::TimescaleConnector;
use chrono::{DateTime, Utc};
use tokio_postgres::types::ToSql;

/// Hypertable manager for spatial-temporal data.
pub struct HypertableManager {
    connector: TimescaleConnector,
    table_name: String,
    time_column: String,
    geometry_column: String,
}

impl HypertableManager {
    /// Create a new hypertable manager.
    pub fn new(
        connector: TimescaleConnector,
        table_name: String,
        time_column: String,
        geometry_column: String,
    ) -> Self {
        Self {
            connector,
            table_name,
            time_column,
            geometry_column,
        }
    }

    /// Create the table and convert to hypertable.
    pub async fn create(
        &self,
        additional_columns: &[(String, String)],
        chunk_time_interval: Option<&str>,
    ) -> Result<()> {
        let client = self.connector.get_conn().await?;

        // Create the base table
        let mut columns = vec![
            "id BIGSERIAL".to_string(),
            format!("{} TIMESTAMPTZ NOT NULL", self.time_column),
            format!("{} GEOMETRY(POINT, 4326)", self.geometry_column),
        ];

        for (col_name, col_type) in additional_columns {
            columns.push(format!("{} {}", col_name, col_type));
        }

        let create_sql = format!(
            "CREATE TABLE IF NOT EXISTS {} ({})",
            self.table_name,
            columns.join(", ")
        );

        client
            .execute(&create_sql, &[])
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        // Convert to hypertable
        self.connector
            .create_hypertable(&self.table_name, &self.time_column, chunk_time_interval)
            .await?;

        // Create spatial index
        let index_sql = format!(
            "CREATE INDEX IF NOT EXISTS {}_geom_idx ON {} USING GIST ({})",
            self.table_name, self.table_name, self.geometry_column
        );

        client
            .execute(&index_sql, &[])
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        // Create time index
        let time_index_sql = format!(
            "CREATE INDEX IF NOT EXISTS {}_time_idx ON {} ({})",
            self.table_name, self.table_name, self.time_column
        );

        client
            .execute(&time_index_sql, &[])
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(())
    }

    /// Insert a spatial-temporal record.
    pub async fn insert(
        &self,
        time: DateTime<Utc>,
        x: f64,
        y: f64,
        properties: &[(String, &(dyn ToSql + Sync))],
    ) -> Result<i64> {
        let client = self.connector.get_conn().await?;

        let mut columns = vec![self.time_column.clone(), self.geometry_column.clone()];
        let mut placeholders = vec![
            "$1".to_string(),
            "ST_SetSRID(ST_MakePoint($2, $3), 4326)".to_string(),
        ];
        for (param_index, (col_name, _)) in (4..).zip(properties.iter()) {
            columns.push(col_name.clone());
            placeholders.push(format!("${}", param_index));
        }

        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({}) RETURNING id",
            self.table_name,
            columns.join(", "),
            placeholders.join(", ")
        );

        let mut params: Vec<&(dyn ToSql + Sync)> = vec![&time, &x, &y];
        for (_, value) in properties {
            params.push(*value);
        }

        let row = client
            .query_one(&sql, &params)
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(row.get(0))
    }

    /// Query data within a time range and bounding box.
    pub async fn query_spatiotemporal(
        &self,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
    ) -> Result<Vec<tokio_postgres::Row>> {
        let client = self.connector.get_conn().await?;

        let sql = format!(
            "SELECT * FROM {} WHERE {} >= $1 AND {} < $2 AND ST_Intersects({}, ST_MakeEnvelope($3, $4, $5, $6, 4326))",
            self.table_name, self.time_column, self.time_column, self.geometry_column
        );

        let rows = client
            .query(
                &sql,
                &[&start_time, &end_time, &min_x, &min_y, &max_x, &max_y],
            )
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(rows)
    }

    /// Get aggregated statistics over time buckets.
    pub async fn time_bucket_aggregate(
        &self,
        bucket_interval: &str,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<Vec<tokio_postgres::Row>> {
        let client = self.connector.get_conn().await?;

        let sql = format!(
            "SELECT time_bucket(INTERVAL '{}', {}) as bucket, count(*) as count FROM {} WHERE {} >= $1 AND {} < $2 GROUP BY bucket ORDER BY bucket",
            bucket_interval, self.time_column, self.table_name, self.time_column, self.time_column
        );

        let rows = client
            .query(&sql, &[&start_time, &end_time])
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(rows)
    }

    /// Setup retention policy.
    pub async fn setup_retention(&self, retention_interval: &str) -> Result<()> {
        self.connector
            .add_retention_policy(&self.table_name, retention_interval)
            .await
    }

    /// Setup compression policy.
    pub async fn setup_compression(
        &self,
        compress_after: &str,
        segment_by: Option<&[&str]>,
    ) -> Result<()> {
        self.connector
            .enable_compression(&self.table_name, segment_by)
            .await?;
        self.connector
            .add_compression_policy(&self.table_name, compress_after)
            .await
    }
}
