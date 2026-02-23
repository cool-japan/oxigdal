//! GPU utilization metrics.

use crate::error::Result;
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram, Meter};

/// Metrics for GPU operations.
pub struct GpuMetrics {
    // GPU utilization
    /// Histogram of GPU utilization percentages.
    pub gpu_utilization_percent: Histogram<f64>,
    /// Histogram of GPU memory used in bytes.
    pub gpu_memory_used_bytes: Histogram<f64>,
    /// Histogram of total GPU memory in bytes.
    pub gpu_memory_total_bytes: Histogram<f64>,
    /// Histogram of GPU temperatures in Celsius.
    pub gpu_temperature_celsius: Histogram<f64>,

    // GPU operations
    /// Counter for GPU kernel executions.
    pub gpu_kernel_count: Counter<u64>,
    /// Histogram of GPU kernel execution durations.
    pub gpu_kernel_duration: Histogram<f64>,
    /// Counter for GPU memory transfer operations.
    pub gpu_memory_transfer_count: Counter<u64>,
    /// Histogram of GPU memory transfer durations.
    pub gpu_memory_transfer_duration: Histogram<f64>,
    /// Total bytes transferred to/from GPU.
    pub gpu_memory_transfer_bytes: Counter<u64>,

    // GPU processing
    /// Counter for GPU raster processing operations.
    pub gpu_raster_processing_count: Counter<u64>,
    /// Histogram of GPU raster processing durations.
    pub gpu_raster_processing_duration: Histogram<f64>,
    /// Counter for GPU ML inference operations.
    pub gpu_ml_inference_count: Counter<u64>,
    /// Histogram of GPU ML inference durations.
    pub gpu_ml_inference_duration: Histogram<f64>,

    // GPU errors
    /// Counter for GPU errors.
    pub gpu_errors: Counter<u64>,
    /// Counter for GPU out-of-memory errors.
    pub gpu_out_of_memory: Counter<u64>,
}

impl GpuMetrics {
    /// Create new GPU metrics.
    pub fn new(meter: Meter) -> Result<Self> {
        Ok(Self {
            // GPU utilization
            gpu_utilization_percent: meter
                .f64_histogram("oxigdal.gpu.utilization.percent")
                .with_description("GPU utilization percentage")
                .init(),
            gpu_memory_used_bytes: meter
                .f64_histogram("oxigdal.gpu.memory.used.bytes")
                .with_description("GPU memory used in bytes")
                .init(),
            gpu_memory_total_bytes: meter
                .f64_histogram("oxigdal.gpu.memory.total.bytes")
                .with_description("Total GPU memory in bytes")
                .init(),
            gpu_temperature_celsius: meter
                .f64_histogram("oxigdal.gpu.temperature.celsius")
                .with_description("GPU temperature in Celsius")
                .init(),

            // GPU operations
            gpu_kernel_count: meter
                .u64_counter("oxigdal.gpu.kernel.count")
                .with_description("Number of GPU kernels executed")
                .init(),
            gpu_kernel_duration: meter
                .f64_histogram("oxigdal.gpu.kernel.duration")
                .with_description("GPU kernel execution duration in milliseconds")
                .init(),
            gpu_memory_transfer_count: meter
                .u64_counter("oxigdal.gpu.memory_transfer.count")
                .with_description("Number of GPU memory transfers")
                .init(),
            gpu_memory_transfer_duration: meter
                .f64_histogram("oxigdal.gpu.memory_transfer.duration")
                .with_description("GPU memory transfer duration in milliseconds")
                .init(),
            gpu_memory_transfer_bytes: meter
                .u64_counter("oxigdal.gpu.memory_transfer.bytes")
                .with_description("Bytes transferred to/from GPU")
                .init(),

            // GPU processing
            gpu_raster_processing_count: meter
                .u64_counter("oxigdal.gpu.raster_processing.count")
                .with_description("Number of GPU raster processing operations")
                .init(),
            gpu_raster_processing_duration: meter
                .f64_histogram("oxigdal.gpu.raster_processing.duration")
                .with_description("GPU raster processing duration in milliseconds")
                .init(),
            gpu_ml_inference_count: meter
                .u64_counter("oxigdal.gpu.ml_inference.count")
                .with_description("Number of GPU ML inference operations")
                .init(),
            gpu_ml_inference_duration: meter
                .f64_histogram("oxigdal.gpu.ml_inference.duration")
                .with_description("GPU ML inference duration in milliseconds")
                .init(),

            // GPU errors
            gpu_errors: meter
                .u64_counter("oxigdal.gpu.errors")
                .with_description("Number of GPU errors")
                .init(),
            gpu_out_of_memory: meter
                .u64_counter("oxigdal.gpu.out_of_memory")
                .with_description("Number of GPU out-of-memory errors")
                .init(),
        })
    }

    /// Record GPU utilization.
    pub fn record_utilization(
        &self,
        percent: f64,
        memory_used: u64,
        memory_total: u64,
        gpu_id: u32,
    ) {
        let attrs = vec![KeyValue::new("gpu_id", gpu_id as i64)];

        self.gpu_utilization_percent.record(percent, &attrs);
        self.gpu_memory_used_bytes
            .record(memory_used as f64, &attrs);
        self.gpu_memory_total_bytes
            .record(memory_total as f64, &attrs);
    }

    /// Record GPU kernel execution.
    pub fn record_kernel(&self, duration_ms: f64, kernel_name: &str, success: bool) {
        let attrs = vec![
            KeyValue::new("kernel_name", kernel_name.to_string()),
            KeyValue::new("success", success),
        ];

        self.gpu_kernel_count.add(1, &attrs);
        self.gpu_kernel_duration.record(duration_ms, &attrs);

        if !success {
            self.gpu_errors.add(1, &attrs);
        }
    }

    /// Record GPU memory transfer.
    pub fn record_memory_transfer(
        &self,
        duration_ms: f64,
        bytes: u64,
        direction: &str,
        success: bool,
    ) {
        let attrs = vec![
            KeyValue::new("direction", direction.to_string()),
            KeyValue::new("success", success),
        ];

        self.gpu_memory_transfer_count.add(1, &attrs);
        self.gpu_memory_transfer_duration
            .record(duration_ms, &attrs);
        if success {
            self.gpu_memory_transfer_bytes.add(bytes, &attrs);
        }
    }

    /// Record GPU out-of-memory error.
    pub fn record_out_of_memory(&self, requested_bytes: u64) {
        let attrs = vec![KeyValue::new("requested_bytes", requested_bytes as i64)];
        self.gpu_out_of_memory.add(1, &attrs);
        self.gpu_errors.add(1, &attrs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::global;

    #[test]
    fn test_gpu_metrics_creation() {
        let meter = global::meter("test");
        let metrics = GpuMetrics::new(meter);
        assert!(metrics.is_ok());
    }
}
