//! Azure Monitor integration.
//!
//! Talks to the real Azure Monitor REST surface on the Azure Resource Manager
//! control plane (`management.azure.com`) -- the Metrics API, metric-alert /
//! action-group / diagnostic-setting management, and the Activity Log -- plus
//! the Log Analytics query API (`api.loganalytics.io`), authenticated with
//! this crate's `azure_core::credentials::TokenCredential` (see
//! [`super::AzureConfig`]).
//!
//! `send_metric` (custom-metric ingestion, which uses a per-region
//! data-plane endpoint) and `send_diagnostic_log` (which has no single
//! ingestion REST API) return [`CloudEnhancedError::NotImplemented`] rather
//! than a fabricated success.

use crate::error::{CloudEnhancedError, Result};
use azure_core::credentials::TokenCredential;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Default base URL of the Azure Resource Manager control plane.
const DEFAULT_ARM_BASE_URL: &str = "https://management.azure.com";

/// Default base URL of the Log Analytics query API.
const DEFAULT_LOG_ANALYTICS_BASE_URL: &str = "https://api.loganalytics.io";

const METRICS_API_VERSION: &str = "2019-07-01";
const METRIC_ALERTS_API_VERSION: &str = "2018-03-01";
const ACTION_GROUPS_API_VERSION: &str = "2023-01-01";
const ACTIVITY_LOG_API_VERSION: &str = "2015-04-01";
const DIAGNOSTIC_SETTINGS_API_VERSION: &str = "2021-05-01-preview";

/// Azure Monitor client.
#[derive(Debug, Clone)]
pub struct MonitorClient {
    subscription_id: String,
    arm_base_url: String,
    log_analytics_base_url: String,
    credential: Arc<dyn TokenCredential>,
    http_client: reqwest::Client,
}

impl MonitorClient {
    /// Returns the subscription ID.
    pub fn subscription_id(&self) -> &str {
        &self.subscription_id
    }
}

impl MonitorClient {
    /// Creates a new Monitor client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be created.
    pub fn new(config: &super::AzureConfig) -> Result<Self> {
        Self::with_base_urls(config, DEFAULT_ARM_BASE_URL, DEFAULT_LOG_ANALYTICS_BASE_URL)
    }

    /// Creates a new Monitor client pointed at custom ARM and Log Analytics
    /// base URLs (primarily for tests, which spin up local mock servers).
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn with_base_urls(
        config: &super::AzureConfig,
        arm_base_url: impl Into<String>,
        log_analytics_base_url: impl Into<String>,
    ) -> Result<Self> {
        let http_client = reqwest::Client::builder().build().map_err(|e| {
            CloudEnhancedError::configuration(format!("Failed to build HTTP client: {e}"))
        })?;

        Ok(Self {
            subscription_id: config.subscription_id().to_string(),
            arm_base_url: arm_base_url.into(),
            log_analytics_base_url: log_analytics_base_url.into(),
            credential: config.credential.clone(),
            http_client,
        })
    }

    /// Obtains a bearer token for the given OAuth2 scope.
    async fn token_for(&self, scope: &str) -> Result<String> {
        let token = self
            .credential
            .get_token(&[scope], None)
            .await
            .map_err(|e| {
                CloudEnhancedError::authentication(format!(
                    "Failed to acquire token for {scope}: {e}"
                ))
            })?;
        Ok(token.token.secret().to_string())
    }

    async fn arm_token(&self) -> Result<String> {
        self.token_for("https://management.azure.com/.default")
            .await
    }

    /// Sends a custom metric to Azure Monitor.
    ///
    /// # Errors
    ///
    /// Not implemented: custom-metric ingestion targets a per-region
    /// data-plane endpoint (`https://{region}.monitoring.azure.com`) that is
    /// not derivable from the resource id alone; returns
    /// [`CloudEnhancedError::NotImplemented`] rather than a fabricated
    /// success.
    pub async fn send_metric(
        &self,
        resource_id: &str,
        metric_namespace: &str,
        metric_name: &str,
        value: f64,
        dimensions: HashMap<String, String>,
    ) -> Result<()> {
        tracing::info!(
            "send_metric requested but not implemented: {}/{} = {} (resource: {}, {} dimensions)",
            metric_namespace,
            metric_name,
            value,
            resource_id,
            dimensions.len()
        );

        Err(CloudEnhancedError::not_implemented(
            "MonitorClient::send_metric requires the per-region custom-metrics ingestion endpoint, which is not yet wired up",
        ))
    }

