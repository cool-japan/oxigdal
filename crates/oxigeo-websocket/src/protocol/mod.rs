//! WebSocket protocol implementation
//!
//! This module provides protocol support for WebSocket communication including:
//! - Binary and JSON message formats
//! - Message framing and encoding
//! - Compression support (gzip, zstd)
//! - Geospatial-optimized binary protocols

pub mod binary;
pub mod compression;
pub mod framing;
pub mod json;
pub mod message;

pub use binary::{BinaryCodec, BinaryMessage, GeospatialBinaryProtocol};
pub use compression::{CompressionCodec, CompressionLevel, CompressionType};
pub use framing::{Frame, FrameCodec, FrameHeader, FrameType};
pub use json::{JsonCodec, JsonMessage};
pub use message::{Message, MessageType, Payload};

use crate::error::{Error, Result};
use bytes::{Bytes, BytesMut};
use serde::{Deserialize, Serialize};

/// Protocol version
pub const PROTOCOL_VERSION: u8 = 1;

/// Maximum message size (16MB)
pub const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;

/// Message format enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageFormat {
    /// JSON format
    Json,
    /// Binary format
    Binary,
    /// MessagePack format
    MessagePack,
}

/// Protocol configuration
#[derive(Debug, Clone)]
pub struct ProtocolConfig {
    /// Message format
    pub format: MessageFormat,
    /// Compression type
    pub compression: Option<CompressionType>,
    /// Compression level
    pub compression_level: CompressionLevel,
    /// Enable framing
    pub enable_framing: bool,
    /// Maximum message size
    pub max_message_size: usize,
}

impl Default for ProtocolConfig {
    fn default() -> Self {
        Self {
            format: MessageFormat::Binary,
            compression: Some(CompressionType::Zstd),
            compression_level: CompressionLevel::Default,
            enable_framing: true,
            max_message_size: MAX_MESSAGE_SIZE,
        }
    }
}

/// Protocol codec for encoding/decoding messages
pub struct ProtocolCodec {
    config: ProtocolConfig,
    compression_codec: Option<CompressionCodec>,
    frame_codec: FrameCodec,
}

impl ProtocolCodec {
    /// Create a new protocol codec
    pub fn new(config: ProtocolConfig) -> Self {
        let compression_codec = config
            .compression
            .map(|ct| CompressionCodec::new(ct, config.compression_level));

        Self {
            config,
            compression_codec,
            frame_codec: FrameCodec::new(),
        }
    }

    /// Encode a message
    pub fn encode(&self, message: &Message) -> Result<Bytes> {
        // Serialize message based on format
        let mut data = match self.config.format {
            MessageFormat::Json => {
                let json = serde_json::to_vec(message)?;
                BytesMut::from(&json[..])
            }
            MessageFormat::Binary => BinaryCodec::encode(message)?,
            MessageFormat::MessagePack => {
                let msgpack = rmp_serde::to_vec(message)?;
                BytesMut::from(&msgpack[..])
            }
        };

        // Apply compression if enabled
        if let Some(ref codec) = self.compression_codec {
            data = codec.compress(&data)?;
        }

        // Check message size
        if data.len() > self.config.max_message_size {
            return Err(Error::Protocol(format!(
                "Message size {} exceeds maximum {}",
                data.len(),
                self.config.max_message_size
            )));
        }

        // Apply framing if enabled
        if self.config.enable_framing {
            let frame = Frame::new(
                FrameType::Data,
                PROTOCOL_VERSION,
                self.compression_codec.is_some(),
                data.freeze(),
            );
            self.frame_codec.encode(&frame)
        } else {
            Ok(data.freeze())
        }
    }

    /// Decode a message
    pub fn decode(&self, data: &[u8]) -> Result<Message> {
        // Decode framing if enabled
        let payload = if self.config.enable_framing {
            let frame = self.frame_codec.decode(data)?;
            frame.payload
        } else {
            Bytes::copy_from_slice(data)
        };

        // Decompress if needed
        let decompressed = if let Some(ref codec) = self.compression_codec {
            codec.decompress(&payload)?
        } else {
            payload
        };

        // Deserialize based on format
        match self.config.format {
            MessageFormat::Json => {
                let message: Message = serde_json::from_slice(&decompressed)?;
                Ok(message)
            }
            MessageFormat::Binary => BinaryCodec::decode(&decompressed),
            MessageFormat::MessagePack => {
                let message: Message = rmp_serde::from_slice(&decompressed)?;
                Ok(message)
            }
        }
    }

    /// Get protocol configuration
    pub fn config(&self) -> &ProtocolConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_codec_json() -> Result<()> {
        let config = ProtocolConfig {
            format: MessageFormat::Json,
            compression: None,
            enable_framing: false,
            ..Default::default()
        };

        let codec = ProtocolCodec::new(config);
        let message = Message::ping();

        let encoded = codec.encode(&message)?;
        let decoded = codec.decode(&encoded)?;

        assert_eq!(message.message_type(), decoded.message_type());
        Ok(())
    }

    #[test]
    fn test_protocol_codec_binary() -> Result<()> {
        let config = ProtocolConfig {
            format: MessageFormat::Binary,
            compression: None,
            enable_framing: false,
            ..Default::default()
        };

        let codec = ProtocolCodec::new(config);
        let message = Message::ping();

        let encoded = codec.encode(&message)?;
        let decoded = codec.decode(&encoded)?;

        assert_eq!(message.message_type(), decoded.message_type());
        Ok(())
    }

    #[test]
    fn test_protocol_codec_with_compression() -> Result<()> {
        let config = ProtocolConfig {
            format: MessageFormat::Binary,
            compression: Some(CompressionType::Zstd),
            compression_level: CompressionLevel::Fast,
            enable_framing: true,
            ..Default::default()
        };

        let codec = ProtocolCodec::new(config);
        let message = Message::ping();

        let encoded = codec.encode(&message)?;
        let decoded = codec.decode(&encoded)?;

        assert_eq!(message.message_type(), decoded.message_type());
        Ok(())
    }
}
