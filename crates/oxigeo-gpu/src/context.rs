//! GPU context management for OxiGeo.
//!
//! This module handles WGPU device initialization, adapter selection,
//! and resource management for GPU-accelerated operations.

use crate::error::{GpuError, GpuResult};
use crate::pipeline_cache::{SharedPipelineCache, new_shared_pipeline_cache};
use crate::workgroup_tuner::WorkgroupTuner;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tracing::{debug, info, warn};
use wgpu::{
    Adapter, AdapterInfo, Backend, Backends, Device, DeviceDescriptor, Features, Instance,
    InstanceDescriptor, Limits, PowerPreference, Queue, RequestAdapterOptions,
};

/// GPU backend preference for adapter selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendPreference {
    /// Prefer Vulkan backend (cross-platform, best performance on Linux/Windows).
    Vulkan,
    /// Prefer Metal backend (best performance on macOS/iOS).
    Metal,
    /// Prefer DX12 backend (best performance on Windows).
    DX12,
    /// Prefer WebGPU backend (for browser environments).
    WebGPU,
    /// Auto-select the best available backend for the platform.
    Auto,
    /// Try all available backends in order of preference.
    All,
}

impl BackendPreference {
    /// Convert to WGPU backends flags.
    pub fn to_backends(&self) -> Backends {
        match self {
            Self::Vulkan => Backends::VULKAN,
            Self::Metal => Backends::METAL,
            Self::DX12 => Backends::DX12,
            Self::WebGPU => Backends::BROWSER_WEBGPU,
            Self::Auto => Backends::PRIMARY,
            Self::All => Backends::all(),
        }
    }

    /// Get platform-specific default backend.
    pub fn platform_default() -> Self {
        #[cfg(target_os = "macos")]
        return Self::Metal;

        #[cfg(target_os = "windows")]
        return Self::DX12;

        #[cfg(target_os = "linux")]
        return Self::Vulkan;

        #[cfg(target_arch = "wasm32")]
        return Self::WebGPU;

        #[cfg(not(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_arch = "wasm32"
        )))]
        return Self::Auto;
    }
}

/// Power preference for GPU selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuPowerPreference {
    /// Prefer low power consumption (integrated GPU).
    LowPower,
    /// Prefer high performance (discrete GPU).
    HighPerformance,
    /// No preference, use system default.
    Default,
}

impl From<GpuPowerPreference> for PowerPreference {
    fn from(pref: GpuPowerPreference) -> Self {
        match pref {
            GpuPowerPreference::LowPower => PowerPreference::LowPower,
            GpuPowerPreference::HighPerformance => PowerPreference::HighPerformance,
            GpuPowerPreference::Default => PowerPreference::None,
        }
    }
}

/// Configuration for GPU context initialization.
#[derive(Debug, Clone)]
pub struct GpuContextConfig {
    /// Backend preference for adapter selection.
    pub backend: BackendPreference,
    /// Power preference for GPU selection.
    pub power_preference: GpuPowerPreference,
    /// Required GPU features.
    pub required_features: Features,
    /// Required GPU limits.
    pub required_limits: Option<Limits>,
    /// Enable debug mode (validation layers).
    pub debug: bool,
    /// Label for the device (for debugging).
    pub label: Option<String>,
}

impl Default for GpuContextConfig {
    fn default() -> Self {
        Self {
            backend: BackendPreference::platform_default(),
            power_preference: GpuPowerPreference::HighPerformance,
            required_features: Features::empty(),
            required_limits: None,
            debug: cfg!(debug_assertions),
            label: Some("OxiGeo GPU Context".to_string()),
        }
    }
}

impl GpuContextConfig {
    /// Create a new GPU context configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set backend preference.
    pub fn with_backend(mut self, backend: BackendPreference) -> Self {
        self.backend = backend;
        self
    }

    /// Set power preference.
    pub fn with_power_preference(mut self, power: GpuPowerPreference) -> Self {
        self.power_preference = power;
        self
    }

    /// Set required features.
    pub fn with_features(mut self, features: Features) -> Self {
        self.required_features = features;
        self
    }

