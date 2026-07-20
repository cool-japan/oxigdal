//! Google Cloud Monitoring integration.
//!
//! Talks to the real Cloud Monitoring v3 REST API
//! (<https://monitoring.googleapis.com/v3>) and the Uptime Check API, which
//! shares the same base URL and API version. Authentication is obtained by
//! delegating to [`super::workload_identity::WorkloadIdentityClient`], which
//! already implements the GCE metadata server / IAM Credentials token flow.

use crate::error::{CloudEnhancedError, Result};
use crate::gcp::workload_identity::WorkloadIdentityClient;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Default base URL of the Cloud Monitoring API.
const DEFAULT_MONITORING_BASE_URL: &str = "https://monitoring.googleapis.com";

/// OAuth2 scope requested for calls to the Cloud Monitoring API.
const MONITORING_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

/// Cloud Monitoring client.
#[derive(Debug, Clone)]
pub struct MonitoringClient {
    project_id: String,
    /// Base URL of the Cloud Monitoring API (overridable for tests).
    monitoring_base_url: String,
    http_client: reqwest::Client,
    /// Auth provider, reusing the GCE metadata / IAM Credentials token flow.
    identity: WorkloadIdentityClient,
}

impl MonitoringClient {
    /// Returns the project ID.
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
}

impl MonitoringClient {
    /// Creates a new Monitoring client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be created.
    pub async fn new(config: &super::GcpConfig) -> Result<Self> {
        Self::with_urls(config, DEFAULT_MONITORING_BASE_URL, None::<String>)
    }

