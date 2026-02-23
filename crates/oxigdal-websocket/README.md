# OxiGDAL WebSocket

Advanced real-time WebSocket communication for OxiGDAL with comprehensive broadcasting, pub/sub, and live updates.

## Features

### WebSocket Server (~1,500 LOC)
- **Tokio-tungstenite** based WebSocket server
- **Connection Management**: Track and manage thousands of concurrent connections
- **Heartbeat/Ping-Pong**: Automatic connection health monitoring
- **Connection Pooling**: Efficient resource management and reuse

### Protocol (~1,000 LOC)
- **Binary Protocol**: Optimized geospatial binary encoding
- **JSON Protocol**: Standard JSON for compatibility
- **Message Framing**: Efficient message packaging and parsing
- **Compression**: Zstd and gzip compression support

### Broadcasting (~800 LOC)
- **Pub/Sub Channels**: Topic-based message distribution
- **Room Management**: Group-based communication
- **Selective Broadcasting**: Filter-based message routing
- **Message Filters**: Geographic, attribute, and custom filtering

### Live Updates (~800 LOC)
- **Tile Updates**: Real-time map tile notifications
- **Feature Updates**: GeoJSON feature change tracking
- **Change Streams**: MongoDB-style change stream processing
- **Incremental Updates**: Delta-based updates for bandwidth efficiency

### Client SDK (~600 LOC)
- **JavaScript Client**: Browser and Node.js compatible
- **TypeScript Definitions**: Full type safety
- **Reconnection Logic**: Automatic reconnection with exponential backoff
- **Client-Side Caching**: Tile and feature caching

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-websocket = { workspace = true }
```

## Usage

### Starting a Server

```rust
use oxigdal_websocket::server::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = Server::builder()
        .max_connections(10_000)
        .max_message_size(16 * 1024 * 1024)
        .heartbeat_interval(30)
        .build();

    server.start().await?;
    Ok(())
}
```

### Broadcasting Messages

```rust
use oxigdal_websocket::broadcast::BroadcastSystem;
use oxigdal_websocket::protocol::message::Message;

let system = BroadcastSystem::new(Default::default());

// Subscribe to topic
let subscriber_id = uuid::Uuid::new_v4();
system.subscribe("geo-updates".to_string(), subscriber_id).await?;

// Publish message
let message = Message::ping();
system.publish("geo-updates", message).await?;
```

### Tile Updates

```rust
use oxigdal_websocket::updates::tile_updates::{TileCoord, TileUpdate, TileUpdateManager};

let manager = TileUpdateManager::new(1000);
let coord = TileCoord::new(10, 512, 384);
let data = vec![/* tile data */];

let update = TileUpdate::full(coord, data, "png".to_string());
manager.add_update(update)?;
```

### JavaScript Client

```javascript
const client = new OxiGDALWebSocketClient();

client.connect('ws://localhost:9001');

client.on('connected', () => {
    console.log('Connected to server');

    // Subscribe to tile updates
    client.subscribe('tile-updates', (tile) => {
        console.log('Received tile:', tile);
    });

    // Join a room
    client.joinRoom('map-viewers');
});

client.on('tileUpdate', (tile) => {
    // Handle tile update
    console.log(`Tile ${tile.z}/${tile.x}/${tile.y} updated`);
});
```

## Architecture

### Server Components

- **Connection**: WebSocket connection wrapper with state management
- **HeartbeatMonitor**: Monitors connection health with ping/pong
- **ConnectionManager**: Manages all active connections
- **ConnectionPool**: Pools idle connections for reuse
- **Server**: Main server orchestrator

### Protocol Components

- **ProtocolCodec**: Encoding/decoding messages
- **FrameCodec**: Message framing
- **CompressionCodec**: Data compression
- **BinaryCodec**: Geospatial binary protocol
- **JsonCodec**: JSON protocol

### Broadcasting Components

- **TopicChannel**: Pub/sub topic implementation
- **RoomManager**: Group chat room management
- **MessageRouter**: Message routing and distribution
- **MessageFilter**: Filtering rules for selective broadcasting

### Update Components

- **TileUpdateManager**: Manages tile update notifications
- **FeatureUpdateManager**: Tracks feature changes
- **ChangeStream**: MongoDB-style change streams
- **IncrementalUpdateManager**: Delta-based updates

## COOLJAPAN Compliance

✅ **Pure Rust**: 100% Rust implementation
✅ **No unwrap()**: All errors handled properly
✅ **Files < 2000 lines**: All source files under 2000 LOC
✅ **Workspace deps**: Uses workspace dependencies
✅ **No warnings**: Compiles without warnings

## Statistics

- **Total LOC**: ~5,300
- **Test Coverage**: Comprehensive unit and integration tests
- **Documentation**: Full API documentation with examples

## License

Apache-2.0

## Author

COOLJAPAN OU (Team Kitasan)
