//! Google Cloud Dataflow integration.
//!
//! Talks to the real Cloud Dataflow REST API
//! (<https://dataflow.googleapis.com/v1b3>). Authentication is obtained by
//! delegating to [`super::workload_identity::WorkloadIdentityClient`], which
//! already implements the GCE metadata server / IAM Credentials token flow.

use crate::error::{CloudEnhancedError, Result};
use crate::gcp::workload_identity::WorkloadIdentityClient;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::sleep;

/// Default base URL of the Cloud Dataflow API.
const DEFAULT_DATAFLOW_BASE_URL: &str = "https://dataflow.googleapis.com";

/// OAuth2 scope requested for calls to the Cloud Dataflow API.
const DATAFLOW_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

/// Dataflow client for pipeline management.
#[derive(Debug, Clone)]
pub struct DataflowClient {
    project_id: String,
    /// Base URL of the Cloud Dataflow API (overridable for tests).
    dataflow_base_url: String,
    http_client: reqwest::Client,
    /// Auth provider, reusing the GCE metadata / IAM Credentials token flow.
    identity: WorkloadIdentityClient,
}

impl DataflowClient {
    /// Returns the project ID.
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
}

impl DataflowClient {
    /// Creates a new Dataflow client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be created.
    pub fn new(config: &super::GcpConfig) -> Result<Self> {
        Self::with_urls(config, DEFAULT_DATAFLOW_BASE_URL, None::<String>)
    }

