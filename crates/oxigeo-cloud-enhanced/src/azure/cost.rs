//! Azure Cost Management integration.
//!
//! Talks to the real Azure Cost Management / Consumption / Advisor REST APIs
//! on the Azure Resource Manager control plane (`management.azure.com`),
//! authenticated with this crate's `azure_core::credentials::TokenCredential`
//! (see [`super::AzureConfig`]), mirroring the HTTP plumbing established in
//! [`super::managed_identity`].
//!
//! Two operations remain unimplemented and return
//! [`CloudEnhancedError::NotImplemented`] rather than a fabricated success:
//! `create_cost_alert` (the caller-provided arguments do not carry enough
//! information to construct a Cost Management scheduled action / budget
//! notification) and `create_export` (the Cost Management exports API requires
//! a fully-qualified destination storage-account resource id, which the
//! caller-provided container name alone cannot supply).

use crate::error::{CloudEnhancedError, Result};
use azure_core::credentials::TokenCredential;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Default base URL of the Azure Resource Manager control plane.
const DEFAULT_ARM_BASE_URL: &str = "https://management.azure.com";

/// API version for the `Microsoft.CostManagement` resource provider.
const COST_MANAGEMENT_API_VERSION: &str = "2023-11-01";

/// API version for the `Microsoft.Consumption` resource provider (budgets,
/// usage details).
const CONSUMPTION_API_VERSION: &str = "2023-05-01";

/// API version for the `Microsoft.Advisor` resource provider
/// (recommendations).
const ADVISOR_API_VERSION: &str = "2023-01-01";

/// Azure Cost Management client.
#[derive(Debug, Clone)]
pub struct CostClient {
    subscription_id: String,
    /// Base URL of the Azure Resource Manager control plane (overridable for
    /// tests).
    arm_base_url: String,
    credential: Arc<dyn TokenCredential>,
    http_client: reqwest::Client,
}

impl CostClient {
    /// Returns the subscription ID.
    pub fn subscription_id(&self) -> &str {
        &self.subscription_id
    }
}

impl CostClient {
    /// Creates a new Cost Management client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be created.
    pub fn new(config: &super::AzureConfig) -> Result<Self> {
        Self::with_arm_base_url(config, DEFAULT_ARM_BASE_URL)
    }

    /// Creates a new Cost Management client pointed at a custom ARM base URL
    /// (primarily for tests, which spin up a local mock server).
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn with_arm_base_url(
        config: &super::AzureConfig,
        arm_base_url: impl Into<String>,
    ) -> Result<Self> {
        let http_client = reqwest::Client::builder().build().map_err(|e| {
            CloudEnhancedError::configuration(format!("Failed to build HTTP client: {e}"))
        })?;

        Ok(Self {
            subscription_id: config.subscription_id().to_string(),
            arm_base_url: arm_base_url.into(),
            credential: config.credential.clone(),
            http_client,
        })
    }

    /// Obtains a bearer token for authenticating ARM management-plane calls.
    async fn bearer_token(&self) -> Result<String> {
        let token = self
            .credential
            .get_token(&["https://management.azure.com/.default"], None)
            .await
            .map_err(|e| {
                CloudEnhancedError::authentication(format!(
                    "Failed to acquire Azure Resource Manager token: {e}"
                ))
            })?;
        Ok(token.token.secret().to_string())
    }

    /// Builds a full ARM URL from a scope + provider path, normalizing the
    /// leading slash of `scope`.
    fn scoped_url(&self, scope: &str, provider_path: &str, api_version: &str) -> String {
        let scope = scope.trim_start_matches('/');
        format!(
            "{}/{scope}/{provider_path}?api-version={api_version}",
            self.arm_base_url
        )
    }

