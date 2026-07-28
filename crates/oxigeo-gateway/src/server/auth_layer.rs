//! Authentication and per-route authorization middleware for the serving layer.
//!
//! Three pieces live here:
//!
//! * [`auth_middleware`] — a global `from_fn_with_state` layer that authenticates a request
//!   *if* it carries an `Authorization` header, enforces authentication when
//!   [`GatewayState::require_auth`] is set (except for `/health`), enforces MFA when the
//!   configuration demands it, and injects the resulting [`AuthContext`] into the request
//!   extensions for downstream handlers.
//! * [`require_permission`] — a re-usable [`tower::Layer`] applied per route group that gates a
//!   route behind a single permission string, checking the token-embedded permissions first and
//!   then falling back to the server-authoritative [`RbacManager`].
//! * [`client_ip`] — a best-effort peer-IP helper shared with the rate-limit layer.
//!
//! The WebSocket route performs its own pre-upgrade token handling (a token may arrive as a
//! `?token=` query parameter); [`auth_middleware`] only ever inspects the header.
//!
//! [`GatewayState::require_auth`]: super::state::GatewayState::require_auth

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tower::{Layer, Service};

use crate::auth::AuthContext;
use crate::auth::rbac::RbacManager;
use crate::error::GatewayError;

use super::state::GatewayState;

/// Global authentication middleware.
///
/// Behaviour:
///
/// * When no authenticator is configured the request passes straight through.
/// * When an `Authorization` header is present it is handed to the [`MultiAuthenticator`]; on
///   success the [`AuthContext`] is placed in the request extensions (and MFA is enforced when
///   `config.auth.require_mfa` is set — an unverified context yields `401`); on failure a `401`
///   error response is returned.
/// * When no header is present and authentication is required, every path except `/health`
///   returns `401`.
///
/// The RBAC manager is always inserted into the request extensions so a downstream
/// [`require_permission`] layer (which cannot access `State`) can perform server-side lookups.
///
/// [`MultiAuthenticator`]: crate::auth::MultiAuthenticator
pub(crate) async fn auth_middleware(
    State(state): State<GatewayState>,
    mut req: Request,
    next: Next,
) -> Response {
    // Expose the RBAC manager to any `require_permission` layer further down the stack.
    if let Some(rbac) = state.rbac.clone() {
        req.extensions_mut().insert(rbac);
    }

    let Some(authenticator) = state.authenticator.clone() else {
        // No authenticator configured: authentication is a no-op.
        return next.run(req).await;
    };

    let path = req.uri().path().to_string();

    let header_value = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());

    match header_value {
        Some(header) => match authenticator.authenticate(&header).await {
            Ok(context) => {
                if state.config.auth.require_mfa && !context.mfa_verified {
                    return GatewayError::AuthenticationFailed(
                        "MFA required but not verified".to_string(),
                    )
                    .into_response();
                }
                req.extensions_mut().insert(context);
                next.run(req).await
            }
            Err(error) => error.into_response(),
        },
        None => {
            if state.require_auth && path != "/health" {
                GatewayError::AuthenticationFailed("authentication required".to_string())
                    .into_response()
            } else {
                next.run(req).await
            }
        }
    }
}

/// Best-effort client IP address for a request, honouring `X-Forwarded-For` only from trusted peers.
///
/// The forwarded client address — the first, client-closest entry of `X-Forwarded-For`, trimmed of
/// surrounding whitespace — is trusted **only** when the direct TCP peer recorded by axum's
/// `ConnectInfo<SocketAddr>` extension is one of `trusted_proxies`. Otherwise the peer's own
/// address is returned, so a client that is not itself a trusted proxy cannot spoof its rate-limit
/// key or logged address by sending an `X-Forwarded-For` header. Returns `None` when no direct peer
/// address is available at all (for example an in-process `oneshot` test request that carries no
/// `ConnectInfo` extension).
pub(crate) fn client_ip(req: &Request, trusted_proxies: &[IpAddr]) -> Option<String> {
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip())?;

    // Honour a forwarded client address only when the direct peer is a trusted proxy.
    if trusted_proxies.contains(&peer)
        && let Some(entry) = forwarded_client_ip(req)
    {
        return Some(entry);
    }

    Some(peer.to_string())
}

/// Extracts the first (client-closest) `X-Forwarded-For` entry, trimmed, when present and non-empty.
fn forwarded_client_ip(req: &Request) -> Option<String> {
    req.headers()
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
}

/// Builds a [`tower::Layer`] that requires the given permission on every route it wraps.
///
/// Apply it per route group, e.g. `.route_layer(require_permission("dataset.read"))`. A request
/// is admitted when it carries an [`AuthContext`] (injected by `auth_middleware`) whose
/// token-embedded permissions include `permission`, or when the server-side [`RbacManager`]
/// grants it to the authenticated user. Missing context yields `401`; an authenticated but
/// unauthorized request yields `403`.
pub fn require_permission(permission: &'static str) -> RequirePermissionLayer {
    RequirePermissionLayer { permission }
}

/// [`tower::Layer`] produced by [`require_permission`]; wraps a service so requests must satisfy a
/// fixed permission before reaching it.
#[derive(Clone, Copy)]
pub struct RequirePermissionLayer {
    permission: &'static str,
}

impl<S> Layer<S> for RequirePermissionLayer {
    type Service = RequirePermissionService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequirePermissionService {
            inner,
            permission: self.permission,
        }
    }
}

/// [`tower::Service`] produced by [`RequirePermissionLayer`]; performs the permission check and
/// either short-circuits with an error response or forwards to the inner service.
#[derive(Clone)]
pub struct RequirePermissionService<S> {
    inner: S,
    permission: &'static str,
}

