//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use crate::auth::{AuthContext, Authenticator, Identity};
use crate::error::{GatewayError, Result};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use dashmap::DashMap;
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{Notify, mpsc, oneshot};
use tokio::time::interval;

use super::functions::{
    MAX_FRAME_SIZE, PROTOCOL_MAGIC, PROTOCOL_VERSION, calculate_checksum, current_timestamp_micros,
};

/// Channel metadata.
#[derive(Debug, Clone, Default)]
pub struct ChannelMetadata {
    /// Channel name
    pub name: Option<String>,
    /// Custom metadata
    pub custom: std::collections::HashMap<String, String>,
    /// Created timestamp
    pub created_at: u64,
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Total bytes received
    pub bytes_received: u64,
    /// Messages sent
    pub messages_sent: u64,
    /// Messages received
    pub messages_received: u64,
}
/// Channel type for categorizing data streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ChannelType {
    /// General purpose data channel
    Data = 0x00,
    /// Tile data streaming
    Tile = 0x01,
    /// Metadata streaming
    Metadata = 0x02,
    /// Query results
    Query = 0x03,
    /// Event notifications
    Event = 0x04,
    /// File transfer
    File = 0x05,
    /// Raster data streaming
    Raster = 0x06,
    /// Vector data streaming
    Vector = 0x07,
    /// Custom channel type
    Custom = 0xFF,
}
impl ChannelType {
    /// Creates a `ChannelType` from a byte value.
    pub fn from_byte(byte: u8) -> Self {
        match byte {
            0x00 => Self::Data,
            0x01 => Self::Tile,
            0x02 => Self::Metadata,
            0x03 => Self::Query,
            0x04 => Self::Event,
            0x05 => Self::File,
            0x06 => Self::Raster,
            0x07 => Self::Vector,
            _ => Self::Custom,
        }
    }
    /// Converts the `ChannelType` to its byte representation.
    pub fn to_byte(self) -> u8 {
        self as u8
    }
}
/// Heartbeat manager for connection health monitoring.
pub struct HeartbeatManager {
    /// Configuration
    config: HeartbeatConfig,
    /// Last ping sent time
    last_ping_sent: RwLock<Instant>,
    /// Last pong received time
    last_pong_received: RwLock<Instant>,
    /// Outstanding ping (waiting for pong)
    outstanding_ping: RwLock<Option<u64>>,
    /// Missed heartbeats count
    missed_count: AtomicU32,
    /// Running flag
    running: AtomicBool,
    /// Frame sender
    frame_tx: mpsc::Sender<Frame>,
    /// Disconnection callback
    on_disconnect: RwLock<Option<oneshot::Sender<()>>>,
}
impl HeartbeatManager {
    /// Creates a new heartbeat manager.
    pub fn new(config: HeartbeatConfig, frame_tx: mpsc::Sender<Frame>) -> Self {
        let now = Instant::now();
        Self {
            config,
            last_ping_sent: RwLock::new(now),
            last_pong_received: RwLock::new(now),
            outstanding_ping: RwLock::new(None),
            missed_count: AtomicU32::new(0),
            running: AtomicBool::new(false),
            frame_tx,
            on_disconnect: RwLock::new(None),
        }
    }
    /// Starts the heartbeat loop.
    pub async fn start(self: Arc<Self>) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();
        *self.on_disconnect.write() = Some(tx);
        self.running.store(true, Ordering::SeqCst);
        let manager = Arc::clone(&self);
        tokio::spawn(async move {
            manager.heartbeat_loop().await;
        });
        rx
    }
    /// Heartbeat loop.
    async fn heartbeat_loop(&self) {
        let mut interval_timer = interval(self.config.interval);
        while self.running.load(Ordering::SeqCst) {
            interval_timer.tick().await;
            let last_pong = *self.last_pong_received.read();
            let elapsed = last_pong.elapsed();
            if elapsed > self.config.timeout {
                self.missed_count.fetch_add(1, Ordering::SeqCst);
                let missed = self.missed_count.load(Ordering::SeqCst);
                tracing::warn!(
                    "Heartbeat timeout: missed {} heartbeats (elapsed: {:?})",
                    missed,
                    elapsed
                );
                if missed >= self.config.max_missed {
                    tracing::error!("Maximum missed heartbeats exceeded, disconnecting");
                    self.trigger_disconnect();
                    return;
                }
            }
            if let Err(e) = self.send_ping().await {
                tracing::error!("Failed to send heartbeat ping: {}", e);
            }
        }
    }
    /// Sends a ping frame.
    async fn send_ping(&self) -> Result<()> {
        let timestamp = current_timestamp_micros();
        let mut payload = BytesMut::with_capacity(8);
        payload.put_u64(timestamp);
        let frame = Frame::ping(payload.freeze());
        *self.outstanding_ping.write() = Some(timestamp);
        *self.last_ping_sent.write() = Instant::now();
        self.frame_tx
            .send(frame)
            .await
            .map_err(|e| GatewayError::WebSocketError(format!("Failed to send ping: {}", e)))
    }
    /// Handles a received pong frame.
    pub fn handle_pong(&self, payload: &[u8]) -> Result<Duration> {
        if payload.len() < 8 {
            return Err(GatewayError::WebSocketError(
                "Invalid pong payload".to_string(),
            ));
        }
        let sent_timestamp = {
            let mut buf = &payload[..8];
            buf.get_u64()
        };
        let outstanding = self.outstanding_ping.read();
        if outstanding.as_ref() != Some(&sent_timestamp) {
            return Err(GatewayError::WebSocketError(
                "Unexpected pong timestamp".to_string(),
            ));
        }
        drop(outstanding);
        *self.outstanding_ping.write() = None;
        *self.last_pong_received.write() = Instant::now();
        self.missed_count.store(0, Ordering::SeqCst);
        let now = current_timestamp_micros();
        let rtt_micros = now.saturating_sub(sent_timestamp);
        Ok(Duration::from_micros(rtt_micros))
    }
    /// Handles a received ping frame by sending pong.
    pub async fn handle_ping(&self, payload: Bytes) -> Result<()> {
        let frame = Frame::pong(payload);
        self.frame_tx
            .send(frame)
            .await
            .map_err(|e| GatewayError::WebSocketError(format!("Failed to send pong: {}", e)))
    }
    /// Stops the heartbeat manager.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
    /// Triggers disconnection.
    fn trigger_disconnect(&self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(tx) = self.on_disconnect.write().take() {
            let _ = tx.send(());
        }
    }
    /// Returns the last round-trip time.
    pub fn last_rtt(&self) -> Duration {
        let last_ping = *self.last_ping_sent.read();
        let last_pong = *self.last_pong_received.read();
        if last_pong > last_ping {
            last_pong.duration_since(last_ping)
        } else {
            Duration::ZERO
        }
    }
}
/// Backpressure configuration.
#[derive(Debug, Clone)]
pub struct BackpressureConfig {
    /// High watermark (pause when exceeded)
    pub high_watermark: usize,
    /// Low watermark (resume when below)
    pub low_watermark: usize,
    /// Per-channel limits
    pub channel_limit: usize,
}
/// Frame flags bitfield.
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameFlags(u8);
impl FrameFlags {
    /// No flags set
    pub const NONE: u8 = 0x00;
    /// Compressed payload
    pub const COMPRESSED: u8 = 0x01;
    /// Encrypted payload
    pub const ENCRYPTED: u8 = 0x02;
    /// Fragment: more fragments follow
    pub const FRAGMENT: u8 = 0x04;
    /// Fragment: last fragment
    pub const FRAGMENT_END: u8 = 0x08;
    /// Acknowledgment required
    pub const ACK_REQUIRED: u8 = 0x10;
    /// Creates new frame flags.
    pub fn new(flags: u8) -> Self {
        Self(flags)
    }
    /// Checks if compressed flag is set.
    pub fn is_compressed(self) -> bool {
        self.0 & Self::COMPRESSED != 0
    }
    /// Checks if encrypted flag is set.
    pub fn is_encrypted(self) -> bool {
        self.0 & Self::ENCRYPTED != 0
    }
    /// Checks if fragment flag is set.
    pub fn is_fragment(self) -> bool {
        self.0 & Self::FRAGMENT != 0
    }
    /// Checks if fragment end flag is set.
    pub fn is_fragment_end(self) -> bool {
        self.0 & Self::FRAGMENT_END != 0
    }
    /// Checks if acknowledgment is required.
    pub fn requires_ack(self) -> bool {
        self.0 & Self::ACK_REQUIRED != 0
    }
    /// Returns the raw flag bits.
    pub fn bits(self) -> u8 {
        self.0
    }
}
/// Priority level for channel messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(u8)]
pub enum Priority {
    /// Low priority (background transfers)
    Low = 0,
    /// Normal priority (default)
    #[default]
    Normal = 1,
    /// High priority (interactive data)
    High = 2,
    /// Critical priority (control messages)
    Critical = 3,
}
impl Priority {
    /// Creates a `Priority` from a byte value.
    pub fn from_byte(byte: u8) -> Self {
        match byte {
            0 => Self::Low,
            2 => Self::High,
            3 => Self::Critical,
            _ => Self::Normal,
        }
    }
    /// Converts the `Priority` to its byte representation.
    pub fn to_byte(self) -> u8 {
        self as u8
    }
}
/// Reconnection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconnectionState {
    /// Connected
    Connected,
    /// Attempting to reconnect
    Reconnecting,
    /// Gave up reconnecting
    Failed,
}
/// Multiplexer configuration.
#[derive(Debug, Clone)]
pub struct MultiplexerConfig {
    /// Maximum number of channels
    pub max_channels: usize,
    /// Channel buffer size
    pub channel_buffer_size: usize,
    /// Heartbeat configuration
    pub heartbeat: HeartbeatConfig,
    /// Backpressure configuration
    pub backpressure: BackpressureConfig,
    /// Enable compression by default
    pub default_compression: bool,
}
/// Backpressure manager for flow control.
pub struct BackpressureManager {
    /// Configuration
    config: BackpressureConfig,
    /// Total buffer size across all channels
    total_buffer: AtomicU64,
    /// Per-channel buffer sizes
    channel_buffers: DashMap<u32, AtomicU64>,
    /// Paused channels
    paused_channels: DashMap<u32, bool>,
    /// Frame sender for flow control messages
    frame_tx: mpsc::Sender<Frame>,
}
impl BackpressureManager {
    /// Creates a new backpressure manager.
    pub fn new(config: BackpressureConfig, frame_tx: mpsc::Sender<Frame>) -> Self {
        Self {
            config,
            total_buffer: AtomicU64::new(0),
            channel_buffers: DashMap::new(),
            paused_channels: DashMap::new(),
            frame_tx,
        }
    }
    /// Records bytes added to a channel buffer.
    pub async fn record_bytes(&self, channel_id: u32, bytes: u64) -> Result<bool> {
        let total = self.total_buffer.fetch_add(bytes, Ordering::SeqCst) + bytes;
        self.channel_buffers
            .entry(channel_id)
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(bytes, Ordering::SeqCst);
        if total > self.config.high_watermark as u64 {
            self.pause_channel(channel_id).await?;
            return Ok(true);
        }
        if let Some(channel_buffer) = self.channel_buffers.get(&channel_id) {
            if channel_buffer.load(Ordering::SeqCst) > self.config.channel_limit as u64 {
                self.pause_channel(channel_id).await?;
                return Ok(true);
            }
        }
        Ok(false)
    }
    /// Records bytes consumed from a channel buffer.
    pub async fn consume_bytes(&self, channel_id: u32, bytes: u64) -> Result<bool> {
        let total = self.total_buffer.fetch_sub(
            bytes.min(self.total_buffer.load(Ordering::SeqCst)),
            Ordering::SeqCst,
        );
        let new_total = total.saturating_sub(bytes);
        if let Some(channel_buffer) = self.channel_buffers.get(&channel_id) {
            let current = channel_buffer.load(Ordering::SeqCst);
            channel_buffer.store(current.saturating_sub(bytes), Ordering::SeqCst);
        }
        if new_total < self.config.low_watermark as u64
            && self
                .paused_channels
                .get(&channel_id)
                .map(|v| *v)
                .unwrap_or(false)
        {
            self.resume_channel(channel_id).await?;
            return Ok(true);
        }
        Ok(false)
    }
    /// Pauses a channel due to backpressure.
    async fn pause_channel(&self, channel_id: u32) -> Result<()> {
        self.paused_channels.insert(channel_id, true);
        let frame = Frame::flow_pause(channel_id);
        self.frame_tx
            .send(frame)
            .await
            .map_err(|e| GatewayError::WebSocketError(format!("Failed to send pause frame: {}", e)))
    }
    /// Resumes a channel after backpressure relief.
    async fn resume_channel(&self, channel_id: u32) -> Result<()> {
        self.paused_channels.insert(channel_id, false);
        let frame = Frame::flow_resume(channel_id);
        self.frame_tx.send(frame).await.map_err(|e| {
            GatewayError::WebSocketError(format!("Failed to send resume frame: {}", e))
        })
    }
    /// Checks if a channel is paused.
    pub fn is_paused(&self, channel_id: u32) -> bool {
        self.paused_channels
            .get(&channel_id)
            .map(|v| *v)
            .unwrap_or(false)
    }
    /// Returns total buffer size.
    pub fn total_buffer_size(&self) -> u64 {
        self.total_buffer.load(Ordering::SeqCst)
    }
    /// Returns channel buffer size.
    pub fn channel_buffer_size(&self, channel_id: u32) -> u64 {
        self.channel_buffers
            .get(&channel_id)
            .map(|v| v.load(Ordering::SeqCst))
            .unwrap_or(0)
    }
}
/// Reconnection configuration.
#[derive(Debug, Clone)]
pub struct ReconnectionConfig {
    /// Initial delay between reconnection attempts
    pub initial_delay: Duration,
    /// Maximum delay between reconnection attempts
    pub max_delay: Duration,
    /// Delay multiplier for exponential backoff
    pub multiplier: f64,
    /// Maximum number of reconnection attempts (None = unlimited)
    pub max_attempts: Option<u32>,
    /// Jitter factor (0.0 to 1.0)
    pub jitter: f64,
}
/// Single multiplexed channel.
pub struct Channel {
    /// Channel ID
    id: u32,
    /// Channel type
    channel_type: ChannelType,
    /// Channel state
    state: RwLock<ChannelState>,
    /// Outgoing message sender
    outgoing_tx: mpsc::Sender<Frame>,
    /// Incoming message receiver
    incoming_rx: RwLock<Option<mpsc::Receiver<Frame>>>,
    /// Incoming message sender (for multiplexer to push frames)
    incoming_tx: mpsc::Sender<Frame>,
    /// Next sequence number
    next_sequence: AtomicU32,
    /// Buffer size for backpressure
    buffer_size: AtomicU64,
    /// Flow control: paused flag
    paused: AtomicBool,
    /// Resume notification
    resume_notify: Arc<Notify>,
    /// Channel metadata
    metadata: RwLock<ChannelMetadata>,
    /// Priority
    priority: Priority,
}
impl Channel {
    /// Creates a new channel.
    pub fn new(
        id: u32,
        channel_type: ChannelType,
        outgoing_tx: mpsc::Sender<Frame>,
        buffer_size: usize,
    ) -> Self {
        let (incoming_tx, incoming_rx) = mpsc::channel(buffer_size);
        Self {
            id,
            channel_type,
            state: RwLock::new(ChannelState::Opening),
            outgoing_tx,
            incoming_rx: RwLock::new(Some(incoming_rx)),
            incoming_tx,
            next_sequence: AtomicU32::new(0),
            buffer_size: AtomicU64::new(0),
            paused: AtomicBool::new(false),
            resume_notify: Arc::new(Notify::new()),
            metadata: RwLock::new(ChannelMetadata {
                created_at: current_timestamp_micros(),
                ..Default::default()
            }),
            priority: Priority::Normal,
        }
    }
    /// Returns the channel ID.
    pub fn id(&self) -> u32 {
        self.id
    }
    /// Returns the channel type.
    pub fn channel_type(&self) -> ChannelType {
        self.channel_type
    }
    /// Returns the channel state.
    pub fn state(&self) -> ChannelState {
        *self.state.read()
    }
    /// Sets the channel state.
    pub fn set_state(&self, state: ChannelState) {
        *self.state.write() = state;
    }
    /// Checks if the channel is open.
    pub fn is_open(&self) -> bool {
        *self.state.read() == ChannelState::Open
    }
    /// Checks if the channel is paused.
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }
    /// Sends data on this channel.
    pub async fn send(&self, data: Bytes) -> Result<()> {
        while self.paused.load(Ordering::SeqCst) {
            self.resume_notify.notified().await;
        }
        if !self.is_open() {
            return Err(GatewayError::WebSocketError(
                "Channel is not open".to_string(),
            ));
        }
        let sequence = self.next_sequence.fetch_add(1, Ordering::SeqCst);
        let frame = Frame::data(self.id, sequence, data.clone());
        {
            let mut metadata = self.metadata.write();
            metadata.bytes_sent += data.len() as u64;
            metadata.messages_sent += 1;
        }
        self.buffer_size
            .fetch_add(data.len() as u64, Ordering::SeqCst);
        self.outgoing_tx
            .send(frame)
            .await
            .map_err(|e| GatewayError::WebSocketError(format!("Failed to send frame: {}", e)))
    }
    /// Sends compressed data on this channel.
    pub async fn send_compressed(&self, data: Bytes) -> Result<()> {
        while self.paused.load(Ordering::SeqCst) {
            self.resume_notify.notified().await;
        }
        if !self.is_open() {
            return Err(GatewayError::WebSocketError(
                "Channel is not open".to_string(),
            ));
        }
        let sequence = self.next_sequence.fetch_add(1, Ordering::SeqCst);
        let frame = Frame::data(self.id, sequence, data.clone()).with_compression()?;
        {
            let mut metadata = self.metadata.write();
            metadata.bytes_sent += data.len() as u64;
            metadata.messages_sent += 1;
        }
        self.outgoing_tx
            .send(frame)
            .await
            .map_err(|e| GatewayError::WebSocketError(format!("Failed to send frame: {}", e)))
    }
    /// Receives data from this channel.
    pub async fn recv(&self) -> Option<Bytes> {
        let mut rx = {
            let mut guard = self.incoming_rx.write();
            guard.take()?
        };
        let result = match rx.recv().await {
            Some(frame) => {
                {
                    let mut metadata = self.metadata.write();
                    metadata.bytes_received += frame.payload.len() as u64;
                    metadata.messages_received += 1;
                }
                Some(frame.payload)
            }
            None => None,
        };
        *self.incoming_rx.write() = Some(rx);
        result
    }
    /// Takes the incoming receiver for external handling.
    pub fn take_receiver(&self) -> Option<mpsc::Receiver<Frame>> {
        self.incoming_rx.write().take()
    }
    /// Gets the sender for pushing incoming frames.
    pub fn incoming_sender(&self) -> mpsc::Sender<Frame> {
        self.incoming_tx.clone()
    }
    /// Pauses the channel (backpressure).
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
        *self.state.write() = ChannelState::Paused;
    }
    /// Resumes the channel.
    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        *self.state.write() = ChannelState::Open;
        self.resume_notify.notify_waiters();
    }
    /// Returns channel metadata.
    pub fn metadata(&self) -> ChannelMetadata {
        self.metadata.read().clone()
    }
    /// Sets channel name.
    pub fn set_name(&self, name: String) {
        self.metadata.write().name = Some(name);
    }
    /// Returns the priority level.
    pub fn priority(&self) -> Priority {
        self.priority
    }
    /// Closes the channel.
    pub async fn close(&self) -> Result<()> {
        self.set_state(ChannelState::Closing);
        let frame = Frame {
            frame_type: FrameType::ChannelClose,
            flags: FrameFlags::default(),
            priority: Priority::High,
            channel_id: self.id,
            sequence: 0,
            timestamp: current_timestamp_micros(),
            payload: Bytes::new(),
        };
        self.outgoing_tx
            .send(frame)
            .await
            .map_err(|e| GatewayError::WebSocketError(format!("Failed to send close frame: {}", e)))
    }
}
/// Channel state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelState {
    /// Channel is being opened
    Opening,
    /// Channel is open and active
    Open,
    /// Channel is being closed
    Closing,
    /// Channel is closed
    Closed,
    /// Channel is paused (backpressure)
    Paused,
}
/// Binary protocol frame structure.
///
/// Frame format:
/// ```text
/// +--------+--------+--------+--------+
/// | Magic (4 bytes)                   |
/// +--------+--------+--------+--------+
/// | Ver    | Type   | Flags  | Prio   |
/// +--------+--------+--------+--------+
/// | Channel ID (4 bytes)              |
/// +--------+--------+--------+--------+
/// | Sequence Number (4 bytes)         |
/// +--------+--------+--------+--------+
/// | Timestamp (8 bytes)               |
/// +--------+--------+--------+--------+
/// | Payload Length (4 bytes)          |
/// +--------+--------+--------+--------+
/// | Checksum (4 bytes)                |
/// +--------+--------+--------+--------+
/// | Payload (variable)                |
/// +--------+--------+--------+--------+
/// ```
#[derive(Debug, Clone)]
pub struct Frame {
    /// Frame type
    pub frame_type: FrameType,
    /// Frame flags
    pub flags: FrameFlags,
    /// Message priority
    pub priority: Priority,
    /// Channel ID (0 for control channel)
    pub channel_id: u32,
    /// Sequence number
    pub sequence: u32,
    /// Timestamp in microseconds since epoch
    pub timestamp: u64,
    /// Payload data
    pub payload: Bytes,
}
impl Frame {
    /// Header size in bytes.
    pub const HEADER_SIZE: usize = 32;
    /// Creates a new data frame.
    pub fn data(channel_id: u32, sequence: u32, payload: Bytes) -> Self {
        Self {
            frame_type: FrameType::Data,
            flags: FrameFlags::default(),
            priority: Priority::Normal,
            channel_id,
            sequence,
            timestamp: current_timestamp_micros(),
            payload,
        }
    }
    /// Creates a ping frame.
    pub fn ping(payload: Bytes) -> Self {
        Self {
            frame_type: FrameType::Ping,
            flags: FrameFlags::default(),
            priority: Priority::Critical,
            channel_id: 0,
            sequence: 0,
            timestamp: current_timestamp_micros(),
            payload,
        }
    }
    /// Creates a pong frame.
    pub fn pong(payload: Bytes) -> Self {
        Self {
            frame_type: FrameType::Pong,
            flags: FrameFlags::default(),
            priority: Priority::Critical,
            channel_id: 0,
            sequence: 0,
            timestamp: current_timestamp_micros(),
            payload,
        }
    }
    /// Creates a channel open request frame.
    pub fn channel_open(channel_id: u32, channel_type: ChannelType, metadata: Bytes) -> Self {
        let mut payload = BytesMut::with_capacity(1 + metadata.len());
        payload.put_u8(channel_type.to_byte());
        payload.put(metadata);
        Self {
            frame_type: FrameType::ChannelOpen,
            flags: FrameFlags::default(),
            priority: Priority::High,
            channel_id,
            sequence: 0,
            timestamp: current_timestamp_micros(),
            payload: payload.freeze(),
        }
    }
    /// Creates an authentication frame.
    pub fn auth(token: &str) -> Self {
        Self {
            frame_type: FrameType::Auth,
            flags: FrameFlags::default(),
            priority: Priority::Critical,
            channel_id: 0,
            sequence: 0,
            timestamp: current_timestamp_micros(),
            payload: Bytes::copy_from_slice(token.as_bytes()),
        }
    }
    /// Creates a flow control pause frame.
    pub fn flow_pause(channel_id: u32) -> Self {
        Self {
            frame_type: FrameType::FlowPause,
            flags: FrameFlags::default(),
            priority: Priority::Critical,
            channel_id,
            sequence: 0,
            timestamp: current_timestamp_micros(),
            payload: Bytes::new(),
        }
    }
    /// Creates a flow control resume frame.
    pub fn flow_resume(channel_id: u32) -> Self {
        Self {
            frame_type: FrameType::FlowResume,
            flags: FrameFlags::default(),
            priority: Priority::Critical,
            channel_id,
            sequence: 0,
            timestamp: current_timestamp_micros(),
            payload: Bytes::new(),
        }
    }
    /// Creates an error frame.
    pub fn error(channel_id: u32, code: u32, message: &str) -> Self {
        let mut payload = BytesMut::with_capacity(4 + message.len());
        payload.put_u32(code);
        payload.put(message.as_bytes());
        Self {
            frame_type: FrameType::Error,
            flags: FrameFlags::default(),
            priority: Priority::High,
            channel_id,
            sequence: 0,
            timestamp: current_timestamp_micros(),
            payload: payload.freeze(),
        }
    }
    /// Encodes the frame to bytes.
    pub fn encode(&self) -> Bytes {
        let payload_len = self.payload.len();
        let total_len = Self::HEADER_SIZE + payload_len;
        let mut buffer = BytesMut::with_capacity(total_len);
        buffer.put_slice(&PROTOCOL_MAGIC);
        buffer.put_u8(PROTOCOL_VERSION);
        buffer.put_u8(self.frame_type.to_byte());
        buffer.put_u8(self.flags.bits());
        buffer.put_u8(self.priority.to_byte());
        buffer.put_u32(self.channel_id);
        buffer.put_u32(self.sequence);
        buffer.put_u64(self.timestamp);
        buffer.put_u32(payload_len as u32);
        let checksum = calculate_checksum(&buffer[4..], &self.payload);
        buffer.put_u32(checksum);
        buffer.put(self.payload.clone());
        buffer.freeze()
    }
    /// Decodes a frame from bytes.
    pub fn decode(mut data: Bytes) -> Result<Self> {
        if data.len() < Self::HEADER_SIZE {
            return Err(GatewayError::WebSocketError(
                "Frame too small for header".to_string(),
            ));
        }
        let magic = data.split_to(4);
        if magic[..] != PROTOCOL_MAGIC {
            return Err(GatewayError::WebSocketError(
                "Invalid protocol magic".to_string(),
            ));
        }
        let version = data.get_u8();
        if version != PROTOCOL_VERSION {
            return Err(GatewayError::WebSocketError(format!(
                "Unsupported protocol version: {}",
                version
            )));
        }
        let frame_type_byte = data.get_u8();
        let frame_type = FrameType::from_byte(frame_type_byte).ok_or_else(|| {
            GatewayError::WebSocketError(format!("Unknown frame type: {}", frame_type_byte))
        })?;
        let flags = FrameFlags::new(data.get_u8());
        let priority = Priority::from_byte(data.get_u8());
        let channel_id = data.get_u32();
        let sequence = data.get_u32();
        let timestamp = data.get_u64();
        let payload_len = data.get_u32() as usize;
        let checksum = data.get_u32();
        if payload_len > MAX_FRAME_SIZE {
            return Err(GatewayError::WebSocketError(format!(
                "Payload too large: {} bytes",
                payload_len
            )));
        }
        if data.len() < payload_len {
            return Err(GatewayError::WebSocketError(
                "Incomplete payload data".to_string(),
            ));
        }
        let payload = data.split_to(payload_len);
        let mut header_for_checksum = BytesMut::with_capacity(24);
        header_for_checksum.put_u8(version);
        header_for_checksum.put_u8(frame_type_byte);
        header_for_checksum.put_u8(flags.bits());
        header_for_checksum.put_u8(priority.to_byte());
        header_for_checksum.put_u32(channel_id);
        header_for_checksum.put_u32(sequence);
        header_for_checksum.put_u64(timestamp);
        header_for_checksum.put_u32(payload_len as u32);
        let calculated_checksum = calculate_checksum(&header_for_checksum, &payload);
        if checksum != calculated_checksum {
            return Err(GatewayError::WebSocketError(
                "Frame checksum mismatch".to_string(),
            ));
        }
        Ok(Self {
            frame_type,
            flags,
            priority,
            channel_id,
            sequence,
            timestamp,
            payload,
        })
    }
    /// Sets the compressed flag and compresses the payload.
    pub fn with_compression(mut self) -> Result<Self> {
        use crate::websocket::compression::{CompressionMethod, MessageCompressor};
        let compressor = MessageCompressor::new(CompressionMethod::Deflate);
        let compressed = compressor.compress(&self.payload)?;
        self.payload = Bytes::from(compressed);
        self.flags = FrameFlags::new(self.flags.bits() | FrameFlags::COMPRESSED);
        Ok(self)
    }
}
/// Heartbeat configuration.
#[derive(Debug, Clone)]
pub struct HeartbeatConfig {
    /// Heartbeat interval
    pub interval: Duration,
    /// Heartbeat timeout
    pub timeout: Duration,
    /// Maximum missed heartbeats before disconnect
    pub max_missed: u32,
}
/// Priority queue for outgoing frames.
pub struct PriorityQueue {
    /// Queues by priority level
    queues: [RwLock<VecDeque<Frame>>; 4],
}
impl PriorityQueue {
    /// Creates a new priority queue.
    pub fn new() -> Self {
        Self {
            queues: [
                RwLock::new(VecDeque::new()),
                RwLock::new(VecDeque::new()),
                RwLock::new(VecDeque::new()),
                RwLock::new(VecDeque::new()),
            ],
        }
    }
    /// Enqueues a frame.
    pub fn enqueue(&self, frame: Frame) {
        let priority = frame.priority.to_byte() as usize;
        let idx = priority.min(3);
        self.queues[idx].write().push_back(frame);
    }
    /// Dequeues the highest priority frame.
    pub fn dequeue(&self) -> Option<Frame> {
        for i in (0..4).rev() {
            let mut queue = self.queues[i].write();
            if let Some(frame) = queue.pop_front() {
                return Some(frame);
            }
        }
        None
    }
    /// Returns total number of queued frames.
    pub fn len(&self) -> usize {
        self.queues.iter().map(|q| q.read().len()).sum()
    }
    /// Checks if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.queues.iter().all(|q| q.read().is_empty())
    }
}
/// Channel multiplexer for WebSocket connections.
///
/// Manages multiple logical channels over a single WebSocket connection,
/// providing efficient binary protocol, heartbeat monitoring, authentication,
/// and backpressure handling.
pub struct ChannelMultiplexer {
    /// Configuration
    config: MultiplexerConfig,
    /// Connection ID
    connection_id: String,
    /// Active channels
    channels: Arc<DashMap<u32, Arc<Channel>>>,
    /// Next channel ID
    next_channel_id: AtomicU32,
    /// Outgoing frame sender
    outgoing_tx: mpsc::Sender<Frame>,
    /// Incoming frame receiver (for frame processing)
    incoming_rx: RwLock<Option<mpsc::Receiver<Frame>>>,
    /// Incoming frame sender (for external frame injection)
    incoming_tx: mpsc::Sender<Frame>,
    /// Heartbeat manager
    heartbeat: Arc<HeartbeatManager>,
    /// Backpressure manager
    backpressure: Arc<BackpressureManager>,
    /// Authentication context
    auth_context: RwLock<Option<AuthContext>>,
    /// Running flag
    running: AtomicBool,
    /// Pending channel open requests
    pending_opens: DashMap<u32, oneshot::Sender<Result<Arc<Channel>>>>,
}
impl ChannelMultiplexer {
    /// Creates a new channel multiplexer.
    pub fn new(connection_id: String, config: MultiplexerConfig) -> (Self, mpsc::Receiver<Frame>) {
        let (outgoing_tx, outgoing_rx) = mpsc::channel(config.channel_buffer_size);
        let (incoming_tx, incoming_rx) = mpsc::channel(config.channel_buffer_size);
        let heartbeat = Arc::new(HeartbeatManager::new(
            config.heartbeat.clone(),
            outgoing_tx.clone(),
        ));
        let backpressure = Arc::new(BackpressureManager::new(
            config.backpressure.clone(),
            outgoing_tx.clone(),
        ));
        let mux = Self {
            config,
            connection_id,
            channels: Arc::new(DashMap::new()),
            next_channel_id: AtomicU32::new(1),
            outgoing_tx,
            incoming_rx: RwLock::new(Some(incoming_rx)),
            incoming_tx,
            heartbeat,
            backpressure,
            auth_context: RwLock::new(None),
            running: AtomicBool::new(false),
            pending_opens: DashMap::new(),
        };
        (mux, outgoing_rx)
    }
    /// Returns the connection ID.
    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }
    /// Returns the outgoing frame sender.
    pub fn outgoing_sender(&self) -> mpsc::Sender<Frame> {
        self.outgoing_tx.clone()
    }
    /// Returns the incoming frame sender.
    pub fn incoming_sender(&self) -> mpsc::Sender<Frame> {
        self.incoming_tx.clone()
    }
    /// Starts the multiplexer with an authenticator.
    pub async fn start<A: Authenticator + 'static>(
        self: Arc<Self>,
        authenticator: Option<Arc<A>>,
    ) -> Result<()> {
        self.running.store(true, Ordering::SeqCst);
        let heartbeat = Arc::clone(&self.heartbeat);
        let disconnect_rx = heartbeat.start().await;
        let mux = Arc::clone(&self);
        let auth = authenticator;
        tokio::spawn(async move {
            mux.frame_processing_loop(auth).await;
        });
        let mux_clone = Arc::clone(&self);
        tokio::spawn(async move {
            let _ = disconnect_rx.await;
            mux_clone.shutdown().await;
        });
        Ok(())
    }
    /// Frame processing loop.
    async fn frame_processing_loop<A: Authenticator + 'static>(
        &self,
        authenticator: Option<Arc<A>>,
    ) {
        let mut rx = match self.incoming_rx.write().take() {
            Some(rx) => rx,
            None => return,
        };
        while self.running.load(Ordering::SeqCst) {
            match rx.recv().await {
                Some(frame) => {
                    if let Err(e) = self.process_frame(frame, authenticator.as_ref()).await {
                        tracing::error!("Frame processing error: {}", e);
                    }
                }
                None => break,
            }
        }
    }
    /// Processes a received frame.
    async fn process_frame<A: Authenticator + 'static>(
        &self,
        frame: Frame,
        authenticator: Option<&Arc<A>>,
    ) -> Result<()> {
        match frame.frame_type {
            FrameType::Data => self.handle_data_frame(frame).await,
            FrameType::Ping => self.heartbeat.handle_ping(frame.payload).await,
            FrameType::Pong => {
                let _ = self.heartbeat.handle_pong(&frame.payload);
                Ok(())
            }
            FrameType::Auth => self.handle_auth_frame(frame, authenticator).await,
            FrameType::ChannelOpen => self.handle_channel_open(frame).await,
            FrameType::ChannelOpenAck => self.handle_channel_open_ack(frame).await,
            FrameType::ChannelClose => self.handle_channel_close(frame).await,
            FrameType::ChannelCloseAck => self.handle_channel_close_ack(frame).await,
            FrameType::FlowPause => self.handle_flow_pause(frame).await,
            FrameType::FlowResume => self.handle_flow_resume(frame).await,
            FrameType::Error => self.handle_error_frame(frame).await,
            _ => Ok(()),
        }
    }
    /// Handles a data frame.
    async fn handle_data_frame(&self, frame: Frame) -> Result<()> {
        let channel = self.channels.get(&frame.channel_id).ok_or_else(|| {
            GatewayError::WebSocketError(format!("Unknown channel: {}", frame.channel_id))
        })?;
        self.backpressure
            .record_bytes(frame.channel_id, frame.payload.len() as u64)
            .await?;
        channel
            .incoming_sender()
            .send(frame)
            .await
            .map_err(|e| GatewayError::WebSocketError(format!("Failed to forward frame: {}", e)))
    }
    /// Handles an authentication frame.
    async fn handle_auth_frame<A: Authenticator + 'static>(
        &self,
        frame: Frame,
        authenticator: Option<&Arc<A>>,
    ) -> Result<()> {
        let token = String::from_utf8(frame.payload.to_vec())
            .map_err(|e| GatewayError::WebSocketError(format!("Invalid auth token: {}", e)))?;
        let auth_result = if let Some(auth) = authenticator {
            auth.authenticate(&token).await
        } else {
            Ok(AuthContext::new(
                Identity::new("anonymous".to_string()),
                crate::auth::AuthMethod::Session,
            ))
        };
        let response_payload = match &auth_result {
            Ok(_) => {
                let mut buf = BytesMut::with_capacity(5);
                buf.put_u8(1);
                buf.put_u32(0);
                buf.freeze()
            }
            Err(e) => {
                let msg = e.to_string();
                let mut buf = BytesMut::with_capacity(5 + msg.len());
                buf.put_u8(0);
                buf.put_u32(1);
                buf.put(msg.as_bytes());
                buf.freeze()
            }
        };
        let response = Frame {
            frame_type: FrameType::AuthResponse,
            flags: FrameFlags::default(),
            priority: Priority::Critical,
            channel_id: 0,
            sequence: 0,
            timestamp: current_timestamp_micros(),
            payload: response_payload,
        };
        self.outgoing_tx.send(response).await.map_err(|e| {
            GatewayError::WebSocketError(format!("Failed to send auth response: {}", e))
        })?;
        if let Ok(ctx) = auth_result {
            *self.auth_context.write() = Some(ctx);
        }
        Ok(())
    }
    /// Handles a channel open request.
    async fn handle_channel_open(&self, frame: Frame) -> Result<()> {
        if self.channels.len() >= self.config.max_channels {
            let error = Frame::error(frame.channel_id, 1, "Maximum channels reached");
            return self
                .outgoing_tx
                .send(error)
                .await
                .map_err(|e| GatewayError::WebSocketError(e.to_string()));
        }
        let channel_type = if !frame.payload.is_empty() {
            ChannelType::from_byte(frame.payload[0])
        } else {
            ChannelType::Data
        };
        let channel = Arc::new(Channel::new(
            frame.channel_id,
            channel_type,
            self.outgoing_tx.clone(),
            self.config.channel_buffer_size,
        ));
        channel.set_state(ChannelState::Open);
        self.channels.insert(frame.channel_id, channel);
        let ack = Frame {
            frame_type: FrameType::ChannelOpenAck,
            flags: FrameFlags::default(),
            priority: Priority::High,
            channel_id: frame.channel_id,
            sequence: 0,
            timestamp: current_timestamp_micros(),
            payload: Bytes::new(),
        };
        self.outgoing_tx
            .send(ack)
            .await
            .map_err(|e| GatewayError::WebSocketError(format!("Failed to send ack: {}", e)))
    }
    /// Handles a channel open acknowledgment.
    async fn handle_channel_open_ack(&self, frame: Frame) -> Result<()> {
        if let Some((_, tx)) = self.pending_opens.remove(&frame.channel_id) {
            if let Some(channel) = self.channels.get(&frame.channel_id) {
                channel.set_state(ChannelState::Open);
                let _ = tx.send(Ok(channel.clone()));
            }
        }
        Ok(())
    }
    /// Handles a channel close request.
    async fn handle_channel_close(&self, frame: Frame) -> Result<()> {
        if let Some((_, channel)) = self.channels.remove(&frame.channel_id) {
            channel.set_state(ChannelState::Closed);
        }
        let ack = Frame {
            frame_type: FrameType::ChannelCloseAck,
            flags: FrameFlags::default(),
            priority: Priority::High,
            channel_id: frame.channel_id,
            sequence: 0,
            timestamp: current_timestamp_micros(),
            payload: Bytes::new(),
        };
        self.outgoing_tx
            .send(ack)
            .await
            .map_err(|e| GatewayError::WebSocketError(format!("Failed to send close ack: {}", e)))
    }
    /// Handles a channel close acknowledgment.
    async fn handle_channel_close_ack(&self, frame: Frame) -> Result<()> {
        if let Some((_, channel)) = self.channels.remove(&frame.channel_id) {
            channel.set_state(ChannelState::Closed);
        }
        Ok(())
    }
    /// Handles a flow pause frame.
    async fn handle_flow_pause(&self, frame: Frame) -> Result<()> {
        if let Some(channel) = self.channels.get(&frame.channel_id) {
            channel.pause();
        }
        Ok(())
    }
    /// Handles a flow resume frame.
    async fn handle_flow_resume(&self, frame: Frame) -> Result<()> {
        if let Some(channel) = self.channels.get(&frame.channel_id) {
            channel.resume();
        }
        Ok(())
    }
    /// Handles an error frame.
    async fn handle_error_frame(&self, frame: Frame) -> Result<()> {
        if frame.payload.len() >= 4 {
            let mut buf = &frame.payload[..];
            let code = buf.get_u32();
            let message = String::from_utf8_lossy(buf);
            tracing::error!(
                "Received error on channel {}: code={}, message={}",
                frame.channel_id,
                code,
                message
            );
        }
        Ok(())
    }
    /// Opens a new channel.
    pub async fn open_channel(&self, channel_type: ChannelType) -> Result<Arc<Channel>> {
        if self.channels.len() >= self.config.max_channels {
            return Err(GatewayError::WebSocketError(
                "Maximum channels reached".to_string(),
            ));
        }
        let channel_id = self.next_channel_id.fetch_add(1, Ordering::SeqCst);
        let channel = Arc::new(Channel::new(
            channel_id,
            channel_type,
            self.outgoing_tx.clone(),
            self.config.channel_buffer_size,
        ));
        self.channels.insert(channel_id, Arc::clone(&channel));
        let (tx, rx) = oneshot::channel();
        self.pending_opens.insert(channel_id, tx);
        let frame = Frame::channel_open(channel_id, channel_type, Bytes::new());
        self.outgoing_tx.send(frame).await.map_err(|e| {
            GatewayError::WebSocketError(format!("Failed to send open request: {}", e))
        })?;
        match tokio::time::timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(GatewayError::WebSocketError(
                "Channel open cancelled".to_string(),
            )),
            Err(_) => {
                self.pending_opens.remove(&channel_id);
                self.channels.remove(&channel_id);
                Err(GatewayError::Timeout("Channel open timeout".to_string()))
            }
        }
    }
    /// Gets a channel by ID.
    pub fn get_channel(&self, channel_id: u32) -> Option<Arc<Channel>> {
        self.channels.get(&channel_id).map(|c| Arc::clone(&c))
    }
    /// Returns all active channels.
    pub fn channels(&self) -> Vec<Arc<Channel>> {
        self.channels.iter().map(|c| Arc::clone(&c)).collect()
    }
    /// Returns the number of active channels.
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }
    /// Returns the authentication context.
    pub fn auth_context(&self) -> Option<AuthContext> {
        self.auth_context.read().clone()
    }
    /// Checks if the connection is authenticated.
    pub fn is_authenticated(&self) -> bool {
        self.auth_context.read().is_some()
    }
    /// Returns the heartbeat manager.
    pub fn heartbeat(&self) -> &Arc<HeartbeatManager> {
        &self.heartbeat
    }
    /// Returns the backpressure manager.
    pub fn backpressure(&self) -> &Arc<BackpressureManager> {
        &self.backpressure
    }
    /// Shuts down the multiplexer.
    pub async fn shutdown(&self) {
        self.running.store(false, Ordering::SeqCst);
        self.heartbeat.stop();
        for entry in self.channels.iter() {
            let _ = entry.close().await;
        }
        self.channels.clear();
    }
    /// Checks if the multiplexer is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}
