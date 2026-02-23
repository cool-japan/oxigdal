//! External workflow system integrations.
//!
//! Provides integration with popular workflow orchestration platforms:
//! - Apache Airflow
//! - Prefect
//! - Temporal.io
//! - Webhooks
//! - Message queues (Kafka, RabbitMQ)

#[cfg(feature = "integrations")]
pub mod airflow;
#[cfg(feature = "integrations")]
pub mod prefect;
#[cfg(feature = "integrations")]
pub mod temporal;

use crate::engine::WorkflowDefinition;
use crate::error::{Result, WorkflowError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "integrations")]
pub use airflow::AirflowIntegration;
#[cfg(feature = "integrations")]
pub use prefect::PrefectIntegration;
#[cfg(feature = "integrations")]
pub use temporal::TemporalIntegration;

/// Integration type enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntegrationType {
    /// Apache Airflow.
    Airflow,
    /// Prefect.
    Prefect,
    /// Temporal.io.
    Temporal,
    /// Webhook.
    Webhook,
    /// Kafka message queue.
    Kafka,
    /// RabbitMQ message queue.
    RabbitMq,
}

/// Integration configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationConfig {
    /// Integration type.
    pub integration_type: IntegrationType,
    /// Endpoint URL.
    pub endpoint: String,
    /// Authentication credentials.
    pub auth: Option<AuthConfig>,
    /// Additional configuration.
    pub extra_config: HashMap<String, String>,
}

/// Authentication configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthConfig {
    /// API key authentication.
    ApiKey {
        /// API key.
        key: String,
    },
    /// Basic authentication.
    Basic {
        /// Username.
        username: String,
        /// Password.
        password: String,
    },
    /// OAuth2 authentication.
    OAuth2 {
        /// Access token.
        token: String,
    },
    /// None (no authentication).
    None,
}

/// Webhook configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    /// Webhook URL.
    pub url: String,
    /// HTTP method.
    pub method: HttpMethod,
    /// Headers to include.
    pub headers: HashMap<String, String>,
    /// Authentication.
    pub auth: Option<AuthConfig>,
    /// Retry configuration.
    pub retry: Option<RetryConfig>,
}

/// HTTP method enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpMethod {
    /// GET method.
    Get,
    /// POST method.
    Post,
    /// PUT method.
    Put,
    /// DELETE method.
    Delete,
    /// PATCH method.
    Patch,
}

/// Retry configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of retries.
    pub max_retries: usize,
    /// Initial retry delay in milliseconds.
    pub initial_delay_ms: u64,
    /// Maximum retry delay in milliseconds.
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

/// Integration manager for external systems.
pub struct IntegrationManager {
    configs: HashMap<String, IntegrationConfig>,
}

impl IntegrationManager {
    /// Create a new integration manager.
    pub fn new() -> Self {
        Self {
            configs: HashMap::new(),
        }
    }

    /// Register an integration.
    pub fn register(&mut self, name: String, config: IntegrationConfig) {
        self.configs.insert(name, config);
    }

    /// Get an integration configuration.
    pub fn get(&self, name: &str) -> Option<&IntegrationConfig> {
        self.configs.get(name)
    }

    /// Remove an integration.
    pub fn remove(&mut self, name: &str) -> Option<IntegrationConfig> {
        self.configs.remove(name)
    }

    /// List all integrations.
    pub fn list(&self) -> Vec<String> {
        self.configs.keys().cloned().collect()
    }

    /// Export workflow to external format.
    pub fn export_workflow(
        &self,
        workflow: &WorkflowDefinition,
        integration_type: IntegrationType,
    ) -> Result<String> {
        match integration_type {
            #[cfg(feature = "integrations")]
            IntegrationType::Airflow => AirflowIntegration::export_workflow(workflow),
            #[cfg(feature = "integrations")]
            IntegrationType::Prefect => PrefectIntegration::export_workflow(workflow),
            #[cfg(feature = "integrations")]
            IntegrationType::Temporal => TemporalIntegration::export_workflow(workflow),
            _ => Err(WorkflowError::integration(
                integration_type.as_str(),
                "Export not implemented for this integration type",
            )),
        }
    }

    /// Trigger workflow via webhook.
    #[cfg(feature = "integrations")]
    pub async fn trigger_webhook(
        &self,
        config: &WebhookConfig,
        payload: &serde_json::Value,
    ) -> Result<String> {
        use reqwest::Client;

        let client = Client::new();
        let mut request = match config.method {
            HttpMethod::Get => client.get(&config.url),
            HttpMethod::Post => client.post(&config.url),
            HttpMethod::Put => client.put(&config.url),
            HttpMethod::Delete => client.delete(&config.url),
            HttpMethod::Patch => client.patch(&config.url),
        };

        // Add headers
        for (key, value) in &config.headers {
            request = request.header(key, value);
        }

        // Add authentication
        if let Some(auth) = &config.auth {
            request = match auth {
                AuthConfig::ApiKey { key } => request.header("X-API-Key", key),
                AuthConfig::Basic { username, password } => {
                    request.basic_auth(username, Some(password))
                }
                AuthConfig::OAuth2 { token } => request.bearer_auth(token),
                AuthConfig::None => request,
            };
        }

        // Send request
        let response =
            request.json(payload).send().await.map_err(|e| {
                WorkflowError::integration("webhook", format!("Request failed: {}", e))
            })?;

        let status = response.status();
        let body = response.text().await.map_err(|e| {
            WorkflowError::integration("webhook", format!("Failed to read response: {}", e))
        })?;

        if !status.is_success() {
            return Err(WorkflowError::integration(
                "webhook",
                format!("Request failed with status {}: {}", status, body),
            ));
        }

        Ok(body)
    }
}

impl Default for IntegrationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl IntegrationType {
    /// Get string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Airflow => "airflow",
            Self::Prefect => "prefect",
            Self::Temporal => "temporal",
            Self::Webhook => "webhook",
            Self::Kafka => "kafka",
            Self::RabbitMq => "rabbitmq",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integration_manager() {
        let mut manager = IntegrationManager::new();

        let config = IntegrationConfig {
            integration_type: IntegrationType::Webhook,
            endpoint: "https://example.com/webhook".to_string(),
            auth: Some(AuthConfig::ApiKey {
                key: "test-key".to_string(),
            }),
            extra_config: HashMap::new(),
        };

        manager.register("test-integration".to_string(), config);

        assert!(manager.get("test-integration").is_some());
        assert_eq!(manager.list().len(), 1);
    }

    #[test]
    fn test_integration_type_str() {
        assert_eq!(IntegrationType::Airflow.as_str(), "airflow");
        assert_eq!(IntegrationType::Webhook.as_str(), "webhook");
    }

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_delay_ms, 1000);
    }
}