    /// Creates a new Dataflow client pointed at custom Dataflow API and
    /// (optionally) GCE metadata server base URLs.
    ///
    /// This is primarily intended for tests, which spin up local mock servers
    /// rather than talking to the real `dataflow.googleapis.com` and
    /// `metadata.google.internal` endpoints.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn with_urls(
        config: &super::GcpConfig,
        dataflow_base_url: impl Into<String>,
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
            dataflow_base_url: dataflow_base_url.into(),
            http_client,
            identity,
        })
    }

    /// Obtains a bearer token for authenticating to the Cloud Dataflow API,
    /// using the instance's attached service account.
    async fn bearer_token(&self) -> Result<String> {
        let token = self
            .identity
            .generate_access_token("default", vec![DATAFLOW_SCOPE.to_string()], 3600)
            .await?;
        Ok(token.access_token)
    }

    /// Launches a Dataflow job from a classic template.
    ///
    /// # Errors
    ///
    /// Returns an error if the job cannot be launched.
    pub async fn launch_template_job(
        &self,
        location: &str,
        template_gcs_path: &str,
        parameters: HashMap<String, String>,
    ) -> Result<String> {
        tracing::info!(
            "Launching Dataflow template job: {} in location: {} with {} parameters",
            template_gcs_path,
            location,
            parameters.len()
        );

        let token = self.bearer_token().await?;
        let url = format!(
            "{}/v1b3/projects/{}/locations/{}/templates:launch?gcsPath={}",
            self.dataflow_base_url,
            self.project_id,
            location,
            urlencode(template_gcs_path),
        );

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&token)
            .json(&LaunchTemplateRequest { parameters })
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::gcp_service(format!(
                    "Dataflow templates:launch request failed: {e}"
                ))
            })?;

        let body: LaunchJobResponse =
            parse_dataflow_response(response, "launch template job").await?;
        job_id_from(body)
    }

    /// Launches a Dataflow Flex template job.
    ///
    /// # Errors
    ///
    /// Returns an error if the job cannot be launched.
    pub async fn launch_flex_template_job(
        &self,
        location: &str,
        container_spec_gcs_path: &str,
        parameters: HashMap<String, String>,
    ) -> Result<String> {
        tracing::info!(
            "Launching Dataflow Flex template job: {} in location: {} with {} parameters",
            container_spec_gcs_path,
            location,
            parameters.len()
        );

        let token = self.bearer_token().await?;
        let url = format!(
            "{}/v1b3/projects/{}/locations/{}/flexTemplates:launch",
            self.dataflow_base_url, self.project_id, location,
        );

        let job_name = format!("job-{}", uuid::Uuid::new_v4());
        let request = LaunchFlexTemplateRequest {
            launch_parameter: FlexLaunchParameter {
                job_name,
                container_spec_gcs_path: container_spec_gcs_path.to_string(),
                parameters,
            },
        };

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&token)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::gcp_service(format!(
                    "Dataflow flexTemplates:launch request failed: {e}"
                ))
            })?;

        let body: LaunchJobResponse =
            parse_dataflow_response(response, "launch flex template job").await?;
        job_id_from(body)
    }

    /// Gets the status of a Dataflow job.
    ///
    /// # Errors
    ///
    /// Returns an error if the status cannot be retrieved.
    pub async fn get_job_status(&self, location: &str, job_id: &str) -> Result<JobStatus> {
        tracing::info!("Getting job status: {} in location: {}", job_id, location);

        let token = self.bearer_token().await?;
        let url = format!(
            "{}/v1b3/projects/{}/locations/{}/jobs/{}",
            self.dataflow_base_url, self.project_id, location, job_id,
        );

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::gcp_service(format!("Dataflow jobs.get request failed: {e}"))
            })?;

        let job: DataflowJob = parse_dataflow_response(response, "get job status").await?;
        Ok(job.into_status())
    }

    /// Waits for a job to complete.
    ///
    /// # Errors
    ///
    /// Returns an error if the job fails or times out.
    pub async fn wait_for_job(
        &self,
        location: &str,
        job_id: &str,
        poll_interval: Duration,
        timeout: Duration,
    ) -> Result<JobStatus> {
        let start = std::time::Instant::now();

        loop {
            let status = self.get_job_status(location, job_id).await?;

            match status.state {
                JobState::Done => return Ok(status),
                JobState::Failed | JobState::Cancelled => {
                    return Err(CloudEnhancedError::query_execution(format!(
                        "Dataflow job {} failed or was cancelled",
                        job_id
                    )));
                }
                JobState::Running | JobState::Queued => {
                    if start.elapsed() > timeout {
                        return Err(CloudEnhancedError::timeout(format!(
                            "Dataflow job {} timed out after {:?}",
                            job_id, timeout
                        )));
                    }
                    sleep(poll_interval).await;
                }
            }
        }
    }

    /// Cancels a running job.
    ///
    /// # Errors
    ///
    /// Returns an error if the job cannot be cancelled.
    pub async fn cancel_job(&self, location: &str, job_id: &str) -> Result<()> {
        tracing::info!("Cancelling job: {} in location: {}", job_id, location);
        self.update_job_state(location, job_id, "JOB_STATE_CANCELLED", "cancel job")
            .await
    }

    /// Drains a running job.
    ///
    /// # Errors
    ///
    /// Returns an error if the job cannot be drained.
    pub async fn drain_job(&self, location: &str, job_id: &str) -> Result<()> {
        tracing::info!("Draining job: {} in location: {}", job_id, location);
        self.update_job_state(location, job_id, "JOB_STATE_DRAINED", "drain job")
            .await
    }

    /// Issues a `jobs.update` `PUT` transitioning `job_id` to `requested_state`.
    async fn update_job_state(
        &self,
        location: &str,
        job_id: &str,
        requested_state: &str,
        action: &str,
    ) -> Result<()> {
        let token = self.bearer_token().await?;
        let url = format!(
            "{}/v1b3/projects/{}/locations/{}/jobs/{}",
            self.dataflow_base_url, self.project_id, location, job_id,
        );

        let response = self
            .http_client
            .put(&url)
            .bearer_auth(&token)
            .json(&UpdateJobRequest {
                requested_state: requested_state.to_string(),
            })
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::gcp_service(format!("Dataflow jobs.update request failed: {e}"))
            })?;

        parse_dataflow_response::<DataflowJob>(response, action).await?;
        Ok(())
    }

    /// Lists jobs in a project location.
    ///
    /// # Errors
    ///
    /// Returns an error if the jobs cannot be listed.
    pub async fn list_jobs(&self, location: &str, filter: Option<&str>) -> Result<Vec<JobInfo>> {
        tracing::info!(
            "Listing jobs in location: {} with filter: {:?}",
            location,
            filter
        );

        let token = self.bearer_token().await?;
        let mut url = format!(
            "{}/v1b3/projects/{}/locations/{}/jobs",
            self.dataflow_base_url, self.project_id, location,
        );
        if let Some(filter) = filter {
            url.push_str("?filter=");
            url.push_str(&urlencode(filter));
        }

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::gcp_service(format!("Dataflow jobs.list request failed: {e}"))
            })?;

        let body: ListJobsResponse = parse_dataflow_response(response, "list jobs").await?;
        Ok(body.jobs.into_iter().map(DataflowJob::into_info).collect())
    }

    /// Gets job metrics.
    ///
    /// # Errors
    ///
    /// Returns an error if the metrics cannot be retrieved.
    pub async fn get_job_metrics(&self, location: &str, job_id: &str) -> Result<JobMetrics> {
        tracing::info!(
            "Getting metrics for job: {} in location: {}",
            job_id,
            location
        );

        let token = self.bearer_token().await?;
        let url = format!(
            "{}/v1b3/projects/{}/locations/{}/jobs/{}/metrics",
            self.dataflow_base_url, self.project_id, location, job_id,
        );

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::gcp_service(format!(
                    "Dataflow jobs.getMetrics request failed: {e}"
                ))
            })?;

        let body: JobMetricsResponse = parse_dataflow_response(response, "get job metrics").await?;

        let mut metrics = HashMap::new();
        for metric in body.metrics {
            if let (Some(name), Some(value)) = (
                metric.name.and_then(|n| n.name),
                metric.scalar.and_then(|s| s.as_f64()),
            ) {
                metrics.insert(name, value);
            }
        }

        Ok(JobMetrics {
            job_id: job_id.to_string(),
            metrics,
        })
    }
}

