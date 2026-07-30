//! HTTP fetch backend for WASM
//!
//! This module provides a DataSource implementation using the browser's fetch API
//! with advanced features including retry logic, parallel range requests, bandwidth
//! throttling, and progress tracking.
//!
//! # Overview
//!
//! The fetch module is the network layer for oxigeo-wasm, providing:
//!
//! - **HTTP Range Requests**: Efficient partial file reads using byte ranges
//! - **Retry Logic**: Automatic retry with exponential backoff
//! - **Parallel Fetching**: Multiple concurrent range requests
//! - **Statistics Tracking**: Bandwidth, latency, success rates
//! - **Priority Queuing**: Request prioritization for responsive UX
//! - **Error Recovery**: Graceful handling of network failures
//!
//! # HTTP Range Requests
//!
//! Range requests allow reading specific byte ranges from remote files:
//!
//! ```text
//! GET /image.tif HTTP/1.1
//! Range: bytes=1024-2047
//!
//! HTTP/1.1 206 Partial Content
//! Content-Range: bytes 1024-2047/1000000
//! Content-Length: 1024
//! ```
//!
//! This is essential for COG files where we only need specific tiles.
//!
//! # Retry Strategy
//!
//! Failed requests are automatically retried with exponential backoff:
//!
//! | Attempt | Delay |
//! |---------|-------|
//! | 1       | 1s    |
//! | 2       | 2s    |
//! | 3       | 4s    |
//! | 4       | 8s    |
//!
//! Maximum delay is capped at 60s to prevent excessive waiting.
//!
//! # Performance Characteristics
//!
//! Typical latencies on good connections:
//! - HEAD request: 10-50ms
//! - Small range (< 1KB): 20-100ms
//! - Tile range (256KB): 100-500ms
//! - Large range (1MB): 500-2000ms
//!
//! Parallel requests can improve throughput by 3-5x for well-cached content.
//!
//! # CORS Requirements
//!
//! The server must send appropriate CORS headers:
//!
//! ```text
//! Access-Control-Allow-Origin: *
//! Access-Control-Allow-Methods: GET, HEAD
//! Access-Control-Allow-Headers: Range
//! Access-Control-Expose-Headers: Content-Length, Content-Range, Accept-Ranges
//! Accept-Ranges: bytes
//! ```
//!
//! # Error Handling
//!
//! Network errors are categorized into:
//!
//! ## Retryable Errors
//! - Connection timeouts
//! - DNS failures
//! - Server errors (5xx)
//! - Rate limiting (429)
//!
//! ## Non-Retryable Errors
//! - File not found (404)
//! - Access denied (403)
//! - Bad request (400)
//! - CORS errors
//!
//! # Examples
//!
//! ```ignore
//! use oxigeo_wasm::fetch::{FetchBackend, RetryConfig};
//! use oxigeo_core::io::ByteRange;
//!
//! // Simple fetch
//! let backend = FetchBackend::new("https://example.com/image.tif".to_string())
//!     .await
//!     .expect("Failed to create backend");
//!
//! // Read a byte range
//! let data = backend.read_range_async(ByteRange::from_offset_length(0, 1024))
//!     .await
//!     .expect("Failed to read range");
//!
//! // Enhanced fetch with retry
//! use oxigeo_wasm::fetch::EnhancedFetchBackend;
//!
//! let mut enhanced = EnhancedFetchBackend::new("https://example.com/image.tif".to_string())
//!     .await
//!     .expect("Failed to create backend");
//!
//! let data = enhanced.fetch_range_with_retry(ByteRange::from_offset_length(0, 1024))
//!     .await
//!     .expect("Failed to fetch");
//!
//! println!("Bandwidth: {:.2} Mbps", enhanced.stats().average_throughput_bps() * 8.0 / 1_000_000.0);
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, Request, RequestInit, RequestMode, Response};

use oxigeo_core::error::{IoError, OxiGeoError, Result};
use oxigeo_core::io::{ByteRange, DataSource};

use crate::error::{FetchError, WasmError, WasmResult};
use crate::memory_source::{dst_too_small, needed_len};

/// Default maximum retry attempts
pub const DEFAULT_MAX_RETRIES: u32 = 3;

/// Default retry delay in milliseconds
pub const DEFAULT_RETRY_DELAY_MS: u64 = 1000;

/// Default request timeout in milliseconds
pub const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 30000;

