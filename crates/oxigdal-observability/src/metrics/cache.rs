//! Cache performance metrics.

use crate::error::Result;
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram, Meter, UpDownCounter};

/// Metrics for cache operations.
pub struct CacheMetrics {
    // Hit/miss statistics
    /// Counter for cache hits.
    pub cache_hits: Counter<u64>,
    /// Counter for cache misses.
    pub cache_misses: Counter<u64>,
    /// Histogram of cache hit ratios.
    pub cache_hit_ratio: Histogram<f64>,

    // Cache operations
    /// Counter for cache get operations.
    pub cache_get_count: Counter<u64>,
    /// Histogram of cache get operation durations.
    pub cache_get_duration: Histogram<f64>,
    /// Counter for cache put operations.
    pub cache_put_count: Counter<u64>,
    /// Histogram of cache put operation durations.
    pub cache_put_duration: Histogram<f64>,
    /// Counter for cache evictions.
    pub cache_evictions: Counter<u64>,
    /// Counter for cache invalidations.
    pub cache_invalidations: Counter<u64>,

    // Cache size
    /// Current cache size in bytes.
    pub cache_size_bytes: UpDownCounter<i64>,
    /// Current number of cache entries.
    pub cache_entries: UpDownCounter<i64>,
    /// Histogram of maximum cache sizes.
    pub cache_max_size_bytes: Histogram<f64>,

    // Cache efficiency
    /// Bytes saved by cache hits.
    pub cache_bytes_saved: Counter<u64>,
    /// Time saved by cache hits in milliseconds.
    pub cache_time_saved_ms: Histogram<f64>,

    // Per-layer cache statistics
    /// Counter for layer-specific cache hits.
    pub layer_cache_hits: Counter<u64>,
    /// Counter for layer-specific cache misses.
    pub layer_cache_misses: Counter<u64>,

    // Prefetch statistics
    /// Counter for prefetch operations.
    pub prefetch_count: Counter<u64>,
    /// Counter for successful prefetch hits.
    pub prefetch_hit_count: Counter<u64>,
    /// Counter for prefetched items never used.
    pub prefetch_waste_count: Counter<u64>,
}

impl CacheMetrics {
    /// Create new cache metrics.
    pub fn new(meter: Meter) -> Result<Self> {
        Ok(Self {
            // Hit/miss statistics
            cache_hits: meter
                .u64_counter("oxigdal.cache.hits")
                .with_description("Number of cache hits")
                .build(),
            cache_misses: meter
                .u64_counter("oxigdal.cache.misses")
                .with_description("Number of cache misses")
                .build(),
            cache_hit_ratio: meter
                .f64_histogram("oxigdal.cache.hit_ratio")
                .with_description("Cache hit ratio (0.0 to 1.0)")
                .build(),

            // Cache operations
            cache_get_count: meter
                .u64_counter("oxigdal.cache.get.count")
                .with_description("Number of cache get operations")
                .build(),
            cache_get_duration: meter
                .f64_histogram("oxigdal.cache.get.duration")
                .with_description("Duration of cache get operations in milliseconds")
                .build(),
            cache_put_count: meter
                .u64_counter("oxigdal.cache.put.count")
                .with_description("Number of cache put operations")
                .build(),
            cache_put_duration: meter
                .f64_histogram("oxigdal.cache.put.duration")
                .with_description("Duration of cache put operations in milliseconds")
                .build(),
            cache_evictions: meter
                .u64_counter("oxigdal.cache.evictions")
                .with_description("Number of cache evictions")
                .build(),
            cache_invalidations: meter
                .u64_counter("oxigdal.cache.invalidations")
                .with_description("Number of cache invalidations")
                .build(),

            // Cache size
            cache_size_bytes: meter
                .i64_up_down_counter("oxigdal.cache.size.bytes")
                .with_description("Current cache size in bytes")
                .build(),
            cache_entries: meter
                .i64_up_down_counter("oxigdal.cache.entries")
                .with_description("Number of entries in cache")
                .build(),
            cache_max_size_bytes: meter
                .f64_histogram("oxigdal.cache.max_size.bytes")
                .with_description("Maximum cache size in bytes")
                .build(),

            // Cache efficiency
            cache_bytes_saved: meter
                .u64_counter("oxigdal.cache.bytes_saved")
                .with_description("Bytes saved by cache hits")
                .build(),
            cache_time_saved_ms: meter
                .f64_histogram("oxigdal.cache.time_saved.ms")
                .with_description("Time saved by cache hits in milliseconds")
                .build(),

            // Per-layer cache statistics
            layer_cache_hits: meter
                .u64_counter("oxigdal.cache.layer.hits")
                .with_description("Number of layer cache hits")
                .build(),
            layer_cache_misses: meter
                .u64_counter("oxigdal.cache.layer.misses")
                .with_description("Number of layer cache misses")
                .build(),

            // Prefetch statistics
            prefetch_count: meter
                .u64_counter("oxigdal.cache.prefetch.count")
                .with_description("Number of prefetch operations")
                .build(),
            prefetch_hit_count: meter
                .u64_counter("oxigdal.cache.prefetch.hits")
                .with_description("Number of successful prefetch hits")
                .build(),
            prefetch_waste_count: meter
                .u64_counter("oxigdal.cache.prefetch.waste")
                .with_description("Number of prefetched items never used")
                .build(),
        })
    }

