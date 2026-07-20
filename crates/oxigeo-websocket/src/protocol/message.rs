//! Message types and payloads

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Message type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum MessageType {
    /// Ping message
    Ping = 0,
    /// Pong message
    Pong = 1,
    /// Subscribe request
    Subscribe = 2,
    /// Unsubscribe request
    Unsubscribe = 3,
    /// Publish message
    Publish = 4,
    /// Data message
    Data = 5,
    /// Tile update
    TileUpdate = 6,
    /// Feature update
    FeatureUpdate = 7,
    /// Change stream event
    ChangeStream = 8,
    /// Error message
    Error = 9,
    /// Acknowledgement
    Ack = 10,
    /// Join room
    JoinRoom = 11,
    /// Leave room
    LeaveRoom = 12,
    /// Broadcast message
    Broadcast = 13,
    /// System event
    SystemEvent = 14,
}

/// Message payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Payload {
    /// Empty payload
    Empty,
    /// Text payload
    Text(String),
    /// Binary payload
    Binary(Vec<u8>),
    /// JSON payload
    Json(serde_json::Value),
    /// Tile data payload
    TileData(TilePayload),
    /// Feature data payload
    FeatureData(FeaturePayload),
    /// Change event payload
    ChangeEvent(ChangePayload),
    /// Subscribe payload
    Subscribe(SubscribePayload),
    /// Room payload
    Room(RoomPayload),
    /// Error payload
    Error(ErrorPayload),
}

/// WebSocket message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Message ID
    pub id: Uuid,
    /// Message type
    pub msg_type: MessageType,
    /// Timestamp (milliseconds since epoch)
    pub timestamp: i64,
    /// Payload
    pub payload: Payload,
    /// Optional correlation ID (for request/response)
    pub correlation_id: Option<Uuid>,
}

impl Message {
    /// Create a new message
    pub fn new(msg_type: MessageType, payload: Payload) -> Self {
        Self {
            id: Uuid::new_v4(),
            msg_type,
            timestamp: chrono::Utc::now().timestamp_millis(),
            payload,
            correlation_id: None,
        }
    }

    /// Create a ping message
    pub fn ping() -> Self {
        Self::new(MessageType::Ping, Payload::Empty)
    }

    /// Create a pong message
    pub fn pong() -> Self {
        Self::new(MessageType::Pong, Payload::Empty)
    }

    /// Create a subscribe message
    pub fn subscribe(topic: String, filter: Option<serde_json::Value>) -> Self {
        Self::new(
            MessageType::Subscribe,
            Payload::Subscribe(SubscribePayload { topic, filter }),
        )
    }

    /// Create an unsubscribe message
    pub fn unsubscribe(topic: String) -> Self {
        Self::new(
            MessageType::Unsubscribe,
            Payload::Subscribe(SubscribePayload {
                topic,
                filter: None,
            }),
        )
    }

    /// Create a data message
    pub fn data(data: Bytes) -> Self {
        Self::new(MessageType::Data, Payload::Binary(data.to_vec()))
    }

    /// Create an error message
    pub fn error(code: u32, message: String) -> Self {
        Self::new(
            MessageType::Error,
            Payload::Error(ErrorPayload { code, message }),
        )
    }

    /// Create a join room message
    pub fn join_room(room: String) -> Self {
        Self::new(MessageType::JoinRoom, Payload::Room(RoomPayload { room }))
    }

    /// Create a leave room message
    pub fn leave_room(room: String) -> Self {
        Self::new(MessageType::LeaveRoom, Payload::Room(RoomPayload { room }))
    }

    /// Get message type
    pub fn message_type(&self) -> MessageType {
        self.msg_type
    }

    /// Set correlation ID
    pub fn with_correlation_id(mut self, id: Uuid) -> Self {
        self.correlation_id = Some(id);
        self
    }

    /// Check if this is a response to another message
    pub fn is_response_to(&self, message_id: &Uuid) -> bool {
        self.correlation_id.as_ref() == Some(message_id)
    }
}

/// Tile payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TilePayload {
    /// Tile Z coordinate (zoom level)
    pub z: u8,
    /// Tile X coordinate
    pub x: u32,
    /// Tile Y coordinate
    pub y: u32,
    /// Tile data
    pub data: Vec<u8>,
    /// Tile format (e.g., "png", "webp", "mvt")
    pub format: String,
    /// Optional delta encoding (if incremental update)
    pub delta: Option<Vec<u8>>,
}

/// Feature payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeaturePayload {
    /// Feature ID
    pub id: String,
    /// Layer name
    pub layer: String,
    /// GeoJSON feature
    pub feature: serde_json::Value,
    /// Change type
    pub change_type: ChangeType,
}

/// Change type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeType {
    /// Feature created
    Created,
    /// Feature updated
    Updated,
    /// Feature deleted
    Deleted,
}

/// Change event payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePayload {
    /// Change ID
    pub change_id: u64,
    /// Collection/layer name
    pub collection: String,
    /// Change type
    pub change_type: ChangeType,
    /// Document ID
    pub document_id: String,
    /// Optional change data
    pub data: Option<serde_json::Value>,
}

/// Subscribe payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribePayload {
    /// Topic to subscribe to
    pub topic: String,
    /// Optional filter
    pub filter: Option<serde_json::Value>,
}

/// Room payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomPayload {
    /// Room name
    pub room: String,
}

/// Error payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPayload {
    /// Error code
    pub code: u32,
    /// Error message
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation() {
        let msg = Message::ping();
        assert_eq!(msg.msg_type, MessageType::Ping);
        assert!(matches!(msg.payload, Payload::Empty));
    }

    #[test]
    fn test_message_correlation() {
        let request = Message::ping();
        let response = Message::pong().with_correlation_id(request.id);

        assert!(response.is_response_to(&request.id));
    }

    #[test]
    fn test_subscribe_message() {
        let msg = Message::subscribe("tiles".to_string(), None);
        assert_eq!(msg.msg_type, MessageType::Subscribe);

        assert!(
            matches!(msg.payload, Payload::Subscribe(_)),
            "Expected Subscribe payload"
        );
        if let Payload::Subscribe(sub) = &msg.payload {
            assert_eq!(sub.topic, "tiles");
        }
    }

    #[test]
    fn test_tile_payload() {
        let payload = TilePayload {
            z: 10,
            x: 512,
            y: 384,
            data: vec![1, 2, 3, 4],
            format: "png".to_string(),
            delta: None,
        };

        assert_eq!(payload.z, 10);
        assert_eq!(payload.x, 512);
        assert_eq!(payload.y, 384);
    }

    #[test]
    fn test_feature_payload() {
        let feature = serde_json::json!({
            "type": "Feature",
            "geometry": {
                "type": "Point",
                "coordinates": [0.0, 0.0]
            },
            "properties": {}
        });

        let payload = FeaturePayload {
            id: "feature1".to_string(),
            layer: "layer1".to_string(),
            feature,
            change_type: ChangeType::Created,
        };

        assert_eq!(payload.id, "feature1");
        assert_eq!(payload.change_type, ChangeType::Created);
    }
}
