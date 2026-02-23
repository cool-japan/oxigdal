//! WASM-specific error types and handling
//!
//! This module provides comprehensive error handling for WebAssembly operations,
//! including fetch errors, canvas errors, worker errors, and tile cache errors.

use oxigdal_core::error::OxiGdalError;
use std::fmt;
use wasm_bindgen::prelude::*;

/// Result type for WASM operations
pub type WasmResult<T> = std::result::Result<T, WasmError>;

/// Comprehensive WASM error types
#[derive(Debug, Clone)]
pub enum WasmError {
    /// Fetch API errors
    Fetch(FetchError),

    /// Canvas rendering errors
    Canvas(CanvasError),

    /// Web Worker errors
    Worker(WorkerError),

    /// Tile cache errors
    TileCache(TileCacheError),

    /// JavaScript interop errors
    JsInterop(JsInteropError),

    /// OxiGDAL core errors
    OxiGdal(String),

    /// Invalid operation
    InvalidOperation {
        /// Operation description
        operation: String,
        /// Reason for invalidity
        reason: String,
    },

    /// Resource not found
    NotFound {
        /// Resource type
        resource: String,
        /// Resource identifier
        identifier: String,
    },

    /// Out of memory
    OutOfMemory {
        /// Requested size in bytes
        requested: usize,
        /// Available size in bytes
        available: Option<usize>,
    },

    /// Timeout error
    Timeout {
        /// Operation that timed out
        operation: String,
        /// Duration in milliseconds
        duration_ms: u64,
    },

    /// Format error
    Format {
        /// Expected format
        expected: String,
        /// Actual format
        actual: String,
    },
}

/// Fetch-related errors
#[derive(Debug, Clone)]
pub enum FetchError {
    /// Network request failed
    NetworkFailure {
        /// URL that failed
        url: String,
        /// Error message
        message: String,
    },

    /// HTTP error response
    HttpError {
        /// HTTP status code
        status: u16,
        /// Status text
        status_text: String,
        /// URL
        url: String,
    },

    /// CORS error
    CorsError {
        /// URL
        url: String,
        /// Details
        details: String,
    },

    /// Range request not supported
    RangeNotSupported {
        /// URL
        url: String,
    },

    /// Response parsing failed
    ParseError {
        /// Expected type
        expected: String,
        /// Error details
        message: String,
    },

    /// Request timeout
    Timeout {
        /// URL
        url: String,
        /// Timeout duration in milliseconds
        timeout_ms: u64,
    },

    /// Retry limit exceeded
    RetryLimitExceeded {
        /// URL
        url: String,
        /// Number of attempts
        attempts: u32,
    },

    /// Invalid response size
    InvalidSize {
        /// Expected size
        expected: u64,
        /// Actual size
        actual: u64,
    },
}

/// Canvas rendering errors
#[derive(Debug, Clone)]
pub enum CanvasError {
    /// Failed to create ImageData
    ImageDataCreation {
        /// Width
        width: u32,
        /// Height
        height: u32,
        /// Error message
        message: String,
    },

    /// Invalid dimensions
    InvalidDimensions {
        /// Width
        width: u32,
        /// Height
        height: u32,
        /// Reason
        reason: String,
    },

    /// Color space conversion failed
    ColorSpaceConversion {
        /// Source color space
        from: String,
        /// Target color space
        to: String,
        /// Error details
        details: String,
    },

    /// Buffer size mismatch
    BufferSizeMismatch {
        /// Expected size
        expected: usize,
        /// Actual size
        actual: usize,
    },

    /// Canvas context unavailable
    ContextUnavailable {
        /// Context type
        context_type: String,
    },

    /// Rendering operation failed
    RenderingFailed {
        /// Operation
        operation: String,
        /// Error message
        message: String,
    },

    /// Invalid parameter provided
    InvalidParameter(String),
}

/// Web Worker errors
#[derive(Debug, Clone)]
pub enum WorkerError {
    /// Worker creation failed
    CreationFailed {
        /// Error message
        message: String,
    },

    /// Worker terminated unexpectedly
    Terminated {
        /// Worker ID
        worker_id: u32,
    },

    /// Message posting failed
    PostMessageFailed {
        /// Worker ID
        worker_id: u32,
        /// Error details
        message: String,
    },

    /// Worker pool exhausted
    PoolExhausted {
        /// Pool size
        pool_size: usize,
        /// Pending jobs
        pending_jobs: usize,
    },

