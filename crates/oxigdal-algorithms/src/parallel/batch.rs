//! Parallel batch processing
//!
//! This module provides utilities for processing multiple files or datasets
//! in parallel with proper error handling and result collection.

use core::sync::atomic::{AtomicUsize, Ordering};
use rayon::prelude::*;

use crate::error::{AlgorithmError, Result};

/// Configuration for batch processing
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// Number of threads to use
    pub num_threads: Option<usize>,
    /// Maximum number of items to process in parallel
    pub max_parallel: usize,
    /// Enable progress tracking
    pub progress: bool,
    /// Continue processing on errors
    pub continue_on_error: bool,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            num_threads: None,
            max_parallel: 100,
            progress: false,
            continue_on_error: false,
        }
    }
}

impl BatchConfig {
    /// Creates a new batch configuration
    #[must_use]
    pub const fn new() -> Self {
        Self {
            num_threads: None,
            max_parallel: 100,
            progress: false,
            continue_on_error: false,
        }
    }

    /// Sets the number of threads
    #[must_use]
    pub const fn with_threads(mut self, num_threads: usize) -> Self {
        self.num_threads = Some(num_threads);
        self
    }

    /// Sets the maximum number of parallel items
    #[must_use]
    pub const fn with_max_parallel(mut self, max_parallel: usize) -> Self {
        self.max_parallel = max_parallel;
        self
    }

    /// Enables progress tracking
    #[must_use]
    pub const fn with_progress(mut self, progress: bool) -> Self {
        self.progress = progress;
        self
    }

    /// Sets continue on error behavior
    #[must_use]
    pub const fn with_continue_on_error(mut self, continue_on_error: bool) -> Self {
        self.continue_on_error = continue_on_error;
        self
    }
}

/// Result of a batch operation
#[derive(Debug, Clone)]
pub struct BatchResult<T> {
    /// Successfully processed items
    pub successes: Vec<T>,
    /// Failed items with their errors
    pub failures: Vec<(usize, String)>,
    /// Total number of items processed
    pub total: usize,
}

impl<T> BatchResult<T> {
    /// Creates a new batch result
    #[must_use]
    pub const fn new(successes: Vec<T>, failures: Vec<(usize, String)>, total: usize) -> Self {
        Self {
            successes,
            failures,
            total,
        }
    }

    /// Returns true if all items succeeded
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.failures.is_empty()
    }

    /// Returns the number of successful items
    #[must_use]
    pub fn success_count(&self) -> usize {
        self.successes.len()
    }

    /// Returns the number of failed items
    #[must_use]
    pub fn failure_count(&self) -> usize {
        self.failures.len()
    }

    /// Returns the success rate as a percentage
    #[must_use]
    pub fn success_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.success_count() as f64 / self.total as f64) * 100.0
        }
    }
}

/// Progress tracker for batch operations
pub struct BatchProgress {
    total: usize,
    processed: AtomicUsize,
    succeeded: AtomicUsize,
    failed: AtomicUsize,
}

impl BatchProgress {
    /// Creates a new progress tracker
    #[must_use]
    pub const fn new(total: usize) -> Self {
        Self {
            total,
            processed: AtomicUsize::new(0),
            succeeded: AtomicUsize::new(0),
            failed: AtomicUsize::new(0),
        }
    }

    /// Records a successful item
    pub fn record_success(&self) {
        self.succeeded.fetch_add(1, Ordering::Relaxed);
        let current = self.processed.fetch_add(1, Ordering::Relaxed) + 1;
        self.log_progress(current);
    }

    /// Records a failed item
    pub fn record_failure(&self) {
        self.failed.fetch_add(1, Ordering::Relaxed);
        let current = self.processed.fetch_add(1, Ordering::Relaxed) + 1;
        self.log_progress(current);
    }

    /// Logs progress
    fn log_progress(&self, current: usize) {
        if current % 10 == 0 || current == self.total {
            let percent = (current * 100) / self.total;
            let succeeded = self.succeeded.load(Ordering::Relaxed);
            let failed = self.failed.load(Ordering::Relaxed);
            tracing::info!(
                "Batch progress: {}/{} ({}%) - Success: {}, Failed: {}",
                current,
                self.total,
                percent,
                succeeded,
                failed
            );
        }
    }

