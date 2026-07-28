//! Azure Synapse Analytics integration.
//!
//! SQL / Spark **pool management** talks to the real Azure Resource Manager
//! control plane (`management.azure.com`,
//! `Microsoft.Synapse/workspaces/...`). **Spark jobs** use the workspace's
//! Livy REST endpoint and **pipelines** use the workspace's Synapse artifacts
//! REST endpoint (`{workspace}.dev.azuresynapse.net`). All calls are
//! authenticated with this crate's `azure_core::credentials::TokenCredential`
//! (see [`super::AzureConfig`]).
//!
//! [`SynapseClient::execute_query`] returns
//! [`CloudEnhancedError::NotImplemented`]: Synapse SQL query execution speaks
//! the TDS wire protocol (not a REST API), which is out of scope for this
//! pure-`reqwest` client -- returning a typed error (rather than a silent
//! empty result set) ensures callers cannot mistake an unimplemented path for
//! "the query returned zero rows". Serverless Spark pools have no ARM
//! start/stop operation, so [`SynapseClient::start_spark_pool`] /
//! [`SynapseClient::stop_spark_pool`] likewise return
//! [`CloudEnhancedError::NotImplemented`].

use crate::error::{CloudEnhancedError, Result};
use azure_core::credentials::TokenCredential;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

/// Default base URL of the Azure Resource Manager control plane.
const DEFAULT_ARM_BASE_URL: &str = "https://management.azure.com";

const SYNAPSE_ARM_API_VERSION: &str = "2021-06-01";
const LIVY_API_VERSION: &str = "2019-11-01-preview";
const ARTIFACTS_API_VERSION: &str = "2020-12-01";

/// Azure Synapse Analytics client.
#[derive(Debug, Clone)]
pub struct SynapseClient {
    workspace_name: String,
    resource_group: Option<String>,
    subscription_id: String,
    arm_base_url: String,
    dev_base_url: String,
    credential: Arc<dyn TokenCredential>,
    http_client: reqwest::Client,
}

impl SynapseClient {
    /// Returns the workspace name.
    pub fn workspace_name(&self) -> &str {
        &self.workspace_name
    }

    /// Returns the subscription ID.
    pub fn subscription_id(&self) -> &str {
        &self.subscription_id
    }
}

impl SynapseClient {
    /// Creates a new Synapse client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be created.
    pub fn new(config: &super::AzureConfig) -> Result<Self> {
        let workspace_name =
            std::env::var("AZURE_SYNAPSE_WORKSPACE").unwrap_or_else(|_| "default".to_string());
        let dev_base_url = format!("https://{workspace_name}.dev.azuresynapse.net");
        Self::with_base_urls(config, workspace_name, DEFAULT_ARM_BASE_URL, dev_base_url)
    }

    /// Creates a new Synapse client with explicit workspace name and base URLs
    /// (primarily for tests, which spin up local mock servers).
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn with_base_urls(
        config: &super::AzureConfig,
        workspace_name: impl Into<String>,
        arm_base_url: impl Into<String>,
        dev_base_url: impl Into<String>,
    ) -> Result<Self> {
        let http_client = reqwest::Client::builder().build().map_err(|e| {
            CloudEnhancedError::configuration(format!("Failed to build HTTP client: {e}"))
        })?;

        Ok(Self {
            workspace_name: workspace_name.into(),
            resource_group: config.resource_group().map(str::to_string),
            subscription_id: config.subscription_id().to_string(),
            arm_base_url: arm_base_url.into(),
            dev_base_url: dev_base_url.into(),
            credential: config.credential.clone(),
            http_client,
        })
    }

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

    async fn dev_token(&self) -> Result<String> {
        self.token_for("https://dev.azuresynapse.net/.default")
            .await
    }

    fn resource_group(&self) -> Result<&str> {
        self.resource_group.as_deref().ok_or_else(|| {
            CloudEnhancedError::configuration(
                "Resource group is required for Synapse pool management".to_string(),
            )
        })
    }

