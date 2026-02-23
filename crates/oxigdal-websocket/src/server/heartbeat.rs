//! Heartbeat monitoring for WebSocket connections

use crate::error::Result;
use crate::server::DEFAULT_HEARTBEAT_INTERVAL_SECS;
use crate::server::connection::{Connection, ConnectionId};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::interval;

/// Heartbeat configuration
#[derive(Debug, Clone)]
pub struct HeartbeatConfig {
    /// Ping interval in seconds
    pub interval_secs: u64,
    /// Timeout in seconds (connection closed if no pong received)
    pub timeout_secs: u64,
    /// Enable heartbeat monitoring
    pub enabled: bool,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            interval_secs: DEFAULT_HEARTBEAT_INTERVAL_SECS,
            timeout_secs: DEFAULT_HEARTBEAT_INTERVAL_SECS * 3,
            enabled: true,
        }
    }
}

/// Heartbeat monitor
pub struct HeartbeatMonitor {
    config: HeartbeatConfig,
    connections: Arc<RwLock<HashMap<ConnectionId, Arc<Connection>>>>,
    shutdown: Arc<RwLock<bool>>,
}

impl HeartbeatMonitor {
    /// Create a new heartbeat monitor
    pub fn new(config: HeartbeatConfig) -> Self {
        Self {
            config,
            connections: Arc::new(RwLock::new(HashMap::new())),
            shutdown: Arc::new(RwLock::new(false)),
        }
    }

    /// Add a connection to monitor
    pub async fn add_connection(&self, connection: Arc<Connection>) {
        let mut connections = self.connections.write().await;
        connections.insert(connection.id(), connection);
    }

    /// Remove a connection from monitoring
    pub async fn remove_connection(&self, id: &ConnectionId) {
        let mut connections = self.connections.write().await;
        connections.remove(id);
    }

    /// Get number of monitored connections
    pub async fn connection_count(&self) -> usize {
        self.connections.read().await.len()
    }

    /// Start heartbeat monitoring
    pub async fn start(self: Arc<Self>) -> Result<()> {
        if !self.config.enabled {
            tracing::info!("Heartbeat monitoring disabled");
            return Ok(());
        }

        let mut tick = interval(Duration::from_secs(self.config.interval_secs));

        loop {
            // Check for shutdown
            if *self.shutdown.read().await {
                break;
            }

            tick.tick().await;
            self.check_heartbeats().await;
        }

        tracing::info!("Heartbeat monitor stopped");
        Ok(())
    }

    /// Check heartbeats for all connections
    async fn check_heartbeats(&self) {
        let connections = self.connections.read().await;
        let mut to_remove = Vec::new();

        for (id, conn) in connections.iter() {
            // Check if connection is idle
            if conn.is_idle(self.config.timeout_secs) {
                tracing::warn!(
                    "Connection {} idle for {} seconds, closing",
                    id,
                    self.config.timeout_secs
                );
                to_remove.push(*id);

                // Close the connection
                if let Err(e) = conn.close().await {
                    tracing::error!("Failed to close idle connection {}: {}", id, e);
                }
            } else {
                // Send ping
                if let Err(e) = conn.ping().await {
                    tracing::error!("Failed to ping connection {}: {}", id, e);
                    to_remove.push(*id);
                }
            }
        }

        // Remove dead connections
        if !to_remove.is_empty() {
            drop(connections);
            let mut connections = self.connections.write().await;
            for id in to_remove {
                connections.remove(&id);
            }
        }
    }

    /// Shutdown the heartbeat monitor
    pub async fn shutdown(&self) {
        let mut shutdown = self.shutdown.write().await;
        *shutdown = true;
    }

    /// Get heartbeat statistics
    pub async fn stats(&self) -> HeartbeatStats {
        let connections = self.connections.read().await;
        let mut total_idle_time = 0u64;
        let mut max_idle_time = 0u64;

        for conn in connections.values() {
            let idle_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
                .saturating_sub(conn.last_activity());

            total_idle_time += idle_time;
            max_idle_time = max_idle_time.max(idle_time);
        }

        let count = connections.len();
        let avg_idle_time = if count > 0 {
            total_idle_time / count as u64
        } else {
            0
        };

        HeartbeatStats {
            monitored_connections: count,
            average_idle_time_secs: avg_idle_time,
            max_idle_time_secs: max_idle_time,
        }
    }
}

/// Heartbeat statistics
#[derive(Debug, Clone)]
pub struct HeartbeatStats {
    /// Number of monitored connections
    pub monitored_connections: usize,
    /// Average idle time in seconds
    pub average_idle_time_secs: u64,
    /// Maximum idle time in seconds
    pub max_idle_time_secs: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heartbeat_config_default() {
        let config = HeartbeatConfig::default();
        assert!(config.enabled);
        assert_eq!(config.interval_secs, DEFAULT_HEARTBEAT_INTERVAL_SECS);
    }

    #[tokio::test]
    async fn test_heartbeat_monitor() {
        let config = HeartbeatConfig {
            interval_secs: 1,
            timeout_secs: 3,
            enabled: true,
        };

        let monitor = HeartbeatMonitor::new(config);
        assert_eq!(monitor.connection_count().await, 0);
    }

    #[tokio::test]
    async fn test_heartbeat_stats() {
        let config = HeartbeatConfig::default();
        let monitor = HeartbeatMonitor::new(config);

        let stats = monitor.stats().await;
        assert_eq!(stats.monitored_connections, 0);
        assert_eq!(stats.average_idle_time_secs, 0);
    }
}