/// Maximum parallel range requests
#[allow(dead_code)]
pub const DEFAULT_MAX_PARALLEL_REQUESTS: usize = 6;

/// HTTP fetch backend using browser's fetch API
#[derive(Debug)]
pub struct FetchBackend {
    url: String,
    size: u64,
    supports_range: bool,
}

impl FetchBackend {
    /// Creates a new fetch backend
    ///
    /// Performs a HEAD request to determine file size and range request support.
    pub async fn new(url: String) -> Result<Self> {
        // Perform HEAD request to get size and check range support
        let window = web_sys::window().ok_or_else(|| OxiGeoError::Internal {
            message: "No window object available".to_string(),
        })?;

        let opts = RequestInit::new();
        opts.set_method("HEAD");
        opts.set_mode(RequestMode::Cors);

        let request = Request::new_with_str_and_init(&url, &opts).map_err(|e| {
            OxiGeoError::Io(IoError::Network {
                message: format!("Failed to create request: {:?}", e),
            })
        })?;

        let response = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| {
                OxiGeoError::Io(IoError::Network {
                    message: format!("Fetch failed: {:?}", e),
                })
            })?;

        let response: Response = response.dyn_into().map_err(|_| OxiGeoError::Internal {
            message: "Response is not a Response object".to_string(),
        })?;

        if !response.ok() {
            return Err(OxiGeoError::Io(IoError::Http {
                status: response.status(),
                message: response.status_text(),
            }));
        }

        // Get content length
        let headers = response.headers();
        let size = headers
            .get("content-length")
            .ok()
            .flatten()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        // Check range support
        let supports_range = headers
            .get("accept-ranges")
            .ok()
            .flatten()
            .map(|v| v.to_lowercase() == "bytes")
            .unwrap_or(false);

        Ok(Self {
            url,
            size,
            supports_range,
        })
    }

    /// Returns the URL
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns whether range requests are supported
    #[must_use]
    pub const fn supports_range(&self) -> bool {
        self.supports_range
    }

    /// Performs a range request
    async fn fetch_range_async(&self, range: ByteRange) -> Result<Vec<u8>> {
        let window = web_sys::window().ok_or_else(|| OxiGeoError::Internal {
            message: "No window object available".to_string(),
        })?;

        let opts = RequestInit::new();
        opts.set_method("GET");
        opts.set_mode(RequestMode::Cors);

        let headers = Headers::new().map_err(|e| OxiGeoError::Internal {
            message: format!("Failed to create headers: {:?}", e),
        })?;

        headers
            .set("Range", &format!("bytes={}-{}", range.start, range.end - 1))
            .map_err(|e| OxiGeoError::Internal {
                message: format!("Failed to set Range header: {:?}", e),
            })?;

        opts.set_headers(&headers);

        let request = Request::new_with_str_and_init(&self.url, &opts).map_err(|e| {
            OxiGeoError::Io(IoError::Network {
                message: format!("Failed to create request: {:?}", e),
            })
        })?;

        let response = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| {
                OxiGeoError::Io(IoError::Network {
                    message: format!("Fetch failed: {:?}", e),
                })
            })?;

        let response: Response = response.dyn_into().map_err(|_| OxiGeoError::Internal {
            message: "Response is not a Response object".to_string(),
        })?;

        if !response.ok() && response.status() != 206 {
            return Err(OxiGeoError::Io(IoError::Http {
                status: response.status(),
                message: response.status_text(),
            }));
        }

        let array_buffer =
            JsFuture::from(response.array_buffer().map_err(|e| OxiGeoError::Internal {
                message: format!("Failed to get array buffer: {:?}", e),
            })?)
            .await
            .map_err(|e| {
                OxiGeoError::Io(IoError::Read {
                    message: format!("Failed to read response: {:?}", e),
                })
            })?;

        let uint8_array = js_sys::Uint8Array::new(&array_buffer);
        let bytes = uint8_array.to_vec();

        // Defense in depth: a non-empty range that comes back with an empty
        // body (a CORS-opaque response, or a server that answered `ok`/`206`
        // with no content) is never usable — surface a typed error instead of
        // handing callers a buffer that is silently shorter than requested.
        // We do not require an *exact* length match here: servers may
        // legitimately truncate a range near end-of-file (e.g. a fixed-size
        // metadata read past a small file's end), and callers that read
        // structured data (e.g. TIFF/IFD parsing) already validate the
        // returned length against what they need before indexing it.
        if range.end > range.start && bytes.is_empty() {
            return Err(OxiGeoError::Io(IoError::Read {
                message: format!(
                    "short read: requested {} bytes at offset {}, got an empty body",
                    range.end - range.start,
                    range.start
                ),
            }));
        }

        Ok(bytes)
    }
}

