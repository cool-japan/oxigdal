//! Core stream types and traits.

use crate::error::{Result, StreamingError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// A stream element containing data and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamElement {
    /// The actual data payload
    pub data: Vec<u8>,

    /// Event timestamp
    pub event_time: DateTime<Utc>,

    /// Processing timestamp
    pub processing_time: DateTime<Utc>,

    /// Optional key for partitioning
    pub key: Option<Vec<u8>>,

    /// Metadata
    pub metadata: StreamMetadata,
}

impl StreamElement {
    /// Create a new stream element.
    pub fn new(data: Vec<u8>, event_time: DateTime<Utc>) -> Self {
        Self {
            data,
            event_time,
            processing_time: Utc::now(),
            key: None,
            metadata: StreamMetadata::default(),
        }
    }

    /// Create a new stream element with a key.
    pub fn with_key(mut self, key: Vec<u8>) -> Self {
        self.key = Some(key);
        self
    }

    /// Create a new stream element with metadata.
    pub fn with_metadata(mut self, metadata: StreamMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    /// Get the size in bytes of this element.
    pub fn size_bytes(&self) -> usize {
        self.data.len() + self.key.as_ref().map_or(0, |k| k.len())
    }
}

/// Metadata associated with a stream element.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StreamMetadata {
    /// Source identifier
    pub source_id: Option<String>,

    /// Partition ID
    pub partition_id: Option<u32>,

    /// Sequence number
    pub sequence_number: Option<u64>,

    /// Custom attributes
    pub attributes: std::collections::HashMap<String, String>,
}

/// Message types in a stream.
#[derive(Debug, Clone)]
pub enum StreamMessage {
    /// Data element
    Data(StreamElement),

    /// Watermark for event time progress
    Watermark(DateTime<Utc>),

    /// Checkpoint barrier
    Checkpoint(u64),

    /// End of stream marker
    EndOfStream,
}

/// Configuration for a stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamConfig {
    /// Buffer size for the stream
    pub buffer_size: usize,

    /// Whether to use bounded channels
    pub bounded: bool,

    /// Timeout for operations
    pub timeout: Duration,

    /// Enable checkpointing
    pub enable_checkpointing: bool,

    /// Checkpoint interval
    pub checkpoint_interval: Duration,

    /// Parallelism level
    pub parallelism: usize,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            buffer_size: 1024,
            bounded: true,
            timeout: Duration::from_secs(30),
            enable_checkpointing: false,
            checkpoint_interval: Duration::from_secs(60),
            parallelism: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1),
        }
    }
}

/// A source that produces stream elements.
#[async_trait]
pub trait StreamSource: Send + Sync {
    /// Read the next element from the source.
    async fn next(&mut self) -> Result<Option<StreamMessage>>;

    /// Check if the source has more elements.
    async fn has_next(&self) -> bool;

    /// Close the source.
    async fn close(&mut self) -> Result<()>;
}

/// A sink that consumes stream elements.
#[async_trait]
pub trait StreamSink: Send + Sync {
    /// Write an element to the sink.
    async fn write(&mut self, element: StreamMessage) -> Result<()>;

    /// Flush buffered elements.
    async fn flush(&mut self) -> Result<()>;

    /// Close the sink.
    async fn close(&mut self) -> Result<()>;
}

/// A stream of data elements with transformation capabilities.
pub struct Stream {
    /// Configuration
    config: StreamConfig,

    /// Sender for stream messages
    sender: Sender<StreamMessage>,

    /// Receiver for stream messages
    receiver: Receiver<StreamMessage>,

    /// Stream state
    state: Arc<RwLock<StreamState>>,
}

/// Internal state of a stream.
#[derive(Debug)]
struct StreamState {
    /// Is the stream closed?
    closed: bool,

    /// Current watermark
    watermark: Option<DateTime<Utc>>,

    /// Last checkpoint ID
    last_checkpoint: Option<u64>,

    /// Total elements processed
    elements_processed: u64,

    /// Total bytes processed
    bytes_processed: u64,
}

impl Stream {
    /// Create a new stream with default configuration.
    pub fn new() -> Self {
        Self::with_config(StreamConfig::default())
    }

    /// Create a new stream with custom configuration.
    pub fn with_config(config: StreamConfig) -> Self {
        let (sender, receiver) = if config.bounded {
            bounded(config.buffer_size)
        } else {
            unbounded()
        };

        Self {
            config,
            sender,
            receiver,
            state: Arc::new(RwLock::new(StreamState {
                closed: false,
                watermark: None,
                last_checkpoint: None,
                elements_processed: 0,
                bytes_processed: 0,
            })),
        }
    }

    /// Send a message to the stream.
    pub async fn send(&self, message: StreamMessage) -> Result<()> {
        let state = self.state.read().await;
        if state.closed {
            return Err(StreamingError::StreamClosed);
        }
        drop(state);

        self.sender
            .send(message)
            .map_err(|_| StreamingError::SendError)?;

        Ok(())
    }

    /// Receive a message from the stream.
    pub async fn recv(&self) -> Result<StreamMessage> {
        match self.receiver.recv_timeout(self.config.timeout) {
            Ok(msg) => {
                // Update state
                let mut state = self.state.write().await;
                match &msg {
                    StreamMessage::Data(elem) => {
                        state.elements_processed += 1;
                        state.bytes_processed += elem.size_bytes() as u64;
                    }
                    StreamMessage::Watermark(wm) => {
                        state.watermark = Some(*wm);
                    }
                    StreamMessage::Checkpoint(id) => {
                        state.last_checkpoint = Some(*id);
                    }
                    StreamMessage::EndOfStream => {
                        state.closed = true;
                    }
                }
                Ok(msg)
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => Err(StreamingError::Timeout),
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                Err(StreamingError::RecvError)
            }
        }
    }

