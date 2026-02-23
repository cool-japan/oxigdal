//! CORS (Cross-Origin Resource Sharing) middleware.

use super::{Middleware, Request, Response};
use crate::error::Result;

/// CORS configuration.
#[derive(Debug, Clone)]
pub struct CorsConfig {
    /// Allowed origins
    pub allowed_origins: Vec<String>,
    /// Allowed methods
    pub allowed_methods: Vec<String>,
    /// Allowed headers
    pub allowed_headers: Vec<String>,
    /// Allow credentials
    pub allow_credentials: bool,
    /// Max age for preflight cache
    pub max_age: u64,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allowed_origins: vec!["*".to_string()],
            allowed_methods: vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "DELETE".to_string(),
            ],
            allowed_headers: vec!["*".to_string()],
            allow_credentials: false,
            max_age: 3600,
        }
    }
}

/// CORS middleware.
pub struct CorsMiddleware {
    config: CorsConfig,
}

impl CorsMiddleware {
    /// Creates a new CORS middleware.
    pub fn new(config: CorsConfig) -> Self {
        Self { config }
    }

    /// Checks if origin is allowed.
    fn _is_origin_allowed(&self, origin: &str) -> bool {
        self.config.allowed_origins.contains(&"*".to_string())
            || self.config.allowed_origins.contains(&origin.to_string())
    }
}

#[async_trait::async_trait]
impl Middleware for CorsMiddleware {
    async fn before_request(&self, _request: &mut Request) -> Result<()> {
        Ok(())
    }

    async fn after_response(&self, response: &mut Response) -> Result<()> {
        if !self.config.allowed_origins.is_empty() {
            let origins = self.config.allowed_origins.join(", ");
            response
                .headers
                .insert("Access-Control-Allow-Origin".to_string(), origins);
        }

        if !self.config.allowed_methods.is_empty() {
            let methods = self.config.allowed_methods.join(", ");
            response
                .headers
                .insert("Access-Control-Allow-Methods".to_string(), methods);
        }

        if !self.config.allowed_headers.is_empty() {
            let headers = self.config.allowed_headers.join(", ");
            response
                .headers
                .insert("Access-Control-Allow-Headers".to_string(), headers);
        }

        if self.config.allow_credentials {
            response.headers.insert(
                "Access-Control-Allow-Credentials".to_string(),
                "true".to_string(),
            );
        }

        response.headers.insert(
            "Access-Control-Max-Age".to_string(),
            self.config.max_age.to_string(),
        );

        Ok(())
    }
}
