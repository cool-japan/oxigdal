//! WebSocket client implementation.

use crate::error::{Error, Result};
use crate::protocol::{Compression, EventType, Message, MessageFormat, PROTOCOL_VERSION};
use crate::stream::{
    EventData, EventStream, FeatureData, FeatureStream, MessageStream, TileData, TileStream,
};
use futures::{SinkExt, StreamExt};
use std::ops::Range;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::protocol::Message as WsMessage,
};
use tracing::{debug, info};

/// WebSocket client configuration.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Server URL
    pub url: String,
    /// Connection timeout
    pub connect_timeout: Duration,
    /// Message timeout
    pub message_timeout: Duration,
    /// Preferred message format
    pub format: MessageFormat,
    /// Preferred compression
    pub compression: Compression,
    /// Reconnect on disconnect
    pub auto_reconnect: bool,
    /// Maximum reconnect attempts
    pub max_reconnect_attempts: usize,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            url: "ws://localhost:9001/ws".to_string(),
            connect_timeout: Duration::from_secs(10),
            message_timeout: Duration::from_secs(30),
            format: MessageFormat::MessagePack,
            compression: Compression::Zstd,
            auto_reconnect: true,
            max_reconnect_attempts: 5,
        }
    }
}

/// WebSocket client.
pub struct WebSocketClient {
    config: ClientConfig,
    socket: Option<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    /// Message sender for internal message queue (reserved for async processing)
    #[allow(dead_code)]
    message_tx: mpsc::UnboundedSender<Message>,
    message_rx: Option<mpsc::UnboundedReceiver<Message>>,
    format: MessageFormat,
    compression: Compression,
}

impl WebSocketClient {
    /// Create a new client with default configuration.
    pub fn new() -> Self {
        Self::with_config(ClientConfig::default())
    }

    /// Create a new client with custom configuration.
    pub fn with_config(config: ClientConfig) -> Self {
        let (message_tx, message_rx) = mpsc::unbounded_channel();
        let format = config.format;
        let compression = config.compression;

        Self {
            config,
            socket: None,
            message_tx,
            message_rx: Some(message_rx),
            format,
            compression,
        }
    }

    /// Connect to the WebSocket server.
    pub async fn connect(url: &str) -> Result<Self> {
        let config = ClientConfig {
            url: url.to_string(),
            ..Default::default()
        };

        let mut client = Self::with_config(config);
        client.do_connect().await?;
        client.handshake().await?;

        Ok(client)
    }

    /// Perform the actual connection.
    async fn do_connect(&mut self) -> Result<()> {
        info!("Connecting to {}", self.config.url);

        let connect_future = connect_async(&self.config.url);
        let (ws_stream, _) = timeout(self.config.connect_timeout, connect_future)
            .await
            .map_err(|_| Error::Timeout("Connection timeout".to_string()))?
            .map_err(|e| Error::Connection(e.to_string()))?;

        self.socket = Some(ws_stream);
        info!("Connected to {}", self.config.url);

        Ok(())
    }

    /// Perform protocol handshake.
    async fn handshake(&mut self) -> Result<()> {
        debug!("Performing handshake");

        let handshake_msg = Message::Handshake {
            version: PROTOCOL_VERSION,
            format: self.config.format,
            compression: self.config.compression,
        };

        self.send_message(handshake_msg).await?;

        // Wait for handshake acknowledgement
        let ack = timeout(self.config.message_timeout, self.receive_message())
            .await
            .map_err(|_| Error::Timeout("Handshake timeout".to_string()))??;

        match ack {
            Message::HandshakeAck {
                version,
                format,
                compression,
            } => {
                if version != PROTOCOL_VERSION {
                    return Err(Error::Protocol(format!(
                        "Protocol version mismatch: expected {}, got {}",
                        PROTOCOL_VERSION, version
                    )));
                }
                self.format = format;
                self.compression = compression;
                info!(
                    "Handshake complete: format={:?}, compression={:?}",
                    format, compression
                );
                Ok(())
            }
            _ => Err(Error::Protocol(
                "Expected handshake acknowledgement".to_string(),
            )),
        }
    }

