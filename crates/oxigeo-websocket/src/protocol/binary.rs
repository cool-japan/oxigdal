//! Binary protocol implementation for geospatial data

use crate::error::{Error, Result};
use crate::protocol::message::{Message, MessageType, Payload};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use bytes::{BufMut, Bytes, BytesMut};
use std::io::{Cursor, Read};

/// Binary protocol version
pub const BINARY_PROTOCOL_VERSION: u8 = 1;

/// Binary message codec
pub struct BinaryCodec;

impl BinaryCodec {
    /// Encode a message to binary format
    pub fn encode(message: &Message) -> Result<BytesMut> {
        let mut buf = BytesMut::new();

        // Write version
        buf.put_u8(BINARY_PROTOCOL_VERSION);

        // Write message type
        buf.put_u8(message.msg_type as u8);

        // Write message ID
        buf.put_slice(message.id.as_bytes());

        // Write timestamp
        buf.put_i64(message.timestamp);

        // Write correlation ID flag and value
        if let Some(corr_id) = message.correlation_id {
            buf.put_u8(1);
            buf.put_slice(corr_id.as_bytes());
        } else {
            buf.put_u8(0);
        }

        // Encode payload
        Self::encode_payload(&message.payload, &mut buf)?;

        Ok(buf)
    }

    /// Decode a message from binary format
    pub fn decode(data: &[u8]) -> Result<Message> {
        let mut cursor = Cursor::new(data);

        // Read version
        let version = cursor
            .read_u8()
            .map_err(|e| Error::Protocol(format!("Failed to read version: {}", e)))?;

        if version != BINARY_PROTOCOL_VERSION {
            return Err(Error::Protocol(format!(
                "Unsupported protocol version: {}",
                version
            )));
        }

        // Read message type
        let msg_type_u8 = cursor
            .read_u8()
            .map_err(|e| Error::Protocol(format!("Failed to read message type: {}", e)))?;
        let msg_type = Self::decode_message_type(msg_type_u8)?;

        // Read message ID
        let mut id_bytes = [0u8; 16];
        cursor
            .read_exact(&mut id_bytes)
            .map_err(|e| Error::Protocol(format!("Failed to read message ID: {}", e)))?;
        let id = uuid::Uuid::from_bytes(id_bytes);

        // Read timestamp
        let timestamp = cursor
            .read_i64::<BigEndian>()
            .map_err(|e| Error::Protocol(format!("Failed to read timestamp: {}", e)))?;

        // Read correlation ID
        let has_corr_id = cursor
            .read_u8()
            .map_err(|e| Error::Protocol(format!("Failed to read correlation flag: {}", e)))?;
        let correlation_id = if has_corr_id == 1 {
            let mut corr_id_bytes = [0u8; 16];
            cursor
                .read_exact(&mut corr_id_bytes)
                .map_err(|e| Error::Protocol(format!("Failed to read correlation ID: {}", e)))?;
            Some(uuid::Uuid::from_bytes(corr_id_bytes))
        } else {
            None
        };

        // Decode payload
        let payload = Self::decode_payload(&mut cursor)?;

        Ok(Message {
            id,
            msg_type,
            timestamp,
            payload,
            correlation_id,
        })
    }

