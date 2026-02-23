//! Replication protocol implementation.

use super::{ReplicationEvent, VectorClock};
use crate::error::{HaError, HaResult};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use uuid::Uuid;

/// Replication protocol version.
pub const PROTOCOL_VERSION: u8 = 1;

/// Compression algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionAlgorithm {
    /// No compression.
    None,
    /// LZ4 compression.
    Lz4,
    /// Zstandard compression.
    Zstd,
    /// Gzip compression.
    Gzip,
}

/// Replication message types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageType {
    /// Handshake message.
    Handshake,
    /// Event message.
    Event,
    /// Batch of events.
    EventBatch,
    /// Acknowledgment.
    Ack,
    /// Heartbeat.
    Heartbeat,
    /// Sync request.
    SyncRequest,
    /// Sync response.
    SyncResponse,
}

/// Replication handshake message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeMessage {
    /// Protocol version.
    pub version: u8,
    /// Node ID.
    pub node_id: Uuid,
    /// Node name.
    pub node_name: String,
    /// Supported compression algorithms.
    pub supported_compression: Vec<CompressionAlgorithm>,
}

/// Event batch message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventBatchMessage {
    /// Batch ID.
    pub batch_id: Uuid,
    /// Events in the batch.
    pub events: Vec<ReplicationEvent>,
    /// Vector clock.
    pub vector_clock: VectorClock,
}

/// Acknowledgment message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckMessage {
    /// Acknowledged event/batch ID.
    pub id: Uuid,
    /// Node ID sending the ack.
    pub node_id: Uuid,
    /// Success flag.
    pub success: bool,
    /// Optional error message.
    pub error: Option<String>,
}

/// Heartbeat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatMessage {
    /// Node ID.
    pub node_id: Uuid,
    /// Timestamp.
    pub timestamp: i64,
    /// Current sequence number.
    pub sequence: u64,
}

/// Sync request message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRequestMessage {
    /// Requesting node ID.
    pub node_id: Uuid,
    /// Last known sequence number.
    pub last_sequence: u64,
}

/// Sync response message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResponseMessage {
    /// Events to sync.
    pub events: Vec<ReplicationEvent>,
    /// Current sequence number.
    pub current_sequence: u64,
}

/// Replication protocol message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMessage {
    /// Message type.
    pub message_type: MessageType,
    /// Message ID.
    pub message_id: Uuid,
    /// Payload.
    pub payload: Vec<u8>,
    /// Compression algorithm used.
    pub compression: CompressionAlgorithm,
    /// Checksum.
    pub checksum: u32,
}

impl ProtocolMessage {
    /// Create a new protocol message.
    pub fn new(
        message_type: MessageType,
        payload: Vec<u8>,
        compression: CompressionAlgorithm,
    ) -> Self {
        let checksum = crc32fast::hash(&payload);
        Self {
            message_type,
            message_id: Uuid::new_v4(),
            payload,
            compression,
            checksum,
        }
    }

    /// Create a handshake message.
    pub fn handshake(node_id: Uuid, node_name: String) -> HaResult<Self> {
        let handshake = HandshakeMessage {
            version: PROTOCOL_VERSION,
            node_id,
            node_name,
            supported_compression: vec![
                CompressionAlgorithm::None,
                CompressionAlgorithm::Lz4,
                CompressionAlgorithm::Zstd,
                CompressionAlgorithm::Gzip,
            ],
        };

        let payload = oxicode::serde::encode_to_vec(&handshake, oxicode::config::standard())?;

        Ok(Self::new(
            MessageType::Handshake,
            payload,
            CompressionAlgorithm::None,
        ))
    }

    /// Create an event batch message.
    pub fn event_batch(
        events: Vec<ReplicationEvent>,
        vector_clock: VectorClock,
        compression: CompressionAlgorithm,
    ) -> HaResult<Self> {
        let batch = EventBatchMessage {
            batch_id: Uuid::new_v4(),
            events,
            vector_clock,
        };

        let mut payload = oxicode::serde::encode_to_vec(&batch, oxicode::config::standard())?;

        payload = compress_data(&payload, compression)?;

        Ok(Self::new(MessageType::EventBatch, payload, compression))
    }

    /// Create an acknowledgment message.
    pub fn ack(id: Uuid, node_id: Uuid, success: bool, error: Option<String>) -> HaResult<Self> {
        let ack = AckMessage {
            id,
            node_id,
            success,
            error,
        };

        let payload = oxicode::serde::encode_to_vec(&ack, oxicode::config::standard())?;

        Ok(Self::new(
            MessageType::Ack,
            payload,
            CompressionAlgorithm::None,
        ))
    }

    /// Create a heartbeat message.
    pub fn heartbeat(node_id: Uuid, sequence: u64) -> HaResult<Self> {
        let heartbeat = HeartbeatMessage {
            node_id,
            timestamp: chrono::Utc::now().timestamp_millis(),
            sequence,
        };

        let payload = oxicode::serde::encode_to_vec(&heartbeat, oxicode::config::standard())?;

        Ok(Self::new(
            MessageType::Heartbeat,
            payload,
            CompressionAlgorithm::None,
        ))
    }

