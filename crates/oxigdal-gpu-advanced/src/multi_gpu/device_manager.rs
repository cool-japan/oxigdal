//! GPU device management and capabilities detection.

use crate::error::{GpuAdvancedError, Result};
use wgpu::{Adapter, Backend, DeviceType};

/// Device capabilities and features
#[derive(Debug, Clone)]
pub struct DeviceCapabilities {
    /// Supports compute shaders
    pub compute: bool,
    /// Supports timestamp queries
    pub timestamp_query: bool,
    /// Supports pipeline statistics
    pub pipeline_statistics: bool,
    /// Supports texture compression BC
    pub texture_compression_bc: bool,
    /// Supports texture compression ETC2
    pub texture_compression_etc2: bool,
    /// Supports texture compression ASTC
    pub texture_compression_astc: bool,
    /// Supports indirect first instance
    pub indirect_first_instance: bool,
    /// Supports shader f16
    pub shader_f16: bool,
    /// Supports push constants
    pub push_constants: bool,
    /// Supports multi draw indirect
    pub multi_draw_indirect: bool,
    /// Supports multi draw indirect count
    pub multi_draw_indirect_count: bool,
}

impl DeviceCapabilities {
    /// Detect capabilities from adapter
    pub fn from_adapter(adapter: &Adapter) -> Self {
        let features = adapter.features();

        Self {
            compute: true, // Always available in WGPU
            timestamp_query: features.contains(wgpu::Features::TIMESTAMP_QUERY),
            pipeline_statistics: features.contains(wgpu::Features::PIPELINE_STATISTICS_QUERY),
            texture_compression_bc: features.contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
            texture_compression_etc2: features.contains(wgpu::Features::TEXTURE_COMPRESSION_ETC2),
            texture_compression_astc: features.contains(wgpu::Features::TEXTURE_COMPRESSION_ASTC),
            indirect_first_instance: features.contains(wgpu::Features::INDIRECT_FIRST_INSTANCE),
            shader_f16: features.contains(wgpu::Features::SHADER_F16),
            // Note: PUSH_CONSTANTS feature was removed in newer WGPU versions
            // Push constants are now part of the core API
            push_constants: true,
            // MULTI_DRAW_INDIRECT was replaced with MULTI_DRAW_INDIRECT_COUNT
            multi_draw_indirect: features.contains(wgpu::Features::MULTI_DRAW_INDIRECT_COUNT),
            multi_draw_indirect_count: features.contains(wgpu::Features::MULTI_DRAW_INDIRECT_COUNT),
        }
    }

    /// Check if device supports all required features
    pub fn supports_required_features(&self) -> bool {
        self.compute
    }

    /// Check if device supports optimal features
    pub fn supports_optimal_features(&self) -> bool {
        self.compute && self.timestamp_query && self.push_constants
    }
}

/// Device performance class
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DevicePerformanceClass {
    /// Low-end device (integrated GPU, mobile)
    Low,
    /// Mid-range device (mainstream discrete GPU)
    Medium,
    /// High-end device (high-performance discrete GPU)
    High,
    /// Extreme performance (datacenter GPU)
    Extreme,
}

impl DevicePerformanceClass {
    /// Classify device based on characteristics
    pub fn classify(device_type: DeviceType, _backend: Backend, limits: &wgpu::Limits) -> Self {
        // Simple heuristic based on device type and limits
        match device_type {
            DeviceType::DiscreteGpu => {
                // Check memory and compute capabilities
                if limits.max_compute_workgroup_size_x >= 1024
                    && limits.max_buffer_size >= 4_000_000_000
                {
                    Self::Extreme
                } else if limits.max_compute_workgroup_size_x >= 512 {
                    Self::High
                } else {
                    Self::Medium
                }
            }
            DeviceType::IntegratedGpu => {
                if limits.max_compute_workgroup_size_x >= 512 {
                    Self::Medium
                } else {
                    Self::Low
                }
            }
            DeviceType::VirtualGpu => Self::Medium,
            DeviceType::Cpu | DeviceType::Other => Self::Low,
        }
    }

    /// Get workload multiplier for this performance class
    pub fn workload_multiplier(&self) -> f32 {
        match self {
            Self::Extreme => 4.0,
            Self::High => 2.0,
            Self::Medium => 1.0,
            Self::Low => 0.5,
        }
    }
}

/// Device filter for selection
#[derive(Debug, Clone)]
pub struct DeviceFilter {
    /// Minimum performance class
    pub min_performance: Option<DevicePerformanceClass>,
    /// Preferred backend
    pub preferred_backend: Option<Backend>,
    /// Required device type
    pub required_type: Option<DeviceType>,
    /// Minimum memory
    pub min_memory: Option<u64>,
    /// Required features
    pub required_features: wgpu::Features,
}

impl Default for DeviceFilter {
    fn default() -> Self {
        Self {
            min_performance: None,
            preferred_backend: None,
            required_type: None,
            min_memory: None,
            required_features: wgpu::Features::empty(),
        }
    }
}

impl DeviceFilter {
    /// Create a new device filter
    pub fn new() -> Self {
        Self::default()
    }

    /// Set minimum performance class
    pub fn with_min_performance(mut self, perf: DevicePerformanceClass) -> Self {
        self.min_performance = Some(perf);
        self
    }

    /// Set preferred backend
    pub fn with_preferred_backend(mut self, backend: Backend) -> Self {
        self.preferred_backend = Some(backend);
        self
    }

