//! Advanced cloud storage backends for OxiGDAL
//!
//! This crate provides comprehensive cloud storage integration for OxiGDAL, including:
//!
//! - **Cloud Providers**: S3, Azure Blob Storage, Google Cloud Storage
//! - **Authentication**: OAuth 2.0, service accounts, API keys, SAS tokens, IAM roles
//! - **Advanced Caching**: Multi-level cache with memory and disk tiers, compression, LRU+LFU eviction
//! - **Intelligent Prefetching**: Predictive prefetch, access pattern analysis, bandwidth management
//! - **Retry Logic**: Exponential backoff, jitter, circuit breaker, retry budgets
//! - **HTTP Backend**: Enhanced HTTP/HTTPS with authentication and retry support
//!
//! # Features
//!
//! - `s3` - AWS S3 support
//! - `azure-blob` - Azure Blob Storage support
//! - `gcs` - Google Cloud Storage support
//! - `http` - HTTP/HTTPS backend
//! - `cache` - Advanced caching layer
//! - `prefetch` - Intelligent prefetching
//! - `retry` - Retry logic with backoff
//!
//! # Examples
//!
//! ## AWS S3
//!
//! ```rust,no_run
//! # #[cfg(feature = "s3")]
//! # async fn example() -> oxigdal_cloud::Result<()> {
//! use oxigdal_cloud::backends::S3Backend;
//! use oxigdal_cloud::backends::CloudStorageBackend;
//!
//! let backend = S3Backend::new("my-bucket", "data/zarr")
//!     .with_region("us-west-2");
//!
//! // Get object
//! let data = backend.get("file.tif").await?;
//!
//! // Put object
//! backend.put("output.tif", &data).await?;
//!
//! # Ok(())
//! # }
//! ```
//!
//! ## Multi-cloud Abstraction
//!
//! ```rust,no_run
//! # async fn example() -> oxigdal_cloud::Result<()> {
//! use oxigdal_cloud::CloudBackend;
//!
//! // Parse URL and create appropriate backend
//! let backend = CloudBackend::from_url("s3://bucket/file.tif")?;
//! let data = backend.get().await?;
//!
//! # Ok(())
//! # }
//! ```
//!
//! ## Advanced Caching
//!
//! ```rust,no_run
//! # #[cfg(feature = "cache")]
//! # async fn example() -> oxigdal_cloud::Result<()> {
//! use oxigdal_cloud::cache::{CacheConfig, MultiLevelCache};
//! use bytes::Bytes;
//!
//! let config = CacheConfig::new()
//!     .with_max_memory_size(100 * 1024 * 1024) // 100 MB
//!     .with_cache_dir("/tmp/oxigdal-cache");
//!
//! let cache = MultiLevelCache::new(config)?;
//!
//! // Cache data
//! cache.put("key".to_string(), Bytes::from("data")).await?;
//!
//! // Retrieve from cache
//! let data = cache.get(&"key".to_string()).await?;
//!
//! # Ok(())
//! # }
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
// Allow partial documentation during development
#![allow(missing_docs)]
// Allow dead code for backend features
#![allow(dead_code)]
// Allow matches! suggestions - explicit patterns preferred for cloud errors
#![allow(clippy::match_like_matches_macro)]
// Allow expect() for internal cloud state invariants
#![allow(clippy::expect_used)]
// Allow complex types in cloud interfaces
#![allow(clippy::type_complexity)]
// Allow manual div_ceil for bandwidth calculations
#![allow(clippy::manual_div_ceil)]
// Allow unused variables in platform-specific code
#![allow(unused_variables)]
// Allow collapsible matches for explicit error handling
#![allow(clippy::collapsible_match)]
// Allow async fn in traits for cloud operations
#![allow(async_fn_in_trait)]
// Allow stripping prefix manually for URL path handling
#![allow(clippy::manual_strip)]
// Allow first element access with get(0)
#![allow(clippy::get_first)]
// Allow field assignment outside initializer
#![allow(clippy::field_reassign_with_default)]
// Allow unused imports in feature-gated modules
#![allow(unused_imports)]
// Allow method names that may conflict with std traits
#![allow(clippy::should_implement_trait)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod auth;
pub mod backends;
#[cfg(feature = "cache")]
pub mod cache;
pub mod error;
#[cfg(feature = "async")]
pub mod multicloud;
#[cfg(feature = "prefetch")]
pub mod prefetch;
#[cfg(feature = "retry")]
pub mod retry;

