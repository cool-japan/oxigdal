//! WebSocket upgrade endpoint for the gateway serving layer.
//!
//! This module supplies the transport that the in-memory [`WebSocketManager`] (subsystem A)
//! lacks. [`ws_handler`] authenticates the request **before** performing the protocol upgrade
//! (reading a bearer token from the `Authorization` header or a `?token=` query parameter),
//! enforces the per-user connection cap, and applies the configured maximum message size.
//! After the upgrade, [`handle_socket`] mints a connection id, registers the connection with the
//! manager, then runs two concurrent halves: a single outbound task that owns the write sink and
//! interleaves queued messages with periodic keepalive pings, and an inbound loop that feeds every
//! received frame through [`WebSocketManager::handle_message`]. A malformed message is logged and
//! skipped so it can never tear down the connection; only a close frame or a transport error ends
//! the session, after which the connection is unregistered and the outbound task aborted.
//!
//! [`from_axum`] and [`to_axum`] hand-map between axum 0.8's `axum::extract::ws::Message`
//! (whose `Text` payload is `Utf8Bytes` and whose `Binary`/`Ping`/`Pong` payloads are `Bytes`) and
//! the crate's [`WsMessage`], for which no `From`/`Into` conversion exists.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{RawQuery, State};
use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;
use axum::response::{IntoResponse, Response};
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::error::GatewayError;
use crate::server::state::GatewayState;
use crate::websocket::{Connection, WebSocketManager, WsMessage};

/// Handles a WebSocket upgrade request.
///
/// Authentication happens **pre-upgrade**: the bearer token is taken from the `Authorization`
/// header (used verbatim, including its scheme) or, failing that, from a `token=` query parameter
/// which is wrapped as `Bearer <token>` before being handed to the authenticator. When
/// `state.require_auth` is set, a missing or invalid token yields `401` before any upgrade occurs;
/// otherwise a present, valid token is used only to resolve the caller's user id (best effort). If
/// the resolved user already holds `ws_config.max_connections_per_user` live connections the
/// request is rejected with `429`. On success the configured maximum message size is applied and
/// the socket is upgraded into [`handle_socket`].
///
/// # Security tradeoff: the `?token=` query fallback
///
/// The `token=` query-parameter fallback exists because browser WebSocket clients cannot set an
/// `Authorization` header on the upgrade request. Passing a credential in a URL is generally
/// discouraged (URLs leak into logs, proxies and browser history), so the gateway mitigates it by
/// **excluding the query string from its own request tracing** (the trace span records only the
/// method and the URI path — see `server::router`). Operators who control the client should still
/// prefer the `Authorization` header and treat the query token strictly as a browser-compatibility
/// fallback.
pub(crate) async fn ws_handler(
    State(state): State<GatewayState>,
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    // Resolve the credential: the full Authorization header value (scheme included), or a
    // `token=` query parameter synthesized into a `Bearer <token>` value.
    let auth_value = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .or_else(|| query_token(query.as_deref()).map(|token| format!("Bearer {token}")));

    // Authenticate. When auth is not required this is best effort and only serves to resolve a
    // user id; when it is required an invalid credential rejects the upgrade with 401.
    let mut user_id: Option<String> = None;
    if let (Some(value), Some(authenticator)) =
        (auth_value.as_deref(), state.authenticator.as_ref())
    {
        match authenticator.authenticate(value).await {
            Ok(context) => user_id = Some(context.identity.user_id.clone()),
            Err(error) => {
                if state.require_auth {
                    return error.into_response();
                }
                tracing::debug!("websocket authentication failed (auth not required): {error}");
            }
        }
    }
    if state.require_auth && user_id.is_none() {
        return GatewayError::AuthenticationFailed(
            "authentication required for websocket upgrade".to_string(),
        )
        .into_response();
    }

    // Enforce the per-user connection cap (only meaningful once a user id is known).
    if let Some(uid) = user_id.as_ref() {
        let active = state.ws_manager.get_user_connections(uid).len();
        if active >= state.ws_config.max_connections_per_user {
            return GatewayError::RateLimitExceeded {
                message: format!(
                    "websocket connection limit ({}) reached for user",
                    state.ws_config.max_connections_per_user
                ),
                retry_after: None,
            }
            .into_response();
        }
    }

    let manager = Arc::clone(&state.ws_manager);
    let max_message_size = state.ws_config.max_message_size;
    // Clamp the keepalive period to a positive value: `tokio::time::interval` panics on zero.
    let ping_interval = Duration::from_secs(state.ws_config.ping_interval.max(1));

    ws.max_message_size(max_message_size)
        .on_upgrade(move |socket| handle_socket(socket, manager, user_id, ping_interval))
}

