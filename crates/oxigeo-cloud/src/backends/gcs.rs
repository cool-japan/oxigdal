//! Google Cloud Storage backend with comprehensive features
//!
//! This module provides GCS integration with SDK support, object operations,
//! IAM authentication, bucket management, and signed URLs.

use bytes::Bytes;
use std::time::Duration;

use crate::auth::Credentials;
use crate::error::{CloudError, GcsError, Result};
use crate::retry::{RetryConfig, RetryExecutor};
use oxigeo_core::io::ByteRange;

use super::CloudStorageBackend;

/// GCS storage class
#[derive(Debug, Clone, Copy)]
pub enum GcsStorageClass {
    /// Standard storage
    Standard,
    /// Nearline storage
    Nearline,
    /// Coldline storage
    Coldline,
    /// Archive storage
    Archive,
}

impl GcsStorageClass {
    /// Returns the GCS API string representation of this storage class.
    #[must_use]
    pub fn as_api_str(self) -> &'static str {
        match self {
            Self::Standard => "STANDARD",
            Self::Nearline => "NEARLINE",
            Self::Coldline => "COLDLINE",
            Self::Archive => "ARCHIVE",
        }
    }
}

/// Google Cloud Storage backend
#[derive(Debug, Clone)]
pub struct GcsBackend {
    /// GCS bucket name
    pub bucket: String,
    /// Object prefix (path within bucket)
    pub prefix: String,
    /// Project ID
    pub project_id: Option<String>,
    /// Storage class
    pub storage_class: GcsStorageClass,
    /// Request timeout
    pub timeout: Duration,
    /// Retry configuration
    pub retry_config: RetryConfig,
    /// Credentials
    pub credentials: Option<Credentials>,
}

impl GcsBackend {
    /// Creates a new GCS backend
    ///
    /// # Arguments
    /// * `bucket` - The GCS bucket name
    #[must_use]
    pub fn new(bucket: impl Into<String>) -> Self {
        Self {
            bucket: bucket.into(),
            prefix: String::new(),
            project_id: None,
            storage_class: GcsStorageClass::Standard,
            timeout: Duration::from_secs(300),
            retry_config: RetryConfig::default(),
            credentials: None,
        }
    }

    /// Sets the object prefix
    #[must_use]
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// Sets the project ID
    #[must_use]
    pub fn with_project_id(mut self, project_id: impl Into<String>) -> Self {
        self.project_id = Some(project_id.into());
        self
    }

    /// Sets storage class
    #[must_use]
    pub fn with_storage_class(mut self, storage_class: GcsStorageClass) -> Self {
        self.storage_class = storage_class;
        self
    }

    /// Sets request timeout
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Sets retry configuration
    #[must_use]
    pub fn with_retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = config;
        self
    }

    /// Sets credentials
    #[must_use]
    pub fn with_credentials(mut self, credentials: Credentials) -> Self {
        self.credentials = Some(credentials);
        self
    }

    fn full_object_name(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            key.to_string()
        } else {
            format!("{}/{}", self.prefix, key)
        }
    }

    /// Returns the GCS bucket resource name in the format used by the API.
    fn bucket_resource_name(&self) -> String {
        format!("projects/_/buckets/{}", self.bucket)
    }
}

// The google-cloud-storage 1.15 crate provides a Pure Rust implementation
// using two clients:
//   - `Storage` (HTTP) for read/write object data
//   - `StorageControl` (gRPC) for delete/list/get metadata
// Both are feature-gated under `#[cfg(all(feature = "gcs", feature = "async"))]`.

#[cfg(all(feature = "gcs", feature = "async"))]
use google_cloud_storage::client::{Storage, StorageControl};

