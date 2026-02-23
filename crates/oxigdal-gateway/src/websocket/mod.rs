//! WebSocket multiplexing and connection management.

pub mod channel;
pub mod compression;
pub mod multiplexer;
pub mod router;

use crate::error::{GatewayError, Result};
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

/// WebSocket message type.
#[derive(Debug, Clone)]
pub enum WsMessage {
    /// Text message
    Text(String),
    /// Binary message
    Binary(Vec<u8>),
    /// Ping message
    Ping(Vec<u8>),
    /// Pong message
    Pong(Vec<u8>),
    /// Close message
    Close,
}

/// WebSocket connection ID.
pub type ConnectionId = String;

/// WebSocket connection information.
#[derive(Debug, Clone)]
pub struct Connection {
    /// Connection ID
    pub id: ConnectionId,
    /// User ID if authenticated
    pub user_id: Option<String>,
    /// Connection metadata
    pub metadata: std::collections::HashMap<String, String>,
    /// Connected timestamp
    pub connected_at: chrono::DateTime<chrono::Utc>,
    /// Last activity timestamp
    pub last_activity: chrono::DateTime<chrono::Utc>,
}

impl Connection {
    /// Creates a new connection.
    pub fn new(id: ConnectionId) -> Self {
        let now = chrono::Utc::now();
        Self {
            id,
            user_id: None,
            metadata: std::collections::HashMap::new(),
            connected_at: now,
            last_activity: now,
        }
    }

    /// Updates last activity timestamp.
    pub fn update_activity(&mut self) {
        self.last_activity = chrono::Utc::now();
    }
}

/// WebSocket manager.
pub struct WebSocketManager {
    connections: Arc<DashMap<ConnectionId, Connection>>,
    senders: Arc<DashMap<ConnectionId, mpsc::UnboundedSender<WsMessage>>>,
    router: Arc<router::MessageRouter>,
}

impl WebSocketManager {
    /// Creates a new WebSocket manager.
    pub fn new() -> Self {
        Self {
            connections: Arc::new(DashMap::new()),
            senders: Arc::new(DashMap::new()),
            router: Arc::new(router::MessageRouter::new()),
        }
    }

    /// Registers a new connection.
    pub fn register_connection(
        &self,
        conn: Connection,
        sender: mpsc::UnboundedSender<WsMessage>,
    ) -> Result<()> {
        let conn_id = conn.id.clone();
        self.connections.insert(conn_id.clone(), conn);
        self.senders.insert(conn_id, sender);
        Ok(())
    }

    /// Unregisters a connection.
    pub fn unregister_connection(&self, conn_id: &str) -> Result<()> {
        self.connections.remove(conn_id);
        self.senders.remove(conn_id);
        Ok(())
    }

    /// Gets a connection by ID.
    pub fn get_connection(&self, conn_id: &str) -> Option<Connection> {
        self.connections.get(conn_id).map(|c| c.clone())
    }

