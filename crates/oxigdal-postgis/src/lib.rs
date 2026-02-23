//! OxiGDAL PostGIS - PostgreSQL/PostGIS Integration
//!
//! This crate provides PostgreSQL/PostGIS integration for the OxiGDAL ecosystem,
//! enabling spatial database workflows with connection pooling and async operations.
//!
//! # Features
//!
//! - **Connection Pooling**: Efficient connection management with deadpool-postgres
//! - **Spatial Queries**: Fluent API for building spatial queries
//! - **Streaming**: Stream large result sets efficiently
//! - **Batch Operations**: Batch inserts for high performance
//! - **Transaction Support**: Full transaction management with savepoints
//! - **Type Safety**: Strong type conversions between OxiGDAL and PostGIS
//! - **WKB Support**: Efficient Well-Known Binary encoding/decoding
//!
//! # Example
//!
//! ```ignore
//! use oxigdal_postgis::*;
//! use oxigdal_core::types::BoundingBox;
//!
//! # async fn example() -> Result<()> {
//! // Create connection pool
//! let config = ConnectionConfig::new("gis_database")
//!     .host("localhost")
//!     .user("postgres")
//!     .password("password");
//!
//! let pool = ConnectionPool::new(config)?;
//!
//! // Check PostGIS is available
//! let health = pool.health_check().await?;
//! if !health.postgis_installed {
//!     eprintln!("PostGIS is not installed!");
//!     return Ok(());
//! }
//!
//! // Query features within bounding box
//! let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0)?;
//! let features = SpatialQuery::new("buildings")?
//!     .where_bbox(&bbox)?
//!     .limit(1000)
//!     .execute(&pool)
//!     .await?;
//!
//! println!("Found {} features", features.len());
//!
//! // Write features to database
//! let mut writer = PostGisWriter::new(pool.clone(), "results")
//!     .srid(4326)
//!     .create_table(true);
//!
//! for feature in features {
//!     writer.add_to_batch(feature);
//! }
//! writer.flush().await?;
//! # Ok(())
//! # }
//! ```
//!
//! # SQL Injection Prevention
//!
//! All SQL generation is protected against SQL injection attacks through:
//! - Parameterized queries
//! - Identifier validation and quoting
//! - Safe SQL builders
//!
//! # Performance
//!
//! - Connection pooling reduces connection overhead
//! - Batch operations improve write performance
//! - Streaming API supports large datasets
//! - Spatial indexes are automatically used
//!
//! # Requirements
//!
//! - PostgreSQL 12 or later
//! - PostGIS 3.0 or later

#![warn(missing_docs)]
#![warn(clippy::all)]
// Pedantic disabled to reduce noise - default clippy::all is sufficient
// #![warn(clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]
// Allow collapsible match for explicit SQL building patterns
#![allow(clippy::collapsible_match)]
#![allow(clippy::collapsible_if)]
// Allow expect() for internal database state invariants
#![allow(clippy::expect_used)]
// Allow async fn in traits for database operations
#![allow(async_fn_in_trait)]
// Allow dead code for future features
#![allow(dead_code)]
// Allow unsafe blocks in transaction module for FFI
#![allow(unsafe_code)]

pub mod connection;
pub mod error;
pub mod query;
pub mod reader;
pub mod sql;
pub mod transaction;
pub mod types;
pub mod wkb;
pub mod writer;

// Re-export commonly used items
pub use connection::{ConnectionConfig, ConnectionPool, HealthCheckResult, PoolConfig, SslMode};
pub use error::{PostGisError, Result};
pub use query::{JoinType, SpatialJoin, SpatialQuery};
pub use reader::PostGisReader;
pub use sql::functions;
pub use transaction::Transaction;
pub use types::{FeatureBuilder, PostGisGeometry, srid};
pub use wkb::{ByteOrder, WkbDecoder, WkbEncoder, WkbGeometryType};
pub use writer::PostGisWriter;

/// Crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Crate name
pub const NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
        assert_eq!(NAME, "oxigdal-postgis");
    }
}
