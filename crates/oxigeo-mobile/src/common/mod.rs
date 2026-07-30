//! Common mobile utilities shared between iOS and Android.
//!
//! This module provides platform-agnostic utilities for mobile platforms,
//! including caching, tile management, and memory optimization.

pub mod cache;
pub(crate) mod tile_read;
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
pub unsafe extern "C" fn oxigeo_mobile_get_stats(out_stats: *mut MobileStats) -> OxiGeoErrorCode {
    if out_stats.is_null() {
        crate::ffi::error::set_last_error("Null pointer for out_stats".to_string());
        return OxiGeoErrorCode::NullPointer;
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

    OxiGeoErrorCode::Success
}

/// Resets mobile performance statistics.
#[unsafe(no_mangle)]
pub extern "C" fn oxigeo_mobile_reset_stats() -> OxiGeoErrorCode {
    TOTAL_BYTES_READ.store(0, Ordering::Relaxed);
    TOTAL_TILES_CACHED.store(0, Ordering::Relaxed);
    CACHE_HITS.store(0, Ordering::Relaxed);
    CACHE_MISSES.store(0, Ordering::Relaxed);
    OxiGeoErrorCode::Success
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
pub extern "C" fn oxigeo_mobile_optimize_memory(
    available_mb: std::os::raw::c_int,
) -> OxiGeoErrorCode {
    if available_mb <= 0 {
        crate::ffi::error::set_last_error("Invalid available memory".to_string());
        return OxiGeoErrorCode::InvalidArgument;
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
        return OxiGeoErrorCode::AllocationFailed;
    }

    OxiGeoErrorCode::Success
}

/// Enables or disables offline mode.
///
/// When offline mode is enabled, only cached data is used.
///
/// # Parameters
/// - `enabled`: 1 to enable, 0 to disable
#[unsafe(no_mangle)]
pub extern "C" fn oxigeo_mobile_set_offline_mode(enabled: std::os::raw::c_int) -> OxiGeoErrorCode {
    tiles::set_offline_mode(enabled != 0);
    OxiGeoErrorCode::Success
}

/// Prefetches tiles for a bounding box at given zoom levels.
///
/// This is useful for preparing offline use. Each tile is populated with
/// **real** pixel data read from `dataset` (via the same GeoTIFF overview
/// pipeline used by [`crate::ffi::raster::oxigeo_dataset_read_tile`]), not a
/// placeholder buffer, so the offline tile cache is actually usable once the
/// device goes offline.
///
/// # Parameters
/// - `dataset`: Dataset handle (must be open for reading; obtained from
///   `oxigeo_dataset_open`)
/// - `bbox`: Bounding box to prefetch
/// - `min_zoom`: Minimum zoom level
/// - `max_zoom`: Maximum zoom level
///
/// # Returns
/// - Number of tiles actually populated with real pixel data and cached, or
///   -1 on error. Tiles that fail to read (e.g. because they fall outside the
///   dataset's actual raster extent) are skipped and not counted.
///
/// # Safety
/// - `dataset` must be a valid, non-null handle returned by
///   `oxigeo_dataset_open` and must remain valid for the duration of this call
/// - `bbox` must be a valid pointer to an `OxiGeoBbox`
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigeo_mobile_prefetch_tiles(
    dataset: *const OxiGeoDataset,
    bbox: *const OxiGeoBbox,
    min_zoom: std::os::raw::c_int,
    max_zoom: std::os::raw::c_int,
) -> std::os::raw::c_int {
    if dataset.is_null() {
        crate::ffi::error::set_last_error("Null pointer for dataset".to_string());
        return -1;
    }

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

    // SAFETY: `dataset` was checked non-null above; the caller contract
    // requires it to be a valid handle from `oxigeo_dataset_open`, matching
    // the same opaque-pointer-to-`DatasetHandle` convention used throughout
    // `ffi::raster` (e.g. `oxigeo_dataset_read_tile`).
    let handle = unsafe { &*(dataset as *const crate::ffi::raster::DatasetHandle) };

    let reader_mutex = match &handle.reader {
        Some(r) => r,
        None => {
            crate::ffi::error::set_last_error(
                "Dataset not open for reading; cannot prefetch tiles".to_string(),
            );
            return -1;
        }
    };

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

            if *tile_x < 0 || *tile_y < 0 {
                // Tiles outside the valid XYZ coordinate space cannot be read;
                // skip rather than fabricate data for them.
                continue;
            }

            // Read the real dataset pixels for this tile through the same
            // overview-selection pipeline used by `oxigeo_dataset_read_tile`:
            // higher zoom levels use less-downsampled overviews.
            let reader = match reader_mutex.lock() {
                Ok(r) => r,
                Err(e) => {
                    crate::ffi::error::set_last_error(format!("Failed to lock reader: {}", e));
                    return -1;
                }
            };

            let overview_level = if *tile_z < reader.overview_count() as i32 {
                (reader.overview_count() as i32 - 1 - *tile_z).max(0) as usize
            } else {
                0
            };

            // Band-aware: `read_tile` would hand back one raw block, which on a
            // `PlanarConfiguration = 2` file is a single band's plane, and the
            // cache entry would then claim `band_count` channels for it. See
            // [`tile_read`] (cool-japan/oxigeo#14).
            let tile = match tile_read::read_block_interleaved(
                &reader,
                overview_level,
                *tile_x as u32,
                *tile_y as u32,
            ) {
                Ok(tile) => tile,
                Err(_) => {
                    // This tile falls outside the dataset's real raster
                    // extent (or the overview read failed for some other
                    // reason). Skip it rather than caching fabricated pixels.
                    drop(reader);
                    continue;
                }
            };
            drop(reader);

            let tile_data_size = tile.data.len();

            // Cache the real tile data, labelled with the block's own geometry.
            cache::put_cached_tile(cache_key, tile.data, tile.width, tile.height, tile.channels);

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
    use crate::ffi::raster::{
        oxigeo_dataset_close, oxigeo_dataset_create, oxigeo_dataset_flush, oxigeo_dataset_open,
        oxigeo_dataset_write_region,
    };
    use crate::ffi::types::OxiGeoDataType;
    use std::ffi::CString;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Per-test scratch fixture inside the system temp dir (house policy: no
    /// hardcoded absolute paths).
    ///
    /// The leaf name embeds the process id and a monotonic counter, so no two
    /// test binaries — nor two concurrent runs of this one — can ever land on
    /// the same file.  Dropping the guard removes the fixture, so a panicking
    /// test leaks nothing.
    struct TempPath(std::path::PathBuf);

    impl TempPath {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!(
                "oxigeo_mobile_prefetch_{}_{seq}_{name}",
                std::process::id()
            )))
        }
    }

    impl std::ops::Deref for TempPath {
        type Target = std::path::Path;

        fn deref(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl AsRef<std::path::Path> for TempPath {
        fn as_ref(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// Creates a real, single-band GeoTIFF filled with `pixel_value` at
    /// `width`x`height`, flushes it to `path`, and reopens it read-only so
    /// tests can exercise the genuine `reader.read_tile` pipeline that
    /// `oxigeo_mobile_prefetch_tiles` now uses (instead of a null handle).
    ///
    /// Returns the opened read-only dataset handle; caller must
    /// `oxigeo_dataset_close` it and remove the temp file when done.
    fn build_readable_test_dataset(
        path: &std::path::Path,
        width: i32,
        height: i32,
        pixel_value: u8,
    ) -> *mut OxiGeoDataset {
        let path_cstring = CString::new(path.to_str().expect("valid utf-8 path"))
            .expect("path has no interior NUL");

        let mut write_dataset: *mut OxiGeoDataset = std::ptr::null_mut();
        unsafe {
            let result = oxigeo_dataset_create(
                path_cstring.as_ptr(),
                width,
                height,
                1,
                OxiGeoDataType::Byte,
                &mut write_dataset,
            );
            assert_eq!(result, OxiGeoErrorCode::Success, "dataset create failed");

            let mut buffer_data = vec![pixel_value; (width * height) as usize];
            let buffer = OxiGeoBuffer {
                data: buffer_data.as_mut_ptr(),
                length: buffer_data.len(),
                width,
                height,
                channels: 1,
            };
            let write_result = crate::ffi::raster::oxigeo_dataset_write_region(
                write_dataset,
                0,
                0,
                width,
                height,
                1,
                &buffer,
            );
            assert_eq!(
                write_result,
                OxiGeoErrorCode::Success,
                "region write failed"
            );

            let flush_result = oxigeo_dataset_flush(write_dataset);
            assert_eq!(flush_result, OxiGeoErrorCode::Success, "flush failed");
            oxigeo_dataset_close(write_dataset);
        }

        let mut read_dataset: *mut OxiGeoDataset = std::ptr::null_mut();
        unsafe {
            let result = oxigeo_dataset_open(path_cstring.as_ptr(), &mut read_dataset);
            assert_eq!(result, OxiGeoErrorCode::Success, "reopen for read failed");
        }
        read_dataset
    }

    #[test]
    fn test_stats() {
        oxigeo_mobile_reset_stats();

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

        let result = unsafe { oxigeo_mobile_get_stats(&mut stats) };
        assert_eq!(result, OxiGeoErrorCode::Success);
        assert_eq!(stats.total_bytes_read, 1024);
        assert_eq!(stats.tiles_cached, 10);
        assert_eq!(stats.cache_hits, 1);
        assert_eq!(stats.cache_misses, 1);
        assert!((stats.cache_hit_ratio - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_memory_optimization() {
        let result = oxigeo_mobile_optimize_memory(100);
        assert_eq!(result, OxiGeoErrorCode::Success);

        let result = oxigeo_mobile_optimize_memory(500);
        assert_eq!(result, OxiGeoErrorCode::Success);

        let result = oxigeo_mobile_optimize_memory(-1);
        assert_eq!(result, OxiGeoErrorCode::InvalidArgument);
    }

    #[test]
    fn test_offline_mode() {
        let result = oxigeo_mobile_set_offline_mode(1);
        assert_eq!(result, OxiGeoErrorCode::Success);

        let result = oxigeo_mobile_set_offline_mode(0);
        assert_eq!(result, OxiGeoErrorCode::Success);
    }

    #[test]
    fn test_prefetch_tiles_null_dataset() {
        let bbox = OxiGeoBbox {
            min_x: -1.0,
            min_y: -1.0,
            max_x: 1.0,
            max_y: 1.0,
        };

        let result = unsafe { oxigeo_mobile_prefetch_tiles(std::ptr::null(), &bbox, 0, 5) };
        assert_eq!(result, -1);
    }

    #[test]
    fn test_prefetch_tiles_invalid_bbox() {
        let temp_path = TempPath::new("null_bbox.tif");
        let dataset_ptr = build_readable_test_dataset(&temp_path, 8, 8, 42);

        // null bbox
        let result = unsafe { oxigeo_mobile_prefetch_tiles(dataset_ptr, std::ptr::null(), 0, 5) };
        assert_eq!(result, -1);

        unsafe { oxigeo_dataset_close(dataset_ptr) };
    }

    #[test]
    fn test_prefetch_tiles_invalid_zoom() {
        let temp_path = TempPath::new("invalid_zoom.tif");
        let dataset_ptr = build_readable_test_dataset(&temp_path, 8, 8, 42);

        let bbox = OxiGeoBbox {
            min_x: -1.0,
            min_y: -1.0,
            max_x: 1.0,
            max_y: 1.0,
        };

        // Invalid zoom (negative)
        let result = unsafe { oxigeo_mobile_prefetch_tiles(dataset_ptr, &bbox, -1, 5) };
        assert_eq!(result, -1);

        // Invalid zoom (max < min)
        let result = unsafe { oxigeo_mobile_prefetch_tiles(dataset_ptr, &bbox, 5, 2) };
        assert_eq!(result, -1);

        unsafe { oxigeo_dataset_close(dataset_ptr) };
    }

    #[test]
    fn test_prefetch_tiles_small_bbox() {
        let temp_path = TempPath::new("small_bbox.tif");
        let dataset_ptr = build_readable_test_dataset(&temp_path, 8, 8, 42);

        // Initialize cache
        let _ = cache::init_cache(50);
        oxigeo_mobile_reset_stats();

        let bbox = OxiGeoBbox {
            min_x: -0.5,
            min_y: -0.5,
            max_x: 0.5,
            max_y: 0.5,
        };

        // Make sure offline mode is off
        let _ = oxigeo_mobile_set_offline_mode(0);

        let result = unsafe { oxigeo_mobile_prefetch_tiles(dataset_ptr, &bbox, 0, 2) };
        assert!(result >= 0);

        unsafe { oxigeo_dataset_close(dataset_ptr) };
    }

    #[test]
    fn test_prefetch_tiles_invalid_geo_bbox() {
        let temp_path = TempPath::new("invalid_geo_bbox.tif");
        let dataset_ptr = build_readable_test_dataset(&temp_path, 8, 8, 42);

        let bbox = OxiGeoBbox {
            min_x: 10.0,
            min_y: 5.0,
            max_x: 5.0, // min > max
            max_y: 10.0,
        };

        let result = unsafe { oxigeo_mobile_prefetch_tiles(dataset_ptr, &bbox, 0, 2) };
        assert_eq!(result, -1);

        unsafe { oxigeo_dataset_close(dataset_ptr) };
    }

    #[test]
    fn test_prefetch_tiles_offline_mode() {
        let temp_path = TempPath::new("offline_mode.tif");
        let dataset_ptr = build_readable_test_dataset(&temp_path, 8, 8, 42);

        // Initialize cache
        let _ = cache::init_cache(50);
        oxigeo_mobile_reset_stats();

        let bbox = OxiGeoBbox {
            min_x: -0.5,
            min_y: -0.5,
            max_x: 0.5,
            max_y: 0.5,
        };

        // Enable offline mode - should only return cached tiles
        let _ = oxigeo_mobile_set_offline_mode(1);

        let result = unsafe { oxigeo_mobile_prefetch_tiles(dataset_ptr, &bbox, 0, 1) };
        // In offline mode with empty cache, no tiles should be prefetched
        assert_eq!(result, 0);

        // Restore offline mode
        let _ = oxigeo_mobile_set_offline_mode(0);

        unsafe { oxigeo_dataset_close(dataset_ptr) };
    }

    /// Verifies the critical fix: prefetched tiles contain **real** dataset
    /// pixels, not an all-zero placeholder buffer.
    #[test]
    fn test_prefetch_tiles_caches_real_pixel_data() {
        const PIXEL_VALUE: u8 = 201;

        let temp_path = TempPath::new("real_pixels.tif");
        let dataset_ptr = build_readable_test_dataset(&temp_path, 64, 64, PIXEL_VALUE);

        let _ = cache::init_cache(50);
        oxigeo_mobile_reset_stats();
        let _ = oxigeo_mobile_set_offline_mode(0);

        // A bbox just inside the full world extent maps to exactly the
        // single tile (0, 0) at zoom 0, which is the dataset's one real
        // GeoTIFF tile.
        let bbox = OxiGeoBbox {
            min_x: -179.0,
            min_y: -85.0,
            max_x: 179.0,
            max_y: 85.0,
        };

        let result = unsafe { oxigeo_mobile_prefetch_tiles(dataset_ptr, &bbox, 0, 0) };
        assert_eq!(result, 1, "expected exactly one real tile to be prefetched");

        let cached = cache::get_cached_tile("tile_0_0_0");
        let (data, _width, _height, channels) = cached.expect("tile must be cached after prefetch");
        assert_eq!(channels, 1);
        assert!(!data.is_empty());
        assert!(
            data.contains(&PIXEL_VALUE),
            "prefetched tile must contain real dataset pixels"
        );
        assert!(
            !data.iter().all(|&b| b == 0),
            "prefetched tile must not be an all-zero placeholder"
        );

        unsafe { oxigeo_dataset_close(dataset_ptr) };
    }
}
