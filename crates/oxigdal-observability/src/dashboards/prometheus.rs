//! Prometheus recording rules for OxiGDAL metrics.

use crate::error::Result;
use serde::{Deserialize, Serialize};

/// Prometheus recording rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingRule {
    /// Rule name.
    pub record: String,

    /// PromQL expression.
    pub expr: String,

    /// Labels to add.
    pub labels: Option<std::collections::HashMap<String, String>>,
}

/// Recording rule group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleGroup {
    /// Group name.
    pub name: String,

    /// Evaluation interval.
    pub interval: String,

    /// Rules in the group.
    pub rules: Vec<RecordingRule>,
}

/// Create OxiGDAL recording rules.
pub fn create_oxigdal_rules() -> Vec<RuleGroup> {
    vec![
        // Raster operation rules
        RuleGroup {
            name: "oxigdal_raster".to_string(),
            interval: "30s".to_string(),
            rules: vec![
                RecordingRule {
                    record: "oxigdal:raster:read_rate:5m".to_string(),
                    expr: "rate(oxigdal_raster_read_count[5m])".to_string(),
                    labels: None,
                },
                RecordingRule {
                    record: "oxigdal:raster:write_rate:5m".to_string(),
                    expr: "rate(oxigdal_raster_write_count[5m])".to_string(),
                    labels: None,
                },
                RecordingRule {
                    record: "oxigdal:raster:read_throughput_mbps:5m".to_string(),
                    expr: "rate(oxigdal_raster_read_bytes[5m]) / 1024 / 1024".to_string(),
                    labels: None,
                },
            ],
        },
        // Cache rules
        RuleGroup {
            name: "oxigdal_cache".to_string(),
            interval: "30s".to_string(),
            rules: vec![
                RecordingRule {
                    record: "oxigdal:cache:hit_ratio:5m".to_string(),
                    expr: "rate(oxigdal_cache_hits[5m]) / (rate(oxigdal_cache_hits[5m]) + rate(oxigdal_cache_misses[5m]))".to_string(),
                    labels: None,
                },
                RecordingRule {
                    record: "oxigdal:cache:eviction_rate:5m".to_string(),
                    expr: "rate(oxigdal_cache_evictions[5m])".to_string(),
                    labels: None,
                },
            ],
        },
        // Query rules
        RuleGroup {
            name: "oxigdal_query".to_string(),
            interval: "30s".to_string(),
            rules: vec![
                RecordingRule {
                    record: "oxigdal:query:duration:p95".to_string(),
                    expr: "histogram_quantile(0.95, rate(oxigdal_query_duration_bucket[5m]))".to_string(),
                    labels: None,
                },
                RecordingRule {
                    record: "oxigdal:query:duration:p99".to_string(),
                    expr: "histogram_quantile(0.99, rate(oxigdal_query_duration_bucket[5m]))".to_string(),
                    labels: None,
                },
                RecordingRule {
                    record: "oxigdal:query:error_rate:5m".to_string(),
                    expr: "rate(oxigdal_query_errors[5m]) / rate(oxigdal_query_count[5m])".to_string(),
                    labels: None,
                },
            ],
        },
    ]
}

/// Export recording rules as Prometheus YAML.
pub fn export_prometheus_yaml(groups: &[RuleGroup]) -> Result<String> {
    serde_json::to_string_pretty(&serde_json::json!({
        "groups": groups
    }))
    .map_err(crate::error::ObservabilityError::Serialization)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_rules() {
        let rules = create_oxigdal_rules();
        assert!(!rules.is_empty());
    }

    #[test]
    fn test_export_yaml() {
        let rules = create_oxigdal_rules();
        let yaml = export_prometheus_yaml(&rules);
        assert!(yaml.is_ok());
    }
}
