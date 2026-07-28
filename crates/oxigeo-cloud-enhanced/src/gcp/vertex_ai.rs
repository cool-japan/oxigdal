//! Google Vertex AI integration for machine learning.
//!
//! Talks to the real Vertex AI REST API
//! (`https://{location}-aiplatform.googleapis.com/v1`). Authentication is
//! obtained by delegating to
//! [`super::workload_identity::WorkloadIdentityClient`], which already
//! implements the GCE metadata server / IAM Credentials token flow.
//!
//! Several Vertex AI mutations (`models:upload`, `endpoints`,
//! `endpoints:deployModel`, `models.delete`) are asynchronous
//! [long-running operations][lro]; those methods poll the returned operation to
//! completion and surface the operation's real result rather than fabricating
//! an identifier.
//!
//! [lro]: https://cloud.google.com/vertex-ai/docs/reference/rest/v1/projects.locations.operations

use crate::error::{CloudEnhancedError, Result};
use serde::{Deserialize, Serialize};

use super::workload_identity::WorkloadIdentityClient;

/// OAuth2 scope requested for calls to the Vertex AI API.
const VERTEX_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

/// Maximum number of operation polls performed while awaiting a long-running
/// operation.
const MAX_POLL_ATTEMPTS: u32 = 20;

/// Delay between long-running-operation polls.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

/// Vertex AI client.
#[derive(Debug, Clone)]
pub struct VertexAiClient {
    project_id: String,
    location: String,
    /// Base URL of the Vertex AI API (overridable for tests).
    aiplatform_base_url: String,
    http_client: reqwest::Client,
    /// Auth provider, reusing the GCE metadata / IAM Credentials token flow.
    identity: WorkloadIdentityClient,
}

impl VertexAiClient {
    /// Returns the project ID.
    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    /// Returns the location.
    pub fn location(&self) -> &str {
        &self.location
    }
}

impl VertexAiClient {
    /// Creates a new Vertex AI client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be created.
    pub fn new(config: &super::GcpConfig) -> Result<Self> {
        let location = config.location().unwrap_or("us-central1").to_string();
        let base = format!("https://{location}-aiplatform.googleapis.com");
        Self::with_urls(config, base, None::<String>)
    }

