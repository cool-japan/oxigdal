//! Configuration management for observability system.

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Global observability configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityConfig {
    /// Telemetry configuration.
    pub telemetry: TelemetrySettings,

    /// Metrics configuration.
    pub metrics: MetricsSettings,

    /// Tracing configuration.
    pub tracing: TracingSettings,

    /// Logging configuration.
    pub logging: LoggingSettings,

    /// SLO configuration.
    pub slo: SloSettings,

    /// Alerting configuration.
    pub alerting: AlertingSettings,

    /// Dashboard configuration.
    pub dashboards: DashboardSettings,
}

/// Telemetry settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySettings {
    /// Whether telemetry collection is enabled.
    pub enabled: bool,
    /// Name of the service being monitored.
    pub service_name: String,
    /// Version of the service being monitored.
    pub service_version: String,
    /// Optional namespace for the service.
    pub service_namespace: Option<String>,
    /// Deployment environment (e.g., development, staging, production).
    pub environment: String,
}

/// Metrics settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSettings {
    /// Whether metrics collection is enabled.
    pub enabled: bool,
    /// Interval between metric collections in seconds.
    pub collection_interval_secs: u64,
    /// Interval between metric exports in seconds.
    pub export_interval_secs: u64,
    /// List of configured metric exporters.
    pub exporters: Vec<ExporterConfig>,
}

/// Exporter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExporterConfig {
    /// Type of exporter (e.g., prometheus, influxdb, statsd).
    pub exporter_type: String,
    /// Endpoint URL for the exporter.
    pub endpoint: String,
    /// Whether this exporter is enabled.
    pub enabled: bool,
    /// Optional batch size for export operations.
    pub batch_size: Option<usize>,
    /// Optional timeout in seconds for export operations.
    pub timeout_secs: Option<u64>,
}

/// Tracing settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracingSettings {
    /// Whether distributed tracing is enabled.
    pub enabled: bool,
    /// Sampling rate for traces (0.0 to 1.0).
    pub sampling_rate: f64,
    /// Maximum number of spans allowed per trace.
    pub max_spans_per_trace: usize,
    /// Timeout in seconds after which spans are auto-completed.
    pub span_timeout_secs: u64,
}

/// Logging settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingSettings {
    /// Whether logging is enabled.
    pub enabled: bool,
    /// Minimum log level (e.g., debug, info, warn, error).
    pub level: String,
    /// Log output format.
    pub format: LogFormat,
    /// Log output destination.
    pub output: LogOutput,
}

/// Log format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogFormat {
    /// JSON structured log format.
    Json,
    /// Plain text log format.
    Text,
    /// Human-readable pretty-printed format.
    Pretty,
}

/// Log output destination.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogOutput {
    /// Output logs to standard output.
    Stdout,
    /// Output logs to a file.
    File {
        /// Path to the log file.
        path: String,
    },
    /// Output logs to both stdout and a file.
    Both {
        /// Path to the log file.
        path: String,
    },
}

/// SLO settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SloSettings {
    /// Whether SLO monitoring is enabled.
    pub enabled: bool,
    /// Interval in seconds between SLO evaluations.
    pub evaluation_interval_secs: u64,
    /// List of configured SLO definitions.
    pub slos: Vec<SloDefinition>,
}

/// SLO definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SloDefinition {
    /// Name of the SLO.
    pub name: String,
    /// Target percentage (0.0 to 100.0) for the SLO.
    pub target: f64,
    /// Query expression to calculate the SLI.
    pub sli_query: String,
    /// Rolling window size in days for SLO calculation.
    pub window_days: u32,
}

/// Alerting settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertingSettings {
    /// Whether alerting is enabled.
    pub enabled: bool,
    /// Interval in seconds between alert rule evaluations.
    pub evaluation_interval_secs: u64,
    /// List of configured alert rules.
    pub rules: Vec<AlertRuleConfig>,
    /// List of alert routing rules.
    pub routes: Vec<RouteConfig>,
}

/// Alert rule configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRuleConfig {
    /// Name of the alert rule.
    pub name: String,
    /// Query expression that triggers the alert when true.
    pub expression: String,
    /// Duration in seconds the condition must be true before firing.
    pub duration_secs: u64,
    /// Severity level of the alert (e.g., warning, critical).
    pub severity: String,
    /// Additional labels attached to the alert.
    pub labels: std::collections::HashMap<String, String>,
}

/// Route configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteConfig {
    /// Label matchers to determine which alerts match this route.
    pub matchers: std::collections::HashMap<String, String>,
    /// List of destination identifiers to send matched alerts to.
    pub destinations: Vec<String>,
}

