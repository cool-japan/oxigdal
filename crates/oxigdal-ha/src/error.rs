//! Error types for the oxigdal-ha crate.

use std::io;
use thiserror::Error;

/// Result type for HA operations.
pub type HaResult<T> = Result<T, HaError>;

/// Errors that can occur in high availability operations.
#[derive(Debug, Error)]
pub enum HaError {
    /// Replication error.
    #[error("Replication error: {0}")]
    Replication(String),

    /// Replication lag exceeded threshold.
    #[error("Replication lag exceeded: {lag_ms}ms > {threshold_ms}ms")]
    ReplicationLagExceeded { lag_ms: u64, threshold_ms: u64 },

    /// Replication conflict detected.
    #[error("Replication conflict: {0}")]
    ReplicationConflict(String),

    /// Failover error.
    #[error("Failover error: {0}")]
    Failover(String),

    /// Failover timeout.
    #[error("Failover timeout: operation took longer than {timeout_ms}ms")]
    FailoverTimeout { timeout_ms: u64 },

    /// Leader election failed.
    #[error("Leader election failed: {0}")]
    LeaderElectionFailed(String),

    /// No healthy replicas available.
    #[error("No healthy replicas available")]
    NoHealthyReplicas,

    /// Conflict resolution error.
    #[error("Conflict resolution error: {0}")]
    ConflictResolution(String),

    /// Recovery error.
    #[error("Recovery error: {0}")]
    Recovery(String),

    /// Point-in-time recovery failed.
    #[error("Point-in-time recovery failed: {0}")]
    PitrFailed(String),

    /// Snapshot error.
    #[error("Snapshot error: {0}")]
    Snapshot(String),

    /// WAL (Write-Ahead Log) error.
    #[error("WAL error: {0}")]
    Wal(String),

    /// Backup error.
    #[error("Backup error: {0}")]
    Backup(String),

    /// Backup verification failed.
    #[error("Backup verification failed: {0}")]
    BackupVerificationFailed(String),

    /// Backup not found.
    #[error("Backup not found: {id}")]
    BackupNotFound { id: String },

    /// Disaster recovery error.
    #[error("Disaster recovery error: {0}")]
    DisasterRecovery(String),

    /// DR failover failed.
    #[error("DR failover failed: {0}")]
    DrFailoverFailed(String),

    /// Health check error.
    #[error("Health check error: {0}")]
    HealthCheck(String),

    /// Health check timeout.
    #[error("Health check timeout for {service}")]
    HealthCheckTimeout { service: String },

    /// Service unhealthy.
    #[error("Service unhealthy: {service}")]
    ServiceUnhealthy { service: String },

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Invalid state.
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// Timeout error.
    #[error("Operation timeout: {0}")]
    Timeout(String),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Deserialization error.
    #[error("Deserialization error: {0}")]
    Deserialization(String),

    /// Network error.
    #[error("Network error: {0}")]
    Network(String),

    /// Compression error.
    #[error("Compression error: {0}")]
    Compression(String),

    /// Decompression error.
    #[error("Decompression error: {0}")]
    Decompression(String),

    /// Checksum mismatch.
    #[error("Checksum mismatch: expected {expected:x}, got {actual:x}")]
    ChecksumMismatch { expected: u32, actual: u32 },

    /// Not implemented.
    #[error("Not implemented: {0}")]
    NotImplemented(String),
}

impl From<serde_json::Error> for HaError {
    fn from(err: serde_json::Error) -> Self {
        HaError::Serialization(err.to_string())
    }
}

impl From<oxicode::Error> for HaError {
    fn from(err: oxicode::Error) -> Self {
        HaError::Serialization(err.to_string())
    }
}
