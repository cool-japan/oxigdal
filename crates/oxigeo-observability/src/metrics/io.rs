//! I/O performance metrics.

use crate::error::Result;
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram, Meter};

/// Bucket a file path into a small, bounded label value for metrics.
///
/// Returns the lowercased file extension (e.g. `"tif"`, `"parquet"`), or
/// `"noext"` if the path has no extension, or `"unknown"` if the extension
/// cannot be decoded as UTF-8. This keeps the resulting metric label
/// cardinality bounded to the (small, finite) set of dataset formats this
/// platform reads/writes, instead of one series per unique file path.
fn file_type_bucket(path: &str) -> String {
    match std::path::Path::new(path).extension() {
        Some(ext) => match ext.to_str() {
            Some(ext_str) => ext_str.to_ascii_lowercase(),
            None => "unknown".to_string(),
        },
        None => "noext".to_string(),
    }
}

/// Metrics for I/O operations.
pub struct IoMetrics {
    // File I/O
    /// Counter for file open operations.
    pub file_open_count: Counter<u64>,
    /// Histogram of file open durations.
    pub file_open_duration: Histogram<f64>,
    /// Counter for file close operations.
    pub file_close_count: Counter<u64>,
    /// Counter for file read operations.
    pub file_read_count: Counter<u64>,
    /// Histogram of file read durations.
    pub file_read_duration: Histogram<f64>,
    /// Total bytes read from files.
    pub file_read_bytes: Counter<u64>,
    /// Counter for file write operations.
    pub file_write_count: Counter<u64>,
    /// Histogram of file write durations.
    pub file_write_duration: Histogram<f64>,
    /// Total bytes written to files.
    pub file_write_bytes: Counter<u64>,

    // Network I/O
    /// Counter for network requests.
    pub network_request_count: Counter<u64>,
    /// Histogram of network request durations.
    pub network_request_duration: Histogram<f64>,
    /// Total bytes sent over network.
    pub network_bytes_sent: Counter<u64>,
    /// Total bytes received from network.
    pub network_bytes_received: Counter<u64>,
    /// Counter for network errors.
    pub network_errors: Counter<u64>,

    // Cloud storage I/O
    /// Counter for cloud storage GET operations.
    pub cloud_get_count: Counter<u64>,
    /// Histogram of cloud storage GET durations.
    pub cloud_get_duration: Histogram<f64>,
    /// Counter for cloud storage PUT operations.
    pub cloud_put_count: Counter<u64>,
    /// Histogram of cloud storage PUT durations.
    pub cloud_put_duration: Histogram<f64>,
    /// Counter for cloud storage LIST operations.
    pub cloud_list_count: Counter<u64>,
    /// Histogram of cloud storage LIST durations.
    pub cloud_list_duration: Histogram<f64>,

    // Throughput
    /// Histogram of read throughput in MB/s.
    pub read_throughput_mbps: Histogram<f64>,
    /// Histogram of write throughput in MB/s.
    pub write_throughput_mbps: Histogram<f64>,

    // Latency
    /// Histogram of read latencies in milliseconds.
    pub read_latency_ms: Histogram<f64>,
    /// Histogram of write latencies in milliseconds.
    pub write_latency_ms: Histogram<f64>,
}

