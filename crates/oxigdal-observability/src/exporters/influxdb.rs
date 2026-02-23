//! InfluxDB exporter.

use super::{Metric, MetricExporter, MetricValue};
use crate::error::Result;

/// InfluxDB exporter.
pub struct InfluxDbExporter {
    #[allow(dead_code)]
    endpoint: String,
    #[allow(dead_code)]
    database: String,
    #[allow(dead_code)]
    client: reqwest::Client,
}

impl InfluxDbExporter {
    /// Create a new InfluxDB exporter.
    pub fn new(endpoint: String, database: String) -> Self {
        Self {
            endpoint,
            database,
            client: reqwest::Client::new(),
        }
    }
}

impl MetricExporter for InfluxDbExporter {
    fn export(&self, metrics: &[Metric]) -> Result<()> {
        let mut lines = Vec::new();

        for metric in metrics {
            let tags = metric
                .labels
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(",");

            let tag_str = if tags.is_empty() {
                String::new()
            } else {
                format!(",{}", tags)
            };

            let value_str = match &metric.value {
                MetricValue::Counter(v) => format!("value={}i", v),
                MetricValue::Gauge(v) => format!("value={}", v),
                MetricValue::Histogram(_) => "value=0".to_string(),
                MetricValue::Summary(summary) => {
                    format!("sum={},count={}i", summary.sum, summary.count)
                }
                MetricValue::Distribution(dist) => {
                    format!("mean={},stddev={}", dist.mean, dist.std_dev)
                }
            };

            let timestamp = metric.timestamp.timestamp_nanos_opt().unwrap_or(0);
            let line = format!("{}{} {} {}", metric.name, tag_str, value_str, timestamp);
            lines.push(line);
        }

        // In production, POST to InfluxDB
        Ok(())
    }

    fn flush(&self) -> Result<()> {
        Ok(())
    }

    fn name(&self) -> &str {
        "influxdb"
    }
}
