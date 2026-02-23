//! Error types for cloud storage operations
//!
//! This module provides a comprehensive error hierarchy for all cloud storage operations,
//! including S3, Azure Blob Storage, Google Cloud Storage, HTTP, authentication, and caching.

use oxigdal_core::error::{IoError, OxiGdalError};

/// Result type for cloud storage operations
pub type Result<T> = core::result::Result<T, CloudError>;

/// Main error type for cloud storage operations
#[derive(Debug, thiserror::Error)]
pub enum CloudError {
    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] IoError),

    /// AWS S3 error
    #[error("S3 error: {0}")]
    S3(#[from] S3Error),

    /// Azure Blob Storage error
    #[error("Azure error: {0}")]
    Azure(#[from] AzureError),

    /// Google Cloud Storage error
    #[error("GCS error: {0}")]
    Gcs(#[from] GcsError),

    /// HTTP error
    #[error("HTTP error: {0}")]
    Http(#[from] HttpError),

    /// Authentication error
    #[error("Authentication error: {0}")]
    Auth(#[from] AuthError),

    /// Retry error
    #[error("Retry error: {0}")]
    Retry(#[from] RetryError),

    /// Cache error
    #[error("Cache error: {0}")]
    Cache(#[from] CacheError),

    /// Invalid URL
    #[error("Invalid URL: {url}")]
    InvalidUrl {
        /// The invalid URL
        url: String,
    },

    /// Unsupported protocol
    #[error("Unsupported protocol: {protocol}")]
    UnsupportedProtocol {
        /// Protocol name
        protocol: String,
    },

    /// Object not found
    #[error("Object not found: {key}")]
    NotFound {
        /// Object key/path
        key: String,
    },

    /// Permission denied
    #[error("Permission denied: {message}")]
    PermissionDenied {
        /// Error message
        message: String,
    },

    /// Operation timeout
    #[error("Operation timeout: {message}")]
    Timeout {
        /// Error message
        message: String,
    },

    /// Rate limit exceeded
    #[error("Rate limit exceeded: {message}")]
    RateLimitExceeded {
        /// Error message
        message: String,
    },

    /// Invalid configuration
    #[error("Invalid configuration: {message}")]
    InvalidConfiguration {
        /// Error message
        message: String,
    },

    /// Operation not supported
    #[error("Operation not supported: {operation}")]
    NotSupported {
        /// Operation description
        operation: String,
    },

    /// Internal error
    #[error("Internal error: {message}")]
    Internal {
        /// Error message
        message: String,
    },
}

/// AWS S3-specific errors
#[derive(Debug, thiserror::Error)]
pub enum S3Error {
    /// SDK error
    #[error("S3 SDK error: {message}")]
    Sdk {
        /// Error message
        message: String,
    },

    /// Bucket not found
    #[error("Bucket not found: {bucket}")]
    BucketNotFound {
        /// Bucket name
        bucket: String,
    },

    /// Access denied
    #[error("Access denied to bucket '{bucket}': {message}")]
    AccessDenied {
        /// Bucket name
        bucket: String,
        /// Error message
        message: String,
    },

    /// Invalid bucket name
    #[error("Invalid bucket name: {bucket}")]
    InvalidBucketName {
        /// Bucket name
        bucket: String,
    },

    /// Object too large
    #[error("Object too large: {size} bytes (max: {max_size})")]
    ObjectTooLarge {
        /// Object size
        size: u64,
        /// Maximum allowed size
        max_size: u64,
    },

    /// Multipart upload error
    #[error("Multipart upload error: {message}")]
    MultipartUpload {
        /// Error message
        message: String,
    },

    /// STS assume role error
    #[error("STS assume role error: {message}")]
    StsAssumeRole {
        /// Error message
        message: String,
    },

    /// Region error
    #[error("Region error: {message}")]
    Region {
        /// Error message
        message: String,
    },
}

/// Azure Blob Storage-specific errors
#[derive(Debug, thiserror::Error)]
pub enum AzureError {
    /// SDK error
    #[error("Azure SDK error: {message}")]
    Sdk {
        /// Error message
        message: String,
    },

    /// Container not found
    #[error("Container not found: {container}")]
    ContainerNotFound {
        /// Container name
        container: String,
    },

    /// Blob not found
    #[error("Blob not found: {blob}")]
    BlobNotFound {
        /// Blob name
        blob: String,
    },

    /// Access denied
    #[error("Access denied to container '{container}': {message}")]
    AccessDenied {
        /// Container name
        container: String,
        /// Error message
        message: String,
    },

    /// Invalid SAS token
    #[error("Invalid SAS token: {message}")]
    InvalidSasToken {
        /// Error message
        message: String,
    },

    /// Account error
    #[error("Account error: {message}")]
    Account {
        /// Error message
        message: String,
    },

    /// Lease error
    #[error("Lease error: {message}")]
    Lease {
        /// Error message
        message: String,
    },
}

/// Google Cloud Storage-specific errors
#[derive(Debug, thiserror::Error)]
pub enum GcsError {
    /// SDK error
    #[error("GCS SDK error: {message}")]
    Sdk {
        /// Error message
        message: String,
    },

    /// Bucket not found
    #[error("Bucket not found: {bucket}")]
    BucketNotFound {
        /// Bucket name
        bucket: String,
    },

    /// Object not found
    #[error("Object not found: {object}")]
    ObjectNotFound {
        /// Object name
        object: String,
    },

    /// Access denied
    #[error("Access denied to bucket '{bucket}': {message}")]
    AccessDenied {
        /// Bucket name
        bucket: String,
        /// Error message
        message: String,
    },

    /// Invalid project ID
    #[error("Invalid project ID: {project_id}")]
    InvalidProjectId {
        /// Project ID
        project_id: String,
    },

    /// Service account error
    #[error("Service account error: {message}")]
    ServiceAccount {
        /// Error message
        message: String,
    },

    /// Signed URL error
    #[error("Signed URL error: {message}")]
    SignedUrl {
        /// Error message
        message: String,
    },
}

/// HTTP-specific errors
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    /// Network error
    #[error("Network error: {message}")]
    Network {
        /// Error message
        message: String,
    },

    /// HTTP status error
    #[error("HTTP {status}: {message}")]
    Status {
        /// HTTP status code
        status: u16,
        /// Error message
        message: String,
    },

    /// Invalid header
    #[error("Invalid header '{name}': {message}")]
    InvalidHeader {
        /// Header name
        name: String,
        /// Error message
        message: String,
    },

    /// Request build error
    #[error("Request build error: {message}")]
    RequestBuild {
        /// Error message
        message: String,
    },

    /// Response parse error
    #[error("Response parse error: {message}")]
    ResponseParse {
        /// Error message
        message: String,
    },

    /// TLS error
    #[error("TLS error: {message}")]
    Tls {
        /// Error message
        message: String,
    },
}

/// Authentication-specific errors
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// Credentials not found
    #[error("Credentials not found: {message}")]
    CredentialsNotFound {
        /// Error message
        message: String,
    },

    /// Invalid credentials
    #[error("Invalid credentials: {message}")]
    InvalidCredentials {
        /// Error message
        message: String,
    },

    /// Token expired
    #[error("Token expired: {message}")]
    TokenExpired {
        /// Error message
        message: String,
    },

    /// OAuth2 error
    #[error("OAuth2 error: {message}")]
    OAuth2 {
        /// Error message
        message: String,
    },

    /// Service account key error
    #[error("Service account key error: {message}")]
    ServiceAccountKey {
        /// Error message
        message: String,
    },

    /// API key error
    #[error("API key error: {message}")]
    ApiKey {
        /// Error message
        message: String,
    },

    /// SAS token error
    #[error("SAS token error: {message}")]
    SasToken {
        /// Error message
        message: String,
    },

    /// IAM role error
    #[error("IAM role error: {message}")]
    IamRole {
        /// Error message
        message: String,
    },
}

/// Retry-specific errors
#[derive(Debug, thiserror::Error)]
pub enum RetryError {
    /// Maximum retries exceeded
    #[error("Maximum retries exceeded: {attempts} attempts")]
    MaxRetriesExceeded {
        /// Number of attempts
        attempts: usize,
    },

    /// Circuit breaker open
    #[error("Circuit breaker open: {message}")]
    CircuitBreakerOpen {
        /// Error message
        message: String,
    },

    /// Retry budget exhausted
    #[error("Retry budget exhausted: {message}")]
    BudgetExhausted {
        /// Error message
        message: String,
    },

    /// Non-retryable error
    #[error("Non-retryable error: {message}")]
    NonRetryable {
        /// Error message
        message: String,
    },
}

/// Cache-specific errors
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// Cache miss
    #[error("Cache miss for key: {key}")]
    Miss {
        /// Cache key
        key: String,
    },

    /// Cache write error
    #[error("Cache write error: {message}")]
    WriteError {
        /// Error message
        message: String,
    },

    /// Cache read error
    #[error("Cache read error: {message}")]
    ReadError {
        /// Error message
        message: String,
    },

    /// Cache invalidation error
    #[error("Cache invalidation error: {message}")]
    InvalidationError {
        /// Error message
        message: String,
    },

    /// Cache full
    #[error("Cache full: {message}")]
    Full {
        /// Error message
        message: String,
    },

    /// Compression error
    #[error("Compression error: {message}")]
    Compression {
        /// Error message
        message: String,
    },

    /// Decompression error
    #[error("Decompression error: {message}")]
    Decompression {
        /// Error message
        message: String,
    },
}

// Conversions from OxiGDAL errors
impl From<OxiGdalError> for CloudError {
    fn from(err: OxiGdalError) -> Self {
        match err {
            OxiGdalError::Io(e) => Self::Io(e),
            OxiGdalError::NotSupported { operation } => Self::NotSupported { operation },
            OxiGdalError::Internal { message } => Self::Internal { message },
            _ => Self::Internal {
                message: format!("{err}"),
            },
        }
    }
}

#[cfg(feature = "std")]
impl From<std::io::Error> for CloudError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.into())
    }
}

