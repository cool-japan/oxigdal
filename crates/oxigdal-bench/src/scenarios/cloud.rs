//! Cloud storage benchmark scenarios.
//!
//! This module provides benchmark scenarios for cloud storage operations including:
//! - S3 read/write performance
//! - GCS operations
//! - Azure Blob Storage
//! - Caching strategies
//! - Prefetching performance
//! - Range request optimization
//!
//! # Scope: no real network I/O
//!
//! Every scenario in this module is **deterministic and offline**: it synthesizes
//! in-memory byte buffers and performs local serialization/checksum work that is
//! representative of the CPU-side cost pattern of the named cloud operation
//! (chunking, checksumming, LRU bookkeeping, etc.). None of them construct an
//! `oxigdal-cloud` client or perform any network call, so the measured durations
//! reflect only local allocation/iteration/hashing cost, not real S3/GCS/Azure
//! network latency, TLS handshake overhead, or server-side throttling. This is
//! intentional: a benchmark suite that depends on live credentials or network
//! reachability would be flaky and unusable in CI. If real network-backed
//! measurements are needed, add them as a separate opt-in scenario/feature backed
//! by a local mock S3-compatible endpoint rather than changing these scenarios.

use crate::error::{BenchError, Result};
use crate::scenarios::BenchmarkScenario;
use std::path::PathBuf;

/// Cloud storage provider types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudProvider {
    /// Amazon S3.
    S3,
    /// Google Cloud Storage.
    Gcs,
    /// Azure Blob Storage.
    Azure,
}

/// S3 read benchmark scenario.
pub struct S3ReadScenario {
    bucket: String,
    key: String,
    #[allow(dead_code)]
    region: String,
    range_requests: bool,
    chunk_size: Option<usize>,
}

impl S3ReadScenario {
    /// Creates a new S3 read benchmark scenario.
    pub fn new<S1, S2, S3>(bucket: S1, key: S2, region: S3) -> Self
    where
        S1: Into<String>,
        S2: Into<String>,
        S3: Into<String>,
    {
        Self {
            bucket: bucket.into(),
            key: key.into(),
            region: region.into(),
            range_requests: false,
            chunk_size: None,
        }
    }

    /// Enables range requests with specified chunk size.
    pub fn with_range_requests(mut self, chunk_size: usize) -> Self {
        self.range_requests = true;
        self.chunk_size = Some(chunk_size);
        self
    }
}

impl BenchmarkScenario for S3ReadScenario {
    fn name(&self) -> &str {
        "s3_read"
    }

    fn description(&self) -> &str {
        "Benchmark local synthetic-data deserialization/buffering overhead representative \
         of S3 object read patterns (no real network I/O)"
    }

    fn setup(&mut self) -> Result<()> {
        // Validate configuration
        if self.bucket.is_empty() {
            return Err(BenchError::InvalidConfiguration(
                "Bucket name cannot be empty".to_string(),
            ));
        }

        if self.key.is_empty() {
            return Err(BenchError::InvalidConfiguration(
                "Object key cannot be empty".to_string(),
            ));
        }

        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        #[cfg(feature = "cloud")]
        {
            // Simulate S3 read: fetch data from in-memory buffer (no real network)
            // This represents the deserialization + buffering work done after receiving bytes
            let total_size = 4 * 1024 * 1024usize; // 4MB object
            if self.range_requests {
                let chunk_size = self.chunk_size.unwrap_or(8 * 1024 * 1024);
                let mut offset = 0usize;
                let mut chunks_read = 0usize;
                while offset < total_size {
                    let end = (offset + chunk_size).min(total_size);
                    let chunk_len = end - offset;
                    // Simulate range-request deserialization work
                    let _chunk: Vec<u8> =
                        (0..chunk_len).map(|i| ((offset + i) % 256) as u8).collect();
                    offset = end;
                    chunks_read += 1;
                }
                // prevent optimization
                let _ = chunks_read;
            } else {
                // Simulate full object read
                let _data: Vec<u8> = (0..total_size).map(|i| (i % 256) as u8).collect();
            }
        }

        #[cfg(not(feature = "cloud"))]
        {
            return Err(BenchError::missing_dependency("oxigdal-cloud", "cloud"));
        }

        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        Ok(())
    }
}

