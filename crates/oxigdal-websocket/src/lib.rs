//! OxiGDAL WebSocket - Advanced Real-Time Communication
//!
//! This crate provides comprehensive WebSocket support for the OxiGDAL geospatial library,
//! enabling real-time data streaming, broadcasting, and live updates.
//!
//! # Features
//!
//! - **WebSocket Server**: Full-featured server with connection management, heartbeat, and pooling
//! - **Protocol Support**: Binary and JSON protocols with compression and framing
//! - **Broadcasting**: Pub/sub channels, room management, and message filtering
//! - **Live Updates**: Tile updates, feature changes, and change streams
//! - **Client SDK**: JavaScript/TypeScript client with reconnection and caching
//!
//! # Examples
//!
//! ## Starting a WebSocket Server
//!
//! ```rust,no_run
//! use oxigdal_websocket::server::Server;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let server = Server::builder()
//!         .max_connections(10_000)
//!         .build();
//!
//!     server.start().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Broadcasting Messages
//!
//! ```rust,no_run
//! use oxigdal_websocket::broadcast::BroadcastSystem;
//! use oxigdal_websocket::protocol::message::Message;
//!
//! # async fn example() -> oxigdal_websocket::error::Result<()> {
//! let system = BroadcastSystem::new(Default::default());
//!
//! // Publish to a topic
//! let message = Message::ping();
//! system.publish("geospatial-updates", message).await?;
//! # Ok(())
//! # }
//! ```

#![deny(missing_docs)]
#![warn(clippy::unwrap_used)]

pub mod broadcast;
pub mod client_sdk;
pub mod error;
pub mod protocol;
pub mod server;
pub mod updates;

// Re-export commonly used types
pub use broadcast::{BroadcastConfig, BroadcastSystem};
pub use client_sdk::{
    ClientSdkConfig, generate_javascript_client, generate_typescript_definitions,
};
pub use error::{Error, Result};
pub use protocol::{Message, MessageFormat, MessageType, Payload, ProtocolCodec, ProtocolConfig};
pub use server::{Server, ServerConfig};
pub use updates::{
    ChangeStream, FeatureUpdateManager, IncrementalUpdateManager, TileUpdateManager, UpdateConfig,
    UpdateSystem,
};

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::broadcast::{BroadcastConfig, BroadcastSystem, Room, RoomManager};
    pub use crate::client_sdk::{
        ClientSdkConfig, generate_javascript_client, generate_typescript_definitions,
    };
    pub use crate::error::{Error, Result};
    pub use crate::protocol::{
        BinaryCodec, CompressionType, Message, MessageFormat, MessageType, Payload, ProtocolCodec,
        ProtocolConfig,
    };
    pub use crate::server::{Connection, ConnectionId, HeartbeatConfig, Server, ServerConfig};
    pub use crate::updates::{
        ChangeStream, FeatureUpdate, IncrementalUpdate, TileUpdate, UpdateConfig, UpdateSystem,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exports() {
        // Verify that key types are exported
        let _: Option<Error> = None;
        let _: Option<Message> = None;
    }

    #[test]
    fn test_prelude_imports() {
        use prelude::*;

        // Verify that prelude imports work
        let _ = ServerConfig::default();
        let _ = ProtocolConfig::default();
        let _ = BroadcastConfig::default();
        let _ = UpdateConfig::default();
    }
}