/// Extracts the launched job's ID from a launch response, erroring if Dataflow
/// returned no job (rather than fabricating an ID).
fn job_id_from(response: LaunchJobResponse) -> Result<String> {
    response.job.and_then(|job| job.id).ok_or_else(|| {
        CloudEnhancedError::gcp_service(
            "Dataflow launch response did not contain a job id".to_string(),
        )
    })
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

/// Verifies `response` is a success and deserializes its JSON body as `T`.
async fn parse_dataflow_response<T: for<'de> Deserialize<'de>>(
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
            "Dataflow API returned status {status} while trying to {action}: {body}"
        )));
    }

    response.json::<T>().await.map_err(|e| {
        CloudEnhancedError::gcp_service(format!(
            "Failed to parse Dataflow API response while trying to {action}: {e}"
        ))
    })
}

/// Normalizes a Dataflow `JOB_STATE_*` string into a [`JobState`].
fn normalize_state(raw: &str) -> JobState {
    match raw {
        "JOB_STATE_QUEUED" | "JOB_STATE_PENDING" => JobState::Queued,
        "JOB_STATE_DONE" | "JOB_STATE_DRAINED" | "JOB_STATE_UPDATED" | "JOB_STATE_STOPPED" => {
            JobState::Done
        }
        "JOB_STATE_FAILED" => JobState::Failed,
        "JOB_STATE_CANCELLED" => JobState::Cancelled,
        // RUNNING, DRAINING, CANCELLING and any unknown-but-active state.
        _ => JobState::Running,
    }
}

// ---------------------------------------------------------------------
// Wire (JSON) types for the Cloud Dataflow REST API.
// ---------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct LaunchTemplateRequest {
    parameters: HashMap<String, String>,
}

