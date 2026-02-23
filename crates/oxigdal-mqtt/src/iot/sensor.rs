//! Sensor data types and handling

use crate::error::Result;
use crate::iot::IotMessage;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Sensor type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SensorType {
    /// Temperature sensor
    Temperature,
    /// Humidity sensor
    Humidity,
    /// Pressure sensor
    Pressure,
    /// Light sensor
    Light,
    /// Motion/PIR sensor
    Motion,
    /// Air quality sensor
    AirQuality,
    /// Gas sensor
    Gas,
    /// Proximity sensor
    Proximity,
    /// Accelerometer
    Accelerometer,
    /// Gyroscope
    Gyroscope,
    /// Magnetometer
    Magnetometer,
    /// GPS
    Gps,
    /// Custom sensor
    Custom,
}

impl SensorType {
    /// Get default unit for sensor type
    pub fn default_unit(&self) -> &'static str {
        match self {
            Self::Temperature => "celsius",
            Self::Humidity => "percent",
            Self::Pressure => "hPa",
            Self::Light => "lux",
            Self::Motion => "boolean",
            Self::AirQuality => "ppm",
            Self::Gas => "ppm",
            Self::Proximity => "cm",
            Self::Accelerometer => "m/s²",
            Self::Gyroscope => "deg/s",
            Self::Magnetometer => "µT",
            Self::Gps => "degrees",
            Self::Custom => "units",
        }
    }
}

/// Sensor data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorData {
    /// Device ID
    pub device_id: String,
    /// Sensor type
    pub sensor_type: SensorType,
    /// Sensor value
    pub value: SensorValue,
    /// Unit of measurement
    pub unit: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Quality indicator (0.0-1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<f64>,
}

impl SensorData {
    /// Create new sensor data
    pub fn new(device_id: impl Into<String>, sensor_type: SensorType, value: SensorValue) -> Self {
        Self {
            device_id: device_id.into(),
            sensor_type,
            value,
            unit: sensor_type.default_unit().to_string(),
            timestamp: Utc::now(),
            quality: None,
        }
    }

    /// Set unit
    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = unit.into();
        self
    }

    /// Set quality
    pub fn with_quality(mut self, quality: f64) -> Self {
        self.quality = Some(quality.clamp(0.0, 1.0));
        self
    }

    /// Set timestamp
    pub fn with_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Convert to IoT message
    pub fn to_iot_message(&self) -> Result<IotMessage> {
        let payload = serde_json::to_value(self)?;
        Ok(IotMessage::new(self.device_id.clone(), "sensor", payload))
    }
}

/// Sensor value (can be scalar, vector, or boolean)
///
/// Custom `Deserialize` implementation is used instead of relying on the derived
/// `#[serde(untagged)]` deserializer to handle `serde_json/arbitrary_precision`
/// correctly when activated by workspace dependencies (e.g. `bigdecimal`). With
/// `arbitrary_precision`, numeric values are stored internally as `Content::Map`
/// rather than `Content::F64`/`Content::I64`, which breaks the generated untagged
/// deserialization code. Deserializing first into `serde_json::Value` sidesteps the
/// issue because `serde_json` handles its own `arbitrary_precision` feature correctly.
///
/// `#[serde(untagged)]` is kept for the `Serialize` derive so that scalar values are
/// written as plain numbers (`42.5`) rather than `{"Scalar":42.5}`.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum SensorValue {
    /// Boolean value
    Boolean(bool),
    /// Vector value (x, y, z)
    Vector([f64; 3]),
    /// Scalar value
    Scalar(f64),
}

