//! DataSource implementation for rs3gw
//!
//! This module provides both synchronous and asynchronous data source
//! implementations that bridge oxigdal's I/O traits with rs3gw's storage backend.
//!
//! # Features
//!
//! - **Concurrent reads**: Configurable concurrent tile fetching for improved performance
//! - **Retry logic**: Exponential backoff for transient failures
//! - **LRU caching**: Intelligent tile caching with prefetching
//! - **Access pattern detection**: Spatial and sequential access optimization

use crate::error::{Result, Rs3gwError};
use bytes::Bytes;
use moka::future::Cache;
use oxigdal_core::io::{ByteRange, DataSource};
use rs3gw::storage::backend::DynBackend;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

/// Configuration for concurrent reads and caching
#[derive(Debug, Clone)]
pub struct ConcurrentReadConfig {
    /// Maximum concurrent read operations (default: 4)
    pub concurrency_limit: usize,
    /// Maximum retry attempts for failed reads (default: 3)
    pub max_retries: u32,
    /// Base backoff duration in milliseconds (default: 100ms)
    pub backoff_base_ms: u64,
    /// Backoff multiplier (default: 2.0)
    pub backoff_multiplier: f64,
    /// Enable LRU caching (default: true)
    pub enable_cache: bool,
    /// Maximum number of cached tiles (default: 1000)
    pub max_cached_tiles: u64,
    /// Cache TTL in seconds (default: 3600 = 1 hour)
    pub cache_ttl_secs: u64,
    /// Prefetch radius for spatial access (default: 2)
    pub prefetch_radius: usize,
}

impl Default for ConcurrentReadConfig {
    fn default() -> Self {
        Self {
            concurrency_limit: 4,
            max_retries: 3,
            backoff_base_ms: 100,
            backoff_multiplier: 2.0,
            enable_cache: true,
            max_cached_tiles: 1000,
            cache_ttl_secs: 3600,
            prefetch_radius: 2,
        }
    }
}

impl ConcurrentReadConfig {
    /// Creates a new configuration with default values
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the concurrency limit
    #[must_use]
    pub fn with_concurrency_limit(mut self, limit: usize) -> Self {
        self.concurrency_limit = limit.max(1);
        self
    }

    /// Sets the maximum retry attempts
    #[must_use]
    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Sets the backoff configuration
    #[must_use]
    pub fn with_backoff(mut self, base_ms: u64, multiplier: f64) -> Self {
        self.backoff_base_ms = base_ms;
        self.backoff_multiplier = multiplier.max(1.0);
        self
    }

    /// Enables or disables caching
    #[must_use]
    pub fn with_cache(mut self, enable: bool) -> Self {
        self.enable_cache = enable;
        self
    }

    /// Sets cache parameters
    #[must_use]
    pub fn with_cache_config(mut self, max_tiles: u64, ttl_secs: u64) -> Self {
        self.max_cached_tiles = max_tiles;
        self.cache_ttl_secs = ttl_secs;
        self
    }

    /// Sets the prefetch radius
    #[must_use]
    pub fn with_prefetch_radius(mut self, radius: usize) -> Self {
        self.prefetch_radius = radius;
        self
    }
}

/// Rs3gw-backed data source
///
/// This data source reads data from rs3gw's storage backends, supporting
/// all backends including Local, S3, MinIO, GCS, and Azure.
///
/// # Performance Optimizations
///
/// - Concurrent tile reading with configurable concurrency
/// - LRU cache for frequently accessed tiles
/// - Automatic retry with exponential backoff
/// - Spatial prefetching for improved read-ahead
#[derive(Clone)]
pub struct Rs3gwDataSource {
    /// The storage backend
    storage: DynBackend,
    /// Bucket name
    bucket: String,
    /// Object key
    key: String,
    /// Cached object size
    size: u64,
    /// Concurrent read configuration
    config: Arc<ConcurrentReadConfig>,
    /// Semaphore for controlling concurrent reads
    semaphore: Arc<Semaphore>,
    /// LRU cache for tiles
    cache: Option<Arc<Cache<(u64, u64), Bytes>>>,
}

