//! AWS Kinesis streaming integration for OxiGDAL
//!
//! This crate provides comprehensive AWS Kinesis integration for OxiGDAL, including:
//!
//! - **Kinesis Data Streams**: Producer with KPL patterns, enhanced fan-out consumer, shard management, DynamoDB checkpointing
//! - **Kinesis Firehose**: Delivery streams with transformations, S3/Redshift/Elasticsearch destinations
//! - **Kinesis Analytics**: SQL queries on streams, tumbling/sliding/session windows, real-time analytics
//! - **Monitoring**: CloudWatch metrics, stream monitoring, alerting system
//!
//! # Features
//!
//! - `streams` - Kinesis Data Streams support (default)
//! - `firehose` - Kinesis Firehose support (default)
//! - `analytics` - Kinesis Analytics support (default)
//! - `monitoring` - CloudWatch monitoring and metrics (default)
//! - `checkpointing` - DynamoDB checkpointing for consumers
//! - `enhanced-fanout` - Enhanced fan-out consumer support
//! - `compression` - Data compression support
//!
//! # Examples
//!
//! ## Kinesis Data Streams - Producer
//!
//! ```rust,no_run
//! # #[cfg(feature = "streams")]
//! # async fn example() -> oxigdal_kinesis::Result<()> {
//! use oxigdal_kinesis::streams::{Producer, ProducerConfig, Record};
//! use bytes::Bytes;
//!
//! // Create AWS Kinesis client
//! let config = aws_config::load_from_env().await;
//! let client = aws_sdk_kinesis::Client::new(&config);
//!
//! // Configure producer
//! let producer_config = ProducerConfig::new("my-stream")
//!     .with_buffer_size(1000)
//!     .with_linger_ms(100);
//!
//! let producer = Producer::new(client, producer_config).await?;
//!
//! // Send records
//! let record = Record::new("partition-key-1", Bytes::from("data"));
//! producer.send(record).await?;
//!
//! // Flush pending records
//! producer.flush().await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Kinesis Data Streams - Consumer
//!
//! ```rust,no_run
//! # #[cfg(feature = "streams")]
//! # async fn example() -> oxigdal_kinesis::Result<()> {
//! use oxigdal_kinesis::streams::{Consumer, ConsumerConfig};
//!
//! let config = aws_config::load_from_env().await;
//! let client = aws_sdk_kinesis::Client::new(&config);
//!
//! let consumer_config = ConsumerConfig::new("my-stream")
//!     .with_max_records(100);
//!
//! let mut consumer = Consumer::new(client, consumer_config, "shard-0001").await?;
//!
//! // Poll for records
//! let records = consumer.poll().await?;
//! for record in records {
//!     println!("Received: {:?}", record.data);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Kinesis Firehose
//!
//! ```rust,no_run
//! # #[cfg(feature = "firehose")]
//! # async fn example() -> oxigdal_kinesis::Result<()> {
//! use oxigdal_kinesis::firehose::{DeliveryStream, DeliveryStreamConfig, FirehoseRecord};
//! use oxigdal_kinesis::firehose::destination::S3DestinationConfig;
//! use bytes::Bytes;
//!
//! let config = aws_config::load_from_env().await;
//! let client = aws_sdk_firehose::Client::new(&config);
//!
//! let s3_config = S3DestinationConfig::new(
//!     "arn:aws:s3:::my-bucket",
//!     "arn:aws:iam::123456789012:role/firehose-role",
//!     "data/",
//! );
//!
//! let stream_config = DeliveryStreamConfig::new("my-delivery-stream")
//!     .with_s3_destination(s3_config);
//!
//! let mut delivery_stream = DeliveryStream::new(client, stream_config);
//! delivery_stream.start().await?;
//!
//! // Send record
//! let record = FirehoseRecord::new(Bytes::from("data"));
//! delivery_stream.send_record(record).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Kinesis Analytics
//!
//! ```rust,no_run
//! # #[cfg(feature = "analytics")]
//! # async fn example() -> oxigdal_kinesis::Result<()> {
//! use oxigdal_kinesis::analytics::sql::QueryBuilder;
//!
//! // Build SQL query
//! let query = QueryBuilder::new()
//!     .select("userId")
//!     .select("COUNT(*) as event_count")
//!     .from("SOURCE_SQL_STREAM")
//!     .window("WINDOW TUMBLING (SIZE 1 MINUTE)")
//!     .group_by("userId")
//!     .build();
//!
//! println!("Query: {}", query.as_str());
//! # Ok(())
//! # }
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod error;

#[cfg(feature = "streams")]
pub mod streams;

#[cfg(feature = "firehose")]
pub mod firehose;

#[cfg(feature = "analytics")]
pub mod analytics;

#[cfg(feature = "monitoring")]
pub mod monitoring;

pub use error::{KinesisError, Result};

/// Kinesis client wrapper providing access to all Kinesis services
#[derive(Clone)]
pub struct KinesisClient {
    #[cfg(feature = "streams")]
    streams: Option<streams::KinesisStreams>,

    #[cfg(feature = "firehose")]
    firehose: Option<firehose::KinesisFirehose>,

    #[cfg(feature = "analytics")]
    analytics: Option<analytics::KinesisAnalytics>,

    #[cfg(feature = "monitoring")]
    monitoring: Option<monitoring::KinesisMonitoring>,
}

impl KinesisClient {
    /// Creates a new Kinesis client
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "streams")]
            streams: None,
            #[cfg(feature = "firehose")]
            firehose: None,
            #[cfg(feature = "analytics")]
            analytics: None,
            #[cfg(feature = "monitoring")]
            monitoring: None,
        }
    }

    /// Creates a new Kinesis client from environment
    #[cfg(feature = "async")]
    pub async fn from_env() -> Self {
        Self {
            #[cfg(feature = "streams")]
            streams: None,
            #[cfg(feature = "firehose")]
            firehose: Some(firehose::KinesisFirehose::from_env().await),
            #[cfg(feature = "analytics")]
            analytics: Some(analytics::KinesisAnalytics::from_env().await),
            #[cfg(feature = "monitoring")]
            monitoring: Some(monitoring::KinesisMonitoring::from_env().await),
        }
    }

    /// Sets the Kinesis Data Streams client
    #[cfg(feature = "streams")]
    pub fn with_streams(mut self, _stream_name: impl Into<String>) -> Self {
        // This would be initialized with actual AWS client in real usage
        self.streams = None; // Placeholder
        self
    }

    /// Gets the Kinesis Data Streams client
    #[cfg(feature = "streams")]
    pub fn streams(&self) -> Option<&streams::KinesisStreams> {
        self.streams.as_ref()
    }

    /// Gets the Kinesis Firehose client
    #[cfg(feature = "firehose")]
    pub fn firehose(&self) -> Option<&firehose::KinesisFirehose> {
        self.firehose.as_ref()
    }

    /// Gets the Kinesis Analytics client
    #[cfg(feature = "analytics")]
    pub fn analytics(&self) -> Option<&analytics::KinesisAnalytics> {
        self.analytics.as_ref()
    }

    /// Gets the monitoring client
    #[cfg(feature = "monitoring")]
    pub fn monitoring(&self) -> Option<&monitoring::KinesisMonitoring> {
        self.monitoring.as_ref()
    }
}

impl Default for KinesisClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kinesis_client_creation() {
        let client = KinesisClient::new();
        assert!(client.firehose().is_none());
    }

    #[test]
    fn test_kinesis_client_default() {
        let client = KinesisClient::default();
        assert!(client.firehose().is_none());
    }
}