    /// Returns current statistics
    #[must_use]
    pub fn stats(&self) -> (usize, usize, usize) {
        (
            self.processed.load(Ordering::Relaxed),
            self.succeeded.load(Ordering::Relaxed),
            self.failed.load(Ordering::Relaxed),
        )
    }
}

/// Process a batch of items in parallel
///
/// This function processes multiple items in parallel, collecting both
/// successes and failures. It provides thread-safe error handling and
/// optional progress tracking.
///
/// # Arguments
///
/// * `items` - Items to process
/// * `config` - Batch configuration
/// * `func` - Function to apply to each item
///
/// # Returns
///
/// Batch result containing successes and failures
///
/// # Example
///
/// ```no_run
/// # #[cfg(feature = "parallel")]
/// # {
/// use oxigdal_algorithms::parallel::{parallel_batch_process, BatchConfig};
///
/// let items = vec![1, 2, 3, 4, 5];
/// let config = BatchConfig::default();
///
/// let result = parallel_batch_process(&items, &config, |&item| {
///     Ok(item * 2)
/// });
///
/// match result {
///     Ok(batch_result) => {
///         println!("Success: {}/{}", batch_result.success_count(), batch_result.total);
///     }
///     Err(e) => eprintln!("Batch processing failed: {}", e),
/// }
/// # }
/// ```
pub fn parallel_batch_process<T, R, F>(
    items: &[T],
    config: &BatchConfig,
    func: F,
) -> Result<BatchResult<R>>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> Result<R> + Sync + Send,
{
    if items.is_empty() {
        return Ok(BatchResult::new(Vec::new(), Vec::new(), 0));
    }

    let progress = if config.progress {
        Some(BatchProgress::new(items.len()))
    } else {
        None
    };

    // Process items in parallel
    let results: Vec<(usize, Result<R>)> = items
        .par_iter()
        .enumerate()
        .map(|(idx, item)| {
            let result = func(item);

            if let Some(ref tracker) = progress {
                match result {
                    Ok(_) => tracker.record_success(),
                    Err(_) => tracker.record_failure(),
                }
            }

            (idx, result)
        })
        .collect();

    // Separate successes and failures
    let mut successes = Vec::new();
    let mut failures = Vec::new();

    for (idx, result) in results {
        match result {
            Ok(value) => successes.push(value),
            Err(e) => {
                failures.push((idx, e.to_string()));
                if !config.continue_on_error {
                    return Err(AlgorithmError::Core(oxigdal_core::OxiGdalError::Internal {
                        message: format!("Batch processing failed at item {}: {}", idx, e),
                    }));
                }
            }
        }
    }

    Ok(BatchResult::new(successes, failures, items.len()))
}

/// Process a batch of items with indices
///
/// Similar to `parallel_batch_process` but also passes the index to the function.
///
/// # Arguments
///
/// * `items` - Items to process
/// * `config` - Batch configuration
/// * `func` - Function to apply to each item (receives index and item)
///
/// # Returns
///
/// Batch result containing successes and failures
pub fn parallel_batch_process_indexed<T, R, F>(
    items: &[T],
    config: &BatchConfig,
    func: F,
) -> Result<BatchResult<R>>
where
    T: Sync,
    R: Send,
    F: Fn(usize, &T) -> Result<R> + Sync + Send,
{
    if items.is_empty() {
        return Ok(BatchResult::new(Vec::new(), Vec::new(), 0));
    }

    let progress = if config.progress {
        Some(BatchProgress::new(items.len()))
    } else {
        None
    };

    let results: Vec<(usize, Result<R>)> = items
        .par_iter()
        .enumerate()
        .map(|(idx, item)| {
            let result = func(idx, item);

            if let Some(ref tracker) = progress {
                match result {
                    Ok(_) => tracker.record_success(),
                    Err(_) => tracker.record_failure(),
                }
            }

            (idx, result)
        })
        .collect();

    let mut successes = Vec::new();
    let mut failures = Vec::new();

    for (idx, result) in results {
        match result {
            Ok(value) => successes.push(value),
            Err(e) => {
                failures.push((idx, e.to_string()));
                if !config.continue_on_error {
                    return Err(AlgorithmError::Core(oxigdal_core::OxiGdalError::Internal {
                        message: format!("Batch processing failed at item {}: {}", idx, e),
                    }));
                }
            }
        }
    }

    Ok(BatchResult::new(successes, failures, items.len()))
}

