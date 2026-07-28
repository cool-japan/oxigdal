//! Router assembly, built-in handlers, layer ordering and the shutdown signal.
//!
//! [`build_router`] wires the gateway's routes (`/health`, `/gateway/metrics`, the GraphQL
//! endpoints, `/ws`) plus the reverse-proxy fallback, then stacks the middleware layers in
//! the precise order the serving layer requires. Remember that axum runs the *last-added*
//! layer *first*, so the layers below are attached inner-to-outer to yield the effective
//! order: trace, version negotiation, in-house middleware bridge, authentication, rate
//! limiting, request timeout and finally the body-size limit closest to the handlers.

use std::time::Duration;

use async_graphql_axum::GraphQLSubscription;
use axum::Router;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use tower_http::trace::TraceLayer;

use crate::error::GatewayError;

use super::state::GatewayState;
use super::{
    auth_layer, graphql, middleware_bridge, proxy, rate_limit_layer, versioning_layer, ws,
};

/// Builds the complete axum router for a gateway, including the middleware stack.
pub(crate) fn build_router(state: GatewayState) -> Router {
    // Capture everything needed for conditional routing / layer configuration before the
    // final `.with_state(state)` consumes the owned state value.
    let graphql_schema = state.graphql.clone();
    let enable_introspection = state.graphql_config.enable_introspection;
    let enable_subscriptions = state.graphql_config.enable_subscriptions;
    let enable_websocket = state.config.enable_websocket;
    let request_timeout = Duration::from_secs(state.config.request_timeout);
    let max_body_size = state.config.max_body_size;

    let mut router = Router::new()
        .route("/health", get(health_handler))
        .route("/gateway/metrics", get(metrics_handler));

    // GraphQL: POST is always available when a schema exists; GET (GraphiQL) is mounted only
    // when introspection is enabled; the subscription websocket only when subscriptions are.
    if let Some(schema) = graphql_schema {
        let mut graphql_routes = post(graphql::graphql_post);
        if enable_introspection {
            graphql_routes = graphql_routes.get(graphql::graphiql);
        }
        router = router.route("/graphql", graphql_routes);
        if enable_subscriptions {
            router = router.route_service("/graphql/ws", GraphQLSubscription::new(schema));
        }
    }

    // The gateway's own WebSocket endpoint (distinct from GraphQL subscriptions).
    if enable_websocket {
        router = router.route("/ws", get(ws::ws_handler));
    }

    router
        .fallback(proxy::proxy_fallback)
        // Innermost layer (added first): cap the body size closest to the handlers.
        .layer(DefaultBodyLimit::max(max_body_size))
        // Per-request timeout returning 504 when exceeded.
        .layer(middleware::from_fn(move |req, next| {
            timeout_middleware(req, next, request_timeout)
        }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit_layer::rate_limit_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_layer::auth_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            middleware_bridge::middleware_bridge,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            versioning_layer::versioning_middleware,
        ))
        // Outermost layer (added last): request tracing spans the whole stack. The span records
        // only the method and the URI *path* -- never the query string -- so credentials that some
        // clients pass as a `?token=` query parameter (browsers cannot set WebSocket auth headers)
        // are never written into traces or logs.
        .layer(TraceLayer::new_for_http().make_span_with(|req: &Request| {
            tracing::info_span!(
                "request",
                method = %req.method(),
                path = %req.uri().path(),
            )
        }))
        .with_state(state)
}

/// Serializes a JSON body into an `application/json` response with the given status.
fn json_response(status: StatusCode, body: &serde_json::Value) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

/// Reports gateway liveness together with a per-backend health snapshot.
pub(crate) async fn health_handler(State(state): State<GatewayState>) -> Response {
    let backends: Vec<serde_json::Value> = state
        .load_balancer
        .get_backends()
        .into_iter()
        .map(|backend| {
            serde_json::json!({
                "id": backend.id,
                "url": backend.url,
                "healthy": backend.healthy,
            })
        })
        .collect();

    let body = serde_json::json!({
        "status": "healthy",
        "service": "oxigeo-gateway",
        "backends": backends,
    });

    json_response(StatusCode::OK, &body)
}

/// Exposes the aggregate request/response/error counters collected by the metrics
/// middleware. Returns a `404`-style JSON payload when metrics collection is disabled.
pub(crate) async fn metrics_handler(State(state): State<GatewayState>) -> Response {
    match state.metrics.as_ref() {
        Some(metrics) => {
            let collector = metrics.collector();
            let body = serde_json::json!({
                "request_count": collector.request_count(),
                "response_count": collector.response_count(),
                "error_count": collector.error_count(),
                "total_bytes_sent": collector.total_bytes_sent(),
            });
            json_response(StatusCode::OK, &body)
        }
        None => {
            let body = serde_json::json!({
                "error": {
                    "code": "METRICS_DISABLED",
                    "message": "metrics collection is not enabled on this gateway",
                }
            });
            json_response(StatusCode::NOT_FOUND, &body)
        }
    }
}

/// Wraps the downstream stack with a per-request timeout, mapping expiry to a `504`.
async fn timeout_middleware(req: Request, next: Next, duration: Duration) -> Response {
    match tokio::time::timeout(duration, next.run(req)).await {
        Ok(response) => response,
        Err(_) => GatewayError::Timeout("request exceeded the configured timeout".to_string())
            .into_response(),
    }
}

/// Resolves when the process receives SIGINT (ctrl-c) or, on Unix, SIGTERM.
///
/// Used by `axum::serve(...).with_graceful_shutdown(...)` so in-flight requests can drain
/// during a rollout instead of being hard-killed. Signal-handler installation failures are
/// logged rather than panicked, honoring the crate's `deny(clippy::panic)` policy.
pub(crate) async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!("failed to install ctrl-c handler: {e}");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(e) => tracing::error!("failed to install SIGTERM handler: {e}"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received; draining in-flight requests");
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::GatewayConfig;
    use crate::server::GatewayServer;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use tower::ServiceExt;

    fn server() -> GatewayServer {
        GatewayServer::builder(GatewayConfig::default())
            .build()
            .expect("default gateway must build")
    }

    #[tokio::test]
    async fn health_endpoint_returns_ok_json() {
        let response = server()
            .router()
            .oneshot(
                HttpRequest::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("request must build"),
            )
            .await
            .expect("router must respond");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body must collect");
        let json: serde_json::Value =
            serde_json::from_slice(&bytes).expect("body must be valid json");
        assert_eq!(json["status"], "healthy");
    }

    #[tokio::test]
    async fn metrics_endpoint_reports_counters() {
        let response = server()
            .router()
            .oneshot(
                HttpRequest::builder()
                    .uri("/gateway/metrics")
                    .body(Body::empty())
                    .expect("request must build"),
            )
            .await
            .expect("router must respond");

        // Metrics are enabled by default, so the counters payload is returned.
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body must collect");
        let json: serde_json::Value =
            serde_json::from_slice(&bytes).expect("body must be valid json");
        assert!(json.get("request_count").is_some());
    }

    #[tokio::test]
    async fn metrics_endpoint_reports_disabled_when_off() {
        let mut config = GatewayConfig::default();
        config.middleware.enable_metrics = false;
        let server = GatewayServer::builder(config)
            .build()
            .expect("gateway must build");
        let response = server
            .router()
            .oneshot(
                HttpRequest::builder()
                    .uri("/gateway/metrics")
                    .body(Body::empty())
                    .expect("request must build"),
            )
            .await
            .expect("router must respond");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
