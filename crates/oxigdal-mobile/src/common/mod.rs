//! Common mobile utilities shared between iOS and Android.
//!
//! This module provides platform-agnostic utilities for mobile platforms,
//! including caching, tile management, and memory optimization.

pub mod cache;
pub mod tiles;

use crate::ffi::types::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Global statistics for mobile operations.
static TOTAL_BYTES_READ: AtomicUsize = AtomicUsize::new(0);
static TOTAL_TILES_CACHED: AtomicUsize = AtomicUsize::new(0);
static CACHE_HITS: AtomicUsize = AtomicUsize::new(0);
static CACHE_MISSES: AtomicUsize = AtomicUsize::new(0);

/// Mobile performance statistics.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MobileStats {
    /// Total bytes read from disk/network
    pub total_bytes_read: usize,
    /// Number of tiles currently cached
    pub tiles_cached: usize,
    /// Number of cache hits
    pub cache_hits: usize,
    /// Number of cache misses
    pub cache_misses: usize,
    /// Cache hit ratio (0.0 to 1.0)
    pub cache_hit_ratio: f64,
}

/// Gets current mobile performance statistics.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_mobile_get_stats(out_stats: *mut MobileStats) -> OxiGdalErrorCode {
    if out_stats.is_null() {
        crate::ffi::error::set_last_error("Null pointer for out_stats".to_string());
        return OxiGdalErrorCode::NullPointer;
    }

    let hits = CACHE_HITS.load(Ordering::Relaxed);
    let misses = CACHE_MISSES.load(Ordering::Relaxed);
    let total = hits + misses;
    let hit_ratio = if total > 0 {
        hits as f64 / total as f64
    } else {
        0.0
    };

    let stats = MobileStats {
        total_bytes_read: TOTAL_BYTES_READ.load(Ordering::Relaxed),
        tiles_cached: TOTAL_TILES_CACHED.load(Ordering::Relaxed),
        cache_hits: hits,
        cache_misses: misses,
        cache_hit_ratio: hit_ratio,
    };

    unsafe {
        *out_stats = stats;
    }

    OxiGdalErrorCode::Success
}

/// Resets mobile performance statistics.
#[unsafe(no_mangle)]
pub extern "C" fn oxigdal_mobile_reset_stats() -> OxiGdalErrorCode {
    TOTAL_BYTES_READ.store(0, Ordering::Relaxed);
    TOTAL_TILES_CACHED.store(0, Ordering::Relaxed);
    CACHE_HITS.store(0, Ordering::Relaxed);
    CACHE_MISSES.store(0, Ordering::Relaxed);
    OxiGdalErrorCode::Success
}

/// Records bytes read (internal use).
pub(crate) fn record_bytes_read(bytes: usize) {
    TOTAL_BYTES_READ.fetch_add(bytes, Ordering::Relaxed);
}

/// Records cache hit (internal use).
pub(crate) fn record_cache_hit() {
    CACHE_HITS.fetch_add(1, Ordering::Relaxed);
}

/// Records cache miss (internal use).
pub(crate) fn record_cache_miss() {
    CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
}

/// Updates tile cache count (internal use).
pub(crate) fn set_tiles_cached(count: usize) {
    TOTAL_TILES_CACHED.store(count, Ordering::Relaxed);
}

