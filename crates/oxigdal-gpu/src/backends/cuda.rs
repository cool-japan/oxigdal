//! CUDA-specific optimizations for NVIDIA GPUs.
//!
//! This module provides CUDA-specific shader optimizations and features
//! when running on NVIDIA hardware through Vulkan backend.

use crate::context::GpuContext;
use crate::error::{GpuError, GpuResult};
use std::collections::HashMap;
use tracing::{debug, info};

/// CUDA compute capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ComputeCapability {
    /// Major version.
    pub major: u32,
    /// Minor version.
    pub minor: u32,
}

impl ComputeCapability {
    /// Create a new compute capability.
    pub fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }

    /// Check if tensor cores are supported (SM 7.0+).
    pub fn supports_tensor_cores(&self) -> bool {
        self.major >= 7
    }

    /// Check if ray tracing cores are supported (SM 7.5+).
    pub fn supports_rt_cores(&self) -> bool {
        self.major > 7 || (self.major == 7 && self.minor >= 5)
    }

    /// Get maximum threads per block.
    pub fn max_threads_per_block(&self) -> u32 {
        if self.major >= 3 { 1024 } else { 512 }
    }

    /// Get warp size.
    pub fn warp_size(&self) -> u32 {
        32
    }

    /// Get maximum shared memory per block (bytes).
    pub fn max_shared_memory(&self) -> u64 {
        if self.major >= 7 {
            96 * 1024 // 96 KB for SM 7.x+
        } else if self.major >= 5 {
            48 * 1024 // 48 KB for SM 5.x-6.x
        } else {
            16 * 1024 // 16 KB for older
        }
    }
}

/// CUDA optimization configuration.
#[derive(Debug, Clone)]
pub struct CudaOptimizationConfig {
    /// Enable warp-level primitives.
    pub enable_warp_ops: bool,
    /// Enable shared memory optimization.
    pub enable_shared_memory: bool,
    /// Enable tensor core operations.
    pub enable_tensor_cores: bool,
    /// Preferred block size.
    pub block_size: (u32, u32, u32),
    /// Enable async compute streams.
    pub enable_async_streams: bool,
}

impl Default for CudaOptimizationConfig {
    fn default() -> Self {
        Self {
            enable_warp_ops: true,
            enable_shared_memory: true,
            enable_tensor_cores: false,
            block_size: (256, 1, 1),
            enable_async_streams: true,
        }
    }
}

/// CUDA kernel optimizer.
pub struct CudaOptimizer {
    compute_capability: ComputeCapability,
    config: CudaOptimizationConfig,
}

impl CudaOptimizer {
    /// Create a new CUDA optimizer.
    pub fn new(compute_capability: ComputeCapability, config: CudaOptimizationConfig) -> Self {
        Self {
            compute_capability,
            config,
        }
    }

    /// Detect compute capability from context.
    pub fn detect(context: &GpuContext) -> GpuResult<Self> {
        let adapter_info = context.adapter_info();

        // Try to detect NVIDIA GPU
        if !adapter_info.name.to_lowercase().contains("nvidia") {
            return Err(GpuError::unsupported_operation(
                "Not a NVIDIA GPU".to_string(),
            ));
        }

        // Estimate compute capability based on device name
        let compute_capability = Self::estimate_compute_capability(&adapter_info.name);

        info!(
            "Detected CUDA compute capability: {}.{}",
            compute_capability.major, compute_capability.minor
        );

        Ok(Self::new(
            compute_capability,
            CudaOptimizationConfig::default(),
        ))
    }

    /// Optimize shader code for CUDA.
    pub fn optimize_shader(&self, shader_code: &str) -> String {
        let mut optimized = shader_code.to_string();

        // Apply CUDA-specific optimizations
        if self.config.enable_warp_ops {
            optimized = self.apply_warp_optimizations(&optimized);
        }

        if self.config.enable_shared_memory {
            optimized = self.apply_shared_memory_optimizations(&optimized);
        }

        if self.config.enable_tensor_cores && self.compute_capability.supports_tensor_cores() {
            optimized = self.apply_tensor_core_optimizations(&optimized);
        }

        optimized
    }

    /// Calculate optimal block size for a kernel.
    pub fn calculate_block_size(&self, num_elements: u64) -> (u32, u32, u32) {
        let max_threads = self.compute_capability.max_threads_per_block();

        // Simple 1D optimization
        if num_elements <= max_threads as u64 {
            return (num_elements as u32, 1, 1);
        }

        // Use configured block size
        self.config.block_size
    }

    /// Calculate grid size for a kernel.
    pub fn calculate_grid_size(
        &self,
        num_elements: u64,
        block_size: (u32, u32, u32),
    ) -> (u32, u32, u32) {
        let blocks_x = ((num_elements as u32 + block_size.0 - 1) / block_size.0).max(1);
        (blocks_x, 1, 1)
    }

