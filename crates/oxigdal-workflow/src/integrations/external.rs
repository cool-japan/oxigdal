//! External integrations module for workflow orchestration.
//!
//! This module provides comprehensive integration capabilities with external systems:
//! - HTTP/REST API integration
//! - Message queue integration (trait-based)
//! - Database integration (trait-based)
//! - Cloud storage integration
//! - External service callbacks
//! - Webhook support
//! - Event emission to external systems

use crate::engine::state::{TaskState, WorkflowState};
use crate::error::{Result, WorkflowError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

// =============================================================================
// HTTP/REST API Integration
// =============================================================================

/// HTTP client configuration for REST API integration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpClientConfig {
    /// Base URL for API requests.
    pub base_url: String,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
    /// Maximum number of retries.
    pub max_retries: u32,
    /// Retry delay in milliseconds.
    pub retry_delay_ms: u64,
    /// Default headers for all requests.
    pub default_headers: HashMap<String, String>,
    /// Authentication configuration.
    pub auth: Option<HttpAuth>,
    /// Enable request/response logging.
    pub enable_logging: bool,
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            timeout_secs: 30,
            max_retries: 3,
            retry_delay_ms: 1000,
            default_headers: HashMap::new(),
            auth: None,
            enable_logging: false,
        }
    }
}

/// HTTP authentication configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HttpAuth {
    /// Bearer token authentication.
    Bearer { token: String },
    /// Basic authentication.
    Basic { username: String, password: String },
    /// API key authentication.
    ApiKey { header_name: String, key: String },
    /// Custom header authentication.
    Custom { headers: HashMap<String, String> },
}

/// HTTP request builder for REST API calls.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    /// HTTP method.
    pub method: HttpMethod,
    /// Request path (appended to base URL).
    pub path: String,
    /// Query parameters.
    pub query_params: HashMap<String, String>,
    /// Request headers.
    pub headers: HashMap<String, String>,
    /// Request body (JSON).
    pub body: Option<serde_json::Value>,
    /// Request timeout override.
    pub timeout: Option<Duration>,
}

/// HTTP method enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpMethod {
    /// GET request.
    Get,
    /// POST request.
    Post,
    /// PUT request.
    Put,
    /// PATCH request.
    Patch,
    /// DELETE request.
    Delete,
    /// HEAD request.
    Head,
    /// OPTIONS request.
    Options,
}

impl HttpMethod {
    /// Get string representation of HTTP method.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
        }
    }
}

/// HTTP response from REST API calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    /// HTTP status code.
    pub status_code: u16,
    /// Response headers.
    pub headers: HashMap<String, String>,
    /// Response body.
    pub body: Option<serde_json::Value>,
    /// Response time in milliseconds.
    pub response_time_ms: u64,
    /// Whether the request was successful (2xx status).
    pub is_success: bool,
}

impl HttpResponse {
    /// Check if response indicates success.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status_code)
    }

    /// Check if response indicates client error.
    pub fn is_client_error(&self) -> bool {
        (400..500).contains(&self.status_code)
    }

    /// Check if response indicates server error.
    pub fn is_server_error(&self) -> bool {
        (500..600).contains(&self.status_code)
    }
}

/// REST API client for external integrations.
#[cfg(feature = "integrations")]
pub struct RestApiClient {
    config: HttpClientConfig,
    client: reqwest::Client,
}

#[cfg(feature = "integrations")]
impl RestApiClient {
    /// Create a new REST API client.
    pub fn new(config: HttpClientConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| WorkflowError::integration("rest", format!("Failed to create client: {}", e)))?;

        Ok(Self { config, client })
    }

    /// Execute an HTTP request.
    pub async fn execute(&self, request: HttpRequest) -> Result<HttpResponse> {
        let url = if request.path.starts_with("http") {
            request.path.clone()
        } else {
            format!("{}{}", self.config.base_url, request.path)
        };

        let start_time = std::time::Instant::now();
        let mut last_error = None;

        for attempt in 0..=self.config.max_retries {
            if attempt > 0 {
                debug!("Retrying request (attempt {})", attempt + 1);
                tokio::time::sleep(Duration::from_millis(
                    self.config.retry_delay_ms * (1 << attempt.min(5)),
                ))
                .await;
            }

            match self.do_request(&url, &request).await {
                Ok(response) => {
                    if self.config.enable_logging {
                        info!(
                            "HTTP {} {} -> {} ({}ms)",
                            request.method.as_str(),
                            url,
                            response.status_code,
                            response.response_time_ms
                        );
                    }
                    return Ok(response);
                }
                Err(e) => {
                    last_error = Some(e);
                    if attempt < self.config.max_retries {
                        warn!("Request failed, will retry: {:?}", last_error);
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            WorkflowError::integration("rest", "Request failed after all retries")
        }))
    }

    async fn do_request(&self, url: &str, request: &HttpRequest) -> Result<HttpResponse> {
        let start_time = std::time::Instant::now();

        let mut req_builder = match request.method {
            HttpMethod::Get => self.client.get(url),
            HttpMethod::Post => self.client.post(url),
            HttpMethod::Put => self.client.put(url),
            HttpMethod::Patch => self.client.patch(url),
            HttpMethod::Delete => self.client.delete(url),
            HttpMethod::Head => self.client.head(url),
            HttpMethod::Options => self.client.request(reqwest::Method::OPTIONS, url),
        };

        // Add default headers
        for (key, value) in &self.config.default_headers {
            req_builder = req_builder.header(key, value);
        }

        // Add request-specific headers
        for (key, value) in &request.headers {
            req_builder = req_builder.header(key, value);
        }

        // Add query parameters
        if !request.query_params.is_empty() {
            req_builder = req_builder.query(&request.query_params);
        }

        // Add authentication
        if let Some(auth) = &self.config.auth {
            req_builder = match auth {
                HttpAuth::Bearer { token } => req_builder.bearer_auth(token),
                HttpAuth::Basic { username, password } => {
                    req_builder.basic_auth(username, Some(password))
                }
                HttpAuth::ApiKey { header_name, key } => req_builder.header(header_name, key),
                HttpAuth::Custom { headers } => {
                    for (k, v) in headers {
                        req_builder = req_builder.header(k, v);
                    }
                    req_builder
                }
            };
        }

        // Add body
        if let Some(body) = &request.body {
            req_builder = req_builder.json(body);
        }

        // Set timeout override
        if let Some(timeout) = request.timeout {
            req_builder = req_builder.timeout(timeout);
        }

        // Send request
        let response = req_builder
            .send()
            .await
            .map_err(|e| WorkflowError::integration("rest", format!("Request failed: {}", e)))?;

        let status_code = response.status().as_u16();
        let mut headers = HashMap::new();
        for (key, value) in response.headers() {
            if let Ok(v) = value.to_str() {
                headers.insert(key.to_string(), v.to_string());
            }
        }

        let body = response
            .json::<serde_json::Value>()
            .await
            .ok();

        let response_time_ms = start_time.elapsed().as_millis() as u64;

        Ok(HttpResponse {
            is_success: (200..300).contains(&status_code),
            status_code,
            headers,
            body,
            response_time_ms,
        })
    }
}

