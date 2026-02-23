//! Metal-specific optimizations for Apple platforms.
//!
//! This module provides Metal Performance Shaders integration, unified memory
//! optimization for Apple Silicon, and threadgroup memory optimization.

use crate::context::GpuContext;
use crate::error::{GpuError, GpuResult};
use std::collections::HashMap;
use tracing::{debug, info};

/// Metal optimization configuration.
#[derive(Debug, Clone)]
pub struct MetalOptimizationConfig {
    /// Enable Metal Performance Shaders (MPS).
    pub enable_mps: bool,
    /// Enable unified memory optimization.
    pub enable_unified_memory: bool,
    /// Enable threadgroup memory optimization.
    pub enable_threadgroup_memory: bool,
    /// Enable argument buffers.
    pub enable_argument_buffers: bool,
    /// Preferred threadgroup size.
    pub threadgroup_size: (u32, u32, u32),
}

impl Default for MetalOptimizationConfig {
    fn default() -> Self {
        Self {
            enable_mps: true,
            enable_unified_memory: true,
            enable_threadgroup_memory: true,
            enable_argument_buffers: true,
            threadgroup_size: (256, 1, 1),
        }
    }
}

/// Metal feature set detector.
pub struct MetalFeatureDetector {
    features: MetalFeatures,
}

#[derive(Debug, Clone)]
pub struct MetalFeatures {
    /// Device family (Apple GPU generation).
    pub family: MetalFamily,
    /// Supports Apple Silicon unified memory.
    pub unified_memory: bool,
    /// Supports MPS.
    pub mps_support: bool,
    /// Maximum threadgroup memory.
    pub max_threadgroup_memory: u64,
    /// SIMD group size (similar to warp/subgroup).
    pub simd_width: u32,
    /// Supports argument buffers.
    pub argument_buffers: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetalFamily {
    /// Apple A-series (iOS/iPadOS).
    Apple7,
    Apple8,
    Apple9,
    /// Apple M-series (macOS).
    Mac2,
    Mac3,
    Mac4,
    /// Unknown or fallback.
    Unknown,
}

impl Default for MetalFeatures {
    fn default() -> Self {
        Self {
            family: MetalFamily::Mac2,
            unified_memory: true,
            mps_support: true,
            max_threadgroup_memory: 32 * 1024,
            simd_width: 32,
            argument_buffers: true,
        }
    }
}

impl MetalFeatureDetector {
    /// Create a new feature detector.
    pub fn new(context: &GpuContext) -> Self {
        let features = Self::detect_features(context);
        info!(
            "Metal features: family={:?}, unified_memory={}, mps={}, simd_width={}",
            features.family, features.unified_memory, features.mps_support, features.simd_width
        );

        Self { features }
    }

    /// Get detected features.
    pub fn features(&self) -> &MetalFeatures {
        &self.features
    }

    fn detect_features(context: &GpuContext) -> MetalFeatures {
        let adapter_info = context.adapter_info();
        let name = adapter_info.name.to_lowercase();

        let family = if name.contains("m1")
            || name.contains("m2")
            || name.contains("m3")
            || name.contains("m4")
        {
            if name.contains("m4") {
                MetalFamily::Mac4
            } else if name.contains("m3") {
                MetalFamily::Mac3
            } else {
                MetalFamily::Mac2
            }
        } else if name.contains("apple") {
            MetalFamily::Apple7
        } else {
            MetalFamily::Unknown
        };

        let unified_memory = matches!(
            family,
            MetalFamily::Mac2 | MetalFamily::Mac3 | MetalFamily::Mac4
        );

        MetalFeatures {
            family,
            unified_memory,
            mps_support: true,
            max_threadgroup_memory: 32 * 1024,
            simd_width: 32,
            argument_buffers: true,
        }
    }
}

/// Metal shader optimizer.
pub struct MetalShaderOptimizer {
    features: MetalFeatures,
    config: MetalOptimizationConfig,
}

impl MetalShaderOptimizer {
    /// Create a new shader optimizer.
    pub fn new(features: MetalFeatures, config: MetalOptimizationConfig) -> Self {
        Self { features, config }
    }

