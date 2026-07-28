//! Load-balanced reverse-proxy fallback for the gateway serving layer.
//!
//! [`proxy_fallback`] is mounted as the axum `.fallback` handler: any request that does not
//! match a built-in route (`/health`, `/gateway/metrics`, the GraphQL routes, `/ws`) is
//! forwarded to an upstream backend chosen by the [`crate::loadbalancer::LoadBalancer`].
//!
//! Behaviour:
//!
//! - With no backends registered it answers `404` with a stable `NO_ROUTE` JSON error.
//! - The request body is buffered once (bounded by `config.max_body_size`) so that the
//!   [`crate::loadbalancer::advanced::FailoverManager`] can resend it across retry attempts.
//! - When a [`crate::transform::TransformEngine`] is configured it is applied to the
//!   upstream-bound request (headers and, when the content type is recognised, the body).
//!   Response transformation is intentionally out of scope for 0.2.1.
//! - Forwarding uses a hyper 1 `client::conn::http1` connection over a raw Tokio `TcpStream`
//!   (wrapped in the crate's Pure-Rust rustls connector for `https://` upstreams). A minimal
//!   local [`TokioIo`] adapter bridges Tokio's `AsyncRead`/`AsyncWrite` to hyper's
//!   `hyper::rt::{Read, Write}` so no `hyper-util` dependency is needed.
//! - Hop-by-hop headers are stripped in both directions, the `Host` header is rewritten to the
//!   backend authority and the client IP is appended to `X-Forwarded-For`.
//! - Each attempt is bounded by `config.request_timeout`. Transport errors and timeouts record a
//!   backend failure and propagate an error (which the failover manager may retry); a `5xx`
//!   response records a failure but is still returned to the client; any other response records a
//!   success. The response body streams straight back to the client without buffering.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::extract::{ConnectInfo, Request, State};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri};
use http_body_util::Full;
use hyper::body::Incoming;

use crate::error::{GatewayError, Result};
use crate::loadbalancer::Backend;
use crate::server::state::GatewayState;
use crate::transform::ContentType;

/// Reverse-proxy fallback handler.
///
/// Forwards the request to a load-balanced backend, applying optional request transformation and
/// cross-backend failover retry. Returns a `404` `NO_ROUTE` error when no backends are configured.
pub(crate) async fn proxy_fallback(State(state): State<GatewayState>, req: Request) -> Response {
    // No upstreams configured: the fallback has nothing to proxy to.
    if state.load_balancer.get_backends().is_empty() {
        return no_route_response();
    }

    // Derive the client IP (for load-balancer stickiness) and the verified direct peer before the
    // request is decomposed. `client_ip` already honours `X-Forwarded-For` only from trusted peers.
    let client_ip = crate::server::auth_layer::client_ip(&req, &state.trusted_proxies);
    let peer_ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip());
    let peer_trusted = peer_ip.is_some_and(|ip| state.trusted_proxies.contains(&ip));

    let (parts, body) = req.into_parts();
    let method = parts.method.clone();
    let path = parts.uri.path().to_string();
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());

    // Original content type governs which body transform (if any) may apply.
    let content_type = parts
        .headers
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    // Buffer the request body ONCE so retries can resend it. Bounded by the configured limit;
    // an oversized or unreadable body is a client error.
    let buffered = match axum::body::to_bytes(body, state.config.max_body_size).await {
        Ok(bytes) => bytes,
        Err(_err) => {
            return GatewayError::InvalidRequest(
                "request body exceeds the configured limit or could not be read".to_string(),
            )
            .into_response();
        }
    };

    // Build the forwarded header set: drop hop-by-hop headers, the inbound Host (replaced per
    // backend), Content-Length (hyper re-derives it from the buffered body) and the inbound
    // X-Forwarded-For (rebuilt below so a client cannot inject a forged chain).
    let mut forward_headers = HeaderMap::new();
    for (name, value) in parts.headers.iter() {
        let lower = name.as_str();
        if is_hop_by_hop(lower)
            || lower.eq_ignore_ascii_case("host")
            || lower.eq_ignore_ascii_case("content-length")
            || lower.eq_ignore_ascii_case("x-forwarded-for")
        {
            continue;
        }
        forward_headers.append(name.clone(), value.clone());
    }

    // Rebuild X-Forwarded-For. The inbound chain is only trustworthy when the direct peer is itself
    // a trusted proxy; otherwise it is dropped (a client could have forged it). The verified direct
    // peer IP is always appended when known, so an untrusted client contributes exactly its real
    // peer address and nothing more.
    let inbound_xff = if peer_trusted {
        parts
            .headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
    } else {
        None
    };
    if let Some(peer) = peer_ip {
        let chain = append_forwarded_for(inbound_xff, &peer.to_string());
        if let Ok(value) = HeaderValue::from_str(&chain) {
            forward_headers.insert(HeaderName::from_static("x-forwarded-for"), value);
        }
    }

    // Optional request transformation (upstream-bound request only).
    let mut forward_body = buffered;
    if let Some(engine) = state.transform.as_deref() {
        let mut header_map = header_map_to_hashmap(&forward_headers);
        if let Err(err) = engine.transform_request_headers(&path, method.as_str(), &mut header_map)
        {
            return err.into_response();
        }
        forward_headers = hashmap_to_header_map(&header_map);

        // Ignore any `; charset=…` parameter when mapping the MIME type.
        let mapped = content_type
            .as_deref()
            .map(|value| value.split(';').next().unwrap_or(value).trim())
            .and_then(ContentType::from_mime);
        if let Some(content_type) = mapped {
            match engine.transform_request_body(
                &path,
                method.as_str(),
                forward_body.to_vec(),
                content_type,
            ) {
                Ok(new_body) => forward_body = Bytes::from(new_body),
                Err(err) => return err.into_response(),
            }
        }
    }

    // Forward with cross-backend failover retry. The selector re-picks a backend each attempt
    // (open circuits are filtered out by `select_backend`); the operation forwards once.
    let failover = std::sync::Arc::clone(&state.failover);
    let result = failover
        .execute_with_retry(
            || state.load_balancer.select_backend(client_ip.as_deref()),
            |backend| {
                forward_once(
                    &state,
                    backend,
                    &method,
                    path_and_query.as_str(),
                    &forward_headers,
                    &forward_body,
                )
            },
        )
        .await;

    match result {
        Ok(response) => response,
        Err(err) => err.into_response(),
    }
}

