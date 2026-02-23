//! Dynamic batching implementation with priority queuing and adaptive sizing
//!
//! This module provides advanced dynamic batching capabilities including
//! priority-based request queuing, adaptive batch size adjustment, and
//! variable-sized input handling through padding strategies.

use crate::error::{InferenceError, MlError, Result};
use crate::models::Model;
use oxigdal_core::buffer::RasterBuffer;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, info, trace, warn};

/// Type alias for padded inputs and original dimensions
type PaddedInputs = (Vec<RasterBuffer>, Vec<(u64, u64)>);

/// Priority levels for batch requests
///
/// Higher priority requests are processed before lower priority ones,
/// even if they arrive later. Within the same priority level, requests
/// are processed in FIFO order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PriorityLevel {
    /// Critical priority - processed immediately, may interrupt batching
    Critical = 4,
    /// High priority - processed before normal requests
    High = 3,
    /// Normal priority - standard processing order
    #[default]
    Normal = 2,
    /// Low priority - processed when system is not busy
    Low = 1,
    /// Background priority - only processed during idle time
    Background = 0,
}

impl PartialOrd for PriorityLevel {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PriorityLevel {
    fn cmp(&self, other: &Self) -> Ordering {
        (*self as u8).cmp(&(*other as u8))
    }
}

/// Configuration for dynamic batching
#[derive(Debug, Clone)]
pub struct DynamicBatchConfig {
    /// Maximum batch size (upper bound)
    pub max_batch_size: usize,
    /// Minimum batch size before forcing execution
    pub min_batch_size: usize,
    /// Initial batch size for adaptive sizing
    pub initial_batch_size: usize,
    /// Timeout for batch formation (milliseconds)
    pub batch_timeout_ms: u64,
    /// Timeout for critical priority requests (shorter than normal)
    pub critical_timeout_ms: u64,
    /// Enable adaptive batch sizing based on patterns
    pub enable_adaptive_sizing: bool,
    /// Enable padding for variable-sized inputs
    pub enable_padding: bool,
    /// Padding strategy for variable-sized inputs
    pub padding_strategy: PaddingStrategy,
    /// Target latency in milliseconds (used for adaptive sizing)
    pub target_latency_ms: u64,
    /// Maximum queue length before rejecting requests
    pub max_queue_length: usize,
    /// Number of parallel workers for batch processing
    pub num_workers: usize,
    /// Enable request coalescing (combine similar requests)
    pub enable_coalescing: bool,
    /// Memory limit for batch formation (bytes)
    pub memory_limit_bytes: Option<usize>,
}

impl Default for DynamicBatchConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 32,
            min_batch_size: 1,
            initial_batch_size: 8,
            batch_timeout_ms: 100,
            critical_timeout_ms: 10,
            enable_adaptive_sizing: true,
            enable_padding: true,
            padding_strategy: PaddingStrategy::default(),
            target_latency_ms: 50,
            max_queue_length: 1024,
            num_workers: 4,
            enable_coalescing: false,
            memory_limit_bytes: None,
        }
    }
}

impl DynamicBatchConfig {
    /// Creates a new configuration builder
    #[must_use]
    pub fn builder() -> DynamicBatchConfigBuilder {
        DynamicBatchConfigBuilder::default()
    }

    /// Creates a low-latency configuration
    #[must_use]
    pub fn low_latency() -> Self {
        Self {
            max_batch_size: 8,
            min_batch_size: 1,
            initial_batch_size: 2,
            batch_timeout_ms: 10,
            critical_timeout_ms: 2,
            enable_adaptive_sizing: true,
            target_latency_ms: 10,
            ..Default::default()
        }
    }

    /// Creates a high-throughput configuration
    #[must_use]
    pub fn high_throughput() -> Self {
        Self {
            max_batch_size: 64,
            min_batch_size: 16,
            initial_batch_size: 32,
            batch_timeout_ms: 200,
            critical_timeout_ms: 50,
            enable_adaptive_sizing: true,
            target_latency_ms: 100,
            ..Default::default()
        }
    }
}

/// Builder for dynamic batch configuration
#[derive(Debug, Default)]
pub struct DynamicBatchConfigBuilder {
    max_batch_size: Option<usize>,
    min_batch_size: Option<usize>,
    initial_batch_size: Option<usize>,
    batch_timeout_ms: Option<u64>,
    critical_timeout_ms: Option<u64>,
    enable_adaptive_sizing: Option<bool>,
    enable_padding: Option<bool>,
    padding_strategy: Option<PaddingStrategy>,
    target_latency_ms: Option<u64>,
    max_queue_length: Option<usize>,
    num_workers: Option<usize>,
    enable_coalescing: Option<bool>,
    memory_limit_bytes: Option<usize>,
}

impl DynamicBatchConfigBuilder {
    /// Sets the maximum batch size
    #[must_use]
    pub fn max_batch_size(mut self, size: usize) -> Self {
        self.max_batch_size = Some(size);
        self
    }

    /// Sets the minimum batch size
    #[must_use]
    pub fn min_batch_size(mut self, size: usize) -> Self {
        self.min_batch_size = Some(size);
        self
    }

    /// Sets the initial batch size for adaptive sizing
    #[must_use]
    pub fn initial_batch_size(mut self, size: usize) -> Self {
        self.initial_batch_size = Some(size);
        self
    }

