//! Kinesis Firehose module for delivery streams

pub mod delivery;
pub mod destination;
pub mod transform;

pub use delivery::{DeliveryStream, DeliveryStreamConfig, FirehoseRecord};
pub use destination::{S3Destination, S3DestinationConfig};
pub use transform::{LambdaTransformer, TransformResult, Transformer};

use crate::error::{KinesisError, Result};
use aws_sdk_firehose::Client as FirehoseClient;
use std::sync::Arc;

/// Kinesis Firehose client wrapper
#[derive(Clone)]
pub struct KinesisFirehose {
    client: Arc<FirehoseClient>,
}

impl KinesisFirehose {
    /// Creates a new Kinesis Firehose client
    pub fn new(client: FirehoseClient) -> Self {
        Self {
            client: Arc::new(client),
        }
    }

    /// Creates a new Kinesis Firehose client from environment
    pub async fn from_env() -> Self {
        let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let client = FirehoseClient::new(&config);
        Self::new(client)
    }

    /// Gets a reference to the Firehose client
    pub fn client(&self) -> &FirehoseClient {
        &self.client
    }

    /// Lists all delivery streams
    pub async fn list_delivery_streams(&self) -> Result<Vec<String>> {
        let response = self
            .client
            .list_delivery_streams()
            .send()
            .await
            .map_err(|e| KinesisError::Firehose {
                message: e.to_string(),
            })?;

        Ok(response
            .delivery_stream_names()
            .iter()
            .map(|s| s.to_string())
            .collect())
    }

    /// Describes a delivery stream
    pub async fn describe_delivery_stream(
        &self,
        delivery_stream_name: &str,
    ) -> Result<DeliveryStreamDescription> {
        let response = self
            .client
            .describe_delivery_stream()
            .delivery_stream_name(delivery_stream_name)
            .send()
            .await
            .map_err(|e| KinesisError::Firehose {
                message: e.to_string(),
            })?;

        let description =
            response
                .delivery_stream_description()
                .ok_or_else(|| KinesisError::Firehose {
                    message: "Delivery stream description not found".to_string(),
                })?;

        Ok(DeliveryStreamDescription {
            delivery_stream_name: description.delivery_stream_name().to_string(),
            delivery_stream_arn: description.delivery_stream_arn().to_string(),
            delivery_stream_status: Some(description.delivery_stream_status().as_str().to_string()),
            delivery_stream_type: Some(description.delivery_stream_type().as_str().to_string()),
        })
    }

    /// Creates a delivery stream
    pub async fn create_delivery_stream(&self, config: &DeliveryStreamConfig) -> Result<String> {
        let delivery_stream = DeliveryStream::new(self.client.as_ref().clone(), config.clone());
        delivery_stream.create().await
    }

    /// Deletes a delivery stream
    pub async fn delete_delivery_stream(&self, delivery_stream_name: &str) -> Result<()> {
        self.client
            .delete_delivery_stream()
            .delivery_stream_name(delivery_stream_name)
            .send()
            .await
            .map_err(|e| KinesisError::Firehose {
                message: e.to_string(),
            })?;

        Ok(())
    }
}

/// Delivery stream description
#[derive(Debug, Clone)]
pub struct DeliveryStreamDescription {
    /// Delivery stream name
    pub delivery_stream_name: String,
    /// Delivery stream ARN
    pub delivery_stream_arn: String,
    /// Delivery stream status
    pub delivery_stream_status: Option<String>,
    /// Delivery stream type
    pub delivery_stream_type: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delivery_stream_description() {
        let desc = DeliveryStreamDescription {
            delivery_stream_name: "test-stream".to_string(),
            delivery_stream_arn:
                "arn:aws:firehose:us-east-1:123456789012:deliverystream/test-stream".to_string(),
            delivery_stream_status: Some("ACTIVE".to_string()),
            delivery_stream_type: Some("DirectPut".to_string()),
        };

        assert_eq!(desc.delivery_stream_name, "test-stream");
        assert_eq!(desc.delivery_stream_status, Some("ACTIVE".to_string()));
    }
}
