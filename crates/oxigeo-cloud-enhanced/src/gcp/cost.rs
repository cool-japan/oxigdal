//! Google Cloud cost management integration.
//!
//! [`CostClient::query_costs`] and its `get_costs_by_*` siblings execute the
//! standard [BigQuery billing export] query against the caller-provided
//! billing export dataset via the BigQuery REST `jobs.query` API
//! (<https://cloud.google.com/bigquery/docs/reference/rest/v2/jobs/query>),
//! authenticated by delegating to
//! [`super::workload_identity::WorkloadIdentityClient`].
//!
//! The budget endpoints (`create_budget`, `delete_budget`, `list_budgets`,
//! `create_cost_alert`) call the real Cloud Billing Budgets API
//! (`billingbudgets.googleapis.com`), and the recommendation endpoints
//! (`get_recommendations`, `get_cud_recommendations`) call the real
//! Recommender API (`recommender.googleapis.com`).
//!
//! `analyze_storage_costs`, `get_cost_forecast`, and
//! `configure_billing_export` remain unimplemented (they need a defined cost
//! model / forecasting model / the Cloud Billing account-management API) and
//! return [`CloudEnhancedError::NotImplemented`] rather than a fabricated
//! success, per this crate's policy of never returning a silently-empty/zeroed
//! "successful" result. See `TODO.md` for the tracked follow-up.
//!
//! [BigQuery billing export]: https://cloud.google.com/billing/docs/how-to/export-data-bigquery-tables/standard-usage

use crate::error::{CloudEnhancedError, Result};
use crate::gcp::workload_identity::WorkloadIdentityClient;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Default base URL of the BigQuery REST API.
const DEFAULT_BIGQUERY_BASE_URL: &str = "https://bigquery.googleapis.com";

/// Default base URL of the Cloud Billing Budgets API.
const DEFAULT_BUDGETS_BASE_URL: &str = "https://billingbudgets.googleapis.com";

/// Default base URL of the Recommender API.
const DEFAULT_RECOMMENDER_BASE_URL: &str = "https://recommender.googleapis.com";

/// Recommender id for machine-type (right-sizing) cost recommendations.
const MACHINE_TYPE_RECOMMENDER: &str = "google.compute.instance.MachineTypeRecommender";

/// Recommender id for committed-use-discount (CUD) recommendations.
const CUD_RECOMMENDER: &str = "google.compute.commitment.UsageCommitmentRecommender";

/// OAuth2 scope requested for calls to the BigQuery API.
const BIGQUERY_SCOPE: &str = "https://www.googleapis.com/auth/bigquery.readonly";

/// OAuth2 scope requested for calls to the Cloud Billing / Recommender APIs.
const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

/// Maximum number of `getQueryResults` polls performed while waiting for an
/// asynchronous BigQuery job to complete.
const MAX_POLL_ATTEMPTS: u32 = 10;

/// Delay between `getQueryResults` polls.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

/// Cost Management client for GCP.
#[derive(Debug, Clone)]
pub struct CostClient {
    project_id: String,
    /// Base URL of the BigQuery REST API (overridable for tests).
    bigquery_base_url: String,
    /// Base URL of the Cloud Billing Budgets API (overridable for tests).
    budgets_base_url: String,
    /// Base URL of the Recommender API (overridable for tests).
    recommender_base_url: String,
    http_client: reqwest::Client,
    /// Auth provider, reusing the GCE metadata / IAM Credentials token flow.
    identity: WorkloadIdentityClient,
}

impl CostClient {
    /// Returns the project ID.
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
}

impl CostClient {
    /// Creates a new Cost client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be created.
    pub fn new(config: &super::GcpConfig) -> Result<Self> {
        Self::with_urls(config, DEFAULT_BIGQUERY_BASE_URL, None::<String>)
    }

