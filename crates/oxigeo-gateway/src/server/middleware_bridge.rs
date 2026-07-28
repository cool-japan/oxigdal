//! Bridge between the axum serving layer and the crate's in-house [`MiddlewareChain`].
//!
//! The in-house middleware trait operates on fully-buffered, case-sensitive
//! [`crate::middleware::Request`] / [`crate::middleware::Response`] structs rather than
//! `http::Request<Body>` / `axum::body::Body`. This module implements a single axum middleware
//! function, [`middleware_bridge`], that adapts between the two worlds:
//!
//! 1. Upgrade / WebSocket requests pass straight through without buffering (they cannot be
//!    materialised into a `Vec<u8>` body).
//! 2. CORS preflight (`OPTIONS`) requests are answered directly via [`preflight_response`],
//!    mirroring the in-house `CorsMiddleware` origin logic.
//! 3. Cacheable `GET` requests are short-circuited from the caching middleware's store, decorated
//!    with an `X-Cache: HIT` header and run through `process_response` so CORS/compression still
//!    apply.
//! 4. Everything else has its body buffered (bounded by `max_body_size`, `413` on overflow),
//!    converted into an in-house request, run through `process_request`, forwarded to the inner
//!    router, then its response buffered and run through `process_response` before conversion
//!    back to an axum response.
//!
//! Header names are canonicalised on the way in via [`canonical_header_name`] so the in-house
//! middleware -- which looks names up with exact `String` keys -- keeps working under HTTP/2
//! header lowercasing.

use std::collections::HashMap;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use http_body_util::BodyExt;

use crate::error::{GatewayError, Result};
use crate::middleware::cors::CorsConfig;
use crate::middleware::{Request as InRequest, Response as InResponse};

use super::state::GatewayState;