#[derive(Debug, Serialize)]
struct LaunchFlexTemplateRequest {
    #[serde(rename = "launchParameter")]
    launch_parameter: FlexLaunchParameter,
}

#[derive(Debug, Serialize)]
struct FlexLaunchParameter {
    #[serde(rename = "jobName")]
    job_name: String,
    #[serde(rename = "containerSpecGcsPath")]
    container_spec_gcs_path: String,
    parameters: HashMap<String, String>,
}

#[derive(Debug, Serialize)]
struct UpdateJobRequest {
    #[serde(rename = "requestedState")]
    requested_state: String,
}

#[derive(Debug, Deserialize)]
struct LaunchJobResponse {
    #[serde(default)]
    job: Option<DataflowJob>,
}

#[derive(Debug, Deserialize)]
struct ListJobsResponse {
    #[serde(default)]
    jobs: Vec<DataflowJob>,
}

#[derive(Debug, Deserialize)]
struct DataflowJob {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "currentState", default)]
    current_state: Option<String>,
    #[serde(rename = "currentStateTime", default)]
    current_state_time: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(rename = "createTime", default)]
    create_time: Option<chrono::DateTime<chrono::Utc>>,
}

impl DataflowJob {
    fn into_status(self) -> JobStatus {
        JobStatus {
            job_id: self.id.unwrap_or_default(),
            state: self
                .current_state
                .as_deref()
                .map(normalize_state)
                .unwrap_or(JobState::Running),
            current_state_time: self.current_state_time.unwrap_or_else(chrono::Utc::now),
        }
    }

    fn into_info(self) -> JobInfo {
        JobInfo {
            job_id: self.id.unwrap_or_default(),
            name: self.name.unwrap_or_default(),
            state: self
                .current_state
                .as_deref()
                .map(normalize_state)
                .unwrap_or(JobState::Running),
            create_time: self.create_time.unwrap_or_else(chrono::Utc::now),
        }
    }
}

#[derive(Debug, Deserialize)]
struct JobMetricsResponse {
    #[serde(default)]
    metrics: Vec<MetricUpdateWire>,
}

#[derive(Debug, Deserialize)]
struct MetricUpdateWire {
    #[serde(default)]
    name: Option<MetricStructuredNameWire>,
    #[serde(default)]
    scalar: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct MetricStructuredNameWire {
    #[serde(default)]
    name: Option<String>,
}

/// Job status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobStatus {
    /// Job ID
    pub job_id: String,
    /// Job state
    pub state: JobState,
    /// Current state time
    pub current_state_time: chrono::DateTime<chrono::Utc>,
}

/// Job state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobState {
    /// Queued
    Queued,
    /// Running
    Running,
    /// Done
    Done,
    /// Failed
    Failed,
    /// Cancelled
    Cancelled,
}

/// Job information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobInfo {
    /// Job ID
    pub job_id: String,
    /// Job name
    pub name: String,
    /// Job state
    pub state: JobState,
    /// Create time
    pub create_time: chrono::DateTime<chrono::Utc>,
}

