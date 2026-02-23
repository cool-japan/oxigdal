//! Schema Registry HTTP client

use crate::error::{Error, Result};
use crate::schema_registry::schema::{Schema, SchemaType, SchemaVersion};
use parking_lot::RwLock;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};

/// Schema Registry configuration
#[derive(Debug, Clone)]
pub struct SchemaRegistryConfig {
    /// Schema Registry URL
    pub url: String,
    /// HTTP timeout
    pub timeout: Duration,
    /// Basic auth username
    pub username: Option<String>,
    /// Basic auth password
    pub password: Option<String>,
    /// Maximum cache size
    pub cache_size: usize,
}

impl SchemaRegistryConfig {
    /// Create a new Schema Registry configuration
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            timeout: Duration::from_secs(30),
            username: None,
            password: None,
            cache_size: 1000,
        }
    }

    /// Set timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set basic auth credentials
    pub fn with_auth(mut self, username: String, password: String) -> Self {
        self.username = Some(username);
        self.password = Some(password);
        self
    }

    /// Set cache size
    pub fn with_cache_size(mut self, size: usize) -> Self {
        self.cache_size = size;
        self
    }
}

/// Schema Registry HTTP client
pub struct SchemaRegistryClient {
    config: SchemaRegistryConfig,
    http_client: Client,
    schema_cache: Arc<RwLock<HashMap<i32, Schema>>>,
    subject_cache: Arc<RwLock<HashMap<(String, i32), Schema>>>,
    id_cache: Arc<RwLock<HashMap<String, i32>>>,
}

impl SchemaRegistryClient {
    /// Create a new Schema Registry client
    pub async fn new(config: SchemaRegistryConfig) -> Result<Self> {
        let http_client_builder = Client::builder().timeout(config.timeout);

        let http_client = http_client_builder
            .build()
            .map_err(|e| Error::Http(format!("Failed to create HTTP client: {}", e)))?;

        info!("Schema Registry client created: {}", config.url);

        Ok(Self {
            config,
            http_client,
            schema_cache: Arc::new(RwLock::new(HashMap::new())),
            subject_cache: Arc::new(RwLock::new(HashMap::new())),
            id_cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Create a request builder with authentication if configured
    fn request_with_auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let (Some(username), Some(password)) = (&self.config.username, &self.config.password) {
            request.basic_auth(username, Some(password))
        } else {
            request
        }
    }

    /// Register a schema
    pub async fn register_schema(&self, subject: &str, schema_str: &str) -> Result<i32> {
        // Check cache first
        {
            let cache = self.id_cache.read();
            let key = format!("{}-{}", subject, schema_str);
            if let Some(&id) = cache.get(&key) {
                debug!("Schema ID found in cache: {}", id);
                return Ok(id);
            }
        }

        let url = format!("{}/subjects/{}/versions", self.config.url, subject);

        #[derive(Serialize)]
        struct RegisterRequest<'a> {
            schema: &'a str,
            #[serde(rename = "schemaType")]
            schema_type: &'a str,
        }

        #[derive(Deserialize)]
        struct RegisterResponse {
            id: i32,
        }

        let request = RegisterRequest {
            schema: schema_str,
            schema_type: "AVRO",
        };

        let response = self
            .request_with_auth(self.http_client.post(&url))
            .json(&request)
            .send()
            .await
            .map_err(|e| Error::Http(format!("Failed to register schema: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::SchemaRegistry(format!(
                "Failed to register schema: {} - {}",
                status, body
            )));
        }

        let result: RegisterResponse = response
            .json()
            .await
            .map_err(|e| Error::Http(format!("Failed to parse response: {}", e)))?;

        // Update cache
        {
            let mut cache = self.id_cache.write();
            let key = format!("{}-{}", subject, schema_str);
            cache.insert(key, result.id);
        }

        info!("Schema registered: {} with ID {}", subject, result.id);

        Ok(result.id)
    }

    /// Get schema by ID
    pub async fn get_schema_by_id(&self, id: i32) -> Result<Schema> {
        // Check cache first
        {
            let cache = self.schema_cache.read();
            if let Some(schema) = cache.get(&id) {
                debug!("Schema found in cache: ID {}", id);
                return Ok(schema.clone());
            }
        }

        let url = format!("{}/schemas/ids/{}", self.config.url, id);

        #[derive(Deserialize)]
        struct SchemaResponse {
            schema: String,
            #[serde(rename = "schemaType")]
            #[allow(dead_code)]
            schema_type: Option<String>,
        }

        let response = self
            .request_with_auth(self.http_client.get(&url))
            .send()
            .await
            .map_err(|e| Error::Http(format!("Failed to get schema: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::SchemaNotFound(format!(
                "Schema ID {} not found: {} - {}",
                id, status, body
            )));
        }

        let result: SchemaResponse = response
            .json()
            .await
            .map_err(|e| Error::Http(format!("Failed to parse response: {}", e)))?;

