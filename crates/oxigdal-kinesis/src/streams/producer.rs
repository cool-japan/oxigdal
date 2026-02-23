//! Kinesis producer with KPL (Kinesis Producer Library) patterns

use crate::error::{KinesisError, Result};
use aws_sdk_kinesis::Client as KinesisClient;
use aws_smithy_types::Blob;
use bytes::Bytes;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

/// Maximum record size (1 MB)
const MAX_RECORD_SIZE: usize = 1_024 * 1024;

/// Maximum batch size (5 MB)
const MAX_BATCH_SIZE: usize = 5 * 1024 * 1024;

/// Maximum records per batch (500)
const MAX_BATCH_RECORDS: usize = 500;

/// Producer configuration
#[derive(Debug, Clone)]
pub struct ProducerConfig {
    /// Stream name
    pub stream_name: String,
    /// Buffer size for batching
    pub buffer_size: usize,
    /// Maximum batch size in bytes
    pub max_batch_size: usize,
    /// Maximum records per batch
    pub max_batch_records: usize,
    /// Batch linger time in milliseconds
    pub linger_ms: u64,
    /// Enable compression
    pub compression: CompressionType,
    /// Retry attempts
    pub retry_attempts: u32,
    /// Retry backoff base in milliseconds
    pub retry_backoff_ms: u64,
    /// Async aggregation enabled
    pub aggregation_enabled: bool,
}

impl Default for ProducerConfig {
    fn default() -> Self {
        Self {
            stream_name: String::new(),
            buffer_size: 1000,
            max_batch_size: MAX_BATCH_SIZE,
            max_batch_records: MAX_BATCH_RECORDS,
            linger_ms: 100,
            compression: CompressionType::None,
            retry_attempts: 3,
            retry_backoff_ms: 100,
            aggregation_enabled: true,
        }
    }
}

impl ProducerConfig {
    /// Creates a new producer configuration
    pub fn new(stream_name: impl Into<String>) -> Self {
        Self {
            stream_name: stream_name.into(),
            ..Default::default()
        }
    }

    /// Sets the buffer size
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }

    /// Sets the batch linger time
    pub fn with_linger_ms(mut self, ms: u64) -> Self {
        self.linger_ms = ms;
        self
    }

    /// Sets the compression type
    pub fn with_compression(mut self, compression: CompressionType) -> Self {
        self.compression = compression;
        self
    }

    /// Sets retry attempts
    pub fn with_retry_attempts(mut self, attempts: u32) -> Self {
        self.retry_attempts = attempts;
        self
    }

    /// Enables or disables aggregation
    pub fn with_aggregation(mut self, enabled: bool) -> Self {
        self.aggregation_enabled = enabled;
        self
    }
}

/// Compression type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionType {
    /// No compression
    None,
    /// Gzip compression
    #[cfg(feature = "compression")]
    Gzip,
    /// Zstd compression
    #[cfg(feature = "compression")]
    Zstd,
}

/// Record to be sent to Kinesis
#[derive(Debug, Clone)]
pub struct Record {
    /// Partition key
    pub partition_key: String,
    /// Data payload
    pub data: Bytes,
    /// Explicit hash key (optional)
    pub explicit_hash_key: Option<String>,
}

impl Record {
    /// Creates a new record
    pub fn new(partition_key: impl Into<String>, data: impl Into<Bytes>) -> Self {
        Self {
            partition_key: partition_key.into(),
            data: data.into(),
            explicit_hash_key: None,
        }
    }