// `read_range_into` / `range_slice` (cool-japan/oxigeo#14) are deliberately
// left at their trait defaults here: this backend holds no bytes of its own --
// every read is a live `fetch()` -- so there is nothing to lend, and the
// synchronous path below cannot produce data to write into a caller's buffer
// either. The defaults (`None`, and "reject an undersized `dst`, then surface
// the same `NotSupported` error") are exactly right.
impl DataSource for FetchBackend {
    fn size(&self) -> Result<u64> {
        Ok(self.size)
    }

    fn read_range(&self, _range: ByteRange) -> Result<Vec<u8>> {
        // In WASM, we need to use async, but DataSource trait is sync
        // This is a blocking wrapper that uses wasm_bindgen_futures
        // In practice, this should be called from async context

        // Use a synchronous fallback: we cache fetched data
        // For now, return an error indicating async should be used
        Err(OxiGeoError::NotSupported {
            operation: "Synchronous read in WASM - use async methods".to_string(),
        })
    }

    fn supports_range_requests(&self) -> bool {
        self.supports_range
    }
}

// For use in async contexts
impl FetchBackend {
    /// Reads a range asynchronously
    pub async fn read_range_async(&self, range: ByteRange) -> Result<Vec<u8>> {
        self.fetch_range_async(range).await
    }

    /// Reads multiple ranges asynchronously
    pub async fn read_ranges_async(&self, ranges: &[ByteRange]) -> Result<Vec<Vec<u8>>> {
        let mut results = Vec::with_capacity(ranges.len());
        for range in ranges {
            results.push(self.fetch_range_async(*range).await?);
        }
        Ok(results)
    }
}

/// Async DataSource wrapper (for future use)
#[allow(dead_code)]
pub struct AsyncFetchBackend {
    inner: FetchBackend,
    cache: std::collections::HashMap<(u64, u64), Vec<u8>>,
}

#[allow(dead_code)] // Reserved for future async implementation
impl AsyncFetchBackend {
    /// Creates a new async fetch backend
    pub async fn new(url: String) -> Result<Self> {
        let inner = FetchBackend::new(url).await?;
        Ok(Self {
            inner,
            cache: std::collections::HashMap::new(),
        })
    }

    /// Prefetches and caches a range
    pub async fn prefetch(&mut self, range: ByteRange) -> Result<()> {
        let data = self.inner.fetch_range_async(range).await?;
        self.cache.insert((range.start, range.end), data);
        Ok(())
    }

    /// Gets cached data or fetches it
    pub async fn get_range(&mut self, range: ByteRange) -> Result<Vec<u8>> {
        let key = (range.start, range.end);
        if let Some(data) = self.cache.get(&key) {
            return Ok(data.clone());
        }

        let data = self.inner.fetch_range_async(range).await?;
        self.cache.insert(key, data.clone());
        Ok(data)
    }
}

/// Synchronous wrapper that pre-fetches all needed data
#[allow(dead_code)]
pub struct PrefetchedFetchBackend {
    url: String,
    size: u64,
    data: Vec<u8>,
}

#[allow(dead_code)] // Reserved for future prefetch optimization
impl PrefetchedFetchBackend {
    /// Creates a new prefetched backend by downloading the entire file
    pub async fn new(url: String) -> Result<Self> {
        let backend = FetchBackend::new(url.clone()).await?;
        let size = backend.size;

        // Fetch entire file
        let data = backend
            .fetch_range_async(ByteRange::from_offset_length(0, size))
            .await?;

        Ok(Self { url, size, data })
    }

    /// Creates a prefetched backend with just the header portion
    pub async fn with_header(url: String, header_size: u64) -> Result<Self> {
        let backend = FetchBackend::new(url.clone()).await?;
        let size = backend.size;

        let data = backend
            .fetch_range_async(ByteRange::from_offset_length(0, header_size))
            .await?;

        Ok(Self { url, size, data })
    }