#[allow(clippy::type_complexity)]
impl<S> Service<Request> for RequirePermissionService<S>
where
    S: Service<Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = std::result::Result<Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let permission = self.permission;
        // Swap in a fresh clone so we drive the instance whose readiness we polled.
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        Box::pin(async move {
            match evaluate_permission(&req, permission) {
                Ok(()) => inner.call(req).await,
                Err(response) => Ok(*response),
            }
        })
    }
}

/// Decides whether `req` satisfies `permission`, returning a boxed error response otherwise.
///
/// The error response is boxed so the `Result` stays small (an axum `Response` is a large value),
/// which keeps the enclosing service future compact.
fn evaluate_permission(req: &Request, permission: &str) -> std::result::Result<(), Box<Response>> {
    let Some(context) = req.extensions().get::<AuthContext>() else {
        return Err(Box::new(
            GatewayError::AuthenticationFailed("authentication required".to_string())
                .into_response(),
        ));
    };

    // Token-embedded permissions first (self-contained, exact match).
    if context.is_authorized(permission) {
        return Ok(());
    }

    // Server-authoritative RBAC fallback (honours wildcards).
    if let Some(rbac) = req.extensions().get::<Arc<RbacManager>>()
        && rbac.has_permission(&context.identity.user_id, permission)
    {
        return Ok(());
    }

    Err(Box::new(
        GatewayError::AuthorizationFailed(format!("missing required permission: {permission}"))
            .into_response(),
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::auth::{AuthMethod, Identity};
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use axum::http::StatusCode;
    use axum::routing::get;
    use tower::ServiceExt;

    #[test]
    fn client_ip_honours_forwarded_from_trusted_peer() {
        // The direct peer 10.0.0.1 is a trusted proxy, so the client-closest X-Forwarded-For entry
        // is used instead of the peer address.
        let mut req = HttpRequest::builder()
            .uri("/")
            .header(
                "x-forwarded-for",
                "203.0.113.7, 70.41.3.18, 150.172.238.178",
            )
            .body(Body::empty())
            .expect("request builds");
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([10, 0, 0, 1], 8080))));
        let trusted = [IpAddr::from([10, 0, 0, 1])];
        assert_eq!(client_ip(&req, &trusted), Some("203.0.113.7".to_string()));
    }

    #[test]
    fn client_ip_ignores_spoofed_forwarded_from_untrusted_peer() {
        // The direct peer 198.51.100.9 is NOT a trusted proxy, so its X-Forwarded-For header is
        // ignored and the real peer address is used -- the client cannot spoof its identity.
        let mut req = HttpRequest::builder()
            .uri("/")
            .header("x-forwarded-for", "203.0.113.7")
            .body(Body::empty())
            .expect("request builds");
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([198, 51, 100, 9], 5555))));
        let trusted = [IpAddr::from([10, 0, 0, 1])];
        assert_eq!(client_ip(&req, &trusted), Some("198.51.100.9".to_string()));
    }

    #[test]
    fn client_ip_uses_connect_info_without_forwarded() {
        let mut req = HttpRequest::builder()
            .uri("/")
            .body(Body::empty())
            .expect("request builds");
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 8080))));
        assert_eq!(client_ip(&req, &[]), Some("127.0.0.1".to_string()));
    }

    #[test]
    fn client_ip_none_when_no_connect_info() {
        // Even with a forwarded header, a request with no direct peer (in-process oneshot) yields
        // None -- there is no trusted peer to authorise the header.
        let req = HttpRequest::builder()
            .uri("/")
            .header("x-forwarded-for", "203.0.113.7")
            .body(Body::empty())
            .expect("request builds");
        assert_eq!(client_ip(&req, &[]), None);
    }

    fn guarded_router() -> Router {
        Router::new()
            .route("/protected", get(|| async { "ok" }))
            .route_layer(require_permission("dataset.read"))
    }

    #[tokio::test]
    async fn require_permission_rejects_missing_context() {
        let response = guarded_router()
            .oneshot(
                HttpRequest::builder()
                    .uri("/protected")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("router responds");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn require_permission_rejects_without_permission() {
        let context = AuthContext::new(Identity::new("u1".to_string()), AuthMethod::Jwt);
        let mut req = HttpRequest::builder()
            .uri("/protected")
            .body(Body::empty())
            .expect("request builds");
        req.extensions_mut().insert(context);

        let response = guarded_router()
            .oneshot(req)
            .await
            .expect("router responds");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn require_permission_allows_with_token_permission() {
        let mut identity = Identity::new("u1".to_string());
        identity.permissions.insert("dataset.read".to_string());
        let context = AuthContext::new(identity, AuthMethod::Jwt);
        let mut req = HttpRequest::builder()
            .uri("/protected")
            .body(Body::empty())
            .expect("request builds");
        req.extensions_mut().insert(context);

        let response = guarded_router()
            .oneshot(req)
            .await
            .expect("router responds");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn require_permission_allows_via_rbac_wildcard() {
        // A super_admin (auto-seeded role with the "*" permission) is admitted even though the
        // token carries no permissions, provided the RBAC manager is present in extensions.
        let rbac = Arc::new(RbacManager::new());
        rbac.assign_role("root", "super_admin")
            .expect("role assignment succeeds");

        let context = AuthContext::new(Identity::new("root".to_string()), AuthMethod::Jwt);
        let mut req = HttpRequest::builder()
            .uri("/protected")
            .body(Body::empty())
            .expect("request builds");
        req.extensions_mut().insert(context);
        req.extensions_mut().insert(rbac);

        let response = guarded_router()
            .oneshot(req)
            .await
            .expect("router responds");
        assert_eq!(response.status(), StatusCode::OK);
    }
}