    /// Sets the batch timeout in milliseconds
    #[must_use]
    pub fn batch_timeout_ms(mut self, ms: u64) -> Self {
        self.batch_timeout_ms = Some(ms);
        self
    }

    /// Sets the critical priority timeout
    #[must_use]
    pub fn critical_timeout_ms(mut self, ms: u64) -> Self {
        self.critical_timeout_ms = Some(ms);
        self
    }

    /// Enables or disables adaptive batch sizing
    #[must_use]
    pub fn enable_adaptive_sizing(mut self, enable: bool) -> Self {
        self.enable_adaptive_sizing = Some(enable);
        self
    }

    /// Enables or disables padding for variable-sized inputs
    #[must_use]
    pub fn enable_padding(mut self, enable: bool) -> Self {
        self.enable_padding = Some(enable);
        self
    }

    /// Sets the padding strategy
    #[must_use]
    pub fn padding_strategy(mut self, strategy: PaddingStrategy) -> Self {
        self.padding_strategy = Some(strategy);
        self
    }

    /// Sets the target latency for adaptive sizing
    #[must_use]
    pub fn target_latency_ms(mut self, ms: u64) -> Self {
        self.target_latency_ms = Some(ms);
        self
    }

    /// Sets the maximum queue length
    #[must_use]
    pub fn max_queue_length(mut self, length: usize) -> Self {
        self.max_queue_length = Some(length);
        self
    }

    /// Sets the number of parallel workers
    #[must_use]
    pub fn num_workers(mut self, workers: usize) -> Self {
        self.num_workers = Some(workers);
        self
    }

    /// Enables or disables request coalescing
    #[must_use]
    pub fn enable_coalescing(mut self, enable: bool) -> Self {
        self.enable_coalescing = Some(enable);
        self
    }

    /// Sets the memory limit for batch formation
    #[must_use]
    pub fn memory_limit_bytes(mut self, bytes: usize) -> Self {
        self.memory_limit_bytes = Some(bytes);
        self
    }

    /// Builds the configuration
    #[must_use]
    pub fn build(self) -> DynamicBatchConfig {
        let defaults = DynamicBatchConfig::default();
        DynamicBatchConfig {
            max_batch_size: self.max_batch_size.unwrap_or(defaults.max_batch_size),
            min_batch_size: self.min_batch_size.unwrap_or(defaults.min_batch_size),
            initial_batch_size: self
                .initial_batch_size
                .unwrap_or(defaults.initial_batch_size),
            batch_timeout_ms: self.batch_timeout_ms.unwrap_or(defaults.batch_timeout_ms),
            critical_timeout_ms: self
                .critical_timeout_ms
                .unwrap_or(defaults.critical_timeout_ms),
            enable_adaptive_sizing: self
                .enable_adaptive_sizing
                .unwrap_or(defaults.enable_adaptive_sizing),
            enable_padding: self.enable_padding.unwrap_or(defaults.enable_padding),
            padding_strategy: self.padding_strategy.unwrap_or(defaults.padding_strategy),
            target_latency_ms: self.target_latency_ms.unwrap_or(defaults.target_latency_ms),
            max_queue_length: self.max_queue_length.unwrap_or(defaults.max_queue_length),
            num_workers: self.num_workers.unwrap_or(defaults.num_workers),
            enable_coalescing: self.enable_coalescing.unwrap_or(defaults.enable_coalescing),
            memory_limit_bytes: self.memory_limit_bytes.or(defaults.memory_limit_bytes),
        }
    }
}

/// Strategy for padding variable-sized inputs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaddingStrategy {
    /// Pad with zeros
    #[default]
    Zero,
    /// Pad by reflecting the edge pixels
    Reflect,
    /// Pad by replicating the edge pixels
    Replicate,
    /// Pad with a constant value
    Constant(i32),
    /// No padding - reject mismatched sizes
    None,
}

/// A prioritized batch request
struct PrioritizedRequest {
    /// Unique request ID for tracking
    id: u64,
    /// The input raster buffer
    input: RasterBuffer,
    /// Original dimensions before padding
    original_dims: (u64, u64),
    /// Request priority level
    priority: PriorityLevel,
    /// Submission timestamp
    submitted_at: Instant,
    /// Estimated memory footprint in bytes
    memory_bytes: usize,
}

impl PrioritizedRequest {
    fn new(id: u64, input: RasterBuffer, priority: PriorityLevel) -> Self {
        let original_dims = (input.width(), input.height());
        let memory_bytes = input.as_bytes().len();
        Self {
            id,
            input,
            original_dims,
            priority,
            submitted_at: Instant::now(),
            memory_bytes,
        }
    }

    fn age(&self) -> Duration {
        self.submitted_at.elapsed()
    }

    fn age_ms(&self) -> u64 {
        self.submitted_at.elapsed().as_millis() as u64
    }
}

