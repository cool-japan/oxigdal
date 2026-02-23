//! OpenTelemetry integration and telemetry setup.

use crate::error::{ObservabilityError, Result};
use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use parking_lot::RwLock;
use std::sync::Arc;

pub mod logs;
pub mod metrics;
pub mod traces;

/// Telemetry configuration.
#[derive(Debug, Clone)]
pub struct TelemetryConfig {
    /// Service name.
    pub service_name: String,

    /// Service version.
    pub service_version: String,

    /// Service namespace.
    pub service_namespace: Option<String>,

    /// Service instance ID.
    pub service_instance_id: Option<String>,

    /// Enable traces.
    pub enable_traces: bool,

    /// Enable metrics.
    pub enable_metrics: bool,

    /// Enable logs.
    pub enable_logs: bool,

    /// Jaeger endpoint for traces.
    pub jaeger_endpoint: Option<String>,

    /// Prometheus endpoint for metrics.
    pub prometheus_endpoint: Option<String>,

    /// OTLP endpoint (supports all signals).
    pub otlp_endpoint: Option<String>,

    /// Sampling rate (0.0 to 1.0).
    pub sampling_rate: f64,

    /// Custom resource attributes.
    pub resource_attributes: Vec<(String, String)>,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            service_name: "oxigdal".to_string(),
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            service_namespace: None,
            service_instance_id: None,
            enable_traces: true,
            enable_metrics: true,
            enable_logs: true,
            jaeger_endpoint: None,
            prometheus_endpoint: None,
            otlp_endpoint: None,
            sampling_rate: 0.1,
            resource_attributes: Vec::new(),
        }
    }
}

impl TelemetryConfig {
    /// Create a new telemetry configuration.
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            ..Default::default()
        }
    }

    /// Set service version.
    pub fn with_service_version(mut self, version: impl Into<String>) -> Self {
        self.service_version = version.into();
        self
    }

    /// Set service namespace.
    pub fn with_service_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.service_namespace = Some(namespace.into());
        self
    }

    /// Set service instance ID.
    pub fn with_service_instance_id(mut self, instance_id: impl Into<String>) -> Self {
        self.service_instance_id = Some(instance_id.into());
        self
    }

    /// Enable or disable traces.
    pub fn with_traces(mut self, enable: bool) -> Self {
        self.enable_traces = enable;
        self
    }

    /// Enable or disable metrics.
    pub fn with_metrics(mut self, enable: bool) -> Self {
        self.enable_metrics = enable;
        self
    }

    /// Enable or disable logs.
    pub fn with_logs(mut self, enable: bool) -> Self {
        self.enable_logs = enable;
        self
    }

    /// Set Jaeger endpoint.
    pub fn with_jaeger_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.jaeger_endpoint = Some(endpoint.into());
        self
    }

    /// Set Prometheus endpoint.
    pub fn with_prometheus_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.prometheus_endpoint = Some(endpoint.into());
        self
    }

    /// Set OTLP endpoint.
    pub fn with_otlp_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.otlp_endpoint = Some(endpoint.into());
        self
    }

    /// Set sampling rate.
    pub fn with_sampling_rate(mut self, rate: f64) -> Self {
        self.sampling_rate = rate.clamp(0.0, 1.0);
        self
    }

    /// Add a resource attribute.
    pub fn with_resource_attribute(
        mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.resource_attributes.push((key.into(), value.into()));
        self
    }
}

/// Telemetry provider managing all observability components.
pub struct TelemetryProvider {
    config: TelemetryConfig,
    meter_provider: Option<Arc<RwLock<SdkMeterProvider>>>,
    is_initialized: Arc<RwLock<bool>>,
}

impl TelemetryProvider {
    /// Create a new telemetry provider.
    pub fn new(config: TelemetryConfig) -> Self {
        Self {
            config,
            meter_provider: None,
            is_initialized: Arc::new(RwLock::new(false)),
        }
    }

