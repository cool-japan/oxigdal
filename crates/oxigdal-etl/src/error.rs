//! Error types for OxiGDAL ETL operations
//!
//! This module provides a comprehensive error hierarchy for all ETL operations including
//! pipeline construction, source/sink operations, transformations, and scheduling.

use thiserror::Error;

/// The main result type for ETL operations
pub type Result<T> = std::result::Result<T, EtlError>;

/// The main error type for ETL operations
#[derive(Debug, Error)]
pub enum EtlError {
    /// Source error occurred
    #[error("Source error: {0}")]
    Source(#[from] SourceError),

    /// Sink error occurred
    #[error("Sink error: {0}")]
    Sink(#[from] SinkError),

    /// Transform error occurred
    #[error("Transform error: {0}")]
    Transform(#[from] TransformError),

    /// Pipeline error occurred
    #[error("Pipeline error: {0}")]
    Pipeline(#[from] PipelineError),

    /// Scheduler error occurred
    #[error("Scheduler error: {0}")]
    Scheduler(#[from] SchedulerError),

    /// Stream processing error
    #[error("Stream error: {0}")]
    Stream(#[from] StreamError),

    /// OxiGDAL core error
    #[error("Core error: {0}")]
    Core(#[from] oxigdal_core::error::OxiGdalError),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Invalid configuration
    #[error("Invalid configuration: {message}")]
    InvalidConfig {
        /// Error message
        message: String,
    },

    /// Operation timed out
    #[error("Operation timed out after {duration:?}")]
    Timeout {
        /// Timeout duration
        duration: std::time::Duration,
    },

    /// Resource exhausted
    #[error("Resource exhausted: {resource}")]
    ResourceExhausted {
        /// Resource name
        resource: String,
    },

    /// Internal error
    #[error("Internal error: {message}")]
    Internal {
        /// Error message
        message: String,
    },
}

/// Source-related errors
#[derive(Debug, Error)]
pub enum SourceError {
    /// Failed to connect to source
    #[error("Failed to connect to source: {0}")]
    ConnectionFailed(String),

    /// Failed to read from source
    #[error("Failed to read from source: {0}")]
    ReadFailed(String),

    /// Source not found
    #[error("Source not found: {0}")]
    NotFound(String),

    /// Invalid source configuration
    #[error("Invalid source configuration: {0}")]
    InvalidConfig(String),

    /// Source exhausted
    #[error("Source exhausted")]
    Exhausted,

    /// Kafka-specific error
    #[cfg(feature = "kafka")]
    #[error("Kafka error: {0}")]
    Kafka(String),

    /// STAC-specific error
    #[cfg(feature = "stac")]
    #[error("STAC error: {0}")]
    Stac(String),

    /// HTTP-specific error
    #[cfg(feature = "http")]
    #[error("HTTP error: {0}")]
    Http(String),

    /// Database-specific error
    #[cfg(feature = "postgres")]
    #[error("Database error: {0}")]
    Database(String),

    /// S3-specific error
    #[cfg(feature = "s3")]
    #[error("S3 error: {0}")]
    S3(String),
}

/// Sink-related errors
#[derive(Debug, Error)]
pub enum SinkError {
    /// Failed to connect to sink
    #[error("Failed to connect to sink: {0}")]
    ConnectionFailed(String),

    /// Failed to write to sink
    #[error("Failed to write to sink: {0}")]
    WriteFailed(String),

    /// Invalid sink configuration
    #[error("Invalid sink configuration: {0}")]
    InvalidConfig(String),

    /// Sink full (backpressure)
    #[error("Sink full, backpressure applied")]
    Full,

    /// Kafka-specific error
    #[cfg(feature = "kafka")]
    #[error("Kafka error: {0}")]
    Kafka(String),

    /// Database-specific error
    #[cfg(feature = "postgres")]
    #[error("Database error: {0}")]
    Database(String),

    /// S3-specific error
    #[cfg(feature = "s3")]
    #[error("S3 error: {0}")]
    S3(String),
}

/// Transform-related errors
#[derive(Debug, Error)]
pub enum TransformError {
    /// Transformation failed
    #[error("Transformation failed: {message}")]
    Failed {
        /// Error message
        message: String,
    },

    /// Invalid input
    #[error("Invalid input: {message}")]
    InvalidInput {
        /// Error message
        message: String,
    },

    /// Type mismatch
    #[error("Type mismatch: expected {expected}, got {actual}")]
    TypeMismatch {
        /// Expected type
        expected: String,
        /// Actual type
        actual: String,
    },

    /// Missing required field
    #[error("Missing required field: {field}")]
    MissingField {
        /// Field name
        field: String,
    },

    /// Join failed
    #[error("Join failed: {message}")]
    JoinFailed {
        /// Error message
        message: String,
    },

    /// Window operation failed
    #[error("Window operation failed: {message}")]
    WindowFailed {
        /// Error message
        message: String,
    },

    /// Aggregation failed
    #[error("Aggregation failed: {message}")]
    AggregationFailed {
        /// Error message
        message: String,
    },
}

/// Pipeline-related errors
#[derive(Debug, Error)]
pub enum PipelineError {
    /// Pipeline construction failed
    #[error("Pipeline construction failed: {message}")]
    ConstructionFailed {
        /// Error message
        message: String,
    },

    /// Invalid pipeline configuration
    #[error("Invalid pipeline configuration: {message}")]
    InvalidConfig {
        /// Error message
        message: String,
    },

    /// Pipeline execution failed
    #[error("Pipeline execution failed: {message}")]
    ExecutionFailed {
        /// Error message
        message: String,
    },

    /// No source configured
    #[error("No source configured")]
    NoSource,

    /// No sink configured
    #[error("No sink configured")]
    NoSink,

    /// Multiple sources not supported
    #[error("Multiple sources not supported")]
    MultipleSources,

    /// Multiple sinks not supported
    #[error("Multiple sinks not supported")]
    MultipleSinks,

    /// Circular dependency detected
    #[error("Circular dependency detected in pipeline")]
    CircularDependency,

    /// Checkpoint failed
    #[error("Checkpoint failed: {message}")]
    CheckpointFailed {
        /// Error message
        message: String,
    },

    /// Recovery failed
    #[error("Recovery failed: {message}")]
    RecoveryFailed {
        /// Error message
        message: String,
    },
}

/// Stream processing errors
#[derive(Debug, Error)]
pub enum StreamError {
    /// Channel closed
    #[error("Channel closed")]
    ChannelClosed,

    /// Backpressure timeout
    #[error("Backpressure timeout after {duration:?}")]
    BackpressureTimeout {
        /// Timeout duration
        duration: std::time::Duration,
    },

    /// Buffer overflow
    #[error("Buffer overflow: capacity {capacity}")]
    BufferOverflow {
        /// Buffer capacity
        capacity: usize,
    },

    /// State management error
    #[error("State management error: {message}")]
    StateFailed {
        /// Error message
        message: String,
    },

    /// Parallel processing error
    #[error("Parallel processing error: {message}")]
    ParallelFailed {
        /// Error message
        message: String,
    },
}

/// Scheduler-related errors
#[derive(Debug, Error)]
pub enum SchedulerError {
    /// Invalid cron expression
    #[cfg(feature = "scheduler")]
    #[error("Invalid cron expression: {expression}")]
    InvalidCron {
        /// Cron expression
        expression: String,
    },

    /// Schedule not found
    #[error("Schedule not found: {id}")]
    NotFound {
        /// Schedule ID
        id: String,
    },

    /// Scheduler not running
    #[error("Scheduler not running")]
    NotRunning,

    /// Task execution failed
    #[error("Task execution failed: {message}")]
    ExecutionFailed {
        /// Error message
        message: String,
    },

    /// Retry limit exceeded
    #[error("Retry limit exceeded: {attempts} attempts")]
    RetryLimitExceeded {
        /// Number of attempts
        attempts: usize,
    },

    /// Resource limit exceeded
    #[error("Resource limit exceeded: {resource}")]
    ResourceLimitExceeded {
        /// Resource name
        resource: String,
    },
}

// Conversion implementations for optional feature errors
#[cfg(feature = "kafka")]
impl From<rdkafka::error::KafkaError> for SourceError {
    fn from(err: rdkafka::error::KafkaError) -> Self {
        Self::Kafka(err.to_string())
    }
}

#[cfg(feature = "kafka")]
impl From<rdkafka::error::KafkaError> for SinkError {
    fn from(err: rdkafka::error::KafkaError) -> Self {
        Self::Kafka(err.to_string())
    }
}

#[cfg(feature = "postgres")]
impl From<tokio_postgres::Error> for SourceError {
    fn from(err: tokio_postgres::Error) -> Self {
        Self::Database(err.to_string())
    }
}

#[cfg(feature = "postgres")]
impl From<tokio_postgres::Error> for SinkError {
    fn from(err: tokio_postgres::Error) -> Self {
        Self::Database(err.to_string())
    }
}

#[cfg(feature = "http")]
impl From<reqwest::Error> for SourceError {
    fn from(err: reqwest::Error) -> Self {
        Self::Http(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = EtlError::InvalidConfig {
            message: "test error".to_string(),
        };
        assert!(err.to_string().contains("test error"));
    }

    #[test]
    fn test_source_error_conversion() {
        let source_err = SourceError::NotFound("/test/path".to_string());
        let etl_err: EtlError = source_err.into();
        assert!(matches!(
            etl_err,
            EtlError::Source(SourceError::NotFound(_))
        ));
    }

    #[test]
    fn test_pipeline_error() {
        let err = PipelineError::NoSource;
        assert_eq!(err.to_string(), "No source configured");
    }

    #[test]
    fn test_stream_error() {
        let err = StreamError::ChannelClosed;
        assert_eq!(err.to_string(), "Channel closed");
    }
}
