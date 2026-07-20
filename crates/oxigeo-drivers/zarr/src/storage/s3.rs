//! S3 storage backend for Zarr arrays
//!
//! This module provides S3-compatible storage support for reading and writing
//! Zarr arrays directly to/from cloud object storage.

#[cfg(feature = "async")]
use super::AsyncStore;
use super::StoreKey;
use crate::error::{Result, StorageError, ZarrError};

#[cfg(feature = "s3")]
use aws_config::BehaviorVersion;
#[cfg(feature = "s3")]
use aws_sdk_s3::{Client, Config};
#[cfg(feature = "s3")]
use bytes::Bytes;

/// S3 storage backend configuration
#[derive(Debug, Clone)]
pub struct S3Storage {
    /// S3 bucket name
    pub bucket: String,
    /// Key prefix (path within bucket)
    pub prefix: String,
    /// AWS region
    pub region: Option<String>,
    /// Endpoint URL (for S3-compatible services)
    pub endpoint: Option<String>,
}

impl S3Storage {
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

    fn full_key(&self, key: &StoreKey) -> String {
        if self.prefix.is_empty() {
            key.as_str().to_string()
        } else {
            format!("{}/{}", self.prefix, key.as_str())
        }
    }

    #[cfg(feature = "s3")]
    async fn create_client(&self) -> Result<Client> {
        let mut config_loader = aws_config::defaults(BehaviorVersion::latest());

        if let Some(ref region) = self.region {
            config_loader = config_loader.region(aws_config::Region::new(region.clone()));
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
}

#[cfg(all(feature = "s3", feature = "async"))]
#[async_trait::async_trait]
impl AsyncStore for S3Storage {
    async fn get(&self, key: &StoreKey) -> Result<Vec<u8>> {
        let client = self.create_client().await?;
        let full_key = self.full_key(key);

        let response = client
            .get_object()
            .bucket(&self.bucket)
            .key(&full_key)
            .send()
            .await
            .map_err(|e| {
                ZarrError::Storage(StorageError::S3 {
                    message: format!("Failed to get object '{full_key}': {e}"),
                })
            })?;

        let data = response.body.collect().await.map_err(|e| {
            ZarrError::Storage(StorageError::S3 {
                message: format!("Failed to read object body: {e}"),
            })
        })?;

        Ok(data.into_bytes().to_vec())
    }

    async fn set(&mut self, key: &StoreKey, value: &[u8]) -> Result<()> {
        let client = self.create_client().await?;
        let full_key = self.full_key(key);

        client
            .put_object()
            .bucket(&self.bucket)
            .key(&full_key)
            .body(Bytes::from(value.to_vec()).into())
            .send()
            .await
            .map_err(|e| {
                ZarrError::Storage(StorageError::S3 {
                    message: format!("Failed to put object '{full_key}': {e}"),
                })
            })?;

        Ok(())
    }

    async fn delete(&mut self, key: &StoreKey) -> Result<()> {
        let client = self.create_client().await?;
        let full_key = self.full_key(key);

        client
            .delete_object()
            .bucket(&self.bucket)
            .key(&full_key)
            .send()
            .await
            .map_err(|e| {
                ZarrError::Storage(StorageError::S3 {
                    message: format!("Failed to delete object '{full_key}': {e}"),
                })
            })?;

        Ok(())
    }

    async fn exists(&self, key: &StoreKey) -> Result<bool> {
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
                    Err(ZarrError::Storage(StorageError::S3 {
                        message: format!("Failed to check object existence '{full_key}': {e}"),
                    }))
                }
            }
        }
    }

    async fn list_prefix(&self, prefix: &StoreKey) -> Result<Vec<StoreKey>> {
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
                ZarrError::Storage(StorageError::S3 {
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
                        results.push(StoreKey::new(relative_key));
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
    fn test_s3_storage_new() {
        let storage = S3Storage::new("my-bucket", "data/zarr");
        assert_eq!(storage.bucket, "my-bucket");
        assert_eq!(storage.prefix, "data/zarr");
    }

    #[test]
    fn test_s3_storage_with_region() {
        let storage = S3Storage::new("my-bucket", "").with_region("us-west-2");
        assert_eq!(storage.region, Some("us-west-2".to_string()));
    }

    #[test]
    fn test_s3_storage_with_endpoint() {
        let storage = S3Storage::new("my-bucket", "").with_endpoint("http://localhost:9000");
        assert_eq!(storage.endpoint, Some("http://localhost:9000".to_string()));
    }

    #[test]
    fn test_s3_storage_full_key() {
        let storage = S3Storage::new("my-bucket", "data/zarr");
        let key = StoreKey::new("array/.zarray".to_string());
        assert_eq!(storage.full_key(&key), "data/zarr/array/.zarray");

        let storage_no_prefix = S3Storage::new("my-bucket", "");
        assert_eq!(storage_no_prefix.full_key(&key), "array/.zarray");
    }

    #[test]
    fn test_s3_storage_builder_chain() {
        let storage = S3Storage::new("my-bucket", "zarr")
            .with_region("eu-west-1")
            .with_endpoint("https://s3.example.com");

        assert_eq!(storage.bucket, "my-bucket");
        assert_eq!(storage.prefix, "zarr");
        assert_eq!(storage.region, Some("eu-west-1".to_string()));
        assert_eq!(storage.endpoint, Some("https://s3.example.com".to_string()));
    }

    // Integration tests require real S3 or MinIO instance
    // These should be run with cargo test --features s3,async -- --ignored
    #[cfg(all(feature = "s3", feature = "async"))]
    #[tokio::test]
    #[ignore] // Requires S3/MinIO setup
    async fn test_s3_storage_roundtrip() {
        // This test requires environment variables:
        // - TEST_S3_BUCKET
        // - TEST_S3_ENDPOINT (optional, for MinIO)
        // - AWS_ACCESS_KEY_ID
        // - AWS_SECRET_ACCESS_KEY

        let bucket = match std::env::var("TEST_S3_BUCKET") {
            Ok(b) => b,
            Err(_) => return, // Skip if not configured
        };

        let mut storage = S3Storage::new(&bucket, "test/zarr");

        if let Ok(endpoint) = std::env::var("TEST_S3_ENDPOINT") {
            storage = storage.with_endpoint(endpoint);
        }

        let key = StoreKey::new("test_data.bin".to_string());
        let data = b"Hello, S3!";

        // Test set
        storage
            .set(&key, data)
            .await
            .expect("Failed to write to S3");

        // Test exists
        let exists = storage
            .exists(&key)
            .await
            .expect("Failed to check existence");
        assert!(exists);

        // Test get
        let retrieved = storage.get(&key).await.expect("Failed to read from S3");
        assert_eq!(retrieved, data);

        // Test delete
        storage
            .delete(&key)
            .await
            .expect("Failed to delete from S3");

        let exists_after = storage
            .exists(&key)
            .await
            .expect("Failed to check existence");
        assert!(!exists_after);
    }

    #[cfg(all(feature = "s3", feature = "async"))]
    #[tokio::test]
    #[ignore] // Requires S3/MinIO setup
    async fn test_s3_storage_list() {
        let bucket = match std::env::var("TEST_S3_BUCKET") {
            Ok(b) => b,
            Err(_) => return,
        };

        let mut storage = S3Storage::new(&bucket, "test/list");

        if let Ok(endpoint) = std::env::var("TEST_S3_ENDPOINT") {
            storage = storage.with_endpoint(endpoint);
        }

        // Create test files
        let key1 = StoreKey::new("file1.txt".to_string());
        let key2 = StoreKey::new("file2.txt".to_string());
        let key3 = StoreKey::new("subdir/file3.txt".to_string());

        storage.set(&key1, b"data1").await.ok();
        storage.set(&key2, b"data2").await.ok();
        storage.set(&key3, b"data3").await.ok();

        // List all
        let prefix = StoreKey::new("".to_string());
        let list = storage.list_prefix(&prefix).await.expect("Failed to list");

        assert!(list.len() >= 3);
        assert!(list.iter().any(|k| k.as_str() == "file1.txt"));
        assert!(list.iter().any(|k| k.as_str() == "file2.txt"));

        // Cleanup
        storage.delete(&key1).await.ok();
        storage.delete(&key2).await.ok();
        storage.delete(&key3).await.ok();
    }
}
