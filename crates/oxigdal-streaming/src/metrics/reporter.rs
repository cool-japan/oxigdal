//! Metrics reporting for streaming operations.

use super::collector::{Metric, MetricType, MetricValue};
use crate::error::Result;
use async_trait::async_trait;
use serde_json;

/// Trait for metrics reporters.
#[async_trait]
pub trait MetricsReporter: Send + Sync {
    /// Report a single metric.
    async fn report_metric(&self, metric: &Metric) -> Result<()>;

    /// Report multiple metrics.
    async fn report_metrics(&self, metrics: &[Metric]) -> Result<()>;

    /// Flush any buffered metrics.
    async fn flush(&self) -> Result<()>;
}

/// Console reporter that prints metrics to stdout.
pub struct ConsoleReporter {
    format: ReportFormat,
}

/// Format for console output.
#[derive(Debug, Clone, Copy)]
pub enum ReportFormat {
    /// Human-readable format
    Human,

    /// JSON format
    Json,

    /// Prometheus format
    Prometheus,
}

impl ConsoleReporter {
    /// Create a new console reporter.
    pub fn new() -> Self {
        Self {
            format: ReportFormat::Human,
        }
    }

    /// Create a console reporter with specified format.
    pub fn with_format(format: ReportFormat) -> Self {
        Self { format }
    }

    /// Format a metric for human-readable output.
    fn format_human(&self, metric: &Metric) -> String {
        let value_str = match &metric.value {
            MetricValue::Integer(v) => format!("{}", v),
            MetricValue::Float(v) => format!("{:.2}", v),
            MetricValue::Histogram { buckets, counts } => {
                let mut s = String::from("[");
                for (bucket, count) in buckets.iter().zip(counts.iter()) {
                    s.push_str(&format!("{}:{}, ", bucket, count));
                }
                s.push(']');
                s
            }
            MetricValue::Summary {
                count,
                sum,
                quantiles,
            } => {
                format!("count={}, sum={:.2}, quantiles={:?}", count, sum, quantiles)
            }
        };

        let tags_str = if metric.tags.is_empty() {
            String::new()
        } else {
            let tags: Vec<String> = metric
                .tags
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            format!(" {{{}}}", tags.join(", "))
        };

        format!(
            "{} [{:?}] = {}{}",
            metric.name, metric.metric_type, value_str, tags_str
        )
    }

    /// Format a metric for Prometheus output.
    fn format_prometheus(&self, metric: &Metric) -> String {
        let help = if let Some(h) = &metric.help {
            format!("# HELP {} {}\n", metric.name, h)
        } else {
            String::new()
        };

        let type_str = match metric.metric_type {
            MetricType::Counter => "counter",
            MetricType::Gauge => "gauge",
            MetricType::Histogram => "histogram",
            MetricType::Summary => "summary",
            MetricType::Timer => "gauge",
        };

        let type_line = format!("# TYPE {} {}\n", metric.name, type_str);

        let value_line = match &metric.value {
            MetricValue::Integer(v) => {
                format!("{} {}", metric.name, v)
            }
            MetricValue::Float(v) => {
                format!("{} {}", metric.name, v)
            }
            MetricValue::Histogram { buckets, counts } => {
                let mut lines = Vec::new();
                let mut cumulative = 0;

                for (bucket, count) in buckets.iter().zip(counts.iter()) {
                    cumulative += count;
                    lines.push(format!(
                        "{}_bucket{{le=\"{}\"}} {}",
                        metric.name, bucket, cumulative
                    ));
                }

                lines.push(format!(
                    "{}_bucket{{le=\"+Inf\"}} {}",
                    metric.name, cumulative
                ));
                lines.push(format!("{}_count {}", metric.name, cumulative));

                lines.join("\n")
            }
            MetricValue::Summary {
                count,
                sum,
                quantiles,
            } => {
                let mut lines = Vec::new();

                for (quantile, value) in quantiles {
                    lines.push(format!(
                        "{}{{quantile=\"{}\"}} {}",
                        metric.name, quantile, value
                    ));
                }

                lines.push(format!("{}_sum {}", metric.name, sum));
                lines.push(format!("{}_count {}", metric.name, count));

                lines.join("\n")
            }
        };

        format!("{}{}{}", help, type_line, value_line)
    }
}

impl Default for ConsoleReporter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MetricsReporter for ConsoleReporter {
    async fn report_metric(&self, metric: &Metric) -> Result<()> {
        match self.format {
            ReportFormat::Human => {
                println!("{}", self.format_human(metric));
            }
            ReportFormat::Json => {
                let json = serde_json::to_string_pretty(metric)?;
                println!("{}", json);
            }
            ReportFormat::Prometheus => {
                println!("{}", self.format_prometheus(metric));
            }
        }

        Ok(())
    }

    async fn report_metrics(&self, metrics: &[Metric]) -> Result<()> {
        for metric in metrics {
            self.report_metric(metric).await?;
        }

        Ok(())
    }

    async fn flush(&self) -> Result<()> {
        Ok(())
    }
}

/// JSON reporter that outputs metrics as JSON.
pub struct JsonReporter {
    pretty: bool,
}

impl JsonReporter {
    /// Create a new JSON reporter.
    pub fn new() -> Self {
        Self { pretty: true }
    }

    /// Create a JSON reporter with compact output.
    pub fn compact() -> Self {
        Self { pretty: false }
    }
}

impl Default for JsonReporter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MetricsReporter for JsonReporter {
    async fn report_metric(&self, metric: &Metric) -> Result<()> {
        let json = if self.pretty {
            serde_json::to_string_pretty(metric)?
        } else {
            serde_json::to_string(metric)?
        };

        println!("{}", json);

        Ok(())
    }

    async fn report_metrics(&self, metrics: &[Metric]) -> Result<()> {
        let json = if self.pretty {
            serde_json::to_string_pretty(metrics)?
        } else {
            serde_json::to_string(metrics)?
        };

        println!("{}", json);

        Ok(())
    }

    async fn flush(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_console_reporter() {
        let reporter = ConsoleReporter::new();

        let metric = Metric::new(
            "test_metric".to_string(),
            MetricType::Counter,
            MetricValue::Integer(42),
        );

        reporter
            .report_metric(&metric)
            .await
            .expect("metric reporting should succeed");
    }

    #[tokio::test]
    async fn test_json_reporter() {
        let reporter = JsonReporter::new();

        let metric = Metric::new(
            "test_metric".to_string(),
            MetricType::Gauge,
            MetricValue::Float(std::f64::consts::E),
        );

        reporter
            .report_metric(&metric)
            .await
            .expect("metric reporting should succeed");
    }

    #[test]
    fn test_format_human() {
        let reporter = ConsoleReporter::new();

        let metric = Metric::new(
            "test".to_string(),
            MetricType::Counter,
            MetricValue::Integer(100),
        );

        let formatted = reporter.format_human(&metric);
        assert!(formatted.contains("test"));
        assert!(formatted.contains("100"));
    }

    #[test]
    fn test_format_prometheus() {
        let reporter = ConsoleReporter::with_format(ReportFormat::Prometheus);

        let metric = Metric::new(
            "test_counter".to_string(),
            MetricType::Counter,
            MetricValue::Integer(42),
        )
        .with_help("Test counter metric".to_string());

        let formatted = reporter.format_prometheus(&metric);
        assert!(formatted.contains("# HELP"));
        assert!(formatted.contains("# TYPE"));
    }
}