/// Forwards a single request to `backend`, bounded by the configured per-attempt timeout, and
/// classifies the outcome for the circuit breaker.
///
/// - A timeout or transport error records a backend failure and returns an error (which the
///   failover manager may retry).
/// - A `5xx` response records a failure but is still returned to the caller.
/// - Any other response records a success.
async fn forward_once(
    state: &GatewayState,
    backend: Backend,
    method: &Method,
    path_and_query: &str,
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<Response> {
    let timeout = Duration::from_secs(state.config.request_timeout);
    let backend_id = backend.id.clone();

    let outcome = tokio::time::timeout(
        timeout,
        do_forward(&backend, method, path_and_query, headers, body),
    )
    .await;

    match outcome {
        // The whole connect+send exceeded the per-attempt budget.
        Err(_elapsed) => {
            state.load_balancer.record_failure(&backend_id);
            Err(GatewayError::Timeout(format!(
                "upstream '{backend_id}' timed out after {}s",
                timeout.as_secs()
            )))
        }
        // Connect / TLS / handshake / send failure.
        Ok(Err(err)) => {
            state.load_balancer.record_failure(&backend_id);
            Err(err)
        }
        // A complete HTTP response was received.
        Ok(Ok(response)) => {
            let (mut response_parts, incoming) = response.into_parts();
            let status = response_parts.status;
            strip_hop_by_hop(&mut response_parts.headers);

            if status.as_u16() >= 500 {
                // Record the failure but still hand the 5xx body back to the client rather than
                // retrying it through the failover manager.
                state.load_balancer.record_failure(&backend_id);
            } else {
                state.load_balancer.record_success(&backend_id);
            }

            // Stream the response body straight through without buffering.
            let body = axum::body::Body::new(incoming);
            Ok(http::Response::from_parts(response_parts, body))
        }
    }
}

/// Opens a connection to the backend, sends the buffered request and returns the raw upstream
/// response. `http://` backends use a plain TCP stream; `https://` backends are wrapped in the
/// crate's shared Pure-Rust rustls connector.
async fn do_forward(
    backend: &Backend,
    method: &Method,
    path_and_query: &str,
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<http::Response<Incoming>> {
    let target = parse_backend(&backend.url)?;

    // Origin-form URI (`/path?query`) with an explicit Host header is the correct shape for a
    // direct (non-proxy) hyper client connection.
    let uri: Uri = path_and_query.parse().map_err(|err| {
        GatewayError::InvalidRequest(format!(
            "invalid upstream request target '{path_and_query}': {err}"
        ))
    })?;

    let mut builder = http::Request::builder().method(method.clone()).uri(uri);
    if let Some(request_headers) = builder.headers_mut() {
        for (name, value) in headers.iter() {
            request_headers.append(name.clone(), value.clone());
        }
        if let Ok(host) = HeaderValue::from_str(&target.authority) {
            request_headers.insert(http::header::HOST, host);
        }
    }
    let request = builder.body(Full::new(body.clone())).map_err(|err| {
        GatewayError::InvalidRequest(format!("failed to build upstream request: {err}"))
    })?;

    let addr = format!("{}:{}", target.host, target.port);
    let tcp = tokio::net::TcpStream::connect(&addr).await.map_err(|err| {
        GatewayError::BackendUnavailable(format!("connect to {addr} failed: {err}"))
    })?;

    if target.tls {
        let connector = crate::loadbalancer::probe::tls_connector()?;
        // Own the host string for the TLS server name (rustls requires an owned/'static name).
        let server_name = rustls::pki_types::ServerName::try_from(target.host.clone())
            .map_err(|err| GatewayError::HttpError(format!("invalid TLS server name: {err}")))?;
        let stream = connector.connect(server_name, tcp).await.map_err(|err| {
            GatewayError::BackendUnavailable(format!("TLS handshake to {addr} failed: {err}"))
        })?;
        send_over(stream, request).await
    } else {
        send_over(tcp, request).await
    }
}

/// Drives a hyper 1 HTTP/1 client connection over `stream` for a single request, spawning the
/// connection driver so the response body can stream after the head is received.
async fn send_over<S>(
    stream: S,
    request: http::Request<Full<Bytes>>,
) -> Result<http::Response<Incoming>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(|err| {
            GatewayError::BackendUnavailable(format!("upstream handshake failed: {err}"))
        })?;

    // Drive the connection in the background; it lives as long as the streaming response body.
    tokio::spawn(async move {
        if let Err(err) = conn.await {
            tracing::debug!("upstream connection driver ended: {err}");
        }
    });

    sender
        .send_request(request)
        .await
        .map_err(|err| GatewayError::BackendUnavailable(format!("upstream request failed: {err}")))
}

/// Parsed pieces of a backend base URL.
struct UpstreamTarget {
    /// Whether the upstream scheme is `https`.
    tls: bool,
    /// Upstream host (no port).
    host: String,
    /// Upstream port (defaulted from the scheme when absent).
    port: u16,
    /// `Host` header authority (`host` alone for the default port, otherwise `host:port`).
    authority: String,
}

/// Parses a backend base URL into its scheme/host/port and the `Host` authority to send.
fn parse_backend(backend_url: &str) -> Result<UpstreamTarget> {
    let parsed = url::Url::parse(backend_url).map_err(|err| {
        GatewayError::LoadBalancerError(format!("invalid backend url '{backend_url}': {err}"))
    })?;

    let tls = match parsed.scheme() {
        "http" => false,
        "https" => true,
        other => {
            return Err(GatewayError::LoadBalancerError(format!(
                "unsupported backend scheme '{other}' in '{backend_url}'"
            )));
        }
    };

    let host = parsed
        .host_str()
        .ok_or_else(|| {
            GatewayError::LoadBalancerError(format!("backend url '{backend_url}' has no host"))
        })?
        .to_string();

    let port = parsed.port().unwrap_or(if tls { 443 } else { 80 });

    let authority = if (tls && port == 443) || (!tls && port == 80) {
        host.clone()
    } else {
        format!("{host}:{port}")
    };

    Ok(UpstreamTarget {
        tls,
        host,
        port,
        authority,
    })
}

/// Hop-by-hop header names that must not be forwarded (RFC 7230 §6.1).
const HOP_BY_HOP: [&str; 8] = [
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// Returns `true` when `name` (case-insensitively) is a hop-by-hop header.
fn is_hop_by_hop(name: &str) -> bool {
    HOP_BY_HOP.iter().any(|hop| name.eq_ignore_ascii_case(hop))
}

/// Removes hop-by-hop headers from `headers`, including any additional header names declared in a
/// `Connection` header value.
fn strip_hop_by_hop(headers: &mut HeaderMap) {
    // Extra hop-by-hop names named by the Connection header (per RFC 7230).
    let mut extra: Vec<String> = Vec::new();
    for value in headers.get_all(http::header::CONNECTION).iter() {
        if let Ok(text) = value.to_str() {
            for token in text.split(',') {
                let token = token.trim();
                if !token.is_empty() {
                    extra.push(token.to_ascii_lowercase());
                }
            }
        }
    }

    let to_remove: Vec<HeaderName> = headers
        .keys()
        .filter(|name| {
            let name = name.as_str();
            is_hop_by_hop(name) || extra.iter().any(|hop| hop.eq_ignore_ascii_case(name))
        })
        .cloned()
        .collect();

    for name in to_remove {
        headers.remove(&name);
    }
}

/// Appends `client_ip` to an existing `X-Forwarded-For` chain, or starts a new one.
fn append_forwarded_for(existing: Option<&str>, client_ip: &str) -> String {
    match existing {
        Some(previous) if !previous.trim().is_empty() => format!("{previous}, {client_ip}"),
        _ => client_ip.to_string(),
    }
}

/// Converts an `http` header map into the `HashMap<String, String>` the transform engine speaks.
///
/// Header names are the (lower-cased) `http` canonical form; the last value wins for a repeated
/// header because the transform DTO is single-valued.
fn header_map_to_hashmap(headers: &HeaderMap) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for (name, value) in headers.iter() {
        if let Ok(text) = value.to_str() {
            map.insert(name.as_str().to_string(), text.to_string());
        }
    }
    map
}

/// Rebuilds an `http` header map from the transform engine's `HashMap`, skipping any entry whose
/// name or value is not a valid HTTP header (never panics on malformed transform output).
fn hashmap_to_header_map(map: &HashMap<String, String>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in map {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            headers.insert(name, value);
        }
    }
    headers
}