/// Job metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobMetrics {
    /// Job ID
    pub job_id: String,
    /// Metrics
    pub metrics: HashMap<String, f64>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Spawns a minimal HTTP/1.1 mock server on an ephemeral local port that
    /// replies to every accepted connection with `body`.
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

    async fn test_client(dataflow_body: &str) -> DataflowClient {
        let metadata_base = spawn_mock_server(
            "HTTP/1.1 200 OK",
            r#"{"access_token":"mock-token","expires_in":3600}"#.to_string(),
        )
        .await;
        let dataflow_base = spawn_mock_server("HTTP/1.1 200 OK", dataflow_body.to_string()).await;

        let config =
            crate::gcp::GcpConfig::new("test-project".to_string(), None).expect("gcp config");
        DataflowClient::with_urls(&config, dataflow_base, Some(metadata_base)).expect("client")
    }

    #[tokio::test]
    async fn test_launch_template_job_returns_real_id() {
        let client =
            test_client(r#"{"job":{"id":"2024-01-01_job","currentState":"JOB_STATE_QUEUED"}}"#)
                .await;

        let id = client
            .launch_template_job("us-central1", "gs://bucket/tmpl", HashMap::new())
            .await
            .expect("job id");

        assert_eq!(id, "2024-01-01_job");
    }

    #[tokio::test]
    async fn test_launch_template_job_without_job_errors() {
        let client = test_client(r#"{}"#).await;

        let result = client
            .launch_template_job("us-central1", "gs://bucket/tmpl", HashMap::new())
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_job_status_parses_state() {
        let client = test_client(
            r#"{"id":"job-1","currentState":"JOB_STATE_RUNNING","currentStateTime":"2024-01-01T00:00:00Z"}"#,
        )
        .await;

        let status = client
            .get_job_status("us-central1", "job-1")
            .await
            .expect("status");

        assert_eq!(status.job_id, "job-1");
        assert_eq!(status.state, JobState::Running);
    }

    #[tokio::test]
    async fn test_list_jobs_parses() {
        let client = test_client(
            r#"{"jobs":[{"id":"a","name":"job-a","currentState":"JOB_STATE_DONE","createTime":"2024-01-01T00:00:00Z"}]}"#,
        )
        .await;

        let jobs = client.list_jobs("us-central1", None).await.expect("jobs");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].name, "job-a");
        assert_eq!(jobs[0].state, JobState::Done);
    }

    #[tokio::test]
    async fn test_get_job_metrics_parses_scalars() {
        let client = test_client(
            r#"{"metrics":[{"name":{"name":"ElementCount"},"scalar":42},{"name":{"name":"NoScalar"}}]}"#,
        )
        .await;

        let metrics = client
            .get_job_metrics("us-central1", "job-1")
            .await
            .expect("metrics");

        assert_eq!(metrics.metrics.get("ElementCount"), Some(&42.0));
        assert!(!metrics.metrics.contains_key("NoScalar"));
    }

    #[tokio::test]
    async fn test_error_status_is_not_swallowed() {
        let metadata_base = spawn_mock_server(
            "HTTP/1.1 200 OK",
            r#"{"access_token":"mock-token","expires_in":3600}"#.to_string(),
        )
        .await;
        let dataflow_base = spawn_mock_server(
            "HTTP/1.1 404 Not Found",
            r#"{"error":{"message":"job not found"}}"#.to_string(),
        )
        .await;
        let config =
            crate::gcp::GcpConfig::new("test-project".to_string(), None).expect("gcp config");
        let client =
            DataflowClient::with_urls(&config, dataflow_base, Some(metadata_base)).expect("client");

        let result = client.get_job_status("us-central1", "missing").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_normalize_state() {
        assert_eq!(normalize_state("JOB_STATE_RUNNING"), JobState::Running);
        assert_eq!(normalize_state("JOB_STATE_DONE"), JobState::Done);
        assert_eq!(normalize_state("JOB_STATE_DRAINED"), JobState::Done);
        assert_eq!(normalize_state("JOB_STATE_FAILED"), JobState::Failed);
        assert_eq!(normalize_state("JOB_STATE_CANCELLED"), JobState::Cancelled);
        assert_eq!(normalize_state("JOB_STATE_QUEUED"), JobState::Queued);
    }

    #[test]
    fn test_urlencode() {
        assert_eq!(urlencode("gs://bucket/a b"), "gs%3A%2F%2Fbucket%2Fa%20b");
    }

    #[test]
    fn test_job_state() {
        assert_eq!(JobState::Running, JobState::Running);
        assert_ne!(JobState::Running, JobState::Done);
    }
}