impl PartialEq for PrioritizedRequest {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for PrioritizedRequest {}

impl PartialOrd for PrioritizedRequest {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritizedRequest {
    fn cmp(&self, other: &Self) -> Ordering {
        // First compare by priority (higher priority first)
        match self.priority.cmp(&other.priority) {
            Ordering::Equal => {
                // Within same priority, older requests come first (FIFO within priority)
                // Reverse because BinaryHeap is max-heap
                other.submitted_at.cmp(&self.submitted_at)
            }
            ord => ord,
        }
    }
}

/// Statistics for adaptive batch sizing
#[derive(Debug, Default)]
struct AdaptiveStats {
    /// Total requests processed
    total_requests: AtomicU64,
    /// Total batches processed
    total_batches: AtomicU64,
    /// Cumulative latency in microseconds
    cumulative_latency_us: AtomicU64,
    /// Current adaptive batch size
    current_batch_size: AtomicUsize,
    /// Number of timeouts (batch formed due to timeout)
    timeout_count: AtomicU64,
    /// Number of full batches (batch formed due to size)
    full_batch_count: AtomicU64,
    /// Number of requests rejected due to queue full
    rejected_count: AtomicU64,
    /// Last adjustment time
    last_adjustment: Mutex<Option<Instant>>,
}

impl AdaptiveStats {
    fn new(initial_batch_size: usize) -> Self {
        Self {
            current_batch_size: AtomicUsize::new(initial_batch_size),
            ..Default::default()
        }
    }

    fn record_batch(&self, batch_size: usize, latency_us: u64, was_timeout: bool) {
        self.total_requests
            .fetch_add(batch_size as u64, AtomicOrdering::Relaxed);
        self.total_batches.fetch_add(1, AtomicOrdering::Relaxed);
        self.cumulative_latency_us
            .fetch_add(latency_us, AtomicOrdering::Relaxed);

        if was_timeout {
            self.timeout_count.fetch_add(1, AtomicOrdering::Relaxed);
        } else {
            self.full_batch_count.fetch_add(1, AtomicOrdering::Relaxed);
        }
    }

    fn avg_latency_us(&self) -> f64 {
        let batches = self.total_batches.load(AtomicOrdering::Relaxed);
        if batches == 0 {
            return 0.0;
        }
        let total = self.cumulative_latency_us.load(AtomicOrdering::Relaxed);
        total as f64 / batches as f64
    }

