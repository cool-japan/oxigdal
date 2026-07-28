//! Azure Machine Learning integration.
//!
//! Talks to the real Azure Machine Learning v2 control-plane REST API on the
//! Azure Resource Manager control plane (`management.azure.com`,
//! `Microsoft.MachineLearningServices/workspaces/...`), authenticated with
//! this crate's `azure_core::credentials::TokenCredential` (see
//! [`super::AzureConfig`]).
//!
//! The two data-plane scoring operations (`invoke_endpoint`,
//! `invoke_batch_endpoint`) target a per-endpoint scoring URI protected by a
//! separate key/token that the control-plane credential cannot mint, so they
//! return [`CloudEnhancedError::NotImplemented`] rather than a fabricated
//! prediction payload.

use crate::error::{CloudEnhancedError, Result};
use azure_core::credentials::TokenCredential;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

/// Default base URL of the Azure Resource Manager control plane.
const DEFAULT_ARM_BASE_URL: &str = "https://management.azure.com";

/// API version for the `Microsoft.MachineLearningServices` resource provider
/// (v2 API surface).
const ML_API_VERSION: &str = "2024-04-01";

/// Azure ML client.
#[derive(Debug, Clone)]
pub struct AzureMlClient {
    workspace_name: String,
    resource_group: String,
    subscription_id: String,
    arm_base_url: String,
    credential: Arc<dyn TokenCredential>,
    http_client: reqwest::Client,
}

impl AzureMlClient {
    /// Returns the workspace name.
    pub fn workspace_name(&self) -> &str {
        &self.workspace_name
    }

    /// Returns the resource group.
    pub fn resource_group(&self) -> &str {
        &self.resource_group
    }

    /// Returns the subscription ID.
    pub fn subscription_id(&self) -> &str {
        &self.subscription_id
    }
}

impl AzureMlClient {
    /// Creates a new Azure ML client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be created.
    pub fn new(config: &super::AzureConfig) -> Result<Self> {
        Self::with_arm_base_url(config, DEFAULT_ARM_BASE_URL)
    }

    /// Creates a new Azure ML client pointed at a custom ARM base URL
    /// (primarily for tests, which spin up a local mock server).
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be created.
    pub fn with_arm_base_url(
        config: &super::AzureConfig,
        arm_base_url: impl Into<String>,
    ) -> Result<Self> {
        let workspace_name =
            std::env::var("AZURE_ML_WORKSPACE").unwrap_or_else(|_| "default".to_string());

        let resource_group = config
            .resource_group()
            .ok_or_else(|| {
                CloudEnhancedError::configuration(
                    "Resource group is required for Azure ML".to_string(),
                )
            })?
            .to_string();

        let http_client = reqwest::Client::builder().build().map_err(|e| {
            CloudEnhancedError::configuration(format!("Failed to build HTTP client: {e}"))
        })?;

        Ok(Self {
            workspace_name,
            resource_group,
            subscription_id: config.subscription_id().to_string(),
            arm_base_url: arm_base_url.into(),
            credential: config.credential.clone(),
            http_client,
        })
    }

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