// =============================================================================
// Message Queue Integration (Trait-based)
// =============================================================================

/// Message for queue communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueMessage {
    /// Unique message ID.
    pub id: String,
    /// Message topic/queue name.
    pub topic: String,
    /// Message payload.
    pub payload: serde_json::Value,
    /// Message headers/metadata.
    pub headers: HashMap<String, String>,
    /// Message timestamp.
    pub timestamp: DateTime<Utc>,
    /// Message priority (0-9, higher is more important).
    pub priority: u8,
    /// Correlation ID for request-response patterns.
    pub correlation_id: Option<String>,
    /// Reply-to topic for response.
    pub reply_to: Option<String>,
}

impl QueueMessage {
    /// Create a new queue message.
    pub fn new(topic: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            topic: topic.into(),
            payload,
            headers: HashMap::new(),
            timestamp: Utc::now(),
            priority: 5,
            correlation_id: None,
            reply_to: None,
        }
    }

    /// Set message correlation ID.
    pub fn with_correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    /// Set reply-to topic.
    pub fn with_reply_to(mut self, reply_to: impl Into<String>) -> Self {
        self.reply_to = Some(reply_to.into());
        self
    }

    /// Set message priority.
    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority.min(9);
        self
    }

    /// Add a header to the message.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }
}

/// Message acknowledgment status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckStatus {
    /// Message processed successfully.
    Ack,
    /// Message processing failed, should be retried.
    Nack,
    /// Message should be rejected (dead-lettered).
    Reject,
}

/// Message queue producer trait.
#[async_trait]
pub trait MessageQueueProducer: Send + Sync {
    /// Send a message to the queue.
    async fn send(&self, message: QueueMessage) -> Result<()>;

    /// Send multiple messages in a batch.
    async fn send_batch(&self, messages: Vec<QueueMessage>) -> Result<Vec<Result<()>>> {
        let mut results = Vec::with_capacity(messages.len());
        for msg in messages {
            results.push(self.send(msg).await);
        }
        Ok(results)
    }

    /// Flush pending messages.
    async fn flush(&self) -> Result<()>;
}

/// Message queue consumer trait.
#[async_trait]
pub trait MessageQueueConsumer: Send + Sync {
    /// Subscribe to a topic.
    async fn subscribe(&self, topic: &str) -> Result<()>;

    /// Unsubscribe from a topic.
    async fn unsubscribe(&self, topic: &str) -> Result<()>;

    /// Receive a message (blocking with timeout).
    async fn receive(&self, timeout: Duration) -> Result<Option<QueueMessage>>;

    /// Acknowledge a message.
    async fn acknowledge(&self, message_id: &str, status: AckStatus) -> Result<()>;

    /// Commit consumer offset.
    async fn commit(&self) -> Result<()>;
}

/// Message queue configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageQueueConfig {
    /// Queue type.
    pub queue_type: MessageQueueType,
    /// Connection endpoints.
    pub endpoints: Vec<String>,
    /// Authentication configuration.
    pub auth: Option<QueueAuth>,
    /// Consumer group ID.
    pub consumer_group: Option<String>,
    /// Message retention period in seconds.
    pub retention_secs: u64,
    /// Maximum message size in bytes.
    pub max_message_size: usize,
    /// Enable message compression.
    pub compression: bool,
}

/// Message queue type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageQueueType {
    /// Apache Kafka.
    Kafka,
    /// RabbitMQ.
    RabbitMq,
    /// Amazon SQS.
    Sqs,
    /// Google Cloud Pub/Sub.
    PubSub,
    /// Azure Service Bus.
    ServiceBus,
    /// Redis Streams.
    Redis,
}

/// Queue authentication configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueueAuth {
    /// Username/password authentication.
    Credentials { username: String, password: String },
    /// API key authentication.
    ApiKey { key: String },
    /// OAuth2 authentication.
    OAuth2 { client_id: String, client_secret: String, token_url: String },
    /// TLS/mTLS authentication.
    Tls { cert_path: String, key_path: String, ca_path: Option<String> },
}

// =============================================================================
// Database Integration (Trait-based)
// =============================================================================

