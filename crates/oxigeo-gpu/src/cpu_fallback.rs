//! Automatic CPU fallback for GPU operations.
//!
//! This module provides a generic fallback system that transparently routes
//! GPU operations to CPU implementations when GPU errors such as device loss
//! or backend unavailability are encountered.
//!
//! # Design
//!
//! The core abstraction is [`execute_with_fallback`], a free function that
//! accepts a GPU closure and a CPU closure.  When the GPU closure returns an
//! error whose kind matches the active [`FallbackConfig`], the CPU closure is
//! invoked instead and its result is returned as if it had come from the GPU.
//!
//! [`FallbackContext`] wraps a [`GpuContext`] together with a `FallbackConfig`
//! and additionally checks the device-lost flag *before* even attempting the
//! GPU closure, avoiding redundant error generation on a known-dead device.
//!
//! # CPU primitive operations
//!
//! The [`cpu`] sub-module contains pure-Rust implementations of common raster
//! element-wise operations used as CPU fallback kernels.

use std::sync::Arc;

use crate::{GpuContext, GpuError, GpuResult};

// ---------------------------------------------------------------------------
// FallbackConfig
// ---------------------------------------------------------------------------

/// Configuration that controls when and how CPU fallback is activated.
///
/// By default fallback is enabled and will trigger on [`GpuError::DeviceLost`]
/// and on "no backend" adapter-not-found errors.
#[derive(Debug, Clone)]
pub struct FallbackConfig {
    /// Whether automatic CPU fallback is enabled at all.
    ///
    /// When `false`, GPU errors are always propagated and `cpu_op` is never
    /// called.
    pub enabled: bool,

    /// Trigger fallback when the GPU error is [`GpuError::DeviceLost`].
    pub trigger_on_device_lost: bool,

    /// Trigger fallback when the GPU error indicates no adapter / no backend
    /// is available (e.g. [`GpuError::NoAdapter`] or
    /// [`GpuError::BackendNotAvailable`]).
    pub trigger_on_no_backend: bool,

    /// If `Some(ms)`, GPU operations that exceed this wall-clock duration will
    /// fall back to CPU.  The timeout is implemented at the *caller* level via
    /// [`execute_with_fallback_timed`]; the free function
    /// [`execute_with_fallback`] ignores this field.
    pub timeout_ms: Option<u64>,

    /// Emit a diagnostic message to `stderr` when falling back to CPU.
    pub log_on_fallback: bool,
}

impl Default for FallbackConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            trigger_on_device_lost: true,
            trigger_on_no_backend: true,
            timeout_ms: None,
            log_on_fallback: false,
        }
    }
}

impl FallbackConfig {
    /// Create a configuration with fallback disabled entirely.
    ///
    /// GPU errors are always propagated; the CPU closure is never called.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }

    /// Create a configuration with verbose logging enabled.
    pub fn with_logging(mut self) -> Self {
        self.log_on_fallback = true;
        self
    }

    /// Set an optional timeout in milliseconds.
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    /// Decide whether `err` should trigger a fallback to CPU.
    ///
    /// Returns `false` whenever `self.enabled == false`, regardless of the
    /// error kind.
    pub fn should_fallback(&self, err: &GpuError) -> bool {
        if !self.enabled {
            return false;
        }

        // Device-lost family
        if self.trigger_on_device_lost && matches!(err, GpuError::DeviceLost { .. }) {
            return true;
        }

        // No-adapter / no-backend family — covers both the structured variants
        // and any message-level descriptions produced by older code paths.
        if self.trigger_on_no_backend {
            let is_no_adapter = matches!(err, GpuError::NoAdapter { .. });
            let is_backend_na = matches!(err, GpuError::BackendNotAvailable { .. });
            // String-based heuristic for error messages that don't map to a
            // single variant (e.g. wgpu internal messages surfaced as Internal).
            let msg = err.to_string();
            let is_msg_match = msg.contains("No wgpu backend")
                || msg.contains("no backend")
                || msg.contains("no adapter");
            if is_no_adapter || is_backend_na || is_msg_match {
                return true;
            }
        }

        false
    }
}

// ---------------------------------------------------------------------------
// ExecutionPath + FallbackResult
// ---------------------------------------------------------------------------

/// Which execution path produced the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExecutionPath {
    /// The GPU operation completed successfully.
    Gpu,
    /// The CPU fallback was used (GPU was unavailable or returned a
    /// fallback-triggering error).
    Cpu,
}

/// The output of a fallback-aware operation, bundling the value with the
/// execution path that produced it.
#[derive(Debug)]
pub struct FallbackResult<T> {
    /// The computed value.
    pub value: T,
    /// Which path (GPU or CPU) produced the value.
    pub path: ExecutionPath,
}

