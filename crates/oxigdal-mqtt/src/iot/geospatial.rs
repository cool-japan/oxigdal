//! Geospatial IoT message types

use crate::error::Result;
use crate::iot::sensor::SensorValue;
use crate::iot::{IotMessage, SensorData, SensorType};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Geographic point (latitude, longitude, altitude)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GeoPoint {
    /// Latitude in degrees
    pub latitude: f64,
    /// Longitude in degrees
    pub longitude: f64,
    /// Altitude in meters (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub altitude: Option<f64>,
}

impl GeoPoint {
    /// Create a new geo point
    pub fn new(latitude: f64, longitude: f64) -> Self {
        Self {
            latitude,
            longitude,
            altitude: None,
        }
    }

    /// Create a new geo point with altitude
    pub fn with_altitude(latitude: f64, longitude: f64, altitude: f64) -> Self {
        Self {
            latitude,
            longitude,
            altitude: Some(altitude),
        }
    }

    /// Validate coordinates
    pub fn validate(&self) -> bool {
        self.latitude >= -90.0
            && self.latitude <= 90.0
            && self.longitude >= -180.0
            && self.longitude <= 180.0
    }

    /// Convert to GeoJSON coordinates
    pub fn to_geojson_coords(&self) -> Vec<f64> {
        if let Some(alt) = self.altitude {
            vec![self.longitude, self.latitude, alt]
        } else {
            vec![self.longitude, self.latitude]
        }
    }
}

/// Geospatial message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoMessage {
    /// Device ID
    pub device_id: String,
    /// Location
    pub location: GeoPoint,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Properties (additional data)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<serde_json::Value>,
}

impl GeoMessage {
    /// Create a new geo message
    pub fn new(device_id: impl Into<String>, location: GeoPoint) -> Self {
        Self {
            device_id: device_id.into(),
            location,
            timestamp: Utc::now(),
            properties: None,
        }
    }

    /// Add properties
    pub fn with_properties(mut self, properties: serde_json::Value) -> Self {
        self.properties = Some(properties);
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
        Ok(IotMessage::new(self.device_id.clone(), "geo", payload))
    }

    /// Convert to GeoJSON Feature
    #[cfg(feature = "geospatial")]
    pub fn to_geojson_feature(&self) -> geojson::Feature {
        use geojson::{Feature, Geometry, Value};

        let geometry = Geometry::new(Value::Point(self.location.to_geojson_coords()));

        let mut properties = serde_json::Map::new();
        properties.insert(
            "device_id".to_string(),
            serde_json::Value::String(self.device_id.clone()),
        );
        properties.insert(
            "timestamp".to_string(),
            serde_json::Value::String(self.timestamp.to_rfc3339()),
        );

        if let Some(ref props) = self.properties {
            if let Some(obj) = props.as_object() {
                properties.extend(obj.clone());
            }
        }

        Feature {
            bbox: None,
            geometry: Some(geometry),
            id: None,
            properties: Some(properties),
            foreign_members: None,
        }
    }
}

/// Geospatial sensor data (sensor reading with location)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoSensorData {
    /// Base sensor data
    pub sensor_data: SensorData,
    /// Location
    pub location: GeoPoint,
}

impl GeoSensorData {
    /// Create new geo sensor data
    pub fn new(sensor_data: SensorData, location: GeoPoint) -> Self {
        Self {
            sensor_data,
            location,
        }
    }

    /// Create from components
    pub fn from_components(
        device_id: impl Into<String>,
        sensor_type: SensorType,
        value: impl Into<SensorValue>,
        location: GeoPoint,
    ) -> Self {
        let sensor_data = SensorData::new(device_id, sensor_type, value.into());
        Self::new(sensor_data, location)
    }

    /// Convert to IoT message
    pub fn to_iot_message(&self) -> Result<IotMessage> {
        let payload = serde_json::to_value(self)?;
        Ok(IotMessage::new(
            self.sensor_data.device_id.clone(),
            "geo_sensor",
            payload,
        ))
    }

    /// Convert to GeoJSON Feature
    #[cfg(feature = "geospatial")]
    pub fn to_geojson_feature(&self) -> geojson::Feature {
        use geojson::{Feature, Geometry, Value};

        let geometry = Geometry::new(Value::Point(self.location.to_geojson_coords()));

        let mut properties = serde_json::Map::new();
        properties.insert(
            "device_id".to_string(),
            serde_json::Value::String(self.sensor_data.device_id.clone()),
        );
        if let Ok(sensor_type_val) = serde_json::to_value(self.sensor_data.sensor_type) {
            properties.insert("sensor_type".to_string(), sensor_type_val);
        }
        if let Ok(value_val) = serde_json::to_value(&self.sensor_data.value) {
            properties.insert("value".to_string(), value_val);
        }
        properties.insert(
            "unit".to_string(),
            serde_json::Value::String(self.sensor_data.unit.clone()),
        );
        properties.insert(
            "timestamp".to_string(),
            serde_json::Value::String(self.sensor_data.timestamp.to_rfc3339()),
        );

        Feature {
            bbox: None,
            geometry: Some(geometry),
            id: None,
            properties: Some(properties),
            foreign_members: None,
        }
    }
}

