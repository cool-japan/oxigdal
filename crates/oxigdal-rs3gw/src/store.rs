//! Zarr Store implementation for rs3gw
//!
//! This module provides Zarr Store trait implementations that use rs3gw
//! as the backend storage, enabling efficient cloud-based Zarr array access.

use crate::error::{Result, Rs3gwError};
use bytes::Bytes;
use rs3gw::storage::backend::DynBackend;

// Re-export Zarr types if the zarr driver is available
// Note: This is feature-gated to avoid circular dependencies
#[cfg(feature = "zarr")]
use oxigdal_zarr::storage::{Store, StoreKey};

/// Rs3gw-backed Zarr store
///
/// This store implementation uses rs3gw for all storage operations,
/// providing high-performance access to Zarr arrays stored in cloud storage.
///
/// # Features
/// - Supports all rs3gw backends (Local, S3, MinIO, GCS, Azure)
/// - Optional deduplication for reducing storage costs
/// - Optional ML-based caching for predictive prefetching
/// - Optional encryption at rest
#[derive(Clone)]
pub struct Rs3gwStore {
    /// The storage backend
    storage: DynBackend,
    /// Bucket name
    bucket: String,
    /// Key prefix for this store
    prefix: String,
    /// Whether this store is read-only
    readonly: bool,
}

impl Rs3gwStore {
    /// Creates a new rs3gw Zarr store
    ///
    /// # Arguments
    /// * `storage` - The storage backend to use
    /// * `bucket` - The bucket name
    /// * `prefix` - The key prefix (typically the Zarr array path)
    pub fn new(storage: DynBackend, bucket: String, prefix: String) -> Self {
        Self {
            storage,
            bucket,
            prefix,
            readonly: false,
        }
    }

    /// Creates a read-only store
    pub fn readonly(storage: DynBackend, bucket: String, prefix: String) -> Self {
        Self {
            storage,
            bucket,
            prefix,
            readonly: true,
        }
    }

    /// Converts a StoreKey to a full object key
    fn to_object_key(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            key.to_string()
        } else {
            format!("{}/{}", self.prefix.trim_end_matches('/'), key)
        }
    }

    /// Ensures the bucket exists (for write operations)
    async fn ensure_bucket(&self) -> Result<()> {
        if !self
            .storage
            .bucket_exists(&self.bucket)
            .await
            .map_err(Rs3gwError::from)?
        {
            self.storage
                .create_bucket(&self.bucket)
                .await
                .map_err(Rs3gwError::from)?;
        }
        Ok(())
    }
}

