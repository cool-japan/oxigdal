//! Cloud storage backend implementations
//!
//! This module provides various cloud storage backends including S3, Azure Blob Storage,
//! Google Cloud Storage, and HTTP.

#[cfg(feature = "s3")]
pub mod s3;

#[cfg(feature = "azure-blob")]
pub mod azure;

#[cfg(feature = "gcs")]
pub mod gcs;

#[cfg(feature = "http")]
pub mod http;

#[cfg(feature = "s3")]
pub use s3::S3Backend;

#[cfg(feature = "azure-blob")]
pub use azure::AzureBlobBackend;

#[cfg(feature = "gcs")]
pub use gcs::GcsBackend;

#[cfg(feature = "http")]
pub use http::HttpBackend;

use crate::error::Result;

/// Common trait for cloud storage backends
#[cfg(feature = "async")]
#[async_trait::async_trait]
pub trait CloudStorageBackend: Send + Sync {
    /// Gets an object from storage
    async fn get(&self, key: &str) -> Result<bytes::Bytes>;

    /// Puts an object to storage
    async fn put(&self, key: &str, data: &[u8]) -> Result<()>;

    /// Deletes an object from storage
    async fn delete(&self, key: &str) -> Result<()>;

    /// Checks if an object exists
    async fn exists(&self, key: &str) -> Result<bool>;

    /// Lists objects with a given prefix
    async fn list_prefix(&self, prefix: &str) -> Result<Vec<String>>;

    /// Returns whether this backend is read-only
    fn is_readonly(&self) -> bool {
        false
    }
}
