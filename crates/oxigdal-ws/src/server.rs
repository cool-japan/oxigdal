//! WebSocket server implementation.

use crate::error::{Error, Result};
use crate::protocol::{Compression, Message, MessageFormat};
use crate::subscription::{Subscription, SubscriptionManager};
use axum::{
    Router,
    extract::{
        State,
        ws::{WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use dashmap::DashMap;
use futures::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// WebSocket server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Bind address
    pub bind_addr: SocketAddr,
    /// Maximum connections
    pub max_connections: usize,
    /// Message buffer size per client
    pub message_buffer_size: usize,
    /// Default message format
    pub default_format: MessageFormat,
    /// Default compression
    pub default_compression: Compression,
    /// Enable CORS
    pub enable_cors: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: SocketAddr::from(([0, 0, 0, 0], 9001)),
            max_connections: 10000,
            message_buffer_size: 1000,
            default_format: MessageFormat::MessagePack,
            default_compression: Compression::Zstd,
            enable_cors: true,
        }
    }
}

/// Client connection state.
struct ClientState {
    /// Client ID
    id: String,
    /// Message sender
    tx: mpsc::UnboundedSender<Message>,
    /// Message format preference
    format: MessageFormat,
    /// Compression preference
    compression: Compression,
}

impl ClientState {
    /// Send a message to the client.
    fn send(&self, message: Message) -> Result<()> {
        self.tx
            .send(message)
            .map_err(|_| Error::Send("Client disconnected".to_string()))
    }
}

/// Shared server state.
#[derive(Clone)]
struct AppState {
    /// Active clients
    clients: Arc<DashMap<String, ClientState>>,
    /// Subscription manager
    subscriptions: Arc<SubscriptionManager>,
    /// Server configuration
    config: Arc<ServerConfig>,
}

impl AppState {
    fn new(config: ServerConfig) -> Self {
        Self {
            clients: Arc::new(DashMap::new()),
            subscriptions: Arc::new(SubscriptionManager::new()),
            config: Arc::new(config),
        }
    }

    /// Broadcast message to all clients.
    fn broadcast(&self, message: Message) {
        for client in self.clients.iter() {
            if let Err(e) = client.send(message.clone()) {
                warn!("Failed to send to client {}: {}", client.id, e);
            }
        }
    }

    /// Send message to specific client.
    fn send_to_client(&self, client_id: &str, message: Message) -> Result<()> {
        if let Some(client) = self.clients.get(client_id) {
            client.send(message)
        } else {
            Err(Error::NotFound(format!("Client not found: {}", client_id)))
        }
    }

    /// Send message to all subscribers of a subscription type.
    #[allow(dead_code)]
    fn send_to_subscribers(&self, subscription_id: &str, message: Message) {
        if let Some(sub) = self.subscriptions.get(subscription_id) {
            if let Err(e) = self.send_to_client(&sub.client_id, message) {
                warn!("Failed to send to subscriber {}: {}", sub.client_id, e);
            }
        }
    }
}

/// WebSocket server.
pub struct WebSocketServer {
    state: AppState,
}

impl WebSocketServer {
    /// Create a new WebSocket server with default configuration.
    pub fn new() -> Self {
        Self::with_config(ServerConfig::default())
    }

    /// Create a new WebSocket server with custom configuration.
    pub fn with_config(config: ServerConfig) -> Self {
        Self {
            state: AppState::new(config),
        }
    }

    /// Create a builder for the server.
    pub fn builder() -> ServerBuilder {
        ServerBuilder::new()
    }

    /// Run the WebSocket server.
    pub async fn run(self) -> Result<()> {
        let bind_addr = self.state.config.bind_addr;

        let mut app = Router::new()
            .route("/ws", get(ws_handler))
            .route("/health", get(health_handler))
            .with_state(self.state.clone());

        if self.state.config.enable_cors {
            app = app.layer(CorsLayer::permissive());
        }

        info!("WebSocket server listening on {}", bind_addr);

        let listener = tokio::net::TcpListener::bind(bind_addr)
            .await
            .map_err(|e| Error::Server(format!("Failed to bind: {}", e)))?;

        axum::serve(listener, app)
            .await
            .map_err(|e| Error::Server(format!("Server error: {}", e)))?;

        Ok(())
    }

    /// Get server statistics.
    pub fn stats(&self) -> ServerStats {
        ServerStats {
            active_connections: self.state.clients.len(),
            total_subscriptions: self.state.subscriptions.count(),
            unique_clients: self.state.subscriptions.client_count(),
        }
    }

