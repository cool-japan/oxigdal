//! Metrics collection with OpenTelemetry.

use crate::error::{ObservabilityError, Result};
use crate::telemetry::TelemetryConfig;
use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry::metrics::{Counter, Histogram, Meter, ObservableGauge, UpDownCounter};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Duration;

/// Initialize metrics collection based on configuration.
pub fn init_metrics(config: &TelemetryConfig, resource: Resource) -> Result<SdkMeterProvider> {
    let provider = if let Some(ref endpoint) = config.prometheus_endpoint {
        create_prometheus_provider(endpoint, resource)?
    } else if let Some(ref endpoint) = config.otlp_endpoint {
        #[cfg(feature = "otlp")]
        {
            create_otlp_provider(endpoint, resource)?
        }
        #[cfg(not(feature = "otlp"))]
        {
            return Err(ObservabilityError::ConfigError(
                "OTLP feature not enabled".to_string(),
            ));
        }
    } else {
        create_stdout_provider(resource)?
    };

    // Set global meter provider
    global::set_meter_provider(provider.clone());

    Ok(provider)
}

/// Create Prometheus provider.
fn create_prometheus_provider(_endpoint: &str, resource: Resource) -> Result<SdkMeterProvider> {
    let exporter = opentelemetry_stdout::MetricsExporter::default();
    let reader = PeriodicReader::builder(exporter, opentelemetry_sdk::runtime::Tokio)
        .with_interval(Duration::from_secs(60))
        .build();

    let provider = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_reader(reader)
        .build();

    Ok(provider)
}

/// Create OTLP provider.
#[cfg(feature = "otlp")]
fn create_otlp_provider(endpoint: &str, resource: Resource) -> Result<SdkMeterProvider> {
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::metrics::reader::DefaultAggregationSelector;
    use opentelemetry_sdk::metrics::reader::DefaultTemporalitySelector;

    let exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(endpoint)
        .build_metrics_exporter(
            Box::new(DefaultAggregationSelector::new()),
            Box::new(DefaultTemporalitySelector::new()),
        )
        .map_err(|e| ObservabilityError::MetricsExportFailed(e.to_string()))?;

    let reader = PeriodicReader::builder(exporter, opentelemetry_sdk::runtime::Tokio)
        .with_interval(Duration::from_secs(60))
        .build();

    let provider = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_reader(reader)
        .build();

    Ok(provider)
}

/// Create stdout provider for development.
fn create_stdout_provider(resource: Resource) -> Result<SdkMeterProvider> {
    let exporter = opentelemetry_stdout::MetricsExporter::default();
    let reader = PeriodicReader::builder(exporter, opentelemetry_sdk::runtime::Tokio)
        .with_interval(Duration::from_secs(60))
        .build();

    let provider = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_reader(reader)
        .build();

    Ok(provider)
}

/// Metric types wrapper for convenient metric creation.
pub struct MetricRegistry {
    meter: Meter,
    counters: Arc<RwLock<Vec<String>>>,
    histograms: Arc<RwLock<Vec<String>>>,
    gauges: Arc<RwLock<Vec<String>>>,
}