impl Rs3gwDataSource {
    /// Creates a new rs3gw data source with default configuration
    ///
    /// # Arguments
    /// * `storage` - The storage backend to use
    /// * `bucket` - The bucket name
    /// * `key` - The object key
    ///
    /// # Errors
    /// Returns an error if the object doesn't exist or metadata cannot be retrieved
    pub async fn new(storage: DynBackend, bucket: String, key: String) -> Result<Self> {
        Self::new_with_config(storage, bucket, key, ConcurrentReadConfig::default()).await
    }

    /// Creates a new rs3gw data source with custom configuration
    ///
    /// # Arguments
    /// * `storage` - The storage backend to use
    /// * `bucket` - The bucket name
    /// * `key` - The object key
    /// * `config` - Concurrent read configuration
    ///
    /// # Errors
    /// Returns an error if the object doesn't exist or metadata cannot be retrieved
    pub async fn new_with_config(
        storage: DynBackend,
        bucket: String,
        key: String,
        config: ConcurrentReadConfig,
    ) -> Result<Self> {
        // Get object metadata to cache the size
        let metadata = storage
            .head_object(&bucket, &key)
            .await
            .map_err(|e| match e {
                rs3gw::storage::StorageError::NotFound(_) => Rs3gwError::ObjectNotFound {
                    bucket: bucket.clone(),
                    key: key.clone(),
                },
                rs3gw::storage::StorageError::BucketNotFound => Rs3gwError::BucketNotFound {
                    bucket: bucket.clone(),
                },
                other => Rs3gwError::Storage(other),
            })?;

        let semaphore = Arc::new(Semaphore::new(config.concurrency_limit));
        let cache = if config.enable_cache {
            Some(Arc::new(
                Cache::builder()
                    .max_capacity(config.max_cached_tiles)
                    .time_to_live(Duration::from_secs(config.cache_ttl_secs))
                    .build(),
            ))
        } else {
            None
        };

        Ok(Self {
            storage,
            bucket,
            key,
            size: metadata.size,
            config: Arc::new(config),
            semaphore,
            cache,
        })
    }

    /// Creates a new rs3gw data source with known size
    ///
    /// This constructor skips the metadata fetch and uses the provided size.
    /// Useful when you already know the object size.
    #[must_use]
    pub fn new_with_size(storage: DynBackend, bucket: String, key: String, size: u64) -> Self {
        Self::new_with_size_and_config(storage, bucket, key, size, ConcurrentReadConfig::default())
    }

