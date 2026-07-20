//! WebSocket protocol definitions and message types.

use serde::{Deserialize, Serialize};
use std::ops::Range;

/// Protocol version for compatibility checking.
pub const PROTOCOL_VERSION: u32 = 1;

/// Message encoding format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MessageFormat {
    /// JSON text format (human-readable, larger size)
    Json,
    /// Binary MessagePack format (compact, efficient)
    #[default]
    MessagePack,
    /// Binary format with optional compression
    Binary,
}

/// Compression algorithm for messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Compression {
    /// No compression
    None,
    /// Zstandard compression
    #[default]
    Zstd,
}

/// WebSocket message types exchanged between client and server.
///
/// Custom [`serde::Deserialize`] is implemented to work around the
/// `serde_json/arbitrary_precision` issue: when that feature is active,
/// serde's internal `Content` type represents numbers as `Map`, causing
/// `[f64; 4]` arrays to fail deserialization through the normal
/// `#[serde(tag = "type")]` machinery.  The fix routes JSON through
/// `serde_json::Value` (which handles `arbitrary_precision` natively) and
/// only uses the derived tagged-enum path for non-JSON formats such as
/// MessagePack.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    /// Handshake message to negotiate protocol
    Handshake {
        /// Protocol version
        version: u32,
        /// Preferred message format
        format: MessageFormat,
        /// Preferred compression
        compression: Compression,
    },

    /// Handshake acknowledgement
    HandshakeAck {
        /// Accepted protocol version
        version: u32,
        /// Accepted message format
        format: MessageFormat,
        /// Accepted compression
        compression: Compression,
    },

    /// Subscribe to tile updates
    SubscribeTiles {
        /// Subscription ID
        subscription_id: String,
        /// Bounding box [min_x, min_y, max_x, max_y]
        bbox: [f64; 4],
        /// Zoom level range
        zoom_range: Range<u8>,
        /// Tile size (default 256)
        tile_size: Option<u32>,
    },

    /// Subscribe to feature updates
    SubscribeFeatures {
        /// Subscription ID
        subscription_id: String,
        /// Bounding box filter (optional)
        bbox: Option<[f64; 4]>,
        /// Attribute filters (key-value pairs)
        filters: Option<Vec<(String, String)>>,
        /// Layer name filter
        layer: Option<String>,
    },

    /// Subscribe to events
    SubscribeEvents {
        /// Subscription ID
        subscription_id: String,
        /// Event types to subscribe to
        event_types: Vec<EventType>,
    },

    /// Unsubscribe from updates
    Unsubscribe {
        /// Subscription ID to cancel
        subscription_id: String,
    },

    /// Tile data response
    TileData {
        /// Subscription ID
        subscription_id: String,
        /// Tile coordinates (x, y, zoom)
        tile: (u32, u32, u8),
        /// Tile data (MVT, PNG, etc.)
        data: Vec<u8>,
        /// MIME type
        mime_type: String,
    },

    /// Feature data response (GeoJSON)
    FeatureData {
        /// Subscription ID
        subscription_id: String,
        /// GeoJSON feature or feature collection
        geojson: String,
        /// Change type (added, updated, deleted)
        change_type: ChangeType,
    },

    /// Event notification
    Event {
        /// Subscription ID
        subscription_id: String,
        /// Event type
        event_type: EventType,
        /// Event payload
        payload: serde_json::Value,
        /// Event timestamp (RFC3339)
        timestamp: String,
    },

    /// Error message
    Error {
        /// Error code
        code: String,
        /// Error message
        message: String,
        /// Request ID that caused the error (if applicable)
        request_id: Option<String>,
    },

    /// Ping message for keep-alive
    Ping {
        /// Ping ID
        id: u64,
    },

    /// Pong response to ping
    Pong {
        /// Ping ID being acknowledged
        id: u64,
    },

    /// Acknowledgement message
    Ack {
        /// Request ID being acknowledged
        request_id: String,
        /// Success status
        success: bool,
        /// Optional message
        message: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Custom Deserialize for Message
// ---------------------------------------------------------------------------
//
// The derived `#[serde(tag = "type")]` implementation routes all formats
// through serde's internal `Content` type, which represents numbers as
// `Content::Map` when `serde_json/arbitrary_precision` is active.  That
// breaks `[f64; 4]` fields.
//
// Fix: for human-readable formats (JSON) we first deserialize into a
// `serde_json::Value` — which always handles `arbitrary_precision`
// correctly — and then dispatch per the `"type"` field.  For non-human-
// readable formats (MessagePack, Binary) the `arbitrary_precision` issue
// does not apply, so we use a private mirror enum with derived
// `Deserialize` instead.

impl<'de> serde::Deserialize<'de> for Message {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;

        if deserializer.is_human_readable() {
            // ----------------------------------------------------------------
            // JSON path — use serde_json::Value as an intermediate to bypass
            // the Content/arbitrary_precision mismatch.
            // ----------------------------------------------------------------
            let value = serde_json::Value::deserialize(deserializer).map_err(D::Error::custom)?;

            let type_str = value
                .get("type")
                .and_then(|t| t.as_str())
                .ok_or_else(|| D::Error::custom("missing 'type' field in Message"))?;

            match type_str {
                "handshake" => {
                    #[derive(serde::Deserialize)]
                    struct HandshakeData {
                        version: u32,
                        format: MessageFormat,
                        compression: Compression,
                    }
                    let d: HandshakeData =
                        serde_json::from_value(value).map_err(D::Error::custom)?;
                    Ok(Message::Handshake {
                        version: d.version,
                        format: d.format,
                        compression: d.compression,
                    })
                }
                "handshake_ack" => {
                    #[derive(serde::Deserialize)]
                    struct HandshakeAckData {
                        version: u32,
                        format: MessageFormat,
                        compression: Compression,
                    }
                    let d: HandshakeAckData =
                        serde_json::from_value(value).map_err(D::Error::custom)?;
                    Ok(Message::HandshakeAck {
                        version: d.version,
                        format: d.format,
                        compression: d.compression,
                    })
                }
                "subscribe_tiles" => {
                    #[derive(serde::Deserialize)]
                    struct SubscribeTilesData {
                        subscription_id: String,
                        bbox: [f64; 4],
                        zoom_range: Range<u8>,
                        tile_size: Option<u32>,
                    }
                    let d: SubscribeTilesData =
                        serde_json::from_value(value).map_err(D::Error::custom)?;
                    Ok(Message::SubscribeTiles {
                        subscription_id: d.subscription_id,
                        bbox: d.bbox,
                        zoom_range: d.zoom_range,
                        tile_size: d.tile_size,
                    })
                }
                "subscribe_features" => {
                    #[derive(serde::Deserialize)]
                    struct SubscribeFeaturesData {
                        subscription_id: String,
                        bbox: Option<[f64; 4]>,
                        filters: Option<Vec<(String, String)>>,
                        layer: Option<String>,
                    }
                    let d: SubscribeFeaturesData =
                        serde_json::from_value(value).map_err(D::Error::custom)?;
                    Ok(Message::SubscribeFeatures {
                        subscription_id: d.subscription_id,
                        bbox: d.bbox,
                        filters: d.filters,
                        layer: d.layer,
                    })
                }
                "subscribe_events" => {
                    #[derive(serde::Deserialize)]
                    struct SubscribeEventsData {
                        subscription_id: String,
                        event_types: Vec<EventType>,
                    }
                    let d: SubscribeEventsData =
                        serde_json::from_value(value).map_err(D::Error::custom)?;
                    Ok(Message::SubscribeEvents {
                        subscription_id: d.subscription_id,
                        event_types: d.event_types,
                    })
                }
                "unsubscribe" => {
                    #[derive(serde::Deserialize)]
                    struct UnsubscribeData {
                        subscription_id: String,
                    }
                    let d: UnsubscribeData =
                        serde_json::from_value(value).map_err(D::Error::custom)?;
                    Ok(Message::Unsubscribe {
                        subscription_id: d.subscription_id,
                    })
                }
                "tile_data" => {
                    #[derive(serde::Deserialize)]
                    struct TileDataData {
                        subscription_id: String,
                        tile: (u32, u32, u8),
                        data: Vec<u8>,
                        mime_type: String,
                    }
                    let d: TileDataData =
                        serde_json::from_value(value).map_err(D::Error::custom)?;
                    Ok(Message::TileData {
                        subscription_id: d.subscription_id,
                        tile: d.tile,
                        data: d.data,
                        mime_type: d.mime_type,
                    })
                }
                "feature_data" => {
                    #[derive(serde::Deserialize)]
                    struct FeatureDataData {
                        subscription_id: String,
                        geojson: String,
                        change_type: ChangeType,
                    }
                    let d: FeatureDataData =
                        serde_json::from_value(value).map_err(D::Error::custom)?;
                    Ok(Message::FeatureData {
                        subscription_id: d.subscription_id,
                        geojson: d.geojson,
                        change_type: d.change_type,
                    })
                }
                "event" => {
                    #[derive(serde::Deserialize)]
                    struct EventData {
                        subscription_id: String,
                        event_type: EventType,
                        payload: serde_json::Value,
                        timestamp: String,
                    }
                    let d: EventData = serde_json::from_value(value).map_err(D::Error::custom)?;
                    Ok(Message::Event {
                        subscription_id: d.subscription_id,
                        event_type: d.event_type,
                        payload: d.payload,
                        timestamp: d.timestamp,
                    })
                }
                "error" => {
                    #[derive(serde::Deserialize)]
                    struct ErrorData {
                        code: String,
                        message: String,
                        request_id: Option<String>,
                    }
                    let d: ErrorData = serde_json::from_value(value).map_err(D::Error::custom)?;
                    Ok(Message::Error {
                        code: d.code,
                        message: d.message,
                        request_id: d.request_id,
                    })
                }
                "ping" => {
                    #[derive(serde::Deserialize)]
                    struct PingData {
                        id: u64,
                    }
                    let d: PingData = serde_json::from_value(value).map_err(D::Error::custom)?;
                    Ok(Message::Ping { id: d.id })
                }
                "pong" => {
                    #[derive(serde::Deserialize)]
                    struct PongData {
                        id: u64,
                    }
                    let d: PongData = serde_json::from_value(value).map_err(D::Error::custom)?;
                    Ok(Message::Pong { id: d.id })
                }
                "ack" => {
                    #[derive(serde::Deserialize)]
                    struct AckData {
                        request_id: String,
                        success: bool,
                        message: Option<String>,
                    }
                    let d: AckData = serde_json::from_value(value).map_err(D::Error::custom)?;
                    Ok(Message::Ack {
                        request_id: d.request_id,
                        success: d.success,
                        message: d.message,
                    })
                }
                other => Err(D::Error::custom(format!("unknown Message type: {other}"))),
            }
        } else {
            // ----------------------------------------------------------------
            // Non-JSON path (MessagePack, Binary) — arbitrary_precision does
            // not apply here, so the normal derived tagged-enum path works.
            // ----------------------------------------------------------------
            #[derive(serde::Deserialize)]
            #[serde(tag = "type", rename_all = "snake_case")]
            enum MessageInner {
                Handshake {
                    version: u32,
                    format: MessageFormat,
                    compression: Compression,
                },
                HandshakeAck {
                    version: u32,
                    format: MessageFormat,
                    compression: Compression,
                },
                SubscribeTiles {
                    subscription_id: String,
                    bbox: [f64; 4],
                    zoom_range: Range<u8>,
                    tile_size: Option<u32>,
                },
                SubscribeFeatures {
                    subscription_id: String,
                    bbox: Option<[f64; 4]>,
                    filters: Option<Vec<(String, String)>>,
                    layer: Option<String>,
                },
                SubscribeEvents {
                    subscription_id: String,
                    event_types: Vec<EventType>,
                },
                Unsubscribe {
                    subscription_id: String,
                },
                TileData {
                    subscription_id: String,
                    tile: (u32, u32, u8),
                    data: Vec<u8>,
                    mime_type: String,
                },
                FeatureData {
                    subscription_id: String,
                    geojson: String,
                    change_type: ChangeType,
                },
                Event {
                    subscription_id: String,
                    event_type: EventType,
                    payload: serde_json::Value,
                    timestamp: String,
                },
                Error {
                    code: String,
                    message: String,
                    request_id: Option<String>,
                },
                Ping {
                    id: u64,
                },
                Pong {
                    id: u64,
                },
                Ack {
                    request_id: String,
                    success: bool,
                    message: Option<String>,
                },
            }

            let inner = MessageInner::deserialize(deserializer)?;
            Ok(match inner {
                MessageInner::Handshake {
                    version,
                    format,
                    compression,
                } => Message::Handshake {
                    version,
                    format,
                    compression,
                },
                MessageInner::HandshakeAck {
                    version,
                    format,
                    compression,
                } => Message::HandshakeAck {
                    version,
                    format,
                    compression,
                },
                MessageInner::SubscribeTiles {
                    subscription_id,
                    bbox,
                    zoom_range,
                    tile_size,
                } => Message::SubscribeTiles {
                    subscription_id,
                    bbox,
                    zoom_range,
                    tile_size,
                },
                MessageInner::SubscribeFeatures {
                    subscription_id,
                    bbox,
                    filters,
                    layer,
                } => Message::SubscribeFeatures {
                    subscription_id,
                    bbox,
                    filters,
                    layer,
                },
                MessageInner::SubscribeEvents {
                    subscription_id,
                    event_types,
                } => Message::SubscribeEvents {
                    subscription_id,
                    event_types,
                },
                MessageInner::Unsubscribe { subscription_id } => {
                    Message::Unsubscribe { subscription_id }
                }
                MessageInner::TileData {
                    subscription_id,
                    tile,
                    data,
                    mime_type,
                } => Message::TileData {
                    subscription_id,
                    tile,
                    data,
                    mime_type,
                },
                MessageInner::FeatureData {
                    subscription_id,
                    geojson,
                    change_type,
                } => Message::FeatureData {
                    subscription_id,
                    geojson,
                    change_type,
                },
                MessageInner::Event {
                    subscription_id,
                    event_type,
                    payload,
                    timestamp,
                } => Message::Event {
                    subscription_id,
                    event_type,
                    payload,
                    timestamp,
                },
                MessageInner::Error {
                    code,
                    message,
                    request_id,
                } => Message::Error {
                    code,
                    message,
                    request_id,
                },
                MessageInner::Ping { id } => Message::Ping { id },
                MessageInner::Pong { id } => Message::Pong { id },
                MessageInner::Ack {
                    request_id,
                    success,
                    message,
                } => Message::Ack {
                    request_id,
                    success,
                    message,
                },
            })
        }
    }
}