    /// Borrows `range` out of the prefetched buffer, or reports the same
    /// end-of-file error [`DataSource::read_range`] reports for it.
    ///
    /// Bounds are checked against the bytes actually held, not against
    /// [`Self::size`]: `with_header` prefetches only the leading portion of a
    /// much larger remote object.
    ///
    /// `usize::try_from` rather than `as usize`: on `wasm32` (a 32-bit target)
    /// a cast would silently truncate an offset past 4 GiB, and the previous
    /// `range.end as usize` check also let an inverted range through to a
    /// panicking slice index.
    fn slice_for(&self, range: ByteRange) -> Result<&[u8]> {
        let eof = || {
            OxiGeoError::Io(IoError::UnexpectedEof {
                offset: range.start,
            })
        };
        let start = usize::try_from(range.start).map_err(|_| eof())?;
        let end = usize::try_from(range.end).map_err(|_| eof())?;
        self.data.get(start..end).ok_or_else(eof)
    }
}

impl DataSource for PrefetchedFetchBackend {
    fn size(&self) -> Result<u64> {
        Ok(self.size)
    }

    fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
        Ok(self.slice_for(range)?.to_vec())
    }

    /// Copies straight out of the prefetched buffer, skipping the intermediate
    /// `Vec` the trait's default implementation would allocate per block
    /// (cool-japan/oxigeo#14).
    fn read_range_into(&self, range: ByteRange, dst: &mut [u8]) -> Result<usize> {
        if let Some(needed) = needed_len(range)
            && dst.len() < needed
        {
            return Err(dst_too_small(needed, dst.len()));
        }
        let src = self.slice_for(range)?;
        let available = dst.len();
        let out = dst
            .get_mut(..src.len())
            .ok_or_else(|| dst_too_small(src.len(), available))?;
        out.copy_from_slice(src);
        Ok(src.len())
    }

    /// Lends the requested bytes straight out of the prefetched buffer: once a
    /// range has been downloaded, re-reading it costs neither an allocation nor
    /// a copy.
    fn range_slice(&self, range: ByteRange) -> Option<&[u8]> {
        self.slice_for(range).ok()
    }
}

/// Retry configuration
#[derive(Debug, Clone, Copy)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Initial retry delay in milliseconds
    pub initial_delay_ms: u64,
    /// Exponential backoff multiplier
    pub backoff_multiplier: f64,
    /// Maximum retry delay in milliseconds
    pub max_delay_ms: u64,
}

impl RetryConfig {
    /// Creates a new retry configuration
    pub const fn new(max_retries: u32, initial_delay_ms: u64) -> Self {
        Self {
            max_retries,
            initial_delay_ms,
            backoff_multiplier: 2.0,
            max_delay_ms: 60000,
        }
    }

    /// Returns the default retry configuration
    pub const fn default_config() -> Self {
        Self::new(DEFAULT_MAX_RETRIES, DEFAULT_RETRY_DELAY_MS)
    }

    /// Calculates the delay for a given retry attempt
    pub fn delay_for_attempt(&self, attempt: u32) -> u64 {
        let delay =
            (self.initial_delay_ms as f64 * self.backoff_multiplier.powi(attempt as i32)) as u64;
        delay.min(self.max_delay_ms)
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self::default_config()
    }
}

/// Fetch statistics
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FetchStats {
    /// Total number of requests
    pub total_requests: u64,
    /// Number of successful requests
    pub successful_requests: u64,
    /// Number of failed requests
    pub failed_requests: u64,
    /// Number of retried requests
    pub retried_requests: u64,
    /// Total bytes fetched
    pub bytes_fetched: u64,
    /// Total time spent fetching (milliseconds)
    pub total_time_ms: f64,
}

impl FetchStats {
    /// Creates new empty statistics
    pub const fn new() -> Self {
        Self {
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            retried_requests: 0,
            bytes_fetched: 0,
            total_time_ms: 0.0,
        }
    }

    /// Returns the success rate
    pub fn success_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            self.successful_requests as f64 / self.total_requests as f64
        }
    }

    /// Returns the average request time in milliseconds
    pub fn average_request_time_ms(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            self.total_time_ms / self.total_requests as f64
        }
    }

    /// Returns the average throughput in bytes per second
    pub fn average_throughput_bps(&self) -> f64 {
        if self.total_time_ms == 0.0 {
            0.0
        } else {
            (self.bytes_fetched as f64 / self.total_time_ms) * 1000.0
        }
    }
}