    /// Worker response timeout
    ResponseTimeout {
        /// Worker ID
        worker_id: u32,
        /// Job ID
        job_id: u64,
        /// Timeout duration in milliseconds
        timeout_ms: u64,
    },

    /// Invalid worker response
    InvalidResponse {
        /// Expected response type
        expected: String,
        /// Actual response
        actual: String,
    },
}

/// Tile cache errors
#[derive(Debug, Clone)]
pub enum TileCacheError {
    /// Cache miss
    Miss {
        /// Tile key
        key: String,
    },

    /// Cache full
    Full {
        /// Current size in bytes
        current_size: usize,
        /// Maximum size in bytes
        max_size: usize,
    },

    /// Invalid tile coordinates
    InvalidCoordinates {
        /// Level
        level: u32,
        /// X coordinate
        x: u32,
        /// Y coordinate
        y: u32,
        /// Reason
        reason: String,
    },

    /// Tile size mismatch
    SizeMismatch {
        /// Expected size
        expected: usize,
        /// Actual size
        actual: usize,
    },

    /// Eviction failed
    EvictionFailed {
        /// Error details
        message: String,
    },
}

/// JavaScript interop errors
#[derive(Debug, Clone)]
pub enum JsInteropError {
    /// Type conversion failed
    TypeConversion {
        /// Expected type
        expected: String,
        /// Actual type
        actual: String,
    },

    /// Property access failed
    PropertyAccess {
        /// Property name
        property: String,
        /// Error message
        message: String,
    },

    /// Function call failed
    FunctionCall {
        /// Function name
        function: String,
        /// Error message
        message: String,
    },

    /// Promise rejection
    PromiseRejection {
        /// Promise description
        promise: String,
        /// Rejection reason
        reason: String,
    },

    /// Invalid JsValue
    InvalidJsValue {
        /// Expected type
        expected: String,
        /// Error details
        details: String,
    },
}

impl fmt::Display for WasmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fetch(e) => write!(f, "Fetch error: {e}"),
            Self::Canvas(e) => write!(f, "Canvas error: {e}"),
            Self::Worker(e) => write!(f, "Worker error: {e}"),
            Self::TileCache(e) => write!(f, "Tile cache error: {e}"),
            Self::JsInterop(e) => write!(f, "JS interop error: {e}"),
            Self::OxiGdal(msg) => write!(f, "OxiGDAL error: {msg}"),
            Self::InvalidOperation { operation, reason } => {
                write!(f, "Invalid operation '{operation}': {reason}")
            }
            Self::NotFound {
                resource,
                identifier,
            } => {
                write!(f, "{resource} not found: {identifier}")
            }
            Self::OutOfMemory {
                requested,
                available,
            } => {
                if let Some(avail) = available {
                    write!(
                        f,
                        "Out of memory: requested {requested} bytes, {avail} available"
                    )
                } else {
                    write!(f, "Out of memory: requested {requested} bytes")
                }
            }
            Self::Timeout {
                operation,
                duration_ms,
            } => {
                write!(f, "Operation '{operation}' timed out after {duration_ms}ms")
            }
            Self::Format { expected, actual } => {
                write!(f, "Format error: expected {expected}, got {actual}")
            }
        }
    }
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NetworkFailure { url, message } => {
                write!(f, "Network failure for {url}: {message}")
            }
            Self::HttpError {
                status,
                status_text,
                url,
            } => {
                write!(f, "HTTP {status} {status_text} for {url}")
            }
            Self::CorsError { url, details } => {
                write!(f, "CORS error for {url}: {details}")
            }
            Self::RangeNotSupported { url } => {
                write!(f, "Range requests not supported for {url}")
            }
            Self::ParseError { expected, message } => {
                write!(f, "Parse error: expected {expected}, {message}")
            }
            Self::Timeout { url, timeout_ms } => {
                write!(f, "Request to {url} timed out after {timeout_ms}ms")
            }
            Self::RetryLimitExceeded { url, attempts } => {
                write!(
                    f,
                    "Retry limit exceeded for {url} after {attempts} attempts"
                )
            }
            Self::InvalidSize { expected, actual } => {
                write!(
                    f,
                    "Invalid response size: expected {expected}, got {actual}"
                )
            }
        }
    }
}

