//! Data sink implementations for ETL pipelines
//!
//! This module provides various data sink implementations including file sinks,
//! S3/Azure/GCS sinks, PostGIS sinks, Kafka sinks, and custom sinks.

use crate::error::{Result, SinkError};
use crate::stream::StreamItem;
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

/// Data sink trait
#[async_trait]
pub trait Sink: Send + Sync {
    /// Write a single item to the sink
    async fn write(&self, item: StreamItem) -> Result<()>;

    /// Write a batch of items to the sink
    async fn write_batch(&self, items: Vec<StreamItem>) -> Result<()> {
        for item in items {
            self.write(item).await?;
        }
        Ok(())
    }

    /// Flush any buffered data
    async fn flush(&self) -> Result<()> {
        Ok(())
    }

    /// Close the sink
    async fn close(&self) -> Result<()> {
        self.flush().await
    }

    /// Get sink name for logging
    fn name(&self) -> &str;

    /// Check if sink is available
    async fn is_available(&self) -> bool {
        true
    }
}

/// File sink configuration
#[derive(Debug, Clone)]
pub struct FileSinkConfig {
    /// Output file path
    pub path: PathBuf,
    /// Append mode (vs truncate)
    pub append: bool,
    /// Buffer size
    pub buffer_size: usize,
    /// Create parent directories
    pub create_dirs: bool,
}

impl Default for FileSinkConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            append: false,
            buffer_size: 8192,
            create_dirs: true,
        }
    }
}

/// File sink for writing to local files
pub struct FileSink {
    config: FileSinkConfig,
    file: tokio::sync::Mutex<Option<File>>,
}

impl FileSink {
    /// Create a new file sink
    pub fn new(path: PathBuf) -> Self {
        Self {
            config: FileSinkConfig {
                path,
                ..Default::default()
            },
            file: tokio::sync::Mutex::new(None),
        }
    }

    /// Create with configuration
    pub fn with_config(config: FileSinkConfig) -> Self {
        Self {
            config,
            file: tokio::sync::Mutex::new(None),
        }
    }

    /// Enable append mode
    pub fn append(mut self, append: bool) -> Self {
        self.config.append = append;
        self
    }

    /// Initialize the file
    async fn ensure_file(&self) -> Result<()> {
        let mut file_guard = self.file.lock().await;

        if file_guard.is_none() {
            // Create parent directories if needed
            if self.config.create_dirs {
                if let Some(parent) = self.config.path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
            }

            // Open or create file
            let file = if self.config.append {
                tokio::fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&self.config.path)
                    .await?
            } else {
                File::create(&self.config.path).await?
            };

            *file_guard = Some(file);
        }

        Ok(())
    }
}

#[async_trait]
impl Sink for FileSink {
    async fn write(&self, item: StreamItem) -> Result<()> {
        self.ensure_file().await?;

        let mut file_guard = self.file.lock().await;
        if let Some(file) = file_guard.as_mut() {
            file.write_all(&item)
                .await
                .map_err(|e| SinkError::WriteFailed(e.to_string()))?;
        }

        Ok(())
    }

    async fn flush(&self) -> Result<()> {
        let mut file_guard = self.file.lock().await;
        if let Some(file) = file_guard.as_mut() {
            file.flush()
                .await
                .map_err(|e| SinkError::WriteFailed(e.to_string()))?;
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "FileSink"
    }
}

/// S3 sink configuration
#[cfg(feature = "s3")]
#[derive(Debug, Clone)]
pub struct S3SinkConfig {
    /// S3 bucket name
    pub bucket: String,
    /// S3 key prefix
    pub prefix: String,
    /// Part size for multipart upload
    pub part_size: usize,
    /// AWS region
    pub region: Option<String>,
}

#[cfg(feature = "s3")]
impl Default for S3SinkConfig {
    fn default() -> Self {
        Self {
            bucket: String::new(),
            prefix: String::new(),
            part_size: 5 * 1024 * 1024, // 5 MB
            region: None,
        }
    }
}

/// S3 sink for writing to Amazon S3
#[cfg(feature = "s3")]
pub struct S3Sink {
    config: S3SinkConfig,
    client: aws_sdk_s3::Client,
    #[allow(dead_code)]
    buffer: tokio::sync::Mutex<Vec<u8>>,
    item_count: tokio::sync::Mutex<usize>,
}

#[cfg(feature = "s3")]
impl S3Sink {
    /// Create a new S3 sink
    pub async fn new(bucket: String, prefix: String) -> Result<Self> {
        #[allow(deprecated)]
        let config = aws_config::load_from_env().await;
        let client = aws_sdk_s3::Client::new(&config);

        Ok(Self {
            config: S3SinkConfig {
                bucket,
                prefix,
                ..Default::default()
            },
            client,
            buffer: tokio::sync::Mutex::new(Vec::new()),
            item_count: tokio::sync::Mutex::new(0),
        })
    }