    /// Builds an ARM URL for a workspace child resource.
    fn workspace_arm_url(&self, suffix: &str) -> Result<String> {
        Ok(format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Synapse/workspaces/{}/{suffix}?api-version={SYNAPSE_ARM_API_VERSION}",
            self.arm_base_url,
            self.subscription_id,
            self.resource_group()?,
            self.workspace_name
        ))
    }

    async fn workspace_location(&self) -> Result<String> {
        let url = format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Synapse/workspaces/{}?api-version={SYNAPSE_ARM_API_VERSION}",
            self.arm_base_url,
            self.subscription_id,
            self.resource_group()?,
            self.workspace_name
        );
        let token = self.arm_token().await?;
        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::query_execution(format!(
                    "Synapse workspace GET request failed: {e}"
                ))
            })?;
        let ws: WorkspaceWire = parse_arm_response(response, "get workspace location").await?;
        Ok(ws.location)
    }

    /// Executes a SQL query in Synapse.
    ///
    /// # Errors
    ///
    /// Not implemented: Synapse SQL uses the TDS wire protocol rather than a
    /// REST API; returns [`CloudEnhancedError::NotImplemented`] rather than a
    /// silent empty result set.
    pub async fn execute_query(&self, database: &str, query: &str) -> Result<QueryResult> {
        tracing::info!(
            "execute_query requested but not implemented on database: {}",
            database
        );
        tracing::debug!("Query: {}", query);

        Err(CloudEnhancedError::not_implemented(
            "SynapseClient::execute_query requires a TDS SQL client, which is out of scope for this REST-only client",
        ))
    }

    /// Starts a Spark pool.
    ///
    /// # Errors
    ///
    /// Not implemented: Synapse Spark pools are serverless and have no ARM
    /// start operation; returns [`CloudEnhancedError::NotImplemented`].
    pub async fn start_spark_pool(&self, pool_name: &str) -> Result<()> {
        tracing::info!(
            "start_spark_pool requested but not implemented: {}",
            pool_name
        );
        Err(CloudEnhancedError::not_implemented(
            "SynapseClient::start_spark_pool: Synapse Spark pools are serverless (no start operation)",
        ))
    }

    /// Stops a Spark pool.
    ///
    /// # Errors
    ///
    /// Not implemented: Synapse Spark pools are serverless and have no ARM
    /// stop operation; returns [`CloudEnhancedError::NotImplemented`].
    pub async fn stop_spark_pool(&self, pool_name: &str) -> Result<()> {
        tracing::info!(
            "stop_spark_pool requested but not implemented: {}",
            pool_name
        );
        Err(CloudEnhancedError::not_implemented(
            "SynapseClient::stop_spark_pool: Synapse Spark pools are serverless (no stop operation)",
        ))
    }

    /// Submits a Spark batch job via the workspace Livy endpoint.
    ///
    /// Returns the Livy batch id.
    ///
    /// # Errors
    ///
    /// Returns an error if the job cannot be submitted.
    pub async fn submit_spark_job(
        &self,
        pool_name: &str,
        job_config: SparkJobConfig,
    ) -> Result<String> {
        tracing::info!("Submitting Spark job to pool: {}", pool_name);

        let url = format!(
            "{}/livyApi/versions/{LIVY_API_VERSION}/sparkPools/{pool_name}/batches",
            self.dev_base_url
        );
        let body = serde_json::json!({
            "name": job_config.name,
            "file": job_config.main,
            "args": job_config.args,
            "driverCores": job_config.driver_cores,
            "driverMemory": format!("{}g", job_config.driver_memory_gb),
            "executorCores": job_config.executor_cores,
            "executorMemory": format!("{}g", job_config.executor_memory_gb),
            "numExecutors": job_config.num_executors,
        });

        let token = self.dev_token().await?;
        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::query_execution(format!(
                    "Livy batch submit request failed: {e}"
                ))
            })?;

        let batch: LivyBatchWire = parse_dev_response(response, "submit Spark job").await?;
        batch.id.map(|id| id.to_string()).ok_or_else(|| {
            CloudEnhancedError::query_execution(
                "Livy batch submit response contained no batch id".to_string(),
            )
        })
    }

    /// Gets the status of a Spark batch job via the workspace Livy endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the status cannot be retrieved.
    pub async fn get_spark_job_status(&self, job_id: &str) -> Result<SparkJobStatus> {
        tracing::info!("Getting Spark job status: {}", job_id);

        // Livy exposes batches at the pool level; the workspace-wide status
        // route accepts the batch id directly.
        let url = format!(
            "{}/livyApi/versions/{LIVY_API_VERSION}/batches/{job_id}",
            self.dev_base_url
        );
        let token = self.dev_token().await?;
        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::query_execution(format!(
                    "Livy batch status request failed: {e}"
                ))
            })?;

        let batch: LivyBatchWire = parse_dev_response(response, "get Spark job status").await?;
        Ok(SparkJobStatus {
            job_id: job_id.to_string(),
            state: normalize_livy_state(batch.state.as_deref().unwrap_or("")),
            start_time: None,
            end_time: None,
            error_message: None,
        })
    }

    /// Waits for a Spark job to complete.
    ///
    /// # Errors
    ///
    /// Returns an error if the job fails or times out.
    pub async fn wait_for_spark_job(
        &self,
        job_id: &str,
        poll_interval: Duration,
        timeout: Duration,
    ) -> Result<SparkJobStatus> {
        let start = std::time::Instant::now();

        loop {
            let status = self.get_spark_job_status(job_id).await?;

            match status.state {
                JobState::Succeeded => return Ok(status),
                JobState::Failed | JobState::Cancelled => {
                    return Err(CloudEnhancedError::query_execution(format!(
                        "Spark job {} failed: {:?}",
                        job_id, status.error_message
                    )));
                }
                JobState::Running | JobState::Starting => {
                    if start.elapsed() > timeout {
                        return Err(CloudEnhancedError::timeout(format!(
                            "Spark job {} timed out after {:?}",
                            job_id, timeout
                        )));
                    }
                    sleep(poll_interval).await;
                }
            }
        }
    }

    /// Creates a dedicated SQL pool via ARM.
    ///
    /// # Errors
    ///
    /// Returns an error if the pool cannot be created.
    pub async fn create_sql_pool(&self, pool_name: &str, sku: &str) -> Result<()> {
        tracing::info!("Creating SQL pool: {} with SKU: {}", pool_name, sku);

        let location = self.workspace_location().await?;
        let url = self.workspace_arm_url(&format!("sqlPools/{pool_name}"))?;
        let body = serde_json::json!({
            "location": location,
            "sku": { "name": sku },
            "properties": {}
        });
        self.arm_put(&url, body, "create SQL pool").await
    }

    /// Pauses a dedicated SQL pool via ARM.
    ///
    /// # Errors
    ///
    /// Returns an error if the pool cannot be paused.
    pub async fn pause_sql_pool(&self, pool_name: &str) -> Result<()> {
        tracing::info!("Pausing SQL pool: {}", pool_name);
        let url = self.workspace_arm_url(&format!("sqlPools/{pool_name}/pause"))?;
        self.arm_post(&url, "pause SQL pool").await
    }

    /// Resumes a dedicated SQL pool via ARM.
    ///
    /// # Errors
    ///
    /// Returns an error if the pool cannot be resumed.
    pub async fn resume_sql_pool(&self, pool_name: &str) -> Result<()> {
        tracing::info!("Resuming SQL pool: {}", pool_name);
        let url = self.workspace_arm_url(&format!("sqlPools/{pool_name}/resume"))?;
        self.arm_post(&url, "resume SQL pool").await
    }

    /// Lists dedicated SQL pool names via ARM.
    ///
    /// # Errors
    ///
    /// Returns an error if the pools cannot be listed.
    pub async fn list_sql_pools(&self) -> Result<Vec<String>> {
        tracing::info!("Listing SQL pools");
        let url = self.workspace_arm_url("sqlPools")?;
        self.arm_list_names(&url, "list SQL pools").await
    }

    /// Lists Spark (big data) pool names via ARM.
    ///
    /// # Errors
    ///
    /// Returns an error if the pools cannot be listed.
    pub async fn list_spark_pools(&self) -> Result<Vec<String>> {
        tracing::info!("Listing Spark pools");
        let url = self.workspace_arm_url("bigDataPools")?;
        self.arm_list_names(&url, "list Spark pools").await
    }

    async fn arm_list_names(&self, url: &str, action: &str) -> Result<Vec<String>> {
        let token = self.arm_token().await?;
        let response = self
            .http_client
            .get(url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::query_execution(format!("Synapse {action} request failed: {e}"))
            })?;
        let body: NamedListWire = parse_arm_response(response, action).await?;
        Ok(body.value.into_iter().filter_map(|v| v.name).collect())
    }

    async fn arm_put(&self, url: &str, body: serde_json::Value, action: &str) -> Result<()> {
        let token = self.arm_token().await?;
        let response = self
            .http_client
            .put(url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::query_execution(format!(
                    "Synapse {action} PUT request failed: {e}"
                ))
            })?;
        ensure_arm_success(response, action).await?;
        Ok(())
    }

    async fn arm_post(&self, url: &str, action: &str) -> Result<()> {
        let token = self.arm_token().await?;
        let response = self
            .http_client
            .post(url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::query_execution(format!(
                    "Synapse {action} POST request failed: {e}"
                ))
            })?;
        ensure_arm_success(response, action).await?;
        Ok(())
    }

    /// Creates or updates a pipeline via the workspace artifacts endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the pipeline cannot be created.
    pub async fn create_pipeline(&self, pipeline_name: &str, definition: &str) -> Result<()> {
        tracing::info!("Creating pipeline: {}", pipeline_name);

        let body: serde_json::Value = serde_json::from_str(definition).map_err(|e| {
            CloudEnhancedError::invalid_argument(format!(
                "Pipeline definition is not valid JSON: {e}"
            ))
        })?;

        let url = format!(
            "{}/pipelines/{pipeline_name}?api-version={ARTIFACTS_API_VERSION}",
            self.dev_base_url
        );
        let token = self.dev_token().await?;
        let response = self
            .http_client
            .put(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::query_execution(format!(
                    "Synapse pipeline PUT request failed: {e}"
                ))
            })?;
        ensure_dev_success(response, "create pipeline").await?;
        Ok(())
    }

    /// Triggers a pipeline run via the workspace artifacts endpoint.
    ///
    /// Returns the run id.
    ///
    /// # Errors
    ///
    /// Returns an error if the pipeline cannot be triggered.
    pub async fn trigger_pipeline(
        &self,
        pipeline_name: &str,
        parameters: HashMap<String, String>,
    ) -> Result<String> {
        tracing::info!(
            "Triggering pipeline: {} with {} parameters",
            pipeline_name,
            parameters.len()
        );

        let url = format!(
            "{}/pipelines/{pipeline_name}/createRun?api-version={ARTIFACTS_API_VERSION}",
            self.dev_base_url
        );
        let token = self.dev_token().await?;
        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&token)
            .json(&parameters)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::query_execution(format!(
                    "Synapse pipeline createRun request failed: {e}"
                ))
            })?;

        let run: CreateRunWire = parse_dev_response(response, "trigger pipeline").await?;
        run.run_id.ok_or_else(|| {
            CloudEnhancedError::query_execution(
                "Pipeline createRun response contained no runId".to_string(),
            )
        })
    }

    /// Gets a pipeline run's status via the workspace artifacts endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the status cannot be retrieved.
    pub async fn get_pipeline_run_status(&self, run_id: &str) -> Result<PipelineRunStatus> {
        tracing::info!("Getting pipeline run status: {}", run_id);

        let url = format!(
            "{}/pipelineruns/{run_id}?api-version={ARTIFACTS_API_VERSION}",
            self.dev_base_url
        );
        let token = self.dev_token().await?;
        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::query_execution(format!(
                    "Synapse pipelinerun GET request failed: {e}"
                ))
            })?;

        let wire: PipelineRunWire = parse_dev_response(response, "get pipeline run status").await?;
        Ok(PipelineRunStatus {
            run_id: wire.run_id.unwrap_or_else(|| run_id.to_string()),
            pipeline_name: wire.pipeline_name.unwrap_or_default(),
            status: normalize_pipeline_status(wire.status.as_deref().unwrap_or("")),
            start_time: wire.run_start,
            end_time: wire.run_end,
        })
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
    Err(CloudEnhancedError::query_execution(format!(
        "Azure Resource Manager returned status {status} while trying to {action}: {body}"
    )))
}