impl fmt::Display for CanvasError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ImageDataCreation {
                width,
                height,
                message,
            } => {
                write!(f, "Failed to create ImageData {width}x{height}: {message}")
            }
            Self::InvalidDimensions {
                width,
                height,
                reason,
            } => {
                write!(f, "Invalid dimensions {width}x{height}: {reason}")
            }
            Self::ColorSpaceConversion { from, to, details } => {
                write!(f, "Color space conversion {from} -> {to} failed: {details}")
            }
            Self::BufferSizeMismatch { expected, actual } => {
                write!(f, "Buffer size mismatch: expected {expected}, got {actual}")
            }
            Self::ContextUnavailable { context_type } => {
                write!(f, "Canvas context '{context_type}' unavailable")
            }
            Self::RenderingFailed { operation, message } => {
                write!(f, "Rendering operation '{operation}' failed: {message}")
            }
            Self::InvalidParameter(msg) => {
                write!(f, "Invalid parameter: {msg}")
            }
        }
    }
}

impl fmt::Display for WorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreationFailed { message } => {
                write!(f, "Worker creation failed: {message}")
            }
            Self::Terminated { worker_id } => {
                write!(f, "Worker {worker_id} terminated unexpectedly")
            }
            Self::PostMessageFailed { worker_id, message } => {
                write!(f, "Failed to post message to worker {worker_id}: {message}")
            }
            Self::PoolExhausted {
                pool_size,
                pending_jobs,
            } => {
                write!(
                    f,
                    "Worker pool exhausted: {pool_size} workers, {pending_jobs} pending jobs"
                )
            }
            Self::ResponseTimeout {
                worker_id,
                job_id,
                timeout_ms,
            } => {
                write!(
                    f,
                    "Worker {worker_id} job {job_id} timed out after {timeout_ms}ms"
                )
            }
            Self::InvalidResponse { expected, actual } => {
                write!(
                    f,
                    "Invalid worker response: expected {expected}, got {actual}"
                )
            }
        }
    }
}

impl fmt::Display for TileCacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Miss { key } => {
                write!(f, "Cache miss for tile {key}")
            }
            Self::Full {
                current_size,
                max_size,
            } => {
                write!(f, "Cache full: {current_size}/{max_size} bytes")
            }
            Self::InvalidCoordinates {
                level,
                x,
                y,
                reason,
            } => {
                write!(f, "Invalid tile coordinates ({level}, {x}, {y}): {reason}")
            }
            Self::SizeMismatch { expected, actual } => {
                write!(f, "Tile size mismatch: expected {expected}, got {actual}")
            }
            Self::EvictionFailed { message } => {
                write!(f, "Cache eviction failed: {message}")
            }
        }
    }
}

impl fmt::Display for JsInteropError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TypeConversion { expected, actual } => {
                write!(
                    f,
                    "Type conversion failed: expected {expected}, got {actual}"
                )
            }
            Self::PropertyAccess { property, message } => {
                write!(f, "Property access failed for '{property}': {message}")
            }
            Self::FunctionCall { function, message } => {
                write!(f, "Function call failed for '{function}': {message}")
            }
            Self::PromiseRejection { promise, reason } => {
                write!(f, "Promise rejected for '{promise}': {reason}")
            }
            Self::InvalidJsValue { expected, details } => {
                write!(f, "Invalid JsValue: expected {expected}, {details}")
            }
        }
    }
}

impl std::error::Error for WasmError {}
impl std::error::Error for FetchError {}
impl std::error::Error for CanvasError {}
impl std::error::Error for WorkerError {}
impl std::error::Error for TileCacheError {}
impl std::error::Error for JsInteropError {}

/// Convert `WasmError` to `JsValue` for WASM bindings
impl From<WasmError> for JsValue {
    fn from(err: WasmError) -> Self {
        JsValue::from_str(&err.to_string())
    }
}

/// Convert `OxiGdalError` to `WasmError`
impl From<OxiGdalError> for WasmError {
    fn from(err: OxiGdalError) -> Self {
        Self::OxiGdal(err.to_string())
    }
}

/// Convert `FetchError` to `WasmError`
impl From<FetchError> for WasmError {
    fn from(err: FetchError) -> Self {
        Self::Fetch(err)
    }
}

/// Convert `CanvasError` to `WasmError`
impl From<CanvasError> for WasmError {
    fn from(err: CanvasError) -> Self {
        Self::Canvas(err)
    }
}

/// Convert `WorkerError` to `WasmError`
impl From<WorkerError> for WasmError {
    fn from(err: WorkerError) -> Self {
        Self::Worker(err)
    }
}