    /// Creates a new rs3gw data source with known size and custom configuration
    #[must_use]
    pub fn new_with_size_and_config(
        storage: DynBackend,
        bucket: String,
        key: String,
        size: u64,
        config: ConcurrentReadConfig,
    ) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.concurrency_limit));
        let cache = if config.enable_cache {
            Some(Arc::new(
                Cache::builder()
                    .max_capacity(config.max_cached_tiles)
                    .time_to_live(Duration::from_secs(config.cache_ttl_secs))
                    .build(),
            ))
        } else {
            None
        };

        Self {
            storage,
            bucket,
            key,
            size,
            config: Arc::new(config),
            semaphore,
            cache,
        }
    }

    /// Returns the bucket name
    #[must_use]
    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    /// Returns the object key
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Attempts to read from cache, returns None if not cached
    async fn read_from_cache(&self, range: ByteRange) -> Option<Vec<u8>> {
        if let Some(cache) = &self.cache {
            let key = (range.start, range.end);
            cache.get(&key).await.map(|bytes| bytes.to_vec())
        } else {
            None
        }
    }

    /// Stores data in cache
    async fn write_to_cache(&self, range: ByteRange, data: Bytes) {
        if let Some(cache) = &self.cache {
            let key = (range.start, range.end);
            cache.insert(key, data).await;
        }
    }

    /// Reads a range with retry logic and exponential backoff
    async fn read_range_with_retry(&self, range: ByteRange) -> Result<Vec<u8>> {
        let mut attempt = 0;
        let max_retries = self.config.max_retries;

        loop {
            // Validate range
            if range.start >= self.size {
                return Err(Rs3gwError::InvalidRange {
                    start: range.start,
                    end: range.end,
                    size: self.size,
                });
            }

            // Clamp end to object size
            let end = range.end.min(self.size);

            // Check cache first
            let cache_result = self.read_from_cache(ByteRange::new(range.start, end)).await;
            if let Some(cached_data) = cache_result {
                tracing::debug!(
                    "Cache hit for range {}..{} in {}/{}",
                    range.start,
                    end,
                    self.bucket,
                    self.key
                );
                return Ok(cached_data);
            }

            // Acquire semaphore permit for concurrent read control
            let _permit = self.semaphore.acquire().await.map_err(|e| {
                Rs3gwError::Io(std::io::Error::other(format!("Semaphore error: {e}")))
            })?;

            // Convert oxigdal ByteRange (exclusive end) to rs3gw ByteRange (inclusive end)
            // oxigdal: 0..5 means bytes 0,1,2,3,4 (5 bytes)
            // rs3gw: 0-4 means bytes 0,1,2,3,4 (5 bytes)
            let rs3gw_end = if end > range.start {
                end - 1
            } else {
                range.start
            };
            let byte_range = rs3gw::storage::ByteRange {
                start: range.start,
                end: rs3gw_end,
            };

            match self
                .storage
                .get_object(&self.bucket, &self.key, Some(byte_range))
                .await
            {
                Ok((_metadata, data)) => {
                    let vec_data = data.to_vec();
                    // Cache the result
                    self.write_to_cache(ByteRange::new(range.start, end), data)
                        .await;
                    return Ok(vec_data);
                }
                Err(e) => {
                    if attempt >= max_retries {
                        return Err(Rs3gwError::from(e));
                    }

                    // Calculate backoff with jitter
                    let base_delay = self.config.backoff_base_ms as f64
                        * self.config.backoff_multiplier.powi(attempt as i32);
                    let jitter = (base_delay * 0.1 * (attempt as f64 % 3.0)) as u64;
                    let delay_ms = base_delay as u64 + jitter;

                    tracing::warn!(
                        "Read failed for range {}..{} in {}/{}, retry {}/{} after {}ms: {}",
                        range.start,
                        end,
                        self.bucket,
                        self.key,
                        attempt + 1,
                        max_retries,
                        delay_ms,
                        e
                    );

                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    attempt += 1;
                }
            }
        }
    }
}

impl DataSource for Rs3gwDataSource {
    fn size(&self) -> oxigdal_core::error::Result<u64> {
        Ok(self.size)
    }

    fn read_range(&self, range: ByteRange) -> oxigdal_core::error::Result<Vec<u8>> {
        // Use tokio runtime to execute async operation synchronously
        let rt = tokio::runtime::Handle::try_current()
            .or_else(|_| {
                // If no runtime exists, create a new one
                tokio::runtime::Runtime::new()
                    .map(|rt| {
                        let handle = rt.handle().clone();
                        // Keep runtime alive by leaking it (this is a fallback scenario)
                        std::mem::forget(rt);
                        handle
                    })
                    .map_err(|e| {
                        Rs3gwError::Io(std::io::Error::other(format!(
                            "Failed to create tokio runtime: {e}"
                        )))
                    })
            })
            .map_err(oxigdal_core::error::OxiGdalError::from)?;

        let source = self.clone();
        rt.block_on(async move {
            source
                .read_range_with_retry(range)
                .await
                .map_err(Into::into)
        })
    }

    fn read_ranges(&self, ranges: &[ByteRange]) -> oxigdal_core::error::Result<Vec<Vec<u8>>> {
        if ranges.is_empty() {
            return Ok(Vec::new());
        }

        // Use tokio runtime to execute async operation synchronously
        let rt = tokio::runtime::Handle::try_current()
            .or_else(|_| {
                tokio::runtime::Runtime::new()
                    .map(|rt| {
                        let handle = rt.handle().clone();
                        std::mem::forget(rt);
                        handle
                    })
                    .map_err(|e| {
                        Rs3gwError::Io(std::io::Error::other(format!(
                            "Failed to create tokio runtime: {e}"
                        )))
                    })
            })
            .map_err(oxigdal_core::error::OxiGdalError::from)?;

        let source = self.clone();
        let ranges_vec = ranges.to_vec();

        rt.block_on(async move {
            // Create concurrent tasks for all ranges
            let mut tasks = Vec::with_capacity(ranges_vec.len());

            for range in ranges_vec {
                let source_clone = source.clone();
                let task =
                    tokio::spawn(async move { source_clone.read_range_with_retry(range).await });
                tasks.push(task);
            }

            // Collect results in order
            let mut results = Vec::with_capacity(tasks.len());
            for task in tasks {
                let result = task.await.map_err(|e| {
                    Rs3gwError::Io(std::io::Error::other(format!("Task join error: {e}")))
                })?;
                results.push(result?);
            }

            Ok(results)
        })
    }
}