    fn timeout_ratio(&self) -> f64 {
        let total = self.total_batches.load(AtomicOrdering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        let timeouts = self.timeout_count.load(AtomicOrdering::Relaxed);
        timeouts as f64 / total as f64
    }
}

/// Dynamic batch processor with priority queuing and adaptive sizing
pub struct DynamicBatchProcessor<M: Model> {
    /// The underlying model
    model: Arc<RwLock<M>>,
    /// Configuration
    config: DynamicBatchConfig,
    /// Priority queue for pending requests
    queue: Arc<Mutex<BinaryHeap<PrioritizedRequest>>>,
    /// Condition variable for signaling batch ready
    batch_ready: Arc<Condvar>,
    /// Next request ID counter
    next_id: AtomicU64,
    /// Adaptive statistics
    stats: Arc<AdaptiveStats>,
    /// Current memory usage in the queue
    queue_memory: AtomicUsize,
    /// Shutdown flag
    shutdown: Arc<std::sync::atomic::AtomicBool>,
}

impl<M: Model + 'static> DynamicBatchProcessor<M> {
    /// Creates a new dynamic batch processor
    #[must_use]
    pub fn new(model: M, config: DynamicBatchConfig) -> Self {
        info!(
            "Creating dynamic batch processor: max_batch={}, min_batch={}, timeout={}ms, adaptive={}",
            config.max_batch_size,
            config.min_batch_size,
            config.batch_timeout_ms,
            config.enable_adaptive_sizing
        );

        Self {
            model: Arc::new(RwLock::new(model)),
            stats: Arc::new(AdaptiveStats::new(config.initial_batch_size)),
            config,
            queue: Arc::new(Mutex::new(BinaryHeap::new())),
            batch_ready: Arc::new(Condvar::new()),
            next_id: AtomicU64::new(0),
            queue_memory: AtomicUsize::new(0),
            shutdown: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Submits a request with normal priority
    ///
    /// # Errors
    /// Returns an error if the queue is full or inference fails
    pub fn submit(&self, input: RasterBuffer) -> Result<RasterBuffer> {
        self.submit_with_priority(input, PriorityLevel::Normal)
    }

    /// Submits a request with the specified priority
    ///
    /// # Errors
    /// Returns an error if the queue is full or inference fails
    pub fn submit_with_priority(
        &self,
        input: RasterBuffer,
        priority: PriorityLevel,
    ) -> Result<RasterBuffer> {
        // Check queue length
        let queue_len = self
            .queue
            .lock()
            .map_err(|e| MlError::InvalidConfig(format!("Failed to lock queue: {}", e)))?
            .len();

        if queue_len >= self.config.max_queue_length {
            self.stats
                .rejected_count
                .fetch_add(1, AtomicOrdering::Relaxed);
            return Err(MlError::Inference(InferenceError::Failed {
                reason: format!(
                    "Queue is full ({} requests pending)",
                    self.config.max_queue_length
                ),
            }));
        }

        // Check memory limit
        if let Some(limit) = self.config.memory_limit_bytes {
            let current = self.queue_memory.load(AtomicOrdering::Relaxed);
            let request_size = input.as_bytes().len();
            if current + request_size > limit {
                return Err(MlError::Inference(InferenceError::Failed {
                    reason: format!(
                        "Memory limit exceeded: {} + {} > {} bytes",
                        current, request_size, limit
                    ),
                }));
            }
        }

        // Generate request ID
        let id = self.next_id.fetch_add(1, AtomicOrdering::Relaxed);

        // Create prioritized request
        let request = PrioritizedRequest::new(id, input, priority);
        let memory_bytes = request.memory_bytes;

        // Add to queue
        {
            let mut queue = self
                .queue
                .lock()
                .map_err(|e| MlError::InvalidConfig(format!("Failed to lock queue: {}", e)))?;
            queue.push(request);
            self.queue_memory
                .fetch_add(memory_bytes, AtomicOrdering::Relaxed);
        }

        trace!("Submitted request {} with priority {:?}", id, priority);

        // Check if we should form a batch
        self.try_form_and_process_batch()
    }

    /// Tries to form a batch and process it
    fn try_form_and_process_batch(&self) -> Result<RasterBuffer> {
        let start = Instant::now();

        // Determine effective batch size
        let effective_batch_size = if self.config.enable_adaptive_sizing {
            self.stats
                .current_batch_size
                .load(AtomicOrdering::Relaxed)
                .clamp(self.config.min_batch_size, self.config.max_batch_size)
        } else {
            self.config.max_batch_size
        };

        // Wait for batch conditions to be met
        let (batch, was_timeout, has_critical) = self.wait_for_batch(effective_batch_size)?;

        if batch.is_empty() {
            return Err(MlError::Inference(InferenceError::Failed {
                reason: "No requests to process".to_string(),
            }));
        }

        let batch_size = batch.len();
        debug!(
            "Processing batch of {} requests (was_timeout={}, has_critical={})",
            batch_size, was_timeout, has_critical
        );

        // Pad inputs if needed
        let (padded_inputs, original_dims): (Vec<_>, Vec<_>) = if self.config.enable_padding {
            self.pad_inputs(batch)?
        } else {
            let dims: Vec<_> = batch.iter().map(|r| r.original_dims).collect();
            let inputs: Vec<_> = batch.into_iter().map(|r| r.input).collect();
            (inputs, dims)
        };

        // Run batch inference
        let results = {
            let mut model = self
                .model
                .write()
                .map_err(|e| MlError::InvalidConfig(format!("Failed to lock model: {}", e)))?;
            model.predict_batch(&padded_inputs)?
        };

        // Unpad results if needed
        let final_results = if self.config.enable_padding {
            self.unpad_outputs(results, &original_dims)?
        } else {
            results
        };

        let latency_us = start.elapsed().as_micros() as u64;

        // Update statistics
        self.stats.record_batch(batch_size, latency_us, was_timeout);

        // Adapt batch size if enabled
        if self.config.enable_adaptive_sizing {
            self.adapt_batch_size(latency_us)?;
        }

        // Return the first result (the one corresponding to this request)
        final_results.into_iter().next().ok_or_else(|| {
            MlError::Inference(InferenceError::Failed {
                reason: "No results returned from batch inference".to_string(),
            })
        })
    }

    /// Waits for batch conditions to be met
    fn wait_for_batch(&self, target_size: usize) -> Result<(Vec<PrioritizedRequest>, bool, bool)> {
        let timeout = Duration::from_millis(self.config.batch_timeout_ms);
        let critical_timeout = Duration::from_millis(self.config.critical_timeout_ms);
        let deadline = Instant::now() + timeout;

        loop {
            let mut queue = self
                .queue
                .lock()
                .map_err(|e| MlError::InvalidConfig(format!("Failed to lock queue: {}", e)))?;

            // Check for critical priority requests
            let has_critical = queue
                .peek()
                .map(|r| r.priority == PriorityLevel::Critical)
                .unwrap_or(false);

            // Determine if we should form a batch now
            let should_form = queue.len() >= target_size
                || (has_critical
                    && queue
                        .peek()
                        .map(|r| r.age() >= critical_timeout)
                        .unwrap_or(false))
                || (!queue.is_empty() && Instant::now() >= deadline)
                || queue.len() >= self.config.max_batch_size;

            if should_form {
                let was_timeout = queue.len() < target_size && Instant::now() >= deadline;
                let batch_size = queue.len().min(self.config.max_batch_size);

                // Extract batch from queue
                let mut batch = Vec::with_capacity(batch_size);
                let mut memory_released = 0;

                for _ in 0..batch_size {
                    if let Some(req) = queue.pop() {
                        memory_released += req.memory_bytes;
                        batch.push(req);
                    }
                }

                self.queue_memory
                    .fetch_sub(memory_released, AtomicOrdering::Relaxed);

                return Ok((batch, was_timeout, has_critical));
            }

            // Wait for more requests or timeout
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                // Timeout - take whatever we have
                if queue.is_empty() {
                    return Ok((Vec::new(), true, false));
                }
                let batch_size = queue.len();
                let mut batch = Vec::with_capacity(batch_size);
                let mut memory_released = 0;

                for _ in 0..batch_size {
                    if let Some(req) = queue.pop() {
                        memory_released += req.memory_bytes;
                        batch.push(req);
                    }
                }

                self.queue_memory
                    .fetch_sub(memory_released, AtomicOrdering::Relaxed);

                return Ok((batch, true, false));
            }

            // Release lock and wait (use a short sleep since we're doing synchronous processing)
            drop(queue);
            thread::sleep(Duration::from_millis(1).min(remaining));
        }
    }

    /// Pads inputs to a uniform size
    fn pad_inputs(&self, requests: Vec<PrioritizedRequest>) -> Result<PaddedInputs> {
        if requests.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }

        // Find maximum dimensions
        let max_width = requests.iter().map(|r| r.input.width()).max().unwrap_or(0);
        let max_height = requests.iter().map(|r| r.input.height()).max().unwrap_or(0);

        let mut padded = Vec::with_capacity(requests.len());
        let mut original_dims = Vec::with_capacity(requests.len());

        for req in requests {
            original_dims.push(req.original_dims);

            let input = req.input;
            if input.width() == max_width && input.height() == max_height {
                // No padding needed
                padded.push(input);
            } else {
                // Apply padding
                let padded_input = self.apply_padding(&input, max_width, max_height)?;
                padded.push(padded_input);
            }
        }

        Ok((padded, original_dims))
    }