/// Builds the `404 NO_ROUTE` JSON response returned when no backends are configured.
fn no_route_response() -> Response {
    let body = serde_json::json!({
        "error": {
            "code": "NO_ROUTE",
            "message": "no route matched and no upstream backends are configured"
        }
    });
    (StatusCode::NOT_FOUND, axum::Json(body)).into_response()
}

/// Minimal adapter bridging a Tokio `AsyncRead`/`AsyncWrite` stream to hyper's
/// [`hyper::rt::Read`]/[`hyper::rt::Write`] traits.
///
/// This lets the reverse proxy drive a hyper 1 client connection without depending on
/// `hyper-util`. The wrapped stream must be `Unpin` (both `tokio::net::TcpStream` and
/// `tokio_rustls::client::TlsStream` over it are), which keeps the pin projection safe.
struct TokioIo<T> {
    /// The wrapped Tokio stream.
    inner: T,
}

impl<T> TokioIo<T> {
    /// Wraps a Tokio stream for use as a hyper IO object.
    fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T> hyper::rt::Read for TokioIo<T>
where
    T: tokio::io::AsyncRead + Unpin,
{
    #[allow(unsafe_code)]
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        // SAFETY: this is the standard, sound TokioIo bridge. `as_mut` exposes hyper's
        // uninitialised tail as `&mut [MaybeUninit<u8>]`, which Tokio's `ReadBuf::uninit` fills;
        // we then `advance` the cursor by exactly the number of bytes Tokio reported as filled,
        // so no uninitialised byte is ever claimed as initialised.
        let filled = unsafe {
            let mut read_buf = tokio::io::ReadBuf::uninit(buf.as_mut());
            match tokio::io::AsyncRead::poll_read(Pin::new(&mut this.inner), cx, &mut read_buf) {
                Poll::Ready(Ok(())) => read_buf.filled().len(),
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Pending => return Poll::Pending,
            }
        };
        // SAFETY: `filled` bytes were just initialised by the reader above.
        unsafe {
            buf.advance(filled);
        }
        Poll::Ready(Ok(()))
    }
}