    /// Creates a new Cost client pointed at custom BigQuery API and
    /// (optionally) GCE metadata server base URLs.
    ///
    /// This is primarily intended for tests, which spin up local mock
    /// servers rather than talking to the real `bigquery.googleapis.com`
    /// and `metadata.google.internal` endpoints.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn with_urls(
        config: &super::GcpConfig,
        bigquery_base_url: impl Into<String>,
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
            bigquery_base_url: bigquery_base_url.into(),
            budgets_base_url: DEFAULT_BUDGETS_BASE_URL.to_string(),
            recommender_base_url: DEFAULT_RECOMMENDER_BASE_URL.to_string(),
            http_client,
            identity,
        })
    }

    /// Overrides the Cloud Billing Budgets API base URL (primarily for tests).
    #[must_use]
    pub fn with_budgets_base_url(mut self, url: impl Into<String>) -> Self {
        self.budgets_base_url = url.into();
        self
    }

    /// Overrides the Recommender API base URL (primarily for tests).
    #[must_use]
    pub fn with_recommender_base_url(mut self, url: impl Into<String>) -> Self {
        self.recommender_base_url = url.into();
        self
    }

    /// Obtains a bearer token with the broad `cloud-platform` scope, used by
    /// the Cloud Billing Budgets and Recommender APIs.
    async fn platform_token(&self) -> Result<String> {
        let token = self
            .identity
            .generate_access_token("default", vec![CLOUD_PLATFORM_SCOPE.to_string()], 3600)
            .await?;
        Ok(token.access_token)
    }

    /// Obtains a bearer token for authenticating to the BigQuery API, using
    /// the instance's attached service account.
    async fn bearer_token(&self) -> Result<String> {
        let token = self
            .identity
            .generate_access_token("default", vec![BIGQUERY_SCOPE.to_string()], 3600)
            .await?;
        Ok(token.access_token)
    }

    /// Queries cost data from BigQuery billing export.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub async fn query_costs(
        &self,
        billing_dataset: &str,
        start_date: &str,
        end_date: &str,
        group_by: Option<Vec<String>>,
    ) -> Result<Vec<CostEntry>> {
        tracing::info!(
            "Querying costs from {} ({} to {}, group_by: {:?})",
            billing_dataset,
            start_date,
            end_date,
            group_by
        );

        validate_identifier(billing_dataset, "billing_dataset")?;
        if let Some(dims) = &group_by {
            for dim in dims {
                if !matches!(dim.as_str(), "date" | "service" | "project") {
                    return Err(CloudEnhancedError::invalid_argument(format!(
                        "Unsupported group_by dimension '{dim}'; supported: date, service, project"
                    )));
                }
            }
        }

        let sql = format!(
            "SELECT \
                FORMAT_DATE('%Y-%m-%d', DATE(usage_start_time)) AS entry_date, \
                currency, \
                service.description AS service_description, \
                project.id AS project_id, \
                SUM(cost) AS total_cost \
             FROM `{}.{}.gcp_billing_export_v1_*` \
             WHERE usage_start_time >= TIMESTAMP(@start_date) \
               AND usage_start_time < TIMESTAMP(@end_date) \
             GROUP BY entry_date, currency, service_description, project_id \
             ORDER BY entry_date",
            self.project_id, billing_dataset
        );

        let response = self
            .execute_query(
                &sql,
                vec![
                    query_param("start_date", start_date),
                    query_param("end_date", end_date),
                ],
            )
            .await?;

        let mut entries = Vec::with_capacity(response.rows.len());
        for row in &response.rows {
            entries.push(CostEntry {
                date: cell_string(row, 0).unwrap_or_default(),
                cost: cell_f64(row, 4)?,
                currency: cell_string(row, 1).unwrap_or_else(|| "USD".to_string()),
                service: cell_string(row, 2),
                project: cell_string(row, 3),
            });
        }
        Ok(entries)
    }

    /// Runs a single-dimension cost aggregation query against the billing
    /// export, grouping by `group_column` (e.g. `"service.description"`).
    async fn query_grouped_costs(
        &self,
        billing_dataset: &str,
        start_date: &str,
        end_date: &str,
        group_column: &str,
    ) -> Result<HashMap<String, f64>> {
        validate_identifier(billing_dataset, "billing_dataset")?;

        let sql = format!(
            "SELECT {group_column} AS grp, SUM(cost) AS total_cost \
             FROM `{}.{}.gcp_billing_export_v1_*` \
             WHERE usage_start_time >= TIMESTAMP(@start_date) \
               AND usage_start_time < TIMESTAMP(@end_date) \
             GROUP BY grp",
            self.project_id, billing_dataset
        );

        let response = self
            .execute_query(
                &sql,
                vec![
                    query_param("start_date", start_date),
                    query_param("end_date", end_date),
                ],
            )
            .await?;

        let mut totals = HashMap::with_capacity(response.rows.len());
        for row in &response.rows {
            let key = cell_string(row, 0).unwrap_or_else(|| "(unknown)".to_string());
            let value = cell_f64(row, 1)?;
            totals.insert(key, value);
        }
        Ok(totals)
    }

    /// Gets cost by service.
    ///
    /// # Errors
    ///
    /// Returns an error if the costs cannot be retrieved.
    pub async fn get_costs_by_service(
        &self,
        billing_dataset: &str,
        start_date: &str,
        end_date: &str,
    ) -> Result<HashMap<String, f64>> {
        tracing::info!(
            "Getting costs by service from {} ({} to {})",
            billing_dataset,
            start_date,
            end_date
        );

        self.query_grouped_costs(billing_dataset, start_date, end_date, "service.description")
            .await
    }

    /// Gets cost by project.
    ///
    /// # Errors
    ///
    /// Returns an error if the costs cannot be retrieved.
    pub async fn get_costs_by_project(
        &self,
        billing_dataset: &str,
        start_date: &str,
        end_date: &str,
    ) -> Result<HashMap<String, f64>> {
        tracing::info!(
            "Getting costs by project from {} ({} to {})",
            billing_dataset,
            start_date,
            end_date
        );

        self.query_grouped_costs(billing_dataset, start_date, end_date, "project.id")
            .await
    }

    /// Gets cost by SKU.
    ///
    /// # Errors
    ///
    /// Returns an error if the costs cannot be retrieved.
    pub async fn get_costs_by_sku(
        &self,
        billing_dataset: &str,
        start_date: &str,
        end_date: &str,
    ) -> Result<HashMap<String, f64>> {
        tracing::info!(
            "Getting costs by SKU from {} ({} to {})",
            billing_dataset,
            start_date,
            end_date
        );

        self.query_grouped_costs(billing_dataset, start_date, end_date, "sku.description")
            .await
    }

    /// Creates a budget via the Cloud Billing Budgets API.
    ///
    /// Returns the created budget's resource name
    /// (`billingAccounts/{acct}/budgets/{id}`).
    ///
    /// # Errors
    ///
    /// Returns an error if the budget cannot be created.
    pub async fn create_budget(
        &self,
        billing_account: &str,
        display_name: &str,
        amount: f64,
        currency_code: &str,
    ) -> Result<String> {
        tracing::info!(
            "Creating budget: {} for billing account: {} (amount: {} {})",
            display_name,
            billing_account,
            amount,
            currency_code
        );

        let url = format!(
            "{}/v1/billingAccounts/{billing_account}/budgets",
            self.budgets_base_url
        );
        let (units, nanos) = split_amount(amount);
        let body = serde_json::json!({
            "displayName": display_name,
            "budgetFilter": {},
            "amount": {
                "specifiedAmount": {
                    "currencyCode": currency_code,
                    "units": units.to_string(),
                    "nanos": nanos,
                }
            }
        });

        let token = self.platform_token().await?;
        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::gcp_service(format!("Budgets create request failed: {e}"))
            })?;

        let wire: BudgetWire = parse_gcp_response(response, "create budget").await?;
        wire.name.ok_or_else(|| {
            CloudEnhancedError::gcp_service("Budgets create response contained no name".to_string())
        })
    }

    /// Deletes a budget via the Cloud Billing Budgets API.
    ///
    /// `budget_name` is the full resource name
    /// (`billingAccounts/{acct}/budgets/{id}`).
    ///
    /// # Errors
    ///
    /// Returns an error if the budget cannot be deleted.
    pub async fn delete_budget(&self, budget_name: &str) -> Result<()> {
        tracing::info!("Deleting budget: {}", budget_name);

        let url = format!(
            "{}/v1/{}",
            self.budgets_base_url,
            budget_name.trim_start_matches('/')
        );
        let token = self.platform_token().await?;
        let response = self
            .http_client
            .delete(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::gcp_service(format!("Budgets delete request failed: {e}"))
            })?;

        let status = response.status();
        if !status.is_success() {
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable response body>".to_string());
            return Err(CloudEnhancedError::gcp_service(format!(
                "Budgets API returned status {status} while deleting budget: {text}"
            )));
        }
        Ok(())
    }

    /// Lists budgets via the Cloud Billing Budgets API.
    ///
    /// # Errors
    ///
    /// Returns an error if the budgets cannot be listed.
    pub async fn list_budgets(&self, billing_account: &str) -> Result<Vec<BudgetInfo>> {
        tracing::info!("Listing budgets for billing account: {}", billing_account);

        let url = format!(
            "{}/v1/billingAccounts/{billing_account}/budgets",
            self.budgets_base_url
        );
        let token = self.platform_token().await?;
        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::gcp_service(format!("Budgets list request failed: {e}"))
            })?;

        let body: BudgetListWire = parse_gcp_response(response, "list budgets").await?;
        Ok(body
            .budgets
            .into_iter()
            .map(BudgetWire::into_info)
            .collect())
    }

    /// Gets cost recommendations via the Recommender API (machine-type /
    /// right-sizing recommender).
    ///
    /// # Errors
    ///
    /// Returns an error if the recommendations cannot be retrieved.
    pub async fn get_recommendations(&self, location: &str) -> Result<Vec<CostRecommendation>> {
        tracing::info!("Getting cost recommendations for location: {}", location);

        let recs = self
            .fetch_recommendations(location, MACHINE_TYPE_RECOMMENDER)
            .await?;
        Ok(recs
            .into_iter()
            .map(|r| CostRecommendation {
                name: r.name.clone().unwrap_or_default(),
                description: r.description.clone().unwrap_or_default(),
                potential_savings: r.savings(),
                currency: r.currency(),
                recommender_type: r
                    .recommender_subtype
                    .clone()
                    .unwrap_or_else(|| MACHINE_TYPE_RECOMMENDER.to_string()),
            })
            .collect())
    }

    /// Gets committed use discount (CUD) recommendations via the Recommender
    /// API.
    ///
    /// # Errors
    ///
    /// Returns an error if the recommendations cannot be retrieved.
    pub async fn get_cud_recommendations(
        &self,
        location: &str,
    ) -> Result<Vec<CommitmentRecommendation>> {
        tracing::info!("Getting CUD recommendations for location: {}", location);

        let recs = self
            .fetch_recommendations(location, CUD_RECOMMENDER)
            .await?;
        Ok(recs
            .into_iter()
            .map(|r| CommitmentRecommendation {
                name: r.name.clone().unwrap_or_default(),
                description: r.description.clone().unwrap_or_default(),
                commitment_amount: 0.0,
                estimated_savings: r.savings(),
                currency: r.currency(),
                term_years: 1,
            })
            .collect())
    }

    /// Fetches recommendations for a given recommender id + location.
    async fn fetch_recommendations(
        &self,
        location: &str,
        recommender: &str,
    ) -> Result<Vec<RecommendationWire>> {
        let url = format!(
            "{}/v1/projects/{}/locations/{location}/recommenders/{recommender}/recommendations",
            self.recommender_base_url, self.project_id
        );
        let token = self.platform_token().await?;
        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::gcp_service(format!("Recommender list request failed: {e}"))
            })?;

        let body: RecommendationListWire =
            parse_gcp_response(response, "list recommendations").await?;
        Ok(body.recommendations)
    }

    /// Analyzes storage costs.
    ///
    /// # Errors
    ///
    /// This is not yet implemented; always returns
    /// [`CloudEnhancedError::NotImplemented`] rather than a fabricated
    /// all-zero analysis.
    pub async fn analyze_storage_costs(
        &self,
        billing_dataset: &str,
        start_date: &str,
        end_date: &str,
    ) -> Result<StorageCostAnalysis> {
        tracing::info!(
            "analyze_storage_costs requested but not implemented: {} ({} to {})",
            billing_dataset,
            start_date,
            end_date
        );

        Err(CloudEnhancedError::not_implemented(
            "CostClient::analyze_storage_costs requires a per-storage-class cost model that is not yet implemented",
        ))
    }

    /// Gets cost forecast.
    ///
    /// # Errors
    ///
    /// This is not yet implemented; always returns
    /// [`CloudEnhancedError::NotImplemented`] rather than a fabricated
    /// zero-cost forecast.
    pub async fn get_cost_forecast(
        &self,
        billing_dataset: &str,
        days_ahead: i32,
    ) -> Result<CostForecast> {
        tracing::info!(
            "get_cost_forecast requested but not implemented: {} ({} days ahead)",
            billing_dataset,
            days_ahead
        );

        Err(CloudEnhancedError::not_implemented(
            "CostClient::get_cost_forecast requires a forecasting model that is not yet implemented",
        ))
    }

    /// Creates a cost alert.
    ///
    /// # Errors
    ///
    /// This is not yet implemented against the Cloud Billing Budgets API;
    /// always returns [`CloudEnhancedError::NotImplemented`].
    pub async fn create_cost_alert(
        &self,
        budget_name: &str,
        threshold_percent: f64,
        notification_channels: Vec<String>,
    ) -> Result<()> {
        tracing::info!(
            "Creating cost alert for budget: {} (threshold: {}%, {} channels)",
            budget_name,
            threshold_percent,
            notification_channels.len()
        );

        let url = format!(
            "{}/v1/{}?updateMask=thresholdRules,notificationsRule",
            self.budgets_base_url,
            budget_name.trim_start_matches('/')
        );
        let body = serde_json::json!({
            "thresholdRules": [{ "thresholdPercent": threshold_percent / 100.0 }],
            "notificationsRule": { "monitoringNotificationChannels": notification_channels }
        });

        let token = self.platform_token().await?;
        let response = self
            .http_client
            .patch(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::gcp_service(format!(
                    "Budgets patch (cost alert) request failed: {e}"
                ))
            })?;

        let status = response.status();
        if !status.is_success() {
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable response body>".to_string());
            return Err(CloudEnhancedError::gcp_service(format!(
                "Budgets API returned status {status} while creating cost alert: {text}"
            )));
        }
        Ok(())
    }

    /// Exports cost data to BigQuery.
    ///
    /// # Errors
    ///
    /// This is not yet implemented against the Cloud Billing API; always
    /// returns [`CloudEnhancedError::NotImplemented`].
    pub async fn configure_billing_export(
        &self,
        billing_account: &str,
        dataset_id: &str,
        table_id: &str,
    ) -> Result<()> {
        tracing::info!(
            "configure_billing_export requested but not implemented for account: {} to {}.{}",
            billing_account,
            dataset_id,
            table_id
        );

        Err(CloudEnhancedError::not_implemented(
            "CostClient::configure_billing_export requires the Cloud Billing API, which is not yet wired up",
        ))
    }

    /// Executes `sql` against BigQuery via `jobs.query`, polling
    /// `jobs.getQueryResults` if the query does not complete synchronously.
    async fn execute_query(
        &self,
        sql: &str,
        query_parameters: Vec<BqQueryParameter>,
    ) -> Result<BqQueryResult> {
        let token = self.bearer_token().await?;
        let url = format!(
            "{}/bigquery/v2/projects/{}/queries",
            self.bigquery_base_url, self.project_id
        );

        let request_body = BqQueryRequest {
            query: sql.to_string(),
            use_legacy_sql: false,
            parameter_mode: "NAMED".to_string(),
            query_parameters,
            timeout_ms: 30_000,
        };

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&token)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::query_execution(format!(
                    "BigQuery jobs.query request failed: {e}"
                ))
            })?;

        let mut body: BqQueryResponse =
            parse_bigquery_response(response, "execute BigQuery query").await?;

        let mut attempts = 0;
        while !body.job_complete {
            let Some(job_reference) = &body.job_reference else {
                return Err(CloudEnhancedError::query_execution(
                    "BigQuery job did not complete synchronously and returned no jobReference to poll",
                ));
            };
            if attempts >= MAX_POLL_ATTEMPTS {
                return Err(CloudEnhancedError::timeout(format!(
                    "BigQuery job '{}' did not complete after {MAX_POLL_ATTEMPTS} polls",
                    job_reference.job_id
                )));
            }
            tokio::time::sleep(POLL_INTERVAL).await;

            let token = self.bearer_token().await?;
            let poll_url = format!(
                "{}/bigquery/v2/projects/{}/queries/{}",
                self.bigquery_base_url, job_reference.project_id, job_reference.job_id
            );
            let response = self
                .http_client
                .get(&poll_url)
                .bearer_auth(&token)
                .send()
                .await
                .map_err(|e| {
                    CloudEnhancedError::query_execution(format!(
                        "BigQuery jobs.getQueryResults request failed: {e}"
                    ))
                })?;
            body = parse_bigquery_response(response, "poll BigQuery query results").await?;
            attempts += 1;
        }

        Ok(BqQueryResult {
            schema: body.schema.unwrap_or(BqSchema { fields: vec![] }),
            rows: body.rows.unwrap_or_default(),
        })
    }
}