impl Default for FetchStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Enhanced fetch backend with retry logic and statistics
pub struct EnhancedFetchBackend {
    /// Base URL
    url: String,
    /// File size
    size: u64,
    /// Range support
    supports_range: bool,
    /// Retry configuration
    retry_config: RetryConfig,
    /// Fetch statistics
    stats: FetchStats,
    /// Request timeout in milliseconds
    #[allow(dead_code)]
    timeout_ms: u64,
}

impl EnhancedFetchBackend {
    /// Creates a new enhanced fetch backend
    pub async fn new(url: String) -> WasmResult<Self> {
        Self::with_config(url, RetryConfig::default(), DEFAULT_REQUEST_TIMEOUT_MS).await
    }

    /// Creates a new enhanced fetch backend with configuration
    pub async fn with_config(
        url: String,
        retry_config: RetryConfig,
        timeout_ms: u64,
    ) -> WasmResult<Self> {
        let (size, supports_range) = Self::probe_url(&url, &retry_config).await?;

        Ok(Self {
            url,
            size,
            supports_range,
            retry_config,
            stats: FetchStats::new(),
            timeout_ms,
        })
    }

    /// Probes a URL to get size and range support
    async fn probe_url(url: &str, retry_config: &RetryConfig) -> WasmResult<(u64, bool)> {
        for attempt in 0..=retry_config.max_retries {
            match Self::head_request(url).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    if attempt < retry_config.max_retries {
                        let delay = retry_config.delay_for_attempt(attempt);
                        Self::sleep_ms(delay).await;
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        Err(WasmError::Fetch(FetchError::RetryLimitExceeded {
            url: url.to_string(),
            attempts: retry_config.max_retries + 1,
        }))
    }

    /// Performs a HEAD request
    async fn head_request(url: &str) -> WasmResult<(u64, bool)> {
        let window = web_sys::window().ok_or_else(|| {
            WasmError::Fetch(FetchError::NetworkFailure {
                url: url.to_string(),
                message: "No window object available".to_string(),
            })
        })?;

        let opts = RequestInit::new();
        opts.set_method("HEAD");
        opts.set_mode(RequestMode::Cors);

        let request = Request::new_with_str_and_init(url, &opts).map_err(|e| {
            WasmError::Fetch(FetchError::NetworkFailure {
                url: url.to_string(),
                message: format!("Failed to create request: {e:?}"),
            })
        })?;

        let response = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| {
                WasmError::Fetch(FetchError::NetworkFailure {
                    url: url.to_string(),
                    message: format!("Fetch failed: {e:?}"),
                })
            })?;

        let response: Response = response.dyn_into().map_err(|_| {
            WasmError::Fetch(FetchError::ParseError {
                expected: "Response".to_string(),
                message: "Not a Response object".to_string(),
            })
        })?;

        if !response.ok() {
            return Err(WasmError::Fetch(FetchError::HttpError {
                status: response.status(),
                status_text: response.status_text(),
                url: url.to_string(),
            }));
        }

        let headers = response.headers();
        let size = headers
            .get("content-length")
            .ok()
            .flatten()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        let supports_range = headers
            .get("accept-ranges")
            .ok()
            .flatten()
            .map(|v| v.to_lowercase() == "bytes")
            .unwrap_or(false);

        Ok((size, supports_range))
    }

    /// Sleeps for the specified duration
    async fn sleep_ms(ms: u64) {
        let promise = js_sys::Promise::new(&mut |resolve, _reject| {
            let window = web_sys::window().expect("Window exists");
            let _ =
                window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms as i32);
        });

        let _ = JsFuture::from(promise).await;
    }

    /// Fetches a range with retry logic
    pub async fn fetch_range_with_retry(&mut self, range: ByteRange) -> WasmResult<Vec<u8>> {
        let start_time = self.current_time_ms();

        for attempt in 0..=self.retry_config.max_retries {
            self.stats.total_requests += 1;

            match self.fetch_range_once(range).await {
                Ok(data) => {
                    let elapsed = self.current_time_ms() - start_time;
                    self.stats.successful_requests += 1;
                    self.stats.bytes_fetched += data.len() as u64;
                    self.stats.total_time_ms += elapsed;

                    if attempt > 0 {
                        self.stats.retried_requests += 1;
                    }

                    return Ok(data);
                }
                Err(e) => {
                    if attempt < self.retry_config.max_retries {
                        let delay = self.retry_config.delay_for_attempt(attempt);
                        Self::sleep_ms(delay).await;
                    } else {
                        self.stats.failed_requests += 1;
                        return Err(e);
                    }
                }
            }
        }

        Err(WasmError::Fetch(FetchError::RetryLimitExceeded {
            url: self.url.clone(),
            attempts: self.retry_config.max_retries + 1,
        }))
    }