    /// Queries cost data via the Cost Management `query` API.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub async fn query_costs(
        &self,
        scope: &str,
        time_period: TimePeriod,
        granularity: CostGranularity,
        grouping: Option<Vec<CostGrouping>>,
    ) -> Result<CostQueryResult> {
        tracing::info!(
            "Querying costs for scope: {} (granularity: {:?}, grouping: {:?})",
            scope,
            granularity,
            grouping
        );

        let result = self
            .run_query(scope, &time_period, granularity, grouping.as_deref())
            .await?;

        Ok(CostQueryResult {
            rows: result.properties.rows,
            columns: result
                .properties
                .columns
                .into_iter()
                .map(|c| ColumnDefinition {
                    name: c.name,
                    column_type: c.column_type,
                })
                .collect(),
        })
    }

    /// Runs a Cost Management `query` request and returns the parsed result.
    async fn run_query(
        &self,
        scope: &str,
        time_period: &TimePeriod,
        granularity: CostGranularity,
        grouping: Option<&[CostGrouping]>,
    ) -> Result<QueryResultWire> {
        let url = self.scoped_url(
            scope,
            "providers/Microsoft.CostManagement/query",
            COST_MANAGEMENT_API_VERSION,
        );

        let grouping_wire: Vec<GroupingWire> = grouping
            .unwrap_or(&[])
            .iter()
            .map(|g| GroupingWire {
                group_type: g.dimension_type.clone(),
                name: g.name.clone(),
            })
            .collect();

        let body = QueryRequest {
            query_type: "ActualCost".to_string(),
            timeframe: "Custom".to_string(),
            time_period: TimePeriodWire {
                from: format!("{}T00:00:00Z", time_period.from),
                to: format!("{}T23:59:59Z", time_period.to),
            },
            dataset: DatasetWire {
                granularity: granularity.as_str().to_string(),
                aggregation: default_aggregation(),
                grouping: grouping_wire,
            },
        };

        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Cost Management query request failed: {e}"
                ))
            })?;

        parse_arm_response(response, "query costs").await
    }

    /// Gets a cost forecast via the Cost Management `forecast` API.
    ///
    /// # Errors
    ///
    /// Returns an error if the forecast cannot be retrieved.
    pub async fn get_cost_forecast(
        &self,
        scope: &str,
        time_period: TimePeriod,
        granularity: CostGranularity,
    ) -> Result<CostForecast> {
        tracing::info!(
            "Getting cost forecast for scope: {} (granularity: {:?})",
            scope,
            granularity
        );

        let url = self.scoped_url(
            scope,
            "providers/Microsoft.CostManagement/forecast",
            COST_MANAGEMENT_API_VERSION,
        );

        let body = ForecastRequest {
            query_type: "ActualCost".to_string(),
            timeframe: "Custom".to_string(),
            time_period: TimePeriodWire {
                from: format!("{}T00:00:00Z", time_period.from),
                to: format!("{}T23:59:59Z", time_period.to),
            },
            dataset: DatasetWire {
                granularity: granularity.as_str().to_string(),
                aggregation: default_aggregation(),
                grouping: vec![],
            },
            include_actual_cost: false,
            include_fresh_partial_cost: false,
        };

        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Cost Management forecast request failed: {e}"
                ))
            })?;

        let result: QueryResultWire = parse_arm_response(response, "get cost forecast").await?;
        let cost_idx = result.properties.column_index("Cost");
        let currency_idx = result.properties.column_index("Currency");
        let date_idx = result
            .properties
            .column_index("UsageDate")
            .or_else(|| result.properties.column_index("BillingMonth"));

        let mut total_cost = 0.0;
        let mut points = Vec::with_capacity(result.properties.rows.len());
        let mut currency = "USD".to_string();
        for row in &result.properties.rows {
            let cost = cost_idx
                .and_then(|i| row.get(i))
                .and_then(json_as_f64)
                .unwrap_or(0.0);
            total_cost += cost;
            if let Some(cur) = currency_idx
                .and_then(|i| row.get(i))
                .and_then(|v| v.as_str())
            {
                currency = cur.to_string();
            }
            let date = date_idx
                .and_then(|i| row.get(i))
                .map(json_as_string)
                .unwrap_or_default();
            points.push(ForecastPoint { date, cost });
        }

        Ok(CostForecast {
            total_cost,
            currency,
            forecast_points: points,
        })
    }

    /// Gets usage details via the Consumption `usageDetails` API.
    ///
    /// # Errors
    ///
    /// Returns an error if the usage details cannot be retrieved.
    pub async fn get_usage_details(
        &self,
        scope: &str,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<UsageDetail>> {
        tracing::info!(
            "Getting usage details for scope: {} ({} to {})",
            scope,
            start_date,
            end_date
        );

        let filter = format!(
            "properties/usageStart ge '{start_date}' and properties/usageEnd le '{end_date}'"
        );
        let scope_trimmed = scope.trim_start_matches('/');
        let url = format!(
            "{}/{scope_trimmed}/providers/Microsoft.Consumption/usageDetails?api-version={CONSUMPTION_API_VERSION}&$filter={}",
            self.arm_base_url,
            urlencode(&filter),
        );

        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Consumption usageDetails request failed: {e}"
                ))
            })?;

        let body: UsageDetailsListWire = parse_arm_response(response, "get usage details").await?;
        Ok(body
            .value
            .into_iter()
            .map(|u| u.properties.into_usage_detail())
            .collect())
    }

    /// Creates a budget via the Consumption `budgets` API.
    ///
    /// # Errors
    ///
    /// Returns an error if the budget cannot be created.
    pub async fn create_budget(
        &self,
        scope: &str,
        budget_name: &str,
        amount: f64,
        time_grain: TimeGrain,
        start_date: &str,
        end_date: &str,
    ) -> Result<()> {
        tracing::info!(
            "Creating budget: {} for scope: {} (amount: {}, time grain: {:?})",
            budget_name,
            scope,
            amount,
            time_grain
        );

        let url = self.scoped_url(
            scope,
            &format!("providers/Microsoft.Consumption/budgets/{budget_name}"),
            CONSUMPTION_API_VERSION,
        );

        let body = BudgetCreateRequest {
            properties: BudgetCreateProperties {
                category: "Cost".to_string(),
                amount,
                time_grain: time_grain.as_str().to_string(),
                time_period: BudgetTimePeriodWire {
                    start_date: format!("{start_date}T00:00:00Z"),
                    end_date: format!("{end_date}T00:00:00Z"),
                },
            },
        };

        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .put(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Consumption budget PUT request failed: {e}"
                ))
            })?;

        ensure_arm_success(response, "create budget").await?;
        Ok(())
    }

    /// Deletes a budget via the Consumption `budgets` API.
    ///
    /// # Errors
    ///
    /// Returns an error if the budget cannot be deleted.
    pub async fn delete_budget(&self, scope: &str, budget_name: &str) -> Result<()> {
        tracing::info!("Deleting budget: {} from scope: {}", budget_name, scope);

        let url = self.scoped_url(
            scope,
            &format!("providers/Microsoft.Consumption/budgets/{budget_name}"),
            CONSUMPTION_API_VERSION,
        );

        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .delete(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Consumption budget DELETE request failed: {e}"
                ))
            })?;

        ensure_arm_success(response, "delete budget").await?;
        Ok(())
    }

    /// Lists budgets via the Consumption `budgets` API.
    ///
    /// # Errors
    ///
    /// Returns an error if the budgets cannot be listed.
    pub async fn list_budgets(&self, scope: &str) -> Result<Vec<BudgetInfo>> {
        tracing::info!("Listing budgets for scope: {}", scope);

        let url = self.scoped_url(
            scope,
            "providers/Microsoft.Consumption/budgets",
            CONSUMPTION_API_VERSION,
        );

        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Consumption budgets LIST request failed: {e}"
                ))
            })?;

        let body: BudgetListWire = parse_arm_response(response, "list budgets").await?;
        Ok(body.value.into_iter().map(BudgetWire::into_info).collect())
    }

    /// Gets a single budget via the Consumption `budgets` API.
    ///
    /// # Errors
    ///
    /// Returns an error if the budget cannot be retrieved.
    pub async fn get_budget(&self, scope: &str, budget_name: &str) -> Result<BudgetInfo> {
        tracing::info!("Getting budget: {} from scope: {}", budget_name, scope);

        let url = self.scoped_url(
            scope,
            &format!("providers/Microsoft.Consumption/budgets/{budget_name}"),
            CONSUMPTION_API_VERSION,
        );

        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Consumption budget GET request failed: {e}"
                ))
            })?;

        let wire: BudgetWire = parse_arm_response(response, "get budget").await?;
        Ok(wire.into_info())
    }

    /// Creates a cost alert.
    ///
    /// # Errors
    ///
    /// This is not implemented: the caller-provided `(threshold, emails)`
    /// arguments do not carry the amount / budget association / scheduled
    /// action definition the Cost Management alerting APIs require, so this
    /// always returns [`CloudEnhancedError::NotImplemented`] rather than a
    /// fabricated success.
    pub async fn create_cost_alert(
        &self,
        scope: &str,
        alert_name: &str,
        threshold: f64,
        notification_emails: Vec<String>,
    ) -> Result<()> {
        tracing::info!(
            "create_cost_alert requested but not implemented: {} for scope: {} (threshold: {}, {} emails)",
            alert_name,
            scope,
            threshold,
            notification_emails.len()
        );

        Err(CloudEnhancedError::not_implemented(
            "CostClient::create_cost_alert requires a budget association / scheduled-action definition not derivable from the provided arguments",
        ))
    }

    /// Gets cost recommendations via the Azure Advisor API (Cost category).
    ///
    /// # Errors
    ///
    /// Returns an error if the recommendations cannot be retrieved.
    pub async fn get_recommendations(&self, scope: &str) -> Result<Vec<CostRecommendation>> {
        tracing::info!("Getting cost recommendations for scope: {}", scope);

        // Advisor recommendations are surfaced at the subscription scope.
        let url = format!(
            "{}/subscriptions/{}/providers/Microsoft.Advisor/recommendations?api-version={ADVISOR_API_VERSION}&$filter={}",
            self.arm_base_url,
            self.subscription_id,
            urlencode("Category eq 'Cost'"),
        );

        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::azure_service(format!(
                    "Advisor recommendations request failed: {e}"
                ))
            })?;

        let body: AdvisorListWire =
            parse_arm_response(response, "get cost recommendations").await?;
        Ok(body
            .value
            .into_iter()
            .map(AdvisorRecommendationWire::into_recommendation)
            .collect())
    }

    /// Gets cost grouped by resource group via the Cost Management `query`
    /// API.
    ///
    /// # Errors
    ///
    /// Returns an error if the costs cannot be retrieved.
    pub async fn get_costs_by_resource_group(
        &self,
        time_period: TimePeriod,
    ) -> Result<HashMap<String, f64>> {
        tracing::info!("Getting costs by resource group");
        self.grouped_costs(&time_period, "ResourceGroupName").await
    }

    /// Gets cost grouped by service via the Cost Management `query` API.
    ///
    /// # Errors
    ///
    /// Returns an error if the costs cannot be retrieved.
    pub async fn get_costs_by_service(
        &self,
        time_period: TimePeriod,
    ) -> Result<HashMap<String, f64>> {
        tracing::info!("Getting costs by service");
        self.grouped_costs(&time_period, "ServiceName").await
    }

    /// Runs a single-dimension grouped cost query at the subscription scope
    /// and aggregates the rows into a `dimension -> total cost` map.
    async fn grouped_costs(
        &self,
        time_period: &TimePeriod,
        dimension: &str,
    ) -> Result<HashMap<String, f64>> {
        let scope = format!("subscriptions/{}", self.subscription_id);
        let grouping = vec![CostGrouping {
            dimension_type: "Dimension".to_string(),
            name: dimension.to_string(),
        }];
        let result = self
            .run_query(
                &scope,
                time_period,
                CostGranularity::Monthly,
                Some(&grouping),
            )
            .await?;

        let cost_idx = result.properties.column_index("Cost");
        let group_idx = result.properties.column_index(dimension);

        let mut totals: HashMap<String, f64> = HashMap::new();
        for row in &result.properties.rows {
            let key = group_idx
                .and_then(|i| row.get(i))
                .map(json_as_string)
                .unwrap_or_else(|| "(unknown)".to_string());
            let cost = cost_idx
                .and_then(|i| row.get(i))
                .and_then(json_as_f64)
                .unwrap_or(0.0);
            *totals.entry(key).or_insert(0.0) += cost;
        }
        Ok(totals)
    }

    /// Exports cost data to storage.
    ///
    /// # Errors
    ///
    /// This is not implemented: the Cost Management `exports` API requires a
    /// fully-qualified destination storage-account resource id, which cannot
    /// be derived from the container name alone, so this always returns
    /// [`CloudEnhancedError::NotImplemented`] rather than a fabricated
    /// success.
    pub async fn create_export(
        &self,
        scope: &str,
        export_name: &str,
        storage_container: &str,
        recurrence: ExportRecurrence,
    ) -> Result<()> {
        tracing::info!(
            "create_export requested but not implemented: {} for scope: {} (container: {}, recurrence: {:?})",
            export_name,
            scope,
            storage_container,
            recurrence
        );

        Err(CloudEnhancedError::not_implemented(
            "CostClient::create_export requires a fully-qualified destination storage-account resource id, which the container name alone cannot supply",
        ))
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

/// Interprets a JSON value as `f64`, accepting both numeric and
/// numeric-string representations.
fn json_as_f64(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.parse::<f64>().ok()))
}