    fn estimate_compute_capability(device_name: &str) -> ComputeCapability {
        let name = device_name.to_lowercase();

        // RTX 40 series (Ada Lovelace) - SM 8.9
        if name.contains("rtx 40") || name.contains("4090") || name.contains("4080") {
            return ComputeCapability::new(8, 9);
        }

        // RTX 30 series (Ampere) - SM 8.6
        if name.contains("rtx 30") || name.contains("3090") || name.contains("3080") {
            return ComputeCapability::new(8, 6);
        }

        // RTX 20 series (Turing) - SM 7.5
        if name.contains("rtx 20") || name.contains("2080") || name.contains("2070") {
            return ComputeCapability::new(7, 5);
        }

        // GTX 10 series (Pascal) - SM 6.1
        if name.contains("gtx 10") || name.contains("1080") || name.contains("1070") {
            return ComputeCapability::new(6, 1);
        }

        // Default to SM 5.0
        ComputeCapability::new(5, 0)
    }

    fn apply_warp_optimizations(&self, shader: &str) -> String {
        // Insert warp-level optimization hints
        let mut optimized = shader.to_string();

        // Add warp size constant if not present
        if !optimized.contains("const WARP_SIZE") {
            let warp_decl = format!(
                "\nconst WARP_SIZE: u32 = {}u;\n",
                self.compute_capability.warp_size()
            );
            optimized.insert_str(0, &warp_decl);
        }

        optimized
    }

    fn apply_shared_memory_optimizations(&self, shader: &str) -> String {
        let mut optimized = shader.to_string();

        // Add shared memory size hints
        let max_shared = self.compute_capability.max_shared_memory();
        if !optimized.contains("MAX_SHARED_MEMORY") {
            let shared_decl = format!("\nconst MAX_SHARED_MEMORY: u32 = {}u;\n", max_shared);
            optimized.insert_str(0, &shared_decl);
        }

        optimized
    }

    fn apply_tensor_core_optimizations(&self, shader: &str) -> String {
        let mut optimized = shader.to_string();

        // Add tensor core hints
        if !optimized.contains("TENSOR_CORES_AVAILABLE") {
            optimized.insert_str(0, "\nconst TENSOR_CORES_AVAILABLE: bool = true;\n");
        }

        optimized
    }
}

/// CUDA async compute stream manager.
pub struct CudaStreamManager {
    streams: HashMap<u32, StreamState>,
    next_stream_id: u32,
}

#[derive(Debug, Clone)]
struct StreamState {
    id: u32,
    in_use: bool,
    priority: i32,
}

impl CudaStreamManager {
    /// Create a new stream manager.
    pub fn new() -> Self {
        Self {
            streams: HashMap::new(),
            next_stream_id: 0,
        }
    }

    /// Create a new compute stream.
    pub fn create_stream(&mut self, priority: i32) -> u32 {
        let stream_id = self.next_stream_id;
        self.next_stream_id += 1;

        self.streams.insert(
            stream_id,
            StreamState {
                id: stream_id,
                in_use: false,
                priority,
            },
        );

        debug!(
            "Created CUDA stream {} with priority {}",
            stream_id, priority
        );

        stream_id
    }

    /// Acquire a stream for use.
    ///
    /// # Errors
    ///
    /// Returns an error if stream ID is invalid.
    pub fn acquire_stream(&mut self, stream_id: u32) -> GpuResult<()> {
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| GpuError::internal("Invalid stream ID"))?;

        if stream.in_use {
            return Err(GpuError::internal("Stream already in use"));
        }

        stream.in_use = true;
        Ok(())
    }

    /// Release a stream.
    ///
    /// # Errors
    ///
    /// Returns an error if stream ID is invalid.
    pub fn release_stream(&mut self, stream_id: u32) -> GpuResult<()> {
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| GpuError::internal("Invalid stream ID"))?;

        stream.in_use = false;
        Ok(())
    }

    /// Get available stream.
    pub fn get_available_stream(&self) -> Option<u32> {
        self.streams
            .values()
            .filter(|s| !s.in_use)
            .max_by_key(|s| s.priority)
            .map(|s| s.id)
    }

    /// Destroy a stream.
    pub fn destroy_stream(&mut self, stream_id: u32) {
        self.streams.remove(&stream_id);
    }

    /// Get number of active streams.
    pub fn active_streams(&self) -> usize {
        self.streams.values().filter(|s| s.in_use).count()
    }
}

impl Default for CudaStreamManager {
    fn default() -> Self {
        Self::new()
    }
}

/// CUDA memory optimization utilities.
pub struct CudaMemoryOptimizer {
    compute_capability: ComputeCapability,
}

impl CudaMemoryOptimizer {
    /// Create a new memory optimizer.
    pub fn new(compute_capability: ComputeCapability) -> Self {
        Self { compute_capability }
    }

    /// Calculate optimal memory alignment.
    pub fn calculate_alignment(&self) -> u64 {
        // CUDA prefers 128-byte alignment for coalesced access
        128
    }

    /// Calculate optimal memory access pattern.
    pub fn optimize_access_pattern(&self, width: u32, height: u32) -> AccessPattern {
        AccessPattern {
            width,
            height,
            block_size: (16, 16, 1), // Common 2D block size
            stride: width,
        }
    }