    /// Fetches a range once (without retry)
    async fn fetch_range_once(&self, range: ByteRange) -> WasmResult<Vec<u8>> {
        let window = web_sys::window().ok_or_else(|| {
            WasmError::Fetch(FetchError::NetworkFailure {
                url: self.url.clone(),
                message: "No window object available".to_string(),
            })
        })?;

        let opts = RequestInit::new();
        opts.set_method("GET");
        opts.set_mode(RequestMode::Cors);

        let headers = Headers::new().map_err(|e| {
            WasmError::Fetch(FetchError::NetworkFailure {
                url: self.url.clone(),
                message: format!("Failed to create headers: {e:?}"),
            })
        })?;

        headers
            .set("Range", &format!("bytes={}-{}", range.start, range.end - 1))
            .map_err(|e| {
                WasmError::Fetch(FetchError::NetworkFailure {
                    url: self.url.clone(),
                    message: format!("Failed to set Range header: {e:?}"),
                })
            })?;

        opts.set_headers(&headers);

        let request = Request::new_with_str_and_init(&self.url, &opts).map_err(|e| {
            WasmError::Fetch(FetchError::NetworkFailure {
                url: self.url.clone(),
                message: format!("Failed to create request: {e:?}"),
            })
        })?;

        let response = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| {
                WasmError::Fetch(FetchError::NetworkFailure {
                    url: self.url.clone(),
                    message: format!("Fetch failed: {e:?}"),
                })
            })?;

        let response: Response = response.dyn_into().map_err(|_| {
            WasmError::Fetch(FetchError::ParseError {
                expected: "Response".to_string(),
                message: "Not a Response object".to_string(),
            })
        })?;

        if !response.ok() && response.status() != 206 {
            return Err(WasmError::Fetch(FetchError::HttpError {
                status: response.status(),
                status_text: response.status_text(),
                url: self.url.clone(),
            }));
        }

        let array_buffer = JsFuture::from(response.array_buffer().map_err(|e| {
            WasmError::Fetch(FetchError::NetworkFailure {
                url: self.url.clone(),
                message: format!("Failed to get array buffer: {e:?}"),
            })
        })?)
        .await
        .map_err(|e| {
            WasmError::Fetch(FetchError::NetworkFailure {
                url: self.url.clone(),
                message: format!("Failed to read response: {e:?}"),
            })
        })?;

        let uint8_array = js_sys::Uint8Array::new(&array_buffer);
        let data = uint8_array.to_vec();

        // Validate size
        let expected_size = (range.end - range.start) as usize;
        if data.len() != expected_size {
            return Err(WasmError::Fetch(FetchError::InvalidSize {
                expected: expected_size as u64,
                actual: data.len() as u64,
            }));
        }

        Ok(data)
    }

    /// Fetches multiple ranges in parallel
    pub async fn fetch_ranges_parallel(
        &mut self,
        ranges: &[ByteRange],
        max_parallel: usize,
    ) -> WasmResult<Vec<Vec<u8>>> {
        let mut results = Vec::with_capacity(ranges.len());
        let mut pending = Vec::new();

        for (i, &range) in ranges.iter().enumerate() {
            pending.push((i, range));

            if pending.len() >= max_parallel || i == ranges.len() - 1 {
                // Fetch this batch
                let mut batch_results = Vec::new();
                for (_idx, range) in &pending {
                    let data = self.fetch_range_with_retry(*range).await?;
                    batch_results.push(data);
                }

                results.extend(batch_results);
                pending.clear();
            }
        }

        Ok(results)
    }

    /// Returns the current time in milliseconds
    fn current_time_ms(&self) -> f64 {
        js_sys::Date::now()
    }

    /// Returns fetch statistics
    pub const fn stats(&self) -> &FetchStats {
        &self.stats
    }

    /// Returns the URL
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns the file size
    pub const fn size(&self) -> u64 {
        self.size
    }

    /// Returns whether range requests are supported
    pub const fn supports_range(&self) -> bool {
        self.supports_range
    }
}

