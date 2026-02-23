//! Connection management for database connectors.

pub mod health;
pub mod pool;

use crate::error::{Error, Result};
use std::fmt;
use url::Url;

/// Database type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseType {
    /// MySQL/MariaDB.
    MySql,
    /// SQLite/SpatiaLite.
    SQLite,
    /// MongoDB.
    MongoDB,
    /// ClickHouse.
    ClickHouse,
    /// TimescaleDB (PostgreSQL).
    TimescaleDB,
    /// Cassandra/ScyllaDB.
    Cassandra,
}

impl fmt::Display for DatabaseType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DatabaseType::MySql => write!(f, "MySQL"),
            DatabaseType::SQLite => write!(f, "SQLite"),
            DatabaseType::MongoDB => write!(f, "MongoDB"),
            DatabaseType::ClickHouse => write!(f, "ClickHouse"),
            DatabaseType::TimescaleDB => write!(f, "TimescaleDB"),
            DatabaseType::Cassandra => write!(f, "Cassandra"),
        }
    }
}

/// Connection string parser.
pub struct ConnectionString {
    /// Database type.
    pub db_type: DatabaseType,
    /// Parsed URL.
    pub url: Url,
}

impl ConnectionString {
    /// Parse a connection string.
    pub fn parse(connection_string: &str) -> Result<Self> {
        let url = Url::parse(connection_string)?;

        let db_type = match url.scheme() {
            "mysql" => DatabaseType::MySql,
            "sqlite" => DatabaseType::SQLite,
            "mongodb" => DatabaseType::MongoDB,
            "clickhouse" | "http" | "https" if connection_string.contains("8123") => {
                DatabaseType::ClickHouse
            }
            "postgres" | "postgresql" => DatabaseType::TimescaleDB,
            "cassandra" => DatabaseType::Cassandra,
            scheme => {
                return Err(Error::InvalidConnectionString(format!(
                    "Unsupported database scheme: {}",
                    scheme
                )));
            }
        };

        Ok(Self { db_type, url })
    }

    /// Get database type.
    pub fn database_type(&self) -> DatabaseType {
        self.db_type
    }

    /// Get host.
    pub fn host(&self) -> Option<String> {
        self.url.host_str().map(|s| s.to_string())
    }

    /// Get port.
    pub fn port(&self) -> Option<u16> {
        self.url.port()
    }

    /// Get database name.
    pub fn database(&self) -> Option<String> {
        self.url.path().strip_prefix('/').map(|s| s.to_string())
    }

    /// Get username.
    pub fn username(&self) -> Option<String> {
        if self.url.username().is_empty() {
            None
        } else {
            Some(self.url.username().to_string())
        }
    }

    /// Get password.
    pub fn password(&self) -> Option<String> {
        self.url.password().map(|s| s.to_string())
    }

    /// Get query parameter.
    pub fn query_param(&self, key: &str) -> Option<String> {
        self.url
            .query_pairs()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mysql() {
        let conn_str = "mysql://user:pass@localhost:3306/mydb";
        let parsed = ConnectionString::parse(conn_str).expect("Failed to parse");

        assert_eq!(parsed.database_type(), DatabaseType::MySql);
        assert_eq!(parsed.host(), Some("localhost".to_string()));
        assert_eq!(parsed.port(), Some(3306));
        assert_eq!(parsed.database(), Some("mydb".to_string()));
        assert_eq!(parsed.username(), Some("user".to_string()));
        assert_eq!(parsed.password(), Some("pass".to_string()));
    }

    #[test]
    fn test_parse_sqlite() {
        let conn_str = "sqlite:///path/to/db.sqlite";
        let parsed = ConnectionString::parse(conn_str).expect("Failed to parse");

        assert_eq!(parsed.database_type(), DatabaseType::SQLite);
    }

    #[test]
    fn test_parse_mongodb() {
        let conn_str = "mongodb://localhost:27017/gis";
        let parsed = ConnectionString::parse(conn_str).expect("Failed to parse");

        assert_eq!(parsed.database_type(), DatabaseType::MongoDB);
        assert_eq!(parsed.host(), Some("localhost".to_string()));
        assert_eq!(parsed.port(), Some(27017));
    }

    #[test]
    fn test_parse_postgresql() {
        let conn_str = "postgresql://user:pass@localhost:5432/timescale";
        let parsed = ConnectionString::parse(conn_str).expect("Failed to parse");

        assert_eq!(parsed.database_type(), DatabaseType::TimescaleDB);
    }
}
