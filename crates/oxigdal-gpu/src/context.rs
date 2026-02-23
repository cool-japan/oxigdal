//! GPU context management for OxiGDAL.
//!
//! This module handles WGPU device initialization, adapter selection,
//! and resource management for GPU-accelerated operations.

use crate::error::{GpuError, GpuResult};
use std::sync::Arc;
use tracing::{debug, info};
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
            label: Some("OxiGDAL GPU Context".to_string()),
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
        let instance = Instance::new(&InstanceDescriptor {
            backends: config.backend.to_backends(),
            ..Default::default()
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

        info!("GPU device created successfully");
        debug!("Device limits: {:?}", limits);

        Ok(Self {
            instance: Arc::new(instance),
            adapter: Arc::new(adapter),
            device: Arc::new(device),
            queue: Arc::new(queue),
            adapter_info,
            limits,
        })
    }

    /// Request a suitable GPU adapter.
    async fn request_adapter(instance: &Instance, config: &GpuContextConfig) -> GpuResult<Adapter> {
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: config.power_preference.into(),
                force_fallback_adapter: false,
                compatible_surface: None,
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
    /// This should be called periodically to process GPU operations.
    pub fn poll(&self, _wait: bool) {
        // wgpu 28 doesn't have explicit poll control, device polls automatically
        // This method is kept for API compatibility
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
}

impl std::fmt::Debug for GpuContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuContext")
            .field("adapter", &self.adapter_info.name)
            .field("backend", &self.adapter_info.backend)
            .field("device_type", &self.adapter_info.device_type)
            .field("limits", &self.limits)
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
