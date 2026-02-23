//! Data transformation for Firehose delivery streams

use crate::error::{KinesisError, Result};
use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};

/// Transform result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransformResult {
    /// Successfully transformed data
    Ok(Bytes),
    /// Failed transformation
    Failed,
    /// Dropped record
    Dropped,
}

/// Transformer trait for data transformation
#[async_trait]
pub trait Transformer: Send + Sync {
    /// Transforms a record
    async fn transform(&self, data: &[u8]) -> Result<TransformResult>;

    /// Transforms a batch of records
    async fn transform_batch(&self, records: Vec<&[u8]>) -> Result<Vec<TransformResult>> {
        let mut results = Vec::with_capacity(records.len());
        for record in records {
            results.push(self.transform(record).await?);
        }
        Ok(results)
    }
}

/// Lambda transformer (invokes AWS Lambda for transformation)
pub struct LambdaTransformer {
    lambda_arn: String,
}

impl LambdaTransformer {
    /// Creates a new Lambda transformer
    pub fn new(lambda_arn: impl Into<String>) -> Self {
        Self {
            lambda_arn: lambda_arn.into(),
        }
    }

    /// Gets the Lambda ARN
    pub fn lambda_arn(&self) -> &str {
        &self.lambda_arn
    }
}

#[async_trait]
impl Transformer for LambdaTransformer {
    async fn transform(&self, data: &[u8]) -> Result<TransformResult> {
        // In a real implementation, this would invoke Lambda
        // For now, this is a placeholder
        Ok(TransformResult::Ok(Bytes::copy_from_slice(data)))
    }
}

/// Identity transformer (no transformation)
pub struct IdentityTransformer;

#[async_trait]
impl Transformer for IdentityTransformer {
    async fn transform(&self, data: &[u8]) -> Result<TransformResult> {
        Ok(TransformResult::Ok(Bytes::copy_from_slice(data)))
    }
}

/// JSON transformer (adds metadata to JSON records)
pub struct JsonTransformer {
    add_timestamp: bool,
    add_sequence: bool,
    sequence_counter: parking_lot::Mutex<u64>,
}

impl JsonTransformer {
    /// Creates a new JSON transformer
    pub fn new() -> Self {
        Self {
            add_timestamp: false,
            add_sequence: false,
            sequence_counter: parking_lot::Mutex::new(0),
        }
    }

    /// Enables timestamp addition
    pub fn with_timestamp(mut self) -> Self {
        self.add_timestamp = true;
        self
    }

    /// Enables sequence number addition
    pub fn with_sequence(mut self) -> Self {
        self.add_sequence = true;
        self
    }
}