    /// Record cache hit.
    pub fn record_hit(&self, cache_type: &str, bytes: u64) {
        let attrs = vec![KeyValue::new("cache_type", cache_type.to_string())];
        self.cache_hits.add(1, &attrs);
        self.cache_bytes_saved.add(bytes, &attrs);
    }

    /// Record cache miss.
    pub fn record_miss(&self, cache_type: &str) {
        let attrs = vec![KeyValue::new("cache_type", cache_type.to_string())];
        self.cache_misses.add(1, &attrs);
    }

    /// Record cache get operation.
    pub fn record_get(&self, duration_ms: f64, hit: bool, cache_type: &str) {
        let attrs = vec![
            KeyValue::new("cache_type", cache_type.to_string()),
            KeyValue::new("hit", hit),
        ];

        self.cache_get_count.add(1, &attrs);
        self.cache_get_duration.record(duration_ms, &attrs);

        if hit {
            self.cache_time_saved_ms.record(duration_ms, &attrs);
        }
    }

    /// Record cache put operation.
    pub fn record_put(&self, duration_ms: f64, bytes: u64, cache_type: &str) {
        let attrs = vec![KeyValue::new("cache_type", cache_type.to_string())];

        self.cache_put_count.add(1, &attrs);
        self.cache_put_duration.record(duration_ms, &attrs);
        self.cache_size_bytes.add(bytes as i64, &attrs);
        self.cache_entries.add(1, &attrs);
    }

    /// Record cache eviction.
    pub fn record_eviction(&self, bytes: u64, cache_type: &str, reason: &str) {
        let attrs = vec![
            KeyValue::new("cache_type", cache_type.to_string()),
            KeyValue::new("reason", reason.to_string()),
        ];

        self.cache_evictions.add(1, &attrs);
        self.cache_size_bytes.add(-(bytes as i64), &attrs);
        self.cache_entries.add(-1, &attrs);
    }

    /// Calculate and record hit ratio.
    pub fn record_hit_ratio(&self, hits: u64, total: u64, cache_type: &str) {
        if total > 0 {
            let ratio = hits as f64 / total as f64;
            let attrs = vec![KeyValue::new("cache_type", cache_type.to_string())];
            self.cache_hit_ratio.record(ratio, &attrs);
        }
    }

    /// Record prefetch operation.
    pub fn record_prefetch(&self, count: u64, cache_type: &str) {
        let attrs = vec![KeyValue::new("cache_type", cache_type.to_string())];
        self.prefetch_count.add(count, &attrs);
    }

    /// Record prefetch hit.
    pub fn record_prefetch_hit(&self, cache_type: &str) {
        let attrs = vec![KeyValue::new("cache_type", cache_type.to_string())];
        self.prefetch_hit_count.add(1, &attrs);
    }

    /// Record prefetch waste (prefetched but never used).
    pub fn record_prefetch_waste(&self, count: u64, cache_type: &str) {
        let attrs = vec![KeyValue::new("cache_type", cache_type.to_string())];
        self.prefetch_waste_count.add(count, &attrs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::global;

    #[test]
    fn test_cache_metrics_creation() {
        let meter = global::meter("test");
        let metrics = CacheMetrics::new(meter);
        assert!(metrics.is_ok());
    }

    #[test]
    fn test_hit_ratio_calculation() {
        let meter = global::meter("test");
        let metrics = CacheMetrics::new(meter).expect("Failed to create metrics");

        // Record some hits and misses
        metrics.record_hit("test", 1024);
        metrics.record_miss("test");
        metrics.record_hit("test", 2048);

        // Calculate hit ratio: 2 hits, 3 total = 0.666...
        metrics.record_hit_ratio(2, 3, "test");
    }
}