    /// Builds a workspace-relative ARM URL (`.../workspaces/{ws}/{suffix}`).
    fn workspace_url(&self, suffix: &str) -> String {
        format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.MachineLearningServices/workspaces/{}/{suffix}?api-version={ML_API_VERSION}",
            self.arm_base_url, self.subscription_id, self.resource_group, self.workspace_name
        )
    }

    /// Fetches the workspace's Azure region (required in the body of several
    /// child-resource `PUT`s).
    async fn workspace_location(&self) -> Result<String> {
        let url = format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.MachineLearningServices/workspaces/{}?api-version={ML_API_VERSION}",
            self.arm_base_url, self.subscription_id, self.resource_group, self.workspace_name
        );
        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!("ML workspace GET request failed: {e}"))
            })?;
        let ws: WorkspaceWire = parse_arm_response(response, "get workspace location").await?;
        Ok(ws.location)
    }

    async fn put_json(&self, url: &str, body: serde_json::Value, action: &str) -> Result<()> {
        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .put(url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!("ML {action} PUT request failed: {e}"))
            })?;
        ensure_arm_success(response, action).await?;
        Ok(())
    }

    async fn delete(&self, url: &str, action: &str) -> Result<()> {
        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .delete(url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!("ML {action} DELETE request failed: {e}"))
            })?;
        ensure_arm_success(response, action).await?;
        Ok(())
    }

    /// Creates an AmlCompute cluster.
    ///
    /// # Errors
    ///
    /// Returns an error if the cluster cannot be created.
    pub async fn create_compute_cluster(
        &self,
        cluster_name: &str,
        vm_size: &str,
        min_nodes: i32,
        max_nodes: i32,
    ) -> Result<()> {
        tracing::info!(
            "Creating compute cluster: {} (VM: {}, nodes: {}-{})",
            cluster_name,
            vm_size,
            min_nodes,
            max_nodes
        );

        let location = self.workspace_location().await?;
        let url = self.workspace_url(&format!("computes/{cluster_name}"));
        let body = serde_json::json!({
            "location": location,
            "properties": {
                "computeType": "AmlCompute",
                "properties": {
                    "vmSize": vm_size,
                    "scaleSettings": {
                        "minNodeCount": min_nodes,
                        "maxNodeCount": max_nodes
                    }
                }
            }
        });
        self.put_json(&url, body, "create compute cluster").await
    }

    /// Deletes a compute cluster.
    ///
    /// # Errors
    ///
    /// Returns an error if the cluster cannot be deleted.
    pub async fn delete_compute_cluster(&self, cluster_name: &str) -> Result<()> {
        tracing::info!("Deleting compute cluster: {}", cluster_name);
        let url = self.workspace_url(&format!("computes/{cluster_name}"));
        self.delete(&url, "delete compute cluster").await
    }

    /// Lists compute cluster names.
    ///
    /// # Errors
    ///
    /// Returns an error if the clusters cannot be listed.
    pub async fn list_compute_clusters(&self) -> Result<Vec<String>> {
        tracing::info!("Listing compute clusters");
        let url = self.workspace_url("computes");
        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!("ML computes LIST request failed: {e}"))
            })?;
        let body: NamedListWire = parse_arm_response(response, "list compute clusters").await?;
        Ok(body.value.into_iter().filter_map(|v| v.name).collect())
    }

    /// Registers a new version of a model.
    ///
    /// Returns the created version string.
    ///
    /// # Errors
    ///
    /// Returns an error if the model cannot be registered.
    pub async fn register_model(
        &self,
        model_name: &str,
        model_path: &str,
        tags: HashMap<String, String>,
    ) -> Result<String> {
        tracing::info!(
            "Registering model: {} from path: {}",
            model_name,
            model_path
        );

        let version = self.next_model_version(model_name).await?;
        let url = self.workspace_url(&format!("models/{model_name}/versions/{version}"));
        let body = serde_json::json!({
            "properties": {
                "modelUri": model_path,
                "modelType": "custom_model",
                "tags": tags,
            }
        });
        self.put_json(&url, body, "register model").await?;
        Ok(version)
    }

    /// Computes the next integer version string for `model_name`, based on the
    /// highest existing numeric version (or `"1"` if none exist).
    async fn next_model_version(&self, model_name: &str) -> Result<String> {
        let url = self.workspace_url(&format!("models/{model_name}/versions"));
        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!(
                    "ML model versions LIST request failed: {e}"
                ))
            })?;

        // A 404 (no such model yet) means we start at version 1.
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok("1".to_string());
        }
        let body: NamedListWire = parse_arm_response(response, "list model versions").await?;
        let max = body
            .value
            .iter()
            .filter_map(|v| v.name.as_deref())
            .filter_map(|n| n.parse::<u64>().ok())
            .max()
            .unwrap_or(0);
        Ok((max + 1).to_string())
    }

    /// Deploys a model to an online endpoint (creates an online deployment).
    ///
    /// # Errors
    ///
    /// Returns an error if the deployment fails.
    pub async fn deploy_model(
        &self,
        endpoint_name: &str,
        deployment_name: &str,
        model_name: &str,
        instance_type: &str,
        instance_count: i32,
    ) -> Result<()> {
        tracing::info!(
            "Deploying model {} to endpoint {} (deployment: {}, type: {}, count: {})",
            model_name,
            endpoint_name,
            deployment_name,
            instance_type,
            instance_count
        );

        let location = self.workspace_location().await?;
        let url = self.workspace_url(&format!(
            "onlineEndpoints/{endpoint_name}/deployments/{deployment_name}"
        ));
        let body = serde_json::json!({
            "location": location,
            "sku": { "name": "Default", "capacity": instance_count },
            "properties": {
                "endpointComputeType": "Managed",
                "model": model_name,
                "instanceType": instance_type,
            }
        });
        self.put_json(&url, body, "deploy model").await
    }

    /// Creates a managed online endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint cannot be created.
    pub async fn create_online_endpoint(&self, endpoint_name: &str) -> Result<()> {
        tracing::info!("Creating online endpoint: {}", endpoint_name);
        let location = self.workspace_location().await?;
        let url = self.workspace_url(&format!("onlineEndpoints/{endpoint_name}"));
        let body = serde_json::json!({
            "location": location,
            "identity": { "type": "SystemAssigned" },
            "properties": { "authMode": "Key" }
        });
        self.put_json(&url, body, "create online endpoint").await
    }

    /// Deletes a managed online endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint cannot be deleted.
    pub async fn delete_online_endpoint(&self, endpoint_name: &str) -> Result<()> {
        tracing::info!("Deleting online endpoint: {}", endpoint_name);
        let url = self.workspace_url(&format!("onlineEndpoints/{endpoint_name}"));
        self.delete(&url, "delete online endpoint").await
    }

    /// Invokes an online endpoint.
    ///
    /// # Errors
    ///
    /// Not implemented: online scoring targets a per-endpoint scoring URI
    /// protected by a key/AAD token the control-plane credential cannot mint;
    /// returns [`CloudEnhancedError::NotImplemented`] rather than a fabricated
    /// prediction.
    pub async fn invoke_endpoint(&self, endpoint_name: &str, _input_data: &str) -> Result<String> {
        tracing::info!(
            "invoke_endpoint requested but not implemented: {}",
            endpoint_name
        );
        Err(CloudEnhancedError::not_implemented(
            "AzureMlClient::invoke_endpoint requires the per-endpoint scoring URI and key, which are not wired up",
        ))
    }

    /// Submits a command training job.
    ///
    /// Returns the created job name.
    ///
    /// # Errors
    ///
    /// Returns an error if the job cannot be submitted.
    pub async fn submit_training_job(&self, job_config: TrainingJobConfig) -> Result<String> {
        tracing::info!("Submitting training job: {}", job_config.experiment_name);

        let job_name = uuid::Uuid::new_v4().to_string();
        let url = self.workspace_url(&format!("jobs/{job_name}"));
        let command = if job_config.arguments.is_empty() {
            format!("python {}", job_config.script_path)
        } else {
            format!(
                "python {} {}",
                job_config.script_path,
                job_config.arguments.join(" ")
            )
        };
        let body = serde_json::json!({
            "properties": {
                "jobType": "Command",
                "experimentName": job_config.experiment_name,
                "computeId": job_config.compute_target,
                "command": command,
                "environmentId": job_config.environment,
                "codeId": job_config.output_path,
            }
        });
        self.put_json(&url, body, "submit training job").await?;
        Ok(job_name)
    }

    /// Gets training job status.
    ///
    /// # Errors
    ///
    /// Returns an error if the status cannot be retrieved.
    pub async fn get_training_job_status(&self, job_id: &str) -> Result<TrainingJobStatus> {
        tracing::info!("Getting training job status: {}", job_id);

        let url = self.workspace_url(&format!("jobs/{job_id}"));
        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!("ML jobs GET request failed: {e}"))
            })?;

        let wire: JobResourceWire = parse_arm_response(response, "get training job status").await?;
        Ok(TrainingJobStatus {
            job_id: job_id.to_string(),
            status: normalize_job_status(&wire.properties.status),
            start_time: None,
            end_time: None,
            metrics: HashMap::new(),
        })
    }

    /// Waits for a training job to complete.
    ///
    /// # Errors
    ///
    /// Returns an error if the job fails or times out.
    pub async fn wait_for_training_job(
        &self,
        job_id: &str,
        poll_interval: Duration,
        timeout: Duration,
    ) -> Result<TrainingJobStatus> {
        let start = std::time::Instant::now();

        loop {
            let status = self.get_training_job_status(job_id).await?;

            match status.status {
                JobStatus::Completed => return Ok(status),
                JobStatus::Failed | JobStatus::Cancelled => {
                    return Err(CloudEnhancedError::ml_service(format!(
                        "Training job {} failed",
                        job_id
                    )));
                }
                JobStatus::Running | JobStatus::Queued => {
                    if start.elapsed() > timeout {
                        return Err(CloudEnhancedError::timeout(format!(
                            "Training job {} timed out after {:?}",
                            job_id, timeout
                        )));
                    }
                    sleep(poll_interval).await;
                }
            }
        }
    }

    /// Lists model containers in the workspace.
    ///
    /// # Errors
    ///
    /// Returns an error if the models cannot be listed.
    pub async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        tracing::info!("Listing models");
        let url = self.workspace_url("models");
        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!("ML models LIST request failed: {e}"))
            })?;
        let body: ModelListWire = parse_arm_response(response, "list models").await?;
        Ok(body
            .value
            .into_iter()
            .map(ModelResourceWire::into_info)
            .collect())
    }

    /// Gets a model container's details.
    ///
    /// # Errors
    ///
    /// Returns an error if the model cannot be retrieved.
    pub async fn get_model(&self, model_name: &str, version: Option<&str>) -> Result<ModelInfo> {
        tracing::info!("Getting model: {} version: {:?}", model_name, version);

        let suffix = match version {
            Some(v) => format!("models/{model_name}/versions/{v}"),
            None => format!("models/{model_name}"),
        };
        let url = self.workspace_url(&suffix);
        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!("ML model GET request failed: {e}"))
            })?;
        let wire: ModelResourceWire = parse_arm_response(response, "get model").await?;
        let mut info = wire.into_info();
        if let Some(v) = version {
            info.version = v.to_string();
        }
        Ok(info)
    }

    /// Creates a batch endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint cannot be created.
    pub async fn create_batch_endpoint(&self, endpoint_name: &str) -> Result<()> {
        tracing::info!("Creating batch endpoint: {}", endpoint_name);
        let location = self.workspace_location().await?;
        let url = self.workspace_url(&format!("batchEndpoints/{endpoint_name}"));
        let body = serde_json::json!({
            "location": location,
            "identity": { "type": "SystemAssigned" },
            "properties": { "authMode": "AADToken" }
        });
        self.put_json(&url, body, "create batch endpoint").await
    }

    /// Invokes a batch endpoint.
    ///
    /// # Errors
    ///
    /// Not implemented: batch scoring is triggered via the endpoint's
    /// data-plane scoring URI (not the ARM control plane); returns
    /// [`CloudEnhancedError::NotImplemented`] rather than a fabricated job id.
    pub async fn invoke_batch_endpoint(
        &self,
        endpoint_name: &str,
        input_data_uri: &str,
        output_data_uri: &str,
    ) -> Result<String> {
        tracing::info!(
            "invoke_batch_endpoint requested but not implemented: {} (input: {}, output: {})",
            endpoint_name,
            input_data_uri,
            output_data_uri
        );
        Err(CloudEnhancedError::not_implemented(
            "AzureMlClient::invoke_batch_endpoint requires the batch endpoint's data-plane scoring URI, which is not wired up",
        ))
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
    Err(CloudEnhancedError::ml_service(format!(
        "Azure ML returned status {status} while trying to {action}: {body}"
    )))
}

