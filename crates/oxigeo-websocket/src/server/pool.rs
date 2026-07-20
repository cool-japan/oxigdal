//! Connection pooling for WebSocket server

use crate::error::Result;
use crate::server::connection::{Connection, ConnectionId};
use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Connection pool configuration
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum pool size
    pub max_size: usize,
    /// Minimum idle connections
    pub min_idle: usize,
    /// Maximum idle time before eviction (seconds)
    pub max_idle_time_secs: u64,
    /// Pool check interval (seconds)
    pub check_interval_secs: u64,
    /// Enable pool
    pub enabled: bool,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_size: 1000,
            min_idle: 10,
            max_idle_time_secs: 600,
            check_interval_secs: 60,
            enabled: true,
        }
    }
}

/// Connection pool entry
struct PoolEntry {
    connection: Arc<Connection>,
    last_used: Instant,
}

/// Connection pool
pub struct ConnectionPool {
    config: PoolConfig,
    idle_connections: Arc<RwLock<VecDeque<PoolEntry>>>,
    active_connections: Arc<RwLock<HashMap<ConnectionId, Arc<Connection>>>>,
    stats: Arc<PoolStatistics>,
}

/// Pool statistics
struct PoolStatistics {
    acquisitions: AtomicU64,
    releases: AtomicU64,
    evictions: AtomicU64,
    creation_failures: AtomicU64,
}

impl ConnectionPool {
    /// Create a new connection pool
    pub fn new(config: PoolConfig) -> Self {
        Self {
            config,
            idle_connections: Arc::new(RwLock::new(VecDeque::new())),
            active_connections: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(PoolStatistics {
                acquisitions: AtomicU64::new(0),
                releases: AtomicU64::new(0),
                evictions: AtomicU64::new(0),
                creation_failures: AtomicU64::new(0),
            }),
        }
    }

    /// Acquire a connection from the pool
    pub async fn acquire(&self) -> Option<Arc<Connection>> {
        if !self.config.enabled {
            return None;
        }

        self.stats.acquisitions.fetch_add(1, Ordering::Relaxed);

        // Try to get from idle pool
        loop {
            let entry = {
                let mut idle = self.idle_connections.write();
                idle.pop_front()
            };

            if let Some(entry) = entry {
                // Check if connection is still valid
                if self.is_connection_valid(&entry).await {
                    let conn = entry.connection.clone();

                    // Move to active
                    let mut active = self.active_connections.write();
                    active.insert(conn.id(), conn.clone());

                    return Some(conn);
                }
            } else {
                break;
            }
        }

        None
    }

    /// Release a connection back to the pool
    pub async fn release(&self, connection: Arc<Connection>) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        self.stats.releases.fetch_add(1, Ordering::Relaxed);

        let id = connection.id();

        // Remove from active
        {
            let mut active = self.active_connections.write();
            active.remove(&id);
        }

        // Add to idle if pool is not full
        let should_close = {
            let mut idle = self.idle_connections.write();
            if idle.len() < self.config.max_size {
                idle.push_back(PoolEntry {
                    connection: connection.clone(),
                    last_used: Instant::now(),
                });
                false
            } else {
                true
            }
        };

        if should_close {
            // Pool is full, close the connection
            connection.close().await?;
        }

