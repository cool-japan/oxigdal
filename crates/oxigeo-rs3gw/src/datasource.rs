//! DataSource implementation for rs3gw
//!
//! This module provides both synchronous and asynchronous data source
//! implementations that bridge oxigeo's I/O traits with rs3gw's storage backend.
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
use oxigeo_core::io::{ByteRange, DataSource};
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
    ///
    /// When `spatial_prefetch` is enabled, after a cache-miss read of a range
    /// of length `len`, the next `prefetch_radius` contiguous ranges of the
    /// same length are eagerly fetched in the background and stored in the
    /// cache so that a subsequent sequential read is a cache hit. This is a
    /// deterministic look-ahead heuristic, not a learned/predictive model.
    pub prefetch_radius: usize,

    /// Enables background spatial read-ahead prefetching (default: false)
    ///
    /// When enabled, every cache-miss read spawns background tasks that
    /// eagerly warm the cache for the next `prefetch_radius` contiguous
    /// ranges. Disabled by default so that callers who never opted into
    /// prefetching don't get surprise background I/O.
    pub spatial_prefetch: bool,

    /// Minimum number of reads observed on this data source before spatial
    /// prefetching activates (default: 0 = activate immediately once
    /// `spatial_prefetch` is enabled).
    ///
    /// This acts as a warm-up gate: sources that only ever perform a single
    /// one-shot read never trigger background prefetch traffic.
    pub prefetch_warmup_reads: usize,
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
            spatial_prefetch: false,
            prefetch_warmup_reads: 0,
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

    /// Enables or disables background spatial read-ahead prefetching
    #[must_use]
    pub fn with_spatial_prefetch(mut self, enabled: bool) -> Self {
        self.spatial_prefetch = enabled;
        self
    }

    /// Sets the warm-up read count before spatial prefetch activates
    #[must_use]
    pub fn with_prefetch_warmup_reads(mut self, reads: usize) -> Self {
        self.prefetch_warmup_reads = reads;
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
    /// Count of reads served so far, used to gate spatial prefetch warm-up
    access_count: Arc<std::sync::atomic::AtomicUsize>,
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
            access_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
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
            access_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
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
    ///
    /// Hands back the cached [`Bytes`] handle rather than a `Vec`: cloning it
    /// is a refcount bump, so a cache hit that is written straight into a
    /// caller's buffer ([`DataSource::read_range_into`]) costs no allocation
    /// at all (cool-japan/oxigeo#14).
    async fn read_from_cache(&self, range: ByteRange) -> Option<Bytes> {
        if let Some(cache) = &self.cache {
            let key = (range.start, range.end);
            cache.get(&key).await
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

    /// Test-only helper: reports whether `range` is currently present in the
    /// cache, used to observe background prefetch without depending on
    /// timing-sensitive backend call counting.
    #[cfg(test)]
    async fn is_cached(&self, range: ByteRange) -> bool {
        self.read_from_cache(range).await.is_some()
    }

    /// Reads a range with retry logic and exponential backoff
    async fn read_range_with_retry(&self, range: ByteRange) -> Result<Vec<u8>> {
        Ok(self.read_range_with_retry_bytes(range).await?.to_vec())
    }

    /// Reads a range with retry logic and exponential backoff, keeping the
    /// result in its [`Bytes`] handle.
    ///
    /// This is the real implementation; [`Self::read_range_with_retry`] is a
    /// `to_vec` wrapper over it. Callers that already own a destination buffer
    /// go through here instead, so the bytes are copied once (into the
    /// caller's buffer) rather than twice (into a fresh `Vec`, then into the
    /// caller's buffer) -- see cool-japan/oxigeo#14.
    async fn read_range_with_retry_bytes(&self, range: ByteRange) -> Result<Bytes> {
        // An empty range is satisfied without touching the backend. rs3gw's
        // byte ranges are *inclusive*, so `start..start` would otherwise be
        // converted into a one-byte request and hand back a byte the caller
        // never asked for. This also matches `oxigeo_core`'s built-in sources,
        // which perform no I/O at all for an empty range.
        if range.end <= range.start {
            return Ok(Bytes::new());
        }

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

            // Convert oxigeo ByteRange (exclusive end) to rs3gw ByteRange (inclusive end)
            // oxigeo: 0..5 means bytes 0,1,2,3,4 (5 bytes)
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
                    // Cache the result; `Bytes` is refcounted, so the caller's
                    // copy and the cached copy share one allocation.
                    self.write_to_cache(ByteRange::new(range.start, end), data.clone())
                        .await;

                    // Count this read and, once warmed up, kick off background
                    // spatial read-ahead prefetch for the following ranges.
                    let reads_so_far = self
                        .access_count
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                        + 1;
                    self.maybe_spawn_prefetch(end, end - range.start, reads_so_far);

                    return Ok(data);
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

    /// Kicks off background spatial read-ahead prefetch for the
    /// `prefetch_radius` contiguous chunks following `next_start`, if
    /// `spatial_prefetch` is enabled, a cache is configured, and the
    /// warm-up gate (`prefetch_warmup_reads`) has been reached.
    ///
    /// This is a deterministic look-ahead heuristic (not a trained model):
    /// it simply assumes that sequential/contiguous access is likely to
    /// continue and pre-warms the cache for the next few chunks of the same
    /// size as the one just read. Failures are logged and otherwise ignored
    /// since prefetch is best-effort by nature.
    fn maybe_spawn_prefetch(&self, next_start: u64, chunk_len: u64, reads_so_far: usize) {
        if !self.config.spatial_prefetch
            || self.cache.is_none()
            || self.config.prefetch_radius == 0
            || chunk_len == 0
            || reads_so_far < self.config.prefetch_warmup_reads
        {
            return;
        }

        let source = self.clone();
        tokio::spawn(async move {
            let mut start = next_start;
            for _ in 0..source.config.prefetch_radius {
                if start >= source.size {
                    break;
                }
                let end = (start + chunk_len).min(source.size);
                let range = ByteRange::new(start, end);

                // Skip work that's already cached.
                if source.read_from_cache(range).await.is_some() {
                    start = end;
                    continue;
                }

                if let Err(e) = source.prefetch_one(range).await {
                    tracing::debug!(
                        "Spatial prefetch failed for range {}..{} in {}/{}: {}",
                        start,
                        end,
                        source.bucket,
                        source.key,
                        e
                    );
                    break;
                }
                start = end;
            }
        });
    }

    /// Fetches a single range from the backend and stores it in the cache,
    /// without retry (prefetch is best-effort: a failure just means we
    /// didn't warm the cache, the real read will still be attempted with
    /// full retry logic when the caller actually requests that range).
    async fn prefetch_one(&self, range: ByteRange) -> Result<()> {
        let _permit =
            self.semaphore.acquire().await.map_err(|e| {
                Rs3gwError::Io(std::io::Error::other(format!("Semaphore error: {e}")))
            })?;

        let rs3gw_end = if range.end > range.start {
            range.end - 1
        } else {
            range.start
        };
        let byte_range = rs3gw::storage::ByteRange {
            start: range.start,
            end: rs3gw_end,
        };

        let (_metadata, data) = self
            .storage
            .get_object(&self.bucket, &self.key, Some(byte_range))
            .await
            .map_err(Rs3gwError::from)?;

        self.write_to_cache(range, data).await;
        Ok(())
    }

    /// Shared body of the synchronous and asynchronous `read_range_into`
    /// overrides (cool-japan/oxigeo#14).
    ///
    /// The trait default would allocate a `Vec` per block and copy it into
    /// `dst`; going through [`Self::read_range_with_retry_bytes`] copies the
    /// fetched (or cached) `Bytes` straight into the caller's buffer instead,
    /// so a block-oriented reader walking thousands of tiles pays no
    /// per-block allocation and one memcpy fewer.
    async fn read_range_into_impl(
        &self,
        range: ByteRange,
        dst: &mut [u8],
    ) -> oxigeo_core::error::Result<usize> {
        // Reject an undersized `dst` before any I/O, leaving it untouched.
        // `checked_sub` keeps an inverted range out of the underflow
        // `ByteRange::len` would hit; such a range is left to
        // `read_range_with_retry_bytes`, which reports it exactly as
        // `read_range` does.
        if let Some(needed) = needed_len(range)
            && dst.len() < needed
        {
            return Err(dst_too_small(needed, dst.len()));
        }

        let data = self.read_range_with_retry_bytes(range).await?;
        // A read near end-of-file is clamped to the object size, so the
        // backend may legitimately return fewer bytes than `range.len()`.
        let available = dst.len();
        let out = dst
            .get_mut(..data.len())
            .ok_or_else(|| dst_too_small(data.len(), available))?;
        out.copy_from_slice(&data);
        Ok(data.len())
    }
}

/// Builds the error a `read_range_into` implementation returns when the
/// caller's destination buffer cannot hold the whole range.
///
/// Mirrors the message `oxigeo_core::io`'s built-in sources produce (their
/// helper is crate-private) so the diagnostic is identical whichever source a
/// caller is holding.
fn dst_too_small(needed: usize, available: usize) -> oxigeo_core::error::OxiGeoError {
    oxigeo_core::error::OxiGeoError::invalid_parameter(
        "dst",
        format!(
            "destination buffer is {available} bytes but the requested range needs {needed}; \
             size it with ByteRange::len()"
        ),
    )
}

/// Computes the destination length `range` requires, or `None` when the range
/// is itself malformed (inverted, or wider than `usize`).
fn needed_len(range: ByteRange) -> Option<usize> {
    usize::try_from(range.end.checked_sub(range.start)?).ok()
}

impl DataSource for Rs3gwDataSource {
    fn size(&self) -> oxigeo_core::error::Result<u64> {
        Ok(self.size)
    }

    fn read_range(&self, range: ByteRange) -> oxigeo_core::error::Result<Vec<u8>> {
        let source = self.clone();

        // If a tokio runtime is already driving the calling task, we must NOT
        // call `block_on` directly on that same runtime's handle -- that
        // panics with "Cannot start a runtime from within a runtime". Instead
        // use `block_in_place` to hand the current OS thread over to blocking
        // work while the runtime schedules other tasks elsewhere, then block
        // on a fresh call into that same handle. If no runtime is current,
        // spin up a throwaway one just for this call (and shut it down
        // properly afterwards, rather than leaking it).
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| {
                handle.block_on(async move {
                    source
                        .read_range_with_retry(range)
                        .await
                        .map_err(Into::into)
                })
            }),
            Err(_) => {
                let rt = tokio::runtime::Runtime::new()
                    .map_err(|e| {
                        Rs3gwError::Io(std::io::Error::other(format!(
                            "Failed to create tokio runtime: {e}"
                        )))
                    })
                    .map_err(oxigeo_core::error::OxiGeoError::from)?;

                rt.block_on(async move {
                    source
                        .read_range_with_retry(range)
                        .await
                        .map_err(Into::into)
                })
            }
        }
    }

    /// Copies the fetched (or cached) bytes straight into `dst`, skipping the
    /// intermediate `Vec` the trait's default implementation would allocate
    /// per block (cool-japan/oxigeo#14).
    ///
    /// Same runtime-reentrancy handling as [`Self::read_range`] above: never
    /// `block_on` directly on a handle that is already driving this task.
    fn read_range_into(
        &self,
        range: ByteRange,
        dst: &mut [u8],
    ) -> oxigeo_core::error::Result<usize> {
        let source = self.clone();

        match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| {
                handle.block_on(source.read_range_into_impl(range, dst))
            }),
            Err(_) => {
                let rt = tokio::runtime::Runtime::new()
                    .map_err(|e| {
                        Rs3gwError::Io(std::io::Error::other(format!(
                            "Failed to create tokio runtime: {e}"
                        )))
                    })
                    .map_err(oxigeo_core::error::OxiGeoError::from)?;

                rt.block_on(source.read_range_into_impl(range, dst))
            }
        }
    }

    // `range_slice` is deliberately left at its `None` default: this source is
    // remote-backed, and its only local store is a `moka` cache whose `get` is
    // async and yields an *owned* `Bytes` handle rather than a borrow tied to
    // `&self` (entries can also be evicted concurrently). There is nothing
    // here that can be lent out for the lifetime of `&self`, so callers must
    // keep using the copying path -- which `read_range_into` above makes as
    // cheap as it can be.

    fn read_ranges(&self, ranges: &[ByteRange]) -> oxigeo_core::error::Result<Vec<Vec<u8>>> {
        if ranges.is_empty() {
            return Ok(Vec::new());
        }

        let source = self.clone();
        let ranges_vec = ranges.to_vec();

        async fn fetch_all(
            source: Rs3gwDataSource,
            ranges_vec: Vec<ByteRange>,
        ) -> oxigeo_core::error::Result<Vec<Vec<u8>>> {
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
        }

        // Same rationale as `read_range` above: never `block_on` directly on
        // a handle that is already driving the current task.
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                tokio::task::block_in_place(|| handle.block_on(fetch_all(source, ranges_vec)))
            }
            Err(_) => {
                let rt = tokio::runtime::Runtime::new()
                    .map_err(|e| {
                        Rs3gwError::Io(std::io::Error::other(format!(
                            "Failed to create tokio runtime: {e}"
                        )))
                    })
                    .map_err(oxigeo_core::error::OxiGeoError::from)?;

                rt.block_on(fetch_all(source, ranges_vec))
            }
        }
    }
}

