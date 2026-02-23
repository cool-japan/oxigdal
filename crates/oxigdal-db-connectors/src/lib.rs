//! Database connectors for OxiGDAL.
//!
//! This crate provides connectors for various database systems with spatial data support:
//! - MySQL/MariaDB with spatial extensions
//! - SQLite/SpatiaLite for embedded spatial databases
//! - MongoDB with native GeoJSON support
//! - ClickHouse for massive-scale spatial analytics
//! - TimescaleDB for time-series geospatial data
//! - Cassandra/ScyllaDB for distributed spatial data storage
//!
//! # Examples
//!
//! ## MySQL
//!
//! ```no_run
//! use oxigdal_db_connectors::mysql::{MySqlConfig, MySqlConnector};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = MySqlConfig::default();
//! let connector = MySqlConnector::new(config)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## MongoDB
//!
//! ```no_run
//! use oxigdal_db_connectors::mongodb::{MongoDbConfig, MongoDbConnector};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = MongoDbConfig::default();
//! let connector = MongoDbConnector::new(config).await?;
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]
#![deny(unsafe_code)]

#[cfg(feature = "cassandra")]
pub mod cassandra;
#[cfg(feature = "clickhouse")]
pub mod clickhouse;
pub mod connection;
pub mod error;
#[cfg(feature = "mongodb")]
pub mod mongodb;
#[cfg(feature = "mysql")]
pub mod mysql;
#[cfg(feature = "sqlite")]
pub mod sqlite;
#[cfg(feature = "postgres")]
pub mod timescale;

// Re-export common types
pub use error::{Error, Result};

/// Database connector trait (for future unified interface).
#[async_trait::async_trait]
pub trait DatabaseConnector: Send + Sync {
    /// Check if the connection is healthy.
    async fn health_check(&self) -> Result<bool>;

    /// Get database version.
    async fn version(&self) -> Result<String>;

    /// List all tables/collections.
    async fn list_tables(&self) -> Result<Vec<String>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_types() {
        let err = Error::Connection("test".to_string());
        assert!(err.to_string().contains("Connection"));
    }
}
