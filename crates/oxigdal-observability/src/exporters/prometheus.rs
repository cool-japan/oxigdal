//! Prometheus exporter.
//!
//! [`PrometheusExporter`] buffers metrics and, on [`MetricExporter::flush`], formats them into
//! the Prometheus text exposition format and POSTs the payload to a Prometheus push gateway
//! endpoint. Actually transmitting the payload requires the `http-exporter` Cargo feature
//! (which pulls in `reqwest`); without it, `flush()` cannot deliver metrics anywhere and
//! returns an [`crate::error::ObservabilityError::ExporterFeatureDisabled`] error rather than
//! silently discarding the buffered metrics while reporting success.

use super::{Metric, MetricExporter, MetricValue};
use crate::error::Result;
use parking_lot::RwLock;
use std::sync::Arc;

/// Prometheus exporter.
pub struct PrometheusExporter {
    endpoint: String,
    #[cfg(feature = "http-exporter")]
    client: reqwest::blocking::Client,
    buffer: Arc<RwLock<Vec<Metric>>>,
}

impl PrometheusExporter {
    /// Create a new Prometheus exporter.
    ///
    /// `endpoint` should be the URL of a Prometheus push gateway (e.g.
    /// `"http://localhost:9091/metrics/job/oxigdal"`). Actually pushing metrics to this
    /// endpoint requires building this crate with the `http-exporter` feature enabled.
    pub fn new(endpoint: String) -> Self {
        Self {
            endpoint,
            #[cfg(feature = "http-exporter")]
            client: reqwest::blocking::Client::new(),
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
                format!("{}{} {}\n", metric.name, labels_str, value)
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

        let body: String = metrics
            .iter()
            .map(|m| self.format_prometheus_metric(m))
            .collect();

        #[cfg(feature = "http-exporter")]
        {
            self.client
                .post(&self.endpoint)
                .header(reqwest::header::CONTENT_TYPE, "text/plain; version=0.0.4")
                .body(body)
                .send()?
                .error_for_status()?;
            Ok(())
        }

        #[cfg(not(feature = "http-exporter"))]
        {
            let _ = body;
            Err(crate::error::ObservabilityError::ExporterFeatureDisabled(
                format!(
                    "PrometheusExporter::flush dropped {} metric(s) destined for '{}': this build \
                 was compiled without the 'http-exporter' feature, so there is no transport to \
                 push metrics to a Prometheus push gateway. Rebuild with the 'http-exporter' \
                 feature enabled to actually deliver metrics.",
                    metrics.len(),
                    self.endpoint
                ),
            ))
        }
    }

    fn name(&self) -> &str {
        "prometheus"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_counter_no_labels() {
        let exporter = PrometheusExporter::new("http://localhost:9091".to_string());
        let metric = Metric::counter("test_counter".to_string(), 42);
        assert_eq!(
            exporter.format_prometheus_metric(&metric),
            "test_counter 42\n"
        );
    }

    #[test]
    fn test_format_counter_with_labels() {
        let exporter = PrometheusExporter::new("http://localhost:9091".to_string());
        let metric = Metric::counter("foo".to_string(), 1).with_label("a", "b");
        assert_eq!(
            exporter.format_prometheus_metric(&metric),
            "foo{a=\"b\"} 1\n"
        );
    }

    #[test]
    fn test_format_gauge_no_labels() {
        let exporter = PrometheusExporter::new("http://localhost:9091".to_string());
        let metric = Metric::gauge("temperature".to_string(), 23.5);
        assert_eq!(
            exporter.format_prometheus_metric(&metric),
            "temperature 23.5\n"
        );
    }

    #[test]
    fn test_prometheus_exporter_flush_empty_buffer_is_ok() {
        // An empty buffer has nothing to transmit, so flush() should succeed even without
        // the http-exporter feature.
        let exporter = PrometheusExporter::new("http://localhost:9091".to_string());
        assert!(exporter.flush().is_ok());
    }

    #[test]
    fn test_prometheus_exporter_export_buffers_metrics() {
        let exporter = PrometheusExporter::new("http://localhost:9091".to_string());
        let metrics = vec![Metric::counter("test_counter".to_string(), 42)];
        assert!(exporter.export(&metrics).is_ok());
        assert_eq!(exporter.buffer.read().len(), 1);
    }

    #[cfg(not(feature = "http-exporter"))]
    #[test]
    fn test_flush_without_http_exporter_feature_errors_instead_of_dropping_silently() {
        let exporter = PrometheusExporter::new("http://localhost:9091/metrics".to_string());
        let metrics = vec![Metric::counter("test_counter".to_string(), 42)];

        exporter.export(&metrics).expect("export should succeed");
        let result = exporter.flush();
        assert!(
            result.is_err(),
            "flush() must not silently report success when there is no transport \
             (http-exporter feature disabled) to deliver metrics"
        );
        // The buffer should be drained regardless (metrics aren't retried), so a subsequent
        // flush on an empty buffer succeeds.
        assert!(exporter.flush().is_ok());
    }

    #[cfg(feature = "http-exporter")]
    fn spawn_mock_push_gateway() -> (String, std::sync::mpsc::Receiver<String>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock push gateway");
        let addr = listener.local_addr().expect("mock gateway local addr");
        let (tx, rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(500)));
                let mut buf = [0u8; 8192];
                let mut received = Vec::new();
                loop {
                    match stream.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => received.extend_from_slice(&buf[..n]),
                        // Timed out / would-block once the client has finished sending: the
                        // full request has arrived, stop reading.
                        Err(_) => break,
                    }
                }
                let text = String::from_utf8_lossy(&received).to_string();
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
                let _ = tx.send(text);
            }
        });

        (format!("http://{addr}/metrics"), rx)
    }

    #[cfg(feature = "http-exporter")]
    #[test]
    fn test_flush_posts_formatted_metrics_to_push_gateway() {
        let (endpoint, rx) = spawn_mock_push_gateway();
        let exporter = PrometheusExporter::new(endpoint);
        let metrics = vec![Metric::counter("test_counter".to_string(), 42)];

        exporter.export(&metrics).expect("export should succeed");
        exporter
            .flush()
            .expect("flush should succeed and POST to the mock push gateway");

        let received = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("mock push gateway should have received a request");
        assert!(received.starts_with("POST"), "request was: {received}");
        assert!(
            received.contains("test_counter 42"),
            "request body should contain the formatted metric, was: {received}"
        );
    }

    #[cfg(feature = "http-exporter")]
    #[test]
    fn test_flush_errors_on_unreachable_endpoint() {
        // Bind then immediately drop a listener so its port is guaranteed to have nothing
        // listening on it.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        drop(listener);

        let exporter = PrometheusExporter::new(format!("http://{addr}/metrics"));
        let metrics = vec![Metric::counter("test_counter".to_string(), 42)];
        exporter.export(&metrics).expect("export should succeed");

        let result = exporter.flush();
        assert!(
            result.is_err(),
            "flush() against an unreachable endpoint must return an error, not silently succeed"
        );
    }
}