#[cfg(feature = "async")]
mod async_impl {
    use super::*;
    use oxigeo_core::io::AsyncDataSource;

    #[async_trait::async_trait]
    impl AsyncDataSource for Rs3gwDataSource {
        async fn size(&self) -> oxigeo_core::error::Result<u64> {
            Ok(self.size)
        }

        async fn read_range(&self, range: ByteRange) -> oxigeo_core::error::Result<Vec<u8>> {
            self.read_range_with_retry(range).await.map_err(Into::into)
        }

        /// Copies the fetched (or cached) bytes straight into `dst`, skipping
        /// the intermediate `Vec` the trait's default implementation would
        /// allocate per block (cool-japan/oxigeo#14).
        async fn read_range_into(
            &self,
            range: ByteRange,
            dst: &mut [u8],
        ) -> oxigeo_core::error::Result<usize> {
            self.read_range_into_impl(range, dst).await
        }

        async fn read_ranges(
            &self,
            ranges: &[ByteRange],
        ) -> oxigeo_core::error::Result<Vec<Vec<u8>>> {
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

/// Helper to convert oxigeo ByteRange to rs3gw ByteRange
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

    /// Puts `payload` under `test-bucket/data.bin` on a throwaway local
    /// backend and returns a data source over it.
    async fn source_over(payload: &'static [u8]) -> (Rs3gwDataSource, TempDir) {
        let (backend, temp_dir) = create_test_backend().await;
        backend
            .create_bucket("test-bucket")
            .await
            .expect("Failed to create bucket");
        backend
            .put_object(
                "test-bucket",
                "data.bin",
                Bytes::from_static(payload),
                std::collections::HashMap::new(),
            )
            .await
            .expect("Failed to put object");
        let source =
            Rs3gwDataSource::new(backend, "test-bucket".to_string(), "data.bin".to_string())
                .await
                .expect("Failed to create data source");
        (source, temp_dir)
    }

    const ISSUE_14_PAYLOAD: &[u8] = b"0123456789ABCDEF";

    /// cool-japan/oxigeo#14: `read_range_into` must be byte-equivalent to
    /// `read_range` -- including where this source *clamps* a read to the
    /// object size instead of erroring, in which case it reports its own
    /// shorter length.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_issue_14_read_range_into_matches_read_range() {
        let (source, _temp_dir) = source_over(ISSUE_14_PAYLOAD).await;

        for range in [
            ByteRange::new(0, 16),  // whole object
            ByteRange::new(5, 10),  // interior
            ByteRange::new(0, 1),   // leading boundary
            ByteRange::new(15, 16), // trailing boundary
            ByteRange::new(4, 4),   // empty
            ByteRange::new(10, 30), // past EOF -- clamped to 10..16 by this source
        ] {
            let expected = DataSource::read_range(&source, range).expect("read_range");
            let mut dst = vec![0xAAu8; 32];
            let written =
                DataSource::read_range_into(&source, range, &mut dst).expect("read_range_into");
            assert_eq!(written, expected.len(), "count mismatch for {range:?}");
            assert_eq!(
                &dst[..written],
                &expected[..],
                "bytes mismatch for {range:?}"
            );
            assert!(
                dst[written..].iter().all(|b| *b == 0xAA),
                "tail beyond {written} bytes must be left alone for {range:?}"
            );
        }

        // A start at or past end-of-object errors identically on both paths.
        for range in [ByteRange::new(16, 20), ByteRange::new(99, 100)] {
            assert!(
                DataSource::read_range(&source, range).is_err(),
                "read_range {range:?}"
            );
            let mut dst = vec![0u8; 32];
            assert!(
                DataSource::read_range_into(&source, range, &mut dst).is_err(),
                "read_range_into {range:?}"
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_issue_14_read_range_into_buffer_sizing() {
        let (source, _temp_dir) = source_over(ISSUE_14_PAYLOAD).await;
        let range = ByteRange::new(4, 12);

        // Too short: rejected before any I/O, `dst` untouched.
        let mut dst = vec![0xEEu8; 7];
        let err = DataSource::read_range_into(&source, range, &mut dst)
            .expect_err("short dst must be rejected");
        assert!(
            matches!(
                err,
                oxigeo_core::error::OxiGeoError::InvalidParameter { parameter, .. }
                    if parameter == "dst"
            ),
            "expected an InvalidParameter(dst) error, got {err}"
        );
        assert_eq!(dst, vec![0xEE; 7], "dst must be untouched");

        // Exactly-sized and over-sized destinations both work.
        let mut dst = vec![0xEEu8; 8];
        assert_eq!(
            DataSource::read_range_into(&source, range, &mut dst).expect("exact dst"),
            8
        );
        assert_eq!(&dst[..], b"456789AB");

        // An empty range writes nothing, even into an empty destination.
        assert_eq!(
            DataSource::read_range_into(&source, ByteRange::new(3, 3), &mut [])
                .expect("empty range"),
            0
        );
    }

    /// A remote-backed source has nothing it can lend for the lifetime of
    /// `&self` (its `moka` cache yields owned, evictable `Bytes`), so it must
    /// keep answering `None` and let callers use the copying path.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_issue_14_range_slice_is_none_for_remote_source() {
        let (source, _temp_dir) = source_over(ISSUE_14_PAYLOAD).await;

        // Even after a read has warmed the cache, nothing can be borrowed.
        let _ = DataSource::read_range(&source, ByteRange::new(0, 8)).expect("warm the cache");
        assert!(source.is_cached(ByteRange::new(0, 8)).await);
        assert!(DataSource::range_slice(&source, ByteRange::new(0, 8)).is_none());
        assert!(DataSource::range_slice(&source, ByteRange::new(0, 16)).is_none());
    }

    /// The async sibling overrides `read_range_into` too, with the same
    /// contract.
    #[cfg(feature = "async")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_issue_14_async_read_range_into_matches_read_range() {
        use oxigeo_core::io::AsyncDataSource;

        let (source, _temp_dir) = source_over(ISSUE_14_PAYLOAD).await;

        for range in [
            ByteRange::new(0, 16),
            ByteRange::new(5, 10),
            ByteRange::new(15, 16),
            ByteRange::new(4, 4),
            ByteRange::new(10, 30),
        ] {
            let expected = AsyncDataSource::read_range(&source, range)
                .await
                .expect("async read_range");
            let mut dst = vec![0xAAu8; 32];
            let written = AsyncDataSource::read_range_into(&source, range, &mut dst)
                .await
                .expect("async read_range_into");
            assert_eq!(written, expected.len(), "count mismatch for {range:?}");
            assert_eq!(
                &dst[..written],
                &expected[..],
                "bytes mismatch for {range:?}"
            );
            assert!(
                dst[written..].iter().all(|b| *b == 0xAA),
                "tail must be left alone for {range:?}"
            );
        }

        let mut dst = vec![0xEEu8; 3];
        let err = AsyncDataSource::read_range_into(&source, ByteRange::new(0, 8), &mut dst)
            .await
            .expect_err("short dst must be rejected");
        assert!(
            matches!(
                err,
                oxigeo_core::error::OxiGeoError::InvalidParameter { parameter, .. }
                    if parameter == "dst"
            ),
            "expected an InvalidParameter(dst) error, got {err}"
        );
        assert_eq!(dst, vec![0xEE; 3], "dst must be untouched");
    }

    /// Regression test for the "Cannot start a runtime from within a
    /// runtime" panic: calling the *synchronous* `DataSource::read_range`
    /// from code that is already executing inside a tokio task (e.g. a WCS
    /// handler) must not panic. This is exactly the call path
    /// `oxigeo-drivers/geotiff`'s COG reader and `oxigeo-services`'s WCS
    /// coverage handler exercise in production.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_sync_read_range_from_within_tokio_runtime_does_not_panic() {
        let (backend, _temp_dir) = create_test_backend().await;

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

        let source =
            Rs3gwDataSource::new(backend, "test-bucket".to_string(), "data.bin".to_string())
                .await
                .expect("Failed to create data source");

        // We are inside a #[tokio::test] task right now, so a `Handle` is
        // already current. Calling the sync trait method directly here
        // (exactly as e.g. oxigeo-drivers' COG reader or oxigeo-services'
        // WCS handler would from within an async request handler) previously
        // panicked with "Cannot start a runtime from within a runtime".
        let data = DataSource::read_range(&source, ByteRange::new(0, 5))
            .expect("read_range should not panic or error when called from inside a runtime");
        assert_eq!(data, b"01234");

        // Also exercise read_ranges the same way.
        let results =
            DataSource::read_ranges(&source, &[ByteRange::new(0, 5), ByteRange::new(5, 10)])
                .expect("read_ranges should not panic or error when called from inside a runtime");
        assert_eq!(results, vec![b"01234".to_vec(), b"56789".to_vec()]);
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
        use oxigeo_core::io::AsyncDataSource;

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
        use oxigeo_core::io::AsyncDataSource;

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
        use oxigeo_core::io::AsyncDataSource;
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
        use oxigeo_core::io::AsyncDataSource;
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

    #[tokio::test]
    async fn test_spatial_prefetch_warms_cache_for_following_ranges() {
        let (backend, _temp_dir) = create_test_backend().await;

        backend
            .create_bucket("test-bucket")
            .await
            .expect("Failed to create bucket");

        // 100 bytes -> 10 chunks of 10 bytes each.
        let test_data: Vec<u8> = (0..100).map(|i| (i % 256) as u8).collect();
        backend
            .put_object(
                "test-bucket",
                "prefetch.bin",
                Bytes::from(test_data.clone()),
                std::collections::HashMap::new(),
            )
            .await
            .expect("Failed to put object");

        // No warm-up needed, prefetch 2 chunks ahead.
        let config = ConcurrentReadConfig::new()
            .with_cache(true)
            .with_spatial_prefetch(true)
            .with_prefetch_radius(2)
            .with_prefetch_warmup_reads(0);

        let source = Rs3gwDataSource::new_with_config(
            backend,
            "test-bucket".to_string(),
            "prefetch.bin".to_string(),
            config,
        )
        .await
        .expect("Failed to create data source");

        // Trigger a read (cache miss) of the first chunk; this should spawn
        // background prefetch for the next two 10-byte chunks.
        let data = source
            .read_range_with_retry(ByteRange::new(0, 10))
            .await
            .expect("Failed to read range");
        assert_eq!(&data, &test_data[0..10]);

        // Give the spawned background task a chance to run.
        for _ in 0..200 {
            if source.is_cached(ByteRange::new(10, 20)).await
                && source.is_cached(ByteRange::new(20, 30)).await
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        assert!(
            source.is_cached(ByteRange::new(10, 20)).await,
            "expected the next chunk to have been prefetched into the cache"
        );
        assert!(
            source.is_cached(ByteRange::new(20, 30)).await,
            "expected the chunk after that to have been prefetched into the cache (radius=2)"
        );
        // Beyond the configured radius, nothing should have been prefetched.
        assert!(
            !source.is_cached(ByteRange::new(30, 40)).await,
            "prefetch radius is 2, so a third chunk ahead must not be prefetched"
        );
    }

    #[tokio::test]
    async fn test_spatial_prefetch_disabled_by_default_does_not_warm_cache() {
        let (backend, _temp_dir) = create_test_backend().await;

        backend
            .create_bucket("test-bucket")
            .await
            .expect("Failed to create bucket");

        let test_data: Vec<u8> = (0..100).map(|i| (i % 256) as u8).collect();
        backend
            .put_object(
                "test-bucket",
                "no_prefetch.bin",
                Bytes::from(test_data),
                std::collections::HashMap::new(),
            )
            .await
            .expect("Failed to put object");

        // Default config has spatial_prefetch = false.
        let source = Rs3gwDataSource::new(
            backend,
            "test-bucket".to_string(),
            "no_prefetch.bin".to_string(),
        )
        .await
        .expect("Failed to create data source");

        source
            .read_range_with_retry(ByteRange::new(0, 10))
            .await
            .expect("Failed to read range");

        tokio::time::sleep(Duration::from_millis(50)).await;

        assert!(
            !source.is_cached(ByteRange::new(10, 20)).await,
            "spatial_prefetch defaults to false; no background prefetch should occur"
        );
    }

    #[tokio::test]
    async fn test_spatial_prefetch_respects_warmup_gate() {
        let (backend, _temp_dir) = create_test_backend().await;

        backend
            .create_bucket("test-bucket")
            .await
            .expect("Failed to create bucket");

        let test_data: Vec<u8> = (0..100).map(|i| (i % 256) as u8).collect();
        backend
            .put_object(
                "test-bucket",
                "warmup.bin",
                Bytes::from(test_data),
                std::collections::HashMap::new(),
            )
            .await
            .expect("Failed to put object");

        // Require 3 reads before prefetch activates.
        let config = ConcurrentReadConfig::new()
            .with_cache(true)
            .with_spatial_prefetch(true)
            .with_prefetch_radius(1)
            .with_prefetch_warmup_reads(3);

        let source = Rs3gwDataSource::new_with_config(
            backend,
            "test-bucket".to_string(),
            "warmup.bin".to_string(),
            config,
        )
        .await
        .expect("Failed to create data source");

        // First read of a distinct range: below warm-up threshold, no prefetch.
        source
            .read_range_with_retry(ByteRange::new(50, 60))
            .await
            .expect("Failed to read range");
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(
            !source.is_cached(ByteRange::new(0, 10)).await,
            "prefetch must not fire before the warm-up threshold is reached"
        );

        // Two more reads bring the total to 3, satisfying the warm-up gate.
        source
            .read_range_with_retry(ByteRange::new(60, 70))
            .await
            .expect("Failed to read range");
        source
            .read_range_with_retry(ByteRange::new(0, 10))
            .await
            .expect("Failed to read range");

        for _ in 0..200 {
            if source.is_cached(ByteRange::new(10, 20)).await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(
            source.is_cached(ByteRange::new(10, 20)).await,
            "after the warm-up gate is satisfied, prefetch should fire"
        );
    }
}
