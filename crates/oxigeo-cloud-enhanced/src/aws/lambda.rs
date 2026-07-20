//! AWS Lambda integration for serverless processing.

use crate::error::{CloudEnhancedError, Result};
use aws_sdk_lambda::Client as AwsLambdaClient;
use aws_sdk_lambda::primitives::Blob;
use aws_sdk_lambda::types::{Environment, InvocationType, Runtime};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Lambda client for serverless function execution.
#[derive(Debug, Clone)]
pub struct LambdaClient {
    client: Arc<AwsLambdaClient>,
}

impl LambdaClient {
    /// Creates a new Lambda client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be created.
    pub fn new(config: &super::AwsConfig) -> Result<Self> {
        let client = AwsLambdaClient::new(config.sdk_config());
        Ok(Self {
            client: Arc::new(client),
        })
    }

    /// Invokes a Lambda function synchronously.
    ///
    /// # Errors
    ///
    /// Returns an error if the function invocation fails.
    pub async fn invoke_sync(&self, function_name: &str, payload: &[u8]) -> Result<LambdaResponse> {
        let response = self
            .client
            .invoke()
            .function_name(function_name)
            .invocation_type(InvocationType::RequestResponse)
            .payload(Blob::new(payload))
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::aws_service(format!("Failed to invoke Lambda: {}", e))
            })?;

        let status_code = response.status_code();
        let function_error = response.function_error;
        let payload = response
            .payload
            .map(|blob| blob.into_inner())
            .unwrap_or_default();

        if let Some(error) = function_error {
            return Err(CloudEnhancedError::aws_service(format!(
                "Lambda function error: {}",
                error
            )));
        }

        Ok(LambdaResponse {
            status_code,
            payload,
            log_result: response.log_result,
        })
    }

    /// Invokes a Lambda function asynchronously.
    ///
    /// # Errors
    ///
    /// Returns an error if the function invocation fails.
    pub async fn invoke_async(&self, function_name: &str, payload: &[u8]) -> Result<()> {
        self.client
            .invoke()
            .function_name(function_name)
            .invocation_type(InvocationType::Event)
            .payload(Blob::new(payload))
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::aws_service(format!(
                    "Failed to invoke Lambda asynchronously: {}",
                    e
                ))
            })?;

        Ok(())
    }

    /// Invokes a Lambda function with JSON payload.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or invocation fails.
    pub async fn invoke_json<T: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        function_name: &str,
        payload: &T,
    ) -> Result<R> {
        let payload_bytes = serde_json::to_vec(payload)?;
        let response = self.invoke_sync(function_name, &payload_bytes).await?;

        serde_json::from_slice(&response.payload).map_err(|e| {
            CloudEnhancedError::serialization(format!("Failed to deserialize response: {}", e))
        })
    }

    /// Creates a new Lambda function.
    ///
    /// # Errors
    ///
    /// Returns an error if the function cannot be created.
    pub async fn create_function(&self, config: FunctionConfig) -> Result<String> {
        let function_code = aws_sdk_lambda::types::FunctionCode::builder()
            .set_zip_file(config.zip_file.map(Blob::new))
            .set_s3_bucket(config.s3_bucket)
            .set_s3_key(config.s3_key)
            .build();

        let mut request = self
            .client
            .create_function()
            .function_name(&config.name)
            .runtime(config.runtime)
            .role(&config.role)
            .handler(&config.handler)
            .code(function_code)
            .timeout(config.timeout.unwrap_or(300))
            .memory_size(config.memory_size.unwrap_or(128));

        if let Some(desc) = config.description {
            request = request.description(desc);
        }

        if !config.environment_variables.is_empty() {
            let env = Environment::builder()
                .set_variables(Some(config.environment_variables))
                .build();
            request = request.environment(env);
        }

        let response = request.send().await.map_err(|e| {
            CloudEnhancedError::aws_service(format!("Failed to create Lambda function: {}", e))
        })?;

        response
            .function_arn
            .ok_or_else(|| CloudEnhancedError::aws_service("No function ARN returned".to_string()))
    }

    /// Updates a Lambda function's code.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails.
    pub async fn update_function_code(
        &self,
        function_name: &str,
        zip_file: Option<Vec<u8>>,
        s3_bucket: Option<String>,
        s3_key: Option<String>,
    ) -> Result<()> {
        let mut request = self
            .client
            .update_function_code()
            .function_name(function_name);

        if let Some(zip) = zip_file {
            request = request.zip_file(Blob::new(zip));
        }

        if let Some(bucket) = s3_bucket {
            request = request.s3_bucket(bucket);
        }

        if let Some(key) = s3_key {
            request = request.s3_key(key);
        }

        request.send().await.map_err(|e| {
            CloudEnhancedError::aws_service(format!("Failed to update Lambda code: {}", e))
        })?;

        Ok(())
    }

    /// Updates a Lambda function's configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails.
    pub async fn update_function_configuration(
        &self,
        function_name: &str,
        timeout: Option<i32>,
        memory_size: Option<i32>,
        environment_variables: Option<HashMap<String, String>>,
    ) -> Result<()> {
        let mut request = self
            .client
            .update_function_configuration()
            .function_name(function_name);

        if let Some(t) = timeout {
            request = request.timeout(t);
        }

        if let Some(mem) = memory_size {
            request = request.memory_size(mem);
        }

        if let Some(env_vars) = environment_variables {
            let env = Environment::builder().set_variables(Some(env_vars)).build();
            request = request.environment(env);
        }

        request.send().await.map_err(|e| {
            CloudEnhancedError::aws_service(format!("Failed to update Lambda configuration: {}", e))
        })?;

        Ok(())
    }

    /// Deletes a Lambda function.
    ///
    /// # Errors
    ///
    /// Returns an error if the deletion fails.
    pub async fn delete_function(&self, function_name: &str) -> Result<()> {
        self.client
            .delete_function()
            .function_name(function_name)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::aws_service(format!("Failed to delete Lambda function: {}", e))
            })?;

        Ok(())
    }

    /// Gets information about a Lambda function.
    ///
    /// # Errors
    ///
    /// Returns an error if the function cannot be retrieved.
    pub async fn get_function(&self, function_name: &str) -> Result<FunctionInfo> {
        let response = self
            .client
            .get_function()
            .function_name(function_name)
            .send()
            .await
            .map_err(|e| {
                CloudEnhancedError::aws_service(format!("Failed to get Lambda function: {}", e))
            })?;

        let config = response.configuration.ok_or_else(|| {
            CloudEnhancedError::aws_service("No configuration returned".to_string())
        })?;

        Ok(FunctionInfo {
            name: config.function_name.unwrap_or_default(),
            arn: config.function_arn.unwrap_or_default(),
            runtime: config.runtime,
            handler: config.handler.unwrap_or_default(),
            role: config.role.unwrap_or_default(),
            timeout: config.timeout,
            memory_size: config.memory_size,
            last_modified: config.last_modified.unwrap_or_default(),
        })
    }

    /// Lists Lambda functions.
    ///
    /// # Errors
    ///
    /// Returns an error if the list cannot be retrieved.
    pub async fn list_functions(&self, max_items: Option<i32>) -> Result<Vec<String>> {
        let mut request = self.client.list_functions();

        if let Some(max) = max_items {
            request = request.max_items(max);
        }

        let response = request.send().await.map_err(|e| {
            CloudEnhancedError::aws_service(format!("Failed to list Lambda functions: {}", e))
        })?;

        Ok(response
            .functions
            .unwrap_or_default()
            .into_iter()
            .filter_map(|f| f.function_name)
            .collect())
    }
}