impl<'de> serde::Deserialize<'de> for SensorValue {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;
        let value = serde_json::Value::deserialize(deserializer)?;
        match &value {
            serde_json::Value::Bool(b) => Ok(SensorValue::Boolean(*b)),
            serde_json::Value::Array(arr) if arr.len() == 3 => {
                let x = arr[0]
                    .as_f64()
                    .ok_or_else(|| D::Error::custom("expected f64 for vector x"))?;
                let y = arr[1]
                    .as_f64()
                    .ok_or_else(|| D::Error::custom("expected f64 for vector y"))?;
                let z = arr[2]
                    .as_f64()
                    .ok_or_else(|| D::Error::custom("expected f64 for vector z"))?;
                Ok(SensorValue::Vector([x, y, z]))
            }
            _ => {
                if let Some(f) = value.as_f64() {
                    Ok(SensorValue::Scalar(f))
                } else {
                    Err(D::Error::custom(
                        "data did not match any variant of untagged enum SensorValue",
                    ))
                }
            }
        }
    }
}

impl SensorValue {
    /// Get boolean value if applicable
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    /// Get vector value if applicable
    pub fn as_vector(&self) -> Option<[f64; 3]> {
        match self {
            Self::Vector(v) => Some(*v),
            _ => None,
        }
    }

    /// Get scalar value if applicable
    pub fn as_scalar(&self) -> Option<f64> {
        match self {
            Self::Scalar(v) => Some(*v),
            _ => None,
        }
    }
}

impl From<f64> for SensorValue {
    fn from(v: f64) -> Self {
        Self::Scalar(v)
    }
}

impl From<[f64; 3]> for SensorValue {
    fn from(v: [f64; 3]) -> Self {
        Self::Vector(v)
    }
}

impl From<bool> for SensorValue {
    fn from(v: bool) -> Self {
        Self::Boolean(v)
    }
}

/// Sensor message with multiple readings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorMessage {
    /// Device ID
    pub device_id: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Sensor readings
    pub readings: Vec<SensorReading>,
}

impl SensorMessage {
    /// Create new sensor message
    pub fn new(device_id: impl Into<String>) -> Self {
        Self {
            device_id: device_id.into(),
            timestamp: Utc::now(),
            readings: Vec::new(),
        }
    }

    /// Add a reading
    pub fn add_reading(mut self, sensor_type: SensorType, value: impl Into<SensorValue>) -> Self {
        self.readings.push(SensorReading {
            sensor_type,
            value: value.into(),
            unit: sensor_type.default_unit().to_string(),
            quality: None,
        });
        self
    }

    /// Add a reading with unit
    pub fn add_reading_with_unit(
        mut self,
        sensor_type: SensorType,
        value: impl Into<SensorValue>,
        unit: impl Into<String>,
    ) -> Self {
        self.readings.push(SensorReading {
            sensor_type,
            value: value.into(),
            unit: unit.into(),
            quality: None,
        });
        self
    }

    /// Set timestamp
    pub fn with_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Convert to IoT message
    pub fn to_iot_message(&self) -> Result<IotMessage> {
        let payload = serde_json::to_value(self)?;
        Ok(IotMessage::new(self.device_id.clone(), "sensor", payload))
    }
}

/// Single sensor reading
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorReading {
    /// Sensor type
    pub sensor_type: SensorType,
    /// Value
    pub value: SensorValue,
    /// Unit
    pub unit: String,
    /// Quality (0.0-1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<f64>,
}

