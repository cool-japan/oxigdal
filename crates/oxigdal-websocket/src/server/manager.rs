//! Connection manager for WebSocket server

use crate::error::{Error, Result};
use crate::protocol::message::Message;
use crate::server::connection::{Connection, ConnectionId};
use dashmap::DashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::broadcast;

/// Connection manager
pub struct ConnectionManager {
    /// Active connections
    connections: Arc<DashMap<ConnectionId, Arc<Connection>>>,
    /// Connection event broadcaster
    event_tx: broadcast::Sender<ConnectionEvent>,
    /// Total connections counter
    total_connections: Arc<AtomicU64>,
    /// Maximum connections allowed
    max_connections: usize,
}

/// Connection event
#[derive(Debug, Clone)]
pub enum ConnectionEvent {
    /// Connection opened
    Connected(ConnectionId),
    /// Connection closed
    Disconnected(ConnectionId),
    /// Message received
    MessageReceived(ConnectionId, Message),
    /// Error occurred
    Error(ConnectionId, String),
}

impl ConnectionManager {
    /// Create a new connection manager
    pub fn new(max_connections: usize) -> Self {
        let (event_tx, _) = broadcast::channel(1000);

        Self {
            connections: Arc::new(DashMap::new()),
            event_tx,
            total_connections: Arc::new(AtomicU64::new(0)),
            max_connections,
        }
    }

    /// Add a connection
    pub fn add(&self, connection: Arc<Connection>) -> Result<()> {
        // Check if we've reached the limit
        if self.connections.len() >= self.max_connections {
            return Err(Error::ResourceExhausted(format!(
                "Maximum connections ({}) reached",
                self.max_connections
            )));
        }

        let id = connection.id();
        self.connections.insert(id, connection);
        self.total_connections.fetch_add(1, Ordering::Relaxed);

        // Broadcast connection event
        let _ = self.event_tx.send(ConnectionEvent::Connected(id));

        tracing::info!("Connection {} added", id);
        Ok(())
    }

    /// Remove a connection
    pub fn remove(&self, id: &ConnectionId) -> Option<Arc<Connection>> {
        let conn = self.connections.remove(id).map(|(_, v)| v);

        if conn.is_some() {
            let _ = self.event_tx.send(ConnectionEvent::Disconnected(*id));
            tracing::info!("Connection {} removed", id);
        }

        conn
    }

    /// Get a connection by ID
    pub fn get(&self, id: &ConnectionId) -> Option<Arc<Connection>> {
        self.connections.get(id).map(|r| r.value().clone())
    }

    /// Get all connections
    pub fn all(&self) -> Vec<Arc<Connection>> {
        self.connections.iter().map(|r| r.value().clone()).collect()
    }

    /// Get connection count
    pub fn count(&self) -> usize {
        self.connections.len()
    }

    /// Get total connections served
    pub fn total_connections(&self) -> u64 {
        self.total_connections.load(Ordering::Relaxed)
    }

    /// Broadcast a message to all connections
    pub async fn broadcast(&self, message: Message) -> Result<usize> {
        let connections = self.all();
        let mut sent = 0;

        for conn in connections {
            if let Err(e) = conn.send(message.clone()).await {
                tracing::error!("Failed to broadcast to {}: {}", conn.id(), e);
            } else {
                sent += 1;
            }
        }

        Ok(sent)
    }

    /// Broadcast a message to specific connections
    pub async fn broadcast_to(&self, ids: &[ConnectionId], message: Message) -> Result<usize> {
        let mut sent = 0;

        for id in ids {
            if let Some(conn) = self.get(id) {
                if let Err(e) = conn.send(message.clone()).await {
                    tracing::error!("Failed to send to {}: {}", id, e);
                } else {
                    sent += 1;
                }
            }
        }

        Ok(sent)
    }

    /// Broadcast to connections matching a filter
    pub async fn broadcast_filtered<F>(&self, message: Message, filter: F) -> Result<usize>
    where
        F: Fn(&Arc<Connection>) -> bool,
    {
        let connections: Vec<_> = self.all().into_iter().filter(|c| filter(c)).collect();

        let mut sent = 0;
        for conn in connections {
            if let Err(e) = conn.send(message.clone()).await {
                tracing::error!("Failed to broadcast to {}: {}", conn.id(), e);
            } else {
                sent += 1;
            }
        }

        Ok(sent)
    }

