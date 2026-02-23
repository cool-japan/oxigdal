//! Data source implementations for ETL pipelines
//!
//! This module provides various data source implementations including file sources,
//! HTTP/S3 sources, STAC catalog sources, Kafka sources, and database sources.

use crate::error::{Result, SourceError};
use crate::stream::{BoxStream, StreamItem};
use async_trait::async_trait;
use futures::stream::{self, Stream};
use std::path::PathBuf;
use std::pin::Pin;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

/// Data source trait
#[async_trait]
pub trait Source: Send + Sync {
    /// Create a stream from this source
    async fn stream(&self) -> Result<BoxStream<StreamItem>>;

    /// Get source name for logging
    fn name(&self) -> &str;

    /// Check if source is available
    async fn is_available(&self) -> bool {
        true
    }
}

/// File source configuration
#[derive(Debug, Clone)]
pub struct FileSourceConfig {
    /// Path to the file or directory
    pub path: PathBuf,
    /// File pattern (glob) if path is directory
    pub pattern: Option<String>,
    /// Read chunk size
    pub chunk_size: usize,
    /// Line-based reading
    pub line_based: bool,
}

impl Default for FileSourceConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            pattern: None,
            chunk_size: 8192,
            line_based: false,
        }
    }
}

/// File source for reading local files
pub struct FileSource {
    config: FileSourceConfig,
}

impl FileSource {
    /// Create a new file source
    pub fn new(path: PathBuf) -> Self {
        Self {
            config: FileSourceConfig {
                path,
                ..Default::default()
            },
        }
    }

    /// Create with configuration
    pub fn with_config(config: FileSourceConfig) -> Self {
        Self { config }
    }

    /// Set chunk size
    pub fn chunk_size(mut self, size: usize) -> Self {
        self.config.chunk_size = size;
        self
    }

    /// Enable line-based reading
    pub fn line_based(mut self, enabled: bool) -> Self {
        self.config.line_based = enabled;
        self
    }
}

#[async_trait]
impl Source for FileSource {
    async fn stream(&self) -> Result<BoxStream<StreamItem>> {
        let path = self.config.path.clone();

        if !path.exists() {
            return Err(SourceError::NotFound(path.display().to_string()).into());
        }

        if self.config.line_based {
            let file = File::open(&path).await?;
            let reader = BufReader::new(file);
            let lines = reader.lines();

            let stream = stream::unfold(lines, move |mut lines| async move {
                match lines.next_line().await {
                    Ok(Some(line)) => Some((Ok(line.into_bytes()), lines)),
                    Ok(None) => None,
                    Err(e) => Some((Err(SourceError::ReadFailed(e.to_string()).into()), lines)),
                }
            });

            Ok(Box::pin(stream))
        } else {
            let chunk_size = self.config.chunk_size;
            let file = File::open(&path).await?;
            let reader = BufReader::new(file);

            let stream = stream::unfold(reader, move |mut reader| async move {
                let mut buffer = vec![0u8; chunk_size];
                match reader.read(&mut buffer).await {
                    Ok(0) => None,
                    Ok(n) => {
                        buffer.truncate(n);
                        Some((Ok(buffer), reader))
                    }
                    Err(e) => Some((Err(SourceError::ReadFailed(e.to_string()).into()), reader)),
                }
            });

            Ok(Box::pin(stream))
        }
    }

    fn name(&self) -> &str {
        "FileSource"
    }

    async fn is_available(&self) -> bool {
        self.config.path.exists()
    }
}

/// HTTP source configuration
#[cfg(feature = "http")]
#[derive(Debug, Clone)]
pub struct HttpSourceConfig {
    /// URL to fetch from
    pub url: String,
    /// Request headers
    pub headers: std::collections::HashMap<String, String>,
    /// Chunk size for streaming
    pub chunk_size: usize,
    /// Timeout duration
    pub timeout: std::time::Duration,
}

#[cfg(feature = "http")]
impl Default for HttpSourceConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            headers: std::collections::HashMap::new(),
            chunk_size: 8192,
            timeout: std::time::Duration::from_secs(30),
        }
    }
}

/// HTTP source for fetching data from URLs
#[cfg(feature = "http")]
pub struct HttpSource {
    config: HttpSourceConfig,
    client: reqwest::Client,
}

