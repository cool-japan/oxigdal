//! Parallel processing framework for CLI operations
//!
//! Provides comprehensive parallel processing capabilities including:
//! - Thread pool management and configuration
//! - Parallel file processing with progress tracking
//! - Work distribution across threads with load balancing
//! - Progress aggregation across multiple operations
//! - Error collection and handling
//! - Resource management with memory limits
//! - Batch operations with configurable sizes
//! - Pipeline execution for chained operations

// Allow dead code for this module as it provides utility functions
// that may be used in the future
#![allow(dead_code)]

use anyhow::{Context, Result};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

// ============================================================================
// Thread Pool Configuration
// ============================================================================

/// Thread pool configuration for parallel operations
#[derive(Debug, Clone)]
pub struct ThreadPoolConfig {
    /// Number of threads (None = auto-detect)
    pub num_threads: Option<usize>,
    /// Stack size per thread in bytes
    pub stack_size: Option<usize>,
    /// Thread name prefix
    pub thread_name_prefix: String,
    /// Enable thread pinning (CPU affinity)
    pub pin_threads: bool,
    /// Priority level for threads
    pub priority: ThreadPriority,
}

/// Thread priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThreadPriority {
    /// Low priority (background tasks)
    Low,
    /// Normal priority (default)
    #[default]
    Normal,
    /// High priority (time-sensitive tasks)
    High,
}

impl Default for ThreadPoolConfig {
    fn default() -> Self {
        Self {
            num_threads: None,
            stack_size: Some(8 * 1024 * 1024), // 8 MB
            thread_name_prefix: "oxigdal-worker".to_string(),
            pin_threads: false,
            priority: ThreadPriority::Normal,
        }
    }
}

impl ThreadPoolConfig {
    /// Create a new thread pool configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set number of threads
    pub fn with_num_threads(mut self, num: usize) -> Self {
        self.num_threads = Some(num);
        self
    }

    /// Set stack size
    pub fn with_stack_size(mut self, size: usize) -> Self {
        self.stack_size = Some(size);
        self
    }

    /// Set thread name prefix
    pub fn with_name_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.thread_name_prefix = prefix.into();
        self
    }

    /// Enable thread pinning
    pub fn with_pin_threads(mut self, pin: bool) -> Self {
        self.pin_threads = pin;
        self
    }

    /// Set thread priority
    pub fn with_priority(mut self, priority: ThreadPriority) -> Self {
        self.priority = priority;
        self
    }
}

/// Initialize global thread pool with custom configuration
pub fn init_thread_pool(config: ThreadPoolConfig) -> Result<()> {
    let mut builder = rayon::ThreadPoolBuilder::new();

    if let Some(num_threads) = config.num_threads {
        builder = builder.num_threads(num_threads);
    }

    if let Some(stack_size) = config.stack_size {
        builder = builder.stack_size(stack_size);
    }

    let prefix = config.thread_name_prefix;
    builder = builder.thread_name(move |idx| format!("{}-{}", prefix, idx));

    builder
        .build_global()
        .context("Failed to initialize thread pool")?;

    Ok(())
}

/// Get optimal number of threads for current system
pub fn optimal_thread_count() -> usize {
    num_cpus::get()
}

/// Get optimal number of physical cores
pub fn physical_core_count() -> usize {
    num_cpus::get_physical()
}

// ============================================================================
// Work Distribution
// ============================================================================

/// Work item with priority and metadata
#[derive(Debug, Clone)]
pub struct WorkItem<T> {
    /// The actual work data
    pub data: T,
    /// Priority (higher = processed first)
    pub priority: i32,
    /// Estimated cost/time for load balancing
    pub estimated_cost: u64,
    /// Optional group identifier for grouping related work
    pub group_id: Option<String>,
}

impl<T> WorkItem<T> {
    /// Create a new work item with default priority
    pub fn new(data: T) -> Self {
        Self {
            data,
            priority: 0,
            estimated_cost: 1,
            group_id: None,
        }
    }

    /// Set priority
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Set estimated cost
    pub fn with_cost(mut self, cost: u64) -> Self {
        self.estimated_cost = cost;
        self
    }

    /// Set group ID
    pub fn with_group(mut self, group: impl Into<String>) -> Self {
        self.group_id = Some(group.into());
        self
    }
}

/// Work distribution strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DistributionStrategy {
    /// Even distribution across threads
    #[default]
    RoundRobin,
    /// Dynamic work stealing (default rayon behavior)
    WorkStealing,
    /// Cost-based load balancing
    LoadBalanced,
    /// Process by priority
    PriorityBased,
}

/// Work distributor for parallel operations
pub struct WorkDistributor<T> {
    items: Vec<WorkItem<T>>,
    strategy: DistributionStrategy,
    chunk_size: Option<usize>,
}

