//! WebSocket connection multiplexing.

use super::{Connection, ConnectionId, WsMessage};
use crate::error::Result;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Connection multiplexer for managing multiple WebSocket connections.
pub struct ConnectionMultiplexer {
    connections: Arc<dashmap::DashMap<ConnectionId, ConnectionHandle>>,
}

/// Handle for a WebSocket connection.
pub struct ConnectionHandle {
    /// Connection information
    pub connection: Connection,
    /// Message sender
    pub sender: mpsc::UnboundedSender<WsMessage>,
    /// Task handle
    pub task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl ConnectionMultiplexer {
    /// Creates a new connection multiplexer.
    pub fn new() -> Self {
        Self {
            connections: Arc::new(dashmap::DashMap::new()),
        }
    }

    /// Adds a connection to the multiplexer.
    pub fn add_connection(
        &self,
        connection: Connection,
        sender: mpsc::UnboundedSender<WsMessage>,
    ) -> Result<()> {
        let conn_id = connection.id.clone();
        let handle = ConnectionHandle {
            connection,
            sender,
            task_handle: None,
        };

        self.connections.insert(conn_id, handle);
        Ok(())
    }

    /// Removes a connection from the multiplexer.
    pub fn remove_connection(&self, conn_id: &str) -> Result<()> {
        self.connections.remove(conn_id);
        Ok(())
    }

    /// Gets the number of active connections.
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Sends a message to a specific connection.
    pub fn send_to(&self, conn_id: &str, message: WsMessage) -> Result<()> {
        let handle = self.connections.get(conn_id).ok_or_else(|| {
            crate::error::GatewayError::WebSocketError("Connection not found".to_string())
        })?;

        handle
            .sender
            .send(message)
            .map_err(|e| crate::error::GatewayError::WebSocketError(e.to_string()))?;

        Ok(())
    }

    /// Sends a message to all connections.
    pub fn broadcast(&self, message: WsMessage) -> usize {
        let mut count = 0;

        for entry in self.connections.iter() {
            if entry.sender.send(message.clone()).is_ok() {
                count += 1;
            }
        }

        count
    }
}

impl Default for ConnectionMultiplexer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multiplexer_creation() {
        let mux = ConnectionMultiplexer::new();
        assert_eq!(mux.connection_count(), 0);
    }

    #[test]
    fn test_add_connection() {
        let mux = ConnectionMultiplexer::new();
        let conn = Connection::new("conn_1".to_string());
        let (sender, _receiver) = mpsc::unbounded_channel();

        let result = mux.add_connection(conn, sender);
        assert!(result.is_ok());
        assert_eq!(mux.connection_count(), 1);
    }

    #[test]
    fn test_remove_connection() {
        let mux = ConnectionMultiplexer::new();
        let conn = Connection::new("conn_1".to_string());
        let (sender, _receiver) = mpsc::unbounded_channel();

        let _ = mux.add_connection(conn, sender);
        let result = mux.remove_connection("conn_1");
        assert!(result.is_ok());
        assert_eq!(mux.connection_count(), 0);
    }
}
