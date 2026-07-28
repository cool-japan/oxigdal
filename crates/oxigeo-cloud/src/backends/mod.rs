//! Cloud storage backend implementations
//!
//! This module provides various cloud storage backends including S3, Azure Blob Storage,
//! Google Cloud Storage, and HTTP.

#[cfg(feature = "s3")]
pub mod s3;

#[cfg(feature = "azure-blob")]
pub mod azure;

#[cfg(feature = "gcs")]
pub mod gcs;

#[cfg(feature = "http")]
pub mod http;

#[cfg(feature = "s3")]
pub use s3::S3Backend;

#[cfg(feature = "azure-blob")]
pub use azure::AzureBlobBackend;

#[cfg(feature = "gcs")]
pub use gcs::GcsBackend;

#[cfg(feature = "http")]
pub use http::HttpBackend;

use crate::error::Result;
use oxigeo_core::io::ByteRange;

/// Common trait for cloud storage backends
#[cfg(feature = "async")]
#[async_trait::async_trait]
pub trait CloudStorageBackend: Send + Sync {
    /// Gets an object from storage
    async fn get(&self, key: &str) -> Result<bytes::Bytes>;

    /// Gets a byte range of an object from storage.
    ///
    /// The default implementation falls back to downloading the whole
    /// object via [`get`](Self::get) and slicing the requested range out of
    /// it in memory -- this is correct but defeats the purpose of a
    /// byte-range read for backends that can't (or don't yet) support a
    /// native partial read. Backends that support native range reads
    /// (S3, GCS, Azure Blob, plain HTTP via the `Range` header) MUST
    /// override this method so partial reads (COG/Zarr/PMTiles tile
    /// fetches, etc.) don't pull entire remote objects over the network.
    async fn get_range(&self, key: &str, range: ByteRange) -> Result<bytes::Bytes> {
        let data = self.get(key).await?;
        let len = data.len() as u64;
        let start = range.start.min(len);
        let end = range.end.min(len).max(start);
        Ok(data.slice(start as usize..end as usize))
    }

    /// Puts an object to storage
    async fn put(&self, key: &str, data: &[u8]) -> Result<()>;

    /// Deletes an object from storage
    async fn delete(&self, key: &str) -> Result<()>;

    /// Checks if an object exists
    async fn exists(&self, key: &str) -> Result<bool>;

    /// Lists objects with a given prefix
    async fn list_prefix(&self, prefix: &str) -> Result<Vec<String>>;

    /// Returns whether this backend is read-only
    fn is_readonly(&self) -> bool {
        false
    }

    /// Returns whether [`get_range`](Self::get_range) is natively supported
    /// (i.e. does not fall back to a whole-object download). Callers that
    /// care about avoiding full downloads (e.g. a prefetcher deciding
    /// whether partial reads are worthwhile) can check this.
    fn supports_native_range_reads(&self) -> bool {
        false
    }
}

#[cfg(all(test, feature = "async"))]
mod tests {
    use super::*;
    use crate::error::{CloudError, S3Error};
    use std::sync::{Arc, Mutex};

    /// A trivial in-memory backend that only implements the required
    /// methods, to exercise the default `get_range` fallback.
    struct MemoryBackend {
        objects: Mutex<std::collections::HashMap<String, bytes::Bytes>>,
    }

    #[async_trait::async_trait]
    impl CloudStorageBackend for MemoryBackend {
        async fn get(&self, key: &str) -> Result<bytes::Bytes> {
            self.objects
                .lock()
                .expect("lock poisoned")
                .get(key)
                .cloned()
                .ok_or_else(|| {
                    CloudError::S3(S3Error::Sdk {
                        message: format!("no such key: {key}"),
                    })
                })
        }

        async fn put(&self, key: &str, data: &[u8]) -> Result<()> {
            self.objects
                .lock()
                .expect("lock poisoned")
                .insert(key.to_string(), bytes::Bytes::copy_from_slice(data));
            Ok(())
        }

        async fn delete(&self, key: &str) -> Result<()> {
            self.objects.lock().expect("lock poisoned").remove(key);
            Ok(())
        }

        async fn exists(&self, key: &str) -> Result<bool> {
            Ok(self
                .objects
                .lock()
                .expect("lock poisoned")
                .contains_key(key))
        }

        async fn list_prefix(&self, prefix: &str) -> Result<Vec<String>> {
            Ok(self
                .objects
                .lock()
                .expect("lock poisoned")
                .keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect())
        }
    }

    #[tokio::test]
    async fn test_default_get_range_falls_back_to_whole_object_slice() {
        let backend = MemoryBackend {
            objects: Mutex::new(std::collections::HashMap::new()),
        };
        backend.put("k", b"0123456789").await.expect("put failed");

        let mid = backend
            .get_range("k", ByteRange::new(3, 7))
            .await
            .expect("get_range failed");
        assert_eq!(&mid[..], b"3456");

        // Range clamped to object length.
        let clamped = backend
            .get_range("k", ByteRange::new(8, 100))
            .await
            .expect("get_range failed");
        assert_eq!(&clamped[..], b"89");

        // A backend that doesn't override get_range must honestly report so.
        assert!(!backend.supports_native_range_reads());
    }

    #[tokio::test]
    async fn test_default_get_range_propagates_get_errors() {
        let backend = MemoryBackend {
            objects: Mutex::new(std::collections::HashMap::new()),
        };
        let err = backend
            .get_range("missing", ByteRange::new(0, 5))
            .await
            .expect_err("expected an error for a missing key");
        assert!(matches!(err, CloudError::S3(_)));
    }

    #[test]
    fn test_arc_dyn_backend_is_object_safe() {
        // Compile-time check that the trait (with its new default methods)
        // remains object-safe / usable as `Arc<dyn CloudStorageBackend>`.
        let backend: Arc<dyn CloudStorageBackend> = Arc::new(MemoryBackend {
            objects: Mutex::new(std::collections::HashMap::new()),
        });
        assert!(!backend.supports_native_range_reads());
    }
}