/// Lambda function response.
#[derive(Debug, Clone)]
pub struct LambdaResponse {
    /// HTTP status code
    pub status_code: i32,
    /// Response payload
    pub payload: Vec<u8>,
    /// Log result (base64 encoded)
    pub log_result: Option<String>,
}

/// Lambda function configuration.
#[derive(Debug, Clone)]
pub struct FunctionConfig {
    /// Function name
    pub name: String,
    /// Runtime
    pub runtime: Runtime,
    /// IAM role ARN
    pub role: String,
    /// Handler
    pub handler: String,
    /// Function description
    pub description: Option<String>,
    /// Timeout in seconds
    pub timeout: Option<i32>,
    /// Memory size in MB
    pub memory_size: Option<i32>,
    /// Environment variables
    pub environment_variables: HashMap<String, String>,
    /// Function code as ZIP file
    pub zip_file: Option<Vec<u8>>,
    /// S3 bucket for code
    pub s3_bucket: Option<String>,
    /// S3 key for code
    pub s3_key: Option<String>,
}

/// Lambda function information.
#[derive(Debug, Clone)]
pub struct FunctionInfo {
    /// Function name
    pub name: String,
    /// Function ARN
    pub arn: String,
    /// Runtime
    pub runtime: Option<Runtime>,
    /// Handler
    pub handler: String,
    /// IAM role ARN
    pub role: String,
    /// Timeout in seconds
    pub timeout: Option<i32>,
    /// Memory size in MB
    pub memory_size: Option<i32>,
    /// Last modified timestamp
    pub last_modified: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lambda_response() {
        let response = LambdaResponse {
            status_code: 200,
            payload: b"test".to_vec(),
            log_result: Some("logs".to_string()),
        };

        assert_eq!(response.status_code, 200);
        assert_eq!(response.payload, b"test");
    }

    #[test]
    fn test_function_config() {
        let config = FunctionConfig {
            name: "test-function".to_string(),
            runtime: Runtime::Python312,
            role: "arn:aws:iam::123456789012:role/lambda-role".to_string(),
            handler: "index.handler".to_string(),
            description: None,
            timeout: Some(30),
            memory_size: Some(256),
            environment_variables: HashMap::new(),
            zip_file: None,
            s3_bucket: None,
            s3_key: None,
        };

        assert_eq!(config.name, "test-function");
        assert_eq!(config.timeout, Some(30));
    }
}