    /// Encode payload
    fn encode_payload(payload: &Payload, buf: &mut BytesMut) -> Result<()> {
        match payload {
            Payload::Empty => {
                buf.put_u8(0);
            }
            Payload::Text(text) => {
                buf.put_u8(1);
                buf.put_u32(text.len() as u32);
                buf.put_slice(text.as_bytes());
            }
            Payload::Binary(data) => {
                buf.put_u8(2);
                buf.put_u32(data.len() as u32);
                buf.put_slice(data);
            }
            Payload::Json(value) => {
                buf.put_u8(3);
                let json = serde_json::to_vec(value)?;
                buf.put_u32(json.len() as u32);
                buf.put_slice(&json);
            }
            Payload::TileData(tile) => {
                buf.put_u8(4);
                // Encode tile data
                buf.put_u8(tile.z);
                buf.put_u32(tile.x);
                buf.put_u32(tile.y);
                buf.put_u32(tile.format.len() as u32);
                buf.put_slice(tile.format.as_bytes());
                buf.put_u32(tile.data.len() as u32);
                buf.put_slice(&tile.data);
                // Encode delta flag and data
                if let Some(delta) = &tile.delta {
                    buf.put_u8(1);
                    buf.put_u32(delta.len() as u32);
                    buf.put_slice(delta);
                } else {
                    buf.put_u8(0);
                }
            }
            Payload::FeatureData(feature) => {
                buf.put_u8(5);
                // Use MessagePack for complex nested structures
                let encoded = rmp_serde::to_vec(feature)?;
                buf.put_u32(encoded.len() as u32);
                buf.put_slice(&encoded);
            }
            Payload::ChangeEvent(change) => {
                buf.put_u8(6);
                let encoded = rmp_serde::to_vec(change)?;
                buf.put_u32(encoded.len() as u32);
                buf.put_slice(&encoded);
            }
            Payload::Subscribe(sub) => {
                buf.put_u8(7);
                let encoded = rmp_serde::to_vec(sub)?;
                buf.put_u32(encoded.len() as u32);
                buf.put_slice(&encoded);
            }
            Payload::Room(room) => {
                buf.put_u8(8);
                buf.put_u32(room.room.len() as u32);
                buf.put_slice(room.room.as_bytes());
            }
            Payload::Error(err) => {
                buf.put_u8(9);
                buf.put_u32(err.code);
                buf.put_u32(err.message.len() as u32);
                buf.put_slice(err.message.as_bytes());
            }
        }

        Ok(())
    }