    /// Creates a new Vertex AI client pointed at custom Vertex AI API and
    /// (optionally) GCE metadata server base URLs.
    ///
    /// This is primarily intended for tests, which spin up local mock servers
    /// rather than talking to the real `{location}-aiplatform.googleapis.com`
    /// and `metadata.google.internal` endpoints.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn with_urls(
        config: &super::GcpConfig,
        aiplatform_base_url: impl Into<String>,
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
            location: config.location().unwrap_or("us-central1").to_string(),
            aiplatform_base_url: aiplatform_base_url.into(),
            http_client,
            identity,
        })
    }

    /// Obtains a bearer token for authenticating to the Vertex AI API.
    async fn bearer_token(&self) -> Result<String> {
        let token = self
            .identity
            .generate_access_token("default", vec![VERTEX_SCOPE.to_string()], 3600)
            .await?;
        Ok(token.access_token)
    }

    /// The `projects/{p}/locations/{loc}` parent resource prefix.
    fn parent(&self) -> String {
        format!("projects/{}/locations/{}", self.project_id, self.location)
    }

    /// Qualifies a possibly-short id (e.g. `"123"`) into a fully-qualified
    /// resource name (e.g. `"projects/p/locations/l/models/123"`), leaving
    /// already-qualified names (starting with `"projects/"`) untouched.
    fn qualify(&self, id: &str, collection: &str) -> String {
        if id.starts_with("projects/") {
            id.to_string()
        } else {
            format!("{}/{collection}/{id}", self.parent())
        }
    }

    /// Uploads a model to Model Registry.
    ///
    /// Returns the uploaded model's fully-qualified resource name.
    ///
    /// # Errors
    ///
    /// Returns an error if the model cannot be uploaded.
    pub async fn upload_model(
        &self,
        display_name: &str,
        artifact_uri: &str,
        serving_container_image_uri: &str,
    ) -> Result<String> {
        tracing::info!(
            "Uploading model: {} (artifact: {}, container: {})",
            display_name,
            artifact_uri,
            serving_container_image_uri
        );

        let url = format!(
            "{}/v1/{}/models:upload",
            self.aiplatform_base_url,
            self.parent()
        );
        let body = serde_json::json!({
            "model": {
                "displayName": display_name,
                "artifactUri": artifact_uri,
                "containerSpec": { "imageUri": serving_container_image_uri }
            }
        });

        let response = self.await_operation(&url, &body, "upload model").await?;
        response
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| {
                CloudEnhancedError::ml_service(
                    "Vertex AI models:upload operation returned no model name".to_string(),
                )
            })
    }

    /// Deploys a model to an endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the deployment fails.
    pub async fn deploy_model(
        &self,
        endpoint_id: &str,
        model_id: &str,
        machine_type: &str,
        min_replica_count: i32,
        max_replica_count: i32,
    ) -> Result<()> {
        tracing::info!(
            "Deploying model {} to endpoint {} (machine: {}, replicas: {}-{})",
            model_id,
            endpoint_id,
            machine_type,
            min_replica_count,
            max_replica_count
        );

        let endpoint = self.qualify(endpoint_id, "endpoints");
        let model = self.qualify(model_id, "models");
        let url = format!("{}/v1/{}:deployModel", self.aiplatform_base_url, endpoint);
        let body = serde_json::json!({
            "deployedModel": {
                "model": model,
                "dedicatedResources": {
                    "machineSpec": { "machineType": machine_type },
                    "minReplicaCount": min_replica_count,
                    "maxReplicaCount": max_replica_count
                }
            }
        });

        self.await_operation(&url, &body, "deploy model").await?;
        Ok(())
    }

    /// Creates an endpoint.
    ///
    /// Returns the created endpoint's fully-qualified resource name.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint cannot be created.
    pub async fn create_endpoint(&self, display_name: &str) -> Result<String> {
        tracing::info!("Creating endpoint: {}", display_name);

        let url = format!(
            "{}/v1/{}/endpoints",
            self.aiplatform_base_url,
            self.parent()
        );
        let body = serde_json::json!({ "displayName": display_name });

        let response = self.await_operation(&url, &body, "create endpoint").await?;
        response
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| {
                CloudEnhancedError::ml_service(
                    "Vertex AI endpoints.create operation returned no endpoint name".to_string(),
                )
            })
    }

    /// Deletes an endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint cannot be deleted.
    pub async fn delete_endpoint(&self, endpoint_id: &str) -> Result<()> {
        tracing::info!("Deleting endpoint: {}", endpoint_id);

        let endpoint = self.qualify(endpoint_id, "endpoints");
        let url = format!("{}/v1/{}", self.aiplatform_base_url, endpoint);
        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .delete(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::gcp_service(format!(
                    "Vertex AI endpoints.delete request failed: {e}"
                ))
            })?;
        let operation: OperationWire = parse_vertex_response(response, "delete endpoint").await?;
        self.drive_operation(operation, "delete endpoint").await?;
        Ok(())
    }

    /// Predicts using an endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the prediction fails.
    pub async fn predict(
        &self,
        endpoint_id: &str,
        instances: Vec<serde_json::Value>,
    ) -> Result<Vec<serde_json::Value>> {
        tracing::info!("Making prediction with {} instances", instances.len());

        let endpoint = self.qualify(endpoint_id, "endpoints");
        let url = format!("{}/v1/{}:predict", self.aiplatform_base_url, endpoint);
        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&token)
            .json(&serde_json::json!({ "instances": instances }))
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!("Vertex AI predict request failed: {e}"))
            })?;

        let body: PredictResponse = parse_vertex_response(response, "predict").await?;
        Ok(body.predictions)
    }

    /// Creates a training pipeline.
    ///
    /// Returns the created pipeline's fully-qualified resource name.
    ///
    /// # Errors
    ///
    /// Returns an error if the pipeline cannot be created.
    pub async fn create_training_pipeline(
        &self,
        display_name: &str,
        training_task_definition: &str,
        training_task_inputs: serde_json::Value,
        model_to_upload: Option<ModelSpec>,
    ) -> Result<String> {
        tracing::info!("Creating training pipeline: {}", display_name);

        let url = format!(
            "{}/v1/{}/trainingPipelines",
            self.aiplatform_base_url,
            self.parent()
        );
        let mut body = serde_json::json!({
            "displayName": display_name,
            "trainingTaskDefinition": training_task_definition,
            "trainingTaskInputs": training_task_inputs,
        });
        if let Some(spec) = model_to_upload {
            body["modelToUpload"] = serde_json::json!({
                "displayName": spec.display_name,
                "artifactUri": spec.artifact_uri,
                "containerSpec": { "imageUri": spec.serving_container_image_uri }
            });
        }

        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!(
                    "Vertex AI trainingPipelines.create request failed: {e}"
                ))
            })?;

        let pipeline: TrainingPipelineWire =
            parse_vertex_response(response, "create training pipeline").await?;
        pipeline.name.ok_or_else(|| {
            CloudEnhancedError::ml_service(
                "Vertex AI trainingPipelines.create returned no pipeline name".to_string(),
            )
        })
    }

    /// Gets training pipeline status.
    ///
    /// # Errors
    ///
    /// Returns an error if the status cannot be retrieved.
    pub async fn get_training_pipeline_status(&self, pipeline_id: &str) -> Result<PipelineStatus> {
        tracing::info!("Getting training pipeline status: {}", pipeline_id);

        let pipeline = self.qualify(pipeline_id, "trainingPipelines");
        let url = format!("{}/v1/{}", self.aiplatform_base_url, pipeline);
        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!(
                    "Vertex AI trainingPipelines.get request failed: {e}"
                ))
            })?;

        let wire: TrainingPipelineWire =
            parse_vertex_response(response, "get training pipeline status").await?;
        Ok(PipelineStatus {
            pipeline_id: wire.name.unwrap_or_else(|| pipeline_id.to_string()),
            state: wire
                .state
                .as_deref()
                .map(normalize_pipeline_state)
                .unwrap_or(PipelineState::Running),
            create_time: wire.create_time.unwrap_or_else(chrono::Utc::now),
            update_time: wire.update_time.unwrap_or_else(chrono::Utc::now),
        })
    }

    /// Creates a batch prediction job.
    ///
    /// Returns the created job's fully-qualified resource name.
    ///
    /// # Errors
    ///
    /// Returns an error if the job cannot be created.
    pub async fn create_batch_prediction_job(
        &self,
        display_name: &str,
        model_id: &str,
        input_config: BatchInputConfig,
        output_config: BatchOutputConfig,
    ) -> Result<String> {
        tracing::info!("Creating batch prediction job: {}", display_name);

        let url = format!(
            "{}/v1/{}/batchPredictionJobs",
            self.aiplatform_base_url,
            self.parent()
        );
        let body = serde_json::json!({
            "displayName": display_name,
            "model": self.qualify(model_id, "models"),
            "inputConfig": {
                "instancesFormat": input_config.format,
                "gcsSource": { "uris": input_config.gcs_source }
            },
            "outputConfig": {
                "predictionsFormat": "jsonl",
                "gcsDestination": { "outputUriPrefix": output_config.gcs_destination_prefix }
            }
        });

        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!(
                    "Vertex AI batchPredictionJobs.create request failed: {e}"
                ))
            })?;

        let wire: NamedResourceWire =
            parse_vertex_response(response, "create batch prediction job").await?;
        wire.name.ok_or_else(|| {
            CloudEnhancedError::ml_service(
                "Vertex AI batchPredictionJobs.create returned no job name".to_string(),
            )
        })
    }

    /// Lists models.
    ///
    /// # Errors
    ///
    /// Returns an error if the models cannot be listed.
    pub async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        tracing::info!("Listing models");

        let url = format!("{}/v1/{}/models", self.aiplatform_base_url, self.parent());
        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!("Vertex AI models.list request failed: {e}"))
            })?;

        let body: ListModelsResponse = parse_vertex_response(response, "list models").await?;
        Ok(body
            .models
            .into_iter()
            .map(|m| ModelInfo {
                _model_id: m.name.unwrap_or_default(),
                display_name: m.display_name.unwrap_or_default(),
                create_time: m.create_time.unwrap_or_else(chrono::Utc::now),
            })
            .collect())
    }

    /// Deletes a model.
    ///
    /// # Errors
    ///
    /// Returns an error if the model cannot be deleted.
    pub async fn delete_model(&self, model_id: &str) -> Result<()> {
        tracing::info!("Deleting model: {}", model_id);

        let model = self.qualify(model_id, "models");
        let url = format!("{}/v1/{}", self.aiplatform_base_url, model);
        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .delete(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!(
                    "Vertex AI models.delete request failed: {e}"
                ))
            })?;
        let operation: OperationWire = parse_vertex_response(response, "delete model").await?;
        self.drive_operation(operation, "delete model").await?;
        Ok(())
    }

    /// POSTs `body` to `url` and drives the returned long-running operation to
    /// completion, returning its `response` object.
    async fn await_operation(
        &self,
        url: &str,
        body: &serde_json::Value,
        action: &str,
    ) -> Result<serde_json::Value> {
        let token = self.bearer_token().await?;
        let response = self
            .http_client
            .post(url)
            .bearer_auth(&token)
            .json(body)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!("Vertex AI {action} request failed: {e}"))
            })?;

        let operation: OperationWire = parse_vertex_response(response, action).await?;
        self.drive_operation(operation, action).await
    }

    /// Polls `operation` until it reports `done`, returning its `response`
    /// object (or an error if the operation failed).
    async fn drive_operation(
        &self,
        mut operation: OperationWire,
        action: &str,
    ) -> Result<serde_json::Value> {
        let mut attempts = 0;
        loop {
            if let Some(error) = operation.error {
                return Err(CloudEnhancedError::ml_service(format!(
                    "Vertex AI operation for '{action}' failed with code {}: {}",
                    error.code, error.message
                )));
            }
            if operation.done {
                return Ok(operation.response.unwrap_or(serde_json::Value::Null));
            }
            let Some(name) = operation.name.clone() else {
                return Err(CloudEnhancedError::ml_service(format!(
                    "Vertex AI operation for '{action}' is not done and has no name to poll"
                )));
            };
            if attempts >= MAX_POLL_ATTEMPTS {
                return Err(CloudEnhancedError::timeout(format!(
                    "Vertex AI operation '{name}' for '{action}' did not complete after {MAX_POLL_ATTEMPTS} polls"
                )));
            }
            tokio::time::sleep(POLL_INTERVAL).await;

            let token = self.bearer_token().await?;
            let url = format!("{}/v1/{name}", self.aiplatform_base_url);
            let response = self
                .http_client
                .get(&url)
                .bearer_auth(&token)
                .send()
                .await
                .map_err(|e| {
                    CloudEnhancedError::ml_service(format!(
                        "Vertex AI operations.get request failed: {e}"
                    ))
                })?;
            operation = parse_vertex_response(response, "poll operation").await?;
            attempts += 1;
        }
    }
}