    /// Sets the explicit hash key
    pub fn with_explicit_hash_key(mut self, hash_key: impl Into<String>) -> Self {
        self.explicit_hash_key = Some(hash_key.into());
        self
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

/// Producer for Kinesis Data Streams
pub struct Producer {
    client: Arc<KinesisClient>,
    #[allow(dead_code)]
    config: ProducerConfig,
    tx: mpsc::Sender<ProducerMessage>,
    metrics: Arc<ProducerMetrics>,
}

impl Producer {
    /// Creates a new producer
    pub async fn new(client: KinesisClient, config: ProducerConfig) -> Result<Self> {
        let (tx, rx) = mpsc::channel(config.buffer_size);
        let metrics = Arc::new(ProducerMetrics::default());

        let producer = Self {
            client: Arc::new(client),
            config: config.clone(),
            tx,
            metrics: Arc::clone(&metrics),
        };

        // Start background worker
        tokio::spawn(producer_worker(
            Arc::clone(&producer.client),
            config,
            rx,
            Arc::clone(&metrics),
        ));

        Ok(producer)
    }

    /// Sends a record to the stream
    pub async fn send(&self, record: Record) -> Result<()> {
        record.validate()?;

        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        self.tx
            .send(ProducerMessage::Record {
                record,
                response: response_tx,
            })
            .await
            .map_err(|_| KinesisError::InvalidState {
                message: "Producer worker has stopped".to_string(),
            })?;

        response_rx.await.map_err(|_| KinesisError::InvalidState {
            message: "Failed to receive response from producer worker".to_string(),
        })?
    }

    /// Sends a batch of records
    pub async fn send_batch(&self, records: Vec<Record>) -> Result<Vec<Result<()>>> {
        let mut results = Vec::with_capacity(records.len());

        for record in records {
            results.push(self.send(record).await);
        }

        Ok(results)
    }

    /// Flushes all pending records
    pub async fn flush(&self) -> Result<()> {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        self.tx
            .send(ProducerMessage::Flush {
                response: response_tx,
            })
            .await
            .map_err(|_| KinesisError::InvalidState {
                message: "Producer worker has stopped".to_string(),
            })?;

        response_rx.await.map_err(|_| KinesisError::InvalidState {
            message: "Failed to receive flush response".to_string(),
        })?
    }

    /// Gets producer metrics
    pub fn metrics(&self) -> &ProducerMetrics {
        &self.metrics
    }
}

/// Producer message
enum ProducerMessage {
    Record {
        record: Record,
        response: tokio::sync::oneshot::Sender<Result<()>>,
    },
    Flush {
        response: tokio::sync::oneshot::Sender<Result<()>>,
    },
}

/// Producer worker task
async fn producer_worker(
    client: Arc<KinesisClient>,
    config: ProducerConfig,
    mut rx: mpsc::Receiver<ProducerMessage>,
    metrics: Arc<ProducerMetrics>,
) {
    let mut batch = Vec::new();
    let mut batch_size = 0;
    let mut last_flush = Instant::now();

    loop {
        let timeout = Duration::from_millis(config.linger_ms);
        let should_flush = batch.len() >= config.max_batch_records
            || batch_size >= config.max_batch_size
            || last_flush.elapsed() >= timeout;

        if should_flush && !batch.is_empty() {
            flush_batch(&client, &config, &mut batch, &mut batch_size, &metrics).await;
            last_flush = Instant::now();
        }

        match tokio::time::timeout(timeout, rx.recv()).await {
            Ok(Some(ProducerMessage::Record { record, response })) => {
                let record_size = record.data.len();
                batch.push((record, response));
                batch_size += record_size;

                metrics.increment_records_received();
            }
            Ok(Some(ProducerMessage::Flush { response })) => {
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

    info!("Producer worker stopped");
}

/// Flushes a batch of records to Kinesis
async fn flush_batch(
    client: &KinesisClient,
    config: &ProducerConfig,
    batch: &mut Vec<(Record, tokio::sync::oneshot::Sender<Result<()>>)>,
    batch_size: &mut usize,
    metrics: &ProducerMetrics,
) {
    if batch.is_empty() {
        return;
    }

    debug!("Flushing batch of {} records", batch.len());

    let records: Result<Vec<_>> = batch
        .iter()
        .map(|(record, _)| {
            aws_sdk_kinesis::types::PutRecordsRequestEntry::builder()
                .partition_key(&record.partition_key)
                .data(Blob::new(record.data.clone()))
                .set_explicit_hash_key(record.explicit_hash_key.clone())
                .build()
                .map_err(|e| {
                    error!("Failed to build record: {}", e);
                    KinesisError::Service {
                        message: e.to_string(),
                    }
                })
        })
        .collect();

    let records = match records {
        Ok(r) => r,
        Err(e) => {
            for (_, sender) in batch.drain(..) {
                let _ = sender.send(Err(KinesisError::Service {
                    message: e.to_string(),
                }));
            }
            *batch_size = 0;
            return;
        }
    };

    // Send to Kinesis with retry logic
    let mut attempt = 0;
    loop {
        match client
            .put_records()
            .stream_name(&config.stream_name)
            .set_records(Some(records.clone()))
            .send()
            .await
        {
            Ok(aws_response) => {
                let failed_count = aws_response.failed_record_count().unwrap_or(0);
                metrics.increment_records_sent(batch.len() as u64 - failed_count as u64);

                if failed_count > 0 {
                    warn!("Failed to send {} records", failed_count);
                    metrics.increment_records_failed(failed_count as u64);
                }

                // Send responses
                let result_records = aws_response.records();
                for (idx, (_, sender)) in batch.drain(..).enumerate() {
                    if let Some(record_result) = result_records.get(idx) {
                        if let Some(error_code) = record_result.error_code() {
                            let error_message =
                                record_result.error_message().unwrap_or("Unknown error");
                            let _ = sender.send(Err(KinesisError::Service {
                                message: format!("{}: {}", error_code, error_message),
                            }));
                        } else {
                            let _ = sender.send(Ok(()));
                        }
                    } else {
                        let _ = sender.send(Ok(()));
                    }
                }

                *batch_size = 0;
                break;
            }
            Err(e) => {
                attempt += 1;
                if attempt >= config.retry_attempts {
                    error!("Failed to send batch after {} attempts: {}", attempt, e);
                    metrics.increment_records_failed(batch.len() as u64);

                    for (_, sender) in batch.drain(..) {
                        let _ = sender.send(Err(KinesisError::from_aws_error(&e)));
                    }
                    *batch_size = 0;
                    break;
                }

                let backoff_ms = config.retry_backoff_ms * 2_u64.pow(attempt - 1);
                warn!(
                    "Failed to send batch (attempt {}), retrying after {}ms: {}",
                    attempt, backoff_ms, e
                );
                tokio::time::sleep(tokio::time::Duration::from_millis(backoff_ms)).await;
            }
        }
    }
}

/// Producer metrics
#[derive(Default)]
pub struct ProducerMetrics {
    records_received: parking_lot::Mutex<u64>,
    records_sent: parking_lot::Mutex<u64>,
    records_failed: parking_lot::Mutex<u64>,
}

impl ProducerMetrics {
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
    fn test_record_creation() {
        let record = Record::new("partition-1", Bytes::from("test data"));
        assert_eq!(record.partition_key, "partition-1");
        assert_eq!(record.data, Bytes::from("test data"));
        assert!(record.explicit_hash_key.is_none());
    }

    #[test]
    fn test_record_with_explicit_hash() {
        let record =
            Record::new("partition-1", Bytes::from("test data")).with_explicit_hash_key("hash-123");
        assert!(record.explicit_hash_key.is_some());
    }

    #[test]
    fn test_record_validation() {
        let record = Record::new("partition-1", Bytes::from("test data"));
        assert!(record.validate().is_ok());

        let large_data = vec![0u8; MAX_RECORD_SIZE + 1];
        let large_record = Record::new("partition-1", Bytes::from(large_data));
        assert!(large_record.validate().is_err());
    }

    #[test]
    fn test_producer_config() {
        let config = ProducerConfig::new("test-stream")
            .with_buffer_size(500)
            .with_linger_ms(200)
            .with_retry_attempts(5);

        assert_eq!(config.stream_name, "test-stream");
        assert_eq!(config.buffer_size, 500);
        assert_eq!(config.linger_ms, 200);
        assert_eq!(config.retry_attempts, 5);
    }

    #[test]
    fn test_producer_metrics() {
        let metrics = ProducerMetrics::default();
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