/// Dashboard settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardSettings {
    /// Whether dashboard integration is enabled.
    pub enabled: bool,
    /// Optional Grafana server URL.
    pub grafana_url: Option<String>,
    /// Optional Prometheus server URL for data source.
    pub prometheus_url: Option<String>,
    /// Whether to automatically import dashboards on startup.
    pub auto_import: bool,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            telemetry: TelemetrySettings {
                enabled: true,
                service_name: "oxigdal".to_string(),
                service_version: env!("CARGO_PKG_VERSION").to_string(),
                service_namespace: None,
                environment: "development".to_string(),
            },
            metrics: MetricsSettings {
                enabled: true,
                collection_interval_secs: 10,
                export_interval_secs: 60,
                exporters: vec![],
            },
            tracing: TracingSettings {
                enabled: true,
                sampling_rate: 0.1,
                max_spans_per_trace: 1000,
                span_timeout_secs: 300,
            },
            logging: LoggingSettings {
                enabled: true,
                level: "info".to_string(),
                format: LogFormat::Json,
                output: LogOutput::Stdout,
            },
            slo: SloSettings {
                enabled: false,
                evaluation_interval_secs: 60,
                slos: vec![],
            },
            alerting: AlertingSettings {
                enabled: false,
                evaluation_interval_secs: 60,
                rules: vec![],
                routes: vec![],
            },
            dashboards: DashboardSettings {
                enabled: false,
                grafana_url: None,
                prometheus_url: None,
                auto_import: false,
            },
        }
    }
}

impl ObservabilityConfig {
    /// Load configuration from YAML file.
    pub fn from_yaml_file(path: impl AsRef<Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        serde_json::from_str(&content).map_err(crate::error::ObservabilityError::Serialization)
    }

    /// Load configuration from JSON file.
    pub fn from_json_file(path: impl AsRef<Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        serde_json::from_str(&content).map_err(crate::error::ObservabilityError::Serialization)
    }

    /// Save configuration to YAML file.
    pub fn to_yaml_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Save configuration to JSON file.
    pub fn to_json_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Validate configuration.
    pub fn validate(&self) -> Result<()> {
        // Validate sampling rate
        if self.tracing.sampling_rate < 0.0 || self.tracing.sampling_rate > 1.0 {
            return Err(crate::error::ObservabilityError::InvalidConfig(
                "Sampling rate must be between 0.0 and 1.0".to_string(),
            ));
        }

        // Validate intervals
        if self.metrics.collection_interval_secs == 0 {
            return Err(crate::error::ObservabilityError::InvalidConfig(
                "Collection interval must be greater than 0".to_string(),
            ));
        }

        // Validate SLO targets
        for slo in &self.slo.slos {
            if slo.target < 0.0 || slo.target > 100.0 {
                return Err(crate::error::ObservabilityError::InvalidConfig(format!(
                    "SLO target must be between 0 and 100: {}",
                    slo.name
                )));
            }
        }

        Ok(())
    }
}

/// Configuration builder for fluent API.
pub struct ConfigBuilder {
    config: ObservabilityConfig,
}

impl ConfigBuilder {
    /// Create a new configuration builder.
    pub fn new() -> Self {
        Self {
            config: ObservabilityConfig::default(),
        }
    }

    /// Set service name.
    pub fn service_name(mut self, name: impl Into<String>) -> Self {
        self.config.telemetry.service_name = name.into();
        self
    }

    /// Set service version.
    pub fn service_version(mut self, version: impl Into<String>) -> Self {
        self.config.telemetry.service_version = version.into();
        self
    }

    /// Set environment.
    pub fn environment(mut self, env: impl Into<String>) -> Self {
        self.config.telemetry.environment = env.into();
        self
    }

    /// Enable/disable metrics.
    pub fn metrics_enabled(mut self, enabled: bool) -> Self {
        self.config.metrics.enabled = enabled;
        self
    }

    /// Enable/disable tracing.
    pub fn tracing_enabled(mut self, enabled: bool) -> Self {
        self.config.tracing.enabled = enabled;
        self
    }

    /// Set sampling rate.
    pub fn sampling_rate(mut self, rate: f64) -> Self {
        self.config.tracing.sampling_rate = rate.clamp(0.0, 1.0);
        self
    }

    /// Add an exporter.
    pub fn add_exporter(mut self, exporter: ExporterConfig) -> Self {
        self.config.metrics.exporters.push(exporter);
        self
    }

    /// Add an SLO.
    pub fn add_slo(mut self, slo: SloDefinition) -> Self {
        self.config.slo.slos.push(slo);
        self.config.slo.enabled = true;
        self
    }

    /// Build the configuration.
    pub fn build(self) -> Result<ObservabilityConfig> {
        self.config.validate()?;
        Ok(self.config)
    }
}

impl Default for ConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ObservabilityConfig::default();
        assert!(config.telemetry.enabled);
        assert_eq!(config.telemetry.service_name, "oxigdal");
    }

    #[test]
    fn test_config_validation() {
        let mut config = ObservabilityConfig::default();
        assert!(config.validate().is_ok());

        config.tracing.sampling_rate = 1.5;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_builder() {
        let config = ConfigBuilder::new()
            .service_name("test-service")
            .service_version("1.0.0")
            .environment("production")
            .sampling_rate(0.5)
            .metrics_enabled(true)
            .build()
            .expect("Failed to build config");

        assert_eq!(config.telemetry.service_name, "test-service");
        assert_eq!(config.tracing.sampling_rate, 0.5);
    }
}
