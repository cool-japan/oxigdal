//! MQTT client connection management

use super::{ClientConfig, ClientInner, ClientState, ConnectionEvent, MessageHandler};
use crate::error::{ConnectionError, MqttError, Result};
use crate::types::{Message, TopicFilter};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

/// MQTT client
pub struct MqttClient {
    /// Inner client state
    inner: Arc<ClientInner>,
    /// Event loop task handle
    event_loop_handle: Option<tokio::task::JoinHandle<Result<()>>>,
}

impl MqttClient {
    /// Create a new MQTT client
    pub fn new(config: ClientConfig) -> Result<Self> {
        let (inner, event_loop) = ClientInner::new(config)?;
        let inner_arc = Arc::new(inner);

        // Start event loop in background
        let state = Arc::clone(&inner_arc.state);
        let event_tx = inner_arc.event_tx.clone();
        let handlers = Arc::clone(&inner_arc.handlers);
        let inflight = Arc::clone(&inner_arc.inflight);
        let running = Arc::clone(&inner_arc.running);
        let auto_reconnect = inner_arc.config.auto_reconnect;
        let reconnect_delay = inner_arc.config.reconnect.initial_delay;

        let handle = tokio::spawn(async move {
            ClientInner::run_event_loop(
                state,
                event_tx,
                handlers,
                inflight,
                running,
                auto_reconnect,
                reconnect_delay,
                event_loop,
            )
            .await
        });

        Ok(Self {
            inner: inner_arc,
            event_loop_handle: Some(handle),
        })
    }

    /// Connect to the MQTT broker
    pub async fn connect(&mut self) -> Result<()> {
        let current_state = self.inner.get_state().await;
        if current_state == ClientState::Connected {
            return Ok(());
        }

        self.inner.update_state(ClientState::Connecting).await;
        info!("Connecting to MQTT broker...");

        // Wait for connection establishment
        let mut retries = 0;
        let max_retries = 30; // 30 seconds timeout
        while retries < max_retries {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let state = self.inner.get_state().await;
            if state == ClientState::Connected || state == ClientState::Connecting {
                // Event loop is handling connection, just wait
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                if self.inner.get_state().await == ClientState::Connected {
                    info!("Successfully connected to MQTT broker");
                    if self
                        .inner
                        .event_tx
                        .send(ConnectionEvent::Connected)
                        .is_err()
                    {
                        debug!("No event listeners connected");
                    }
                    return Ok(());
                }
            }
            retries += 1;
        }

        Err(MqttError::Connection(ConnectionError::ConnectFailed {
            broker: self.inner.config.connection.broker.clone(),
            reason: "Connection timeout".to_string(),
        }))
    }

    /// Disconnect from the MQTT broker
    pub async fn disconnect(&mut self) -> Result<()> {
        info!("Disconnecting from MQTT broker...");
        self.inner.disconnect().await?;

        // Wait for event loop to stop
        if let Some(handle) = self.event_loop_handle.take() {
            if let Err(e) = handle.await {
                warn!("Event loop task error: {}", e);
            }
        }

        Ok(())
    }

    /// Check if client is connected
    pub async fn is_connected(&self) -> bool {
        self.inner.get_state().await == ClientState::Connected
    }

    /// Get current client state
    pub async fn state(&self) -> ClientState {
        self.inner.get_state().await
    }

    /// Subscribe to a topic with a handler
    pub async fn subscribe(
        &self,
        filter: TopicFilter,
        handler: Arc<dyn MessageHandler>,
    ) -> Result<()> {
        if !self.is_connected().await {
            return Err(MqttError::Connection(ConnectionError::ConnectionLost {
                reason: "Not connected".to_string(),
            }));
        }

        self.inner.register_handler(filter.pattern.clone(), handler);
        self.inner.subscribe(filter).await
    }

    /// Subscribe to multiple topics
    pub async fn subscribe_many(
        &self,
        filters: Vec<(TopicFilter, Arc<dyn MessageHandler>)>,
    ) -> Result<()> {
        for (filter, handler) in filters {
            self.subscribe(filter, handler).await?;
        }
        Ok(())
    }

    /// Unsubscribe from a topic
    pub async fn unsubscribe(&self, topic: &str) -> Result<()> {
        if !self.is_connected().await {
            return Err(MqttError::Connection(ConnectionError::ConnectionLost {
                reason: "Not connected".to_string(),
            }));
        }

        self.inner.unsubscribe(topic).await
    }

    /// Publish a message
    pub async fn publish(&self, message: Message) -> Result<()> {
        if !self.is_connected().await {
            return Err(MqttError::Connection(ConnectionError::ConnectionLost {
                reason: "Not connected".to_string(),
            }));
        }

        self.inner.publish(message).await
    }

    /// Publish multiple messages
    pub async fn publish_many(&self, messages: Vec<Message>) -> Result<()> {
        for message in messages {
            self.publish(message).await?;
        }
        Ok(())
    }

    /// Subscribe to connection events
    pub fn subscribe_events(&self) -> broadcast::Receiver<ConnectionEvent> {
        self.inner.event_tx.subscribe()
    }

    /// Get list of active subscriptions
    pub async fn subscriptions(&self) -> Vec<String> {
        self.inner
            .subscriptions
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Get number of in-flight messages
    pub fn inflight_count(&self) -> usize {
        self.inner.inflight.len()
    }

    /// Wait for all in-flight messages to complete
    pub async fn wait_for_inflight(&self, timeout: std::time::Duration) -> Result<()> {
        let start = std::time::Instant::now();
        while !self.inner.inflight.is_empty() {
            if start.elapsed() > timeout {
                return Err(MqttError::Timeout {
                    timeout_ms: timeout.as_millis() as u64,
                });
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        Ok(())
    }

    /// Get client configuration
    pub fn config(&self) -> &ClientConfig {
        &self.inner.config
    }
}

impl Drop for MqttClient {
    fn drop(&mut self) {
        // Stop the event loop
        self.inner.stop();
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::types::{ConnectionOptions, QoS};

    #[tokio::test]
    async fn test_client_creation() {
        let conn_opts = ConnectionOptions::new("localhost", 1883, "test-client");
        let config = ClientConfig::new(conn_opts);
        let client = MqttClient::new(config);
        assert!(client.is_ok());
    }

    #[tokio::test]
    async fn test_client_state() {
        let conn_opts = ConnectionOptions::new("localhost", 1883, "test-client");
        let config = ClientConfig::new(conn_opts);
        let client = MqttClient::new(config).expect("Failed to create client");

        assert_eq!(client.state().await, ClientState::Disconnected);
        assert!(!client.is_connected().await);
    }

    #[tokio::test]
    async fn test_message_creation() {
        let msg = Message::new("test/topic", b"hello".to_vec())
            .with_qos(QoS::AtLeastOnce)
            .with_retain(true);

        assert_eq!(msg.topic, "test/topic");
        assert_eq!(msg.qos, QoS::AtLeastOnce);
        assert!(msg.retain);
    }
}
