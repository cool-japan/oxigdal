//! In-process integration tests for the gateway serving layer.
//!
//! These drive a fully-assembled [`oxigeo_gateway::GatewayServer`] through
//! `tower::ServiceExt::oneshot` on its axum router, exercising the built-in routes and the
//! whole middleware stack (version negotiation, the in-house middleware bridge, authentication,
//! rate limiting) without opening a socket. Real-socket behaviour (reverse proxy, WebSocket,
//! subscriptions) lives in `server_proxy_ws.rs`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use oxigeo_gateway::auth::Identity;
use oxigeo_gateway::auth::jwt::JwtAuthenticator;
use oxigeo_gateway::graphql::GraphQLConfig;
use oxigeo_gateway::versioning::deprecation::{DeprecationManager, DeprecationPolicy};
use oxigeo_gateway::versioning::{ApiVersion, VersionNegotiator, VersionRegistry, VersionStrategy};
use oxigeo_gateway::{GatewayConfig, GatewayServer};

/// Collects a response body into raw bytes (bounded, so a runaway body cannot hang the test).
async fn collect_body(response: axum::response::Response) -> Vec<u8> {
    axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("response body collects")
        .to_vec()
}

/// Parses a response body as JSON.
async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = collect_body(response).await;
    serde_json::from_slice(&bytes).expect("response body is valid json")
}

/// Reads a response body as UTF-8 (lossy) text.
async fn body_text(response: axum::response::Response) -> String {
    String::from_utf8_lossy(&collect_body(response).await).into_owned()
}

/// Reads a single response header value as an owned string.
fn header_value(response: &axum::response::Response, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

/// 1. `/health` answers `200` with a `status` field.
#[tokio::test]
async fn health_returns_200_with_status() {
    let server = GatewayServer::builder(GatewayConfig::default())
        .build()
        .expect("gateway builds");

    let response = server
        .router()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["status"], "healthy");
}

