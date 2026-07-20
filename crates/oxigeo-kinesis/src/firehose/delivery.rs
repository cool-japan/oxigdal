//! Delivery stream implementation

use crate::error::{KinesisError, Result};
use crate::firehose::destination::S3DestinationConfig;
use aws_sdk_firehose::Client as FirehoseClient;
use aws_smithy_types::Blob;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

/// Maximum record size (1 MB)
const MAX_RECORD_SIZE: usize = 1_024 * 1024;

/// Maximum batch size (4 MB)
const MAX_BATCH_SIZE: usize = 4 * 1024 * 1024;

/// Maximum records per batch (500)
const MAX_BATCH_RECORDS: usize = 500;

/// Delivery stream configuration
#[derive(Debug, Clone)]
pub struct DeliveryStreamConfig {
    /// Delivery stream name
    pub delivery_stream_name: String,
    /// S3 destination configuration
    pub s3_destination: Option<S3DestinationConfig>,
    /// Buffer size in MB
    pub buffer_size_mb: i32,
    /// Buffer interval in seconds
    pub buffer_interval_seconds: i32,
    /// Compression format
    pub compression_format: CompressionFormat,
    /// Error output prefix (for S3)
    pub error_output_prefix: Option<String>,
    /// Enable data transformation
    pub enable_transformation: bool,
    /// Lambda ARN for transformation
    pub transformation_lambda_arn: Option<String>,
}

impl Default for DeliveryStreamConfig {
    fn default() -> Self {
        Self {
            delivery_stream_name: String::new(),
            s3_destination: None,
            buffer_size_mb: 5,
            buffer_interval_seconds: 300,
            compression_format: CompressionFormat::None,
            error_output_prefix: None,
            enable_transformation: false,
            transformation_lambda_arn: None,
        }
    }
}

impl DeliveryStreamConfig {
    /// Creates a new delivery stream configuration
    pub fn new(delivery_stream_name: impl Into<String>) -> Self {
        Self {
            delivery_stream_name: delivery_stream_name.into(),
            ..Default::default()
        }
    }

    /// Sets the S3 destination
    pub fn with_s3_destination(mut self, config: S3DestinationConfig) -> Self {
        self.s3_destination = Some(config);
        self
    }

    /// Sets the buffer size
    pub fn with_buffer_size_mb(mut self, size_mb: i32) -> Self {
        self.buffer_size_mb = size_mb;
        self
    }

    /// Sets the buffer interval
    pub fn with_buffer_interval_seconds(mut self, seconds: i32) -> Self {
        self.buffer_interval_seconds = seconds;
        self
    }

    /// Sets the compression format
    pub fn with_compression(mut self, format: CompressionFormat) -> Self {
        self.compression_format = format;
        self
    }

    /// Enables data transformation
    pub fn with_transformation(mut self, lambda_arn: impl Into<String>) -> Self {
        self.enable_transformation = true;
        self.transformation_lambda_arn = Some(lambda_arn.into());
        self
    }
}

/// Compression format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionFormat {
    /// No compression
    None,
    /// Gzip compression
    Gzip,
    /// Snappy compression
    Snappy,
    /// Zip compression
    Zip,
}

impl CompressionFormat {
    /// Converts to AWS SDK compression type
    pub fn to_aws_compression(&self) -> &str {
        match self {
            Self::None => "UNCOMPRESSED",
            Self::Gzip => "GZIP",
            Self::Snappy => "SNAPPY",
            Self::Zip => "ZIP",
        }
    }
}

/// Record to be delivered via Firehose
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirehoseRecord {
    /// Data payload
    pub data: Bytes,
}

impl FirehoseRecord {
    /// Creates a new Firehose record
    pub fn new(data: impl Into<Bytes>) -> Self {
        Self { data: data.into() }
    }

    /// Validates the record size
    pub fn validate(&self) -> Result<()> {
        let size = self.data.len();
        if size > MAX_RECORD_SIZE {
            return Err(KinesisError::RecordTooLarge {
                size,
                max_size: MAX_RECORD_SIZE,
            });
        }
        Ok(())
    }
}