async fn parse_arm_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
    action: &str,
) -> Result<T> {
    let response = ensure_arm_success(response, action).await?;
    response.json::<T>().await.map_err(|e| {
        CloudEnhancedError::ml_service(format!(
            "Failed to parse Azure ML response while trying to {action}: {e}"
        ))
    })
}

/// Normalizes an Azure ML job status string into a [`JobStatus`].
fn normalize_job_status(raw: &str) -> JobStatus {
    match raw {
        "Completed" => JobStatus::Completed,
        "Failed" => JobStatus::Failed,
        "Canceled" | "CancelRequested" | "Cancelled" => JobStatus::Cancelled,
        "NotStarted" | "Queued" => JobStatus::Queued,
        _ => JobStatus::Running,
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
struct JobResourceWire {
    #[serde(default)]
    properties: JobPropertiesWire,
}

#[derive(Debug, Deserialize, Default)]
struct JobPropertiesWire {
    #[serde(default)]
    status: String,
}

#[derive(Debug, Deserialize)]
struct ModelListWire {
    #[serde(default)]
    value: Vec<ModelResourceWire>,
}

#[derive(Debug, Deserialize)]
struct ModelResourceWire {
    #[serde(default)]
    name: String,
    #[serde(default)]
    properties: ModelPropertiesWire,
}

#[derive(Debug, Deserialize, Default)]
struct ModelPropertiesWire {
    #[serde(default)]
    description: Option<String>,
    #[serde(rename = "modelVersion", default)]
    model_version: Option<String>,
    #[serde(default)]
    tags: HashMap<String, String>,
}

impl ModelResourceWire {
    fn into_info(self) -> ModelInfo {
        ModelInfo {
            name: self.name,
            version: self
                .properties
                .model_version
                .unwrap_or_else(|| "1".to_string()),
            description: self.properties.description,
            _tags: self.properties.tags,
            created_time: chrono::Utc::now(),
        }
    }
}

/// Training job configuration.
#[derive(Debug, Clone)]
pub struct TrainingJobConfig {
    /// Experiment name
    pub experiment_name: String,
    /// Compute target
    pub compute_target: String,
    /// Script path
    pub script_path: String,
    /// Script arguments
    pub arguments: Vec<String>,
    /// Environment name
    pub environment: String,
    /// Input datasets
    pub inputs: HashMap<String, String>,
    /// Output path
    pub output_path: String,
}

/// Training job status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingJobStatus {
    /// Job ID
    pub job_id: String,
    /// Job status
    pub status: JobStatus,
    /// Start time
    pub start_time: Option<chrono::DateTime<chrono::Utc>>,
    /// End time
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
    /// Metrics
    pub metrics: HashMap<String, f64>,
}

/// Job status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    /// Queued
    Queued,
    /// Running
    Running,
    /// Completed
    Completed,
    /// Failed
    Failed,
    /// Cancelled
    Cancelled,
}

