//! WebSocket connection management

use crate::error::{Error, Result};
use crate::protocol::ProtocolCodec;
use crate::protocol::message::Message;
use futures::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;
use tokio::net::TcpStream;
use tokio::sync::{Mutex, mpsc};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use uuid::Uuid;

/// Connection ID type
pub type ConnectionId = Uuid;

/// Connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Connecting
    Connecting,
    /// Connected
    Connected,
    /// Disconnecting
    Disconnecting,
    /// Disconnected
    Disconnected,
}

/// WebSocket connection
pub struct Connection {
    /// Connection ID
    id: ConnectionId,
    /// Remote address
    remote_addr: SocketAddr,
    /// Connection state
    state: Arc<Mutex<ConnectionState>>,
    /// WebSocket stream
    ws: Arc<Mutex<WebSocketStream<TcpStream>>>,
    /// Protocol codec
    codec: Arc<ProtocolCodec>,
    /// Outgoing message queue
    tx: mpsc::UnboundedSender<Message>,
    /// Last activity timestamp
    last_activity: Arc<AtomicU64>,
    /// Connection metadata
    metadata: Arc<Mutex<ConnectionMetadata>>,
    /// Message statistics
    stats: Arc<ConnectionStatistics>,
}

/// Connection metadata
#[derive(Debug, Default, Clone)]
pub struct ConnectionMetadata {
    /// User ID (if authenticated)
    pub user_id: Option<String>,
    /// Custom tags
    pub tags: std::collections::HashMap<String, String>,
    /// Subscribed topics
    pub subscriptions: std::collections::HashSet<String>,
    /// Joined rooms
    pub rooms: std::collections::HashSet<String>,
}

/// Connection statistics
#[derive(Debug, Default)]
pub struct ConnectionStatistics {
    /// Messages sent
    pub messages_sent: AtomicU64,
    /// Messages received
    pub messages_received: AtomicU64,
    /// Bytes sent
    pub bytes_sent: AtomicU64,
    /// Bytes received
    pub bytes_received: AtomicU64,
    /// Errors encountered
    pub errors: AtomicU64,
}

impl Connection {
    /// Create a new connection
    pub fn new(
        ws: WebSocketStream<TcpStream>,
        remote_addr: SocketAddr,
        codec: ProtocolCodec,
    ) -> (Self, mpsc::UnboundedReceiver<Message>) {
        let (tx, rx) = mpsc::unbounded_channel();

        let connection = Self {
            id: Uuid::new_v4(),
            remote_addr,
            state: Arc::new(Mutex::new(ConnectionState::Connected)),
            ws: Arc::new(Mutex::new(ws)),
            codec: Arc::new(codec),
            tx,
            last_activity: Arc::new(AtomicU64::new(Self::current_timestamp())),
            metadata: Arc::new(Mutex::new(ConnectionMetadata::default())),
            stats: Arc::new(ConnectionStatistics::default()),
        };

        (connection, rx)
    }

    /// Get connection ID
    pub fn id(&self) -> ConnectionId {
        self.id
    }

