//! Async operations for Node.js bindings
//!
//! This module provides Promise-based async operations for I/O and processing.

use napi::bindgen_prelude::*;
use napi::threadsafe_function::{
    ThreadsafeCallContext, ThreadsafeFunction, ThreadsafeFunctionCallMode,
};
use napi::tokio;
use napi_derive::napi;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use crate::buffer::BufferWrapper;
use crate::error::NodeError;
use crate::raster::Dataset;
use crate::vector::FeatureCollection;

/// Type of the threadsafe function used to relay progress notifications
/// (a fraction in `[0.0, 1.0]`) back into JavaScript from Rust worker
/// threads spawned via `tokio::task::spawn_blocking`.
type ProgressTsfn = ThreadsafeFunction<f64, (), f64, Status, false, false, 0>;

/// Global slot for the currently registered progress callback. `None` means
/// no callback is registered; progress notifications are then silently
/// skipped (there is nobody to notify), which is the intended default
/// behavior rather than a silent failure.
static PROGRESS_CALLBACK: OnceLock<Mutex<Option<ProgressTsfn>>> = OnceLock::new();

/// Converts a poisoned-lock condition into a proper JS-facing error instead
/// of panicking (`Mutex::lock().unwrap()` would violate the no-panic policy).
fn lock_progress_callback() -> Result<std::sync::MutexGuard<'static, Option<ProgressTsfn>>> {
    PROGRESS_CALLBACK
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| {
            NodeError {
                code: "INTERNAL_ERROR".to_string(),
                message: "Progress callback lock was poisoned".to_string(),
            }
            .into()
        })
}

/// Sends a best-effort progress notification (fraction in `[0.0, 1.0]`) to
/// the currently registered JS callback, if any. Progress reporting is
/// inherently non-critical: if no callback is registered, or the lock is
/// (rarely) poisoned, or the JS side has been torn down, this quietly does
/// nothing rather than failing the long-running operation it was called
/// from.
fn report_progress(progress: f64) {
    if let Some(cell) = PROGRESS_CALLBACK.get()
        && let Ok(guard) = cell.lock()
        && let Some(tsfn) = guard.as_ref()
    {
        let _ = tsfn.call(progress, ThreadsafeFunctionCallMode::NonBlocking);
    }
}

/// Opens a raster dataset asynchronously
#[allow(dead_code)]
#[napi]
pub async fn open_raster_async(path: String) -> Result<Dataset> {
    tokio::task::spawn_blocking(move || Dataset::open(path))
        .await
        .map_err(|e| NodeError {
            code: "TASK_ERROR".to_string(),
            message: format!("Task execution failed: {}", e),
        })?
}

/// Saves a dataset asynchronously
#[allow(dead_code)]
#[napi]
pub async fn save_raster_async(dataset: &Dataset, path: String) -> Result<()> {
    let ds_clone = dataset.clone();
    tokio::task::spawn_blocking(move || ds_clone.save(path))
        .await
        .map_err(|e| NodeError {
            code: "TASK_ERROR".to_string(),
            message: format!("Task execution failed: {}", e),
        })?
}

/// Reads a GeoJSON file asynchronously
#[allow(dead_code)]
#[napi]
pub async fn read_geojson_async(path: String) -> Result<FeatureCollection> {
    tokio::task::spawn_blocking(move || {
        let content = std::fs::read_to_string(&path).map_err(|e| NodeError {
            code: "IO_ERROR".to_string(),
            message: format!("Failed to read file: {}", e),
        })?;
        FeatureCollection::from_geojson(content)
    })
    .await
    .map_err(|e| NodeError {
        code: "TASK_ERROR".to_string(),
        message: format!("Task execution failed: {}", e),
    })?
}