    /// Decode payload
    fn decode_payload(cursor: &mut Cursor<&[u8]>) -> Result<Payload> {
        let payload_type = cursor
            .read_u8()
            .map_err(|e| Error::Protocol(format!("Failed to read payload type: {}", e)))?;

        match payload_type {
            0 => Ok(Payload::Empty),
            1 => {
                // Text
                let len = cursor
                    .read_u32::<BigEndian>()
                    .map_err(|e| Error::Protocol(format!("Failed to read text length: {}", e)))?
                    as usize;
                let mut text_bytes = vec![0u8; len];
                cursor
                    .read_exact(&mut text_bytes)
                    .map_err(|e| Error::Protocol(format!("Failed to read text: {}", e)))?;
                let text = String::from_utf8(text_bytes)
                    .map_err(|e| Error::Protocol(format!("Invalid UTF-8: {}", e)))?;
                Ok(Payload::Text(text))
            }
            2 => {
                // Binary
                let len = cursor
                    .read_u32::<BigEndian>()
                    .map_err(|e| Error::Protocol(format!("Failed to read binary length: {}", e)))?
                    as usize;
                let mut data = vec![0u8; len];
                cursor
                    .read_exact(&mut data)
                    .map_err(|e| Error::Protocol(format!("Failed to read binary: {}", e)))?;
                Ok(Payload::Binary(data))
            }
            3 => {
                // JSON
                let len = cursor
                    .read_u32::<BigEndian>()
                    .map_err(|e| Error::Protocol(format!("Failed to read JSON length: {}", e)))?
                    as usize;
                let mut json_bytes = vec![0u8; len];
                cursor
                    .read_exact(&mut json_bytes)
                    .map_err(|e| Error::Protocol(format!("Failed to read JSON: {}", e)))?;
                let value: serde_json::Value = serde_json::from_slice(&json_bytes)?;
                Ok(Payload::Json(value))
            }
            4 => {
                // TileData
                let z = cursor
                    .read_u8()
                    .map_err(|e| Error::Protocol(format!("Failed to read tile z: {}", e)))?;
                let x = cursor
                    .read_u32::<BigEndian>()
                    .map_err(|e| Error::Protocol(format!("Failed to read tile x: {}", e)))?;
                let y = cursor
                    .read_u32::<BigEndian>()
                    .map_err(|e| Error::Protocol(format!("Failed to read tile y: {}", e)))?;

                let format_len = cursor
                    .read_u32::<BigEndian>()
                    .map_err(|e| Error::Protocol(format!("Failed to read format length: {}", e)))?
                    as usize;
                let mut format_bytes = vec![0u8; format_len];
                cursor
                    .read_exact(&mut format_bytes)
                    .map_err(|e| Error::Protocol(format!("Failed to read format: {}", e)))?;
                let format = String::from_utf8(format_bytes)
                    .map_err(|e| Error::Protocol(format!("Invalid format UTF-8: {}", e)))?;

                let data_len = cursor
                    .read_u32::<BigEndian>()
                    .map_err(|e| Error::Protocol(format!("Failed to read data length: {}", e)))?
                    as usize;
                let mut data = vec![0u8; data_len];
                cursor
                    .read_exact(&mut data)
                    .map_err(|e| Error::Protocol(format!("Failed to read tile data: {}", e)))?;

                let has_delta = cursor
                    .read_u8()
                    .map_err(|e| Error::Protocol(format!("Failed to read delta flag: {}", e)))?;
                let delta = if has_delta == 1 {
                    let delta_len = cursor.read_u32::<BigEndian>().map_err(|e| {
                        Error::Protocol(format!("Failed to read delta length: {}", e))
                    })? as usize;
                    let mut delta_data = vec![0u8; delta_len];
                    cursor
                        .read_exact(&mut delta_data)
                        .map_err(|e| Error::Protocol(format!("Failed to read delta: {}", e)))?;
                    Some(delta_data)
                } else {
                    None
                };

                Ok(Payload::TileData(crate::protocol::message::TilePayload {
                    z,
                    x,
                    y,
                    data,
                    format,
                    delta,
                }))
            }
            5..=7 => {
                // FeatureData, ChangeEvent, Subscribe (MessagePack encoded)
                let len = cursor
                    .read_u32::<BigEndian>()
                    .map_err(|e| Error::Protocol(format!("Failed to read length: {}", e)))?
                    as usize;
                let mut data = vec![0u8; len];
                cursor
                    .read_exact(&mut data)
                    .map_err(|e| Error::Protocol(format!("Failed to read data: {}", e)))?;

                match payload_type {
                    5 => {
                        let feature = rmp_serde::from_slice(&data)?;
                        Ok(Payload::FeatureData(feature))
                    }
                    6 => {
                        let change = rmp_serde::from_slice(&data)?;
                        Ok(Payload::ChangeEvent(change))
                    }
                    7 => {
                        let sub = rmp_serde::from_slice(&data)?;
                        Ok(Payload::Subscribe(sub))
                    }
                    _ => Err(Error::Protocol("Invalid payload type".to_string())),
                }
            }
            8 => {
                // Room
                let len = cursor
                    .read_u32::<BigEndian>()
                    .map_err(|e| Error::Protocol(format!("Failed to read room length: {}", e)))?
                    as usize;
                let mut room_bytes = vec![0u8; len];
                cursor
                    .read_exact(&mut room_bytes)
                    .map_err(|e| Error::Protocol(format!("Failed to read room: {}", e)))?;
                let room = String::from_utf8(room_bytes)
                    .map_err(|e| Error::Protocol(format!("Invalid room UTF-8: {}", e)))?;
                Ok(Payload::Room(crate::protocol::message::RoomPayload {
                    room,
                }))
            }
            9 => {
                // Error
                let code = cursor
                    .read_u32::<BigEndian>()
                    .map_err(|e| Error::Protocol(format!("Failed to read error code: {}", e)))?;
                let len = cursor.read_u32::<BigEndian>().map_err(|e| {
                    Error::Protocol(format!("Failed to read error message length: {}", e))
                })? as usize;
                let mut msg_bytes = vec![0u8; len];
                cursor
                    .read_exact(&mut msg_bytes)
                    .map_err(|e| Error::Protocol(format!("Failed to read error message: {}", e)))?;
                let message = String::from_utf8(msg_bytes)
                    .map_err(|e| Error::Protocol(format!("Invalid error message UTF-8: {}", e)))?;
                Ok(Payload::Error(crate::protocol::message::ErrorPayload {
                    code,
                    message,
                }))
            }
            _ => Err(Error::Protocol(format!(
                "Unknown payload type: {}",
                payload_type
            ))),
        }
    }