/// Request priority
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequestPriority {
    /// Low priority
    Low,
    /// Normal priority
    Normal,
    /// High priority
    High,
    /// Critical priority
    Critical,
}

/// Prioritized fetch request
#[derive(Debug, Clone)]
pub struct PrioritizedRequest {
    /// Range to fetch
    pub range: ByteRange,
    /// Priority
    pub priority: RequestPriority,
    /// Request ID
    pub id: u64,
}

impl PrioritizedRequest {
    /// Creates a new prioritized request
    pub const fn new(range: ByteRange, priority: RequestPriority, id: u64) -> Self {
        Self {
            range,
            priority,
            id,
        }
    }
}

impl PartialEq for PrioritizedRequest {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for PrioritizedRequest {}

impl PartialOrd for PrioritizedRequest {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritizedRequest {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Lower priority comes first in sort, so higher priority is at the end (for pop())
        self.priority.cmp(&other.priority)
    }
}

/// Request queue with priority management
pub struct RequestQueue {
    /// Pending requests (sorted by priority)
    requests: Vec<PrioritizedRequest>,
    /// Next request ID
    next_id: u64,
    /// Completed request IDs
    completed: HashMap<u64, Vec<u8>>,
}

impl RequestQueue {
    /// Creates a new request queue
    pub fn new() -> Self {
        Self {
            requests: Vec::new(),
            next_id: 0,
            completed: HashMap::new(),
        }
    }

    /// Adds a request to the queue
    pub fn add(&mut self, range: ByteRange, priority: RequestPriority) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let request = PrioritizedRequest::new(range, priority, id);
        self.requests.push(request);
        self.requests.sort();

        id
    }

    /// Gets the next request to process
    pub fn next(&mut self) -> Option<PrioritizedRequest> {
        self.requests.pop()
    }

    /// Marks a request as completed
    pub fn complete(&mut self, id: u64, data: Vec<u8>) {
        self.completed.insert(id, data);
    }

    /// Gets completed request data
    pub fn get_completed(&self, id: u64) -> Option<&Vec<u8>> {
        self.completed.get(&id)
    }

    /// Returns the number of pending requests
    pub fn pending_count(&self) -> usize {
        self.requests.len()
    }

    /// Clears all completed requests
    pub fn clear_completed(&mut self) {
        self.completed.clear();
    }
}