    /// Set part size for multipart uploads
    pub fn part_size(mut self, size: usize) -> Self {
        self.config.part_size = size;
        self
    }

    /// Generate object key for item
    async fn generate_key(&self) -> String {
        let count = self.item_count.lock().await;
        format!("{}/item_{:010}.bin", self.config.prefix, *count)
    }
}

#[cfg(feature = "s3")]
#[async_trait]
impl Sink for S3Sink {
    async fn write(&self, item: StreamItem) -> Result<()> {
        let key = self.generate_key().await;

        self.client
            .put_object()
            .bucket(&self.config.bucket)
            .key(&key)
            .body(item.into())
            .send()
            .await
            .map_err(|e| SinkError::S3(e.to_string()))?;

        let mut count = self.item_count.lock().await;
        *count += 1;

        Ok(())
    }

    async fn write_batch(&self, items: Vec<StreamItem>) -> Result<()> {
        for item in items {
            self.write(item).await?;
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "S3Sink"
    }
}

/// Kafka sink configuration
#[cfg(feature = "kafka")]
#[derive(Debug, Clone)]
pub struct KafkaSinkConfig {
    /// Bootstrap servers
    pub brokers: String,
    /// Topic to produce to
    pub topic: String,
    /// Compression type
    pub compression: String,
    /// Batch size
    pub batch_size: usize,
}

#[cfg(feature = "kafka")]
impl Default for KafkaSinkConfig {
    fn default() -> Self {
        Self {
            brokers: String::new(),
            topic: String::new(),
            compression: "none".to_string(),
            batch_size: 1000,
        }
    }
}

/// Kafka sink for producing messages
#[cfg(feature = "kafka")]
pub struct KafkaSink {
    config: KafkaSinkConfig,
    producer: rdkafka::producer::FutureProducer,
}

#[cfg(feature = "kafka")]
impl KafkaSink {
    /// Create a new Kafka sink
    pub fn new(brokers: String, topic: String) -> Result<Self> {
        use rdkafka::config::ClientConfig;
        use rdkafka::producer::FutureProducer;

        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &brokers)
            .set("compression.type", "none")
            .create()
            .map_err(|e| SinkError::Kafka(e.to_string()))?;

        Ok(Self {
            config: KafkaSinkConfig {
                brokers,
                topic,
                ..Default::default()
            },
            producer,
        })
    }

    /// Set compression type
    pub fn compression(mut self, compression: String) -> Self {
        self.config.compression = compression;
        self
    }
}

#[cfg(feature = "kafka")]
#[async_trait]
impl Sink for KafkaSink {
    async fn write(&self, item: StreamItem) -> Result<()> {
        use rdkafka::producer::FutureRecord;
        use std::time::Duration;

        let record: FutureRecord<'_, str, Vec<u8>> =
            FutureRecord::to(&self.config.topic).payload(&item);

        self.producer
            .send(record, Duration::from_secs(0))
            .await
            .map_err(|(e, _)| SinkError::Kafka(e.to_string()))?;

        Ok(())
    }

    async fn flush(&self) -> Result<()> {
        use rdkafka::producer::Producer;
        use std::time::Duration;

        self.producer
            .flush(Duration::from_secs(10))
            .map_err(|e| SinkError::Kafka(e.to_string()))?;

        Ok(())
    }