/// S3 write benchmark scenario.
pub struct S3WriteScenario {
    bucket: String,
    key: String,
    #[allow(dead_code)]
    region: String,
    #[allow(dead_code)]
    data_size: usize,
    multipart: bool,
    part_size: Option<usize>,
    cleanup: bool,
}

impl S3WriteScenario {
    /// Creates a new S3 write benchmark scenario.
    pub fn new<S1, S2, S3>(bucket: S1, key: S2, region: S3, data_size: usize) -> Self
    where
        S1: Into<String>,
        S2: Into<String>,
        S3: Into<String>,
    {
        Self {
            bucket: bucket.into(),
            key: key.into(),
            region: region.into(),
            data_size,
            multipart: false,
            part_size: None,
            cleanup: true,
        }
    }

    /// Enables multipart upload with specified part size.
    pub fn with_multipart(mut self, part_size: usize) -> Self {
        self.multipart = true;
        self.part_size = Some(part_size);
        self
    }

    /// Sets whether to cleanup the uploaded object after benchmark.
    pub fn with_cleanup(mut self, cleanup: bool) -> Self {
        self.cleanup = cleanup;
        self
    }
}

impl BenchmarkScenario for S3WriteScenario {
    fn name(&self) -> &str {
        "s3_write"
    }

    fn description(&self) -> &str {
        "Benchmark local synthetic-data serialization/checksum overhead representative \
         of S3 object write patterns (no real network I/O)"
    }

    fn setup(&mut self) -> Result<()> {
        if self.bucket.is_empty() {
            return Err(BenchError::InvalidConfiguration(
                "Bucket name cannot be empty".to_string(),
            ));
        }

        if self.key.is_empty() {
            return Err(BenchError::InvalidConfiguration(
                "Object key cannot be empty".to_string(),
            ));
        }

        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        #[cfg(feature = "cloud")]
        {
            // Simulate S3 write: serialize + chunk data (no real network)
            let data: Vec<u8> = (0..self.data_size).map(|i| (i % 256) as u8).collect();
            if self.multipart {
                let part_size = self.part_size.unwrap_or(5 * 1024 * 1024);
                let mut offset = 0usize;
                let mut part_number = 1u32;
                while offset < data.len() {
                    let end = (offset + part_size).min(data.len());
                    // Simulate part checksum computation (MD5-like CRC)
                    let checksum: u32 = data[offset..end]
                        .iter()
                        .enumerate()
                        .fold(0u32, |acc, (i, &b)| {
                            acc.wrapping_add((b as u32).wrapping_mul(i as u32 + 1))
                        });
                    let _ = (part_number, checksum);
                    offset = end;
                    part_number += 1;
                }
            } else {
                // Simulate single PUT: compute content hash
                let checksum: u32 = data.iter().enumerate().fold(0u32, |acc, (i, &b)| {
                    acc.wrapping_add((b as u32).wrapping_mul(i as u32 + 1))
                });
                let _ = checksum;
            }
        }

        #[cfg(not(feature = "cloud"))]
        {
            return Err(BenchError::missing_dependency("oxigdal-cloud", "cloud"));
        }

        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        #[cfg(feature = "cloud")]
        {
            if self.cleanup {
                // Object cleanup would happen here with a live client
                let _ = &self.key;
                let _ = &self.bucket;
            }
        }

        Ok(())
    }
}

/// Cloud caching benchmark scenario.
pub struct CachingScenario {
    #[allow(dead_code)]
    provider: CloudProvider,
    #[allow(dead_code)]
    bucket: String,
    #[allow(dead_code)]
    key: String,
    cache_dir: PathBuf,
    cache_size_mb: usize,
    access_pattern: CacheAccessPattern,
}