#[cfg(all(feature = "gcs", feature = "async"))]
impl GcsBackend {
    /// Resolves an explicit `google_cloud_auth` credentials value from
    /// `self.credentials`, if one was configured via
    /// [`with_credentials`](Self::with_credentials).
    ///
    /// Returns `Ok(None)` when no explicit override applies (either no
    /// credentials were configured, or [`Credentials::None`] was set
    /// explicitly to request Application Default Credentials), and `Err`
    /// when an unsupported `Credentials` variant was configured, so callers
    /// never silently fall back to ADC when the user asked for something we
    /// cannot honor.
    fn google_credentials(&self) -> Result<Option<google_cloud_auth::credentials::Credentials>> {
        match &self.credentials {
            None | Some(Credentials::None) => Ok(None),
            Some(Credentials::ServiceAccount { key_json, .. }) => {
                let key_value: serde_json::Value = serde_json::from_str(key_json).map_err(|e| {
                    CloudError::Gcs(GcsError::ServiceAccount {
                        message: format!("Invalid service account key JSON: {e}"),
                    })
                })?;

                let mut builder =
                    google_cloud_auth::credentials::service_account::Builder::new(key_value);
                if let Some(project_id) = &self.project_id {
                    builder = builder.with_quota_project_id(project_id.clone());
                }

                let credentials = builder.build().map_err(|e| {
                    CloudError::Gcs(GcsError::ServiceAccount {
                        message: format!("Failed to build service account credentials: {e}"),
                    })
                })?;
                Ok(Some(credentials))
            }
            Some(Credentials::ApiKey { key }) => Ok(Some(
                google_cloud_auth::credentials::api_key_credentials::Builder::new(key.clone())
                    .build(),
            )),
            Some(other) => Err(CloudError::Gcs(GcsError::Sdk {
                message: format!(
                    "Unsupported credentials variant '{}' for GCS (expected ServiceAccount, ApiKey, or None)",
                    other.variant_name()
                ),
            })),
        }
    }

    /// Creates the GCS HTTP storage client for read/write operations.
    async fn create_storage_client(&self) -> Result<Storage> {
        let mut builder = Storage::builder();
        if let Some(credentials) = self.google_credentials()? {
            builder = builder.with_credentials(credentials);
        }
        let client = builder.build().await.map_err(|e| {
            CloudError::Gcs(GcsError::Sdk {
                message: format!("Failed to build GCS Storage client: {e}"),
            })
        })?;
        Ok(client)
    }

    /// Creates the GCS gRPC StorageControl client for delete/list/metadata.
    async fn create_control_client(&self) -> Result<StorageControl> {
        let mut builder = StorageControl::builder();
        if let Some(credentials) = self.google_credentials()? {
            builder = builder.with_credentials(credentials);
        }
        let client = builder.build().await.map_err(|e| {
            CloudError::Gcs(GcsError::Sdk {
                message: format!("Failed to build GCS StorageControl client: {e}"),
            })
        })?;
        Ok(client)
    }

    /// Checks whether a GCS error string represents a 404 / not-found condition.
    fn is_not_found(err_msg: &str) -> bool {
        err_msg.contains("404")
            || err_msg.contains("NotFound")
            || err_msg.contains("not found")
            || err_msg.contains("no such object")
    }
}

#[cfg(all(feature = "gcs", feature = "async"))]
#[async_trait::async_trait]
impl CloudStorageBackend for GcsBackend {
    async fn get(&self, key: &str) -> Result<Bytes> {
        let mut executor = RetryExecutor::new(self.retry_config.clone());

        executor
            .execute(|| async {
                let object_name = self.full_object_name(key);
                tracing::debug!("Getting GCS object: {}/{}", self.bucket, object_name);

                let client = self.create_storage_client().await?;
                let bucket_path = self.bucket_resource_name();

                let mut reader = client
                    .read_object(&bucket_path, &object_name)
                    .send()
                    .await
                    .map_err(|e| {
                        let msg = format!("{e}");
                        if Self::is_not_found(&msg) {
                            CloudError::Gcs(GcsError::ObjectNotFound {
                                object: format!("{}/{}", self.bucket, object_name),
                            })
                        } else {
                            CloudError::Gcs(GcsError::Sdk {
                                message: format!(
                                    "Failed to read GCS object '{}/{}': {e}",
                                    self.bucket, object_name
                                ),
                            })
                        }
                    })?;

                let mut data = Vec::new();
                while let Some(chunk) = reader.next().await.transpose().map_err(|e| {
                    CloudError::Gcs(GcsError::Sdk {
                        message: format!("Failed to read GCS object body: {e}"),
                    })
                })? {
                    data.extend_from_slice(&chunk);
                }

                Ok(Bytes::from(data))
            })
            .await
    }

