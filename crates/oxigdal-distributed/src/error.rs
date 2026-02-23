//! Error types for distributed processing operations.
//!
//! This module provides comprehensive error handling for distributed geospatial
//! processing, including Flight RPC errors, worker failures, and coordination issues.

/// Result type for distributed operations.
pub type Result<T> = std::result::Result<T, DistributedError>;

/// Errors that can occur during distributed processing.
#[derive(Debug, thiserror::Error)]
pub enum DistributedError {
    /// Error during Flight RPC communication.
    #[error("Flight RPC error: {0}")]
    FlightRpc(String),

    /// Error connecting to a worker node.
    #[error("Worker connection error: {0}")]
    WorkerConnection(String),

    /// Worker failed to complete a task.
    #[error("Worker task failure: {0}")]
    WorkerTaskFailure(String),

    /// Coordinator error.
    #[error("Coordinator error: {0}")]
    Coordinator(String),

    /// Data partitioning error.
    #[error("Partitioning error: {0}")]
    Partitioning(String),

    /// Data shuffle error.
    #[error("Shuffle error: {0}")]
    Shuffle(String),

    /// Task serialization/deserialization error.
    #[error("Task serialization error: {0}")]
    TaskSerialization(String),

    /// Network timeout error.
    #[error("Network timeout: {0}")]
    Timeout(String),

    /// Authentication error.
    #[error("Authentication failed: {0}")]
    Authentication(String),

    /// Resource allocation error.
    #[error("Resource allocation error: {0}")]
    ResourceAllocation(String),

    /// Result aggregation error.
    #[error("Result aggregation error: {0}")]
    Aggregation(String),

    /// Arrow error during data transfer.
    #[error("Arrow error: {0}")]
    Arrow(String),