/// 2. With no backends registered the fallback proxy returns a `404 NO_ROUTE`.
#[tokio::test]
async fn fallback_without_backends_returns_404_no_route() {
    let server = GatewayServer::builder(GatewayConfig::default())
        .build()
        .expect("gateway builds");

    let response = server
        .router()
        .oneshot(
            Request::builder()
                .uri("/no/such/route")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let json = body_json(response).await;
    assert_eq!(json["error"]["code"], "NO_ROUTE");
}

/// 3. GraphQL executes over HTTP: `__typename` resolves to the query root, and a
///    context-dependent query returns data (proving the per-request `GraphQLContext` injection).
#[tokio::test]
async fn graphql_typename_and_context_dependent_query_succeed() {
    let server = GatewayServer::builder(GatewayConfig::default())
        .build()
        .expect("gateway builds");

    let typename = server
        .router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/graphql")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"query":"{ __typename }"}"#))
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(typename.status(), StatusCode::OK);
    let json = body_json(typename).await;
    assert_eq!(json["data"]["__typename"], "QueryRoot");

    let datasets = server
        .router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/graphql")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"query":"{ datasets { totalCount } }"}"#))
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(datasets.status(), StatusCode::OK);
    let json = body_json(datasets).await;
    assert!(
        json.get("errors").is_none(),
        "context-dependent resolver returned errors: {json}"
    );
    assert_eq!(json["data"]["datasets"]["totalCount"], 1000);
}

/// 4. The GraphiQL playground (`GET /graphql`) is mounted only when introspection is enabled.
#[tokio::test]
async fn graphiql_is_gated_by_introspection() {
    // Introspection on (default): the playground is served and points at the subscription route.
    let server = GatewayServer::builder(GatewayConfig::default())
        .build()
        .expect("gateway builds");
    let response = server
        .router()
        .oneshot(
            Request::builder()
                .uri("/graphql")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body_text(response).await.contains("/graphql/ws"));

    // Introspection off: `GET /graphql` is not mounted (POST still is, so axum answers 405).
    let graphql_config = GraphQLConfig {
        enable_introspection: false,
        ..GraphQLConfig::default()
    };
    let server = GatewayServer::builder(GatewayConfig::default())
        .with_graphql_config(graphql_config)
        .build()
        .expect("gateway builds");
    let response = server
        .router()
        .oneshot(
            Request::builder()
                .uri("/graphql")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert!(
        response.status() == StatusCode::METHOD_NOT_ALLOWED
            || response.status() == StatusCode::NOT_FOUND,
        "GET /graphql should be absent without introspection, got {}",
        response.status()
    );
}

/// 5. Rate limiting exempts `/health` but limits other paths once the budget is exhausted.
#[tokio::test]
async fn rate_limit_exempts_health_but_limits_other_paths() {
    let mut config = GatewayConfig::default();
    config.rate_limit.max_requests = 2;
    let server = GatewayServer::builder(config)
        .build()
        .expect("gateway builds");

    // `/health` is always exempt: three calls succeed even though the budget is two.
    for _ in 0..3 {
        let response = server
            .router()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("router responds");
        assert_eq!(response.status(), StatusCode::OK);
    }

    // A non-health path consumes the shared budget; the first two calls are admitted.
    for _ in 0..2 {
        let response = server
            .router()
            .oneshot(
                Request::builder()
                    .uri("/limited")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("router responds");
        assert_ne!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    // The third call is rate limited, carrying Retry-After and the rate-limit headers.
    let limited = server
        .router()
        .oneshot(
            Request::builder()
                .uri("/limited")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(limited.headers().get("retry-after").is_some());
    assert_eq!(
        header_value(&limited, "x-ratelimit-limit").as_deref(),
        Some("2")
    );
}

/// 6. `require_auth` rejects missing/garbage credentials and a valid token's user id reaches the
///    resolvers (the created dataset is owned by the authenticated user).
#[tokio::test]
async fn auth_required_is_enforced_and_valid_token_injects_user_id() {
    const SECRET: &str = "integration-test-secret-key-please-32bytes";
    let mut config = GatewayConfig::default();
    config.auth.jwt_secret = Some(SECRET.to_string());
    let jwt_expiration = config.auth.jwt_expiration;
    let server = GatewayServer::builder(config)
        .require_auth(true)
        .build()
        .expect("gateway builds");

    // (a) No credentials -> 401.
    let response = server
        .router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/graphql")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"query":"{ __typename }"}"#))
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // (b) A malformed bearer token -> 401.
    let response = server
        .router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/graphql")
                .header("content-type", "application/json")
                .header("authorization", "Bearer not.a.valid.jwt")
                .body(Body::from(r#"{"query":"{ __typename }"}"#))
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // (c) A real token authenticates and its user id flows into the GraphQL context.
    let token = JwtAuthenticator::new(SECRET.as_bytes(), jwt_expiration)
        .create_token(&Identity::new("u1".to_string()))
        .expect("token mints");
    let mutation = serde_json::json!({
        "query": "mutation { createDataset(input: { name: \"T\", format: GEO_TIFF, srs: \"EPSG:4326\" }) { ownerId } }"
    })
    .to_string();
    let response = server
        .router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/graphql")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(mutation))
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert!(
        json.get("errors").is_none(),
        "unexpected graphql errors: {json}"
    );
    assert_eq!(json["data"]["createDataset"]["ownerId"], "u1");
}

/// 7. When MFA is required, a valid (but unverified) token is still rejected with `401`.
#[tokio::test]
async fn mfa_required_rejects_a_valid_token() {
    const SECRET: &str = "integration-test-mfa-secret-key-32bytes!!";
    let mut config = GatewayConfig::default();
    config.auth.jwt_secret = Some(SECRET.to_string());
    config.auth.require_mfa = true;
    let jwt_expiration = config.auth.jwt_expiration;
    let server = GatewayServer::builder(config)
        .require_auth(true)
        .build()
        .expect("gateway builds");

    let token = JwtAuthenticator::new(SECRET.as_bytes(), jwt_expiration)
        .create_token(&Identity::new("u1".to_string()))
        .expect("token mints");
    let response = server
        .router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/graphql")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(r#"{"query":"{ __typename }"}"#))
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(body_text(response).await.contains("MFA"));
}

/// 9. CORS: a preflight `OPTIONS` is synthesised as `204`, and a simple request carrying an
///    `Origin` is annotated by the in-house CORS middleware.
#[tokio::test]
async fn cors_preflight_and_simple_request_are_handled() {
    let mut config = GatewayConfig::default();
    config.middleware.enable_caching = false;
    config.middleware.cors.allowed_origins = vec!["https://example.test".to_string()];
    let server = GatewayServer::builder(config)
        .build()
        .expect("gateway builds");

    // Preflight: OPTIONS with Origin + Access-Control-Request-Method -> synthesised 204.
    let preflight = server
        .router()
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/graphql")
                .header("origin", "https://example.test")
                .header("access-control-request-method", "POST")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(preflight.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        header_value(&preflight, "access-control-allow-origin").as_deref(),
        Some("https://example.test")
    );
    assert_eq!(header_value(&preflight, "vary").as_deref(), Some("Origin"));

    // Simple GET carrying an Origin -> the response is annotated with the allow-origin + Vary.
    let simple = server
        .router()
        .oneshot(
            Request::builder()
                .uri("/health")
                .header("origin", "https://example.test")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(simple.status(), StatusCode::OK);
    assert_eq!(
        header_value(&simple, "access-control-allow-origin").as_deref(),
        Some("https://example.test")
    );
    assert_eq!(header_value(&simple, "vary").as_deref(), Some("Origin"));
}

/// 10. Compression is applied only when the client advertises support via `Accept-Encoding`.
#[tokio::test]
async fn compression_is_negotiated_via_accept_encoding() {
    let mut config = GatewayConfig::default();
    config.middleware.enable_caching = false;
    config.middleware.compression.min_size = 1;
    let server = GatewayServer::builder(config)
        .build()
        .expect("gateway builds");

    let with_gzip = server
        .router()
        .oneshot(
            Request::builder()
                .uri("/health")
                .header("accept-encoding", "gzip")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(with_gzip.status(), StatusCode::OK);
    assert_eq!(
        header_value(&with_gzip, "content-encoding").as_deref(),
        Some("gzip")
    );

    let without = server
        .router()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(without.status(), StatusCode::OK);
    assert!(header_value(&without, "content-encoding").is_none());
}

/// 11. A cacheable `GET` is stored on the first call and served from cache (`X-Cache: HIT`) on
///     the second.
#[tokio::test]
async fn caching_returns_hit_on_the_second_get() {
    let server = GatewayServer::builder(GatewayConfig::default())
        .build()
        .expect("gateway builds");

    let first = server
        .router()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(header_value(&first, "x-cache").as_deref(), Some("MISS"));

    let second = server
        .router()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(header_value(&second, "x-cache").as_deref(), Some("HIT"));
}

/// 12. Version negotiation echoes the resolved version, rejects an unsupported one with `406`,
///     and attaches a deprecation `Warning` for a deprecated version.
#[tokio::test]
async fn version_negotiation_sets_headers_and_deprecation_warning() {
    let mut registry = VersionRegistry::new(ApiVersion::new(1, 0, 0));
    registry.add_version(ApiVersion::new(2, 0, 0));
    let negotiator = VersionNegotiator::new(registry, VersionStrategy::CustomHeader)
        .with_header_name("X-API-Version");

    let mut deprecations = DeprecationManager::new();
    deprecations.add_policy(DeprecationPolicy::new(
        ApiVersion::new(1, 0, 0),
        "API version 1.0 is deprecated",
    ));

    let mut config = GatewayConfig::default();
    config.middleware.enable_caching = false;
    let server = GatewayServer::builder(config)
        .with_version_negotiator(negotiator)
        .with_deprecation_manager(deprecations)
        .build()
        .expect("gateway builds");

    // Supported v2 -> resolved-version header echoed back.
    let v2 = server
        .router()
        .oneshot(
            Request::builder()
                .uri("/health")
                .header("x-api-version", "2.0.0")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(v2.status(), StatusCode::OK);
    assert_eq!(header_value(&v2, "x-api-version").as_deref(), Some("2.0.0"));

    // Unsupported v9 -> 406 Not Acceptable.
    let v9 = server
        .router()
        .oneshot(
            Request::builder()
                .uri("/health")
                .header("x-api-version", "9.0.0")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(v9.status(), StatusCode::NOT_ACCEPTABLE);

    // Deprecated v1 -> a Warning header is attached.
    let v1 = server
        .router()
        .oneshot(
            Request::builder()
                .uri("/health")
                .header("x-api-version", "1.0.0")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(v1.status(), StatusCode::OK);
    assert!(v1.headers().contains_key("warning"));
}

/// 13. A request body larger than `max_body_size` is rejected with `413 Payload Too Large`.
#[tokio::test]
async fn oversized_request_body_returns_413() {
    let config = GatewayConfig {
        max_body_size: 64,
        ..Default::default()
    };
    let server = GatewayServer::builder(config)
        .build()
        .expect("gateway builds");

    let big_body = "x".repeat(256);
    let response = server
        .router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/graphql")
                .header("content-type", "application/json")
                .body(Body::from(big_body))
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}