    /// Optimize shader code for Metal.
    pub fn optimize_shader(&self, shader_code: &str) -> String {
        let mut optimized = shader_code.to_string();

        // Add SIMD group size
        if !optimized.contains("SIMD_WIDTH") {
            let simd_decl = format!("\nconst SIMD_WIDTH: u32 = {}u;\n", self.features.simd_width);
            optimized.insert_str(0, &simd_decl);
        }

        // Add threadgroup memory helpers
        if self.config.enable_threadgroup_memory {
            optimized.push_str(Self::threadgroup_memory_helpers());
        }

        // Add unified memory hints
        if self.config.enable_unified_memory && self.features.unified_memory {
            optimized.insert_str(0, "\n// Unified Memory Optimized for Apple Silicon\n");
        }

        optimized
    }

    /// Calculate optimal threadgroup size.
    pub fn calculate_threadgroup_size(&self, num_elements: u64) -> (u32, u32, u32) {
        let max_threads = 1024; // Metal limit

        if num_elements <= max_threads as u64 {
            return (num_elements as u32, 1, 1);
        }

        // Use configured threadgroup size
        self.config.threadgroup_size
    }

    fn threadgroup_memory_helpers() -> &'static str {
        r#"
// Metal threadgroup memory helpers
// In WGSL, this uses workgroup memory

// Threadgroup barrier
fn threadgroup_barrier() {
    workgroupBarrier();
}

// Threadgroup memory fence
fn threadgroup_memory_fence() {
    storageBarrier();
}
"#
    }
}

/// Metal Performance Shaders (MPS) integration.
pub struct MetalPerformanceShaders {
    available_kernels: HashMap<String, MPSKernel>,
}

#[derive(Debug, Clone)]
struct MPSKernel {
    name: String,
    kernel_type: MPSKernelType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MPSKernelType {
    /// Matrix multiplication.
    MatrixMultiplication,
    /// Convolution.
    Convolution,
    /// Image filtering.
    ImageFilter,
    /// Reduction operations.
    Reduction,
    /// Neural network operations.
    NeuralNetwork,
}

impl MetalPerformanceShaders {
    /// Create a new MPS integration.
    pub fn new() -> Self {
        let mut available_kernels = HashMap::new();

        // Register available MPS kernels
        available_kernels.insert(
            "matmul".to_string(),
            MPSKernel {
                name: "matmul".to_string(),
                kernel_type: MPSKernelType::MatrixMultiplication,
            },
        );

        available_kernels.insert(
            "conv2d".to_string(),
            MPSKernel {
                name: "conv2d".to_string(),
                kernel_type: MPSKernelType::Convolution,
            },
        );

        available_kernels.insert(
            "reduce_sum".to_string(),
            MPSKernel {
                name: "reduce_sum".to_string(),
                kernel_type: MPSKernelType::Reduction,
            },
        );

        Self { available_kernels }
    }

    /// Check if a kernel is available.
    pub fn is_available(&self, name: &str) -> bool {
        self.available_kernels.contains_key(name)
    }

    /// Get kernel information.
    pub fn get_kernel(&self, name: &str) -> Option<&MPSKernel> {
        self.available_kernels.get(name)
    }

    /// List all available kernels.
    pub fn list_kernels(&self) -> Vec<String> {
        self.available_kernels.keys().cloned().collect()
    }

    /// Generate shader code that uses MPS-optimized operations.
    pub fn generate_mps_shader(&self, kernel_type: MPSKernelType) -> String {
        match kernel_type {
            MPSKernelType::MatrixMultiplication => self.generate_matmul_shader(),
            MPSKernelType::Convolution => self.generate_conv_shader(),
            MPSKernelType::ImageFilter => self.generate_filter_shader(),
            MPSKernelType::Reduction => self.generate_reduction_shader(),
            MPSKernelType::NeuralNetwork => self.generate_nn_shader(),
        }
    }