/// Verifies `response` is a success and deserializes its JSON body as `T`.
async fn parse_vertex_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
    action: &str,
) -> Result<T> {
    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<unreadable response body>".to_string());
        return Err(CloudEnhancedError::ml_service(format!(
            "Vertex AI API returned status {status} while trying to {action}: {body}"
        )));
    }

    response.json::<T>().await.map_err(|e| {
        CloudEnhancedError::ml_service(format!(
            "Failed to parse Vertex AI API response while trying to {action}: {e}"
        ))
    })
}

/// Normalizes a Vertex `PIPELINE_STATE_*` string into a [`PipelineState`].
fn normalize_pipeline_state(raw: &str) -> PipelineState {
    match raw {
        "PIPELINE_STATE_SUCCEEDED" => PipelineState::Succeeded,
        "PIPELINE_STATE_FAILED" => PipelineState::Failed,
        "PIPELINE_STATE_CANCELLED" => PipelineState::Cancelled,
        _ => PipelineState::Running,
    }
}

// ---------------------------------------------------------------------
// Wire (JSON) types for the Vertex AI REST API.
// ---------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct OperationWire {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    error: Option<OperationError>,
    #[serde(default)]
    response: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct OperationError {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
struct PredictResponse {
    #[serde(default)]
    predictions: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct TrainingPipelineWire {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(rename = "createTime", default)]
    create_time: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(rename = "updateTime", default)]
    update_time: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Deserialize)]