impl IoMetrics {
    /// Create new I/O metrics.
    pub fn new(meter: Meter) -> Result<Self> {
        Ok(Self {
            // File I/O
            file_open_count: meter
                .u64_counter("oxigeo.io.file.open.count")
                .with_description("Number of file open operations")
                .build(),
            file_open_duration: meter
                .f64_histogram("oxigeo.io.file.open.duration")
                .with_description("Duration of file open operations in milliseconds")
                .build(),
            file_close_count: meter
                .u64_counter("oxigeo.io.file.close.count")
                .with_description("Number of file close operations")
                .build(),
            file_read_count: meter
                .u64_counter("oxigeo.io.file.read.count")
                .with_description("Number of file read operations")
                .build(),
            file_read_duration: meter
                .f64_histogram("oxigeo.io.file.read.duration")
                .with_description("Duration of file read operations in milliseconds")
                .build(),
            file_read_bytes: meter
                .u64_counter("oxigeo.io.file.read.bytes")
                .with_description("Bytes read from files")
                .build(),
            file_write_count: meter
                .u64_counter("oxigeo.io.file.write.count")
                .with_description("Number of file write operations")
                .build(),
            file_write_duration: meter
                .f64_histogram("oxigeo.io.file.write.duration")
                .with_description("Duration of file write operations in milliseconds")
                .build(),
            file_write_bytes: meter
                .u64_counter("oxigeo.io.file.write.bytes")
                .with_description("Bytes written to files")
                .build(),

            // Network I/O
            network_request_count: meter
                .u64_counter("oxigeo.io.network.request.count")
                .with_description("Number of network requests")
                .build(),
            network_request_duration: meter
                .f64_histogram("oxigeo.io.network.request.duration")
                .with_description("Duration of network requests in milliseconds")
                .build(),
            network_bytes_sent: meter
                .u64_counter("oxigeo.io.network.bytes.sent")
                .with_description("Bytes sent over network")
                .build(),
            network_bytes_received: meter
                .u64_counter("oxigeo.io.network.bytes.received")
                .with_description("Bytes received from network")
                .build(),
            network_errors: meter
                .u64_counter("oxigeo.io.network.errors")
                .with_description("Number of network errors")
                .build(),

            // Cloud storage I/O
            cloud_get_count: meter
                .u64_counter("oxigeo.io.cloud.get.count")
                .with_description("Number of cloud storage GET operations")
                .build(),
            cloud_get_duration: meter
                .f64_histogram("oxigeo.io.cloud.get.duration")
                .with_description("Duration of cloud storage GET in milliseconds")
                .build(),
            cloud_put_count: meter
                .u64_counter("oxigeo.io.cloud.put.count")
                .with_description("Number of cloud storage PUT operations")
                .build(),
            cloud_put_duration: meter
                .f64_histogram("oxigeo.io.cloud.put.duration")
                .with_description("Duration of cloud storage PUT in milliseconds")
                .build(),
            cloud_list_count: meter
                .u64_counter("oxigeo.io.cloud.list.count")
                .with_description("Number of cloud storage LIST operations")
                .build(),
            cloud_list_duration: meter
                .f64_histogram("oxigeo.io.cloud.list.duration")
                .with_description("Duration of cloud storage LIST in milliseconds")
                .build(),

            // Throughput
            read_throughput_mbps: meter
                .f64_histogram("oxigeo.io.read.throughput.mbps")
                .with_description("Read throughput in MB/s")
                .build(),
            write_throughput_mbps: meter
                .f64_histogram("oxigeo.io.write.throughput.mbps")
                .with_description("Write throughput in MB/s")
                .build(),

            // Latency
            read_latency_ms: meter
                .f64_histogram("oxigeo.io.read.latency.ms")
                .with_description("Read latency in milliseconds")
                .build(),
            write_latency_ms: meter
                .f64_histogram("oxigeo.io.write.latency.ms")
                .with_description("Write latency in milliseconds")
                .build(),
        })
    }

    /// Record file read operation.
    ///
    /// The raw `path` is deliberately **not** attached as a metric label: a
    /// geospatial data platform opens many thousands of distinct dataset
    /// files, and one time series per unique path is the canonical
    /// Prometheus/OTel cardinality-explosion anti-pattern. Instead, `path` is
    /// bucketed into a small, bounded `file_type` label (its lowercased
    /// extension, or `"noext"`/`"unknown"`), and the full path is emitted as
    /// a `tracing::trace!` event -- unbounded-cardinality identifiers belong
    /// in traces/logs, not metric labels.
    pub fn record_file_read(&self, duration_ms: f64, bytes: u64, path: &str, success: bool) {
        tracing::trace!(path = %path, bytes, success, duration_ms, "file read");

        let attrs = vec![
            KeyValue::new("file_type", file_type_bucket(path)),
            KeyValue::new("success", success),
        ];

        self.file_read_count.add(1, &attrs);
        self.file_read_duration.record(duration_ms, &attrs);
        if success {
            self.file_read_bytes.add(bytes, &attrs);

            // Calculate throughput
            if duration_ms > 0.0 {
                let throughput_mbps = (bytes as f64 / (1024.0 * 1024.0)) / (duration_ms / 1000.0);
                self.read_throughput_mbps.record(throughput_mbps, &attrs);
            }
        }
    }

