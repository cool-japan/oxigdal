# OxiGDAL Edge Computing Platform

Edge computing platform for OxiGDAL with offline-first architecture and minimal footprint for resource-constrained devices.

## Features

### Core Capabilities

- **Lightweight Edge Runtime**: Optimized runtime for embedded and IoT devices
- **Offline-First Architecture**: Local-first data processing with optional cloud sync
- **Minimal Footprint**: Binary size reduction and reduced dependency tree
- **Resource Management**: CPU, memory, and storage constraints for edge devices

### Synchronization

- **Edge-to-Cloud Sync**: Flexible synchronization protocols (manual, periodic, incremental, batch, real-time)
- **CRDT-Based Conflict Resolution**: Automatic conflict resolution using Conflict-free Replicated Data Types
- **Sync Protocols**: Vector clocks for causality tracking
- **Batch Operations**: Efficient batching for bandwidth-limited environments

### Data Management

- **Local Caching**: LRU, LFU, TTL, and size-based eviction policies
- **Edge-Optimized Compression**: LZ4, Snappy, and Deflate with adaptive selection
- **Persistent Storage**: Optional persistent cache for offline operation

### Performance

- **Binary Size Optimization**: Minimal dependencies for embedded deployment
- **Resource-Constrained Support**: Configurable memory, CPU, and storage limits
- **Async Runtime**: Tokio-based async execution with minimal features

## Quick Start

```rust
use oxigdal_edge::{EdgeRuntime, EdgeConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create edge runtime with minimal footprint
    let config = EdgeConfig::minimal();
    let runtime = EdgeRuntime::new(config).await?;

    // Start runtime
    runtime.start().await?;

    // Process data locally
    let result = runtime.execute(async {
        // Your edge processing logic
        Ok(42)
    }).await?;

    // Stop runtime
    runtime.stop().await?;

    Ok(())
}
```

## Configuration Modes

### Minimal Mode (Embedded Devices)

```rust
let config = EdgeConfig::minimal();
// - 1 MB cache
// - Fast compression
// - Offline mode
// - Manual sync
```

### Offline-First Mode

```rust
let config = EdgeConfig::offline_first();
// - 50 MB cache
// - Persistent storage
// - Batch sync
// - Balanced compression
```

### Custom Configuration

```rust
let config = EdgeConfig {
    mode: RuntimeMode::Hybrid,
    constraints: ResourceConstraints {
        max_memory_bytes: 10 * 1024 * 1024,
        max_cpu_percent: 50.0,
        max_storage_bytes: 20 * 1024 * 1024,
        max_concurrent_ops: 5,
        operation_timeout_secs: 30,
    },
    cache_config: CacheConfig::minimal(),
    compression_level: CompressionLevel::Fast,
    sync_strategy: SyncStrategy::Incremental,
    data_dir: PathBuf::from(".edge"),
    enable_metrics: true,
    heartbeat_interval_secs: 60,
};
```

## Cache Management

```rust
use oxigdal_edge::{Cache, CacheConfig, CachePolicy};

let config = CacheConfig {
    max_size: 10 * 1024 * 1024, // 10 MB
    policy: CachePolicy::Lru,
    ttl_secs: Some(3600), // 1 hour
    persistent: true,
    cache_dir: Some(PathBuf::from(".cache")),
    max_entries: 1000,
};

let cache = Cache::new(config)?;

// Store data
cache.put("key".to_string(), Bytes::from("data"))?;

// Retrieve data
if let Some(data) = cache.get("key")? {
    println!("Retrieved: {:?}", data);
}
```

## Compression

```rust
use oxigdal_edge::{EdgeCompressor, CompressionStrategy, CompressionLevel};

// Fast compression for real-time
let compressor = EdgeCompressor::fast();
let compressed = compressor.compress(data)?;

// Adaptive compression
let adaptive = AdaptiveCompressor::new(CompressionLevel::Balanced);
let (compressed, strategy) = adaptive.compress(data)?;
let decompressed = adaptive.decompress(&compressed, strategy)?;
```

## Synchronization

```rust
use oxigdal_edge::sync::{SyncManager, SyncStrategy, SyncItem};

let cache = Arc::new(Cache::new(CacheConfig::minimal())?);
let manager = SyncManager::new(SyncStrategy::Incremental, cache)?;

// Add items to sync queue
let item = SyncItem::new(
    "item-1".to_string(),
    "key-1".to_string(),
    vec![1, 2, 3],
    1,
);
manager.add_pending(item);

// Start automatic sync
manager.start().await?;

// Or trigger manual sync
manager.sync_now().await?;
```

## Conflict Resolution

```rust
use oxigdal_edge::{ConflictResolver, CrdtMap, VectorClock};

let resolver = ConflictResolver::new("edge-node-1".to_string());

// Create CRDT map for automatic conflict resolution
let mut map = resolver.create_map();
map.insert("key1", "value1");
map.insert("key2", "value2");

// Merge with another node's data
let mut map2 = CrdtMap::new("edge-node-2".to_string());
map2.insert("key2", "value2_updated");
map.merge(&map2);
```

## Resource Management

```rust
use oxigdal_edge::{ResourceManager, ResourceConstraints};

let constraints = ResourceConstraints::minimal();
let manager = ResourceManager::new(constraints)?;

// Track operations
let _op_guard = manager.start_operation()?;

// Track memory
let _mem_guard = manager.allocate_memory(1024)?;

// Check health
let health = manager.health_check();
match health {
    HealthStatus::Healthy => println!("System healthy"),
    HealthStatus::Degraded => println!("System degraded"),
    HealthStatus::Critical => println!("System critical"),
}
```

## Performance Benchmarks

Run benchmarks comparing edge vs cloud performance:

```bash
cargo bench --package oxigdal-edge
```

Key metrics:
- Cache operations: 100k+ ops/sec
- LZ4 compression: 500+ MB/s
- Snappy compression: 1+ GB/s
- Memory allocation tracking: < 100ns overhead
- CRDT operations: 1M+ ops/sec

## Architecture

```
┌─────────────────────────────────────────┐
│         Edge Runtime                     │
│  ┌────────────┐  ┌──────────────┐       │
│  │ Executor   │  │ Scheduler    │       │
│  └────────────┘  └──────────────┘       │
└─────────────────────────────────────────┘
         │                    │
         ▼                    ▼
┌──────────────┐      ┌──────────────┐
│   Cache      │      │   Resource   │
│              │      │   Manager    │
└──────────────┘      └──────────────┘
         │                    │
         ▼                    ▼
┌──────────────┐      ┌──────────────┐
│ Compression  │      │     Sync     │
│              │      │   Manager    │
└──────────────┘      └──────────────┘
         │                    │
         ▼                    ▼
┌─────────────────────────────────────────┐
│         Local Storage                    │
│  (Optional Persistent Cache)            │
└─────────────────────────────────────────┘
```

## Use Cases

1. **IoT Sensors**: Process geospatial sensor data locally on edge devices
2. **Mobile Mapping**: Offline-first mobile GIS applications
3. **Remote Monitoring**: Field data collection with intermittent connectivity
4. **Embedded Systems**: Geospatial processing on resource-constrained devices
5. **Distributed Edge Networks**: Multi-node edge computing with CRDT sync

## COOLJAPAN Policies

- ✅ Pure Rust implementation (no C/Fortran dependencies)
- ✅ No `unwrap()` usage (comprehensive error handling)
- ✅ All files < 2000 lines
- ✅ Workspace dependencies
- ✅ Comprehensive tests and benchmarks

## License

Apache-2.0

## Authors

COOLJAPAN OU (Team Kitasan)