impl<T: Send + Sync> WorkDistributor<T> {
    /// Create a new work distributor
    pub fn new(items: Vec<WorkItem<T>>) -> Self {
        Self {
            items,
            strategy: DistributionStrategy::default(),
            chunk_size: None,
        }
    }

    /// Create from raw items (wraps in WorkItem)
    pub fn from_items(items: impl IntoIterator<Item = T>) -> Self {
        let work_items: Vec<WorkItem<T>> = items.into_iter().map(WorkItem::new).collect();
        Self::new(work_items)
    }

    /// Set distribution strategy
    pub fn with_strategy(mut self, strategy: DistributionStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Set chunk size for processing
    pub fn with_chunk_size(mut self, size: usize) -> Self {
        self.chunk_size = Some(size);
        self
    }

    /// Distribute and process work items
    pub fn process<R, F>(&mut self, processor: F) -> Result<Vec<R>>
    where
        R: Send,
        F: Fn(&T) -> Result<R> + Send + Sync,
    {
        // Sort items based on strategy
        match self.strategy {
            DistributionStrategy::PriorityBased => {
                self.items
                    .sort_by_key(|item| std::cmp::Reverse(item.priority));
            }
            DistributionStrategy::LoadBalanced => {
                // Sort by estimated cost (largest first for better load balancing)
                self.items
                    .sort_by_key(|item| std::cmp::Reverse(item.estimated_cost));
            }
            _ => {}
        }

        // Process items in parallel
        let results: Result<Vec<R>> = self
            .items
            .par_iter()
            .map(|item| processor(&item.data))
            .collect();

        results
    }

    /// Process with progress tracking
    pub fn process_with_progress<R, F>(
        &mut self,
        processor: F,
        progress_message: &str,
    ) -> Result<Vec<R>>
    where
        R: Send,
        F: Fn(&T) -> Result<R> + Send + Sync,
    {
        let pb = ProgressBar::new(self.items.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg} [{bar:40.cyan/blue}] {pos}/{len} ({per_sec}, ETA: {eta})")
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("=>-"),
        );
        pb.set_message(progress_message.to_string());

        let results: Result<Vec<R>> = self
            .items
            .par_iter()
            .map(|item| {
                let result = processor(&item.data);
                pb.inc(1);
                result
            })
            .collect();

        pb.finish_with_message(format!("{}: complete", progress_message));

        results
    }
}

// ============================================================================
// Progress Aggregation
// ============================================================================

/// Progress statistics for a single operation
#[derive(Debug, Clone)]
pub struct ProgressStats {
    /// Total items to process
    pub total: u64,
    /// Items completed successfully
    pub completed: u64,
    /// Items that failed
    pub failed: u64,
    /// Items currently in progress
    pub in_progress: u64,
    /// Start time
    pub start_time: Instant,
    /// Bytes processed (if applicable)
    pub bytes_processed: u64,
}

impl Default for ProgressStats {
    fn default() -> Self {
        Self {
            total: 0,
            completed: 0,
            failed: 0,
            in_progress: 0,
            start_time: Instant::now(),
            bytes_processed: 0,
        }
    }
}

impl ProgressStats {
    /// Calculate throughput (items per second)
    pub fn items_per_second(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.completed as f64 / elapsed
        } else {
            0.0
        }
    }

    /// Calculate bytes throughput
    pub fn bytes_per_second(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.bytes_processed as f64 / elapsed
        } else {
            0.0
        }
    }

    /// Estimate time remaining
    pub fn estimated_remaining(&self) -> Duration {
        let remaining = self.total.saturating_sub(self.completed + self.failed);
        let rate = self.items_per_second();
        if rate > 0.0 {
            Duration::from_secs_f64(remaining as f64 / rate)
        } else {
            Duration::MAX
        }
    }

    /// Get completion percentage
    pub fn percent_complete(&self) -> f64 {
        if self.total > 0 {
            ((self.completed + self.failed) as f64 / self.total as f64) * 100.0
        } else {
            0.0
        }
    }
}

/// Aggregated progress across multiple operations
pub struct ProgressAggregator {
    operations: Arc<RwLock<HashMap<String, ProgressStats>>>,
    multi_progress: Arc<MultiProgress>,
    progress_bars: Arc<Mutex<HashMap<String, ProgressBar>>>,
}

impl ProgressAggregator {
    /// Create a new progress aggregator
    pub fn new() -> Self {
        Self {
            operations: Arc::new(RwLock::new(HashMap::new())),
            multi_progress: Arc::new(MultiProgress::new()),
            progress_bars: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a new operation
    pub fn register_operation(&self, name: &str, total: u64) -> Result<()> {
        let stats = ProgressStats {
            total,
            ..Default::default()
        };

        let mut ops = self
            .operations
            .write()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        ops.insert(name.to_string(), stats);

        // Create progress bar
        let pb = self.multi_progress.add(ProgressBar::new(total));
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} {msg}: [{bar:40.cyan/blue}] {pos}/{len}")
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("=>-"),
        );
        pb.set_message(name.to_string());

        let mut pbs = self
            .progress_bars
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        pbs.insert(name.to_string(), pb);

        Ok(())
    }