/// Database query result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    /// Number of rows affected.
    pub rows_affected: u64,
    /// Column names.
    pub columns: Vec<String>,
    /// Row data.
    pub rows: Vec<Vec<serde_json::Value>>,
    /// Query execution time in milliseconds.
    pub execution_time_ms: u64,
}

impl QueryResult {
    /// Create an empty query result.
    pub fn empty() -> Self {
        Self {
            rows_affected: 0,
            columns: Vec::new(),
            rows: Vec::new(),
            execution_time_ms: 0,
        }
    }

    /// Get the number of rows.
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Check if the result is empty.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Database connection trait.
#[async_trait]
pub trait DatabaseConnection: Send + Sync {
    /// Execute a query and return results.
    async fn query(&self, sql: &str, params: &[serde_json::Value]) -> Result<QueryResult>;

    /// Execute a statement (INSERT, UPDATE, DELETE).
    async fn execute(&self, sql: &str, params: &[serde_json::Value]) -> Result<u64>;

    /// Begin a transaction.
    async fn begin_transaction(&self) -> Result<Box<dyn DatabaseTransaction>>;

    /// Check connection health.
    async fn ping(&self) -> Result<()>;

    /// Close the connection.
    async fn close(&self) -> Result<()>;
}

/// Database transaction trait.
#[async_trait]
pub trait DatabaseTransaction: Send + Sync {
    /// Execute a query within the transaction.
    async fn query(&self, sql: &str, params: &[serde_json::Value]) -> Result<QueryResult>;

    /// Execute a statement within the transaction.
    async fn execute(&self, sql: &str, params: &[serde_json::Value]) -> Result<u64>;

    /// Commit the transaction.
    async fn commit(self: Box<Self>) -> Result<()>;

    /// Rollback the transaction.
    async fn rollback(self: Box<Self>) -> Result<()>;
}

/// Database configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Database type.
    pub db_type: DatabaseType,
    /// Connection string or host.
    pub connection_string: String,
    /// Database name.
    pub database: Option<String>,
    /// Connection pool minimum size.
    pub pool_min: u32,
    /// Connection pool maximum size.
    pub pool_max: u32,
    /// Connection timeout in seconds.
    pub connect_timeout_secs: u64,
    /// Query timeout in seconds.
    pub query_timeout_secs: u64,
    /// Enable SSL/TLS.
    pub ssl: bool,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            db_type: DatabaseType::PostgreSql,
            connection_string: String::new(),
            database: None,
            pool_min: 1,
            pool_max: 10,
            connect_timeout_secs: 30,
            query_timeout_secs: 60,
            ssl: false,
        }
    }
}

/// Database type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DatabaseType {
    /// PostgreSQL.
    PostgreSql,
    /// MySQL.
    MySql,
    /// SQLite.
    Sqlite,
    /// Microsoft SQL Server.
    MsSql,
    /// MongoDB (document store).
    MongoDb,
    /// ClickHouse.
    ClickHouse,
}

// =============================================================================
// Cloud Storage Integration
// =============================================================================

/// Cloud storage object metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMetadata {
    /// Object key/path.
    pub key: String,
    /// Object size in bytes.
    pub size: u64,
    /// Content type.
    pub content_type: Option<String>,
    /// Last modified timestamp.
    pub last_modified: Option<DateTime<Utc>>,
    /// ETag/checksum.
    pub etag: Option<String>,
    /// Custom metadata.
    pub metadata: HashMap<String, String>,
}

/// Cloud storage upload options.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UploadOptions {
    /// Content type.
    pub content_type: Option<String>,
    /// Content encoding.
    pub content_encoding: Option<String>,
    /// Cache control header.
    pub cache_control: Option<String>,
    /// Custom metadata.
    pub metadata: HashMap<String, String>,
    /// Server-side encryption.
    pub encryption: Option<StorageEncryption>,
    /// Storage class.
    pub storage_class: Option<String>,
}

/// Server-side encryption configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageEncryption {
    /// Server-managed keys.
    ServerManaged,
    /// Customer-managed keys.
    CustomerManaged { key_id: String },
}

/// Cloud storage trait.
#[async_trait]
pub trait CloudStorage: Send + Sync {
    /// Upload data to storage.
    async fn upload(&self, key: &str, data: &[u8], options: UploadOptions) -> Result<ObjectMetadata>;

    /// Download data from storage.
    async fn download(&self, key: &str) -> Result<Vec<u8>>;

    /// Get object metadata.
    async fn head(&self, key: &str) -> Result<ObjectMetadata>;

    /// Delete an object.
    async fn delete(&self, key: &str) -> Result<()>;

    /// List objects with prefix.
    async fn list(&self, prefix: &str, max_keys: Option<usize>) -> Result<Vec<ObjectMetadata>>;

    /// Copy an object.
    async fn copy(&self, source_key: &str, dest_key: &str) -> Result<ObjectMetadata>;

    /// Generate a presigned URL for download.
    async fn presigned_download_url(&self, key: &str, expires_in: Duration) -> Result<String>;

    /// Generate a presigned URL for upload.
    async fn presigned_upload_url(&self, key: &str, expires_in: Duration) -> Result<String>;
}

/// Cloud storage configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudStorageConfig {
    /// Storage provider.
    pub provider: StorageProvider,
    /// Bucket/container name.
    pub bucket: String,
    /// Region.
    pub region: Option<String>,
    /// Endpoint override (for S3-compatible storage).
    pub endpoint: Option<String>,
    /// Access credentials.
    pub credentials: Option<StorageCredentials>,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
}

/// Storage provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageProvider {
    /// Amazon S3.
    S3,
    /// Google Cloud Storage.
    Gcs,
    /// Azure Blob Storage.
    AzureBlob,
    /// MinIO (S3-compatible).
    Minio,
}