    /// Decode message type
    fn decode_message_type(value: u8) -> Result<MessageType> {
        match value {
            0 => Ok(MessageType::Ping),
            1 => Ok(MessageType::Pong),
            2 => Ok(MessageType::Subscribe),
            3 => Ok(MessageType::Unsubscribe),
            4 => Ok(MessageType::Publish),
            5 => Ok(MessageType::Data),
            6 => Ok(MessageType::TileUpdate),
            7 => Ok(MessageType::FeatureUpdate),
            8 => Ok(MessageType::ChangeStream),
            9 => Ok(MessageType::Error),
            10 => Ok(MessageType::Ack),
            11 => Ok(MessageType::JoinRoom),
            12 => Ok(MessageType::LeaveRoom),
            13 => Ok(MessageType::Broadcast),
            14 => Ok(MessageType::SystemEvent),
            _ => Err(Error::Protocol(format!("Invalid message type: {}", value))),
        }
    }
}

/// Geospatial binary protocol optimizations
pub struct GeospatialBinaryProtocol;

impl GeospatialBinaryProtocol {
    /// Encode coordinates with variable-length encoding
    pub fn encode_coordinates(coords: &[f64]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(coords.len() * 8);
        for &coord in coords {
            buf.write_f64::<BigEndian>(coord).ok();
        }
        buf
    }

    /// Decode coordinates
    pub fn decode_coordinates(data: &[u8]) -> Result<Vec<f64>> {
        let mut cursor = Cursor::new(data);
        let mut coords = Vec::new();

        while cursor.position() < data.len() as u64 {
            let coord = cursor
                .read_f64::<BigEndian>()
                .map_err(|e| Error::Protocol(format!("Failed to read coordinate: {}", e)))?;
            coords.push(coord);
        }

        Ok(coords)
    }

    /// Encode tile coordinates (z, x, y) efficiently
    pub fn encode_tile_coords(z: u8, x: u32, y: u32) -> [u8; 9] {
        let mut buf = [0u8; 9];
        buf[0] = z;
        buf[1..5].copy_from_slice(&x.to_be_bytes());
        buf[5..9].copy_from_slice(&y.to_be_bytes());
        buf
    }

    /// Decode tile coordinates
    pub fn decode_tile_coords(data: &[u8; 9]) -> (u8, u32, u32) {
        let z = data[0];
        let x = u32::from_be_bytes([data[1], data[2], data[3], data[4]]);
        let y = u32::from_be_bytes([data[5], data[6], data[7], data[8]]);
        (z, x, y)
    }
}

/// Binary message wrapper
pub struct BinaryMessage {
    data: Bytes,
}

impl BinaryMessage {
    /// Create a new binary message
    pub fn new(data: Bytes) -> Self {
        Self { data }
    }

    /// Get message data
    pub fn data(&self) -> &Bytes {
        &self.data
    }

    /// Convert to message
    pub fn to_message(&self) -> Result<Message> {
        BinaryCodec::decode(&self.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_codec_ping() -> Result<()> {
        let msg = Message::ping();
        let encoded = BinaryCodec::encode(&msg)?;
        let decoded = BinaryCodec::decode(&encoded)?;

        assert_eq!(msg.msg_type, decoded.msg_type);
        assert_eq!(msg.id, decoded.id);
        Ok(())
    }

    #[test]
    fn test_binary_codec_text() -> Result<()> {
        let msg = Message::new(MessageType::Data, Payload::Text("Hello".to_string()));
        let encoded = BinaryCodec::encode(&msg)?;
        let decoded = BinaryCodec::decode(&encoded)?;

        assert_eq!(msg.msg_type, decoded.msg_type);
        assert!(
            matches!(decoded.payload, Payload::Text(_)),
            "Expected text payload"
        );
        if let Payload::Text(text) = &decoded.payload {
            assert_eq!(text, "Hello");
        }
        Ok(())
    }

    #[test]
    fn test_geospatial_coordinates() -> Result<()> {
        let coords = vec![1.0, 2.0, 3.0, 4.0];
        let encoded = GeospatialBinaryProtocol::encode_coordinates(&coords);
        let decoded = GeospatialBinaryProtocol::decode_coordinates(&encoded)?;

        assert_eq!(coords, decoded);
        Ok(())
    }

    #[test]
    fn test_tile_coords() {
        let (z, x, y) = (10, 512, 384);
        let encoded = GeospatialBinaryProtocol::encode_tile_coords(z, x, y);
        let decoded = GeospatialBinaryProtocol::decode_tile_coords(&encoded);

        assert_eq!((z, x, y), decoded);
    }
}