#[cfg(feature = "zarr")]
impl Store for Rs3gwStore {
    fn exists(&self, key: &StoreKey) -> oxigdal_zarr::error::Result<bool> {
        let object_key = self.to_object_key(key.as_str());
        let storage = self.storage.clone();
        let bucket = self.bucket.clone();

        // Try to get current runtime handle
        match tokio::runtime::Handle::try_current() {
            Ok(_handle) => {
                // We're already in a tokio runtime, use block_in_place
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async move {
                        storage
                            .head_object(&bucket, &object_key)
                            .await
                            .map(|_| true)
                            .or_else(|e| match e {
                                rs3gw::storage::StorageError::NotFound(_) => Ok(false),
                                rs3gw::storage::StorageError::BucketNotFound => Ok(false),
                                other => Err(map_storage_error(other)),
                            })
                    })
                })
            }
            Err(_) => {
                // No runtime exists, create a new one
                let rt = tokio::runtime::Runtime::new().map_err(|e| {
                    map_rs3gw_error(Rs3gwError::Io(std::io::Error::other(format!(
                        "Failed to create tokio runtime: {e}"
                    ))))
                })?;

                rt.block_on(async move {
                    storage
                        .head_object(&bucket, &object_key)
                        .await
                        .map(|_| true)
                        .or_else(|e| match e {
                            rs3gw::storage::StorageError::NotFound(_) => Ok(false),
                            rs3gw::storage::StorageError::BucketNotFound => Ok(false),
                            other => Err(map_storage_error(other)),
                        })
                })
            }
        }
    }

    fn get(&self, key: &StoreKey) -> oxigdal_zarr::error::Result<Vec<u8>> {
        let object_key = self.to_object_key(key.as_str());
        let storage = self.storage.clone();
        let bucket = self.bucket.clone();

        // Try to get current runtime handle
        match tokio::runtime::Handle::try_current() {
            Ok(_handle) => {
                // We're already in a tokio runtime, use block_in_place
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async move {
                        let (_metadata, data) = storage
                            .get_object(&bucket, &object_key, None)
                            .await
                            .map_err(map_storage_error)?;

                        Ok(data.to_vec())
                    })
                })
            }
            Err(_) => {
                // No runtime exists, create a new one
                let rt = tokio::runtime::Runtime::new().map_err(|e| {
                    map_rs3gw_error(Rs3gwError::Io(std::io::Error::other(format!(
                        "Failed to create tokio runtime: {e}"
                    ))))
                })?;

                rt.block_on(async move {
                    let (_metadata, data) = storage
                        .get_object(&bucket, &object_key, None)
                        .await
                        .map_err(map_storage_error)?;

                    Ok(data.to_vec())
                })
            }
        }
    }

    fn set(&mut self, key: &StoreKey, value: &[u8]) -> oxigdal_zarr::error::Result<()> {
        if self.readonly {
            return Err(oxigdal_zarr::error::ZarrError::Storage(
                oxigdal_zarr::error::StorageError::ReadOnly,
            ));
        }

        let object_key = self.to_object_key(key.as_str());
        let data = Bytes::copy_from_slice(value);
        let storage = self.storage.clone();
        let bucket = self.bucket.clone();

        // Try to get current runtime handle
        match tokio::runtime::Handle::try_current() {
            Ok(_handle) => {
                // We're already in a tokio runtime, use block_in_place
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async move {
                        // Ensure bucket exists
                        if !storage
                            .bucket_exists(&bucket)
                            .await
                            .map_err(map_storage_error)?
                        {
                            storage
                                .create_bucket(&bucket)
                                .await
                                .map_err(map_storage_error)?;
                        }

                        storage
                            .put_object(
                                &bucket,
                                &object_key,
                                data,
                                std::collections::HashMap::new(),
                            )
                            .await
                            .map_err(map_storage_error)?;

                        Ok(())
                    })
                })
            }
            Err(_) => {
                // No runtime exists, create a new one
                let rt = tokio::runtime::Runtime::new().map_err(|e| {
                    map_rs3gw_error(Rs3gwError::Io(std::io::Error::other(format!(
                        "Failed to create tokio runtime: {e}"
                    ))))
                })?;

                rt.block_on(async move {
                    // Ensure bucket exists
                    if !storage
                        .bucket_exists(&bucket)
                        .await
                        .map_err(map_storage_error)?
                    {
                        storage
                            .create_bucket(&bucket)
                            .await
                            .map_err(map_storage_error)?;
                    }

                    storage
                        .put_object(&bucket, &object_key, data, std::collections::HashMap::new())
                        .await
                        .map_err(map_storage_error)?;

                    Ok(())
                })
            }
        }
    }

    fn delete(&mut self, key: &StoreKey) -> oxigdal_zarr::error::Result<()> {
        if self.readonly {
            return Err(oxigdal_zarr::error::ZarrError::Storage(
                oxigdal_zarr::error::StorageError::ReadOnly,
            ));
        }

        let object_key = self.to_object_key(key.as_str());
        let storage = self.storage.clone();
        let bucket = self.bucket.clone();

        // Try to get current runtime handle
        match tokio::runtime::Handle::try_current() {
            Ok(_handle) => {
                // We're already in a tokio runtime, use block_in_place
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async move {
                        storage
                            .delete_object(&bucket, &object_key)
                            .await
                            .map_err(map_storage_error)?;

                        Ok(())
                    })
                })
            }
            Err(_) => {
                // No runtime exists, create a new one
                let rt = tokio::runtime::Runtime::new().map_err(|e| {
                    map_rs3gw_error(Rs3gwError::Io(std::io::Error::other(format!(
                        "Failed to create tokio runtime: {e}"
                    ))))
                })?;

                rt.block_on(async move {
                    storage
                        .delete_object(&bucket, &object_key)
                        .await
                        .map_err(map_storage_error)?;

                    Ok(())
                })
            }
        }
    }

    fn list_prefix(&self, prefix: &StoreKey) -> oxigdal_zarr::error::Result<Vec<StoreKey>> {
        let search_prefix = self.to_object_key(prefix.as_str());
        let storage = self.storage.clone();
        let bucket = self.bucket.clone();
        let store_prefix = self.prefix.clone();

        // Try to get current runtime handle
        match tokio::runtime::Handle::try_current() {
            Ok(_handle) => {
                // We're already in a tokio runtime, use block_in_place
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async move {
                        let mut keys = Vec::new();
                        let mut continuation_token: Option<String> = None;

                        loop {
                            let result = storage
                                .list_objects(
                                    &bucket,
                                    Some(&search_prefix),
                                    None,
                                    1000,
                                    continuation_token.as_deref(),
                                )
                                .await
                                .map_err(map_storage_error)?;

                            for (key, _) in result.objects {
                                // Strip the store prefix to get the relative key
                                let relative_key = if !store_prefix.is_empty() {
                                    key.strip_prefix(&format!(
                                        "{}/",
                                        store_prefix.trim_end_matches('/')
                                    ))
                                    .unwrap_or(&key)
                                } else {
                                    &key
                                };
                                keys.push(StoreKey::new(relative_key.to_string()));
                            }

                            if !result.is_truncated {
                                break;
                            }

                            continuation_token = result.next_continuation_token;
                        }

                        Ok(keys)
                    })
                })
            }
            Err(_) => {
                // No runtime exists, create a new one
                let rt = tokio::runtime::Runtime::new().map_err(|e| {
                    map_rs3gw_error(Rs3gwError::Io(std::io::Error::other(format!(
                        "Failed to create tokio runtime: {e}"
                    ))))
                })?;

                rt.block_on(async move {
                    let mut keys = Vec::new();
                    let mut continuation_token: Option<String> = None;

                    loop {
                        let result = storage
                            .list_objects(
                                &bucket,
                                Some(&search_prefix),
                                None,
                                1000,
                                continuation_token.as_deref(),
                            )
                            .await
                            .map_err(map_storage_error)?;

                        for (key, _) in result.objects {
                            // Strip the store prefix to get the relative key
                            let relative_key = if !store_prefix.is_empty() {
                                key.strip_prefix(&format!(
                                    "{}/",
                                    store_prefix.trim_end_matches('/')
                                ))
                                .unwrap_or(&key)
                            } else {
                                &key
                            };
                            keys.push(StoreKey::new(relative_key.to_string()));
                        }

                        if !result.is_truncated {
                            break;
                        }

                        continuation_token = result.next_continuation_token;
                    }

                    Ok(keys)
                })
            }
        }
    }

    fn is_readonly(&self) -> bool {
        self.readonly
    }

    fn flush(&mut self) -> oxigdal_zarr::error::Result<()> {
        // No-op for rs3gw (writes are synchronous)
        Ok(())
    }
}