/// Renders a JSON value as a plain string (unquoted for string values).
fn json_as_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn default_aggregation() -> HashMap<String, AggregationWire> {
    let mut map = HashMap::new();
    map.insert(
        "totalCost".to_string(),
        AggregationWire {
            name: "Cost".to_string(),
            function: "Sum".to_string(),
        },
    );
    map
}

/// Verifies that `response` carries a success status.
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
    Err(CloudEnhancedError::azure_service(format!(
        "Azure Resource Manager returned status {status} while trying to {action}: {body}"
    )))
}

/// Verifies `response` is a success and deserializes its JSON body as `T`.
async fn parse_arm_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
    action: &str,
) -> Result<T> {
    let response = ensure_arm_success(response, action).await?;
    response.json::<T>().await.map_err(|e| {
        CloudEnhancedError::azure_service(format!(
            "Failed to parse Azure Resource Manager response while trying to {action}: {e}"
        ))
    })
}

// ---------------------------------------------------------------------
// Wire (JSON) types for the Cost Management / Consumption / Advisor APIs.
// ---------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct QueryRequest {
    #[serde(rename = "type")]
    query_type: String,
    timeframe: String,
    #[serde(rename = "timePeriod")]
    time_period: TimePeriodWire,
    dataset: DatasetWire,
}

