//! Prometheus recording rules for OxiGeo metrics.

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

/// Create OxiGeo recording rules.
pub fn create_oxigeo_rules() -> Vec<RuleGroup> {
    vec![
        // Raster operation rules
        RuleGroup {
            name: "oxigeo_raster".to_string(),
            interval: "30s".to_string(),
            rules: vec![
                RecordingRule {
                    record: "oxigeo:raster:read_rate:5m".to_string(),
                    expr: "rate(oxigeo_raster_read_count[5m])".to_string(),
                    labels: None,
                },
                RecordingRule {
                    record: "oxigeo:raster:write_rate:5m".to_string(),
                    expr: "rate(oxigeo_raster_write_count[5m])".to_string(),
                    labels: None,
                },
                RecordingRule {
                    record: "oxigeo:raster:read_throughput_mbps:5m".to_string(),
                    expr: "rate(oxigeo_raster_read_bytes[5m]) / 1024 / 1024".to_string(),
                    labels: None,
                },
            ],
        },
        // Cache rules
        RuleGroup {
            name: "oxigeo_cache".to_string(),
            interval: "30s".to_string(),
            rules: vec![
                RecordingRule {
                    record: "oxigeo:cache:hit_ratio:5m".to_string(),
                    expr: "rate(oxigeo_cache_hits[5m]) / (rate(oxigeo_cache_hits[5m]) + rate(oxigeo_cache_misses[5m]))".to_string(),
                    labels: None,
                },
                RecordingRule {
                    record: "oxigeo:cache:eviction_rate:5m".to_string(),
                    expr: "rate(oxigeo_cache_evictions[5m])".to_string(),
                    labels: None,
                },
            ],
        },
        // Query rules
        RuleGroup {
            name: "oxigeo_query".to_string(),
            interval: "30s".to_string(),
            rules: vec![
                RecordingRule {
                    record: "oxigeo:query:duration:p95".to_string(),
                    expr: "histogram_quantile(0.95, rate(oxigeo_query_duration_bucket[5m]))".to_string(),
                    labels: None,
                },
                RecordingRule {
                    record: "oxigeo:query:duration:p99".to_string(),
                    expr: "histogram_quantile(0.99, rate(oxigeo_query_duration_bucket[5m]))".to_string(),
                    labels: None,
                },
                RecordingRule {
                    record: "oxigeo:query:error_rate:5m".to_string(),
                    expr: "rate(oxigeo_query_errors[5m]) / rate(oxigeo_query_count[5m])".to_string(),
                    labels: None,
                },
            ],
        },
    ]
}

/// Double-quote and escape a scalar for embedding in block-style YAML.
///
/// Always double-quoting (rather than emitting an unquoted "plain" scalar)
/// sidesteps YAML's plain-scalar ambiguity rules -- PromQL expressions
/// routinely contain `:`, `[`, `]`, `{`, `}`, `#`, which are all YAML
/// indicator characters that are unsafe in an unquoted plain scalar
/// depending on position. A double-quoted scalar is unambiguous as long as
/// backslashes/quotes/control characters are escaped, which this does.
fn yaml_double_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Export recording rules as conventional block-style Prometheus recording
/// rules YAML (the format Prometheus/`promtool` and Alertmanager tooling
/// expects for a `.rules.yml` file), suitable for hand-editing or diffing.
///
/// This previously serialized to minified JSON despite the name and doc
/// comment promising YAML output; it now emits real YAML directly (no
/// third-party YAML crate dependency needed for this bounded, well-known
/// schema).
pub fn export_prometheus_yaml(groups: &[RuleGroup]) -> Result<String> {
    let mut out = String::new();
    out.push_str("groups:\n");

    for group in groups {
        out.push_str(&format!("  - name: {}\n", yaml_double_quote(&group.name)));
        out.push_str(&format!(
            "    interval: {}\n",
            yaml_double_quote(&group.interval)
        ));
        out.push_str("    rules:\n");

        for rule in &group.rules {
            out.push_str(&format!(
                "      - record: {}\n",
                yaml_double_quote(&rule.record)
            ));
            out.push_str(&format!(
                "        expr: {}\n",
                yaml_double_quote(&rule.expr)
            ));

            if let Some(labels) = &rule.labels
                && !labels.is_empty()
            {
                out.push_str("        labels:\n");
                // Sort keys for deterministic, diffable output (the
                // underlying HashMap has no stable iteration order).
                let mut keys: Vec<&String> = labels.keys().collect();
                keys.sort();
                for key in keys {
                    let value = &labels[key];
                    out.push_str(&format!(
                        "          {}: {}\n",
                        yaml_double_quote(key),
                        yaml_double_quote(value)
                    ));
                }
            }
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_rules() {
        let rules = create_oxigeo_rules();
        assert!(!rules.is_empty());
    }

    #[test]
    fn test_export_yaml() {
        let rules = create_oxigeo_rules();
        let yaml = export_prometheus_yaml(&rules);
        assert!(yaml.is_ok());
    }

    #[test]
    fn test_export_yaml_is_real_yaml_not_json() {
        let rules = create_oxigeo_rules();
        let text = export_prometheus_yaml(&rules).expect("export should succeed");

        // Must NOT look like JSON: no braces/brackets-as-structure at all.
        assert!(
            !text.trim_start().starts_with('{'),
            "output looks like JSON: {text}"
        );
        assert!(
            !text.contains("\":"),
            "output looks like quoted-JSON keys: {text}"
        );

        // Must look like conventional block-style Prometheus rules YAML.
        assert!(text.starts_with("groups:\n"));
        assert!(text.contains("  - name: \"oxigeo_raster\"\n"));
        assert!(text.contains("    interval: \"30s\"\n"));
        assert!(text.contains("    rules:\n"));
        assert!(text.contains("      - record:"));
        assert!(text.contains("        expr:"));
    }

    #[test]
    fn test_export_yaml_escapes_and_orders_labels_deterministically() {
        let groups = vec![RuleGroup {
            name: "g".to_string(),
            interval: "1m".to_string(),
            rules: vec![RecordingRule {
                record: "r".to_string(),
                expr: "rate(x[5m]) / y{a=\"b\"}".to_string(),
                labels: Some(
                    [
                        ("zeta".to_string(), "1".to_string()),
                        ("alpha".to_string(), "2".to_string()),
                    ]
                    .into_iter()
                    .collect(),
                ),
            }],
        }];

        let text = export_prometheus_yaml(&groups).expect("export should succeed");
        // The PromQL expr embeds a literal double quote and braces; the
        // escaped form must still round-trip as one YAML scalar.
        assert!(text.contains("expr: \"rate(x[5m]) / y{a=\\\"b\\\"}\""));

        // Labels sorted alphabetically (alpha before zeta) regardless of
        // HashMap iteration order.
        let alpha_pos = text.find("alpha").expect("alpha present");
        let zeta_pos = text.find("zeta").expect("zeta present");
        assert!(alpha_pos < zeta_pos, "labels should be sorted: {text}");
    }
}