pub use error::{CloudError, Result};

#[cfg(feature = "s3")]
pub use backends::s3::S3Backend;

#[cfg(feature = "azure-blob")]
pub use backends::azure::AzureBlobBackend;

#[cfg(feature = "gcs")]
pub use backends::gcs::GcsBackend;

#[cfg(feature = "http")]
pub use backends::http::HttpBackend;

#[cfg(feature = "async")]
pub use multicloud::{
    CloudProvider, CloudProviderConfig, CloudRegion, CrossCloudTransferConfig,
    CrossCloudTransferResult, MultiCloudManager, MultiCloudManagerBuilder, ProviderHealth,
    RoutingStrategy, TransferCostEstimate,
};

use url::Url;

/// Multi-cloud storage backend abstraction
#[derive(Debug)]
pub enum CloudBackend {
    /// AWS S3 backend
    #[cfg(feature = "s3")]
    S3 {
        /// S3 backend instance
        backend: S3Backend,
        /// Object key
        key: String,
    },

    /// Azure Blob Storage backend
    #[cfg(feature = "azure-blob")]
    Azure {
        /// Azure backend instance
        backend: AzureBlobBackend,
        /// Blob name
        blob: String,
    },

    /// Google Cloud Storage backend
    #[cfg(feature = "gcs")]
    Gcs {
        /// GCS backend instance
        backend: GcsBackend,
        /// Object name
        object: String,
    },

    /// HTTP/HTTPS backend
    #[cfg(feature = "http")]
    Http {
        /// HTTP backend instance
        backend: HttpBackend,
        /// Resource path
        path: String,
    },
}

impl CloudBackend {
    /// Creates a cloud backend from a URL
    ///
    /// Supported URL formats:
    /// - `s3://bucket/key` - AWS S3
    /// - `az://container/blob` - Azure Blob Storage
    /// - `gs://bucket/object` - Google Cloud Storage
    /// - `<http://example.com/path>` or `<https://example.com/path>` - HTTP/HTTPS
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # fn example() -> oxigdal_cloud::Result<()> {
    /// use oxigdal_cloud::CloudBackend;
    ///
    /// let backend = CloudBackend::from_url("s3://my-bucket/data/file.tif")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_url(url: &str) -> Result<Self> {
        let parsed = Url::parse(url)?;

