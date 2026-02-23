//! Backend configuration for rs3gw integration
//!
//! This module provides a unified configuration interface for different
//! storage backends supported by rs3gw.

use crate::error::{Result, Rs3gwError};
use rs3gw::storage::backend::{
    BackendConfig as Rs3gwBackendConfig, BackendType, DynBackend, create_backend_from_config,
};
use std::collections::HashMap;
use std::path::PathBuf;

/// Unified backend configuration for OxiGDAL
///
/// This enum provides a user-friendly interface for configuring various
/// storage backends supported by rs3gw.
#[derive(Debug, Clone)]
pub enum OxigdalBackend {
    /// Local filesystem storage
    Local {
        /// Root directory for storage
        root: PathBuf,
    },

    /// AWS S3 storage
    S3 {
        /// AWS region
        region: String,
        /// S3 bucket name
        bucket: String,
        /// Optional endpoint URL for S3-compatible services
        endpoint: Option<String>,
        /// Optional access key (uses AWS SDK defaults if not provided)
        access_key: Option<String>,
        /// Optional secret key (uses AWS SDK defaults if not provided)
        secret_key: Option<String>,
    },

    /// MinIO storage (S3-compatible)
    MinIO {
        /// MinIO endpoint URL
        endpoint: String,
        /// MinIO bucket name
        bucket: String,
        /// MinIO access key
        access_key: String,
        /// MinIO secret key
        secret_key: String,
        /// Optional region (defaults to "us-east-1")
        region: Option<String>,
    },

    /// Google Cloud Storage
    Gcs {
        /// GCS bucket name
        bucket: String,
        /// Optional project ID
        project_id: Option<String>,
    },

    /// Azure Blob Storage
    Azure {
        /// Azure container name
        container: String,
        /// Azure storage account name
        account: String,
        /// Azure storage account key
        account_key: String,
    },
}

impl OxigdalBackend {
    /// Creates a storage backend from this configuration
    ///
    /// # Errors
    /// Returns an error if the backend cannot be initialized
    pub async fn create_storage(&self) -> Result<DynBackend> {
        let (config, storage_root) = self.to_rs3gw_config();
        create_backend_from_config(config, storage_root)
            .await
            .map_err(Rs3gwError::from)
    }

    /// Converts to rs3gw's BackendConfig
    fn to_rs3gw_config(&self) -> (Rs3gwBackendConfig, Option<PathBuf>) {
        match self {
            Self::Local { root } => (
                Rs3gwBackendConfig {
                    backend_type: BackendType::Local,
                    endpoint: None,
                    access_key: None,
                    secret_key: None,
                    region: None,
                    use_ssl: false,
                    extra: HashMap::new(),
                },
                Some(root.clone()),
            ),

            Self::S3 {
                region,
                bucket: _,
                endpoint,
                access_key,
                secret_key,
            } => (
                Rs3gwBackendConfig {
                    backend_type: BackendType::S3,
                    endpoint: endpoint.clone(),
                    access_key: access_key.clone(),
                    secret_key: secret_key.clone(),
                    region: Some(region.clone()),
                    use_ssl: true,
                    extra: HashMap::new(),
                },
                None,
            ),

            Self::MinIO {
                endpoint,
                bucket: _,
                access_key,
                secret_key,
                region,
            } => (
                Rs3gwBackendConfig {
                    backend_type: BackendType::MinIO,
                    endpoint: Some(endpoint.clone()),
                    access_key: Some(access_key.clone()),
                    secret_key: Some(secret_key.clone()),
                    region: Some(region.clone().unwrap_or_else(|| "us-east-1".to_string())),
                    use_ssl: endpoint.starts_with("https"),
                    extra: HashMap::new(),
                },
                None,
            ),

            Self::Gcs {
                bucket: _,
                project_id,
            } => {
                let mut extra = HashMap::new();
                if let Some(pid) = project_id {
                    extra.insert(
                        "project_id".to_string(),
                        serde_json::Value::String(pid.clone()),
                    );
                }
                (
                    Rs3gwBackendConfig {
                        backend_type: BackendType::Gcs,
                        endpoint: None,
                        access_key: None,
                        secret_key: None,
                        region: None,
                        use_ssl: true,
                        extra,
                    },
                    None,
                )
            }

            Self::Azure {
                container: _,
                account,
                account_key,
            } => (
                Rs3gwBackendConfig {
                    backend_type: BackendType::Azure,
                    endpoint: None,
                    access_key: Some(account.clone()),
                    secret_key: Some(account_key.clone()),
                    region: None,
                    use_ssl: true,
                    extra: HashMap::new(),
                },
                None,
            ),
        }
    }

