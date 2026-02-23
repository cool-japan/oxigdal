//! Error types for advanced GPU operations.

use thiserror::Error;

/// Result type for advanced GPU operations
pub type Result<T> = std::result::Result<T, GpuAdvancedError>;

/// Error types for advanced GPU operations
#[derive(Debug, Error)]
pub enum GpuAdvancedError {
    /// GPU device error
    #[error("GPU device error: {0}")]
    DeviceError(String),

    /// Multi-GPU error
    #[error("Multi-GPU error: {0}")]
    MultiGpuError(String),

    /// Memory pool error
    #[error("Memory pool error: {0}")]
    MemoryPoolError(String),

    /// Memory allocation failed
    #[error("Memory allocation failed: size={size}, available={available}")]
    AllocationFailed {
        /// Requested allocation size
        size: u64,
        /// Available memory
        available: u64,
    },

    /// Memory out of bounds
    #[error("Memory out of bounds: offset={offset}, size={size}, pool_size={pool_size}")]
    OutOfBounds {
        /// Offset into memory pool
        offset: u64,
        /// Requested size
        size: u64,
        /// Total pool size
        pool_size: u64,
    },

    /// Shader compiler error
    #[error("Shader compiler error: {0}")]
    ShaderCompilerError(String),

    /// Shader optimization error
    #[error("Shader optimization error: {0}")]
    ShaderOptimizationError(String),

    /// Shader validation error
    #[error("Shader validation error: {0}")]
    ShaderValidationError(String),

    /// Shader cache error
    #[error("Shader cache error: {0}")]
    ShaderCacheError(String),

    /// GPU computation error
    #[error("GPU computation error: {0}")]
    ComputationError(String),

    /// Buffer error
    #[error("Buffer error: {0}")]
    BufferError(String),

    /// Synchronization error
    #[error("Synchronization error: {0}")]
    SyncError(String),

    /// Work stealing error
    #[error("Work stealing error: {0}")]
    WorkStealingError(String),

    /// Load balancing error
    #[error("Load balancing error: {0}")]
    LoadBalancingError(String),

    /// GPU not found
    #[error("No GPU found matching criteria: {0}")]
    GpuNotFound(String),

    /// Invalid GPU index
    #[error("Invalid GPU index: {index}, total GPUs: {total}")]
    InvalidGpuIndex {
        /// Requested GPU index
        index: usize,
        /// Total number of available GPUs
        total: usize,
    },

    /// Shader not found
    #[error("Shader not found: {0}")]
    ShaderNotFound(String),

    /// ML inference error
    #[error("ML inference error: {0}")]
    MlInferenceError(String),

    /// Terrain analysis error
    #[error("Terrain analysis error: {0}")]
    TerrainAnalysisError(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    /// Base GPU error
    #[error("Base GPU error: {0}")]
    GpuError(#[from] oxigdal_gpu::error::GpuError),

    /// WGPU request device error
    #[error("WGPU request device error: {0}")]
    RequestDeviceError(#[from] wgpu::RequestDeviceError),

    /// WGPU buffer async error
    #[error("WGPU buffer async error: {0}")]
    BufferAsyncError(#[from] wgpu::BufferAsyncError),

    /// Invalid parameter
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    /// Not implemented
    #[error("Feature not implemented: {0}")]
    NotImplemented(String),
}

impl GpuAdvancedError {
    /// Create a device error
    pub fn device_error(msg: impl Into<String>) -> Self {
        Self::DeviceError(msg.into())
    }

    /// Create a multi-GPU error
    pub fn multi_gpu_error(msg: impl Into<String>) -> Self {
        Self::MultiGpuError(msg.into())
    }

    /// Create a memory pool error
    pub fn memory_pool_error(msg: impl Into<String>) -> Self {
        Self::MemoryPoolError(msg.into())
    }

    /// Create a shader compiler error
    pub fn shader_compiler_error(msg: impl Into<String>) -> Self {
        Self::ShaderCompilerError(msg.into())
    }

    /// Create a computation error
    pub fn computation_error(msg: impl Into<String>) -> Self {
        Self::ComputationError(msg.into())
    }

    /// Create an invalid parameter error
    pub fn invalid_parameter(msg: impl Into<String>) -> Self {
        Self::InvalidParameter(msg.into())
    }
}
