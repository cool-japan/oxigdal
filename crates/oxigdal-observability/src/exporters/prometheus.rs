//! Prometheus exporter.

use super::{Metric, MetricExporter, MetricValue};
use crate::error::Result;
use parking_lot::RwLock;
use std::sync::Arc;

/// Prometheus exporter.
pub struct PrometheusExporter {
    #[allow(dead_code)]
    endpoint: String,
    #[allow(dead_code)]
    client: reqwest::Client,
    buffer: Arc<RwLock<Vec<Metric>>>,
}

impl PrometheusExporter {
    /// Create a new Prometheus exporter.
    pub fn new(endpoint: String) -> Self {
        Self {
            endpoint,
            client: reqwest::Client::new(),
            buffer: Arc::new(RwLock::new(Vec::new())),
        }
    }

    fn format_prometheus_metric(&self, metric: &Metric) -> String {
        let labels_str = if metric.labels.is_empty() {
            String::new()
        } else {
            let labels: Vec<String> = metric
                .labels
                .iter()
                .map(|(k, v)| format!("{}=\"{}\"", k, v))
                .collect();
            format!("{{{}}}", labels.join(","))
        };

        match &metric.value {
            MetricValue::Counter(value) => {
                format!("{}{}value {}\n", metric.name, labels_str, value)
            }
            MetricValue::Gauge(value) => {
                format!("{}{} {}\n", metric.name, labels_str, value)
            }
            MetricValue::Histogram(_) => {
                // Simplified histogram formatting
                format!("{}{} 0\n", metric.name, labels_str)
            }
            MetricValue::Summary(summary) => {
                format!(
                    "{}_sum{} {}\n{}_count{} {}\n",
                    metric.name, labels_str, summary.sum, metric.name, labels_str, summary.count
                )
            }
            MetricValue::Distribution(dist) => {
                format!("{}{} {}\n", metric.name, labels_str, dist.mean)
            }
        }
    }
}

impl MetricExporter for PrometheusExporter {
    fn export(&self, metrics: &[Metric]) -> Result<()> {
        self.buffer.write().extend_from_slice(metrics);
        Ok(())
    }

    fn flush(&self) -> Result<()> {
        let metrics = {
            let mut buffer = self.buffer.write();
            std::mem::take(&mut *buffer)
        };

        if metrics.is_empty() {
            return Ok(());
        }

        let _body: String = metrics
            .iter()
            .map(|m| self.format_prometheus_metric(m))
            .collect();

        // In production, this would POST to Prometheus push gateway
        // For now, just clear the buffer
        Ok(())
    }

    fn name(&self) -> &str {
        "prometheus"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prometheus_exporter() {
        let exporter = PrometheusExporter::new("http://localhost:9091".to_string());
        let metrics = vec![Metric::counter("test_counter".to_string(), 42)];

        assert!(exporter.export(&metrics).is_ok());
        assert!(exporter.flush().is_ok());
    }
}