/// Model information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Model name
    pub name: String,
    /// Model version
    pub version: String,
    /// Description
    pub description: Option<String>,
    /// Tags
    pub _tags: HashMap<String, String>,
    /// Created time
    pub created_time: chrono::DateTime<chrono::Utc>,
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

    async fn client(body: &str) -> AzureMlClient {
        let arm_base = spawn_mock_server("HTTP/1.1 200 OK", body.to_string()).await;
        AzureMlClient::with_arm_base_url(&test_config(), arm_base).expect("client")
    }

    #[tokio::test]
    async fn test_list_compute_clusters_parses() {
        let c = client(r#"{"value":[{"name":"cpu-cluster"},{"name":"gpu-cluster"}]}"#).await;
        let clusters = c.list_compute_clusters().await.expect("clusters");
        assert_eq!(clusters, vec!["cpu-cluster", "gpu-cluster"]);
    }

    #[tokio::test]
    async fn test_get_training_job_status_maps_status() {
        let c = client(r#"{"properties":{"status":"Completed"}}"#).await;
        let status = c.get_training_job_status("job-1").await.expect("status");
        assert_eq!(status.status, JobStatus::Completed);
    }

    #[tokio::test]
    async fn test_list_models_parses() {
        let c = client(
            r#"{"value":[{"name":"m1","properties":{"description":"d","modelVersion":"3"}}]}"#,
        )
        .await;
        let models = c.list_models().await.expect("models");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "m1");
        assert_eq!(models[0].version, "3");
    }

    #[tokio::test]
    async fn test_invoke_endpoint_is_not_implemented() {
        let c = client("{}").await;
        let result = c.invoke_endpoint("ep", "{}").await;
        assert!(matches!(result, Err(CloudEnhancedError::NotImplemented(_))));
    }

    #[tokio::test]
    async fn test_invoke_batch_endpoint_is_not_implemented() {
        let c = client("{}").await;
        let result = c.invoke_batch_endpoint("ep", "in", "out").await;
        assert!(matches!(result, Err(CloudEnhancedError::NotImplemented(_))));
    }

    #[tokio::test]
    async fn test_error_status_not_swallowed() {
        let c = AzureMlClient::with_arm_base_url(
            &test_config(),
            spawn_mock_server("HTTP/1.1 403 Forbidden", r#"{"error":"x"}"#.to_string()).await,
        )
        .expect("client");
        let result = c.list_models().await;
        assert!(result.is_err());
    }

    #[test]
    fn test_normalize_job_status() {
        assert_eq!(normalize_job_status("Completed"), JobStatus::Completed);
        assert_eq!(normalize_job_status("Failed"), JobStatus::Failed);
        assert_eq!(normalize_job_status("Canceled"), JobStatus::Cancelled);
        assert_eq!(normalize_job_status("Queued"), JobStatus::Queued);
        assert_eq!(normalize_job_status("Running"), JobStatus::Running);
    }

    #[test]
    fn test_training_job_config() {
        let config = TrainingJobConfig {
            experiment_name: "test-experiment".to_string(),
            compute_target: "cpu-cluster".to_string(),
            script_path: "train.py".to_string(),
            arguments: vec!["--epochs".to_string(), "10".to_string()],
            environment: "pytorch-env".to_string(),
            inputs: HashMap::new(),
            output_path: "/outputs".to_string(),
        };
        assert_eq!(config.experiment_name, "test-experiment");
        assert_eq!(config.arguments.len(), 2);
    }
}
