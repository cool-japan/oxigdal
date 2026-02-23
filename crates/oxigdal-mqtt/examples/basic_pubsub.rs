//! Basic publish-subscribe example

use oxigdal_mqtt::client::{ClientConfig, MqttClient};
use oxigdal_mqtt::publisher::{Publisher, PublisherConfig};
use oxigdal_mqtt::subscriber::{Subscriber, SubscriberConfig};
use oxigdal_mqtt::types::{ConnectionOptions, Message, QoS, TopicFilter};
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> oxigdal_mqtt::error::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Create subscriber client
    let sub_conn_opts = ConnectionOptions::new("localhost", 1883, "subscriber-example");
    let sub_client_config = ClientConfig::new(sub_conn_opts);
    let mut sub_client = MqttClient::new(sub_client_config)?;
    sub_client.connect().await?;

    // Create subscriber
    let sub_config = SubscriberConfig::new();
    let subscriber = Subscriber::new(Arc::new(sub_client), sub_config);

    // Subscribe to topic
    let filter = TopicFilter::new("test/+/message", QoS::AtLeastOnce);
    subscriber
        .subscribe_callback(filter, |msg: Message| {
            tracing::info!(
                "Received message on topic '{}': {:?}",
                msg.topic,
                msg.payload_str()
            );
            Ok(())
        })
        .await?;

    // Create publisher client
    let pub_conn_opts = ConnectionOptions::new("localhost", 1883, "publisher-example");
    let pub_client_config = ClientConfig::new(pub_conn_opts);
    let mut pub_client = MqttClient::new(pub_client_config)?;
    pub_client.connect().await?;

    // Create publisher
    let pub_config = PublisherConfig::new().with_qos(QoS::AtLeastOnce);
    let publisher = Publisher::new(Arc::new(pub_client), pub_config);

    // Publish some messages
    for i in 0..10 {
        let topic = format!("test/{}/message", i);
        let payload = format!("Hello, MQTT! Message #{}", i);

        publisher.publish_simple(&topic, payload.as_bytes()).await?;

        tracing::info!("Published message to topic: {}", topic);

        // Wait a bit between messages
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // Wait for messages to be processed
    tokio::time::sleep(Duration::from_secs(2)).await;

    Ok(())
}
