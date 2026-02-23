//! Common types for MQTT operations

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Quality of Service level for MQTT messages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum QoS {
    /// At most once delivery (fire and forget)
    #[default]
    AtMostOnce = 0,
    /// At least once delivery (acknowledged)
    AtLeastOnce = 1,
    /// Exactly once delivery (assured)
    ExactlyOnce = 2,
}

impl QoS {
    /// Convert to u8
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Convert from u8
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::AtMostOnce),
            1 => Some(Self::AtLeastOnce),
            2 => Some(Self::ExactlyOnce),
            _ => None,
        }
    }

    /// Convert to rumqttc QoS
    pub fn to_rumqttc(self) -> rumqttc::QoS {
        match self {
            Self::AtMostOnce => rumqttc::QoS::AtMostOnce,
            Self::AtLeastOnce => rumqttc::QoS::AtLeastOnce,
            Self::ExactlyOnce => rumqttc::QoS::ExactlyOnce,
        }
    }

    /// Convert from rumqttc QoS
    pub fn from_rumqttc(qos: rumqttc::QoS) -> Self {
        match qos {
            rumqttc::QoS::AtMostOnce => Self::AtMostOnce,
            rumqttc::QoS::AtLeastOnce => Self::AtLeastOnce,
            rumqttc::QoS::ExactlyOnce => Self::ExactlyOnce,
        }
    }
}

/// MQTT protocol version
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MqttVersion {
    /// MQTT 3.1.1
    V311,
    /// MQTT 5.0
    #[default]
    V5,
}

impl MqttVersion {
    /// Get MQTT version ID
    pub fn as_u8(self) -> u8 {
        match self {
            Self::V311 => 4,
            Self::V5 => 5,
        }
    }
}

/// MQTT message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Topic
    pub topic: String,
    /// Payload
    pub payload: Vec<u8>,
    /// Quality of Service
    pub qos: QoS,
    /// Retain flag
    pub retain: bool,
    /// Duplicate flag
    pub dup: bool,
    /// Packet identifier (for QoS > 0)
    pub packet_id: Option<u16>,
}

impl Message {
    /// Create a new message
    pub fn new(topic: impl Into<String>, payload: impl Into<Vec<u8>>) -> Self {
        Self {
            topic: topic.into(),
            payload: payload.into(),
            qos: QoS::default(),
            retain: false,
            dup: false,
            packet_id: None,
        }
    }

    /// Set QoS level
    pub fn with_qos(mut self, qos: QoS) -> Self {
        self.qos = qos;
        self
    }

    /// Set retain flag
    pub fn with_retain(mut self, retain: bool) -> Self {
        self.retain = retain;
        self
    }

    /// Get payload as string
    pub fn payload_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.payload).ok()
    }

    /// Get payload size in bytes
    pub fn size(&self) -> usize {
        self.payload.len()
    }
}

/// Topic filter for subscriptions
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TopicFilter {
    /// Filter pattern (can include wildcards)
    pub pattern: String,
    /// Quality of Service
    pub qos: QoS,
}

impl TopicFilter {
    /// Create a new topic filter
    pub fn new(pattern: impl Into<String>, qos: QoS) -> Self {
        Self {
            pattern: pattern.into(),
            qos,
        }
    }

    /// Check if this filter matches a topic
    pub fn matches(&self, topic: &str) -> bool {
        topic_matches(&self.pattern, topic)
    }

    /// Validate the topic filter
    pub fn validate(&self) -> crate::error::Result<()> {
        validate_topic_filter(&self.pattern)
    }
}

/// Last Will and Testament message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastWill {
    /// Topic
    pub topic: String,
    /// Message payload
    pub message: Vec<u8>,
    /// QoS level
    pub qos: QoS,
    /// Retain flag
    pub retain: bool,
}

impl LastWill {
    /// Create a new last will message
    pub fn new(topic: impl Into<String>, message: impl Into<Vec<u8>>) -> Self {
        Self {
            topic: topic.into(),
            message: message.into(),
            qos: QoS::default(),
            retain: false,
        }
    }

    /// Set QoS level
    pub fn with_qos(mut self, qos: QoS) -> Self {
        self.qos = qos;
        self
    }