    /// Queries metrics from Azure Monitor via the Metrics REST API.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub async fn query_metrics(
        &self,
        resource_id: &str,
        metric_names: Vec<String>,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        aggregation: MetricAggregation,
    ) -> Result<Vec<MetricData>> {
        tracing::info!(
            "Querying metrics: {:?} for resource: {} ({} to {})",
            metric_names,
            resource_id,
            start_time,
            end_time
        );

        let timespan = format!("{}/{}", start_time.to_rfc3339(), end_time.to_rfc3339());
        let url = format!(
            "{}/{}/providers/microsoft.insights/metrics?api-version={METRICS_API_VERSION}&metricnames={}&timespan={}&aggregation={}",
            self.arm_base_url,
            resource_id.trim_start_matches('/'),
            urlencode(&metric_names.join(",")),
            urlencode(&timespan),
            aggregation.as_str(),
        );

        let token = self.arm_token().await?;
        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!("Azure Monitor metrics request failed: {e}"))
            })?;

        let body: MetricsResponseWire = parse_arm_response(response, "query metrics").await?;
        Ok(body.value.into_iter().map(MetricWire::into_data).collect())
    }

    /// Runs a Kusto query against a Log Analytics workspace.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub async fn query_logs(
        &self,
        workspace_id: &str,
        query: &str,
        timespan: Option<&str>,
    ) -> Result<LogQueryResult> {
        tracing::info!(
            "Querying logs in workspace: {} with timespan: {:?}",
            workspace_id,
            timespan
        );
        tracing::debug!("Query: {}", query);

        let url = format!(
            "{}/v1/workspaces/{}/query",
            self.log_analytics_base_url, workspace_id
        );
        let mut body = serde_json::json!({ "query": query });
        if let Some(ts) = timespan {
            body["timespan"] = serde_json::Value::String(ts.to_string());
        }

        let token = self
            .token_for("https://api.loganalytics.io/.default")
            .await?;
        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!("Log Analytics query request failed: {e}"))
            })?;

        let status = response.status();
        if !status.is_success() {
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable response body>".to_string());
            return Err(CloudEnhancedError::monitoring(format!(
                "Log Analytics query returned status {status}: {text}"
            )));
        }

        let body: LogQueryResponseWire = response.json().await.map_err(|e| {
            CloudEnhancedError::monitoring(format!("Failed to parse Log Analytics response: {e}"))
        })?;
        Ok(LogQueryResult {
            tables: body
                .tables
                .into_iter()
                .map(LogTableWire::into_table)
                .collect(),
        })
    }

    fn metric_alert_path(&self, resource_group: &str, alert_name: &str) -> String {
        format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/microsoft.insights/metricAlerts/{}?api-version={METRIC_ALERTS_API_VERSION}",
            self.arm_base_url, self.subscription_id, resource_group, alert_name
        )
    }

    /// Creates a metric alert rule via the ARM `metricAlerts` API.
    ///
    /// # Errors
    ///
    /// Returns an error if the alert cannot be created.
    pub async fn create_metric_alert(
        &self,
        resource_group: &str,
        alert_name: &str,
        config: MetricAlertConfig,
    ) -> Result<()> {
        tracing::info!(
            "Creating metric alert: {} in resource group: {}",
            alert_name,
            resource_group
        );

        let url = self.metric_alert_path(resource_group, alert_name);
        let body = serde_json::json!({
            "location": "global",
            "properties": {
                "severity": 3,
                "enabled": true,
                "scopes": [config.target_resource_id],
                "evaluationFrequency": "PT1M",
                "windowSize": "PT5M",
                "criteria": {
                    "odata.type": "Microsoft.Azure.Monitor.SingleResourceMultipleMetricCriteria",
                    "allOf": [{
                        "name": "metric1",
                        "criterionType": "StaticThresholdCriterion",
                        "metricName": config.metric_name,
                        "metricNamespace": config.metric_namespace,
                        "operator": config.operator,
                        "threshold": config.threshold,
                        "timeAggregation": config.time_aggregation.as_str(),
                    }]
                },
                "actions": config.action_group_ids.iter()
                    .map(|id| serde_json::json!({ "actionGroupId": id }))
                    .collect::<Vec<_>>(),
            }
        });

        let token = self.arm_token().await?;
        let response = self
            .http_client
            .put(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!("metricAlerts PUT request failed: {e}"))
            })?;
        ensure_arm_success(response, "create metric alert").await?;
        Ok(())
    }

    /// Deletes a metric alert rule via the ARM `metricAlerts` API.
    ///
    /// # Errors
    ///
    /// Returns an error if the alert cannot be deleted.
    pub async fn delete_metric_alert(&self, resource_group: &str, alert_name: &str) -> Result<()> {
        tracing::info!(
            "Deleting metric alert: {} from resource group: {}",
            alert_name,
            resource_group
        );

        let url = self.metric_alert_path(resource_group, alert_name);
        let token = self.arm_token().await?;
        let response = self
            .http_client
            .delete(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!("metricAlerts DELETE request failed: {e}"))
            })?;
        ensure_arm_success(response, "delete metric alert").await?;
        Ok(())
    }

    /// Lists metric alert rule names in a resource group.
    ///
    /// # Errors
    ///
    /// Returns an error if the alerts cannot be listed.
    pub async fn list_alerts(&self, resource_group: &str) -> Result<Vec<String>> {
        tracing::info!("Listing alerts in resource group: {}", resource_group);

        let url = format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/microsoft.insights/metricAlerts?api-version={METRIC_ALERTS_API_VERSION}",
            self.arm_base_url, self.subscription_id, resource_group
        );
        let token = self.arm_token().await?;
        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!("metricAlerts LIST request failed: {e}"))
            })?;

        let body: NamedListWire = parse_arm_response(response, "list alerts").await?;
        Ok(body.value.into_iter().filter_map(|v| v.name).collect())
    }

    /// Creates an action group via the ARM `actionGroups` API.
    ///
    /// # Errors
    ///
    /// Returns an error if the action group cannot be created.
    pub async fn create_action_group(
        &self,
        resource_group: &str,
        action_group_name: &str,
        receivers: Vec<ActionReceiver>,
    ) -> Result<()> {
        tracing::info!(
            "Creating action group: {} in resource group: {} with {} receivers",
            action_group_name,
            resource_group,
            receivers.len()
        );

        let mut email_receivers = Vec::new();
        let mut sms_receivers = Vec::new();
        let mut webhook_receivers = Vec::new();
        for receiver in &receivers {
            match receiver.receiver_type {
                ReceiverType::Email => {
                    if let Some(email) = &receiver.email_address {
                        email_receivers.push(serde_json::json!({
                            "name": receiver.name,
                            "emailAddress": email,
                        }));
                    }
                }
                ReceiverType::Sms => {
                    if let Some(phone) = &receiver.phone_number {
                        sms_receivers.push(serde_json::json!({
                            "name": receiver.name,
                            "countryCode": "1",
                            "phoneNumber": phone,
                        }));
                    }
                }
                ReceiverType::Webhook | ReceiverType::AzureFunction | ReceiverType::LogicApp => {
                    if let Some(webhook) = &receiver.webhook_url {
                        webhook_receivers.push(serde_json::json!({
                            "name": receiver.name,
                            "serviceUri": webhook,
                        }));
                    }
                }
            }
        }

        // groupShortName is limited to 12 characters by the API.
        let short_name: String = action_group_name.chars().take(12).collect();
        let url = format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/microsoft.insights/actionGroups/{}?api-version={ACTION_GROUPS_API_VERSION}",
            self.arm_base_url, self.subscription_id, resource_group, action_group_name
        );
        let body = serde_json::json!({
            "location": "Global",
            "properties": {
                "groupShortName": short_name,
                "enabled": true,
                "emailReceivers": email_receivers,
                "smsReceivers": sms_receivers,
                "webhookReceivers": webhook_receivers,
            }
        });

        let token = self.arm_token().await?;
        let response = self
            .http_client
            .put(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!("actionGroups PUT request failed: {e}"))
            })?;
        ensure_arm_success(response, "create action group").await?;
        Ok(())
    }

    /// Sends a diagnostic log.
    ///
    /// # Errors
    ///
    /// Not implemented: Azure has no single "send diagnostic log" REST API
    /// (diagnostic logs are emitted by resources and routed via diagnostic
    /// settings); returns [`CloudEnhancedError::NotImplemented`] rather than a
    /// fabricated success.
    pub async fn send_diagnostic_log(
        &self,
        resource_id: &str,
        category: &str,
        _log_data: &str,
    ) -> Result<()> {
        tracing::info!(
            "send_diagnostic_log requested but not implemented for resource: {} category: {}",
            resource_id,
            category
        );

        Err(CloudEnhancedError::not_implemented(
            "MonitorClient::send_diagnostic_log has no corresponding Azure REST ingestion API",
        ))
    }

    /// Gets activity log events via the ARM Activity Log API.
    ///
    /// # Errors
    ///
    /// Returns an error if the events cannot be retrieved.
    pub async fn get_activity_log(
        &self,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        filter: Option<&str>,
    ) -> Result<Vec<ActivityLogEvent>> {
        tracing::info!(
            "Getting activity log from {} to {} with filter: {:?}",
            start_time,
            end_time,
            filter
        );

        let mut filter_expr = format!(
            "eventTimestamp ge '{}' and eventTimestamp le '{}'",
            start_time.to_rfc3339(),
            end_time.to_rfc3339()
        );
        if let Some(extra) = filter {
            filter_expr.push_str(" and ");
            filter_expr.push_str(extra);
        }

        let url = format!(
            "{}/subscriptions/{}/providers/microsoft.insights/eventtypes/management/values?api-version={ACTIVITY_LOG_API_VERSION}&$filter={}",
            self.arm_base_url,
            self.subscription_id,
            urlencode(&filter_expr),
        );

        let token = self.arm_token().await?;
        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!("Activity Log request failed: {e}"))
            })?;

        let body: ActivityLogListWire = parse_arm_response(response, "get activity log").await?;
        Ok(body
            .value
            .into_iter()
            .map(ActivityLogWire::into_event)
            .collect())
    }

    fn diagnostic_setting_url(&self, resource_id: &str, setting_name: &str) -> String {
        format!(
            "{}/{}/providers/microsoft.insights/diagnosticSettings/{}?api-version={DIAGNOSTIC_SETTINGS_API_VERSION}",
            self.arm_base_url,
            resource_id.trim_start_matches('/'),
            setting_name
        )
    }

    /// Creates a diagnostic setting via the ARM `diagnosticSettings` API.
    ///
    /// # Errors
    ///
    /// Returns an error if the setting cannot be created.
    pub async fn create_diagnostic_setting(
        &self,
        resource_id: &str,
        setting_name: &str,
        workspace_id: Option<String>,
        storage_account_id: Option<String>,
        event_hub_authorization_rule_id: Option<String>,
    ) -> Result<()> {
        tracing::info!(
            "Creating diagnostic setting: {} for resource: {}",
            setting_name,
            resource_id
        );

        let mut properties = serde_json::Map::new();
        if let Some(ws) = workspace_id {
            properties.insert("workspaceId".to_string(), serde_json::Value::String(ws));
        }
        if let Some(sa) = storage_account_id {
            properties.insert(
                "storageAccountId".to_string(),
                serde_json::Value::String(sa),
            );
        }
        if let Some(eh) = event_hub_authorization_rule_id {
            properties.insert(
                "eventHubAuthorizationRuleId".to_string(),
                serde_json::Value::String(eh),
            );
        }
        let body = serde_json::json!({ "properties": properties });

        let url = self.diagnostic_setting_url(resource_id, setting_name);
        let token = self.arm_token().await?;
        let response = self
            .http_client
            .put(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!(
                    "diagnosticSettings PUT request failed: {e}"
                ))
            })?;
        ensure_arm_success(response, "create diagnostic setting").await?;
        Ok(())
    }

    /// Deletes a diagnostic setting via the ARM `diagnosticSettings` API.
    ///
    /// # Errors
    ///
    /// Returns an error if the setting cannot be deleted.
    pub async fn delete_diagnostic_setting(
        &self,
        resource_id: &str,
        setting_name: &str,
    ) -> Result<()> {
        tracing::info!(
            "Deleting diagnostic setting: {} for resource: {}",
            setting_name,
            resource_id
        );

        let url = self.diagnostic_setting_url(resource_id, setting_name);
        let token = self.arm_token().await?;
        let response = self
            .http_client
            .delete(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::monitoring(format!(
                    "diagnosticSettings DELETE request failed: {e}"
                ))
            })?;
        ensure_arm_success(response, "delete diagnostic setting").await?;
        Ok(())
    }
}

