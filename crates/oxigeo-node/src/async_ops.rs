//! Async operations for Node.js bindings
//!
//! This module provides Promise-based async operations for I/O and processing.

use napi::bindgen_prelude::*;
use napi::threadsafe_function::{
    ThreadsafeCallContext, ThreadsafeFunction, ThreadsafeFunctionCallMode,
};
use napi::tokio;
use napi_derive::napi;
use oxigeo_core::buffer::RasterBuffer;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

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

/// Batch processes multiple rasters asynchronously.
///
/// Each input raster is opened, has `operation` (one of the per-pixel
/// transforms `identity`, `abs`, `negate`, `square`, `sqrt`) applied to every
/// band, and is written to `output_dir/processed_<name>`. Files are processed
/// concurrently on the blocking thread pool.
///
/// If a [`CancellationToken`] is supplied and cancelled, files not yet started
/// are skipped and the call returns a `CANCELLED` error (already-written
/// outputs are left in place).
#[allow(dead_code)]
#[napi]
pub async fn batch_process_rasters(
    paths: Vec<String>,
    output_dir: String,
    operation: String,
    token: Option<&CancellationToken>,
) -> Result<Vec<String>> {
    // Validate the operation once, up front, so an unknown operation fails fast
    // instead of after opening files.
    let op = PixelOp::parse(&operation)?;
    let cancel = token.map(CancellationToken::flag);

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
        let task_cancel = cancel.clone();

        let task = tokio::task::spawn_blocking(move || -> Result<Option<String>> {
            // Skip files that have not started once cancellation is observed.
            if task_cancel
                .as_ref()
                .is_some_and(|c| c.load(Ordering::SeqCst))
            {
                return Ok(None);
            }

            let dataset = Dataset::open(path)?;
            // Single-threaded per file (files are already processed
            // concurrently); chunk_size 0 == whole-band.
            let processed =
                apply_operation_parallel(&dataset, op, 0, 1, task_cancel.clone(), false)?;
            processed.save(output_path.clone())?;
            Ok(Some(output_path))
        });

        tasks.push(task);
    }

    let total = tasks.len();
    let mut results = Vec::new();
    let mut cancelled = false;
    for (completed, task) in tasks.into_iter().enumerate() {
        let result = task.await.map_err(|e| NodeError {
            code: "TASK_ERROR".to_string(),
            message: format!("Task execution failed: {}", e),
        })??;

        match result {
            Some(path) => results.push(path),
            None => cancelled = true,
        }

        if total > 0 {
            report_progress((completed + 1) as f64 / total as f64);
        }
    }

    if cancelled {
        return Err(NodeError {
            code: "CANCELLED".to_string(),
            message: "Batch processing was cancelled".to_string(),
        }
        .into());
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

impl CancellationToken {
    /// Returns a shared handle to the underlying cancellation flag so that a
    /// worker running on another thread (inside `spawn_blocking`) can poll it
    /// between chunks/files and abort cooperatively (crate-internal).
    pub(crate) fn flag(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        std::sync::Arc::clone(&self.cancelled)
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

/// A pixel-wise (row-independent) operation that the parallel/batch raster
/// processors can apply band-by-band.
///
/// Every variant is a pure function of a single pixel value, which is what
/// makes the chunked, multi-threaded execution below correct: horizontal
/// chunks never need neighbouring rows, so they can be computed fully
/// independently and reassembled without seams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PixelOp {
    /// Copy the input unchanged.
    Identity,
    /// Absolute value.
    Abs,
    /// Arithmetic negation.
    Negate,
    /// Square the value.
    Square,
    /// Square root (yields NaN for negative inputs, per IEEE-754).
    Sqrt,
}

impl PixelOp {
    /// Parses a JS-facing operation string into a [`PixelOp`], returning an
    /// `INVALID_OPERATION` error for anything unsupported.
    fn parse(operation: &str) -> Result<Self> {
        match operation {
            "identity" => Ok(Self::Identity),
            "abs" => Ok(Self::Abs),
            "negate" => Ok(Self::Negate),
            "square" => Ok(Self::Square),
            "sqrt" => Ok(Self::Sqrt),
            _ => Err(NodeError {
                code: "INVALID_OPERATION".to_string(),
                message: format!(
                    "Unknown operation '{}' (supported: identity, abs, negate, square, sqrt)",
                    operation
                ),
            }
            .into()),
        }
    }

    /// Applies the operation to a single pixel value.
    #[inline]
    fn apply(self, value: f64) -> f64 {
        match self {
            Self::Identity => value,
            Self::Abs => value.abs(),
            Self::Negate => -value,
            Self::Square => value * value,
            Self::Sqrt => value.sqrt(),
        }
    }
}

/// A unit of work: the pixels of one band between two rows.
struct Chunk {
    band: usize,
    y_start: u64,
    y_end: u64,
}

/// Applies a per-pixel operation to every band of `dataset`, splitting each
/// band into `chunk_size`-row chunks and processing them across `num_threads`
/// worker threads.
///
/// - `chunk_size == 0` is treated as "one chunk per band" (whole band).
/// - `num_threads == 0` uses [`std::thread::available_parallelism`].
/// - If `cancel` is set at any point, the work aborts with a `CANCELLED` error
///   rather than returning partial/fake data.
/// - When `report` is true, a progress fraction is emitted after each chunk.
fn apply_operation_parallel(
    dataset: &Dataset,
    op: PixelOp,
    chunk_size: u32,
    num_threads: u32,
    cancel: Option<Arc<AtomicBool>>,
    report: bool,
) -> Result<Dataset> {
    let bands = dataset.bands();

    // Output starts as a copy of the input so per-band dtype/dims/nodata are
    // preserved; the computed values overwrite the pixels below.
    let mut output: Vec<RasterBuffer> = bands.to_vec();

    // Build the chunk list up front so worker threads can steal indices from a
    // shared atomic counter.
    let rows_per_chunk = if chunk_size == 0 {
        u64::MAX
    } else {
        u64::from(chunk_size)
    };
    let mut chunks: Vec<Chunk> = Vec::new();
    for (band_index, band) in bands.iter().enumerate() {
        let height = band.height();
        let mut y = 0u64;
        while y < height {
            let y_end = y.saturating_add(rows_per_chunk).min(height);
            chunks.push(Chunk {
                band: band_index,
                y_start: y,
                y_end,
            });
            y = y_end;
        }
    }

    let total_chunks = chunks.len();
    if total_chunks == 0 {
        return Ok(dataset.with_bands(output));
    }

    let requested_threads = if num_threads == 0 {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    } else {
        num_threads as usize
    };
    let worker_count = requested_threads.clamp(1, total_chunks);

    let next_index = AtomicUsize::new(0);
    let completed = AtomicUsize::new(0);
    let cancel_ref = cancel.as_ref();

    // Each worker returns the chunks it computed as (chunk_index, values); a
    // returned `Err` signals either observed cancellation or a pixel-access
    // error. All reads are through shared `&` references (RasterBuffer is Sync),
    // so no data races are possible.
    #[allow(clippy::type_complexity)]
    let worker_results: Vec<std::result::Result<Vec<(usize, Vec<f64>)>, WorkerError>> =
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..worker_count)
                .map(|_| {
                    scope.spawn(
                        || -> std::result::Result<Vec<(usize, Vec<f64>)>, WorkerError> {
                            let mut local = Vec::new();
                            loop {
                                if cancel_ref.is_some_and(|c| c.load(Ordering::SeqCst)) {
                                    return Err(WorkerError::Cancelled);
                                }
                                let idx = next_index.fetch_add(1, Ordering::SeqCst);
                                if idx >= total_chunks {
                                    break;
                                }
                                let chunk = &chunks[idx];
                                let band = &bands[chunk.band];
                                let width = band.width();
                                let mut values = Vec::with_capacity(
                                    (width * (chunk.y_end - chunk.y_start)) as usize,
                                );
                                for y in chunk.y_start..chunk.y_end {
                                    for x in 0..width {
                                        let raw = band
                                            .get_pixel(x, y)
                                            .map_err(|e| WorkerError::Pixel(e.to_string()))?;
                                        values.push(op.apply(raw));
                                    }
                                }
                                local.push((idx, values));

                                if report {
                                    let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
                                    report_progress(done as f64 / total_chunks as f64);
                                }
                            }
                            Ok(local)
                        },
                    )
                })
                .collect();

            handles
                .into_iter()
                .map(|h| {
                    h.join().unwrap_or(Err(WorkerError::Pixel(
                        "worker thread panicked".to_string(),
                    )))
                })
                .collect()
        });

    // Reassemble: fold every worker's chunks back into the output bands. Any
    // error (cancellation or pixel access) aborts without producing data.
    for result in worker_results {
        let local = match result {
            Ok(local) => local,
            Err(WorkerError::Cancelled) => {
                return Err(NodeError {
                    code: "CANCELLED".to_string(),
                    message: "Operation was cancelled".to_string(),
                }
                .into());
            }
            Err(WorkerError::Pixel(message)) => {
                return Err(NodeError {
                    code: "PROCESSING_ERROR".to_string(),
                    message,
                }
                .into());
            }
        };

        for (chunk_index, values) in local {
            let chunk = &chunks[chunk_index];
            let band = &mut output[chunk.band];
            let width = band.width();
            let mut cursor = 0usize;
            for y in chunk.y_start..chunk.y_end {
                for x in 0..width {
                    band.set_pixel(x, y, values[cursor])
                        .map_err(|e| NodeError {
                            code: "PROCESSING_ERROR".to_string(),
                            message: e.to_string(),
                        })?;
                    cursor += 1;
                }
            }
        }
    }

    Ok(dataset.with_bands(output))
}

