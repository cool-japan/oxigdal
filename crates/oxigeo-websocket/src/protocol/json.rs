//! JSON protocol implementation

use crate::error::{Error, Result};
use crate::protocol::message::Message;
use bytes::Bytes;

/// JSON message codec
pub struct JsonCodec;

impl JsonCodec {
    /// Encode a message to JSON
    pub fn encode(message: &Message) -> Result<Bytes> {
        let json = serde_json::to_vec(message)?;
        Ok(Bytes::from(json))
    }

    /// Decode a message from JSON
    pub fn decode(data: &[u8]) -> Result<Message> {
        let message: Message = serde_json::from_slice(data)?;
        Ok(message)
    }

    /// Encode a message to pretty JSON (for debugging)
    pub fn encode_pretty(message: &Message) -> Result<Bytes> {
        let json = serde_json::to_vec_pretty(message)?;
        Ok(Bytes::from(json))
    }

    /// Validate JSON structure without full parsing
    pub fn validate(data: &[u8]) -> Result<()> {
        serde_json::from_slice::<serde_json::Value>(data)?;
        Ok(())
    }
}

/// JSON message wrapper
pub struct JsonMessage {
    data: Bytes,
}

impl JsonMessage {
    /// Create a new JSON message
    pub fn new(data: Bytes) -> Self {
        Self { data }
    }

    /// Create from string
    pub fn from_string(s: String) -> Self {
        Self {
            data: Bytes::from(s.into_bytes()),
        }
    }

    /// Get message data
    pub fn data(&self) -> &Bytes {
        &self.data
    }

    /// Convert to message
    pub fn to_message(&self) -> Result<Message> {
        JsonCodec::decode(&self.data)
    }

    /// Get as string
    pub fn as_string(&self) -> Result<String> {
        String::from_utf8(self.data.to_vec())
            .map_err(|e| Error::Protocol(format!("Invalid UTF-8: {}", e)))
    }

    /// Validate structure
    pub fn validate(&self) -> Result<()> {
        JsonCodec::validate(&self.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_codec_ping() -> Result<()> {
        let msg = Message::ping();
        let encoded = JsonCodec::encode(&msg)?;
        let decoded = JsonCodec::decode(&encoded)?;

        assert_eq!(msg.msg_type, decoded.msg_type);
        assert_eq!(msg.id, decoded.id);
        Ok(())
    }

    #[test]
    fn test_json_codec_pretty() -> Result<()> {
        let msg = Message::ping();
        let encoded = JsonCodec::encode_pretty(&msg)?;
        let decoded = JsonCodec::decode(&encoded)?;

        assert_eq!(msg.msg_type, decoded.msg_type);

        // Verify it's actually pretty-printed
        let s = String::from_utf8(encoded.to_vec()).ok();
        assert!(s.is_some());
        assert!(s.as_ref().is_some_and(|s| s.contains('\n')));
        Ok(())
    }

    #[test]
    fn test_json_message() -> Result<()> {
        let msg = Message::ping();
        let encoded = JsonCodec::encode(&msg)?;
        let json_msg = JsonMessage::new(encoded);

        let decoded = json_msg.to_message()?;
        assert_eq!(msg.msg_type, decoded.msg_type);

        json_msg.validate()?;
        Ok(())
    }

    #[test]
    fn test_json_validate_invalid() {
        let invalid = b"not valid json";
        let result = JsonCodec::validate(invalid);
        assert!(result.is_err());
    }
}