/// Frame type identifiers for the binary protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameType {
    /// Data frame carrying payload
    Data = 0x01,
    /// Control frame for channel management
    Control = 0x02,
    /// Heartbeat ping
    Ping = 0x03,
    /// Heartbeat pong
    Pong = 0x04,
    /// Authentication frame
    Auth = 0x05,
    /// Authentication response
    AuthResponse = 0x06,
    /// Channel open request
    ChannelOpen = 0x10,
    /// Channel open acknowledgment
    ChannelOpenAck = 0x11,
    /// Channel close request
    ChannelClose = 0x12,
    /// Channel close acknowledgment
    ChannelCloseAck = 0x13,
    /// Flow control: pause
    FlowPause = 0x20,
    /// Flow control: resume
    FlowResume = 0x21,
    /// Error frame
    Error = 0xFF,
}
impl FrameType {
    /// Creates a `FrameType` from a byte value.
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x01 => Some(Self::Data),
            0x02 => Some(Self::Control),
            0x03 => Some(Self::Ping),
            0x04 => Some(Self::Pong),
            0x05 => Some(Self::Auth),
            0x06 => Some(Self::AuthResponse),
            0x10 => Some(Self::ChannelOpen),
            0x11 => Some(Self::ChannelOpenAck),
            0x12 => Some(Self::ChannelClose),
            0x13 => Some(Self::ChannelCloseAck),
            0x20 => Some(Self::FlowPause),
            0x21 => Some(Self::FlowResume),
            0xFF => Some(Self::Error),
            _ => None,
        }
    }
    /// Converts the `FrameType` to its byte representation.
    pub fn to_byte(self) -> u8 {
        self as u8
    }
}
/// Reconnection handler with exponential backoff.
pub struct ReconnectionHandler {
    /// Configuration
    config: ReconnectionConfig,
    /// Current state
    state: RwLock<ReconnectionState>,
    /// Current attempt number
    attempt: AtomicU32,
    /// Current delay
    current_delay: RwLock<Duration>,
}
impl ReconnectionHandler {
    /// Creates a new reconnection handler.
    pub fn new(config: ReconnectionConfig) -> Self {
        let initial_delay = config.initial_delay;
        Self {
            config,
            state: RwLock::new(ReconnectionState::Connected),
            attempt: AtomicU32::new(0),
            current_delay: RwLock::new(initial_delay),
        }
    }
    /// Called when connection is lost.
    pub fn on_disconnect(&self) {
        *self.state.write() = ReconnectionState::Reconnecting;
    }
    /// Called when reconnection succeeds.
    pub fn on_reconnect(&self) {
        *self.state.write() = ReconnectionState::Connected;
        self.attempt.store(0, Ordering::SeqCst);
        *self.current_delay.write() = self.config.initial_delay;
    }
    /// Returns the current state.
    pub fn state(&self) -> ReconnectionState {
        *self.state.read()
    }
    /// Calculates the next delay for reconnection attempt.
    pub fn next_delay(&self) -> Option<Duration> {
        let attempt = self.attempt.fetch_add(1, Ordering::SeqCst);
        if let Some(max) = self.config.max_attempts {
            if attempt >= max {
                *self.state.write() = ReconnectionState::Failed;
                return None;
            }
        }
        let mut delay = *self.current_delay.read();
        if self.config.jitter > 0.0 {
            let jitter_range = (delay.as_millis() as f64 * self.config.jitter) as u64;
            if jitter_range > 0 {
                let jitter =
                    (current_timestamp_micros() % jitter_range) as i64 - (jitter_range as i64 / 2);
                delay = Duration::from_millis((delay.as_millis() as i64 + jitter).max(1) as u64);
            }
        }
        let next_delay = Duration::from_millis(
            ((delay.as_millis() as f64) * self.config.multiplier)
                .min(self.config.max_delay.as_millis() as f64) as u64,
        );
        *self.current_delay.write() = next_delay;
        Some(delay)
    }
    /// Resets the reconnection handler.
    pub fn reset(&self) {
        self.attempt.store(0, Ordering::SeqCst);
        *self.current_delay.write() = self.config.initial_delay;
        *self.state.write() = ReconnectionState::Connected;
    }
    /// Returns the current attempt number.
    pub fn attempt(&self) -> u32 {
        self.attempt.load(Ordering::SeqCst)
    }
}