impl<T: Clone> Clone for FallbackResult<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            path: self.path,
        }
    }
}

// ---------------------------------------------------------------------------
// Core free functions
// ---------------------------------------------------------------------------

/// Execute `gpu_op` and, if it returns a fallback-triggering error, run
/// `cpu_op` instead.
///
/// # Behaviour
///
/// | GPU result | `config.enabled` | Action |
/// |---|---|---|
/// | `Ok(v)` | any | return `FallbackResult { value: v, path: Gpu }` |
/// | fallback-triggering error | `true` | invoke `cpu_op`, return `Cpu` path |
/// | fallback-triggering error | `false` | propagate the GPU error |
/// | non-fallback error | any | propagate the GPU error unchanged |
///
/// # Examples
///
/// ```rust
/// use oxigeo_gpu::cpu_fallback::{FallbackConfig, ExecutionPath, execute_with_fallback};
/// use oxigeo_gpu::GpuError;
///
/// let cfg = FallbackConfig::default();
/// let result = execute_with_fallback(
///     &cfg,
///     || Err::<u32, GpuError>(GpuError::device_lost("test")),
///     || 42_u32,
/// ).unwrap();
///
/// assert_eq!(result.value, 42);
/// assert_eq!(result.path, ExecutionPath::Cpu);
/// ```
pub fn execute_with_fallback<T, GpuFn, CpuFn>(
    config: &FallbackConfig,
    gpu_op: GpuFn,
    cpu_op: CpuFn,
) -> GpuResult<FallbackResult<T>>
where
    GpuFn: FnOnce() -> GpuResult<T>,
    CpuFn: FnOnce() -> T,
{
    match gpu_op() {
        Ok(value) => Ok(FallbackResult {
            value,
            path: ExecutionPath::Gpu,
        }),
        Err(err) if config.should_fallback(&err) => {
            if config.log_on_fallback {
                eprintln!("[oxigeo-gpu] GPU op failed ({err}), falling back to CPU");
            }
            Ok(FallbackResult {
                value: cpu_op(),
                path: ExecutionPath::Cpu,
            })
        }
        Err(err) => Err(err),
    }
}

