//! MQTT client implementation

mod connection;
mod reconnect;

pub use connection::MqttClient;
pub use reconnect::{ReconnectOptions, ReconnectStrategy};

use crate::error::{ConnectionError, MqttError, ProtocolError, Result};
use crate::types::{ConnectionOptions, Message, QoS, TopicFilter};
use async_trait::async_trait;
use dashmap::DashMap;
use rumqttc::{AsyncClient, Event, EventLoop, Packet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::time::Duration;
use tokio::sync::{RwLock, broadcast};
use tracing::{debug, error, info, warn};

/// Maximum number of in-flight messages
const MAX_INFLIGHT: usize = 100;

/// Default channel capacity
const DEFAULT_CHANNEL_CAPACITY: usize = 1000;

/// Client state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientState {
    /// Disconnected
    Disconnected,
    /// Connecting
    Connecting,
    /// Connected
    Connected,
    /// Disconnecting
    Disconnecting,
}

/// Connection event
#[derive(Debug, Clone)]
pub enum ConnectionEvent {
    /// Connected to broker
    Connected,
    /// Disconnected from broker
    Disconnected {
        /// Reason for disconnection
        reason: String,
    },
    /// Connection error
    Error {
        /// Error message
        error: String,
    },
}

/// Message handler trait
#[async_trait]
pub trait MessageHandler: Send + Sync {
    /// Handle incoming message
    async fn handle_message(&self, message: Message) -> Result<()>;
}

// Implement MessageHandler for Arc<dyn MessageHandler> to allow polymorphic usage
#[async_trait]
impl MessageHandler for Arc<dyn MessageHandler> {
    async fn handle_message(&self, message: Message) -> Result<()> {
        (**self).handle_message(message).await
    }
}

/// Client configuration
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Connection options
    pub connection: ConnectionOptions,
    /// Reconnect options
    pub reconnect: ReconnectOptions,
    /// Maximum in-flight messages
    pub max_inflight: usize,
    /// Message channel capacity
    pub channel_capacity: usize,
    /// Enable automatic reconnection
    pub auto_reconnect: bool,
}

impl ClientConfig {
    /// Create new client configuration
    pub fn new(connection: ConnectionOptions) -> Self {
        Self {
            connection,
            reconnect: ReconnectOptions::default(),
            max_inflight: MAX_INFLIGHT,
            channel_capacity: DEFAULT_CHANNEL_CAPACITY,
            auto_reconnect: true,
        }
    }

    /// Set reconnect options
    pub fn with_reconnect(mut self, reconnect: ReconnectOptions) -> Self {
        self.reconnect = reconnect;
        self
    }

    /// Set maximum in-flight messages
    pub fn with_max_inflight(mut self, max_inflight: usize) -> Self {
        self.max_inflight = max_inflight;
        self
    }

    /// Set automatic reconnection
    pub fn with_auto_reconnect(mut self, auto_reconnect: bool) -> Self {
        self.auto_reconnect = auto_reconnect;
        self
    }
}

/// Internal client state
struct ClientInner {
    /// rumqttc async client
    client: AsyncClient,
    /// Current state
    state: Arc<RwLock<ClientState>>,
    /// Configuration
    config: ClientConfig,
    /// Connection event broadcaster
    event_tx: broadcast::Sender<ConnectionEvent>,
    /// Message handlers by topic
    handlers: Arc<DashMap<String, Arc<dyn MessageHandler>>>,
    /// Active subscriptions
    subscriptions: Arc<DashMap<String, QoS>>,
    /// Next packet ID
    next_packet_id: AtomicU16,
    /// Running flag
    running: Arc<AtomicBool>,
    /// In-flight message tracking
    inflight: Arc<DashMap<u16, Message>>,
}

impl ClientInner {
    /// Create new client inner
    fn new(config: ClientConfig) -> Result<(Self, EventLoop)> {
        let mqtt_opts = config.connection.to_rumqttc();
        let (client, event_loop) = AsyncClient::new(mqtt_opts, config.channel_capacity);
        let (event_tx, _) = broadcast::channel(100);

        let inner = Self {
            client,
            state: Arc::new(RwLock::new(ClientState::Disconnected)),
            config,
            event_tx,
            handlers: Arc::new(DashMap::new()),
            subscriptions: Arc::new(DashMap::new()),
            next_packet_id: AtomicU16::new(1),
            running: Arc::new(AtomicBool::new(false)),
            inflight: Arc::new(DashMap::new()),
        };

        Ok((inner, event_loop))
    }

    /// Get next packet ID
    fn next_packet_id(&self) -> u16 {
        let id = self.next_packet_id.fetch_add(1, Ordering::SeqCst);
        if id == 0 {
            self.next_packet_id.store(1, Ordering::SeqCst);
            1
        } else {
            id
        }
    }

    /// Update state
    async fn update_state(&self, new_state: ClientState) {
        let mut state = self.state.write().await;
        *state = new_state;
    }