/// Change type for feature updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeType {
    /// Feature was added
    Added,
    /// Feature was updated
    Updated,
    /// Feature was deleted
    Deleted,
}

/// Event types that can be subscribed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    /// File change notification
    FileChange,
    /// Processing status update
    ProcessingStatus,
    /// Error notification
    Error,
    /// Progress update
    Progress,
    /// Custom event
    Custom,
}

/// Serialization helpers for messages.
impl Message {
    /// Serialize message to JSON.
    pub fn to_json(&self) -> crate::error::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }

    /// Deserialize message from JSON.
    pub fn from_json(s: &str) -> crate::error::Result<Self> {
        serde_json::from_str(s).map_err(Into::into)
    }

    /// Serialize message to MessagePack.
    pub fn to_msgpack(&self) -> crate::error::Result<Vec<u8>> {
        rmp_serde::to_vec(self).map_err(Into::into)
    }

    /// Deserialize message from MessagePack.
    pub fn from_msgpack(data: &[u8]) -> crate::error::Result<Self> {
        rmp_serde::from_slice(data).map_err(Into::into)
    }

    /// Compress data using zstd.
    pub fn compress(data: &[u8], level: i32) -> crate::error::Result<Vec<u8>> {
        oxiarc_zstd::encode_all(data, level)
            .map_err(|e| crate::error::Error::Compression(e.to_string()))
    }

    /// Decompress zstd data.
    pub fn decompress(data: &[u8]) -> crate::error::Result<Vec<u8>> {
        oxiarc_zstd::decode_all(data).map_err(|e| crate::error::Error::Decompression(e.to_string()))
    }

    /// Encode message with specified format and compression.
    pub fn encode(
        &self,
        format: MessageFormat,
        compression: Compression,
    ) -> crate::error::Result<Vec<u8>> {
        let data = match format {
            MessageFormat::Json => self.to_json()?.into_bytes(),
            MessageFormat::MessagePack | MessageFormat::Binary => self.to_msgpack()?,
        };

        match compression {
            Compression::None => Ok(data),
            Compression::Zstd => Self::compress(&data, 3),
        }
    }

    /// Decode message with specified format and compression.
    pub fn decode(
        data: &[u8],
        format: MessageFormat,
        compression: Compression,
    ) -> crate::error::Result<Self> {
        let decompressed = match compression {
            Compression::None => data.to_vec(),
            Compression::Zstd => Self::decompress(data)?,
        };

        match format {
            MessageFormat::Json => {
                let s = String::from_utf8(decompressed)
                    .map_err(|e| crate::error::Error::Deserialization(e.to_string()))?;
                Self::from_json(&s)
            }
            MessageFormat::MessagePack | MessageFormat::Binary => Self::from_msgpack(&decompressed),
        }
    }
}

