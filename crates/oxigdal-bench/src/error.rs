//! Error types for benchmarking operations.
//!
//! This module provides comprehensive error handling for all benchmarking
//! and profiling operations in oxigdal-bench.

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Result type alias for benchmarking operations.
pub type Result<T> = std::result::Result<T, BenchError>;

/// Comprehensive error type for benchmarking operations.
#[derive(Debug, thiserror::Error)]
pub enum BenchError {
    /// I/O error occurred during benchmarking.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Serialization/deserialization error.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Profiler initialization or execution error.
    #[error("Profiler error: {source}")]
    Profiler {
        /// The underlying error source
        source: ProfilerErrorKind,
    },

    /// Benchmark execution error.
    #[error("Benchmark execution error: {message}")]
    BenchmarkExecution {
        /// Description of the error
        message: String,
        /// Optional benchmark name
        benchmark_name: Option<String>,
    },

    /// Report generation error.
    #[error("Report generation error: {format} - {message}")]
    ReportGeneration {
        /// Report format that failed
        format: String,
        /// Error message
        message: String,
    },

    /// Regression detection error.
    #[error("Regression detection error: {0}")]
    RegressionDetection(String),

    /// Baseline file error.
    #[error("Baseline error for '{path}': {message}")]
    Baseline {
        /// Path to the baseline file
        path: PathBuf,
        /// Error message
        message: String,
    },

    /// Comparison error.
    #[error("Comparison error: {0}")]
    Comparison(String),

    /// Invalid configuration error.
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// Missing dependency error (e.g., feature not enabled).
    #[error("Missing dependency: {dependency} (enable feature: {feature})")]
    MissingDependency {
        /// Name of the missing dependency
        dependency: String,
        /// Feature flag to enable
        feature: String,
    },

    /// Scenario execution error.
    #[error("Scenario '{scenario}' failed: {message}")]
    ScenarioFailed {
        /// Scenario name
        scenario: String,
        /// Error message
        message: String,
    },

    /// System resource error.
    #[error("System resource error: {0}")]
    SystemResource(String),

    /// Memory profiling error.
    #[error("Memory profiling error: {0}")]
    MemoryProfiling(String),

    /// CPU profiling error.
    #[error("CPU profiling error: {0}")]
    CpuProfiling(String),

    /// Flamegraph generation error.
    #[error("Flamegraph generation error: {0}")]
    Flamegraph(String),

    /// Data validation error.
    #[error("Data validation error: {0}")]
    DataValidation(String),

    /// Timeout error.
    #[error("Operation timed out after {seconds} seconds")]
    Timeout {
        /// Timeout duration in seconds
        seconds: u64,
    },

    /// oxigdal-core error.
    #[cfg(feature = "raster")]
    #[error("OxiGDAL core error: {0}")]
    Core(#[from] oxigdal_core::error::OxiGdalError),

    /// Generic error for other cases.
    #[error("{0}")]
    Other(String),
}

/// Specific profiler error kinds.
#[derive(Debug)]
pub enum ProfilerErrorKind {
    /// Failed to initialize profiler.
    InitializationFailed(String),

    /// Failed to start profiling.
    StartFailed(String),

    /// Failed to stop profiling.
    StopFailed(String),

    /// Failed to collect profiling data.
    CollectionFailed(String),

    /// Failed to generate report.
    ReportGenerationFailed(String),

    /// Unsupported profiler feature.
    UnsupportedFeature(String),
}

impl fmt::Display for ProfilerErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InitializationFailed(msg) => write!(f, "initialization failed: {msg}"),
            Self::StartFailed(msg) => write!(f, "start failed: {msg}"),
            Self::StopFailed(msg) => write!(f, "stop failed: {msg}"),
            Self::CollectionFailed(msg) => write!(f, "collection failed: {msg}"),
            Self::ReportGenerationFailed(msg) => write!(f, "report generation failed: {msg}"),
            Self::UnsupportedFeature(msg) => write!(f, "unsupported feature: {msg}"),
        }
    }
}

impl std::error::Error for ProfilerErrorKind {}

