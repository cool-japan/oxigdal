//! WebSocket server implementation
//!
//! This module provides a comprehensive WebSocket server with:
//! - Connection management and pooling
//! - Heartbeat/ping-pong mechanism
//! - Connection lifecycle management
//! - Backpressure handling

pub mod connection;
pub mod heartbeat;
pub mod manager;
pub mod pool;
mod ws_server;

pub use connection::{Connection, ConnectionId, ConnectionState};
pub use heartbeat::{HeartbeatConfig, HeartbeatMonitor};
pub use manager::{ConnectionManager, ConnectionStats};
pub use pool::{ConnectionPool, PoolConfig, PoolStats};
pub use ws_server::{Server, ServerConfig};

/// Default maximum number of connections
pub const DEFAULT_MAX_CONNECTIONS: usize = 10_000;
/// Default maximum message size (16MB)
pub const DEFAULT_MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;
/// Default heartbeat interval in seconds
pub const DEFAULT_HEARTBEAT_INTERVAL_SECS: u64 = 30;
/// Default heartbeat timeout in seconds
pub const DEFAULT_HEARTBEAT_TIMEOUT_SECS: u64 = 90;
/// Default connection timeout in seconds
pub const DEFAULT_CONNECTION_TIMEOUT_SECS: u64 = 300;