/// Delivery stream
pub struct DeliveryStream {
    client: Arc<FirehoseClient>,
    config: DeliveryStreamConfig,
    tx: Option<mpsc::Sender<DeliveryMessage>>,
    metrics: Arc<DeliveryMetrics>,
}

impl DeliveryStream {
    /// Creates a new delivery stream
    pub fn new(client: FirehoseClient, config: DeliveryStreamConfig) -> Self {
        Self {
            client: Arc::new(client),
            config,
            tx: None,
            metrics: Arc::new(DeliveryMetrics::default()),
        }
    }

    /// Starts the delivery stream with buffering
    pub async fn start(&mut self) -> Result<()> {
        let (tx, rx) = mpsc::channel(1000);
        self.tx = Some(tx);

        // Start background worker
        tokio::spawn(delivery_worker(
            Arc::clone(&self.client),
            self.config.clone(),
            rx,
            Arc::clone(&self.metrics),
        ));

        Ok(())
    }

    /// Creates the delivery stream in AWS
    pub async fn create(&self) -> Result<String> {
        info!(
            "Creating delivery stream: {}",
            self.config.delivery_stream_name
        );

        let s3_config =
            self.config
                .s3_destination
                .as_ref()
                .ok_or_else(|| KinesisError::InvalidConfig {
                    message: "S3 destination required".to_string(),
                })?;

        let s3_destination = aws_sdk_firehose::types::ExtendedS3DestinationConfiguration::builder()
            .bucket_arn(&s3_config.bucket_arn)
            .role_arn(&s3_config.role_arn)
            .prefix(&s3_config.prefix)
            .buffering_hints(
                aws_sdk_firehose::types::BufferingHints::builder()
                    .size_in_mbs(self.config.buffer_size_mb)
                    .interval_in_seconds(self.config.buffer_interval_seconds)
                    .build(),
            )
            .compression_format(aws_sdk_firehose::types::CompressionFormat::from(
                self.config.compression_format.to_aws_compression(),
            ))
            .build()
            .map_err(|e| KinesisError::Firehose {
                message: e.to_string(),
            })?;

        let response = self
            .client
            .create_delivery_stream()
            .delivery_stream_name(&self.config.delivery_stream_name)
            .delivery_stream_type(aws_sdk_firehose::types::DeliveryStreamType::DirectPut)
            .extended_s3_destination_configuration(s3_destination)
            .send()
            .await
            .map_err(|e| KinesisError::Firehose {
                message: e.to_string(),
            })?;

        let arn = response
            .delivery_stream_arn()
            .unwrap_or_default()
            .to_string();
        info!("Delivery stream created: {}", arn);

        Ok(arn)
    }

    /// Sends a record to the delivery stream
    pub async fn send_record(&self, record: FirehoseRecord) -> Result<()> {
        record.validate()?;

        if let Some(tx) = &self.tx {
            let (response_tx, response_rx) = tokio::sync::oneshot::channel();

            tx.send(DeliveryMessage::Record {
                record,
                response: response_tx,
            })
            .await
            .map_err(|_| KinesisError::InvalidState {
                message: "Delivery worker has stopped".to_string(),
            })?;

            response_rx.await.map_err(|_| KinesisError::InvalidState {
                message: "Failed to receive response from delivery worker".to_string(),
            })?
        } else {
            // Direct send without buffering
            self.send_record_direct(record).await
        }
    }

    /// Sends a record directly without buffering
    async fn send_record_direct(&self, record: FirehoseRecord) -> Result<()> {
        let aws_record = aws_sdk_firehose::types::Record::builder()
            .data(Blob::new(record.data))
            .build()
            .map_err(|e| KinesisError::Firehose {
                message: e.to_string(),
            })?;

        self.client
            .put_record()
            .delivery_stream_name(&self.config.delivery_stream_name)
            .record(aws_record)
            .send()
            .await
            .map_err(|e| KinesisError::Firehose {
                message: e.to_string(),
            })?;

        self.metrics.increment_records_sent(1);
        Ok(())
    }

