//! Tests for `Paginator::stream` — lazy streaming pagination.
//!
//! Each test spins up a minimal in-process HTTP/1.1 server on a random loopback
//! port, configures a `StacClient` pointing at it, and drives the resulting
//! `Stream` via Tokio.  All I/O is done synchronously inside the server thread
//! using `std::net::TcpListener`; no additional HTTP server crate is needed.

#![cfg(all(feature = "reqwest", feature = "async"))]
#![allow(clippy::panic)]
#![allow(clippy::expect_used)]

use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    thread,
    time::Duration,
};

use futures::StreamExt;
use oxigdal_stac::{Paginator, SearchParams, StacClient};

// ──────────────────────────────────────────────────────────────────────────────
// Minimal blocking HTTP/1.1 mock server
// ──────────────────────────────────────────────────────────────────────────────

/// One pre-configured response the mock server will hand back for the *next*
/// incoming `POST /search` request.
#[derive(Clone)]
struct MockResponse {
    status: u16,
    body: String,
}

impl MockResponse {
    fn ok(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            body: body.into(),
        }
    }

    fn error(body: impl Into<String>) -> Self {
        Self {
            status: 500,
            body: body.into(),
        }
    }
}

/// Binds a `TcpListener` on a random loopback port and returns the bound port
/// together with the listener so the caller can hand it to the server thread.
fn bind_random_port() -> (u16, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind random port");
    let port = listener.local_addr().expect("local_addr").port();
    (port, listener)
}

/// Serve one HTTP response per element in `responses`, then stop.
///
/// The function consumes the `TcpListener` and `Vec<MockResponse>` and spawns
/// a background thread.  Returns the thread handle so the test can join.
fn spawn_mock_server(
    listener: TcpListener,
    responses: Vec<MockResponse>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        listener
            .set_nonblocking(false)
            .expect("set_nonblocking false");

        for resp in responses {
            // Accept one connection per expected request.
            let (mut stream, _) = listener.accept().expect("accept connection");
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .expect("set_read_timeout");
            stream
                .set_write_timeout(Some(Duration::from_secs(10)))
                .expect("set_write_timeout");

            // Drain the request (we don't need to parse it; we always reply with
            // the pre-configured response).
            drain_request(&mut stream);

            // Write the HTTP response.
            let response = build_http_response(resp.status, &resp.body);
            stream
                .write_all(response.as_bytes())
                .expect("write response");
            stream.flush().expect("flush");
            // Let the OS close the connection when `stream` drops.
        }
    })
}

/// Read bytes from the stream until the HTTP request headers end (`\r\n\r\n`).
/// Also consume the body if Content-Length is present.  We deliberately keep
/// this simple — it is good enough for `reqwest` which sends well-formed HTTP.
fn drain_request(stream: &mut TcpStream) {
    let mut buf = vec![0u8; 8192];
    let mut total = Vec::new();

    loop {
        let n = match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        total.extend_from_slice(&buf[..n]);

        // Look for end of headers.
        if let Some(header_end) = find_header_end(&total) {
            let header_section = String::from_utf8_lossy(&total[..header_end]);
            if let Some(cl) = extract_content_length(&header_section) {
                // Bytes of body already buffered.
                let already = total.len().saturating_sub(header_end + 4);
                let remaining = cl.saturating_sub(already);
                if remaining > 0 {
                    let mut body_buf = vec![0u8; remaining.min(65536)];
                    let _ = stream.read(&mut body_buf);
                }
            }
            break;
        }
    }
}

/// Returns the byte offset of `\r\n\r\n` in the buffer, or `None`.
fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Extracts the Content-Length value (as `usize`) from an HTTP header block.
fn extract_content_length(headers: &str) -> Option<usize> {
    for line in headers.lines() {
        let lower = line.to_lowercase();
        if lower.starts_with("content-length:") {
            return line
                .split(':')
                .nth(1)
                .and_then(|v| v.trim().parse::<usize>().ok());
        }
    }
    None
}

/// Formats a minimal HTTP/1.1 response.
fn build_http_response(status: u16, body: &str) -> String {
    let reason = match status {
        200 => "OK",
        500 => "Internal Server Error",
        _ => "Unknown",
    };
    format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {len}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        status = status,
        reason = reason,
        len = body.len(),
        body = body,
    )
}