    /// Set required limits.
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.required_limits = Some(limits);
        self
    }

    /// Enable debug mode.
    pub fn with_debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    /// Set device label.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Request `SHADER_F16` GPU feature support.
    ///
    /// When this flag is set the context will require the adapter to expose
    /// `wgpu::Features::SHADER_F16`, which enables the `enable f16;`
    /// directive in WGSL shaders and allows native half-precision arithmetic
    /// on the GPU.
    ///
    /// # Availability
    ///
    /// Not all adapters support this feature.  Check with
    /// [`GpuContext::supports_feature`] after context creation.  If the
    /// adapter lacks support, context creation will fail — consider first
    /// checking via [`wgpu::Adapter::features`] or falling back to the
    /// widening path provided by [`crate::buffer::from_f16_slice_widening`].
    pub fn with_f16_support(mut self) -> Self {
        self.required_features |= Features::SHADER_F16;
        self
    }

    /// Request `IMMEDIATES` GPU feature support (push constants).
    ///
    /// When this flag is set the context will require the adapter to expose
    /// [`wgpu::Features::IMMEDIATES`], which enables:
    /// - `var<immediate>` variables in WGSL shaders.
    /// - [`wgpu::PipelineLayoutDescriptor::immediate_size`] > 0.
    /// - [`wgpu::ComputePass::set_immediates`] / [`wgpu::RenderPass::set_immediates`].
    ///
    /// This corresponds to Vulkan push constants and is available on Vulkan,
    /// Metal, DX12, and WebGPU (as a proposal).
    ///
    /// # Availability
    ///
    /// Check adapter support before requesting this feature.  If unsupported,
    /// use a uniform buffer to pass small per-dispatch constants instead.
    pub fn with_push_constants(mut self) -> Self {
        self.required_features |= Features::IMMEDIATES;
        self
    }

    /// Request cooperative-matrix and subgroup GPU features.
    ///
    /// When this flag is set the context will attempt to request:
    /// - [`wgpu::Features::EXPERIMENTAL_COOPERATIVE_MATRIX`] — hardware tile
    ///   MMA (WMMA / tensor-core) support.
    /// - [`wgpu::Features::SUBGROUP`] — subgroup intrinsics in compute and
    ///   fragment shaders (required by the cooperative-matrix extension).
    ///
    /// Both flags are added to [`GpuContextConfig::required_features`].
    /// If the adapter does not expose these features, context creation will
    /// fail.  Use [`crate::cooperative_matrix::supports_cooperative_matrix`]
    /// after creating a context **without** this method to check availability
    /// before upgrading.
    ///
    /// # Fallback
    ///
    /// The workgroup-tiled GEMM path in
    /// [`crate::cooperative_matrix::build_cooperative_matrix_gemm_pipeline`]
    /// is correct on **all** adapters regardless of this flag.  Requesting
    /// cooperative-matrix features is only necessary when the shader code
    /// explicitly uses `subgroupMatrix*` builtins.
    pub fn with_cooperative_matrix(mut self) -> Self {
        self.required_features |= Features::SUBGROUP;
        self.required_features |= Features::EXPERIMENTAL_COOPERATIVE_MATRIX;
        self
    }

    /// Build a [`GpuContext`] from this configuration.
    ///
    /// Convenience async builder that calls [`GpuContext::with_config`].
    ///
    /// # Errors
    ///
    /// Returns an error if no suitable GPU adapter is found or device
    /// request fails (e.g., a required feature is unsupported).
    pub async fn build(self) -> crate::error::GpuResult<GpuContext> {
        GpuContext::with_config(self).await
    }
}

