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
use oxigeo_zarr::storage::{Store, StoreKey};

/// Rs3gw-backed Zarr store
///
/// This store implementation uses rs3gw for all storage operations,
/// providing high-performance access to Zarr arrays stored in cloud storage.
///
/// # Features
/// - Supports all rs3gw backends (Local, S3, MinIO, GCS, Azure)
/// - Optional deduplication for reducing storage costs
/// - Optional ML-based caching for predictive prefetching
/// - Optional encryption at rest (enable the `encryption` feature and attach
///   an [`EncryptionConfig`](crate::features::EncryptionConfig) via
///   [`Rs3gwStore::with_encryption`]). When configured, every object written
///   through [`Store::set`] is encrypted with AES-256-GCM or ChaCha20-Poly1305
///   before it reaches the backend, and transparently decrypted on
///   [`Store::get`]. Data at rest never contains plaintext.
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
    /// Optional encryption-at-rest configuration. When `Some` and enabled,
    /// object payloads are encrypted before `put` and decrypted after `get`.
    #[cfg(feature = "encryption")]
    encryption: Option<crate::features::encryption::EncryptionConfig>,
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
            #[cfg(feature = "encryption")]
            encryption: None,
        }
    }

    /// Creates a read-only store
    pub fn readonly(storage: DynBackend, bucket: String, prefix: String) -> Self {
        Self {
            storage,
            bucket,
            prefix,
            readonly: true,
            #[cfg(feature = "encryption")]
            encryption: None,
        }
    }

    /// Enables encryption-at-rest for this store.
    ///
    /// Once configured, every payload written through [`Store::set`] /
    /// [`AsyncStore::set`](oxigeo_zarr::storage::AsyncStore::set) is encrypted
    /// with the algorithm and key in `config` before it is handed to the
    /// backend, and payloads read through `get` are decrypted transparently.
    /// Zarr metadata objects (`zarr.json`, `.zarray`, `.zgroup`, `.zattrs`,
    /// `.zmetadata`) are only encrypted when
    /// [`EncryptionConfig::with_metadata_encryption`](crate::features::EncryptionConfig::with_metadata_encryption)
    /// was set; data chunks are always encrypted.
    ///
    /// If `config` is disabled (no key), this is a no-op and data is stored in
    /// plaintext.
    #[cfg(feature = "encryption")]
    #[must_use]
    pub fn with_encryption(
        mut self,
        config: crate::features::encryption::EncryptionConfig,
    ) -> Self {
        self.encryption = Some(config);
        self
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

/// Returns true if `key`'s final path component is a Zarr metadata document.
#[cfg(all(feature = "zarr", feature = "encryption"))]
fn is_metadata_key(key: &str) -> bool {
    let name = key.rsplit('/').next().unwrap_or(key);
    matches!(
        name,
        "zarr.json" | ".zarray" | ".zgroup" | ".zattrs" | ".zmetadata"
    )
}

#[cfg(feature = "zarr")]
impl Rs3gwStore {
    /// Selects the encryption config that applies to `key`, honoring the
    /// `encrypt_metadata` flag. Returns `None` when the payload should be
    /// stored/read as plaintext.
    #[cfg(feature = "encryption")]
    fn active_encryption(
        &self,
        key: &str,
    ) -> Option<&crate::features::encryption::EncryptionConfig> {
        match &self.encryption {
            Some(config) if config.is_enabled() => {
                if is_metadata_key(key) && !config.encrypt_metadata {
                    None
                } else {
                    Some(config)
                }
            }
            _ => None,
        }
    }

    /// Encrypts `value` for storage if encryption applies to `key`, otherwise
    /// returns the bytes unchanged.
    #[cfg(feature = "encryption")]
    fn encode_value(&self, key: &str, value: &[u8]) -> oxigeo_zarr::error::Result<Vec<u8>> {
        match self.active_encryption(key) {
            Some(config) => config.encrypt(value).map_err(map_encryption_error),
            None => Ok(value.to_vec()),
        }
    }

    /// Non-encryption build: payloads pass through untouched.
    #[cfg(not(feature = "encryption"))]
    fn encode_value(&self, _key: &str, value: &[u8]) -> oxigeo_zarr::error::Result<Vec<u8>> {
        Ok(value.to_vec())
    }

    /// Decrypts `raw` after reading if encryption applies to `key`, otherwise
    /// returns the bytes unchanged.
    #[cfg(feature = "encryption")]
    fn decode_value(&self, key: &str, raw: Vec<u8>) -> oxigeo_zarr::error::Result<Vec<u8>> {
        match self.active_encryption(key) {
            Some(config) => config.decrypt(&raw).map_err(map_encryption_error),
            None => Ok(raw),
        }
    }

    /// Non-encryption build: payloads pass through untouched.
    #[cfg(not(feature = "encryption"))]
    fn decode_value(&self, _key: &str, raw: Vec<u8>) -> oxigeo_zarr::error::Result<Vec<u8>> {
        Ok(raw)
    }
}

#[cfg(feature = "zarr")]
impl Store for Rs3gwStore {
    fn exists(&self, key: &StoreKey) -> oxigeo_zarr::error::Result<bool> {
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

    fn get(&self, key: &StoreKey) -> oxigeo_zarr::error::Result<Vec<u8>> {
        let object_key = self.to_object_key(key.as_str());
        let storage = self.storage.clone();
        let bucket = self.bucket.clone();

        // Try to get current runtime handle
        let fetched: oxigeo_zarr::error::Result<Vec<u8>> =
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
            };

        self.decode_value(key.as_str(), fetched?)
    }

    fn set(&mut self, key: &StoreKey, value: &[u8]) -> oxigeo_zarr::error::Result<()> {
        if self.readonly {
            return Err(oxigeo_zarr::error::ZarrError::Storage(
                oxigeo_zarr::error::StorageError::ReadOnly,
            ));
        }

        let object_key = self.to_object_key(key.as_str());
        let encoded = self.encode_value(key.as_str(), value)?;
        let data = Bytes::from(encoded);
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

    fn delete(&mut self, key: &StoreKey) -> oxigeo_zarr::error::Result<()> {
        if self.readonly {
            return Err(oxigeo_zarr::error::ZarrError::Storage(
                oxigeo_zarr::error::StorageError::ReadOnly,
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

    fn list_prefix(&self, prefix: &StoreKey) -> oxigeo_zarr::error::Result<Vec<StoreKey>> {
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

    fn flush(&mut self) -> oxigeo_zarr::error::Result<()> {
        // No-op for rs3gw (writes are synchronous)
        Ok(())
    }
}

#[cfg(all(feature = "zarr", feature = "async"))]
mod async_impl {
    use super::*;
    use oxigeo_zarr::storage::AsyncStore;

    #[async_trait::async_trait]
    impl AsyncStore for Rs3gwStore {
        async fn exists(&self, key: &StoreKey) -> oxigeo_zarr::error::Result<bool> {
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

        async fn get(&self, key: &StoreKey) -> oxigeo_zarr::error::Result<Vec<u8>> {
            let object_key = self.to_object_key(key.as_str());

            let (_metadata, data) = self
                .storage
                .get_object(&self.bucket, &object_key, None)
                .await
                .map_err(map_storage_error)?;

            self.decode_value(key.as_str(), data.to_vec())
        }

        async fn set(&mut self, key: &StoreKey, value: &[u8]) -> oxigeo_zarr::error::Result<()> {
            if self.readonly {
                return Err(oxigeo_zarr::error::ZarrError::Storage(
                    oxigeo_zarr::error::StorageError::ReadOnly,
                ));
            }

            let object_key = self.to_object_key(key.as_str());
            let encoded = self.encode_value(key.as_str(), value)?;
            let data = Bytes::from(encoded);

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

        async fn delete(&mut self, key: &StoreKey) -> oxigeo_zarr::error::Result<()> {
            if self.readonly {
                return Err(oxigeo_zarr::error::ZarrError::Storage(
                    oxigeo_zarr::error::StorageError::ReadOnly,
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
        ) -> oxigeo_zarr::error::Result<Vec<StoreKey>> {
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

        async fn flush(&mut self) -> oxigeo_zarr::error::Result<()> {
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
fn map_storage_error(err: rs3gw::storage::StorageError) -> oxigeo_zarr::error::ZarrError {
    use oxigeo_zarr::error::{StorageError, ZarrError};

    match err {
        rs3gw::storage::StorageError::NotFound(path) => {
            ZarrError::Storage(StorageError::KeyNotFound { key: path })
        }
        other => ZarrError::Storage(StorageError::Network {
            message: other.to_string(),
        }),
    }
}

#[cfg(all(feature = "zarr", feature = "encryption"))]
fn map_encryption_error(
    err: crate::features::encryption::EncryptionError,
) -> oxigeo_zarr::error::ZarrError {
    use oxigeo_zarr::error::{StorageError, ZarrError};

    ZarrError::Storage(StorageError::Network {
        message: format!("encryption error: {err}"),
    })
}

#[cfg(feature = "zarr")]
fn map_rs3gw_error(err: Rs3gwError) -> oxigeo_zarr::error::ZarrError {
    use oxigeo_zarr::error::{StorageError, ZarrError};

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

    #[cfg(feature = "encryption")]
    async fn create_backend() -> (rs3gw::storage::backend::DynBackend, TempDir) {
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

        (backend, temp_dir)
    }

    #[cfg(feature = "encryption")]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_store_encrypts_data_at_rest() {
        use crate::features::encryption::EncryptionConfig;

        let (backend, _temp_dir) = create_backend().await;
        let encryption = EncryptionConfig::new().with_key(vec![7u8; 32]);
        let mut store =
            Rs3gwStore::new(backend.clone(), "sec-zarr".to_string(), "array".to_string())
                .with_encryption(encryption);

        let chunk_key = StoreKey::new("c.0.0".to_string());
        let plaintext = b"TOP-SECRET-PIXELS-0123456789-abcdef";
        store
            .set(&chunk_key, plaintext)
            .expect("Failed to set value");

        // The raw bytes on the backend must be ciphertext, never the plaintext.
        let (_meta, raw) = backend
            .get_object("sec-zarr", "array/c.0.0", None)
            .await
            .expect("Failed to read raw object");
        let raw = raw.to_vec();
        assert_ne!(raw.as_slice(), plaintext.as_slice());
        assert!(
            !raw.windows(plaintext.len()).any(|w| w == plaintext),
            "plaintext leaked into storage backend"
        );

        // Reading back through the store must decrypt transparently.
        let recovered = store.get(&chunk_key).expect("Failed to get value");
        assert_eq!(recovered, plaintext);
    }

    #[cfg(feature = "encryption")]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_store_metadata_plaintext_when_flag_off() {
        use crate::features::encryption::EncryptionConfig;

        let (backend, _temp_dir) = create_backend().await;
        // encrypt_metadata defaults to false.
        let encryption = EncryptionConfig::new().with_key(vec![9u8; 32]);
        let mut store = Rs3gwStore::new(
            backend.clone(),
            "meta-zarr".to_string(),
            "array".to_string(),
        )
        .with_encryption(encryption);

        let meta_key = StoreKey::new("zarr.json".to_string());
        let meta = b"{\"zarr_format\":3,\"node_type\":\"array\"}";
        store.set(&meta_key, meta).expect("Failed to set metadata");

        // Metadata stays plaintext because encrypt_metadata is false.
        let (_m, raw_meta) = backend
            .get_object("meta-zarr", "array/zarr.json", None)
            .await
            .expect("Failed to read raw metadata");
        assert_eq!(raw_meta.to_vec().as_slice(), meta.as_slice());
        // But it still round-trips through the store.
        assert_eq!(store.get(&meta_key).expect("get meta"), meta);

        // A data chunk in the same store is still encrypted.
        let chunk_key = StoreKey::new("c.0.0".to_string());
        store.set(&chunk_key, b"chunk-bytes").expect("set chunk");
        let (_m2, raw_chunk) = backend
            .get_object("meta-zarr", "array/c.0.0", None)
            .await
            .expect("Failed to read raw chunk");
        assert_ne!(raw_chunk.to_vec().as_slice(), b"chunk-bytes".as_slice());
    }

    #[cfg(feature = "encryption")]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_store_metadata_encrypted_when_flag_on() {
        use crate::features::encryption::EncryptionConfig;

        let (backend, _temp_dir) = create_backend().await;
        let encryption = EncryptionConfig::new()
            .with_key(vec![3u8; 32])
            .with_metadata_encryption(true);
        let mut store =
            Rs3gwStore::new(backend.clone(), "meta-enc".to_string(), "array".to_string())
                .with_encryption(encryption);

        let meta_key = StoreKey::new(".zarray".to_string());
        let meta = b"{\"shape\":[10,10]}";
        store.set(&meta_key, meta).expect("Failed to set metadata");

        let (_m, raw_meta) = backend
            .get_object("meta-enc", "array/.zarray", None)
            .await
            .expect("Failed to read raw metadata");
        assert_ne!(raw_meta.to_vec().as_slice(), meta.as_slice());
        assert_eq!(store.get(&meta_key).expect("get meta"), meta);
    }

    #[cfg(feature = "encryption")]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_store_wrong_key_fails_to_decrypt() {
        use crate::features::encryption::EncryptionConfig;

        let (backend, _temp_dir) = create_backend().await;
        let mut writer = Rs3gwStore::new(backend.clone(), "wk".to_string(), "array".to_string())
            .with_encryption(EncryptionConfig::new().with_key(vec![1u8; 32]));

        let key = StoreKey::new("c.0.0".to_string());
        writer.set(&key, b"payload").expect("set");

        // A reader with a different key must not be able to recover the data.
        let reader = Rs3gwStore::readonly(backend, "wk".to_string(), "array".to_string())
            .with_encryption(EncryptionConfig::new().with_key(vec![2u8; 32]));
        assert!(reader.get(&key).is_err());
    }

    #[cfg(feature = "encryption")]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_store_disabled_encryption_is_plaintext() {
        use crate::features::encryption::EncryptionConfig;

        let (backend, _temp_dir) = create_backend().await;
        // Disabled config (no key) must behave as a no-op passthrough.
        let mut store = Rs3gwStore::new(backend.clone(), "plain".to_string(), "array".to_string())
            .with_encryption(EncryptionConfig::disabled());

        let key = StoreKey::new("c.0.0".to_string());
        let value = b"visible-bytes";
        store.set(&key, value).expect("set");

        let (_m, raw) = backend
            .get_object("plain", "array/c.0.0", None)
            .await
            .expect("raw read");
        assert_eq!(raw.to_vec().as_slice(), value.as_slice());
        assert_eq!(store.get(&key).expect("get"), value);
    }
}
