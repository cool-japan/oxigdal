//! Error types for rs3gw integration
//!
//! This module provides error mapping between rs3gw's `StorageError` and
//! oxigdal-core's `OxiGdalError`.

use oxigdal_core::error::{IoError, OxiGdalError};
use rs3gw::storage::StorageError;
use thiserror::Error;

/// Error type for rs3gw operations
#[derive(Debug, Error)]
pub enum Rs3gwError {
    /// Storage backend error
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    /// Configuration error
    #[error("Configuration error: {message}")]
    Configuration {
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
    #[error("Object not found: bucket={bucket}, key={key}")]
    ObjectNotFound {
        /// Bucket name
        bucket: String,
        /// Object key
        key: String,
    },

    /// Invalid range request
    #[error("Invalid range: start={start}, end={end}, size={size}")]
    InvalidRange {
        /// Range start offset
        start: u64,
        /// Range end offset
        end: u64,
        /// Object size
        size: u64,
    },

    /// Backend initialization error
    #[error("Backend initialization failed: {message}")]
    BackendInit {
        /// Error message
        message: String,
    },

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<Rs3gwError> for OxiGdalError {
    fn from(err: Rs3gwError) -> Self {
        match err {
            Rs3gwError::Storage(e) => map_storage_error(e),
            Rs3gwError::Configuration { message } => OxiGdalError::InvalidParameter {
                parameter: "configuration",
                message,
            },
            Rs3gwError::BucketNotFound { bucket } => {
                OxiGdalError::Io(IoError::NotFound { path: bucket })
            }
            Rs3gwError::ObjectNotFound { bucket, key } => OxiGdalError::Io(IoError::NotFound {
                path: format!("{bucket}/{key}"),
            }),
            Rs3gwError::InvalidRange { start, end, size } => OxiGdalError::OutOfBounds {
                message: format!("Invalid byte range [{start}, {end}) for object of size {size}"),
            },
            Rs3gwError::BackendInit { message } => OxiGdalError::Internal { message },
            Rs3gwError::Io(e) => OxiGdalError::Io(IoError::Read {
                message: e.to_string(),
            }),
        }
    }
}

/// Maps rs3gw's StorageError to oxigdal's OxiGdalError
fn map_storage_error(err: StorageError) -> OxiGdalError {
    match err {
        StorageError::NotFound(ref path) => {
            OxiGdalError::Io(IoError::NotFound { path: path.clone() })
        }
        StorageError::BucketNotFound => OxiGdalError::Io(IoError::NotFound {
            path: "bucket".to_string(),
        }),
        StorageError::BucketAlreadyExists => OxiGdalError::InvalidParameter {
            parameter: "bucket",
            message: "Bucket already exists".to_string(),
        },
        StorageError::BucketNotEmpty => OxiGdalError::InvalidParameter {
            parameter: "bucket",
            message: "Bucket is not empty".to_string(),
        },
        StorageError::InvalidRange => OxiGdalError::OutOfBounds {
            message: "Invalid byte range".to_string(),
        },
        StorageError::MultipartNotFound => OxiGdalError::Io(IoError::NotFound {
            path: "multipart upload".to_string(),
        }),
        StorageError::InvalidPartNumber => OxiGdalError::InvalidParameter {
            parameter: "part_number",
            message: "Invalid part number".to_string(),
        },
        StorageError::AccessDenied => OxiGdalError::InvalidParameter {
            parameter: "credentials",
            message: "Access denied".to_string(),
        },
        StorageError::InvalidBucketName(ref name) => OxiGdalError::InvalidParameter {
            parameter: "bucket",
            message: format!("Invalid bucket name: {name}"),
        },
        StorageError::TooManyBuckets => OxiGdalError::InvalidParameter {
            parameter: "bucket",
            message: "Too many buckets".to_string(),
        },
        StorageError::InvalidPart(ref msg) => OxiGdalError::InvalidParameter {
            parameter: "part",
            message: format!("Invalid part: {msg}"),
        },
        StorageError::InvalidKey(ref key) => OxiGdalError::InvalidParameter {
            parameter: "key",
            message: format!("Invalid key: {key}"),
        },
        StorageError::InsufficientStorage => OxiGdalError::Internal {
            message: "Insufficient storage: no space left on device".to_string(),
        },
        StorageError::Internal(ref msg) => OxiGdalError::Internal {
            message: msg.clone(),
        },
        StorageError::Io(e) => OxiGdalError::Io(IoError::Read {
            message: e.to_string(),
        }),
    }
}

/// Result type alias for rs3gw operations
pub type Result<T> = std::result::Result<T, Rs3gwError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_conversion_storage_not_found() {
        let rs3gw_err = Rs3gwError::Storage(StorageError::NotFound("/test/path".to_string()));
        let oxigdal_err: OxiGdalError = rs3gw_err.into();

        assert!(
            matches!(&oxigdal_err, OxiGdalError::Io(IoError::NotFound { .. })),
            "Expected NotFound error"
        );
        if let OxiGdalError::Io(IoError::NotFound { path }) = oxigdal_err {
            assert_eq!(path, "/test/path");
        }
    }

    #[test]
    fn test_error_conversion_object_not_found() {
        let rs3gw_err = Rs3gwError::ObjectNotFound {
            bucket: "mybucket".to_string(),
            key: "mykey".to_string(),
        };
        let oxigdal_err: OxiGdalError = rs3gw_err.into();

        assert!(
            matches!(&oxigdal_err, OxiGdalError::Io(IoError::NotFound { .. })),
            "Expected NotFound error"
        );
        if let OxiGdalError::Io(IoError::NotFound { path }) = oxigdal_err {
            assert_eq!(path, "mybucket/mykey");
        }
    }

    #[test]
    fn test_error_conversion_invalid_range() {
        let rs3gw_err = Rs3gwError::InvalidRange {
            start: 100,
            end: 200,
            size: 50,
        };
        let oxigdal_err: OxiGdalError = rs3gw_err.into();

        assert!(
            matches!(&oxigdal_err, OxiGdalError::OutOfBounds { .. }),
            "Expected OutOfBounds error"
        );
        if let OxiGdalError::OutOfBounds { message } = oxigdal_err {
            assert!(message.contains("100"));
            assert!(message.contains("200"));
            assert!(message.contains("50"));
        }
    }
}