    /// Get current state
    async fn get_state(&self) -> ClientState {
        *self.state.read().await
    }

    /// Publish connection event
    fn publish_event(&self, event: ConnectionEvent) {
        if self.event_tx.send(event).is_err() {
            debug!("No event listeners connected");
        }
    }

    /// Subscribe to a topic
    async fn subscribe(&self, filter: TopicFilter) -> Result<()> {
        filter.validate()?;

        self.client
            .subscribe(&filter.pattern, filter.qos.to_rumqttc())
            .await
            .map_err(|e| {
                MqttError::Subscription(crate::error::SubscriptionError::SubscribeFailed {
                    topic: filter.pattern.clone(),
                    reason: e.to_string(),
                })
            })?;

        self.subscriptions
            .insert(filter.pattern.clone(), filter.qos);
        info!("Subscribed to topic: {}", filter.pattern);

        Ok(())
    }

    /// Unsubscribe from a topic
    async fn unsubscribe(&self, topic: &str) -> Result<()> {
        self.client.unsubscribe(topic).await.map_err(|e| {
            MqttError::Subscription(crate::error::SubscriptionError::UnsubscribeFailed {
                topic: topic.to_string(),
                reason: e.to_string(),
            })
        })?;

        self.subscriptions.remove(topic);
        self.handlers.remove(topic);
        info!("Unsubscribed from topic: {}", topic);

        Ok(())
    }