struct NamedResourceWire {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListModelsResponse {
    #[serde(default)]
    models: Vec<ModelWire>,
}

#[derive(Debug, Deserialize)]
struct ModelWire {
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "displayName", default)]
    display_name: Option<String>,
    #[serde(rename = "createTime", default)]
    create_time: Option<chrono::DateTime<chrono::Utc>>,
}

/// Model specification.
#[derive(Debug, Clone)]
pub struct ModelSpec {
    /// Display name
    pub display_name: String,
    /// Artifact URI
    pub artifact_uri: String,
    /// Serving container image URI
    pub serving_container_image_uri: String,
}

/// Pipeline status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStatus {
    /// Pipeline ID
    pub pipeline_id: String,
    /// Pipeline state
    pub state: PipelineState,
    /// Create time
    pub create_time: chrono::DateTime<chrono::Utc>,
    /// Update time
    pub update_time: chrono::DateTime<chrono::Utc>,
}

/// Pipeline state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PipelineState {
    /// Running
    Running,
    /// Succeeded
    Succeeded,
    /// Failed
    Failed,
    /// Cancelled
    Cancelled,
}

/// Batch input configuration.
#[derive(Debug, Clone)]
pub struct BatchInputConfig {
    /// GCS source URIs
    pub gcs_source: Vec<String>,
    /// Input format
    pub format: String,
}