/// Validates that `value` is a safe BigQuery identifier component (dataset
/// name), rejecting anything that could not appear unescaped in the `FROM`
/// clause of a query (dataset/table names cannot be bound as BigQuery query
/// parameters, only literal values can).
fn validate_identifier(value: &str, what: &str) -> Result<()> {
    let is_valid = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if is_valid {
        Ok(())
    } else {
        Err(CloudEnhancedError::invalid_argument(format!(
            "Invalid {what}: '{value}' (must contain only letters, digits, underscores, and hyphens)"
        )))
    }
}

fn query_param(name: &str, value: &str) -> BqQueryParameter {
    BqQueryParameter {
        name: name.to_string(),
        parameter_type: BqParameterType {
            param_type: "STRING".to_string(),
        },
        parameter_value: BqParameterValue {
            value: value.to_string(),
        },
    }
}

fn cell_string(row: &BqRow, index: usize) -> Option<String> {
    row.f
        .get(index)
        .and_then(|cell| cell.v.as_ref())
        .and_then(|v| v.as_str().map(str::to_string))
}

fn cell_f64(row: &BqRow, index: usize) -> Result<f64> {
    let raw = row
        .f
        .get(index)
        .and_then(|cell| cell.v.as_ref())
        .and_then(|v| v.as_str());
    match raw {
        Some(s) => s.parse::<f64>().map_err(|e| {
            CloudEnhancedError::query_execution(format!(
                "BigQuery returned a non-numeric cost value '{s}': {e}"
            ))
        }),
        None => Ok(0.0),
    }
}