    /// Try to receive a message without blocking.
    pub fn try_recv(&self) -> Result<Option<StreamMessage>> {
        match self.receiver.try_recv() {
            Ok(msg) => Ok(Some(msg)),
            Err(crossbeam_channel::TryRecvError::Empty) => Ok(None),
            Err(crossbeam_channel::TryRecvError::Disconnected) => Err(StreamingError::RecvError),
        }
    }

    /// Get the current watermark.
    pub async fn watermark(&self) -> Option<DateTime<Utc>> {
        self.state.read().await.watermark
    }

    /// Get the last checkpoint ID.
    pub async fn last_checkpoint(&self) -> Option<u64> {
        self.state.read().await.last_checkpoint
    }

    /// Get the number of elements processed.
    pub async fn elements_processed(&self) -> u64 {
        self.state.read().await.elements_processed
    }

    /// Get the total bytes processed.
    pub async fn bytes_processed(&self) -> u64 {
        self.state.read().await.bytes_processed
    }

    /// Check if the stream is closed.
    pub async fn is_closed(&self) -> bool {
        self.state.read().await.closed
    }

    /// Close the stream.
    pub async fn close(&self) -> Result<()> {
        let mut state = self.state.write().await;
        state.closed = true;
        Ok(())
    }

    /// Get a clone of the sender.
    pub fn sender(&self) -> Sender<StreamMessage> {
        self.sender.clone()
    }

    /// Get a clone of the receiver.
    pub fn receiver(&self) -> Receiver<StreamMessage> {
        self.receiver.clone()
    }

    /// Get the stream configuration.
    pub fn config(&self) -> &StreamConfig {
        &self.config
    }
}

impl Default for Stream {
    fn default() -> Self {
        Self::new()
    }
}

/// A channel-based stream source.
pub struct ChannelSource {
    receiver: Receiver<StreamMessage>,
    closed: bool,
}

impl ChannelSource {
    /// Create a new channel source.
    pub fn new(receiver: Receiver<StreamMessage>) -> Self {
        Self {
            receiver,
            closed: false,
        }
    }
}

#[async_trait]
impl StreamSource for ChannelSource {
    async fn next(&mut self) -> Result<Option<StreamMessage>> {
        if self.closed {
            return Ok(None);
        }

        match self.receiver.try_recv() {
            Ok(msg) => {
                if matches!(msg, StreamMessage::EndOfStream) {
                    self.closed = true;
                }
                Ok(Some(msg))
            }
            Err(crossbeam_channel::TryRecvError::Empty) => Ok(None),
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                self.closed = true;
                Ok(None)
            }
        }
    }

    async fn has_next(&self) -> bool {
        !self.closed && !self.receiver.is_empty()
    }

    async fn close(&mut self) -> Result<()> {
        self.closed = true;
        Ok(())
    }
}

/// A channel-based stream sink.
pub struct ChannelSink {
    sender: Sender<StreamMessage>,
    buffer: Vec<StreamMessage>,
    buffer_size: usize,
}

impl ChannelSink {
    /// Create a new channel sink.
    pub fn new(sender: Sender<StreamMessage>) -> Self {
        Self::with_buffer_size(sender, 100)
    }

    /// Create a new channel sink with a custom buffer size.
    pub fn with_buffer_size(sender: Sender<StreamMessage>, buffer_size: usize) -> Self {
        Self {
            sender,
            buffer: Vec::with_capacity(buffer_size),
            buffer_size,
        }
    }
}

#[async_trait]
impl StreamSink for ChannelSink {
    async fn write(&mut self, element: StreamMessage) -> Result<()> {
        self.buffer.push(element);

        if self.buffer.len() >= self.buffer_size {
            self.flush().await?;
        }

        Ok(())
    }

    async fn flush(&mut self) -> Result<()> {
        for msg in self.buffer.drain(..) {
            self.sender
                .send(msg)
                .map_err(|_| StreamingError::SendError)?;
        }
        Ok(())
    }

    async fn close(&mut self) -> Result<()> {
        self.flush().await?;
        self.sender
            .send(StreamMessage::EndOfStream)
            .map_err(|_| StreamingError::SendError)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stream_element_creation() {
        let now = Utc::now();
        let data = vec![1, 2, 3, 4];
        let elem = StreamElement::new(data.clone(), now);

        assert_eq!(elem.data, data);
        assert_eq!(elem.event_time, now);
        assert!(elem.key.is_none());
    }

    #[tokio::test]
    async fn test_stream_send_recv() {
        let stream = Stream::new();
        let now = Utc::now();
        let elem = StreamElement::new(vec![1, 2, 3], now);

        stream
            .send(StreamMessage::Data(elem.clone()))
            .await
            .expect("stream send should succeed");

        match stream.recv().await.expect("stream recv should succeed") {
            StreamMessage::Data(received) => {
                assert_eq!(received.data, elem.data);
            }
            _ => panic!("Expected data message"),
        }
    }

    #[tokio::test]
    async fn test_stream_watermark() {
        let stream = Stream::new();
        let now = Utc::now();

        stream
            .send(StreamMessage::Watermark(now))
            .await
            .expect("stream send should succeed");
        let _ = stream.recv().await.expect("stream recv should succeed");

        assert_eq!(stream.watermark().await, Some(now));
    }

    #[tokio::test]
    async fn test_stream_close() {
        let stream = Stream::new();
        assert!(!stream.is_closed().await);

        stream.close().await.expect("stream close should succeed");
        assert!(stream.is_closed().await);

        let result = stream.send(StreamMessage::EndOfStream).await;
        assert!(matches!(result, Err(StreamingError::StreamClosed)));
    }
}