/// Internal error raised by a parallel worker thread.
enum WorkerError {
    /// The shared cancellation flag was observed to be set.
    Cancelled,
    /// A pixel read failed (carries the underlying error message).
    Pixel(String),
}

/// Processes a large raster in parallel chunks.
///
/// Splits every band into `chunk_size`-row chunks and applies `operation`
/// across `num_threads` worker threads (see [`ParallelConfig`]). Supported
/// operations are the per-pixel transforms `identity`, `abs`, `negate`,
/// `square`, and `sqrt`.
///
/// When a [`CancellationToken`] is supplied and cancelled while the work is in
/// flight, the operation aborts with a `CANCELLED` error instead of returning a
/// partially processed dataset.
#[allow(dead_code)]
#[napi]
pub async fn process_raster_parallel(
    dataset: &Dataset,
    operation: String,
    config: Option<ParallelConfig>,
    token: Option<&CancellationToken>,
) -> Result<Dataset> {
    let cfg = config.unwrap_or_default();
    let ds_clone = dataset.clone();
    let op = PixelOp::parse(&operation)?;
    let cancel = token.map(CancellationToken::flag);

    if cfg.report_progress {
        report_progress(0.0);
    }

    let result = tokio::task::spawn_blocking(move || -> Result<Dataset> {
        apply_operation_parallel(
            &ds_clone,
            op,
            cfg.chunk_size,
            cfg.num_threads,
            cancel,
            cfg.report_progress,
        )
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
    ///
    /// # Safety
    ///
    /// `napi-rs` requires `async` methods taking `&mut self` to be declared
    /// `unsafe`: the mutable borrow must stay valid across the `.await`
    /// suspension point, which the JS event loop cannot enforce statically.
    /// The generated N-API binding always awaits each call to completion
    /// before the next one runs, so this holds under normal sequential use;
    /// callers must not race a second call into the same `RasterStream`
    /// (e.g. via `Promise.all`) against an in-flight `read_next_chunk`/`reset`.
    #[napi]
    #[allow(unsafe_code)]
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
    pub fn reset(&mut self) {
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

    /// Builds a two-band float32 dataset where band `b` holds `x + y + b*100`.
    fn sample_dataset(width: u32, height: u32) -> Dataset {
        let mut ds = Dataset::create(width, height, 2, "float32".to_string())
            .expect("create dataset for test");
        for band in 0..2u32 {
            let mut buf = crate::buffer::BufferWrapper::new(width, height, "float32".to_string())
                .expect("create band buffer");
            for y in 0..height {
                for x in 0..width {
                    buf.set_pixel(x, y, (x + y + band * 100) as f64)
                        .expect("set pixel");
                }
            }
            ds.write_band(band, &buf).expect("write band");
        }
        ds
    }

    #[test]
    fn pixel_op_parse_rejects_unknown() {
        assert!(PixelOp::parse("identity").is_ok());
        assert!(PixelOp::parse("sqrt").is_ok());
        assert!(PixelOp::parse("bogus").is_err());
    }

    #[test]
    fn parallel_square_transforms_every_band_and_pixel() {
        let ds = sample_dataset(10, 7);
        // chunk_size 3 forces multiple chunks per band; 4 workers exercises the
        // work-stealing path.
        let result = apply_operation_parallel(&ds, PixelOp::Square, 3, 4, None, false)
            .expect("square should succeed");

        for band in 0..2u32 {
            let out = result.read_band(band).expect("read band");
            for y in 0..7u32 {
                for x in 0..10u32 {
                    let input = (x + y + band * 100) as f64;
                    let expected = input * input;
                    let actual = out.get_pixel(x, y).expect("get pixel");
                    assert!(
                        (actual - expected).abs() < 1e-3,
                        "band {band} ({x},{y}): expected {expected}, got {actual}"
                    );
                }
            }
        }
    }

    #[test]
    fn parallel_identity_is_a_faithful_copy() {
        let ds = sample_dataset(5, 5);
        let result = apply_operation_parallel(&ds, PixelOp::Identity, 0, 0, None, false)
            .expect("identity should succeed");
        for band in 0..2u32 {
            let out = result.read_band(band).expect("read band");
            for y in 0..5u32 {
                for x in 0..5u32 {
                    let expected = (x + y + band * 100) as f64;
                    let actual = out.get_pixel(x, y).expect("get pixel");
                    assert!((actual - expected).abs() < f64::EPSILON);
                }
            }
        }
    }

    #[test]
    fn parallel_operation_honors_prior_cancellation() {
        let ds = sample_dataset(16, 16);
        let flag = Arc::new(AtomicBool::new(true)); // already cancelled
        let err = apply_operation_parallel(&ds, PixelOp::Abs, 2, 4, Some(flag), false);
        match err {
            Err(e) => assert!(
                e.to_string().contains("CANCELLED"),
                "expected CANCELLED error, got {e}"
            ),
            Ok(_) => panic!("expected cancellation to abort processing"),
        }
    }

    #[test]
    fn cancellation_token_flag_reflects_state() {
        let token = CancellationToken::new();
        let flag = token.flag();
        assert!(!flag.load(Ordering::SeqCst));
        token.cancel();
        assert!(
            flag.load(Ordering::SeqCst),
            "shared flag must observe cancellation through the token"
        );
        token.reset();
        assert!(!flag.load(Ordering::SeqCst));
    }
}