async fn parse_arm_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
    action: &str,
) -> Result<T> {
    let response = ensure_arm_success(response, action).await?;
    response.json::<T>().await.map_err(|e| {
        CloudEnhancedError::query_execution(format!(
            "Failed to parse Azure Resource Manager response while trying to {action}: {e}"
        ))
    })
}

async fn ensure_dev_success(
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
    Err(CloudEnhancedError::query_execution(format!(
        "Synapse workspace endpoint returned status {status} while trying to {action}: {body}"
    )))
}

async fn parse_dev_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
    action: &str,
) -> Result<T> {
    let response = ensure_dev_success(response, action).await?;
    response.json::<T>().await.map_err(|e| {
        CloudEnhancedError::query_execution(format!(
            "Failed to parse Synapse workspace response while trying to {action}: {e}"
        ))
    })
}

fn normalize_livy_state(raw: &str) -> JobState {
    match raw {
        "not_started" | "starting" | "recovering" => JobState::Starting,
        "success" => JobState::Succeeded,
        "dead" | "error" => JobState::Failed,
        "killed" | "shutting_down" => JobState::Cancelled,
        // running, idle, busy, and any unknown-but-active state.
        _ => JobState::Running,
    }
}

fn normalize_pipeline_status(raw: &str) -> PipelineStatus {
    match raw {
        "Succeeded" => PipelineStatus::Succeeded,
        "Failed" => PipelineStatus::Failed,
        "Cancelled" | "Canceled" => PipelineStatus::Cancelled,
        _ => PipelineStatus::InProgress,
    }
}