#[cfg(all(feature = "zarr", feature = "async"))]
mod async_impl {
    use super::*;
    use oxigdal_zarr::storage::AsyncStore;

    #[async_trait::async_trait]
    impl AsyncStore for Rs3gwStore {
        async fn exists(&self, key: &StoreKey) -> oxigdal_zarr::error::Result<bool> {
            let object_key = self.to_object_key(key.as_str());

            self.storage
                .head_object(&self.bucket, &object_key)
                .await
                .map(|_| true)
                .or_else(|e| match e {
                    rs3gw::storage::StorageError::NotFound(_) => Ok(false),
                    rs3gw::storage::StorageError::BucketNotFound => Ok(false),
                    other => Err(map_storage_error(other)),
                })
        }

        async fn get(&self, key: &StoreKey) -> oxigdal_zarr::error::Result<Vec<u8>> {
            let object_key = self.to_object_key(key.as_str());

            let (_metadata, data) = self
                .storage
                .get_object(&self.bucket, &object_key, None)
                .await
                .map_err(map_storage_error)?;

            Ok(data.to_vec())
        }

        async fn set(&mut self, key: &StoreKey, value: &[u8]) -> oxigdal_zarr::error::Result<()> {
            if self.readonly {
                return Err(oxigdal_zarr::error::ZarrError::Storage(
                    oxigdal_zarr::error::StorageError::ReadOnly,
                ));
            }

            let object_key = self.to_object_key(key.as_str());
            let data = Bytes::copy_from_slice(value);

            // Ensure bucket exists
            self.ensure_bucket().await.map_err(map_rs3gw_error)?;

            self.storage
                .put_object(
                    &self.bucket,
                    &object_key,
                    data,
                    std::collections::HashMap::new(),
                )
                .await
                .map_err(map_storage_error)?;

            Ok(())
        }

        async fn delete(&mut self, key: &StoreKey) -> oxigdal_zarr::error::Result<()> {
            if self.readonly {
                return Err(oxigdal_zarr::error::ZarrError::Storage(
                    oxigdal_zarr::error::StorageError::ReadOnly,
                ));
            }

            let object_key = self.to_object_key(key.as_str());

            self.storage
                .delete_object(&self.bucket, &object_key)
                .await
                .map_err(map_storage_error)?;

            Ok(())
        }

        async fn list_prefix(
            &self,
            prefix: &StoreKey,
        ) -> oxigdal_zarr::error::Result<Vec<StoreKey>> {
            let search_prefix = self.to_object_key(prefix.as_str());

            let mut keys = Vec::new();
            let mut continuation_token: Option<String> = None;

            loop {
                let result = self
                    .storage
                    .list_objects(
                        &self.bucket,
                        Some(&search_prefix),
                        None,
                        1000,
                        continuation_token.as_deref(),
                    )
                    .await
                    .map_err(map_storage_error)?;

                for (key, _) in result.objects {
                    // Strip the store prefix to get the relative key
                    let relative_key = if !self.prefix.is_empty() {
                        key.strip_prefix(&format!("{}/", self.prefix.trim_end_matches('/')))
                            .unwrap_or(&key)
                    } else {
                        &key
                    };
                    keys.push(StoreKey::new(relative_key.to_string()));
                }

                if !result.is_truncated {
                    break;
                }

                continuation_token = result.next_continuation_token;
            }

            Ok(keys)
        }