    /// Returns the bucket/container name for this backend
    #[must_use]
    pub fn bucket_name(&self) -> &str {
        match self {
            Self::Local { .. } => "local",
            Self::S3 { bucket, .. } => bucket,
            Self::MinIO { bucket, .. } => bucket,
            Self::Gcs { bucket, .. } => bucket,
            Self::Azure { container, .. } => container,
        }
    }
}

/// Parses a URL into a backend configuration
///
/// Supported URL schemes:
/// - `file:///path/to/dir` -> Local backend
/// - `s3://bucket/path` -> S3 backend
/// - `minio://endpoint/bucket/path` -> MinIO backend
/// - `gs://bucket/path` -> GCS backend
/// - `az://container/path` -> Azure backend
///
/// # Arguments
/// * `url` - The URL to parse
///
/// # Returns
/// A tuple of (backend config, bucket, object key)
///
/// # Errors
/// Returns an error if the URL scheme is not supported or the URL is malformed
pub fn parse_url(url: &str) -> Result<(OxigdalBackend, String, String)> {
    if let Some(path) = url.strip_prefix("file://") {
        // Local filesystem
        let path = PathBuf::from(path);
        let parent = path
            .parent()
            .ok_or_else(|| Rs3gwError::Configuration {
                message: "Invalid local file URL".to_string(),
            })?
            .to_path_buf();
        let filename = path
            .file_name()
            .ok_or_else(|| Rs3gwError::Configuration {
                message: "Invalid local file URL: no filename".to_string(),
            })?
            .to_string_lossy()
            .to_string();

        Ok((
            OxigdalBackend::Local { root: parent },
            "local".to_string(),
            filename,
        ))
    } else if let Some(rest) = url.strip_prefix("s3://") {
        // S3 storage
        let parts: Vec<&str> = rest.splitn(2, '/').collect();
        let bucket = parts
            .first()
            .ok_or_else(|| Rs3gwError::Configuration {
                message: "Invalid S3 URL: missing bucket".to_string(),
            })?
            .to_string();
        let key = parts.get(1).unwrap_or(&"").to_string();

        Ok((
            OxigdalBackend::S3 {
                region: "us-east-1".to_string(),
                bucket: bucket.clone(),
                endpoint: None,
                access_key: None,
                secret_key: None,
            },
            bucket,
            key,
        ))
    } else if let Some(rest) = url.strip_prefix("gs://") {
        // Google Cloud Storage
        let parts: Vec<&str> = rest.splitn(2, '/').collect();
        let bucket = parts
            .first()
            .ok_or_else(|| Rs3gwError::Configuration {
                message: "Invalid GCS URL: missing bucket".to_string(),
            })?
            .to_string();
        let key = parts.get(1).unwrap_or(&"").to_string();

        Ok((
            OxigdalBackend::Gcs {
                bucket: bucket.clone(),
                project_id: None,
            },
            bucket,
            key,
        ))
    } else if let Some(rest) = url.strip_prefix("az://") {
        // Azure Blob Storage
        let parts: Vec<&str> = rest.splitn(2, '/').collect();
        let container = parts
            .first()
            .ok_or_else(|| Rs3gwError::Configuration {
                message: "Invalid Azure URL: missing container".to_string(),
            })?
            .to_string();
        let key = parts.get(1).unwrap_or(&"").to_string();

        Ok((
            OxigdalBackend::Azure {
                container: container.clone(),
                account: String::new(),     // Must be configured separately
                account_key: String::new(), // Must be configured separately
            },
            container,
            key,
        ))
    } else {
        Err(Rs3gwError::Configuration {
            message: format!("Unsupported URL scheme: {url}"),
        })
    }
}

/// Builder for configuring an S3 backend
#[derive(Debug, Clone, Default)]
pub struct S3BackendBuilder {
    region: String,
    bucket: String,
    endpoint: Option<String>,
    access_key: Option<String>,
    secret_key: Option<String>,
}

impl S3BackendBuilder {
    /// Creates a new S3 backend builder
    #[must_use]
    pub fn new(bucket: impl Into<String>) -> Self {
        Self {
            region: "us-east-1".to_string(),
            bucket: bucket.into(),
            ..Default::default()
        }
    }

    /// Sets the AWS region
    #[must_use]
    pub fn region(mut self, region: impl Into<String>) -> Self {
        self.region = region.into();
        self
    }

