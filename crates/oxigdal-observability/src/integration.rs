//! Integration with external monitoring systems.

use crate::error::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Integration manager for external systems.
pub struct IntegrationManager {
    client: Client,
    grafana: Option<GrafanaIntegration>,
    prometheus: Option<PrometheusIntegration>,
    datadog: Option<DatadogIntegration>,
}

/// Grafana integration.
#[derive(Debug, Clone)]
pub struct GrafanaIntegration {
    url: String,
    api_key: String,
}

/// Prometheus integration.
#[derive(Debug, Clone)]
pub struct PrometheusIntegration {
    url: String,
}

/// Datadog integration.
#[derive(Debug, Clone)]
pub struct DatadogIntegration {
    api_key: String,
    #[allow(dead_code)]
    app_key: String,
}

impl IntegrationManager {
    /// Create a new integration manager.
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            grafana: None,
            prometheus: None,
            datadog: None,
        }
    }

    /// Configure Grafana integration.
    pub fn with_grafana(mut self, url: String, api_key: String) -> Self {
        self.grafana = Some(GrafanaIntegration { url, api_key });
        self
    }

    /// Configure Prometheus integration.
    pub fn with_prometheus(mut self, url: String) -> Self {
        self.prometheus = Some(PrometheusIntegration { url });
        self
    }

    /// Configure Datadog integration.
    pub fn with_datadog(mut self, api_key: String, app_key: String) -> Self {
        self.datadog = Some(DatadogIntegration { api_key, app_key });
        self
    }

    /// Import dashboard to Grafana.
    pub async fn import_grafana_dashboard(&self, dashboard_json: &str) -> Result<()> {
        let grafana = self.grafana.as_ref().ok_or_else(|| {
            crate::error::ObservabilityError::InvalidConfig("Grafana not configured".to_string())
        })?;

        let url = format!("{}/api/dashboards/db", grafana.url);
        let _response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", grafana.api_key))
            .header("Content-Type", "application/json")
            .body(dashboard_json.to_string())
            .send()
            .await?;

        Ok(())
    }

    /// Query Prometheus.
    pub async fn query_prometheus(&self, query: &str) -> Result<PrometheusResponse> {
        let prom = self.prometheus.as_ref().ok_or_else(|| {
            crate::error::ObservabilityError::InvalidConfig("Prometheus not configured".to_string())
        })?;

        let base_url = format!("{}/api/v1/query", prom.url);
        let mut parsed_url = url::Url::parse(&base_url)
            .map_err(|e| crate::error::ObservabilityError::InvalidConfig(e.to_string()))?;
        parsed_url.query_pairs_mut().append_pair("query", query);
        let response: reqwest::Response = self.client.get(parsed_url.as_str()).send().await?;

        let data: PrometheusResponse = response.json().await?;
        Ok(data)
    }

    /// Send metrics to Datadog.
    pub async fn send_datadog_metrics(&self, metrics: Vec<DatadogMetric>) -> Result<()> {
        let datadog = self.datadog.as_ref().ok_or_else(|| {
            crate::error::ObservabilityError::InvalidConfig("Datadog not configured".to_string())
        })?;

        let url = "https://api.datadoghq.com/api/v1/series";
        let _response = self
            .client
            .post(url)
            .header("DD-API-KEY", &datadog.api_key)
            .json(&serde_json::json!({ "series": metrics }))
            .send()
            .await?;

        Ok(())
    }
}

impl Default for IntegrationManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Prometheus query response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrometheusResponse {
    /// Status of the query (success or error).
    pub status: String,
    /// Data returned by the query.
    pub data: PrometheusData,
}

/// Prometheus data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrometheusData {
    /// Type of result (vector, matrix, scalar, string).
    #[serde(rename = "resultType")]
    pub result_type: String,
    /// Query results.
    pub result: Vec<PrometheusResult>,
}

/// Prometheus result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrometheusResult {
    /// Label set for this metric.
    pub metric: std::collections::HashMap<String, String>,
    /// Timestamp and value pair.
    pub value: (f64, String),
}

/// Datadog metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatadogMetric {
    /// Name of the metric.
    pub metric: String,
    /// Data points as timestamp-value pairs.
    pub points: Vec<(i64, f64)>,
    /// Tags associated with the metric.
    pub tags: Vec<String>,
    /// Type of metric (gauge, count, rate).
    #[serde(rename = "type")]
    pub metric_type: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integration_manager_creation() {
        let manager =
            IntegrationManager::new().with_prometheus("http://localhost:9090".to_string());

        assert!(manager.prometheus.is_some());
    }
}