    /// Estimate shared memory usage.
    pub fn estimate_shared_memory(&self, block_size: (u32, u32, u32), element_size: u64) -> u64 {
        let total_threads = block_size.0 as u64 * block_size.1 as u64 * block_size.2 as u64;
        total_threads * element_size
    }

    /// Check if shared memory size is valid.
    pub fn is_valid_shared_memory(&self, size: u64) -> bool {
        size <= self.compute_capability.max_shared_memory()
    }
}

/// Memory access pattern for CUDA kernels.
#[derive(Debug, Clone)]
pub struct AccessPattern {
    /// Width of the data.
    pub width: u32,
    /// Height of the data.
    pub height: u32,
    /// Block size for kernel launch.
    pub block_size: (u32, u32, u32),
    /// Stride for memory access.
    pub stride: u32,
}

/// Warp-level primitive operations.
pub struct WarpPrimitives;

impl WarpPrimitives {
    /// Generate shader code for warp shuffle.
    pub fn warp_shuffle_shader() -> &'static str {
        r#"
// Warp shuffle operation (emulated in WGSL)
fn warp_shuffle(value: f32, src_lane: u32) -> f32 {
    // In actual CUDA, this would use __shfl_sync
    // In WGSL, this is a placeholder that needs workgroup memory
    return value;
}

fn warp_shuffle_down(value: f32, delta: u32) -> f32 {
    // Placeholder for __shfl_down_sync
    return value;
}

fn warp_shuffle_up(value: f32, delta: u32) -> f32 {
    // Placeholder for __shfl_up_sync
    return value;
}

fn warp_shuffle_xor(value: f32, mask: u32) -> f32 {
    // Placeholder for __shfl_xor_sync
    return value;
}
"#
    }

    /// Generate shader code for warp reduce.
    pub fn warp_reduce_shader() -> &'static str {
        r#"
// Warp-level reduction
fn warp_reduce_sum(value: f32) -> f32 {
    var result = value;
    // Unrolled butterfly reduction
    result += warp_shuffle_down(result, 16u);
    result += warp_shuffle_down(result, 8u);
    result += warp_shuffle_down(result, 4u);
    result += warp_shuffle_down(result, 2u);
    result += warp_shuffle_down(result, 1u);
    return result;
}

fn warp_reduce_max(value: f32) -> f32 {
    var result = value;
    result = max(result, warp_shuffle_down(result, 16u));
    result = max(result, warp_shuffle_down(result, 8u));
    result = max(result, warp_shuffle_down(result, 4u));
    result = max(result, warp_shuffle_down(result, 2u));
    result = max(result, warp_shuffle_down(result, 1u));
    return result;
}

fn warp_reduce_min(value: f32) -> f32 {
    var result = value;
    result = min(result, warp_shuffle_down(result, 16u));
    result = min(result, warp_shuffle_down(result, 8u));
    result = min(result, warp_shuffle_down(result, 4u));
    result = min(result, warp_shuffle_down(result, 2u));
    result = min(result, warp_shuffle_down(result, 1u));
    return result;
}
"#
    }

    /// Generate shader code for warp vote.
    pub fn warp_vote_shader() -> &'static str {
        r#"
// Warp vote operations
fn warp_all(predicate: bool) -> bool {
    // Placeholder for __all_sync
    return predicate;
}

fn warp_any(predicate: bool) -> bool {
    // Placeholder for __any_sync
    return predicate;
}

fn warp_ballot(predicate: bool) -> u32 {
    // Placeholder for __ballot_sync
    return select(0u, 1u, predicate);
}
"#
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_capability() {
        let cc = ComputeCapability::new(7, 5);
        assert!(cc.supports_tensor_cores());
        assert!(cc.supports_rt_cores());
        assert_eq!(cc.max_threads_per_block(), 1024);
        assert_eq!(cc.warp_size(), 32);
    }

    #[test]
    fn test_cuda_optimizer() {
        let cc = ComputeCapability::new(8, 0);
        let optimizer = CudaOptimizer::new(cc, CudaOptimizationConfig::default());

        let shader = "fn main() {}";
        let optimized = optimizer.optimize_shader(shader);

        assert!(optimized.contains("WARP_SIZE"));
        assert!(optimized.contains("MAX_SHARED_MEMORY"));
    }

    #[test]
    fn test_stream_manager() {
        let mut manager = CudaStreamManager::new();

        let stream1 = manager.create_stream(0);
        let _stream2 = manager.create_stream(1);

        assert!(manager.acquire_stream(stream1).is_ok());
        assert!(manager.acquire_stream(stream1).is_err()); // Already in use

        assert!(manager.release_stream(stream1).is_ok());
        assert!(manager.acquire_stream(stream1).is_ok());
    }

    #[test]
    fn test_memory_optimizer() {
        let cc = ComputeCapability::new(7, 0);
        let optimizer = CudaMemoryOptimizer::new(cc);

        assert_eq!(optimizer.calculate_alignment(), 128);

        let shared_mem = optimizer.estimate_shared_memory((256, 1, 1), 4);
        assert_eq!(shared_mem, 1024);

        assert!(optimizer.is_valid_shared_memory(48 * 1024));
        assert!(!optimizer.is_valid_shared_memory(128 * 1024));
    }
}