    /// Set retain flag
    pub fn with_retain(mut self, retain: bool) -> Self {
        self.retain = retain;
        self
    }
}

/// Connection options
#[derive(Debug, Clone)]
pub struct ConnectionOptions {
    /// Broker address
    pub broker: String,
    /// Port
    pub port: u16,
    /// Client ID
    pub client_id: String,
    /// Username
    pub username: Option<String>,
    /// Password
    pub password: Option<String>,
    /// Keep alive interval
    pub keep_alive: Duration,
    /// Clean session flag
    pub clean_session: bool,
    /// Last will message
    pub last_will: Option<LastWill>,
    /// MQTT protocol version
    pub version: MqttVersion,
    /// Maximum packet size
    pub max_packet_size: usize,
    /// Connection timeout
    pub connection_timeout: Duration,
}

impl ConnectionOptions {
    /// Create new connection options
    pub fn new(broker: impl Into<String>, port: u16, client_id: impl Into<String>) -> Self {
        Self {
            broker: broker.into(),
            port,
            client_id: client_id.into(),
            username: None,
            password: None,
            keep_alive: Duration::from_secs(60),
            clean_session: true,
            last_will: None,
            version: MqttVersion::default(),
            max_packet_size: 256 * 1024, // 256 KB
            connection_timeout: Duration::from_secs(30),
        }
    }

    /// Set credentials
    pub fn with_credentials(mut self, username: String, password: String) -> Self {
        self.username = Some(username);
        self.password = Some(password);
        self
    }

    /// Set keep alive interval
    pub fn with_keep_alive(mut self, keep_alive: Duration) -> Self {
        self.keep_alive = keep_alive;
        self
    }

    /// Set clean session flag
    pub fn with_clean_session(mut self, clean_session: bool) -> Self {
        self.clean_session = clean_session;
        self
    }

    /// Set last will message
    pub fn with_last_will(mut self, last_will: LastWill) -> Self {
        self.last_will = Some(last_will);
        self
    }

    /// Set MQTT version
    pub fn with_version(mut self, version: MqttVersion) -> Self {
        self.version = version;
        self
    }

    /// Convert to rumqttc MqttOptions
    pub fn to_rumqttc(&self) -> rumqttc::MqttOptions {
        let mut opts = rumqttc::MqttOptions::new(&self.client_id, &self.broker, self.port);

        if let Some(ref username) = self.username {
            opts.set_credentials(username, self.password.as_deref().unwrap_or(""));
        }

        opts.set_keep_alive(self.keep_alive);
        opts.set_clean_session(self.clean_session);
        opts.set_max_packet_size(self.max_packet_size, self.max_packet_size);

        // Set last will if configured
        if let Some(ref will) = self.last_will {
            let will_msg = rumqttc::LastWill::new(
                &will.topic,
                will.message.clone(),
                will.qos.to_rumqttc(),
                will.retain,
            );
            opts.set_last_will(will_msg);
        }

        opts
    }
}

impl Default for ConnectionOptions {
    fn default() -> Self {
        Self::new("localhost", 1883, generate_client_id())
    }
}

/// Generate a random client ID
fn generate_client_id() -> String {
    format!("oxigdal-mqtt-{}", uuid::Uuid::new_v4())
}

/// Check if a topic matches a filter pattern
fn topic_matches(pattern: &str, topic: &str) -> bool {
    let pattern_parts: Vec<&str> = pattern.split('/').collect();
    let topic_parts: Vec<&str> = topic.split('/').collect();

    if pattern_parts.contains(&"#") {
        // Multi-level wildcard must be last
        let hash_index = pattern_parts.iter().position(|&p| p == "#");
        if let Some(idx) = hash_index {
            if idx != pattern_parts.len() - 1 {
                return false;
            }
            // Match up to the # wildcard
            for i in 0..idx {
                if i >= topic_parts.len() {
                    return false;
                }
                if pattern_parts[i] != "+" && pattern_parts[i] != topic_parts[i] {
                    return false;
                }
            }
            return true;
        }
    }

    if pattern_parts.len() != topic_parts.len() {
        return false;
    }

    for (p, t) in pattern_parts.iter().zip(topic_parts.iter()) {
        if *p != "+" && *p != *t {
            return false;
        }
    }

    true
}

