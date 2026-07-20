//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

#[cfg(test)]
use super::types::{
    BackpressureConfig, Channel, ChannelMultiplexer, ChannelState, ChannelType, Frame, FrameFlags,
    FrameType, HeartbeatConfig, MultiplexerConfig, Priority, PriorityQueue, ReconnectionConfig,
    ReconnectionHandler, ReconnectionState,
};
#[cfg(test)]
use bytes::{Buf, Bytes};
#[cfg(test)]
use std::time::Duration;
#[cfg(test)]
use tokio::sync::mpsc;

/// Magic bytes for protocol identification
pub(crate) const PROTOCOL_MAGIC: [u8; 4] = [0x4F, 0x58, 0x47, 0x44];
/// Protocol version
pub(crate) const PROTOCOL_VERSION: u8 = 1;
/// Default heartbeat interval in seconds
pub(crate) const DEFAULT_HEARTBEAT_INTERVAL: u64 = 30;
/// Default heartbeat timeout in seconds
pub(crate) const DEFAULT_HEARTBEAT_TIMEOUT: u64 = 90;
/// Maximum frame size (16 MB)
pub(crate) const MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;
/// Default buffer high watermark for backpressure
pub(crate) const DEFAULT_HIGH_WATERMARK: usize = 64 * 1024;
/// Default buffer low watermark for backpressure
pub(crate) const DEFAULT_LOW_WATERMARK: usize = 16 * 1024;
/// Calculate checksum using a simple but fast algorithm.
pub(crate) fn calculate_checksum(header: &[u8], payload: &[u8]) -> u32 {
    let mut hash = blake3::Hasher::new();
    hash.update(header);
    hash.update(payload);
    let result = hash.finalize();
    let bytes = result.as_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}
