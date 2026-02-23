//! Error types for observability operations.

use thiserror::Error;

/// Result type for observability operations.
pub type Result<T> = std::result::Result<T, ObservabilityError>;

/// Errors that can occur during observability operations.
#[derive(Debug, Error)]
pub enum ObservabilityError {
    /// Telemetry initialization failed.
    #[error("Failed to initialize telemetry: {0}")]
    TelemetryInit(String),

    /// Telemetry configuration error.
    #[error("Invalid telemetry configuration: {0}")]
    InvalidConfig(String),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Metrics export failed.
    #[error("Failed to export metrics: {0}")]
    MetricsExportFailed(String),

    /// Trace export failed.
    #[error("Failed to export traces: {0}")]
    TraceExportFailed(String),

    /// Failed to create span.
    #[error("Failed to create span: {0}")]
    SpanCreationFailed(String),

    /// Context propagation error.
    #[error("Failed to propagate context: {0}")]
    ContextPropagationFailed(String),

    /// Exporter connection failed.
    #[error("Failed to connect to exporter: {0}")]
    ExporterConnectionFailed(String),

    /// Invalid metric value.
    #[error("Invalid metric value: {0}")]
    InvalidMetricValue(String),

    /// Alert routing failed.
    #[error("Failed to route alert: {0}")]
    AlertRoutingFailed(String),

    /// Alert deduplication error.
    #[error("Alert deduplication error: {0}")]
    AlertDeduplicationError(String),

    /// SLO calculation error.
    #[error("SLO calculation error: {0}")]
    SloCalculationError(String),

    /// Anomaly detection error.
    #[error("Anomaly detection error: {0}")]
    AnomalyDetectionError(String),

    /// Dashboard generation error.
    #[error("Failed to generate dashboard: {0}")]
    DashboardGenerationFailed(String),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// HTTP client error.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// URL parse error.
    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),

    /// Operation timeout.
    #[error("Operation timed out")]
    Timeout,

    /// Resource not found.
    #[error("Resource not found: {0}")]
    NotFound(String),

    /// Other error.
    #[error("Other error: {0}")]
    Other(String),
}