    /// Verify message integrity.
    pub fn verify_checksum(&self) -> HaResult<()> {
        let actual = crc32fast::hash(&self.payload);
        if actual == self.checksum {
            Ok(())
        } else {
            Err(HaError::ChecksumMismatch {
                expected: self.checksum,
                actual,
            })
        }
    }

    /// Decode the payload.
    pub fn decode_payload<T: for<'de> Deserialize<'de>>(&self) -> HaResult<T> {
        self.verify_checksum()?;

        let decompressed = decompress_data(&self.payload, self.compression)?;

        let (decoded, _) = oxicode::serde::decode_owned_from_slice::<T, _>(
            &decompressed,
            oxicode::config::standard(),
        )?;
        Ok(decoded)
    }

    /// Encode to bytes.
    pub fn encode(&self) -> HaResult<Bytes> {
        let data = oxicode::serde::encode_to_vec(self, oxicode::config::standard())?;
        Ok(Bytes::from(data))
    }

    /// Decode from bytes.
    pub fn decode(data: &[u8]) -> HaResult<Self> {
        let (decoded, _) =
            oxicode::serde::decode_owned_from_slice::<Self, _>(data, oxicode::config::standard())?;
        Ok(decoded)
    }
}

/// Compress data using the specified algorithm.
pub fn compress_data(data: &[u8], algorithm: CompressionAlgorithm) -> HaResult<Vec<u8>> {
    match algorithm {
        CompressionAlgorithm::None => Ok(data.to_vec()),
        CompressionAlgorithm::Lz4 => {
            let mut encoder = lz4::EncoderBuilder::new()
                .level(4)
                .build(Vec::new())
                .map_err(|e| HaError::Compression(e.to_string()))?;

            encoder
                .write_all(data)
                .map_err(|e| HaError::Compression(e.to_string()))?;

            let (compressed, result) = encoder.finish();
            result.map_err(|e| HaError::Compression(e.to_string()))?;

            Ok(compressed)
        }
        CompressionAlgorithm::Zstd => {
            let compressed =
                zstd::encode_all(data, 3).map_err(|e| HaError::Compression(e.to_string()))?;
            Ok(compressed)
        }
        CompressionAlgorithm::Gzip => {
            use flate2::Compression;
            use flate2::write::GzEncoder;

            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder
                .write_all(data)
                .map_err(|e| HaError::Compression(e.to_string()))?;

            encoder
                .finish()
                .map_err(|e| HaError::Compression(e.to_string()))
        }
    }
}

/// Decompress data using the specified algorithm.
pub fn decompress_data(data: &[u8], algorithm: CompressionAlgorithm) -> HaResult<Vec<u8>> {
    match algorithm {
        CompressionAlgorithm::None => Ok(data.to_vec()),
        CompressionAlgorithm::Lz4 => {
            let mut decoder =
                lz4::Decoder::new(data).map_err(|e| HaError::Decompression(e.to_string()))?;

            let mut decompressed = Vec::new();
            decoder
                .read_to_end(&mut decompressed)
                .map_err(|e| HaError::Decompression(e.to_string()))?;

            Ok(decompressed)
        }
        CompressionAlgorithm::Zstd => {
            zstd::decode_all(data).map_err(|e| HaError::Decompression(e.to_string()))
        }
        CompressionAlgorithm::Gzip => {
            use flate2::read::GzDecoder;

            let mut decoder = GzDecoder::new(data);
            let mut decompressed = Vec::new();
            decoder
                .read_to_end(&mut decompressed)
                .map_err(|e| HaError::Decompression(e.to_string()))?;

            Ok(decompressed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_message_handshake() {
        let node_id = Uuid::new_v4();
        let message = ProtocolMessage::handshake(node_id, "test-node".to_string()).ok();
        assert!(message.is_some());

        if let Some(msg) = message {
            assert_eq!(msg.message_type, MessageType::Handshake);
            assert!(msg.verify_checksum().is_ok());

            if let Ok(handshake) = msg.decode_payload::<HandshakeMessage>() {
                assert_eq!(handshake.node_id, node_id);
                assert_eq!(handshake.node_name, "test-node");
            }
        }
    }

    #[test]
    fn test_compression() {
        let data = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

        for algorithm in [
            CompressionAlgorithm::None,
            CompressionAlgorithm::Lz4,
            CompressionAlgorithm::Zstd,
            CompressionAlgorithm::Gzip,
        ] {
            let compressed = compress_data(&data, algorithm).ok();
            assert!(compressed.is_some());

            if let Some(comp) = compressed {
                let decompressed = decompress_data(&comp, algorithm).ok();
                assert!(decompressed.is_some());
                if let Some(decomp) = decompressed {
                    assert_eq!(decomp, data);
                }
            }
        }
    }
}