/// Optimizes memory usage based on available memory.
///
/// # Parameters
/// - `available_mb`: Available memory in megabytes
///
/// # Returns
/// - Success if optimizations applied
#[unsafe(no_mangle)]
pub extern "C" fn oxigdal_mobile_optimize_memory(
    available_mb: std::os::raw::c_int,
) -> OxiGdalErrorCode {
    if available_mb <= 0 {
        crate::ffi::error::set_last_error("Invalid available memory".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    // Adjust cache size based on available memory
    let cache_size_mb = if available_mb < 100 {
        // Very low memory - minimal cache
        10
    } else if available_mb < 300 {
        // Low memory - moderate cache
        50
    } else if available_mb < 500 {
        // Medium memory - larger cache
        100
    } else {
        // High memory - maximum cache
        200
    };

    // Apply cache size limit
    if let Err(e) = cache::set_max_cache_size_mb(cache_size_mb) {
        crate::ffi::error::set_last_error(e.to_string());
        return OxiGdalErrorCode::AllocationFailed;
    }

    OxiGdalErrorCode::Success
}

/// Enables or disables offline mode.
///
/// When offline mode is enabled, only cached data is used.
///
/// # Parameters
/// - `enabled`: 1 to enable, 0 to disable
#[unsafe(no_mangle)]
pub extern "C" fn oxigdal_mobile_set_offline_mode(
    enabled: std::os::raw::c_int,
) -> OxiGdalErrorCode {
    tiles::set_offline_mode(enabled != 0);
    OxiGdalErrorCode::Success
}

/// Prefetches tiles for a bounding box at given zoom levels.
///
/// This is useful for preparing offline use.
///
/// # Parameters
/// - `dataset`: Dataset handle
/// - `bbox`: Bounding box to prefetch
/// - `min_zoom`: Minimum zoom level
/// - `max_zoom`: Maximum zoom level
///
/// # Returns
/// - Number of tiles prefetched, or -1 on error
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_mobile_prefetch_tiles(
    _dataset: *const OxiGdalDataset,
    bbox: *const OxiGdalBbox,
    min_zoom: std::os::raw::c_int,
    max_zoom: std::os::raw::c_int,
) -> std::os::raw::c_int {
    if bbox.is_null() {
        crate::ffi::error::set_last_error("Null pointer for bbox".to_string());
        return -1;
    }

    if min_zoom < 0 || max_zoom < min_zoom || max_zoom > 22 {
        crate::ffi::error::set_last_error("Invalid zoom levels".to_string());
        return -1;
    }

    let bbox_ref = unsafe { &*bbox };

    // Validate bounding box
    if bbox_ref.min_x > bbox_ref.max_x || bbox_ref.min_y > bbox_ref.max_y {
        crate::ffi::error::set_last_error("Invalid bounding box: min > max".to_string());
        return -1;
    }

    // Validate bounding box is within valid geographic range
    if bbox_ref.min_x < -180.0
        || bbox_ref.max_x > 180.0
        || bbox_ref.min_y < -85.051129
        || bbox_ref.max_y > 85.051129
    {
        crate::ffi::error::set_last_error(
            "Bounding box extends beyond valid Web Mercator range".to_string(),
        );
        return -1;
    }

    // Calculate total tiles to prefetch to check feasibility
    let mut total_tile_count: i64 = 0;
    for zoom in min_zoom..=max_zoom {
        let zoom_tiles = tiles::tiles_for_bbox(bbox_ref, zoom);
        total_tile_count += zoom_tiles.len() as i64;
    }

    // Safety limit: don't try to prefetch more than 10,000 tiles at once
    const MAX_PREFETCH_TILES: i64 = 10_000;
    if total_tile_count > MAX_PREFETCH_TILES {
        crate::ffi::error::set_last_error(format!(
            "Too many tiles to prefetch: {} (max: {}). \
             Reduce the bounding box or zoom range.",
            total_tile_count, MAX_PREFETCH_TILES
        ));
        return -1;
    }

    let mut prefetched_count: i32 = 0;

    // Iterate through zoom levels and prefetch tiles
    for zoom in min_zoom..=max_zoom {
        let zoom_tiles = tiles::tiles_for_bbox(bbox_ref, zoom);

        for (tile_x, tile_y, tile_z) in &zoom_tiles {
            // Generate cache key for this tile
            let cache_key = format!("tile_{}_{}_{}", tile_z, tile_x, tile_y);

            // Check if already cached
            if cache::get_cached_tile(&cache_key).is_some() {
                // Already cached, skip
                prefetched_count += 1;
                continue;
            }

            // Check offline mode - in offline mode, we can only use cached data
            if tiles::is_offline_mode() {
                continue;
            }

            // Generate tile data from the dataset
            // Calculate the geographic bounds for this tile
            let (min_lon, min_lat, max_lon, max_lat) =
                tiles::tile_to_bbox(*tile_x, *tile_y, *tile_z);

            // Create a simple tile representation
            // In a full implementation, this would read the dataset at the
            // appropriate resolution and resample to tile size
            let tile_size = tiles::TILE_SIZE;
            let channels = 3; // RGB
            let tile_data_size = (tile_size * tile_size * channels) as usize;

            // Create a placeholder tile with geographic info encoded
            // The actual rendering would be done by the tile rendering pipeline
            let tile_data = vec![0u8; tile_data_size];

            // Cache the tile
            cache::put_cached_tile(cache_key, tile_data, tile_size, tile_size, channels);

            prefetched_count += 1;

            // Record statistics
            record_bytes_read(tile_data_size);
        }
    }

    prefetched_count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stats() {
        oxigdal_mobile_reset_stats();

        record_bytes_read(1024);
        record_cache_hit();
        record_cache_miss();
        set_tiles_cached(10);

        let mut stats = MobileStats {
            total_bytes_read: 0,
            tiles_cached: 0,
            cache_hits: 0,
            cache_misses: 0,
            cache_hit_ratio: 0.0,
        };

        let result = unsafe { oxigdal_mobile_get_stats(&mut stats) };
        assert_eq!(result, OxiGdalErrorCode::Success);
        assert_eq!(stats.total_bytes_read, 1024);
        assert_eq!(stats.tiles_cached, 10);
        assert_eq!(stats.cache_hits, 1);
        assert_eq!(stats.cache_misses, 1);
        assert!((stats.cache_hit_ratio - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_memory_optimization() {
        let result = oxigdal_mobile_optimize_memory(100);
        assert_eq!(result, OxiGdalErrorCode::Success);

        let result = oxigdal_mobile_optimize_memory(500);
        assert_eq!(result, OxiGdalErrorCode::Success);

        let result = oxigdal_mobile_optimize_memory(-1);
        assert_eq!(result, OxiGdalErrorCode::InvalidArgument);
    }

    #[test]
    fn test_offline_mode() {
        let result = oxigdal_mobile_set_offline_mode(1);
        assert_eq!(result, OxiGdalErrorCode::Success);

        let result = oxigdal_mobile_set_offline_mode(0);
        assert_eq!(result, OxiGdalErrorCode::Success);
    }

    #[test]
    fn test_prefetch_tiles_invalid_bbox() {
        // null bbox
        let result =
            unsafe { oxigdal_mobile_prefetch_tiles(std::ptr::null(), std::ptr::null(), 0, 5) };
        assert_eq!(result, -1);
    }

    #[test]
    fn test_prefetch_tiles_invalid_zoom() {
        let bbox = OxiGdalBbox {
            min_x: -1.0,
            min_y: -1.0,
            max_x: 1.0,
            max_y: 1.0,
        };

        // Invalid zoom (negative)
        let result = unsafe { oxigdal_mobile_prefetch_tiles(std::ptr::null(), &bbox, -1, 5) };
        assert_eq!(result, -1);

        // Invalid zoom (max < min)
        let result = unsafe { oxigdal_mobile_prefetch_tiles(std::ptr::null(), &bbox, 5, 2) };
        assert_eq!(result, -1);
    }

    #[test]
    fn test_prefetch_tiles_small_bbox() {
        // Initialize cache
        let _ = cache::init_cache(50);
        oxigdal_mobile_reset_stats();

        let bbox = OxiGdalBbox {
            min_x: -0.5,
            min_y: -0.5,
            max_x: 0.5,
            max_y: 0.5,
        };

        // Make sure offline mode is off
        let _ = oxigdal_mobile_set_offline_mode(0);

        let result = unsafe { oxigdal_mobile_prefetch_tiles(std::ptr::null(), &bbox, 0, 2) };
        assert!(result >= 0);
    }

    #[test]
    fn test_prefetch_tiles_invalid_geo_bbox() {
        let bbox = OxiGdalBbox {
            min_x: 10.0,
            min_y: 5.0,
            max_x: 5.0, // min > max
            max_y: 10.0,
        };

        let result = unsafe { oxigdal_mobile_prefetch_tiles(std::ptr::null(), &bbox, 0, 2) };
        assert_eq!(result, -1);
    }

    #[test]
    fn test_prefetch_tiles_offline_mode() {
        // Initialize cache
        let _ = cache::init_cache(50);
        oxigdal_mobile_reset_stats();

        let bbox = OxiGdalBbox {
            min_x: -0.5,
            min_y: -0.5,
            max_x: 0.5,
            max_y: 0.5,
        };

        // Enable offline mode - should only return cached tiles
        let _ = oxigdal_mobile_set_offline_mode(1);

        let result = unsafe { oxigdal_mobile_prefetch_tiles(std::ptr::null(), &bbox, 0, 1) };
        // In offline mode with empty cache, no tiles should be prefetched
        assert_eq!(result, 0);

        // Restore offline mode
        let _ = oxigdal_mobile_set_offline_mode(0);
    }
}
