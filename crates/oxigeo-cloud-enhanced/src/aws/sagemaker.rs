//! AWS SageMaker integration for machine learning.

use crate::error::{CloudEnhancedError, Result};
use aws_sdk_sagemaker::Client as AwsSageMakerClient;
use aws_sdk_sagemaker::types::{ContainerDefinition, EndpointStatus, TrainingJobStatus};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

/// SageMaker client for ML operations.
#[derive(Debug, Clone)]
pub struct SageMakerClient {
    client: Arc<AwsSageMakerClient>,
    sdk_config: Arc<aws_config::SdkConfig>,
}

impl SageMakerClient {
    /// Creates a new SageMaker client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be created.
    pub fn new(config: &super::AwsConfig) -> Result<Self> {
        let client = AwsSageMakerClient::new(config.sdk_config());
        Ok(Self {
            client: Arc::new(client),
            sdk_config: Arc::new(config.sdk_config().clone()),
        })
    }

    /// Creates a SageMaker model.
    ///
    /// # Errors
    ///
    /// Returns an error if the model cannot be created.
    pub async fn create_model(&self, config: ModelConfig) -> Result<String> {
        let container = ContainerDefinition::builder()
            .image(config.image)
            .set_model_data_url(config.model_data_url)
            .build();

        let response = self
            .client
            .create_model()
            .model_name(&config.name)
            .execution_role_arn(&config.execution_role_arn)
            .primary_container(container)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!("Failed to create SageMaker model: {}", e))
            })?;

        response
            .model_arn
            .ok_or_else(|| CloudEnhancedError::ml_service("No model ARN returned".to_string()))
    }

    /// Deletes a SageMaker model.
    ///
    /// # Errors
    ///
    /// Returns an error if the model cannot be deleted.
    pub async fn delete_model(&self, model_name: &str) -> Result<()> {
        self.client
            .delete_model()
            .model_name(model_name)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!("Failed to delete SageMaker model: {}", e))
            })?;

        Ok(())
    }

    /// Creates a SageMaker endpoint configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint configuration cannot be created.
    pub async fn create_endpoint_config(
        &self,
        name: &str,
        model_name: &str,
        instance_type: &str,
        initial_instance_count: i32,
    ) -> Result<String> {
        let production_variant = aws_sdk_sagemaker::types::ProductionVariant::builder()
            .variant_name("AllTraffic")
            .model_name(model_name)
            .instance_type(instance_type.parse().map_err(|_| {
                CloudEnhancedError::invalid_argument(format!(
                    "Invalid instance type: {}",
                    instance_type
                ))
            })?)
            .initial_instance_count(initial_instance_count)
            .build();

        let response = self
            .client
            .create_endpoint_config()
            .endpoint_config_name(name)
            .production_variants(production_variant)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!(
                    "Failed to create endpoint configuration: {}",
                    e
                ))
            })?;

        response.endpoint_config_arn.ok_or_else(|| {
            CloudEnhancedError::ml_service("No endpoint config ARN returned".to_string())
        })
    }

    /// Deletes a SageMaker endpoint configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint configuration cannot be deleted.
    pub async fn delete_endpoint_config(&self, name: &str) -> Result<()> {
        self.client
            .delete_endpoint_config()
            .endpoint_config_name(name)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!(
                    "Failed to delete endpoint configuration: {}",
                    e
                ))
            })?;

        Ok(())
    }

    /// Creates a SageMaker endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint cannot be created.
    pub async fn create_endpoint(&self, name: &str, config_name: &str) -> Result<String> {
        let response = self
            .client
            .create_endpoint()
            .endpoint_name(name)
            .endpoint_config_name(config_name)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!("Failed to create endpoint: {}", e))
            })?;

        response
            .endpoint_arn
            .ok_or_else(|| CloudEnhancedError::ml_service("No endpoint ARN returned".to_string()))
    }

    /// Waits for an endpoint to be in service.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint fails to become ready or times out.
    pub async fn wait_for_endpoint(
        &self,
        endpoint_name: &str,
        poll_interval: Duration,
        timeout: Duration,
    ) -> Result<()> {
        let start = std::time::Instant::now();

        loop {
            let status = self.get_endpoint_status(endpoint_name).await?;

            match status {
                EndpointStatus::InService => return Ok(()),
                EndpointStatus::Failed => {
                    return Err(CloudEnhancedError::ml_service(format!(
                        "Endpoint {} failed",
                        endpoint_name
                    )));
                }
                EndpointStatus::Creating | EndpointStatus::Updating => {
                    if start.elapsed() > timeout {
                        return Err(CloudEnhancedError::timeout(format!(
                            "Endpoint {} timed out after {:?}",
                            endpoint_name, timeout
                        )));
                    }
                    sleep(poll_interval).await;
                }
                _ => {
                    return Err(CloudEnhancedError::ml_service(format!(
                        "Unknown endpoint status: {:?}",
                        status
                    )));
                }
            }
        }
    }

    /// Gets the status of an endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the status cannot be retrieved.
    pub async fn get_endpoint_status(&self, endpoint_name: &str) -> Result<EndpointStatus> {
        let response = self
            .client
            .describe_endpoint()
            .endpoint_name(endpoint_name)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!("Failed to describe endpoint: {}", e))
            })?;

        Ok(response.endpoint_status.unwrap_or(EndpointStatus::Creating))
    }

    /// Deletes a SageMaker endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint cannot be deleted.
    pub async fn delete_endpoint(&self, endpoint_name: &str) -> Result<()> {
        self.client
            .delete_endpoint()
            .endpoint_name(endpoint_name)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!("Failed to delete endpoint: {}", e))
            })?;

        Ok(())
    }

    /// Invokes a SageMaker endpoint for inference.
    ///
    /// # Errors
    ///
    /// Returns an error if the invocation fails.
    pub async fn invoke_endpoint(
        &self,
        endpoint_name: &str,
        body: &[u8],
        content_type: Option<&str>,
    ) -> Result<Vec<u8>> {
        let runtime_client = aws_sdk_sagemakerruntime::Client::new(&self.sdk_config);

        let mut request = runtime_client
            .invoke_endpoint()
            .endpoint_name(endpoint_name)
            .body(aws_smithy_types::Blob::new(body));

        if let Some(ct) = content_type {
            request = request.content_type(ct);
        }

        let response = request.send().await.map_err(|e| {
            CloudEnhancedError::ml_service(format!("Failed to invoke endpoint: {}", e))
        })?;

        Ok(response
            .body
            .ok_or_else(|| CloudEnhancedError::ml_service("No response body returned".to_string()))?
            .into_inner())
    }

    /// Creates a training job.
    ///
    /// # Errors
    ///
    /// Returns an error if the training job cannot be created.
    pub async fn create_training_job(&self, config: TrainingJobConfig) -> Result<String> {
        let algorithm_specification = aws_sdk_sagemaker::types::AlgorithmSpecification::builder()
            .training_image(config.training_image)
            .training_input_mode(config.training_input_mode.parse().map_err(|_| {
                CloudEnhancedError::invalid_argument("Invalid training input mode".to_string())
            })?)
            .build();

        let resource_config = aws_sdk_sagemaker::types::ResourceConfig::builder()
            .instance_type(config.instance_type.parse().map_err(|_| {
                CloudEnhancedError::invalid_argument("Invalid instance type".to_string())
            })?)
            .instance_count(config.instance_count)
            .volume_size_in_gb(config.volume_size_gb)
            .build();

        let stopping_condition = aws_sdk_sagemaker::types::StoppingCondition::builder()
            .max_runtime_in_seconds(config.max_runtime_seconds)
            .build();

        let output_config = aws_sdk_sagemaker::types::OutputDataConfig::builder()
            .s3_output_path(config.output_path)
            .build();

        let response = self
            .client
            .create_training_job()
            .training_job_name(&config.job_name)
            .algorithm_specification(algorithm_specification)
            .role_arn(&config.role_arn)
            .set_input_data_config(Some(config.input_data_config))
            .output_data_config(output_config)
            .resource_config(resource_config)
            .stopping_condition(stopping_condition)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!("Failed to create training job: {}", e))
            })?;

        response.training_job_arn.ok_or_else(|| {
            CloudEnhancedError::ml_service("No training job ARN returned".to_string())
        })
    }

    /// Gets the status of a training job.
    ///
    /// # Errors
    ///
    /// Returns an error if the status cannot be retrieved.
    pub async fn get_training_job_status(&self, job_name: &str) -> Result<TrainingJobStatus> {
        let response = self
            .client
            .describe_training_job()
            .training_job_name(job_name)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::ml_service(format!("Failed to describe training job: {}", e))
            })?;

        response.training_job_status.ok_or_else(|| {
            CloudEnhancedError::ml_service("No training job status returned".to_string())
        })
    }

    /// Lists training jobs.
    ///
    /// # Errors
    ///
    /// Returns an error if the list cannot be retrieved.
    pub async fn list_training_jobs(&self, max_results: Option<i32>) -> Result<Vec<String>> {
        let mut request = self.client.list_training_jobs();

        if let Some(max) = max_results {
            request = request.max_results(max);
        }

        let response = request.send().await.map_err(|e| {
            CloudEnhancedError::ml_service(format!("Failed to list training jobs: {}", e))
        })?;

        Ok(response
            .training_job_summaries
            .into_iter()
            .flat_map(|summaries| summaries.into_iter().filter_map(|s| s.training_job_name))
            .collect())
    }
}

