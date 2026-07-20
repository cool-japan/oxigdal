//! TimescaleDB continuous aggregates for real-time analytics.

use crate::error::{Error, Result};
use crate::timescale::TimescaleConnector;

/// Continuous aggregate manager.
pub struct ContinuousAggregateManager {
    connector: TimescaleConnector,
    view_name: String,
    source_table: String,
}

impl ContinuousAggregateManager {
    /// Create a new continuous aggregate manager.
    pub fn new(connector: TimescaleConnector, view_name: String, source_table: String) -> Self {
        Self {
            connector,
            view_name,
            source_table,
        }
    }

    /// Create a continuous aggregate for spatial-temporal data.
    pub async fn create(
        &self,
        time_column: &str,
        bucket_interval: &str,
        aggregate_query: &str,
    ) -> Result<()> {
        let client = self.connector.get_conn().await?;

        let sql = format!(
            "CREATE MATERIALIZED VIEW {} WITH (timescaledb.continuous) AS SELECT time_bucket(INTERVAL '{}', {}) as bucket, {} FROM {} GROUP BY bucket",
            self.view_name, bucket_interval, time_column, aggregate_query, self.source_table
        );

        client
            .execute(&sql, &[])
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(())
    }

    /// Add a refresh policy to the continuous aggregate.
    pub async fn add_refresh_policy(
        &self,
        start_offset: &str,
        end_offset: &str,
        schedule_interval: &str,
    ) -> Result<()> {
        let client = self.connector.get_conn().await?;

        let sql = format!(
            "SELECT add_continuous_aggregate_policy('{}', start_offset => INTERVAL '{}', end_offset => INTERVAL '{}', schedule_interval => INTERVAL '{}')",
            self.view_name, start_offset, end_offset, schedule_interval
        );

        client
            .execute(&sql, &[])
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(())
    }

    /// Refresh the continuous aggregate.
    pub async fn refresh(&self, start_time: Option<&str>, end_time: Option<&str>) -> Result<()> {
        let client = self.connector.get_conn().await?;

        let time_clause = match (start_time, end_time) {
            (Some(start), Some(end)) => format!(", '{}', '{}'", start, end),
            _ => String::new(),
        };

        let sql = format!(
            "CALL refresh_continuous_aggregate('{}'{}) ",
            self.view_name, time_clause
        );

        client
            .execute(&sql, &[])
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(())
    }

    /// Drop the continuous aggregate.
    pub async fn drop(&self) -> Result<()> {
        let client = self.connector.get_conn().await?;

        let sql = format!(
            "DROP MATERIALIZED VIEW IF EXISTS {} CASCADE",
            self.view_name
        );

        client
            .execute(&sql, &[])
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(())
    }

    /// Query the continuous aggregate.
    pub async fn query(&self, where_clause: Option<&str>) -> Result<Vec<tokio_postgres::Row>> {
        let client = self.connector.get_conn().await?;

        let sql = if let Some(clause) = where_clause {
            format!("SELECT * FROM {} WHERE {}", self.view_name, clause)
        } else {
            format!("SELECT * FROM {}", self.view_name)
        };

        let rows = client
            .query(&sql, &[])
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(rows)
    }
}