    /// Update operation progress
    pub fn update(&self, name: &str, completed_delta: u64, failed_delta: u64) -> Result<()> {
        let mut ops = self
            .operations
            .write()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;

        if let Some(stats) = ops.get_mut(name) {
            stats.completed += completed_delta;
            stats.failed += failed_delta;
        }

        let pbs = self
            .progress_bars
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        if let Some(pb) = pbs.get(name) {
            pb.inc(completed_delta + failed_delta);
        }

        Ok(())
    }

    /// Update bytes processed
    pub fn update_bytes(&self, name: &str, bytes: u64) -> Result<()> {
        let mut ops = self
            .operations
            .write()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;

        if let Some(stats) = ops.get_mut(name) {
            stats.bytes_processed += bytes;
        }

        Ok(())
    }

    /// Get aggregated statistics
    pub fn get_aggregate_stats(&self) -> Result<ProgressStats> {
        let ops = self
            .operations
            .read()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;

        let mut aggregate = ProgressStats::default();
        for stats in ops.values() {
            aggregate.total += stats.total;
            aggregate.completed += stats.completed;
            aggregate.failed += stats.failed;
            aggregate.in_progress += stats.in_progress;
            aggregate.bytes_processed += stats.bytes_processed;
        }

        Ok(aggregate)
    }

    /// Finish an operation
    pub fn finish_operation(&self, name: &str, message: &str) -> Result<()> {
        let pbs = self
            .progress_bars
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        if let Some(pb) = pbs.get(name) {
            pb.finish_with_message(message.to_string());
        }
        Ok(())
    }
}

impl Default for ProgressAggregator {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Error Collection
// ============================================================================

/// Error information with context
#[derive(Debug, Clone)]
pub struct ErrorInfo {
    /// Error message
    pub message: String,
    /// Source file/item that caused the error
    pub source: Option<String>,
    /// When the error occurred
    pub timestamp: Instant,
    /// Is this error recoverable?
    pub recoverable: bool,
    /// Additional context
    pub context: HashMap<String, String>,
}

impl ErrorInfo {
    /// Create a new error info
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            source: None,
            timestamp: Instant::now(),
            recoverable: true,
            context: HashMap::new(),
        }
    }

    /// Set source
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Set recoverable flag
    pub fn with_recoverable(mut self, recoverable: bool) -> Self {
        self.recoverable = recoverable;
        self
    }

    /// Add context
    pub fn with_context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context.insert(key.into(), value.into());
        self
    }
}

/// Error collector for parallel operations
pub struct ErrorCollector {
    errors: Arc<Mutex<Vec<ErrorInfo>>>,
    max_errors: usize,
    stop_on_error: Arc<AtomicBool>,
    error_count: Arc<AtomicUsize>,
}

impl ErrorCollector {
    /// Create a new error collector
    pub fn new() -> Self {
        Self {
            errors: Arc::new(Mutex::new(Vec::new())),
            max_errors: 1000,
            stop_on_error: Arc::new(AtomicBool::new(false)),
            error_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Set maximum errors to collect
    pub fn with_max_errors(mut self, max: usize) -> Self {
        self.max_errors = max;
        self
    }

    /// Enable stop on first error
    pub fn with_stop_on_error(self, stop: bool) -> Self {
        self.stop_on_error.store(stop, Ordering::SeqCst);
        self
    }

    /// Check if should stop processing
    pub fn should_stop(&self) -> bool {
        self.stop_on_error.load(Ordering::SeqCst) && self.error_count.load(Ordering::SeqCst) > 0
    }

    /// Add an error
    pub fn add_error(&self, error: ErrorInfo) -> Result<()> {
        let count = self.error_count.fetch_add(1, Ordering::SeqCst);
        if count < self.max_errors {
            let mut errors = self
                .errors
                .lock()
                .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
            errors.push(error);
        }
        Ok(())
    }

    /// Add error from Result
    pub fn collect<T>(&self, result: Result<T>, source: Option<&str>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(e) => {
                let mut error_info = ErrorInfo::new(format!("{:?}", e));
                if let Some(src) = source {
                    error_info = error_info.with_source(src);
                }
                // Ignore errors when adding to collector (best effort)
                let _ = self.add_error(error_info);
                None
            }
        }
    }

    /// Get all collected errors
    pub fn get_errors(&self) -> Result<Vec<ErrorInfo>> {
        let errors = self
            .errors
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        Ok(errors.clone())
    }

    /// Get error count
    pub fn error_count(&self) -> usize {
        self.error_count.load(Ordering::SeqCst)
    }

    /// Check if there are any errors
    pub fn has_errors(&self) -> bool {
        self.error_count.load(Ordering::SeqCst) > 0
    }

    /// Get summary of errors
    pub fn summary(&self) -> Result<String> {
        let errors = self.get_errors()?;
        let total = self.error_count();

        if errors.is_empty() {
            return Ok("No errors".to_string());
        }

        let mut summary = format!("Total errors: {}\n", total);
        for (i, error) in errors.iter().enumerate().take(10) {
            summary.push_str(&format!(
                "  {}. {} (source: {})\n",
                i + 1,
                error.message,
                error.source.as_deref().unwrap_or("unknown")
            ));
        }

        if total > 10 {
            summary.push_str(&format!("  ... and {} more errors\n", total - 10));
        }

        Ok(summary)
    }
}

impl Default for ErrorCollector {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Parallel File Processing
// ============================================================================

/// Parallel file processor with advanced features
pub struct ParallelFileProcessor {
    multi_progress: Arc<MultiProgress>,
    file_count: Arc<AtomicUsize>,
    error_count: Arc<AtomicUsize>,
    bytes_processed: Arc<AtomicU64>,
    error_collector: ErrorCollector,
}

impl ParallelFileProcessor {
    /// Create a new parallel file processor
    pub fn new() -> Self {
        Self {
            multi_progress: Arc::new(MultiProgress::new()),
            file_count: Arc::new(AtomicUsize::new(0)),
            error_count: Arc::new(AtomicUsize::new(0)),
            bytes_processed: Arc::new(AtomicU64::new(0)),
            error_collector: ErrorCollector::new(),
        }
    }

