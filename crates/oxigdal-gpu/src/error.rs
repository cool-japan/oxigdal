//! GPU error types for OxiGDAL.
//!
//! This module provides comprehensive error handling for GPU operations,
//! including device initialization, buffer management, shader compilation,
//! and compute execution errors.

use thiserror::Error;

/// Result type for GPU operations.
pub type GpuResult<T> = Result<T, GpuError>;

/// Errors that can occur during GPU operations.
#[derive(Debug, Error)]
pub enum GpuError {
    /// No suitable GPU adapter found.
    #[error("No suitable GPU adapter found. Backends tried: {backends}")]
    NoAdapter { backends: String },

    /// Failed to request GPU device.
    #[error("Failed to request GPU device: {reason}")]
    DeviceRequest { reason: String },

    /// GPU device lost or disconnected.
    #[error("GPU device lost: {reason}")]
    DeviceLost { reason: String },

    /// Out of GPU memory.
    #[error("Out of GPU memory: requested {requested} bytes, available {available} bytes")]
    OutOfMemory { requested: u64, available: u64 },

    /// Invalid buffer size or alignment.
    #[error("Invalid buffer: {reason}")]
    InvalidBuffer { reason: String },

    /// Shader compilation error.
    #[error("Shader compilation failed: {message}")]
    ShaderCompilation { message: String },

    /// Shader validation error.
    #[error("Shader validation failed: {message}")]
    ShaderValidation { message: String },

    /// Compute pipeline creation error.
    #[error("Failed to create compute pipeline: {reason}")]
    PipelineCreation { reason: String },

    /// Bind group creation error.
    #[error("Failed to create bind group: {reason}")]
    BindGroupCreation { reason: String },

    /// Buffer mapping error.
    #[error("Failed to map buffer: {reason}")]
    BufferMapping { reason: String },

    /// Compute execution timeout.
    #[error("Compute execution timeout after {seconds} seconds")]
    ExecutionTimeout { seconds: u64 },

    /// Compute execution error.
    #[error("Compute execution failed: {reason}")]
    ExecutionFailed { reason: String },

    /// Invalid workgroup size.
    #[error("Invalid workgroup size: {actual}, max allowed: {max}")]
    InvalidWorkgroupSize { actual: u32, max: u32 },

    /// Incompatible data types.
    #[error("Incompatible data types: expected {expected}, got {actual}")]
    IncompatibleTypes { expected: String, actual: String },

    /// Invalid kernel parameters.
    #[error("Invalid kernel parameters: {reason}")]
    InvalidKernelParams { reason: String },

    /// Raster dimension mismatch.
    #[error(
        "Raster dimension mismatch: expected {expected_width}x{expected_height}, \
         got {actual_width}x{actual_height}"
    )]
    DimensionMismatch {
        expected_width: u32,
        expected_height: u32,
        actual_width: u32,
        actual_height: u32,
    },

    /// Unsupported operation on current GPU.
    #[error("Unsupported operation on current GPU: {operation}")]
    UnsupportedOperation { operation: String },

    /// Backend not available.
    #[error("Backend {backend} not available on this platform")]
    BackendNotAvailable { backend: String },

    /// Core library error.
    #[error("Core library error: {0}")]
    Core(#[from] oxigdal_core::error::OxiGdalError),

    /// IO error during GPU operations.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Async task join error.
    #[error("Async task failed: {0}")]
    TaskJoin(String),

    /// Internal error (should not happen).
    #[error("Internal GPU error: {0}")]
    Internal(String),
}

impl GpuError {
    /// Create a new adapter not found error.
    pub fn no_adapter(backends: impl Into<String>) -> Self {
        Self::NoAdapter {
            backends: backends.into(),
        }
    }

    /// Create a new device request error.
    pub fn device_request(reason: impl Into<String>) -> Self {
        Self::DeviceRequest {
            reason: reason.into(),
        }
    }

    /// Create a new device lost error.
    pub fn device_lost(reason: impl Into<String>) -> Self {
        Self::DeviceLost {
            reason: reason.into(),
        }
    }

    /// Create a new out of memory error.
    pub fn out_of_memory(requested: u64, available: u64) -> Self {
        Self::OutOfMemory {
            requested,
            available,
        }
    }