/// Axum middleware that runs each request/response through the in-house [`MiddlewareChain`].
///
/// See the module documentation for the exact flow ordering. Upgrade and WebSocket requests are
/// passed through untouched; all other requests are fully buffered so the trait-based middleware
/// can operate on them.
///
/// [`MiddlewareChain`]: crate::middleware::MiddlewareChain
pub(crate) async fn middleware_bridge(
    State(state): State<GatewayState>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    let method = req.method().clone();

    // 1. Upgrade / WebSocket pass-through: these bodies cannot be buffered.
    if is_upgrade_or_ws(&req, &path) {
        return next.run(req).await;
    }

    // 2. CORS preflight synthesis. The in-house CorsMiddleware never answers OPTIONS itself, so we
    //    synthesise the 204 response here when the request looks like a real preflight. When the
    //    origin is not allowed the chain fails and we fall through to normal processing (which will
    //    404/405 naturally).
    if method == Method::OPTIONS
        && state.config.middleware.enable_cors
        && req.headers().contains_key(header::ORIGIN)
        && req
            .headers()
            .contains_key(header::ACCESS_CONTROL_REQUEST_METHOD)
        && let Some(origin) = req
            .headers()
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok())
        && let Some(response) = preflight_response(&state.config.middleware.cors, origin)
    {
        return response;
    }

    // 3. Cache short-circuit for GET requests when caching is enabled.
    if method == Method::GET
        && let Some(caching) = state.caching.as_ref()
        && let Some(mut hit) = caching.lookup("GET", &path)
    {
        hit.headers.insert("X-Cache".to_string(), "HIT".to_string());
        let inhouse_req = InRequest {
            method: "GET".to_string(),
            path: path.clone(),
            headers: canonical_headers(req.headers()),
            body: Vec::new(),
        };
        // Run the cached body through process_response FIRST so CORS/compression headers
        // are applied against the real inbound request.
        let decorated = match state
            .middleware_chain
            .process_response(&inhouse_req, hit)
            .await
        {
            Ok(response) => response,
            Err(error) => return error.into_response(),
        };
        return inhouse_response_to_axum(decorated);
    }

    // 4. Buffer the request body (bounded); a body larger than the configured limit is a 413.
    let max = state.config.max_body_size;
    let (parts, body) = req.into_parts();
    let bytes = match axum::body::to_bytes(body, max).await {
        Ok(bytes) => bytes,
        Err(_) => return payload_too_large(),
    };

    let method_str = parts.method.as_str().to_string();
    let inhouse_req = InRequest {
        method: method_str.clone(),
        path: path.clone(),
        headers: canonical_headers(&parts.headers),
        body: bytes.to_vec(),
    };

    // 5. process_request, keeping the (possibly mutated) request for the response side.
    let processed = match state.middleware_chain.process_request(inhouse_req).await {
        Ok(request) => request,
        Err(error) => return error.into_response(),
    };

    // Rebuild the axum request, preserving the original URI (query string) and extensions so
    // AuthContext / VersionContext inserted by earlier layers survive.
    let rebuilt = match rebuild_axum_request(&parts, &processed) {
        Ok(request) => request,
        Err(error) => return error.into_response(),
    };

    // 6. Run the inner router, then buffer the response (or pass it straight through when it is
    //    larger than the configured limit).
    let response = next.run(rebuilt).await;
    let (resp_parts, resp_body) = response.into_parts();
    let (resp_parts, body_bytes) = match buffer_response(resp_parts, resp_body, max).await {
        BufferOutcome::PassThrough(response) => return response,
        BufferOutcome::Buffered { parts, body } => (parts, body),
    };

    // 7. Build the in-house response and run it through process_response with the SAME request.
    let inhouse_resp = InResponse {
        status: resp_parts.status.as_u16(),
        headers: canonical_headers(&resp_parts.headers),
        body: body_bytes.to_vec(),
    };
    let mut processed_resp = match state
        .middleware_chain
        .process_response(&processed, inhouse_resp)
        .await
    {
        Ok(response) => response,
        Err(error) => return error.into_response(),
    };

    // A cache miss on a cacheable method is surfaced with X-Cache: MISS on the way out.
    if state.caching.is_some() && method_str == "GET" {
        processed_resp
            .headers
            .entry("X-Cache".to_string())
            .or_insert_with(|| "MISS".to_string());
    }

    // 8. Convert back to an axum response, skipping unparseable header pairs.
    inhouse_response_to_axum(processed_resp)
}

/// Whether the request must bypass buffering: an HTTP upgrade (WebSocket handshake) or a
/// WebSocket / GraphQL-subscription route whose body must remain a live stream.
fn is_upgrade_or_ws(req: &Request, path: &str) -> bool {
    if path == "/ws" || path.starts_with("/graphql/ws") {
        return true;
    }
    if req.headers().contains_key(header::UPGRADE) {
        return true;
    }
    req.headers()
        .get(header::CONNECTION)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_ascii_lowercase().contains("upgrade"))
        .unwrap_or(false)
}

/// Synthesises a CORS preflight (`204 No Content`) response for `origin` against `cors`.
///
/// Mirrors the in-house `CorsMiddleware` origin logic: the exact origin is echoed unless the
/// allow-list is the bare wildcard and credentials are disabled (in which case a literal `*` is
/// emitted). Returns `None` when `origin` is not permitted, letting the caller fall through to
/// normal request processing.
pub(crate) fn preflight_response(cors: &CorsConfig, origin: &str) -> Option<Response> {
    let wildcard = cors.allowed_origins.iter().any(|allowed| allowed == "*");
    let allowed = wildcard || cors.allowed_origins.iter().any(|allowed| allowed == origin);
    if !allowed {
        return None;
    }

    // Wildcard + credentials is forbidden by the Fetch spec, so echo the exact origin whenever
    // credentials are enabled.
    let allow_origin = if wildcard && !cors.allow_credentials {
        "*".to_string()
    } else {
        origin.to_string()
    };

    let mut builder = Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, allow_origin)
        .header(header::VARY, "Origin");

    if !cors.allowed_methods.is_empty() {
        builder = builder.header(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            cors.allowed_methods.join(", "),
        );
    }
    if !cors.allowed_headers.is_empty() {
        builder = builder.header(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            cors.allowed_headers.join(", "),
        );
    }
    if cors.allow_credentials {
        builder = builder.header(header::ACCESS_CONTROL_ALLOW_CREDENTIALS, "true");
    }
    builder = builder.header(header::ACCESS_CONTROL_MAX_AGE, cors.max_age.to_string());

    builder.body(Body::empty()).ok()
}