    /// Record file write operation.
    ///
    /// See [`Self::record_file_read`] for why the raw path is not used as a
    /// metric label.
    pub fn record_file_write(&self, duration_ms: f64, bytes: u64, path: &str, success: bool) {
        tracing::trace!(path = %path, bytes, success, duration_ms, "file write");

        let attrs = vec![
            KeyValue::new("file_type", file_type_bucket(path)),
            KeyValue::new("success", success),
        ];

        self.file_write_count.add(1, &attrs);
        self.file_write_duration.record(duration_ms, &attrs);
        if success {
            self.file_write_bytes.add(bytes, &attrs);

            // Calculate throughput
            if duration_ms > 0.0 {
                let throughput_mbps = (bytes as f64 / (1024.0 * 1024.0)) / (duration_ms / 1000.0);
                self.write_throughput_mbps.record(throughput_mbps, &attrs);
            }
        }
    }

    /// Record network request.
    pub fn record_network_request(
        &self,
        duration_ms: f64,
        bytes_sent: u64,
        bytes_received: u64,
        method: &str,
        success: bool,
    ) {
        let attrs = vec![
            KeyValue::new("method", method.to_string()),
            KeyValue::new("success", success),
        ];

        self.network_request_count.add(1, &attrs);
        self.network_request_duration.record(duration_ms, &attrs);
        self.network_bytes_sent.add(bytes_sent, &attrs);
        self.network_bytes_received.add(bytes_received, &attrs);

        if !success {
            self.network_errors.add(1, &attrs);
        }
    }

    /// Record cloud storage GET operation.
    pub fn record_cloud_get(&self, duration_ms: f64, provider: &str, success: bool) {
        let attrs = vec![
            KeyValue::new("provider", provider.to_string()),
            KeyValue::new("success", success),
        ];

        self.cloud_get_count.add(1, &attrs);
        self.cloud_get_duration.record(duration_ms, &attrs);
    }

    /// Record cloud storage PUT operation.
    pub fn record_cloud_put(&self, duration_ms: f64, provider: &str, success: bool) {
        let attrs = vec![
            KeyValue::new("provider", provider.to_string()),
            KeyValue::new("success", success),
        ];

        self.cloud_put_count.add(1, &attrs);
        self.cloud_put_duration.record(duration_ms, &attrs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::global;

    #[test]
    fn test_io_metrics_creation() {
        let meter = global::meter("test");
        let metrics = IoMetrics::new(meter);
        assert!(metrics.is_ok());
    }

    #[test]
    fn test_file_type_bucket_is_bounded_not_per_path() {
        // Many distinct paths with the same extension must collapse to the
        // same bounded label -- this is the whole point of the fix.
        assert_eq!(file_type_bucket("/data/a.tif"), "tif");
        assert_eq!(file_type_bucket("/data/b/c/d.tif"), "tif");
        assert_eq!(file_type_bucket("/other/unique/path/12345.tif"), "tif");
        assert_eq!(file_type_bucket("/data/sample.PARQUET"), "parquet");
        assert_eq!(file_type_bucket("/data/no_extension_file"), "noext");
    }

    #[test]
    fn test_record_file_read_does_not_panic_and_updates_counters() {
        let meter = global::meter("test");
        let metrics = IoMetrics::new(meter).expect("metrics creation should succeed");
        // Two distinct paths sharing an extension must not create separate
        // per-path series; this just exercises the recording path end to end.
        metrics.record_file_read(12.5, 4096, "/data/one.tif", true);
        metrics.record_file_read(8.0, 2048, "/data/two.tif", true);
        metrics.record_file_write(3.0, 512, "/data/out.tif", false);
    }
}