/// GPU context holding device and queue.
///
/// This is the main entry point for GPU operations. It manages the WGPU
/// device and queue, and provides methods for creating buffers, pipelines,
/// and executing compute shaders.
#[derive(Clone)]
pub struct GpuContext {
    /// WGPU instance.
    instance: Arc<Instance>,
    /// WGPU adapter.
    adapter: Arc<Adapter>,
    /// WGPU device.
    device: Arc<Device>,
    /// WGPU queue.
    queue: Arc<Queue>,
    /// Adapter information.
    adapter_info: AdapterInfo,
    /// Device limits.
    limits: Limits,
    /// Workgroup sizes tuned to this adapter's limits.
    pub tuner: WorkgroupTuner,
    /// Atomic flag set to `true` when the GPU device is lost.
    ///
    /// Written from the wgpu device-lost callback (a background thread);
    /// read on the submission path before every `queue.submit`.
    device_lost: Arc<AtomicBool>,
    /// Shared cache of compiled compute pipelines.
    ///
    /// Keyed by `(shader_hash, entry_point, layout_tag)`.  Must be cleared
    /// whenever the underlying `wgpu::Device` is replaced (device-lost
    /// recovery), because compiled pipelines are bound to a specific device.
    pipeline_cache: SharedPipelineCache,
}

impl GpuContext {
    /// Create a new GPU context with default configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if no suitable GPU adapter is found or device
    /// request fails.
    pub async fn new() -> GpuResult<Self> {
        Self::with_config(GpuContextConfig::default()).await
    }

    /// Create a new GPU context with custom configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if no suitable GPU adapter is found or device
    /// request fails.
    pub async fn with_config(config: GpuContextConfig) -> GpuResult<Self> {
        info!(
            "Initializing GPU context with backend: {:?}",
            config.backend
        );

        // Create WGPU instance
        let instance = Instance::new(InstanceDescriptor {
            backends: config.backend.to_backends(),
            ..InstanceDescriptor::new_without_display_handle()
        });

        // Request adapter
        let adapter = Self::request_adapter(&instance, &config).await?;
        let adapter_info = adapter.get_info();

        info!(
            "Selected GPU adapter: {} ({:?})",
            adapter_info.name, adapter_info.backend
        );
        debug!("Adapter info: {:?}", adapter_info);

        // Get adapter limits
        let adapter_limits = adapter.limits();

        // Derive workgroup sizes from the adapter's actual capabilities before
        // we potentially clamp/override them via `required_limits`.
        let tuner = WorkgroupTuner::derive_from_limits(&adapter_limits);

        let limits = config
            .required_limits
            .unwrap_or_else(|| Self::default_limits(&adapter_limits));

        // Validate limits
        if !Self::validate_limits(&limits, &adapter_limits) {
            return Err(GpuError::device_request(format!(
                "Requested limits exceed adapter capabilities: \
                 max_compute_workgroup_size_x: {} (adapter: {})",
                limits.max_compute_workgroup_size_x, adapter_limits.max_compute_workgroup_size_x
            )));
        }

        // Request device
        let (device, queue) = adapter
            .request_device(&DeviceDescriptor {
                label: config.label.as_deref(),
                required_features: config.required_features,
                required_limits: limits.clone(),
                memory_hints: Default::default(),
                experimental_features: Default::default(),
                trace: Default::default(),
            })
            .await
            .map_err(|e| GpuError::device_request(e.to_string()))?;

        // Install device-lost callback: atomically flags that the device has
        // been lost so that subsequent `queue.submit` calls can short-circuit.
        let device_lost = Arc::new(AtomicBool::new(false));
        let flag_clone = Arc::clone(&device_lost);
        device.set_device_lost_callback(move |_reason, message| {
            warn!("GPU device lost: {}", message);
            flag_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        });

        info!("GPU device created successfully");
        debug!("Device limits: {:?}", limits);
        debug!("Workgroup tuner: {:?}", tuner);

        Ok(Self {
            instance: Arc::new(instance),
            adapter: Arc::new(adapter),
            device: Arc::new(device),
            queue: Arc::new(queue),
            adapter_info,
            limits,
            tuner,
            device_lost,
            pipeline_cache: new_shared_pipeline_cache(),
        })
    }