/// Percent-encodes `value` for use in a URL query component.
fn urlencode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

/// Renders a JSON value as a plain string (unquoted for string values).
fn json_as_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

async fn ensure_arm_success(
    response: reqwest::Response,
    action: &str,
) -> Result<reqwest::Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "<unreadable response body>".to_string());
    Err(CloudEnhancedError::monitoring(format!(
        "Azure Monitor returned status {status} while trying to {action}: {body}"
    )))
}

async fn parse_arm_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
    action: &str,
) -> Result<T> {
    let response = ensure_arm_success(response, action).await?;
    response.json::<T>().await.map_err(|e| {
        CloudEnhancedError::monitoring(format!(
            "Failed to parse Azure Monitor response while trying to {action}: {e}"
        ))
    })
}

// ---------------------------------------------------------------------
// Wire (JSON) types.
// ---------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct MetricsResponseWire {
    #[serde(default)]
    value: Vec<MetricWire>,
}

#[derive(Debug, Deserialize)]
struct MetricWire {
    #[serde(default)]
    name: MetricNameWire,
    #[serde(default)]
    timeseries: Vec<TimeSeriesWire>,
}

#[derive(Debug, Deserialize, Default)]
struct MetricNameWire {
    #[serde(default)]
    value: String,
}

#[derive(Debug, Deserialize)]
struct TimeSeriesWire {
    #[serde(default)]
    data: Vec<MetricValueWire>,
}