impl<T> hyper::rt::Write for TokioIo<T>
where
    T: tokio::io::AsyncWrite + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        tokio::io::AsyncWrite::poll_write(Pin::new(&mut this.inner), cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        tokio::io::AsyncWrite::poll_flush(Pin::new(&mut this.inner), cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        tokio::io::AsyncWrite::poll_shutdown(Pin::new(&mut this.inner), cx)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::GatewayConfig;
    use crate::server::state::{BuilderOptions, GatewayState};

    #[test]
    fn hop_by_hop_helper_classifies_names() {
        assert!(is_hop_by_hop("Connection"));
        assert!(is_hop_by_hop("connection"));
        assert!(is_hop_by_hop("Transfer-Encoding"));
        assert!(is_hop_by_hop("Upgrade"));
        assert!(is_hop_by_hop("proxy-authorization"));
        assert!(!is_hop_by_hop("Content-Type"));
        assert!(!is_hop_by_hop("X-Forwarded-For"));
        assert!(!is_hop_by_hop("Authorization"));
    }

    #[test]
    fn strip_hop_by_hop_removes_fixed_list_and_connection_tokens() {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::CONNECTION,
            HeaderValue::from_static("keep-alive, X-Custom"),
        );
        headers.insert(
            http::header::TRANSFER_ENCODING,
            HeaderValue::from_static("chunked"),
        );
        headers.insert(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        headers.insert(
            HeaderName::from_static("x-custom"),
            HeaderValue::from_static("v"),
        );
        headers.insert(
            HeaderName::from_static("x-keep"),
            HeaderValue::from_static("v"),
        );

        strip_hop_by_hop(&mut headers);

        // Fixed-list hop-by-hop headers are gone.
        assert!(!headers.contains_key(http::header::CONNECTION));
        assert!(!headers.contains_key(http::header::TRANSFER_ENCODING));
        // The header named by the Connection value is also stripped.
        assert!(!headers.contains_key(HeaderName::from_static("x-custom")));
        // End-to-end headers survive.
        assert!(headers.contains_key(http::header::CONTENT_TYPE));
        assert!(headers.contains_key(HeaderName::from_static("x-keep")));
    }

    #[test]
    fn parse_backend_extracts_scheme_host_port_authority() {
        let http = parse_backend("http://localhost:8080").expect("valid http url");
        assert!(!http.tls);
        assert_eq!(http.host, "localhost");
        assert_eq!(http.port, 8080);
        assert_eq!(http.authority, "localhost:8080");

        let https = parse_backend("https://example.com").expect("valid https url");
        assert!(https.tls);
        assert_eq!(https.host, "example.com");
        assert_eq!(https.port, 443);
        // Default port is omitted from the authority.
        assert_eq!(https.authority, "example.com");

        let default_http = parse_backend("http://example.com:80").expect("valid http url");
        assert_eq!(default_http.authority, "example.com");

        assert!(parse_backend("ftp://example.com").is_err());
        assert!(parse_backend("not a url").is_err());
    }

    #[test]
    fn upstream_uri_built_from_backend_and_path_query() {
        // The backend supplies the authority; the original request supplies path + query.
        let target = parse_backend("http://localhost:8080").expect("valid url");
        let uri: Uri = "/api/data?limit=5".parse().expect("valid origin-form uri");

        assert_eq!(target.authority, "localhost:8080");
        assert_eq!(uri.path(), "/api/data");
        assert_eq!(uri.query(), Some("limit=5"));
    }

    #[test]
    fn forwarded_for_append_logic() {
        assert_eq!(
            append_forwarded_for(Some("1.2.3.4"), "5.6.7.8"),
            "1.2.3.4, 5.6.7.8"
        );
        assert_eq!(append_forwarded_for(None, "5.6.7.8"), "5.6.7.8");
        assert_eq!(append_forwarded_for(Some(""), "5.6.7.8"), "5.6.7.8");
        assert_eq!(append_forwarded_for(Some("   "), "5.6.7.8"), "5.6.7.8");
    }

    #[test]
    fn header_hashmap_round_trip_preserves_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        headers.insert(
            HeaderName::from_static("x-custom"),
            HeaderValue::from_static("value"),
        );

        let map = header_map_to_hashmap(&headers);
        assert_eq!(
            map.get("content-type").map(String::as_str),
            Some("application/json")
        );
        assert_eq!(map.get("x-custom").map(String::as_str), Some("value"));

        let rebuilt = hashmap_to_header_map(&map);
        assert_eq!(
            rebuilt.get("content-type").and_then(|v| v.to_str().ok()),
            Some("application/json")
        );
        assert_eq!(
            rebuilt.get("x-custom").and_then(|v| v.to_str().ok()),
            Some("value")
        );
    }

    #[tokio::test]
    async fn no_backends_yields_404_no_route() {
        let state = GatewayState::build(GatewayConfig::default(), BuilderOptions::default())
            .expect("state builds");
        let request = axum::http::Request::builder()
            .uri("/some/unmatched/path")
            .body(axum::body::Body::empty())
            .expect("request builds");

        let response = proxy_fallback(State(state), request).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("collect body");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("NO_ROUTE"), "body was: {text}");
    }
}