/// Verifies `response` is a success and deserializes its JSON body as `T`.
async fn parse_bigquery_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
    action: &str,
) -> Result<T> {
    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<unreadable response body>".to_string());
        return Err(CloudEnhancedError::query_execution(format!(
            "BigQuery API returned status {status} while trying to {action}: {body}"
        )));
    }

    response.json::<T>().await.map_err(|e| {
        CloudEnhancedError::query_execution(format!(
            "Failed to parse BigQuery API response while trying to {action}: {e}"
        ))
    })
}

// ---------------------------------------------------------------------
// Wire (JSON) types for the BigQuery `jobs.query` / `jobs.getQueryResults`
// REST API.
// ---------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct BqQueryRequest {
    query: String,
    #[serde(rename = "useLegacySql")]
    use_legacy_sql: bool,
    #[serde(rename = "parameterMode")]
    parameter_mode: String,
    #[serde(rename = "queryParameters")]
    query_parameters: Vec<BqQueryParameter>,
    #[serde(rename = "timeoutMs")]
    timeout_ms: u64,
}

#[derive(Debug, Serialize)]
struct BqQueryParameter {
    name: String,
    #[serde(rename = "parameterType")]
    parameter_type: BqParameterType,
    #[serde(rename = "parameterValue")]
    parameter_value: BqParameterValue,
}