        Ok(())
    }

    /// Evict idle connections
    pub async fn evict_idle(&self) -> Result<usize> {
        if !self.config.enabled {
            return Ok(0);
        }

        let max_idle = Duration::from_secs(self.config.max_idle_time_secs);

        // First, collect entries to evict while holding the lock
        let entries_to_evict: Vec<PoolEntry> = {
            let mut idle = self.idle_connections.write();
            let mut retained = VecDeque::new();
            let mut to_evict = Vec::new();

            while let Some(entry) = idle.pop_front() {
                if entry.last_used.elapsed() > max_idle {
                    to_evict.push(entry);
                } else {
                    retained.push_back(entry);
                }
            }

            *idle = retained;
            to_evict
        };

        // Now close the connections without holding the lock
        let mut evicted = 0;
        for entry in entries_to_evict {
            if let Err(e) = entry.connection.close().await {
                tracing::error!("Failed to close evicted connection: {}", e);
            }
            evicted += 1;
            self.stats.evictions.fetch_add(1, Ordering::Relaxed);
        }

        Ok(evicted)
    }

    /// Maintain pool size
    pub async fn maintain(&self) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        // Evict idle connections
        self.evict_idle().await?;

        Ok(())
    }

    /// Check if a connection is still valid
    async fn is_connection_valid(&self, entry: &PoolEntry) -> bool {
        // Check if too old
        let max_idle = Duration::from_secs(self.config.max_idle_time_secs);
        if entry.last_used.elapsed() > max_idle {
            return false;
        }

        // Check connection state
        matches!(
            entry.connection.state().await,
            crate::server::connection::ConnectionState::Connected
        )
    }

    /// Get pool statistics
    pub fn stats(&self) -> PoolStats {
        let idle = self.idle_connections.read();
        let active = self.active_connections.read();

        PoolStats {
            idle_connections: idle.len(),
            active_connections: active.len(),
            total_acquisitions: self.stats.acquisitions.load(Ordering::Relaxed),
            total_releases: self.stats.releases.load(Ordering::Relaxed),
            total_evictions: self.stats.evictions.load(Ordering::Relaxed),
            total_creation_failures: self.stats.creation_failures.load(Ordering::Relaxed),
        }
    }

    /// Get idle connection count
    pub fn idle_count(&self) -> usize {
        self.idle_connections.read().len()
    }

    /// Get active connection count
    pub fn active_count(&self) -> usize {
        self.active_connections.read().len()
    }

    /// Clear all idle connections
    pub async fn clear_idle(&self) -> Result<usize> {
        let entries: Vec<PoolEntry> = {
            let mut idle = self.idle_connections.write();
            idle.drain(..).collect()
        };

        let count = entries.len();

        for entry in entries {
            if let Err(e) = entry.connection.close().await {
                tracing::error!("Failed to close connection during clear: {}", e);
            }
        }

        Ok(count)
    }

    /// Shutdown the pool
    pub async fn shutdown(&self) -> Result<()> {
        // Close all idle connections
        self.clear_idle().await?;

        // Close all active connections
        let connections: Vec<_> = {
            let active = self.active_connections.write();
            active.values().cloned().collect()
        };

        for conn in connections {
            if let Err(e) = conn.close().await {
                tracing::error!("Failed to close connection during shutdown: {}", e);
            }
        }

        self.active_connections.write().clear();

        Ok(())
    }
}

/// Pool statistics snapshot
#[derive(Debug, Clone)]
pub struct PoolStats {
    /// Idle connections
    pub idle_connections: usize,
    /// Active connections
    pub active_connections: usize,
    /// Total acquisitions
    pub total_acquisitions: u64,
    /// Total releases
    pub total_releases: u64,
    /// Total evictions
    pub total_evictions: u64,
    /// Total creation failures
    pub total_creation_failures: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_config_default() {
        let config = PoolConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_size, 1000);
        assert_eq!(config.min_idle, 10);
    }

    #[test]
    fn test_pool_creation() {
        let config = PoolConfig::default();
        let pool = ConnectionPool::new(config);

        assert_eq!(pool.idle_count(), 0);
        assert_eq!(pool.active_count(), 0);
    }

    #[test]
    fn test_pool_stats() {
        let config = PoolConfig::default();
        let pool = ConnectionPool::new(config);

        let stats = pool.stats();
        assert_eq!(stats.idle_connections, 0);
        assert_eq!(stats.active_connections, 0);
        assert_eq!(stats.total_acquisitions, 0);
    }

    #[tokio::test]
    async fn test_pool_disabled() {
        let config = PoolConfig {
            enabled: false,
            ..Default::default()
        };
        let pool = ConnectionPool::new(config);

        let conn = pool.acquire().await;
        assert!(conn.is_none());
    }
}
