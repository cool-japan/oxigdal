//! Tutorial 10: Mobile Integration
//!
//! This tutorial demonstrates the platform-agnostic mobile utilities in
//! `oxigdal-mobile` that back the iOS/Android FFI layer:
//! - XYZ tile coordinate math (`common::tiles`)
//! - Offline-mode toggling
//! - LRU tile caching (`common::cache`)
//! - Resolution-at-zoom calculations
//!
//! Run with:
//! ```bash
//! cargo run --example tutorial_10_mobile_integration
//! ```

use oxigdal_mobile::common::cache::{
    get_cached_tile, init_cache, put_cached_tile, set_max_cache_size_mb,
};
use oxigdal_mobile::common::tiles::{
    TILE_SIZE, is_offline_mode, lonlat_to_tile, resolution_at_zoom, set_offline_mode, tile_to_bbox,
    tiles_for_bbox,
};
use oxigdal_mobile::ffi::types::OxiGdalBbox;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Tutorial 10: Mobile Integration ===\n");

    // Step 1: Tile Coordinate Math
    println!("Step 1: XYZ Tile Coordinate Math");
    println!("----------------------------------");

    let lon = -122.4194;
    let lat = 37.7749;
    let zoom = 12;

    let (tile_x, tile_y) = lonlat_to_tile(lon, lat, zoom);
    println!("San Francisco ({lon}, {lat}) at zoom {zoom}:");
    println!("  Tile: ({tile_x}, {tile_y})");
    println!("  Tile size: {TILE_SIZE}px");

    let (min_lon, min_lat, max_lon, max_lat) = tile_to_bbox(tile_x, tile_y, zoom);
    println!(
        "  Tile bounds: [{:.4}, {:.4}, {:.4}, {:.4}]",
        min_lon, min_lat, max_lon, max_lat
    );

    let resolution = resolution_at_zoom(lat, zoom);
    println!("  Ground resolution: {:.2} m/pixel", resolution);

    // Step 2: Tiles covering a bounding box
    println!("\n\nStep 2: Tiles Covering a Region");
    println!("---------------------------------");

    let bbox = OxiGdalBbox {
        min_x: -122.52,
        min_y: 37.70,
        max_x: -122.35,
        max_y: 37.82,
    };

    for test_zoom in [8, 10, 12] {
        let tiles = tiles_for_bbox(&bbox, test_zoom);
        println!("  Zoom {test_zoom}: {} tiles needed", tiles.len());
    }

    // Step 3: Offline mode
    println!("\n\nStep 3: Offline Mode");
    println!("---------------------");

    println!("Offline mode (initial): {}", is_offline_mode());
    set_offline_mode(true);
    println!("Offline mode (after enabling): {}", is_offline_mode());
    set_offline_mode(false);

    // Step 4: Tile caching
    println!("\n\nStep 4: Mobile-Optimized Tile Cache");
    println!("--------------------------------------");

    init_cache(32).map_err(|e| format!("failed to init cache: {e}"))?;
    set_max_cache_size_mb(64).map_err(|e| format!("failed to resize cache: {e}"))?;
    println!("Initialized LRU tile cache (max 64 MB)");

    let key = format!("{zoom}/{tile_x}/{tile_y}");
    let fake_tile_data = vec![0u8; (TILE_SIZE * TILE_SIZE * 4) as usize];

    println!(
        "Cache lookup before insert: {:?}",
        get_cached_tile(&key).map(|(d, ..)| d.len())
    );

    put_cached_tile(key.clone(), fake_tile_data.clone(), TILE_SIZE, TILE_SIZE, 4);
    println!("Cached tile: {key} ({} bytes)", fake_tile_data.len());

    if let Some((data, width, height, channels)) = get_cached_tile(&key) {
        println!(
            "Cache hit: {}x{} tile, {} channels, {} bytes",
            width,
            height,
            channels,
            data.len()
        );
    }

    // Step 5: Battery/Storage-conscious deployment notes
    println!("\n\nStep 5: Deployment Considerations");
    println!("------------------------------------");

    println!("1. Edge Deployment:");
    println!("   - Mobile devices (iOS/Android) via the `oxigdal-mobile` C FFI layer");
    println!("   - `oxigdal-mobile-enhanced` adds battery/network/storage helpers");
    println!("   - Considerations: model size, latency, battery");

    println!("\n2. Offline-first design:");
    println!("   - Toggle offline mode with `set_offline_mode`");
    println!("   - Pre-warm the tile cache before going offline");
    println!("   - Persist cached tiles to disk for cross-session reuse");

    // Summary
    println!("\n\n=== Tutorial Complete! ===");
    println!("\nTopics Covered:");
    println!("  1. XYZ tile coordinate math");
    println!("  2. Computing tiles that cover a bounding box");
    println!("  3. Offline mode toggling");
    println!("  4. Mobile-optimized LRU tile caching");
    println!("  5. Deployment considerations");

    println!("\nKey Points:");
    println!("  - `oxigdal-mobile::common` is the platform-agnostic core reused by");
    println!("    the iOS and Android FFI bindings");
    println!("  - Tile math and caching are pure functions/free functions, easy to test");
    println!("  - Enable the `ios`/`android` features for the platform FFI surface");

    Ok(())
}