        match parsed.scheme() {
            #[cfg(feature = "s3")]
            "s3" => {
                let bucket = parsed.host_str().ok_or_else(|| CloudError::InvalidUrl {
                    url: url.to_string(),
                })?;

                let key = parsed.path().trim_start_matches('/').to_string();

                Ok(Self::S3 {
                    backend: S3Backend::new(bucket, ""),
                    key,
                })
            }

            #[cfg(feature = "azure-blob")]
            "az" | "azure" => {
                let container = parsed.host_str().ok_or_else(|| CloudError::InvalidUrl {
                    url: url.to_string(),
                })?;

                // Account name should be in the username part of the URL
                let account = parsed.username();
                if account.is_empty() {
                    return Err(CloudError::InvalidUrl {
                        url: url.to_string(),
                    });
                }

                let blob = parsed.path().trim_start_matches('/').to_string();

                Ok(Self::Azure {
                    backend: AzureBlobBackend::new(account, container),
                    blob,
                })
            }

            #[cfg(feature = "gcs")]
            "gs" | "gcs" => {
                let bucket = parsed.host_str().ok_or_else(|| CloudError::InvalidUrl {
                    url: url.to_string(),
                })?;

                let object = parsed.path().trim_start_matches('/').to_string();

                Ok(Self::Gcs {
                    backend: GcsBackend::new(bucket),
                    object,
                })
            }

            #[cfg(feature = "http")]
            "http" | "https" => {
                // Reconstruct base URL without the path
                let base_url = format!(
                    "{}://{}",
                    parsed.scheme(),
                    parsed.host_str().ok_or_else(|| CloudError::InvalidUrl {
                        url: url.to_string(),
                    })?
                );

                let path = parsed.path().trim_start_matches('/').to_string();

                Ok(Self::Http {
                    backend: HttpBackend::new(base_url),
                    path,
                })
            }

            scheme => Err(CloudError::UnsupportedProtocol {
                protocol: scheme.to_string(),
            }),
        }
    }

    /// Gets data from the cloud backend
    #[cfg(feature = "async")]
    pub async fn get(&self) -> Result<bytes::Bytes> {
        use backends::CloudStorageBackend;

        match self {
            #[cfg(feature = "s3")]
            Self::S3 { backend, key } => backend.get(key).await,

            #[cfg(feature = "azure-blob")]
            Self::Azure { backend, blob } => backend.get(blob).await,

            #[cfg(feature = "gcs")]
            Self::Gcs { backend, object } => backend.get(object).await,

            #[cfg(feature = "http")]
            Self::Http { backend, path } => backend.get(path).await,
        }
    }

    /// Puts data to the cloud backend
    #[cfg(feature = "async")]
    pub async fn put(&self, data: &[u8]) -> Result<()> {
        use backends::CloudStorageBackend;

        match self {
            #[cfg(feature = "s3")]
            Self::S3 { backend, key } => backend.put(key, data).await,

            #[cfg(feature = "azure-blob")]
            Self::Azure { backend, blob } => backend.put(blob, data).await,

            #[cfg(feature = "gcs")]
            Self::Gcs { backend, object } => backend.put(object, data).await,

            #[cfg(feature = "http")]
            Self::Http { .. } => Err(CloudError::NotSupported {
                operation: "HTTP backend is read-only".to_string(),
            }),
        }
    }

    /// Checks if the object exists
    #[cfg(feature = "async")]
    pub async fn exists(&self) -> Result<bool> {
        use backends::CloudStorageBackend;

        match self {
            #[cfg(feature = "s3")]
            Self::S3 { backend, key } => backend.exists(key).await,

            #[cfg(feature = "azure-blob")]
            Self::Azure { backend, blob } => backend.exists(blob).await,

            #[cfg(feature = "gcs")]
            Self::Gcs { backend, object } => backend.exists(object).await,

            #[cfg(feature = "http")]
            Self::Http { backend, path } => backend.exists(path).await,
        }
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "s3")]
    fn test_cloud_backend_from_url_s3() {
        let backend = CloudBackend::from_url("s3://my-bucket/path/to/file.tif");
        assert!(backend.is_ok());

        if let Ok(CloudBackend::S3 { backend, key }) = backend {
            assert_eq!(backend.bucket, "my-bucket");
            assert_eq!(key, "path/to/file.tif");
        } else {
            panic!("Expected S3 backend");
        }
    }

    #[test]
    #[cfg(feature = "gcs")]
    fn test_cloud_backend_from_url_gcs() {
        let backend = CloudBackend::from_url("gs://my-bucket/path/to/file.tif");
        assert!(backend.is_ok());

        if let Ok(CloudBackend::Gcs { backend, object }) = backend {
            assert_eq!(backend.bucket, "my-bucket");
            assert_eq!(object, "path/to/file.tif");
        } else {
            panic!("Expected GCS backend");
        }
    }

    #[test]
    #[cfg(feature = "http")]
    fn test_cloud_backend_from_url_http() {
        let backend = CloudBackend::from_url("https://example.com/path/to/file.tif");
        assert!(backend.is_ok());

        if let Ok(CloudBackend::Http { backend, path }) = backend {
            assert!(backend.base_url.contains("example.com"));
            assert_eq!(path, "path/to/file.tif");
        } else {
            panic!("Expected HTTP backend");
        }
    }

    #[test]
    fn test_cloud_backend_from_url_invalid() {
        let backend = CloudBackend::from_url("invalid://url");
        assert!(backend.is_err());
    }
}