#[cfg(feature = "http")]
impl HttpSource {
    /// Create a new HTTP source
    pub fn new(url: String) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(HttpSourceConfig::default().timeout)
            .build()
            .map_err(|e| SourceError::Http(e.to_string()))?;

        Ok(Self {
            config: HttpSourceConfig {
                url,
                ..Default::default()
            },
            client,
        })
    }

    /// Add a header
    pub fn header(mut self, key: String, value: String) -> Self {
        self.config.headers.insert(key, value);
        self
    }

    /// Set timeout
    pub fn timeout(mut self, timeout: std::time::Duration) -> Self {
        self.config.timeout = timeout;
        self
    }
}

#[cfg(feature = "http")]
#[async_trait]
impl Source for HttpSource {
    async fn stream(&self) -> Result<BoxStream<StreamItem>> {
        use futures::stream;

        let mut request = self.client.get(&self.config.url);

        for (key, value) in &self.config.headers {
            request = request.header(key, value);
        }

        let response = request
            .send()
            .await
            .map_err(|e| SourceError::Http(e.to_string()))?;

        if !response.status().is_success() {
            return Err(SourceError::Http(format!("HTTP error: {}", response.status())).into());
        }

        // Fetch all bytes at once and create a single-item stream
        // For streaming support, enable reqwest "stream" feature
        let data = response
            .bytes()
            .await
            .map_err(|e| SourceError::Http(e.to_string()))?
            .to_vec();

        let stream = stream::once(async move { Ok(data) });

        Ok(Box::pin(stream))
    }

    fn name(&self) -> &str {
        "HttpSource"
    }
}

/// STAC catalog source configuration
#[cfg(feature = "stac")]
#[derive(Debug, Clone)]
pub struct StacSourceConfig {
    /// STAC API endpoint
    pub endpoint: String,
    /// Collection ID
    pub collection: Option<String>,
    /// Bounding box filter [west, south, east, north]
    pub bbox: Option<[f64; 4]>,
    /// Datetime filter
    pub datetime: Option<String>,
    /// Maximum items to fetch
    pub limit: Option<usize>,
}

/// STAC catalog source for fetching items from STAC APIs
#[cfg(feature = "stac")]
pub struct StacSource {
    config: StacSourceConfig,
    client: reqwest::Client,
}

#[cfg(feature = "stac")]
impl StacSource {
    /// Create a new STAC source
    pub fn new(endpoint: String) -> Self {
        Self {
            config: StacSourceConfig {
                endpoint,
                collection: None,
                bbox: None,
                datetime: None,
                limit: None,
            },
            client: reqwest::Client::new(),
        }
    }

    /// Set collection
    pub fn collection(mut self, collection: String) -> Self {
        self.config.collection = Some(collection);
        self
    }

    /// Set bounding box
    pub fn bbox(mut self, bbox: [f64; 4]) -> Self {
        self.config.bbox = Some(bbox);
        self
    }

    /// Set datetime filter
    pub fn datetime(mut self, datetime: String) -> Self {
        self.config.datetime = Some(datetime);
        self
    }

    /// Set limit
    pub fn limit(mut self, limit: usize) -> Self {
        self.config.limit = Some(limit);
        self
    }
}

#[cfg(feature = "stac")]
#[async_trait]
impl Source for StacSource {
    async fn stream(&self) -> Result<BoxStream<StreamItem>> {
        let mut url = format!("{}/search", self.config.endpoint);
        let mut query_params = Vec::new();

        if let Some(collection) = &self.config.collection {
            query_params.push(format!("collections={}", collection));
        }

        if let Some(bbox) = &self.config.bbox {
            query_params.push(format!(
                "bbox={},{},{},{}",
                bbox[0], bbox[1], bbox[2], bbox[3]
            ));
        }

        if let Some(datetime) = &self.config.datetime {
            query_params.push(format!("datetime={}", datetime));
        }

        if let Some(limit) = &self.config.limit {
            query_params.push(format!("limit={}", limit));
        }

        if !query_params.is_empty() {
            url.push('?');
            url.push_str(&query_params.join("&"));
        }

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| SourceError::Stac(e.to_string()))?;

        let data = response
            .bytes()
            .await
            .map_err(|e| SourceError::Stac(e.to_string()))?
            .to_vec();

