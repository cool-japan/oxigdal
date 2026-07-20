//! Cloud storage configuration

use std::time::Duration;

// ============================================================================
// Configuration Constants
// ============================================================================

/// Default maximum concurrent requests
pub const DEFAULT_MAX_CONCURRENT_REQUESTS: usize = 64;

/// Default maximum retries for transient failures
pub const DEFAULT_MAX_RETRIES: u32 = 5;

/// Default base delay for exponential backoff (milliseconds)
pub const DEFAULT_BASE_DELAY_MS: u64 = 100;

/// Default maximum delay for exponential backoff (milliseconds)
pub const DEFAULT_MAX_DELAY_MS: u64 = 30_000;

/// Default connection pool size per host
pub const DEFAULT_POOL_SIZE_PER_HOST: usize = 32;

/// Default request timeout (seconds)
pub const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 60;

/// Default prefetch queue size
pub const DEFAULT_PREFETCH_QUEUE_SIZE: usize = 128;

/// Default batch request size threshold (bytes)
pub const DEFAULT_BATCH_SIZE_THRESHOLD: usize = 4 * 1024 * 1024; // 4 MB

/// Default minimum request size for batching (bytes)
pub const DEFAULT_MIN_REQUEST_SIZE: usize = 64 * 1024; // 64 KB

// ============================================================================
// Cloud Storage Configuration
// ============================================================================

/// Configuration for cloud storage optimizations
#[derive(Debug, Clone)]
pub struct CloudStorageConfig {
    /// Maximum number of concurrent requests
    pub max_concurrent_requests: usize,
    /// Maximum number of retries for transient failures
    pub max_retries: u32,
    /// Base delay for exponential backoff
    pub base_delay: Duration,
    /// Maximum delay for exponential backoff
    pub max_delay: Duration,
    /// Connection pool size per host
    pub pool_size_per_host: usize,
    /// Request timeout
    pub request_timeout: Duration,
    /// Enable prefetching
    pub enable_prefetch: bool,
    /// Prefetch queue size
    pub prefetch_queue_size: usize,
    /// Enable request batching
    pub enable_batching: bool,
    /// Batch size threshold (bytes)
    pub batch_size_threshold: usize,
    /// Minimum request size for batching
    pub min_request_size: usize,
    /// Enable range requests
    pub enable_range_requests: bool,
    /// Enable streaming reads
    pub enable_streaming: bool,
    /// Streaming buffer size
    pub streaming_buffer_size: usize,
}

impl Default for CloudStorageConfig {
    fn default() -> Self {
        Self {
            max_concurrent_requests: DEFAULT_MAX_CONCURRENT_REQUESTS,
            max_retries: DEFAULT_MAX_RETRIES,
            base_delay: Duration::from_millis(DEFAULT_BASE_DELAY_MS),
            max_delay: Duration::from_millis(DEFAULT_MAX_DELAY_MS),
            pool_size_per_host: DEFAULT_POOL_SIZE_PER_HOST,
            request_timeout: Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS),
            enable_prefetch: true,
            prefetch_queue_size: DEFAULT_PREFETCH_QUEUE_SIZE,
            enable_batching: true,
            batch_size_threshold: DEFAULT_BATCH_SIZE_THRESHOLD,
            min_request_size: DEFAULT_MIN_REQUEST_SIZE,
            enable_range_requests: true,
            enable_streaming: false,
            streaming_buffer_size: 8 * 1024 * 1024, // 8 MB
        }
    }
}

impl CloudStorageConfig {
    /// Creates a new configuration with default values
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum number of concurrent requests
    #[must_use]
    pub fn with_max_concurrent_requests(mut self, max: usize) -> Self {
        self.max_concurrent_requests = max.max(1);
        self
    }

    /// Sets the maximum number of retries
    #[must_use]
    pub fn with_max_retries(mut self, max: u32) -> Self {
        self.max_retries = max;
        self
    }

    /// Sets the base delay for exponential backoff
    #[must_use]
    pub fn with_base_delay(mut self, delay: Duration) -> Self {
        self.base_delay = delay;
        self
    }

    /// Sets the maximum delay for exponential backoff
    #[must_use]
    pub fn with_max_delay(mut self, delay: Duration) -> Self {
        self.max_delay = delay;
        self
    }

    /// Sets the connection pool size per host
    #[must_use]
    pub fn with_pool_size_per_host(mut self, size: usize) -> Self {
        self.pool_size_per_host = size.max(1);
        self
    }

    /// Sets the request timeout
    #[must_use]
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Enables or disables prefetching
    #[must_use]
    pub fn with_prefetch(mut self, enable: bool) -> Self {
        self.enable_prefetch = enable;
        self
    }

    /// Sets the prefetch queue size
    #[must_use]
    pub fn with_prefetch_queue_size(mut self, size: usize) -> Self {
        self.prefetch_queue_size = size;
        self
    }

    /// Enables or disables request batching
    #[must_use]
    pub fn with_batching(mut self, enable: bool) -> Self {
        self.enable_batching = enable;
        self
    }

    /// Enables or disables range requests
    #[must_use]
    pub fn with_range_requests(mut self, enable: bool) -> Self {
        self.enable_range_requests = enable;
        self
    }

    /// Enables or disables streaming reads
    #[must_use]
    pub fn with_streaming(mut self, enable: bool) -> Self {
        self.enable_streaming = enable;
        self
    }

    /// Creates a high-throughput configuration optimized for bulk transfers
    #[must_use]
    pub fn high_throughput() -> Self {
        Self {
            max_concurrent_requests: 128,
            max_retries: 3,
            base_delay: Duration::from_millis(50),
            max_delay: Duration::from_secs(10),
            pool_size_per_host: 64,
            request_timeout: Duration::from_secs(120),
            enable_prefetch: true,
            prefetch_queue_size: 256,
            enable_batching: true,
            batch_size_threshold: 8 * 1024 * 1024,
            min_request_size: 128 * 1024,
            enable_range_requests: true,
            enable_streaming: true,
            streaming_buffer_size: 16 * 1024 * 1024,
        }
    }

    /// Creates a low-latency configuration optimized for interactive access
    #[must_use]
    pub fn low_latency() -> Self {
        Self {
            max_concurrent_requests: 32,
            max_retries: 2,
            base_delay: Duration::from_millis(25),
            max_delay: Duration::from_secs(5),
            pool_size_per_host: 16,
            request_timeout: Duration::from_secs(30),
            enable_prefetch: false,
            prefetch_queue_size: 64,
            enable_batching: false,
            batch_size_threshold: 1024 * 1024,
            min_request_size: 32 * 1024,
            enable_range_requests: true,
            enable_streaming: false,
            streaming_buffer_size: 4 * 1024 * 1024,
        }
    }

    /// Creates a memory-efficient configuration for constrained environments
    #[must_use]
    pub fn memory_efficient() -> Self {
        Self {
            max_concurrent_requests: 8,
            max_retries: 5,
            base_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(60),
            pool_size_per_host: 4,
            request_timeout: Duration::from_secs(120),
            enable_prefetch: false,
            prefetch_queue_size: 16,
            enable_batching: true,
            batch_size_threshold: 2 * 1024 * 1024,
            min_request_size: 64 * 1024,
            enable_range_requests: true,
            enable_streaming: true,
            streaming_buffer_size: 2 * 1024 * 1024,
        }
    }
}
