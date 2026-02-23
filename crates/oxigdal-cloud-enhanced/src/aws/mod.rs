//! AWS cloud platform integrations.
//!
//! This module provides deep integrations with AWS services including
//! S3 Select, Athena, Glue, Lambda, SageMaker, CloudWatch, and cost optimization.

pub mod athena;
pub mod cloudwatch;
pub mod cost_optimizer;
pub mod glue;
pub mod lambda;
pub mod s3_select;
pub mod sagemaker;

use crate::error::Result;
use aws_config::BehaviorVersion;
use std::sync::Arc;

/// AWS configuration for enhanced services.
#[derive(Debug, Clone)]
pub struct AwsConfig {
    /// AWS region
    pub region: String,
    /// AWS SDK config
    pub(crate) sdk_config: Arc<aws_config::SdkConfig>,
}

impl AwsConfig {
    /// Creates a new AWS configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the AWS configuration cannot be loaded.
    pub async fn new(region: Option<String>) -> Result<Self> {
        let mut config_loader = aws_config::defaults(BehaviorVersion::latest());

        if let Some(ref r) = region {
            config_loader = config_loader.region(aws_config::Region::new(r.clone()));
        }

        let sdk_config = config_loader.load().await;
        let region = sdk_config
            .region()
            .map(|r| r.as_ref().to_string())
            .unwrap_or_else(|| "us-east-1".to_string());

        Ok(Self {
            region,
            sdk_config: Arc::new(sdk_config),
        })
    }

    /// Gets the AWS region.
    pub fn region(&self) -> &str {
        &self.region
    }

    /// Gets the SDK config.
    pub(crate) fn sdk_config(&self) -> &aws_config::SdkConfig {
        &self.sdk_config
    }
}

/// AWS client manager for all services.
#[derive(Debug)]
pub struct AwsClient {
    config: AwsConfig,
    s3_select: s3_select::S3SelectClient,
    athena: athena::AthenaClient,
    glue: glue::GlueClient,
    lambda: lambda::LambdaClient,
    sagemaker: sagemaker::SageMakerClient,
    cloudwatch: cloudwatch::CloudWatchClient,
    cost_optimizer: cost_optimizer::CostOptimizer,
}

impl AwsClient {
    /// Creates a new AWS client manager.
    ///
    /// # Errors
    ///
    /// Returns an error if the AWS configuration cannot be loaded.
    pub async fn new(region: Option<String>) -> Result<Self> {
        let config = AwsConfig::new(region).await?;

        Ok(Self {
            s3_select: s3_select::S3SelectClient::new(&config)?,
            athena: athena::AthenaClient::new(&config)?,
            glue: glue::GlueClient::new(&config)?,
            lambda: lambda::LambdaClient::new(&config)?,
            sagemaker: sagemaker::SageMakerClient::new(&config)?,
            cloudwatch: cloudwatch::CloudWatchClient::new(&config)?,
            cost_optimizer: cost_optimizer::CostOptimizer::new(&config)?,
            config,
        })
    }

    /// Gets a reference to the S3 Select client.
    pub fn s3_select(&self) -> &s3_select::S3SelectClient {
        &self.s3_select
    }

    /// Gets a reference to the Athena client.
    pub fn athena(&self) -> &athena::AthenaClient {
        &self.athena
    }

    /// Gets a reference to the Glue client.
    pub fn glue(&self) -> &glue::GlueClient {
        &self.glue
    }

    /// Gets a reference to the Lambda client.
    pub fn lambda(&self) -> &lambda::LambdaClient {
        &self.lambda
    }

    /// Gets a reference to the SageMaker client.
    pub fn sagemaker(&self) -> &sagemaker::SageMakerClient {
        &self.sagemaker
    }

    /// Gets a reference to the CloudWatch client.
    pub fn cloudwatch(&self) -> &cloudwatch::CloudWatchClient {
        &self.cloudwatch
    }

    /// Gets a reference to the cost optimizer.
    pub fn cost_optimizer(&self) -> &cost_optimizer::CostOptimizer {
        &self.cost_optimizer
    }

    /// Gets the AWS region.
    pub fn region(&self) -> &str {
        self.config.region()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_aws_config_creation() {
        let config = AwsConfig::new(Some("us-west-2".to_string())).await;
        assert!(config.is_ok());
        if let Ok(config) = config {
            assert_eq!(config.region(), "us-west-2");
        }
    }
}
