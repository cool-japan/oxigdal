//! Vector operation metrics.

use crate::error::Result;
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram, Meter, UpDownCounter};

/// Metrics for vector operations.
pub struct VectorMetrics {
    // Read operations
    /// Counter for vector read operations.
    pub read_count: Counter<u64>,
    /// Histogram of vector read durations.
    pub read_duration: Histogram<f64>,
    /// Total features read.
    pub features_read: Counter<u64>,

    // Write operations
    /// Counter for vector write operations.
    pub write_count: Counter<u64>,
    /// Histogram of vector write durations.
    pub write_duration: Histogram<f64>,
    /// Total features written.
    pub features_written: Counter<u64>,

    // Geometry operations
    /// Counter for buffer operations.
    pub buffer_count: Counter<u64>,
    /// Histogram of buffer operation durations.
    pub buffer_duration: Histogram<f64>,
    /// Counter for intersection operations.
    pub intersection_count: Counter<u64>,
    /// Histogram of intersection operation durations.
    pub intersection_duration: Histogram<f64>,
    /// Counter for union operations.
    pub union_count: Counter<u64>,
    /// Histogram of union operation durations.
    pub union_duration: Histogram<f64>,
    /// Counter for simplify operations.
    pub simplify_count: Counter<u64>,
    /// Histogram of simplify operation durations.
    pub simplify_duration: Histogram<f64>,

    // Spatial operations
    /// Counter for spatial index build operations.
    pub spatial_index_build_count: Counter<u64>,
    /// Histogram of spatial index build durations.
    pub spatial_index_build_duration: Histogram<f64>,
    /// Counter for spatial query operations.
    pub spatial_query_count: Counter<u64>,
    /// Histogram of spatial query durations.
    pub spatial_query_duration: Histogram<f64>,
    /// Counter for spatial join operations.
    pub spatial_join_count: Counter<u64>,
    /// Histogram of spatial join durations.
    pub spatial_join_duration: Histogram<f64>,

    // Transform operations
    /// Counter for reprojection operations.
    pub reproject_count: Counter<u64>,
    /// Histogram of reprojection durations.
    pub reproject_duration: Histogram<f64>,
    /// Counter for transform operations.
    pub transform_count: Counter<u64>,
    /// Histogram of transform durations.
    pub transform_duration: Histogram<f64>,

    // Statistics
    /// Current number of active vector layers.
    pub active_layers: UpDownCounter<i64>,
    /// Histogram of feature counts per layer.
    pub feature_count: Histogram<f64>,
    /// Histogram of geometry complexity scores.
    pub geometry_complexity: Histogram<f64>,
    /// Histogram of layer sizes in bytes.
    pub layer_size_bytes: Histogram<f64>,
}

impl VectorMetrics {
    /// Create new vector metrics.
    pub fn new(meter: Meter) -> Result<Self> {
        Ok(Self {
            // Read operations
            read_count: meter
                .u64_counter("oxigdal.vector.read.count")
                .with_description("Number of vector read operations")
                .init(),
            read_duration: meter
                .f64_histogram("oxigdal.vector.read.duration")
                .with_description("Duration of vector read operations in milliseconds")
                .init(),
            features_read: meter
                .u64_counter("oxigdal.vector.features.read")
                .with_description("Number of features read")
                .init(),

            // Write operations
            write_count: meter
                .u64_counter("oxigdal.vector.write.count")
                .with_description("Number of vector write operations")
                .init(),
            write_duration: meter
                .f64_histogram("oxigdal.vector.write.duration")
                .with_description("Duration of vector write operations in milliseconds")
                .init(),
            features_written: meter
                .u64_counter("oxigdal.vector.features.written")
                .with_description("Number of features written")
                .init(),

            // Geometry operations
            buffer_count: meter
                .u64_counter("oxigdal.vector.buffer.count")
                .with_description("Number of buffer operations")
                .init(),
            buffer_duration: meter
                .f64_histogram("oxigdal.vector.buffer.duration")
                .with_description("Duration of buffer operations in milliseconds")
                .init(),
            intersection_count: meter
                .u64_counter("oxigdal.vector.intersection.count")
                .with_description("Number of intersection operations")
                .init(),
            intersection_duration: meter
                .f64_histogram("oxigdal.vector.intersection.duration")
                .with_description("Duration of intersection operations in milliseconds")
                .init(),
            union_count: meter
                .u64_counter("oxigdal.vector.union.count")
                .with_description("Number of union operations")
                .init(),
            union_duration: meter
                .f64_histogram("oxigdal.vector.union.duration")
                .with_description("Duration of union operations in milliseconds")
                .init(),
            simplify_count: meter
                .u64_counter("oxigdal.vector.simplify.count")
                .with_description("Number of simplify operations")
                .init(),
            simplify_duration: meter
                .f64_histogram("oxigdal.vector.simplify.duration")
                .with_description("Duration of simplify operations in milliseconds")
                .init(),

            // Spatial operations
            spatial_index_build_count: meter
                .u64_counter("oxigdal.vector.spatial_index.build.count")
                .with_description("Number of spatial index builds")
                .init(),
            spatial_index_build_duration: meter
                .f64_histogram("oxigdal.vector.spatial_index.build.duration")
                .with_description("Duration of spatial index build in milliseconds")
                .init(),
            spatial_query_count: meter
                .u64_counter("oxigdal.vector.spatial_query.count")
                .with_description("Number of spatial queries")
                .init(),
            spatial_query_duration: meter
                .f64_histogram("oxigdal.vector.spatial_query.duration")
                .with_description("Duration of spatial queries in milliseconds")
                .init(),
            spatial_join_count: meter
                .u64_counter("oxigdal.vector.spatial_join.count")
                .with_description("Number of spatial joins")
                .init(),
            spatial_join_duration: meter
                .f64_histogram("oxigdal.vector.spatial_join.duration")
                .with_description("Duration of spatial joins in milliseconds")
                .init(),

            // Transform operations
            reproject_count: meter
                .u64_counter("oxigdal.vector.reproject.count")
                .with_description("Number of vector reprojection operations")
                .init(),
            reproject_duration: meter
                .f64_histogram("oxigdal.vector.reproject.duration")
                .with_description("Duration of vector reprojection in milliseconds")
                .init(),
            transform_count: meter
                .u64_counter("oxigdal.vector.transform.count")
                .with_description("Number of vector transform operations")
                .init(),
            transform_duration: meter
                .f64_histogram("oxigdal.vector.transform.duration")
                .with_description("Duration of vector transform in milliseconds")
                .init(),

            // Statistics
            active_layers: meter
                .i64_up_down_counter("oxigdal.vector.active_layers")
                .with_description("Number of active vector layers")
                .init(),
            feature_count: meter
                .f64_histogram("oxigdal.vector.feature_count")
                .with_description("Number of features in layer")
                .init(),
            geometry_complexity: meter
                .f64_histogram("oxigdal.vector.geometry.complexity")
                .with_description("Geometry complexity (vertex count)")
                .init(),
            layer_size_bytes: meter
                .f64_histogram("oxigdal.vector.layer.size.bytes")
                .with_description("Layer size in bytes")
                .init(),
        })
    }