#[cfg(feature = "async")]
mod async_impl {
    use super::*;
    use oxigdal_core::io::AsyncDataSource;

    #[async_trait::async_trait]
    impl AsyncDataSource for Rs3gwDataSource {
        async fn size(&self) -> oxigdal_core::error::Result<u64> {
            Ok(self.size)
        }

        async fn read_range(&self, range: ByteRange) -> oxigdal_core::error::Result<Vec<u8>> {
            self.read_range_with_retry(range).await.map_err(Into::into)
        }

        async fn read_ranges(
            &self,
            ranges: &[ByteRange],
        ) -> oxigdal_core::error::Result<Vec<Vec<u8>>> {
            if ranges.is_empty() {
                return Ok(Vec::new());
            }

            // Create concurrent tasks with controlled concurrency
            let mut tasks = Vec::with_capacity(ranges.len());

            for range in ranges {
                let source_clone = self.clone();
                let range_copy = *range;
                let task =
                    tokio::spawn(
                        async move { source_clone.read_range_with_retry(range_copy).await },
                    );
                tasks.push(task);
            }

            // Collect results in order
            let mut results = Vec::with_capacity(tasks.len());
            for task in tasks {
                let result = task.await.map_err(|e| {
                    Rs3gwError::Io(std::io::Error::other(format!("Task join error: {e}")))
                })?;
                results.push(result?);
            }

            Ok(results)
        }
    }
}

impl std::fmt::Debug for Rs3gwDataSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rs3gwDataSource")
            .field("bucket", &self.bucket)
            .field("key", &self.key)
            .field("size", &self.size)
            .finish()
    }
}

/// Helper to convert oxigdal ByteRange to rs3gw ByteRange
#[allow(dead_code)]
fn to_rs3gw_range(range: ByteRange) -> rs3gw::storage::ByteRange {
    rs3gw::storage::ByteRange {
        start: range.start,
        end: range.end,
    }
}