/// Canonicalises an HTTP header name into the exact casing the in-house middleware expects.
///
/// Most names are produced by title-casing each hyphen-separated segment (`origin` -> `Origin`,
/// `content-length` -> `Content-Length`). A small override table covers the names whose canonical
/// casing is not plain title-case: `X-Request-ID`, `ETag`, and `If-None-Match`.
pub(crate) fn canonical_header_name(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "x-request-id" => return "X-Request-ID".to_string(),
        "etag" => return "ETag".to_string(),
        "if-none-match" => return "If-None-Match".to_string(),
        _ => {}
    }

    lower
        .split('-')
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}

/// Converts an [`http::HeaderMap`] into the in-house case-sensitive header map.
///
/// Header values that are not valid visible ASCII are dropped rather than panicking. When a name
/// appears multiple times only the last value survives (the in-house model stores one value per
/// name).
///
/// [`http::HeaderMap`]: axum::http::HeaderMap
fn canonical_headers(map: &axum::http::HeaderMap) -> HashMap<String, String> {
    let mut out = HashMap::with_capacity(map.len());
    for (name, value) in map.iter() {
        if let Ok(value) = value.to_str() {
            out.insert(canonical_header_name(name.as_str()), value.to_string());
        }
    }
    out
}

/// Whether `name` is a length/framing header that must be recomputed by the transport rather than
/// copied verbatim (a stale `Content-Length` or `Transfer-Encoding` would conflict with the fixed
/// buffered body we rebuild from).
fn is_length_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("content-length") || name.eq_ignore_ascii_case("transfer-encoding")
}