#[derive(Debug, Deserialize)]
struct MetricValueWire {
    #[serde(rename = "timeStamp", default)]
    time_stamp: Option<DateTime<Utc>>,
    #[serde(default)]
    average: Option<f64>,
    #[serde(default)]
    count: Option<f64>,
    #[serde(default)]
    maximum: Option<f64>,
    #[serde(default)]
    minimum: Option<f64>,
    #[serde(default)]
    total: Option<f64>,
}

impl MetricWire {
    fn into_data(self) -> MetricData {
        let mut timeseries = Vec::new();
        for ts in self.timeseries {
            for point in ts.data {
                timeseries.push(TimeSeriesElement {
                    timestamp: point.time_stamp.unwrap_or_else(Utc::now),
                    average: point.average,
                    count: point.count,
                    maximum: point.maximum,
                    minimum: point.minimum,
                    total: point.total,
                });
            }
        }
        MetricData {
            name: self.name.value,
            timeseries,
        }
    }
}

#[derive(Debug, Deserialize)]
struct LogQueryResponseWire {
    #[serde(default)]
    tables: Vec<LogTableWire>,
}

#[derive(Debug, Deserialize)]
struct LogTableWire {
    #[serde(default)]
    name: String,
    #[serde(default)]
    columns: Vec<LogColumnWire>,
    #[serde(default)]
    rows: Vec<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
struct LogColumnWire {
    #[serde(default)]
    name: String,
}

impl LogTableWire {
    fn into_table(self) -> LogTable {
        LogTable {
            name: self.name,
            columns: self.columns.into_iter().map(|c| c.name).collect(),
            rows: self
                .rows
                .into_iter()
                .map(|row| row.iter().map(json_as_string).collect())
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct NamedListWire {
    #[serde(default)]
    value: Vec<NamedResourceWire>,
}

#[derive(Debug, Deserialize)]
struct NamedResourceWire {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ActivityLogListWire {
    #[serde(default)]
    value: Vec<ActivityLogWire>,
}

#[derive(Debug, Deserialize)]
struct ActivityLogWire {
    #[serde(rename = "eventTimestamp", default)]
    event_timestamp: Option<DateTime<Utc>>,
    #[serde(default)]
    category: LocalizableValueWire,
    #[serde(rename = "operationName", default)]
    operation_name: LocalizableValueWire,
    #[serde(rename = "resourceId", default)]
    resource_id: String,
    #[serde(default)]
    status: LocalizableValueWire,
    #[serde(default)]
    level: String,
}

#[derive(Debug, Deserialize, Default)]
struct LocalizableValueWire {
    #[serde(default)]
    value: String,
}

impl ActivityLogWire {
    fn into_event(self) -> ActivityLogEvent {
        ActivityLogEvent {
            event_time: self.event_timestamp.unwrap_or_else(Utc::now),
            category: self.category.value,
            operation_name: self.operation_name.value,
            resource_id: self.resource_id,
            status: self.status.value,
            level: self.level,
        }
    }
}

/// Metric aggregation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetricAggregation {
    /// Average
    Average,
    /// Count
    Count,
    /// Maximum
    Maximum,
    /// Minimum
    Minimum,
    /// Total
    Total,
}

impl MetricAggregation {
    fn as_str(self) -> &'static str {
        match self {
            MetricAggregation::Average => "Average",
            MetricAggregation::Count => "Count",
            MetricAggregation::Maximum => "Maximum",
            MetricAggregation::Minimum => "Minimum",
            MetricAggregation::Total => "Total",
        }
    }
}

/// Metric data point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricData {
    /// Metric name
    pub name: String,
    /// Time series data
    pub timeseries: Vec<TimeSeriesElement>,
}

/// Time series element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeriesElement {
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Average value
    pub average: Option<f64>,
    /// Count
    pub count: Option<f64>,
    /// Maximum
    pub maximum: Option<f64>,
    /// Minimum
    pub minimum: Option<f64>,
    /// Total
    pub total: Option<f64>,
}

/// Log query result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogQueryResult {
    /// Result tables
    pub tables: Vec<LogTable>,
}

