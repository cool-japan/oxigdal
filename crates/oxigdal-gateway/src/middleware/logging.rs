//! Request/response logging middleware.

use super::{Middleware, Request, Response};
use crate::error::Result;

/// Logging middleware.
pub struct LoggingMiddleware;

impl LoggingMiddleware {
    /// Creates a new logging middleware.
    pub fn new() -> Self {
        Self
    }
}

impl Default for LoggingMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Middleware for LoggingMiddleware {
    async fn before_request(&self, request: &mut Request) -> Result<()> {
        tracing::info!("Request: {} {}", request.method, request.path);
        Ok(())
    }

    async fn after_response(&self, response: &mut Response) -> Result<()> {
        tracing::info!(
            "Response: {} - {} bytes",
            response.status,
            response.body.len()
        );
        Ok(())
    }
}