#[derive(Debug, Serialize)]
struct BqParameterType {
    #[serde(rename = "type")]
    param_type: String,
}

#[derive(Debug, Serialize)]
struct BqParameterValue {
    value: String,
}

#[derive(Debug, Deserialize)]
struct BqQueryResponse {
    #[serde(default)]
    schema: Option<BqSchema>,
    #[serde(default)]
    rows: Option<Vec<BqRow>>,
    #[serde(rename = "jobComplete", default)]
    job_complete: bool,
    #[serde(rename = "jobReference", default)]
    job_reference: Option<BqJobReference>,
}

#[derive(Debug, Deserialize)]
struct BqSchema {
    #[serde(default)]
    #[allow(dead_code)]
    fields: Vec<BqField>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct BqField {
    name: String,
    #[serde(rename = "type")]
    field_type: String,
}

#[derive(Debug, Deserialize)]
struct BqRow {
    #[serde(default)]
    f: Vec<BqCell>,
}

#[derive(Debug, Deserialize)]
struct BqCell {
    #[serde(default)]
    v: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct BqJobReference {
    #[serde(rename = "jobId")]
    job_id: String,
    #[serde(rename = "projectId")]
    project_id: String,
}

/// Normalized result of a completed BigQuery query.
struct BqQueryResult {
    #[allow(dead_code)]
    schema: BqSchema,
    rows: Vec<BqRow>,
}

/// Splits a floating-point currency amount into whole `units` and `nanos`
/// (billionths), matching Google's `Money`/`Amount` wire representation.
fn split_amount(amount: f64) -> (i64, i32) {
    let units = amount.trunc() as i64;
    let nanos = ((amount - amount.trunc()) * 1_000_000_000.0).round() as i32;
    (units, nanos)
}

/// Reassembles a `units` + `nanos` money value into an `f64`.
fn join_amount(units: i64, nanos: i32) -> f64 {
    units as f64 + f64::from(nanos) / 1_000_000_000.0
}

/// Verifies `response` is a success and deserializes its JSON body as `T`.
async fn parse_gcp_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
    action: &str,
) -> Result<T> {
    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<unreadable response body>".to_string());
        return Err(CloudEnhancedError::gcp_service(format!(
            "GCP API returned status {status} while trying to {action}: {body}"
        )));
    }
    response.json::<T>().await.map_err(|e| {
        CloudEnhancedError::gcp_service(format!(
            "Failed to parse GCP API response while trying to {action}: {e}"
        ))
    })
}

