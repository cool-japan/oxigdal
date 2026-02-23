//! Error types for the oxigdal-cluster crate.
//!
//! This module defines all error types that can occur during distributed
//! cluster operations including task scheduling, worker management, data
//! replication, and fault tolerance.

use std::io;

/// Result type alias for cluster operations.
pub type Result<T> = std::result::Result<T, ClusterError>;

/// Main error type for cluster operations.
#[derive(Debug, thiserror::Error)]
pub enum ClusterError {
    /// Task scheduling errors
    #[error("Task scheduling error: {0}")]
    SchedulerError(String),

    /// Task not found
    #[error("Task not found: {0}")]
    TaskNotFound(String),

    /// Task dependency cycle detected
    #[error("Task dependency cycle detected: {0}")]
    DependencyCycle(String),

    /// Worker pool errors
    #[error("Worker pool error: {0}")]
    WorkerPoolError(String),

    /// Worker not found
    #[error("Worker not found: {0}")]
    WorkerNotFound(String),

    /// Worker unhealthy
    #[error("Worker unhealthy: {0}")]
    WorkerUnhealthy(String),

    /// Worker capacity exceeded
    #[error("Worker capacity exceeded: {0}")]
    CapacityExceeded(String),

    /// Data locality errors
    #[error("Data locality error: {0}")]
    DataLocalityError(String),

    /// Data not available on any worker
    #[error("Data not available: {0}")]
    DataNotAvailable(String),

    /// Fault tolerance errors
    #[error("Fault tolerance error: {0}")]
    FaultToleranceError(String),

    /// Maximum retries exceeded
    #[error("Maximum retries exceeded for task: {0}")]
    MaxRetriesExceeded(String),

    /// Checkpoint error
    #[error("Checkpoint error: {0}")]
    CheckpointError(String),

    /// Distributed cache errors
    #[error("Cache error: {0}")]
    CacheError(String),

    /// Cache coherency violation
    #[error("Cache coherency violation: {0}")]
    CoherencyViolation(String),

    /// Replication errors
    #[error("Replication error: {0}")]
    ReplicationError(String),

    /// Quorum not reached
    #[error("Quorum not reached: required {required}, got {actual}")]
    QuorumNotReached {
        /// The required quorum size
        required: usize,
        /// The actual number of responses received
        actual: usize,
    },

    /// Replica placement error
    #[error("Replica placement error: {0}")]
    ReplicaPlacementError(String),

    /// Coordinator errors
    #[error("Coordinator error: {0}")]
    CoordinatorError(String),

    /// Leader election failed
    #[error("Leader election failed: {0}")]
    LeaderElectionFailed(String),

    /// No leader available
    #[error("No leader available")]
    NoLeader,

    /// Consensus error
    #[error("Consensus error: {0}")]
    ConsensusError(String),

    /// Metrics collection error
    #[error("Metrics error: {0}")]
    MetricsError(String),

    /// Serialization/deserialization errors
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Network communication errors
    #[error("Network error: {0}")]
    NetworkError(String),

    /// Timeout error
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Resource exhausted
    #[error("Resource exhausted: {0}")]
    ResourceExhausted(String),

    /// Invalid state
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// Task execution error
    #[error("Task execution error: {0}")]
    ExecutionError(String),

    /// Task cancelled
    #[error("Task cancelled: {0}")]
    TaskCancelled(String),

    /// IO errors
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// JSON errors
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Raft errors
    #[error("Raft error: {0}")]
    RaftError(String),

    /// Quota errors
    #[error("Quota exceeded: {0}")]
    QuotaExceeded(String),

    /// Reservation errors
    #[error("Reservation not found: {0}")]
    ReservationNotFound(String),

    /// Resource not available
    #[error("Resource not available: {0}")]
    ResourceNotAvailable(String),

    /// Invalid operation
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// Workflow errors
    #[error("Workflow not found: {0}")]
    WorkflowNotFound(String),

    /// Monitoring errors
    #[error("Metric not found: {0}")]
    MetricNotFound(String),

    /// Alert not found
    #[error("Alert not found: {0}")]
    AlertNotFound(String),

    /// Security errors
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Permission denied
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Secret not found
    #[error("Secret not found: {0}")]
    SecretNotFound(String),

    /// Compression error
    #[error("Compression error: {0}")]
    CompressionError(String),

    /// Other errors
    #[error("Other error: {0}")]
    Other(String),
}

impl ClusterError {
    /// Check if the error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ClusterError::NetworkError(_)
                | ClusterError::Timeout(_)
                | ClusterError::WorkerUnhealthy(_)
                | ClusterError::QuorumNotReached { .. }
                | ClusterError::NoLeader
                | ClusterError::ResourceExhausted(_)
        )
    }

    /// Check if the error indicates a permanent failure.
    pub fn is_permanent(&self) -> bool {
        matches!(
            self,
            ClusterError::DependencyCycle(_)
                | ClusterError::TaskNotFound(_)
                | ClusterError::ConfigError(_)
                | ClusterError::InvalidState(_)
                | ClusterError::MaxRetriesExceeded(_)
        )
    }

    /// Check if the error requires failover.
    pub fn requires_failover(&self) -> bool {
        matches!(
            self,
            ClusterError::WorkerNotFound(_)
                | ClusterError::WorkerUnhealthy(_)
                | ClusterError::NoLeader
                | ClusterError::LeaderElectionFailed(_)
        )
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_error_retryable() {
        let err = ClusterError::NetworkError("connection failed".to_string());
        assert!(err.is_retryable());
        assert!(!err.is_permanent());

        let err = ClusterError::DependencyCycle("cycle detected".to_string());
        assert!(!err.is_retryable());
        assert!(err.is_permanent());
    }

    #[test]
    fn test_error_requires_failover() {
        let err = ClusterError::WorkerNotFound("worker123".to_string());
        assert!(err.requires_failover());

        let err = ClusterError::NoLeader;
        assert!(err.requires_failover());
    }

    #[test]
    fn test_quorum_error() {
        let err = ClusterError::QuorumNotReached {
            required: 3,
            actual: 2,
        };
        assert!(err.is_retryable());
        assert!(!err.is_permanent());
    }
}
