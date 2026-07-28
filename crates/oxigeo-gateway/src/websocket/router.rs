//! WebSocket message routing.

use super::{ConnectionId, WsMessage};
use crate::error::{GatewayError, Result};
use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Registry mapping a connection id to the channel that writes back to that connection.
type SenderRegistry = Arc<DashMap<ConnectionId, mpsc::UnboundedSender<WsMessage>>>;

/// Message handler trait.
#[async_trait::async_trait]
pub trait MessageHandler: Send + Sync {
    /// Handles a WebSocket message.
    async fn handle(&self, conn_id: &str, message: WsMessage) -> Result<Option<WsMessage>>;
}

/// Message router for WebSocket messages.
pub struct MessageRouter {
    handlers: Arc<parking_lot::RwLock<HashMap<String, Arc<dyn MessageHandler>>>>,
    default_handler: Option<Arc<dyn MessageHandler>>,
    /// Per-connection outbound channels, used to deliver a handler's response back to the
    /// originating connection. Shared with the owning [`super::WebSocketManager`] so both see
    /// the same set of live connections.
    senders: SenderRegistry,
}

impl MessageRouter {
    /// Creates a new message router with its own (empty) connection registry.
    pub fn new() -> Self {
        Self {
            handlers: Arc::new(parking_lot::RwLock::new(HashMap::new())),
            default_handler: None,
            senders: Arc::new(DashMap::new()),
        }
    }

    /// Creates a router that shares an existing connection-sender registry, so responses can
    /// be delivered to connections registered by the owning manager.
    pub fn with_senders(senders: SenderRegistry) -> Self {
        Self {
            handlers: Arc::new(parking_lot::RwLock::new(HashMap::new())),
            default_handler: None,
            senders,
        }
    }

    /// Registers the outbound channel for a connection so responses reach it.
    pub fn register_sender(&self, conn_id: ConnectionId, sender: mpsc::UnboundedSender<WsMessage>) {
        self.senders.insert(conn_id, sender);
    }

    /// Removes a connection's outbound channel.
    pub fn unregister_sender(&self, conn_id: &str) {
        self.senders.remove(conn_id);
    }

    /// Registers a message handler for a route.
    pub fn register_handler(&self, route: String, handler: Arc<dyn MessageHandler>) {
        self.handlers.write().insert(route, handler);
    }

    /// Sets the default handler.
    pub fn set_default_handler(&mut self, handler: Arc<dyn MessageHandler>) {
        self.default_handler = Some(handler);
    }

    /// Routes a message to the appropriate handler and delivers any response back to the
    /// originating connection.
    pub async fn route_message(&self, conn_id: &str, message: WsMessage) -> Result<()> {
        // Extract route from message (simplified)
        let route = self.extract_route(&message)?;

        let handler = {
            let handlers = self.handlers.read();
            handlers.get(&route).cloned()
        };

        let response = if let Some(handler) = handler {
            handler.handle(conn_id, message).await?
        } else if let Some(default_handler) = &self.default_handler {
            default_handler.handle(conn_id, message).await?
        } else {
            return Err(GatewayError::WebSocketError(format!(
                "No handler for route: {}",
                route
            )));
        };

        // Deliver the response back to the originating connection over its outbound channel.
        if let Some(resp) = response {
            let sender = self.senders.get(conn_id).ok_or_else(|| {
                GatewayError::WebSocketError(format!(
                    "cannot deliver response: connection '{conn_id}' has no registered sender"
                ))
            })?;
            sender.send(resp).map_err(|e| {
                GatewayError::WebSocketError(format!("failed to send response to '{conn_id}': {e}"))
            })?;
        }

        Ok(())
    }

    /// Extracts route from message.
    fn extract_route(&self, message: &WsMessage) -> Result<String> {
        match message {
            WsMessage::Text(text) => {
                // Try to parse as JSON and extract route
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(text)
                    && let Some(route) = json.get("route").and_then(|r| r.as_str())
                {
                    return Ok(route.to_string());
                }
                Ok("default".to_string())
            }
            _ => Ok("default".to_string()),
        }
    }
}

impl Default for MessageRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// Echo message handler (for testing).
pub struct EchoHandler;

#[async_trait::async_trait]
impl MessageHandler for EchoHandler {
    async fn handle(&self, _conn_id: &str, message: WsMessage) -> Result<Option<WsMessage>> {
        Ok(Some(message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_router_creation() {
        let router = MessageRouter::new();
        assert!(router.handlers.read().is_empty());
    }

    #[tokio::test]
    async fn test_register_handler() {
        let router = MessageRouter::new();
        let handler = Arc::new(EchoHandler);

        router.register_handler("echo".to_string(), handler);
        assert_eq!(router.handlers.read().len(), 1);
    }

    #[tokio::test]
    async fn test_echo_handler() {
        let handler = EchoHandler;
        let message = WsMessage::Text("test".to_string());

        let result = handler.handle("conn_1", message.clone()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_route_message_delivers_response_to_connection() {
        let router = MessageRouter::new();
        router.register_handler("default".to_string(), Arc::new(EchoHandler));

        // Register an outbound channel for the connection.
        let (tx, mut rx) = mpsc::unbounded_channel();
        router.register_sender("conn_1".to_string(), tx);

        router
            .route_message("conn_1", WsMessage::Text("ping".to_string()))
            .await
            .expect("route should succeed");

        // The echoed response must actually arrive on the connection's channel.
        let received = rx
            .try_recv()
            .expect("a response should have been delivered");
        match received {
            WsMessage::Text(text) => assert_eq!(text, "ping"),
            other => panic!("unexpected message: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_route_message_without_sender_errors() {
        let router = MessageRouter::new();
        router.register_handler("default".to_string(), Arc::new(EchoHandler));

        // No sender registered for conn_1 -> delivering the echo response must error rather
        // than silently dropping it.
        let result = router
            .route_message("conn_1", WsMessage::Text("ping".to_string()))
            .await;
        assert!(result.is_err());
    }
}
