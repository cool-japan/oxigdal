//! Advanced batch inference for efficient model serving
//!
//! This module provides sophisticated batch processing capabilities including
//! dynamic batching, batch scheduling, memory pooling, and parallel execution.
//!
//! # Dynamic Batching
//!
//! Dynamic batching optimizes model inference by grouping requests together:
//! - **Priority Queuing**: Requests can have different priority levels (Critical, High, Normal, Low)
//! - **Adaptive Batch Size**: Batch size adjusts based on request patterns and system load
//! - **Variable Input Handling**: Inputs of different sizes are padded to batch together
//! - **Latency-Throughput Balance**: Configurable timeout to balance latency vs throughput
//!
//! # Example
//!
//! ```ignore
//! use oxigdal_ml::batch::{DynamicBatchConfig, DynamicBatchProcessor, PriorityLevel};
//!
//! let config = DynamicBatchConfig::builder()
//!     .max_batch_size(32)
//!     .min_batch_size(4)
//!     .batch_timeout_ms(100)
//!     .enable_adaptive_sizing(true)
//!     .build();
//!
//! let processor = DynamicBatchProcessor::new(model, config);
//!
//! // Submit a high-priority request
//! let result = processor.submit(input, PriorityLevel::High)?;
//! ```

mod dynamic;
#[cfg(test)]
mod tests;

pub use dynamic::{
    DynamicBatchConfig, DynamicBatchConfigBuilder, DynamicBatchProcessor, DynamicBatchStats,
    PaddingStrategy, PriorityLevel,
};

use crate::error::{InferenceError, MlError, Result};
use crate::models::Model;
use indicatif::{ProgressBar, ProgressStyle};
use oxigdal_core::buffer::RasterBuffer;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use sysinfo::System;
use tracing::{debug, info, warn};

/// Batch inference configuration
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// Maximum batch size
    pub max_batch_size: usize,
    /// Timeout for batch formation (milliseconds)
    pub batch_timeout_ms: u64,
    /// Enable dynamic batching
    pub dynamic_batching: bool,
    /// Number of parallel batches
    pub parallel_batches: usize,
    /// Enable memory pooling
    pub memory_pooling: bool,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 32,
            batch_timeout_ms: 100,
            dynamic_batching: true,
            parallel_batches: 4,
            memory_pooling: true,
        }
    }
}

impl BatchConfig {
    /// Creates a new batch configuration builder
    #[must_use]
    pub fn builder() -> BatchConfigBuilder {
        BatchConfigBuilder::default()
    }

    /// Auto-tunes batch size based on available memory
    ///
    /// # Arguments
    /// * `sample_size_bytes` - Estimated memory per sample in bytes
    /// * `memory_fraction` - Fraction of available memory to use (0.0-1.0)
    ///
    /// # Returns
    /// Recommended batch size based on system memory
    #[must_use]
    pub fn auto_tune_batch_size(sample_size_bytes: usize, memory_fraction: f32) -> usize {
        let memory_fraction = memory_fraction.clamp(0.1, 0.9);

        let mut system = System::new_all();
        system.refresh_all();

        let available_memory = system.available_memory() as usize;
        let usable_memory = (available_memory as f32 * memory_fraction) as usize;

        let batch_size = if sample_size_bytes > 0 {
            (usable_memory / sample_size_bytes).clamp(1, 256)
        } else {
            32
        };

        info!(
            "Auto-tuned batch size: {} (available memory: {} MB, sample size: {} MB)",
            batch_size,
            available_memory / (1024 * 1024),
            sample_size_bytes / (1024 * 1024)
        );

        batch_size
    }
}

/// Builder for batch configuration
#[derive(Debug, Default)]
pub struct BatchConfigBuilder {
    max_batch_size: Option<usize>,
    batch_timeout_ms: Option<u64>,
    dynamic_batching: Option<bool>,
    parallel_batches: Option<usize>,
    memory_pooling: Option<bool>,
}