// ---------------------------------------------------------------------
// Wire (JSON) types for the Cloud Billing Budgets and Recommender APIs.
// ---------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct BudgetListWire {
    #[serde(default)]
    budgets: Vec<BudgetWire>,
}

#[derive(Debug, Deserialize)]
struct BudgetWire {
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "displayName", default)]
    display_name: Option<String>,
    #[serde(default)]
    amount: Option<BudgetAmountWire>,
}

#[derive(Debug, Deserialize)]
struct BudgetAmountWire {
    #[serde(rename = "specifiedAmount", default)]
    specified_amount: Option<MoneyWire>,
}

#[derive(Debug, Deserialize, Default)]
struct MoneyWire {
    #[serde(rename = "currencyCode", default)]
    currency_code: String,
    #[serde(default)]
    units: Option<String>,
    #[serde(default)]
    nanos: i32,
}

impl BudgetWire {
    fn into_info(self) -> BudgetInfo {
        let money = self
            .amount
            .and_then(|a| a.specified_amount)
            .unwrap_or_default();
        let units = money.units.and_then(|u| u.parse::<i64>().ok()).unwrap_or(0);
        BudgetInfo {
            name: self.name.unwrap_or_default(),
            display_name: self.display_name.unwrap_or_default(),
            amount: join_amount(units, money.nanos),
            currency_code: if money.currency_code.is_empty() {
                "USD".to_string()
            } else {
                money.currency_code
            },
        }
    }
}

#[derive(Debug, Deserialize)]
struct RecommendationListWire {
    #[serde(default)]
    recommendations: Vec<RecommendationWire>,
}

#[derive(Debug, Deserialize)]
struct RecommendationWire {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(rename = "recommenderSubtype", default)]
    recommender_subtype: Option<String>,
    #[serde(rename = "primaryImpact", default)]
    primary_impact: Option<ImpactWire>,
}

#[derive(Debug, Deserialize)]
struct ImpactWire {
    #[serde(rename = "costProjection", default)]
    cost_projection: Option<CostProjectionWire>,
}

#[derive(Debug, Deserialize)]
struct CostProjectionWire {
    #[serde(default)]
    cost: Option<MoneyWire>,
}

impl RecommendationWire {
    /// Estimated savings magnitude in currency units (the Recommender API
    /// reports cost *savings* as a negative cost, so the sign is inverted).
    fn savings(&self) -> f64 {
        self.primary_impact
            .as_ref()
            .and_then(|i| i.cost_projection.as_ref())
            .and_then(|c| c.cost.as_ref())
            .map(|m| {
                let units = m
                    .units
                    .as_deref()
                    .and_then(|u| u.parse::<i64>().ok())
                    .unwrap_or(0);
                -join_amount(units, m.nanos)
            })
            .unwrap_or(0.0)
    }

    fn currency(&self) -> String {
        self.primary_impact
            .as_ref()
            .and_then(|i| i.cost_projection.as_ref())
            .and_then(|c| c.cost.as_ref())
            .map(|m| m.currency_code.clone())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "USD".to_string())
    }
}

/// Cost entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEntry {
    /// Date
    pub date: String,
    /// Cost
    pub cost: f64,
    /// Currency
    pub currency: String,
    /// Service
    pub service: Option<String>,
    /// Project
    pub project: Option<String>,
}

/// Budget information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetInfo {
    /// Budget name
    pub name: String,
    /// Display name
    pub display_name: String,
    /// Budget amount
    pub amount: f64,
    /// Currency code
    pub currency_code: String,
}