    /// Close all connections
    pub async fn close_all(&self) -> Result<()> {
        let connections = self.all();

        for conn in connections {
            if let Err(e) = conn.close().await {
                tracing::error!("Failed to close connection {}: {}", conn.id(), e);
            }
        }

        self.connections.clear();
        Ok(())
    }

    /// Close idle connections
    pub async fn close_idle(&self, timeout_secs: u64) -> Result<usize> {
        let mut closed = 0;
        let to_close: Vec<_> = self
            .all()
            .into_iter()
            .filter(|c| c.is_idle(timeout_secs))
            .collect();

        for conn in to_close {
            let id = conn.id();
            if let Err(e) = conn.close().await {
                tracing::error!("Failed to close idle connection {}: {}", id, e);
            } else {
                self.remove(&id);
                closed += 1;
            }
        }

        Ok(closed)
    }

    /// Get connections by room
    pub async fn get_by_room(&self, room: &str) -> Vec<Arc<Connection>> {
        let mut result = Vec::new();

        for conn in self.all() {
            let metadata = conn.metadata().await;
            if metadata.rooms.contains(room) {
                result.push(conn);
            }
        }

        result
    }

    /// Get connections by topic
    pub async fn get_by_topic(&self, topic: &str) -> Vec<Arc<Connection>> {
        let mut result = Vec::new();

        for conn in self.all() {
            let metadata = conn.metadata().await;
            if metadata.subscriptions.contains(topic) {
                result.push(conn);
            }
        }

        result
    }

    /// Subscribe to connection events
    pub fn subscribe(&self) -> broadcast::Receiver<ConnectionEvent> {
        self.event_tx.subscribe()
    }

    /// Get manager statistics
    pub fn stats(&self) -> ConnectionManagerStats {
        let connections = self.all();

        let mut total_messages_sent = 0u64;
        let mut total_messages_received = 0u64;
        let mut total_bytes_sent = 0u64;
        let mut total_bytes_received = 0u64;
        let mut total_errors = 0u64;

        for conn in &connections {
            let stats = conn.stats();
            total_messages_sent += stats.messages_sent;
            total_messages_received += stats.messages_received;
            total_bytes_sent += stats.bytes_sent;
            total_bytes_received += stats.bytes_received;
            total_errors += stats.errors;
        }

        ConnectionManagerStats {
            active_connections: connections.len(),
            total_connections: self.total_connections(),
            messages_sent: total_messages_sent,
            messages_received: total_messages_received,
            bytes_sent: total_bytes_sent,
            bytes_received: total_bytes_received,
            errors: total_errors,
        }
    }
}

/// Connection manager statistics
#[derive(Debug, Clone)]
pub struct ConnectionManagerStats {
    /// Active connections
    pub active_connections: usize,
    /// Total connections served
    pub total_connections: u64,
    /// Total messages sent
    pub messages_sent: u64,
    /// Total messages received
    pub messages_received: u64,
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Total bytes received
    pub bytes_received: u64,
    /// Total errors
    pub errors: u64,
}

/// Connection statistics (re-export for convenience)
pub use crate::server::connection::ConnectionStats;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_manager_new() {
        let manager = ConnectionManager::new(1000);
        assert_eq!(manager.count(), 0);
        assert_eq!(manager.total_connections(), 0);
    }

    #[test]
    fn test_connection_manager_stats() {
        let manager = ConnectionManager::new(1000);
        let stats = manager.stats();

        assert_eq!(stats.active_connections, 0);
        assert_eq!(stats.total_connections, 0);
        assert_eq!(stats.messages_sent, 0);
    }

    #[tokio::test]
    async fn test_connection_manager_events() {
        let manager = ConnectionManager::new(1000);
        let mut rx = manager.subscribe();

        // Should be able to subscribe
        assert!(rx.try_recv().is_err());
    }
}