    /// Process files in parallel with a custom function
    pub fn process_files<P, F>(
        &self,
        files: Vec<P>,
        processor: F,
        progress_message: &str,
    ) -> Result<Vec<Result<()>>>
    where
        P: AsRef<Path> + Send + Sync,
        F: Fn(&Path) -> Result<()> + Send + Sync,
    {
        let pb = self
            .multi_progress
            .add(ProgressBar::new(files.len() as u64));
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg} [{bar:40.cyan/blue}] {pos}/{len} ({per_sec}, ETA: {eta})")
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("=>-"),
        );
        pb.set_message(progress_message.to_string());

        let results: Vec<Result<()>> = files
            .par_iter()
            .map(|file| {
                let file_path = file.as_ref();
                let result = processor(file_path);

                if result.is_ok() {
                    self.file_count.fetch_add(1, Ordering::SeqCst);
                } else {
                    self.error_count.fetch_add(1, Ordering::SeqCst);
                }

                pb.inc(1);
                result
            })
            .collect();

        pb.finish_with_message(format!(
            "{}: {} succeeded, {} failed",
            progress_message,
            self.file_count.load(Ordering::SeqCst),
            self.error_count.load(Ordering::SeqCst)
        ));

        Ok(results)
    }

    /// Process files with result collection
    pub fn process_files_with_results<P, T, F>(
        &self,
        files: Vec<P>,
        processor: F,
        progress_message: &str,
    ) -> Result<Vec<(PathBuf, Result<T>)>>
    where
        P: AsRef<Path> + Send + Sync,
        T: Send,
        F: Fn(&Path) -> Result<T> + Send + Sync,
    {
        let pb = self
            .multi_progress
            .add(ProgressBar::new(files.len() as u64));
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg} [{bar:40.cyan/blue}] {pos}/{len} ({per_sec}, ETA: {eta})")
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("=>-"),
        );
        pb.set_message(progress_message.to_string());

        let results: Vec<(PathBuf, Result<T>)> = files
            .par_iter()
            .map(|file| {
                let file_path = file.as_ref();
                let result = processor(file_path);

                if result.is_ok() {
                    self.file_count.fetch_add(1, Ordering::SeqCst);
                } else {
                    self.error_count.fetch_add(1, Ordering::SeqCst);
                }

                pb.inc(1);
                (file_path.to_path_buf(), result)
            })
            .collect();

        pb.finish_with_message(format!(
            "{}: {} succeeded, {} failed",
            progress_message,
            self.file_count.load(Ordering::SeqCst),
            self.error_count.load(Ordering::SeqCst)
        ));

        Ok(results)
    }

    /// Add bytes processed
    pub fn add_bytes(&self, bytes: u64) {
        self.bytes_processed.fetch_add(bytes, Ordering::SeqCst);
    }

    /// Get statistics
    pub fn stats(&self) -> (usize, usize, u64) {
        (
            self.file_count.load(Ordering::SeqCst),
            self.error_count.load(Ordering::SeqCst),
            self.bytes_processed.load(Ordering::SeqCst),
        )
    }

    /// Get error collector
    pub fn error_collector(&self) -> &ErrorCollector {
        &self.error_collector
    }
}

impl Default for ParallelFileProcessor {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Parallel Band Processor
// ============================================================================

/// Parallel band processor for multi-band raster operations
pub struct ParallelBandProcessor {
    multi_progress: Arc<MultiProgress>,
}

impl ParallelBandProcessor {
    /// Create a new parallel band processor
    pub fn new() -> Self {
        Self {
            multi_progress: Arc::new(MultiProgress::new()),
        }
    }