impl Default for RequestQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_config() {
        let config = RetryConfig::new(3, 1000);
        assert_eq!(config.delay_for_attempt(0), 1000);
        assert_eq!(config.delay_for_attempt(1), 2000);
        assert_eq!(config.delay_for_attempt(2), 4000);
    }

    #[test]
    fn test_fetch_stats() {
        let mut stats = FetchStats::new();
        stats.total_requests = 10;
        stats.successful_requests = 8;
        stats.bytes_fetched = 1000;
        stats.total_time_ms = 100.0;

        assert_eq!(stats.success_rate(), 0.8);
        assert_eq!(stats.average_request_time_ms(), 10.0);
        assert_eq!(stats.average_throughput_bps(), 10000.0);
    }

    #[test]
    fn test_request_priority() {
        let low = PrioritizedRequest::new(
            ByteRange::from_offset_length(0, 100),
            RequestPriority::Low,
            1,
        );
        let high = PrioritizedRequest::new(
            ByteRange::from_offset_length(0, 100),
            RequestPriority::High,
            2,
        );

        // Higher priority should sort greater (to be at end for pop())
        assert!(high > low);
    }

    #[test]
    fn test_request_queue() {
        let mut queue = RequestQueue::new();

        let _id1 = queue.add(ByteRange::from_offset_length(0, 100), RequestPriority::Low);
        let id2 = queue.add(
            ByteRange::from_offset_length(100, 100),
            RequestPriority::High,
        );

        // High priority should come first
        let next = queue.next().expect("Should have request");
        assert_eq!(next.id, id2);

        queue.complete(id2, vec![1, 2, 3]);
        assert!(queue.get_completed(id2).is_some());

        assert_eq!(queue.pending_count(), 1);
    }

    /// Builds a backend over an already-downloaded buffer, the state
    /// `PrefetchedFetchBackend::new` / `with_header` leave behind (both are
    /// browser-only, so the constructors themselves cannot run under `cargo
    /// test`). `size` is deliberately larger than `data`, mirroring
    /// `with_header`'s partial prefetch of a big remote object.
    fn prefetched(data: Vec<u8>, size: u64) -> PrefetchedFetchBackend {
        PrefetchedFetchBackend {
            url: "https://example.invalid/cog.tif".to_string(),
            size,
            data,
        }
    }

    /// cool-japan/oxigeo#14: the zero-copy entry points must agree with
    /// `read_range` byte for byte, and error for error.
    #[test]
    fn test_issue_14_prefetched_read_range_into_matches_read_range() {
        let src = prefetched((0u8..32).collect(), 1 << 20);
        for range in [
            ByteRange::new(0, 32),
            ByteRange::new(8, 20),
            ByteRange::new(0, 1),
            ByteRange::new(31, 32),
            ByteRange::new(5, 5),
            ByteRange::new(32, 32),
        ] {
            let expected = src.read_range(range).expect("read_range");
            let mut dst = vec![0x5Au8; expected.len()];
            let written = src.read_range_into(range, &mut dst).expect("read_into");
            assert_eq!(written, expected.len(), "count mismatch for {range:?}");
            assert_eq!(dst, expected, "bytes mismatch for {range:?}");
        }

        // Beyond the *prefetched* bytes (even though `size` says the remote
        // object is far larger) and inverted ranges must fail identically.
        for range in [
            ByteRange::new(28, 40),
            ByteRange::new(32, 33),
            ByteRange::new(20, 8),
        ] {
            assert!(src.read_range(range).is_err(), "read_range {range:?}");
            let mut dst = vec![0u8; 64];
            let err = src
                .read_range_into(range, &mut dst)
                .expect_err("read_range_into should reject");
            assert!(
                matches!(err, OxiGeoError::Io(IoError::UnexpectedEof { .. })),
                "expected EOF for {range:?}, got {err}"
            );
        }
    }

    #[test]
    fn test_issue_14_prefetched_read_range_into_buffer_sizing() {
        let src = prefetched((0u8..16).collect(), 16);
        let range = ByteRange::new(4, 12);

        let mut dst = vec![0xEEu8; 12];
        assert_eq!(src.read_range_into(range, &mut dst).expect("read_into"), 8);
        assert_eq!(&dst[..8], &(4u8..12).collect::<Vec<u8>>()[..]);
        assert_eq!(&dst[8..], &[0xEE; 4], "tail must be left alone");

        let mut dst = vec![0xEEu8; 7];
        let err = src
            .read_range_into(range, &mut dst)
            .expect_err("short dst must be rejected");
        assert!(
            matches!(err, OxiGeoError::InvalidParameter { parameter, .. } if parameter == "dst"),
            "expected an InvalidParameter(dst) error, got {err}"
        );
        assert_eq!(dst, vec![0xEE; 7], "dst must be untouched");

        assert_eq!(
            src.read_range_into(ByteRange::new(3, 3), &mut [])
                .expect("empty range"),
            0
        );
    }

    #[test]
    fn test_issue_14_prefetched_range_slice_borrows_backing_buffer() {
        let src = prefetched((0u8..64).collect(), 1 << 30);
        let borrowed = src.range_slice(ByteRange::new(16, 48)).expect("borrow");
        assert_eq!(borrowed, &src.data[16..48]);
        assert!(
            std::ptr::eq(borrowed.as_ptr(), src.data[16..48].as_ptr()),
            "range_slice must borrow the prefetched buffer, not copy it"
        );

        assert!(
            src.range_slice(ByteRange::new(9, 9))
                .expect("empty")
                .is_empty()
        );
        assert!(
            src.range_slice(ByteRange::new(60, 65)).is_none(),
            "past the prefetched bytes"
        );
        assert!(src.range_slice(ByteRange::new(40, 8)).is_none(), "inverted");
        assert!(
            src.range_slice(ByteRange::new(u64::MAX - 1, u64::MAX))
                .is_none(),
            "unrepresentable offset"
        );
    }

    /// The live-fetch backend holds nothing it could lend, and says so.
    #[test]
    fn test_issue_14_fetch_backend_cannot_lend() {
        let backend = FetchBackend {
            url: "https://example.invalid/cog.tif".to_string(),
            size: 4096,
            supports_range: true,
        };
        assert!(backend.range_slice(ByteRange::new(0, 16)).is_none());
        assert!(
            backend
                .read_range_into(ByteRange::new(0, 16), &mut [0u8; 16])
                .is_err()
        );
    }

    // WASM-specific tests would use wasm-bindgen-test
}
