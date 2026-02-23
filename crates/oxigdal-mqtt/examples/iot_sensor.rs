//! IoT sensor data example

#[cfg(feature = "geospatial")]
use oxigdal_mqtt::client::{ClientConfig, MqttClient};
#[cfg(feature = "geospatial")]
use oxigdal_mqtt::iot::{
    GeoPoint, GeoSensorData, IotPublisher, SensorData, SensorMessage, SensorType,
};
#[cfg(feature = "geospatial")]
use oxigdal_mqtt::publisher::{Publisher, PublisherConfig};
#[cfg(feature = "geospatial")]
use oxigdal_mqtt::types::ConnectionOptions;
#[cfg(feature = "geospatial")]
use std::sync::Arc;
#[cfg(feature = "geospatial")]
use std::time::Duration;

#[cfg(feature = "geospatial")]
#[tokio::main]
async fn main() -> oxigdal_mqtt::error::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Create client
    let conn_opts = ConnectionOptions::new("localhost", 1883, "iot-sensor-example");
    let client_config = ClientConfig::new(conn_opts);
    let mut client = MqttClient::new(client_config)?;
    client.connect().await?;

    // Create publisher
    let pub_config = PublisherConfig::new();
    let publisher = Arc::new(Publisher::new(Arc::new(client), pub_config));

    // Create IoT publisher
    let iot_pub = IotPublisher::new(publisher, "devices/{device_id}/{message_type}");

    // Example 1: Simple sensor data
    tracing::info!("Publishing temperature sensor data...");
    let temp_data =
        SensorData::new("sensor-001", SensorType::Temperature, 25.5.into()).with_quality(0.95);

    iot_pub.publish_sensor(temp_data).await?;

    // Example 2: Multi-sensor message
    tracing::info!("Publishing multi-sensor message...");
    let sensor_msg = SensorMessage::new("device-001")
        .add_reading(SensorType::Temperature, 26.0)
        .add_reading(SensorType::Humidity, 60.0)
        .add_reading(SensorType::Pressure, 1013.25);

    iot_pub.publish(sensor_msg.to_iot_message()?).await?;

    // Example 3: Geospatial sensor data
    tracing::info!("Publishing geospatial sensor data...");
    let location = GeoPoint::new(51.5074, -0.1278); // London
    let geo_sensor = GeoSensorData::from_components(
        "sensor-002",
        SensorType::AirQuality,
        45.0, // PM2.5 value
        location,
    );

    iot_pub.publish_geo_sensor(geo_sensor).await?;

    // Example 4: Accelerometer data (vector)
    tracing::info!("Publishing accelerometer data...");
    let accel_data = SensorData::new(
        "sensor-003",
        SensorType::Accelerometer,
        [0.1, 0.2, 9.8].into(),
    )
    .with_unit("m/s²");

    iot_pub.publish_sensor(accel_data).await?;

    // Wait for messages to be sent
    tokio::time::sleep(Duration::from_secs(1)).await;

    tracing::info!("All sensor data published successfully!");

    Ok(())
}

#[cfg(not(feature = "geospatial"))]
fn main() {
    println!("This example requires the 'geospatial' feature to be enabled.");
    println!("Run with: cargo run --example iot_sensor --features geospatial");
}