    /// Process bands in parallel
    pub fn process_bands<T, F>(
        &self,
        band_indices: Vec<usize>,
        processor: F,
        progress_message: &str,
    ) -> Result<Vec<T>>
    where
        T: Send,
        F: Fn(usize) -> Result<T> + Send + Sync,
    {
        let pb = self
            .multi_progress
            .add(ProgressBar::new(band_indices.len() as u64));
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg} [{bar:40.green/blue}] {pos}/{len}")
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("=>-"),
        );
        pb.set_message(progress_message.to_string());

        let results: Result<Vec<T>> = band_indices
            .par_iter()
            .map(|&idx| {
                let result = processor(idx);
                pb.inc(1);
                result
            })
            .collect();

        pb.finish_with_message(format!("{}: complete", progress_message));

        results
    }
}

impl Default for ParallelBandProcessor {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Parallel Tile Processor
// ============================================================================

/// Parallel tile processor for chunked raster operations
pub struct ParallelTileProcessor {
    tile_size: (usize, usize),
    multi_progress: Arc<MultiProgress>,
}

impl ParallelTileProcessor {
    /// Create a new parallel tile processor
    pub fn new(tile_width: usize, tile_height: usize) -> Self {
        Self {
            tile_size: (tile_width, tile_height),
            multi_progress: Arc::new(MultiProgress::new()),
        }
    }

    /// Get tile size
    pub fn tile_size(&self) -> (usize, usize) {
        self.tile_size
    }

    /// Process raster in tiles with overlap
    pub fn process_tiles<T, F>(
        &self,
        raster_width: usize,
        raster_height: usize,
        overlap: usize,
        processor: F,
        progress_message: &str,
    ) -> Result<Vec<T>>
    where
        T: Send,
        F: Fn(usize, usize, usize, usize) -> Result<T> + Send + Sync,
    {
        let (tile_w, tile_h) = self.tile_size;

        // Calculate tile grid
        let tiles_x = raster_width.div_ceil(tile_w);
        let tiles_y = raster_height.div_ceil(tile_h);
        let total_tiles = tiles_x * tiles_y;

        let pb = self
            .multi_progress
            .add(ProgressBar::new(total_tiles as u64));
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg} [{bar:40.yellow/blue}] {pos}/{len} tiles")
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("=>-"),
        );
        pb.set_message(progress_message.to_string());

        let mut tile_specs = Vec::with_capacity(total_tiles);
        for ty in 0..tiles_y {
            for tx in 0..tiles_x {
                let x_start = tx * tile_w;
                let y_start = ty * tile_h;
                let x_end = (x_start + tile_w + overlap).min(raster_width);
                let y_end = (y_start + tile_h + overlap).min(raster_height);

                tile_specs.push((x_start, y_start, x_end - x_start, y_end - y_start));
            }
        }

        let results: Result<Vec<T>> = tile_specs
            .par_iter()
            .map(|&(x, y, w, h)| {
                let result = processor(x, y, w, h);
                pb.inc(1);
                result
            })
            .collect();

        pb.finish_with_message(format!("{}: complete", progress_message));

        results
    }
}

// ============================================================================
// Batch Operations
// ============================================================================

/// Batch operation manager
pub struct BatchManager {
    batch_size: usize,
    multi_progress: Arc<MultiProgress>,
}

impl BatchManager {
    /// Create a new batch manager
    pub fn new(batch_size: usize) -> Self {
        Self {
            batch_size: batch_size.max(1),
            multi_progress: Arc::new(MultiProgress::new()),
        }
    }

    /// Get batch size
    pub fn batch_size(&self) -> usize {
        self.batch_size
    }

    /// Process items in batches
    pub fn process_batches<T, R, F>(
        &self,
        items: Vec<T>,
        processor: F,
        progress_message: &str,
    ) -> Result<Vec<R>>
    where
        T: Send + Clone,
        R: Send,
        F: Fn(Vec<T>) -> Result<Vec<R>> + Send + Sync,
    {
        let batches: Vec<Vec<T>> = items
            .chunks(self.batch_size)
            .map(|chunk| chunk.to_vec())
            .collect();

        let pb = self
            .multi_progress
            .add(ProgressBar::new(batches.len() as u64));
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg} [{bar:40.magenta/blue}] {pos}/{len} batches")
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("=>-"),
        );
        pb.set_message(progress_message.to_string());

        let results: Result<Vec<Vec<R>>> = batches
            .into_par_iter()
            .map(|batch| {
                let result = processor(batch);
                pb.inc(1);
                result
            })
            .collect();

        pb.finish_with_message(format!("{}: complete", progress_message));

        results.map(|batches| batches.into_iter().flatten().collect())
    }
}

// ============================================================================
// Resource Management
// ============================================================================