// ---------------------------------------------------------------------
// Wire (JSON) types.
// ---------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct WorkspaceWire {
    #[serde(default)]
    location: String,
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
struct LivyBatchWire {
    #[serde(default)]
    id: Option<i64>,
    #[serde(default)]
    state: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateRunWire {
    #[serde(rename = "runId", default)]
    run_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PipelineRunWire {
    #[serde(rename = "runId", default)]
    run_id: Option<String>,
    #[serde(rename = "pipelineName", default)]
    pipeline_name: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(rename = "runStart", default)]
    run_start: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(rename = "runEnd", default)]
    run_end: Option<chrono::DateTime<chrono::Utc>>,
}

/// Query result from Synapse.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    /// Column names
    pub columns: Vec<String>,
    /// Row data
    pub rows: Vec<Vec<String>>,
    /// Row count
    pub row_count: usize,
}

/// Spark job configuration.
#[derive(Debug, Clone)]
pub struct SparkJobConfig {
    /// Job name
    pub name: String,
    /// Main class or file
    pub main: String,
    /// Arguments
    pub args: Vec<String>,
    /// Driver cores
    pub driver_cores: i32,
    /// Driver memory (GB)
    pub driver_memory_gb: i32,
    /// Executor cores
    pub executor_cores: i32,
    /// Executor memory (GB)
    pub executor_memory_gb: i32,
    /// Number of executors
    pub num_executors: i32,
}

