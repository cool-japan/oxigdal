//! Data streaming utilities for WebSocket connections.

use crate::error::Result;
use crate::protocol::Message;
use bytes::Bytes;
use futures::stream::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc;

/// A stream of WebSocket messages.
pub struct MessageStream {
    receiver: mpsc::UnboundedReceiver<Message>,
}

impl MessageStream {
    /// Create a new message stream.
    pub fn new(receiver: mpsc::UnboundedReceiver<Message>) -> Self {
        Self { receiver }
    }

    /// Receive the next message.
    pub async fn next_message(&mut self) -> Option<Message> {
        self.receiver.recv().await
    }
}

impl Stream for MessageStream {
    type Item = Message;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}

/// A stream of tile data.
pub struct TileStream {
    receiver: mpsc::UnboundedReceiver<TileData>,
}

impl TileStream {
    /// Create a new tile stream.
    pub fn new(receiver: mpsc::UnboundedReceiver<TileData>) -> Self {
        Self { receiver }
    }

    /// Receive the next tile.
    pub async fn next_tile(&mut self) -> Option<TileData> {
        self.receiver.recv().await
    }
}

impl Stream for TileStream {
    type Item = TileData;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}

/// Tile data with metadata.
#[derive(Debug, Clone)]
pub struct TileData {
    /// Tile X coordinate
    pub x: u32,
    /// Tile Y coordinate
    pub y: u32,
    /// Zoom level
    pub zoom: u8,
    /// Tile data
    pub data: Bytes,
    /// MIME type (e.g., "application/x-protobuf" for MVT)
    pub mime_type: String,
}

impl TileData {
    /// Create new tile data.
    pub fn new(x: u32, y: u32, zoom: u8, data: Vec<u8>, mime_type: String) -> Self {
        Self {
            x,
            y,
            zoom,
            data: Bytes::from(data),
            mime_type,
        }
    }

    /// Get tile coordinates as (x, y, zoom).
    pub fn coords(&self) -> (u32, u32, u8) {
        (self.x, self.y, self.zoom)
    }

    /// Get data size in bytes.
    pub fn size(&self) -> usize {
        self.data.len()
    }
}

/// A stream of feature data.
pub struct FeatureStream {
    receiver: mpsc::UnboundedReceiver<FeatureData>,
}

impl FeatureStream {
    /// Create a new feature stream.
    pub fn new(receiver: mpsc::UnboundedReceiver<FeatureData>) -> Self {
        Self { receiver }
    }

    /// Receive the next feature.
    pub async fn next_feature(&mut self) -> Option<FeatureData> {
        self.receiver.recv().await
    }
}

impl Stream for FeatureStream {
    type Item = FeatureData;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}

/// Feature data with metadata.
#[derive(Debug, Clone)]
pub struct FeatureData {
    /// GeoJSON string
    pub geojson: String,
    /// Change type
    pub change_type: crate::protocol::ChangeType,
    /// Layer name
    pub layer: Option<String>,
}

impl FeatureData {
    /// Create new feature data.
    pub fn new(
        geojson: String,
        change_type: crate::protocol::ChangeType,
        layer: Option<String>,
    ) -> Self {
        Self {
            geojson,
            change_type,
            layer,
        }
    }

    /// Parse GeoJSON.
    pub fn parse_json(&self) -> Result<serde_json::Value> {
        serde_json::from_str(&self.geojson).map_err(Into::into)
    }
}

/// A stream of events.
pub struct EventStream {
    receiver: mpsc::UnboundedReceiver<EventData>,
}

impl EventStream {
    /// Create a new event stream.
    pub fn new(receiver: mpsc::UnboundedReceiver<EventData>) -> Self {
        Self { receiver }
    }

    /// Receive the next event.
    pub async fn next_event(&mut self) -> Option<EventData> {
        self.receiver.recv().await
    }
}

impl Stream for EventStream {
    type Item = EventData;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}

/// Event data with metadata.
#[derive(Debug, Clone)]
pub struct EventData {
    /// Event type
    pub event_type: crate::protocol::EventType,
    /// Event payload
    pub payload: serde_json::Value,
    /// Event timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl EventData {
    /// Create new event data.
    pub fn new(event_type: crate::protocol::EventType, payload: serde_json::Value) -> Self {
        Self {
            event_type,
            payload,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Create event with explicit timestamp.
    pub fn with_timestamp(
        event_type: crate::protocol::EventType,
        payload: serde_json::Value,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        Self {
            event_type,
            payload,
            timestamp,
        }
    }
}

/// Backpressure control for streams.
pub struct BackpressureController {
    /// Maximum buffer size
    max_buffer_size: usize,
    /// Current buffer size
    current_buffer_size: usize,
    /// High watermark (percentage of max)
    high_watermark: f64,
    /// Low watermark (percentage of max)
    low_watermark: f64,
    /// Current state
    state: BackpressureState,
}

/// Backpressure state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackpressureState {
    /// Normal operation
    Normal,
    /// High pressure - slow down
    High,
    /// Critical - stop sending
    Critical,
}

impl BackpressureController {
    /// Create a new backpressure controller.
    pub fn new(max_buffer_size: usize) -> Self {
        Self {
            max_buffer_size,
            current_buffer_size: 0,
            high_watermark: 0.7,
            low_watermark: 0.3,
            state: BackpressureState::Normal,
        }
    }