    /// Creates a new Monitoring client pointed at custom Monitoring API and
    /// (optionally) GCE metadata server base URLs.
    ///
    /// This is primarily intended for tests, which spin up local mock
    /// servers rather than talking to the real `monitoring.googleapis.com`
    /// and `metadata.google.internal` endpoints.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn with_urls(
        config: &super::GcpConfig,
        monitoring_base_url: impl Into<String>,
        metadata_base_url: Option<impl Into<String>>,
    ) -> Result<Self> {
        let http_client = reqwest::Client::builder().build().map_err(|e| {
            CloudEnhancedError::configuration(format!("Failed to build HTTP client: {e}"))
        })?;

        let identity = match metadata_base_url {
            Some(url) => WorkloadIdentityClient::with_metadata_base_url(config, url)?,
            None => WorkloadIdentityClient::new(config)?,
        };

        Ok(Self {
            project_id: config.project_id().to_string(),
            monitoring_base_url: monitoring_base_url.into(),
            http_client,
            identity,
        })
    }

    /// Obtains a bearer token for authenticating to the Cloud Monitoring
    /// API, using the instance's attached service account.
    async fn bearer_token(&self) -> Result<String> {
        let token = self
            .identity
            .generate_access_token("default", vec![MONITORING_SCOPE.to_string()], 3600)
            .await?;
        Ok(token.access_token)
    }

    /// Qualifies a possibly-short resource ID (e.g. `"policy-123"`) into a
    /// fully-qualified Cloud Monitoring resource name (e.g.
    /// `"projects/my-project/alertPolicies/policy-123"`), leaving
    /// already-qualified names (starting with `"projects/"`) untouched.
    fn qualify(&self, id: &str, collection: &str) -> String {
        if id.starts_with("projects/") {
            id.to_string()
        } else {
            format!("projects/{}/{collection}/{id}", self.project_id)
        }
    }

    /// Writes a time series (metric) to Cloud Monitoring.
    ///
    /// # Errors
    ///
    /// Returns an error if the write fails.
    pub async fn write_time_series(
        &self,
        metric_type: &str,
        value: f64,
        labels: HashMap<String, String>,
    ) -> Result<()> {
        tracing::info!(
            "Writing time series: {} = {} ({} labels)",
            metric_type,
            value,
            labels.len()
        );

        let token = self.bearer_token().await?;
        let url = format!(
            "{}/v3/projects/{}/timeSeries",
            self.monitoring_base_url, self.project_id
        );

        let now = Utc::now();
        let body = CreateTimeSeriesRequest {
            time_series: vec![TimeSeriesWire {
                metric: MetricWire {
                    metric_type: metric_type.to_string(),
                    labels,
                },
                resource: Some(MonitoredResourceWire {
                    resource_type: "global".to_string(),
                    labels: HashMap::from([("project_id".to_string(), self.project_id.clone())]),
                }),
                points: vec![PointWire {
                    interval: TimeIntervalWire {
                        end_time: now.to_rfc3339(),
                    },
                    value: TypedValueWire {
                        double_value: value,
                    },
                }],
            }],
        };

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!("timeSeries.create request failed: {e}"))
            })?;

        ensure_success(response, "write time series").await?;
        Ok(())
    }

    /// Lists time series.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub async fn list_time_series(
        &self,
        filter: &str,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<Vec<TimeSeriesData>> {
        tracing::info!(
            "Listing time series with filter: {} ({} to {})",
            filter,
            start_time,
            end_time
        );

        let token = self.bearer_token().await?;
        let url = format!(
            "{}/v3/projects/{}/timeSeries",
            self.monitoring_base_url, self.project_id
        );

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .query(&[
                ("filter", filter.to_string()),
                ("interval.startTime", start_time.to_rfc3339()),
                ("interval.endTime", end_time.to_rfc3339()),
                ("view", "FULL".to_string()),
            ])
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!("timeSeries.list request failed: {e}"))
            })?;

        let body: ListTimeSeriesResponse =
            parse_monitoring_response(response, "list time series").await?;
        Ok(body.time_series.into_iter().map(Into::into).collect())
    }

    /// Creates an alert policy.
    ///
    /// # Errors
    ///
    /// Returns an error if the policy cannot be created.
    pub async fn create_alert_policy(
        &self,
        display_name: &str,
        conditions: Vec<AlertCondition>,
        notification_channels: Vec<String>,
    ) -> Result<String> {
        tracing::info!(
            "Creating alert policy: {} ({} conditions, {} channels)",
            display_name,
            conditions.len(),
            notification_channels.len()
        );

        let token = self.bearer_token().await?;
        let url = format!(
            "{}/v3/projects/{}/alertPolicies",
            self.monitoring_base_url, self.project_id
        );

        let body = AlertPolicyWire {
            display_name: display_name.to_string(),
            combiner: "OR".to_string(),
            conditions: conditions.into_iter().map(Into::into).collect(),
            notification_channels: notification_channels
                .into_iter()
                .map(|c| self.qualify(&c, "notificationChannels"))
                .collect(),
        };

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!("alertPolicies.create request failed: {e}"))
            })?;

        let created: AlertPolicyResource =
            parse_monitoring_response(response, "create alert policy").await?;
        Ok(created.name)
    }

    /// Deletes an alert policy.
    ///
    /// # Errors
    ///
    /// Returns an error if the policy cannot be deleted.
    pub async fn delete_alert_policy(&self, policy_id: &str) -> Result<()> {
        tracing::info!("Deleting alert policy: {}", policy_id);

        let token = self.bearer_token().await?;
        let name = self.qualify(policy_id, "alertPolicies");
        let url = format!("{}/v3/{name}", self.monitoring_base_url);

        let response = self
            .http_client
            .delete(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!("alertPolicies.delete request failed: {e}"))
            })?;

        ensure_success(response, "delete alert policy").await?;
        Ok(())
    }

    /// Lists alert policies.
    ///
    /// # Errors
    ///
    /// Returns an error if the policies cannot be listed.
    pub async fn list_alert_policies(&self) -> Result<Vec<String>> {
        tracing::info!("Listing alert policies");

        let token = self.bearer_token().await?;
        let url = format!(
            "{}/v3/projects/{}/alertPolicies",
            self.monitoring_base_url, self.project_id
        );

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!("alertPolicies.list request failed: {e}"))
            })?;

        let body: ListAlertPoliciesResponse =
            parse_monitoring_response(response, "list alert policies").await?;
        Ok(body.alert_policies.into_iter().map(|p| p.name).collect())
    }

    /// Creates a notification channel.
    ///
    /// # Errors
    ///
    /// Returns an error if the channel cannot be created.
    pub async fn create_notification_channel(
        &self,
        display_name: &str,
        channel_type: &str,
        labels: HashMap<String, String>,
    ) -> Result<String> {
        tracing::info!(
            "Creating notification channel: {} (type: {}, {} labels)",
            display_name,
            channel_type,
            labels.len()
        );

        let token = self.bearer_token().await?;
        let url = format!(
            "{}/v3/projects/{}/notificationChannels",
            self.monitoring_base_url, self.project_id
        );

        let body = NotificationChannelWire {
            channel_type: channel_type.to_string(),
            display_name: display_name.to_string(),
            labels,
        };

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!(
                    "notificationChannels.create request failed: {e}"
                ))
            })?;

        let created: NotificationChannelResource =
            parse_monitoring_response(response, "create notification channel").await?;
        Ok(created.name)
    }

    /// Deletes a notification channel.
    ///
    /// # Errors
    ///
    /// Returns an error if the channel cannot be deleted.
    pub async fn delete_notification_channel(&self, channel_id: &str) -> Result<()> {
        tracing::info!("Deleting notification channel: {}", channel_id);

        let token = self.bearer_token().await?;
        let name = self.qualify(channel_id, "notificationChannels");
        let url = format!("{}/v3/{name}", self.monitoring_base_url);

        let response = self
            .http_client
            .delete(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!(
                    "notificationChannels.delete request failed: {e}"
                ))
            })?;

        ensure_success(response, "delete notification channel").await?;
        Ok(())
    }

    /// Creates an uptime check.
    ///
    /// # Errors
    ///
    /// Returns an error if the check cannot be created.
    pub async fn create_uptime_check(
        &self,
        display_name: &str,
        resource_type: &str,
        host: &str,
        path: &str,
    ) -> Result<String> {
        tracing::info!(
            "Creating uptime check: {} (type: {}, host: {}, path: {})",
            display_name,
            resource_type,
            host,
            path
        );

        let token = self.bearer_token().await?;
        let url = format!(
            "{}/v3/projects/{}/uptimeCheckConfigs",
            self.monitoring_base_url, self.project_id
        );

        let body = UptimeCheckConfigWire {
            display_name: display_name.to_string(),
            monitored_resource: MonitoredResourceWire {
                resource_type: resource_type.to_string(),
                labels: HashMap::from([
                    ("host".to_string(), host.to_string()),
                    ("project_id".to_string(), self.project_id.clone()),
                ]),
            },
            http_check: HttpCheckWire {
                path: path.to_string(),
            },
            period: "60s".to_string(),
            timeout: "10s".to_string(),
        };

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!(
                    "uptimeCheckConfigs.create request failed: {e}"
                ))
            })?;

        let created: UptimeCheckConfigResource =
            parse_monitoring_response(response, "create uptime check").await?;
        Ok(created.name)
    }

    /// Deletes an uptime check.
    ///
    /// # Errors
    ///
    /// Returns an error if the check cannot be deleted.
    pub async fn delete_uptime_check(&self, check_id: &str) -> Result<()> {
        tracing::info!("Deleting uptime check: {}", check_id);

        let token = self.bearer_token().await?;
        let name = self.qualify(check_id, "uptimeCheckConfigs");
        let url = format!("{}/v3/{name}", self.monitoring_base_url);

        let response = self
            .http_client
            .delete(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!(
                    "uptimeCheckConfigs.delete request failed: {e}"
                ))
            })?;

        ensure_success(response, "delete uptime check").await?;
        Ok(())
    }
}