        let schema = Schema {
            id,
            schema: result.schema.clone(),
            schema_type: SchemaType::Avro,
            version: None,
            subject: None,
        };

        // Update cache
        {
            let mut cache = self.schema_cache.write();
            cache.insert(id, schema.clone());
        }

        Ok(schema)
    }

    /// Get schema by subject and version
    pub async fn get_schema_by_subject(
        &self,
        subject: &str,
        version: SchemaVersion,
    ) -> Result<Schema> {
        let version_str = match version {
            SchemaVersion::Latest => "latest".to_string(),
            SchemaVersion::Version(v) => v.to_string(),
        };

        let url = format!(
            "{}/subjects/{}/versions/{}",
            self.config.url, subject, version_str
        );

        #[derive(Deserialize)]
        struct SubjectSchemaResponse {
            id: i32,
            schema: String,
            version: i32,
            subject: String,
            #[serde(rename = "schemaType")]
            #[allow(dead_code)]
            schema_type: Option<String>,
        }

        let response = self
            .request_with_auth(self.http_client.get(&url))
            .send()
            .await
            .map_err(|e| Error::Http(format!("Failed to get schema: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::SchemaNotFound(format!(
                "Schema for subject {} version {} not found: {} - {}",
                subject, version_str, status, body
            )));
        }

        let result: SubjectSchemaResponse = response
            .json()
            .await
            .map_err(|e| Error::Http(format!("Failed to parse response: {}", e)))?;

        let schema = Schema {
            id: result.id,
            schema: result.schema,
            schema_type: SchemaType::Avro,
            version: Some(result.version),
            subject: Some(result.subject),
        };

        // Update caches
        {
            let mut cache = self.schema_cache.write();
            cache.insert(result.id, schema.clone());
        }
        {
            let mut cache = self.subject_cache.write();
            cache.insert((subject.to_string(), result.version), schema.clone());
        }

        Ok(schema)
    }

    /// Test schema compatibility
    pub async fn test_compatibility(&self, subject: &str, schema_str: &str) -> Result<bool> {
        let url = format!(
            "{}/compatibility/subjects/{}/versions/latest",
            self.config.url, subject
        );

        #[derive(Serialize)]
        struct CompatibilityRequest<'a> {
            schema: &'a str,
        }

        #[derive(Deserialize)]
        struct CompatibilityResponse {
            is_compatible: bool,
        }

        let request = CompatibilityRequest { schema: schema_str };

        let response = self
            .request_with_auth(self.http_client.post(&url))
            .json(&request)
            .send()
            .await
            .map_err(|e| Error::Http(format!("Failed to test compatibility: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::SchemaRegistry(format!(
                "Failed to test compatibility: {} - {}",
                status, body
            )));
        }

        let result: CompatibilityResponse = response
            .json()
            .await
            .map_err(|e| Error::Http(format!("Failed to parse response: {}", e)))?;

        Ok(result.is_compatible)
    }

    /// Get all subjects
    pub async fn get_subjects(&self) -> Result<Vec<String>> {
        let url = format!("{}/subjects", self.config.url);

        let response = self
            .request_with_auth(self.http_client.get(&url))
            .send()
            .await
            .map_err(|e| Error::Http(format!("Failed to get subjects: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::SchemaRegistry(format!(
                "Failed to get subjects: {} - {}",
                status, body
            )));
        }

        let subjects: Vec<String> = response
            .json()
            .await
            .map_err(|e| Error::Http(format!("Failed to parse response: {}", e)))?;

        Ok(subjects)
    }

    /// Delete subject
    pub async fn delete_subject(&self, subject: &str) -> Result<Vec<i32>> {
        let url = format!("{}/subjects/{}", self.config.url, subject);

        let response = self
            .request_with_auth(self.http_client.delete(&url))
            .send()
            .await
            .map_err(|e| Error::Http(format!("Failed to delete subject: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::SchemaRegistry(format!(
                "Failed to delete subject: {} - {}",
                status, body
            )));
        }

        let versions: Vec<i32> = response
            .json()
            .await
            .map_err(|e| Error::Http(format!("Failed to parse response: {}", e)))?;

        Ok(versions)
    }

    /// Clear cache
    pub fn clear_cache(&self) {
        self.schema_cache.write().clear();
        self.subject_cache.write().clear();
        self.id_cache.write().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_registry_config() {
        let config = SchemaRegistryConfig::new("http://localhost:8081")
            .with_timeout(Duration::from_secs(10))
            .with_auth("user".to_string(), "pass".to_string())
            .with_cache_size(500);

        assert_eq!(config.url, "http://localhost:8081");
        assert_eq!(config.timeout, Duration::from_secs(10));
        assert_eq!(config.username, Some("user".to_string()));
        assert_eq!(config.cache_size, 500);
    }
}
