//! Cassandra/ScyllaDB spatial database connector.
//!
//! Provides support for storing spatial data in Cassandra/ScyllaDB
//! using User Defined Types (UDTs).

pub mod reader;
pub mod types;
pub mod writer;

use crate::error::{Error, Result};
use scylla::{Session, SessionBuilder};
use std::sync::Arc;
use std::time::Duration;

/// Cassandra connector configuration.
#[derive(Debug, Clone)]
pub struct CassandraConfig {
    /// Contact points (node addresses).
    pub contact_points: Vec<String>,
    /// Keyspace name.
    pub keyspace: String,
    /// Datacenter (for local DC policy).
    pub local_dc: Option<String>,
    /// Connection timeout.
    pub connection_timeout: Duration,
    /// Request timeout.
    pub request_timeout: Duration,
    /// Username for authentication.
    pub username: Option<String>,
    /// Password for authentication.
    pub password: Option<String>,
}

impl Default for CassandraConfig {
    fn default() -> Self {
        Self {
            contact_points: vec!["127.0.0.1:9042".to_string()],
            keyspace: "gis".to_string(),
            local_dc: None,
            connection_timeout: Duration::from_secs(30),
            request_timeout: Duration::from_secs(30),
            username: None,
            password: None,
        }
    }
}

/// Cassandra spatial database connector.
pub struct CassandraConnector {
    session: Arc<Session>,
    config: CassandraConfig,
}

impl CassandraConnector {
    /// Create a new Cassandra connector.
    pub async fn new(config: CassandraConfig) -> Result<Self> {
        let mut builder = SessionBuilder::new()
            .known_nodes(&config.contact_points)
            .connection_timeout(config.connection_timeout);

        if let Some(username) = &config.username {
            if let Some(password) = &config.password {
                builder = builder.user(username, password);
            }
        }

        let session = builder
            .build()
            .await
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        // Use keyspace
        session
            .use_keyspace(&config.keyspace, false)
            .await
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        Ok(Self {
            session: Arc::new(session),
            config,
        })
    }

    /// Get session reference.
    pub fn session(&self) -> Arc<Session> {
        Arc::clone(&self.session)
    }

    /// Check if the connection is healthy.
    pub async fn health_check(&self) -> Result<bool> {
        let result = self
            .session
            .query_unpaged("SELECT now() FROM system.local", &[])
            .await;

        Ok(result.is_ok())
    }

    /// Get cluster version.
    pub async fn version(&self) -> Result<String> {
        let result = self
            .session
            .query_unpaged("SELECT release_version FROM system.local", &[])
            .await
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        if let Some(row) = result.rows.and_then(|r| r.into_iter().next()) {
            if let Some(Some(scylla::frame::response::result::CqlValue::Text(v))) =
                row.columns.first()
            {
                return Ok(v.clone());
            }
        }

        Err(Error::Cassandra("Failed to get version".to_string()))
    }

    /// Create keyspace if not exists.
    pub async fn create_keyspace(
        &self,
        replication_strategy: &str,
        replication_factor: u32,
    ) -> Result<()> {
        let cql = format!(
            "CREATE KEYSPACE IF NOT EXISTS {} WITH replication = {{'class': '{}', 'replication_factor': {}}}",
            self.config.keyspace, replication_strategy, replication_factor
        );

        self.session
            .query_unpaged(cql, &[])
            .await
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        Ok(())
    }

    /// Create point UDT.
    pub async fn create_point_type(&self) -> Result<()> {
        let cql = "CREATE TYPE IF NOT EXISTS point (x double, y double)";

        self.session
            .query_unpaged(cql, &[])
            .await
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        Ok(())
    }

    /// Create a table with spatial data.
    pub async fn create_spatial_table(
        &self,
        table_name: &str,
        partition_key: &str,
        clustering_key: Option<&str>,
        additional_columns: &[(String, String)],
    ) -> Result<()> {
        // Ensure point type exists
        self.create_point_type().await?;

        let mut columns = vec![
            format!("{} uuid", partition_key),
            "location frozen<point>".to_string(),
        ];

        if let Some(cluster_key) = clustering_key {
            columns.push(format!("{} timestamp", cluster_key));
        }

        for (col_name, col_type) in additional_columns {
            columns.push(format!("{} {}", col_name, col_type));
        }

        let primary_key = if let Some(cluster_key) = clustering_key {
            format!("PRIMARY KEY ({}, {})", partition_key, cluster_key)
        } else {
            format!("PRIMARY KEY ({})", partition_key)
        };

        let cql = format!(
            "CREATE TABLE IF NOT EXISTS {} ({}, {})",
            table_name,
            columns.join(", "),
            primary_key
        );

        self.session
            .query_unpaged(cql, &[])
            .await
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        Ok(())
    }

    /// Drop a table.
    pub async fn drop_table(&self, table_name: &str) -> Result<()> {
        let cql = format!("DROP TABLE IF EXISTS {}", table_name);

        self.session
            .query_unpaged(cql, &[])
            .await
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        Ok(())
    }

    /// List all tables in the keyspace.
    pub async fn list_tables(&self) -> Result<Vec<String>> {
        let cql = format!(
            "SELECT table_name FROM system_schema.tables WHERE keyspace_name = '{}'",
            self.config.keyspace
        );

        let result = self
            .session
            .query_unpaged(cql, &[])
            .await
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        let mut tables = Vec::new();

        if let Some(rows) = result.rows {
            for row in rows {
                if let Some(Some(scylla::frame::response::result::CqlValue::Text(table_name))) =
                    row.columns.first()
                {
                    tables.push(table_name.clone());
                }
            }
        }

        Ok(tables)
    }

    /// Execute raw CQL.
    pub async fn execute(&self, cql: &str) -> Result<()> {
        self.session
            .query_unpaged(cql, &[])
            .await
            .map_err(|e| Error::Cassandra(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = CassandraConfig::default();
        assert_eq!(config.keyspace, "gis");
        assert_eq!(config.contact_points.len(), 1);
    }
}