    /// Request a suitable GPU adapter.
    async fn request_adapter(instance: &Instance, config: &GpuContextConfig) -> GpuResult<Adapter> {
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: config.power_preference.into(),
                force_fallback_adapter: false,
                compatible_surface: None,
                apply_limit_buckets: false,
            })
            .await;

        adapter.map_err(|_| {
            let backends = match config.backend {
                BackendPreference::Auto => "Auto (PRIMARY)".to_string(),
                BackendPreference::All => "All".to_string(),
                backend => format!("{backend:?}"),
            };
            GpuError::no_adapter(backends)
        })
    }

    /// Get default limits based on adapter capabilities.
    fn default_limits(adapter_limits: &Limits) -> Limits {
        Limits {
            max_compute_workgroup_size_x: adapter_limits.max_compute_workgroup_size_x.min(256),
            max_compute_workgroup_size_y: adapter_limits.max_compute_workgroup_size_y.min(256),
            max_compute_workgroup_size_z: adapter_limits.max_compute_workgroup_size_z.min(64),
            max_compute_invocations_per_workgroup: adapter_limits
                .max_compute_invocations_per_workgroup
                .min(256),
            max_compute_workgroups_per_dimension: adapter_limits
                .max_compute_workgroups_per_dimension,
            ..Default::default()
        }
    }

    /// Validate that requested limits don't exceed adapter capabilities.
    fn validate_limits(requested: &Limits, adapter: &Limits) -> bool {
        requested.max_compute_workgroup_size_x <= adapter.max_compute_workgroup_size_x
            && requested.max_compute_workgroup_size_y <= adapter.max_compute_workgroup_size_y
            && requested.max_compute_workgroup_size_z <= adapter.max_compute_workgroup_size_z
            && requested.max_compute_invocations_per_workgroup
                <= adapter.max_compute_invocations_per_workgroup
    }

    /// Get the WGPU device.
    pub fn device(&self) -> &Device {
        &self.device
    }

    /// Get the WGPU queue.
    pub fn queue(&self) -> &Queue {
        &self.queue
    }

    /// Get the WGPU adapter.
    pub fn adapter(&self) -> &Adapter {
        &self.adapter
    }

    /// Get the WGPU instance.
    pub fn instance(&self) -> &Instance {
        &self.instance
    }

    /// Get adapter information.
    pub fn adapter_info(&self) -> &AdapterInfo {
        &self.adapter_info
    }

    /// Get device limits.
    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    /// Get the backend being used.
    pub fn backend(&self) -> Backend {
        self.adapter_info.backend
    }

    /// Check if the device supports a specific feature.
    pub fn supports_feature(&self, feature: Features) -> bool {
        self.device.features().contains(feature)
    }

    /// Get maximum workgroup size for compute shaders.
    pub fn max_workgroup_size(&self) -> (u32, u32, u32) {
        (
            self.limits.max_compute_workgroup_size_x,
            self.limits.max_compute_workgroup_size_y,
            self.limits.max_compute_workgroup_size_z,
        )
    }

    /// Poll the device for completed operations.
    ///
    /// This must be called to service pending GPU work — most importantly the
    /// `map_async` callbacks that back [`crate::GpuBuffer::read`]. Without a
    /// poll those callbacks are never invoked and a readback future never
    /// resolves.
    ///
    /// When `wait` is `true` this blocks until the device is idle
    /// ([`wgpu::PollType::Wait`]); when `false` it performs a single
    /// non-blocking poll ([`wgpu::PollType::Poll`]). Poll errors (e.g. a lost
    /// device) are intentionally swallowed here — callers observe device loss
    /// through the operation that submitted the work.
    pub fn poll(&self, wait: bool) {
        let poll_type = if wait {
            wgpu::PollType::wait_indefinitely()
        } else {
            wgpu::PollType::Poll
        };
        let _ = self.device.poll(poll_type);
    }

    /// Spawn a background thread that keeps the wgpu device polled.
    ///
    /// This is an **advanced helper** intended for callers that run async GPU
    /// readbacks without a runtime that issues device polls automatically (e.g.
    /// bare `pollster` or custom executors).
    ///
    /// The spawned thread calls `device.poll(wgpu::PollType::Poll)` in a tight
    /// loop with a 1 ms sleep between iterations.  Call
    /// [`std::thread::JoinHandle::join`] when the thread is no longer needed
    /// (the caller is responsible for signalling termination through a shared
    /// flag or by dropping all `Arc<Device>` clones and letting the thread
    /// detect the closed handle).
    ///
    /// In most tokio/async-std environments the runtime's built-in submission
    /// loop makes this unnecessary.
    pub fn spawn_poll_task(&self) -> std::thread::JoinHandle<()> {
        let device = Arc::clone(&self.device);
        std::thread::spawn(move || {
            loop {
                let poll_result = device.poll(wgpu::PollType::Poll);
                // If the device queue is empty and all work is done, sleep
                // more aggressively to avoid spinning.
                if matches!(poll_result, Ok(wgpu::PollStatus::QueueEmpty)) {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
        })
    }

    /// Check if the device is still valid.
    pub fn is_valid(&self) -> bool {
        // Try to create a small buffer as a health check
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("health_check"),
            size: 4,
            usage: wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: false,
        });
        true
    }

    /// Override the workgroup tuner (useful in tests or for custom tuning).
    ///
    /// Returns `self` so it can be chained in a builder-style pattern:
    ///
    /// ```rust,no_run
    /// # use oxigeo_gpu::{GpuContext, WorkgroupTuner};
    /// # async fn ex() -> Result<(), Box<dyn std::error::Error>> {
    /// let gpu = GpuContext::new().await?.with_tuner(WorkgroupTuner::unlimited());
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_tuner(mut self, tuner: WorkgroupTuner) -> Self {
        self.tuner = tuner;
        self
    }

    // -------------------------------------------------------------------------
    // Device-lost recovery
    // -------------------------------------------------------------------------

    /// Returns `true` if the GPU device has been lost and operations will fail.
    ///
    /// The flag is set atomically by the wgpu device-lost callback registered
    /// during [`GpuContext::new`] / [`GpuContext::with_config`].
    pub fn is_device_lost(&self) -> bool {
        self.device_lost.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Checks whether the device has been lost and returns an error if so.
    ///
    /// Call this before every `queue.submit` to short-circuit operations on a
    /// defunct device instead of generating silent GPU errors.
    pub fn check_device_lost(&self) -> GpuResult<()> {
        if self.is_device_lost() {
            Err(GpuError::device_lost("device was lost"))
        } else {
            Ok(())
        }
    }

    /// Returns a reference to the shared pipeline cache.
    ///
    /// The cache maps `(shader_hash, entry_point, layout_tag)` keys to
    /// previously compiled [`wgpu::ComputePipeline`]s wrapped in `Arc`.
    ///
    /// Kernel implementations can call
    /// [`PipelineCache::get_or_insert_with`][crate::pipeline_cache::PipelineCache::get_or_insert_with]
    /// on the inner [`crate::pipeline_cache::PipelineCache`] to avoid
    /// recompiling the same WGSL shader on every kernel construction.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use oxigeo_gpu::{GpuContext, pipeline_cache::PipelineCacheKey};
    ///
    /// # async fn ex(ctx: &GpuContext) {
    /// let cache = ctx.pipeline_cache();
    /// if let Ok(mut guard) = cache.lock() {
    ///     println!("cached pipelines: {}", guard.len());
    /// }
    /// # }
    /// ```
    pub fn pipeline_cache(&self) -> &SharedPipelineCache {
        &self.pipeline_cache
    }

    /// Replaces the internal device-lost flag with an externally-supplied one.
    ///
    /// This builder method is intended **for testing only** — it allows a test
    /// to inject a shared `Arc<AtomicBool>` whose state it controls directly,
    /// exercising the `check_device_lost` / `is_device_lost` paths without
    /// requiring a real GPU device.
    ///
    /// ```rust
    /// use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    /// // NB: GpuContext::new() requires a GPU; this is conceptual.
    /// // let ctx = ctx.with_device_lost_flag(Arc::new(AtomicBool::new(true)));
    /// ```
    pub fn with_device_lost_flag(mut self, flag: Arc<AtomicBool>) -> Self {
        self.device_lost = flag;
        self
    }

    /// Attempts to recreate the GPU device on the same adapter.
    ///
    /// This creates a fresh `GpuContext` via the same wgpu instance, selecting
    /// an adapter with matching properties to the one this context was built
    /// on.  The old context **must be dropped** after calling this method;
    /// continuing to use it will only generate further device-lost errors.
    ///
    /// # Errors
    ///
    /// Returns an error if adapter enumeration fails or the device cannot be
    /// re-created (e.g., the GPU has been physically removed).
    pub async fn reinitialize(&self) -> GpuResult<GpuContext> {
        // Re-use the existing instance so we stay within the same set of
        // enabled backends.  We enumerate adapters and pick the first one
        // whose backend matches the original; fall back to `GpuContext::new`
        // which will pick the platform default if none match.
        let original_backend = self.adapter_info.backend;
        let adapters = self.instance.enumerate_adapters(Backends::all()).await;
        let matching = adapters
            .into_iter()
            .find(|a| a.get_info().backend == original_backend);

        if let Some(adapter) = matching {
            let adapter_info = adapter.get_info();
            let adapter_limits = adapter.limits();
            let tuner = WorkgroupTuner::derive_from_limits(&adapter_limits);
            let limits = Self::default_limits(&adapter_limits);

            let (device, queue) = adapter
                .request_device(&DeviceDescriptor {
                    label: Some("OxiGeo GPU Context (reinit)"),
                    required_features: Features::empty(),
                    required_limits: limits.clone(),
                    memory_hints: Default::default(),
                    experimental_features: Default::default(),
                    trace: Default::default(),
                })
                .await
                .map_err(|e| GpuError::device_request(format!("reinitialize: {e}")))?;

            let device_lost = Arc::new(AtomicBool::new(false));
            let flag_clone = Arc::clone(&device_lost);
            device.set_device_lost_callback(move |_reason, message| {
                warn!("GPU device lost (reinit): {}", message);
                flag_clone.store(true, std::sync::atomic::Ordering::SeqCst);
            });

            info!(
                "GPU device successfully reinitialized on adapter: {}",
                adapter_info.name
            );

            // Evict all stale pipelines compiled against the old device before
            // handing the cache to the new context.
            let pipeline_cache = new_shared_pipeline_cache();

            Ok(Self {
                instance: Arc::clone(&self.instance),
                adapter: Arc::new(adapter),
                device: Arc::new(device),
                queue: Arc::new(queue),
                adapter_info,
                limits,
                tuner,
                device_lost,
                pipeline_cache,
            })
        } else {
            // No matching adapter found — fall back to a completely fresh init.
            warn!(
                "No adapter matching original backend {:?} found during reinitialize, \
                 falling back to platform default",
                original_backend
            );
            GpuContext::new().await
        }
    }
}