    /// Sends a batch of records
    pub async fn send_batch(&self, records: Vec<FirehoseRecord>) -> Result<Vec<Result<()>>> {
        let mut results = Vec::with_capacity(records.len());

        for record in records {
            results.push(self.send_record(record).await);
        }

        Ok(results)
    }

    /// Flushes all pending records
    pub async fn flush(&self) -> Result<()> {
        if let Some(tx) = &self.tx {
            let (response_tx, response_rx) = tokio::sync::oneshot::channel();

            tx.send(DeliveryMessage::Flush {
                response: response_tx,
            })
            .await
            .map_err(|_| KinesisError::InvalidState {
                message: "Delivery worker has stopped".to_string(),
            })?;

            response_rx.await.map_err(|_| KinesisError::InvalidState {
                message: "Failed to receive flush response".to_string(),
            })?
        } else {
            Ok(())
        }
    }

    /// Gets delivery metrics
    pub fn metrics(&self) -> &DeliveryMetrics {
        &self.metrics
    }
}

/// Delivery message
enum DeliveryMessage {
    Record {
        record: FirehoseRecord,
        response: tokio::sync::oneshot::Sender<Result<()>>,
    },
    Flush {
        response: tokio::sync::oneshot::Sender<Result<()>>,
    },
}

/// Delivery worker task
async fn delivery_worker(
    client: Arc<FirehoseClient>,
    config: DeliveryStreamConfig,
    mut rx: mpsc::Receiver<DeliveryMessage>,
    metrics: Arc<DeliveryMetrics>,
) {
    let mut batch = Vec::new();
    let mut batch_size = 0;
    let mut last_flush = Instant::now();
    let flush_interval = Duration::from_secs(config.buffer_interval_seconds as u64);

    loop {
        let should_flush = batch.len() >= MAX_BATCH_RECORDS
            || batch_size >= MAX_BATCH_SIZE
            || last_flush.elapsed() >= flush_interval;

        if should_flush && !batch.is_empty() {
            flush_batch(&client, &config, &mut batch, &mut batch_size, &metrics).await;
            last_flush = Instant::now();
        }

        match tokio::time::timeout(flush_interval, rx.recv()).await {
            Ok(Some(DeliveryMessage::Record { record, response })) => {
                let record_size = record.data.len();
                batch.push((record, response));
                batch_size += record_size;

                metrics.increment_records_received();
            }
            Ok(Some(DeliveryMessage::Flush { response })) => {
                if !batch.is_empty() {
                    flush_batch(&client, &config, &mut batch, &mut batch_size, &metrics).await;
                }
                let _ = response.send(Ok(()));
            }
            Ok(None) => {
                // Channel closed, flush remaining and exit
                if !batch.is_empty() {
                    flush_batch(&client, &config, &mut batch, &mut batch_size, &metrics).await;
                }
                break;
            }
            Err(_) => {
                // Timeout, will check should_flush in next iteration
            }
        }
    }

    info!("Delivery worker stopped");
}

/// Flushes a batch of records to Firehose
async fn flush_batch(
    client: &FirehoseClient,
    config: &DeliveryStreamConfig,
    batch: &mut Vec<(FirehoseRecord, tokio::sync::oneshot::Sender<Result<()>>)>,
    batch_size: &mut usize,
    metrics: &DeliveryMetrics,
) {
    if batch.is_empty() {
        return;
    }

    debug!("Flushing batch of {} records", batch.len());

    let records: Result<Vec<_>> = batch
        .iter()
        .map(|(record, _)| {
            aws_sdk_firehose::types::Record::builder()
                .data(Blob::new(record.data.clone()))
                .build()
                .map_err(|e| KinesisError::Firehose {
                    message: format!("Failed to build record: {:?}", e),
                })
        })
        .collect();

    let records = match records {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to build records: {}", e);
            for (_, response) in batch.drain(..) {
                let _ = response.send(Err(e.clone()));
            }
            *batch_size = 0;
            return;
        }
    };

    match client
        .put_record_batch()
        .delivery_stream_name(&config.delivery_stream_name)
        .set_records(Some(records))
        .send()
        .await
    {
        Ok(aws_response) => {
            let failed_count = aws_response.failed_put_count();
            metrics.increment_records_sent(batch.len() as u64 - failed_count as u64);

            if failed_count > 0 {
                warn!("Failed to deliver {} records", failed_count);
                metrics.increment_records_failed(failed_count as u64);
            }

            // Send responses
            for (idx, (_, response)) in batch.drain(..).enumerate() {
                if let Some(record_result) = aws_response.request_responses().get(idx) {
                    if record_result.error_code().is_some() {
                        let error_message = record_result
                            .error_message()
                            .unwrap_or("Unknown error")
                            .to_string();
                        let _ = response.send(Err(KinesisError::Firehose {
                            message: error_message,
                        }));
                    } else {
                        let _ = response.send(Ok(()));
                    }
                } else {
                    let _ = response.send(Ok(()));
                }
            }

            *batch_size = 0;
        }
        Err(e) => {
            error!("Failed to send batch: {}", e);
            metrics.increment_records_failed(batch.len() as u64);

            for (_, response) in batch.drain(..) {
                let _ = response.send(Err(KinesisError::Firehose {
                    message: e.to_string(),
                }));
            }
            *batch_size = 0;
        }
    }
}