impl BatchConfigBuilder {
    /// Sets the maximum batch size
    #[must_use]
    pub fn max_batch_size(mut self, size: usize) -> Self {
        self.max_batch_size = Some(size);
        self
    }

    /// Sets the batch timeout
    #[must_use]
    pub fn batch_timeout_ms(mut self, ms: u64) -> Self {
        self.batch_timeout_ms = Some(ms);
        self
    }

    /// Enables dynamic batching
    #[must_use]
    pub fn dynamic_batching(mut self, enable: bool) -> Self {
        self.dynamic_batching = Some(enable);
        self
    }

    /// Sets the number of parallel batches
    #[must_use]
    pub fn parallel_batches(mut self, count: usize) -> Self {
        self.parallel_batches = Some(count);
        self
    }

    /// Enables memory pooling
    #[must_use]
    pub fn memory_pooling(mut self, enable: bool) -> Self {
        self.memory_pooling = Some(enable);
        self
    }

    /// Builds the configuration
    #[must_use]
    pub fn build(self) -> BatchConfig {
        BatchConfig {
            max_batch_size: self.max_batch_size.unwrap_or(32),
            batch_timeout_ms: self.batch_timeout_ms.unwrap_or(100),
            dynamic_batching: self.dynamic_batching.unwrap_or(true),
            parallel_batches: self.parallel_batches.unwrap_or(4),
            memory_pooling: self.memory_pooling.unwrap_or(true),
        }
    }
}

/// Batch processor for efficient model serving
pub struct BatchProcessor<M: Model> {
    model: Arc<Mutex<M>>,
    config: BatchConfig,
    queue: Arc<Mutex<VecDeque<BatchRequest>>>,
    stats: Arc<Mutex<BatchStats>>,
}

/// A single batch request
struct BatchRequest {
    input: RasterBuffer,
    timestamp: Instant,
}

impl BatchRequest {
    fn new(input: RasterBuffer) -> Self {
        Self {
            input,
            timestamp: Instant::now(),
        }
    }

    fn age(&self) -> Duration {
        self.timestamp.elapsed()
    }
}

impl<M: Model> BatchProcessor<M> {
    /// Creates a new batch processor
    #[must_use]
    pub fn new(model: M, config: BatchConfig) -> Self {
        info!(
            "Creating batch processor with max_batch_size={}, timeout={}ms",
            config.max_batch_size, config.batch_timeout_ms
        );

        Self {
            model: Arc::new(Mutex::new(model)),
            config,
            queue: Arc::new(Mutex::new(VecDeque::new())),
            stats: Arc::new(Mutex::new(BatchStats::default())),
        }
    }

