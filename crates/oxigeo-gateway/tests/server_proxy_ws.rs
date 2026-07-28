//! Real-socket integration tests for the gateway serving layer.
//!
//! Unlike the in-process `server_serving.rs` tests, these bind ephemeral ports and drive the
//! server through `serve_with_listener` so the load-balanced reverse proxy, the WebSocket
//! endpoint, and the GraphQL subscription mount are exercised over a genuine TCP connection.
//! HTTP requests use a tiny raw `Connection: close` client (no `reqwest`, which is banned); the
//! WebSocket echo test uses `tokio-tungstenite`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::net::SocketAddr;
use std::time::Duration;

use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::routing::get;
use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use oxigeo_gateway::loadbalancer::Backend;
use oxigeo_gateway::websocket::WebSocketConfig;
use oxigeo_gateway::{GatewayConfig, GatewayServer};

/// Spawns a minimal upstream HTTP server on an ephemeral port and returns its address.
///
/// Every request except `GET /fail` is echoed with a fixed marker plus the received
/// `X-Forwarded-For` value; `/fail` always answers `500`. The echo fallback also makes the
/// gateway's `/health` probe succeed (any path returns `200`).
async fn spawn_upstream() -> SocketAddr {
    async fn echo(uri: Uri, headers: HeaderMap) -> impl IntoResponse {
        let forwarded = headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("none")
            .to_string();
        format!("UPSTREAM_MARKER path={} xff={}", uri.path(), forwarded)
    }

    async fn fail() -> impl IntoResponse {
        (StatusCode::INTERNAL_SERVER_ERROR, "UPSTREAM_FAIL")
    }

    let app = axum::Router::new().route("/fail", get(fail)).fallback(echo);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("upstream binds");
    let addr = listener.local_addr().expect("upstream address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    addr
}

/// Serves a gateway on an ephemeral port and returns its address.
async fn spawn_gateway(server: GatewayServer) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("gateway binds");
    let addr = listener.local_addr().expect("gateway address");
    tokio::spawn(async move {
        let _ = server.serve_with_listener(listener).await;
    });
    addr
}

/// Waits until a TCP connection to `addr` succeeds (bounded), guarding against a startup race.
async fn wait_ready(addr: SocketAddr) {
    for _ in 0..40 {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Issues a raw HTTP/1.1 request with `Connection: close` and returns `(status, full response)`.
///
/// Reading to EOF is safe because the server closes the connection after the response; the body
/// substring survives any chunked framing so callers can assert on markers directly.
async fn raw_request(addr: SocketAddr, method: &str, path: &str) -> (u16, String) {
    let io = async {
        let mut stream = tokio::net::TcpStream::connect(addr).await?;
        let request =
            format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await?;
        stream.flush().await?;
        let mut buffer = Vec::new();
        stream.read_to_end(&mut buffer).await?;
        Ok::<_, std::io::Error>(String::from_utf8_lossy(&buffer).into_owned())
    };
    let text = tokio::time::timeout(Duration::from_secs(5), io)
        .await
        .expect("raw request timed out")
        .expect("raw request i/o");
    (parse_status(&text), text)
}

/// Extracts the numeric status code from an HTTP/1.1 status line.
fn parse_status(response: &str) -> u16 {
    response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(0)
}

/// Reads the next text frame from a WebSocket stream, skipping control frames (ping/pong).
async fn next_text<S>(socket: &mut S) -> String
where
    S: futures::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("websocket read timed out")
            .expect("websocket stream ended")
            .expect("websocket protocol error");
        match message {
            Message::Text(text) => return text.as_str().to_string(),
            Message::Ping(_) | Message::Pong(_) => continue,
            Message::Close(_) => panic!("websocket closed unexpectedly"),
            _ => continue,
        }
    }
}