    /// Initialize telemetry with the given configuration.
    pub async fn init(&mut self) -> Result<()> {
        {
            let initialized = self.is_initialized.write();
            if *initialized {
                return Err(ObservabilityError::TelemetryInit(
                    "Telemetry already initialized".to_string(),
                ));
            }
        }

        // Create resource with service information
        let resource = self.create_resource()?;

        // Initialize traces if enabled
        if self.config.enable_traces {
            self.init_traces(&resource).await?;
        }

        // Initialize metrics if enabled
        if self.config.enable_metrics {
            self.init_metrics(&resource)?;
        }

        // Initialize logs if enabled
        if self.config.enable_logs {
            self.init_logs()?;
        }

        *self.is_initialized.write() = true;
        Ok(())
    }
    /// Create OpenTelemetry resource with service information.
    fn create_resource(&self) -> Result<Resource> {
        let mut attributes = vec![
            KeyValue::new("service.name", self.config.service_name.clone()),
            KeyValue::new("service.version", self.config.service_version.clone()),
        ];

        if let Some(ref namespace) = self.config.service_namespace {
            attributes.push(KeyValue::new("service.namespace", namespace.clone()));
        }

        if let Some(ref instance_id) = self.config.service_instance_id {
            attributes.push(KeyValue::new("service.instance.id", instance_id.clone()));
        }

        // Add custom resource attributes
        for (key, value) in &self.config.resource_attributes {
            attributes.push(KeyValue::new(key.clone(), value.clone()));
        }

        Ok(Resource::new(attributes))
    }

    /// Initialize distributed tracing.
    async fn init_traces(&self, resource: &Resource) -> Result<()> {
        traces::init_tracing(&self.config, resource.clone()).await
    }

    /// Initialize metrics collection.
    fn init_metrics(&mut self, resource: &Resource) -> Result<()> {
        let meter_provider = metrics::init_metrics(&self.config, resource.clone())?;
        self.meter_provider = Some(Arc::new(RwLock::new(meter_provider)));
        Ok(())
    }

    /// Initialize structured logging.
    fn init_logs(&self) -> Result<()> {
        logs::init_logging(&self.config)
    }

    /// Check if telemetry is initialized.
    pub fn is_initialized(&self) -> bool {
        *self.is_initialized.read()
    }

    /// Shutdown telemetry gracefully.
    pub async fn shutdown(&self) -> Result<()> {
        if !self.is_initialized() {
            return Ok(());
        }

        // Shutdown traces
        if self.config.enable_traces {
            global::shutdown_tracer_provider();
        }

        // Shutdown metrics
        if let Some(ref provider) = self.meter_provider {
            let provider = provider.read();
            provider.shutdown().map_err(|e| {
                ObservabilityError::Other(format!("Failed to shutdown meter provider: {}", e))
            })?;
        }

        Ok(())
    }

    /// Get the meter provider.
    pub fn meter_provider(&self) -> Option<Arc<RwLock<SdkMeterProvider>>> {
        self.meter_provider.clone()
    }
}

/// Initialize global telemetry with default configuration.
pub async fn init_default() -> Result<TelemetryProvider> {
    let config = TelemetryConfig::default();
    let mut provider = TelemetryProvider::new(config);
    provider.init().await?;
    Ok(provider)
}

/// Initialize global telemetry with custom configuration.
pub async fn init_with_config(config: TelemetryConfig) -> Result<TelemetryProvider> {
    let mut provider = TelemetryProvider::new(config);
    provider.init().await?;
    Ok(provider)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_config_builder() {
        let config = TelemetryConfig::new("test-service")
            .with_service_version("1.0.0")
            .with_service_namespace("testing")
            .with_sampling_rate(0.5)
            .with_resource_attribute("env", "dev");

        assert_eq!(config.service_name, "test-service");
        assert_eq!(config.service_version, "1.0.0");
        assert_eq!(config.service_namespace, Some("testing".to_string()));
        assert_eq!(config.sampling_rate, 0.5);
        assert_eq!(config.resource_attributes.len(), 1);
    }

    #[test]
    fn test_sampling_rate_clamping() {
        let config = TelemetryConfig::default().with_sampling_rate(1.5);
        assert_eq!(config.sampling_rate, 1.0);

        let config = TelemetryConfig::default().with_sampling_rate(-0.5);
        assert_eq!(config.sampling_rate, 0.0);
    }
}