/// Process a batch in chunks
///
/// This is useful when you want to control memory usage by processing
/// items in smaller batches.
///
/// # Arguments
///
/// * `items` - Items to process
/// * `chunk_size` - Number of items per chunk
/// * `config` - Batch configuration
/// * `func` - Function to apply to each chunk
///
/// # Returns
///
/// Batch result containing successes and failures
pub fn parallel_batch_process_chunked<T, R, F>(
    items: &[T],
    chunk_size: usize,
    config: &BatchConfig,
    func: F,
) -> Result<BatchResult<R>>
where
    T: Sync,
    R: Send,
    F: Fn(&[T]) -> Result<Vec<R>> + Sync + Send,
{
    if items.is_empty() {
        return Ok(BatchResult::new(Vec::new(), Vec::new(), 0));
    }

    let chunks: Vec<&[T]> = items.chunks(chunk_size).collect();

    let progress = if config.progress {
        Some(BatchProgress::new(chunks.len()))
    } else {
        None
    };

    let results: Vec<Result<Vec<R>>> = chunks
        .par_iter()
        .map(|chunk| {
            let result = func(chunk);

            if let Some(ref tracker) = progress {
                match result {
                    Ok(_) => tracker.record_success(),
                    Err(_) => tracker.record_failure(),
                }
            }

            result
        })
        .collect();

    let mut successes = Vec::new();
    let mut failures = Vec::new();
    let mut chunk_idx = 0;

    for result in results {
        match result {
            Ok(values) => successes.extend(values),
            Err(e) => {
                failures.push((chunk_idx, e.to_string()));
                if !config.continue_on_error {
                    return Err(AlgorithmError::Core(oxigdal_core::OxiGdalError::Internal {
                        message: format!("Chunk processing failed at chunk {}: {}", chunk_idx, e),
                    }));
                }
            }
        }
        chunk_idx += 1;
    }

    Ok(BatchResult::new(successes, failures, items.len()))
}