        async fn flush(&mut self) -> oxigdal_zarr::error::Result<()> {
            // No-op for rs3gw (writes are synchronous)
            Ok(())
        }
    }
}

impl std::fmt::Debug for Rs3gwStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rs3gwStore")
            .field("bucket", &self.bucket)
            .field("prefix", &self.prefix)
            .field("readonly", &self.readonly)
            .finish()
    }
}

// Helper functions for error mapping

#[cfg(feature = "zarr")]
fn map_storage_error(err: rs3gw::storage::StorageError) -> oxigdal_zarr::error::ZarrError {
    use oxigdal_zarr::error::{StorageError, ZarrError};

    match err {
        rs3gw::storage::StorageError::NotFound(path) => {
            ZarrError::Storage(StorageError::KeyNotFound { key: path })
        }
        other => ZarrError::Storage(StorageError::Network {
            message: other.to_string(),
        }),
    }
}

#[cfg(feature = "zarr")]
fn map_rs3gw_error(err: Rs3gwError) -> oxigdal_zarr::error::ZarrError {
    use oxigdal_zarr::error::{StorageError, ZarrError};

    match err {
        Rs3gwError::ObjectNotFound { key, .. } => {
            ZarrError::Storage(StorageError::KeyNotFound { key })
        }
        Rs3gwError::Io(e) => ZarrError::Storage(StorageError::Network {
            message: e.to_string(),
        }),
        other => ZarrError::Storage(StorageError::Network {
            message: other.to_string(),
        }),
    }
}

#[cfg(test)]
#[cfg(feature = "zarr")]
mod tests {
    use super::*;
    use rs3gw::storage::backend::{BackendConfig, BackendType};
    use tempfile::TempDir;

    async fn create_test_store() -> (Rs3gwStore, TempDir) {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let storage_root = temp_dir.path().to_path_buf();

        let config = BackendConfig {
            backend_type: BackendType::Local,
            endpoint: None,
            access_key: None,
            secret_key: None,
            region: None,
            use_ssl: false,
            extra: std::collections::HashMap::new(),
        };

        let backend =
            rs3gw::storage::backend::create_backend_from_config(config, Some(storage_root))
                .await
                .expect("Failed to create backend");

        let store = Rs3gwStore::new(backend, "test-zarr".to_string(), "array".to_string());

        (store, temp_dir)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_store_set_get() {
        let (mut store, _temp_dir) = create_test_store().await;

        let key = StoreKey::new("chunk.0.0".to_string());
        let value = b"test chunk data";

        store.set(&key, value).expect("Failed to set value");

        let retrieved = store.get(&key).expect("Failed to get value");
        assert_eq!(retrieved, value);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_store_exists() {
        let (mut store, _temp_dir) = create_test_store().await;

        let key = StoreKey::new("chunk.1.1".to_string());

        assert!(!store.exists(&key).expect("exists check failed"));

        store.set(&key, b"data").expect("Failed to set value");

        assert!(store.exists(&key).expect("exists check failed"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_store_delete() {
        let (mut store, _temp_dir) = create_test_store().await;

        let key = StoreKey::new("chunk.2.2".to_string());

        store.set(&key, b"data").expect("Failed to set value");
        assert!(store.exists(&key).expect("exists check failed"));

        store.delete(&key).expect("Failed to delete");
        assert!(!store.exists(&key).expect("exists check failed"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_store_list_prefix() {
        let (mut store, _temp_dir) = create_test_store().await;

        // Create some chunks
        for i in 0..3 {
            let key = StoreKey::new(format!("chunk.{i}.0"));
            store.set(&key, b"data").expect("Failed to set value");
        }

        let prefix = StoreKey::new("chunk.".to_string());
        let keys = store.list_prefix(&prefix).expect("Failed to list");

        assert_eq!(keys.len(), 3);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_readonly_store() {
        let (mut writable_store, _temp_dir) = create_test_store().await;

        let key = StoreKey::new("chunk.0.0".to_string());
        writable_store
            .set(&key, b"data")
            .expect("Failed to set value");

        // Create readonly store with same backend
        let readonly_store = Rs3gwStore::readonly(
            writable_store.storage.clone(),
            "test-zarr".to_string(),
            "array".to_string(),
        );

        // Read should work
        let data = readonly_store.get(&key).expect("Failed to read");
        assert_eq!(data, b"data");

        // Write should fail
        let mut readonly_store_mut = readonly_store;
        let result = readonly_store_mut.set(&key, b"new data");
        assert!(result.is_err());
    }
}