    /// Publish a message
    async fn publish(&self, message: Message) -> Result<()> {
        // Validate topic
        if message.topic.is_empty() {
            return Err(MqttError::Protocol(ProtocolError::InvalidTopic {
                topic: message.topic,
            }));
        }

        // Check message size
        if message.size() > self.config.connection.max_packet_size {
            return Err(MqttError::Publication(
                crate::error::PublicationError::PayloadTooLarge {
                    size: message.size(),
                    max_size: self.config.connection.max_packet_size,
                },
            ));
        }

        // Check in-flight limit
        if self.inflight.len() >= self.config.max_inflight {
            warn!("In-flight message limit reached, waiting...");
            // Wait for some messages to complete
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let packet_id = if message.qos != QoS::AtMostOnce {
            let pid = self.next_packet_id();
            self.inflight.insert(pid, message.clone());
            Some(pid)
        } else {
            None
        };

        self.client
            .publish(
                &message.topic,
                message.qos.to_rumqttc(),
                message.retain,
                message.payload.clone(),
            )
            .await
            .map_err(|e| {
                if let Some(pid) = packet_id {
                    self.inflight.remove(&pid);
                }
                MqttError::Publication(crate::error::PublicationError::PublishFailed {
                    topic: message.topic.clone(),
                    reason: e.to_string(),
                })
            })?;

        debug!(
            "Published message to topic: {} (QoS: {:?}, size: {} bytes)",
            message.topic,
            message.qos,
            message.size()
        );

        Ok(())
    }

    /// Register a message handler for a topic
    fn register_handler(&self, topic: String, handler: Arc<dyn MessageHandler>) {
        self.handlers.insert(topic.clone(), handler);
        debug!("Registered handler for topic: {}", topic);
    }

    /// Handle incoming packet (static version for event loop)
    async fn handle_packet_static(
        packet: Packet,
        handlers: &DashMap<String, Arc<dyn MessageHandler>>,
        inflight: &DashMap<u16, Message>,
        state: &Arc<RwLock<ClientState>>,
        event_tx: &broadcast::Sender<ConnectionEvent>,
    ) -> Result<()> {
        match packet {
            Packet::Publish(publish) => {
                let message = Message {
                    topic: publish.topic.clone(),
                    payload: publish.payload.to_vec(),
                    qos: QoS::from_rumqttc(publish.qos),
                    retain: publish.retain,
                    dup: publish.dup,
                    packet_id: Some(publish.pkid),
                };

                debug!(
                    "Received message on topic: {} (size: {} bytes)",
                    message.topic,
                    message.size()
                );

                // Find matching handlers - collect first to avoid lifetime issues
                let handler_entries: Vec<(String, Arc<dyn MessageHandler>)> = handlers
                    .iter()
                    .map(|entry| (entry.key().clone(), Arc::clone(entry.value())))
                    .collect();

                let mut handled = false;
                for (pattern, handler) in handler_entries {
                    let filter = TopicFilter::new(pattern.clone(), QoS::AtMostOnce);
                    if filter.matches(&message.topic) {
                        if let Err(e) = handler.handle_message(message.clone()).await {
                            error!("Handler error for topic {}: {}", message.topic, e);
                        } else {
                            handled = true;
                        }
                    }
                }

                if !handled {
                    debug!("No handler found for topic: {}", message.topic);
                }
            }
            Packet::PubAck(puback) => {
                debug!("Received PUBACK for packet: {}", puback.pkid);
                inflight.remove(&puback.pkid);
            }
            Packet::PubRec(pubrec) => {
                debug!("Received PUBREC for packet: {}", pubrec.pkid);
            }
            Packet::PubComp(pubcomp) => {
                debug!("Received PUBCOMP for packet: {}", pubcomp.pkid);
                inflight.remove(&pubcomp.pkid);
            }
            Packet::SubAck(suback) => {
                debug!("Received SUBACK for packet: {}", suback.pkid);
            }
            Packet::UnsubAck(unsuback) => {
                debug!("Received UNSUBACK for packet: {}", unsuback.pkid);
            }
            Packet::PingResp => {
                debug!("Received PINGRESP");
            }
            Packet::Disconnect => {
                info!("Received DISCONNECT from broker");
                {
                    let mut st = state.write().await;
                    *st = ClientState::Disconnected;
                }
                if event_tx
                    .send(ConnectionEvent::Disconnected {
                        reason: "Broker disconnected".to_string(),
                    })
                    .is_err()
                {
                    debug!("No event listeners connected");
                }
            }
            _ => {
                debug!("Received packet: {:?}", packet);
            }
        }

        Ok(())
    }

    /// Start event loop
    #[allow(clippy::too_many_arguments)]
    async fn run_event_loop(
        state: Arc<RwLock<ClientState>>,
        event_tx: broadcast::Sender<ConnectionEvent>,
        handlers: Arc<DashMap<String, Arc<dyn MessageHandler>>>,
        inflight: Arc<DashMap<u16, Message>>,
        running: Arc<AtomicBool>,
        auto_reconnect: bool,
        reconnect_delay: Duration,
        mut event_loop: EventLoop,
    ) -> Result<()> {
        running.store(true, Ordering::SeqCst);
        info!("Starting MQTT event loop");

        while running.load(Ordering::SeqCst) {
            match tokio::time::timeout(Duration::from_secs(1), event_loop.poll()).await {
                Ok(Ok(event)) => {
                    match event {
                        Event::Incoming(packet) => {
                            if let Err(e) = Self::handle_packet_static(
                                packet, &handlers, &inflight, &state, &event_tx,
                            )
                            .await
                            {
                                error!("Error handling packet: {}", e);
                            }
                        }
                        Event::Outgoing(_outgoing) => {
                            // Outgoing packets are handled by rumqttc
                        }
                    }
                }
                Ok(Err(e)) => {
                    error!("Event loop error: {}", e);
                    if auto_reconnect {
                        warn!("Connection lost, will attempt reconnection");
                        {
                            let mut st = state.write().await;
                            *st = ClientState::Disconnected;
                        }
                        if event_tx
                            .send(ConnectionEvent::Disconnected {
                                reason: e.to_string(),
                            })
                            .is_err()
                        {
                            debug!("No event listeners connected");
                        }

                        // Wait before reconnecting
                        tokio::time::sleep(reconnect_delay).await;
                    } else {
                        return Err(MqttError::Connection(ConnectionError::ConnectionLost {
                            reason: e.to_string(),
                        }));
                    }
                }
                Err(_) => {
                    // Timeout - continue polling
                    continue;
                }
            }
        }

        info!("Event loop stopped");
        Ok(())
    }

    /// Stop event loop
    fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Disconnect from broker
    async fn disconnect(&self) -> Result<()> {
        self.update_state(ClientState::Disconnecting).await;
        info!("Disconnecting from broker");

        self.client.disconnect().await.map_err(|e| {
            MqttError::Connection(ConnectionError::ConnectionLost {
                reason: e.to_string(),
            })
        })?;

        self.stop();
        self.update_state(ClientState::Disconnected).await;
        self.publish_event(ConnectionEvent::Disconnected {
            reason: "Client disconnected".to_string(),
        });

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_client_creation() {
        let conn_opts = ConnectionOptions::new("localhost", 1883, "test-client");
        let config = ClientConfig::new(conn_opts);
        let inner = ClientInner::new(config);
        assert!(inner.is_ok());
    }

    #[tokio::test]
    async fn test_packet_id_generation() {
        let conn_opts = ConnectionOptions::new("localhost", 1883, "test-client");
        let config = ClientConfig::new(conn_opts);
        let (inner, _event_loop) = ClientInner::new(config).expect("Failed to create client");

        let id1 = inner.next_packet_id();
        let id2 = inner.next_packet_id();
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[tokio::test]
    async fn test_state_management() {
        let conn_opts = ConnectionOptions::new("localhost", 1883, "test-client");
        let config = ClientConfig::new(conn_opts);
        let (inner, _event_loop) = ClientInner::new(config).expect("Failed to create client");

        assert_eq!(inner.get_state().await, ClientState::Disconnected);

        inner.update_state(ClientState::Connected).await;
        assert_eq!(inner.get_state().await, ClientState::Connected);
    }
}
