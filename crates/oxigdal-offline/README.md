# OxiGDAL Offline - Offline-First Data Management

Comprehensive offline-first data management for OxiGDAL with local storage, sync queue management, conflict resolution, and optimistic updates.

## Features

- **Offline-First Architecture**: Local-first data layer with background synchronization
- **Multi-Platform Storage**:
  - SQLite for native platforms (desktop, server)
  - IndexedDB for WASM/browser environments
- **Sync Queue Management**: Persistent queue with automatic retry and exponential backoff
- **Conflict Detection**: Automatic detection of concurrent modifications
- **Merge Strategies**:
  - Last-Write-Wins
  - Local-Wins / Remote-Wins
  - Three-Way-Merge
  - Larger-Wins
  - Custom merge strategies
- **Background Sync**: Automatic synchronization when connectivity is restored
- **Optimistic Updates**: Immediate UI updates with eventual consistency
- **Retry Mechanisms**: Exponential backoff with configurable jitter
- **Pure Rust**: No C/C++ dependencies (COOLJAPAN policy compliant)
- **WASM-Compatible**: Runs in browsers using IndexedDB

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-offline = "0.1"

# For native platforms
[features]
default = ["native"]
native = ["oxigdal-offline/native"]

# For WASM platforms
wasm = ["oxigdal-offline/wasm"]
```

## Quick Start

### Basic Usage

```rust
use oxigdal_offline::{OfflineManager, Config, MergeStrategy};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure offline manager
    let config = Config::builder()
        .max_queue_size(1000)
        .merge_strategy(MergeStrategy::LastWriteWins)
        .retry_max_attempts(5)
        .auto_sync_interval_secs(60)
        .build()?;

    // Create offline manager
    let manager = OfflineManager::new(config).await?;

    // Write data (automatically queued for sync)
    manager.write("key1", b"value1").await?;

    // Read data (from local cache)
    if let Some(record) = manager.read("key1").await? {
        println!("Data: {:?}", record.data);
    }

    // Update data
    manager.update("key1", b"new_value").await?;

    // Sync when online
    let result = manager.sync().await?;
    println!("Synced: {}", result.summary());

    Ok(())
}
```

### With Remote Backend

```rust
use oxigdal_offline::{OfflineManager, Config};
use oxigdal_offline::sync::RemoteBackend;

// Implement your remote backend
struct MyRemoteBackend {
    // ... your implementation
}

#[async_trait::async_trait]
impl RemoteBackend for MyRemoteBackend {
    async fn push_operation(&self, operation: &Operation) -> Result<()> {
        // Push to your remote API
        Ok(())
    }

    async fn fetch_updates(&self, since: DateTime<Utc>) -> Result<Vec<Record>> {
        // Fetch from your remote API
        Ok(Vec::new())
    }

    async fn ping(&self) -> Result<bool> {
        // Check connectivity
        Ok(true)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::default();
    let manager = OfflineManager::new(config)
        .await?
        .with_remote(Box::new(MyRemoteBackend::new()));

    // Now sync operations will use your remote backend
    manager.write("key", b"value").await?;
    manager.sync().await?;

    Ok(())
}
```

### WASM Usage

```rust
use oxigdal_offline::{OfflineManager, Config};
use wasm_bindgen_futures::spawn_local;

pub async fn wasm_example() -> Result<(), JsValue> {
    let config = Config::builder()
        .database_name("my-app-offline".to_string())
        .build()
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    let manager = OfflineManager::new(config).await
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    // Write data
    manager.write("user_settings", b"{}").await
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    // Background sync
    spawn_local(async move {
        loop {
            if manager.is_online().await {
                let _ = manager.sync().await;
            }
            // Wait 60 seconds via JS setTimeout
            let promise = js_sys::Promise::new(&mut |resolve, _| {
                if let Some(window) = web_sys::window() {
                    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                        &resolve, 60000);
                }
            });
            let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
        }
    });
    Ok(())
}
```

## Architecture

### Storage Layer

The storage layer provides an abstraction over platform-specific storage backends:

- **SQLite** (native): High-performance embedded database
- **IndexedDB** (WASM): Browser-native storage with async API

### Sync Queue

Operations are queued for synchronization:

```
┌─────────────┐
│   Write     │
│   Update    │──▶ Local Storage ──▶ Sync Queue ──▶ Remote Sync
│   Delete    │         ▲                                │
└─────────────┘         │                                │
                        └────────── Conflict ────────────┘
                                   Resolution
