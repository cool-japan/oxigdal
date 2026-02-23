//! Advanced GPU computing for geospatial operations with multi-GPU support.
//!
//! This crate provides advanced GPU computing capabilities for OxiGDAL including:
//! - Multi-GPU orchestration and load balancing
//! - Advanced memory pool management
//! - Shader compilation and optimization
//! - GPU-accelerated terrain analysis
//! - GPU-based ML inference
//!
//! # Features
//!
//! - **Multi-GPU Support**: Automatically detect and utilize multiple GPUs
//! - **Memory Pooling**: Efficient GPU memory management with sub-allocation
//! - **Shader Optimization**: Compile and optimize WGSL shaders
//! - **Work Stealing**: Dynamic load balancing across GPUs
//! - **GPU Affinity**: Thread-to-GPU pinning for optimal performance
//!
//! # Examples
//!
//! ```rust,no_run
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! use oxigdal_gpu_advanced::multi_gpu::{MultiGpuManager, SelectionStrategy};
//!
//! // Create multi-GPU manager
//! let manager = MultiGpuManager::new(SelectionStrategy::LeastLoaded).await?;
//!
//! // Print available GPUs
//! manager.print_gpu_info();
//!
//! // Select best GPU for task
//! let gpu = manager.select_gpu()?;
//! println!("Selected GPU: {}", gpu.info.name);
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]
#![deny(clippy::unwrap_used, clippy::panic)]

pub mod adaptive;
pub mod error;
pub mod gpu_ml;
pub mod gpu_terrain;
pub mod kernels;
pub mod memory_compaction;
pub mod memory_pool;
pub mod multi_gpu;
pub mod pipeline_builder;
pub mod profiling;
pub mod shader_compiler;

// Re-exports
pub use adaptive::{
    AdaptiveConfig, AdaptiveSelector, Algorithm, AlgorithmStats, DeviceInfo, ExecutionStrategy,
    TuningParams, WorkloadInfo,
};
pub use error::{GpuAdvancedError, Result};
pub use gpu_ml::{ActivationType, GpuMlInference, InferenceStats, PoolType};
pub use gpu_terrain::{GpuTerrainAnalyzer, TerrainMetrics};
pub use kernels::{
    EdgeDetectionKernel, FftKernel, HistogramEqKernel, KernelParams, KernelRegistry,
    MatrixMultiplyKernel, MorphologyKernel, TextureAnalysisKernel,
};
pub use memory_compaction::{
    CompactionConfig, CompactionResult, CompactionStats, CompactionStrategy, FragmentationInfo,
    MemoryCompactor,
};
pub use memory_pool::{MemoryAllocation, MemoryPool, MemoryPoolStats};
pub use multi_gpu::{
    GpuDevice, GpuDeviceInfo, MultiGpuManager, SelectionStrategy,
    affinity::{AffinityGuard, AffinityManager, AffinityStats},
    device_manager::{DeviceCapabilities, DeviceFilter, DeviceManager, DevicePerformanceClass},
    load_balancer::{LoadBalancer, LoadStats},
    sync::{Barrier, Event, Fence, GpuSemaphore, SemaphoreGuard, SyncManager, SyncStats},
    work_queue::{BatchSubmitter, WorkQueue, WorkStealingQueue},
};
pub use pipeline_builder::{
    Pipeline, PipelineBuilder, PipelineConfig, PipelineInfo, PipelineStage,
};
pub use profiling::{
    BottleneckKind, BottleneckSeverity, GpuProfiler, KernelStats, PerformanceBottleneck,
    ProfileSession, ProfilingConfig, ProfilingMetrics, ProfilingReport,
};
pub use shader_compiler::{
    CompiledShader, CompilerStats, ShaderCompiler, ShaderPreprocessor,
    analyzer::{PerformanceClass, ShaderAnalysis, ShaderAnalyzer},
    cache::{CacheStats, ShaderCache},
    optimizer::{OptimizationConfig, OptimizationLevel, OptimizationMetrics, ShaderOptimizer},
};

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Library name
pub const NAME: &str = env!("CARGO_PKG_NAME");

/// Get library information
pub fn info() -> LibraryInfo {
    LibraryInfo {
        name: NAME.to_string(),
        version: VERSION.to_string(),
        description: "Advanced GPU computing with multi-GPU support for OxiGDAL".to_string(),
    }
}

/// Library information
#[derive(Debug, Clone)]
pub struct LibraryInfo {
    /// Library name
    pub name: String,
    /// Library version
    pub version: String,
    /// Library description
    pub description: String,
}

impl LibraryInfo {
    /// Print library information
    pub fn print(&self) {
        println!("{} v{}", self.name, self.version);
        println!("{}", self.description);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_library_info() {
        let info = info();
        assert_eq!(info.name, NAME);
        assert_eq!(info.version, VERSION);
        assert!(!info.description.is_empty());
    }

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
        assert!(VERSION.contains('.'));
    }
}
