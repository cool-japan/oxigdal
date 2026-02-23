# oxigdal-pwa

Progressive Web App capabilities for OxiGDAL - Build offline-capable geospatial applications.

## Features

- **Service Worker Integration**: Full service worker lifecycle management
- **Offline Caching**: Multiple caching strategies (cache-first, network-first, stale-while-revalidate)
- **Background Sync**: Queue operations for background synchronization
- **Push Notifications**: Complete push notification support
- **Web App Manifest**: Automated manifest generation
- **PWA Lifecycle**: Install prompts and update management
- **Geospatial Optimizations**: Tile caching and geospatial data handling

## Usage

### Basic PWA Setup

```rust
use oxigdal_pwa::{PwaApp, PwaConfig};

let config = PwaConfig::new()
    .with_service_worker_url("/sw.js")
    .with_cache_management(true)
    .with_geospatial_cache(true);

let mut app = PwaApp::new(config);
app.initialize().await?;
```

### Service Worker Registration

```rust
use oxigdal_pwa::service_worker::ServiceWorkerRegistry;

let registry = ServiceWorkerRegistry::with_script_url("/sw.js")
    .with_scope("/app");

let registration = registry.register().await?;
```

### Caching Strategies

```rust
use oxigdal_pwa::cache::strategies::CacheStrategy;

// Cache-first for static assets
let static_cache = CacheStrategy::cache_first("static-v1");

// Network-first for API calls
let api_cache = CacheStrategy::network_first("api-v1");

// Stale-while-revalidate for dynamic content
let dynamic_cache = CacheStrategy::stale_while_revalidate("dynamic-v1");
```

### Geospatial Tile Caching

```rust
use oxigdal_pwa::cache::geospatial::{GeospatialCache, BoundingBox};

let cache = GeospatialCache::with_defaults();

// Prefetch tiles for a region
let bbox = BoundingBox::new(-122.5, 37.7, -122.3, 37.9)?;
let tiles = cache.prefetch_tiles(&bbox, 10..13, "https://tiles.example.com").await?;
```

### Push Notifications

```rust
use oxigdal_pwa::notifications::{NotificationManager, NotificationConfig};

let manager = NotificationManager::new();
let permission = NotificationManager::request_permission().await?;

if permission.is_granted() {
    let config = NotificationConfig::new("New Data Available")
        .with_body("Your geospatial data has been updated")
        .with_icon("/icon.png");

    manager.show(&config).await?;
}
```

### Web App Manifest

```rust
use oxigdal_pwa::manifest::{ManifestBuilder, DisplayMode};

let manifest = ManifestBuilder::geospatial("GeoApp", "Geo")
    .description("A powerful geospatial PWA")
    .colors("#ffffff", "#007bff")
    .add_standard_icons("/icons")
    .build();

let json = manifest.to_json()?;
```

### Background Sync

```rust
use oxigdal_pwa::sync::{BackgroundSync, SyncOptions};

let sync = BackgroundSync::new(registration);
let options = SyncOptions::new("upload-data");

sync.register(&options).await?;
```

## Service Worker Template

A complete service worker template is provided in `templates/service-worker.js`. It includes:

- Install and activate lifecycle events
- Multiple caching strategies
- Background sync handling
- Push notification support
- Message handling for cache management

## Examples

See `examples/basic_pwa.rs` for a complete PWA application example.

## COOLJAPAN Policies

- ✅ Pure Rust implementation
- ✅ WASM-compatible
- ✅ No `unwrap()` usage
- ✅ Comprehensive error handling

## License

Apache-2.0