        let stream = stream::once(async move { Ok(data) });

        Ok(Box::pin(stream))
    }

    fn name(&self) -> &str {
        "StacSource"
    }
}

/// Kafka source configuration
#[cfg(feature = "kafka")]
#[derive(Debug, Clone)]
pub struct KafkaSourceConfig {
    /// Bootstrap servers
    pub brokers: String,
    /// Topic to consume from
    pub topic: String,
    /// Consumer group ID
    pub group_id: String,
    /// Auto offset reset
    pub auto_offset_reset: String,
}

/// Kafka source for consuming messages
#[cfg(feature = "kafka")]
pub struct KafkaSource {
    config: KafkaSourceConfig,
}

#[cfg(feature = "kafka")]
impl KafkaSource {
    /// Create a new Kafka source
    pub fn new(brokers: String, topic: String) -> Self {
        Self {
            config: KafkaSourceConfig {
                brokers,
                topic,
                group_id: "oxigdal-etl".to_string(),
                auto_offset_reset: "earliest".to_string(),
            },
        }
    }

    /// Set consumer group ID
    pub fn group_id(mut self, group_id: String) -> Self {
        self.config.group_id = group_id;
        self
    }
}

#[cfg(feature = "kafka")]
#[async_trait]
impl Source for KafkaSource {
    async fn stream(&self) -> Result<BoxStream<StreamItem>> {
        // Note: Kafka consumer requires proper lifecycle management with Arc/spawn
        // For now, return an error indicating this feature needs implementation
        // In production, use a background task or Arc-wrapped consumer

        Err(SourceError::InvalidConfig(
            "Kafka source requires proper consumer lifecycle management. \
             Use a custom source with Arc-wrapped consumer for production use."
                .to_string(),
        )
        .into())
    }

    fn name(&self) -> &str {
        "KafkaSource"
    }
}

/// Custom source wrapper
pub struct CustomSource<F, S>
where
    F: Fn() -> Pin<Box<dyn futures::Future<Output = Result<S>> + Send>> + Send + Sync,
    S: Stream<Item = Result<StreamItem>> + Send + 'static,
{
    name: String,
    factory: F,
}

impl<F, S> CustomSource<F, S>
where
    F: Fn() -> Pin<Box<dyn futures::Future<Output = Result<S>> + Send>> + Send + Sync,
    S: Stream<Item = Result<StreamItem>> + Send + 'static,
{
    /// Create a new custom source
    pub fn new(name: String, factory: F) -> Self {
        Self { name, factory }
    }
}

#[async_trait]
impl<F, S> Source for CustomSource<F, S>
where
    F: Fn() -> Pin<Box<dyn futures::Future<Output = Result<S>> + Send>> + Send + Sync,
    S: Stream<Item = Result<StreamItem>> + Send + 'static,
{
    async fn stream(&self) -> Result<BoxStream<StreamItem>> {
        let stream = (self.factory)().await?;
        Ok(Box::pin(stream))
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_file_source() {
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        write!(temp_file, "test data").expect("Failed to write");
        let path = temp_file.path().to_path_buf();

        let source = FileSource::new(path);
        assert!(source.is_available().await);

        let mut stream = source.stream().await.expect("Failed to create stream");
        let item = stream.next().await;
        assert!(item.is_some());
    }

    #[tokio::test]
    async fn test_file_source_line_based() {
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        writeln!(temp_file, "line1").expect("Failed to write");
        writeln!(temp_file, "line2").expect("Failed to write");
        let path = temp_file.path().to_path_buf();

        let source = FileSource::new(path).line_based(true);
        let mut stream = source.stream().await.expect("Failed to create stream");

        let line1 = stream
            .next()
            .await
            .expect("Expected line1")
            .expect("Failed to read");
        assert_eq!(line1, b"line1");

        let line2 = stream
            .next()
            .await
            .expect("Expected line2")
            .expect("Failed to read");
        assert_eq!(line2, b"line2");
    }

    #[tokio::test]
    async fn test_file_source_not_found() {
        let source = FileSource::new(PathBuf::from("/nonexistent/path"));
        assert!(!source.is_available().await);

        let result = source.stream().await;
        assert!(result.is_err());
    }
}