/// Storage credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageCredentials {
    /// Access key credentials.
    AccessKey { access_key: String, secret_key: String },
    /// Service account credentials.
    ServiceAccount { key_file: String },
    /// Instance profile (AWS) or managed identity (Azure).
    InstanceProfile,
}

// =============================================================================
// External Service Callbacks
// =============================================================================

/// Callback event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallbackEventType {
    /// Workflow started.
    WorkflowStarted,
    /// Workflow completed.
    WorkflowCompleted,
    /// Workflow failed.
    WorkflowFailed,
    /// Workflow cancelled.
    WorkflowCancelled,
    /// Task started.
    TaskStarted,
    /// Task completed.
    TaskCompleted,
    /// Task failed.
    TaskFailed,
    /// Task retry.
    TaskRetry,
    /// Custom event.
    Custom,
}

impl CallbackEventType {
    /// Get string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::WorkflowStarted => "workflow.started",
            Self::WorkflowCompleted => "workflow.completed",
            Self::WorkflowFailed => "workflow.failed",
            Self::WorkflowCancelled => "workflow.cancelled",
            Self::TaskStarted => "task.started",
            Self::TaskCompleted => "task.completed",
            Self::TaskFailed => "task.failed",
            Self::TaskRetry => "task.retry",
            Self::Custom => "custom",
        }
    }
}

/// Callback payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackPayload {
    /// Event type.
    pub event_type: CallbackEventType,
    /// Workflow ID.
    pub workflow_id: String,
    /// Execution ID.
    pub execution_id: String,
    /// Task ID (if applicable).
    pub task_id: Option<String>,
    /// Event timestamp.
    pub timestamp: DateTime<Utc>,
    /// Event data.
    pub data: serde_json::Value,
    /// Workflow state snapshot (optional).
    pub state_snapshot: Option<serde_json::Value>,
}

impl CallbackPayload {
    /// Create a new callback payload for workflow events.
    pub fn for_workflow(
        event_type: CallbackEventType,
        workflow_id: impl Into<String>,
        execution_id: impl Into<String>,
        data: serde_json::Value,
    ) -> Self {
        Self {
            event_type,
            workflow_id: workflow_id.into(),
            execution_id: execution_id.into(),
            task_id: None,
            timestamp: Utc::now(),
            data,
            state_snapshot: None,
        }
    }

    /// Create a new callback payload for task events.
    pub fn for_task(
        event_type: CallbackEventType,
        workflow_id: impl Into<String>,
        execution_id: impl Into<String>,
        task_id: impl Into<String>,
        data: serde_json::Value,
    ) -> Self {
        Self {
            event_type,
            workflow_id: workflow_id.into(),
            execution_id: execution_id.into(),
            task_id: Some(task_id.into()),
            timestamp: Utc::now(),
            data,
            state_snapshot: None,
        }
    }

    /// Add state snapshot to the payload.
    pub fn with_state_snapshot(mut self, state: &WorkflowState) -> Self {
        self.state_snapshot = serde_json::to_value(state).ok();
        self
    }
}

/// Callback handler trait.
#[async_trait]
pub trait CallbackHandler: Send + Sync {
    /// Handle a callback event.
    async fn handle(&self, payload: CallbackPayload) -> Result<()>;

    /// Get the handler name.
    fn name(&self) -> &str;

    /// Check if the handler is enabled for an event type.
    fn is_enabled_for(&self, event_type: CallbackEventType) -> bool;
}

/// Callback configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackConfig {
    /// Callback URL.
    pub url: String,
    /// HTTP method.
    pub method: HttpMethod,
    /// Headers to include.
    pub headers: HashMap<String, String>,
    /// Authentication.
    pub auth: Option<HttpAuth>,
    /// Enabled event types.
    pub enabled_events: Vec<CallbackEventType>,
    /// Include state snapshot.
    pub include_state: bool,
    /// Retry configuration.
    pub retry: RetryConfig,
}

/// Retry configuration for callbacks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of retries.
    pub max_retries: u32,
    /// Initial delay in milliseconds.
    pub initial_delay_ms: u64,
    /// Maximum delay in milliseconds.
    pub max_delay_ms: u64,
    /// Backoff multiplier.
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay_ms: 1000,
            max_delay_ms: 30000,
            backoff_multiplier: 2.0,
        }
    }
}

/// HTTP callback handler implementation.
pub struct HttpCallbackHandler {
    config: CallbackConfig,
    #[cfg(feature = "integrations")]
    client: reqwest::Client,
}

impl HttpCallbackHandler {
    /// Create a new HTTP callback handler.
    #[cfg(feature = "integrations")]
    pub fn new(config: CallbackConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| WorkflowError::integration("callback", format!("Failed to create client: {}", e)))?;

        Ok(Self { config, client })
    }

    /// Get the callback configuration.
    pub fn config(&self) -> &CallbackConfig {
        &self.config
    }
}