/// 14a. A request through the gateway is forwarded to the backend with `X-Forwarded-For` set.
#[tokio::test(flavor = "multi_thread")]
async fn reverse_proxy_forwards_request_and_sets_forwarded_for() {
    let upstream = spawn_upstream().await;
    wait_ready(upstream).await;

    let server = GatewayServer::builder(GatewayConfig::default())
        .with_backend(Backend::new(
            "upstream".to_string(),
            format!("http://{upstream}"),
            1,
        ))
        .build()
        .expect("gateway builds");
    let gateway = spawn_gateway(server).await;
    wait_ready(gateway).await;

    let (status, body) = raw_request(gateway, "GET", "/echo").await;
    assert_eq!(
        status, 200,
        "proxied request should hit the upstream: {body}"
    );
    assert!(
        body.contains("UPSTREAM_MARKER"),
        "upstream marker missing: {body}"
    );
    assert!(
        body.contains("127.0.0.1"),
        "X-Forwarded-For was not propagated to the upstream: {body}"
    );
}

/// 14b. Repeated upstream `5xx` responses open the backend circuit; the next request is `503`.
#[tokio::test(flavor = "multi_thread")]
async fn reverse_proxy_opens_circuit_and_returns_503() {
    let upstream = spawn_upstream().await;
    wait_ready(upstream).await;

    let mut config = GatewayConfig::default();
    config.loadbalancer.circuit_breaker_threshold = 2;
    config.loadbalancer.circuit_breaker_timeout = 60;
    let server = GatewayServer::builder(config)
        .with_backend(Backend::new(
            "upstream".to_string(),
            format!("http://{upstream}"),
            1,
        ))
        .build()
        .expect("gateway builds");
    let gateway = spawn_gateway(server).await;
    wait_ready(gateway).await;

    // Each 5xx is returned to the client but records a backend failure.
    let (first, _) = raw_request(gateway, "GET", "/fail").await;
    assert_eq!(first, 500);
    let (second, _) = raw_request(gateway, "GET", "/fail").await;
    assert_eq!(second, 500);

    // The circuit is now open (threshold = 2): backend selection fails -> 503.
    let (third, body) = raw_request(gateway, "GET", "/fail").await;
    assert_eq!(
        third, 503,
        "an open circuit should leave no selectable backend: {body}"
    );
}

/// 15. The `/ws` endpoint echoes text frames and survives keepalive pings.
#[tokio::test(flavor = "multi_thread")]
async fn websocket_echo_survives_keepalive_pings() {
    // A short keepalive so a ping fires within the test window (< 2s).
    let ws_config = WebSocketConfig {
        ping_interval: 1,
        ..Default::default()
    };
    let server = GatewayServer::builder(GatewayConfig::default())
        .with_ws_config(ws_config)
        .build()
        .expect("gateway builds");
    let gateway = spawn_gateway(server).await;
    wait_ready(gateway).await;

    let (mut socket, _response) = connect_async(format!("ws://{gateway}/ws"))
        .await
        .expect("websocket connects");

    // The default echo handler bounces any text back verbatim.
    socket
        .send(Message::text("hello".to_string()))
        .await
        .expect("first frame sends");
    assert_eq!(next_text(&mut socket).await, "hello");

    // Let at least one keepalive ping fire, then confirm the connection still works.
    tokio::time::sleep(Duration::from_millis(1300)).await;
    socket
        .send(Message::text("world".to_string()))
        .await
        .expect("second frame sends");
    assert_eq!(next_text(&mut socket).await, "world");

    socket.close(None).await.ok();
}

/// 16. The GraphQL subscription service is mounted at `/graphql/ws`: a plain (non-upgrade) GET is
///     rejected by the service rather than falling through to the `404 NO_ROUTE` proxy fallback.
#[tokio::test(flavor = "multi_thread")]
async fn graphql_subscription_route_is_mounted() {
    let server = GatewayServer::builder(GatewayConfig::default())
        .build()
        .expect("gateway builds");
    let gateway = spawn_gateway(server).await;
    wait_ready(gateway).await;

    let (status, body) = raw_request(gateway, "GET", "/graphql/ws").await;
    assert_ne!(status, 404, "subscription route should be mounted: {body}");
    assert!(
        !body.contains("NO_ROUTE"),
        "request fell through to the proxy fallback instead of the subscription route: {body}"
    );
    assert!(
        (400..500).contains(&status),
        "expected a 4xx upgrade rejection from the subscription service, got {status}: {body}"
    );
}
