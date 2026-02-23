//! Metrics collection middleware.

use super::{Middleware, Request, Response};
use crate::error::Result;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Metrics collector.
#[derive(Debug)]
pub struct MetricsCollector {
    request_count: Arc<AtomicU64>,
    response_count: Arc<AtomicU64>,
    error_count: Arc<AtomicU64>,
    total_bytes_sent: Arc<AtomicU64>,
}

impl MetricsCollector {
    /// Creates a new metrics collector.
    pub fn new() -> Self {
        Self {
            request_count: Arc::new(AtomicU64::new(0)),
            response_count: Arc::new(AtomicU64::new(0)),
            error_count: Arc::new(AtomicU64::new(0)),
            total_bytes_sent: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Gets request count.
    pub fn request_count(&self) -> u64 {
        self.request_count.load(Ordering::Relaxed)
    }

    /// Gets response count.
    pub fn response_count(&self) -> u64 {
        self.response_count.load(Ordering::Relaxed)
    }

    /// Gets error count.
    pub fn error_count(&self) -> u64 {
        self.error_count.load(Ordering::Relaxed)
    }

    /// Gets total bytes sent.
    pub fn total_bytes_sent(&self) -> u64 {
        self.total_bytes_sent.load(Ordering::Relaxed)
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Metrics middleware.
pub struct MetricsMiddleware {
    collector: Arc<MetricsCollector>,
}

impl MetricsMiddleware {
    /// Creates a new metrics middleware.
    pub fn new() -> Self {
        Self {
            collector: Arc::new(MetricsCollector::new()),
        }
    }

    /// Gets the metrics collector.
    pub fn collector(&self) -> &MetricsCollector {
        &self.collector
    }
}

impl Default for MetricsMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Middleware for MetricsMiddleware {
    async fn before_request(&self, _request: &mut Request) -> Result<()> {
        self.collector.request_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    async fn after_response(&self, response: &mut Response) -> Result<()> {
        self.collector
            .response_count
            .fetch_add(1, Ordering::Relaxed);
        self.collector
            .total_bytes_sent
            .fetch_add(response.body.len() as u64, Ordering::Relaxed);

        if response.status >= 400 {
            self.collector.error_count.fetch_add(1, Ordering::Relaxed);
        }

        Ok(())
    }
}
