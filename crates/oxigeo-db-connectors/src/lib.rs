//! Database connectors for OxiGeo.
//!
//! This crate provides connectors for various database systems with spatial data support:
//! - MySQL/MariaDB with spatial extensions
//! - SQLite/SpatiaLite for embedded spatial databases
//! - MongoDB with native GeoJSON support
//! - ClickHouse for massive-scale spatial analytics
//! - TimescaleDB for time-series geospatial data
//! - Cassandra/ScyllaDB for distributed spatial data storage
//!
//! # Features
//!
//! The default feature set is **100% Pure Rust**: `postgres`, `sqlite` and
//! `clickhouse`. The `mysql`, `mongodb` and `cassandra` connectors pull
//! non-Pure-Rust C/asm dependencies (libz-sys, ring, aws-lc-sys respectively)
//! and are therefore **opt-in** — enable them explicitly, e.g.
//! `features = ["mysql"]`.
//!
//! # Examples
//!
//! ## SQLite (pure Rust, enabled by default)
//!
//! ```no_run
//! use oxigeo_db_connectors::sqlite::SqliteConnector;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let connector = SqliteConnector::memory()?;
//! assert!(connector.health_check()?);
//! # Ok(())
//! # }
//! ```
//!
//! ## MySQL (opt-in `mysql` feature)
//!
//! ```ignore
//! use oxigeo_db_connectors::mysql::{MySqlConfig, MySqlConnector};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = MySqlConfig::default();
//! let connector = MySqlConnector::new(config)?;
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
pub(crate) mod sql;
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