impl BenchError {
    /// Creates a new profiler initialization error.
    pub fn profiler_init<S: Into<String>>(message: S) -> Self {
        Self::Profiler {
            source: ProfilerErrorKind::InitializationFailed(message.into()),
        }
    }

    /// Creates a new profiler start error.
    pub fn profiler_start<S: Into<String>>(message: S) -> Self {
        Self::Profiler {
            source: ProfilerErrorKind::StartFailed(message.into()),
        }
    }

    /// Creates a new profiler stop error.
    pub fn profiler_stop<S: Into<String>>(message: S) -> Self {
        Self::Profiler {
            source: ProfilerErrorKind::StopFailed(message.into()),
        }
    }

    /// Creates a new profiler collection error.
    pub fn profiler_collect<S: Into<String>>(message: S) -> Self {
        Self::Profiler {
            source: ProfilerErrorKind::CollectionFailed(message.into()),
        }
    }

    /// Creates a new benchmark execution error.
    pub fn benchmark_execution<S: Into<String>>(message: S) -> Self {
        Self::BenchmarkExecution {
            message: message.into(),
            benchmark_name: None,
        }
    }

    /// Creates a new benchmark execution error with name.
    pub fn benchmark_execution_with_name<S1: Into<String>, S2: Into<String>>(
        name: S1,
        message: S2,
    ) -> Self {
        Self::BenchmarkExecution {
            message: message.into(),
            benchmark_name: Some(name.into()),
        }
    }

    /// Creates a new report generation error.
    pub fn report_generation<S1: Into<String>, S2: Into<String>>(format: S1, message: S2) -> Self {
        Self::ReportGeneration {
            format: format.into(),
            message: message.into(),
        }
    }

    /// Creates a new baseline error.
    pub fn baseline<P: Into<PathBuf>, S: Into<String>>(path: P, message: S) -> Self {
        Self::Baseline {
            path: path.into(),
            message: message.into(),
        }
    }

    /// Creates a new scenario failed error.
    pub fn scenario_failed<S1: Into<String>, S2: Into<String>>(scenario: S1, message: S2) -> Self {
        Self::ScenarioFailed {
            scenario: scenario.into(),
            message: message.into(),
        }
    }

    /// Creates a new missing dependency error.
    pub fn missing_dependency<S1: Into<String>, S2: Into<String>>(
        dependency: S1,
        feature: S2,
    ) -> Self {
        Self::MissingDependency {
            dependency: dependency.into(),
            feature: feature.into(),
        }
    }

    /// Creates a new timeout error.
    pub fn timeout(seconds: u64) -> Self {
        Self::Timeout { seconds }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = BenchError::benchmark_execution("Test failed");
        assert!(err.to_string().contains("Test failed"));

        let err = BenchError::benchmark_execution_with_name("my_bench", "Test failed");
        assert!(err.to_string().contains("Test failed"));

        let err = BenchError::report_generation("HTML", "Template error");
        assert!(err.to_string().contains("HTML"));
        assert!(err.to_string().contains("Template error"));

        let err = BenchError::missing_dependency("criterion", "benchmarking");
        assert!(err.to_string().contains("criterion"));
        assert!(err.to_string().contains("benchmarking"));

        let err = BenchError::timeout(30);
        assert!(err.to_string().contains("30"));
    }

    #[test]
    fn test_profiler_errors() {
        let err = BenchError::profiler_init("Failed to initialize");
        assert!(err.to_string().contains("initialization"));

        let err = BenchError::profiler_start("Failed to start");
        assert!(err.to_string().contains("start"));

        let err = BenchError::profiler_stop("Failed to stop");
        assert!(err.to_string().contains("stop"));

        let err = BenchError::profiler_collect("Failed to collect");
        assert!(err.to_string().contains("collection"));
    }

    #[test]
    fn test_scenario_error() {
        let err = BenchError::scenario_failed("raster_read", "File not found");
        assert!(err.to_string().contains("raster_read"));
        assert!(err.to_string().contains("File not found"));
    }

    #[test]
    fn test_baseline_error() {
        let err = BenchError::baseline("/tmp/baseline.json", "Corrupted file");
        assert!(err.to_string().contains("baseline.json"));
        assert!(err.to_string().contains("Corrupted"));
    }
}
