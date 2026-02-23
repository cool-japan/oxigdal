//! WebSocket message routing.

use super::WsMessage;
use crate::error::{GatewayError, Result};
use std::collections::HashMap;
use std::sync::Arc;

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
}

impl MessageRouter {
    /// Creates a new message router.
    pub fn new() -> Self {
        Self {
            handlers: Arc::new(parking_lot::RwLock::new(HashMap::new())),
            default_handler: None,
        }
    }

    /// Registers a message handler for a route.
    pub fn register_handler(&self, route: String, handler: Arc<dyn MessageHandler>) {
        self.handlers.write().insert(route, handler);
    }

    /// Sets the default handler.
    pub fn set_default_handler(&mut self, handler: Arc<dyn MessageHandler>) {
        self.default_handler = Some(handler);
    }

    /// Routes a message to the appropriate handler.
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

        // Send response if any
        if let Some(_resp) = response {
            // In a real implementation, send response back to connection
        }

        Ok(())
    }

    /// Extracts route from message.
    fn extract_route(&self, message: &WsMessage) -> Result<String> {
        match message {
            WsMessage::Text(text) => {
                // Try to parse as JSON and extract route
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(text) {
                    if let Some(route) = json.get("route").and_then(|r| r.as_str()) {
                        return Ok(route.to_string());
                    }
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
}