    /// Broadcast a message to all connected clients.
    pub fn broadcast(&self, message: Message) {
        self.state.broadcast(message);
    }

    /// Send a message to a specific client.
    pub fn send_to_client(&self, client_id: &str, message: Message) -> Result<()> {
        self.state.send_to_client(client_id, message)
    }

    /// Get subscription manager.
    pub fn subscriptions(&self) -> &SubscriptionManager {
        &self.state.subscriptions
    }
}

impl Default for WebSocketServer {
    fn default() -> Self {
        Self::new()
    }
}

/// Server statistics.
#[derive(Debug, Clone)]
pub struct ServerStats {
    /// Number of active WebSocket connections
    pub active_connections: usize,
    /// Total number of subscriptions
    pub total_subscriptions: usize,
    /// Number of unique clients with subscriptions
    pub unique_clients: usize,
}

/// Builder for WebSocket server.
pub struct ServerBuilder {
    config: ServerConfig,
}

impl ServerBuilder {
    /// Create a new server builder.
    pub fn new() -> Self {
        Self {
            config: ServerConfig::default(),
        }
    }

    /// Set bind address.
    pub fn bind(mut self, addr: &str) -> Result<Self> {
        self.config.bind_addr = addr
            .parse()
            .map_err(|e| Error::InvalidParameter(format!("Invalid address: {}", e)))?;
        Ok(self)
    }

    /// Set maximum connections.
    pub fn max_connections(mut self, max: usize) -> Self {
        self.config.max_connections = max;
        self
    }

    /// Set message buffer size.
    pub fn message_buffer_size(mut self, size: usize) -> Self {
        self.config.message_buffer_size = size;
        self
    }

    /// Set default message format.
    pub fn default_format(mut self, format: MessageFormat) -> Self {
        self.config.default_format = format;
        self
    }

    /// Set default compression.
    pub fn default_compression(mut self, compression: Compression) -> Self {
        self.config.default_compression = compression;
        self
    }

    /// Enable CORS.
    pub fn enable_cors(mut self, enable: bool) -> Self {
        self.config.enable_cors = enable;
        self
    }

    /// Build the server.
    pub fn build(self) -> WebSocketServer {
        WebSocketServer::with_config(self.config)
    }
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Health check handler.
async fn health_handler() -> &'static str {
    "OK"
}

/// WebSocket upgrade handler.
async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

/// Handle WebSocket connection.
async fn handle_socket(socket: WebSocket, state: AppState) {
    let client_id = Uuid::new_v4().to_string();
    info!("New WebSocket connection: {}", client_id);

    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel();

    // Default protocol settings
    let mut format = state.config.default_format;
    let mut compression = state.config.default_compression;

    // Add client to state
    let client_state = ClientState {
        id: client_id.clone(),
        tx: tx.clone(),
        format,
        compression,
    };
    state.clients.insert(client_id.clone(), client_state);

    // Spawn task to send messages to client
    let client_id_clone = client_id.clone();
    tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            // Encode message
            let data = match message.encode(format, compression) {
                Ok(data) => data,
                Err(e) => {
                    error!("Failed to encode message: {}", e);
                    continue;
                }
            };

            // Send as binary message
            if let Err(e) = sender
                .send(axum::extract::ws::Message::Binary(data.into()))
                .await
            {
                error!("Failed to send message to {}: {}", client_id_clone, e);
                break;
            }
        }
    });

    // Handle incoming messages
    while let Some(msg) = receiver.next().await {
        let msg = match msg {
            Ok(msg) => msg,
            Err(e) => {
                error!("WebSocket error for {}: {}", client_id, e);
                break;
            }
        };

        let data = match msg {
            axum::extract::ws::Message::Binary(data) => data.to_vec(),
            axum::extract::ws::Message::Text(text) => text.as_bytes().to_vec(),
            axum::extract::ws::Message::Close(_) => {
                info!("Client {} disconnected", client_id);
                break;
            }
            axum::extract::ws::Message::Ping(_) | axum::extract::ws::Message::Pong(_) => {
                continue;
            }
        };

        // Decode message
        let message = match Message::decode(&data, format, compression) {
            Ok(msg) => msg,
            Err(e) => {
                error!("Failed to decode message from {}: {}", client_id, e);
                continue;
            }
        };

        // Handle message
        if let Err(e) =
            handle_message(message, &client_id, &state, &mut format, &mut compression).await
        {
            error!("Error handling message from {}: {}", client_id, e);
        }
    }

    // Cleanup on disconnect
    info!("Cleaning up client {}", client_id);
    state.clients.remove(&client_id);
    if let Err(e) = state.subscriptions.remove_client(&client_id) {
        error!("Failed to remove client subscriptions: {}", e);
    }
}