    /// Applies padding to a single buffer
    fn apply_padding(
        &self,
        input: &RasterBuffer,
        target_width: u64,
        target_height: u64,
    ) -> Result<RasterBuffer> {
        let src_width = input.width();
        let src_height = input.height();
        let data_type = input.data_type();
        let bytes_per_pixel = data_type.size_bytes() as u64;

        // Calculate padded buffer size
        let padded_size = (target_width * target_height * bytes_per_pixel) as usize;
        let mut padded_data = vec![0u8; padded_size];

        // Determine padding value based on strategy
        let pad_value: u8 = match self.config.padding_strategy {
            PaddingStrategy::Zero => 0,
            PaddingStrategy::Constant(v) => v as u8,
            PaddingStrategy::Reflect | PaddingStrategy::Replicate => 0, // Will handle specially
            PaddingStrategy::None => {
                return Err(MlError::Inference(InferenceError::InvalidInputShape {
                    expected: vec![target_height as usize, target_width as usize],
                    actual: vec![src_height as usize, src_width as usize],
                }));
            }
        };

        // Fill with padding value
        if pad_value != 0 {
            padded_data.fill(pad_value);
        }

        // Copy original data
        let src_bytes = input.as_bytes();
        let src_row_bytes = (src_width * bytes_per_pixel) as usize;
        let dst_row_bytes = (target_width * bytes_per_pixel) as usize;

        for y in 0..src_height as usize {
            let src_start = y * src_row_bytes;
            let src_end = src_start + src_row_bytes;
            let dst_start = y * dst_row_bytes;
            let dst_end = dst_start + src_row_bytes;

            if src_end <= src_bytes.len() && dst_end <= padded_data.len() {
                padded_data[dst_start..dst_end].copy_from_slice(&src_bytes[src_start..src_end]);
            }
        }

        // Handle reflect/replicate padding for edges
        match self.config.padding_strategy {
            PaddingStrategy::Reflect => {
                self.apply_reflect_padding(
                    &mut padded_data,
                    src_width,
                    src_height,
                    target_width,
                    target_height,
                    bytes_per_pixel,
                );
            }
            PaddingStrategy::Replicate => {
                self.apply_replicate_padding(
                    &mut padded_data,
                    src_width,
                    src_height,
                    target_width,
                    target_height,
                    bytes_per_pixel,
                );
            }
            _ => {}
        }

        RasterBuffer::new(
            padded_data,
            target_width,
            target_height,
            data_type,
            input.nodata(),
        )
        .map_err(|e| {
            MlError::Inference(InferenceError::Failed {
                reason: format!("Failed to create padded buffer: {}", e),
            })
        })
    }

    /// Applies reflect padding to the edges
    fn apply_reflect_padding(
        &self,
        data: &mut [u8],
        src_width: u64,
        src_height: u64,
        target_width: u64,
        target_height: u64,
        bytes_per_pixel: u64,
    ) {
        let dst_row_bytes = (target_width * bytes_per_pixel) as usize;
        let bpp = bytes_per_pixel as usize;

        // Reflect right edge
        for y in 0..src_height as usize {
            for x in src_width as usize..target_width as usize {
                let reflect_x =
                    src_width as usize - 1 - (x - src_width as usize) % src_width as usize;
                let src_offset = y * dst_row_bytes + reflect_x * bpp;
                let dst_offset = y * dst_row_bytes + x * bpp;
                if src_offset + bpp <= data.len() && dst_offset + bpp <= data.len() {
                    for b in 0..bpp {
                        data[dst_offset + b] = data[src_offset + b];
                    }
                }
            }
        }

        // Reflect bottom edge
        for y in src_height as usize..target_height as usize {
            let reflect_y =
                src_height as usize - 1 - (y - src_height as usize) % src_height as usize;
            let src_row_start = reflect_y * dst_row_bytes;
            let dst_row_start = y * dst_row_bytes;
            if src_row_start + dst_row_bytes <= data.len()
                && dst_row_start + dst_row_bytes <= data.len()
            {
                let (left, right) = data.split_at_mut(dst_row_start);
                if src_row_start + dst_row_bytes <= left.len() {
                    right[..dst_row_bytes]
                        .copy_from_slice(&left[src_row_start..src_row_start + dst_row_bytes]);
                }
            }
        }
    }