impl Default for JsonTransformer {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Transformer for JsonTransformer {
    async fn transform(&self, data: &[u8]) -> Result<TransformResult> {
        // Parse JSON
        let mut value: serde_json::Value =
            serde_json::from_slice(data).map_err(|e| KinesisError::Deserialization {
                message: e.to_string(),
            })?;

        // Add timestamp if enabled
        if self.add_timestamp {
            if let Some(obj) = value.as_object_mut() {
                obj.insert(
                    "_timestamp".to_string(),
                    serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
                );
            }
        }

        // Add sequence if enabled
        if self.add_sequence {
            if let Some(obj) = value.as_object_mut() {
                let seq = {
                    let mut counter = self.sequence_counter.lock();
                    *counter += 1;
                    *counter
                };
                obj.insert(
                    "_sequence".to_string(),
                    serde_json::Value::Number(seq.into()),
                );
            }
        }

        // Serialize back to JSON
        let transformed = serde_json::to_vec(&value).map_err(|e| KinesisError::Serialization {
            message: e.to_string(),
        })?;

        Ok(TransformResult::Ok(Bytes::from(transformed)))
    }
}

/// CSV to JSON transformer
pub struct CsvToJsonTransformer {
    headers: Vec<String>,
}

impl CsvToJsonTransformer {
    /// Creates a new CSV to JSON transformer
    pub fn new(headers: Vec<String>) -> Self {
        Self { headers }
    }
}

#[async_trait]
impl Transformer for CsvToJsonTransformer {
    async fn transform(&self, data: &[u8]) -> Result<TransformResult> {
        // Parse CSV line
        let line = std::str::from_utf8(data)
            .map_err(|e| KinesisError::Deserialization {
                message: e.to_string(),
            })?
            .trim();

        let values: Vec<&str> = line.split(',').collect();

        if values.len() != self.headers.len() {
            return Ok(TransformResult::Failed);
        }

        // Build JSON object
        let mut obj = serde_json::Map::new();
        for (header, value) in self.headers.iter().zip(values.iter()) {
            obj.insert(header.clone(), serde_json::Value::String(value.to_string()));
        }

        let json = serde_json::to_vec(&obj).map_err(|e| KinesisError::Serialization {
            message: e.to_string(),
        })?;

        Ok(TransformResult::Ok(Bytes::from(json)))
    }
}

/// Filter transformer (filters records based on predicate)
pub struct FilterTransformer<F>
where
    F: Fn(&[u8]) -> bool + Send + Sync,
{
    predicate: F,
}

impl<F> FilterTransformer<F>
where
    F: Fn(&[u8]) -> bool + Send + Sync,
{
    /// Creates a new filter transformer
    pub fn new(predicate: F) -> Self {
        Self { predicate }
    }
}

#[async_trait]
impl<F> Transformer for FilterTransformer<F>
where
    F: Fn(&[u8]) -> bool + Send + Sync,
{
    async fn transform(&self, data: &[u8]) -> Result<TransformResult> {
        if (self.predicate)(data) {
            Ok(TransformResult::Ok(Bytes::copy_from_slice(data)))
        } else {
            Ok(TransformResult::Dropped)
        }
    }
}

/// Compression transformer
#[cfg(feature = "compression")]
pub struct CompressionTransformer {
    compression_type: CompressionType,
}

#[cfg(feature = "compression")]
#[derive(Debug, Clone, Copy)]
/// Compression type for data transformation
pub enum CompressionType {
    /// GZIP compression
    Gzip,
    /// Zstandard compression
    Zstd,
}

#[cfg(feature = "compression")]
impl CompressionTransformer {
    /// Creates a new compression transformer
    pub fn new(compression_type: CompressionType) -> Self {
        Self { compression_type }
    }
}

#[cfg(feature = "compression")]
#[async_trait]
impl Transformer for CompressionTransformer {
    async fn transform(&self, data: &[u8]) -> Result<TransformResult> {
        use std::io::Write;

        let compressed = match self.compression_type {
            CompressionType::Gzip => {
                let mut encoder =
                    flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                encoder
                    .write_all(data)
                    .map_err(|e| KinesisError::Compression {
                        message: e.to_string(),
                    })?;
                encoder.finish().map_err(|e| KinesisError::Compression {
                    message: e.to_string(),
                })?
            }
            CompressionType::Zstd => {
                let mut encoder =
                    zstd::Encoder::new(Vec::new(), 3).map_err(|e| KinesisError::Compression {
                        message: e.to_string(),
                    })?;
                encoder
                    .write_all(data)
                    .map_err(|e| KinesisError::Compression {
                        message: e.to_string(),
                    })?;
                encoder.finish().map_err(|e| KinesisError::Compression {
                    message: e.to_string(),
                })?
            }
        };

        Ok(TransformResult::Ok(Bytes::from(compressed)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_identity_transformer() {
        let transformer = IdentityTransformer;
        let data = b"test data";
        let result = transformer.transform(data).await.ok();

        assert!(
            matches!(result, Some(TransformResult::Ok(_))),
            "Expected Ok result"
        );
        if let Some(TransformResult::Ok(transformed)) = result {
            assert_eq!(transformed, Bytes::from("test data"));
        }
    }

    #[tokio::test]
    async fn test_json_transformer() {
        let transformer = JsonTransformer::new().with_timestamp().with_sequence();
        let data = br#"{"key":"value"}"#;
        let result = transformer.transform(data).await.ok();

        assert!(
            matches!(result, Some(TransformResult::Ok(_))),
            "Expected Ok result"
        );
        if let Some(TransformResult::Ok(transformed)) = result {
            let json: serde_json::Value = serde_json::from_slice(&transformed)
                .ok()
                .flatten()
                .unwrap_or_default();
            assert!(json.get("_timestamp").is_some());
            assert!(json.get("_sequence").is_some());
            assert_eq!(json.get("key").and_then(|v| v.as_str()), Some("value"));
        }
    }

    #[tokio::test]
    async fn test_csv_to_json_transformer() {
        let transformer = CsvToJsonTransformer::new(vec![
            "name".to_string(),
            "age".to_string(),
            "city".to_string(),
        ]);
        let data = b"John,30,NYC";
        let result = transformer.transform(data).await.ok();

        assert!(
            matches!(result, Some(TransformResult::Ok(_))),
            "Expected Ok result"
        );
        if let Some(TransformResult::Ok(transformed)) = result {
            let json: serde_json::Value = serde_json::from_slice(&transformed)
                .ok()
                .flatten()
                .unwrap_or_default();
            assert_eq!(json.get("name").and_then(|v| v.as_str()), Some("John"));
            assert_eq!(json.get("age").and_then(|v| v.as_str()), Some("30"));
            assert_eq!(json.get("city").and_then(|v| v.as_str()), Some("NYC"));
        }
    }

    #[tokio::test]
    async fn test_filter_transformer() {
        let transformer = FilterTransformer::new(|data| {
            std::str::from_utf8(data)
                .ok()
                .map(|s| s.contains("accept"))
                .unwrap_or(false)
        });

        let accept_data = b"This should accept";
        let result = transformer.transform(accept_data).await.ok();
        assert!(matches!(result, Some(TransformResult::Ok(_))));

        let reject_data = b"This should reject";
        let result = transformer.transform(reject_data).await.ok();
        assert!(matches!(result, Some(TransformResult::Dropped)));
    }

    #[test]
    fn test_lambda_transformer_creation() {
        let transformer =
            LambdaTransformer::new("arn:aws:lambda:us-east-1:123456789012:function:my-function");
        assert_eq!(
            transformer.lambda_arn(),
            "arn:aws:lambda:us-east-1:123456789012:function:my-function"
        );
    }
}
