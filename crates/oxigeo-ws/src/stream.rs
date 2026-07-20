//! Data streaming utilities for WebSocket connections.
use crate::error::{Error, Result};
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
        self.state = if ratio >= 0.9 {
            BackpressureState::Critical
        } else if ratio >= self.high_watermark {
            BackpressureState::High
        } else if ratio <= self.low_watermark {
            BackpressureState::Normal
        } else {
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
/// Format tag: payload is the raw (uncompressed) tile bytes.
const DELTA_TAG_RAW: u8 = 0x00;
/// Format tag: payload is a compact varint-encoded diff against the cached
/// previous tile.
const DELTA_TAG_DELTA: u8 = 0x01;
/// Maximum number of bits a LEB128 varint may occupy in this codec
/// (5 groups of 7 bits covers the full `u32` range).
const MAX_VARINT_SHIFT: u32 = 35;

/// Write `value` as an unsigned LEB128 varint into `buf`.
fn write_varint(buf: &mut Vec<u8>, mut value: u32) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 {
            break;
        }
    }
}

/// Read an unsigned LEB128 varint from `data` starting at `*pos`, advancing
/// `*pos` past the bytes consumed.
fn read_varint(data: &[u8], pos: &mut usize) -> Result<u32> {
    let mut result: u32 = 0;
    let mut shift: u32 = 0;
    loop {
        let byte = *data
            .get(*pos)
            .ok_or_else(|| Error::Deserialization("truncated varint in delta payload".into()))?;
        *pos += 1;
        result |= ((byte & 0x7f) as u32) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= MAX_VARINT_SHIFT {
            return Err(Error::Deserialization(
                "varint too long in delta payload".into(),
            ));
        }
    }
    Ok(result)
}