#[derive(Debug, Serialize)]
struct ForecastRequest {
    #[serde(rename = "type")]
    query_type: String,
    timeframe: String,
    #[serde(rename = "timePeriod")]
    time_period: TimePeriodWire,
    dataset: DatasetWire,
    #[serde(rename = "includeActualCost")]
    include_actual_cost: bool,
    #[serde(rename = "includeFreshPartialCost")]
    include_fresh_partial_cost: bool,
}

#[derive(Debug, Serialize)]
struct TimePeriodWire {
    from: String,
    to: String,
}

#[derive(Debug, Serialize)]
struct DatasetWire {
    granularity: String,
    aggregation: HashMap<String, AggregationWire>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    grouping: Vec<GroupingWire>,
}

#[derive(Debug, Serialize)]
struct AggregationWire {
    name: String,
    function: String,
}

#[derive(Debug, Serialize)]
struct GroupingWire {
    #[serde(rename = "type")]
    group_type: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct QueryResultWire {
    #[serde(default)]
    properties: QueryPropertiesWire,
}

#[derive(Debug, Deserialize, Default)]
struct QueryPropertiesWire {
    #[serde(default)]
    columns: Vec<QueryColumnWire>,
    #[serde(default)]
    rows: Vec<Vec<serde_json::Value>>,
}

impl QueryPropertiesWire {
    /// Finds the index of the column whose name equals `name`
    /// (case-insensitive).
    fn column_index(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
    }
}

#[derive(Debug, Deserialize)]
struct QueryColumnWire {
    #[serde(default)]
    name: String,
    #[serde(rename = "type", default)]
    column_type: String,
}

#[derive(Debug, Deserialize)]
struct UsageDetailsListWire {
    #[serde(default)]
    value: Vec<UsageDetailWire>,
}

#[derive(Debug, Deserialize)]
struct UsageDetailWire {
    #[serde(default)]
    properties: UsageDetailPropertiesWire,
}

#[derive(Debug, Deserialize, Default)]
struct UsageDetailPropertiesWire {
    #[serde(rename = "resourceId", default)]
    resource_id: String,
    #[serde(rename = "resourceName", default)]
    resource_name: String,
    #[serde(rename = "consumedService", default)]
    consumed_service: String,
    #[serde(default)]
    date: String,
    #[serde(default)]
    quantity: f64,
    #[serde(default)]
    cost: Option<f64>,
    #[serde(rename = "costInBillingCurrency", default)]
    cost_in_billing_currency: Option<f64>,
    #[serde(rename = "billingCurrency", default)]
    billing_currency: Option<String>,
    #[serde(default)]
    currency: Option<String>,
}

impl UsageDetailPropertiesWire {
    fn into_usage_detail(self) -> UsageDetail {
        UsageDetail {
            resource_id: self.resource_id,
            resource_name: self.resource_name,
            service_name: self.consumed_service,
            usage_date: self.date,
            quantity: self.quantity,
            cost: self.cost.or(self.cost_in_billing_currency).unwrap_or(0.0),
            currency: self
                .currency
                .or(self.billing_currency)
                .unwrap_or_else(|| "USD".to_string()),
        }
    }
}

#[derive(Debug, Serialize)]
struct BudgetCreateRequest {
    properties: BudgetCreateProperties,
}

#[derive(Debug, Serialize)]
struct BudgetCreateProperties {
    category: String,
    amount: f64,
    #[serde(rename = "timeGrain")]
    time_grain: String,
    #[serde(rename = "timePeriod")]
    time_period: BudgetTimePeriodWire,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct BudgetTimePeriodWire {
    #[serde(rename = "startDate", default)]
    start_date: String,
    #[serde(rename = "endDate", default)]
    end_date: String,
}

#[derive(Debug, Deserialize)]
struct BudgetListWire {
    #[serde(default)]
    value: Vec<BudgetWire>,
}

#[derive(Debug, Deserialize)]
struct BudgetWire {
    #[serde(default)]
    name: String,
    #[serde(default)]
    properties: BudgetPropertiesWire,
}

#[derive(Debug, Deserialize, Default)]
struct BudgetPropertiesWire {
    #[serde(default)]
    amount: f64,
    #[serde(rename = "timeGrain", default)]
    time_grain: String,
    #[serde(rename = "timePeriod", default)]
    time_period: BudgetTimePeriodWire,
    #[serde(rename = "currentSpend", default)]
    current_spend: Option<CurrentSpendWire>,
}

#[derive(Debug, Deserialize, Default)]
struct CurrentSpendWire {
    #[serde(default)]
    amount: f64,
    #[serde(default)]
    unit: Option<String>,
}

impl BudgetWire {
    fn into_info(self) -> BudgetInfo {
        let current_spend = self.properties.current_spend.unwrap_or_default();
        BudgetInfo {
            name: self.name,
            amount: self.properties.amount,
            currency: current_spend.unit.unwrap_or_else(|| "USD".to_string()),
            time_grain: TimeGrain::from_wire(&self.properties.time_grain),
            start_date: self.properties.time_period.start_date,
            end_date: self.properties.time_period.end_date,
            current_spend: current_spend.amount,
        }
    }
}

#[derive(Debug, Deserialize)]
struct AdvisorListWire {
    #[serde(default)]
    value: Vec<AdvisorRecommendationWire>,
}

#[derive(Debug, Deserialize)]
struct AdvisorRecommendationWire {
    #[serde(default)]
    properties: AdvisorPropertiesWire,
}

#[derive(Debug, Deserialize, Default)]
struct AdvisorPropertiesWire {
    #[serde(default)]
    category: String,
    #[serde(rename = "shortDescription", default)]
    short_description: AdvisorShortDescriptionWire,
    #[serde(rename = "extendedProperties", default)]
    extended_properties: HashMap<String, serde_json::Value>,
    #[serde(rename = "resourceMetadata", default)]
    resource_metadata: AdvisorResourceMetadataWire,
}

#[derive(Debug, Deserialize, Default)]
struct AdvisorShortDescriptionWire {
    #[serde(default)]
    problem: String,
    #[serde(default)]
    solution: String,
}

#[derive(Debug, Deserialize, Default)]
struct AdvisorResourceMetadataWire {
    #[serde(rename = "resourceId", default)]
    resource_id: String,
}

impl AdvisorRecommendationWire {
    fn into_recommendation(self) -> CostRecommendation {
        let props = self.properties;
        let savings = props
            .extended_properties
            .get("savingsAmount")
            .or_else(|| props.extended_properties.get("annualSavingsAmount"))
            .and_then(json_as_f64)
            .unwrap_or(0.0);
        let description = if props.short_description.solution.is_empty() {
            props.short_description.problem
        } else {
            props.short_description.solution
        };
        CostRecommendation {
            recommendation_type: props.category,
            description,
            potential_savings: savings,
            resource_id: props.resource_metadata.resource_id,
        }
    }
}

/// Time period for cost queries.
#[derive(Debug, Clone)]
pub struct TimePeriod {
    /// Start date (YYYY-MM-DD)
    pub from: String,
    /// End date (YYYY-MM-DD)
    pub to: String,
}

/// Cost granularity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CostGranularity {
    /// Daily
    Daily,
    /// Monthly
    Monthly,
}

impl CostGranularity {
    fn as_str(self) -> &'static str {
        match self {
            CostGranularity::Daily => "Daily",
            CostGranularity::Monthly => "Monthly",
        }
    }
}