impl MetricRegistry {
    /// Create a new metric registry.
    pub fn new(meter: Meter) -> Self {
        Self {
            meter,
            counters: Arc::new(RwLock::new(Vec::new())),
            histograms: Arc::new(RwLock::new(Vec::new())),
            gauges: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create a counter metric.
    pub fn create_counter(
        &self,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Result<Counter<u64>> {
        let name_str = name.into();
        let counter = self
            .meter
            .u64_counter(name_str.clone())
            .with_description(description.into())
            .init();

        self.counters.write().push(name_str);
        Ok(counter)
    }

    /// Create an up-down counter metric.
    pub fn create_up_down_counter(
        &self,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Result<UpDownCounter<i64>> {
        let counter = self
            .meter
            .i64_up_down_counter(name.into())
            .with_description(description.into())
            .init();

        Ok(counter)
    }

    /// Create a histogram metric.
    pub fn create_histogram(
        &self,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Result<Histogram<f64>> {
        let name_str = name.into();
        let histogram = self
            .meter
            .f64_histogram(name_str.clone())
            .with_description(description.into())
            .init();

        self.histograms.write().push(name_str);
        Ok(histogram)
    }

    /// Create an observable gauge metric.
    pub fn create_gauge<F>(
        &self,
        name: impl Into<String>,
        description: impl Into<String>,
        callback: F,
    ) -> Result<ObservableGauge<f64>>
    where
        F: Fn(&dyn opentelemetry::metrics::AsyncInstrument<f64>) + Send + Sync + 'static,
    {
        let name_str = name.into();
        let gauge = self
            .meter
            .f64_observable_gauge(name_str.clone())
            .with_description(description.into())
            .with_callback(callback)
            .init();

        self.gauges.write().push(name_str);
        Ok(gauge)
    }

    /// Get list of registered counter names.
    pub fn counter_names(&self) -> Vec<String> {
        self.counters.read().clone()
    }

    /// Get list of registered histogram names.
    pub fn histogram_names(&self) -> Vec<String> {
        self.histograms.read().clone()
    }

    /// Get list of registered gauge names.
    pub fn gauge_names(&self) -> Vec<String> {
        self.gauges.read().clone()
    }
}

/// Common metric attributes.
pub struct MetricAttributes {
    attributes: Vec<KeyValue>,
}

impl MetricAttributes {
    /// Create new metric attributes.
    pub fn new() -> Self {
        Self {
            attributes: Vec::new(),
        }
    }

    /// Add a string attribute.
    pub fn add_string(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes
            .push(KeyValue::new(key.into(), value.into()));
        self
    }

    /// Add an integer attribute.
    pub fn add_int(mut self, key: impl Into<String>, value: i64) -> Self {
        self.attributes.push(KeyValue::new(key.into(), value));
        self
    }

    /// Add a float attribute.
    pub fn add_float(mut self, key: impl Into<String>, value: f64) -> Self {
        self.attributes.push(KeyValue::new(key.into(), value));
        self
    }

    /// Add a boolean attribute.
    pub fn add_bool(mut self, key: impl Into<String>, value: bool) -> Self {
        self.attributes.push(KeyValue::new(key.into(), value));
        self
    }

    /// Get the attributes as a slice.
    pub fn as_slice(&self) -> &[KeyValue] {
        &self.attributes
    }
}

impl Default for MetricAttributes {
    fn default() -> Self {
        Self::new()
    }
}

/// Histogram bucket boundaries for different use cases.
pub struct HistogramBounds;

impl HistogramBounds {
    /// Latency buckets in milliseconds (0.1ms to 60s).
    pub fn latency_ms() -> Vec<f64> {
        vec![
            0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0,
            10000.0, 30000.0, 60000.0,
        ]
    }

    /// Size buckets in bytes (1KB to 1GB).
    pub fn size_bytes() -> Vec<f64> {
        vec![
            1024.0,
            10240.0,
            102400.0,
            1048576.0,
            10485760.0,
            104857600.0,
            1073741824.0,
        ]
    }

    /// Percentage buckets (0% to 100%).
    pub fn percentage() -> Vec<f64> {
        vec![
            0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 95.0, 99.0, 100.0,
        ]
    }

    /// Count buckets (1 to 1M).
    pub fn count() -> Vec<f64> {
        vec![1.0, 10.0, 100.0, 1000.0, 10000.0, 100000.0, 1000000.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_attributes() {
        let attrs = MetricAttributes::new()
            .add_string("service", "oxigdal")
            .add_int("version", 1)
            .add_float("ratio", 0.5)
            .add_bool("enabled", true);

        assert_eq!(attrs.as_slice().len(), 4);
    }

    #[test]
    fn test_histogram_bounds() {
        let latency = HistogramBounds::latency_ms();
        assert!(!latency.is_empty());
        assert_eq!(latency[0], 0.1);

        let size = HistogramBounds::size_bytes();
        assert!(!size.is_empty());
        assert_eq!(size[0], 1024.0);

        let percentage = HistogramBounds::percentage();
        assert!(!percentage.is_empty());
        assert_eq!(percentage[0], 0.0);

        let count = HistogramBounds::count();
        assert!(!count.is_empty());
        assert_eq!(count[0], 1.0);
    }
}