/// SageMaker model configuration.
#[derive(Debug, Clone)]
pub struct ModelConfig {
    /// Model name
    pub name: String,
    /// Execution role ARN
    pub execution_role_arn: String,
    /// Container image
    pub image: String,
    /// Model data URL (S3)
    pub model_data_url: Option<String>,
}

/// Training job configuration.
#[derive(Debug, Clone)]
pub struct TrainingJobConfig {
    /// Job name
    pub job_name: String,
    /// Role ARN
    pub role_arn: String,
    /// Training image
    pub training_image: String,
    /// Training input mode
    pub training_input_mode: String,
    /// Instance type
    pub instance_type: String,
    /// Instance count
    pub instance_count: i32,
    /// Volume size in GB
    pub volume_size_gb: i32,
    /// Max runtime in seconds
    pub max_runtime_seconds: i32,
    /// Input data configuration
    pub input_data_config: Vec<aws_sdk_sagemaker::types::Channel>,
    /// Output path (S3)
    pub output_path: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_config() {
        let config = ModelConfig {
            name: "test-model".to_string(),
            execution_role_arn: "arn:aws:iam::123456789012:role/sagemaker-role".to_string(),
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/my-model:latest".to_string(),
            model_data_url: Some("s3://bucket/model.tar.gz".to_string()),
        };

        assert_eq!(config.name, "test-model");
        assert!(config.model_data_url.is_some());
    }
}