#[cfg(feature = "integrations")]
#[async_trait]
impl CallbackHandler for HttpCallbackHandler {
    async fn handle(&self, payload: CallbackPayload) -> Result<()> {
        let mut request = match self.config.method {
            HttpMethod::Get => self.client.get(&self.config.url),
            HttpMethod::Post => self.client.post(&self.config.url),
            HttpMethod::Put => self.client.put(&self.config.url),
            HttpMethod::Patch => self.client.patch(&self.config.url),
            HttpMethod::Delete => self.client.delete(&self.config.url),
            HttpMethod::Head => self.client.head(&self.config.url),
            HttpMethod::Options => self.client.request(reqwest::Method::OPTIONS, &self.config.url),
        };

        // Add headers
        for (key, value) in &self.config.headers {
            request = request.header(key, value);
        }

        // Add authentication
        if let Some(auth) = &self.config.auth {
            request = match auth {
                HttpAuth::Bearer { token } => request.bearer_auth(token),
                HttpAuth::Basic { username, password } => {
                    request.basic_auth(username, Some(password))
                }
                HttpAuth::ApiKey { header_name, key } => request.header(header_name, key),
                HttpAuth::Custom { headers } => {
                    for (k, v) in headers {
                        request = request.header(k, v);
                    }
                    request
                }
            };
        }

        let mut last_error = None;
        for attempt in 0..=self.config.retry.max_retries {
            if attempt > 0 {
                let delay = std::cmp::min(
                    (self.config.retry.initial_delay_ms as f64
                        * self.config.retry.backoff_multiplier.powi(attempt as i32))
                        as u64,
                    self.config.retry.max_delay_ms,
                );
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }

            // Clone request for retry
            let req = request.try_clone().ok_or_else(|| {
                WorkflowError::integration("callback", "Failed to clone request")
            })?;

            match req.json(&payload).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        debug!(
                            "Callback delivered successfully: {} -> {}",
                            payload.event_type.as_str(),
                            self.config.url
                        );
                        return Ok(());
                    } else if response.status().is_server_error() {
                        last_error = Some(WorkflowError::integration(
                            "callback",
                            format!("Server error: {}", response.status()),
                        ));
                    } else {
                        return Err(WorkflowError::integration(
                            "callback",
                            format!("Client error: {}", response.status()),
                        ));
                    }
                }
                Err(e) => {
                    last_error = Some(WorkflowError::integration(
                        "callback",
                        format!("Request failed: {}", e),
                    ));
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            WorkflowError::integration("callback", "Callback failed after all retries")
        }))
    }

    fn name(&self) -> &str {
        "http_callback"
    }

    fn is_enabled_for(&self, event_type: CallbackEventType) -> bool {
        self.config.enabled_events.contains(&event_type)
    }
}

// =============================================================================
// Webhook Support
// =============================================================================

/// Webhook receiver for incoming workflow triggers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookTrigger {
    /// Trigger ID.
    pub id: String,
    /// Workflow ID to trigger.
    pub workflow_id: String,
    /// Secret for HMAC validation.
    pub secret: Option<String>,
    /// Allowed source IPs.
    pub allowed_ips: Vec<String>,
    /// Parameter mapping from webhook payload.
    pub parameter_mapping: HashMap<String, String>,
    /// Whether the webhook is active.
    pub active: bool,
}

impl WebhookTrigger {
    /// Create a new webhook trigger.
    pub fn new(workflow_id: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            workflow_id: workflow_id.into(),
            secret: None,
            allowed_ips: Vec::new(),
            parameter_mapping: HashMap::new(),
            active: true,
        }
    }

    /// Set the webhook secret.
    pub fn with_secret(mut self, secret: impl Into<String>) -> Self {
        self.secret = Some(secret.into());
        self
    }

    /// Add allowed IP addresses.
    pub fn with_allowed_ips(mut self, ips: Vec<String>) -> Self {
        self.allowed_ips = ips;
        self
    }

    /// Add parameter mapping.
    pub fn with_parameter(mut self, webhook_field: impl Into<String>, workflow_param: impl Into<String>) -> Self {
        self.parameter_mapping.insert(webhook_field.into(), workflow_param.into());
        self
    }

    /// Validate HMAC signature.
    pub fn validate_signature(&self, payload: &[u8], signature: &str) -> bool {
        use std::fmt::Write;

        if let Some(secret) = &self.secret {
            // Simple HMAC-SHA256 validation (placeholder - would use actual crypto lib)
            // In production, use ring or similar for proper HMAC
            let expected = format!("sha256={}", hex_encode(payload, secret.as_bytes()));
            constant_time_compare(&expected, signature)
        } else {
            true // No secret configured, skip validation
        }
    }
}

/// Constant-time string comparison to prevent timing attacks.
fn constant_time_compare(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let result = a
        .bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y));
    result == 0
}