/// Delta encoder for efficient tile updates.
///
/// Encoded output is always tagged with a leading format byte so a decoder
/// (see [`DeltaEncoder::apply_delta`]) can tell raw payloads apart from
/// diffs: `DELTA_TAG_RAW` for the untouched tile bytes, or
/// `DELTA_TAG_DELTA` for a compact varint diff against the previously
/// cached tile. The diff format is only used when it is actually smaller
/// than sending the tile raw; otherwise `encode` falls back to the raw
/// payload so output can never expand beyond `1 + new.len()` bytes.
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
    ///
    /// The result is always tag-prefixed (see [`DeltaEncoder::apply_delta`])
    /// so it can be decoded without external knowledge of whether a diff or
    /// a raw payload was chosen.
    pub fn encode(&self, tile: &TileData) -> Result<Vec<u8>> {
        let key = tile.coords();
        // Compute the diff (if any) against the cached previous tile first,
        // and let the `Ref` guard from `get` drop at the end of this block
        // before we `insert`. DashMap's per-shard `RwLock` is not
        // reentrant: holding a read guard while writing to the same shard
        // (guaranteed here, since it's the same key) deadlocks the calling
        // task instead of erroring.
        let delta = match self.cache.get(&key) {
            Some(prev_data) => Some(Self::compute_delta(&prev_data, &tile.data)?),
            None => None,
        };
        self.cache.insert(key, tile.data.clone());
        match delta {
            Some(delta) => Ok(delta),
            None => Ok(Self::tag_raw(&tile.data)),
        }
    }
    /// Wrap `data` in the raw-payload tag.
    fn tag_raw(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + data.len());
        out.push(DELTA_TAG_RAW);
        out.extend_from_slice(data);
        out
    }
    /// Build the varint diff payload (without the leading tag byte)
    /// describing how to turn `old` into `new`.
    fn encode_diff_payload(old: &[u8], new: &[u8]) -> Vec<u8> {
        let mut payload = Vec::new();
        write_varint(&mut payload, new.len() as u32);
        let mut last_index: i64 = -1;
        let common = old.len().min(new.len());
        for i in 0..new.len() {
            let changed = if i < common { old[i] != new[i] } else { true };
            if changed {
                // Gap since the previous changed index, so runs of changes
                // close together cost a single zero byte instead of a full
                // absolute index.
                let gap = (i as i64 - last_index - 1) as u32;
                write_varint(&mut payload, gap);
                payload.push(new[i]);
                last_index = i as i64;
            }
        }
        payload
    }
    /// Compute delta between two byte arrays, falling back to a tagged raw
    /// payload whenever the diff would not actually be smaller than `new`.
    fn compute_delta(old: &[u8], new: &[u8]) -> Result<Vec<u8>> {
        let diff_payload = Self::encode_diff_payload(old, new);
        if diff_payload.len() < new.len() {
            let mut out = Vec::with_capacity(1 + diff_payload.len());
            out.push(DELTA_TAG_DELTA);
            out.extend_from_slice(&diff_payload);
            Ok(out)
        } else {
            Ok(Self::tag_raw(new))
        }
    }
    /// Decode a payload produced by [`DeltaEncoder::encode`] (or
    /// `DeltaEncoder::compute_delta`) back into the reconstructed tile
    /// bytes.
    ///
    /// `old` must be the same "previous tile" bytes that were used to
    /// produce a diff-tagged payload; it is ignored for raw-tagged
    /// payloads.
    pub fn apply_delta(old: &[u8], encoded: &[u8]) -> Result<Vec<u8>> {
        let mut pos = 0usize;
        let tag = *encoded
            .first()
            .ok_or_else(|| Error::Deserialization("empty delta payload".into()))?;
        pos += 1;
        match tag {
            DELTA_TAG_RAW => Ok(encoded[pos..].to_vec()),
            DELTA_TAG_DELTA => {
                let new_len = read_varint(encoded, &mut pos)? as usize;
                let mut out = vec![0u8; new_len];
                let common = old.len().min(new_len);
                out[..common].copy_from_slice(&old[..common]);
                let mut index: i64 = -1;
                while pos < encoded.len() {
                    let gap = read_varint(encoded, &mut pos)?;
                    let byte = *encoded.get(pos).ok_or_else(|| {
                        Error::Deserialization("truncated delta value byte".into())
                    })?;
                    pos += 1;
                    index += 1 + gap as i64;
                    let idx = usize::try_from(index)
                        .map_err(|_| Error::Deserialization("invalid delta index".into()))?;
                    if idx >= new_len {
                        return Err(Error::Deserialization("delta index out of range".into()));
                    }
                    out[idx] = byte;
                }
                Ok(out)
            }
            other => Err(Error::Deserialization(format!(
                "unknown delta format tag: {other}"
            ))),
        }
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
        // First encode of a coordinate is a cache miss: 1 tag byte + the raw
        // tile bytes, never larger than that.
        if let Ok(data) = delta1 {
            assert_eq!(data.len(), 6);
            assert_eq!(data[0], DELTA_TAG_RAW);
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
            // A single changed byte out of 5 must produce output smaller
            // than the raw tile, not larger (this was the original bug).
            assert!(data.len() < tile2.size());
            assert_eq!(data[0], DELTA_TAG_DELTA);
            // And it must decode back to the exact new tile bytes.
            let restored = DeltaEncoder::apply_delta(&tile1.data, &data);
            assert!(restored.is_ok());
            if let Ok(restored) = restored {
                assert_eq!(restored, tile2.data.to_vec());
            }
        }
    }

    #[test]
    fn test_delta_encoder_raw_fallback_when_diff_would_expand() {
        // Every byte differs, so the varint diff (>= 2 bytes/change) can
        // never beat the raw payload: the encoder must fall back to raw
        // instead of emitting something larger than the input.
        let old = vec![0u8; 5];
        let new = vec![9u8; 5];
        let encoded = DeltaEncoder::compute_delta(&old, &new);
        assert!(encoded.is_ok());
        if let Ok(data) = encoded {
            assert_eq!(data[0], DELTA_TAG_RAW);
            assert_eq!(data.len(), 1 + new.len());
            let restored = DeltaEncoder::apply_delta(&old, &data);
            assert!(restored.is_ok());
            if let Ok(restored) = restored {
                assert_eq!(restored, new);
            }
        }
    }

    #[test]
    fn test_delta_encoder_grow_and_shrink_round_trip() {
        let old = vec![1, 2, 3];
        let grown = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let encoded = DeltaEncoder::compute_delta(&old, &grown);
        assert!(encoded.is_ok());
        if let Ok(data) = encoded {
            let restored = DeltaEncoder::apply_delta(&old, &data);
            assert!(restored.is_ok());
            if let Ok(restored) = restored {
                assert_eq!(restored, grown);
            }
        }
        let shrunk = vec![1, 99];
        let encoded = DeltaEncoder::compute_delta(&grown, &shrunk);
        assert!(encoded.is_ok());
        if let Ok(data) = encoded {
            let restored = DeltaEncoder::apply_delta(&grown, &data);
            assert!(restored.is_ok());
            if let Ok(restored) = restored {
                assert_eq!(restored, shrunk);
            }
        }
    }

    #[test]
    fn test_delta_encoder_apply_delta_rejects_malformed_input() {
        // Empty payload has no tag byte.
        assert!(DeltaEncoder::apply_delta(&[], &[]).is_err());
        // Unknown tag byte.
        assert!(DeltaEncoder::apply_delta(&[], &[0xff]).is_err());
        // Delta tag with a truncated varint length.
        assert!(DeltaEncoder::apply_delta(&[], &[DELTA_TAG_DELTA]).is_err());
        // Delta tag whose diff entry is missing its value byte.
        assert!(DeltaEncoder::apply_delta(&[1, 2, 3], &[DELTA_TAG_DELTA, 3, 0]).is_err());
    }
}