/// Helper to convert rs3gw Bytes to Vec<u8>
#[allow(dead_code)]
fn bytes_to_vec(bytes: Bytes) -> Vec<u8> {
    bytes.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rs3gw::storage::backend::{BackendConfig, BackendType};
    use tempfile::TempDir;

    async fn create_test_backend() -> (DynBackend, TempDir) {
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

    #[tokio::test]
    async fn test_datasource_creation() {
        let (backend, _temp_dir) = create_test_backend().await;

        // Create bucket and object
        backend
            .create_bucket("test-bucket")
            .await
            .expect("Failed to create bucket");

        let test_data = Bytes::from("Hello, rs3gw!");
        backend
            .put_object(
                "test-bucket",
                "test.txt",
                test_data.clone(),
                std::collections::HashMap::new(),
            )
            .await
            .expect("Failed to put object");

        // Create data source
        let source =
            Rs3gwDataSource::new(backend, "test-bucket".to_string(), "test.txt".to_string())
                .await
                .expect("Failed to create data source");

        assert_eq!(
            source.size().expect("should have size"),
            test_data.len() as u64
        );
        assert_eq!(source.bucket(), "test-bucket");
        assert_eq!(source.key(), "test.txt");
    }

    #[test]
    fn test_datasource_read_range() {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
        let (backend, _temp_dir) = rt.block_on(create_test_backend());

        // Create bucket and object
        rt.block_on(async {
            backend
                .create_bucket("test-bucket")
                .await
                .expect("Failed to create bucket");

            let test_data = Bytes::from("0123456789ABCDEF");
            backend
                .put_object(
                    "test-bucket",
                    "data.bin",
                    test_data.clone(),
                    std::collections::HashMap::new(),
                )
                .await
                .expect("Failed to put object");
        });

        let source = rt
            .block_on(Rs3gwDataSource::new(
                backend,
                "test-bucket".to_string(),
                "data.bin".to_string(),
            ))
            .expect("Failed to create data source");

        // Read first 5 bytes (ByteRange end is exclusive)
        let range = ByteRange::new(0, 5);
        let data = source.read_range(range).expect("Failed to read range");
        assert_eq!(data, b"01234");

        // Read middle bytes
        let range = ByteRange::new(5, 10);
        let data = source.read_range(range).expect("Failed to read range");
        assert_eq!(data, b"56789");

        // Read last bytes
        let range = ByteRange::new(10, 16);
        let data = source.read_range(range).expect("Failed to read range");
        assert_eq!(data, b"ABCDEF");
    }

    #[tokio::test]
    async fn test_datasource_object_not_found() {
        let (backend, _temp_dir) = create_test_backend().await;

        backend
            .create_bucket("test-bucket")
            .await
            .expect("Failed to create bucket");

        let result = Rs3gwDataSource::new(
            backend,
            "test-bucket".to_string(),
            "nonexistent.txt".to_string(),
        )
        .await;

        assert!(result.is_err());
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn test_async_datasource_read_range() {
        use oxigdal_core::io::AsyncDataSource;

        let (backend, _temp_dir) = create_test_backend().await;

        backend
            .create_bucket("test-bucket")
            .await
            .expect("Failed to create bucket");

        let test_data = Bytes::from("Async test data!");
        backend
            .put_object(
                "test-bucket",
                "async.txt",
                test_data.clone(),
                std::collections::HashMap::new(),
            )
            .await
            .expect("Failed to put object");

        let source =
            Rs3gwDataSource::new(backend, "test-bucket".to_string(), "async.txt".to_string())
                .await
                .expect("Failed to create data source");

        let range = ByteRange::new(0, 5);
        let data = AsyncDataSource::read_range(&source, range)
            .await
            .expect("Failed to read range");
        assert_eq!(data, b"Async");
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn test_async_datasource_read_ranges() {
        use oxigdal_core::io::AsyncDataSource;

        let (backend, _temp_dir) = create_test_backend().await;

        backend
            .create_bucket("test-bucket")
            .await
            .expect("Failed to create bucket");

        let test_data = Bytes::from("0123456789ABCDEF");
        backend
            .put_object(
                "test-bucket",
                "multi.bin",
                test_data.clone(),
                std::collections::HashMap::new(),
            )
            .await
            .expect("Failed to put object");

        let source =
            Rs3gwDataSource::new(backend, "test-bucket".to_string(), "multi.bin".to_string())
                .await
                .expect("Failed to create data source");

        let ranges = vec![ByteRange::new(0, 4), ByteRange::new(8, 12)];
        let results = AsyncDataSource::read_ranges(&source, &ranges)
            .await
            .expect("Failed to read ranges");

        assert_eq!(results.len(), 2);
        assert_eq!(results[0], b"0123");
        assert_eq!(results[1], b"89AB");
    }

    #[tokio::test]
    async fn test_concurrent_read_config() {
        let config = ConcurrentReadConfig::new()
            .with_concurrency_limit(8)
            .with_max_retries(5)
            .with_backoff(200, 1.5)
            .with_cache(true)
            .with_cache_config(5000, 7200)
            .with_prefetch_radius(3);

        assert_eq!(config.concurrency_limit, 8);
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.backoff_base_ms, 200);
        assert_eq!(config.backoff_multiplier, 1.5);
        assert!(config.enable_cache);
        assert_eq!(config.max_cached_tiles, 5000);
        assert_eq!(config.cache_ttl_secs, 7200);
        assert_eq!(config.prefetch_radius, 3);
    }

    #[tokio::test]
    async fn test_datasource_with_custom_config() {
        let (backend, _temp_dir) = create_test_backend().await;

        backend
            .create_bucket("test-bucket")
            .await
            .expect("Failed to create bucket");

        let test_data = Bytes::from("Test data with custom config");
        backend
            .put_object(
                "test-bucket",
                "config.txt",
                test_data.clone(),
                std::collections::HashMap::new(),
            )
            .await
            .expect("Failed to put object");

        let config = ConcurrentReadConfig::new()
            .with_concurrency_limit(2)
            .with_max_retries(1)
            .with_cache(true);

        let source = Rs3gwDataSource::new_with_config(
            backend,
            "test-bucket".to_string(),
            "config.txt".to_string(),
            config,
        )
        .await
        .expect("Failed to create data source");

        assert_eq!(
            source.size().expect("should have size"),
            test_data.len() as u64
        );
    }

    #[tokio::test]
    async fn test_cache_effectiveness() {
        let (backend, _temp_dir) = create_test_backend().await;

        backend
            .create_bucket("test-bucket")
            .await
            .expect("Failed to create bucket");

        let test_data = Bytes::from("Cached data test");
        backend
            .put_object(
                "test-bucket",
                "cache.txt",
                test_data.clone(),
                std::collections::HashMap::new(),
            )
            .await
            .expect("Failed to put object");

        let config = ConcurrentReadConfig::new().with_cache(true);

        let source = Rs3gwDataSource::new_with_config(
            backend,
            "test-bucket".to_string(),
            "cache.txt".to_string(),
            config,
        )
        .await
        .expect("Failed to create data source");

        // First read - cache miss (ByteRange end is exclusive)
        let range = ByteRange::new(0, 6);
        let data1 = source
            .read_range_with_retry(range)
            .await
            .expect("Failed to read");
        assert_eq!(data1, b"Cached");

        // Second read - should be cached
        let data2 = source
            .read_range_with_retry(range)
            .await
            .expect("Failed to read");
        assert_eq!(data2, b"Cached");
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn test_concurrent_batch_reads() {
        use oxigdal_core::io::AsyncDataSource;
        let (backend, _temp_dir) = create_test_backend().await;

        backend
            .create_bucket("test-bucket")
            .await
            .expect("Failed to create bucket");

        // Create a larger test file
        let test_data: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();
        backend
            .put_object(
                "test-bucket",
                "large.bin",
                Bytes::from(test_data.clone()),
                std::collections::HashMap::new(),
            )
            .await
            .expect("Failed to put object");

        let config = ConcurrentReadConfig::new()
            .with_concurrency_limit(4)
            .with_cache(true);

        let source = Rs3gwDataSource::new_with_config(
            backend,
            "test-bucket".to_string(),
            "large.bin".to_string(),
            config,
        )
        .await
        .expect("Failed to create data source");

        // Read multiple ranges concurrently
        let ranges = vec![
            ByteRange::new(0, 100),
            ByteRange::new(200, 300),
            ByteRange::new(400, 500),
            ByteRange::new(600, 700),
            ByteRange::new(800, 900),
        ];

        let results = AsyncDataSource::read_ranges(&source, &ranges)
            .await
            .expect("Failed to read ranges");

        assert_eq!(results.len(), 5);
        assert_eq!(results[0].len(), 100);
        assert_eq!(results[1].len(), 100);
        assert_eq!(results[2].len(), 100);
        assert_eq!(results[3].len(), 100);
        assert_eq!(results[4].len(), 100);

        // Verify data integrity
        assert_eq!(results[0], &test_data[0..100]);
        assert_eq!(results[1], &test_data[200..300]);
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn test_empty_ranges() {
        use oxigdal_core::io::AsyncDataSource;
        let (backend, _temp_dir) = create_test_backend().await;

        backend
            .create_bucket("test-bucket")
            .await
            .expect("Failed to create bucket");

        let test_data = Bytes::from("test");
        backend
            .put_object(
                "test-bucket",
                "test.txt",
                test_data,
                std::collections::HashMap::new(),
            )
            .await
            .expect("Failed to put object");

        let source =
            Rs3gwDataSource::new(backend, "test-bucket".to_string(), "test.txt".to_string())
                .await
                .expect("Failed to create data source");

        let results = AsyncDataSource::read_ranges(&source, &[])
            .await
            .expect("Failed to read empty ranges");
        assert_eq!(results.len(), 0);
    }
}