/// Cost recommendation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostRecommendation {
    /// Recommendation name
    pub name: String,
    /// Description
    pub description: String,
    /// Potential savings
    pub potential_savings: f64,
    /// Currency
    pub currency: String,
    /// Recommender type
    pub recommender_type: String,
}

/// Commitment (CUD) recommendation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitmentRecommendation {
    /// Recommendation name
    pub name: String,
    /// Description
    pub description: String,
    /// Commitment amount
    pub commitment_amount: f64,
    /// Estimated savings
    pub estimated_savings: f64,
    /// Currency
    pub currency: String,
    /// Term (1 year or 3 years)
    pub term_years: i32,
}

/// Storage cost analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageCostAnalysis {
    /// Total cost
    pub total_cost: f64,
    /// Currency
    pub currency: String,
    /// Cost by storage class
    pub by_storage_class: HashMap<String, f64>,
    /// Recommendations
    pub recommendations: Vec<String>,
}

/// Cost forecast.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostForecast {
    /// Forecasted cost
    pub forecasted_cost: f64,
    /// Currency
    pub currency: String,
    /// Forecast end date
    pub forecast_end_date: DateTime<Utc>,
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

    async fn test_client(bigquery_body: &str) -> CostClient {
        let metadata_base = spawn_mock_server(
            "HTTP/1.1 200 OK",
            "application/json",
            r#"{"access_token":"mock-token","expires_in":3600}"#.to_string(),
        )
        .await;
        let bigquery_base = spawn_mock_server(
            "HTTP/1.1 200 OK",
            "application/json",
            bigquery_body.to_string(),
        )
        .await;

        let config =
            crate::gcp::GcpConfig::new("test-project".to_string(), None).expect("gcp config");
        CostClient::with_urls(&config, bigquery_base, Some(metadata_base)).expect("cost client")
    }

    #[tokio::test]
    async fn test_query_costs_parses_rows() {
        let body = r#"{
            "jobComplete": true,
            "schema": {"fields": [
                {"name": "entry_date", "type": "STRING"},
                {"name": "currency", "type": "STRING"},
                {"name": "service_description", "type": "STRING"},
                {"name": "project_id", "type": "STRING"},
                {"name": "total_cost", "type": "FLOAT"}
            ]},
            "rows": [
                {"f": [
                    {"v": "2024-01-01"},
                    {"v": "USD"},
                    {"v": "Compute Engine"},
                    {"v": "my-project"},
                    {"v": "123.45"}
                ]}
            ]
        }"#;
        let client = test_client(body).await;

        let entries = client
            .query_costs("billing_export", "2024-01-01", "2024-02-01", None)
            .await
            .expect("cost entries");

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].date, "2024-01-01");
        assert_eq!(entries[0].cost, 123.45);
        assert_eq!(entries[0].currency, "USD");
        assert_eq!(entries[0].service.as_deref(), Some("Compute Engine"));
        assert_eq!(entries[0].project.as_deref(), Some("my-project"));
    }

    #[tokio::test]
    async fn test_get_costs_by_service_aggregates() {
        let body = r#"{
            "jobComplete": true,
            "rows": [
                {"f": [{"v": "Compute Engine"}, {"v": "100.0"}]},
                {"f": [{"v": "Cloud Storage"}, {"v": "5.5"}]}
            ]
        }"#;
        let client = test_client(body).await;

        let totals = client
            .get_costs_by_service("billing_export", "2024-01-01", "2024-02-01")
            .await
            .expect("cost totals");

        assert_eq!(totals.get("Compute Engine"), Some(&100.0));
        assert_eq!(totals.get("Cloud Storage"), Some(&5.5));
    }

    #[tokio::test]
    async fn test_query_costs_rejects_unsafe_dataset_name() {
        let client = test_client(r#"{"jobComplete": true, "rows": []}"#).await;

        let result = client
            .query_costs("bad`dataset", "2024-01-01", "2024-02-01", None)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_query_costs_rejects_unsupported_group_by() {
        let client = test_client(r#"{"jobComplete": true, "rows": []}"#).await;

        let result = client
            .query_costs(
                "billing_export",
                "2024-01-01",
                "2024-02-01",
                Some(vec!["not_a_real_dimension".to_string()]),
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_cost_forecast_is_not_implemented() {
        let client = test_client(r#"{"jobComplete": true, "rows": []}"#).await;

        let result = client.get_cost_forecast("billing_export", 30).await;
        assert!(matches!(result, Err(CloudEnhancedError::NotImplemented(_))));
    }

    async fn client_with_budgets(budgets_body: &str) -> CostClient {
        let metadata_base = spawn_mock_server(
            "HTTP/1.1 200 OK",
            "application/json",
            r#"{"access_token":"mock-token","expires_in":3600}"#.to_string(),
        )
        .await;
        let budgets_base = spawn_mock_server(
            "HTTP/1.1 200 OK",
            "application/json",
            budgets_body.to_string(),
        )
        .await;
        let config =
            crate::gcp::GcpConfig::new("test-project".to_string(), None).expect("gcp config");
        CostClient::with_urls(&config, DEFAULT_BIGQUERY_BASE_URL, Some(metadata_base))
            .expect("cost client")
            .with_budgets_base_url(budgets_base)
    }

    #[tokio::test]
    async fn test_create_budget_returns_real_name() {
        let client = client_with_budgets(
            r#"{"name":"billingAccounts/ABC/budgets/123","displayName":"Monthly"}"#,
        )
        .await;

        let name = client
            .create_budget("ABC", "Monthly", 1000.0, "USD")
            .await
            .expect("budget name");
        assert_eq!(name, "billingAccounts/ABC/budgets/123");
    }

    #[tokio::test]
    async fn test_list_budgets_parses_amount() {
        let client = client_with_budgets(
            r#"{"budgets":[{"name":"billingAccounts/ABC/budgets/1","displayName":"b","amount":{"specifiedAmount":{"currencyCode":"USD","units":"1500","nanos":500000000}}}]}"#,
        )
        .await;

        let budgets = client.list_budgets("ABC").await.expect("budgets");
        assert_eq!(budgets.len(), 1);
        assert_eq!(budgets[0].amount, 1500.5);
        assert_eq!(budgets[0].currency_code, "USD");
    }

    #[tokio::test]
    async fn test_get_recommendations_parses_savings() {
        let metadata_base = spawn_mock_server(
            "HTTP/1.1 200 OK",
            "application/json",
            r#"{"access_token":"mock-token","expires_in":3600}"#.to_string(),
        )
        .await;
        let recommender_base = spawn_mock_server(
            "HTTP/1.1 200 OK",
            "application/json",
            r#"{"recommendations":[{"name":"r1","description":"Resize","recommenderSubtype":"CHANGE_MACHINE_TYPE","primaryImpact":{"costProjection":{"cost":{"currencyCode":"USD","units":"-40","nanos":0}}}}]}"#
                .to_string(),
        )
        .await;
        let config =
            crate::gcp::GcpConfig::new("test-project".to_string(), None).expect("gcp config");
        let client = CostClient::with_urls(&config, DEFAULT_BIGQUERY_BASE_URL, Some(metadata_base))
            .expect("cost client")
            .with_recommender_base_url(recommender_base);

        let recs = client
            .get_recommendations("us-central1-a")
            .await
            .expect("recs");
        assert_eq!(recs.len(), 1);
        // Savings reported as negative cost -> positive savings.
        assert_eq!(recs[0].potential_savings, 40.0);
        assert_eq!(recs[0].currency, "USD");
    }

    #[tokio::test]
    async fn test_budgets_error_status_not_swallowed() {
        let client = {
            let metadata_base = spawn_mock_server(
                "HTTP/1.1 200 OK",
                "application/json",
                r#"{"access_token":"mock-token","expires_in":3600}"#.to_string(),
            )
            .await;
            let budgets_base = spawn_mock_server(
                "HTTP/1.1 403 Forbidden",
                "application/json",
                r#"{"error":"x"}"#.to_string(),
            )
            .await;
            let config =
                crate::gcp::GcpConfig::new("test-project".to_string(), None).expect("gcp config");
            CostClient::with_urls(&config, DEFAULT_BIGQUERY_BASE_URL, Some(metadata_base))
                .expect("cost client")
                .with_budgets_base_url(budgets_base)
        };

        assert!(client.list_budgets("ABC").await.is_err());
    }

    #[test]
    fn test_split_join_amount() {
        let (units, nanos) = split_amount(1500.5);
        assert_eq!(units, 1500);
        assert_eq!(nanos, 500_000_000);
        assert!((join_amount(units, nanos) - 1500.5).abs() < 1e-9);
    }

    #[tokio::test]
    async fn test_get_cost_forecast_is_still_not_implemented() {
        let client = test_client(r#"{"jobComplete": true, "rows": []}"#).await;
        let result = client.get_cost_forecast("billing_export", 30).await;
        assert!(matches!(result, Err(CloudEnhancedError::NotImplemented(_))));
    }

    #[test]
    fn test_validate_identifier() {
        assert!(validate_identifier("billing_export_2024", "dataset").is_ok());
        assert!(validate_identifier("my-dataset", "dataset").is_ok());
        assert!(validate_identifier("bad`name", "dataset").is_err());
        assert!(validate_identifier("", "dataset").is_err());
        assert!(validate_identifier("has space", "dataset").is_err());
    }

    #[test]
    fn test_cost_entry() {
        let entry = CostEntry {
            date: "2024-01-01".to_string(),
            cost: 100.0,
            currency: "USD".to_string(),
            service: Some("Compute Engine".to_string()),
            project: Some("my-project".to_string()),
        };

        assert_eq!(entry.cost, 100.0);
        assert_eq!(entry.currency, "USD");
    }

    #[test]
    fn test_budget_info() {
        let budget = BudgetInfo {
            name: "budgets/123".to_string(),
            display_name: "Monthly Budget".to_string(),
            amount: 1000.0,
            currency_code: "USD".to_string(),
        };

        assert_eq!(budget.amount, 1000.0);
    }
}
