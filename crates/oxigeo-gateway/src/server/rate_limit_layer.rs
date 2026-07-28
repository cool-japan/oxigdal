//! Rate-limiting middleware for the serving layer.
//!
//! [`rate_limit_middleware`] wraps the crate's [`RateLimiter`] (a fixed-window
//! [`StandardRateLimiter`] built in [`GatewayState`], or a caller-supplied override) into an axum
//! `from_fn_with_state` layer. It identifies the caller (authenticated user id, else best-effort
//! client IP, else `"anonymous"`), keys the limiter by identifier plus request path, and uses the
//! atomic [`RateLimiter::try_acquire`] so the check-and-record pair cannot be raced under
//! concurrency.
//!
//! Allowed requests are annotated with `X-RateLimit-Limit` / `X-RateLimit-Remaining` headers
//! (best-effort, read from the limiter's fixed-window counter); a limited request short-circuits
//! with a `429` built from [`GatewayError::RateLimitExceeded`] (which also carries `Retry-After`
//! via its [`IntoResponse`] impl) plus the same rate-limit headers.
//!
//! The `/health` route is always exempt, and when no limiter is configured the request passes
//! straight through.
//!
//! [`StandardRateLimiter`]: crate::rate_limit::StandardRateLimiter
//! [`GatewayState`]: super::state::GatewayState

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::auth::AuthContext;
use crate::error::GatewayError;
use crate::rate_limit::{Decision, RateLimitKey};

use super::auth_layer::client_ip;
use super::state::GatewayState;

/// Global rate-limiting middleware.
///
/// See the module documentation for the identification, keying, and header behaviour. Errors
/// surfaced by the limiter itself (e.g. a storage backend failure) are mapped to their standard
/// error response.
pub(crate) async fn rate_limit_middleware(
    State(state): State<GatewayState>,
    req: Request,
    next: Next,
) -> Response {
    let Some(limiter) = state.rate_limiter.clone() else {
        return next.run(req).await;
    };

    let path = req.uri().path().to_string();
    if path == "/health" {
        return next.run(req).await;
    }

    // Identify the caller: authenticated user id, else best-effort IP, else anonymous.
    let identifier = req
        .extensions()
        .get::<AuthContext>()
        .map(|context| context.identity.user_id.clone())
        .or_else(|| client_ip(&req, &state.trusted_proxies))
        .unwrap_or_else(|| "anonymous".to_string());

    let key = RateLimitKey::new(identifier).with_resource(path);

    match limiter.try_acquire(&key).await {
        Ok(Decision::Allowed) => {
            let mut response = next.run(req).await;
            // Best-effort remaining budget from the fixed-window counter.
            let limit = state.config.rate_limit.max_requests;
            if limit > 0 {
                let used = limiter.count(&key).await.unwrap_or(0);
                let remaining = limit.saturating_sub(used);
                append_rate_limit_headers(response.headers_mut(), limit, remaining);
            }
            response
        }
        Ok(Decision::Limited {
            retry_after, limit, ..
        }) => {
            let mut response = GatewayError::RateLimitExceeded {
                message: "rate limit exceeded".to_string(),
                retry_after: Some(retry_after.as_secs()),
            }
            .into_response();
            append_rate_limit_headers(response.headers_mut(), limit, 0);
            response
        }
        Err(error) => error.into_response(),
    }
}

/// Inserts the `X-RateLimit-Limit` / `X-RateLimit-Remaining` headers, skipping any value that
/// cannot be encoded as a header value (never happens for plain integers, but handled without
/// panicking regardless).
fn append_rate_limit_headers(headers: &mut HeaderMap, limit: u64, remaining: u64) {
    if let Ok(value) = HeaderValue::from_str(&limit.to_string()) {
        headers.insert(HeaderName::from_static("x-ratelimit-limit"), value);
    }
    if let Ok(value) = HeaderValue::from_str(&remaining.to_string()) {
        headers.insert(HeaderName::from_static("x-ratelimit-remaining"), value);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::GatewayConfig;
    use crate::rate_limit::RateLimitConfig;
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use axum::http::StatusCode;
    use axum::middleware::from_fn_with_state;
    use axum::routing::get;
    use std::time::Duration;
    use tower::ServiceExt;

    use super::super::state::BuilderOptions;

    fn build_state(max_requests: u64) -> GatewayState {
        let config = GatewayConfig {
            rate_limit: RateLimitConfig::new(max_requests, Duration::from_secs(60)),
            ..Default::default()
        };
        GatewayState::build(config, BuilderOptions::default()).expect("state builds")
    }

    fn app(state: GatewayState) -> Router {
        Router::new()
            .route("/data", get(|| async { "ok" }))
            .route("/health", get(|| async { "up" }))
            .layer(from_fn_with_state(state, rate_limit_middleware))
    }

    fn request(path: &str) -> Request {
        HttpRequest::builder()
            .uri(path)
            .body(Body::empty())
            .expect("request builds")
    }

    fn header<'a>(response: &'a Response, name: &str) -> Option<&'a str> {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
    }

    #[tokio::test]
    async fn allows_within_budget_and_reports_headers() {
        let app = app(build_state(2));
        let response = app
            .clone()
            .oneshot(request("/data"))
            .await
            .expect("router responds");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(header(&response, "x-ratelimit-limit"), Some("2"));
        assert_eq!(header(&response, "x-ratelimit-remaining"), Some("1"));
    }

    #[tokio::test]
    async fn blocks_once_budget_is_exhausted() {
        let app = app(build_state(2));
        for _ in 0..2 {
            let response = app
                .clone()
                .oneshot(request("/data"))
                .await
                .expect("router responds");
            assert_eq!(response.status(), StatusCode::OK);
        }

        let response = app
            .clone()
            .oneshot(request("/data"))
            .await
            .expect("router responds");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(response.headers().get("retry-after").is_some());
        assert_eq!(header(&response, "x-ratelimit-limit"), Some("2"));
        assert_eq!(header(&response, "x-ratelimit-remaining"), Some("0"));
    }

    #[tokio::test]
    async fn health_route_is_never_limited() {
        let app = app(build_state(1));
        for _ in 0..5 {
            let response = app
                .clone()
                .oneshot(request("/health"))
                .await
                .expect("router responds");
            assert_eq!(response.status(), StatusCode::OK);
        }
    }

    #[tokio::test]
    async fn no_limiter_passes_through() {
        let mut config = GatewayConfig::default();
        config.rate_limit.enabled = false;
        let state = GatewayState::build(config, BuilderOptions::default()).expect("state builds");
        let app = app(state);
        let response = app
            .clone()
            .oneshot(request("/data"))
            .await
            .expect("router responds");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get("x-ratelimit-limit").is_none());
    }
}