    /// Send a message to the server.
    async fn send_message(&mut self, message: Message) -> Result<()> {
        let socket = self
            .socket
            .as_mut()
            .ok_or_else(|| Error::Connection("Not connected".to_string()))?;

        let data = message.encode(self.format, self.compression)?;
        socket
            .send(WsMessage::Binary(data.into()))
            .await
            .map_err(|e| Error::Send(e.to_string()))?;

        Ok(())
    }

    /// Receive a message from the server.
    async fn receive_message(&mut self) -> Result<Message> {
        let socket = self
            .socket
            .as_mut()
            .ok_or_else(|| Error::Connection("Not connected".to_string()))?;

        let msg = socket
            .next()
            .await
            .ok_or_else(|| Error::Receive("Connection closed".to_string()))?
            .map_err(|e| Error::Receive(e.to_string()))?;

        let data = match msg {
            WsMessage::Binary(payload) => payload.to_vec(),
            WsMessage::Text(text) => text.as_bytes().to_vec(),
            WsMessage::Close(_) => {
                return Err(Error::Connection("Server closed connection".to_string()));
            }
            _ => {
                return Err(Error::InvalidMessage("Unexpected message type".to_string()));
            }
        };

        Message::decode(&data, self.format, self.compression)
    }

    /// Subscribe to tile updates.
    pub async fn subscribe_tiles(
        &mut self,
        bbox: [f64; 4],
        zoom_range: Range<u8>,
    ) -> Result<String> {
        let subscription_id = uuid::Uuid::new_v4().to_string();

        let msg = Message::SubscribeTiles {
            subscription_id: subscription_id.clone(),
            bbox,
            zoom_range,
            tile_size: Some(256),
        };

        self.send_message(msg).await?;

        // Wait for acknowledgement
        let ack = timeout(self.config.message_timeout, self.receive_message())
            .await
            .map_err(|_| Error::Timeout("Subscribe timeout".to_string()))??;

        match ack {
            Message::Ack { success: true, .. } => Ok(subscription_id),
            Message::Ack { message, .. } => Err(Error::Subscription(
                message.unwrap_or_else(|| "Failed to subscribe".to_string()),
            )),
            Message::Error { message, .. } => Err(Error::Subscription(message)),
            _ => Err(Error::Protocol("Expected acknowledgement".to_string())),
        }
    }

    /// Subscribe to feature updates.
    pub async fn subscribe_features(&mut self, layer: Option<String>) -> Result<String> {
        let subscription_id = uuid::Uuid::new_v4().to_string();

        let msg = Message::SubscribeFeatures {
            subscription_id: subscription_id.clone(),
            bbox: None,
            filters: None,
            layer,
        };

        self.send_message(msg).await?;

        // Wait for acknowledgement
        let ack = timeout(self.config.message_timeout, self.receive_message())
            .await
            .map_err(|_| Error::Timeout("Subscribe timeout".to_string()))??;

        match ack {
            Message::Ack { success: true, .. } => Ok(subscription_id),
            Message::Ack { message, .. } => Err(Error::Subscription(
                message.unwrap_or_else(|| "Failed to subscribe".to_string()),
            )),
            Message::Error { message, .. } => Err(Error::Subscription(message)),
            _ => Err(Error::Protocol("Expected acknowledgement".to_string())),
        }
    }

    /// Subscribe to events.
    pub async fn subscribe_events(&mut self, event_types: Vec<EventType>) -> Result<String> {
        let subscription_id = uuid::Uuid::new_v4().to_string();

        let msg = Message::SubscribeEvents {
            subscription_id: subscription_id.clone(),
            event_types,
        };

        self.send_message(msg).await?;

        // Wait for acknowledgement
        let ack = timeout(self.config.message_timeout, self.receive_message())
            .await
            .map_err(|_| Error::Timeout("Subscribe timeout".to_string()))??;

        match ack {
            Message::Ack { success: true, .. } => Ok(subscription_id),
            Message::Ack { message, .. } => Err(Error::Subscription(
                message.unwrap_or_else(|| "Failed to subscribe".to_string()),
            )),
            Message::Error { message, .. } => Err(Error::Subscription(message)),
            _ => Err(Error::Protocol("Expected acknowledgement".to_string())),
        }
    }