    /// Gets all connections.
    pub fn get_all_connections(&self) -> Vec<Connection> {
        self.connections
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Gets connections by user ID.
    pub fn get_user_connections(&self, user_id: &str) -> Vec<Connection> {
        self.connections
            .iter()
            .filter(|entry| {
                entry
                    .value()
                    .user_id
                    .as_ref()
                    .map(|uid| uid == user_id)
                    .unwrap_or(false)
            })
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Sends a message to a specific connection.
    pub fn send_to_connection(&self, conn_id: &str, message: WsMessage) -> Result<()> {
        let sender = self.senders.get(conn_id).ok_or_else(|| {
            GatewayError::WebSocketError(format!("Connection not found: {}", conn_id))
        })?;

        sender
            .send(message)
            .map_err(|e| GatewayError::WebSocketError(format!("Failed to send message: {}", e)))?;

        Ok(())
    }

    /// Broadcasts a message to all connections.
    pub fn broadcast(&self, message: WsMessage) -> Result<usize> {
        let mut count = 0;

        for entry in self.senders.iter() {
            if entry.value().send(message.clone()).is_ok() {
                count += 1;
            }
        }

        Ok(count)
    }

    /// Broadcasts a message to all connections of a user.
    pub fn broadcast_to_user(&self, user_id: &str, message: WsMessage) -> Result<usize> {
        let mut count = 0;

        for entry in self.connections.iter() {
            let conn = entry.value();
            if conn
                .user_id
                .as_ref()
                .map(|uid| uid == user_id)
                .unwrap_or(false)
            {
                if let Some(sender) = self.senders.get(&conn.id) {
                    if sender.send(message.clone()).is_ok() {
                        count += 1;
                    }
                }
            }
        }

        Ok(count)
    }

    /// Handles an incoming message.
    pub async fn handle_message(&self, conn_id: &str, message: WsMessage) -> Result<()> {
        // Update activity
        if let Some(mut conn) = self.connections.get_mut(conn_id) {
            conn.update_activity();
        }

        // Route message
        self.router.route_message(conn_id, message).await?;

        Ok(())
    }

    /// Cleans up inactive connections.
    pub fn cleanup_inactive(&self, timeout_seconds: i64) -> usize {
        let now = chrono::Utc::now();
        let mut removed = 0;

        let inactive: Vec<ConnectionId> = self
            .connections
            .iter()
            .filter(|entry| {
                let conn = entry.value();
                let elapsed = (now - conn.last_activity).num_seconds();
                elapsed > timeout_seconds
            })
            .map(|entry| entry.key().clone())
            .collect();

        for conn_id in inactive {
            if self.unregister_connection(&conn_id).is_ok() {
                removed += 1;
            }
        }

        removed
    }

    /// Gets connection count.
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Gets the message router.
    pub fn router(&self) -> &router::MessageRouter {
        &self.router
    }
}

impl Default for WebSocketManager {
    fn default() -> Self {
        Self::new()
    }
}

/// WebSocket configuration.
#[derive(Debug, Clone)]
pub struct WebSocketConfig {
    /// Maximum message size in bytes
    pub max_message_size: usize,
    /// Ping interval in seconds
    pub ping_interval: u64,
    /// Connection timeout in seconds
    pub connection_timeout: u64,
    /// Enable per-message compression
    pub enable_compression: bool,
    /// Maximum connections per user
    pub max_connections_per_user: usize,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            max_message_size: 1024 * 1024, // 1MB
            ping_interval: 30,
            connection_timeout: 300, // 5 minutes
            enable_compression: true,
            max_connections_per_user: 10,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_creation() {
        let conn = Connection::new("conn_1".to_string());
        assert_eq!(conn.id, "conn_1");
        assert!(conn.user_id.is_none());
    }

    #[test]
    fn test_connection_activity_update() {
        let mut conn = Connection::new("conn_1".to_string());
        let first_activity = conn.last_activity;

        std::thread::sleep(std::time::Duration::from_millis(10));
        conn.update_activity();

        assert!(conn.last_activity > first_activity);
    }

    #[tokio::test]
    async fn test_manager_registration() {
        let manager = WebSocketManager::new();
        let conn = Connection::new("conn_1".to_string());
        let (sender, _receiver) = mpsc::unbounded_channel();

        let result = manager.register_connection(conn, sender);
        assert!(result.is_ok());

        assert_eq!(manager.connection_count(), 1);
    }

    #[tokio::test]
    async fn test_manager_unregistration() {
        let manager = WebSocketManager::new();
        let conn = Connection::new("conn_1".to_string());
        let (sender, _receiver) = mpsc::unbounded_channel();

        let _ = manager.register_connection(conn, sender);
        assert_eq!(manager.connection_count(), 1);

        let result = manager.unregister_connection("conn_1");
        assert!(result.is_ok());
        assert_eq!(manager.connection_count(), 0);
    }

    #[tokio::test]
    async fn test_send_to_connection() {
        let manager = WebSocketManager::new();
        let conn = Connection::new("conn_1".to_string());
        let (sender, mut receiver) = mpsc::unbounded_channel();

        let _ = manager.register_connection(conn, sender);

        let result = manager.send_to_connection("conn_1", WsMessage::Text("test".to_string()));
        assert!(result.is_ok());

        let received = receiver.try_recv();
        assert!(received.is_ok());
    }

    #[tokio::test]
    async fn test_broadcast() {
        let manager = WebSocketManager::new();

        let conn1 = Connection::new("conn_1".to_string());
        let (sender1, mut receiver1) = mpsc::unbounded_channel();
        let _ = manager.register_connection(conn1, sender1);

        let conn2 = Connection::new("conn_2".to_string());
        let (sender2, mut receiver2) = mpsc::unbounded_channel();
        let _ = manager.register_connection(conn2, sender2);

        let result = manager.broadcast(WsMessage::Text("broadcast".to_string()));
        assert!(result.is_ok());
        assert_eq!(result.ok(), Some(2));

        assert!(receiver1.try_recv().is_ok());
        assert!(receiver2.try_recv().is_ok());
    }

    #[test]
    fn test_get_user_connections() {
        let manager = WebSocketManager::new();

        let mut conn1 = Connection::new("conn_1".to_string());
        conn1.user_id = Some("user1".to_string());
        let (sender1, _) = mpsc::unbounded_channel();
        let _ = manager.register_connection(conn1, sender1);

        let mut conn2 = Connection::new("conn_2".to_string());
        conn2.user_id = Some("user1".to_string());
        let (sender2, _) = mpsc::unbounded_channel();
        let _ = manager.register_connection(conn2, sender2);

        let user_conns = manager.get_user_connections("user1");
        assert_eq!(user_conns.len(), 2);
    }
}
