//! Structured logging with tracing integration.

use crate::error::{ObservabilityError, Result};
use crate::telemetry::TelemetryConfig;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Initialize structured logging.
pub fn init_logging(_config: &TelemetryConfig) -> Result<()> {
    // Create environment filter
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    // Create formatting layer
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .with_span_events(FmtSpan::CLOSE);

    // Create registry
    let registry = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer);

    // Initialize without OpenTelemetry layer for now
    // The OpenTelemetry layer needs to be added after tracer provider is initialized
    registry.try_init().map_err(|e| {
        ObservabilityError::TelemetryInit(format!("Failed to initialize logging: {}", e))
    })?;

    Ok(())
}

/// Log level utilities.
pub enum LogLevel {
    /// Trace level - most verbose, for detailed debugging.
    Trace,
    /// Debug level - debugging information.
    Debug,
    /// Info level - general informational messages.
    Info,
    /// Warn level - warning messages.
    Warn,
    /// Error level - error messages.
    Error,
}

impl LogLevel {
    /// Convert to tracing level.
    pub fn to_tracing_level(&self) -> tracing::Level {
        match self {
            LogLevel::Trace => tracing::Level::TRACE,
            LogLevel::Debug => tracing::Level::DEBUG,
            LogLevel::Info => tracing::Level::INFO,
            LogLevel::Warn => tracing::Level::WARN,
            LogLevel::Error => tracing::Level::ERROR,
        }
    }
}

impl std::str::FromStr for LogLevel {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "trace" => Ok(LogLevel::Trace),
            "debug" => Ok(LogLevel::Debug),
            "info" => Ok(LogLevel::Info),
            "warn" => Ok(LogLevel::Warn),
            "error" => Ok(LogLevel::Error),
            _ => Err(format!("Invalid log level: {}", s)),
        }
    }
}

/// Structured log context builder.
pub struct LogContext {
    fields: Vec<(String, String)>,
}

impl LogContext {
    /// Create a new log context.
    pub fn new() -> Self {
        Self { fields: Vec::new() }
    }

    /// Add a field to the context.
    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.push((key.into(), value.into()));
        self
    }

    /// Get the fields.
    pub fn fields(&self) -> &[(String, String)] {
        &self.fields
    }
}

impl Default for LogContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Structured log macros helpers.
#[macro_export]
macro_rules! log_with_context {
    ($level:expr, $ctx:expr, $($arg:tt)*) => {
        match $level {
            $crate::telemetry::logs::LogLevel::Trace => tracing::trace!($($arg)*),
            $crate::telemetry::logs::LogLevel::Debug => tracing::debug!($($arg)*),
            $crate::telemetry::logs::LogLevel::Info => tracing::info!($($arg)*),
            $crate::telemetry::logs::LogLevel::Warn => tracing::warn!($($arg)*),
            $crate::telemetry::logs::LogLevel::Error => tracing::error!($($arg)*),
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_log_level_conversion() {
        assert_eq!(LogLevel::Trace.to_tracing_level(), tracing::Level::TRACE);
        assert_eq!(LogLevel::Debug.to_tracing_level(), tracing::Level::DEBUG);
        assert_eq!(LogLevel::Info.to_tracing_level(), tracing::Level::INFO);
        assert_eq!(LogLevel::Warn.to_tracing_level(), tracing::Level::WARN);
        assert_eq!(LogLevel::Error.to_tracing_level(), tracing::Level::ERROR);
    }

    #[test]
    fn test_log_level_from_str() {
        assert!(matches!(LogLevel::from_str("trace"), Ok(LogLevel::Trace)));
        assert!(matches!(LogLevel::from_str("debug"), Ok(LogLevel::Debug)));
        assert!(matches!(LogLevel::from_str("info"), Ok(LogLevel::Info)));
        assert!(matches!(LogLevel::from_str("warn"), Ok(LogLevel::Warn)));
        assert!(matches!(LogLevel::from_str("error"), Ok(LogLevel::Error)));
        assert!(LogLevel::from_str("invalid").is_err());
    }

    #[test]
    fn test_log_context() {
        let ctx = LogContext::new()
            .with_field("service", "oxigdal")
            .with_field("version", "1.0.0");

        assert_eq!(ctx.fields().len(), 2);
    }

    #[test]
    fn test_log_span() {
        let span = tracing::error_span!("error");
        let _guard = span.enter();

        tracing::error!("Test error message");
    }
}
