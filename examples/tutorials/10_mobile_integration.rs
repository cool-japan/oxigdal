//! Tutorial 10: Mobile Integration (iOS/Android)
//!
//! This tutorial demonstrates integrating OxiGDAL with mobile platforms:
//! - iOS integration via Swift
//! - Android integration via Kotlin/JNI
//! - Mobile-optimized operations
//! - Offline mapping
//! - Location-based services
//! - Battery and performance considerations
//!
//! Run with:
//! ```bash
//! cargo run --example 10_mobile_integration
//! ```
//!
//! Note: This example demonstrates the Rust side; see docs for platform-specific code

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_mobile::cache::{MobileCache, MobileCacheConfig};
use oxigdal_mobile::location::{LocationService, LocationUpdate};
use oxigdal_mobile::offline::{OfflineMap, TilePackage};
use oxigdal_mobile::optimization::{BatteryOptimizer, MemoryManager};
use oxigdal_mobile::tile::{MobileTileServer, TileFormat};
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Tutorial 10: Mobile Integration ===\n");

    // Step 1: Mobile-Specific Challenges
    println!("Step 1: Mobile Challenges");
    println!("-------------------------");

    println!("Key considerations for mobile deployment:");

    println!("\n1. Limited Resources:");
    println!("   - Memory: Typically 1-8 GB");
    println!("   - Storage: Often limited, user-controlled");
    println!("   - CPU: Less powerful than desktop");
    println!("   - Battery: Critical constraint");

    println!("\n2. Network Connectivity:");
    println!("   - Intermittent connection");
    println!("   - Variable bandwidth");
    println!("   - Data usage limits");
    println!("   - Need for offline capability");

    println!("\n3. Platform Differences:");
    println!("   - iOS: Swift, Objective-C");
    println!("   - Android: Kotlin, Java");
    println!("   - Different permission models");
    println!("   - Platform-specific APIs");

    // Step 2: iOS Integration
    println!("\n\nStep 2: iOS Integration");
    println!("-----------------------");

    println!("Building OxiGDAL for iOS:");

    println!("\n1. Setup Cargo:");
    println!("   # Add iOS targets");
    println!("   rustup target add aarch64-apple-ios");
    println!("   rustup target add aarch64-apple-ios-sim");
    println!("   rustup target add x86_64-apple-ios");

    println!("\n2. Build for iOS:");
    println!("   # Device (ARM64)");
    println!("   cargo build --release --target aarch64-apple-ios");
    println!();
    println!("   # Simulator");
    println!("   cargo build --release --target aarch64-apple-ios-sim");

    println!("\n3. Create XCFramework:");
    println!("   xcodebuild -create-xcframework \\");
    println!("     -library target/aarch64-apple-ios/release/liboxigdal.a \\");
    println!("     -library target/aarch64-apple-ios-sim/release/liboxigdal.a \\");
    println!("     -output OxiGDAL.xcframework");

    println!("\n4. Swift Integration:");
    println!("   ```swift");
    println!("   import OxiGDAL");
    println!();
    println!("   // Initialize");
    println!("   let gdal = OxiGDAL.shared");
    println!();
    println!("   // Load map tile");
    println!("   gdal.loadTile(z: 10, x: 500, y: 300) {{ tile, error in");
    println!("       guard let tile = tile else {{ return }}");
    println!("       self.displayTile(tile)");
    println!("   }}");
    println!("   ```");

    // Step 3: Android Integration
    println!("\n\nStep 3: Android Integration");
    println!("---------------------------");

    println!("Building OxiGDAL for Android:");

    println!("\n1. Setup NDK:");
    println!("   # Add Android targets");
    println!("   rustup target add aarch64-linux-android");
    println!("   rustup target add armv7-linux-androideabi");
    println!("   rustup target add x86_64-linux-android");

    println!("\n2. Configure cargo-ndk:");
    println!("   cargo install cargo-ndk");

    println!("\n3. Build for Android:");
    println!("   # ARM64 (most modern devices)");
    println!("   cargo ndk -t arm64-v8a build --release");
    println!();
    println!("   # ARMv7 (older devices)");
    println!("   cargo ndk -t armeabi-v7a build --release");

    println!("\n4. Kotlin Integration:");
    println!("   ```kotlin");
    println!("   // Load native library");
    println!("   companion object {{");
    println!("       init {{");
    println!("           System.loadLibrary(\"oxigdal\")");
    println!("       }}");
    println!("   }}");
    println!();
    println!("   // JNI declarations");
    println!("   external fun loadTile(z: Int, x: Int, y: Int): ByteArray");
    println!("   external fun processRaster(data: ByteArray): ByteArray");
    println!("   ```");

    // Step 4: Offline Map Support
    println!("\n\nStep 4: Offline Map Support");
    println!("---------------------------");

    println!("Creating offline map packages...");

    // Define region
    let region = BoundingBox::new(-122.5, 37.7, -122.3, 37.9)?; // San Francisco area

    println!("\nRegion of interest:");
    println!("  Bounds: [{:.4}, {:.4}, {:.4}, {:.4}]",
             region.min_x(), region.min_y(), region.max_x(), region.max_y());

    // Configure tile package
    let zoom_levels = vec![10, 11, 12, 13, 14, 15];

    println!("  Zoom levels: {:?}", zoom_levels);

    // Calculate package size
    let mut total_tiles = 0;
    for zoom in &zoom_levels {
        let tiles_x = (2_u32.pow(*zoom) as f64 * (region.max_x() - region.min_x()) / 360.0) as u32;
        let tiles_y = (2_u32.pow(*zoom) as f64 * (region.max_y() - region.min_y()) / 180.0) as u32;
        let tiles = tiles_x * tiles_y;
        total_tiles += tiles;

        println!("    Level {}: {}x{} = {} tiles", zoom, tiles_x, tiles_y, tiles);
    }

    println!("\nTotal tiles: {}", total_tiles);
    println!("Estimated size: {:.2} MB (assuming 20 KB/tile)",
             total_tiles as f64 * 20.0 / 1024.0);

    // Create tile package
    println!("\nCreating tile package...");

    let package_config = TilePackageConfig {
        name: "San Francisco".to_string(),
        region,
        zoom_levels,
        tile_format: TileFormat::Png,
        compression: true,
    };

    println!("  Format: {:?}", package_config.tile_format);
    println!("  Compression: {}", package_config.compression);

    // In production:
    // let package = TilePackage::create(&package_config)?;
    // package.download_tiles().await?;
    // package.save("sf_offline.mbtiles")?;

    println!("\nPackage creation would:");
    println!("  1. Download all tiles from server");
    println!("  2. Compress and optimize");
    println!("  3. Store in MBTiles format");
    println!("  4. Generate metadata and indices");

    // Step 5: Mobile Cache Configuration
    println!("\n\nStep 5: Mobile Cache Configuration");
    println!("-----------------------------------");

    let cache_config = MobileCacheConfig {
        max_memory_mb: 128,        // Conservative for mobile
        max_disk_mb: 512,          // User can configure
        eviction_policy: "LRU".to_string(),
        preload_viewport: true,
        compress_cache: true,
    };

    println!("Mobile cache configuration:");
    println!("  Max memory: {} MB", cache_config.max_memory_mb);
    println!("  Max disk: {} MB", cache_config.max_disk_mb);
    println!("  Eviction: {}", cache_config.eviction_policy);
    println!("  Preload viewport: {}", cache_config.preload_viewport);
    println!("  Compression: {}", cache_config.compress_cache);

    let cache = MobileCache::new(cache_config)?;

    println!("\nCache strategies:");
    println!("  - Memory cache for immediate viewport");
    println!("  - Disk cache for visited areas");
    println!("  - Aggressive eviction on low memory");
    println!("  - Prefetch based on movement direction");

    // Step 6: Location Services
    println!("\n\nStep 6: Location Services");
    println!("-------------------------");

    println!("Integrating with device location...");

    // Location service configuration
    let location_config = LocationServiceConfig {
        accuracy: "balanced".to_string(),   // balanced, high, low
        update_interval_ms: 1000,           // 1 second
        min_distance_meters: 10.0,          // 10 meters
        background_updates: false,          // Battery consideration
    };

    println!("\nLocation configuration:");
    println!("  Accuracy: {}", location_config.accuracy);
    println!("  Update interval: {} ms", location_config.update_interval_ms);
    println!("  Min distance: {} m", location_config.min_distance_meters);
    println!("  Background: {}", location_config.background_updates);

    // Simulate location update
    let location = LocationUpdate {
        latitude: 37.7749,
        longitude: -122.4194,
        altitude: 10.0,
        accuracy: 5.0,
        timestamp: chrono::Utc::now(),
    };

    println!("\nCurrent location:");
    println!("  Lat: {:.6}", location.latitude);
    println!("  Lon: {:.6}", location.longitude);
    println!("  Altitude: {:.1} m", location.altitude);
    println!("  Accuracy: {:.1} m", location.accuracy);

    // Query nearby tiles
    println!("\nQuerying nearby tiles at zoom 14...");

    let zoom = 14;
    let tile_x = lon_to_tile_x(location.longitude, zoom);
    let tile_y = lat_to_tile_y(location.latitude, zoom);

    println!("  Current tile: ({}, {}) at z={}", tile_x, tile_y, zoom);

    // Prefetch surrounding tiles
    let prefetch_radius = 2;
    let mut prefetch_tiles = Vec::new();

    for dy in -prefetch_radius..=prefetch_radius {
        for dx in -prefetch_radius..=prefetch_radius {
            prefetch_tiles.push((tile_x + dx, tile_y + dy, zoom));
        }
    }

    println!("  Prefetching {} tiles", prefetch_tiles.len());

    // Step 7: Battery Optimization
    println!("\n\nStep 7: Battery Optimization");
    println!("-----------------------------");

    println!("Battery-aware strategies:");

    println!("\n1. Adaptive Quality:");
    println!("   - Reduce tile resolution when battery low");
    println!("   - Lower update frequency");
    println!("   - Defer non-critical operations");

    println!("\n2. Network Usage:");
    println!("   - Batch tile downloads");
    println!("   - Use WiFi when available");
    println!("   - Respect user data settings");
    println!("   - Cache aggressively");

    println!("\n3. GPS Usage:");
    println!("   - Use coarse location when possible");
    println!("   - Increase update interval");
    println!("   - Disable in background");
    println!("   - Geofencing for triggers");

    println!("\n4. Computation:");
    println!("   - Defer heavy processing");
    println!("   - Use GPU wisely (overhead)");
    println!("   - Batch operations");
    println!("   - Offload to server when possible");

    let battery_optimizer = BatteryOptimizer::new();

    let battery_level = 0.45; // 45%
    let charging = false;

    println!("\nBattery status:");
    println!("  Level: {}%", (battery_level * 100.0) as u32);
    println!("  Charging: {}", charging);

    let quality_level = battery_optimizer.recommend_quality(battery_level, charging);

    println!("\nRecommended settings:");
    println!("  Quality: {}", quality_level);
    println!("  Update interval: {} ms", quality_level.update_interval);
    println!("  Prefetch radius: {} tiles", quality_level.prefetch_radius);

    // Step 8: Memory Management
    println!("\n\nStep 8: Memory Management");
    println!("-------------------------");

    println!("Mobile memory constraints:");

    let memory_manager = MemoryManager::new();

    let total_memory_mb = 4096; // 4 GB device
    let available_memory_mb = 1024; // 1 GB available

    println!("  Total device memory: {} MB", total_memory_mb);
    println!("  Available memory: {} MB", available_memory_mb);

    let recommended_cache = memory_manager.recommend_cache_size(
        available_memory_mb,
        total_memory_mb,
    );

    println!("\nRecommended cache sizes:");
    println!("  Memory cache: {} MB", recommended_cache.memory_mb);
    println!("  Disk cache: {} MB", recommended_cache.disk_mb);

    println!("\nMemory warnings:");

    let warning_threshold = 200; // MB
    let critical_threshold = 100; // MB

    if available_memory_mb < critical_threshold {
        println!("  CRITICAL: Memory extremely low");
        println!("  Actions: Purge all caches, reduce quality, warn user");
    } else if available_memory_mb < warning_threshold {
        println!("  WARNING: Memory low");
        println!("  Actions: Clear disk cache, reduce prefetch");
    } else {
        println!("  OK: Memory sufficient");
    }

    // Step 9: Platform-Specific Features
    println!("\n\nStep 9: Platform-Specific Features");
    println!("-----------------------------------");

    println!("\niOS Specific:");
    println!("  - MapKit integration");
    println!("  - Core Location services");
    println!("  - Background location updates");
    println!("  - Significant location change");
    println!("  - Region monitoring (geofencing)");

    println!("\nAndroid Specific:");
    println!("  - Google Maps integration");
    println!("  - Fused Location Provider");
    println!("  - WorkManager for background tasks");
    println!("  - Geofencing API");
    println!("  - Doze mode handling");

    // Step 10: Best Practices
    println!("\n\nStep 10: Best Practices");
    println!("-----------------------");

    println!("\n1. User Experience:");
    println!("   - Show loading indicators");
    println!("   - Handle offline gracefully");
    println!("   - Smooth map interactions");
    println!("   - Quick startup time");

    println!("\n2. Privacy:");
    println!("   - Request permissions appropriately");
    println!("   - Explain location usage");
    println!("   - Allow offline-only mode");
    println!("   - Don't track without consent");

    println!("\n3. Data Usage:");
    println!("   - Compress transfers");
    println!("   - Use vector tiles when possible");
    println!("   - Offer download for offline");
    println!("   - Show data usage estimates");

    println!("\n4. Performance:");
    println!("   - Lazy loading");
    println!("   - Virtual scrolling for lists");
    println!("   - Optimize render pipeline");
    println!("   - Profile on real devices");

    println!("\n5. Testing:");
    println!("   - Test on low-end devices");
    println!("   - Test with poor connectivity");
    println!("   - Test battery drain");
    println!("   - Test memory pressure");

    // Summary
    println!("\n\n=== Tutorial Complete! ===");
    println!("\nTopics Covered:");
    println!("  1. Mobile deployment challenges");
    println!("  2. iOS integration (Swift/XCFramework)");
    println!("  3. Android integration (Kotlin/JNI)");
    println!("  4. Offline map packages");
    println!("  5. Mobile cache strategies");
    println!("  6. Location services integration");
    println!("  7. Battery optimization");
    println!("  8. Memory management");
    println!("  9. Platform-specific features");
    println!("  10. Best practices");

    println!("\nKey Takeaways:");
    println!("  - Mobile requires aggressive optimization");
    println!("  - Battery and data usage are critical");
    println!("  - Offline support is essential");
    println!("  - Platform-specific considerations matter");
    println!("  - Test on real devices with constraints");

    println!("\nNext Steps:");
    println!("  - Build a sample iOS app with OxiGDAL");
    println!("  - Build a sample Android app with OxiGDAL");
    println!("  - Explore the cookbook examples for real-world scenarios");
    println!("  - Check the migration guides in docs/");

    println!("\nCongratulations on completing all tutorials!");

    Ok(())
}

// Helper functions

struct TilePackageConfig {
    name: String,
    region: BoundingBox,
    zoom_levels: Vec<u32>,
    tile_format: TileFormat,
    compression: bool,
}

struct LocationServiceConfig {
    accuracy: String,
    update_interval_ms: u64,
    min_distance_meters: f64,
    background_updates: bool,
}

struct QualityLevel {
    update_interval: u64,
    prefetch_radius: i32,
}

struct CacheSize {
    memory_mb: u32,
    disk_mb: u32,
}

/// Convert longitude to tile X
fn lon_to_tile_x(lon: f64, zoom: u32) -> i32 {
    ((lon + 180.0) / 360.0 * (1 << zoom) as f64) as i32
}

/// Convert latitude to tile Y
fn lat_to_tile_y(lat: f64, zoom: u32) -> i32 {
    let lat_rad = lat.to_radians();
    ((1.0 - lat_rad.tan().asinh() / std::f64::consts::PI) / 2.0 * (1 << zoom) as f64) as i32
}