    /// Core OxiGDAL error.
    #[error("OxiGDAL core error: {0}")]
    Core(#[from] oxigdal_core::error::OxiGdalError),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Tonic transport error.
    #[error("Transport error: {0}")]
    Transport(#[from] tonic::transport::Error),

    /// Tonic status error.
    #[error("RPC status error: {0}")]
    Status(#[from] tonic::Status),

    /// Generic error with custom message.
    #[error("{0}")]
    Custom(String),
}

impl DistributedError {
    /// Create a Flight RPC error.
    pub fn flight_rpc<S: Into<String>>(msg: S) -> Self {
        Self::FlightRpc(msg.into())
    }

    /// Create a worker connection error.
    pub fn worker_connection<S: Into<String>>(msg: S) -> Self {
        Self::WorkerConnection(msg.into())
    }

    /// Create a worker task failure error.
    pub fn worker_task_failure<S: Into<String>>(msg: S) -> Self {
        Self::WorkerTaskFailure(msg.into())
    }

    /// Create a coordinator error.
    pub fn coordinator<S: Into<String>>(msg: S) -> Self {
        Self::Coordinator(msg.into())
    }

    /// Create a partitioning error.
    pub fn partitioning<S: Into<String>>(msg: S) -> Self {
        Self::Partitioning(msg.into())
    }

    /// Create a shuffle error.
    pub fn shuffle<S: Into<String>>(msg: S) -> Self {
        Self::Shuffle(msg.into())
    }

    /// Create a task serialization error.
    pub fn task_serialization<S: Into<String>>(msg: S) -> Self {
        Self::TaskSerialization(msg.into())
    }

    /// Create a timeout error.
    pub fn timeout<S: Into<String>>(msg: S) -> Self {
        Self::Timeout(msg.into())
    }

    /// Create an authentication error.
    pub fn authentication<S: Into<String>>(msg: S) -> Self {
        Self::Authentication(msg.into())
    }

    /// Create a resource allocation error.
    pub fn resource_allocation<S: Into<String>>(msg: S) -> Self {
        Self::ResourceAllocation(msg.into())
    }

    /// Create an aggregation error.
    pub fn aggregation<S: Into<String>>(msg: S) -> Self {
        Self::Aggregation(msg.into())
    }

    /// Create an Arrow error.
    pub fn arrow<S: Into<String>>(msg: S) -> Self {
        Self::Arrow(msg.into())
    }

    /// Create a custom error.
    pub fn custom<S: Into<String>>(msg: S) -> Self {
        Self::Custom(msg.into())
    }
}

impl From<arrow::error::ArrowError> for DistributedError {
    fn from(err: arrow::error::ArrowError) -> Self {
        Self::Arrow(err.to_string())
    }
}

impl From<DistributedError> for tonic::Status {
    fn from(err: DistributedError) -> Self {
        match err {
            DistributedError::FlightRpc(msg) => {
                tonic::Status::internal(format!("Flight RPC error: {}", msg))
            }
            DistributedError::WorkerConnection(msg) => {
                tonic::Status::unavailable(format!("Worker connection error: {}", msg))
            }
            DistributedError::WorkerTaskFailure(msg) => {
                tonic::Status::internal(format!("Worker task failure: {}", msg))
            }
            DistributedError::Coordinator(msg) => {
                tonic::Status::internal(format!("Coordinator error: {}", msg))
            }
            DistributedError::Partitioning(msg) => {
                tonic::Status::invalid_argument(format!("Partitioning error: {}", msg))
            }
            DistributedError::Shuffle(msg) => {
                tonic::Status::internal(format!("Shuffle error: {}", msg))
            }
            DistributedError::TaskSerialization(msg) => {
                tonic::Status::invalid_argument(format!("Task serialization error: {}", msg))
            }
            DistributedError::Timeout(msg) => {
                tonic::Status::deadline_exceeded(format!("Timeout: {}", msg))
            }
            DistributedError::Authentication(msg) => {
                tonic::Status::unauthenticated(format!("Authentication failed: {}", msg))
            }
            DistributedError::ResourceAllocation(msg) => {
                tonic::Status::resource_exhausted(format!("Resource allocation error: {}", msg))
            }
            DistributedError::Aggregation(msg) => {
                tonic::Status::internal(format!("Aggregation error: {}", msg))
            }
            DistributedError::Arrow(msg) => {
                tonic::Status::internal(format!("Arrow error: {}", msg))
            }
            DistributedError::Core(err) => tonic::Status::internal(format!("Core error: {}", err)),
            DistributedError::Io(err) => tonic::Status::internal(format!("I/O error: {}", err)),
            DistributedError::Json(err) => {
                tonic::Status::invalid_argument(format!("JSON error: {}", err))
            }
            DistributedError::Transport(err) => {
                tonic::Status::unavailable(format!("Transport error: {}", err))
            }
            DistributedError::Status(status) => status,
            DistributedError::Custom(msg) => tonic::Status::internal(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = DistributedError::flight_rpc("test error");
        assert!(matches!(err, DistributedError::FlightRpc(_)));

        let err = DistributedError::worker_connection("connection failed");
        assert!(matches!(err, DistributedError::WorkerConnection(_)));

        let err = DistributedError::timeout("operation timed out");
        assert!(matches!(err, DistributedError::Timeout(_)));
    }

    #[test]
    fn test_error_display() {
        let err = DistributedError::flight_rpc("test error");
        let msg = format!("{}", err);
        assert!(msg.contains("Flight RPC error"));
        assert!(msg.contains("test error"));
    }

    #[test]
    fn test_to_tonic_status() {
        let err = DistributedError::flight_rpc("test");
        let status: tonic::Status = err.into();
        assert_eq!(status.code(), tonic::Code::Internal);

        let err = DistributedError::authentication("invalid token");
        let status: tonic::Status = err.into();
        assert_eq!(status.code(), tonic::Code::Unauthenticated);

        let err = DistributedError::timeout("too slow");
        let status: tonic::Status = err.into();
        assert_eq!(status.code(), tonic::Code::DeadlineExceeded);
    }
}