/// Subscription filter for spatial queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpatialFilter {
    /// Bounding box [min_x, min_y, max_x, max_y]
    pub bbox: [f64; 4],
    /// Coordinate reference system (EPSG code)
    pub crs: Option<String>,
}

/// Subscription filter for temporal queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalFilter {
    /// Start time (RFC3339)
    pub start: Option<String>,
    /// End time (RFC3339)
    pub end: Option<String>,
}

/// Subscription filter combining spatial, temporal, and attribute filters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionFilter {
    /// Spatial filter
    pub spatial: Option<SpatialFilter>,
    /// Temporal filter
    pub temporal: Option<TemporalFilter>,
    /// Attribute filters (key-value pairs)
    pub attributes: Option<Vec<(String, String)>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_json_roundtrip() {
        let msg = Message::Ping { id: 42 };
        let json_str = msg.to_json().expect("Failed to serialize message to JSON");
        let decoded =
            Message::from_json(&json_str).expect("Failed to deserialize message from JSON");

        assert!(matches!(decoded, Message::Ping { id: 42 }));
    }

    #[test]
    fn test_message_msgpack_roundtrip() {
        let msg = Message::Ping { id: 42 };
        let msgpack_bytes = msg
            .to_msgpack()
            .expect("Failed to serialize message to MessagePack");
        let decoded = Message::from_msgpack(&msgpack_bytes)
            .expect("Failed to deserialize message from MessagePack");

        assert!(matches!(decoded, Message::Ping { id: 42 }));
    }

    #[test]
    fn test_compression_roundtrip() {
        let data = b"Hello, WebSocket!";
        let compressed = Message::compress(data, 3).expect("Failed to compress data");
        let decompressed = Message::decompress(&compressed).expect("Failed to decompress data");

        assert_eq!(data, decompressed.as_slice());
    }

    #[test]
    fn test_message_encode_decode() {
        let msg = Message::SubscribeTiles {
            subscription_id: "test-123".to_string(),
            bbox: [-180.0, -90.0, 180.0, 90.0],
            zoom_range: 0..14,
            tile_size: Some(256),
        };

        // Test JSON encoding
        let encoded = msg
            .encode(MessageFormat::Json, Compression::None)
            .expect("Failed to encode message as JSON");
        let decoded = Message::decode(&encoded, MessageFormat::Json, Compression::None)
            .expect("Failed to decode message from JSON");

        assert!(
            matches!(
                decoded,
                Message::SubscribeTiles {
                    subscription_id,
                    bbox,
                    zoom_range,
                    tile_size,
                } if subscription_id == "test-123"
                    && bbox == [-180.0, -90.0, 180.0, 90.0]
                    && zoom_range == (0..14)
                    && tile_size == Some(256)
            ),
            "Decoded message does not match expected values"
        );

        // Test MessagePack with compression
        let encoded = msg
            .encode(MessageFormat::MessagePack, Compression::Zstd)
            .expect("Failed to encode message as MessagePack with Zstd");
        let decoded = Message::decode(&encoded, MessageFormat::MessagePack, Compression::Zstd)
            .expect("Failed to decode message from MessagePack with Zstd");

        assert!(
            matches!(
                decoded,
                Message::SubscribeTiles {
                    subscription_id,
                    bbox,
                    zoom_range,
                    tile_size,
                } if subscription_id == "test-123"
                    && bbox == [-180.0, -90.0, 180.0, 90.0]
                    && zoom_range == (0..14)
                    && tile_size == Some(256)
            ),
            "Decoded message does not match expected values"
        );
    }
}
