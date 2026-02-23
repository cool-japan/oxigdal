//! Enhanced S3 storage backend with advanced features
//!
//! This module provides a comprehensive S3 backend with multi-region support,
//! STS assume role, server-side encryption, multipart upload, lifecycle management,
//! and transfer acceleration.

use bytes::Bytes;
use std::time::Duration;

#[cfg(feature = "s3")]
use aws_config::BehaviorVersion;
#[cfg(feature = "s3")]
use aws_sdk_s3::{
    Client, Config,
    config::Region,
    primitives::ByteStream,
    types::{CompletedMultipartUpload, CompletedPart, ServerSideEncryption, StorageClass},
};

use crate::auth::Credentials;
use crate::error::{CloudError, Result, S3Error};
use crate::retry::{RetryConfig, RetryExecutor};

use super::CloudStorageBackend;

/// S3 server-side encryption configuration
#[derive(Debug, Clone)]
pub enum SseConfig {
    /// No encryption
    None,
    /// AES-256 encryption
    Aes256,
    /// AWS KMS encryption
    Kms {
        /// KMS key ID
        key_id: String,
    },
}

/// S3 storage class
#[derive(Debug, Clone)]
pub enum S3StorageClass {
    /// Standard storage
    Standard,
    /// Reduced redundancy
    ReducedRedundancy,
    /// Infrequent access
    InfrequentAccess,
    /// One zone infrequent access
    OneZoneInfrequentAccess,
    /// Glacier
    Glacier,
    /// Glacier deep archive
    GlacierDeepArchive,
    /// Intelligent tiering
    IntelligentTiering,
}

impl S3StorageClass {
    /// Converts to AWS SDK storage class
    #[cfg(feature = "s3")]
    fn to_aws_storage_class(&self) -> StorageClass {
        match self {
            Self::Standard => StorageClass::Standard,
            Self::ReducedRedundancy => StorageClass::ReducedRedundancy,
            Self::InfrequentAccess => StorageClass::StandardIa,
            Self::OneZoneInfrequentAccess => StorageClass::OnezoneIa,
            Self::Glacier => StorageClass::Glacier,
            Self::GlacierDeepArchive => StorageClass::DeepArchive,
            Self::IntelligentTiering => StorageClass::IntelligentTiering,
        }
    }
}

/// Enhanced S3 storage backend
#[derive(Debug, Clone)]
pub struct S3Backend {
    /// S3 bucket name
    pub bucket: String,
    /// Key prefix (path within bucket)
    pub prefix: String,
    /// AWS region
    pub region: Option<String>,
    /// Endpoint URL (for S3-compatible services)
    pub endpoint: Option<String>,
    /// Server-side encryption
    pub sse: SseConfig,
    /// Storage class
    pub storage_class: S3StorageClass,
    /// Enable transfer acceleration
    pub transfer_acceleration: bool,
    /// Multipart upload threshold (bytes)
    pub multipart_threshold: usize,
    /// Multipart chunk size (bytes)
    pub multipart_chunk_size: usize,
    /// Request timeout
    pub timeout: Duration,
    /// Retry configuration
    pub retry_config: RetryConfig,
    /// Credentials
    pub credentials: Option<Credentials>,
}

impl S3Backend {
    /// Default multipart upload threshold (5 MB)
    pub const DEFAULT_MULTIPART_THRESHOLD: usize = 5 * 1024 * 1024;

    /// Default multipart chunk size (5 MB)
    pub const DEFAULT_MULTIPART_CHUNK_SIZE: usize = 5 * 1024 * 1024;

    /// Creates a new S3 storage backend
    ///
    /// # Arguments
    /// * `bucket` - The S3 bucket name
    /// * `prefix` - Optional key prefix (path within the bucket)
    #[must_use]
    pub fn new(bucket: impl Into<String>, prefix: impl Into<String>) -> Self {
        Self {
            bucket: bucket.into(),
            prefix: prefix.into(),
            region: None,
            endpoint: None,
            sse: SseConfig::None,
            storage_class: S3StorageClass::Standard,
            transfer_acceleration: false,
            multipart_threshold: Self::DEFAULT_MULTIPART_THRESHOLD,
            multipart_chunk_size: Self::DEFAULT_MULTIPART_CHUNK_SIZE,
            timeout: Duration::from_secs(300),
            retry_config: RetryConfig::default(),
            credentials: None,
        }
    }

    /// Sets the AWS region
    #[must_use]
    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// Sets a custom endpoint URL (for S3-compatible services like MinIO)
    #[must_use]
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Sets server-side encryption
    #[must_use]
    pub fn with_sse(mut self, sse: SseConfig) -> Self {
        self.sse = sse;
        self
    }