/// Cache access patterns.
#[derive(Debug, Clone, Copy)]
pub enum CacheAccessPattern {
    /// Sequential access (cache hit expected after first read).
    Sequential,
    /// Random access (variable cache hit rate).
    Random,
    /// Repeated access (high cache hit rate expected).
    Repeated,
}

impl CachingScenario {
    /// Creates a new caching benchmark scenario.
    pub fn new<S1, S2, P>(provider: CloudProvider, bucket: S1, key: S2, cache_dir: P) -> Self
    where
        S1: Into<String>,
        S2: Into<String>,
        P: Into<PathBuf>,
    {
        Self {
            provider,
            bucket: bucket.into(),
            key: key.into(),
            cache_dir: cache_dir.into(),
            cache_size_mb: 100,
            access_pattern: CacheAccessPattern::Sequential,
        }
    }

    /// Sets the cache size in megabytes.
    pub fn with_cache_size(mut self, size_mb: usize) -> Self {
        self.cache_size_mb = size_mb;
        self
    }

    /// Sets the access pattern.
    pub fn with_access_pattern(mut self, pattern: CacheAccessPattern) -> Self {
        self.access_pattern = pattern;
        self
    }
}

impl BenchmarkScenario for CachingScenario {
    fn name(&self) -> &str {
        "cloud_caching"
    }

    fn description(&self) -> &str {
        "Benchmark local in-memory LRU cache hit/miss overhead representative of cloud \
         storage caching patterns (no real network I/O)"
    }

    fn setup(&mut self) -> Result<()> {
        std::fs::create_dir_all(&self.cache_dir)?;
        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        #[cfg(feature = "cloud")]
        {
            // Simulate cloud caching: first access is a "miss" (compute), repeated are "hits" (clone)
            let object_size = 1024 * 1024usize; // 1MB
            let source_data: Vec<u8> = (0..object_size).map(|i| (i % 256) as u8).collect();
            let access_count = match self.access_pattern {
                CacheAccessPattern::Sequential => 5usize,
                CacheAccessPattern::Random => 3,
                CacheAccessPattern::Repeated => 10,
            };
            // Simulate a simple in-memory LRU cache with capacity capped by cache_size_mb
            let max_entries = (self.cache_size_mb * 1024 * 1024) / object_size.max(1);
            let max_entries = max_entries.max(1);
            let mut cache: std::collections::VecDeque<Vec<u8>> = std::collections::VecDeque::new();
            for i in 0..access_count {
                let cache_key_matches = match self.access_pattern {
                    CacheAccessPattern::Repeated => true,
                    CacheAccessPattern::Sequential => i == 0, // only first access is a miss
                    CacheAccessPattern::Random => i % 3 == 0,
                };
                if !cache_key_matches || cache.is_empty() {
                    // Cache miss: fetch + insert
                    let data = source_data.clone();
                    if cache.len() >= max_entries {
                        cache.pop_front();
                    }
                    cache.push_back(data);
                } else {
                    // Cache hit: just access
                    let _hit = cache.back();
                }
            }
        }

        #[cfg(not(feature = "cloud"))]
        {
            return Err(BenchError::missing_dependency("oxigdal-cloud", "cloud"));
        }

        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        // Clean up cache directory
        if self.cache_dir.exists() {
            let _ = std::fs::remove_dir_all(&self.cache_dir);
        }
        Ok(())
    }
}

/// Prefetch benchmark scenario.
pub struct PrefetchScenario {
    #[allow(dead_code)]
    provider: CloudProvider,
    #[allow(dead_code)]
    bucket: String,
    keys: Vec<String>,
    prefetch_count: usize,
    parallel_requests: usize,
}

impl PrefetchScenario {
    /// Creates a new prefetch benchmark scenario.
    pub fn new<S>(provider: CloudProvider, bucket: S, keys: Vec<String>) -> Self
    where
        S: Into<String>,
    {
        Self {
            provider,
            bucket: bucket.into(),
            keys,
            prefetch_count: 5,
            parallel_requests: 4,
        }
    }

    /// Sets the number of objects to prefetch.
    pub fn with_prefetch_count(mut self, count: usize) -> Self {
        self.prefetch_count = count;
        self
    }

