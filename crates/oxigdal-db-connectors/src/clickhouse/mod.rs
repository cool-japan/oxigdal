//! ClickHouse spatial database connector.
//!
//! Provides support for reading and writing spatial data to ClickHouse
//! for massive-scale analytics.

pub mod reader;
pub mod types;
pub mod writer;

use crate::error::{Error, Result};
use clickhouse::Client;
use std::time::Duration;

/// ClickHouse connector configuration.
#[derive(Debug, Clone)]
pub struct ClickHouseConfig {
    /// Database URL.
    pub url: String,
    /// Database name.
    pub database: String,
    /// Username.
    pub username: String,
    /// Password.
    pub password: String,
    /// Connection timeout.
    pub connection_timeout: Duration,
    /// Query timeout.
    pub query_timeout: Duration,
    /// Compression enabled.
    pub compression: bool,
}

impl Default for ClickHouseConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:8123".to_string(),
            database: "default".to_string(),
            username: "default".to_string(),
            password: String::new(),
            connection_timeout: Duration::from_secs(30),
            query_timeout: Duration::from_secs(300),
            compression: true,
        }
    }
}

/// ClickHouse spatial database connector.
pub struct ClickHouseConnector {
    client: Client,
    config: ClickHouseConfig,
}

impl ClickHouseConnector {
    /// Create a new ClickHouse connector.
    pub fn new(config: ClickHouseConfig) -> Result<Self> {
        let client = Client::default()
            .with_url(&config.url)
            .with_database(&config.database)
            .with_user(&config.username)
            .with_password(&config.password);

        Ok(Self { client, config })
    }

    /// Get client reference.
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Check if the connection is healthy.
    pub async fn health_check(&self) -> Result<bool> {
        let result: std::result::Result<u8, clickhouse::error::Error> =
            self.client.query("SELECT 1").fetch_one().await;

        Ok(result.is_ok())
    }

    /// Get database version.
    pub async fn version(&self) -> Result<String> {
        let version: String = self
            .client
            .query("SELECT version()")
            .fetch_one()
            .await
            .map_err(|e| Error::ClickHouse(e.to_string()))?;

        Ok(version)
    }

    /// List all tables.
    pub async fn list_tables(&self) -> Result<Vec<String>> {
        #[derive(Debug, clickhouse::Row, serde::Deserialize)]
        struct TableName {
            name: String,
        }

        let tables: Vec<TableName> = self
            .client
            .query("SELECT name FROM system.tables WHERE database = ?")
            .bind(&self.config.database)
            .fetch_all()
            .await
            .map_err(|e| Error::ClickHouse(e.to_string()))?;

        Ok(tables.into_iter().map(|t| t.name).collect())
    }

    /// Create a table with spatial columns.
    pub async fn create_spatial_table(
        &self,
        table_name: &str,
        additional_columns: &[(String, String)],
        engine: &str,
    ) -> Result<()> {
        let mut columns = vec![
            "id UInt64".to_string(),
            "point Tuple(Float64, Float64)".to_string(),
        ];

        for (col_name, col_type) in additional_columns {
            columns.push(format!("{} {}", col_name, col_type));
        }

        let create_sql = format!(
            "CREATE TABLE IF NOT EXISTS {} ({}) ENGINE = {}",
            table_name,
            columns.join(", "),
            engine
        );

        self.client
            .query(&create_sql)
            .execute()
            .await
            .map_err(|e| Error::ClickHouse(e.to_string()))?;

        Ok(())
    }

    /// Drop a table.
    pub async fn drop_table(&self, table_name: &str) -> Result<()> {
        let sql = format!("DROP TABLE IF EXISTS {}", table_name);

        self.client
            .query(&sql)
            .execute()
            .await
            .map_err(|e| Error::ClickHouse(e.to_string()))?;

        Ok(())
    }

    /// Get table schema.
    pub async fn table_schema(&self, table_name: &str) -> Result<Vec<(String, String)>> {
        #[derive(Debug, clickhouse::Row, serde::Deserialize)]
        struct ColumnInfo {
            name: String,
            #[serde(rename = "type")]
            type_: String,
        }

        let columns: Vec<ColumnInfo> = self
            .client
            .query("SELECT name, type FROM system.columns WHERE database = ? AND table = ?")
            .bind(&self.config.database)
            .bind(table_name)
            .fetch_all()
            .await
            .map_err(|e| Error::ClickHouse(e.to_string()))?;

        Ok(columns.into_iter().map(|c| (c.name, c.type_)).collect())
    }

    /// Execute raw SQL.
    pub async fn execute(&self, sql: &str) -> Result<()> {
        self.client
            .query(sql)
            .execute()
            .await
            .map_err(|e| Error::ClickHouse(e.to_string()))?;

        Ok(())
    }

    /// Count rows in a table.
    pub async fn count_table(&self, table_name: &str) -> Result<u64> {
        let sql = format!("SELECT count() FROM {}", table_name);

        let count: u64 = self
            .client
            .query(&sql)
            .fetch_one()
            .await
            .map_err(|e| Error::ClickHouse(e.to_string()))?;

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = ClickHouseConfig::default();
        assert_eq!(config.database, "default");
        assert_eq!(config.username, "default");
    }
}