    async fn get_range(&self, key: &str, range: ByteRange) -> Result<Bytes> {
        if range.is_empty() {
            return Ok(Bytes::new());
        }

        let mut executor = RetryExecutor::new(self.retry_config.clone());
        let offset = range.start;
        let count = range.end - range.start;

        executor
            .execute(|| async {
                let object_name = self.full_object_name(key);
                tracing::debug!(
                    "Getting GCS object range {}..{} of {}/{}",
                    offset,
                    offset + count,
                    self.bucket,
                    object_name
                );

                let client = self.create_storage_client().await?;
                let bucket_path = self.bucket_resource_name();

                let mut reader = client
                    .read_object(&bucket_path, &object_name)
                    .set_read_range(google_cloud_storage::model_ext::ReadRange::segment(
                        offset, count,
                    ))
                    .send()
                    .await
                    .map_err(|e| {
                        let msg = format!("{e}");
                        if Self::is_not_found(&msg) {
                            CloudError::Gcs(GcsError::ObjectNotFound {
                                object: format!("{}/{}", self.bucket, object_name),
                            })
                        } else {
                            CloudError::Gcs(GcsError::Sdk {
                                message: format!(
                                    "Failed to read GCS object range '{}/{}': {e}",
                                    self.bucket, object_name
                                ),
                            })
                        }
                    })?;

                let mut data = Vec::new();
                while let Some(chunk) = reader.next().await.transpose().map_err(|e| {
                    CloudError::Gcs(GcsError::Sdk {
                        message: format!("Failed to read ranged GCS object body: {e}"),
                    })
                })? {
                    data.extend_from_slice(&chunk);
                }

                Ok(Bytes::from(data))
            })
            .await
    }

    fn supports_native_range_reads(&self) -> bool {
        true
    }

