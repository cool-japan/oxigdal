//! Integration tests for `HttpObjectStore` HTTP transport.
//!
//! Each test spins up a minimal in-process TCP server that speaks just enough
//! HTTP/1.1 to exercise the client implementation, without requiring any
//! external network access.

#![cfg(feature = "cloud-http")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use oxigeo_streaming::cloud::retry::RetryPolicy;
use oxigeo_streaming::cloud::{CloudCredentials, CloudError, HttpObjectStore, ObjectUrl};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

// ─────────────────────────────────────────────────────────────────────────────
// Mock server helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Spawn a TCP mock server that responds to up to `max_connections` requests
/// with a fixed, static byte slice.  Returns the bound port.
///
/// Each accepted connection reads (and discards) the request, then writes the
/// provided `response_bytes` verbatim before closing.
fn spawn_mock_server_static(max_connections: usize, response_bytes: &'static [u8]) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind to loopback");
    let port = listener.local_addr().expect("local_addr").port();

    thread::spawn(move || {
        for stream in listener.incoming().take(max_connections) {
            thread::spawn(move || {
                let mut stream = stream.expect("accept tcp stream");
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .ok();
                // Consume the request without parsing it
                let mut buf = [0u8; 8192];
                let _ = stream.read(&mut buf).unwrap_or(0);
                stream.write_all(response_bytes).ok();
            });
        }
    });

    port
}

/// Spawn a mock server that captures the raw request bytes sent by the client
/// into `captured`, then responds with `response_bytes`.
fn spawn_mock_server_capturing(
    max_connections: usize,
    response_bytes: &'static [u8],
    captured: Arc<Mutex<Vec<u8>>>,
) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind to loopback");
    let port = listener.local_addr().expect("local_addr").port();

    thread::spawn(move || {
        for stream in listener.incoming().take(max_connections) {
            let captured_clone = Arc::clone(&captured);
            thread::spawn(move || {
                let mut stream = stream.expect("accept tcp stream");
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .ok();

                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                {
                    let mut guard = captured_clone.lock().expect("lock captured buffer");
                    guard.extend_from_slice(&buf[..n]);
                }
                stream.write_all(response_bytes).ok();
            });
        }
    });

    port
}

