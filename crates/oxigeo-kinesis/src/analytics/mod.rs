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
            create_timestamp: detail.create_timestamp().copied(),
        })
    }

    /// Creates an analytics application
    pub async fn create_application(&self, config: &ApplicationConfig) -> Result<String> {
        let application = AnalyticsApplication::new(self.client.as_ref().clone(), config.clone());
        application.create().await
    }

    /// Deletes an analytics application.
    ///
    /// `DeleteApplication` requires the application's real `CreateTimestamp` as
    /// an optimistic-concurrency token, so this fetches the current description
    /// first and threads the actual creation time through rather than
    /// fabricating a wall-clock value (which would never match).
    pub async fn delete_application(&self, application_name: &str) -> Result<()> {
        // Fetch the real application description to obtain its creation
        // timestamp, which AWS uses as a conditional-delete token.
        let desc = self.describe_application(application_name).await?;

        let create_timestamp = desc
            .create_timestamp
            .ok_or_else(|| KinesisError::Analytics {
                message: format!(
                    "cannot delete application '{}': create timestamp unavailable from description",
                    application_name
                ),
            })?;

        self.client
            .delete_application()
            .application_name(application_name)
            .create_timestamp(create_timestamp)
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
    /// Application creation timestamp (used as the optimistic-concurrency token
    /// required by `DeleteApplication`). `None` if AWS did not report it.
    pub create_timestamp: Option<aws_smithy_types::DateTime>,
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
            create_timestamp: Some(aws_smithy_types::DateTime::from_secs(1_600_000_000)),
        };

        assert_eq!(desc.application_name, "test-app");
        assert_eq!(desc.application_status, Some("RUNNING".to_string()));
        assert_eq!(desc.create_timestamp.map(|t| t.secs()), Some(1_600_000_000));
    }
}
