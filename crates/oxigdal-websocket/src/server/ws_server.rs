//! WebSocket server implementation

use crate::error::{Error, Result};
use crate::protocol::{MessageFormat, ProtocolCodec, ProtocolConfig};
use crate::server::connection::Connection;
use crate::server::heartbeat::{HeartbeatConfig, HeartbeatMonitor};
use crate::server::manager::ConnectionManager;
use crate::server::pool::{ConnectionPool, PoolConfig};
use crate::server::{DEFAULT_MAX_CONNECTIONS, DEFAULT_MAX_MESSAGE_SIZE};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tokio_tungstenite::accept_async;

/// Server configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Bind address
    pub bind_addr: SocketAddr,
    /// Maximum connections
    pub max_connections: usize,
    /// Maximum message size
    pub max_message_size: usize,
    /// Protocol configuration
    pub protocol: ProtocolConfig,
    /// Heartbeat configuration
    pub heartbeat: HeartbeatConfig,
    /// Pool configuration
    pub pool: PoolConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: SocketAddr::from(([0, 0, 0, 0], 9001)),
            max_connections: DEFAULT_MAX_CONNECTIONS,
            max_message_size: DEFAULT_MAX_MESSAGE_SIZE,
            protocol: ProtocolConfig::default(),
            heartbeat: HeartbeatConfig::default(),
            pool: PoolConfig::default(),
        }
    }
}

/// WebSocket server
pub struct Server {
    config: ServerConfig,
    manager: Arc<ConnectionManager>,
    pool: Arc<ConnectionPool>,
    heartbeat: Arc<HeartbeatMonitor>,
    shutdown: Arc<RwLock<bool>>,
}

impl Server {
    /// Create a new server
    pub fn new(config: ServerConfig) -> Self {
        let manager = Arc::new(ConnectionManager::new(config.max_connections));
        let pool = Arc::new(ConnectionPool::new(config.pool.clone()));
        let heartbeat = Arc::new(HeartbeatMonitor::new(config.heartbeat.clone()));

        Self {
            config,
            manager,
            pool,
            heartbeat,
            shutdown: Arc::new(RwLock::new(false)),
        }
    }

    /// Create a server builder
    pub fn builder() -> ServerBuilder {
        ServerBuilder::new()
    }