/// Like [`execute_with_fallback`], but additionally checks whether the GPU op
/// takes longer than `config.timeout_ms` milliseconds.  If the timeout fires
/// before the GPU closure returns, `cpu_op` is called and `ExecutionPath::Cpu`
/// is reported.
///
/// When `config.timeout_ms` is `None` this function behaves identically to
/// [`execute_with_fallback`].
///
/// # Note
///
/// The GPU closure is executed on a background thread and its result is
/// forwarded over a channel.  A `recv_timeout` call on the receiving end
/// implements the deadline.  If the deadline fires the CPU closure is called
/// on the calling thread; the background GPU thread continues to completion
/// and its result is silently discarded.
///
/// The GPU closure must be `Send + 'static` so it can be moved into the
/// background thread.
pub fn execute_with_fallback_timed<T, GpuFn, CpuFn>(
    config: &FallbackConfig,
    gpu_op: GpuFn,
    cpu_op: CpuFn,
) -> GpuResult<FallbackResult<T>>
where
    T: Send + 'static,
    GpuFn: FnOnce() -> GpuResult<T> + Send + 'static,
    CpuFn: FnOnce() -> T,
{
    let timeout = match config.timeout_ms {
        None => return execute_with_fallback(config, gpu_op, cpu_op),
        Some(ms) => std::time::Duration::from_millis(ms),
    };

    let (tx, rx) = std::sync::mpsc::channel::<GpuResult<T>>();
    std::thread::spawn(move || {
        // Ignore send errors — the receiver may have timed out and dropped.
        let _ = tx.send(gpu_op());
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(value)) => Ok(FallbackResult {
            value,
            path: ExecutionPath::Gpu,
        }),
        Ok(Err(err)) if config.should_fallback(&err) => {
            if config.log_on_fallback {
                eprintln!("[oxigeo-gpu] GPU op failed ({err}), falling back to CPU");
            }
            Ok(FallbackResult {
                value: cpu_op(),
                path: ExecutionPath::Cpu,
            })
        }
        Ok(Err(err)) => Err(err),
        // Timeout or sender dropped — fall back to CPU.
        Err(_recv_err) => {
            if config.log_on_fallback {
                eprintln!(
                    "[oxigeo-gpu] GPU op timed out after {}ms, falling back to CPU",
                    timeout.as_millis()
                );
            }
            Ok(FallbackResult {
                value: cpu_op(),
                path: ExecutionPath::Cpu,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// FallbackContext
// ---------------------------------------------------------------------------

/// A [`GpuContext`] wrapper that carries a [`FallbackConfig`] and provides
/// fallback-aware operation dispatch.
///
/// Before invoking the GPU closure, `FallbackContext` checks the device-lost
/// flag.  If the device is already known to be lost, it routes directly to the
/// CPU closure without attempting any GPU work.
///
/// # Example
///
/// ```rust,no_run
/// use oxigeo_gpu::{GpuContext, GpuError};
/// use oxigeo_gpu::cpu_fallback::{FallbackConfig, FallbackContext, ExecutionPath};
///
/// # async fn example() -> Result<(), GpuError> {
/// let gpu = GpuContext::new().await?;
/// let fb = FallbackContext::with_default_config(gpu);
///
/// let result = fb.execute(
///     || Ok::<u32, GpuError>(1_u32),
///     || 2_u32,
/// )?;
///
/// assert_eq!(result.path, ExecutionPath::Gpu);
/// # Ok(())
/// # }
/// ```
pub struct FallbackContext {
    inner: Arc<GpuContext>,
    config: FallbackConfig,
}

impl FallbackContext {
    /// Create a new `FallbackContext` with the supplied configuration.
    pub fn new(ctx: GpuContext, config: FallbackConfig) -> Self {
        Self {
            inner: Arc::new(ctx),
            config,
        }
    }

    /// Create a `FallbackContext` with the default [`FallbackConfig`].
    pub fn with_default_config(ctx: GpuContext) -> Self {
        Self::new(ctx, FallbackConfig::default())
    }

    /// Return a reference to the underlying [`GpuContext`].
    pub fn gpu_context(&self) -> &GpuContext {
        &self.inner
    }

    /// Return a reference to the active [`FallbackConfig`].
    pub fn fallback_config(&self) -> &FallbackConfig {
        &self.config
    }

    /// Return `true` if the GPU device has been lost.
    pub fn is_device_lost(&self) -> bool {
        self.inner.is_device_lost()
    }

    /// Execute `gpu_op` with automatic CPU fallback per the configured policy.
    ///
    /// If the device-lost flag is already set, the GPU closure is skipped
    /// entirely and `cpu_op` is called directly (provided fallback is enabled).
    /// Otherwise the behaviour mirrors [`execute_with_fallback`].
    pub fn execute<T, GpuFn, CpuFn>(
        &self,
        gpu_op: GpuFn,
        cpu_op: CpuFn,
    ) -> GpuResult<FallbackResult<T>>
    where
        GpuFn: FnOnce() -> GpuResult<T>,
        CpuFn: FnOnce() -> T,
    {
        if self.inner.is_device_lost() {
            if self.config.enabled {
                if self.config.log_on_fallback {
                    eprintln!("[oxigeo-gpu] Device already lost, running CPU directly");
                }
                return Ok(FallbackResult {
                    value: cpu_op(),
                    path: ExecutionPath::Cpu,
                });
            }
            return Err(GpuError::device_lost("device was lost"));
        }
        execute_with_fallback(&self.config, gpu_op, cpu_op)
    }
}

impl std::fmt::Debug for FallbackContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FallbackContext")
            .field("config", &self.config)
            .field("device_lost", &self.inner.is_device_lost())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// CPU fallback primitive operations
// ---------------------------------------------------------------------------

/// Pure-Rust CPU implementations of common raster element-wise operations.
///
/// These are used as CPU fallback kernels when the GPU is unavailable.
/// All functions operate on `&[f32]` slices and return owned `Vec<f32>`.
///
/// When the input slices have different lengths the shorter slice determines
/// how many elements are processed (same semantics as `Iterator::zip`).
pub mod cpu {
    /// Element-wise addition: `result[i] = a[i] + b[i]`.
    pub fn add_slices(a: &[f32], b: &[f32]) -> Vec<f32> {
        a.iter().zip(b.iter()).map(|(x, y)| x + y).collect()
    }

    /// Element-wise subtraction: `result[i] = a[i] - b[i]`.
    pub fn sub_slices(a: &[f32], b: &[f32]) -> Vec<f32> {
        a.iter().zip(b.iter()).map(|(x, y)| x - y).collect()
    }

    /// Element-wise multiplication: `result[i] = a[i] * b[i]`.
    pub fn mul_slices(a: &[f32], b: &[f32]) -> Vec<f32> {
        a.iter().zip(b.iter()).map(|(x, y)| x * y).collect()
    }

    /// Element-wise division: `result[i] = a[i] / b[i]`.
    ///
    /// Division by zero follows IEEE 754 semantics (produces `±inf` or `NaN`).
    pub fn div_slices(a: &[f32], b: &[f32]) -> Vec<f32> {
        a.iter().zip(b.iter()).map(|(x, y)| x / y).collect()
    }

    /// Element-wise maximum: `result[i] = a[i].max(b[i])`.
    pub fn max_slices(a: &[f32], b: &[f32]) -> Vec<f32> {
        a.iter().zip(b.iter()).map(|(x, y)| x.max(*y)).collect()
    }

    /// Element-wise minimum: `result[i] = a[i].min(b[i])`.
    pub fn min_slices(a: &[f32], b: &[f32]) -> Vec<f32> {
        a.iter().zip(b.iter()).map(|(x, y)| x.min(*y)).collect()
    }

    /// Scalar addition: `result[i] = data[i] + scalar`.
    pub fn add_scalar(data: &[f32], scalar: f32) -> Vec<f32> {
        data.iter().map(|x| x + scalar).collect()
    }

    /// Scalar subtraction: `result[i] = data[i] - scalar`.
    pub fn sub_scalar(data: &[f32], scalar: f32) -> Vec<f32> {
        data.iter().map(|x| x - scalar).collect()
    }

    /// Scalar multiplication: `result[i] = data[i] * scalar`.
    pub fn mul_scalar(data: &[f32], scalar: f32) -> Vec<f32> {
        data.iter().map(|x| x * scalar).collect()
    }

    /// Scalar division: `result[i] = data[i] / scalar`.
    ///
    /// Division by zero follows IEEE 754 semantics.
    pub fn div_scalar(data: &[f32], scalar: f32) -> Vec<f32> {
        data.iter().map(|x| x / scalar).collect()
    }

    /// Clamp all elements to `[lo, hi]`: `result[i] = data[i].clamp(lo, hi)`.
    pub fn clamp(data: &[f32], lo: f32, hi: f32) -> Vec<f32> {
        data.iter().map(|x| x.clamp(lo, hi)).collect()
    }

    /// Absolute value: `result[i] = data[i].abs()`.
    pub fn abs(data: &[f32]) -> Vec<f32> {
        data.iter().map(|x| x.abs()).collect()
    }

    /// Square root: `result[i] = data[i].sqrt()`.
    pub fn sqrt(data: &[f32]) -> Vec<f32> {
        data.iter().map(|x| x.sqrt()).collect()
    }

    /// Power: `result[i] = data[i].powf(exponent)`.
    pub fn powf(data: &[f32], exponent: f32) -> Vec<f32> {
        data.iter().map(|x| x.powf(exponent)).collect()
    }

    // -----------------------------------------------------------------------
    // Reductions
    // -----------------------------------------------------------------------

    /// Arithmetic mean of all elements.  Returns `0.0` for empty slices.
    pub fn mean(data: &[f32]) -> f32 {
        if data.is_empty() {
            return 0.0;
        }
        data.iter().sum::<f32>() / data.len() as f32
    }

    /// Sum of all elements.  Returns `0.0` for empty slices.
    pub fn sum(data: &[f32]) -> f32 {
        data.iter().sum()
    }

    /// Minimum element value.  Returns [`f32::MAX`] for empty slices.
    pub fn min_value(data: &[f32]) -> f32 {
        data.iter().cloned().fold(f32::MAX, f32::min)
    }

    /// Maximum element value.  Returns [`f32::MIN`] for empty slices.
    pub fn max_value(data: &[f32]) -> f32 {
        data.iter().cloned().fold(f32::MIN, f32::max)
    }

    /// Population variance of all elements.  Returns `0.0` for slices with
    /// fewer than two elements.
    pub fn variance(data: &[f32]) -> f32 {
        if data.len() < 2 {
            return 0.0;
        }
        let m = mean(data);
        data.iter().map(|x| (x - m) * (x - m)).sum::<f32>() / data.len() as f32
    }

    /// Population standard deviation.  Returns `0.0` for slices with fewer
    /// than two elements.
    pub fn std_dev(data: &[f32]) -> f32 {
        variance(data).sqrt()
    }

    // -----------------------------------------------------------------------
    // NDVI helper
    // -----------------------------------------------------------------------

    /// Normalised Difference Vegetation Index: `(nir - red) / (nir + red)`.
    ///
    /// Pixels where `nir + red == 0` produce `0.0` rather than `NaN` /
    /// infinity, matching the common no-data convention.
    pub fn ndvi(red: &[f32], nir: &[f32]) -> Vec<f32> {
        red.iter()
            .zip(nir.iter())
            .map(|(r, n)| {
                let denom = n + r;
                if denom == 0.0 { 0.0 } else { (n - r) / denom }
            })
            .collect()
    }
}