    /// Get remote address
    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }

    /// Get connection state
    pub async fn state(&self) -> ConnectionState {
        *self.state.lock().await
    }

    /// Set connection state
    pub async fn set_state(&self, new_state: ConnectionState) {
        let mut state = self.state.lock().await;
        *state = new_state;
    }

    /// Send a message
    pub async fn send(&self, message: Message) -> Result<()> {
        self.tx
            .send(message)
            .map_err(|e| Error::Connection(format!("Failed to send message: {}", e)))?;
        Ok(())
    }

    /// Receive a message
    pub async fn receive(&self) -> Result<Option<Message>> {
        let mut ws = self.ws.lock().await;

        match ws.next().await {
            Some(Ok(ws_msg)) => {
                self.update_activity();
                self.stats.messages_received.fetch_add(1, Ordering::Relaxed);

                match ws_msg {
                    WsMessage::Binary(data) => {
                        let bytes = data.as_slice();
                        self.stats
                            .bytes_received
                            .fetch_add(bytes.len() as u64, Ordering::Relaxed);
                        let message = self.codec.decode(bytes)?;
                        Ok(Some(message))
                    }
                    WsMessage::Text(text) => {
                        let bytes = text.as_slice();
                        self.stats
                            .bytes_received
                            .fetch_add(bytes.len() as u64, Ordering::Relaxed);
                        let message = self.codec.decode(bytes)?;
                        Ok(Some(message))
                    }
                    WsMessage::Ping(data) => {
                        // Respond with pong
                        ws.send(WsMessage::Pong(data)).await?;
                        Ok(None)
                    }
                    WsMessage::Pong(_) => {
                        // Update activity on pong
                        Ok(None)
                    }
                    WsMessage::Close(_) => {
                        self.set_state(ConnectionState::Disconnecting).await;
                        Ok(None)
                    }
                    _ => Ok(None),
                }
            }
            Some(Err(e)) => {
                self.stats.errors.fetch_add(1, Ordering::Relaxed);
                Err(Error::WebSocket(e.to_string()))
            }
            None => {
                self.set_state(ConnectionState::Disconnected).await;
                Ok(None)
            }
        }
    }

    /// Process outgoing messages
    pub async fn process_outgoing(&self, mut rx: mpsc::UnboundedReceiver<Message>) -> Result<()> {
        while let Some(message) = rx.recv().await {
            if let Err(e) = self.send_message(message).await {
                tracing::error!("Failed to send message: {}", e);
                self.stats.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
        Ok(())
    }

    /// Send a message directly to the WebSocket
    async fn send_message(&self, message: Message) -> Result<()> {
        let encoded = self.codec.encode(&message)?;
        self.stats
            .bytes_sent
            .fetch_add(encoded.len() as u64, Ordering::Relaxed);
        self.stats.messages_sent.fetch_add(1, Ordering::Relaxed);

        let mut ws = self.ws.lock().await;
        ws.send(WsMessage::Binary(encoded.to_vec().into())).await?;

        self.update_activity();
        Ok(())
    }

    /// Send a ping
    pub async fn ping(&self) -> Result<()> {
        let mut ws = self.ws.lock().await;
        ws.send(WsMessage::Ping(Vec::new().into())).await?;
        self.update_activity();
        Ok(())
    }

    /// Close the connection
    pub async fn close(&self) -> Result<()> {
        self.set_state(ConnectionState::Disconnecting).await;
        let mut ws = self.ws.lock().await;
        ws.close(None).await?;
        self.set_state(ConnectionState::Disconnected).await;
        Ok(())
    }

    /// Get metadata
    pub async fn metadata(&self) -> ConnectionMetadata {
        self.metadata.lock().await.clone()
    }

    /// Update metadata
    pub async fn update_metadata<F>(&self, f: F)
    where
        F: FnOnce(&mut ConnectionMetadata),
    {
        let mut metadata = self.metadata.lock().await;
        f(&mut metadata);
    }

    /// Get last activity timestamp
    pub fn last_activity(&self) -> u64 {
        self.last_activity.load(Ordering::Relaxed)
    }

    /// Update last activity
    fn update_activity(&self) {
        self.last_activity
            .store(Self::current_timestamp(), Ordering::Relaxed);
    }

    /// Get current timestamp in seconds
    fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Check if connection is idle
    pub fn is_idle(&self, timeout_secs: u64) -> bool {
        let now = Self::current_timestamp();
        let last = self.last_activity();
        now.saturating_sub(last) > timeout_secs
    }

    /// Get statistics
    pub fn stats(&self) -> ConnectionStats {
        ConnectionStats {
            messages_sent: self.stats.messages_sent.load(Ordering::Relaxed),
            messages_received: self.stats.messages_received.load(Ordering::Relaxed),
            bytes_sent: self.stats.bytes_sent.load(Ordering::Relaxed),
            bytes_received: self.stats.bytes_received.load(Ordering::Relaxed),
            errors: self.stats.errors.load(Ordering::Relaxed),
        }
    }
}

/// Connection statistics snapshot
#[derive(Debug, Clone, Default)]
pub struct ConnectionStats {
    /// Messages sent
    pub messages_sent: u64,
    /// Messages received
    pub messages_received: u64,
    /// Bytes sent
    pub bytes_sent: u64,
    /// Bytes received
    pub bytes_received: u64,
    /// Errors
    pub errors: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_id() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_connection_state() {
        assert_eq!(ConnectionState::Connected, ConnectionState::Connected);
        assert_ne!(ConnectionState::Connected, ConnectionState::Disconnected);
    }

    #[test]
    fn test_connection_metadata() {
        let mut metadata = ConnectionMetadata {
            user_id: Some("user123".to_string()),
            ..Default::default()
        };
        metadata
            .tags
            .insert("role".to_string(), "admin".to_string());

        assert_eq!(metadata.user_id, Some("user123".to_string()));
        assert_eq!(metadata.tags.get("role"), Some(&"admin".to_string()));
    }

    #[test]
    fn test_connection_stats() {
        let stats = ConnectionStatistics::default();
        stats.messages_sent.fetch_add(5, Ordering::Relaxed);
        stats.bytes_sent.fetch_add(1024, Ordering::Relaxed);

        assert_eq!(stats.messages_sent.load(Ordering::Relaxed), 5);
        assert_eq!(stats.bytes_sent.load(Ordering::Relaxed), 1024);
    }
}
