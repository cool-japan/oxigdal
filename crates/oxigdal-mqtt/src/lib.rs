//! OxiGDAL MQTT - MQTT Protocol Support for IoT and Geospatial Data
//!
//! This crate provides comprehensive MQTT protocol support for OxiGDAL, enabling
//! real-time IoT sensor data integration, pub/sub messaging, and geospatial data streaming.
//!
//! # Features
//!
//! - **MQTT 3.1.1 and 5.0** protocol support
//! - **QoS levels** 0, 1, and 2 (at-most-once, at-least-once, exactly-once)
//! - **Async client** with automatic reconnection
//! - **Publisher** with batch publishing and persistence
//! - **Subscriber** with topic routing and message handlers
//! - **IoT integration** for sensor data, geospatial messages, and time-series
//! - **Pure Rust** implementation (COOLJAPAN Policy compliant)
//!
//! # Examples
//!
//! ## Basic Publisher
//!
//! ```no_run
//! use oxigdal_mqtt::client::{ClientConfig, MqttClient};
//! use oxigdal_mqtt::publisher::{Publisher, PublisherConfig};
//! use oxigdal_mqtt::types::{ConnectionOptions, Message, QoS};
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> oxigdal_mqtt::error::Result<()> {
//!     // Create connection options
//!     let conn_opts = ConnectionOptions::new("mqtt://localhost", 1883, "publisher-1");
//!
//!     // Create and connect client
//!     let client_config = ClientConfig::new(conn_opts);
//!     let mut client = MqttClient::new(client_config)?;
//!     client.connect().await?;
//!
//!     // Create publisher
//!     let pub_config = PublisherConfig::new().with_qos(QoS::AtLeastOnce);
//!     let publisher = Publisher::new(Arc::new(client), pub_config);
//!
//!     // Publish message
//!     publisher.publish_simple("sensor/temperature", b"25.5").await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Basic Subscriber
//!
//! ```no_run
//! use oxigdal_mqtt::client::{ClientConfig, MqttClient};
//! use oxigdal_mqtt::subscriber::{Subscriber, SubscriberConfig};
//! use oxigdal_mqtt::types::{ConnectionOptions, QoS, TopicFilter};
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> oxigdal_mqtt::error::Result<()> {
//!     // Create and connect client
//!     let conn_opts = ConnectionOptions::new("mqtt://localhost", 1883, "subscriber-1");
//!     let client_config = ClientConfig::new(conn_opts);
//!     let mut client = MqttClient::new(client_config)?;
//!     client.connect().await?;
//!
//!     // Create subscriber
//!     let sub_config = SubscriberConfig::new();
//!     let subscriber = Subscriber::new(Arc::new(client), sub_config);
//!
//!     // Subscribe to topic
//!     let filter = TopicFilter::new("sensor/+/temperature", QoS::AtLeastOnce);
//!     subscriber.subscribe_callback(filter, |msg| {
//!         println!("Received: {:?}", msg.payload_str());
//!         Ok(())
//!     }).await?;
//!
//!     // Keep running
//!     tokio::signal::ctrl_c().await.ok();
//!
//!     Ok(())
//! }
//! ```
//!
//! ## IoT Sensor Data
//!
//! ```no_run
//! use oxigdal_mqtt::iot::{SensorData, SensorType, IotPublisher};
//! use oxigdal_mqtt::client::{ClientConfig, MqttClient};
//! use oxigdal_mqtt::publisher::{Publisher, PublisherConfig};
//! use oxigdal_mqtt::types::ConnectionOptions;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> oxigdal_mqtt::error::Result<()> {
//!     // Setup client and publisher
//!     let conn_opts = ConnectionOptions::new("mqtt://localhost", 1883, "iot-device-1");
//!     let client_config = ClientConfig::new(conn_opts);
//!     let mut client = MqttClient::new(client_config)?;
//!     client.connect().await?;
//!
//!     let pub_config = PublisherConfig::new();
//!     let publisher = Arc::new(Publisher::new(Arc::new(client), pub_config));
//!
//!     // Create IoT publisher
//!     let iot_pub = IotPublisher::new(publisher, "devices/{device_id}/{message_type}");
//!
//!     // Publish sensor data
//!     let sensor_data = SensorData::new("sensor-001", SensorType::Temperature, 25.5.into())
//!         .with_quality(0.95);
//!
//!     iot_pub.publish_sensor(sensor_data).await?;
//!
//!     Ok(())
//! }
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]
#![warn(clippy::all)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::panic)]
#![allow(clippy::module_name_repetitions)]

#[cfg(feature = "std")]
extern crate std;

pub mod client;
pub mod error;
pub mod iot;
pub mod publisher;
pub mod subscriber;
pub mod types;

// Re-export commonly used items
pub use client::{ClientConfig, MqttClient};
pub use error::{MqttError, Result};
pub use publisher::{Publisher, PublisherConfig};
pub use subscriber::{Subscriber, SubscriberConfig};
pub use types::{Message, QoS, TopicFilter};

/// Crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Crate name
pub const NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
        assert_eq!(NAME, "oxigdal-mqtt");
    }
}