    /// Update buffer size and return new state.
    pub fn update(&mut self, buffer_size: usize) -> BackpressureState {
        self.current_buffer_size = buffer_size;
        let ratio = buffer_size as f64 / self.max_buffer_size as f64;

        // Use hysteresis pattern to prevent oscillation between states
        self.state = if ratio >= 0.9 {
            BackpressureState::Critical
        } else if ratio >= self.high_watermark {
            BackpressureState::High
        } else if ratio <= self.low_watermark {
            // Only return to Normal when below low_watermark (hysteresis)
            BackpressureState::Normal
        } else {
            // Between low and high watermark - keep current state
            self.state
        };

        self.state
    }

    /// Get current state.
    pub fn state(&self) -> BackpressureState {
        self.state
    }

    /// Check if should throttle.
    pub fn should_throttle(&self) -> bool {
        matches!(
            self.state,
            BackpressureState::High | BackpressureState::Critical
        )
    }

    /// Check if should drop messages.
    pub fn should_drop(&self) -> bool {
        self.state == BackpressureState::Critical
    }
}

/// Delta encoder for efficient tile updates.
pub struct DeltaEncoder {
    /// Previous tile data cache
    cache: dashmap::DashMap<(u32, u32, u8), Bytes>,
}

impl DeltaEncoder {
    /// Create a new delta encoder.
    pub fn new() -> Self {
        Self {
            cache: dashmap::DashMap::new(),
        }
    }

    /// Encode tile data with delta compression.
    pub fn encode(&self, tile: &TileData) -> Result<Vec<u8>> {
        let key = tile.coords();

        if let Some(prev_data) = self.cache.get(&key) {
            // Compute delta
            let delta = Self::compute_delta(&prev_data, &tile.data)?;

            // Update cache
            self.cache.insert(key, tile.data.clone());

            Ok(delta)
        } else {
            // First time - store full data
            self.cache.insert(key, tile.data.clone());
            Ok(tile.data.to_vec())
        }
    }

    /// Compute delta between two byte arrays.
    fn compute_delta(old: &[u8], new: &[u8]) -> Result<Vec<u8>> {
        // Simple delta: store differences
        // In production, use a proper diff algorithm like xdelta
        let mut delta = Vec::new();

        // Store new length
        delta.extend_from_slice(&(new.len() as u32).to_le_bytes());

        // Store changed bytes
        for (i, (&old_byte, &new_byte)) in old.iter().zip(new.iter()).enumerate() {
            if old_byte != new_byte {
                // Store position and new value
                delta.extend_from_slice(&(i as u32).to_le_bytes());
                delta.push(new_byte);
            }
        }

        // Handle size differences
        if new.len() > old.len() {
            for (i, &byte) in new[old.len()..].iter().enumerate() {
                let pos = old.len() + i;
                delta.extend_from_slice(&(pos as u32).to_le_bytes());
                delta.push(byte);
            }
        }

        Ok(delta)
    }

    /// Clear cache.
    pub fn clear(&self) {
        self.cache.clear();
    }

    /// Get cache size.
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }
}

impl Default for DeltaEncoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_message_stream() {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut stream = MessageStream::new(rx);

        let send_result = tx.send(Message::Ping { id: 1 });
        assert!(send_result.is_ok());

        let msg = stream.next_message().await;
        assert!(msg.is_some());
        if let Some(Message::Ping { id }) = msg {
            assert_eq!(id, 1);
        }
    }

    #[tokio::test]
    async fn test_tile_stream() {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut stream = TileStream::new(rx);

        let tile = TileData::new(0, 0, 5, vec![1, 2, 3], "application/x-protobuf".to_string());

        let send_result = tx.send(tile.clone());
        assert!(send_result.is_ok());

        let received = stream.next_tile().await;
        assert!(received.is_some());
        if let Some(tile) = received {
            assert_eq!(tile.coords(), (0, 0, 5));
            assert_eq!(tile.size(), 3);
        }
    }

    #[test]
    fn test_backpressure_controller() {
        let mut controller = BackpressureController::new(100);

        assert_eq!(controller.update(30), BackpressureState::Normal);
        assert!(!controller.should_throttle());

        assert_eq!(controller.update(75), BackpressureState::High);
        assert!(controller.should_throttle());

        assert_eq!(controller.update(95), BackpressureState::Critical);
        assert!(controller.should_drop());

        assert_eq!(controller.update(25), BackpressureState::Normal);
        assert!(!controller.should_throttle());
    }

    #[test]
    fn test_delta_encoder() {
        let encoder = DeltaEncoder::new();

        let tile1 = TileData::new(
            0,
            0,
            5,
            vec![1, 2, 3, 4, 5],
            "application/x-protobuf".to_string(),
        );

        let delta1 = encoder.encode(&tile1);
        assert!(delta1.is_ok());
        if let Ok(data) = delta1 {
            assert_eq!(data.len(), 5); // Full data on first encode
        }

        let tile2 = TileData::new(
            0,
            0,
            5,
            vec![1, 2, 9, 4, 5],
            "application/x-protobuf".to_string(),
        );

        let delta2 = encoder.encode(&tile2);
        assert!(delta2.is_ok());
        if let Ok(data) = delta2 {
            assert!(data.len() < 5); // Delta should be smaller
        }
    }
}
