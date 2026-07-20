//! Kinesis Analytics module for real-time stream processing

pub mod application;
pub mod sql;
pub mod window;

pub use application::{
    AnalyticsApplication, ApplicationConfig, ApplicationStatus, RuntimeEnvironment,
};
pub use sql::{QueryBuilder, SqlQuery};
pub use window::{SessionWindow, SlidingWindow, TumblingWindow, WindowType};

use crate::error::{KinesisError, Result};
use aws_sdk_kinesisanalyticsv2::Client as AnalyticsClient;
use std::sync::Arc;

/// Kinesis Analytics client wrapper
#[derive(Clone)]
pub struct KinesisAnalytics {
    client: Arc<AnalyticsClient>,
}

impl KinesisAnalytics {
    /// Creates a new Kinesis Analytics client
    pub fn new(client: AnalyticsClient) -> Self {
        Self {
            client: Arc::new(client),
        }
    }

    /// Creates a new Kinesis Analytics client from environment
    pub async fn from_env() -> Self {
        let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let client = AnalyticsClient::new(&config);
        Self::new(client)
    }

    /// Gets a reference to the Analytics client
    pub fn client(&self) -> &AnalyticsClient {
        &self.client
    }

    /// Lists all analytics applications
    pub async fn list_applications(&self) -> Result<Vec<String>> {
        let response =
            self.client
                .list_applications()
                .send()
                .await
                .map_err(|e| KinesisError::Analytics {
                    message: e.to_string(),
                })?;

        Ok(response
            .application_summaries()
            .iter()
            .map(|s| s.application_name().to_string())
            .collect())
    }

    /// Describes an analytics application
    pub async fn describe_application(
        &self,
        application_name: &str,
    ) -> Result<ApplicationDescription> {
        let response = self
            .client
            .describe_application()
            .application_name(application_name)
            .send()
            .await
            .map_err(|e| KinesisError::Analytics {
                message: e.to_string(),
            })?;

        let detail = response
            .application_detail()
            .ok_or_else(|| KinesisError::Analytics {
                message: "Application detail not found".to_string(),
            })?;

        Ok(ApplicationDescription {
            application_name: detail.application_name().to_string(),
            application_arn: detail.application_arn().to_string(),
            application_status: Some(detail.application_status().as_str().to_string()),
            runtime_environment: Some(detail.runtime_environment().as_str().to_string()),
        })
    }

    /// Creates an analytics application
    pub async fn create_application(&self, config: &ApplicationConfig) -> Result<String> {
        let application = AnalyticsApplication::new(self.client.as_ref().clone(), config.clone());
        application.create().await
    }

    /// Deletes an analytics application
    pub async fn delete_application(&self, application_name: &str) -> Result<()> {
        // Get current application version
        let _desc = self.describe_application(application_name).await?;
        let _version_id = 1; // This would be retrieved from the description in a real implementation

        self.client
            .delete_application()
            .application_name(application_name)
            .create_timestamp(aws_sdk_kinesisanalyticsv2::primitives::DateTime::from_secs(
                chrono::Utc::now().timestamp(),
            ))
            .send()
            .await
            .map_err(|e| KinesisError::Analytics {
                message: e.to_string(),
            })?;

        Ok(())
    }

    /// Starts an analytics application
    pub async fn start_application(&self, application_name: &str) -> Result<()> {
        self.client
            .start_application()
            .application_name(application_name)
            .send()
            .await
            .map_err(|e| KinesisError::Analytics {
                message: e.to_string(),
            })?;

        Ok(())
    }

    /// Stops an analytics application
    pub async fn stop_application(&self, application_name: &str) -> Result<()> {
        self.client
            .stop_application()
            .application_name(application_name)
            .send()
            .await
            .map_err(|e| KinesisError::Analytics {
                message: e.to_string(),
            })?;

        Ok(())
    }
}

/// Application description
#[derive(Debug, Clone)]
pub struct ApplicationDescription {
    /// Application name
    pub application_name: String,
    /// Application ARN
    pub application_arn: String,
    /// Application status
    pub application_status: Option<String>,
    /// Runtime environment
    pub runtime_environment: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_application_description() {
        let desc = ApplicationDescription {
            application_name: "test-app".to_string(),
            application_arn: "arn:aws:kinesisanalytics:us-east-1:123456789012:application/test-app"
                .to_string(),
            application_status: Some("RUNNING".to_string()),
            runtime_environment: Some("SQL-1_0".to_string()),
        };

        assert_eq!(desc.application_name, "test-app");
        assert_eq!(desc.application_status, Some("RUNNING".to_string()));
    }
}
