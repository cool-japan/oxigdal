//! Client traffic redirection during failover.

use crate::error::{HaError, HaResult};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

/// Redirect strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RedirectStrategy {
    /// Immediate redirect.
    Immediate,
    /// Gradual redirect (drain connections).
    Gradual,
    /// Wait for connections to finish.
    WaitForCompletion,
}

/// Client connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConnection {
    /// Connection ID.
    pub id: Uuid,
    /// Client address.
    pub client_address: String,
    /// Current node serving the connection.
    pub current_node_id: Uuid,
    /// Connection start time.
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// Is active.
    pub is_active: bool,
}

/// Redirect result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedirectResult {
    /// Number of connections redirected.
    pub redirected_count: usize,
    /// Number of connections closed.
    pub closed_count: usize,
    /// Duration in milliseconds.
    pub duration_ms: u64,
}

/// Client redirect manager.
pub struct ClientRedirect {
    /// Active connections.
    connections: Arc<DashMap<Uuid, ClientConnection>>,
    /// Redirect strategy.
    strategy: RedirectStrategy,
}

impl ClientRedirect {
    /// Create a new client redirect manager.
    pub fn new(strategy: RedirectStrategy) -> Self {
        Self {
            connections: Arc::new(DashMap::new()),
            strategy,
        }
    }

    /// Register a client connection.
    pub fn register_connection(&self, connection: ClientConnection) -> HaResult<()> {
        info!(
            "Registering connection {} from {}",
            connection.id, connection.client_address
        );
        self.connections.insert(connection.id, connection);
        Ok(())
    }

    /// Unregister a client connection.
    pub fn unregister_connection(&self, connection_id: Uuid) -> HaResult<()> {
        info!("Unregistering connection {}", connection_id);
        self.connections.remove(&connection_id);
        Ok(())
    }

    /// Get active connections for a node.
    pub fn get_node_connections(&self, node_id: Uuid) -> Vec<ClientConnection> {
        self.connections
            .iter()
            .filter(|entry| entry.value().current_node_id == node_id && entry.value().is_active)
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Redirect connections from old node to new node.
    pub async fn redirect_connections(
        &self,
        old_node_id: Uuid,
        new_node_id: Uuid,
    ) -> HaResult<RedirectResult> {
        let start_time = chrono::Utc::now();

        info!(
            "Redirecting connections from {} to {} using {:?} strategy",
            old_node_id, new_node_id, self.strategy
        );

        let connections = self.get_node_connections(old_node_id);
        let total_connections = connections.len();

        info!("Found {} active connections to redirect", total_connections);

        let mut redirected_count = 0;
        let mut closed_count = 0;

        match self.strategy {
            RedirectStrategy::Immediate => {
                for conn in connections {
                    if self.redirect_connection(conn.id, new_node_id).await.is_ok() {
                        redirected_count += 1;
                    } else {
                        closed_count += 1;
                    }
                }
            }
            RedirectStrategy::Gradual => {
                for conn in connections {
                    if self.redirect_connection(conn.id, new_node_id).await.is_ok() {
                        redirected_count += 1;
                    } else {
                        closed_count += 1;
                    }

                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                }
            }
            RedirectStrategy::WaitForCompletion => {
                info!("Waiting for connections to complete naturally");

                for conn in connections {
                    if let Some(mut connection) = self.connections.get_mut(&conn.id) {
                        connection.is_active = false;
                    }
                    closed_count += 1;
                }
            }
        }

        let duration_ms = (chrono::Utc::now() - start_time).num_milliseconds() as u64;

        info!(
            "Redirect complete: {} redirected, {} closed in {}ms",
            redirected_count, closed_count, duration_ms
        );

        Ok(RedirectResult {
            redirected_count,
            closed_count,
            duration_ms,
        })
    }

    /// Redirect a single connection.
    async fn redirect_connection(&self, connection_id: Uuid, new_node_id: Uuid) -> HaResult<()> {
        let mut connection = self
            .connections
            .get_mut(&connection_id)
            .ok_or_else(|| HaError::Failover(format!("Connection {} not found", connection_id)))?;

        connection.current_node_id = new_node_id;

        Ok(())
    }

    /// Get total active connections.
    pub fn get_active_connection_count(&self) -> usize {
        self.connections
            .iter()
            .filter(|entry| entry.value().is_active)
            .count()
    }

    /// Get connections by node.
    pub fn get_connection_count_by_node(&self, node_id: Uuid) -> usize {
        self.get_node_connections(node_id).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_client_redirect() {
        let redirect = ClientRedirect::new(RedirectStrategy::Immediate);

        let old_node_id = Uuid::new_v4();
        let new_node_id = Uuid::new_v4();

        for i in 0..5 {
            let connection = ClientConnection {
                id: Uuid::new_v4(),
                client_address: format!("client{}", i),
                current_node_id: old_node_id,
                started_at: chrono::Utc::now(),
                is_active: true,
            };
            assert!(redirect.register_connection(connection).is_ok());
        }

        assert_eq!(redirect.get_connection_count_by_node(old_node_id), 5);

        let result = redirect
            .redirect_connections(old_node_id, new_node_id)
            .await
            .ok();
        assert!(result.is_some());

        if let Some(r) = result {
            assert_eq!(r.redirected_count, 5);
            assert_eq!(r.closed_count, 0);
        }

        assert_eq!(redirect.get_connection_count_by_node(old_node_id), 0);
        assert_eq!(redirect.get_connection_count_by_node(new_node_id), 5);
    }
}
