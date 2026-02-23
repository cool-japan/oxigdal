//! Connection pooling utilities.

use crate::error::{Error, Result};
use std::time::Duration;

/// Connection pool configuration.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Minimum number of connections.
    pub min_connections: usize,
    /// Maximum number of connections.
    pub max_connections: usize,
    /// Connection timeout.
    pub connection_timeout: Duration,
    /// Idle connection timeout.
    pub idle_timeout: Option<Duration>,
    /// Maximum connection lifetime.
    pub max_lifetime: Option<Duration>,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            min_connections: 1,
            max_connections: 10,
            connection_timeout: Duration::from_secs(30),
            idle_timeout: Some(Duration::from_secs(600)),
            max_lifetime: Some(Duration::from_secs(3600)),
        }
    }
}

impl PoolConfig {
    /// Create a new pool configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set minimum connections.
    pub fn with_min_connections(mut self, min: usize) -> Self {
        self.min_connections = min;
        self
    }

    /// Set maximum connections.
    pub fn with_max_connections(mut self, max: usize) -> Self {
        self.max_connections = max;
        self
    }

    /// Set connection timeout.
    pub fn with_connection_timeout(mut self, timeout: Duration) -> Self {
        self.connection_timeout = timeout;
        self
    }

    /// Set idle timeout.
    pub fn with_idle_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Set maximum lifetime.
    pub fn with_max_lifetime(mut self, lifetime: Option<Duration>) -> Self {
        self.max_lifetime = lifetime;
        self
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<()> {
        if self.min_connections > self.max_connections {
            return Err(Error::Configuration(
                "min_connections must be <= max_connections".to_string(),
            ));
        }

        if self.max_connections == 0 {
            return Err(Error::Configuration(
                "max_connections must be > 0".to_string(),
            ));
        }

        Ok(())
    }
}

/// Connection pool statistics.
#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    /// Number of active connections.
    pub active_connections: usize,
    /// Number of idle connections.
    pub idle_connections: usize,
    /// Total connections.
    pub total_connections: usize,
    /// Number of pending connection requests.
    pub pending_requests: usize,
}

impl PoolStats {
    /// Create a new pool stats.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if the pool is healthy.
    pub fn is_healthy(&self) -> bool {
        self.total_connections > 0 && self.active_connections < self.total_connections
    }
}
