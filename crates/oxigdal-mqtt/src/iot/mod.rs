//! IoT integration for MQTT

mod geospatial;
mod sensor;
mod timeseries;

pub use geospatial::{GeoMessage, GeoPoint, GeoSensorData};
pub use sensor::{SensorData, SensorMessage, SensorType};
pub use timeseries::{Aggregation, TimeSeriesMessage, TimeSeriesPoint};

use crate::error::{IotError, MqttError, Result};
use crate::publisher::Publisher;
use crate::types::{Message, QoS};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// IoT message format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IotMessage {
    /// Device ID
    pub device_id: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Message type
    pub message_type: String,
    /// Payload (JSON)
    pub payload: serde_json::Value,
    /// Metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl IotMessage {
    /// Create a new IoT message
    pub fn new(
        device_id: impl Into<String>,
        message_type: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            device_id: device_id.into(),
            timestamp: Utc::now(),
            message_type: message_type.into(),
            payload,
            metadata: None,
        }
    }

    /// Set metadata
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Set timestamp
    pub fn with_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Convert to MQTT message
    pub fn to_mqtt_message(&self, topic: impl Into<String>) -> Result<Message> {
        let payload = serde_json::to_vec(self)?;
        Ok(Message::new(topic, payload))
    }

    /// Parse from MQTT message
    pub fn from_mqtt_message(message: &Message) -> Result<Self> {
        serde_json::from_slice(&message.payload)
            .map_err(|e| MqttError::Iot(IotError::Decoding(e.to_string())))
    }
}

/// IoT publisher for device messages
pub struct IotPublisher {
    /// Base publisher
    publisher: Arc<Publisher>,
    /// Topic template (e.g., "devices/{device_id}/{message_type}")
    topic_template: String,
    /// Default QoS
    qos: QoS,
}

impl IotPublisher {
    /// Create a new IoT publisher
    pub fn new(publisher: Arc<Publisher>, topic_template: impl Into<String>) -> Self {
        Self {
            publisher,
            topic_template: topic_template.into(),
            qos: QoS::AtLeastOnce,
        }
    }

    /// Set QoS
    pub fn with_qos(mut self, qos: QoS) -> Self {
        self.qos = qos;
        self
    }

    /// Build topic from template
    fn build_topic(&self, device_id: &str, message_type: &str) -> String {
        self.topic_template
            .replace("{device_id}", device_id)
            .replace("{message_type}", message_type)
    }

    /// Publish an IoT message
    pub async fn publish(&self, message: IotMessage) -> Result<()> {
        let topic = self.build_topic(&message.device_id, &message.message_type);
        let mqtt_msg = message.to_mqtt_message(topic)?.with_qos(self.qos);
        self.publisher.publish(mqtt_msg).await
    }

    /// Publish sensor data
    pub async fn publish_sensor(&self, data: SensorData) -> Result<()> {
        let message = data.to_iot_message()?;
        self.publish(message).await
    }

    /// Publish geospatial sensor data
    pub async fn publish_geo_sensor(&self, data: GeoSensorData) -> Result<()> {
        let message = data.to_iot_message()?;
        self.publish(message).await
    }

    /// Publish time-series data
    pub async fn publish_timeseries(&self, data: TimeSeriesMessage) -> Result<()> {
        let message = data.to_iot_message()?;
        self.publish(message).await
    }

    /// Get topic template
    pub fn topic_template(&self) -> &str {
        &self.topic_template
    }
}

/// Device telemetry message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryMessage {
    /// Device ID
    pub device_id: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Metrics
    pub metrics: Vec<Metric>,
}

impl TelemetryMessage {
    /// Create new telemetry message
    pub fn new(device_id: impl Into<String>) -> Self {
        Self {
            device_id: device_id.into(),
            timestamp: Utc::now(),
            metrics: Vec::new(),
        }
    }

    /// Add a metric
    pub fn add_metric(
        mut self,
        name: impl Into<String>,
        value: f64,
        unit: impl Into<String>,
    ) -> Self {
        self.metrics.push(Metric {
            name: name.into(),
            value,
            unit: unit.into(),
        });
        self
    }