/// Delivery metrics
#[derive(Default)]
pub struct DeliveryMetrics {
    records_received: parking_lot::Mutex<u64>,
    records_sent: parking_lot::Mutex<u64>,
    records_failed: parking_lot::Mutex<u64>,
}

impl DeliveryMetrics {
    fn increment_records_received(&self) {
        *self.records_received.lock() += 1;
    }

    fn increment_records_sent(&self, count: u64) {
        *self.records_sent.lock() += count;
    }

    fn increment_records_failed(&self, count: u64) {
        *self.records_failed.lock() += count;
    }

    /// Gets the number of records received
    pub fn records_received(&self) -> u64 {
        *self.records_received.lock()
    }

    /// Gets the number of records sent
    pub fn records_sent(&self) -> u64 {
        *self.records_sent.lock()
    }

    /// Gets the number of records failed
    pub fn records_failed(&self) -> u64 {
        *self.records_failed.lock()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_firehose_record_creation() {
        let record = FirehoseRecord::new(Bytes::from("test data"));
        assert_eq!(record.data, Bytes::from("test data"));
    }

    #[test]
    fn test_firehose_record_validation() {
        let record = FirehoseRecord::new(Bytes::from("test data"));
        assert!(record.validate().is_ok());

        let large_data = vec![0u8; MAX_RECORD_SIZE + 1];
        let large_record = FirehoseRecord::new(Bytes::from(large_data));
        assert!(large_record.validate().is_err());
    }

    #[test]
    fn test_delivery_stream_config() {
        let config = DeliveryStreamConfig::new("test-stream")
            .with_buffer_size_mb(10)
            .with_buffer_interval_seconds(60)
            .with_compression(CompressionFormat::Gzip);

        assert_eq!(config.delivery_stream_name, "test-stream");
        assert_eq!(config.buffer_size_mb, 10);
        assert_eq!(config.buffer_interval_seconds, 60);
        assert_eq!(config.compression_format, CompressionFormat::Gzip);
    }

    #[test]
    fn test_compression_format_conversion() {
        assert_eq!(CompressionFormat::None.to_aws_compression(), "UNCOMPRESSED");
        assert_eq!(CompressionFormat::Gzip.to_aws_compression(), "GZIP");
        assert_eq!(CompressionFormat::Snappy.to_aws_compression(), "SNAPPY");
        assert_eq!(CompressionFormat::Zip.to_aws_compression(), "ZIP");
    }

    #[test]
    fn test_delivery_metrics() {
        let metrics = DeliveryMetrics::default();
        assert_eq!(metrics.records_received(), 0);
        assert_eq!(metrics.records_sent(), 0);
        assert_eq!(metrics.records_failed(), 0);

        metrics.increment_records_received();
        assert_eq!(metrics.records_received(), 1);

        metrics.increment_records_sent(10);
        assert_eq!(metrics.records_sent(), 10);

        metrics.increment_records_failed(2);
        assert_eq!(metrics.records_failed(), 2);
    }
}