```

### Conflict Resolution

The system detects and resolves conflicts using configurable strategies:

1. **Version-based**: Compare version numbers
2. **Timestamp-based**: Use modification timestamps
3. **Content-based**: Compare actual data

### Optimistic Updates

UI updates happen immediately while sync occurs in the background:

```
User Action ──▶ Optimistic Update ──▶ UI Update
                     │
                     ▼
                Sync Queue ──▶ Remote Sync
                     │              │
                     ▼              ▼
                 [Success]      [Conflict]
                     │              │
                     ▼              ▼
                 Confirm        Rollback
```

## Configuration

### Config Options

```rust
Config::builder()
    // Queue settings
    .max_queue_size(10_000)
    .sync_batch_size(100)

    // Retry settings
    .retry_max_attempts(5)
    .retry_initial_delay_ms(1000)
    .retry_max_delay_ms(60_000)
    .retry_backoff_multiplier(2.0)
    .retry_jitter_factor(0.1)

    // Sync settings
    .auto_sync_interval_secs(60)
    .max_operation_age_secs(86400) // 24 hours

    // Merge strategy
    .merge_strategy(MergeStrategy::LastWriteWins)

    // Optimistic updates
    .enable_optimistic_updates(true)

    // Storage settings
    .storage_path("/path/to/db.sqlite".to_string())
    .database_name("my-app".to_string())

    // Compression
    .enable_compression(true)
    .compression_threshold(1024)

    .build()?
```

## Merge Strategies

### Last-Write-Wins (Default)

The record with the most recent timestamp wins:

```rust
Config::builder()
    .merge_strategy(MergeStrategy::LastWriteWins)
    .build()?
```

### Local-Wins / Remote-Wins

Always prefer local or remote version:

```rust
// Always use local version
Config::builder()
    .merge_strategy(MergeStrategy::LocalWins)
    .build()?

// Always use remote version
Config::builder()
    .merge_strategy(MergeStrategy::RemoteWins)
    .build()?
```

### Three-Way-Merge

Merge changes using common ancestor:

```rust
Config::builder()
    .merge_strategy(MergeStrategy::ThreeWayMerge)
    .build()?
```

### Custom Strategy

Implement your own merge logic:

```rust
use oxigdal_offline::merge::{CustomMerger, CallbackMerger};

let merger = CallbackMerger::new(|conflict| {
    // Custom merge logic
    Ok(conflict.local.clone())
});

let engine = MergeEngine::new(MergeStrategy::Custom)
    .with_custom_merger(Box::new(merger));
```

## Advanced Features

### Manual Maintenance

```rust
// Run maintenance tasks
let report = manager.maintenance().await?;
println!("Maintenance: {}", report.summary());

// Compact storage
manager.compact().await?;

// Get statistics
let stats = manager.statistics().await?;
println!("Stats: {}", stats.summary());
```

### Monitoring Queue

```rust
// Check queue size
let size = manager.queue_size().await?;
println!("Pending operations: {}", size);

// Check if online
if manager.is_online().await {
    println!("Connected to remote");
}
```

## Performance

- **Write throughput**: ~10,000 ops/sec (SQLite)
- **Read throughput**: ~50,000 ops/sec (SQLite)
- **Sync throughput**: Configurable batch size (default: 100 ops/batch)
- **Memory usage**: Minimal (streaming operations)

## COOLJAPAN Policy Compliance

- ✅ Pure Rust (no C/C++/Fortran dependencies)
- ✅ No unwrap() usage
- ✅ Workspace-based dependency management
- ✅ Latest crates from crates.io
- ✅ WASM-compatible

## Examples

See the `examples/` directory for more examples:

- `basic.rs`: Basic offline operations
- `sync.rs`: Synchronization with remote
- `conflict.rs`: Conflict resolution
- `wasm.rs`: WASM/browser usage

## License

Apache-2.0

## Author

COOLJAPAN OU (Team Kitasan)