/// Simple hex encoding for signature validation.
fn hex_encode(data: &[u8], _key: &[u8]) -> String {
    // Placeholder implementation
    // In production, use proper HMAC-SHA256
    data.iter()
        .take(32)
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// Webhook registry for managing webhook endpoints.
pub struct WebhookRegistry {
    triggers: Arc<RwLock<HashMap<String, WebhookTrigger>>>,
}

impl WebhookRegistry {
    /// Create a new webhook registry.
    pub fn new() -> Self {
        Self {
            triggers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a webhook trigger.
    pub async fn register(&self, trigger: WebhookTrigger) -> String {
        let id = trigger.id.clone();
        let mut triggers = self.triggers.write().await;
        triggers.insert(id.clone(), trigger);
        info!("Registered webhook trigger: {}", id);
        id
    }

    /// Unregister a webhook trigger.
    pub async fn unregister(&self, trigger_id: &str) -> Option<WebhookTrigger> {
        let mut triggers = self.triggers.write().await;
        let removed = triggers.remove(trigger_id);
        if removed.is_some() {
            info!("Unregistered webhook trigger: {}", trigger_id);
        }
        removed
    }

    /// Get a webhook trigger by ID.
    pub async fn get(&self, trigger_id: &str) -> Option<WebhookTrigger> {
        let triggers = self.triggers.read().await;
        triggers.get(trigger_id).cloned()
    }

    /// List all webhook triggers.
    pub async fn list(&self) -> Vec<WebhookTrigger> {
        let triggers = self.triggers.read().await;
        triggers.values().cloned().collect()
    }

    /// Find triggers for a workflow.
    pub async fn find_by_workflow(&self, workflow_id: &str) -> Vec<WebhookTrigger> {
        let triggers = self.triggers.read().await;
        triggers
            .values()
            .filter(|t| t.workflow_id == workflow_id && t.active)
            .cloned()
            .collect()
    }
}

impl Default for WebhookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Event Emission to External Systems
// =============================================================================

/// External event for emission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalEvent {
    /// Event ID.
    pub id: String,
    /// Event type.
    pub event_type: String,
    /// Event source.
    pub source: String,
    /// Event subject.
    pub subject: Option<String>,
    /// Event timestamp.
    pub timestamp: DateTime<Utc>,
    /// Event data.
    pub data: serde_json::Value,
    /// Event metadata.
    pub metadata: HashMap<String, String>,
}

impl ExternalEvent {
    /// Create a new external event.
    pub fn new(event_type: impl Into<String>, source: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            event_type: event_type.into(),
            source: source.into(),
            subject: None,
            timestamp: Utc::now(),
            data,
            metadata: HashMap::new(),
        }
    }

    /// Set event subject.
    pub fn with_subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    /// Add metadata.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Create from workflow state.
    pub fn from_workflow_state(state: &WorkflowState, event_type: impl Into<String>) -> Self {
        Self::new(
            event_type,
            format!("workflow/{}", state.workflow_id),
            serde_json::json!({
                "workflow_id": state.workflow_id,
                "execution_id": state.execution_id,
                "status": format!("{:?}", state.status),
            }),
        )
        .with_subject(state.execution_id.clone())
    }

    /// Create from task state.
    pub fn from_task_state(workflow_id: &str, task: &TaskState, event_type: impl Into<String>) -> Self {
        Self::new(
            event_type,
            format!("workflow/{}/task/{}", workflow_id, task.task_id),
            serde_json::json!({
                "task_id": task.task_id,
                "status": format!("{:?}", task.status),
                "attempts": task.attempts,
            }),
        )
        .with_subject(task.task_id.clone())
    }
}

/// Event emitter trait.
#[async_trait]
pub trait EventEmitter: Send + Sync {
    /// Emit an event.
    async fn emit(&self, event: ExternalEvent) -> Result<()>;

    /// Emit multiple events.
    async fn emit_batch(&self, events: Vec<ExternalEvent>) -> Result<Vec<Result<()>>> {
        let mut results = Vec::with_capacity(events.len());
        for event in events {
            results.push(self.emit(event).await);
        }
        Ok(results)
    }

    /// Get emitter name.
    fn name(&self) -> &str;
}

/// Event emitter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEmitterConfig {
    /// Emitter type.
    pub emitter_type: EventEmitterType,
    /// Target endpoint/topic.
    pub target: String,
    /// Authentication.
    pub auth: Option<EmitterAuth>,
    /// Batch size for batched emission.
    pub batch_size: usize,
    /// Flush interval in milliseconds.
    pub flush_interval_ms: u64,
}

/// Event emitter type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventEmitterType {
    /// HTTP webhook.
    Webhook,
    /// Message queue.
    MessageQueue,
    /// CloudEvents.
    CloudEvents,
    /// Custom.
    Custom,
}

/// Emitter authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EmitterAuth {
    /// Bearer token.
    Bearer { token: String },
    /// API key.
    ApiKey { key: String },
    /// OAuth2.
    OAuth2 { client_id: String, client_secret: String },
}

/// Multi-emitter that broadcasts events to multiple targets.
pub struct MultiEventEmitter {
    emitters: Vec<Arc<dyn EventEmitter>>,
}

impl MultiEventEmitter {
    /// Create a new multi-emitter.
    pub fn new() -> Self {
        Self {
            emitters: Vec::new(),
        }
    }

    /// Add an emitter.
    pub fn add_emitter(&mut self, emitter: Arc<dyn EventEmitter>) {
        self.emitters.push(emitter);
    }

    /// Remove an emitter by name.
    pub fn remove_emitter(&mut self, name: &str) {
        self.emitters.retain(|e| e.name() != name);
    }

    /// Get the number of registered emitters.
    pub fn emitter_count(&self) -> usize {
        self.emitters.len()
    }
}

impl Default for MultiEventEmitter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventEmitter for MultiEventEmitter {
    async fn emit(&self, event: ExternalEvent) -> Result<()> {
        let mut errors = Vec::new();

        for emitter in &self.emitters {
            if let Err(e) = emitter.emit(event.clone()).await {
                error!("Failed to emit event via {}: {}", emitter.name(), e);
                errors.push(e);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(WorkflowError::integration(
                "multi_emitter",
                format!("Failed to emit to {} targets", errors.len()),
            ))
        }
    }

    fn name(&self) -> &str {
        "multi_emitter"
    }
}

// =============================================================================
// Integration Registry
// =============================================================================

/// External integration registry.
pub struct ExternalIntegrationRegistry {
    /// Registered callback handlers.
    callbacks: Arc<RwLock<Vec<Arc<dyn CallbackHandler>>>>,
    /// Webhook registry.
    webhooks: Arc<WebhookRegistry>,
    /// Event emitters.
    emitters: Arc<RwLock<MultiEventEmitter>>,
}

impl ExternalIntegrationRegistry {
    /// Create a new external integration registry.
    pub fn new() -> Self {
        Self {
            callbacks: Arc::new(RwLock::new(Vec::new())),
            webhooks: Arc::new(WebhookRegistry::new()),
            emitters: Arc::new(RwLock::new(MultiEventEmitter::new())),
        }
    }