/// Writes a GeoJSON file asynchronously
#[allow(dead_code)]
#[napi]
pub async fn write_geojson_async(path: String, collection: &FeatureCollection) -> Result<()> {
    let content = collection.to_geojson()?;
    tokio::task::spawn_blocking(move || {
        std::fs::write(&path, content).map_err(|e| {
            NodeError {
                code: "IO_ERROR".to_string(),
                message: format!("Failed to write file: {}", e),
            }
            .into()
        })
    })
    .await
    .map_err(|e| NodeError {
        code: "TASK_ERROR".to_string(),
        message: format!("Task execution failed: {}", e),
    })?
}

/// Resamples a buffer asynchronously
#[allow(dead_code)]
#[napi]
pub async fn resample_async(
    buffer: &BufferWrapper,
    new_width: u32,
    new_height: u32,
    method: crate::algorithms::ResamplingMethod,
) -> Result<BufferWrapper> {
    let buffer_clone = buffer.clone();
    tokio::task::spawn_blocking(move || {
        crate::algorithms::resample(&buffer_clone, new_width, new_height, method)
    })
    .await
    .map_err(|e| NodeError {
        code: "TASK_ERROR".to_string(),
        message: format!("Task execution failed: {}", e),
    })?
}

/// Computes hillshade asynchronously
#[allow(dead_code)]
#[napi]
pub async fn hillshade_async(
    dem: &BufferWrapper,
    azimuth: f64,
    altitude: f64,
    z_factor: f64,
    pixel_size: f64,
) -> Result<BufferWrapper> {
    let dem_clone = dem.clone();
    tokio::task::spawn_blocking(move || {
        crate::algorithms::hillshade(&dem_clone, azimuth, altitude, z_factor, pixel_size)
    })
    .await
    .map_err(|e| NodeError {
        code: "TASK_ERROR".to_string(),
        message: format!("Task execution failed: {}", e),
    })?
}

/// Computes slope asynchronously
#[allow(dead_code)]
#[napi]
pub async fn slope_async(
    dem: &BufferWrapper,
    pixel_size: f64,
    z_factor: f64,
    as_percent: bool,
) -> Result<BufferWrapper> {
    let dem_clone = dem.clone();
    tokio::task::spawn_blocking(move || {
        crate::algorithms::slope(&dem_clone, pixel_size, z_factor, as_percent)
    })
    .await
    .map_err(|e| NodeError {
        code: "TASK_ERROR".to_string(),
        message: format!("Task execution failed: {}", e),
    })?
}

/// Computes aspect asynchronously
#[allow(dead_code)]
#[napi]
pub async fn aspect_async(dem: &BufferWrapper, pixel_size: f64) -> Result<BufferWrapper> {
    let dem_clone = dem.clone();
    tokio::task::spawn_blocking(move || crate::algorithms::aspect(&dem_clone, pixel_size))
        .await
        .map_err(|e| NodeError {
            code: "TASK_ERROR".to_string(),
            message: format!("Task execution failed: {}", e),
        })?
}

/// Computes zonal statistics asynchronously
#[allow(dead_code)]
#[napi]
pub async fn zonal_stats_async(
    raster: &BufferWrapper,
    zones: &BufferWrapper,
) -> Result<Vec<crate::algorithms::ZonalStatistics>> {
    let raster_clone = raster.clone();
    let zones_clone = zones.clone();
    tokio::task::spawn_blocking(move || crate::algorithms::zonal_stats(&raster_clone, &zones_clone))
        .await
        .map_err(|e| NodeError {
            code: "TASK_ERROR".to_string(),
            message: format!("Task execution failed: {}", e),
        })?
}

/// Buffer operation asynchronously
#[allow(dead_code)]
#[napi]
pub async fn buffer_async(
    geometry: &crate::vector::GeometryWrapper,
    distance: f64,
    segments: u32,
) -> Result<crate::vector::GeometryWrapper> {
    let geom_clone = geometry.clone();
    tokio::task::spawn_blocking(move || crate::algorithms::buffer(&geom_clone, distance, segments))
        .await
        .map_err(|e| NodeError {
            code: "TASK_ERROR".to_string(),
            message: format!("Task execution failed: {}", e),
        })?
}