    /// Submits a request for batch inference
    ///
    /// When dynamic batching is enabled, requests are collected and processed
    /// together to improve throughput. For single requests, use `DynamicBatchProcessor`
    /// for more advanced batching with priority queuing and adaptive sizing.
    ///
    /// # Errors
    /// Returns an error if inference fails
    pub fn infer(&self, input: RasterBuffer) -> Result<RasterBuffer> {
        // Capture start time before creating request for statistics
        let start_time = Instant::now();
        let request = BatchRequest::new(input);

        // For synchronous single-request processing, run immediately
        // For true dynamic batching with priority queuing and adaptive sizing,
        // use DynamicBatchProcessor instead
        let result = if self.config.dynamic_batching {
            // Add to queue and check if we should form a batch
            let mut queue = self
                .queue
                .lock()
                .map_err(|e| MlError::InvalidConfig(format!("Failed to lock queue: {}", e)))?;
            queue.push_back(request);

            // Check if we should form a batch now
            let timeout = Duration::from_millis(self.config.batch_timeout_ms);
            let should_batch = queue.len() >= self.config.max_batch_size
                || queue.front().map(|r| r.age() >= timeout).unwrap_or(false);

            if should_batch {
                // Form and process batch
                let batch_size = queue.len().min(self.config.max_batch_size);
                let batch: Vec<_> = queue.drain(..batch_size).map(|r| r.input).collect();
                drop(queue); // Release lock before inference

                let results = {
                    let mut model = self.model.lock().map_err(|e| {
                        MlError::InvalidConfig(format!("Failed to lock model: {}", e))
                    })?;
                    model.predict_batch(&batch)?
                };

                // Return the first result (our request)
                results.into_iter().next().ok_or_else(|| {
                    MlError::Inference(InferenceError::Failed {
                        reason: "No results returned from batch".to_string(),
                    })
                })?
            } else {
                // Not enough requests yet - process just this one
                let our_request = queue.pop_back().ok_or_else(|| {
                    MlError::Inference(InferenceError::Failed {
                        reason: "Request disappeared from queue".to_string(),
                    })
                })?;
                drop(queue); // Release lock before inference

                let mut model = self
                    .model
                    .lock()
                    .map_err(|e| MlError::InvalidConfig(format!("Failed to lock model: {}", e)))?;
                model.predict(&our_request.input)?
            }
        } else {
            // Dynamic batching disabled - process immediately
            let mut model = self
                .model
                .lock()
                .map_err(|e| MlError::InvalidConfig(format!("Failed to lock model: {}", e)))?;
            model.predict(&request.input)?
        };

        // Update statistics
        if let Ok(mut stats) = self.stats.lock() {
            stats.total_requests += 1;
            stats.total_latency_ms += start_time.elapsed().as_millis() as u64;
        }

        Ok(result)
    }

    /// Processes a batch of inputs
    ///
    /// # Errors
    /// Returns an error if inference fails
    pub fn infer_batch(&self, inputs: Vec<RasterBuffer>) -> Result<Vec<RasterBuffer>> {
        self.infer_batch_with_progress(inputs, false)
    }

    /// Processes a batch of inputs with optional progress tracking
    ///
    /// # Arguments
    /// * `inputs` - Input raster buffers to process
    /// * `show_progress` - Whether to display a progress bar
    ///
    /// # Errors
    /// Returns an error if inference fails
    pub fn infer_batch_with_progress(
        &self,
        inputs: Vec<RasterBuffer>,
        show_progress: bool,
    ) -> Result<Vec<RasterBuffer>> {
        let batch_size = inputs.len();
        debug!("Processing batch of size {}", batch_size);

        let start = Instant::now();

        // Create progress bar if requested
        let progress = if show_progress {
            let pb = ProgressBar::new(batch_size as u64);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template(
                        "[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} ({per_sec}) {msg}",
                    )
                    .map_err(|e| crate::error::MlError::InvalidConfig(e.to_string()))?,
            );
            Some(pb)
        } else {
            None
        };

        // Use parallel processing if configured
        let results = if self.config.parallel_batches > 1 && batch_size > 1 {
            self.parallel_batch_inference_with_progress(inputs, progress.as_ref())?
        } else {
            let mut model = self.model.lock().map_err(|e| {
                crate::error::MlError::InvalidConfig(format!("Failed to lock model: {}", e))
            })?;
            model.predict_batch(&inputs)?
        };

        if let Some(pb) = progress {
            pb.finish_with_message("Batch inference complete");
        }

        // Update statistics
        if let Ok(mut stats) = self.stats.lock() {
            stats.total_requests += batch_size;
            stats.total_batches += 1;
            stats.total_latency_ms += start.elapsed().as_millis() as u64;

            if batch_size > stats.max_batch_size {
                stats.max_batch_size = batch_size;
            }
        }