    /// Applies replicate padding to the edges
    fn apply_replicate_padding(
        &self,
        data: &mut [u8],
        src_width: u64,
        src_height: u64,
        target_width: u64,
        target_height: u64,
        bytes_per_pixel: u64,
    ) {
        let dst_row_bytes = (target_width * bytes_per_pixel) as usize;
        let bpp = bytes_per_pixel as usize;

        // Replicate right edge
        let edge_x = (src_width - 1) as usize;
        for y in 0..src_height as usize {
            let src_offset = y * dst_row_bytes + edge_x * bpp;
            for x in src_width as usize..target_width as usize {
                let dst_offset = y * dst_row_bytes + x * bpp;
                if src_offset + bpp <= data.len() && dst_offset + bpp <= data.len() {
                    for b in 0..bpp {
                        data[dst_offset + b] = data[src_offset + b];
                    }
                }
            }
        }

        // Replicate bottom edge
        let edge_y = (src_height - 1) as usize;
        let edge_row_start = edge_y * dst_row_bytes;
        for y in src_height as usize..target_height as usize {
            let dst_row_start = y * dst_row_bytes;
            if edge_row_start + dst_row_bytes <= data.len()
                && dst_row_start + dst_row_bytes <= data.len()
            {
                let (left, right) = data.split_at_mut(dst_row_start);
                if edge_row_start + dst_row_bytes <= left.len() {
                    right[..dst_row_bytes]
                        .copy_from_slice(&left[edge_row_start..edge_row_start + dst_row_bytes]);
                }
            }
        }
    }

    /// Unpads outputs to original dimensions
    fn unpad_outputs(
        &self,
        outputs: Vec<RasterBuffer>,
        original_dims: &[(u64, u64)],
    ) -> Result<Vec<RasterBuffer>> {
        if outputs.len() != original_dims.len() {
            return Err(MlError::Inference(InferenceError::BatchSizeMismatch {
                expected: original_dims.len(),
                actual: outputs.len(),
            }));
        }

        let mut unpadded = Vec::with_capacity(outputs.len());

        for (output, &(orig_width, orig_height)) in outputs.into_iter().zip(original_dims.iter()) {
            if output.width() == orig_width && output.height() == orig_height {
                // No unpadding needed
                unpadded.push(output);
            } else {
                // Crop to original size
                let cropped = self.crop_to_size(&output, orig_width, orig_height)?;
                unpadded.push(cropped);
            }
        }

        Ok(unpadded)
    }

    /// Crops a buffer to the specified size
    fn crop_to_size(
        &self,
        buffer: &RasterBuffer,
        target_width: u64,
        target_height: u64,
    ) -> Result<RasterBuffer> {
        let data_type = buffer.data_type();
        let bytes_per_pixel = data_type.size_bytes() as u64;

        let src_row_bytes = (buffer.width() * bytes_per_pixel) as usize;
        let dst_row_bytes = (target_width * bytes_per_pixel) as usize;
        let cropped_size = (target_width * target_height * bytes_per_pixel) as usize;

        let mut cropped_data = vec![0u8; cropped_size];
        let src_bytes = buffer.as_bytes();

        for y in 0..target_height as usize {
            let src_start = y * src_row_bytes;
            let src_end = src_start + dst_row_bytes;
            let dst_start = y * dst_row_bytes;
            let dst_end = dst_start + dst_row_bytes;

            if src_end <= src_bytes.len() && dst_end <= cropped_data.len() {
                cropped_data[dst_start..dst_end].copy_from_slice(&src_bytes[src_start..src_end]);
            }
        }

        RasterBuffer::new(
            cropped_data,
            target_width,
            target_height,
            data_type,
            buffer.nodata(),
        )
        .map_err(|e| {
            MlError::Inference(InferenceError::Failed {
                reason: format!("Failed to create cropped buffer: {}", e),
            })
        })
    }

    /// Adapts the batch size based on observed latency
    fn adapt_batch_size(&self, latency_us: u64) -> Result<()> {
        // Only adjust periodically
        let should_adjust = {
            let mut last = self
                .stats
                .last_adjustment
                .lock()
                .map_err(|e| MlError::InvalidConfig(format!("Failed to lock stats: {}", e)))?;

            let now = Instant::now();
            if last
                .map(|t| now.duration_since(t) < Duration::from_secs(1))
                .unwrap_or(false)
            {
                return Ok(());
            }
            *last = Some(now);
            true
        };

        if !should_adjust {
            return Ok(());
        }

        let target_us = self.config.target_latency_ms * 1000;
        let current_size = self.stats.current_batch_size.load(AtomicOrdering::Relaxed);
        let timeout_ratio = self.stats.timeout_ratio();

        let new_size = if latency_us > target_us * 2 {
            // Latency too high - reduce batch size significantly
            (current_size * 3 / 4).max(self.config.min_batch_size)
        } else if latency_us > target_us {
            // Latency above target - reduce slightly
            (current_size - 1).max(self.config.min_batch_size)
        } else if timeout_ratio > 0.7 {
            // Too many timeouts - reduce batch size to form batches faster
            (current_size * 3 / 4).max(self.config.min_batch_size)
        } else if latency_us < target_us / 2 && timeout_ratio < 0.3 {
            // Latency well below target and good batch utilization - increase
            (current_size + 1).min(self.config.max_batch_size)
        } else {
            current_size
        };

        if new_size != current_size {
            debug!(
                "Adapting batch size: {} -> {} (latency={}us, target={}us, timeout_ratio={:.2})",
                current_size, new_size, latency_us, target_us, timeout_ratio
            );
            self.stats
                .current_batch_size
                .store(new_size, AtomicOrdering::Relaxed);
        }

        Ok(())
    }

