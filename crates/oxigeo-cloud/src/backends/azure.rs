//! Azure Blob Storage backend with comprehensive features
//!
//! This module provides Azure Blob Storage integration with SDK support,
//! blob operations, SAS token support, container management, and hierarchical namespace.

use bytes::Bytes;
use std::time::Duration;

use crate::auth::Credentials;
use crate::error::{AzureError, CloudError, Result};
use crate::retry::{RetryConfig, RetryExecutor};
use oxigeo_core::io::ByteRange;

use super::CloudStorageBackend;

/// Azure Blob Storage access tier
#[derive(Debug, Clone, Copy)]
pub enum AccessTier {
    /// Hot access tier
    Hot,
    /// Cool access tier
    Cool,
    /// Archive access tier
    Archive,
}

/// Azure Blob Storage backend
#[derive(Debug, Clone)]
pub struct AzureBlobBackend {
    /// Storage account name
    pub account_name: String,
    /// Container name
    pub container: String,
    /// Blob prefix (path within container)
    pub prefix: String,
    /// SAS token for authentication
    pub sas_token: Option<String>,
    /// Account key for authentication
    pub account_key: Option<String>,
    /// Access tier
    pub access_tier: AccessTier,
    /// Request timeout
    pub timeout: Duration,
    /// Retry configuration
    pub retry_config: RetryConfig,
    /// Credentials
    pub credentials: Option<Credentials>,
    /// Enable hierarchical namespace (Data Lake Gen2)
    pub hierarchical_namespace: bool,
}

impl AzureBlobBackend {
    /// Creates a new Azure Blob Storage backend
    ///
    /// # Arguments
    /// * `account_name` - The Azure storage account name
    /// * `container` - The container name
    #[must_use]
    pub fn new(account_name: impl Into<String>, container: impl Into<String>) -> Self {
        Self {
            account_name: account_name.into(),
            container: container.into(),
            prefix: String::new(),
            sas_token: None,
            account_key: None,
            access_tier: AccessTier::Hot,
            timeout: Duration::from_secs(300),
            retry_config: RetryConfig::default(),
            credentials: None,
            hierarchical_namespace: false,
        }
    }

    /// Sets the blob prefix
    #[must_use]
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// Sets SAS token authentication
    #[must_use]
    pub fn with_sas_token(mut self, token: impl Into<String>) -> Self {
        self.sas_token = Some(token.into());
        self
    }

    /// Sets account key authentication
    #[must_use]
    pub fn with_account_key(mut self, key: impl Into<String>) -> Self {
        self.account_key = Some(key.into());
        self
    }

    /// Sets access tier
    #[must_use]
    pub fn with_access_tier(mut self, tier: AccessTier) -> Self {
        self.access_tier = tier;
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

    /// Enables hierarchical namespace (Data Lake Gen2)
    #[must_use]
    pub fn with_hierarchical_namespace(mut self, enabled: bool) -> Self {
        self.hierarchical_namespace = enabled;
        self
    }

    fn full_blob_name(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            key.to_string()
        } else {
            format!("{}/{}", self.prefix, key)
        }
    }

    /// Gets the blob endpoint URL
    fn get_blob_endpoint(&self) -> String {
        if self.hierarchical_namespace {
            format!("https://{}.dfs.core.windows.net", self.account_name)
        } else {
            format!("https://{}.blob.core.windows.net", self.account_name)
        }
    }
}

// The azure_storage_blobs 0.21 crate provides a Pure Rust implementation.
// Authentication is performed via `StorageCredentials` (access_key, sas_token,
// bearer_token, or anonymous).  We prefer account_key when available, falling
// back to sas_token and finally anonymous.

#[cfg(all(feature = "azure-blob", feature = "async"))]
use azure_storage::{CloudLocation, StorageCredentials};
#[cfg(all(feature = "azure-blob", feature = "async"))]
use azure_storage_blobs::prelude::{AccessTier as SdkAccessTier, ClientBuilder};

#[cfg(all(feature = "azure-blob", feature = "async"))]
impl From<AccessTier> for SdkAccessTier {
    fn from(tier: AccessTier) -> Self {
        match tier {
            AccessTier::Hot => SdkAccessTier::Hot,
            AccessTier::Cool => SdkAccessTier::Cool,
            AccessTier::Archive => SdkAccessTier::Archive,
        }
    }
}