/// Cost grouping dimension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostGrouping {
    /// Dimension type (Dimension, TagKey)
    pub dimension_type: String,
    /// Dimension name (ResourceGroupName, ServiceName, etc.)
    pub name: String,
}

/// Cost query result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostQueryResult {
    /// Result rows
    pub rows: Vec<Vec<serde_json::Value>>,
    /// Column definitions
    pub columns: Vec<ColumnDefinition>,
}

/// Column definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnDefinition {
    /// Column name
    pub name: String,
    /// Column type
    pub column_type: String,
}

/// Cost forecast.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostForecast {
    /// Total forecasted cost
    pub total_cost: f64,
    /// Currency
    pub currency: String,
    /// Forecast data points
    pub forecast_points: Vec<ForecastPoint>,
}

/// Forecast data point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastPoint {
    /// Date
    pub date: String,
    /// Forecasted cost
    pub cost: f64,
}

/// Usage detail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageDetail {
    /// Resource ID
    pub resource_id: String,
    /// Resource name
    pub resource_name: String,
    /// Service name
    pub service_name: String,
    /// Usage date
    pub usage_date: String,
    /// Quantity
    pub quantity: f64,
    /// Cost
    pub cost: f64,
    /// Currency
    pub currency: String,
}

/// Time grain for budgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeGrain {
    /// Monthly
    Monthly,
    /// Quarterly
    Quarterly,
    /// Annually
    Annually,
}