/// Parallel map operation
///
/// A simpler version that just returns all results or the first error.
///
/// # Arguments
///
/// * `items` - Items to process
/// * `func` - Function to apply to each item
///
/// # Returns
///
/// Vector of results
///
/// # Errors
///
/// Returns the first error encountered
pub fn parallel_map<T, R, F>(items: &[T], func: F) -> Result<Vec<R>>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> Result<R> + Sync + Send,
{
    items.par_iter().map(func).collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn test_batch_config() {
        let config = BatchConfig::default();
        assert!(config.num_threads.is_none());
        assert_eq!(config.max_parallel, 100);
        assert!(!config.progress);
        assert!(!config.continue_on_error);
    }

    #[test]
    fn test_batch_config_builder() {
        let config = BatchConfig::new()
            .with_threads(4)
            .with_max_parallel(50)
            .with_progress(true)
            .with_continue_on_error(true);

        assert_eq!(config.num_threads, Some(4));
        assert_eq!(config.max_parallel, 50);
        assert!(config.progress);
        assert!(config.continue_on_error);
    }

    #[test]
    fn test_parallel_batch_process_success() {
        let items = vec![1, 2, 3, 4, 5];
        let config = BatchConfig::default();

        let result =
            parallel_batch_process(&items, &config, |&item| Ok(item * 2)).expect("should work");

        assert!(result.is_success());
        assert_eq!(result.success_count(), 5);
        assert_eq!(result.failure_count(), 0);
        assert_eq!(result.successes, vec![2, 4, 6, 8, 10]);
    }

    #[test]
    fn test_parallel_batch_process_with_errors() {
        let items = vec![1, 2, 3, 4, 5];
        let config = BatchConfig::new().with_continue_on_error(true);

        let result = parallel_batch_process(&items, &config, |&item| {
            if item % 2 == 0 {
                Err(AlgorithmError::Core(oxigdal_core::OxiGdalError::Internal {
                    message: "Even number".to_string(),
                }))
            } else {
                Ok(item * 2)
            }
        })
        .expect("should work");

        assert!(!result.is_success());
        assert_eq!(result.success_count(), 3); // 1, 3, 5
        assert_eq!(result.failure_count(), 2); // 2, 4
        assert!(result.success_rate() > 59.0 && result.success_rate() < 61.0);
    }

    #[test]
    fn test_parallel_batch_process_fail_fast() {
        let items = vec![1, 2, 3, 4, 5];
        let config = BatchConfig::new().with_continue_on_error(false);

        let result = parallel_batch_process(&items, &config, |&item| {
            if item % 2 == 0 {
                Err(AlgorithmError::Core(oxigdal_core::OxiGdalError::Internal {
                    message: "Even number".to_string(),
                }))
            } else {
                Ok(item * 2)
            }
        });

        assert!(result.is_err());
    }

    #[test]
    fn test_parallel_batch_process_indexed() {
        let items = vec!["a", "b", "c"];
        let config = BatchConfig::default();

        let result = parallel_batch_process_indexed(&items, &config, |idx, &item| {
            Ok(format!("{}:{}", idx, item))
        })
        .expect("should work");

        assert!(result.is_success());
        assert_eq!(result.success_count(), 3);

        // Results should contain index:item pairs
        assert!(result.successes.contains(&"0:a".to_string()));
        assert!(result.successes.contains(&"1:b".to_string()));
        assert!(result.successes.contains(&"2:c".to_string()));
    }

    #[test]
    fn test_parallel_batch_process_chunked() {
        let items: Vec<i32> = (0..100).collect();
        let config = BatchConfig::default();

        let result = parallel_batch_process_chunked(&items, 10, &config, |chunk| {
            Ok(chunk.iter().map(|&x| x * 2).collect())
        })
        .expect("should work");

        assert!(result.is_success());
        assert_eq!(result.success_count(), 100);
    }

    #[test]
    fn test_parallel_map() {
        let items = vec![1, 2, 3, 4, 5];
        let result = parallel_map(&items, |&item| Ok(item * 2)).expect("should work");

        assert_eq!(result, vec![2, 4, 6, 8, 10]);
    }

    #[test]
    fn test_parallel_map_with_error() {
        let items = vec![1, 2, 3, 4, 5];
        let result = parallel_map(&items, |&item| {
            if item == 3 {
                Err(AlgorithmError::Core(oxigdal_core::OxiGdalError::Internal {
                    message: "Error".to_string(),
                }))
            } else {
                Ok(item * 2)
            }
        });

        assert!(result.is_err());
    }

    #[test]
    fn test_batch_result() {
        let result = BatchResult::new(vec![1, 2, 3], vec![(1, "error".to_string())], 4);

        assert!(!result.is_success());
        assert_eq!(result.success_count(), 3);
        assert_eq!(result.failure_count(), 1);
        assert_eq!(result.total, 4);
        assert_eq!(result.success_rate(), 75.0);
    }

    #[test]
    fn test_batch_progress() {
        let progress = BatchProgress::new(10);

        progress.record_success();
        progress.record_success();
        progress.record_failure();

        let (processed, succeeded, failed) = progress.stats();
        assert_eq!(processed, 3);
        assert_eq!(succeeded, 2);
        assert_eq!(failed, 1);
    }

    #[test]
    fn test_empty_batch() {
        let items: Vec<i32> = Vec::new();
        let config = BatchConfig::default();

        let result =
            parallel_batch_process(&items, &config, |&item| Ok(item * 2)).expect("should work");

        assert!(result.is_success());
        assert_eq!(result.total, 0);
        assert_eq!(result.success_count(), 0);
    }
}