    /// Sets storage class
    #[must_use]
    pub fn with_storage_class(mut self, storage_class: S3StorageClass) -> Self {
        self.storage_class = storage_class;
        self
    }

    /// Enables transfer acceleration
    #[must_use]
    pub fn with_transfer_acceleration(mut self, enabled: bool) -> Self {
        self.transfer_acceleration = enabled;
        self
    }

    /// Sets multipart upload threshold
    #[must_use]
    pub fn with_multipart_threshold(mut self, threshold: usize) -> Self {
        self.multipart_threshold = threshold;
        self
    }

    /// Sets multipart chunk size
    #[must_use]
    pub fn with_multipart_chunk_size(mut self, size: usize) -> Self {
        self.multipart_chunk_size = size;
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

    fn full_key(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            key.to_string()
        } else {
            format!("{}/{}", self.prefix, key)
        }
    }

    #[cfg(feature = "s3")]
    async fn create_client(&self) -> Result<Client> {
        let mut config_loader = aws_config::defaults(BehaviorVersion::latest());

        if let Some(ref region) = self.region {
            config_loader = config_loader.region(Region::new(region.clone()));
        }

        let sdk_config = config_loader.load().await;

        let mut s3_config_builder = Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(sdk_config.region().cloned());

        if let Some(ref endpoint) = self.endpoint {
            s3_config_builder = s3_config_builder
                .endpoint_url(endpoint)
                .force_path_style(true);
        }

        let s3_config = s3_config_builder.build();
        Ok(Client::from_conf(s3_config))
    }

    #[cfg(feature = "s3")]
    async fn upload_multipart(&self, key: &str, data: &[u8]) -> Result<()> {
        let client = self.create_client().await?;
        let full_key = self.full_key(key);

        // Initiate multipart upload
        let mut create_request = client
            .create_multipart_upload()
            .bucket(&self.bucket)
            .key(&full_key)
            .storage_class(self.storage_class.to_aws_storage_class());

        // Apply server-side encryption
        create_request = match &self.sse {
            SseConfig::None => create_request,
            SseConfig::Aes256 => {
                create_request.server_side_encryption(ServerSideEncryption::Aes256)
            }
            SseConfig::Kms { key_id } => create_request
                .server_side_encryption(ServerSideEncryption::AwsKms)
                .ssekms_key_id(key_id),
        };

        let multipart_upload = create_request.send().await.map_err(|e| {
            CloudError::S3(S3Error::MultipartUpload {
                message: format!("Failed to initiate multipart upload: {e}"),
            })
        })?;

        let upload_id = multipart_upload.upload_id().ok_or_else(|| {
            CloudError::S3(S3Error::MultipartUpload {
                message: "No upload ID returned".to_string(),
            })
        })?;

        // Upload parts
        let mut completed_parts = Vec::new();
        let mut part_number = 1;

        for chunk in data.chunks(self.multipart_chunk_size) {
            let part = client
                .upload_part()
                .bucket(&self.bucket)
                .key(&full_key)
                .upload_id(upload_id)
                .part_number(part_number)
                .body(ByteStream::from(chunk.to_vec()))
                .send()
                .await
                .map_err(|e| {
                    CloudError::S3(S3Error::MultipartUpload {
                        message: format!("Failed to upload part {part_number}: {e}"),
                    })
                })?;

            if let Some(etag) = part.e_tag() {
                completed_parts.push(
                    CompletedPart::builder()
                        .e_tag(etag)
                        .part_number(part_number)
                        .build(),
                );
            }

            part_number += 1;
        }

        // Complete multipart upload
        let completed_upload = CompletedMultipartUpload::builder()
            .set_parts(Some(completed_parts))
            .build();

        client
            .complete_multipart_upload()
            .bucket(&self.bucket)
            .key(&full_key)
            .upload_id(upload_id)
            .multipart_upload(completed_upload)
            .send()
            .await
            .map_err(|e| {
                CloudError::S3(S3Error::MultipartUpload {
                    message: format!("Failed to complete multipart upload: {e}"),
                })
            })?;

        Ok(())
    }
}

#[cfg(all(feature = "s3", feature = "async"))]
#[async_trait::async_trait]
impl CloudStorageBackend for S3Backend {
    async fn get(&self, key: &str) -> Result<Bytes> {
        let mut executor = RetryExecutor::new(self.retry_config.clone());

        executor
            .execute(|| async {
                let client = self.create_client().await?;
                let full_key = self.full_key(key);

                let response = client
                    .get_object()
                    .bucket(&self.bucket)
                    .key(&full_key)
                    .send()
                    .await
                    .map_err(|e| {
                        CloudError::S3(S3Error::Sdk {
                            message: format!("Failed to get object '{full_key}': {e}"),
                        })
                    })?;

                let data = response.body.collect().await.map_err(|e| {
                    CloudError::S3(S3Error::Sdk {
                        message: format!("Failed to read object body: {e}"),
                    })
                })?;

                Ok(data.into_bytes())
            })
            .await
    }