/// Rebuilds an axum request from the original parts and the post-`process_request` in-house
/// request, preserving the original URI (including query string) and all request extensions.
fn rebuild_axum_request(
    parts: &axum::http::request::Parts,
    processed: &InRequest,
) -> Result<Request> {
    let mut builder = axum::http::Request::builder()
        .method(parts.method.clone())
        .uri(parts.uri.clone())
        .version(parts.version);

    for (name, value) in &processed.headers {
        if is_length_header(name) {
            continue;
        }
        if let (Ok(header_name), Ok(header_value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            builder = builder.header(header_name, header_value);
        }
    }

    let mut request = builder
        .body(Body::from(processed.body.clone()))
        .map_err(|error| {
            GatewayError::InternalError(format!("failed to rebuild request: {error}"))
        })?;
    *request.extensions_mut() = parts.extensions.clone();
    Ok(request)
}

/// Converts an in-house [`InResponse`] back into an axum response.
///
/// Header names/values that cannot be parsed are skipped (never panicking). Length/framing headers
/// are dropped so the transport recomputes `Content-Length` from the fixed body.
fn inhouse_response_to_axum(response: InResponse) -> Response {
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut builder = Response::builder().status(status);

    for (name, value) in &response.headers {
        if is_length_header(name) {
            continue;
        }
        if let (Ok(header_name), Ok(header_value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            builder = builder.header(header_name, header_value);
        }
    }

    match builder.body(Body::from(response.body)) {
        Ok(response) => response,
        Err(_) => {
            GatewayError::InternalError("failed to build response".to_string()).into_response()
        }
    }
}

/// Reads the declared `Content-Length` of a header map, if present and parseable.
fn content_length(headers: &axum::http::HeaderMap) -> Option<u64> {
    headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
}

/// Outcome of buffering a response body for in-house middleware processing.
enum BufferOutcome {
    /// The response must be returned as-is (too large to buffer, or unrecoverable), skipping
    /// response-side middleware.
    PassThrough(Response),
    /// The body was buffered and is ready for the in-house middleware chain.
    Buffered {
        /// The response head (status, headers, version).
        parts: axum::http::response::Parts,
        /// The fully buffered body.
        body: Bytes,
    },
}

/// Buffers a response body up to `max` bytes.
///
/// When the declared `Content-Length` already exceeds `max`, the response is streamed straight
/// through unbuffered ([`BufferOutcome::PassThrough`]) so oversized bodies are never materialised.
/// A body without a declared length that turns out larger than `max` is likewise passed through
/// after collection. Buffered bodies are returned for middleware processing.
async fn buffer_response(
    parts: axum::http::response::Parts,
    body: Body,
    max: usize,
) -> BufferOutcome {
    if content_length(&parts.headers)
        .map(|len| len > max as u64)
        .unwrap_or(false)
    {
        return BufferOutcome::PassThrough(Response::from_parts(parts, body));
    }

    match body.collect().await {
        Ok(collected) => {
            let bytes = collected.to_bytes();
            if bytes.len() > max {
                BufferOutcome::PassThrough(Response::from_parts(parts, Body::from(bytes)))
            } else {
                BufferOutcome::Buffered { parts, body: bytes }
            }
        }
        Err(_) => BufferOutcome::PassThrough(
            GatewayError::InternalError("failed to buffer upstream response body".to_string())
                .into_response(),
        ),
    }
}

/// Builds a `413 Payload Too Large` JSON response for a request body over `max_body_size`.
fn payload_too_large() -> Response {
    let body = serde_json::json!({
        "error": {
            "code": "PAYLOAD_TOO_LARGE",
            "message": "request body exceeds the maximum allowed size",
            "status": 413,
            "retryable": false,
        }
    });
    (StatusCode::PAYLOAD_TOO_LARGE, axum::Json(body)).into_response()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct Marker(u32);

    #[test]
    fn canonical_header_name_uses_override_table_then_title_case() {
        // Override table entries.
        assert_eq!(canonical_header_name("x-request-id"), "X-Request-ID");
        assert_eq!(canonical_header_name("X-Request-Id"), "X-Request-ID");
        assert_eq!(canonical_header_name("etag"), "ETag");
        assert_eq!(canonical_header_name("ETAG"), "ETag");
        assert_eq!(canonical_header_name("if-none-match"), "If-None-Match");

        // Title-case fallback covering names the in-house middleware looks up.
        assert_eq!(canonical_header_name("origin"), "Origin");
        assert_eq!(canonical_header_name("accept"), "Accept");
        assert_eq!(canonical_header_name("ACCEPT"), "Accept");
        assert_eq!(canonical_header_name("content-length"), "Content-Length");
        assert_eq!(canonical_header_name("content-type"), "Content-Type");
        assert_eq!(canonical_header_name("accept-encoding"), "Accept-Encoding");
        assert_eq!(canonical_header_name("cache-control"), "Cache-Control");
    }

    #[test]
    fn preflight_echoes_allowed_origin() {
        let cors = CorsConfig {
            allowed_origins: vec!["https://a.example".to_string()],
            ..CorsConfig::default()
        };
        let response = preflight_response(&cors, "https://a.example").expect("origin allowed");

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some("https://a.example")
        );
        assert_eq!(
            response
                .headers()
                .get("vary")
                .and_then(|value| value.to_str().ok()),
            Some("Origin")
        );
    }

    #[test]
    fn preflight_denies_unlisted_origin() {
        let cors = CorsConfig {
            allowed_origins: vec!["https://a.example".to_string()],
            ..CorsConfig::default()
        };
        assert!(preflight_response(&cors, "https://evil.example").is_none());
    }

    #[test]
    fn preflight_wildcard_without_credentials_emits_star() {
        let cors = CorsConfig::default(); // wildcard origins, credentials off
        let response = preflight_response(&cors, "https://client.example")
            .expect("wildcard allows any origin");
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some("*")
        );
    }

    #[test]
    fn preflight_wildcard_with_credentials_echoes_origin() {
        let cors = CorsConfig {
            allow_credentials: true,
            ..CorsConfig::default()
        };
        let response = preflight_response(&cors, "https://client.example")
            .expect("wildcard allows any origin");
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some("https://client.example")
        );
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-credentials")
                .and_then(|value| value.to_str().ok()),
            Some("true")
        );
    }

    #[test]
    fn request_roundtrip_preserves_query_extension_and_headers() {
        let mut request = axum::http::Request::builder()
            .method("POST")
            .uri("/api/thing?a=1&b=2")
            .header("x-custom", "hello")
            .body(Body::from("payload"))
            .unwrap();
        request.extensions_mut().insert(Marker(42));

        let (parts, _body) = request.into_parts();
        let inhouse = InRequest {
            method: parts.method.as_str().to_string(),
            path: parts.uri.path().to_string(),
            headers: canonical_headers(&parts.headers),
            body: b"payload".to_vec(),
        };

        // The in-house header map must carry the canonicalised name.
        assert_eq!(inhouse.headers.get("X-Custom"), Some(&"hello".to_string()));

        let rebuilt = rebuild_axum_request(&parts, &inhouse).expect("rebuild succeeds");

        // Query string survives the roundtrip.
        assert_eq!(rebuilt.uri().query(), Some("a=1&b=2"));
        assert_eq!(rebuilt.uri().path(), "/api/thing");
        // Extensions survive the roundtrip.
        assert_eq!(rebuilt.extensions().get::<Marker>(), Some(&Marker(42)));
        // Custom header survives (case-insensitive lookup).
        assert_eq!(
            rebuilt
                .headers()
                .get("x-custom")
                .and_then(|value| value.to_str().ok()),
            Some("hello")
        );
    }

    #[test]
    fn inhouse_response_conversion_skips_length_headers() {
        let mut headers = HashMap::new();
        headers.insert("X-Custom".to_string(), "value".to_string());
        headers.insert("Content-Length".to_string(), "999".to_string());
        headers.insert("Transfer-Encoding".to_string(), "chunked".to_string());
        let response = InResponse {
            status: 201,
            headers,
            body: b"body".to_vec(),
        };

        let axum_response = inhouse_response_to_axum(response);
        assert_eq!(axum_response.status(), StatusCode::CREATED);
        assert_eq!(
            axum_response
                .headers()
                .get("x-custom")
                .and_then(|value| value.to_str().ok()),
            Some("value")
        );
        // Stale length/framing headers are dropped so the transport recomputes them.
        assert!(axum_response.headers().get("content-length").is_none());
        assert!(axum_response.headers().get("transfer-encoding").is_none());
    }

    #[tokio::test]
    async fn oversized_response_passes_through_unbuffered() {
        let response = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_LENGTH, "1000000")
            .header("x-marker", "keep")
            .body(Body::from("small-body"))
            .unwrap();
        let (parts, body) = response.into_parts();

        match buffer_response(parts, body, 16).await {
            BufferOutcome::PassThrough(response) => {
                assert_eq!(response.status(), StatusCode::OK);
                assert_eq!(
                    response
                        .headers()
                        .get("x-marker")
                        .and_then(|value| value.to_str().ok()),
                    Some("keep")
                );
            }
            BufferOutcome::Buffered { .. } => panic!("expected oversized response to pass through"),
        }
    }

    #[tokio::test]
    async fn small_response_is_buffered() {
        let response = Response::builder()
            .status(StatusCode::OK)
            .body(Body::from("hello"))
            .unwrap();
        let (parts, body) = response.into_parts();

        match buffer_response(parts, body, 1024).await {
            BufferOutcome::Buffered { body, .. } => assert_eq!(&body[..], b"hello"),
            BufferOutcome::PassThrough(_) => panic!("expected small response to be buffered"),
        }
    }
}
