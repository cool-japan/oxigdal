# oxigdal-mqtt

MQTT protocol support for OxiGDAL - IoT sensor data integration, pub/sub messaging, and geospatial time-series streaming.

## Features

- **MQTT Protocol Support**
  - MQTT 3.1.1 and 5.0
  - Async client with automatic reconnection
  - QoS levels 0, 1, and 2
  - Retained messages
  - Last Will and Testament (LWT)

- **Publisher**
  - Simple and batch publishing
  - Configurable QoS and retention
  - Message persistence (optional)
  - Concurrent publishing with backpressure

- **Subscriber**
  - Topic wildcards (`+` and `#`)
  - Message routing
  - Multiple handlers per topic
  - Channel-based message delivery

- **IoT Integration**
  - Sensor data types (temperature, humidity, pressure, etc.)
  - Geospatial messages with location data
  - Time-series data with aggregation
  - Device telemetry and status messages

- **Pure Rust**
  - No C/C++ dependencies (uses `rumqttc`)
  - COOLJAPAN Policy compliant
  - No unwrap() usage
  - Comprehensive error handling

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-mqtt = "0.1"
```

### Basic Publisher

```rust
use oxigdal_mqtt::client::{ClientConfig, MqttClient};
use oxigdal_mqtt::publisher::{Publisher, PublisherConfig};
use oxigdal_mqtt::types::{ConnectionOptions, QoS};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create and connect client
    let conn_opts = ConnectionOptions::new("localhost", 1883, "publisher-1");
    let client_config = ClientConfig::new(conn_opts);
    let mut client = MqttClient::new(client_config)?;
    client.connect().await?;

    // Create publisher
    let pub_config = PublisherConfig::new().with_qos(QoS::AtLeastOnce);
    let publisher = Publisher::new(Arc::new(client), pub_config);

    // Publish message
    publisher.publish_simple("sensor/temperature", b"25.5").await?;

    Ok(())
}
```

### Basic Subscriber

```rust
use oxigdal_mqtt::client::{ClientConfig, MqttClient};
use oxigdal_mqtt::subscriber::{Subscriber, SubscriberConfig};
use oxigdal_mqtt::types::{ConnectionOptions, QoS, TopicFilter};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create and connect client
    let conn_opts = ConnectionOptions::new("localhost", 1883, "subscriber-1");
    let client_config = ClientConfig::new(conn_opts);
    let mut client = MqttClient::new(client_config)?;
    client.connect().await?;

    // Create subscriber
    let sub_config = SubscriberConfig::new();
    let subscriber = Subscriber::new(Arc::new(client), sub_config);

    // Subscribe to topic
    let filter = TopicFilter::new("sensor/+/temperature", QoS::AtLeastOnce);
    subscriber.subscribe_callback(filter, |msg| {
        println!("Received: {:?}", msg.payload_str());
        Ok(())
    }).await?;

    // Keep running
    tokio::signal::ctrl_c().await?;

    Ok(())
}
```

### IoT Sensor Data

```rust
use oxigdal_mqtt::iot::{SensorData, SensorType, IotPublisher};
use oxigdal_mqtt::client::{ClientConfig, MqttClient};
use oxigdal_mqtt::publisher::{Publisher, PublisherConfig};
use oxigdal_mqtt::types::ConnectionOptions;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup client and publisher
    let conn_opts = ConnectionOptions::new("localhost", 1883, "iot-device-1");
    let client_config = ClientConfig::new(conn_opts);
    let mut client = MqttClient::new(client_config)?;
    client.connect().await?;

    let pub_config = PublisherConfig::new();
    let publisher = Arc::new(Publisher::new(Arc::new(client), pub_config));

    // Create IoT publisher
    let iot_pub = IotPublisher::new(publisher, "devices/{device_id}/{message_type}");

    // Publish sensor data
    let sensor_data = SensorData::new("sensor-001", SensorType::Temperature, 25.5.into())
        .with_quality(0.95);

    iot_pub.publish_sensor(sensor_data).await?;

    Ok(())
}
```

## Features

- `default`: Includes `std` and `mqtt5`
- `std`: Standard library support
- `mqtt3`: MQTT 3.1.1 protocol
- `mqtt5`: MQTT 5.0 protocol (default)
- `persistence`: Message persistence with sled
- `tls`: TLS/SSL support
- `websocket`: WebSocket transport
- `compression`: Message compression
- `geospatial`: Geospatial message types with GeoJSON support

## Architecture

The crate is organized into several modules:

- `client`: MQTT client with connection management and auto-reconnection
- `publisher`: Message publishing with batching and persistence
- `subscriber`: Message subscription with routing and handlers
- `iot`: IoT-specific types (sensors, geospatial, time-series)
- `types`: Core types (Message, QoS, TopicFilter, etc.)
- `error`: Comprehensive error types

## Examples

See the `examples/` directory for more examples:

- `basic_pubsub.rs`: Basic publish-subscribe example
- `iot_sensor.rs`: IoT sensor data publishing
- `persistent_client.rs`: Publisher with message persistence

Run examples with:

```bash
cargo run --example basic_pubsub
cargo run --example iot_sensor --features geospatial
```

## Testing

Run tests with:

```bash
cargo test
```

Run benchmarks with:

```bash
cargo bench
```

## License

Licensed under Apache License 2.0.

## Authors

COOLJAPAN OU (Team Kitasan)