#[cfg(all(feature = "azure-blob", feature = "async"))]
impl AzureBlobBackend {
    /// Builds `StorageCredentials` from the backend's auth fields.
    fn storage_credentials(&self) -> Result<StorageCredentials> {
        if let Some(ref key) = self.account_key {
            // `StorageCredentials::access_key` accepts `K: Into<Secret>` where
            // `Secret` is from `azure_core 0.21` (re-exported by `azure_storage`).
            // Passing a `String` works because `azure_core 0.21` implements
            // `From<String> for Secret`.
            Ok(StorageCredentials::access_key(
                self.account_name.clone(),
                key.clone(),
            ))
        } else if let Some(ref token) = self.sas_token {
            StorageCredentials::sas_token(token).map_err(|e| {
                CloudError::Azure(AzureError::InvalidSasToken {
                    message: format!("Invalid SAS token: {e}"),
                })
            })
        } else {
            // Fall back to anonymous credentials — operations will fail unless
            // the container has public access enabled.
            Ok(StorageCredentials::anonymous())
        }
    }

    /// Returns a `ClientBuilder` pre-configured for the backend.
    ///
    /// When [`hierarchical_namespace`](Self::with_hierarchical_namespace) is
    /// enabled, requests are routed to the account's `*.dfs.core.windows.net`
    /// Data Lake Gen2 endpoint instead of the default `*.blob.core.windows.net`
    /// endpoint.
    fn client_builder(&self) -> Result<ClientBuilder> {
        let creds = self.storage_credentials()?;
        let cloud_location = if self.hierarchical_namespace {
            CloudLocation::Custom {
                account: self.account_name.clone(),
                uri: self.get_blob_endpoint(),
            }
        } else {
            CloudLocation::Public {
                account: self.account_name.clone(),
            }
        };
        Ok(ClientBuilder::with_location(cloud_location, creds))
    }

    /// Checks whether an error string represents a 404 / not-found condition.
    ///
    /// `azure_storage_blobs` 0.21 uses `azure_core` 0.21 internally while the
    /// workspace depends on `azure_core` 1.0; we cannot call the 0.21
    /// `as_http_error()` method directly from this crate.  String matching on
    /// the formatted error is the safe cross-version approach (mirrors the S3
    /// backend's pattern).
    fn is_not_found(err_msg: &str) -> bool {
        err_msg.contains("404")
            || err_msg.contains("NotFound")
            || err_msg.contains("BlobNotFound")
            || err_msg.contains("ContainerNotFound")
            || err_msg.contains("ResourceNotFound")
    }
}

#[cfg(all(feature = "azure-blob", feature = "async"))]
#[async_trait::async_trait]
impl CloudStorageBackend for AzureBlobBackend {
    async fn get(&self, key: &str) -> Result<Bytes> {
        let mut executor = RetryExecutor::new(self.retry_config.clone());

        executor
            .execute(|| async {
                let blob_name = self.full_blob_name(key);
                tracing::debug!(
                    "Getting blob: {}/{}/{}",
                    self.account_name,
                    self.container,
                    blob_name
                );

                let builder = self.client_builder()?;
                let blob_client = builder
                    .blob_service_client()
                    .container_client(&self.container)
                    .blob_client(&blob_name);

                let data = blob_client.get_content().await.map_err(|e| {
                    let msg = format!("{e}");
                    if Self::is_not_found(&msg) {
                        CloudError::Azure(AzureError::BlobNotFound {
                            blob: format!("{}/{}", self.container, blob_name),
                        })
                    } else {
                        CloudError::Azure(AzureError::Sdk {
                            message: format!(
                                "Failed to get blob '{}/{}/{}': {e}",
                                self.account_name, self.container, blob_name
                            ),
                        })
                    }
                })?;

                Ok(Bytes::from(data))
            })
            .await
    }