    /// Create a new invalid buffer error.
    pub fn invalid_buffer(reason: impl Into<String>) -> Self {
        Self::InvalidBuffer {
            reason: reason.into(),
        }
    }

    /// Create a new shader compilation error.
    pub fn shader_compilation(message: impl Into<String>) -> Self {
        Self::ShaderCompilation {
            message: message.into(),
        }
    }

    /// Create a new shader validation error.
    pub fn shader_validation(message: impl Into<String>) -> Self {
        Self::ShaderValidation {
            message: message.into(),
        }
    }

    /// Create a new pipeline creation error.
    pub fn pipeline_creation(reason: impl Into<String>) -> Self {
        Self::PipelineCreation {
            reason: reason.into(),
        }
    }

    /// Create a new bind group creation error.
    pub fn bind_group_creation(reason: impl Into<String>) -> Self {
        Self::BindGroupCreation {
            reason: reason.into(),
        }
    }

    /// Create a new buffer mapping error.
    pub fn buffer_mapping(reason: impl Into<String>) -> Self {
        Self::BufferMapping {
            reason: reason.into(),
        }
    }

    /// Create a new execution timeout error.
    pub fn execution_timeout(seconds: u64) -> Self {
        Self::ExecutionTimeout { seconds }
    }

    /// Create a new execution failed error.
    pub fn execution_failed(reason: impl Into<String>) -> Self {
        Self::ExecutionFailed {
            reason: reason.into(),
        }
    }

    /// Create a new invalid workgroup size error.
    pub fn invalid_workgroup_size(actual: u32, max: u32) -> Self {
        Self::InvalidWorkgroupSize { actual, max }
    }

    /// Create a new incompatible types error.
    pub fn incompatible_types(expected: impl Into<String>, actual: impl Into<String>) -> Self {
        Self::IncompatibleTypes {
            expected: expected.into(),
            actual: actual.into(),
        }
    }

    /// Create a new invalid kernel parameters error.
    pub fn invalid_kernel_params(reason: impl Into<String>) -> Self {
        Self::InvalidKernelParams {
            reason: reason.into(),
        }
    }

    /// Create a new dimension mismatch error.
    pub fn dimension_mismatch(
        expected_width: u32,
        expected_height: u32,
        actual_width: u32,
        actual_height: u32,
    ) -> Self {
        Self::DimensionMismatch {
            expected_width,
            expected_height,
            actual_width,
            actual_height,
        }
    }

    /// Create a new unsupported operation error.
    pub fn unsupported_operation(operation: impl Into<String>) -> Self {
        Self::UnsupportedOperation {
            operation: operation.into(),
        }
    }

    /// Create a new backend not available error.
    pub fn backend_not_available(backend: impl Into<String>) -> Self {
        Self::BackendNotAvailable {
            backend: backend.into(),
        }
    }

    /// Create a new internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    /// Check if this error is recoverable.
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::ExecutionTimeout { .. }
                | Self::BufferMapping { .. }
                | Self::InvalidKernelParams { .. }
        )
    }

    /// Check if this error suggests falling back to CPU.
    pub fn should_fallback_to_cpu(&self) -> bool {
        matches!(
            self,
            Self::NoAdapter { .. }
                | Self::DeviceLost { .. }
                | Self::OutOfMemory { .. }
                | Self::UnsupportedOperation { .. }
                | Self::BackendNotAvailable { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = GpuError::no_adapter("Vulkan, Metal, DX12");
        assert!(matches!(err, GpuError::NoAdapter { .. }));
        assert!(err.should_fallback_to_cpu());

        let err = GpuError::out_of_memory(1_000_000_000, 500_000_000);
        assert!(matches!(err, GpuError::OutOfMemory { .. }));
        assert!(err.should_fallback_to_cpu());
    }

    #[test]
    fn test_recoverable_errors() {
        let err = GpuError::execution_timeout(30);
        assert!(err.is_recoverable());

        let err = GpuError::device_lost("GPU reset");
        assert!(!err.is_recoverable());
    }

    #[test]
    fn test_error_messages() {
        let err = GpuError::dimension_mismatch(1024, 768, 512, 512);
        let msg = err.to_string();
        assert!(msg.contains("1024"));
        assert!(msg.contains("768"));
        assert!(msg.contains("512"));
    }
}