    /// Submits multiple requests as a batch
    ///
    /// # Errors
    /// Returns an error if the queue is full or inference fails
    pub fn submit_batch(&self, inputs: Vec<RasterBuffer>) -> Result<Vec<RasterBuffer>> {
        self.submit_batch_with_priority(inputs, PriorityLevel::Normal)
    }

    /// Submits multiple requests with the specified priority
    ///
    /// # Errors
    /// Returns an error if the queue is full or inference fails
    pub fn submit_batch_with_priority(
        &self,
        inputs: Vec<RasterBuffer>,
        priority: PriorityLevel,
    ) -> Result<Vec<RasterBuffer>> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        let batch_size = inputs.len();
        debug!(
            "Submitting batch of {} inputs with priority {:?}",
            batch_size, priority
        );

        // For batch submissions, we can process directly without queuing
        // if the batch is already the right size
        if batch_size >= self.config.min_batch_size {
            let start = Instant::now();

            // Pad if needed
            let (padded_inputs, original_dims) = if self.config.enable_padding {
                self.pad_batch_inputs(inputs)?
            } else {
                let dims: Vec<_> = inputs.iter().map(|i| (i.width(), i.height())).collect();
                (inputs, dims)
            };

            // Run inference
            let results = {
                let mut model = self
                    .model
                    .write()
                    .map_err(|e| MlError::InvalidConfig(format!("Failed to lock model: {}", e)))?;
                model.predict_batch(&padded_inputs)?
            };

            // Unpad if needed
            let final_results = if self.config.enable_padding {
                self.unpad_outputs(results, &original_dims)?
            } else {
                results
            };

            let latency_us = start.elapsed().as_micros() as u64;
            self.stats.record_batch(batch_size, latency_us, false);

            return Ok(final_results);
        }

        // For small batches, submit individually and collect results
        let mut results = Vec::with_capacity(batch_size);
        for input in inputs {
            let result = self.submit_with_priority(input, priority)?;
            results.push(result);
        }

        Ok(results)
    }

    /// Pads a batch of inputs
    fn pad_batch_inputs(&self, inputs: Vec<RasterBuffer>) -> Result<PaddedInputs> {
        if inputs.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }

        // Find maximum dimensions
        let max_width = inputs.iter().map(|i| i.width()).max().unwrap_or(0);
        let max_height = inputs.iter().map(|i| i.height()).max().unwrap_or(0);

        let mut padded = Vec::with_capacity(inputs.len());
        let mut original_dims = Vec::with_capacity(inputs.len());

        for input in inputs {
            let dims = (input.width(), input.height());
            original_dims.push(dims);

            if input.width() == max_width && input.height() == max_height {
                padded.push(input);
            } else {
                let padded_input = self.apply_padding(&input, max_width, max_height)?;
                padded.push(padded_input);
            }
        }

        Ok((padded, original_dims))
    }

    /// Returns the current queue length
    #[must_use]
    pub fn queue_length(&self) -> usize {
        self.queue.lock().map(|q| q.len()).unwrap_or(0)
    }

    /// Returns the current adaptive batch size
    #[must_use]
    pub fn current_batch_size(&self) -> usize {
        self.stats.current_batch_size.load(AtomicOrdering::Relaxed)
    }

    /// Returns the current statistics
    #[must_use]
    pub fn statistics(&self) -> DynamicBatchStats {
        DynamicBatchStats {
            total_requests: self.stats.total_requests.load(AtomicOrdering::Relaxed),
            total_batches: self.stats.total_batches.load(AtomicOrdering::Relaxed),
            avg_latency_us: self.stats.avg_latency_us(),
            timeout_ratio: self.stats.timeout_ratio(),
            current_batch_size: self.stats.current_batch_size.load(AtomicOrdering::Relaxed),
            rejected_count: self.stats.rejected_count.load(AtomicOrdering::Relaxed),
            queue_length: self.queue_length(),
            queue_memory_bytes: self.queue_memory.load(AtomicOrdering::Relaxed),
        }
    }

    /// Clears all pending requests from the queue
    pub fn clear_queue(&self) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.clear();
            self.queue_memory.store(0, AtomicOrdering::Relaxed);
        }
    }

    /// Shuts down the processor
    pub fn shutdown(&self) {
        self.shutdown.store(true, AtomicOrdering::Relaxed);
        self.batch_ready.notify_all();
    }
}

/// Statistics for dynamic batch processing
#[derive(Debug, Clone)]
pub struct DynamicBatchStats {
    /// Total requests processed
    pub total_requests: u64,
    /// Total batches processed
    pub total_batches: u64,
    /// Average latency in microseconds
    pub avg_latency_us: f64,
    /// Ratio of batches formed due to timeout
    pub timeout_ratio: f64,
    /// Current adaptive batch size
    pub current_batch_size: usize,
    /// Number of rejected requests
    pub rejected_count: u64,
    /// Current queue length
    pub queue_length: usize,
    /// Current queue memory usage in bytes
    pub queue_memory_bytes: usize,
}

impl DynamicBatchStats {
    /// Returns the throughput in requests per second
    #[must_use]
    pub fn throughput(&self) -> f64 {
        if self.avg_latency_us > 0.0 && self.total_batches > 0 {
            let avg_batch_size = self.total_requests as f64 / self.total_batches as f64;
            (avg_batch_size * 1_000_000.0) / self.avg_latency_us
        } else {
            0.0
        }
    }