    /// Set required device type
    pub fn with_required_type(mut self, device_type: DeviceType) -> Self {
        self.required_type = Some(device_type);
        self
    }

    /// Set minimum memory
    pub fn with_min_memory(mut self, memory: u64) -> Self {
        self.min_memory = Some(memory);
        self
    }

    /// Set required features
    pub fn with_required_features(mut self, features: wgpu::Features) -> Self {
        self.required_features = features;
        self
    }

    /// Check if adapter matches filter
    pub fn matches(&self, adapter: &Adapter) -> bool {
        let info = adapter.get_info();
        let limits = adapter.limits();

        // Check device type
        if let Some(req_type) = self.required_type {
            if info.device_type != req_type {
                return false;
            }
        }

        // Check backend
        if let Some(pref_backend) = self.preferred_backend {
            if info.backend != pref_backend {
                return false;
            }
        }

        // Check performance class
        if let Some(min_perf) = self.min_performance {
            let perf_class =
                DevicePerformanceClass::classify(info.device_type, info.backend, &limits);
            if perf_class < min_perf {
                return false;
            }
        }

        // Check minimum memory
        if let Some(min_mem) = self.min_memory {
            if limits.max_buffer_size < min_mem {
                return false;
            }
        }

        // Check features
        let features = adapter.features();
        if !features.contains(self.required_features) {
            return false;
        }

        true
    }
}

/// Device manager for GPU enumeration and selection
pub struct DeviceManager {
    /// WGPU instance
    instance: wgpu::Instance,
}

impl DeviceManager {
    /// Create a new device manager
    pub fn new() -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        Self { instance }
    }

    /// Enumerate all adapters
    pub async fn enumerate_adapters(&self) -> Vec<Adapter> {
        self.instance
            .enumerate_adapters(wgpu::Backends::all())
            .await
    }

    /// Enumerate adapters matching filter
    pub async fn enumerate_filtered(&self, filter: &DeviceFilter) -> Vec<Adapter> {
        self.enumerate_adapters()
            .await
            .into_iter()
            .filter(|adapter| filter.matches(adapter))
            .collect()
    }

    /// Get best adapter matching filter
    pub async fn get_best_adapter(&self, filter: &DeviceFilter) -> Result<Adapter> {
        let mut adapters = self.enumerate_filtered(filter).await;

        if adapters.is_empty() {
            return Err(GpuAdvancedError::GpuNotFound(
                "No adapter matching filter".to_string(),
            ));
        }

        // Sort by performance class and backend
        adapters.sort_by(|a, b| {
            let info_a = a.get_info();
            let info_b = b.get_info();
            let limits_a = a.limits();
            let limits_b = b.limits();

            let perf_a =
                DevicePerformanceClass::classify(info_a.device_type, info_a.backend, &limits_a);
            let perf_b =
                DevicePerformanceClass::classify(info_b.device_type, info_b.backend, &limits_b);

            perf_b.cmp(&perf_a)
        });

        adapters
            .into_iter()
            .next()
            .ok_or_else(|| GpuAdvancedError::GpuNotFound("No adapter available".to_string()))
    }

    /// Print device information
    pub async fn print_device_info(&self) {
        let adapters = self.enumerate_adapters().await;
        println!("Found {} GPU adapter(s):", adapters.len());

        for (i, adapter) in adapters.iter().enumerate() {
            let info = adapter.get_info();
            let limits = adapter.limits();
            let features = adapter.features();
            let caps = DeviceCapabilities::from_adapter(adapter);
            let perf_class =
                DevicePerformanceClass::classify(info.device_type, info.backend, &limits);

            println!("\n  Adapter {}:", i);
            println!("    Name: {}", info.name);
            println!("    Backend: {:?}", info.backend);
            println!("    Device Type: {:?}", info.device_type);
            println!("    Performance Class: {:?}", perf_class);
            println!("    Max Buffer Size: {} bytes", limits.max_buffer_size);
            println!(
                "    Max Texture 2D: {}x{}",
                limits.max_texture_dimension_2d, limits.max_texture_dimension_2d
            );
            println!(
                "    Max Workgroup Size: {}x{}x{}",
                limits.max_compute_workgroup_size_x,
                limits.max_compute_workgroup_size_y,
                limits.max_compute_workgroup_size_z
            );
            println!("    Features: {:?}", features.bits());
            println!("    Timestamp Query: {}", caps.timestamp_query);
            println!("    Push Constants: {}", caps.push_constants);
        }
    }
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_device_manager_creation() {
        let manager = DeviceManager::new();
        let adapters = manager.enumerate_adapters().await;
        println!("Found {} adapters", adapters.len());
    }

    #[tokio::test]
    async fn test_device_filter() {
        let filter = DeviceFilter::new().with_min_performance(DevicePerformanceClass::Low);

        let manager = DeviceManager::new();
        let adapters = manager.enumerate_filtered(&filter).await;
        println!("Found {} filtered adapters", adapters.len());
    }

    #[tokio::test]
    async fn test_performance_classification() {
        let manager = DeviceManager::new();
        let adapters = manager.enumerate_adapters().await;

        for adapter in adapters {
            let info = adapter.get_info();
            let limits = adapter.limits();
            let perf_class =
                DevicePerformanceClass::classify(info.device_type, info.backend, &limits);
            println!("{}: {:?}", info.name, perf_class);
        }
    }
}