/// Batch output configuration.
#[derive(Debug, Clone)]
pub struct BatchOutputConfig {
    /// GCS destination URI prefix
    pub gcs_destination_prefix: String,
}

/// Model information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Model ID
    pub _model_id: String,
    /// Display name
    pub display_name: String,
    /// Create time
    pub create_time: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
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

    async fn test_client(vertex_body: &str) -> VertexAiClient {
        let metadata_base = spawn_mock_server(
            "HTTP/1.1 200 OK",
            r#"{"access_token":"mock-token","expires_in":3600}"#.to_string(),
        )
        .await;
        let vertex_base = spawn_mock_server("HTTP/1.1 200 OK", vertex_body.to_string()).await;

        let config =
            crate::gcp::GcpConfig::new("test-project".to_string(), Some("us-central1".to_string()))
                .expect("gcp config");
        VertexAiClient::with_urls(&config, vertex_base, Some(metadata_base)).expect("client")
    }

    #[tokio::test]
    async fn test_create_endpoint_returns_real_name() {
        let client = test_client(
            r#"{"name":"projects/p/locations/l/operations/1","done":true,"response":{"name":"projects/p/locations/l/endpoints/999"}}"#,
        )
        .await;

        let name = client.create_endpoint("my-endpoint").await.expect("name");
        assert_eq!(name, "projects/p/locations/l/endpoints/999");
    }

    #[tokio::test]
    async fn test_upload_model_returns_model_name() {
        let client = test_client(
            r#"{"name":"projects/p/locations/l/operations/2","done":true,"response":{"model":"projects/p/locations/l/models/555"}}"#,
        )
        .await;

        let name = client
            .upload_model("m", "gs://bucket/model", "gcr.io/img")
            .await
            .expect("model name");
        assert_eq!(name, "projects/p/locations/l/models/555");
    }

    #[tokio::test]
    async fn test_operation_error_is_surfaced() {
        let client = test_client(
            r#"{"name":"projects/p/locations/l/operations/3","done":true,"error":{"code":7,"message":"permission denied"}}"#,
        )
        .await;

        let result = client.create_endpoint("my-endpoint").await;
        assert!(matches!(result, Err(CloudEnhancedError::MlService(_))));
    }

    #[tokio::test]
    async fn test_predict_parses_predictions() {
        let client = test_client(r#"{"predictions":[{"score":0.9},{"score":0.1}]}"#).await;

        let preds = client
            .predict("123", vec![serde_json::json!({"x": 1})])
            .await
            .expect("predictions");
        assert_eq!(preds.len(), 2);
    }

    #[tokio::test]
    async fn test_list_models_parses() {
        let client = test_client(
            r#"{"models":[{"name":"projects/p/locations/l/models/1","displayName":"m1","createTime":"2024-01-01T00:00:00Z"}]}"#,
        )
        .await;

        let models = client.list_models().await.expect("models");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].display_name, "m1");
    }

    #[tokio::test]
    async fn test_error_status_is_not_swallowed() {
        let metadata_base = spawn_mock_server(
            "HTTP/1.1 200 OK",
            r#"{"access_token":"mock-token","expires_in":3600}"#.to_string(),
        )
        .await;
        let vertex_base = spawn_mock_server(
            "HTTP/1.1 403 Forbidden",
            r#"{"error":{"message":"denied"}}"#.to_string(),
        )
        .await;
        let config = crate::gcp::GcpConfig::new("test-project".to_string(), None).expect("config");
        let client =
            VertexAiClient::with_urls(&config, vertex_base, Some(metadata_base)).expect("client");

        let result = client.list_models().await;
        assert!(result.is_err());
    }

    #[test]
    fn test_normalize_pipeline_state() {
        assert_eq!(
            normalize_pipeline_state("PIPELINE_STATE_SUCCEEDED"),
            PipelineState::Succeeded
        );
        assert_eq!(
            normalize_pipeline_state("PIPELINE_STATE_RUNNING"),
            PipelineState::Running
        );
    }

    #[test]
    fn test_pipeline_state() {
        assert_eq!(PipelineState::Running, PipelineState::Running);
        assert_ne!(PipelineState::Running, PipelineState::Succeeded);
    }
}