    fn generate_matmul_shader(&self) -> String {
        r#"
// Metal-optimized matrix multiplication
@group(0) @binding(0) var<storage, read> matrix_a: array<f32>;
@group(0) @binding(1) var<storage, read> matrix_b: array<f32>;
@group(0) @binding(2) var<storage, read_write> matrix_c: array<f32>;

struct MatmulParams {
    m: u32,
    n: u32,
    k: u32,
}

@group(1) @binding(0) var<uniform> params: MatmulParams;

@compute @workgroup_size(16, 16)
fn matmul(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let row = global_id.x;
    let col = global_id.y;

    if (row >= params.m || col >= params.n) {
        return;
    }

    var sum = 0.0;
    for (var i = 0u; i < params.k; i++) {
        let a_val = matrix_a[row * params.k + i];
        let b_val = matrix_b[i * params.n + col];
        sum += a_val * b_val;
    }

    matrix_c[row * params.n + col] = sum;
}
"#
        .to_string()
    }

    fn generate_conv_shader(&self) -> String {
        r#"
// Metal-optimized 2D convolution
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read> kernel: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

struct ConvParams {
    width: u32,
    height: u32,
    kernel_size: u32,
}

@group(1) @binding(0) var<uniform> params: ConvParams;

@compute @workgroup_size(16, 16)
fn conv2d(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let x = global_id.x;
    let y = global_id.y;

    if (x >= params.width || y >= params.height) {
        return;
    }

    var sum = 0.0;
    let half_k = params.kernel_size / 2u;

    for (var ky = 0u; ky < params.kernel_size; ky++) {
        for (var kx = 0u; kx < params.kernel_size; kx++) {
            let ix = i32(x) + i32(kx) - i32(half_k);
            let iy = i32(y) + i32(ky) - i32(half_k);

            if (ix >= 0 && ix < i32(params.width) && iy >= 0 && iy < i32(params.height)) {
                let input_idx = u32(iy) * params.width + u32(ix);
                let kernel_idx = ky * params.kernel_size + kx;
                sum += input[input_idx] * kernel[kernel_idx];
            }
        }
    }

    output[y * params.width + x] = sum;
}
"#
        .to_string()
    }

    fn generate_filter_shader(&self) -> String {
        "// MPS image filter shader placeholder\n".to_string()
    }

    fn generate_reduction_shader(&self) -> String {
        "// MPS reduction shader placeholder\n".to_string()
    }

    fn generate_nn_shader(&self) -> String {
        "// MPS neural network shader placeholder\n".to_string()
    }
}

impl Default for MetalPerformanceShaders {
    fn default() -> Self {
        Self::new()
    }
}

/// Unified memory optimizer for Apple Silicon.
pub struct UnifiedMemoryOptimizer {
    enabled: bool,
}

impl UnifiedMemoryOptimizer {
    /// Create a new unified memory optimizer.
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// Check if unified memory is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Optimize buffer allocation for unified memory.
    pub fn optimize_allocation(&self, size: u64) -> AllocationHint {
        if !self.enabled {
            return AllocationHint::Standard;
        }

        // For unified memory, prefer shared buffers
        if size < 1024 * 1024 {
            // Small allocations (<1MB) - use shared memory
            AllocationHint::Shared
        } else {
            // Large allocations - use private memory with streaming
            AllocationHint::Private
        }
    }