#[cfg(test)]
#[allow(clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_sensor_data_creation() {
        let data =
            SensorData::new("sensor-001", SensorType::Temperature, 25.5.into()).with_quality(0.95);

        assert_eq!(data.device_id, "sensor-001");
        assert_eq!(data.sensor_type, SensorType::Temperature);
        assert_eq!(data.value.as_scalar(), Some(25.5));
        assert_eq!(data.unit, "celsius");
        assert_eq!(data.quality, Some(0.95));
    }

    #[test]
    fn test_sensor_value_types() {
        let scalar = SensorValue::Scalar(42.0);
        assert_eq!(scalar.as_scalar(), Some(42.0));
        assert!(scalar.as_vector().is_none());

        let vector = SensorValue::Vector([1.0, 2.0, 3.0]);
        assert_eq!(vector.as_vector(), Some([1.0, 2.0, 3.0]));
        assert!(vector.as_scalar().is_none());

        let boolean = SensorValue::Boolean(true);
        assert_eq!(boolean.as_bool(), Some(true));
        assert!(boolean.as_scalar().is_none());
    }

    #[test]
    fn test_sensor_message() {
        let msg = SensorMessage::new("device-001")
            .add_reading(SensorType::Temperature, 25.5)
            .add_reading(SensorType::Humidity, 60.0)
            .add_reading(SensorType::Accelerometer, [0.1, 0.2, 9.8]);

        assert_eq!(msg.readings.len(), 3);
        assert_eq!(msg.readings[0].sensor_type, SensorType::Temperature);
        assert_eq!(msg.readings[1].sensor_type, SensorType::Humidity);
        assert_eq!(msg.readings[2].sensor_type, SensorType::Accelerometer);
    }

    #[test]
    fn test_sensor_type_units() {
        assert_eq!(SensorType::Temperature.default_unit(), "celsius");
        assert_eq!(SensorType::Humidity.default_unit(), "percent");
        assert_eq!(SensorType::Pressure.default_unit(), "hPa");
        assert_eq!(SensorType::Light.default_unit(), "lux");
    }

    #[test]
    fn test_sensor_data_serialization() {
        let data = SensorData::new("sensor-001", SensorType::Temperature, 25.5.into());

        // Serialize to JSON
        let json = serde_json::to_string(&data).expect("Failed to serialize SensorData to JSON");

        // Deserialize from JSON
        let deserialized: SensorData =
            serde_json::from_str(&json).expect("Failed to deserialize SensorData from JSON");

        // Verify fields
        assert_eq!(deserialized.device_id, "sensor-001");
        assert_eq!(deserialized.sensor_type, SensorType::Temperature);
        assert_eq!(deserialized.value.as_scalar(), Some(25.5));
        assert_eq!(deserialized.unit, "celsius");
    }

    #[test]
    fn test_sensor_value_serialization_roundtrip() {
        // Test Boolean variant
        let boolean = SensorValue::Boolean(true);
        let json = serde_json::to_string(&boolean).expect("Failed to serialize Boolean");
        let deserialized: SensorValue =
            serde_json::from_str(&json).expect("Failed to deserialize Boolean");
        assert_eq!(deserialized.as_bool(), Some(true));

        // Test Vector variant
        let vector = SensorValue::Vector([1.0, 2.0, 3.0]);
        let json = serde_json::to_string(&vector).expect("Failed to serialize Vector");
        let deserialized: SensorValue =
            serde_json::from_str(&json).expect("Failed to deserialize Vector");
        assert_eq!(deserialized.as_vector(), Some([1.0, 2.0, 3.0]));

        // Test Scalar variant
        let scalar = SensorValue::Scalar(42.5);
        let json = serde_json::to_string(&scalar).expect("Failed to serialize Scalar");
        let deserialized: SensorValue =
            serde_json::from_str(&json).expect("Failed to deserialize Scalar");
        assert_eq!(deserialized.as_scalar(), Some(42.5));

        // Test that boolean doesn't get confused with scalar
        let bool_json = "true";
        let result: SensorValue =
            serde_json::from_str(bool_json).expect("Failed to deserialize boolean");
        assert!(
            result.as_bool().is_some(),
            "Boolean should deserialize as Boolean, not Scalar"
        );

        // Test that array doesn't get confused with scalar
        let array_json = "[1.0, 2.0, 3.0]";
        let result: SensorValue =
            serde_json::from_str(array_json).expect("Failed to deserialize array");
        assert!(
            result.as_vector().is_some(),
            "Array should deserialize as Vector, not Scalar"
        );

        // Test that number deserializes as scalar
        let number_json = "25.5";
        let result: SensorValue =
            serde_json::from_str(number_json).expect("Failed to deserialize number");
        assert!(
            result.as_scalar().is_some(),
            "Number should deserialize as Scalar"
        );
    }
}