    fn name(&self) -> &str {
        "KafkaSink"
    }
}

/// PostGIS sink configuration
#[cfg(feature = "postgres")]
#[derive(Debug, Clone)]
pub struct PostGisSinkConfig {
    /// Table name
    pub table: String,
    /// Schema name
    pub schema: String,
    /// Batch size for bulk inserts
    pub batch_size: usize,
    /// Create table if not exists
    pub create_table: bool,
}

#[cfg(feature = "postgres")]
impl Default for PostGisSinkConfig {
    fn default() -> Self {
        Self {
            table: String::new(),
            schema: "public".to_string(),
            batch_size: 1000,
            create_table: false,
        }
    }
}

/// PostGIS sink for writing to PostgreSQL/PostGIS
#[cfg(feature = "postgres")]
pub struct PostGisSink {
    config: PostGisSinkConfig,
    pool: deadpool_postgres::Pool,
    buffer: tokio::sync::Mutex<Vec<StreamItem>>,
}

#[cfg(feature = "postgres")]
impl PostGisSink {
    /// Create a new PostGIS sink
    pub async fn new(pool: deadpool_postgres::Pool, table: String) -> Self {
        Self {
            config: PostGisSinkConfig {
                table,
                ..Default::default()
            },
            pool,
            buffer: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    /// Set schema
    pub fn schema(mut self, schema: String) -> Self {
        self.config.schema = schema;
        self
    }

    /// Set batch size
    pub fn batch_size(mut self, size: usize) -> Self {
        self.config.batch_size = size;
        self
    }

    /// Flush buffer to database
    async fn flush_buffer(&self) -> Result<()> {
        let mut buffer = self.buffer.lock().await;
        if buffer.is_empty() {
            return Ok(());
        }

        let client = self
            .pool
            .get()
            .await
            .map_err(|e| SinkError::Database(e.to_string()))?;

        // Simple insert for now - in production, use COPY or prepared statements
        for item in buffer.drain(..) {
            let data = String::from_utf8_lossy(&item);
            let query = format!(
                "INSERT INTO {}.{} (data) VALUES ($1)",
                self.config.schema, self.config.table
            );

            client
                .execute(&query, &[&data.as_ref()])
                .await
                .map_err(|e| SinkError::Database(e.to_string()))?;
        }

        Ok(())
    }
}

#[cfg(feature = "postgres")]
#[async_trait]
impl Sink for PostGisSink {
    async fn write(&self, item: StreamItem) -> Result<()> {
        let mut buffer = self.buffer.lock().await;
        buffer.push(item);

        if buffer.len() >= self.config.batch_size {
            drop(buffer);
            self.flush_buffer().await?;
        }

        Ok(())
    }

    async fn flush(&self) -> Result<()> {
        self.flush_buffer().await
    }

    fn name(&self) -> &str {
        "PostGisSink"
    }
}

/// Custom sink wrapper
pub struct CustomSink<F>
where
    F: Fn(StreamItem) -> Pin<Box<dyn futures::Future<Output = Result<()>> + Send>> + Send + Sync,
{
    name: String,
    writer: F,
}

impl<F> CustomSink<F>
where
    F: Fn(StreamItem) -> Pin<Box<dyn futures::Future<Output = Result<()>> + Send>> + Send + Sync,
{
    /// Create a new custom sink
    pub fn new(name: String, writer: F) -> Self {
        Self { name, writer }
    }
}

use std::pin::Pin;

#[async_trait]
impl<F> Sink for CustomSink<F>
where
    F: Fn(StreamItem) -> Pin<Box<dyn futures::Future<Output = Result<()>> + Send>> + Send + Sync,
{
    async fn write(&self, item: StreamItem) -> Result<()> {
        (self.writer)(item).await
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_file_sink() {
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let path = temp_file.path().to_path_buf();

        let sink = FileSink::new(path.clone());
        sink.write(vec![1, 2, 3, 4]).await.expect("Failed to write");
        sink.flush().await.expect("Failed to flush");

        let content = tokio::fs::read(&path).await.expect("Failed to read");
        assert_eq!(content, vec![1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn test_file_sink_batch() {
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let path = temp_file.path().to_path_buf();

        let sink = FileSink::new(path.clone());
        let items = vec![vec![1, 2], vec![3, 4], vec![5, 6]];
        sink.write_batch(items)
            .await
            .expect("Failed to write batch");
        sink.flush().await.expect("Failed to flush");

        let content = tokio::fs::read(&path).await.expect("Failed to read");
        assert_eq!(content, vec![1, 2, 3, 4, 5, 6]);
    }

    #[tokio::test]
    async fn test_custom_sink() {
        use std::sync::Arc;
        use tokio::sync::Mutex;

        let received = Arc::new(Mutex::new(Vec::new()));
        let received_clone = Arc::clone(&received);

        let sink = CustomSink::new("TestSink".to_string(), move |item| {
            let received = Arc::clone(&received_clone);
            Box::pin(async move {
                received.lock().await.push(item);
                Ok(())
            })
        });

        sink.write(vec![1, 2, 3]).await.expect("Failed to write");
        sink.write(vec![4, 5, 6]).await.expect("Failed to write");

        let data = received.lock().await;
        assert_eq!(data.len(), 2);
        assert_eq!(data[0], vec![1, 2, 3]);
        assert_eq!(data[1], vec![4, 5, 6]);
    }
}
