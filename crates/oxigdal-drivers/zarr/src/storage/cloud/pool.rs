//! Connection pool statistics

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

// ============================================================================
// Connection Pool
// ============================================================================

/// Connection pool statistics
#[derive(Debug, Default)]
pub struct ConnectionPoolStats {
    /// Total connections created
    pub connections_created: AtomicU64,
    /// Current active connections
    pub active_connections: AtomicUsize,
    /// Peak active connections
    pub peak_connections: AtomicUsize,
    /// Total requests served
    pub requests_served: AtomicU64,
    /// Connection reuse count
    pub connection_reuses: AtomicU64,
    /// Connection errors
    pub connection_errors: AtomicU64,
}

impl ConnectionPoolStats {
    /// Creates new statistics tracker
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a new connection
    pub fn record_connection_created(&self) {
        self.connections_created.fetch_add(1, Ordering::Relaxed);
        let active = self.active_connections.fetch_add(1, Ordering::Relaxed) + 1;

        // Update peak if necessary
        let mut peak = self.peak_connections.load(Ordering::Relaxed);
        while active > peak {
            match self.peak_connections.compare_exchange_weak(
                peak,
                active,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(p) => peak = p,
            }
        }
    }

    /// Records a connection release
    pub fn record_connection_released(&self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }

    /// Records a request served
    pub fn record_request(&self) {
        self.requests_served.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a connection reuse
    pub fn record_reuse(&self) {
        self.connection_reuses.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a connection error
    pub fn record_error(&self) {
        self.connection_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns a summary of the statistics
    #[must_use]
    pub fn summary(&self) -> ConnectionPoolStatsSummary {
        ConnectionPoolStatsSummary {
            connections_created: self.connections_created.load(Ordering::Relaxed),
            active_connections: self.active_connections.load(Ordering::Relaxed),
            peak_connections: self.peak_connections.load(Ordering::Relaxed),
            requests_served: self.requests_served.load(Ordering::Relaxed),
            connection_reuses: self.connection_reuses.load(Ordering::Relaxed),
            connection_errors: self.connection_errors.load(Ordering::Relaxed),
        }
    }
}

/// Summary of connection pool statistics
#[derive(Debug, Clone)]
pub struct ConnectionPoolStatsSummary {
    /// Total connections created
    pub connections_created: u64,
    /// Current active connections
    pub active_connections: usize,
    /// Peak active connections
    pub peak_connections: usize,
    /// Total requests served
    pub requests_served: u64,
    /// Connection reuse count
    pub connection_reuses: u64,
    /// Connection errors
    pub connection_errors: u64,
}

impl ConnectionPoolStatsSummary {
    /// Calculates the connection reuse ratio
    #[must_use]
    pub fn reuse_ratio(&self) -> f64 {
        if self.requests_served == 0 {
            return 0.0;
        }
        self.connection_reuses as f64 / self.requests_served as f64
    }

    /// Calculates the error ratio
    #[must_use]
    pub fn error_ratio(&self) -> f64 {
        let total = self.requests_served + self.connection_errors;
        if total == 0 {
            return 0.0;
        }
        self.connection_errors as f64 / total as f64
    }
}
