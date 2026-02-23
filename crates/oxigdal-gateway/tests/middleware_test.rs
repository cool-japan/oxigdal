//! Middleware integration tests.

use oxigdal_gateway::middleware::{
    Middleware, Request, Response,
    cors::{CorsConfig, CorsMiddleware},
};
use std::collections::HashMap;

#[tokio::test]
async fn test_cors_middleware() {
    let config = CorsConfig::default();
    let middleware = CorsMiddleware::new(config);

    let mut response = Response {
        status: 200,
        headers: HashMap::new(),
        body: Vec::new(),
    };

    let result = middleware.after_response(&mut response).await;
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
        ..CorsConfig::default()
    };

    let middleware = CorsMiddleware::new(config);

    let mut response = Response {
        status: 200,
        headers: HashMap::new(),
        body: Vec::new(),
    };

    let _ = middleware.after_response(&mut response).await;
    assert_eq!(
        response.headers.get("Access-Control-Allow-Credentials"),
        Some(&"true".to_string())
    );
}

#[tokio::test]
async fn test_compression_middleware() {
    use oxigdal_gateway::middleware::compression::{CompressionConfig, CompressionMiddleware};

    let config = CompressionConfig::default();
    let middleware = CompressionMiddleware::new(config);

    let large_body = vec![b'x'; 2048]; // 2KB of data

    let mut response = Response {
        status: 200,
        headers: HashMap::new(),
        body: large_body.clone(),
    };

    let result = middleware.after_response(&mut response).await;
    assert!(result.is_ok());

    // Body should be compressed
    assert!(response.body.len() < large_body.len());
    assert!(response.headers.contains_key("Content-Encoding"));
}

#[tokio::test]
async fn test_logging_middleware() {
    use oxigdal_gateway::middleware::logging::LoggingMiddleware;

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

    let result = middleware.after_response(&mut response).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_metrics_middleware() {
    use oxigdal_gateway::middleware::metrics::MetricsMiddleware;

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

    let _ = middleware.after_response(&mut response).await;
    assert_eq!(middleware.collector().response_count(), 1);
    assert_eq!(middleware.collector().total_bytes_sent(), 100);
}
