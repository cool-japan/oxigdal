//! Query performance metrics.

use crate::error::Result;
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram, Meter};

/// Metrics for query operations.
pub struct QueryMetrics {
    // Query execution
    /// Counter for total queries executed.
    pub query_count: Counter<u64>,
    /// Histogram of query durations in milliseconds.
    pub query_duration: Histogram<f64>,
    /// Counter for query errors.
    pub query_errors: Counter<u64>,

    // Query types
    /// Counter for spatial queries.
    pub spatial_query_count: Counter<u64>,
    /// Counter for attribute queries.
    pub attribute_query_count: Counter<u64>,
    /// Counter for temporal queries.
    pub temporal_query_count: Counter<u64>,
    /// Counter for SQL queries.
    pub sql_query_count: Counter<u64>,

    // Query complexity
    /// Histogram of query complexity scores.
    pub query_complexity_score: Histogram<f64>,
    /// Histogram of query result counts.
    pub query_result_count: Histogram<f64>,
    /// Histogram of query result sizes in bytes.
    pub query_result_bytes: Histogram<f64>,

    // Query optimization
    /// Histogram of query planning durations.
    pub query_plan_duration: Histogram<f64>,
    /// Histogram of query execution durations.
    pub query_execution_duration: Histogram<f64>,
    /// Counter for queries that used indexes.
    pub index_usage_count: Counter<u64>,
    /// Counter for full table scans.
    pub full_scan_count: Counter<u64>,

    // Query cache
    /// Counter for query cache hits.
    pub query_cache_hits: Counter<u64>,
    /// Counter for query cache misses.
    pub query_cache_misses: Counter<u64>,
}

impl QueryMetrics {
    /// Create new query metrics.
    pub fn new(meter: Meter) -> Result<Self> {
        Ok(Self {
            // Query execution
            query_count: meter
                .u64_counter("oxigdal.query.count")
                .with_description("Number of queries executed")
                .build(),
            query_duration: meter
                .f64_histogram("oxigdal.query.duration")
                .with_description("Query duration in milliseconds")
                .build(),
            query_errors: meter
                .u64_counter("oxigdal.query.errors")
                .with_description("Number of query errors")
                .build(),

            // Query types
            spatial_query_count: meter
                .u64_counter("oxigdal.query.spatial.count")
                .with_description("Number of spatial queries")
                .build(),
            attribute_query_count: meter
                .u64_counter("oxigdal.query.attribute.count")
                .with_description("Number of attribute queries")
                .build(),
            temporal_query_count: meter
                .u64_counter("oxigdal.query.temporal.count")
                .with_description("Number of temporal queries")
                .build(),
            sql_query_count: meter
                .u64_counter("oxigdal.query.sql.count")
                .with_description("Number of SQL queries")
                .build(),

            // Query complexity
            query_complexity_score: meter
                .f64_histogram("oxigdal.query.complexity")
                .with_description("Query complexity score")
                .build(),
            query_result_count: meter
                .f64_histogram("oxigdal.query.result.count")
                .with_description("Number of results returned")
                .build(),
            query_result_bytes: meter
                .f64_histogram("oxigdal.query.result.bytes")
                .with_description("Size of query results in bytes")
                .build(),

            // Query optimization
            query_plan_duration: meter
                .f64_histogram("oxigdal.query.plan.duration")
                .with_description("Query planning duration in milliseconds")
                .build(),
            query_execution_duration: meter
                .f64_histogram("oxigdal.query.execution.duration")
                .with_description("Query execution duration in milliseconds")
                .build(),
            index_usage_count: meter
                .u64_counter("oxigdal.query.index_usage")
                .with_description("Number of times indexes were used")
                .build(),
            full_scan_count: meter
                .u64_counter("oxigdal.query.full_scan")
                .with_description("Number of full table scans")
                .build(),

            // Query cache
            query_cache_hits: meter
                .u64_counter("oxigdal.query.cache.hits")
                .with_description("Number of query cache hits")
                .build(),
            query_cache_misses: meter
                .u64_counter("oxigdal.query.cache.misses")
                .with_description("Number of query cache misses")
                .build(),
        })
    }

    /// Record query execution.
    pub fn record_query(
        &self,
        duration_ms: f64,
        query_type: &str,
        result_count: u64,
        success: bool,
    ) {
        let attrs = vec![
            KeyValue::new("query_type", query_type.to_string()),
            KeyValue::new("success", success),
        ];

        self.query_count.add(1, &attrs);
        self.query_duration.record(duration_ms, &attrs);

        if success {
            self.query_result_count.record(result_count as f64, &attrs);
        } else {
            self.query_errors.add(1, &attrs);
        }
    }

    /// Record spatial query.
    pub fn record_spatial_query(&self, duration_ms: f64, predicate: &str, success: bool) {
        let attrs = vec![
            KeyValue::new("predicate", predicate.to_string()),
            KeyValue::new("success", success),
        ];

        self.spatial_query_count.add(1, &attrs);
        self.query_duration.record(duration_ms, &attrs);
    }

    /// Record query planning.
    pub fn record_query_plan(&self, duration_ms: f64, used_index: bool) {
        let attrs = vec![KeyValue::new("used_index", used_index)];

        self.query_plan_duration.record(duration_ms, &attrs);

        if used_index {
            self.index_usage_count.add(1, &attrs);
        } else {
            self.full_scan_count.add(1, &attrs);
        }
    }

    /// Record query cache hit.
    pub fn record_cache_hit(&self, query_type: &str) {
        let attrs = vec![KeyValue::new("query_type", query_type.to_string())];
        self.query_cache_hits.add(1, &attrs);
    }

    /// Record query cache miss.
    pub fn record_cache_miss(&self, query_type: &str) {
        let attrs = vec![KeyValue::new("query_type", query_type.to_string())];
        self.query_cache_misses.add(1, &attrs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::global;

    #[test]
    fn test_query_metrics_creation() {
        let meter = global::meter("test");
        let metrics = QueryMetrics::new(meter);
        assert!(metrics.is_ok());
    }
}