    async fn put(&self, key: &str, data: &[u8]) -> Result<()> {
        let mut executor = RetryExecutor::new(self.retry_config.clone());
        let data_owned = data.to_vec();

        executor
            .execute(|| async {
                let object_name = self.full_object_name(key);
                tracing::debug!(
                    "Putting GCS object: {}/{} ({} bytes)",
                    self.bucket,
                    object_name,
                    data_owned.len()
                );

                let client = self.create_storage_client().await?;
                let bucket_path = self.bucket_resource_name();
                let payload = bytes::Bytes::copy_from_slice(&data_owned);

                client
                    .write_object(&bucket_path, &object_name, payload)
                    .set_storage_class(self.storage_class.as_api_str())
                    .send_buffered()
                    .await
                    .map_err(|e| {
                        CloudError::Gcs(GcsError::Sdk {
                            message: format!(
                                "Failed to write GCS object '{}/{}': {e}",
                                self.bucket, object_name
                            ),
                        })
                    })?;

                Ok(())
            })
            .await
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let mut executor = RetryExecutor::new(self.retry_config.clone());

        executor
            .execute(|| async {
                let object_name = self.full_object_name(key);
                tracing::debug!("Deleting GCS object: {}/{}", self.bucket, object_name);

                let client = self.create_control_client().await?;

                client
                    .delete_object()
                    .set_bucket(&self.bucket)
                    .set_object(&object_name)
                    .send()
                    .await
                    .map_err(|e| {
                        CloudError::Gcs(GcsError::Sdk {
                            message: format!(
                                "Failed to delete GCS object '{}/{}': {e}",
                                self.bucket, object_name
                            ),
                        })
                    })?;

                Ok(())
            })
            .await
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let object_name = self.full_object_name(key);
        tracing::debug!(
            "Checking GCS object exists: {}/{}",
            self.bucket,
            object_name
        );

        let client = self.create_control_client().await?;

        match client
            .get_object()
            .set_bucket(&self.bucket)
            .set_object(&object_name)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => {
                let msg = format!("{e}");
                if Self::is_not_found(&msg) {
                    Ok(false)
                } else {
                    Err(CloudError::Gcs(GcsError::Sdk {
                        message: format!(
                            "Failed to check GCS object existence '{}/{}': {e}",
                            self.bucket, object_name
                        ),
                    }))
                }
            }
        }
    }

    async fn list_prefix(&self, prefix: &str) -> Result<Vec<String>> {
        // `ItemPaginator` must be in scope for `items.next().await` to work.
        use google_cloud_gax::paginator::ItemPaginator;

        let full_prefix = self.full_object_name(prefix);
        tracing::debug!(
            "Listing GCS objects: {} with prefix {}",
            self.bucket,
            full_prefix
        );

        let client = self.create_control_client().await?;
        let bucket_resource = self.bucket_resource_name();

        let mut items = client
            .list_objects()
            .set_parent(&bucket_resource)
            .set_prefix(&full_prefix)
            .by_item();

        let mut results = Vec::new();

        while let Some(item_result) = items.next().await {
            let obj = item_result.map_err(|e| {
                CloudError::Gcs(GcsError::Sdk {
                    message: format!("Failed to list GCS objects with prefix '{full_prefix}': {e}"),
                })
            })?;

            // Strip the bucket prefix so callers get paths relative to our configured prefix
            let relative_key = if !self.prefix.is_empty() {
                obj.name
                    .strip_prefix(&format!("{}/", self.prefix))
                    .unwrap_or(&obj.name)
                    .to_string()
            } else {
                obj.name
            };
            results.push(relative_key);
        }

        Ok(results)
    }

    fn is_readonly(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gcs_backend_new() {
        let backend = GcsBackend::new("my-bucket");
        assert_eq!(backend.bucket, "my-bucket");
        assert_eq!(backend.prefix, "");
    }

    #[test]
    fn test_gcs_backend_builder() {
        let backend = GcsBackend::new("my-bucket")
            .with_prefix("data/objects")
            .with_project_id("my-project-123")
            .with_storage_class(GcsStorageClass::Coldline)
            .with_timeout(Duration::from_secs(600));

        assert_eq!(backend.prefix, "data/objects");
        assert_eq!(backend.project_id, Some("my-project-123".to_string()));
        assert!(matches!(backend.storage_class, GcsStorageClass::Coldline));
        assert_eq!(backend.timeout, Duration::from_secs(600));
    }

    #[test]
    fn test_gcs_backend_full_object_name() {
        let backend = GcsBackend::new("bucket").with_prefix("prefix");
        assert_eq!(backend.full_object_name("file.txt"), "prefix/file.txt");

        let backend_no_prefix = GcsBackend::new("bucket");
        assert_eq!(backend_no_prefix.full_object_name("file.txt"), "file.txt");
    }

    #[test]
    fn test_gcs_backend_bucket_resource_name() {
        let backend = GcsBackend::new("my-bucket");
        assert_eq!(
            backend.bucket_resource_name(),
            "projects/_/buckets/my-bucket"
        );
    }

    #[test]
    fn test_storage_class_api_strings() {
        assert_eq!(GcsStorageClass::Standard.as_api_str(), "STANDARD");
        assert_eq!(GcsStorageClass::Nearline.as_api_str(), "NEARLINE");
        assert_eq!(GcsStorageClass::Coldline.as_api_str(), "COLDLINE");
        assert_eq!(GcsStorageClass::Archive.as_api_str(), "ARCHIVE");
    }

    #[cfg(all(feature = "gcs", feature = "async"))]
    #[test]
    fn test_google_credentials_none_when_unset() {
        let backend = GcsBackend::new("bucket");
        assert!(
            backend
                .google_credentials()
                .expect("should not error")
                .is_none()
        );

        let backend_explicit_none = GcsBackend::new("bucket").with_credentials(Credentials::None);
        assert!(
            backend_explicit_none
                .google_credentials()
                .expect("should not error")
                .is_none()
        );
    }

    #[cfg(all(feature = "gcs", feature = "async"))]
    #[tokio::test]
    async fn test_google_credentials_service_account_builds() {
        // `build()` only deserializes the JSON shape at this point -- it does
        // not validate the PEM contents of `private_key`, so a placeholder
        // key is sufficient to exercise the wiring end-to-end.
        let key_json = serde_json::json!({
            "client_email": "test@my-project.iam.gserviceaccount.com",
            "private_key_id": "test-key-id",
            "private_key": "-----BEGIN PRIVATE KEY-----\nBLAHBLAHBLAH\n-----END PRIVATE KEY-----\n",
            "project_id": "my-project",
        })
        .to_string();

        let backend = GcsBackend::new("bucket")
            .with_project_id("my-quota-project")
            .with_credentials(
                Credentials::service_account_from_json(key_json).expect("valid JSON"),
            );

        let creds = backend
            .google_credentials()
            .expect("service account credentials should build successfully");
        assert!(creds.is_some());
    }

    #[cfg(all(feature = "gcs", feature = "async"))]
    #[test]
    fn test_google_credentials_service_account_invalid_json_errors() {
        let backend = GcsBackend::new("bucket").with_credentials(Credentials::ServiceAccount {
            key_json: "not valid service-account json".to_string(),
            project_id: None,
        });

        let err = backend
            .google_credentials()
            .expect_err("malformed service account JSON should error");
        assert!(err.to_string().contains("Service account error"));
    }

    #[cfg(all(feature = "gcs", feature = "async"))]
    #[tokio::test]
    async fn test_google_credentials_api_key_builds() {
        let backend =
            GcsBackend::new("bucket").with_credentials(Credentials::api_key("my-api-key"));

        let creds = backend
            .google_credentials()
            .expect("API key credentials should build successfully");
        assert!(creds.is_some());
    }

    #[cfg(all(feature = "gcs", feature = "async"))]
    #[test]
    fn test_google_credentials_unsupported_variant_errors() {
        let backend =
            GcsBackend::new("bucket").with_credentials(Credentials::sas_token("sas-token"));

        let err = backend
            .google_credentials()
            .expect_err("SasToken is not a supported GCS credentials variant");
        let message = err.to_string();
        assert!(
            message.contains("SasToken"),
            "error should name the unsupported variant, got: {message}"
        );
    }
}
