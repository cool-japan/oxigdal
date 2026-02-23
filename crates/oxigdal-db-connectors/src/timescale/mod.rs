//! TimescaleDB connector for time-series geospatial data.
//!
//! Builds on PostgreSQL/PostGIS with TimescaleDB extensions for
//! efficient time-series storage and querying.

pub mod continuous_agg;
pub mod hypertable;

use crate::error::{Error, Result};
use deadpool_postgres::{Config as PoolConfig, ManagerConfig, Pool, RecyclingMethod, Runtime};
use std::time::Duration;
use tokio_postgres::NoTls;

/// TimescaleDB connector configuration.
#[derive(Debug, Clone)]
pub struct TimescaleConfig {
    /// Database host.
    pub host: String,
    /// Database port.
    pub port: u16,
    /// Database name.
    pub database: String,
    /// Username.
    pub username: String,
    /// Password.
    pub password: String,
    /// Maximum pool size.
    pub max_connections: usize,
    /// Connection timeout.
    pub connection_timeout: Duration,
}

impl Default for TimescaleConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5432,
            database: "timescale".to_string(),
            username: "postgres".to_string(),
            password: String::new(),
            max_connections: 10,
            connection_timeout: Duration::from_secs(30),
        }
    }
}

/// TimescaleDB connector.
pub struct TimescaleConnector {
    pool: Pool,
    #[allow(dead_code)]
    config: TimescaleConfig,
}

impl TimescaleConnector {
    /// Create a new TimescaleDB connector.
    pub fn new(config: TimescaleConfig) -> Result<Self> {
        let mut pg_config = PoolConfig::new();
        pg_config.host = Some(config.host.clone());
        pg_config.port = Some(config.port);
        pg_config.dbname = Some(config.database.clone());
        pg_config.user = Some(config.username.clone());
        pg_config.password = Some(config.password.clone());

        // Note: ManagerConfig with recycling_method is configured internally by deadpool_postgres
        let _manager_config = ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        };

        let pool = pg_config
            .create_pool(Some(Runtime::Tokio1), NoTls)
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(Self { pool, config })
    }

    /// Get a connection from the pool.
    pub async fn get_conn(&self) -> Result<deadpool_postgres::Object> {
        self.pool
            .get()
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))
    }

    /// Check if the connection is healthy.
    pub async fn health_check(&self) -> Result<bool> {
        let client = self.get_conn().await?;
        let row = client
            .query_one("SELECT 1", &[])
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        let result: i32 = row.get(0);
        Ok(result == 1)
    }

    /// Get TimescaleDB version.
    pub async fn version(&self) -> Result<String> {
        let client = self.get_conn().await?;
        let row = client
            .query_one(
                "SELECT extversion FROM pg_extension WHERE extname='timescaledb'",
                &[],
            )
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(row.get(0))
    }

    /// Check if TimescaleDB is installed.
    pub async fn is_timescale_installed(&self) -> Result<bool> {
        let client = self.get_conn().await?;
        let row = client
            .query_one(
                "SELECT EXISTS(SELECT 1 FROM pg_extension WHERE extname='timescaledb')",
                &[],
            )
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(row.get(0))
    }

    /// Create a hypertable.
    pub async fn create_hypertable(
        &self,
        table_name: &str,
        time_column: &str,
        chunk_time_interval: Option<&str>,
    ) -> Result<()> {
        let client = self.get_conn().await?;

        let interval = chunk_time_interval.unwrap_or("1 day");

        let sql = format!(
            "SELECT create_hypertable('{}', '{}', chunk_time_interval => INTERVAL '{}')",
            table_name, time_column, interval
        );

        client
            .execute(&sql, &[])
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(())
    }

    /// Check if a table is a hypertable.
    pub async fn is_hypertable(&self, table_name: &str) -> Result<bool> {
        let client = self.get_conn().await?;

        let row = client
            .query_one(
                "SELECT EXISTS(SELECT 1 FROM timescaledb_information.hypertables WHERE hypertable_name = $1)",
                &[&table_name],
            )
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(row.get(0))
    }

    /// Add a retention policy to a hypertable.
    pub async fn add_retention_policy(
        &self,
        table_name: &str,
        retention_interval: &str,
    ) -> Result<()> {
        let client = self.get_conn().await?;

        let sql = format!(
            "SELECT add_retention_policy('{}', INTERVAL '{}')",
            table_name, retention_interval
        );

        client
            .execute(&sql, &[])
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(())
    }

    /// Remove retention policy from a hypertable.
    pub async fn remove_retention_policy(&self, table_name: &str) -> Result<()> {
        let client = self.get_conn().await?;

        let sql = format!("SELECT remove_retention_policy('{}')", table_name);

        client
            .execute(&sql, &[])
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(())
    }

    /// Enable compression on a hypertable.
    pub async fn enable_compression(
        &self,
        table_name: &str,
        segment_by: Option<&[&str]>,
    ) -> Result<()> {
        let client = self.get_conn().await?;

        let segment_clause = if let Some(cols) = segment_by {
            format!(", segment_by => '{}'", cols.join(","))
        } else {
            String::new()
        };

        let sql = format!(
            "ALTER TABLE {} SET (timescaledb.compress{})",
            table_name, segment_clause
        );

        client
            .execute(&sql, &[])
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(())
    }

    /// Add compression policy to a hypertable.
    pub async fn add_compression_policy(
        &self,
        table_name: &str,
        compress_after: &str,
    ) -> Result<()> {
        let client = self.get_conn().await?;

        let sql = format!(
            "SELECT add_compression_policy('{}', INTERVAL '{}')",
            table_name, compress_after
        );

        client
            .execute(&sql, &[])
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(())
    }

    /// Get hypertable statistics.
    pub async fn hypertable_stats(&self, table_name: &str) -> Result<HypertableStats> {
        let client = self.get_conn().await?;

        let row = client
            .query_one(
                "SELECT * FROM timescaledb_information.hypertables WHERE hypertable_name = $1",
                &[&table_name],
            )
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        let hypertable_schema: String = row.get("hypertable_schema");
        let hypertable_name: String = row.get("hypertable_name");
        let num_dimensions: i32 = row.get("num_dimensions");

        Ok(HypertableStats {
            schema: hypertable_schema,
            name: hypertable_name,
            num_dimensions,
        })
    }

    /// List all hypertables.
    pub async fn list_hypertables(&self) -> Result<Vec<String>> {
        let client = self.get_conn().await?;

        let rows = client
            .query(
                "SELECT hypertable_name FROM timescaledb_information.hypertables",
                &[],
            )
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))?;

        Ok(rows.iter().map(|row| row.get(0)).collect())
    }

    /// Execute raw SQL.
    pub async fn execute(&self, sql: &str) -> Result<u64> {
        let client = self.get_conn().await?;
        client
            .execute(sql, &[])
            .await
            .map_err(|e| Error::TimescaleDB(e.to_string()))
    }
}

/// Hypertable statistics.
#[derive(Debug, Clone)]
pub struct HypertableStats {
    /// Schema name.
    pub schema: String,
    /// Hypertable name.
    pub name: String,
    /// Number of dimensions.
    pub num_dimensions: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = TimescaleConfig::default();
        assert_eq!(config.database, "timescale");
        assert_eq!(config.port, 5432);
    }
}