impl TimeGrain {
    fn as_str(self) -> &'static str {
        match self {
            TimeGrain::Monthly => "Monthly",
            TimeGrain::Quarterly => "Quarterly",
            TimeGrain::Annually => "Annually",
        }
    }

    fn from_wire(value: &str) -> Self {
        match value {
            "Quarterly" => TimeGrain::Quarterly,
            "Annually" => TimeGrain::Annually,
            _ => TimeGrain::Monthly,
        }
    }
}

/// Budget information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetInfo {
    /// Budget name
    pub name: String,
    /// Budget amount
    pub amount: f64,
    /// Currency
    pub currency: String,
    /// Time grain
    pub time_grain: TimeGrain,
    /// Start date
    pub start_date: String,
    /// End date
    pub end_date: String,
    /// Current spend
    pub current_spend: f64,
}

/// Cost recommendation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostRecommendation {
    /// Recommendation type
    pub recommendation_type: String,
    /// Description
    pub description: String,
    /// Potential savings
    pub potential_savings: f64,
    /// Resource ID
    pub resource_id: String,
}

/// Export recurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportRecurrence {
    /// Daily
    Daily,
    /// Weekly
    Weekly,
    /// Monthly
    Monthly,
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

    async fn test_client(body: &str) -> CostClient {
        let arm_base = spawn_mock_server("HTTP/1.1 200 OK", body.to_string()).await;
        CostClient::with_arm_base_url(&test_config(), arm_base).expect("client")
    }

    fn period() -> TimePeriod {
        TimePeriod {
            from: "2024-01-01".to_string(),
            to: "2024-01-31".to_string(),
        }
    }

    #[tokio::test]
    async fn test_query_costs_parses_columns_and_rows() {
        let body = r#"{"properties":{"columns":[{"name":"Cost","type":"Number"},{"name":"Currency","type":"String"}],"rows":[[12.5,"USD"]]}}"#;
        let client = test_client(body).await;

        let result = client
            .query_costs(
                "subscriptions/sub-123",
                period(),
                CostGranularity::Monthly,
                None,
            )
            .await
            .expect("result");

        assert_eq!(result.columns.len(), 2);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.columns[0].name, "Cost");
    }

    #[tokio::test]
    async fn test_get_cost_forecast_sums_real_rows_not_zero() {
        let body = r#"{"properties":{"columns":[{"name":"Cost","type":"Number"},{"name":"UsageDate","type":"Number"},{"name":"Currency","type":"String"}],"rows":[[10.0,"2024-02-01","USD"],[15.0,"2024-02-02","USD"]]}}"#;
        let client = test_client(body).await;

        let forecast = client
            .get_cost_forecast("subscriptions/sub-123", period(), CostGranularity::Daily)
            .await
            .expect("forecast");

        assert_eq!(forecast.total_cost, 25.0);
        assert_eq!(forecast.currency, "USD");
        assert_eq!(forecast.forecast_points.len(), 2);
    }

    #[tokio::test]
    async fn test_get_budget_parses_real_response_not_hardcoded() {
        let body = r#"{"name":"my-budget","properties":{"amount":250.0,"timeGrain":"Quarterly","timePeriod":{"startDate":"2024-01-01T00:00:00Z","endDate":"2024-12-31T00:00:00Z"},"currentSpend":{"amount":42.0,"unit":"EUR"}}}"#;
        let client = test_client(body).await;

        let budget = client
            .get_budget("subscriptions/sub-123", "my-budget")
            .await
            .expect("budget");

        // Must reflect the response, not the old hardcoded amount:1000/spend:500.
        assert_eq!(budget.name, "my-budget");
        assert_eq!(budget.amount, 250.0);
        assert_eq!(budget.current_spend, 42.0);
        assert_eq!(budget.currency, "EUR");
        assert_eq!(budget.time_grain, TimeGrain::Quarterly);
    }

    #[tokio::test]
    async fn test_list_budgets_parses() {
        let body = r#"{"value":[{"name":"b1","properties":{"amount":100.0,"timeGrain":"Monthly","timePeriod":{"startDate":"2024-01-01T00:00:00Z","endDate":"2024-12-31T00:00:00Z"},"currentSpend":{"amount":10.0,"unit":"USD"}}}]}"#;
        let client = test_client(body).await;

        let budgets = client
            .list_budgets("subscriptions/sub-123")
            .await
            .expect("budgets");
        assert_eq!(budgets.len(), 1);
        assert_eq!(budgets[0].name, "b1");
    }

    #[tokio::test]
    async fn test_get_costs_by_service_aggregates() {
        let body = r#"{"properties":{"columns":[{"name":"Cost","type":"Number"},{"name":"ServiceName","type":"String"}],"rows":[[100.0,"Storage"],[5.5,"Storage"],[20.0,"Compute"]]}}"#;
        let client = test_client(body).await;

        let totals = client.get_costs_by_service(period()).await.expect("totals");
        assert_eq!(totals.get("Storage"), Some(&105.5));
        assert_eq!(totals.get("Compute"), Some(&20.0));
    }

    #[tokio::test]
    async fn test_get_recommendations_parses() {
        let body = r#"{"value":[{"properties":{"category":"Cost","shortDescription":{"problem":"Underused VM","solution":"Resize VM"},"extendedProperties":{"savingsAmount":"120.5"},"resourceMetadata":{"resourceId":"/subscriptions/s/rid"}}}]}"#;
        let client = test_client(body).await;

        let recs = client
            .get_recommendations("subscriptions/sub-123")
            .await
            .expect("recs");
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].description, "Resize VM");
        assert_eq!(recs[0].potential_savings, 120.5);
    }

    #[tokio::test]
    async fn test_create_budget_success() {
        let client = test_client(r#"{"name":"b1","properties":{}}"#).await;
        let result = client
            .create_budget(
                "subscriptions/sub-123",
                "b1",
                500.0,
                TimeGrain::Monthly,
                "2024-01-01",
                "2024-12-31",
            )
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_error_status_is_not_swallowed() {
        let arm_base = spawn_mock_server(
            "HTTP/1.1 403 Forbidden",
            r#"{"error":{"code":"AuthorizationFailed"}}"#.to_string(),
        )
        .await;
        let client = CostClient::with_arm_base_url(&test_config(), arm_base).expect("client");

        let result = client.list_budgets("subscriptions/sub-123").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_cost_alert_is_not_implemented() {
        let client = test_client("{}").await;
        let result = client
            .create_cost_alert("subscriptions/sub-123", "alert", 80.0, vec![])
            .await;
        assert!(matches!(result, Err(CloudEnhancedError::NotImplemented(_))));
    }

    #[tokio::test]
    async fn test_create_export_is_not_implemented() {
        let client = test_client("{}").await;
        let result = client
            .create_export(
                "subscriptions/sub-123",
                "exp",
                "container",
                ExportRecurrence::Daily,
            )
            .await;
        assert!(matches!(result, Err(CloudEnhancedError::NotImplemented(_))));
    }

    #[test]
    fn test_time_period() {
        let period = period();
        assert_eq!(period.from, "2024-01-01");
    }

    #[test]
    fn test_cost_granularity() {
        assert_eq!(CostGranularity::Daily, CostGranularity::Daily);
        assert_ne!(CostGranularity::Daily, CostGranularity::Monthly);
    }
}