    /// Register a callback handler.
    pub async fn register_callback(&self, handler: Arc<dyn CallbackHandler>) {
        let mut callbacks = self.callbacks.write().await;
        info!("Registering callback handler: {}", handler.name());
        callbacks.push(handler);
    }

    /// Dispatch a callback to all registered handlers.
    pub async fn dispatch_callback(&self, payload: CallbackPayload) -> Result<()> {
        let callbacks = self.callbacks.read().await;
        let mut errors = Vec::new();

        for handler in callbacks.iter() {
            if handler.is_enabled_for(payload.event_type) {
                if let Err(e) = handler.handle(payload.clone()).await {
                    error!("Callback handler {} failed: {}", handler.name(), e);
                    errors.push(e);
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(WorkflowError::integration(
                "callbacks",
                format!("{} callback handlers failed", errors.len()),
            ))
        }
    }

    /// Get the webhook registry.
    pub fn webhooks(&self) -> &Arc<WebhookRegistry> {
        &self.webhooks
    }

    /// Register an event emitter.
    pub async fn register_emitter(&self, emitter: Arc<dyn EventEmitter>) {
        let mut emitters = self.emitters.write().await;
        info!("Registering event emitter: {}", emitter.name());
        emitters.add_emitter(emitter);
    }

    /// Emit an event to all registered emitters.
    pub async fn emit_event(&self, event: ExternalEvent) -> Result<()> {
        let emitters = self.emitters.read().await;
        emitters.emit(event).await
    }

    /// Emit workflow started event.
    pub async fn emit_workflow_started(&self, state: &WorkflowState) -> Result<()> {
        let event = ExternalEvent::from_workflow_state(state, "workflow.started");
        let callback = CallbackPayload::for_workflow(
            CallbackEventType::WorkflowStarted,
            &state.workflow_id,
            &state.execution_id,
            serde_json::json!({"status": "started"}),
        );

        let emit_result = self.emit_event(event).await;
        let callback_result = self.dispatch_callback(callback).await;

        if emit_result.is_err() || callback_result.is_err() {
            warn!("Some integrations failed for workflow.started event");
        }

        Ok(())
    }

    /// Emit workflow completed event.
    pub async fn emit_workflow_completed(&self, state: &WorkflowState) -> Result<()> {
        let event = ExternalEvent::from_workflow_state(state, "workflow.completed");
        let callback = CallbackPayload::for_workflow(
            CallbackEventType::WorkflowCompleted,
            &state.workflow_id,
            &state.execution_id,
            serde_json::json!({"status": "completed"}),
        ).with_state_snapshot(state);

        let _ = self.emit_event(event).await;
        let _ = self.dispatch_callback(callback).await;

        Ok(())
    }

    /// Emit workflow failed event.
    pub async fn emit_workflow_failed(&self, state: &WorkflowState, error: &str) -> Result<()> {
        let event = ExternalEvent::from_workflow_state(state, "workflow.failed")
            .with_metadata("error", error);
        let callback = CallbackPayload::for_workflow(
            CallbackEventType::WorkflowFailed,
            &state.workflow_id,
            &state.execution_id,
            serde_json::json!({"status": "failed", "error": error}),
        ).with_state_snapshot(state);

        let _ = self.emit_event(event).await;
        let _ = self.dispatch_callback(callback).await;

        Ok(())
    }

    /// Emit task started event.
    pub async fn emit_task_started(&self, workflow_id: &str, task: &TaskState) -> Result<()> {
        let event = ExternalEvent::from_task_state(workflow_id, task, "task.started");
        let callback = CallbackPayload::for_task(
            CallbackEventType::TaskStarted,
            workflow_id,
            "",
            &task.task_id,
            serde_json::json!({"status": "started"}),
        );

        let _ = self.emit_event(event).await;
        let _ = self.dispatch_callback(callback).await;

        Ok(())
    }

    /// Emit task completed event.
    pub async fn emit_task_completed(&self, workflow_id: &str, task: &TaskState) -> Result<()> {
        let event = ExternalEvent::from_task_state(workflow_id, task, "task.completed");
        let callback = CallbackPayload::for_task(
            CallbackEventType::TaskCompleted,
            workflow_id,
            "",
            &task.task_id,
            serde_json::json!({"status": "completed", "output": task.output}),
        );

        let _ = self.emit_event(event).await;
        let _ = self.dispatch_callback(callback).await;

        Ok(())
    }

    /// Emit task failed event.
    pub async fn emit_task_failed(&self, workflow_id: &str, task: &TaskState) -> Result<()> {
        let event = ExternalEvent::from_task_state(workflow_id, task, "task.failed")
            .with_metadata("error", task.error.as_deref().unwrap_or("unknown"));
        let callback = CallbackPayload::for_task(
            CallbackEventType::TaskFailed,
            workflow_id,
            "",
            &task.task_id,
            serde_json::json!({"status": "failed", "error": task.error}),
        );

        let _ = self.emit_event(event).await;
        let _ = self.dispatch_callback(callback).await;

        Ok(())
    }
}

impl Default for ExternalIntegrationRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_method_as_str() {
        assert_eq!(HttpMethod::Get.as_str(), "GET");
        assert_eq!(HttpMethod::Post.as_str(), "POST");
        assert_eq!(HttpMethod::Put.as_str(), "PUT");
        assert_eq!(HttpMethod::Delete.as_str(), "DELETE");
    }

    #[test]
    fn test_http_response_status_checks() {
        let success_response = HttpResponse {
            status_code: 200,
            headers: HashMap::new(),
            body: None,
            response_time_ms: 100,
            is_success: true,
        };
        assert!(success_response.is_success());
        assert!(!success_response.is_client_error());
        assert!(!success_response.is_server_error());

        let client_error = HttpResponse {
            status_code: 404,
            headers: HashMap::new(),
            body: None,
            response_time_ms: 50,
            is_success: false,
        };
        assert!(!client_error.is_success());
        assert!(client_error.is_client_error());

        let server_error = HttpResponse {
            status_code: 500,
            headers: HashMap::new(),
            body: None,
            response_time_ms: 75,
            is_success: false,
        };
        assert!(!server_error.is_success());
        assert!(server_error.is_server_error());
    }

    #[test]
    fn test_queue_message_builder() {
        let msg = QueueMessage::new("test-topic", serde_json::json!({"key": "value"}))
            .with_correlation_id("corr-123")
            .with_reply_to("reply-topic")
            .with_priority(8)
            .with_header("custom", "header");

        assert_eq!(msg.topic, "test-topic");
        assert_eq!(msg.correlation_id, Some("corr-123".to_string()));
        assert_eq!(msg.reply_to, Some("reply-topic".to_string()));
        assert_eq!(msg.priority, 8);
        assert_eq!(msg.headers.get("custom"), Some(&"header".to_string()));
    }

    #[test]
    fn test_query_result() {
        let result = QueryResult::empty();
        assert!(result.is_empty());
        assert_eq!(result.row_count(), 0);

        let result_with_data = QueryResult {
            rows_affected: 1,
            columns: vec!["id".to_string(), "name".to_string()],
            rows: vec![vec![serde_json::json!(1), serde_json::json!("test")]],
            execution_time_ms: 10,
        };
        assert!(!result_with_data.is_empty());
        assert_eq!(result_with_data.row_count(), 1);
    }

    #[test]
    fn test_callback_event_type_as_str() {
        assert_eq!(CallbackEventType::WorkflowStarted.as_str(), "workflow.started");
        assert_eq!(CallbackEventType::TaskCompleted.as_str(), "task.completed");
        assert_eq!(CallbackEventType::Custom.as_str(), "custom");
    }

    #[test]
    fn test_callback_payload_creation() {
        let workflow_payload = CallbackPayload::for_workflow(
            CallbackEventType::WorkflowStarted,
            "wf-1",
            "exec-1",
            serde_json::json!({}),
        );
        assert_eq!(workflow_payload.workflow_id, "wf-1");
        assert_eq!(workflow_payload.execution_id, "exec-1");
        assert!(workflow_payload.task_id.is_none());

        let task_payload = CallbackPayload::for_task(
            CallbackEventType::TaskStarted,
            "wf-1",
            "exec-1",
            "task-1",
            serde_json::json!({}),
        );
        assert_eq!(task_payload.task_id, Some("task-1".to_string()));
    }

    #[test]
    fn test_webhook_trigger_creation() {
        let trigger = WebhookTrigger::new("workflow-1")
            .with_secret("my-secret")
            .with_allowed_ips(vec!["192.168.1.1".to_string()])
            .with_parameter("payload.id", "workflow_param");

        assert_eq!(trigger.workflow_id, "workflow-1");
        assert!(trigger.secret.is_some());
        assert_eq!(trigger.allowed_ips.len(), 1);
        assert!(trigger.parameter_mapping.contains_key("payload.id"));
    }

    #[tokio::test]
    async fn test_webhook_registry() {
        let registry = WebhookRegistry::new();

        let trigger = WebhookTrigger::new("workflow-1");
        let id = registry.register(trigger).await;

        assert!(registry.get(&id).await.is_some());

        let triggers = registry.list().await;
        assert_eq!(triggers.len(), 1);

        let workflow_triggers = registry.find_by_workflow("workflow-1").await;
        assert_eq!(workflow_triggers.len(), 1);

        assert!(registry.unregister(&id).await.is_some());
        assert!(registry.get(&id).await.is_none());
    }

    #[test]
    fn test_external_event_creation() {
        let event = ExternalEvent::new(
            "test.event",
            "test-source",
            serde_json::json!({"data": "value"}),
        )
        .with_subject("subject-1")
        .with_metadata("key", "value");

        assert_eq!(event.event_type, "test.event");
        assert_eq!(event.source, "test-source");
        assert_eq!(event.subject, Some("subject-1".to_string()));
        assert_eq!(event.metadata.get("key"), Some(&"value".to_string()));
    }

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_delay_ms, 1000);
        assert_eq!(config.max_delay_ms, 30000);
        assert!((config.backoff_multiplier - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_http_client_config_default() {
        let config = HttpClientConfig::default();
        assert_eq!(config.timeout_secs, 30);
        assert_eq!(config.max_retries, 3);
        assert!(config.base_url.is_empty());
    }

    #[test]
    fn test_database_config_default() {
        let config = DatabaseConfig::default();
        assert_eq!(config.db_type, DatabaseType::PostgreSql);
        assert_eq!(config.pool_min, 1);
        assert_eq!(config.pool_max, 10);
    }

    #[tokio::test]
    async fn test_multi_event_emitter() {
        let emitter = MultiEventEmitter::new();
        assert_eq!(emitter.emitter_count(), 0);
    }

    #[tokio::test]
    async fn test_external_integration_registry() {
        let registry = ExternalIntegrationRegistry::new();

        let trigger = WebhookTrigger::new("workflow-1");
        let _ = registry.webhooks().register(trigger).await;

        let triggers = registry.webhooks().list().await;
        assert_eq!(triggers.len(), 1);
    }

    #[test]
    fn test_constant_time_compare() {
        assert!(constant_time_compare("test", "test"));
        assert!(!constant_time_compare("test", "Test"));
        assert!(!constant_time_compare("test", "test1"));
    }
}