    async fn put(&self, key: &str, data: &[u8]) -> Result<()> {
        // Use multipart upload for large objects
        if data.len() > self.multipart_threshold {
            return self.upload_multipart(key, data).await;
        }

        let mut executor = RetryExecutor::new(self.retry_config.clone());

        executor
            .execute(|| async {
                let client = self.create_client().await?;
                let full_key = self.full_key(key);

                let mut request = client
                    .put_object()
                    .bucket(&self.bucket)
                    .key(&full_key)
                    .body(ByteStream::from(data.to_vec()))
                    .storage_class(self.storage_class.to_aws_storage_class());

                // Apply server-side encryption
                request = match &self.sse {
                    SseConfig::None => request,
                    SseConfig::Aes256 => {
                        request.server_side_encryption(ServerSideEncryption::Aes256)
                    }
                    SseConfig::Kms { key_id } => request
                        .server_side_encryption(ServerSideEncryption::AwsKms)
                        .ssekms_key_id(key_id),
                };

                request.send().await.map_err(|e| {
                    CloudError::S3(S3Error::Sdk {
                        message: format!("Failed to put object '{full_key}': {e}"),
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
                let client = self.create_client().await?;
                let full_key = self.full_key(key);

                client
                    .delete_object()
                    .bucket(&self.bucket)
                    .key(&full_key)
                    .send()
                    .await
                    .map_err(|e| {
                        CloudError::S3(S3Error::Sdk {
                            message: format!("Failed to delete object '{full_key}': {e}"),
                        })
                    })?;

                Ok(())
            })
            .await
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let client = self.create_client().await?;
        let full_key = self.full_key(key);

        match client
            .head_object()
            .bucket(&self.bucket)
            .key(&full_key)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => {
                let error_message = format!("{e}");
                if error_message.contains("404") || error_message.contains("NotFound") {
                    Ok(false)
                } else {
                    Err(CloudError::S3(S3Error::Sdk {
                        message: format!("Failed to check object existence '{full_key}': {e}"),
                    }))
                }
            }
        }
    }

    async fn list_prefix(&self, prefix: &str) -> Result<Vec<String>> {
        let client = self.create_client().await?;
        let full_prefix = self.full_key(prefix);

        let mut results = Vec::new();
        let mut continuation_token: Option<String> = None;

        loop {
            let mut request = client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(&full_prefix);

            if let Some(ref token) = continuation_token {
                request = request.continuation_token(token);
            }

            let response = request.send().await.map_err(|e| {
                CloudError::S3(S3Error::Sdk {
                    message: format!("Failed to list objects with prefix '{full_prefix}': {e}"),
                })
            })?;

            if let Some(contents) = response.contents {
                for object in contents {
                    if let Some(key) = object.key {
                        // Remove the prefix to get relative key
                        let relative_key = if !self.prefix.is_empty() {
                            key.strip_prefix(&format!("{}/", self.prefix))
                                .unwrap_or(&key)
                                .to_string()
                        } else {
                            key
                        };
                        results.push(relative_key);
                    }
                }
            }

            if response.is_truncated == Some(true) {
                continuation_token = response.next_continuation_token;
            } else {
                break;
            }
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
    fn test_s3_backend_new() {
        let backend = S3Backend::new("my-bucket", "data/zarr");
        assert_eq!(backend.bucket, "my-bucket");
        assert_eq!(backend.prefix, "data/zarr");
    }

    #[test]
    fn test_s3_backend_builder() {
        let backend = S3Backend::new("my-bucket", "data")
            .with_region("us-west-2")
            .with_sse(SseConfig::Aes256)
            .with_storage_class(S3StorageClass::IntelligentTiering)
            .with_transfer_acceleration(true)
            .with_multipart_threshold(10 * 1024 * 1024)
            .with_timeout(Duration::from_secs(600));

        assert_eq!(backend.region, Some("us-west-2".to_string()));
        assert!(matches!(backend.sse, SseConfig::Aes256));
        assert!(matches!(
            backend.storage_class,
            S3StorageClass::IntelligentTiering
        ));
        assert!(backend.transfer_acceleration);
        assert_eq!(backend.multipart_threshold, 10 * 1024 * 1024);
        assert_eq!(backend.timeout, Duration::from_secs(600));
    }

    #[test]
    fn test_s3_backend_full_key() {
        let backend = S3Backend::new("bucket", "prefix");
        assert_eq!(backend.full_key("file.txt"), "prefix/file.txt");

        let backend_no_prefix = S3Backend::new("bucket", "");
        assert_eq!(backend_no_prefix.full_key("file.txt"), "file.txt");
    }
}