    /// Sets the number of parallel requests.
    pub fn with_parallel_requests(mut self, count: usize) -> Self {
        self.parallel_requests = count;
        self
    }
}

impl BenchmarkScenario for PrefetchScenario {
    fn name(&self) -> &str {
        "cloud_prefetch"
    }

    fn description(&self) -> &str {
        "Benchmark local synthetic chunked-fetch/hash overhead representative of \
         parallel cloud storage prefetch patterns (no real network I/O)"
    }

    fn setup(&mut self) -> Result<()> {
        if self.keys.is_empty() {
            return Err(BenchError::InvalidConfiguration(
                "No keys provided for prefetch benchmark".to_string(),
            ));
        }

        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        #[cfg(feature = "cloud")]
        {
            // Simulate parallel prefetching: distribute keys across workers
            use std::sync::{Arc, Mutex};
            let results: Arc<Mutex<Vec<usize>>> = Arc::new(Mutex::new(Vec::new()));
            let keys_to_prefetch: Vec<String> = self
                .keys
                .iter()
                .take(self.prefetch_count)
                .cloned()
                .collect();
            // Simulate parallel fetch with rayon-like chunked processing
            let chunk_size = (keys_to_prefetch.len() / self.parallel_requests.max(1)).max(1);
            for chunk in keys_to_prefetch.chunks(chunk_size) {
                for key in chunk {
                    // Simulate per-key fetch work: compute hash of key + read 64KB
                    let hash: u64 = key.bytes().enumerate().fold(0u64, |acc, (i, b)| {
                        acc.wrapping_add((b as u64).wrapping_mul(i as u64 + 31))
                    });
                    let data_size = (hash % (64 * 1024)) as usize + 1024;
                    let _data: Vec<u8> = (0..data_size).map(|i| (i % 256) as u8).collect();
                    results
                        .lock()
                        .map_err(|e| {
                            BenchError::scenario_failed(self.name(), format!("Lock poisoned: {e}"))
                        })?
                        .push(data_size);
                }
            }
        }

        #[cfg(not(feature = "cloud"))]
        {
            return Err(BenchError::missing_dependency("oxigdal-cloud", "cloud"));
        }

        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Range request optimization benchmark scenario.
pub struct RangeRequestScenario {
    #[allow(dead_code)]
    provider: CloudProvider,
    #[allow(dead_code)]
    bucket: String,
    #[allow(dead_code)]
    key: String,
    range_sizes: Vec<usize>,
}

impl RangeRequestScenario {
    /// Creates a new range request benchmark scenario.
    pub fn new<S1, S2>(provider: CloudProvider, bucket: S1, key: S2) -> Self
    where
        S1: Into<String>,
        S2: Into<String>,
    {
        Self {
            provider,
            bucket: bucket.into(),
            key: key.into(),
            range_sizes: vec![
                64 * 1024,       // 64 KB
                256 * 1024,      // 256 KB
                1024 * 1024,     // 1 MB
                4 * 1024 * 1024, // 4 MB
            ],
        }
    }

    /// Sets the range sizes to benchmark.
    pub fn with_range_sizes(mut self, sizes: Vec<usize>) -> Self {
        self.range_sizes = sizes;
        self
    }
}

impl BenchmarkScenario for RangeRequestScenario {
    fn name(&self) -> &str {
        "range_requests"
    }

    fn description(&self) -> &str {
        "Benchmark local synthetic-data decode/checksum overhead across different range \
         sizes, representative of range-request patterns (no real network I/O)"
    }

    fn setup(&mut self) -> Result<()> {
        Ok(())
    }

    fn execute(&mut self) -> Result<()> {
        #[cfg(feature = "cloud")]
        {
            // Simulate range requests of varying sizes
            for &range_size in &self.range_sizes {
                // Simulate 10 sequential range requests of this size
                for request_idx in 0..10usize {
                    let offset = request_idx * range_size;
                    // Simulate decoding the received bytes at this range
                    let data: Vec<u8> = (0..range_size)
                        .map(|i| {
                            (((offset + i)
                                .wrapping_mul(6364136223846793005)
                                .wrapping_add(1442695040888963407))
                                % 256) as u8
                        })
                        .collect();
                    // Simulate checksum verification
                    let _checksum: u32 =
                        data.iter().fold(0u32, |acc, &b| acc.wrapping_add(b as u32));
                }
            }
        }

        #[cfg(not(feature = "cloud"))]
        {
            return Err(BenchError::missing_dependency("oxigdal-cloud", "cloud"));
        }

        Ok(())
    }

    fn teardown(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s3_read_scenario_creation() {
        let scenario = S3ReadScenario::new("my-bucket", "test.tif", "us-east-1")
            .with_range_requests(1024 * 1024);

        assert_eq!(scenario.name(), "s3_read");
        assert!(scenario.range_requests);
        assert_eq!(scenario.chunk_size, Some(1024 * 1024));
    }

    #[test]
    fn test_s3_write_scenario_creation() {
        let scenario =
            S3WriteScenario::new("my-bucket", "output.tif", "us-east-1", 10 * 1024 * 1024)
                .with_multipart(5 * 1024 * 1024)
                .with_cleanup(false);

        assert_eq!(scenario.name(), "s3_write");
        assert!(scenario.multipart);
        assert!(!scenario.cleanup);
    }

    #[test]
    fn test_caching_scenario_creation() {
        let scenario = CachingScenario::new(
            CloudProvider::S3,
            "my-bucket",
            "test.tif",
            std::env::temp_dir().join("cache"),
        )
        .with_cache_size(200)
        .with_access_pattern(CacheAccessPattern::Random);

        assert_eq!(scenario.name(), "cloud_caching");
        assert_eq!(scenario.cache_size_mb, 200);
    }

    #[test]
    fn test_prefetch_scenario_creation() {
        let keys = vec!["file1.tif".to_string(), "file2.tif".to_string()];
        let scenario = PrefetchScenario::new(CloudProvider::S3, "my-bucket", keys)
            .with_prefetch_count(10)
            .with_parallel_requests(8);

        assert_eq!(scenario.name(), "cloud_prefetch");
        assert_eq!(scenario.prefetch_count, 10);
        assert_eq!(scenario.parallel_requests, 8);
    }

    #[test]
    fn test_range_request_scenario_creation() {
        let scenario = RangeRequestScenario::new(CloudProvider::S3, "my-bucket", "test.tif")
            .with_range_sizes(vec![128 * 1024, 512 * 1024]);

        assert_eq!(scenario.name(), "range_requests");
        assert_eq!(scenario.range_sizes.len(), 2);
    }

    /// Regression test for the truth-in-labeling gap: every cloud scenario's
    /// public `description()` must disclose that it measures local synthetic
    /// serialization/checksum overhead, not real network I/O, so a report
    /// reader cannot mistake these numbers for real S3/GCS/Azure latency.
    #[test]
    fn test_cloud_scenario_descriptions_disclose_no_network_io() {
        let s3_read = S3ReadScenario::new("bucket", "key", "us-east-1");
        let s3_write = S3WriteScenario::new("bucket", "key", "us-east-1", 1024);
        let caching = CachingScenario::new(
            CloudProvider::S3,
            "bucket",
            "key",
            std::env::temp_dir().join("cloud_desc_test_cache"),
        );
        let prefetch = PrefetchScenario::new(CloudProvider::S3, "bucket", vec!["key".to_string()]);
        let range_request = RangeRequestScenario::new(CloudProvider::S3, "bucket", "key");

        let descriptions: [(&str, &str); 5] = [
            ("s3_read", s3_read.description()),
            ("s3_write", s3_write.description()),
            ("cloud_caching", caching.description()),
            ("cloud_prefetch", prefetch.description()),
            ("range_requests", range_request.description()),
        ];

        for (name, description) in descriptions {
            assert!(
                description.contains("no real network I/O"),
                "{name} description does not disclose local-only scope: {description:?}"
            );
        }
    }
}