    /// Start the server
    pub async fn start(self: Arc<Self>) -> Result<()> {
        tracing::info!("Starting WebSocket server on {}", self.config.bind_addr);

        // Start heartbeat monitor
        let heartbeat = self.heartbeat.clone();
        tokio::spawn(async move {
            if let Err(e) = heartbeat.start().await {
                tracing::error!("Heartbeat monitor error: {}", e);
            }
        });

        // Start pool maintenance task
        let pool = self.pool.clone();
        let shutdown = self.shutdown.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                if *shutdown.read().await {
                    break;
                }
                interval.tick().await;
                if let Err(e) = pool.maintain().await {
                    tracing::error!("Pool maintenance error: {}", e);
                }
            }
        });

        // Bind listener
        let listener = TcpListener::bind(&self.config.bind_addr)
            .await
            .map_err(|e| Error::Connection(format!("Failed to bind: {}", e)))?;

        tracing::info!("Server listening on {}", self.config.bind_addr);

        // Accept connections
        loop {
            // Check for shutdown
            if *self.shutdown.read().await {
                break;
            }

            match listener.accept().await {
                Ok((stream, addr)) => {
                    let server = self.clone();
                    tokio::spawn(async move {
                        if let Err(e) = server.handle_connection(stream, addr).await {
                            tracing::error!("Connection error from {}: {}", addr, e);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("Accept error: {}", e);
                }
            }
        }

        tracing::info!("Server stopped");
        Ok(())
    }

    /// Handle a new connection
    async fn handle_connection(&self, stream: TcpStream, addr: SocketAddr) -> Result<()> {
        tracing::info!("New connection from {}", addr);

        // Perform WebSocket handshake
        let ws_stream = accept_async(stream)
            .await
            .map_err(|e| Error::WebSocket(format!("Handshake failed: {}", e)))?;

        // Create protocol codec
        let codec = ProtocolCodec::new(self.config.protocol.clone());

        // Create connection
        let (connection, rx) = Connection::new(ws_stream, addr, codec);
        let connection = Arc::new(connection);

        // Add to manager
        self.manager.add(connection.clone())?;

        // Add to heartbeat monitor
        self.heartbeat.add_connection(connection.clone()).await;

        // Spawn outgoing message handler
        let conn = connection.clone();
        tokio::spawn(async move {
            if let Err(e) = conn.process_outgoing(rx).await {
                tracing::error!("Outgoing message handler error: {}", e);
            }
        });

        // Handle incoming messages
        loop {
            match connection.receive().await {
                Ok(Some(message)) => {
                    tracing::debug!(
                        "Received message from {}: {:?}",
                        addr,
                        message.message_type()
                    );
                    // Handle message (can be extended with custom logic)
                }
                Ok(None) => {
                    // No message, continue
                }
                Err(e) => {
                    tracing::error!("Receive error from {}: {}", addr, e);
                    break;
                }
            }

            // Check connection state
            let state = connection.state().await;
            if state != crate::server::connection::ConnectionState::Connected {
                break;
            }
        }

        // Cleanup
        self.manager.remove(&connection.id());
        self.heartbeat.remove_connection(&connection.id()).await;

        tracing::info!("Connection from {} closed", addr);
        Ok(())
    }

    /// Shutdown the server
    pub async fn shutdown(&self) -> Result<()> {
        tracing::info!("Shutting down server");

        // Set shutdown flag
        let mut shutdown = self.shutdown.write().await;
        *shutdown = true;
        drop(shutdown);

        // Stop heartbeat monitor
        self.heartbeat.shutdown().await;

        // Close all connections
        self.manager.close_all().await?;

        // Shutdown pool
        self.pool.shutdown().await?;

        tracing::info!("Server shutdown complete");
        Ok(())
    }

    /// Get connection manager
    pub fn manager(&self) -> &Arc<ConnectionManager> {
        &self.manager
    }

    /// Get connection pool
    pub fn pool(&self) -> &Arc<ConnectionPool> {
        &self.pool
    }

    /// Get heartbeat monitor
    pub fn heartbeat(&self) -> &Arc<HeartbeatMonitor> {
        &self.heartbeat
    }

    /// Get server statistics
    pub async fn stats(&self) -> ServerStats {
        let manager_stats = self.manager.stats();
        let pool_stats = self.pool.stats();
        let heartbeat_stats = self.heartbeat.stats().await;

        ServerStats {
            active_connections: manager_stats.active_connections,
            total_connections: manager_stats.total_connections,
            messages_sent: manager_stats.messages_sent,
            messages_received: manager_stats.messages_received,
            bytes_sent: manager_stats.bytes_sent,
            bytes_received: manager_stats.bytes_received,
            errors: manager_stats.errors,
            pool_idle: pool_stats.idle_connections,
            pool_active: pool_stats.active_connections,
            heartbeat_monitored: heartbeat_stats.monitored_connections,
        }
    }
}

/// Server statistics
#[derive(Debug, Clone)]
pub struct ServerStats {
    /// Active connections
    pub active_connections: usize,
    /// Total connections served
    pub total_connections: u64,
    /// Messages sent
    pub messages_sent: u64,
    /// Messages received
    pub messages_received: u64,
    /// Bytes sent
    pub bytes_sent: u64,
    /// Bytes received
    pub bytes_received: u64,
    /// Errors
    pub errors: u64,
    /// Pool idle connections
    pub pool_idle: usize,
    /// Pool active connections
    pub pool_active: usize,
    /// Heartbeat monitored connections
    pub heartbeat_monitored: usize,
}

/// Server builder
pub struct ServerBuilder {
    config: ServerConfig,
}

impl ServerBuilder {
    /// Create a new server builder
    pub fn new() -> Self {
        Self {
            config: ServerConfig::default(),
        }
    }

    /// Set bind address
    pub fn bind_addr(mut self, addr: SocketAddr) -> Self {
        self.config.bind_addr = addr;
        self
    }

    /// Set max connections
    pub fn max_connections(mut self, max: usize) -> Self {
        self.config.max_connections = max;
        self
    }

    /// Set max message size
    pub fn max_message_size(mut self, size: usize) -> Self {
        self.config.max_message_size = size;
        self
    }

    /// Set message format
    pub fn message_format(mut self, format: MessageFormat) -> Self {
        self.config.protocol.format = format;
        self
    }

    /// Set heartbeat interval
    pub fn heartbeat_interval(mut self, secs: u64) -> Self {
        self.config.heartbeat.interval_secs = secs;
        self
    }

    /// Enable/disable heartbeat
    pub fn enable_heartbeat(mut self, enabled: bool) -> Self {
        self.config.heartbeat.enabled = enabled;
        self
    }

    /// Set pool config
    pub fn pool_config(mut self, config: PoolConfig) -> Self {
        self.config.pool = config;
        self
    }

    /// Build the server
    pub fn build(self) -> Arc<Server> {
        Arc::new(Server::new(self.config))
    }
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_config_default() {
        let config = ServerConfig::default();
        assert_eq!(config.max_connections, DEFAULT_MAX_CONNECTIONS);
        assert_eq!(config.max_message_size, DEFAULT_MAX_MESSAGE_SIZE);
    }

    #[test]
    fn test_server_builder() {
        let server = Server::builder()
            .max_connections(5000)
            .max_message_size(8 * 1024 * 1024)
            .heartbeat_interval(60)
            .build();

        // Server should be created successfully
        assert!(Arc::strong_count(&server) >= 1);
    }

    #[tokio::test]
    async fn test_server_stats() {
        let server = Server::builder().build();
        let stats = server.stats().await;

        assert_eq!(stats.active_connections, 0);
        assert_eq!(stats.total_connections, 0);
    }
}