    /// Convert to IoT message
    pub fn to_iot_message(&self) -> Result<IotMessage> {
        let payload = serde_json::to_value(self)?;
        Ok(IotMessage::new(
            self.device_id.clone(),
            "telemetry",
            payload,
        ))
    }
}

/// Metric value
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metric {
    /// Metric name
    pub name: String,
    /// Metric value
    pub value: f64,
    /// Unit of measurement
    pub unit: String,
}

/// Device status message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusMessage {
    /// Device ID
    pub device_id: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Status
    pub status: DeviceStatus,
    /// Battery level (0.0-1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery_level: Option<f64>,
    /// Signal strength (-dBm)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_strength: Option<f64>,
}

impl StatusMessage {
    /// Create new status message
    pub fn new(device_id: impl Into<String>, status: DeviceStatus) -> Self {
        Self {
            device_id: device_id.into(),
            timestamp: Utc::now(),
            status,
            battery_level: None,
            signal_strength: None,
        }
    }

    /// Set battery level
    pub fn with_battery(mut self, level: f64) -> Self {
        self.battery_level = Some(level.clamp(0.0, 1.0));
        self
    }

    /// Set signal strength
    pub fn with_signal(mut self, strength: f64) -> Self {
        self.signal_strength = Some(strength);
        self
    }

    /// Convert to IoT message
    pub fn to_iot_message(&self) -> Result<IotMessage> {
        let payload = serde_json::to_value(self)?;
        Ok(IotMessage::new(self.device_id.clone(), "status", payload))
    }
}

/// Device status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceStatus {
    /// Device is online
    Online,
    /// Device is offline
    Offline,
    /// Device is in sleep mode
    Sleep,
    /// Device has an error
    Error,
    /// Device is in maintenance mode
    Maintenance,
}

#[cfg(test)]
#[allow(clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_iot_message_creation() {
        let payload = serde_json::json!({
            "temperature": 25.5,
            "humidity": 60.0
        });

        let msg = IotMessage::new("device-001", "sensor", payload.clone())
            .with_metadata(serde_json::json!({"location": "room-1"}));

        assert_eq!(msg.device_id, "device-001");
        assert_eq!(msg.message_type, "sensor");
        assert_eq!(msg.payload, payload);
        assert!(msg.metadata.is_some());
    }

    #[test]
    fn test_iot_message_serialization() {
        let payload = serde_json::json!({"value": 42});
        let msg = IotMessage::new("device-001", "test", payload);

        let json = serde_json::to_string(&msg).ok();
        assert!(json.is_some());

        let deserialized: IotMessage =
            serde_json::from_str(&json.expect("Serialization should succeed"))
                .expect("Deserialization should succeed");
        assert_eq!(deserialized.device_id, "device-001");
        assert_eq!(deserialized.message_type, "test");
    }

    #[test]
    fn test_telemetry_message() {
        let telemetry = TelemetryMessage::new("device-001")
            .add_metric("temperature", 25.5, "celsius")
            .add_metric("humidity", 60.0, "percent");

        assert_eq!(telemetry.metrics.len(), 2);
        assert_eq!(telemetry.metrics[0].name, "temperature");
        assert_eq!(telemetry.metrics[0].value, 25.5);
    }

    #[test]
    fn test_status_message() {
        let status = StatusMessage::new("device-001", DeviceStatus::Online)
            .with_battery(0.85)
            .with_signal(-50.0);

        assert_eq!(status.status, DeviceStatus::Online);
        assert_eq!(status.battery_level, Some(0.85));
        assert_eq!(status.signal_strength, Some(-50.0));
    }

    #[test]
    fn test_topic_template() {
        let template = "devices/{device_id}/{message_type}";
        let topic = template
            .replace("{device_id}", "dev-123")
            .replace("{message_type}", "telemetry");

        assert_eq!(topic, "devices/dev-123/telemetry");
    }
}