/// Resource manager for parallel operations
pub struct ResourceManager {
    max_memory_bytes: Arc<Mutex<usize>>,
    current_memory_bytes: Arc<Mutex<usize>>,
    max_threads: usize,
    active_threads: Arc<AtomicUsize>,
}

impl ResourceManager {
    /// Create a new resource manager
    pub fn new(max_memory_mb: usize, max_threads: usize) -> Self {
        Self {
            max_memory_bytes: Arc::new(Mutex::new(max_memory_mb * 1024 * 1024)),
            current_memory_bytes: Arc::new(Mutex::new(0)),
            max_threads: max_threads.max(1),
            active_threads: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Try to allocate memory
    pub fn try_allocate(&self, bytes: usize) -> Result<bool> {
        let mut current = self
            .current_memory_bytes
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        let max = self
            .max_memory_bytes
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;

        if *current + bytes <= *max {
            *current += bytes;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Release allocated memory
    pub fn release(&self, bytes: usize) -> Result<()> {
        let mut current = self
            .current_memory_bytes
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;

        *current = current.saturating_sub(bytes);

        Ok(())
    }

    /// Get current memory usage in MB
    pub fn current_usage_mb(&self) -> Result<f64> {
        let current = self
            .current_memory_bytes
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        Ok(*current as f64 / (1024.0 * 1024.0))
    }

    /// Get max memory in MB
    pub fn max_memory_mb(&self) -> Result<f64> {
        let max = self
            .max_memory_bytes
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        Ok(*max as f64 / (1024.0 * 1024.0))
    }

    /// Get max threads
    pub fn max_threads(&self) -> usize {
        self.max_threads
    }

    /// Acquire a thread slot
    pub fn acquire_thread(&self) -> bool {
        let current = self.active_threads.fetch_add(1, Ordering::SeqCst);
        if current >= self.max_threads {
            self.active_threads.fetch_sub(1, Ordering::SeqCst);
            false
        } else {
            true
        }
    }

    /// Release a thread slot
    pub fn release_thread(&self) {
        self.active_threads.fetch_sub(1, Ordering::SeqCst);
    }

    /// Get active thread count
    pub fn active_threads(&self) -> usize {
        self.active_threads.load(Ordering::SeqCst)
    }
}

// ============================================================================
// Pipeline Execution
// ============================================================================

/// Pipeline stage trait
pub trait PipelineStage<I, O>: Send + Sync {
    /// Process input and produce output
    fn process(&self, input: I) -> Result<O>;

    /// Get stage name
    fn name(&self) -> &str;
}

/// Simple function-based pipeline stage
pub struct FnStage<I, O, F>
where
    I: Send + Sync,
    O: Send + Sync,
    F: Fn(I) -> Result<O> + Send + Sync,
{
    name: String,
    func: F,
    _phantom: std::marker::PhantomData<(I, O)>,
}

impl<I, O, F> FnStage<I, O, F>
where
    I: Send + Sync,
    O: Send + Sync,
    F: Fn(I) -> Result<O> + Send + Sync,
{
    /// Create a new function-based stage
    pub fn new(name: impl Into<String>, func: F) -> Self {
        Self {
            name: name.into(),
            func,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<I, O, F> PipelineStage<I, O> for FnStage<I, O, F>
where
    I: Send + Sync,
    O: Send + Sync,
    F: Fn(I) -> Result<O> + Send + Sync,
{
    fn process(&self, input: I) -> Result<O> {
        (self.func)(input)
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Pipeline execution result
#[derive(Debug)]
pub struct PipelineResult<T> {
    /// Successful results
    pub successes: Vec<T>,
    /// Failed items with error info
    pub failures: Vec<ErrorInfo>,
    /// Total processing time
    pub duration: Duration,
    /// Items processed per second
    pub throughput: f64,
}

/// Pipeline executor for chained operations
pub struct PipelineExecutor {
    multi_progress: Arc<MultiProgress>,
    error_collector: ErrorCollector,
}

impl PipelineExecutor {
    /// Create a new pipeline executor
    pub fn new() -> Self {
        Self {
            multi_progress: Arc::new(MultiProgress::new()),
            error_collector: ErrorCollector::new(),
        }
    }

    /// Execute a two-stage pipeline
    pub fn execute_two_stage<I, M, O, S1, S2>(
        &self,
        inputs: Vec<I>,
        stage1: &S1,
        stage2: &S2,
        progress_message: &str,
    ) -> Result<PipelineResult<O>>
    where
        I: Send + Sync + Clone,
        M: Send + Sync,
        O: Send,
        S1: PipelineStage<I, M>,
        S2: PipelineStage<M, O>,
    {
        let start = Instant::now();
        let total = inputs.len() as u64;

        let pb = self.multi_progress.add(ProgressBar::new(total));
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg} [{bar:40.cyan/blue}] {pos}/{len} ({per_sec})")
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("=>-"),
        );
        pb.set_message(progress_message.to_string());

        let results: Vec<Result<O>> = inputs
            .par_iter()
            .map(|input| {
                let mid = stage1.process(input.clone())?;
                let output = stage2.process(mid)?;
                pb.inc(1);
                Ok(output)
            })
            .collect();

        pb.finish_with_message(format!("{}: complete", progress_message));

        let duration = start.elapsed();
        let mut successes = Vec::new();
        let mut failures = Vec::new();

        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(output) => successes.push(output),
                Err(e) => {
                    failures.push(
                        ErrorInfo::new(format!("{:?}", e)).with_source(format!("item_{}", i)),
                    );
                }
            }
        }

        let throughput = if duration.as_secs_f64() > 0.0 {
            total as f64 / duration.as_secs_f64()
        } else {
            0.0
        };

        Ok(PipelineResult {
            successes,
            failures,
            duration,
            throughput,
        })
    }

    /// Execute a three-stage pipeline
    pub fn execute_three_stage<I, M1, M2, O, S1, S2, S3>(
        &self,
        inputs: Vec<I>,
        stage1: &S1,
        stage2: &S2,
        stage3: &S3,
        progress_message: &str,
    ) -> Result<PipelineResult<O>>
    where
        I: Send + Sync + Clone,
        M1: Send + Sync,
        M2: Send + Sync,
        O: Send,
        S1: PipelineStage<I, M1>,
        S2: PipelineStage<M1, M2>,
        S3: PipelineStage<M2, O>,
    {
        let start = Instant::now();
        let total = inputs.len() as u64;

        let pb = self.multi_progress.add(ProgressBar::new(total));
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg} [{bar:40.cyan/blue}] {pos}/{len} ({per_sec})")
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("=>-"),
        );
        pb.set_message(progress_message.to_string());

        let results: Vec<Result<O>> = inputs
            .par_iter()
            .map(|input| {
                let m1 = stage1.process(input.clone())?;
                let m2 = stage2.process(m1)?;
                let output = stage3.process(m2)?;
                pb.inc(1);
                Ok(output)
            })
            .collect();

        pb.finish_with_message(format!("{}: complete", progress_message));

        let duration = start.elapsed();
        let mut successes = Vec::new();
        let mut failures = Vec::new();

        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(output) => successes.push(output),
                Err(e) => {
                    failures.push(
                        ErrorInfo::new(format!("{:?}", e)).with_source(format!("item_{}", i)),
                    );
                }
            }
        }

        let throughput = if duration.as_secs_f64() > 0.0 {
            total as f64 / duration.as_secs_f64()
        } else {
            0.0
        };

        Ok(PipelineResult {
            successes,
            failures,
            duration,
            throughput,
        })
    }

    /// Get error collector
    pub fn error_collector(&self) -> &ErrorCollector {
        &self.error_collector
    }
}

impl Default for PipelineExecutor {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optimal_thread_count() {
        let count = optimal_thread_count();
        assert!(count > 0);
    }

