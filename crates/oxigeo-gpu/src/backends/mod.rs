//! Backend-specific optimizations for different GPU APIs.
//!
//! This module provides platform-specific optimizations for CUDA, Vulkan,
//! Metal, and DirectML backends.

#[cfg(feature = "cuda")]
pub mod cuda;

#[cfg(feature = "vulkan")]
pub mod vulkan;

#[cfg(feature = "metal")]
pub mod metal;

#[cfg(feature = "directml")]
pub mod directml;

// Backend capability detection and optimization utilities

/// Backend capability flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendCapabilities {
    /// Supports tensor cores.
    pub tensor_cores: bool,
    /// Supports ray tracing.
    pub ray_tracing: bool,
    /// Supports mesh shaders.
    pub mesh_shaders: bool,
    /// Supports variable rate shading.
    pub variable_rate_shading: bool,
    /// Supports async compute.
    pub async_compute: bool,
    /// Supports peer-to-peer transfers.
    pub p2p_transfer: bool,
    /// Maximum workgroup size.
    pub max_workgroup_size: (u32, u32, u32),
    /// Maximum compute invocations.
    pub max_compute_invocations: u32,
}

impl Default for BackendCapabilities {
    fn default() -> Self {
        Self {
            tensor_cores: false,
            ray_tracing: false,
            mesh_shaders: false,
            variable_rate_shading: false,
            async_compute: false,
            p2p_transfer: false,
            max_workgroup_size: (256, 256, 64),
            max_compute_invocations: 256,
        }
    }
}

/// Backend-specific optimization hints.
#[derive(Debug, Clone)]
pub enum OptimizationHint {
    /// Use shared memory (CUDA/Vulkan).
    UseSharedMemory,
    /// Use warp-level primitives (CUDA).
    UseWarpPrimitives,
    /// Use subgroup operations (Vulkan).
    UseSubgroupOps,
    /// Use threadgroup memory (Metal).
    UseThreadgroupMemory,
    /// Prefer wave operations (DirectML).
    PreferWaveOps,
    /// Enable async execution.
    EnableAsyncExecution,
}

/// Query backend capabilities.
pub fn query_capabilities(backend: wgpu::Backend) -> BackendCapabilities {
    match backend {
        wgpu::Backend::Vulkan => BackendCapabilities {
            async_compute: true,
            max_workgroup_size: (1024, 1024, 64),
            max_compute_invocations: 1024,
            ..Default::default()
        },
        wgpu::Backend::Metal => BackendCapabilities {
            async_compute: true,
            max_workgroup_size: (1024, 1024, 64),
            max_compute_invocations: 1024,
            ..Default::default()
        },
        wgpu::Backend::Dx12 => BackendCapabilities {
            async_compute: true,
            max_workgroup_size: (1024, 1024, 64),
            max_compute_invocations: 1024,
            ..Default::default()
        },
        _ => BackendCapabilities::default(),
    }
}