/// Convert `TileCacheError` to `WasmError`
impl From<TileCacheError> for WasmError {
    fn from(err: TileCacheError) -> Self {
        Self::TileCache(err)
    }
}

/// Convert `JsInteropError` to `WasmError`
impl From<JsInteropError> for WasmError {
    fn from(err: JsInteropError) -> Self {
        Self::JsInterop(err)
    }
}

/// Helper to convert `JsValue` to `WasmError`
#[allow(dead_code)]
pub fn js_to_wasm_error(js_val: JsValue, context: &str) -> WasmError {
    let message = if let Some(s) = js_val.as_string() {
        s
    } else {
        format!("{js_val:?}")
    };

    WasmError::JsInterop(JsInteropError::FunctionCall {
        function: context.to_string(),
        message,
    })
}

/// Helper to convert errors to `JsValue`
#[allow(dead_code)]
pub fn to_js_value<E: std::fmt::Display>(err: E) -> JsValue {
    JsValue::from_str(&err.to_string())
}

/// Error builder for common patterns
#[allow(dead_code)]
pub struct WasmErrorBuilder;

#[allow(dead_code)]
impl WasmErrorBuilder {
    /// Create a fetch network failure error
    pub fn fetch_network(url: impl Into<String>, message: impl Into<String>) -> WasmError {
        WasmError::Fetch(FetchError::NetworkFailure {
            url: url.into(),
            message: message.into(),
        })
    }

    /// Create a fetch HTTP error
    pub fn fetch_http(
        status: u16,
        status_text: impl Into<String>,
        url: impl Into<String>,
    ) -> WasmError {
        WasmError::Fetch(FetchError::HttpError {
            status,
            status_text: status_text.into(),
            url: url.into(),
        })
    }

    /// Create a canvas ImageData creation error
    pub fn canvas_image_data(width: u32, height: u32, message: impl Into<String>) -> WasmError {
        WasmError::Canvas(CanvasError::ImageDataCreation {
            width,
            height,
            message: message.into(),
        })
    }

    /// Create a worker creation error
    pub fn worker_creation(message: impl Into<String>) -> WasmError {
        WasmError::Worker(WorkerError::CreationFailed {
            message: message.into(),
        })
    }

    /// Create a tile cache miss error
    pub fn cache_miss(key: impl Into<String>) -> WasmError {
        WasmError::TileCache(TileCacheError::Miss { key: key.into() })
    }

    /// Create an invalid operation error
    pub fn invalid_op(operation: impl Into<String>, reason: impl Into<String>) -> WasmError {
        WasmError::InvalidOperation {
            operation: operation.into(),
            reason: reason.into(),
        }
    }

    /// Create a not found error
    pub fn not_found(resource: impl Into<String>, identifier: impl Into<String>) -> WasmError {
        WasmError::NotFound {
            resource: resource.into(),
            identifier: identifier.into(),
        }
    }

    /// Create an out of memory error
    pub fn out_of_memory(requested: usize, available: Option<usize>) -> WasmError {
        WasmError::OutOfMemory {
            requested,
            available,
        }
    }

    /// Create a timeout error
    pub fn timeout(operation: impl Into<String>, duration_ms: u64) -> WasmError {
        WasmError::Timeout {
            operation: operation.into(),
            duration_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = WasmErrorBuilder::fetch_network("https://example.com", "Connection refused");
        assert!(err.to_string().contains("Network failure"));
        assert!(err.to_string().contains("example.com"));
    }

    #[test]
    fn test_error_conversion() {
        let fetch_err = FetchError::HttpError {
            status: 404,
            status_text: "Not Found".to_string(),
            url: "https://example.com".to_string(),
        };
        let wasm_err: WasmError = fetch_err.into();
        assert!(matches!(wasm_err, WasmError::Fetch(_)));
    }

    #[test]
    #[cfg(target_arch = "wasm32")]
    fn test_js_value_conversion() {
        let err = WasmErrorBuilder::invalid_op("test", "invalid");
        let js_val: JsValue = err.into();
        assert!(js_val.is_string());
    }

    #[test]
    fn test_error_builder() {
        let err = WasmErrorBuilder::out_of_memory(1024, Some(512));
        assert!(matches!(err, WasmError::OutOfMemory { .. }));
        assert!(err.to_string().contains("1024"));
    }
}
