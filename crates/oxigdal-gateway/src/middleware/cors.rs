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
    fn is_origin_allowed(&self, origin: &str) -> bool {
        self.config
            .allowed_origins
            .iter()
            .any(|allowed| allowed == "*")
            || self
                .config
                .allowed_origins
                .iter()
                .any(|allowed| allowed == origin)
    }

    /// Whether the configured allow-list is the bare wildcard (`*`), the only case in which
    /// it is legal to emit a literal `*` `Access-Control-Allow-Origin` value.
    fn is_wildcard_configured(&self) -> bool {
        self.config
            .allowed_origins
            .iter()
            .any(|allowed| allowed == "*")
    }
}

#[async_trait::async_trait]
impl Middleware for CorsMiddleware {
    async fn before_request(&self, _request: &mut Request) -> Result<()> {
        Ok(())
    }

    async fn after_response(&self, request: &Request, response: &mut Response) -> Result<()> {
        if let Some(origin) = request.headers.get("Origin")
            && self.is_origin_allowed(origin)
        {
            // Per the Fetch/CORS spec only a single origin (or the literal `*`) is a legal
            // Access-Control-Allow-Origin value -- never a comma-joined list of every
            // configured origin. Wildcard+credentials is disallowed by the spec, so whenever
            // credentials are enabled we always echo back the specific validated origin.
            let allow_origin = if self.is_wildcard_configured() && !self.config.allow_credentials {
                "*".to_string()
            } else {
                origin.clone()
            };

            response
                .headers
                .insert("Access-Control-Allow-Origin".to_string(), allow_origin);
            response
                .headers
                .insert("Vary".to_string(), "Origin".to_string());
        }
        // If the Origin header is absent, or present but not in the allow-list, the
        // Access-Control-Allow-Origin header is omitted entirely rather than leaking the
        // configured allow-list to a disallowed caller.

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

#[cfg(test)]
mod tests {
    use super::*;

    fn request_with_origin(origin: &str) -> Request {
        let mut headers = std::collections::HashMap::new();
        headers.insert("Origin".to_string(), origin.to_string());
        Request {
            method: "GET".to_string(),
            path: "/api/data".to_string(),
            headers,
            body: Vec::new(),
        }
    }

    fn empty_response() -> Response {
        Response {
            status: 200,
            headers: std::collections::HashMap::new(),
            body: Vec::new(),
        }
    }

    #[tokio::test]
    async fn test_allowed_origin_is_echoed_singly() {
        let config = CorsConfig {
            allowed_origins: vec![
                "https://a.example".to_string(),
                "https://b.example".to_string(),
            ],
            ..CorsConfig::default()
        };
        let middleware = CorsMiddleware::new(config);
        let request = request_with_origin("https://a.example");
        let mut response = empty_response();

        middleware
            .after_response(&request, &mut response)
            .await
            .ok();

        assert_eq!(
            response.headers.get("Access-Control-Allow-Origin"),
            Some(&"https://a.example".to_string())
        );
        assert_eq!(response.headers.get("Vary"), Some(&"Origin".to_string()));
    }

    #[tokio::test]
    async fn test_disallowed_origin_gets_no_acao_header() {
        let config = CorsConfig {
            allowed_origins: vec!["https://a.example".to_string()],
            ..CorsConfig::default()
        };
        let middleware = CorsMiddleware::new(config);
        let request = request_with_origin("https://evil.example");
        let mut response = empty_response();

        middleware
            .after_response(&request, &mut response)
            .await
            .ok();

        assert!(!response.headers.contains_key("Access-Control-Allow-Origin"));
        assert!(!response.headers.contains_key("Vary"));
    }

    #[tokio::test]
    async fn test_wildcard_and_credentials_never_co_occur() {
        let config = CorsConfig {
            allowed_origins: vec!["*".to_string()],
            allow_credentials: true,
            ..CorsConfig::default()
        };
        let middleware = CorsMiddleware::new(config);
        let request = request_with_origin("https://client.example");
        let mut response = empty_response();

        middleware
            .after_response(&request, &mut response)
            .await
            .ok();

        // Wildcard is configured, but since credentials are enabled we must echo the exact
        // origin, never the literal "*" (the Fetch spec forbids wildcard + credentials).
        assert_eq!(
            response.headers.get("Access-Control-Allow-Origin"),
            Some(&"https://client.example".to_string())
        );
        assert_eq!(
            response.headers.get("Access-Control-Allow-Credentials"),
            Some(&"true".to_string())
        );
    }

    #[tokio::test]
    async fn test_wildcard_without_credentials_emits_literal_star() {
        let config = CorsConfig {
            allowed_origins: vec!["*".to_string()],
            allow_credentials: false,
            ..CorsConfig::default()
        };
        let middleware = CorsMiddleware::new(config);
        let request = request_with_origin("https://client.example");
        let mut response = empty_response();

        middleware
            .after_response(&request, &mut response)
            .await
            .ok();

        assert_eq!(
            response.headers.get("Access-Control-Allow-Origin"),
            Some(&"*".to_string())
        );
    }

    #[tokio::test]
    async fn test_multiple_origins_are_never_comma_joined() {
        let config = CorsConfig {
            allowed_origins: vec![
                "https://a.example".to_string(),
                "https://b.example".to_string(),
                "https://c.example".to_string(),
            ],
            ..CorsConfig::default()
        };
        let middleware = CorsMiddleware::new(config);
        let request = request_with_origin("https://b.example");
        let mut response = empty_response();

        middleware
            .after_response(&request, &mut response)
            .await
            .ok();

        let acao = response
            .headers
            .get("Access-Control-Allow-Origin")
            .cloned()
            .unwrap_or_default();
        assert!(!acao.contains(','));
        assert_eq!(acao, "https://b.example");
    }

    #[tokio::test]
    async fn test_no_origin_header_sets_no_acao() {
        let middleware = CorsMiddleware::new(CorsConfig::default());
        let request = Request {
            method: "GET".to_string(),
            path: "/api/data".to_string(),
            headers: std::collections::HashMap::new(),
            body: Vec::new(),
        };
        let mut response = empty_response();

        middleware
            .after_response(&request, &mut response)
            .await
            .ok();

        assert!(!response.headers.contains_key("Access-Control-Allow-Origin"));
    }
}