    async fn get_range(&self, key: &str, range: ByteRange) -> Result<Bytes> {
        use futures::StreamExt;

        if range.is_empty() {
            return Ok(Bytes::new());
        }

        let mut executor = RetryExecutor::new(self.retry_config.clone());
        let start = range.start;
        let end = range.end;

        executor
            .execute(|| async {
                let blob_name = self.full_blob_name(key);
                tracing::debug!(
                    "Getting blob range {}..{} of {}/{}/{}",
                    start,
                    end,
                    self.account_name,
                    self.container,
                    blob_name
                );

                let builder = self.client_builder()?;
                let blob_client = builder
                    .blob_service_client()
                    .container_client(&self.container)
                    .blob_client(&blob_name);

                // `GetBlobBuilder::range` takes `impl Into<azure_core::request_options::Range>`.
                // We can't name that type directly (see `is_not_found` doc
                // comment above: `azure_storage_blobs` 0.21 pulls in its own
                // private `azure_core` 0.21, distinct from this workspace's
                // `azure_core` 1.0), but a plain `std::ops::Range<u64>`
                // implements `Into<Range>` for that crate, so we pass one
                // directly without ever naming the target type.
                let mut stream = blob_client.get().range(start..end).into_stream();

                let mut data = Vec::with_capacity((end - start) as usize);
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk.map_err(|e| {
                        let msg = format!("{e}");
                        if Self::is_not_found(&msg) {
                            CloudError::Azure(AzureError::BlobNotFound {
                                blob: format!("{}/{}", self.container, blob_name),
                            })
                        } else {
                            CloudError::Azure(AzureError::Sdk {
                                message: format!(
                                    "Failed to get byte range {}..{} of blob '{}/{}/{}': {e}",
                                    start, end, self.account_name, self.container, blob_name
                                ),
                            })
                        }
                    })?;

                    let body = chunk.data.collect().await.map_err(|e| {
                        CloudError::Azure(AzureError::Sdk {
                            message: format!("Failed to read ranged blob body: {e}"),
                        })
                    })?;
                    data.extend_from_slice(&body);
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
                let blob_name = self.full_blob_name(key);
                tracing::debug!(
                    "Putting blob: {}/{}/{} ({} bytes)",
                    self.account_name,
                    self.container,
                    blob_name,
                    data_owned.len()
                );

                let builder = self.client_builder()?;
                let blob_client = builder
                    .blob_service_client()
                    .container_client(&self.container)
                    .blob_client(&blob_name);

                blob_client
                    .put_block_blob(bytes::Bytes::copy_from_slice(&data_owned))
                    .content_type("application/octet-stream")
                    .access_tier(SdkAccessTier::from(self.access_tier))
                    .await
                    .map_err(|e| {
                        CloudError::Azure(AzureError::Sdk {
                            message: format!(
                                "Failed to put blob '{}/{}/{}': {e}",
                                self.account_name, self.container, blob_name
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
                let blob_name = self.full_blob_name(key);
                tracing::debug!(
                    "Deleting blob: {}/{}/{}",
                    self.account_name,
                    self.container,
                    blob_name
                );

                let builder = self.client_builder()?;
                let blob_client = builder
                    .blob_service_client()
                    .container_client(&self.container)
                    .blob_client(&blob_name);

                blob_client.delete().await.map_err(|e| {
                    CloudError::Azure(AzureError::Sdk {
                        message: format!(
                            "Failed to delete blob '{}/{}/{}': {e}",
                            self.account_name, self.container, blob_name
                        ),
                    })
                })?;

                Ok(())
            })
            .await
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let blob_name = self.full_blob_name(key);
        tracing::debug!(
            "Checking blob exists: {}/{}/{}",
            self.account_name,
            self.container,
            blob_name
        );

        let builder = self.client_builder()?;
        let blob_client = builder
            .blob_service_client()
            .container_client(&self.container)
            .blob_client(&blob_name);

        // `blob_client.exists()` internally calls `get_properties()` and maps
        // a 404 response to `Ok(false)` using azure_core 0.21's typed error API,
        // so we only need to translate non-404 errors into our error type.
        blob_client.exists().await.map_err(|e| {
            CloudError::Azure(AzureError::Sdk {
                message: format!(
                    "Failed to check blob existence '{}/{}/{}': {e}",
                    self.account_name, self.container, blob_name
                ),
            })
        })
    }

    async fn list_prefix(&self, prefix: &str) -> Result<Vec<String>> {
        use futures::StreamExt;

        let full_prefix = self.full_blob_name(prefix);
        tracing::debug!(
            "Listing blobs: {}/{} with prefix {}",
            self.account_name,
            self.container,
            full_prefix
        );

        let builder = self.client_builder()?;
        let container_client = builder
            .blob_service_client()
            .container_client(&self.container);

        let mut stream = container_client
            .list_blobs()
            .prefix(full_prefix.clone())
            .into_stream();

        let mut results = Vec::new();

        while let Some(page_result) = stream.next().await {
            let page = page_result.map_err(|e| {
                CloudError::Azure(AzureError::Sdk {
                    message: format!("Failed to list blobs with prefix '{full_prefix}': {e}"),
                })
            })?;

            for blob in page.blobs.blobs() {
                // Strip the configured prefix so callers get paths relative to it.
                let relative_key = if !self.prefix.is_empty() {
                    blob.name
                        .strip_prefix(&format!("{}/", self.prefix))
                        .unwrap_or(&blob.name)
                        .to_string()
                } else {
                    blob.name.clone()
                };
                results.push(relative_key);
            }
        }

        Ok(results)
    }

    fn is_readonly(&self) -> bool {
        // If only SAS token is provided, check if it has write permissions.
        // For now, assume not readonly.
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_azure_backend_new() {
        let backend = AzureBlobBackend::new("myaccount", "mycontainer");
        assert_eq!(backend.account_name, "myaccount");
        assert_eq!(backend.container, "mycontainer");
        assert_eq!(backend.prefix, "");
    }

    #[test]
    fn test_azure_backend_builder() {
        let backend = AzureBlobBackend::new("myaccount", "mycontainer")
            .with_prefix("data/blobs")
            .with_sas_token("?sv=2020-08-04&ss=bfqt")
            .with_access_tier(AccessTier::Cool)
            .with_hierarchical_namespace(true)
            .with_timeout(Duration::from_secs(600));

        assert_eq!(backend.prefix, "data/blobs");
        assert!(backend.sas_token.is_some());
        assert!(matches!(backend.access_tier, AccessTier::Cool));
        assert!(backend.hierarchical_namespace);
        assert_eq!(backend.timeout, Duration::from_secs(600));
    }

    #[test]
    fn test_azure_backend_full_blob_name() {
        let backend = AzureBlobBackend::new("account", "container").with_prefix("prefix");
        assert_eq!(backend.full_blob_name("file.txt"), "prefix/file.txt");

        let backend_no_prefix = AzureBlobBackend::new("account", "container");
        assert_eq!(backend_no_prefix.full_blob_name("file.txt"), "file.txt");
    }

    #[test]
    fn test_azure_backend_blob_endpoint() {
        let backend = AzureBlobBackend::new("myaccount", "container");
        assert_eq!(
            backend.get_blob_endpoint(),
            "https://myaccount.blob.core.windows.net"
        );

        let backend_dfs = backend.with_hierarchical_namespace(true);
        assert_eq!(
            backend_dfs.get_blob_endpoint(),
            "https://myaccount.dfs.core.windows.net"
        );
    }

    #[cfg(all(feature = "azure-blob", feature = "async"))]
    #[test]
    fn test_client_builder_uses_public_endpoint_by_default() {
        let backend = AzureBlobBackend::new("myaccount", "container")
            .with_account_key("dGVzdC1hY2NvdW50LWtleQ==");

        let builder = backend
            .client_builder()
            .expect("client_builder should succeed with account key credentials");
        let url = builder
            .blob_service_client()
            .url()
            .expect("service client should produce a URL");

        assert_eq!(
            url.host_str(),
            Some("myaccount.blob.core.windows.net"),
            "default (non-hierarchical) backend must hit the standard blob endpoint"
        );
    }

    #[cfg(all(feature = "azure-blob", feature = "async"))]
    #[test]
    fn test_client_builder_uses_dfs_endpoint_when_hierarchical_namespace_enabled() {
        let backend = AzureBlobBackend::new("myaccount", "container")
            .with_account_key("dGVzdC1hY2NvdW50LWtleQ==")
            .with_hierarchical_namespace(true);

        let builder = backend
            .client_builder()
            .expect("client_builder should succeed with account key credentials");
        let url = builder
            .blob_service_client()
            .url()
            .expect("service client should produce a URL");

        assert_eq!(
            url.host_str(),
            Some("myaccount.dfs.core.windows.net"),
            "hierarchical_namespace(true) must route to the Data Lake Gen2 endpoint"
        );
    }

    #[cfg(all(feature = "azure-blob", feature = "async"))]
    #[test]
    fn test_access_tier_conversion_to_sdk_type() {
        assert!(matches!(
            SdkAccessTier::from(AccessTier::Hot),
            SdkAccessTier::Hot
        ));
        assert!(matches!(
            SdkAccessTier::from(AccessTier::Cool),
            SdkAccessTier::Cool
        ));
        assert!(matches!(
            SdkAccessTier::from(AccessTier::Archive),
            SdkAccessTier::Archive
        ));
    }
}
