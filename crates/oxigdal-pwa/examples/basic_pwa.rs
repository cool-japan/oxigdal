//! Basic PWA example demonstrating service worker registration and caching.

use oxigdal_pwa::{
    PwaApp, PwaConfig,
    cache::strategies::CacheStrategy,
    manifest::{DisplayMode, ManifestBuilder},
    notifications::{NotificationConfig, NotificationManager},
};

/// Main PWA application example.
#[allow(dead_code)]
async fn run_example() -> Result<(), Box<dyn std::error::Error>> {
    // Configure PWA
    let config = PwaConfig::new()
        .with_service_worker_url("/sw.js")
        .with_scope("/")
        .with_cache_management(true)
        .with_geospatial_cache(true)
        .with_notifications(true)
        .with_background_sync(true);

    // Create and initialize PWA app
    let mut app = PwaApp::new(config);
    app.initialize().await?;

    // Check if running as PWA
    if app.is_pwa() {
        web_sys::console::log_1(&"Running as PWA".into());
    } else {
        web_sys::console::log_1(&"Running in browser".into());
    }

    // Check if can install
    if app.can_install() {
        web_sys::console::log_1(&"Install prompt available".into());

        // Show install prompt
        if app.prompt_install().await? {
            web_sys::console::log_1(&"User accepted install".into());
        } else {
            web_sys::console::log_1(&"User dismissed install".into());
        }
    }

    // Set up caching strategies
    let static_cache = CacheStrategy::cache_first("static-v1");
    let api_cache = CacheStrategy::network_first("api-v1");
    let tile_cache = CacheStrategy::cache_first("tiles-v1");

    web_sys::console::log_1(
        &format!(
            "Cache strategies configured: {}, {}, {}",
            static_cache.config().cache_name,
            api_cache.config().cache_name,
            tile_cache.config().cache_name
        )
        .into(),
    );

    // Request notification permission
    if let Some(notification_manager) = app.notification_manager() {
        let permission = NotificationManager::request_permission().await?;

        if permission.is_granted() {
            // Show a notification
            let config = NotificationConfig::new("OxiGDAL PWA")
                .with_body("PWA initialized successfully!")
                .with_icon("/icons/icon-192x192.png")
                .with_tag("init");

            notification_manager.show(&config).await?;
        }
    }

    // Generate web app manifest
    let manifest = ManifestBuilder::geospatial("OxiGDAL PWA", "OxiGDAL")
        .description("Progressive Web App for geospatial data processing")
        .start_url("/")
        .display(DisplayMode::Standalone)
        .colors("#ffffff", "#007bff")
        .add_standard_icons("/icons")
        .build();

    let manifest_json = manifest.to_json()?;
    web_sys::console::log_1(&format!("Manifest generated: {}", manifest_json).into());

    // Cache geospatial tiles
    if let Some(_geo_cache) = app.geospatial_cache() {
        use oxigdal_pwa::cache::geospatial::{BoundingBox, TileCoord};

        // Cache a specific tile
        let tile = TileCoord::new(5, 10, 10);
        let tile_url = tile.to_url("https://tiles.example.com");

        web_sys::console::log_1(&format!("Caching tile: {}", tile_url).into());

        // In a real app, you would cache the tile here
        // geo_cache.cache_tile(&tile, &tile_url).await?;

        // Prefetch tiles for a region
        let bbox = BoundingBox::new(-122.5, 37.7, -122.3, 37.9)?;
        web_sys::console::log_1(&format!("Preparing to cache tiles for bbox: {:?}", bbox).into());

        // In a real app:
        // let tiles = geo_cache.prefetch_tiles(&bbox, 10..13, "https://tiles.example.com").await?;
        // web_sys::console::log_1(&format!("Cached {} tiles", tiles.len()).into());
    }

    // Set up background sync
    if let Some(sync) = app.sync_coordinator_mut() {
        use oxigdal_pwa::sync::{QueuedOperation, SyncQueue};

        // Create a sync queue for data uploads
        let mut upload_queue = SyncQueue::new("uploads");

        // Add an operation to the queue
        let operation = QueuedOperation::new(
            "upload-1",
            "upload-geojson",
            serde_json::json!({
                "file": "data.geojson",
                "destination": "/api/upload"
            }),
        );

        upload_queue.enqueue(operation);
        sync.add_queue(upload_queue);

        web_sys::console::log_1(
            &format!("Total queued operations: {}", sync.total_queued()).into(),
        );
    }

    Ok(())
}

fn main() {
    // This example is meant to be used in a WASM context
    #[cfg(target_arch = "wasm32")]
    {
        wasm_bindgen_futures::spawn_local(async {
            if let Err(e) = run_example().await {
                web_sys::console::error_1(&format!("Error: {}", e).into());
            }
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        println!("This example must be run in a WebAssembly environment");
    }
}