impl std::fmt::Debug for GpuContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuContext")
            .field("adapter", &self.adapter_info.name)
            .field("backend", &self.adapter_info.backend)
            .field("device_type", &self.adapter_info.device_type)
            .field("limits", &self.limits)
            .field("tuner", &self.tuner)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_gpu_context_creation() {
        // This test will fail if no GPU is available, which is expected
        match GpuContext::new().await {
            Ok(ctx) => {
                println!("GPU Context created: {:?}", ctx);
                assert!(ctx.is_valid());
            }
            Err(e) => {
                println!("GPU not available (expected in CI): {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_backend_preference() {
        let config = GpuContextConfig::new()
            .with_backend(BackendPreference::platform_default())
            .with_power_preference(GpuPowerPreference::HighPerformance);

        match GpuContext::with_config(config).await {
            Ok(ctx) => {
                println!("Backend: {:?}", ctx.backend());
            }
            Err(e) => {
                println!("GPU not available: {}", e);
            }
        }
    }

    #[test]
    fn test_backend_conversion() {
        assert_eq!(BackendPreference::Vulkan.to_backends(), Backends::VULKAN);
        assert_eq!(BackendPreference::Metal.to_backends(), Backends::METAL);
        assert_eq!(BackendPreference::DX12.to_backends(), Backends::DX12);
    }

    #[test]
    fn test_platform_default() {
        let default = BackendPreference::platform_default();
        println!("Platform default backend: {:?}", default);

        #[cfg(target_os = "macos")]
        assert_eq!(default, BackendPreference::Metal);

        #[cfg(target_os = "windows")]
        assert_eq!(default, BackendPreference::DX12);

        #[cfg(target_os = "linux")]
        assert_eq!(default, BackendPreference::Vulkan);
    }
}
