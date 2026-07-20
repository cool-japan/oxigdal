//! Integration tests for XyzProtocol HTTP fetching.
//!
//! These tests spin up a minimal in-process TCP server that speaks just
//! enough HTTP/1.1 to exercise the protocol implementation.

#![cfg(feature = "tile-http")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use oxigeo_streaming::error::StreamingError;
use oxigeo_streaming::tile::protocol::{
    TileCoordinate, TileFormat, TileProtocol, TileRequest, XyzProtocol,
};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

// ─────────────────────────────────────────────────────────────────────────────
// Minimal HTTP/1.1 mock server helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Spawn a mock HTTP server that handles up to `max_connections` requests.
///
/// `response_bytes` is written verbatim to the TCP stream for every accepted
/// connection.  The response must be a complete HTTP/1.1 message.
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

                // Consume the request (we don't actually parse it)
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf).unwrap_or(0);

                stream.write_all(response_bytes).ok();
            });
        }
    });

    port
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

/// Happy-path: server returns 200 with an ASCII tile body; client fetches it.
#[tokio::test]
async fn test_xyz_get_tile_fetches_http_body() {
    // A simple ASCII body standing in for tile data
    const TILE_BODY: &[u8] = b"FAKE_PNG_BODY";

    static RESPONSE_200: &[u8] = b"HTTP/1.1 200 OK\r\n\
Content-Type: image/png\r\n\
Content-Length: 13\r\n\
ETag: \"abc123\"\r\n\
Cache-Control: max-age=86400\r\n\
\r\n\
FAKE_PNG_BODY";

    let port = spawn_mock_server_static(2, RESPONSE_200);

    let url_template = format!("http://127.0.0.1:{port}/{{z}}/{{x}}/{{y}}.png");
    let protocol = XyzProtocol::new(url_template, 0, 18);

    let coord = TileCoordinate::new(5, 10, 15);
    let request = TileRequest::new(coord, TileFormat::Png);

    let response = protocol
        .get_tile(&request)
        .await
        .expect("get_tile should succeed for 200 response");

    assert_eq!(response.coord, coord);
    assert_eq!(
        &response.data[..],
        TILE_BODY,
        "tile body should match server response"
    );
    assert!(
        response.content_type.contains("image/png"),
        "content type should be image/png, got: {}",
        response.content_type
    );
    assert!(response.etag.is_some(), "ETag header should be extracted");
    assert_eq!(
        response.etag.as_deref(),
        Some("\"abc123\""),
        "ETag value should match"
    );
}

/// Server returns 404 → `TileNotFound` error variant.
#[tokio::test]
async fn test_xyz_get_tile_404_returns_tile_not_found() {
    static RESPONSE_404: &[u8] = b"HTTP/1.1 404 Not Found\r\n\
Content-Length: 0\r\n\
\r\n";

    let port = spawn_mock_server_static(2, RESPONSE_404);

    let url_template = format!("http://127.0.0.1:{port}/{{z}}/{{x}}/{{y}}.png");
    let protocol = XyzProtocol::new(url_template, 0, 18);

    let coord = TileCoordinate::new(5, 99, 99);
    let request = TileRequest::new(coord, TileFormat::Png);

    let err = protocol
        .get_tile(&request)
        .await
        .expect_err("get_tile should fail for 404");

    assert!(
        matches!(err, StreamingError::TileNotFound),
        "expected TileNotFound, got: {:?}",
        err
    );
}

/// Server fails with 500 twice, then succeeds on the third attempt.
/// The retry logic should recover and return the successful response.
#[tokio::test]
async fn test_xyz_get_tile_5xx_retried() {
    // We need to serve: 500, 500, 200 — use a shared counter per connection.
    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_clone = Arc::clone(&call_count);

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();

    thread::spawn(move || {
        for stream in listener.incoming().take(3) {
            let count = call_count_clone.fetch_add(1, Ordering::SeqCst);
            thread::spawn(move || {
                let mut stream = stream.expect("accept");
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .ok();
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf).unwrap_or(0);

                let response: &[u8] = if count < 2 {
                    b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n"
                } else {
                    b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 4\r\n\r\nTILE"
                };
                stream.write_all(response).ok();
            });
        }
    });

    let url_template = format!("http://127.0.0.1:{port}/{{z}}/{{x}}/{{y}}.png");
    let protocol = XyzProtocol::new(url_template, 0, 18);

    let coord = TileCoordinate::new(3, 1, 1);
    let request = TileRequest::new(coord, TileFormat::Png);

    let response = protocol
        .get_tile(&request)
        .await
        .expect("get_tile should succeed after retrying 5xx errors");

    assert_eq!(
        &response.data[..],
        b"TILE",
        "should receive body from the third attempt"
    );
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        3,
        "exactly three HTTP requests should have been made"
    );
}