    /// Unsubscribe from updates.
    pub async fn unsubscribe(&mut self, subscription_id: &str) -> Result<()> {
        let msg = Message::Unsubscribe {
            subscription_id: subscription_id.to_string(),
        };

        self.send_message(msg).await?;

        // Wait for acknowledgement
        let ack = timeout(self.config.message_timeout, self.receive_message())
            .await
            .map_err(|_| Error::Timeout("Unsubscribe timeout".to_string()))??;

        match ack {
            Message::Ack { success: true, .. } => Ok(()),
            Message::Ack { message, .. } => Err(Error::Subscription(
                message.unwrap_or_else(|| "Failed to unsubscribe".to_string()),
            )),
            Message::Error { message, .. } => Err(Error::Subscription(message)),
            _ => Err(Error::Protocol("Expected acknowledgement".to_string())),
        }
    }

    /// Get a stream of all messages.
    pub fn message_stream(&mut self) -> Option<MessageStream> {
        self.message_rx.take().map(MessageStream::new)
    }

    /// Get a stream of tile data.
    pub fn tile_stream(&mut self) -> TileStream {
        let (tx, rx) = mpsc::unbounded_channel();

        // Spawn task to filter tile messages
        let message_rx = self.message_rx.take();
        if let Some(mut message_rx) = message_rx {
            tokio::spawn(async move {
                while let Some(message) = message_rx.recv().await {
                    if let Message::TileData {
                        tile,
                        data,
                        mime_type,
                        ..
                    } = message
                    {
                        let tile_data = TileData::new(tile.0, tile.1, tile.2, data, mime_type);
                        if tx.send(tile_data).is_err() {
                            break;
                        }
                    }
                }
            });
        }

        TileStream::new(rx)
    }

    /// Get a stream of feature data.
    pub fn feature_stream(&mut self) -> FeatureStream {
        let (tx, rx) = mpsc::unbounded_channel();

        // Spawn task to filter feature messages
        let message_rx = self.message_rx.take();
        if let Some(mut message_rx) = message_rx {
            tokio::spawn(async move {
                while let Some(message) = message_rx.recv().await {
                    if let Message::FeatureData {
                        geojson,
                        change_type,
                        ..
                    } = message
                    {
                        let feature_data = FeatureData::new(geojson, change_type, None);
                        if tx.send(feature_data).is_err() {
                            break;
                        }
                    }
                }
            });
        }

        FeatureStream::new(rx)
    }

    /// Get a stream of events.
    pub fn event_stream(&mut self) -> EventStream {
        let (tx, rx) = mpsc::unbounded_channel();

        // Spawn task to filter event messages
        let message_rx = self.message_rx.take();
        if let Some(mut message_rx) = message_rx {
            tokio::spawn(async move {
                while let Some(message) = message_rx.recv().await {
                    if let Message::Event {
                        event_type,
                        payload,
                        timestamp,
                        ..
                    } = message
                    {
                        if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&timestamp) {
                            let event_data = EventData::with_timestamp(
                                event_type,
                                payload,
                                ts.with_timezone(&chrono::Utc),
                            );
                            if tx.send(event_data).is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }

        EventStream::new(rx)
    }

    /// Send a ping.
    pub async fn ping(&mut self, id: u64) -> Result<()> {
        self.send_message(Message::Ping { id }).await
    }

    /// Close the connection.
    pub async fn close(mut self) -> Result<()> {
        if let Some(mut socket) = self.socket.take() {
            socket
                .close(None)
                .await
                .map_err(|e| Error::Connection(e.to_string()))?;
        }
        Ok(())
    }
}

impl Default for WebSocketClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_config_default() {
        let config = ClientConfig::default();
        assert_eq!(config.url, "ws://localhost:9001/ws");
        assert_eq!(config.format, MessageFormat::MessagePack);
        assert_eq!(config.compression, Compression::Zstd);
        assert!(config.auto_reconnect);
    }

    #[test]
    fn test_client_creation() {
        let client = WebSocketClient::new();
        assert!(client.socket.is_none());
    }
}