// ──────────────────────────────────────────────────────────────────────────────
// STAC JSON helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Build a minimal STAC `FeatureCollection` JSON string.
///
/// * `ids` — item IDs to include in `features`.
/// * `next_url` — if `Some`, adds a `rel=next` link.
fn make_feature_collection(ids: &[&str], next_url: Option<&str>) -> String {
    let features: Vec<String> = ids
        .iter()
        .map(|id| {
            format!(
                r#"{{
                    "type": "Feature",
                    "stac_version": "1.0.0",
                    "id": "{id}",
                    "geometry": null,
                    "bbox": [-10.0, -10.0, 10.0, 10.0],
                    "properties": {{"datetime": "2024-01-01T00:00:00Z"}},
                    "links": [],
                    "assets": {{}}
                }}"#
            )
        })
        .collect();

    let links_json = match next_url {
        Some(url) => {
            format!(r#"[{{"rel": "next", "href": "{url}", "type": "application/geo+json"}}]"#)
        }
        None => "[]".to_string(),
    };

    format!(
        r#"{{"type": "FeatureCollection", "features": [{features}], "links": {links}}}"#,
        features = features.join(","),
        links = links_json,
    )
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

/// A single page of 3 items with no `next` link: stream must yield exactly 3
/// `Ok` items and then end.
#[test]
fn test_stream_single_page_yields_all_features() {
    let body = make_feature_collection(&["item-a", "item-b", "item-c"], None);
    let (port, listener) = bind_random_port();
    let _server = spawn_mock_server(listener, vec![MockResponse::ok(body)]);

    let base_url = format!("http://127.0.0.1:{}", port);
    let client = StacClient::new(&base_url).expect("build StacClient");
    let paginator = Paginator::new(client, SearchParams::default());

    let rt = tokio::runtime::Runtime::new().expect("build tokio runtime");
    let results: Vec<_> = rt.block_on(async { paginator.stream().collect().await });

    assert_eq!(results.len(), 3, "should yield exactly 3 items");
    for r in &results {
        assert!(r.is_ok(), "each item should be Ok");
    }
    let ids: Vec<_> = results
        .iter()
        .map(|r| r.as_ref().expect("item Ok").id.clone())
        .collect();
    assert_eq!(ids, vec!["item-a", "item-b", "item-c"]);
}

/// Two pages of 2 items each; the first page carries a `next` link pointing to
/// the mock server's second response.  Stream must yield 4 items in order.
#[test]
fn test_stream_two_pages_yields_all_features() {
    // The second page URL must match what the Paginator actually requests.
    // Paginator sends `POST /search` unconditionally, so both pages land on the
    // same endpoint.  The `next` href just needs to have `?token=…` so that
    // `get_next_token` can extract a token (which causes `has_more = true`).
    let (port, listener) = bind_random_port();
    let base_url = format!("http://127.0.0.1:{}", port);
    let next_url = format!("{}/search?token=page2", base_url);

    let page1 = make_feature_collection(&["item-1", "item-2"], Some(&next_url));
    let page2 = make_feature_collection(&["item-3", "item-4"], None);

    let _server = spawn_mock_server(
        listener,
        vec![MockResponse::ok(page1), MockResponse::ok(page2)],
    );

    let client = StacClient::new(&base_url).expect("build StacClient");
    let paginator = Paginator::new(client, SearchParams::default());

    let rt = tokio::runtime::Runtime::new().expect("build tokio runtime");
    let results: Vec<_> = rt.block_on(async { paginator.stream().collect().await });

    assert_eq!(results.len(), 4, "should yield 4 items across two pages");
    for r in &results {
        assert!(r.is_ok(), "each item should be Ok");
    }
    let ids: Vec<_> = results
        .iter()
        .map(|r| r.as_ref().expect("item Ok").id.clone())
        .collect();
    assert_eq!(ids, vec!["item-1", "item-2", "item-3", "item-4"]);
}

/// A page with no `next` link terminates the stream immediately after yielding
/// its items — no extra HTTP requests are made.
#[test]
fn test_stream_terminates_when_no_next_link() {
    let body = make_feature_collection(&["only-item"], None);
    let (port, listener) = bind_random_port();
    // Only one response is registered; a second request would cause the server
    // thread to block on accept and the test would eventually pass (the server
    // won't crash), but we assert on item count so that would fail if extra
    // requests were made.
    let _server = spawn_mock_server(listener, vec![MockResponse::ok(body)]);

    let base_url = format!("http://127.0.0.1:{}", port);
    let client = StacClient::new(&base_url).expect("build StacClient");
    let paginator = Paginator::new(client, SearchParams::default());

    let rt = tokio::runtime::Runtime::new().expect("build tokio runtime");
    let results: Vec<_> = rt.block_on(async { paginator.stream().collect().await });

    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
    assert_eq!(results[0].as_ref().expect("item Ok").id, "only-item");
}

/// When the server returns HTTP 500 the stream must yield a single `Err` item
/// rather than panicking or hanging.
#[test]
fn test_stream_error_propagates_as_err_item() {
    let (port, listener) = bind_random_port();
    let _server = spawn_mock_server(
        listener,
        vec![MockResponse::error(r#"{"code": "InternalError"}"#)],
    );

    let base_url = format!("http://127.0.0.1:{}", port);
    let client = StacClient::new(&base_url).expect("build StacClient");
    let paginator = Paginator::new(client, SearchParams::default());

    let rt = tokio::runtime::Runtime::new().expect("build tokio runtime");
    let results: Vec<_> = rt.block_on(async { paginator.stream().collect().await });

    // The stream should yield exactly one Err and then end.
    assert_eq!(results.len(), 1, "should yield exactly one error element");
    assert!(
        results[0].is_err(),
        "the single element should be Err, got Ok instead"
    );
}

/// An empty `features` array with a `next` link: the stream must still follow
/// the next link (fetching the second page) rather than stopping early.
#[test]
fn test_stream_skips_empty_pages_and_continues() {
    let (port, listener) = bind_random_port();
    let base_url = format!("http://127.0.0.1:{}", port);
    let next_url = format!("{}/search?token=nonempty", base_url);

    // First page is empty but has a next link; second page has items.
    let page1 = make_feature_collection(&[], Some(&next_url));
    let page2 = make_feature_collection(&["real-item"], None);

    let _server = spawn_mock_server(
        listener,
        vec![MockResponse::ok(page1), MockResponse::ok(page2)],
    );

    let client = StacClient::new(&base_url).expect("build StacClient");
    let paginator = Paginator::new(client, SearchParams::default());

    let rt = tokio::runtime::Runtime::new().expect("build tokio runtime");
    let results: Vec<_> = rt.block_on(async { paginator.stream().collect().await });

    assert_eq!(results.len(), 1, "should yield the item from page 2");
    assert!(results[0].is_ok());
    assert_eq!(results[0].as_ref().expect("item Ok").id, "real-item");
}

/// Verifies that the stream yields items from many pages in the correct order.
/// Five pages of 10 items each → stream must yield 50 items in order.
#[test]
fn test_stream_multiple_pages_ordered() {
    let (port, listener) = bind_random_port();
    let base_url = format!("http://127.0.0.1:{}", port);

    const PAGES: usize = 5;
    const PER_PAGE: usize = 10;

    let mut responses = Vec::new();
    for page_idx in 0..PAGES {
        let ids: Vec<String> = (0..PER_PAGE)
            .map(|i| format!("item-{}", page_idx * PER_PAGE + i))
            .collect();
        let id_refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();

        let next_url_opt = if page_idx + 1 < PAGES {
            Some(format!("{}/search?token=page{}", base_url, page_idx + 1))
        } else {
            None
        };
        let next_url_ref = next_url_opt.as_deref();
        let body = make_feature_collection(&id_refs, next_url_ref);
        responses.push(MockResponse::ok(body));
    }

    let _server = spawn_mock_server(listener, responses);

    let client = StacClient::new(&base_url).expect("build StacClient");
    let paginator = Paginator::new(client, SearchParams::default());

    let rt = tokio::runtime::Runtime::new().expect("build tokio runtime");
    let results: Vec<_> = rt.block_on(async { paginator.stream().collect().await });

    assert_eq!(results.len(), PAGES * PER_PAGE);
    for (i, r) in results.iter().enumerate() {
        assert!(r.is_ok());
        let expected_id = format!("item-{}", i);
        assert_eq!(r.as_ref().expect("item Ok").id, expected_id);
    }
}

/// `take(2)` short-circuits correctly: only 2 items are consumed from the
/// first page even though more items and pages exist.
#[test]
fn test_stream_take_stops_early() {
    let (port, listener) = bind_random_port();
    let base_url = format!("http://127.0.0.1:{}", port);
    let next_url = format!("{}/search?token=page2", base_url);

    // Two pages; we will only `take(2)` items from the first page.
    let page1 = make_feature_collection(&["t-0", "t-1", "t-2", "t-3"], Some(&next_url));
    let page2 = make_feature_collection(&["t-4", "t-5"], None);

    // Give the server both responses even though the stream may only consume
    // part of the first page.
    let _server = spawn_mock_server(
        listener,
        vec![MockResponse::ok(page1), MockResponse::ok(page2)],
    );

    let client = StacClient::new(&base_url).expect("build StacClient");
    let paginator = Paginator::new(client, SearchParams::default());

    let rt = tokio::runtime::Runtime::new().expect("build tokio runtime");

    // Collect only the first 2 items.
    let items: Vec<_> = rt.block_on(async { paginator.stream().take(2).collect().await });

    assert_eq!(items.len(), 2);
    assert_eq!(items[0].as_ref().expect("item 0 Ok").id, "t-0");
    assert_eq!(items[1].as_ref().expect("item 1 Ok").id, "t-1");
}

/// A stream with exactly one empty page (no items, no next link) terminates
/// immediately without yielding any items.
#[test]
fn test_stream_empty_single_page_yields_nothing() {
    let body = make_feature_collection(&[], None);
    let (port, listener) = bind_random_port();
    let _server = spawn_mock_server(listener, vec![MockResponse::ok(body)]);

    let base_url = format!("http://127.0.0.1:{}", port);
    let client = StacClient::new(&base_url).expect("build StacClient");
    let paginator = Paginator::new(client, SearchParams::default());

    let rt = tokio::runtime::Runtime::new().expect("build tokio runtime");
    let results: Vec<_> = rt.block_on(async { paginator.stream().collect().await });

    assert!(
        results.is_empty(),
        "empty page with no next link should yield zero items"
    );
}