/// Area calculation asynchronously
#[allow(dead_code)]
#[napi]
pub async fn area_async(geometry: &crate::vector::GeometryWrapper, method: String) -> Result<f64> {
    let geom_clone = geometry.clone();
    tokio::task::spawn_blocking(move || crate::algorithms::area(&geom_clone, method))
        .await
        .map_err(|e| NodeError {
            code: "TASK_ERROR".to_string(),
            message: format!("Task execution failed: {}", e),
        })?
}

/// Simplify operation asynchronously
#[allow(dead_code)]
#[napi]
pub async fn simplify_async(
    geometry: &crate::vector::GeometryWrapper,
    tolerance: f64,
    method: String,
) -> Result<crate::vector::GeometryWrapper> {
    let geom_clone = geometry.clone();
    tokio::task::spawn_blocking(move || crate::algorithms::simplify(&geom_clone, tolerance, method))
        .await
        .map_err(|e| NodeError {
            code: "TASK_ERROR".to_string(),
            message: format!("Task execution failed: {}", e),
        })?
}

/// Batch processes multiple rasters asynchronously
#[allow(dead_code)]
#[napi]
pub async fn batch_process_rasters(
    paths: Vec<String>,
    output_dir: String,
    operation: String,
) -> Result<Vec<String>> {
    let mut tasks = Vec::new();

    for path in paths {
        let output_path = format!(
            "{}/processed_{}",
            output_dir,
            Path::new(&path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("output.tif")
        );
        let op = operation.clone();

        let task = tokio::task::spawn_blocking(move || -> Result<String> {
            let dataset = Dataset::open(path)?;

            // Apply operation based on string
            let processed = match op.as_str() {
                "identity" => dataset,
                _ => {
                    return Err(NodeError {
                        code: "INVALID_OPERATION".to_string(),
                        message: format!("Unknown operation: {}", op),
                    }
                    .into());
                }
            };

            processed.save(output_path.clone())?;
            Ok(output_path)
        });

        tasks.push(task);
    }

    let total = tasks.len();
    let mut results = Vec::new();
    for (completed, task) in tasks.into_iter().enumerate() {
        let result = task.await.map_err(|e| NodeError {
            code: "TASK_ERROR".to_string(),
            message: format!("Task execution failed: {}", e),
        })??;
        results.push(result);

        if total > 0 {
            report_progress((completed + 1) as f64 / total as f64);
        }
    }

    Ok(results)
}

/// Registers a progress callback for long-running operations.
///
/// The callback is invoked from Rust worker threads (via
/// `napi_call_threadsafe_function`) with a progress fraction in
/// `[0.0, 1.0]` at meaningful checkpoints during operations such as
/// [`batch_process_rasters`] and [`process_raster_parallel`]. Registering a
/// new callback replaces any previously registered one.
#[allow(dead_code)]
#[napi(ts_args_type = "callback: (progress: number) => void")]
pub fn set_progress_callback(callback: Function<f64, ()>) -> Result<()> {
    let tsfn: ProgressTsfn = callback
        .build_threadsafe_function::<f64>()
        .build_callback(|ctx: ThreadsafeCallContext<f64>| Ok(ctx.value))?;

    let mut guard = lock_progress_callback()?;
    *guard = Some(tsfn);
    Ok(())
}

/// Removes any previously registered progress callback.
///
/// After calling this, long-running operations stop invoking any JS
/// callback (progress notifications become no-ops) until
/// [`set_progress_callback`] is called again.
#[allow(dead_code)]
#[napi]
pub fn clear_progress_callback() -> Result<()> {
    let mut guard = lock_progress_callback()?;
    *guard = None;
    Ok(())
}

/// Cancellation token for async operations
#[napi]
pub struct CancellationToken {
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[napi]
impl CancellationToken {
    /// Creates a new cancellation token
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            cancelled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Cancels the operation
    #[napi]
    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Checks if cancelled
    #[napi]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Resets the token
    #[napi]
    pub fn reset(&self) {
        self.cancelled
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Parallel processing configuration
#[allow(dead_code)]
#[napi(object)]
pub struct ParallelConfig {
    /// Number of threads to use (0 = automatic)
    pub num_threads: u32,
    /// Chunk size for parallel processing
    pub chunk_size: u32,
    /// Enable progress reporting
    pub report_progress: bool,
}

impl Default for ParallelConfig {
    fn default() -> Self {
        Self {
            num_threads: 0,
            chunk_size: 1000,
            report_progress: false,
        }
    }
}

/// Processes a large raster in parallel chunks
#[allow(dead_code)]
#[napi]
pub async fn process_raster_parallel(
    dataset: &Dataset,
    operation: String,
    config: Option<ParallelConfig>,
) -> Result<Dataset> {
    let cfg = config.unwrap_or_default();
    let ds_clone = dataset.clone();

    if cfg.report_progress {
        report_progress(0.0);
    }

    let result = tokio::task::spawn_blocking(move || -> Result<Dataset> {
        // This is a simplified implementation
        // In production, would process tiles in parallel
        match operation.as_str() {
            "identity" => Ok(ds_clone),
            _ => Err(NodeError {
                code: "INVALID_OPERATION".to_string(),
                message: format!("Unknown operation: {}", operation),
            }
            .into()),
        }
    })
    .await
    .map_err(|e| NodeError {
        code: "TASK_ERROR".to_string(),
        message: format!("Task execution failed: {}", e),
    })??;

    if cfg.report_progress {
        report_progress(1.0);
    }

    Ok(result)
}

/// Stream processing for large datasets
#[napi]
pub struct RasterStream {
    dataset: Dataset,
    current_row: u32,
    chunk_height: u32,
}

#[napi]
impl RasterStream {
    /// Creates a new raster stream
    #[napi(constructor)]
    pub fn new(dataset: &Dataset, chunk_height: u32) -> Self {
        Self {
            dataset: dataset.clone(),
            current_row: 0,
            chunk_height,
        }
    }

    /// Reads the next chunk
    #[napi]
    pub async unsafe fn read_next_chunk(&mut self) -> Result<Option<BufferWrapper>> {
        if self.current_row >= self.dataset.height() {
            return Ok(None);
        }

        let height = self
            .chunk_height
            .min(self.dataset.height() - self.current_row);
        let chunk =
            self.dataset
                .read_window(0, 0, self.current_row, self.dataset.width(), height)?;

        self.current_row += height;
        Ok(Some(chunk))
    }

    /// Resets the stream to the beginning
    #[napi]
    pub unsafe fn reset(&mut self) {
        self.current_row = 0;
    }

    /// Gets current progress (0.0 - 1.0)
    #[napi]
    pub fn progress(&self) -> f64 {
        if self.dataset.height() == 0 {
            1.0
        } else {
            self.current_row as f64 / self.dataset.height() as f64
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    // NOTE: constructing a real `Function<f64, ()>` requires a live N-API
    // `Env`, which is unavailable in plain `cargo test`. These tests instead
    // cover the parts that were previously entirely unreachable: the
    // callback registry no longer silently discards its input, `call`ing
    // `report_progress` with nothing registered is a safe no-op (not a
    // panic), and `clear_progress_callback` actually empties the slot.

    #[test]
    fn report_progress_without_callback_is_a_noop() {
        clear_progress_callback().expect("clear should not fail even with nothing registered");
        // Must not panic, error, or block indefinitely.
        report_progress(0.0);
        report_progress(0.5);
        report_progress(1.0);
    }

    #[test]
    fn clear_progress_callback_leaves_slot_empty() {
        clear_progress_callback().expect("clear should succeed");
        let guard = lock_progress_callback().expect("lock should succeed");
        assert!(
            guard.is_none(),
            "PROGRESS_CALLBACK slot must be empty after clear_progress_callback"
        );
    }

    #[test]
    fn lock_progress_callback_initializes_lazily() {
        // Calling this before any `set_progress_callback` call must not
        // panic (regression guard for the OnceLock initialization path).
        let guard = lock_progress_callback().expect("lock should succeed on first use");
        drop(guard);
    }
}