    #[test]
    fn test_physical_core_count() {
        let count = physical_core_count();
        assert!(count > 0);
        // In containerized environments, physical cores can exceed cgroup-limited logical cores
        // so we only check that the count is positive and reasonable
        assert!(
            count <= 1024,
            "Physical core count seems unreasonable: {}",
            count
        );
    }

    #[test]
    fn test_thread_pool_config() {
        let config = ThreadPoolConfig::new()
            .with_num_threads(4)
            .with_stack_size(4 * 1024 * 1024)
            .with_name_prefix("test-worker")
            .with_priority(ThreadPriority::High);

        assert_eq!(config.num_threads, Some(4));
        assert_eq!(config.stack_size, Some(4 * 1024 * 1024));
        assert_eq!(config.thread_name_prefix, "test-worker");
        assert_eq!(config.priority, ThreadPriority::High);
    }

    #[test]
    fn test_work_item() {
        let item = WorkItem::new(42)
            .with_priority(10)
            .with_cost(100)
            .with_group("test-group");

        assert_eq!(item.data, 42);
        assert_eq!(item.priority, 10);
        assert_eq!(item.estimated_cost, 100);
        assert_eq!(item.group_id, Some("test-group".to_string()));
    }

    #[test]
    fn test_work_distributor() {
        let items: Vec<i32> = (0..100).collect();
        let mut distributor =
            WorkDistributor::from_items(items).with_strategy(DistributionStrategy::WorkStealing);

        let results = distributor
            .process(|&x| Ok(x * 2))
            .expect("Processing failed");

        assert_eq!(results.len(), 100);
    }

    #[test]
    fn test_progress_stats() {
        let stats = ProgressStats {
            total: 100,
            completed: 50,
            failed: 5,
            ..Default::default()
        };

        assert!((stats.percent_complete() - 55.0).abs() < 0.001);
    }

