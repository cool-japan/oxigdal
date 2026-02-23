//! Gateway performance benchmarks.
#![allow(missing_docs, clippy::expect_used, clippy::panic)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxigdal_gateway::{
    auth::{Authenticator, Identity, api_key::ApiKeyAuthenticator},
    middleware::{Request, Response},
};
use std::collections::HashMap;
use std::hint::black_box;

fn bench_api_key_generation(c: &mut Criterion) {
    let auth = ApiKeyAuthenticator::new();

    c.bench_function("api_key_generation", |b| {
        b.iter(|| {
            let _ = auth.generate_key(
                black_box("user123".to_string()),
                black_box("test-key".to_string()),
                black_box(vec!["read".to_string(), "write".to_string()]),
            );
        })
    });
}

fn bench_api_key_validation(c: &mut Criterion) {
    let auth = ApiKeyAuthenticator::new();
    let key = auth
        .generate_key(
            "user123".to_string(),
            "test-key".to_string(),
            vec!["read".to_string()],
        )
        .ok()
        .unwrap_or_default();

    let runtime = tokio::runtime::Runtime::new()
        .ok()
        .unwrap_or_else(|| panic!("Failed to create runtime"));

    c.bench_function("api_key_validation", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let _ = auth.authenticate(black_box(&key)).await;
            })
        })
    });
}

fn bench_identity_permission_check(c: &mut Criterion) {
    let mut identity = Identity::new("user123".to_string());
    for i in 0..100 {
        identity.permissions.insert(format!("permission_{}", i));
    }

    c.bench_function("identity_permission_check", |b| {
        b.iter(|| {
            let _ = identity.has_permission(black_box("permission_50"));
        })
    });
}

fn bench_request_creation(c: &mut Criterion) {
    c.bench_function("request_creation", |b| {
        b.iter(|| {
            let _req = Request {
                method: black_box("GET".to_string()),
                path: black_box("/api/test".to_string()),
                headers: black_box(HashMap::new()),
                body: black_box(Vec::new()),
            };
        })
    });
}

fn bench_response_creation(c: &mut Criterion) {
    c.bench_function("response_creation", |b| {
        b.iter(|| {
            let _resp = Response {
                status: black_box(200),
                headers: black_box(HashMap::new()),
                body: black_box(Vec::new()),
            };
        })
    });
}

fn bench_cors_headers(c: &mut Criterion) {
    use oxigdal_gateway::middleware::{Middleware, cors::CorsConfig, cors::CorsMiddleware};

    let config = CorsConfig::default();
    let middleware = CorsMiddleware::new(config);
    let runtime = tokio::runtime::Runtime::new()
        .ok()
        .unwrap_or_else(|| panic!("Failed to create runtime"));

    c.bench_function("cors_headers", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let mut response = Response {
                    status: 200,
                    headers: HashMap::new(),
                    body: Vec::new(),
                };
                let _ = middleware.after_response(&mut response).await;
            })
        })
    });
}

fn bench_compression_throughput(c: &mut Criterion) {
    use oxigdal_gateway::middleware::Middleware;
    use oxigdal_gateway::middleware::compression::{
        CompressionAlgorithm, CompressionConfig, CompressionMiddleware,
    };

    let mut group = c.benchmark_group("compression_throughput");

    for size in [1024, 4096, 16384].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let config = CompressionConfig {
                min_size: 0,
                algorithm: CompressionAlgorithm::Gzip,
            };
            let middleware = CompressionMiddleware::new(config);
            let body = vec![b'x'; size];
            let runtime = tokio::runtime::Runtime::new()
                .ok()
                .unwrap_or_else(|| panic!("Failed to create runtime"));

            b.iter(|| {
                runtime.block_on(async {
                    let mut response = Response {
                        status: 200,
                        headers: HashMap::new(),
                        body: body.clone(),
                    };
                    let _ = middleware.after_response(&mut response).await;
                })
            });
        });
    }

    group.finish();
}

fn bench_websocket_broadcast(c: &mut Criterion) {
    use oxigdal_gateway::websocket::{Connection, WebSocketManager, WsMessage};
    use tokio::sync::mpsc;

    let runtime = tokio::runtime::Runtime::new()
        .ok()
        .unwrap_or_else(|| panic!("Failed to create runtime"));

    c.bench_function("websocket_broadcast_100_connections", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let manager = WebSocketManager::new();

                // Register 100 connections
                for i in 0..100 {
                    let conn = Connection::new(format!("conn_{}", i));
                    let (sender, _) = mpsc::unbounded_channel();
                    let _ = manager.register_connection(conn, sender);
                }

                let message = WsMessage::Text("broadcast".to_string());
                let _ = manager.broadcast(black_box(message));
            })
        })
    });
}

criterion_group!(
    benches,
    bench_api_key_generation,
    bench_api_key_validation,
    bench_identity_permission_check,
    bench_request_creation,
    bench_response_creation,
    bench_cors_headers,
    bench_compression_throughput,
    bench_websocket_broadcast
);

criterion_main!(benches);