    /// Sets a custom endpoint URL
    #[must_use]
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Sets explicit credentials
    #[must_use]
    pub fn credentials(
        mut self,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
    ) -> Self {
        self.access_key = Some(access_key.into());
        self.secret_key = Some(secret_key.into());
        self
    }

    /// Builds the backend configuration
    #[must_use]
    pub fn build(self) -> OxigdalBackend {
        OxigdalBackend::S3 {
            region: self.region,
            bucket: self.bucket,
            endpoint: self.endpoint,
            access_key: self.access_key,
            secret_key: self.secret_key,
        }
    }
}

/// Builder for configuring a MinIO backend
#[derive(Debug, Clone)]
pub struct MinioBackendBuilder {
    endpoint: String,
    bucket: String,
    access_key: String,
    secret_key: String,
    region: Option<String>,
}

impl MinioBackendBuilder {
    /// Creates a new MinIO backend builder
    #[must_use]
    pub fn new(
        endpoint: impl Into<String>,
        bucket: impl Into<String>,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            bucket: bucket.into(),
            access_key: access_key.into(),
            secret_key: secret_key.into(),
            region: None,
        }
    }

    /// Sets the region
    #[must_use]
    pub fn region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// Builds the backend configuration
    #[must_use]
    pub fn build(self) -> OxigdalBackend {
        OxigdalBackend::MinIO {
            endpoint: self.endpoint,
            bucket: self.bucket,
            access_key: self.access_key,
            secret_key: self.secret_key,
            region: self.region,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_s3_url() {
        let (backend, bucket, key) =
            parse_url("s3://my-bucket/path/to/file.tif").expect("should parse");
        assert_eq!(bucket, "my-bucket");
        assert_eq!(key, "path/to/file.tif");
        assert!(matches!(backend, OxigdalBackend::S3 { .. }));
    }

    #[test]
    fn test_parse_gs_url() {
        let (backend, bucket, key) =
            parse_url("gs://gcs-bucket/data/image.cog").expect("should parse");
        assert_eq!(bucket, "gcs-bucket");
        assert_eq!(key, "data/image.cog");
        assert!(matches!(backend, OxigdalBackend::Gcs { .. }));
    }

    #[test]
    fn test_parse_az_url() {
        let (backend, container, key) =
            parse_url("az://mycontainer/blob/path").expect("should parse");
        assert_eq!(container, "mycontainer");
        assert_eq!(key, "blob/path");
        assert!(matches!(backend, OxigdalBackend::Azure { .. }));
    }

    #[test]
    fn test_parse_local_url() {
        let (backend, _, key) = parse_url("file:///tmp/data/test.tif").expect("should parse");
        assert_eq!(key, "test.tif");
        assert!(matches!(backend, OxigdalBackend::Local { .. }));
    }

    #[test]
    fn test_unsupported_scheme() {
        let result = parse_url("ftp://example.com/file");
        assert!(result.is_err());
    }

    #[test]
    fn test_s3_backend_builder() {
        let backend = S3BackendBuilder::new("my-bucket")
            .region("eu-west-1")
            .endpoint("https://s3.custom.endpoint.com")
            .credentials(
                "AKIAIOSFODNN7EXAMPLE",
                "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            )
            .build();

        assert!(
            matches!(&backend, OxigdalBackend::S3 { .. }),
            "Expected S3 backend"
        );
        if let OxigdalBackend::S3 {
            region,
            bucket,
            endpoint,
            access_key,
            secret_key,
        } = backend
        {
            assert_eq!(region, "eu-west-1");
            assert_eq!(bucket, "my-bucket");
            assert!(endpoint.is_some());
            assert!(access_key.is_some());
            assert!(secret_key.is_some());
        }
    }

    #[test]
    fn test_minio_backend_builder() {
        let backend = MinioBackendBuilder::new(
            "http://localhost:9000",
            "test-bucket",
            "minioadmin",
            "minioadmin",
        )
        .region("us-west-2")
        .build();

        assert!(
            matches!(&backend, OxigdalBackend::MinIO { .. }),
            "Expected MinIO backend"
        );
        if let OxigdalBackend::MinIO {
            endpoint,
            bucket,
            region,
            ..
        } = backend
        {
            assert_eq!(endpoint, "http://localhost:9000");
            assert_eq!(bucket, "test-bucket");
            assert_eq!(region, Some("us-west-2".to_string()));
        }
    }
}