        Ok(results)
    }

    /// Performs parallel batch inference with progress tracking
    fn parallel_batch_inference_with_progress(
        &self,
        inputs: Vec<RasterBuffer>,
        progress: Option<&ProgressBar>,
    ) -> Result<Vec<RasterBuffer>> {
        use rayon::prelude::*;

        let chunk_size =
            (inputs.len() + self.config.parallel_batches - 1) / self.config.parallel_batches;

        debug!(
            "Splitting batch into {} chunks of ~{} items",
            self.config.parallel_batches, chunk_size
        );

        let results: Result<Vec<_>> = inputs
            .par_chunks(chunk_size)
            .map(|chunk| {
                let chunk_results: Result<Vec<_>> = chunk
                    .iter()
                    .map(|input| {
                        let result = {
                            let mut model = self.model.lock().map_err(|e| {
                                crate::error::MlError::InvalidConfig(format!(
                                    "Failed to lock model: {}",
                                    e
                                ))
                            })?;
                            model.predict(input)
                        };
                        if let Some(pb) = progress {
                            pb.inc(1);
                        }
                        result
                    })
                    .collect();
                chunk_results
            })
            .collect();

        results.map(|chunks| chunks.into_iter().flatten().collect())
    }

    /// Returns the batch statistics
    #[must_use]
    pub fn stats(&self) -> BatchStats {
        self.stats.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Resets the statistics
    pub fn reset_stats(&self) {
        if let Ok(mut stats) = self.stats.lock() {
            *stats = BatchStats::default();
        }
    }
}

/// Batch processing statistics
#[derive(Debug, Clone, Default)]
pub struct BatchStats {
    /// Total number of requests processed
    pub total_requests: usize,
    /// Total number of batches processed
    pub total_batches: usize,
    /// Maximum batch size observed
    pub max_batch_size: usize,
    /// Total latency in milliseconds
    pub total_latency_ms: u64,
}

impl BatchStats {
    /// Returns the average batch size
    #[must_use]
    pub fn avg_batch_size(&self) -> f32 {
        if self.total_batches > 0 {
            self.total_requests as f32 / self.total_batches as f32
        } else {
            0.0
        }
    }

    /// Returns the average latency per request
    #[must_use]
    pub fn avg_latency_ms(&self) -> f32 {
        if self.total_requests > 0 {
            self.total_latency_ms as f32 / self.total_requests as f32
        } else {
            0.0
        }
    }

    /// Returns the throughput (requests per second)
    #[must_use]
    pub fn throughput(&self) -> f32 {
        if self.total_latency_ms > 0 {
            (self.total_requests as f32 * 1000.0) / self.total_latency_ms as f32
        } else {
            0.0
        }
    }
}

/// Dynamic batch scheduler
pub struct BatchScheduler {
    config: BatchConfig,
    pending: VecDeque<BatchRequest>,
    last_batch: Instant,
}

impl BatchScheduler {
    /// Creates a new batch scheduler
    #[must_use]
    pub fn new(config: BatchConfig) -> Self {
        Self {
            config,
            pending: VecDeque::new(),
            last_batch: Instant::now(),
        }
    }

    /// Adds a request to the pending queue
    pub fn add_request(&mut self, input: RasterBuffer) {
        self.pending.push_back(BatchRequest::new(input));
    }

    /// Checks if a batch should be formed
    #[must_use]
    pub fn should_form_batch(&self) -> bool {
        // Form batch if max size reached
        if self.pending.len() >= self.config.max_batch_size {
            return true;
        }

        // Form batch if timeout elapsed and queue not empty
        if !self.pending.is_empty() {
            let timeout = Duration::from_millis(self.config.batch_timeout_ms);
            if self.last_batch.elapsed() >= timeout {
                return true;
            }
        }

        false
    }

    /// Forms a batch from pending requests
    #[must_use]
    pub fn form_batch(&mut self) -> Vec<RasterBuffer> {
        let batch_size = self.pending.len().min(self.config.max_batch_size);
        let batch: Vec<_> = self
            .pending
            .drain(..batch_size)
            .map(|req| {
                let age = req.age();
                if age.as_millis() > 500 {
                    warn!("Request aged {}ms before batching", age.as_millis());
                }
                req.input
            })
            .collect();

        self.last_batch = Instant::now();
        batch
    }

    /// Returns the number of pending requests
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}