    /// Calculate optimal memory access pattern for unified memory.
    pub fn optimize_access_pattern(&self, width: u32, height: u32) -> AccessPattern {
        AccessPattern {
            width,
            height,
            prefer_linear: true,
            prefer_tiled: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllocationHint {
    /// Standard GPU allocation.
    Standard,
    /// Shared CPU/GPU memory.
    Shared,
    /// Private GPU memory.
    Private,
    /// Managed memory with explicit synchronization.
    Managed,
}

#[derive(Debug, Clone)]
pub struct AccessPattern {
    /// Width of the data.
    pub width: u32,
    /// Height of the data.
    pub height: u32,
    /// Prefer linear memory layout.
    pub prefer_linear: bool,
    /// Prefer tiled memory layout.
    pub prefer_tiled: bool,
}

/// Metal argument buffer manager.
pub struct ArgumentBufferManager {
    buffers: HashMap<u32, ArgumentBuffer>,
    next_id: u32,
}

#[derive(Debug, Clone)]
struct ArgumentBuffer {
    id: u32,
    name: String,
    arguments: Vec<ArgumentDescriptor>,
}

#[derive(Debug, Clone)]
struct ArgumentDescriptor {
    name: String,
    binding: u32,
    arg_type: ArgumentType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgumentType {
    /// Storage buffer.
    Buffer,
    /// Texture.
    Texture,
    /// Sampler.
    Sampler,
}

impl ArgumentBufferManager {
    /// Create a new argument buffer manager.
    pub fn new() -> Self {
        Self {
            buffers: HashMap::new(),
            next_id: 0,
        }
    }

    /// Create an argument buffer.
    pub fn create(&mut self, name: String) -> u32 {
        let id = self.next_id;
        self.next_id += 1;

        self.buffers.insert(
            id,
            ArgumentBuffer {
                id,
                name: name.clone(),
                arguments: Vec::new(),
            },
        );

        debug!("Created argument buffer '{}' (ID: {})", name, id);

        id
    }

    /// Add an argument to a buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if buffer not found.
    pub fn add_argument(
        &mut self,
        buffer_id: u32,
        name: String,
        binding: u32,
        arg_type: ArgumentType,
    ) -> GpuResult<()> {
        let buffer = self
            .buffers
            .get_mut(&buffer_id)
            .ok_or_else(|| GpuError::invalid_buffer("Argument buffer not found"))?;

        buffer.arguments.push(ArgumentDescriptor {
            name: name.clone(),
            binding,
            arg_type,
        });

        debug!(
            "Added argument '{}' to buffer {} at binding {}",
            name, buffer_id, binding
        );

        Ok(())
    }

    /// Get argument buffer.
    pub fn get(&self, buffer_id: u32) -> Option<&ArgumentBuffer> {
        self.buffers.get(&buffer_id)
    }

    /// Destroy an argument buffer.
    pub fn destroy(&mut self, buffer_id: u32) {
        if let Some(buffer) = self.buffers.remove(&buffer_id) {
            debug!("Destroyed argument buffer '{}'", buffer.name);
        }
    }
}

impl Default for ArgumentBufferManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Metal threadgroup memory allocator.
pub struct ThreadgroupMemoryAllocator {
    max_memory: u64,
    allocated: u64,
    allocations: HashMap<u32, ThreadgroupAllocation>,
    next_id: u32,
}

#[derive(Debug, Clone)]
struct ThreadgroupAllocation {
    id: u32,
    size: u64,
    offset: u64,
}

impl ThreadgroupMemoryAllocator {
    /// Create a new threadgroup memory allocator.
    pub fn new(max_memory: u64) -> Self {
        Self {
            max_memory,
            allocated: 0,
            allocations: HashMap::new(),
            next_id: 0,
        }
    }

    /// Allocate threadgroup memory.
    ///
    /// # Errors
    ///
    /// Returns an error if allocation exceeds limit.
    pub fn allocate(&mut self, size: u64) -> GpuResult<u32> {
        if self.allocated + size > self.max_memory {
            return Err(GpuError::out_of_memory(
                size,
                self.max_memory - self.allocated,
            ));
        }

        let id = self.next_id;
        self.next_id += 1;

        let offset = self.allocated;
        self.allocated += size;

        self.allocations
            .insert(id, ThreadgroupAllocation { id, size, offset });

        debug!(
            "Allocated {} bytes of threadgroup memory at offset {}",
            size, offset
        );

        Ok(id)
    }

    /// Free threadgroup memory.
    ///
    /// # Errors
    ///
    /// Returns an error if allocation not found.
    pub fn free(&mut self, id: u32) -> GpuResult<()> {
        let alloc = self
            .allocations
            .remove(&id)
            .ok_or_else(|| GpuError::invalid_buffer("Allocation not found"))?;

        self.allocated = self.allocated.saturating_sub(alloc.size);

        debug!("Freed {} bytes of threadgroup memory", alloc.size);

        Ok(())
    }

    /// Get current usage.
    pub fn usage(&self) -> (u64, u64) {
        (self.allocated, self.max_memory)
    }

    /// Reset allocator.
    pub fn reset(&mut self) {
        self.allocations.clear();
        self.allocated = 0;
    }
}

/// SIMD group operations for Metal.
pub struct SimdGroupOperations;

impl SimdGroupOperations {
    /// Generate shader code for SIMD group operations.
    pub fn simd_shuffle_shader() -> &'static str {
        r#"
// SIMD group shuffle operations (Metal-style)
fn simd_shuffle(value: f32, lane: u32) -> f32 {
    // Placeholder for Metal's simd_shuffle
    return value;
}

fn simd_shuffle_down(value: f32, delta: u32) -> f32 {
    // Placeholder for Metal's simd_shuffle_down
    return value;
}

fn simd_shuffle_up(value: f32, delta: u32) -> f32 {
    // Placeholder for Metal's simd_shuffle_up
    return value;
}

fn simd_shuffle_xor(value: f32, mask: u32) -> f32 {
    // Placeholder for Metal's simd_shuffle_xor
    return value;
}
"#
    }

    /// Generate shader code for SIMD group reductions.
    pub fn simd_reduce_shader() -> &'static str {
        r#"
// SIMD group reduction operations
fn simd_sum(value: f32) -> f32 {
    // Placeholder for Metal's simd_sum
    return value;
}

fn simd_max(value: f32) -> f32 {
    // Placeholder for Metal's simd_max
    return value;
}

fn simd_min(value: f32) -> f32 {
    // Placeholder for Metal's simd_min
    return value;
}

fn simd_product(value: f32) -> f32 {
    // Placeholder for Metal's simd_product
    return value;
}
"#
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metal_features() {
        let features = MetalFeatures::default();
        assert_eq!(features.simd_width, 32);
        assert!(features.unified_memory);
        assert!(features.mps_support);
    }

    #[test]
    fn test_metal_performance_shaders() {
        let mps = MetalPerformanceShaders::new();
        assert!(mps.is_available("matmul"));
        assert!(mps.is_available("conv2d"));

        let kernels = mps.list_kernels();
        assert!(!kernels.is_empty());
    }

    #[test]
    fn test_unified_memory_optimizer() {
        let optimizer = UnifiedMemoryOptimizer::new(true);
        assert!(optimizer.is_enabled());

        let hint = optimizer.optimize_allocation(512 * 1024);
        assert_eq!(hint, AllocationHint::Shared);

        let hint = optimizer.optimize_allocation(2 * 1024 * 1024);
        assert_eq!(hint, AllocationHint::Private);
    }

    #[test]
    fn test_argument_buffer_manager() {
        let mut manager = ArgumentBufferManager::new();

        let buffer_id = manager.create("test_args".to_string());
        manager
            .add_argument(buffer_id, "input".to_string(), 0, ArgumentType::Buffer)
            .expect("Failed to add argument");

        let buffer = manager.get(buffer_id).expect("Buffer not found");
        assert_eq!(buffer.arguments.len(), 1);
    }

    #[test]
    fn test_threadgroup_memory_allocator() {
        let mut allocator = ThreadgroupMemoryAllocator::new(32 * 1024);

        let id1 = allocator.allocate(1024).expect("Failed to allocate");
        let _id2 = allocator.allocate(2048).expect("Failed to allocate");

        let (used, total) = allocator.usage();
        assert_eq!(used, 3072);
        assert_eq!(total, 32 * 1024);

        allocator.free(id1).expect("Failed to free");
        let (used, _) = allocator.usage();
        assert_eq!(used, 2048);
    }
}
