//! Custom geospatial metrics for OxiGDAL operations.

pub mod cache;
pub mod cluster;
pub mod gpu;
pub mod io;
pub mod query;
pub mod raster;
pub mod vector;

use crate::error::Result;
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram, Meter, UpDownCounter};
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Instant;

/// Metric collector for all geospatial operations.
pub struct GeoMetrics {
    #[allow(dead_code)]
    meter: Meter,

    // Raster metrics
    /// Raster-related metrics.
    pub raster: raster::RasterMetrics,

    // Vector metrics
    /// Vector-related metrics.
    pub vector: vector::VectorMetrics,

    // I/O metrics
    /// I/O operation metrics.
    pub io: io::IoMetrics,

    // Cache metrics
    /// Cache operation metrics.
    pub cache: cache::CacheMetrics,

    // Query metrics
    /// Query execution metrics.
    pub query: query::QueryMetrics,

    // GPU metrics
    /// GPU operation metrics.
    pub gpu: gpu::GpuMetrics,

    // Cluster metrics
    /// Cluster operation metrics.
    pub cluster: cluster::ClusterMetrics,
}

impl GeoMetrics {
    /// Create a new geo metrics collector.
    pub fn new(meter: Meter) -> Result<Self> {
        Ok(Self {
            raster: raster::RasterMetrics::new(meter.clone())?,
            vector: vector::VectorMetrics::new(meter.clone())?,
            io: io::IoMetrics::new(meter.clone())?,
            cache: cache::CacheMetrics::new(meter.clone())?,
            query: query::QueryMetrics::new(meter.clone())?,
            gpu: gpu::GpuMetrics::new(meter.clone())?,
            cluster: cluster::ClusterMetrics::new(meter.clone())?,
            meter,
        })
    }
}

/// Timer for measuring operation duration.
pub struct Timer {
    /// Start instant of the timer.
    start: Instant,
    /// Histogram to record the duration.
    histogram: Histogram<f64>,
    /// Attributes to attach to the measurement.
    attributes: Vec<KeyValue>,
}

impl Timer {
    /// Create a new timer.
    pub fn new(histogram: Histogram<f64>, attributes: Vec<KeyValue>) -> Self {
        Self {
            start: Instant::now(),
            histogram,
            attributes,
        }
    }

    /// Stop the timer and record the duration.
    pub fn stop(self) {
        let duration = self.start.elapsed();
        self.histogram.record(
            duration.as_secs_f64() * 1000.0, // Convert to milliseconds
            &self.attributes,
        );
    }
}

/// Counter wrapper for convenience.
pub struct MetricCounter {
    /// Underlying counter metric.
    counter: Counter<u64>,
}

impl MetricCounter {
    /// Create a new metric counter.
    pub fn new(counter: Counter<u64>) -> Self {
        Self { counter }
    }

    /// Increment the counter.
    pub fn inc(&self, attributes: &[KeyValue]) {
        self.counter.add(1, attributes);
    }

    /// Add a value to the counter.
    pub fn add(&self, value: u64, attributes: &[KeyValue]) {
        self.counter.add(value, attributes);
    }
}

/// Gauge wrapper for convenience.
pub struct MetricGauge {
    /// Current gauge value.
    value: Arc<RwLock<f64>>,
}

impl MetricGauge {
    /// Create a new metric gauge.
    pub fn new() -> Self {
        Self {
            value: Arc::new(RwLock::new(0.0)),
        }
    }

    /// Set the gauge value.
    pub fn set(&self, value: f64) {
        *self.value.write() = value;
    }

    /// Get the gauge value.
    pub fn get(&self) -> f64 {
        *self.value.read()
    }

    /// Increment the gauge.
    pub fn inc(&self) {
        *self.value.write() += 1.0;
    }

    /// Decrement the gauge.
    pub fn dec(&self) {
        *self.value.write() -= 1.0;
    }
}

impl Default for MetricGauge {
    fn default() -> Self {
        Self::new()
    }
}

/// Up-down counter wrapper for convenience.
pub struct MetricUpDownCounter {
    /// Underlying up-down counter metric.
    counter: UpDownCounter<i64>,
}

impl MetricUpDownCounter {
    /// Create a new up-down counter.
    pub fn new(counter: UpDownCounter<i64>) -> Self {
        Self { counter }
    }

    /// Increment the counter.
    pub fn inc(&self, attributes: &[KeyValue]) {
        self.counter.add(1, attributes);
    }

    /// Decrement the counter.
    pub fn dec(&self, attributes: &[KeyValue]) {
        self.counter.add(-1, attributes);
    }

    /// Add a value to the counter.
    pub fn add(&self, value: i64, attributes: &[KeyValue]) {
        self.counter.add(value, attributes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_gauge() {
        let gauge = MetricGauge::new();
        assert_eq!(gauge.get(), 0.0);

        gauge.set(10.0);
        assert_eq!(gauge.get(), 10.0);

        gauge.inc();
        assert_eq!(gauge.get(), 11.0);

        gauge.dec();
        assert_eq!(gauge.get(), 10.0);
    }
}
