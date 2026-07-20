//! Parallel chunk fetching and cloud storage metrics

use super::pool::{ConnectionPoolStats, ConnectionPoolStatsSummary};
use crate::chunk::ChunkCoord;
use crate::error::ZarrError;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

// ============================================================================
// Parallel Chunk Fetcher
// ============================================================================

/// Result of a parallel chunk fetch operation
#[derive(Debug)]
pub struct ChunkFetchResult {
    /// Chunk coordinate
    pub coord: ChunkCoord,
    /// Fetched data (if successful)
    pub data: Option<Vec<u8>>,
    /// Error (if failed)
    pub error: Option<ZarrError>,
    /// Time taken to fetch
    pub duration: Duration,
    /// Number of retries
    pub retries: u32,
}

impl ChunkFetchResult {
    /// Creates a successful result
    #[must_use]
    pub fn success(coord: ChunkCoord, data: Vec<u8>, duration: Duration, retries: u32) -> Self {
        Self {
            coord,
            data: Some(data),
            error: None,
            duration,
            retries,
        }
    }

    /// Creates a failed result
    #[must_use]
    pub fn failure(coord: ChunkCoord, error: ZarrError, duration: Duration, retries: u32) -> Self {
        Self {
            coord,
            data: None,
            error: Some(error),
            duration,
            retries,
        }
    }

    /// Returns true if the fetch was successful
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.data.is_some()
    }

    /// Takes the data, leaving None
    pub fn take_data(&mut self) -> Option<Vec<u8>> {
        self.data.take()
    }
}

/// Statistics for parallel chunk fetching
#[derive(Debug, Default)]
pub struct ParallelFetchStats {
    /// Total chunks fetched
    pub total_chunks: AtomicU64,
    /// Successfully fetched chunks
    pub successful: AtomicU64,
    /// Failed chunks
    pub failed: AtomicU64,
    /// Total bytes fetched
    pub total_bytes: AtomicU64,
    /// Total retry count
    pub total_retries: AtomicU64,
    /// Total fetch time (nanoseconds)
    pub total_time_ns: AtomicU64,
}

impl ParallelFetchStats {
    /// Creates new statistics tracker
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a fetch result
    pub fn record(&self, result: &ChunkFetchResult) {
        self.total_chunks.fetch_add(1, Ordering::Relaxed);

        if result.is_success() {
            self.successful.fetch_add(1, Ordering::Relaxed);
            if let Some(ref data) = result.data {
                self.total_bytes
                    .fetch_add(data.len() as u64, Ordering::Relaxed);
            }
        } else {
            self.failed.fetch_add(1, Ordering::Relaxed);
        }

        self.total_retries
            .fetch_add(result.retries as u64, Ordering::Relaxed);
        self.total_time_ns
            .fetch_add(result.duration.as_nanos() as u64, Ordering::Relaxed);
    }

    /// Returns the success rate
    #[must_use]
    pub fn success_rate(&self) -> f64 {
        let total = self.total_chunks.load(Ordering::Relaxed);
        let successful = self.successful.load(Ordering::Relaxed);
        if total == 0 {
            return 1.0;
        }
        successful as f64 / total as f64
    }

    /// Returns the average fetch time
    #[must_use]
    pub fn average_fetch_time(&self) -> Duration {
        let total = self.total_chunks.load(Ordering::Relaxed);
        let total_ns = self.total_time_ns.load(Ordering::Relaxed);
        if total == 0 {
            return Duration::ZERO;
        }
        Duration::from_nanos(total_ns / total)
    }

    /// Returns the average bytes per chunk
    #[must_use]
    pub fn average_chunk_size(&self) -> u64 {
        let successful = self.successful.load(Ordering::Relaxed);
        let total_bytes = self.total_bytes.load(Ordering::Relaxed);
        if successful == 0 {
            return 0;
        }
        total_bytes / successful
    }

    /// Returns the throughput in bytes per second
    #[must_use]
    pub fn throughput_bytes_per_sec(&self) -> f64 {
        let total_bytes = self.total_bytes.load(Ordering::Relaxed);
        let total_ns = self.total_time_ns.load(Ordering::Relaxed);
        if total_ns == 0 {
            return 0.0;
        }
        (total_bytes as f64) / (total_ns as f64 / 1_000_000_000.0)
    }
}

// ============================================================================
// Cloud Storage Metrics
// ============================================================================

/// Comprehensive metrics for cloud storage operations
#[derive(Debug, Default)]
pub struct CloudStorageMetrics {
    /// Connection pool statistics
    pub connection_stats: ConnectionPoolStats,
    /// Parallel fetch statistics
    pub fetch_stats: ParallelFetchStats,
    /// Total range requests
    pub range_requests: AtomicU64,
    /// Total batched requests
    pub batched_requests: AtomicU64,
    /// Prefetch statistics
    pub prefetch_hits: AtomicU64,
    /// Prefetch misses
    pub prefetch_misses: AtomicU64,
}

impl CloudStorageMetrics {
    /// Creates new metrics tracker
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a range request
    pub fn record_range_request(&self) {
        self.range_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a batched request
    pub fn record_batched_request(&self, count: u64) {
        self.batched_requests.fetch_add(count, Ordering::Relaxed);
    }

    /// Records a prefetch hit
    pub fn record_prefetch_hit(&self) {
        self.prefetch_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a prefetch miss
    pub fn record_prefetch_miss(&self) {
        self.prefetch_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns the prefetch hit ratio
    #[must_use]
    pub fn prefetch_hit_ratio(&self) -> f64 {
        let hits = self.prefetch_hits.load(Ordering::Relaxed);
        let misses = self.prefetch_misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            return 0.0;
        }
        hits as f64 / total as f64
    }

    /// Returns a summary of all metrics
    #[must_use]
    pub fn summary(&self) -> CloudStorageMetricsSummary {
        CloudStorageMetricsSummary {
            connection_stats: self.connection_stats.summary(),
            fetch_success_rate: self.fetch_stats.success_rate(),
            average_fetch_time: self.fetch_stats.average_fetch_time(),
            throughput: self.fetch_stats.throughput_bytes_per_sec(),
            range_requests: self.range_requests.load(Ordering::Relaxed),
            batched_requests: self.batched_requests.load(Ordering::Relaxed),
            prefetch_hit_ratio: self.prefetch_hit_ratio(),
        }
    }
}

/// Summary of cloud storage metrics
#[derive(Debug, Clone)]
pub struct CloudStorageMetricsSummary {
    /// Connection pool statistics
    pub connection_stats: ConnectionPoolStatsSummary,
    /// Fetch success rate
    pub fetch_success_rate: f64,
    /// Average fetch time
    pub average_fetch_time: Duration,
    /// Throughput in bytes per second
    pub throughput: f64,
    /// Total range requests
    pub range_requests: u64,
    /// Total batched requests
    pub batched_requests: u64,
    /// Prefetch hit ratio
    pub prefetch_hit_ratio: f64,
}