/// Log table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogTable {
    /// Table name
    pub name: String,
    /// Column names
    pub columns: Vec<String>,
    /// Rows
    pub rows: Vec<Vec<String>>,
}

/// Metric alert configuration.
#[derive(Debug, Clone)]
pub struct MetricAlertConfig {
    /// Target resource ID
    pub target_resource_id: String,
    /// Metric name
    pub metric_name: String,
    /// Metric namespace
    pub metric_namespace: String,
    /// Operator (Equals, GreaterThan, LessThan, etc.)
    pub operator: String,
    /// Threshold value
    pub threshold: f64,
    /// Time aggregation
    pub time_aggregation: MetricAggregation,
    /// Action group IDs
    pub action_group_ids: Vec<String>,
}

/// Action receiver.
#[derive(Debug, Clone)]
pub struct ActionReceiver {
    /// Receiver type (Email, SMS, Webhook, etc.)
    pub receiver_type: ReceiverType,
    /// Receiver name
    pub name: String,
    /// Email address (for Email type)
    pub email_address: Option<String>,
    /// Phone number (for SMS type)
    pub phone_number: Option<String>,
    /// Webhook URL (for Webhook type)
    pub webhook_url: Option<String>,
}

/// Receiver type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverType {
    /// Email receiver
    Email,
    /// SMS receiver
    Sms,
    /// Webhook receiver
    Webhook,
    /// Azure Function receiver
    AzureFunction,
    /// Logic App receiver
    LogicApp,
}