/// Handle a received message.
async fn handle_message(
    message: Message,
    client_id: &str,
    state: &AppState,
    format: &mut MessageFormat,
    compression: &mut Compression,
) -> Result<()> {
    match message {
        Message::Handshake {
            version,
            format: client_format,
            compression: client_compression,
        } => {
            debug!("Handshake from {}: v{}", client_id, version);

            // Negotiate protocol
            *format = client_format;
            *compression = client_compression;

            // Update client state
            if let Some(mut client) = state.clients.get_mut(client_id) {
                client.format = *format;
                client.compression = *compression;
            }

            // Send acknowledgement
            state.send_to_client(
                client_id,
                Message::HandshakeAck {
                    version,
                    format: *format,
                    compression: *compression,
                },
            )?;
        }

        Message::SubscribeTiles {
            subscription_id,
            bbox,
            zoom_range,
            ..
        } => {
            debug!("Subscribe tiles from {}: {}", client_id, subscription_id);

            let sub = Subscription::tiles(client_id.to_string(), bbox, zoom_range, None);
            state.subscriptions.add(sub)?;

            state.send_to_client(
                client_id,
                Message::Ack {
                    request_id: subscription_id,
                    success: true,
                    message: Some("Subscribed to tiles".to_string()),
                },
            )?;
        }

        Message::SubscribeFeatures {
            subscription_id,
            layer,
            ..
        } => {
            debug!("Subscribe features from {}: {}", client_id, subscription_id);

            let sub = Subscription::features(client_id.to_string(), layer, None);
            state.subscriptions.add(sub)?;

            state.send_to_client(
                client_id,
                Message::Ack {
                    request_id: subscription_id,
                    success: true,
                    message: Some("Subscribed to features".to_string()),
                },
            )?;
        }

        Message::SubscribeEvents {
            subscription_id,
            event_types,
        } => {
            debug!("Subscribe events from {}: {}", client_id, subscription_id);

            let event_types_set = event_types.into_iter().collect();
            let sub = Subscription::events(client_id.to_string(), event_types_set, None);
            state.subscriptions.add(sub)?;

            state.send_to_client(
                client_id,
                Message::Ack {
                    request_id: subscription_id,
                    success: true,
                    message: Some("Subscribed to events".to_string()),
                },
            )?;
        }

        Message::Unsubscribe { subscription_id } => {
            debug!("Unsubscribe from {}: {}", client_id, subscription_id);

            state.subscriptions.remove(&subscription_id)?;

            state.send_to_client(
                client_id,
                Message::Ack {
                    request_id: subscription_id,
                    success: true,
                    message: Some("Unsubscribed".to_string()),
                },
            )?;
        }

        Message::Ping { id } => {
            state.send_to_client(client_id, Message::Pong { id })?;
        }

        _ => {
            warn!("Unexpected message type from {}", client_id);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_config_default() {
        let config = ServerConfig::default();
        assert_eq!(config.max_connections, 10000);
        assert_eq!(config.message_buffer_size, 1000);
        assert!(config.enable_cors);
    }

    #[test]
    fn test_server_builder() {
        let result = ServerBuilder::new().bind("127.0.0.1:8080");
        assert!(result.is_ok());
        if let Ok(builder) = result {
            let server = builder
                .max_connections(5000)
                .message_buffer_size(500)
                .default_format(MessageFormat::Json)
                .enable_cors(false)
                .build();

            assert_eq!(server.state.config.bind_addr.to_string(), "127.0.0.1:8080");
            assert_eq!(server.state.config.max_connections, 5000);
            assert_eq!(server.state.config.message_buffer_size, 500);
            assert_eq!(server.state.config.default_format, MessageFormat::Json);
            assert!(!server.state.config.enable_cors);
        }
    }

    #[test]
    fn test_app_state() {
        let state = AppState::new(ServerConfig::default());

        assert_eq!(state.clients.len(), 0);
        assert_eq!(state.subscriptions.count(), 0);
    }
}