/// Drives a single upgraded WebSocket connection to completion.
///
/// A fresh connection id is minted and registered with the manager (carrying the authenticated
/// `user_id`) **before** any message is processed, so handler responses and broadcasts can reach
/// this socket. The socket is split; the sole owner of the write half is a spawned outbound task
/// that, via `tokio::select!`, forwards messages queued on the manager's channel and emits a
/// keepalive ping every `ping_interval`. The inbound loop routes each received frame through the
/// manager, logging and skipping errors so a single bad message cannot end the session. On a close
/// frame, transport error, or stream end the connection is unregistered and the outbound task is
/// aborted.
async fn handle_socket(
    socket: WebSocket,
    manager: Arc<WebSocketManager>,
    user_id: Option<String>,
    ping_interval: Duration,
) {
    let conn_id = Uuid::new_v4().to_string();

    let mut connection = Connection::new(conn_id.clone());
    connection.user_id = user_id;

    let (tx, mut rx) = mpsc::unbounded_channel::<WsMessage>();

    // Register before the first inbound message so routed responses have somewhere to land.
    if let Err(error) = manager.register_connection(connection, tx) {
        tracing::warn!("failed to register websocket connection {conn_id}: {error}");
        return;
    }

    let (mut sink, mut stream) = socket.split();

    // Outbound task: the single owner of the write sink. It forwards queued messages and, on a
    // fixed interval, emits a keepalive ping. It exits when the channel closes or a send fails.
    let outbound = tokio::spawn(async move {
        let mut interval = tokio::time::interval(ping_interval);
        // The first tick fires immediately; consume it so the first ping waits a full period.
        interval.tick().await;
        loop {
            tokio::select! {
                outgoing = rx.recv() => match outgoing {
                    Some(message) => {
                        if sink.send(to_axum(message)).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                },
                _ = interval.tick() => {
                    if sink.send(to_axum(WsMessage::Ping(Vec::new()))).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Inbound loop: route every non-close frame; a close frame or transport error ends the loop.
    while let Some(Ok(message)) = stream.next().await {
        if matches!(message, Message::Close(_)) {
            break;
        }
        if let Err(error) = manager.handle_message(&conn_id, from_axum(message)).await {
            tracing::warn!("websocket message handling error on {conn_id}: {error}");
        }
    }

    // Teardown: drop the connection from the shared registry and stop the outbound task.
    let _ = manager.unregister_connection(&conn_id);
    outbound.abort();
}

/// Extracts a raw `token=` value from a URL query string.
///
/// The split is naive (`&`-delimited, no percent-decoding): a token containing URL-encoded
/// characters is returned verbatim. The first `token=` pair wins.
fn query_token(query: Option<&str>) -> Option<String> {
    query?
        .split('&')
        .find_map(|pair| pair.strip_prefix("token=").map(str::to_string))
}

/// Maps an inbound axum WebSocket message onto the crate's [`WsMessage`].
///
/// A `Close` frame (with or without a close reason) maps to [`WsMessage::Close`], discarding the
/// reason since subsystem A carries no close payload.
pub(crate) fn from_axum(msg: Message) -> WsMessage {
    match msg {
        Message::Text(text) => WsMessage::Text(text.to_string()),
        Message::Binary(bytes) => WsMessage::Binary(bytes.to_vec()),
        Message::Ping(bytes) => WsMessage::Ping(bytes.to_vec()),
        Message::Pong(bytes) => WsMessage::Pong(bytes.to_vec()),
        Message::Close(_) => WsMessage::Close,
    }
}

/// Maps a [`WsMessage`] onto an outbound axum WebSocket message.
///
/// [`WsMessage::Close`] becomes `Message::Close(None)` (no close frame reason is attached).
pub(crate) fn to_axum(msg: WsMessage) -> Message {
    match msg {
        WsMessage::Text(text) => Message::Text(text.into()),
        WsMessage::Binary(bytes) => Message::Binary(bytes.into()),
        WsMessage::Ping(bytes) => Message::Ping(bytes.into()),
        WsMessage::Pong(bytes) => Message::Pong(bytes.into()),
        WsMessage::Close => Message::Close(None),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use axum::extract::ws::Utf8Bytes;
    use bytes::Bytes;

    #[test]
    fn from_axum_maps_every_variant() {
        match from_axum(Message::Text(Utf8Bytes::from("hi"))) {
            WsMessage::Text(text) => assert_eq!(text, "hi"),
            other => panic!("expected Text, got {other:?}"),
        }
        match from_axum(Message::Binary(Bytes::from_static(&[1, 2, 3]))) {
            WsMessage::Binary(bytes) => assert_eq!(bytes, vec![1, 2, 3]),
            other => panic!("expected Binary, got {other:?}"),
        }
        match from_axum(Message::Ping(Bytes::from_static(&[9]))) {
            WsMessage::Ping(bytes) => assert_eq!(bytes, vec![9]),
            other => panic!("expected Ping, got {other:?}"),
        }
        match from_axum(Message::Pong(Bytes::from_static(&[8]))) {
            WsMessage::Pong(bytes) => assert_eq!(bytes, vec![8]),
            other => panic!("expected Pong, got {other:?}"),
        }
        match from_axum(Message::Close(None)) {
            WsMessage::Close => {}
            other => panic!("expected Close, got {other:?}"),
        }
    }

    #[test]
    fn to_axum_maps_every_variant() {
        match to_axum(WsMessage::Text("hi".to_string())) {
            Message::Text(text) => assert_eq!(text, "hi"),
            other => panic!("expected Text, got {other:?}"),
        }
        match to_axum(WsMessage::Binary(vec![1, 2, 3])) {
            Message::Binary(bytes) => assert_eq!(bytes.as_ref(), &[1, 2, 3]),
            other => panic!("expected Binary, got {other:?}"),
        }
        match to_axum(WsMessage::Ping(vec![9])) {
            Message::Ping(bytes) => assert_eq!(bytes.as_ref(), &[9]),
            other => panic!("expected Ping, got {other:?}"),
        }
        match to_axum(WsMessage::Pong(vec![8])) {
            Message::Pong(bytes) => assert_eq!(bytes.as_ref(), &[8]),
            other => panic!("expected Pong, got {other:?}"),
        }
        match to_axum(WsMessage::Close) {
            Message::Close(None) => {}
            other => panic!("expected Close(None), got {other:?}"),
        }
    }

    #[test]
    fn roundtrip_preserves_text_payload() {
        // A text payload survives a to->from roundtrip unchanged.
        match from_axum(to_axum(WsMessage::Text("payload".to_string()))) {
            WsMessage::Text(text) => assert_eq!(text, "payload"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn roundtrip_preserves_binary_payload() {
        match from_axum(to_axum(WsMessage::Binary(vec![4, 5, 6, 7]))) {
            WsMessage::Binary(bytes) => assert_eq!(bytes, vec![4, 5, 6, 7]),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn query_token_extracts_first_match() {
        assert_eq!(query_token(Some("token=abc")).as_deref(), Some("abc"));
        assert_eq!(
            query_token(Some("foo=1&token=xyz&bar=2")).as_deref(),
            Some("xyz")
        );
        assert_eq!(query_token(Some("foo=1&bar=2")), None);
        assert_eq!(query_token(Some("")), None);
        assert_eq!(query_token(None), None);
    }
}