    /// Returns the average batch size
    #[must_use]
    pub fn avg_batch_size(&self) -> f64 {
        if self.total_batches > 0 {
            self.total_requests as f64 / self.total_batches as f64
        } else {
            0.0
        }
    }

    /// Returns the average latency in milliseconds
    #[must_use]
    pub fn avg_latency_ms(&self) -> f64 {
        self.avg_latency_us / 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::buffer::RasterBuffer;
    use oxigdal_core::types::RasterDataType;
    use std::collections::BinaryHeap;
    use std::sync::atomic::Ordering as AtomicOrdering;
    use std::time::Duration;

    #[test]
    fn test_prioritized_request_ordering() {
        let buf1 = RasterBuffer::zeros(64, 64, RasterDataType::Float32);
        let buf2 = RasterBuffer::zeros(64, 64, RasterDataType::Float32);
        let buf3 = RasterBuffer::zeros(64, 64, RasterDataType::Float32);

        let req1 = PrioritizedRequest::new(1, buf1, PriorityLevel::Low);
        std::thread::sleep(Duration::from_millis(1));
        let req2 = PrioritizedRequest::new(2, buf2, PriorityLevel::High);
        std::thread::sleep(Duration::from_millis(1));
        let req3 = PrioritizedRequest::new(3, buf3, PriorityLevel::Normal);

        // Higher priority should come first
        assert!(req2 > req1);
        assert!(req2 > req3);
        assert!(req3 > req1);
    }

    #[test]
    fn test_prioritized_request_same_priority_fifo() {
        let buf1 = RasterBuffer::zeros(64, 64, RasterDataType::Float32);
        let req1 = PrioritizedRequest::new(1, buf1, PriorityLevel::Normal);

        std::thread::sleep(Duration::from_millis(10));

        let buf2 = RasterBuffer::zeros(64, 64, RasterDataType::Float32);
        let req2 = PrioritizedRequest::new(2, buf2, PriorityLevel::Normal);

        // Earlier request should come first (have higher priority in max-heap)
        assert!(req1 > req2);
    }

    #[test]
    fn test_prioritized_request_memory_tracking() {
        let buf = RasterBuffer::zeros(256, 256, RasterDataType::Float32);
        let expected_size = 256 * 256 * 4; // 4 bytes per f32

        let req = PrioritizedRequest::new(1, buf, PriorityLevel::Normal);

        assert_eq!(req.memory_bytes, expected_size);
        assert_eq!(req.original_dims, (256, 256));
    }

    #[test]
    fn test_prioritized_request_age() {
        let buf = RasterBuffer::zeros(64, 64, RasterDataType::Float32);
        let req = PrioritizedRequest::new(1, buf, PriorityLevel::Normal);

        std::thread::sleep(Duration::from_millis(10));

        let age = req.age();
        assert!(age >= Duration::from_millis(10));
        assert!(req.age_ms() >= 10);
    }

    #[test]
    fn test_adaptive_stats() {
        let stats = AdaptiveStats::new(8);

        // Initial state
        assert_eq!(stats.current_batch_size.load(AtomicOrdering::Relaxed), 8);
        assert!((stats.avg_latency_us() - 0.0).abs() < 1e-6);
        assert!((stats.timeout_ratio() - 0.0).abs() < 1e-6);

        // Record some batches
        stats.record_batch(4, 10000, false);
        stats.record_batch(8, 20000, true);
        stats.record_batch(6, 15000, false);

        assert_eq!(stats.total_requests.load(AtomicOrdering::Relaxed), 18);
        assert_eq!(stats.total_batches.load(AtomicOrdering::Relaxed), 3);
        assert_eq!(stats.timeout_count.load(AtomicOrdering::Relaxed), 1);
        assert_eq!(stats.full_batch_count.load(AtomicOrdering::Relaxed), 2);

        // Average latency: (10000 + 20000 + 15000) / 3 = 15000 us
        let avg = stats.avg_latency_us();
        assert!((avg - 15000.0).abs() < 1e-6);

        // Timeout ratio: 1/3
        let ratio = stats.timeout_ratio();
        assert!((ratio - 0.333333).abs() < 0.01);
    }

    #[test]
    fn test_binary_heap_priority_ordering() {
        let mut heap = BinaryHeap::new();

        // Add requests with different priorities
        heap.push(PrioritizedRequest::new(
            1,
            RasterBuffer::zeros(64, 64, RasterDataType::Float32),
            PriorityLevel::Low,
        ));
        heap.push(PrioritizedRequest::new(
            2,
            RasterBuffer::zeros(64, 64, RasterDataType::Float32),
            PriorityLevel::Critical,
        ));
        heap.push(PrioritizedRequest::new(
            3,
            RasterBuffer::zeros(64, 64, RasterDataType::Float32),
            PriorityLevel::Normal,
        ));
        heap.push(PrioritizedRequest::new(
            4,
            RasterBuffer::zeros(64, 64, RasterDataType::Float32),
            PriorityLevel::High,
        ));

        // Pop should return in priority order
        assert_eq!(heap.pop().map(|r| r.id), Some(2)); // Critical
        assert_eq!(heap.pop().map(|r| r.id), Some(4)); // High
        assert_eq!(heap.pop().map(|r| r.id), Some(3)); // Normal
        assert_eq!(heap.pop().map(|r| r.id), Some(1)); // Low
    }
}