/// Validate a topic filter
fn validate_topic_filter(filter: &str) -> crate::error::Result<()> {
    use crate::error::{MqttError, ProtocolError};

    if filter.is_empty() {
        return Err(MqttError::Protocol(ProtocolError::InvalidTopic {
            topic: filter.to_string(),
        }));
    }

    let parts: Vec<&str> = filter.split('/').collect();

    for (i, part) in parts.iter().enumerate() {
        // Check for invalid multi-level wildcard
        if part.contains('#') && (*part != "#" || i != parts.len() - 1) {
            return Err(MqttError::Protocol(ProtocolError::InvalidTopic {
                topic: filter.to_string(),
            }));
        }

        // Check for invalid single-level wildcard
        if part.contains('+') && *part != "+" {
            return Err(MqttError::Protocol(ProtocolError::InvalidTopic {
                topic: filter.to_string(),
            }));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qos_conversion() {
        assert_eq!(QoS::from_u8(0), Some(QoS::AtMostOnce));
        assert_eq!(QoS::from_u8(1), Some(QoS::AtLeastOnce));
        assert_eq!(QoS::from_u8(2), Some(QoS::ExactlyOnce));
        assert_eq!(QoS::from_u8(3), None);
    }

    #[test]
    fn test_message_creation() {
        let msg = Message::new("test/topic", b"hello".to_vec())
            .with_qos(QoS::AtLeastOnce)
            .with_retain(true);

        assert_eq!(msg.topic, "test/topic");
        assert_eq!(msg.payload, b"hello");
        assert_eq!(msg.qos, QoS::AtLeastOnce);
        assert!(msg.retain);
        assert_eq!(msg.payload_str(), Some("hello"));
    }

    #[test]
    fn test_topic_matching() {
        assert!(topic_matches(
            "sensor/+/temperature",
            "sensor/1/temperature"
        ));
        assert!(topic_matches(
            "sensor/+/temperature",
            "sensor/2/temperature"
        ));
        assert!(!topic_matches("sensor/+/temperature", "sensor/1/humidity"));
        assert!(!topic_matches(
            "sensor/+/temperature",
            "sensor/1/2/temperature"
        ));

        assert!(topic_matches("sensor/#", "sensor/1/temperature"));
        assert!(topic_matches("sensor/#", "sensor/1/2/temperature"));
        assert!(!topic_matches("sensor/#", "device/1/temperature"));

        assert!(topic_matches("sensor/+/+", "sensor/1/temperature"));
        assert!(!topic_matches("sensor/+/+", "sensor/1/2/temperature"));
    }

    #[test]
    fn test_topic_filter_validation() {
        assert!(
            TopicFilter::new("sensor/+/temperature", QoS::AtMostOnce)
                .validate()
                .is_ok()
        );
        assert!(
            TopicFilter::new("sensor/#", QoS::AtMostOnce)
                .validate()
                .is_ok()
        );

        // Invalid: # not at end
        assert!(
            TopicFilter::new("sensor/#/temperature", QoS::AtMostOnce)
                .validate()
                .is_err()
        );

        // Invalid: + mixed with other characters
        assert!(
            TopicFilter::new("sensor/+abc/temperature", QoS::AtMostOnce)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn test_connection_options() {
        let opts = ConnectionOptions::new("mqtt.example.com", 1883, "client1")
            .with_credentials("user".to_string(), "pass".to_string())
            .with_keep_alive(Duration::from_secs(120))
            .with_clean_session(false);

        assert_eq!(opts.broker, "mqtt.example.com");
        assert_eq!(opts.port, 1883);
        assert_eq!(opts.client_id, "client1");
        assert_eq!(opts.username, Some("user".to_string()));
        assert_eq!(opts.keep_alive, Duration::from_secs(120));
        assert!(!opts.clean_session);
    }

    #[test]
    fn test_last_will() {
        let will = LastWill::new("status/disconnect", b"offline")
            .with_qos(QoS::AtLeastOnce)
            .with_retain(true);

        assert_eq!(will.topic, "status/disconnect");
        assert_eq!(will.message, b"offline");
        assert_eq!(will.qos, QoS::AtLeastOnce);
        assert!(will.retain);
    }
}