impl From<url::ParseError> for CloudError {
    fn from(err: url::ParseError) -> Self {
        Self::InvalidUrl {
            url: err.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = CloudError::NotFound {
            key: "test/file.txt".to_string(),
        };
        assert!(err.to_string().contains("test/file.txt"));
    }

    #[test]
    fn test_s3_error() {
        let err = S3Error::BucketNotFound {
            bucket: "my-bucket".to_string(),
        };
        assert!(err.to_string().contains("my-bucket"));
    }

    #[test]
    fn test_azure_error() {
        let err = AzureError::ContainerNotFound {
            container: "my-container".to_string(),
        };
        assert!(err.to_string().contains("my-container"));
    }

    #[test]
    fn test_gcs_error() {
        let err = GcsError::BucketNotFound {
            bucket: "my-bucket".to_string(),
        };
        assert!(err.to_string().contains("my-bucket"));
    }

    #[test]
    fn test_auth_error() {
        let err = AuthError::TokenExpired {
            message: "Token expired at 2026-01-25".to_string(),
        };
        assert!(err.to_string().contains("expired"));
    }

    #[test]
    fn test_retry_error() {
        let err = RetryError::MaxRetriesExceeded { attempts: 5 };
        assert!(err.to_string().contains("5"));
    }

    #[test]
    fn test_cache_error() {
        let err = CacheError::Miss {
            key: "cache-key".to_string(),
        };
        assert!(err.to_string().contains("cache-key"));
    }
}