/// Spawn a mock server that serves different responses depending on how many
/// connections have already been accepted.  The `responses` slice is indexed
/// by connection number (0-based); the last element is repeated for any
/// additional connections.
fn spawn_mock_server_sequence(responses: Vec<&'static [u8]>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind to loopback");
    let port = listener.local_addr().expect("local_addr").port();
    let call_count = Arc::new(AtomicUsize::new(0));

    thread::spawn(move || {
        let max = responses.len();
        for stream in listener.incoming().take(max) {
            let idx = call_count.fetch_add(1, Ordering::SeqCst);
            let resp = responses[idx.min(max - 1)];
            thread::spawn(move || {
                let mut stream = stream.expect("accept");
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .ok();
                let mut buf = [0u8; 8192];
                let _ = stream.read(&mut buf).unwrap_or(0);
                stream.write_all(resp).ok();
            });
        }
    });

    port
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper: build an http:// ObjectUrl that points at our mock server
// ─────────────────────────────────────────────────────────────────────────────

fn mock_url(port: u16, path: &str) -> ObjectUrl {
    let raw = format!("http://127.0.0.1:{port}/{path}");
    ObjectUrl::parse(&raw).expect("parse mock url")
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

/// A 206 Partial Content response from the mock server should be transparently
/// returned as the raw body bytes.
#[tokio::test]
async fn test_http_object_store_get_range_returns_correct_bytes() {
    const BODY: &[u8] = b"partial-data-xyz";
    static RESPONSE_206: &[u8] = b"HTTP/1.1 206 Partial Content\r\n\
Content-Type: application/octet-stream\r\n\
Content-Length: 16\r\n\
Content-Range: bytes 0-15/1000\r\n\
\r\n\
partial-data-xyz";

    let port = spawn_mock_server_static(2, RESPONSE_206);
    let url = mock_url(port, "object.bin");

    // Zero-delay retry policy for fast tests
    let store = HttpObjectStore::with_retry(RetryPolicy::no_retry());
    let result = store
        .get_range(&url, 0, 15, &CloudCredentials::Anonymous)
        .await
        .expect("get_range should succeed on 206");

    assert_eq!(
        result.as_ref(),
        BODY,
        "body bytes should match the 206 payload"
    );
}

/// A 200 OK response from get_object should return the full body.
#[tokio::test]
async fn test_http_object_store_get_object_full_body() {
    const BODY: &[u8] = b"full-object-body-content";
    static RESPONSE_200: &[u8] = b"HTTP/1.1 200 OK\r\n\
Content-Type: application/octet-stream\r\n\
Content-Length: 24\r\n\
\r\n\
full-object-body-content";

    let port = spawn_mock_server_static(2, RESPONSE_200);
    let url = mock_url(port, "full.bin");

    let store = HttpObjectStore::with_retry(RetryPolicy::no_retry());
    let result = store
        .get_object(&url, &CloudCredentials::Anonymous)
        .await
        .expect("get_object should succeed on 200");

    assert_eq!(
        result.as_ref(),
        BODY,
        "body bytes should match the 200 payload"
    );
}

/// HEAD response with `Content-Length: 12345` should populate `metadata.size`.
#[tokio::test]
async fn test_http_object_store_head_extracts_content_length() {
    static RESPONSE_HEAD: &[u8] = b"HTTP/1.1 200 OK\r\n\
Content-Length: 12345\r\n\
Content-Type: image/tiff\r\n\
ETag: \"deadbeef\"\r\n\
Last-Modified: Mon, 12 Jan 2026 00:00:00 GMT\r\n\
\r\n";

    let port = spawn_mock_server_static(2, RESPONSE_HEAD);
    let url = mock_url(port, "raster.tiff");

    let store = HttpObjectStore::with_retry(RetryPolicy::no_retry());
    let metadata = store
        .head(&url, &CloudCredentials::Anonymous)
        .await
        .expect("head should succeed on 200");

    assert_eq!(
        metadata.size, 12_345,
        "size should be extracted from Content-Length"
    );
    assert_eq!(
        metadata.etag.as_deref(),
        Some("\"deadbeef\""),
        "etag should be extracted"
    );
    assert_eq!(
        metadata.content_type.as_deref(),
        Some("image/tiff"),
        "content_type should be extracted"
    );
    // Last-Modified: Mon, 12 Jan 2026 00:00:00 GMT → Unix timestamp
    // 2026-01-12 00:00:00 UTC
    assert!(
        metadata.last_modified.is_some(),
        "last_modified should be populated from Last-Modified header"
    );
    // Sanity: 2026-01-12 is after 2020-01-01 (1577836800) and before 2030-01-01 (1893456000)
    let ts = metadata.last_modified.expect("last_modified present");
    assert!(ts > 1_577_836_800, "timestamp should be after 2020-01-01");
    assert!(ts < 1_893_456_000, "timestamp should be before 2030-01-01");
}

/// A 429 response followed by a 200 response should cause the client to retry
/// and ultimately return the successful body.
#[tokio::test]
async fn test_http_object_store_429_retried() {
    static RESPONSE_429: &[u8] = b"HTTP/1.1 429 Too Many Requests\r\n\
Content-Length: 0\r\n\
\r\n";

    static RESPONSE_200: &[u8] = b"HTTP/1.1 200 OK\r\n\
Content-Type: application/octet-stream\r\n\
Content-Length: 7\r\n\
\r\n\
success";

    let port = spawn_mock_server_sequence(vec![RESPONSE_429, RESPONSE_200]);

    let url = mock_url(port, "data.bin");

    // Two attempts: first returns 429, second returns 200.
    let store = HttpObjectStore::with_retry(RetryPolicy {
        max_attempts: 2,
        initial_delay_ms: 0, // no delay in tests
        max_delay_ms: 0,
        backoff_multiplier: 1.0,
        jitter_factor: 0.0,
    });

    let result = store
        .get_object(&url, &CloudCredentials::Anonymous)
        .await
        .expect("get_object should succeed after retry on 429");

    assert_eq!(
        result.as_ref(),
        b"success",
        "body should come from the retried 200 response"
    );
}

/// When credentials are Anonymous, the client must not include an
/// `Authorization` header in the outgoing request.
#[tokio::test]
async fn test_http_object_store_anonymous_creds_sends_no_auth_header() {
    static RESPONSE_200: &[u8] = b"HTTP/1.1 200 OK\r\n\
Content-Length: 2\r\n\
\r\n\
OK";

    let captured = Arc::new(Mutex::new(Vec::new()));
    let port = spawn_mock_server_capturing(2, RESPONSE_200, Arc::clone(&captured));
    let url = mock_url(port, "public.bin");

    let store = HttpObjectStore::with_retry(RetryPolicy::no_retry());
    store
        .get_object(&url, &CloudCredentials::Anonymous)
        .await
        .expect("get_object should succeed");

    // Give the server thread a brief moment to capture the request
    std::thread::sleep(std::time::Duration::from_millis(50));

    let raw_request = {
        let guard = captured.lock().expect("lock captured buffer");
        String::from_utf8_lossy(&guard).to_ascii_lowercase()
    };
    assert!(
        !raw_request.contains("authorization"),
        "Anonymous credentials must not produce an Authorization header; \
         raw request was:\n{raw_request}"
    );
}

/// When credentials are Bearer, the client should include
/// `Authorization: Bearer <token>` in the request.
#[tokio::test]
async fn test_http_object_store_bearer_creds_sends_auth_header() {
    static RESPONSE_200: &[u8] = b"HTTP/1.1 200 OK\r\n\
Content-Length: 2\r\n\
\r\n\
OK";

    let captured = Arc::new(Mutex::new(Vec::new()));
    let port = spawn_mock_server_capturing(2, RESPONSE_200, Arc::clone(&captured));
    let url = mock_url(port, "private.bin");

    let store = HttpObjectStore::with_retry(RetryPolicy::no_retry());
    let creds = CloudCredentials::Bearer {
        token: "my-secret-token".to_owned(),
    };
    store
        .get_object(&url, &creds)
        .await
        .expect("get_object should succeed");

    std::thread::sleep(std::time::Duration::from_millis(50));

    let raw_request = {
        let guard = captured.lock().expect("lock captured buffer");
        String::from_utf8_lossy(&guard).to_ascii_lowercase()
    };
    assert!(
        raw_request.contains("authorization: bearer my-secret-token"),
        "Bearer credentials must produce an Authorization header; \
         raw request was:\n{raw_request}"
    );
}

/// A permanent 4xx error (e.g. 403 Forbidden) should not be retried and
/// should produce `CloudError::HttpError { status: 403 }`.
#[tokio::test]
async fn test_http_object_store_403_not_retried() {
    static RESPONSE_403: &[u8] = b"HTTP/1.1 403 Forbidden\r\n\
Content-Length: 0\r\n\
\r\n";

    // Use an aggressive policy so we would notice if retries happened
    let port = spawn_mock_server_static(1, RESPONSE_403);
    let url = mock_url(port, "private.bin");

    let store = HttpObjectStore::with_retry(RetryPolicy::aggressive());
    let err = store
        .get_object(&url, &CloudCredentials::Anonymous)
        .await
        .expect_err("403 should be an error");

    assert!(
        matches!(err, CloudError::HttpError { status: 403, .. }),
        "expected HttpError {{ status: 403 }}, got: {:?}",
        err
    );
}
