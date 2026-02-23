//! rs3gw storage backend integration for OxiGDAL
//!
//! This crate provides high-performance cloud storage access for OxiGDAL by integrating
//! with [rs3gw](https://github.com/cool-japan/rs3gw), a Pure Rust S3-compatible storage gateway.
//!
//! # Features
//!
//! - **Multi-backend Support**: Local, S3, MinIO, GCS, Azure
//! - **High Performance**: Zero-copy operations, ML-based caching, deduplication
//! - **Cloud-Optimized**: Optimized for COG (Cloud-Optimized GeoTIFF) and Zarr access
//! - **Pure Rust**: No C/C++ dependencies (COOLJAPAN Policy compliant)
//! - **Security**: Optional encryption-at-rest with AES-256-GCM
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────┐
//! │     OxiGDAL Drivers                 │
//! │  (GeoTIFF, Zarr, NetCDF, etc.)      │
//! └──────────────┬──────────────────────┘
//!                │
//!                ▼
//! ┌─────────────────────────────────────┐
//! │   oxigdal-rs3gw (This Crate)        │
//! │  ┌──────────┐      ┌──────────┐    │
//! │  │DataSource│      │ZarrStore │    │
//! │  └──────────┘      └──────────┘    │
//! └──────────────┬──────────────────────┘
//!                │
//!                ▼
//! ┌─────────────────────────────────────┐
//! │           rs3gw                     │
//! │  ┌─────────────────────────────┐   │
//! │  │   StorageBackend Trait      │   │
//! │  └─────────────────────────────┘   │
//! │     │     │      │      │      │    │
//! │  Local  S3  MinIO  GCS  Azure  │    │
//! └─────────────────────────────────────┘
//! ```
//!
//! # Usage Examples
//!
//! ## Reading a COG from S3
//!
//! ```no_run
//! use oxigdal_rs3gw::{OxigdalBackend, Rs3gwDataSource};
//! use oxigdal_core::io::DataSource;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Configure S3 backend
//! let backend = OxigdalBackend::S3 {
//!     region: "us-west-2".to_string(),
//!     bucket: "my-cog-bucket".to_string(),
//!     endpoint: None,
//!     access_key: None, // Uses AWS SDK default credentials
//!     secret_key: None,
//! };
//!
//! // Create storage and data source
//! let storage = backend.create_storage().await?;
//! let source = Rs3gwDataSource::new(
//!     storage,
//!     "my-cog-bucket".to_string(),
//!     "images/landsat.cog.tif".to_string()
//! ).await?;
//!
//! // Read data
//! let size = source.size()?;
//! println!("Image size: {} bytes", size);
//! # Ok(())
//! # }
//! ```
//!
//! ## Using with MinIO
//!
//! ```no_run
//! use oxigdal_rs3gw::{MinioBackendBuilder, Rs3gwDataSource};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let backend = MinioBackendBuilder::new(
//!     "http://localhost:9000",
//!     "geospatial-data",
//!     "minioadmin",
//!     "minioadmin"
//! ).build();
//!
//! let storage = backend.create_storage().await?;
//! let source = Rs3gwDataSource::new(
//!     storage,
//!     "geospatial-data".to_string(),
//!     "zarr/temperature.zarr".to_string()
//! ).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Zarr Store Integration
//!
//! ```no_run
//! # #[cfg(feature = "zarr")]
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! use oxigdal_rs3gw::{OxigdalBackend, Rs3gwStore};
//!
//! let backend = OxigdalBackend::Local {
//!     root: "/data/zarr".into(),
//! };
//!
//! let storage = backend.create_storage().await?;
//! let store = Rs3gwStore::new(
//!     storage,
//!     "local".to_string(),
//!     "array.zarr".to_string()
//! );
//!
//! // Use with Zarr array operations
//! // let array = ZarrArray::open(store).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Advanced Features
//!
//! ### COG-Optimized Caching
//!
//! ```no_run
//! # #[cfg(feature = "ml-cache")]
//! # fn example() {
//! use oxigdal_rs3gw::features::{CogCacheConfig, CogAccessPattern};
//!
//! // Configure cache for sequential tile access
//! let cache_config = CogAccessPattern::Sequential.recommended_config();
//! println!("Cache size: {} MB", cache_config.max_size_mb);
//! println!("Prefetch radius: {} tiles", cache_config.prefetch_radius);
//! # }
//! ```
//!
//! ### Zarr Deduplication
//!
//! ```no_run
//! # #[cfg(feature = "dedup")]
//! # fn example() -> Result<(), String> {
//! use oxigdal_rs3gw::features::{ZarrDedupConfig, ZarrDedupPresets};
//!
//! // Use preset for 256KB Zarr chunks
//! let dedup_config = ZarrDedupPresets::medium_chunks();
//!
//! // Estimate potential savings
//! let savings = oxigdal_rs3gw::features::dedup::estimate_savings(10000, 3000);
//! println!("Estimated savings: {:.1}%", savings * 100.0);
//! # Ok(())
//! # }
//! ```
//!
//! ### Encryption
//!
//! ```no_run
//! # #[cfg(feature = "encryption")]
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! use oxigdal_rs3gw::features::{EncryptionConfig, generate_key};
//!
//! // Generate a secure encryption key
//! let key = generate_key()?;
//!
//! // Configure encryption
//! let encryption = EncryptionConfig::new()
//!     .with_key(key)
//!     .with_metadata_encryption(true);
//!
//! encryption.validate().map_err(|e| e.to_string())?;
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]
#![warn(clippy::unwrap_used)]
#![warn(clippy::panic)]

pub mod config;
pub mod datasource;
pub mod error;
pub mod features;

#[cfg(feature = "zarr")]
pub mod store;

// Re-exports for convenience
pub use config::{MinioBackendBuilder, OxigdalBackend, S3BackendBuilder, parse_url};
pub use datasource::Rs3gwDataSource;
pub use error::{Result, Rs3gwError};

#[cfg(feature = "zarr")]
pub use store::Rs3gwStore;

// Version information
/// The version of this crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The version of rs3gw this crate is compatible with
pub const RS3GW_VERSION: &str = "0.1.0";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
        assert!(!RS3GW_VERSION.is_empty());
    }
}
