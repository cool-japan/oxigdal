//! API version negotiation middleware for the gateway serving layer.
//!
//! [`versioning_middleware`] wires the crate's [`crate::versioning::VersionNegotiator`] into the
//! axum request pipeline. It dispatches to the negotiation method matching the negotiator's
//! configured [`crate::versioning::VersionStrategy`] (path, header, or query), maps an
//! unsupported version to a `406 Not Acceptable` response via [`crate::error::GatewayError`]'s
//! `IntoResponse`, and otherwise:
//!
//! 1. inserts the resolved [`crate::versioning::VersionContext`] into the request extensions so
//!    downstream handlers (and the GraphQL context) can read the negotiated version;
//! 2. merges the negotiator's response headers (the resolved-version header, plus a `Warning`
//!    when the version was down-negotiated) into the outgoing response; and
//! 3. appends deprecation headers (`Warning` / `Sunset` / `Link`) when the resolved version has a
//!    deprecation policy registered on the state's [`crate::versioning::deprecation::DeprecationManager`].
//!
//! When no negotiator is configured on the state the request passes straight through unchanged.

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::server::state::GatewayState;
use crate::versioning::VersionStrategy;

/// Negotiates the API version for a request and decorates the response.
///
/// The negotiation method is chosen from `negotiator.strategy()`:
/// [`VersionStrategy::UrlPath`] negotiates from the request path,
/// [`VersionStrategy::AcceptHeader`] and [`VersionStrategy::CustomHeader`] from the request
/// headers, and [`VersionStrategy::QueryParameter`] from the raw query string. A negotiation
/// error (an unsupported version with no compatible fallback) short-circuits to a `406` response.
///
/// On success the resolved [`crate::versioning::VersionContext`] is stored in the request
/// extensions, the inner service runs, and version/deprecation headers are merged into the
/// response. When the state has no negotiator the request is forwarded untouched.
pub(crate) async fn versioning_middleware(
    State(state): State<GatewayState>,
    mut req: Request,
    next: Next,
) -> Response {
    let Some(negotiator) = state.version_negotiator.as_ref() else {
        return next.run(req).await;
    };

    // Dispatch to the negotiation method matching the configured strategy. Each
    // `negotiate_from_*` returns the default version unless the strategy matches, so calling the
    // wrong one would silently ignore the client's version request.
    let negotiation = match negotiator.strategy() {
        VersionStrategy::UrlPath => negotiator.negotiate_from_path(req.uri().path()),
        VersionStrategy::AcceptHeader | VersionStrategy::CustomHeader => {
            negotiator.negotiate_from_headers(req.headers())
        }
        VersionStrategy::QueryParameter => {
            negotiator.negotiate_from_query(req.uri().query().unwrap_or(""))
        }
    };

    let context = match negotiation {
        Ok(context) => context,
        // An unsupported version (with no compatible fallback) maps to `406 Not Acceptable`.
        Err(err) => return err.into_response(),
    };

    // Make the negotiated version visible to downstream handlers and the GraphQL context.
    req.extensions_mut().insert(context.clone());

    let mut response = next.run(req).await;

    // Merge the resolved-version header (and any down-negotiation Warning) into the response.
    for (name, value) in negotiator.create_response_headers(&context).iter() {
        response.headers_mut().insert(name.clone(), value.clone());
    }

    // Append deprecation headers when the resolved version is deprecated. Appending (rather than
    // inserting) preserves any negotiation Warning already set above.
    if let Some(deprecations) = state.deprecations.as_ref()
        && let Some(warning) = deprecations.create_warning(&context.resolved)
    {
        for (name, value) in warning.to_headers().iter() {
            response.headers_mut().append(name.clone(), value.clone());
        }
    }

    response
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::GatewayConfig;
    use crate::server::state::{BuilderOptions, GatewayState};
    use crate::versioning::deprecation::{DeprecationManager, DeprecationPolicy};
    use crate::versioning::{
        ApiVersion, VersionContext, VersionNegotiator, VersionRegistry, VersionStrategy,
    };
    use axum::body::Body;
    use axum::http::StatusCode;
    use axum::middleware::from_fn_with_state;
    use axum::routing::get;
    use axum::{Extension, Router};
    use tower::ServiceExt;

    /// Registry with v1.0.0 (default) and v2.0.0 supported.
    fn test_registry() -> VersionRegistry {
        let mut registry = VersionRegistry::new(ApiVersion::new(1, 0, 0));
        registry.add_version(ApiVersion::new(2, 0, 0));
        registry
    }

    fn custom_header_negotiator() -> VersionNegotiator {
        VersionNegotiator::new(test_registry(), VersionStrategy::CustomHeader)
            .with_header_name("X-API-Version")
    }

    fn build_state(
        negotiator: Option<VersionNegotiator>,
        deprecations: Option<DeprecationManager>,
    ) -> GatewayState {
        let opts = BuilderOptions {
            version_negotiator: negotiator,
            deprecations,
            ..Default::default()
        };
        GatewayState::build(GatewayConfig::default(), opts).expect("state builds")
    }

    /// Leaf handler that echoes the negotiated version, or a sentinel when none was inserted.
    async fn version_echo(context: Option<Extension<VersionContext>>) -> String {
        match context {
            Some(Extension(ctx)) => ctx.resolved.to_string(),
            None => "no-version".to_string(),
        }
    }

    fn app(state: GatewayState) -> Router {
        Router::new()
            .route("/api/data", get(version_echo))
            .layer(from_fn_with_state(state, versioning_middleware))
    }

    async fn body_string(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("collect body");
        String::from_utf8_lossy(&bytes).into_owned()
    }

    #[tokio::test]
    async fn default_version_passes_and_sets_response_header() {
        let state = build_state(Some(custom_header_negotiator()), None);
        let request = axum::http::Request::builder()
            .uri("/api/data")
            .body(Body::empty())
            .expect("request");

        let response = app(state).oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        // The resolved default version is echoed both in a response header and (via the inserted
        // extension) in the handler body.
        let header = response
            .headers()
            .get("x-api-version")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        assert_eq!(header.as_deref(), Some("1.0.0"));

        assert_eq!(body_string(response).await, "1.0.0");
    }

    #[tokio::test]
    async fn context_is_inserted_into_extensions() {
        // Requesting the supported v2.0.0 must reach the handler through the extension.
        let state = build_state(Some(custom_header_negotiator()), None);
        let request = axum::http::Request::builder()
            .uri("/api/data")
            .header("X-API-Version", "2.0.0")
            .body(Body::empty())
            .expect("request");

        let response = app(state).oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_string(response).await, "2.0.0");
    }

    #[tokio::test]
    async fn unsupported_version_returns_406() {
        let state = build_state(Some(custom_header_negotiator()), None);
        let request = axum::http::Request::builder()
            .uri("/api/data")
            .header("X-API-Version", "9.0.0")
            .body(Body::empty())
            .expect("request");

        let response = app(state).oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::NOT_ACCEPTABLE);
    }

    #[tokio::test]
    async fn deprecation_warning_header_present() {
        let mut manager = DeprecationManager::new();
        manager.add_policy(DeprecationPolicy::new(
            ApiVersion::new(1, 0, 0),
            "API version 1.0 is deprecated",
        ));
        // No header -> resolves to the default v1.0.0, which carries a deprecation policy.
        let state = build_state(Some(custom_header_negotiator()), Some(manager));
        let request = axum::http::Request::builder()
            .uri("/api/data")
            .body(Body::empty())
            .expect("request");

        let response = app(state).oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response.headers().contains_key("warning"),
            "expected a deprecation Warning header on the response"
        );
    }

    #[tokio::test]
    async fn missing_negotiator_passes_through() {
        let state = build_state(None, None);
        let request = axum::http::Request::builder()
            .uri("/api/data")
            .body(Body::empty())
            .expect("request");

        let response = app(state).oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        // No version context was inserted, so the handler observes the sentinel and no
        // version header is present.
        assert!(!response.headers().contains_key("x-api-version"));
        assert_eq!(body_string(response).await, "no-version");
    }
}