    #[test]
    fn test_error_collector() {
        let collector = ErrorCollector::new();

        let error = ErrorInfo::new("Test error")
            .with_source("test.txt")
            .with_recoverable(true);

        collector.add_error(error).expect("Failed to add error");

        assert!(collector.has_errors());
        assert_eq!(collector.error_count(), 1);

        let errors = collector.get_errors().expect("Failed to get errors");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "Test error");
    }

    #[test]
    fn test_error_collector_collect() {
        let collector = ErrorCollector::new();

        let ok_result: Result<i32> = Ok(42);
        let err_result: Result<i32> = Err(anyhow::anyhow!("Test error"));

        let ok_value = collector.collect(ok_result, Some("source1"));
        let err_value = collector.collect(err_result, Some("source2"));

        assert_eq!(ok_value, Some(42));
        assert_eq!(err_value, None);
        assert_eq!(collector.error_count(), 1);
    }

    #[test]
    fn test_resource_manager() {
        let rm = ResourceManager::new(100, 4); // 100 MB max

        assert!(
            rm.try_allocate(50 * 1024 * 1024)
                .expect("Allocation failed")
        );
        assert_eq!(
            rm.current_usage_mb().expect("Usage check failed").round() as u64,
            50
        );

        assert!(
            rm.try_allocate(50 * 1024 * 1024)
                .expect("Allocation failed")
        );
        assert!(
            !rm.try_allocate(1024 * 1024)
                .expect("Allocation check failed")
        );

        rm.release(50 * 1024 * 1024).expect("Release failed");
        assert_eq!(
            rm.current_usage_mb().expect("Usage check failed").round() as u64,
            50
        );
    }

    #[test]
    fn test_resource_manager_threads() {
        let rm = ResourceManager::new(100, 2);

        assert!(rm.acquire_thread());
        assert!(rm.acquire_thread());
        assert!(!rm.acquire_thread()); // Should fail, max reached

        rm.release_thread();
        assert!(rm.acquire_thread()); // Should succeed now
    }

    #[test]
    fn test_batch_manager() {
        let bm = BatchManager::new(10);
        let items: Vec<i32> = (0..100).collect();

        let result = bm
            .process_batches(items, Ok, "Test batch processing")
            .expect("Batch processing failed");

        assert_eq!(result.len(), 100);
    }

    #[test]
    fn test_tile_processor() {
        let processor = ParallelTileProcessor::new(256, 256);

        let results = processor
            .process_tiles(1024, 1024, 0, |x, y, w, h| Ok((x, y, w, h)), "Test tiles")
            .expect("Tile processing failed");

        assert_eq!(results.len(), 16); // 4x4 tiles
    }

    #[test]
    fn test_pipeline_stage() {
        let stage = FnStage::new("double", |x: i32| Ok(x * 2));

        let result = stage.process(21).expect("Processing failed");
        assert_eq!(result, 42);
        assert_eq!(stage.name(), "double");
    }

    #[test]
    fn test_pipeline_executor() {
        let executor = PipelineExecutor::new();

        let stage1 = FnStage::new("add_one", |x: i32| Ok(x + 1));
        let stage2 = FnStage::new("double", |x: i32| Ok(x * 2));

        let inputs: Vec<i32> = (0..10).collect();
        let result = executor
            .execute_two_stage(inputs, &stage1, &stage2, "Test pipeline")
            .expect("Pipeline failed");

        assert_eq!(result.successes.len(), 10);
        assert!(result.failures.is_empty());
        assert_eq!(result.successes[0], 2); // (0 + 1) * 2
        assert_eq!(result.successes[1], 4); // (1 + 1) * 2
    }

    #[test]
    fn test_three_stage_pipeline() {
        let executor = PipelineExecutor::new();

        let stage1 = FnStage::new("add_one", |x: i32| Ok(x + 1));
        let stage2 = FnStage::new("double", |x: i32| Ok(x * 2));
        let stage3 = FnStage::new("to_string", |x: i32| Ok(x.to_string()));

        let inputs: Vec<i32> = (0..5).collect();
        let result = executor
            .execute_three_stage(inputs, &stage1, &stage2, &stage3, "Test 3-stage pipeline")
            .expect("Pipeline failed");

        assert_eq!(result.successes.len(), 5);
        assert_eq!(result.successes[0], "2");
        assert_eq!(result.successes[1], "4");
    }

    #[test]
    fn test_progress_aggregator() {
        let agg = ProgressAggregator::new();

        agg.register_operation("op1", 100)
            .expect("Registration failed");
        agg.register_operation("op2", 50)
            .expect("Registration failed");

        agg.update("op1", 10, 0).expect("Update failed");
        agg.update("op2", 5, 1).expect("Update failed");

        let stats = agg.get_aggregate_stats().expect("Stats failed");
        assert_eq!(stats.total, 150);
        assert_eq!(stats.completed, 15);
        assert_eq!(stats.failed, 1);
    }
}
