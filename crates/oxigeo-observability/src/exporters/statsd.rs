//! StatsD exporter.

use super::{Metric, MetricExporter, MetricValue};
use crate::error::Result;

/// StatsD exporter.
pub struct StatsdExporter {
    #[allow(dead_code)]
    address: String,
}

impl StatsdExporter {
    /// Create a new StatsD exporter.
    pub fn new(address: String) -> Self {
        Self { address }
    }
}

impl MetricExporter for StatsdExporter {
    fn export(&self, metrics: &[Metric]) -> Result<()> {
        for metric in metrics {
            let _message = match &metric.value {
                MetricValue::Counter(value) => format!("{}:{}|c", metric.name, value),
                MetricValue::Gauge(value) => format!("{}:{}|g", metric.name, value),
                MetricValue::Histogram(values) => {
                    let avg = if values.count > 0 {
                        values.sum / values.count as f64
                    } else {
                        0.0
                    };
                    format!("{}:{}|h", metric.name, avg)
                }
                MetricValue::Summary(summary) => {
                    let avg = if summary.count > 0 {
                        summary.sum / summary.count as f64
                    } else {
                        0.0
                    };
                    format!("{}:{}|ms", metric.name, avg)
                }
                MetricValue::Distribution(dist) => {
                    format!("{}:{}|d", metric.name, dist.mean)
                }
            };

            // In production, send via UDP to StatsD
        }

        Ok(())
    }

    fn flush(&self) -> Result<()> {
        Ok(())
    }

    fn name(&self) -> &str {
        "statsd"
    }
}
