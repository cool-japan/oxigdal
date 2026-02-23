//! I/O performance metrics.

use crate::error::Result;
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram, Meter};

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
                .u64_counter("oxigdal.io.file.open.count")
                .with_description("Number of file open operations")
                .init(),
            file_open_duration: meter
                .f64_histogram("oxigdal.io.file.open.duration")
                .with_description("Duration of file open operations in milliseconds")
                .init(),
            file_close_count: meter
                .u64_counter("oxigdal.io.file.close.count")
                .with_description("Number of file close operations")
                .init(),
            file_read_count: meter
                .u64_counter("oxigdal.io.file.read.count")
                .with_description("Number of file read operations")
                .init(),
            file_read_duration: meter
                .f64_histogram("oxigdal.io.file.read.duration")
                .with_description("Duration of file read operations in milliseconds")
                .init(),
            file_read_bytes: meter
                .u64_counter("oxigdal.io.file.read.bytes")
                .with_description("Bytes read from files")
                .init(),
            file_write_count: meter
                .u64_counter("oxigdal.io.file.write.count")
                .with_description("Number of file write operations")
                .init(),
            file_write_duration: meter
                .f64_histogram("oxigdal.io.file.write.duration")
                .with_description("Duration of file write operations in milliseconds")
                .init(),
            file_write_bytes: meter
                .u64_counter("oxigdal.io.file.write.bytes")
                .with_description("Bytes written to files")
                .init(),

            // Network I/O
            network_request_count: meter
                .u64_counter("oxigdal.io.network.request.count")
                .with_description("Number of network requests")
                .init(),
            network_request_duration: meter
                .f64_histogram("oxigdal.io.network.request.duration")
                .with_description("Duration of network requests in milliseconds")
                .init(),
            network_bytes_sent: meter
                .u64_counter("oxigdal.io.network.bytes.sent")
                .with_description("Bytes sent over network")
                .init(),
            network_bytes_received: meter
                .u64_counter("oxigdal.io.network.bytes.received")
                .with_description("Bytes received from network")
                .init(),
            network_errors: meter
                .u64_counter("oxigdal.io.network.errors")
                .with_description("Number of network errors")
                .init(),

            // Cloud storage I/O
            cloud_get_count: meter
                .u64_counter("oxigdal.io.cloud.get.count")
                .with_description("Number of cloud storage GET operations")
                .init(),
            cloud_get_duration: meter
                .f64_histogram("oxigdal.io.cloud.get.duration")
                .with_description("Duration of cloud storage GET in milliseconds")
                .init(),
            cloud_put_count: meter
                .u64_counter("oxigdal.io.cloud.put.count")
                .with_description("Number of cloud storage PUT operations")
                .init(),
            cloud_put_duration: meter
                .f64_histogram("oxigdal.io.cloud.put.duration")
                .with_description("Duration of cloud storage PUT in milliseconds")
                .init(),
            cloud_list_count: meter
                .u64_counter("oxigdal.io.cloud.list.count")
                .with_description("Number of cloud storage LIST operations")
                .init(),
            cloud_list_duration: meter
                .f64_histogram("oxigdal.io.cloud.list.duration")
                .with_description("Duration of cloud storage LIST in milliseconds")
                .init(),

            // Throughput
            read_throughput_mbps: meter
                .f64_histogram("oxigdal.io.read.throughput.mbps")
                .with_description("Read throughput in MB/s")
                .init(),
            write_throughput_mbps: meter
                .f64_histogram("oxigdal.io.write.throughput.mbps")
                .with_description("Write throughput in MB/s")
                .init(),

            // Latency
            read_latency_ms: meter
                .f64_histogram("oxigdal.io.read.latency.ms")
                .with_description("Read latency in milliseconds")
                .init(),
            write_latency_ms: meter
                .f64_histogram("oxigdal.io.write.latency.ms")
                .with_description("Write latency in milliseconds")
                .init(),
        })
    }

    /// Record file read operation.
    pub fn record_file_read(&self, duration_ms: f64, bytes: u64, path: &str, success: bool) {
        let attrs = vec![
            KeyValue::new("path", path.to_string()),
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
    pub fn record_file_write(&self, duration_ms: f64, bytes: u64, path: &str, success: bool) {
        let attrs = vec![
            KeyValue::new("path", path.to_string()),
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
}