    /// Record vector read operation.
    pub fn record_read(&self, duration_ms: f64, features: u64, format: &str, success: bool) {
        let attrs = vec![
            KeyValue::new("format", format.to_string()),
            KeyValue::new("success", success),
        ];

        self.read_count.add(1, &attrs);
        self.read_duration.record(duration_ms, &attrs);
        if success {
            self.features_read.add(features, &attrs);
        }
    }

    /// Record vector write operation.
    pub fn record_write(&self, duration_ms: f64, features: u64, format: &str, success: bool) {
        let attrs = vec![
            KeyValue::new("format", format.to_string()),
            KeyValue::new("success", success),
        ];

        self.write_count.add(1, &attrs);
        self.write_duration.record(duration_ms, &attrs);
        if success {
            self.features_written.add(features, &attrs);
        }
    }

    /// Record buffer operation.
    pub fn record_buffer(&self, duration_ms: f64, distance: f64, success: bool) {
        let attrs = vec![
            KeyValue::new("distance", distance),
            KeyValue::new("success", success),
        ];

        self.buffer_count.add(1, &attrs);
        self.buffer_duration.record(duration_ms, &attrs);
    }

    /// Record spatial query operation.
    pub fn record_spatial_query(
        &self,
        duration_ms: f64,
        query_type: &str,
        _results: u64,
        success: bool,
    ) {
        let attrs = vec![
            KeyValue::new("query_type", query_type.to_string()),
            KeyValue::new("success", success),
        ];

        self.spatial_query_count.add(1, &attrs);
        self.spatial_query_duration.record(duration_ms, &attrs);
    }

    /// Record spatial join operation.
    pub fn record_spatial_join(
        &self,
        duration_ms: f64,
        left_features: u64,
        right_features: u64,
        result_features: u64,
        success: bool,
    ) {
        let attrs = vec![
            KeyValue::new("left_features", left_features as i64),
            KeyValue::new("right_features", right_features as i64),
            KeyValue::new("result_features", result_features as i64),
            KeyValue::new("success", success),
        ];

        self.spatial_join_count.add(1, &attrs);
        self.spatial_join_duration.record(duration_ms, &attrs);
    }

    /// Increment active layers.
    pub fn inc_active_layers(&self) {
        self.active_layers.add(1, &[]);
    }

    /// Decrement active layers.
    pub fn dec_active_layers(&self) {
        self.active_layers.add(-1, &[]);
    }

    /// Record feature count.
    pub fn record_feature_count(&self, count: u64, layer_type: &str) {
        let attrs = vec![KeyValue::new("layer_type", layer_type.to_string())];
        self.feature_count.record(count as f64, &attrs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::global;

    #[test]
    fn test_vector_metrics_creation() {
        let meter = global::meter("test");
        let metrics = VectorMetrics::new(meter);
        assert!(metrics.is_ok());
    }
}