/// Spark job status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SparkJobStatus {
    /// Job ID
    pub job_id: String,
    /// Job state
    pub state: JobState,
    /// Start time
    pub start_time: Option<chrono::DateTime<chrono::Utc>>,
    /// End time
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
    /// Error message
    pub error_message: Option<String>,
}

/// Job state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobState {
    /// Starting
    Starting,
    /// Running
    Running,
    /// Succeeded
    Succeeded,
    /// Failed
    Failed,
    /// Cancelled
    Cancelled,
}

/// Pipeline run status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRunStatus {
    /// Run ID
    pub run_id: String,
    /// Pipeline name
    pub pipeline_name: String,
    /// Status
    pub status: PipelineStatus,
    /// Start time
    pub start_time: Option<chrono::DateTime<chrono::Utc>>,
    /// End time
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
}

/// Pipeline status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PipelineStatus {
    /// In progress
    InProgress,
    /// Succeeded
    Succeeded,
    /// Failed
    Failed,
    /// Cancelled
    Cancelled,
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

    async fn client(arm_body: &str, dev_body: &str) -> SynapseClient {
        let arm_base = spawn_mock_server("HTTP/1.1 200 OK", arm_body.to_string()).await;
        let dev_base = spawn_mock_server("HTTP/1.1 200 OK", dev_body.to_string()).await;
        SynapseClient::with_base_urls(&test_config(), "ws", arm_base, dev_base).expect("client")
    }

    #[tokio::test]
    async fn test_execute_query_is_not_implemented() {
        let c = client("{}", "{}").await;
        let result = c.execute_query("db", "SELECT COUNT(*) FROM my_table").await;
        assert!(matches!(result, Err(CloudEnhancedError::NotImplemented(_))));
    }

    #[tokio::test]
    async fn test_start_spark_pool_is_not_implemented() {
        let c = client("{}", "{}").await;
        assert!(matches!(
            c.start_spark_pool("pool").await,
            Err(CloudEnhancedError::NotImplemented(_))
        ));
    }

    #[tokio::test]
    async fn test_list_sql_pools_parses() {
        let c = client(r#"{"value":[{"name":"dw1"},{"name":"dw2"}]}"#, "{}").await;
        let pools = c.list_sql_pools().await.expect("pools");
        assert_eq!(pools, vec!["dw1", "dw2"]);
    }

    #[tokio::test]
    async fn test_submit_spark_job_returns_real_id() {
        let c = client("{}", r#"{"id":42,"state":"starting"}"#).await;
        let config = SparkJobConfig {
            name: "job".to_string(),
            main: "abfss://main.py".to_string(),
            args: vec![],
            driver_cores: 2,
            driver_memory_gb: 4,
            executor_cores: 2,
            executor_memory_gb: 4,
            num_executors: 2,
        };
        let id = c.submit_spark_job("pool", config).await.expect("id");
        assert_eq!(id, "42");
    }

    #[tokio::test]
    async fn test_get_spark_job_status_maps_state() {
        let c = client("{}", r#"{"id":42,"state":"success"}"#).await;
        let status = c.get_spark_job_status("42").await.expect("status");
        assert_eq!(status.state, JobState::Succeeded);
    }

    #[tokio::test]
    async fn test_trigger_pipeline_returns_run_id() {
        let c = client("{}", r#"{"runId":"run-abc"}"#).await;
        let run_id = c
            .trigger_pipeline("pipe", HashMap::new())
            .await
            .expect("run id");
        assert_eq!(run_id, "run-abc");
    }

    #[tokio::test]
    async fn test_get_pipeline_run_status_parses() {
        let c = client(
            "{}",
            r#"{"runId":"run-abc","pipelineName":"pipe","status":"Succeeded"}"#,
        )
        .await;
        let status = c.get_pipeline_run_status("run-abc").await.expect("status");
        assert_eq!(status.status, PipelineStatus::Succeeded);
        assert_eq!(status.pipeline_name, "pipe");
    }

    #[tokio::test]
    async fn test_error_status_not_swallowed() {
        let arm_base =
            spawn_mock_server("HTTP/1.1 403 Forbidden", r#"{"error":"x"}"#.to_string()).await;
        let dev_base = spawn_mock_server("HTTP/1.1 200 OK", "{}".to_string()).await;
        let c = SynapseClient::with_base_urls(&test_config(), "ws", arm_base, dev_base)
            .expect("client");
        assert!(c.list_sql_pools().await.is_err());
    }

    #[test]
    fn test_normalize_livy_state() {
        assert_eq!(normalize_livy_state("success"), JobState::Succeeded);
        assert_eq!(normalize_livy_state("running"), JobState::Running);
        assert_eq!(normalize_livy_state("error"), JobState::Failed);
        assert_eq!(normalize_livy_state("starting"), JobState::Starting);
    }

    #[test]
    fn test_spark_job_config() {
        let config = SparkJobConfig {
            name: "test-job".to_string(),
            main: "Main.py".to_string(),
            args: vec!["arg1".to_string()],
            driver_cores: 2,
            driver_memory_gb: 4,
            executor_cores: 2,
            executor_memory_gb: 4,
            num_executors: 2,
        };
        assert_eq!(config.name, "test-job");
    }

    #[test]
    fn test_job_state() {
        assert_eq!(JobState::Running, JobState::Running);
        assert_ne!(JobState::Running, JobState::Succeeded);
    }
}
