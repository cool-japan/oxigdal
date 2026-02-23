//! OxiGDAL WebSocket Streaming
//!
//! This crate provides WebSocket support for real-time geospatial data streaming.
//!
//! # Features
//!
//! - **WebSocket Server**: Axum-based WebSocket server with connection management
//! - **WebSocket Client**: Async client with reconnection support
//! - **Message Protocol**: Multiple formats (JSON, MessagePack, Binary) with compression
//! - **Tile Streaming**: Real-time map tile delivery with delta encoding
//! - **Feature Streaming**: GeoJSON feature updates with change detection
//! - **Event Streaming**: System events, progress updates, and notifications
//! - **Subscription Management**: Spatial, temporal, and attribute-based filtering
//! - **Backpressure Control**: Automatic throttling and flow control
//!
//! # Examples
//!
//! ## Server
//!
//! ```rust,no_run
//! use oxigdal_ws::server::WebSocketServer;
//!
//! #[tokio::main]
//! async fn main() -> oxigdal_ws::error::Result<()> {
//!     let server = WebSocketServer::builder()
//!         .bind("0.0.0.0:9001")?
//!         .max_connections(10000)
//!         .build();
//!
//!     server.run().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Client
//!
//! ```rust,no_run
//! use oxigdal_ws::client::WebSocketClient;
//!
//! #[tokio::main]
//! async fn main() -> oxigdal_ws::error::Result<()> {
//!     let mut client = WebSocketClient::connect("ws://localhost:9001/ws").await?;
//!
//!     // Subscribe to tiles
//!     let sub_id = client.subscribe_tiles(
//!         [-122.5, 37.5, -122.0, 38.0],
//!         10..14
//!     ).await?;
//!
//!     // Get tile stream
//!     let mut tiles = client.tile_stream();
//!     while let Some(tile) = tiles.next_tile().await {
//!         println!("Received tile: {:?}", tile.coords());
//!     }
//!
//!     Ok(())
//! }
//! ```

#![deny(missing_docs)]
#![warn(clippy::unwrap_used)]

pub mod client;
pub mod error;
pub mod protocol;
pub mod server;
pub mod stream;
pub mod subscription;

/// WebSocket handlers
pub mod handlers {
    /// Event streaming handler
    pub mod events;
    /// Feature streaming handler
    pub mod features;
    /// Tile streaming handler
    pub mod tiles;
}

// Re-export commonly used types
pub use client::{ClientConfig, WebSocketClient};
pub use error::{Error, Result};
pub use protocol::{
    ChangeType, Compression, EventType, Message, MessageFormat, SubscriptionFilter,
};
pub use server::{ServerConfig, WebSocketServer};
pub use stream::{
    BackpressureController, BackpressureState, DeltaEncoder, EventData, EventStream, FeatureData,
    FeatureStream, MessageStream, TileData, TileStream,
};
pub use subscription::{Subscription, SubscriptionManager, SubscriptionType};

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::client::{ClientConfig, WebSocketClient};
    pub use crate::error::{Error, Result};
    pub use crate::protocol::{
        ChangeType, Compression, EventType, Message, MessageFormat, SubscriptionFilter,
    };
    pub use crate::server::{ServerConfig, WebSocketServer};
    pub use crate::stream::{
        BackpressureController, BackpressureState, DeltaEncoder, EventData, EventStream,
        FeatureData, FeatureStream, MessageStream, TileData, TileStream,
    };
    pub use crate::subscription::{Subscription, SubscriptionManager, SubscriptionType};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exports() {
        // Just verify that key types are exported
        let _: Option<Error> = None;
        let _: Option<Message> = None;
    }
}
