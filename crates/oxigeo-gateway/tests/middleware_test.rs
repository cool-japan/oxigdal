//! Middleware integration tests.

use oxigeo_gateway::middleware::{
    Middleware, Request, Response,
    cors::{CorsConfig, CorsMiddleware},
};
use std::collections::HashMap;

fn request_with_origin(origin: &str) -> Request {
    let mut headers = HashMap::new();
    headers.insert("Origin".to_string(), origin.to_string());
    Request {
        method: "GET".to_string(),
        path: "/api/test".to_string(),
        headers,
        body: Vec::new(),
    }
}

#[tokio::test]
async fn test_cors_middleware() {
    let config = CorsConfig::default();
    let middleware = CorsMiddleware::new(config);

    let request = request_with_origin("https://client.example");
    let mut response = Response {
        status: 200,
        headers: HashMap::new(),
        body: Vec::new(),
    };

    let result = middleware.after_response(&request, &mut response).await;
    assert!(result.is_ok());
    assert!(response.headers.contains_key("Access-Control-Allow-Origin"));
    assert!(
        response
            .headers
            .contains_key("Access-Control-Allow-Methods")
    );
}

#[tokio::test]
async fn test_cors_with_credentials() {
    let config = CorsConfig {
        allow_credentials: true,
        allowed_origins: vec!["https://client.example".to_string()],
        ..CorsConfig::default()
    };

    let middleware = CorsMiddleware::new(config);

    let request = request_with_origin("https://client.example");
    let mut response = Response {
        status: 200,
        headers: HashMap::new(),
        body: Vec::new(),
    };

    let _ = middleware.after_response(&request, &mut response).await;
    assert_eq!(
        response.headers.get("Access-Control-Allow-Credentials"),
        Some(&"true".to_string())
    );
    // Wildcard + credentials is never legal: since credentials are enabled, the specific
    // origin must be echoed rather than "*".
    assert_eq!(
        response.headers.get("Access-Control-Allow-Origin"),
        Some(&"https://client.example".to_string())
    );
}

#[tokio::test]
async fn test_cors_disallowed_origin_omits_acao() {
    let config = CorsConfig {
        allowed_origins: vec!["https://allowed.example".to_string()],
        ..CorsConfig::default()
    };
    let middleware = CorsMiddleware::new(config);

    let request = request_with_origin("https://not-allowed.example");
    let mut response = Response {
        status: 200,
        headers: HashMap::new(),
        body: Vec::new(),
    };

    middleware
        .after_response(&request, &mut response)
        .await
        .ok();

    assert!(!response.headers.contains_key("Access-Control-Allow-Origin"));
}

#[tokio::test]
async fn test_compression_middleware() {
    use oxigeo_gateway::middleware::compression::{CompressionConfig, CompressionMiddleware};

    let config = CompressionConfig::default();
    let middleware = CompressionMiddleware::new(config);

    let large_body = vec![b'x'; 2048]; // 2KB of data

    let mut headers = HashMap::new();
    headers.insert("Accept-Encoding".to_string(), "gzip".to_string());
    let request = Request {
        method: "GET".to_string(),
        path: "/api/test".to_string(),
        headers,
        body: Vec::new(),
    };
    let mut response = Response {
        status: 200,
        headers: HashMap::new(),
        body: large_body.clone(),
    };

    let result = middleware.after_response(&request, &mut response).await;
    assert!(result.is_ok());

    // Body should be compressed
    assert!(response.body.len() < large_body.len());
    assert!(response.headers.contains_key("Content-Encoding"));
}

#[tokio::test]
async fn test_logging_middleware() {
    use oxigeo_gateway::middleware::logging::LoggingMiddleware;

    let middleware = LoggingMiddleware::new();

    let mut request = Request {
        method: "GET".to_string(),
        path: "/api/test".to_string(),
        headers: HashMap::new(),
        body: Vec::new(),
    };

    let result = middleware.before_request(&mut request).await;
    assert!(result.is_ok());

    let mut response = Response {
        status: 200,
        headers: HashMap::new(),
        body: Vec::new(),
    };

    let result = middleware.after_response(&request, &mut response).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_metrics_middleware() {
    use oxigeo_gateway::middleware::metrics::MetricsMiddleware;

    let middleware = MetricsMiddleware::new();

    let mut request = Request {
        method: "GET".to_string(),
        path: "/api/test".to_string(),
        headers: HashMap::new(),
        body: Vec::new(),
    };

    let _ = middleware.before_request(&mut request).await;
    assert_eq!(middleware.collector().request_count(), 1);

    let mut response = Response {
        status: 200,
        headers: HashMap::new(),
        body: vec![0; 100],
    };

    let _ = middleware.after_response(&request, &mut response).await;
    assert_eq!(middleware.collector().response_count(), 1);
    assert_eq!(middleware.collector().total_bytes_sent(), 100);
}
