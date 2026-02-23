//! Gateway middleware module.
//!
//! Provides middleware components for CORS, compression, caching, logging, and metrics.

pub mod caching;
pub mod compression;
pub mod cors;
pub mod logging;
pub mod metrics;

use crate::error::Result;
use std::sync::Arc;

/// Middleware configuration.
#[derive(Debug, Clone)]
pub struct MiddlewareConfig {
    /// Enable CORS
    pub enable_cors: bool,
    /// Enable compression
    pub enable_compression: bool,
    /// Enable caching
    pub enable_caching: bool,
    /// Enable logging
    pub enable_logging: bool,
    /// Enable metrics
    pub enable_metrics: bool,
    /// CORS configuration
    pub cors: cors::CorsConfig,
    /// Compression configuration
    pub compression: compression::CompressionConfig,
    /// Cache configuration
    pub cache: caching::CacheConfig,
}

impl Default for MiddlewareConfig {
    fn default() -> Self {
        Self {
            enable_cors: true,
            enable_compression: true,
            enable_caching: true,
            enable_logging: true,
            enable_metrics: true,
            cors: cors::CorsConfig::default(),
            compression: compression::CompressionConfig::default(),
            cache: caching::CacheConfig::default(),
        }
    }
}

/// HTTP request for middleware processing.
#[derive(Debug, Clone)]
pub struct Request {
    /// Request method
    pub method: String,
    /// Request path
    pub path: String,
    /// Request headers
    pub headers: std::collections::HashMap<String, String>,
    /// Request body
    pub body: Vec<u8>,
}

/// HTTP response for middleware processing.
#[derive(Debug, Clone)]
pub struct Response {
    /// Response status code
    pub status: u16,
    /// Response headers
    pub headers: std::collections::HashMap<String, String>,
    /// Response body
    pub body: Vec<u8>,
}

/// Middleware trait.
#[async_trait::async_trait]
pub trait Middleware: Send + Sync {
    /// Processes a request before it reaches the handler.
    async fn before_request(&self, request: &mut Request) -> Result<()>;

    /// Processes a response before it's sent to the client.
    async fn after_response(&self, response: &mut Response) -> Result<()>;
}

/// Middleware chain for processing requests and responses.
pub struct MiddlewareChain {
    middlewares: Vec<Arc<dyn Middleware>>,
}

impl MiddlewareChain {
    /// Creates a new middleware chain.
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
        }
    }

    /// Adds middleware to the chain.
    pub fn add(&mut self, middleware: Arc<dyn Middleware>) {
        self.middlewares.push(middleware);
    }

    /// Processes a request through the middleware chain.
    pub async fn process_request(&self, mut request: Request) -> Result<Request> {
        for middleware in &self.middlewares {
            middleware.before_request(&mut request).await?;
        }

        Ok(request)
    }

    /// Processes a response through the middleware chain.
    pub async fn process_response(&self, mut response: Response) -> Result<Response> {
        for middleware in self.middlewares.iter().rev() {
            middleware.after_response(&mut response).await?;
        }

        Ok(response)
    }
}

impl Default for MiddlewareChain {
    fn default() -> Self {
        Self::new()
    }
}

/// Middleware manager for managing multiple middleware components.
pub struct MiddlewareManager {
    chain: Arc<MiddlewareChain>,
    cors: Option<Arc<cors::CorsMiddleware>>,
    compression: Option<Arc<compression::CompressionMiddleware>>,
    caching: Option<Arc<caching::CachingMiddleware>>,
    logging: Option<Arc<logging::LoggingMiddleware>>,
    metrics: Option<Arc<metrics::MetricsMiddleware>>,
}

impl MiddlewareManager {
    /// Creates a new middleware manager from configuration.
    pub fn from_config(config: MiddlewareConfig) -> Self {
        let mut chain = MiddlewareChain::new();

        let cors = if config.enable_cors {
            let middleware = Arc::new(cors::CorsMiddleware::new(config.cors));
            chain.add(middleware.clone());
            Some(middleware)
        } else {
            None
        };

        let compression = if config.enable_compression {
            let middleware = Arc::new(compression::CompressionMiddleware::new(config.compression));
            chain.add(middleware.clone());
            Some(middleware)
        } else {
            None
        };

        let caching = if config.enable_caching {
            let middleware = Arc::new(caching::CachingMiddleware::new(config.cache));
            chain.add(middleware.clone());
            Some(middleware)
        } else {
            None
        };

        let logging = if config.enable_logging {
            let middleware = Arc::new(logging::LoggingMiddleware::new());
            chain.add(middleware.clone());
            Some(middleware)
        } else {
            None
        };

        let metrics = if config.enable_metrics {
            let middleware = Arc::new(metrics::MetricsMiddleware::new());
            chain.add(middleware.clone());
            Some(middleware)
        } else {
            None
        };

        Self {
            chain: Arc::new(chain),
            cors,
            compression,
            caching,
            logging,
            metrics,
        }
    }

    /// Gets the middleware chain.
    pub fn chain(&self) -> &MiddlewareChain {
        &self.chain
    }

    /// Gets the CORS middleware.
    pub fn cors(&self) -> Option<&cors::CorsMiddleware> {
        self.cors.as_deref()
    }

    /// Gets the compression middleware.
    pub fn compression(&self) -> Option<&compression::CompressionMiddleware> {
        self.compression.as_deref()
    }

    /// Gets the caching middleware.
    pub fn caching(&self) -> Option<&caching::CachingMiddleware> {
        self.caching.as_deref()
    }

    /// Gets the logging middleware.
    pub fn logging(&self) -> Option<&logging::LoggingMiddleware> {
        self.logging.as_deref()
    }

    /// Gets the metrics middleware.
    pub fn metrics(&self) -> Option<&metrics::MetricsMiddleware> {
        self.metrics.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_middleware_config_default() {
        let config = MiddlewareConfig::default();
        assert!(config.enable_cors);
        assert!(config.enable_compression);
        assert!(config.enable_caching);
    }

    #[test]
    fn test_middleware_chain_creation() {
        let chain = MiddlewareChain::new();
        assert_eq!(chain.middlewares.len(), 0);
    }

    #[test]
    fn test_request_creation() {
        let req = Request {
            method: "GET".to_string(),
            path: "/api/test".to_string(),
            headers: std::collections::HashMap::new(),
            body: Vec::new(),
        };

        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/api/test");
    }

    #[test]
    fn test_response_creation() {
        let resp = Response {
            status: 200,
            headers: std::collections::HashMap::new(),
            body: Vec::new(),
        };

        assert_eq!(resp.status, 200);
    }
}