/// Verifies that `response` carries a success status, mapping non-success
/// statuses to a descriptive [`CloudEnhancedError::monitoring`].
async fn ensure_success(response: reqwest::Response, action: &str) -> Result<reqwest::Response> {
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "<unreadable response body>".to_string());
    Err(CloudEnhancedError::monitoring(format!(
        "Cloud Monitoring API returned status {status} while trying to {action}: {body}"
    )))
}

/// Verifies `response` is a success and deserializes its JSON body as `T`.
async fn parse_monitoring_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
    action: &str,
) -> Result<T> {
    let response = ensure_success(response, action).await?;
    response.json::<T>().await.map_err(|e| {
        CloudEnhancedError::monitoring(format!(
            "Failed to parse Cloud Monitoring API response while trying to {action}: {e}"
        ))
    })
}

// ---------------------------------------------------------------------
// Wire (JSON) types for the Cloud Monitoring v3 REST API.
// ---------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct CreateTimeSeriesRequest {
    #[serde(rename = "timeSeries")]
    time_series: Vec<TimeSeriesWire>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TimeSeriesWire {
    metric: MetricWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resource: Option<MonitoredResourceWire>,
    #[serde(default)]
    points: Vec<PointWire>,
}

impl From<TimeSeriesWire> for TimeSeriesData {
    fn from(wire: TimeSeriesWire) -> Self {
        Self {
            metric: MetricDescriptor {
                metric_type: wire.metric.metric_type,
                labels: wire.metric.labels,
            },
            points: wire.points.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct MetricWire {
    #[serde(rename = "type")]
    metric_type: String,
    #[serde(default)]
    labels: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct MonitoredResourceWire {
    #[serde(rename = "type")]
    resource_type: String,
    #[serde(default)]
    labels: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PointWire {
    interval: TimeIntervalWire,
    value: TypedValueWire,
}

impl From<PointWire> for Point {
    fn from(wire: PointWire) -> Self {
        let timestamp = DateTime::parse_from_rfc3339(&wire.interval.end_time)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        Self {
            timestamp,
            value: wire.value.double_value,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct TimeIntervalWire {
    #[serde(rename = "endTime")]
    end_time: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct TypedValueWire {
    #[serde(rename = "doubleValue", default)]
    double_value: f64,
}

#[derive(Debug, Deserialize)]
struct ListTimeSeriesResponse {
    #[serde(rename = "timeSeries", default)]
    time_series: Vec<TimeSeriesWire>,
}

#[derive(Debug, Serialize)]
struct AlertPolicyWire {
    #[serde(rename = "displayName")]
    display_name: String,
    combiner: String,
    conditions: Vec<AlertConditionWire>,
    #[serde(rename = "notificationChannels")]
    notification_channels: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AlertConditionWire {
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "conditionThreshold")]
    condition_threshold: ConditionThresholdWire,
}

impl From<AlertCondition> for AlertConditionWire {
    fn from(condition: AlertCondition) -> Self {
        Self {
            display_name: condition.display_name,
            condition_threshold: ConditionThresholdWire {
                filter: condition.filter,
                comparison: condition.comparison.as_api_str().to_string(),
                threshold_value: condition.threshold_value,
                duration: format!("{}s", condition.duration_seconds.max(0)),
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct ConditionThresholdWire {
    filter: String,
    comparison: String,
    #[serde(rename = "thresholdValue")]
    threshold_value: f64,
    duration: String,
}

#[derive(Debug, Deserialize)]
struct AlertPolicyResource {
    name: String,
}

#[derive(Debug, Deserialize)]
struct ListAlertPoliciesResponse {
    #[serde(rename = "alertPolicies", default)]
    alert_policies: Vec<AlertPolicyResource>,
}

#[derive(Debug, Serialize)]
struct NotificationChannelWire {
    #[serde(rename = "type")]
    channel_type: String,
    #[serde(rename = "displayName")]
    display_name: String,
    labels: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct NotificationChannelResource {
    name: String,
}

#[derive(Debug, Serialize)]
struct UptimeCheckConfigWire {
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "monitoredResource")]
    monitored_resource: MonitoredResourceWire,
    #[serde(rename = "httpCheck")]
    http_check: HttpCheckWire,
    period: String,
    timeout: String,
}

#[derive(Debug, Serialize)]
struct HttpCheckWire {
    path: String,
}

#[derive(Debug, Deserialize)]
struct UptimeCheckConfigResource {
    name: String,
}

/// Time series data point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeriesData {
    /// Metric
    pub metric: MetricDescriptor,
    /// Points
    pub points: Vec<Point>,
}

/// Metric descriptor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricDescriptor {
    /// Metric type
    pub metric_type: String,
    /// Labels
    pub labels: HashMap<String, String>,
}

/// Data point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Point {
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Value
    pub value: f64,
}

/// Alert condition.
#[derive(Debug, Clone)]
pub struct AlertCondition {
    /// Display name
    pub display_name: String,
    /// Filter
    pub filter: String,
    /// Comparison
    pub comparison: Comparison,
    /// Threshold value
    pub threshold_value: f64,
    /// Duration
    pub duration_seconds: i64,
}

/// Comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    /// Greater than
    GreaterThan,
    /// Less than
    LessThan,
    /// Greater than or equal
    GreaterThanOrEqual,
    /// Less than or equal
    LessThanOrEqual,
}

impl Comparison {
    /// Returns the Cloud Monitoring API's string representation of this
    /// comparison operator.
    fn as_api_str(self) -> &'static str {
        match self {
            Self::GreaterThan => "COMPARISON_GT",
            Self::LessThan => "COMPARISON_LT",
            Self::GreaterThanOrEqual => "COMPARISON_GE",
            Self::LessThanOrEqual => "COMPARISON_LE",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Spawns a minimal HTTP/1.1 mock server on an ephemeral local port that
    /// replies to every accepted connection with `body`.
    async fn spawn_mock_server(status_line: &str, content_type: &str, body: String) -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("mock server local addr");

        let response = format!(
            "{status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );

        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let response = response.clone();
                tokio::spawn(async move {
                    let mut buf = [0_u8; 4096];
                    let _ = socket.read(&mut buf).await;
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.flush().await;
                });
            }
        });

        format!("http://{addr}")
    }

    async fn test_client() -> MonitoringClient {
        let metadata_base = spawn_mock_server(
            "HTTP/1.1 200 OK",
            "application/json",
            r#"{"access_token":"mock-token","expires_in":3600}"#.to_string(),
        )
        .await;
        let monitoring_body = r#"{"name":"projects/test-project/alertPolicies/policy-abc"}"#;
        let monitoring_base = spawn_mock_server(
            "HTTP/1.1 200 OK",
            "application/json",
            monitoring_body.to_string(),
        )
        .await;

        let config =
            crate::gcp::GcpConfig::new("test-project".to_string(), None).expect("gcp config");
        MonitoringClient::with_urls(&config, monitoring_base, Some(metadata_base))
            .expect("monitoring client")
    }

    #[tokio::test]
    async fn test_create_alert_policy_returns_real_name() {
        let client = test_client().await;

        let policy_id = client
            .create_alert_policy(
                "High error rate",
                vec![AlertCondition {
                    display_name: "error rate".to_string(),
                    filter: "metric.type=\"custom.googleapis.com/errors\"".to_string(),
                    comparison: Comparison::GreaterThan,
                    threshold_value: 10.0,
                    duration_seconds: 60,
                }],
                vec![],
            )
            .await
            .expect("alert policy");

        assert_eq!(policy_id, "projects/test-project/alertPolicies/policy-abc");
    }

    #[tokio::test]
    async fn test_write_time_series_succeeds() {
        let client = test_client().await;
        let result = client
            .write_time_series("custom.googleapis.com/my_metric", 42.0, HashMap::new())
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_qualify_leaves_fully_qualified_names_untouched() {
        let client = test_client().await;
        assert_eq!(
            client.qualify("projects/other/alertPolicies/x", "alertPolicies"),
            "projects/other/alertPolicies/x"
        );
        assert_eq!(
            client.qualify("policy-123", "alertPolicies"),
            "projects/test-project/alertPolicies/policy-123"
        );
    }

    #[test]
    fn test_comparison() {
        assert_eq!(Comparison::GreaterThan, Comparison::GreaterThan);
        assert_ne!(Comparison::GreaterThan, Comparison::LessThan);
    }

    #[test]
    fn test_comparison_as_api_str() {
        assert_eq!(Comparison::GreaterThan.as_api_str(), "COMPARISON_GT");
        assert_eq!(Comparison::LessThan.as_api_str(), "COMPARISON_LT");
        assert_eq!(Comparison::GreaterThanOrEqual.as_api_str(), "COMPARISON_GE");
        assert_eq!(Comparison::LessThanOrEqual.as_api_str(), "COMPARISON_LE");
    }
}