/// Track point (location with velocity)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct TrackPoint {
    /// Location
    pub location: GeoPoint,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Speed in m/s (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    /// Heading in degrees (0-360, optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading: Option<f64>,
    /// Accuracy in meters (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accuracy: Option<f64>,
}

// Public API for creating track points with velocity data
#[allow(dead_code)]
impl TrackPoint {
    /// Create a new track point
    pub fn new(location: GeoPoint) -> Self {
        Self {
            location,
            timestamp: Utc::now(),
            speed: None,
            heading: None,
            accuracy: None,
        }
    }

    /// Set speed
    pub fn with_speed(mut self, speed: f64) -> Self {
        self.speed = Some(speed);
        self
    }

    /// Set heading
    pub fn with_heading(mut self, heading: f64) -> Self {
        self.heading = Some(heading);
        self
    }

    /// Set accuracy
    pub fn with_accuracy(mut self, accuracy: f64) -> Self {
        self.accuracy = Some(accuracy);
        self
    }
}

/// Movement track
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct MovementTrack {
    /// Device ID
    pub device_id: String,
    /// Track points
    pub points: Vec<TrackPoint>,
}

// Public API for movement track management
#[allow(dead_code)]
impl MovementTrack {
    /// Create a new movement track
    pub fn new(device_id: impl Into<String>) -> Self {
        Self {
            device_id: device_id.into(),
            points: Vec::new(),
        }
    }

    /// Add a point
    pub fn add_point(mut self, point: TrackPoint) -> Self {
        self.points.push(point);
        self
    }

    /// Convert to IoT message
    pub fn to_iot_message(&self) -> Result<IotMessage> {
        let payload = serde_json::to_value(self)?;
        Ok(IotMessage::new(self.device_id.clone(), "track", payload))
    }

    /// Convert to GeoJSON LineString
    #[cfg(feature = "geospatial")]
    pub fn to_geojson_feature(&self) -> geojson::Feature {
        use geojson::{Feature, Geometry, Value};

        let coords: Vec<Vec<f64>> = self
            .points
            .iter()
            .map(|p| p.location.to_geojson_coords())
            .collect();

        let geometry = Geometry::new(Value::LineString(coords));

        let mut properties = serde_json::Map::new();
        properties.insert(
            "device_id".to_string(),
            serde_json::Value::String(self.device_id.clone()),
        );
        properties.insert(
            "point_count".to_string(),
            serde_json::Value::Number(self.points.len().into()),
        );

        Feature {
            bbox: None,
            geometry: Some(geometry),
            id: None,
            properties: Some(properties),
            foreign_members: None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_geo_point_creation() {
        let point = GeoPoint::new(51.5074, -0.1278);
        assert_eq!(point.latitude, 51.5074);
        assert_eq!(point.longitude, -0.1278);
        assert!(point.altitude.is_none());
        assert!(point.validate());
    }

    #[test]
    fn test_geo_point_with_altitude() {
        let point = GeoPoint::with_altitude(51.5074, -0.1278, 11.0);
        assert_eq!(point.altitude, Some(11.0));
        assert_eq!(point.to_geojson_coords(), vec![-0.1278, 51.5074, 11.0]);
    }

    #[test]
    fn test_geo_point_validation() {
        assert!(GeoPoint::new(0.0, 0.0).validate());
        assert!(GeoPoint::new(90.0, 180.0).validate());
        assert!(GeoPoint::new(-90.0, -180.0).validate());
        assert!(!GeoPoint::new(91.0, 0.0).validate());
        assert!(!GeoPoint::new(0.0, 181.0).validate());
    }

    #[test]
    fn test_geo_message_creation() {
        let location = GeoPoint::new(51.5074, -0.1278);
        let msg = GeoMessage::new("device-001", location)
            .with_properties(serde_json::json!({"name": "London"}));

        assert_eq!(msg.device_id, "device-001");
        assert_eq!(msg.location.latitude, 51.5074);
        assert!(msg.properties.is_some());
    }

    #[test]
    fn test_track_point() {
        let location = GeoPoint::new(51.5074, -0.1278);
        let track_point = TrackPoint::new(location)
            .with_speed(5.5)
            .with_heading(90.0)
            .with_accuracy(10.0);

        assert_eq!(track_point.speed, Some(5.5));
        assert_eq!(track_point.heading, Some(90.0));
        assert_eq!(track_point.accuracy, Some(10.0));
    }

    #[test]
    fn test_movement_track() {
        let p1 = TrackPoint::new(GeoPoint::new(51.5074, -0.1278));
        let p2 = TrackPoint::new(GeoPoint::new(51.5084, -0.1268));

        let track = MovementTrack::new("device-001").add_point(p1).add_point(p2);

        assert_eq!(track.points.len(), 2);
    }

    #[test]
    fn test_geo_sensor_data() {
        let sensor_data = SensorData::new("sensor-001", SensorType::Temperature, 25.5.into());
        let location = GeoPoint::new(51.5074, -0.1278);
        let geo_sensor = GeoSensorData::new(sensor_data, location);

        assert_eq!(geo_sensor.sensor_data.device_id, "sensor-001");
        assert_eq!(geo_sensor.location.latitude, 51.5074);
    }
}