/// Activity log event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityLogEvent {
    /// Event time
    pub event_time: DateTime<Utc>,
    /// Category
    pub category: String,
    /// Operation name
    pub operation_name: String,
    /// Resource ID
    pub resource_id: String,
    /// Status
    pub status: String,
    /// Level
    pub level: String,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use azure_core::credentials::TokenCredential;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn spawn_mock_server(status_line: &str, body: String) -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("mock server local addr");

        let response = format!(
            "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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

    #[derive(Debug)]
    struct FakeCredential;

    #[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
    #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
    impl TokenCredential for FakeCredential {
        async fn get_token(
            &self,
            _scopes: &[&str],
            _options: Option<azure_core::credentials::TokenRequestOptions<'_>>,
        ) -> azure_core::Result<azure_core::credentials::AccessToken> {
            Ok(azure_core::credentials::AccessToken::new(
                azure_core::credentials::Secret::new("fake-token".to_string()),
                time::OffsetDateTime::now_utc() + time::Duration::hours(1),
            ))
        }
    }

    fn test_config() -> super::super::AzureConfig {
        super::super::AzureConfig {
            subscription_id: "sub-123".to_string(),
            resource_group: Some("rg".to_string()),
            credential: Arc::new(FakeCredential),
        }
    }

    async fn arm_client(body: &str) -> MonitorClient {
        let arm_base = spawn_mock_server("HTTP/1.1 200 OK", body.to_string()).await;
        MonitorClient::with_base_urls(&test_config(), arm_base, DEFAULT_LOG_ANALYTICS_BASE_URL)
            .expect("client")
    }

    #[tokio::test]
    async fn test_query_metrics_parses_timeseries() {
        let body = r#"{"value":[{"name":{"value":"Percentage CPU"},"timeseries":[{"data":[{"timeStamp":"2024-01-01T00:00:00Z","average":12.5},{"timeStamp":"2024-01-01T00:01:00Z","average":13.0}]}]}]}"#;
        let client = arm_client(body).await;

        let start = Utc::now();
        let end = Utc::now();
        let metrics = client
            .query_metrics(
                "/subscriptions/sub-123/resourceGroups/rg/providers/Microsoft.Compute/virtualMachines/vm1",
                vec!["Percentage CPU".to_string()],
                start,
                end,
                MetricAggregation::Average,
            )
            .await
            .expect("metrics");

        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].name, "Percentage CPU");
        assert_eq!(metrics[0].timeseries.len(), 2);
        assert_eq!(metrics[0].timeseries[0].average, Some(12.5));
    }

    #[tokio::test]
    async fn test_list_alerts_parses_names() {
        let body = r#"{"value":[{"name":"alert-a"},{"name":"alert-b"}]}"#;
        let client = arm_client(body).await;

        let alerts = client.list_alerts("rg").await.expect("alerts");
        assert_eq!(alerts, vec!["alert-a", "alert-b"]);
    }

    #[tokio::test]
    async fn test_get_activity_log_parses() {
        let body = r#"{"value":[{"eventTimestamp":"2024-01-01T00:00:00Z","category":{"value":"Administrative"},"operationName":{"value":"Microsoft.Compute/virtualMachines/write"},"resourceId":"/subscriptions/sub-123/rid","status":{"value":"Succeeded"},"level":"Informational"}]}"#;
        let client = arm_client(body).await;

        let events = client
            .get_activity_log(Utc::now(), Utc::now(), None)
            .await
            .expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].category, "Administrative");
        assert_eq!(events[0].status, "Succeeded");
    }

    #[tokio::test]
    async fn test_query_logs_parses_tables() {
        let la_base = spawn_mock_server(
            "HTTP/1.1 200 OK",
            r#"{"tables":[{"name":"PrimaryResult","columns":[{"name":"Count"}],"rows":[[42]]}]}"#
                .to_string(),
        )
        .await;
        let arm_base = spawn_mock_server("HTTP/1.1 200 OK", "{}".to_string()).await;
        let client =
            MonitorClient::with_base_urls(&test_config(), arm_base, la_base).expect("client");

        let result = client
            .query_logs("workspace-1", "Heartbeat | count", None)
            .await
            .expect("logs");
        assert_eq!(result.tables.len(), 1);
        assert_eq!(result.tables[0].columns, vec!["Count"]);
        assert_eq!(result.tables[0].rows[0], vec!["42"]);
    }

    #[tokio::test]
    async fn test_create_metric_alert_success() {
        let client = arm_client(r#"{"name":"alert"}"#).await;
        let config = MetricAlertConfig {
            target_resource_id: "/subscriptions/sub-123/rid".to_string(),
            metric_name: "Percentage CPU".to_string(),
            metric_namespace: "Microsoft.Compute/virtualMachines".to_string(),
            operator: "GreaterThan".to_string(),
            threshold: 80.0,
            time_aggregation: MetricAggregation::Average,
            action_group_ids: vec!["/subscriptions/sub-123/ag".to_string()],
        };
        let result = client.create_metric_alert("rg", "alert", config).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_metric_is_not_implemented() {
        let client = arm_client("{}").await;
        let result = client
            .send_metric("/rid", "ns", "m", 1.0, HashMap::new())
            .await;
        assert!(matches!(result, Err(CloudEnhancedError::NotImplemented(_))));
    }

    #[tokio::test]
    async fn test_query_metrics_error_status_not_swallowed() {
        let client = MonitorClient::with_base_urls(
            &test_config(),
            spawn_mock_server("HTTP/1.1 404 Not Found", r#"{"error":"x"}"#.to_string()).await,
            DEFAULT_LOG_ANALYTICS_BASE_URL,
        )
        .expect("client");

        let result = client
            .query_metrics(
                "/rid",
                vec!["m".to_string()],
                Utc::now(),
                Utc::now(),
                MetricAggregation::Total,
            )
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_metric_aggregation() {
        assert_eq!(MetricAggregation::Average.as_str(), "Average");
        assert_ne!(MetricAggregation::Average, MetricAggregation::Maximum);
    }

    #[test]
    fn test_action_receiver() {
        let receiver = ActionReceiver {
            receiver_type: ReceiverType::Email,
            name: "admin".to_string(),
            email_address: Some("admin@example.com".to_string()),
            phone_number: None,
            webhook_url: None,
        };
        assert_eq!(receiver.receiver_type, ReceiverType::Email);
        assert!(receiver.email_address.is_some());
    }
}