/// Get current timestamp in microseconds since Unix epoch.
pub(crate) fn current_timestamp_micros() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_frame_type_conversion() {
        assert_eq!(FrameType::from_byte(0x01), Some(FrameType::Data));
        assert_eq!(FrameType::from_byte(0x03), Some(FrameType::Ping));
        assert_eq!(FrameType::from_byte(0xFF), Some(FrameType::Error));
        assert_eq!(FrameType::from_byte(0x99), None);
        assert_eq!(FrameType::Data.to_byte(), 0x01);
        assert_eq!(FrameType::Ping.to_byte(), 0x03);
    }
    #[test]
    fn test_channel_type_conversion() {
        assert_eq!(ChannelType::from_byte(0x01), ChannelType::Tile);
        assert_eq!(ChannelType::from_byte(0x99), ChannelType::Custom);
        assert_eq!(ChannelType::Tile.to_byte(), 0x01);
    }
    #[test]
    fn test_frame_flags() {
        let flags = FrameFlags::new(FrameFlags::COMPRESSED | FrameFlags::ACK_REQUIRED);
        assert!(flags.is_compressed());
        assert!(!flags.is_encrypted());
        assert!(flags.requires_ack());
    }
    #[test]
    fn test_frame_encode_decode() {
        let original = Frame::data(42, 123, Bytes::from("Hello, World!"));
        let encoded = original.encode();
        let decoded = Frame::decode(encoded);
        assert!(decoded.is_ok());
        let decoded = decoded.expect("decode should succeed");
        assert_eq!(decoded.frame_type, FrameType::Data);
        assert_eq!(decoded.channel_id, 42);
        assert_eq!(decoded.sequence, 123);
        assert_eq!(decoded.payload, Bytes::from("Hello, World!"));
    }
    #[test]
    fn test_frame_ping_pong() {
        let ping = Frame::ping(Bytes::from_static(&[1, 2, 3, 4, 5, 6, 7, 8]));
        assert_eq!(ping.frame_type, FrameType::Ping);
        assert_eq!(ping.priority, Priority::Critical);
        let pong = Frame::pong(ping.payload.clone());
        assert_eq!(pong.frame_type, FrameType::Pong);
        assert_eq!(pong.payload, ping.payload);
    }
    #[test]
    fn test_frame_auth() {
        let auth = Frame::auth("my-secret-token");
        assert_eq!(auth.frame_type, FrameType::Auth);
        assert_eq!(auth.payload, Bytes::from("my-secret-token"));
    }
    #[test]
    fn test_frame_error() {
        let error = Frame::error(5, 404, "Not found");
        assert_eq!(error.frame_type, FrameType::Error);
        assert_eq!(error.channel_id, 5);
        let mut payload = &error.payload[..];
        let code = payload.get_u32();
        assert_eq!(code, 404);
    }
    #[tokio::test]
    async fn test_channel_creation() {
        let (tx, _rx) = mpsc::channel(16);
        let channel = Channel::new(1, ChannelType::Tile, tx, 16);
        assert_eq!(channel.id(), 1);
        assert_eq!(channel.channel_type(), ChannelType::Tile);
        assert_eq!(channel.state(), ChannelState::Opening);
    }
    #[tokio::test]
    async fn test_channel_state_transitions() {
        let (tx, _rx) = mpsc::channel(16);
        let channel = Channel::new(1, ChannelType::Data, tx, 16);
        channel.set_state(ChannelState::Open);
        assert!(channel.is_open());
        channel.pause();
        assert!(channel.is_paused());
        assert_eq!(channel.state(), ChannelState::Paused);
        channel.resume();
        assert!(!channel.is_paused());
        assert_eq!(channel.state(), ChannelState::Open);
    }
    #[tokio::test]
    async fn test_channel_send_recv() {
        let (tx, mut rx) = mpsc::channel(16);
        let channel = Channel::new(1, ChannelType::Data, tx, 16);
        channel.set_state(ChannelState::Open);
        let data = Bytes::from("test data");
        let result = channel.send(data.clone()).await;
        assert!(result.is_ok());
        let frame = rx.recv().await;
        assert!(frame.is_some());
        let frame = frame.expect("frame should exist");
        assert_eq!(frame.channel_id, 1);
        assert_eq!(frame.payload, data);
    }
    #[test]
    fn test_priority_queue() {
        let queue = PriorityQueue::new();
        let low = Frame::data(1, 1, Bytes::new());
        let mut low = low;
        low.priority = Priority::Low;
        let high = Frame::data(2, 2, Bytes::new());
        let mut high = high;
        high.priority = Priority::High;
        let critical = Frame::data(3, 3, Bytes::new());
        let mut critical = critical;
        critical.priority = Priority::Critical;
        queue.enqueue(low);
        queue.enqueue(critical);
        queue.enqueue(high);
        let first = queue.dequeue();
        assert!(first.is_some());
        assert_eq!(first.expect("first should exist").channel_id, 3);
        let second = queue.dequeue();
        assert!(second.is_some());
        assert_eq!(second.expect("second should exist").channel_id, 2);
        let third = queue.dequeue();
        assert!(third.is_some());
        assert_eq!(third.expect("third should exist").channel_id, 1);
    }
    #[test]
    fn test_reconnection_handler() {
        let config = ReconnectionConfig {
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(1),
            multiplier: 2.0,
            max_attempts: Some(3),
            jitter: 0.0,
        };
        let handler = ReconnectionHandler::new(config);
        assert_eq!(handler.state(), ReconnectionState::Connected);
        handler.on_disconnect();
        assert_eq!(handler.state(), ReconnectionState::Reconnecting);
        let delay1 = handler.next_delay();
        assert!(delay1.is_some());
        assert_eq!(delay1.expect("delay1"), Duration::from_millis(100));
        let delay2 = handler.next_delay();
        assert!(delay2.is_some());
        assert_eq!(delay2.expect("delay2"), Duration::from_millis(200));
        let delay3 = handler.next_delay();
        assert!(delay3.is_some());
        assert_eq!(delay3.expect("delay3"), Duration::from_millis(400));
        let delay4 = handler.next_delay();
        assert!(delay4.is_none());
        assert_eq!(handler.state(), ReconnectionState::Failed);
    }
    #[test]
    fn test_reconnection_reset() {
        let config = ReconnectionConfig::default();
        let handler = ReconnectionHandler::new(config);
        handler.on_disconnect();
        let _ = handler.next_delay();
        let _ = handler.next_delay();
        assert!(handler.attempt() > 0);
        handler.reset();
        assert_eq!(handler.attempt(), 0);
        assert_eq!(handler.state(), ReconnectionState::Connected);
    }
    #[test]
    fn test_multiplexer_config_default() {
        let config = MultiplexerConfig::default();
        assert_eq!(config.max_channels, 64);
        assert_eq!(config.channel_buffer_size, 256);
        assert!(config.default_compression);
    }
    #[tokio::test]
    async fn test_multiplexer_creation() {
        let (mux, _rx) =
            ChannelMultiplexer::new("test-conn-123".to_string(), MultiplexerConfig::default());
        assert_eq!(mux.connection_id(), "test-conn-123");
        assert_eq!(mux.channel_count(), 0);
        assert!(!mux.is_authenticated());
    }
    #[test]
    fn test_backpressure_config_default() {
        let config = BackpressureConfig::default();
        assert_eq!(config.high_watermark, DEFAULT_HIGH_WATERMARK);
        assert_eq!(config.low_watermark, DEFAULT_LOW_WATERMARK);
    }
    #[test]
    fn test_heartbeat_config_default() {
        let config = HeartbeatConfig::default();
        assert_eq!(config.interval.as_secs(), DEFAULT_HEARTBEAT_INTERVAL);
        assert_eq!(config.timeout.as_secs(), DEFAULT_HEARTBEAT_TIMEOUT);
        assert_eq!(config.max_missed, 3);
    }
    #[test]
    fn test_frame_compression() {
        let data = Bytes::from(vec![b'x'; 1000]);
        let frame = Frame::data(1, 1, data.clone());
        let compressed = frame.with_compression();
        assert!(compressed.is_ok());
        let compressed = compressed.expect("compression should succeed");
        assert!(compressed.flags.is_compressed());
        assert!(compressed.payload.len() < data.len());
    }
    #[test]
    fn test_channel_metadata() {
        let (tx, _rx) = mpsc::channel(16);
        let channel = Channel::new(1, ChannelType::Data, tx, 16);
        channel.set_name("test-channel".to_string());
        let metadata = channel.metadata();
        assert_eq!(metadata.name, Some("test-channel".to_string()));
        assert!(metadata.created_at > 0);
    }
}
