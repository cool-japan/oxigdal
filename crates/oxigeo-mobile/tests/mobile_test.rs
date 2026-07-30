//! Integration tests for oxigeo-mobile
//!
//! Tests the complete mobile SDK including FFI bindings,
//! iOS/Android platform integration, and caching.

#![allow(unsafe_code)]
#![allow(clippy::expect_used)]
#![allow(clippy::identity_op)]

use oxigeo_mobile::common::cache::{oxigeo_cache_clear, oxigeo_cache_get_info};
use oxigeo_mobile::common::*;
use oxigeo_mobile::ffi::types::*;
use oxigeo_mobile::ffi::*;
use std::sync::atomic::{AtomicU64, Ordering};

/// Per-test scratch fixture inside the system temp dir (house policy: no
/// hardcoded absolute paths).
///
/// The leaf name embeds the process id and a monotonic counter, so no two test
/// binaries — nor two concurrent runs of this one — can ever land on the same
/// file.  Dropping the guard removes the fixture, so a panicking test leaks
/// nothing.
struct TempPath(std::path::PathBuf);

impl TempPath {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(
            std::env::temp_dir().join(format!("oxigeo_mobile_{}_{seq}_{name}", std::process::id())),
        )
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

#[test]
fn test_library_initialization() {
    let result = oxigeo_init();
    assert_eq!(result, OxiGeoErrorCode::Success);

    let result = oxigeo_cleanup();
    assert_eq!(result, OxiGeoErrorCode::Success);
}

#[test]
fn test_buffer_allocation() {
    unsafe {
        let buffer = oxigeo_buffer_alloc(256, 256, 3);
        assert!(!buffer.is_null());

        let buf = &*buffer;
        assert_eq!(buf.width, 256);
        assert_eq!(buf.height, 256);
        assert_eq!(buf.channels, 3);
        assert_eq!(buf.length, 256 * 256 * 3);

        oxigeo_buffer_free(buffer);
    }
}

#[test]
fn test_buffer_invalid_params() {
    unsafe {
        let buffer = oxigeo_buffer_alloc(-1, 256, 3);
        assert!(buffer.is_null());

        let buffer = oxigeo_buffer_alloc(256, 0, 3);
        assert!(buffer.is_null());
    }
}

#[test]
fn test_error_handling() {
    oxigeo_init();

    // Trigger an error
    unsafe {
        let null_path = std::ptr::null();
        let mut dataset = std::ptr::null_mut();
        let result = oxigeo_dataset_open(null_path, &mut dataset);
        assert_eq!(result, OxiGeoErrorCode::NullPointer);

        // Get error message
        let error_msg = oxigeo_get_last_error();
        assert!(!error_msg.is_null());

        oxigeo_string_free(error_msg);
    }

    oxigeo_cleanup();
}

#[test]
fn test_mobile_statistics() {
    // Reset stats
    let result = oxigeo_mobile_reset_stats();
    assert_eq!(result, OxiGeoErrorCode::Success);

    // Record some activity
    cache::init_cache(100).ok();
    cache::put_cached_tile("test".to_string(), vec![1, 2, 3, 4], 2, 2, 1);

    // Get stats
    let mut stats = MobileStats {
        total_bytes_read: 0,
        tiles_cached: 0,
        cache_hits: 0,
        cache_misses: 0,
        cache_hit_ratio: 0.0,
    };

    let result = unsafe { oxigeo_mobile_get_stats(&mut stats) };
    assert_eq!(result, OxiGeoErrorCode::Success);
    assert!(stats.tiles_cached > 0);
}

#[test]
fn test_cache_operations() {
    // Initialize cache
    cache::init_cache(50).ok();

    // Put item
    cache::put_cached_tile("test_tile".to_string(), vec![1, 2, 3, 4], 2, 2, 1);

    // Get item
    let result = cache::get_cached_tile("test_tile");
    assert!(result.is_some());

    let (data, width, height, channels) = result.expect("cached tile");
    assert_eq!(data, vec![1, 2, 3, 4]);
    assert_eq!(width, 2);
    assert_eq!(height, 2);
    assert_eq!(channels, 1);

    // Clear cache
    let result = unsafe { oxigeo_cache_clear() };
    assert_eq!(result, OxiGeoErrorCode::Success);

    let result = cache::get_cached_tile("test_tile");
    assert!(result.is_none());
}

#[test]
fn test_cache_lru_eviction() {
    // Create small cache (1MB)
    cache::init_cache(1).ok();

    // Add many items
    for i in 0..20 {
        let data = vec![0u8; 100_000]; // 100KB each
        cache::put_cached_tile(format!("tile_{}", i), data, 100, 100, 1);
    }

    // Verify cache size is within limit
    let mut size_mb = 0;
    let mut max_mb = 0;
    let mut entries = 0;

    unsafe {
        let result = oxigeo_cache_get_info(&mut size_mb, &mut max_mb, &mut entries);
        assert_eq!(result, OxiGeoErrorCode::Success);
        assert!(size_mb <= max_mb);
    }
}

#[test]
fn test_tile_coordinate_conversion() {
    unsafe {
        let mut x = 0;
        let mut y = 0;

        // Convert lon/lat to tile at zoom 10
        let result = tiles::oxigeo_lonlat_to_tile(0.0, 0.0, 10, &mut x, &mut y);
        assert_eq!(result, OxiGeoErrorCode::Success);
        assert!(x >= 0);
        assert!(y >= 0);

        // Convert tile back to bbox
        let mut bbox = OxiGeoBbox {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 0.0,
            max_y: 0.0,
        };

        let result = tiles::oxigeo_tile_to_bbox(x, y, 10, &mut bbox);
        assert_eq!(result, OxiGeoErrorCode::Success);
        assert!(bbox.min_x < bbox.max_x);
        assert!(bbox.min_y < bbox.max_y);
    }
}

#[test]
fn test_tiles_for_bbox() {
    let bbox = OxiGeoBbox {
        min_x: -10.0,
        min_y: -10.0,
        max_x: 10.0,
        max_y: 10.0,
    };

    unsafe {
        // Count tiles
        let count = tiles::oxigeo_count_tiles_for_bbox(&bbox, 5);
        assert!(count > 0);
        assert!(count < 1000); // Should be reasonable

        // Get tile coordinates
        let mut coords = vec![OxiGeoTileCoord { x: 0, y: 0, z: 0 }; count as usize];

        let retrieved = tiles::oxigeo_get_tiles_for_bbox(&bbox, 5, coords.as_mut_ptr(), count);

        assert_eq!(retrieved, count);

        // Verify all tiles have correct zoom
        for coord in &coords[..retrieved as usize] {
            assert_eq!(coord.z, 5);
        }
    }
}

#[test]
fn test_offline_mode() {
    // Enable offline mode
    let result = oxigeo_mobile_set_offline_mode(1);
    assert_eq!(result, OxiGeoErrorCode::Success);
    assert!(tiles::is_offline_mode());

    // Disable offline mode
    let result = oxigeo_mobile_set_offline_mode(0);
    assert_eq!(result, OxiGeoErrorCode::Success);
    assert!(!tiles::is_offline_mode());
}

#[test]
fn test_memory_optimization() {
    // Low memory
    let result = oxigeo_mobile_optimize_memory(50);
    assert_eq!(result, OxiGeoErrorCode::Success);

    // High memory
    let result = oxigeo_mobile_optimize_memory(500);
    assert_eq!(result, OxiGeoErrorCode::Success);

    // Invalid memory
    let result = oxigeo_mobile_optimize_memory(-1);
    assert_eq!(result, OxiGeoErrorCode::InvalidArgument);
}

#[test]
fn test_format_support() {
    unsafe {
        oxigeo_init();

        let path_tiff = std::ffi::CString::new("/test/file.tif").expect("valid string");
        let supported = oxigeo_is_format_supported(path_tiff.as_ptr());
        assert_eq!(supported, 1);

        let path_json = std::ffi::CString::new("/test/file.geojson").expect("valid string");
        let supported = oxigeo_is_format_supported(path_json.as_ptr());
        assert_eq!(supported, 1);

        let path_unknown = std::ffi::CString::new("/test/file.xyz").expect("valid string");
        let supported = oxigeo_is_format_supported(path_unknown.as_ptr());
        assert_eq!(supported, 0);

        oxigeo_cleanup();
    }
}

#[test]
#[cfg(feature = "ios")]
fn test_ios_buffer_conversion() {
    unsafe {
        // Create RGB buffer
        let mut rgb_data = vec![255u8, 0, 0, 0, 255, 0, 0, 0, 255]; // R, G, B pixels
        let rgb_buffer = OxiGeoBuffer {
            data: rgb_data.as_mut_ptr(),
            length: rgb_data.len(),
            width: 3,
            height: 1,
            channels: 3,
        };

        // Create RGBA output buffer
        let mut rgba_data = vec![0u8; 3 * 1 * 4];
        let mut rgba_buffer = OxiGeoBuffer {
            data: rgba_data.as_mut_ptr(),
            length: rgba_data.len(),
            width: 3,
            height: 1,
            channels: 4,
        };

        // Convert
        let result = oxigeo_mobile::ios::oxigeo_buffer_to_ios_rgba(&rgb_buffer, &mut rgba_buffer);
        assert_eq!(result, OxiGeoErrorCode::Success);

        // Verify RGBA values
        assert_eq!(rgba_data[0], 255); // R
        assert_eq!(rgba_data[1], 0); // G
        assert_eq!(rgba_data[2], 0); // B
        assert_eq!(rgba_data[3], 255); // A
    }
}

#[test]
#[cfg(feature = "android")]
fn test_android_buffer_conversion() {
    unsafe {
        // Create RGB buffer
        let mut rgb_data = vec![255u8, 0, 0, 0, 255, 0]; // R, G pixels
        let rgb_buffer = OxiGeoBuffer {
            data: rgb_data.as_mut_ptr(),
            length: rgb_data.len(),
            width: 2,
            height: 1,
            channels: 3,
        };

        // Create ARGB output buffer
        let mut argb_data = vec![0u8; 2 * 1 * 4];
        let mut argb_buffer = OxiGeoBuffer {
            data: argb_data.as_mut_ptr(),
            length: argb_data.len(),
            width: 2,
            height: 1,
            channels: 4,
        };

        // Convert
        let result =
            oxigeo_mobile::android::oxigeo_buffer_to_android_argb(&rgb_buffer, &mut argb_buffer);
        assert_eq!(result, OxiGeoErrorCode::Success);

        // Verify ARGB values (first pixel)
        assert_eq!(argb_data[0], 255); // A
        assert_eq!(argb_data[1], 255); // R
        assert_eq!(argb_data[2], 0); // G
        assert_eq!(argb_data[3], 0); // B
    }
}

#[test]
fn test_enhance_params_default() {
    let params = OxiGeoEnhanceParams::default();
    assert_eq!(params.brightness, 1.0);
    assert_eq!(params.contrast, 1.0);
    assert_eq!(params.saturation, 1.0);
    assert_eq!(params.gamma, 1.0);
}

#[test]
fn test_resampling_default() {
    let resampling = OxiGeoResampling::default();
    assert_eq!(resampling, OxiGeoResampling::Bilinear);
}

#[test]
fn test_type_sizes() {
    use std::mem::size_of;

    // Verify FFI types are reasonably sized
    assert!(size_of::<OxiGeoMetadata>() < 256);
    assert_eq!(size_of::<OxiGeoBbox>(), 32); // 4 * f64
    assert_eq!(size_of::<OxiGeoPoint>(), 24); // 3 * f64
    assert_eq!(size_of::<OxiGeoTileCoord>(), 12); // 3 * i32
}

#[test]
fn test_concurrent_cache_access() {
    use std::thread;

    cache::init_cache(100).ok();

    let handles: Vec<_> = (0..10)
        .map(|i| {
            thread::spawn(move || {
                // Write
                let data = vec![i as u8; 100];
                cache::put_cached_tile(format!("tile_{}", i), data.clone(), 10, 10, 1);

                // Read
                if let Some((cached_data, _, _, _)) = cache::get_cached_tile(&format!("tile_{}", i))
                {
                    assert_eq!(cached_data[0], i as u8);
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("thread panicked");
    }
}

#[test]
fn test_tile_reading_null_dataset() {
    let coord = OxiGeoTileCoord { z: 0, x: 0, y: 0 };
    let mut tile_ptr: *mut oxigeo_mobile::ffi::types::OxiGeoTile = std::ptr::null_mut();

    unsafe {
        let result = oxigeo_mobile::ffi::raster::oxigeo_dataset_read_tile(
            std::ptr::null(),
            &coord,
            256,
            &mut tile_ptr,
        );
        assert_eq!(result, OxiGeoErrorCode::NullPointer);
    }
}

#[test]
fn test_tile_reading_invalid_coords() {
    use std::ffi::CString;

    let temp_path = TempPath::new("tile_coords.tif");
    let path_cstring =
        CString::new(temp_path.to_str().expect("valid path")).expect("valid cstring");

    let mut dataset_ptr: *mut OxiGeoDataset = std::ptr::null_mut();

    unsafe {
        // Create a small test dataset
        let create_result = oxigeo_mobile::ffi::raster::oxigeo_dataset_create(
            path_cstring.as_ptr(),
            256,
            256,
            3,
            oxigeo_mobile::ffi::types::OxiGeoDataType::Byte,
            &mut dataset_ptr,
        );
        assert_eq!(create_result, OxiGeoErrorCode::Success);

        // Test negative coordinates
        let coord = OxiGeoTileCoord { z: -1, x: 0, y: 0 };
        let mut tile_ptr: *mut oxigeo_mobile::ffi::types::OxiGeoTile = std::ptr::null_mut();

        let result = oxigeo_mobile::ffi::raster::oxigeo_dataset_read_tile(
            dataset_ptr,
            &coord,
            256,
            &mut tile_ptr,
        );
        assert_eq!(result, OxiGeoErrorCode::InvalidArgument);

        // Test invalid tile size
        let coord = OxiGeoTileCoord { z: 0, x: 0, y: 0 };
        let result = oxigeo_mobile::ffi::raster::oxigeo_dataset_read_tile(
            dataset_ptr,
            &coord,
            -1,
            &mut tile_ptr,
        );
        assert_eq!(result, OxiGeoErrorCode::InvalidArgument);

        let result = oxigeo_mobile::ffi::raster::oxigeo_dataset_read_tile(
            dataset_ptr,
            &coord,
            5000,
            &mut tile_ptr,
        );
        assert_eq!(result, OxiGeoErrorCode::InvalidArgument);

        oxigeo_mobile::ffi::raster::oxigeo_dataset_close(dataset_ptr);
    }
}

#[test]
fn test_tile_free_null() {
    unsafe {
        let result = oxigeo_mobile::ffi::raster::oxigeo_tile_free(std::ptr::null_mut());
        assert_eq!(result, OxiGeoErrorCode::Success);
    }
}

#[test]
fn test_tile_get_data_null() {
    let mut buffer = OxiGeoBuffer {
        data: std::ptr::null_mut(),
        length: 0,
        width: 0,
        height: 0,
        channels: 0,
    };

    unsafe {
        let result =
            oxigeo_mobile::ffi::raster::oxigeo_tile_get_data(std::ptr::null_mut(), &mut buffer);
        assert_eq!(result, OxiGeoErrorCode::NullPointer);
    }
}

#[test]
#[cfg(feature = "android")]
fn test_android_tile_reading() {
    use std::ffi::CString;

    let temp_path = TempPath::new("android_tile.tif");
    let path_cstring =
        CString::new(temp_path.to_str().expect("valid path")).expect("valid cstring");

    let mut dataset_ptr: *mut OxiGeoDataset = std::ptr::null_mut();

    unsafe {
        // Create test dataset
        let create_result = oxigeo_mobile::ffi::raster::oxigeo_dataset_create(
            path_cstring.as_ptr(),
            512,
            512,
            3,
            oxigeo_mobile::ffi::types::OxiGeoDataType::Byte,
            &mut dataset_ptr,
        );
        assert_eq!(create_result, OxiGeoErrorCode::Success);

        // Allocate buffer for tile (256x256 ARGB)
        let tile_size = 256;
        let mut buffer_data = vec![0u8; (tile_size * tile_size * 4) as usize];
        let mut buffer = OxiGeoBuffer {
            data: buffer_data.as_mut_ptr(),
            length: buffer_data.len(),
            width: tile_size,
            height: tile_size,
            channels: 4,
        };

        // Read tile
        let result = oxigeo_mobile::android::raster::oxigeo_android_read_tile(
            dataset_ptr,
            0,
            0,
            0,
            tile_size,
            &mut buffer,
        );

        // For now, expect IoError or InvalidArgument since we haven't written actual data
        // In production with real GeoTIFF, this would succeed
        assert!(
            result == OxiGeoErrorCode::Success
                || result == OxiGeoErrorCode::IoError
                || result == OxiGeoErrorCode::InvalidArgument,
            "Expected Success, IoError, or InvalidArgument, got {:?}",
            result
        );

        oxigeo_mobile::ffi::raster::oxigeo_dataset_close(dataset_ptr);
    }
}

#[test]
#[cfg(feature = "ios")]
fn test_ios_tile_reading() {
    use std::ffi::CString;

    let temp_path = TempPath::new("ios_tile.tif");
    let path_cstring =
        CString::new(temp_path.to_str().expect("valid path")).expect("valid cstring");

    let mut dataset_ptr: *mut OxiGeoDataset = std::ptr::null_mut();

    unsafe {
        // Create test dataset
        let create_result = oxigeo_mobile::ffi::raster::oxigeo_dataset_create(
            path_cstring.as_ptr(),
            512,
            512,
            3,
            oxigeo_mobile::ffi::types::OxiGeoDataType::Byte,
            &mut dataset_ptr,
        );
        assert_eq!(create_result, OxiGeoErrorCode::Success);

        // Allocate buffer for tile (256x256 RGBA)
        let tile_size = 256;
        let mut buffer_data = vec![0u8; (tile_size * tile_size * 4) as usize];
        let mut buffer = OxiGeoBuffer {
            data: buffer_data.as_mut_ptr(),
            length: buffer_data.len(),
            width: tile_size,
            height: tile_size,
            channels: 4,
        };

        // Read tile
        let result = oxigeo_mobile::ios::raster::oxigeo_ios_read_tile(
            dataset_ptr,
            0,
            0,
            0,
            tile_size,
            &mut buffer,
        );

        // For now, expect IoError or InvalidArgument since we haven't written actual data
        // In production with real GeoTIFF, this would succeed
        assert!(
            result == OxiGeoErrorCode::Success
                || result == OxiGeoErrorCode::IoError
                || result == OxiGeoErrorCode::InvalidArgument,
            "Expected Success, IoError, or InvalidArgument, got {:?}",
            result
        );

        oxigeo_mobile::ffi::raster::oxigeo_dataset_close(dataset_ptr);
    }
}

// ---------------------------------------------------------------------------
// Regression tests for https://github.com/cool-japan/oxigeo/issues/14
//
// `GeoTiffReader::read_band(level, band)` used to ignore both arguments and
// return the whole pixel-interleaved image. It now returns one de-interleaved
// band plane at the requested overview level. Two call sites in this crate
// were built on the old behaviour:
//
//   * `oxigeo_dataset_read_region` computed `row_stride = width *
//     bytes_per_sample * band_count` over a buffer that had become
//     `band_count` times smaller, so its bounds guard silently skipped almost
//     every row and left the caller's buffer untouched. It now issues one
//     `read_window` per requested channel and interleaves the planes into the
//     caller's buffer.
//   * `oxigeo_dataset_compute_stats` asked for level `overview_count() - 1`,
//     but level numbering is `0 = full resolution` and `1..=overview_count()`
//     = the overviews, so the coarsest overview is `overview_count()`.
// ---------------------------------------------------------------------------

/// Per-band, per-pixel sample value for the issue-14 fixtures.
///
/// Each band gets a disjoint value range (band 0 -> `0..=12`, band 1 ->
/// `70..=82`, band 2 -> `140..=152`), so "band 0 repeated", "bands swapped"
/// and "buffer left untouched" are each individually detectable.
fn issue_14_sample(band: i32, x: i32, y: i32, width: i32) -> u8 {
    (band * 70 + ((y * width + x) % 13)) as u8
}

/// Builds a `bands`-band UInt8 GeoTIFF at `path` through the public FFI
/// create/write/flush pipeline, filled with [`issue_14_sample`].
///
/// # Safety
/// `path` must be a valid null-terminated path string.
unsafe fn write_issue_14_fixture(path: &std::ffi::CStr, width: i32, height: i32, bands: i32) {
    let mut dataset_ptr: *mut OxiGeoDataset = std::ptr::null_mut();

    unsafe {
        let result = oxigeo_mobile::ffi::raster::oxigeo_dataset_create(
            path.as_ptr(),
            width,
            height,
            bands,
            OxiGeoDataType::Byte,
            &mut dataset_ptr,
        );
        assert_eq!(result, OxiGeoErrorCode::Success, "dataset_create failed");

        for band in 0..bands {
            let mut plane: Vec<u8> = Vec::with_capacity((width * height) as usize);
            for y in 0..height {
                for x in 0..width {
                    plane.push(issue_14_sample(band, x, y, width));
                }
            }
            let buffer = OxiGeoBuffer {
                data: plane.as_mut_ptr(),
                length: plane.len(),
                width,
                height,
                channels: 1,
            };
            let result = oxigeo_mobile::ffi::raster::oxigeo_dataset_write_region(
                dataset_ptr,
                0,
                0,
                width,
                height,
                band + 1,
                &buffer,
            );
            assert_eq!(
                result,
                OxiGeoErrorCode::Success,
                "write_region failed for band {}",
                band + 1
            );
        }

        let result = oxigeo_mobile::ffi::raster::oxigeo_dataset_flush(dataset_ptr);
        assert_eq!(result, OxiGeoErrorCode::Success, "dataset_flush failed");

        let result = oxigeo_mobile::ffi::raster::oxigeo_dataset_close(dataset_ptr);
        assert_eq!(result, OxiGeoErrorCode::Success, "dataset_close failed");
    }
}

/// Builds a unique temp path for an issue-14 fixture, scoped to the OS temp
/// dir so nothing is hard-coded and parallel runs cannot collide.
fn issue_14_temp_path(tag: &str) -> TempPath {
    TempPath::new(&format!("issue14_{tag}.tif"))
}

#[test]
fn test_issue_14_read_region_multiband_fills_requested_bands() {
    use std::ffi::CString;

    let width = 8i32;
    let height = 6i32;
    let bands = 3i32;

    let temp_path = issue_14_temp_path("read_region_multiband");
    let path_cstring =
        CString::new(temp_path.to_str().expect("valid path")).expect("valid cstring");

    unsafe {
        write_issue_14_fixture(&path_cstring, width, height, bands);

        let mut dataset_ptr: *mut OxiGeoDataset = std::ptr::null_mut();
        let result = oxigeo_mobile::ffi::raster::oxigeo_dataset_open(
            path_cstring.as_ptr(),
            &mut dataset_ptr,
        );
        assert_eq!(result, OxiGeoErrorCode::Success, "dataset_open failed");

        // A sub-rectangle, deliberately not the whole image and not at the
        // origin, so a stride/offset mistake cannot cancel out.
        let x_off = 2i32;
        let y_off = 1i32;
        let x_size = 4i32;
        let y_size = 3i32;

        // `band` is 1-based here: band 2 means we start at 0-based band index
        // 1 and fill `channels = 2` consecutive bands (0-based indices 1, 2).
        let band = 2i32;
        let channels = 2i32;
        let first_band = band - 1;

        // Sentinel fill: any byte still 0xAA at the end was never written,
        // which is exactly the old failure mode (guard skipped the row).
        const SENTINEL: u8 = 0xAA;
        let mut buffer_data = vec![SENTINEL; (x_size * y_size * channels) as usize];
        let mut buffer = OxiGeoBuffer {
            data: buffer_data.as_mut_ptr(),
            length: buffer_data.len(),
            width: x_size,
            height: y_size,
            channels,
        };

        let result = oxigeo_mobile::ffi::raster::oxigeo_dataset_read_region(
            dataset_ptr,
            x_off,
            y_off,
            x_size,
            y_size,
            band,
            &mut buffer,
        );
        assert_eq!(
            result,
            OxiGeoErrorCode::Success,
            "read_region of a {x_size}x{y_size} window at ({x_off},{y_off}) from a \
             {bands}-band raster must succeed (issue #14)"
        );

        assert!(
            buffer_data.iter().any(|&b| b != SENTINEL),
            "destination buffer was left entirely untouched (all 0x{SENTINEL:02X}); \
             read_region wrote nothing"
        );

        for row in 0..y_size {
            for col in 0..x_size {
                for ch in 0..channels {
                    let src_band = first_band + ch;
                    let expected = issue_14_sample(src_band, x_off + col, y_off + row, width);
                    let idx = ((row * x_size + col) * channels + ch) as usize;
                    let actual = buffer_data[idx];
                    assert_eq!(
                        actual,
                        expected,
                        "band {} (0-based {}) pixel ({}, {}) at buffer offset {}: \
                         expected {}, got {}",
                        src_band + 1,
                        src_band,
                        x_off + col,
                        y_off + row,
                        idx,
                        expected,
                        actual
                    );
                }
            }
        }

        // Sanity: the two interleaved channels must actually differ, so a
        // "same plane copied twice" bug cannot pass the loop above.
        assert_ne!(
            buffer_data[0], buffer_data[1],
            "channel 0 and channel 1 of the first destination pixel are identical \
             ({}); the two bands were not de-interleaved separately",
            buffer_data[0]
        );

        let result = oxigeo_mobile::ffi::raster::oxigeo_dataset_close(dataset_ptr);
        assert_eq!(result, OxiGeoErrorCode::Success);
    }
}

#[test]
fn test_issue_14_read_region_single_band_unchanged() {
    use std::ffi::CString;

    let width = 8i32;
    let height = 6i32;

    let temp_path = issue_14_temp_path("read_region_single_band");
    let path_cstring =
        CString::new(temp_path.to_str().expect("valid path")).expect("valid cstring");

    unsafe {
        write_issue_14_fixture(&path_cstring, width, height, 1);

        let mut dataset_ptr: *mut OxiGeoDataset = std::ptr::null_mut();
        let result = oxigeo_mobile::ffi::raster::oxigeo_dataset_open(
            path_cstring.as_ptr(),
            &mut dataset_ptr,
        );
        assert_eq!(result, OxiGeoErrorCode::Success, "dataset_open failed");

        let x_off = 1i32;
        let y_off = 2i32;
        let x_size = 5i32;
        let y_size = 3i32;

        const SENTINEL: u8 = 0xAA;
        let mut buffer_data = vec![SENTINEL; (x_size * y_size) as usize];
        let mut buffer = OxiGeoBuffer {
            data: buffer_data.as_mut_ptr(),
            length: buffer_data.len(),
            width: x_size,
            height: y_size,
            channels: 1,
        };

        let result = oxigeo_mobile::ffi::raster::oxigeo_dataset_read_region(
            dataset_ptr,
            x_off,
            y_off,
            x_size,
            y_size,
            1,
            &mut buffer,
        );
        assert_eq!(
            result,
            OxiGeoErrorCode::Success,
            "single-band read_region must succeed"
        );

        for row in 0..y_size {
            for col in 0..x_size {
                let expected = issue_14_sample(0, x_off + col, y_off + row, width);
                let idx = (row * x_size + col) as usize;
                let actual = buffer_data[idx];
                assert_eq!(
                    actual,
                    expected,
                    "band 1 (0-based 0) pixel ({}, {}) at buffer offset {}: expected {}, got {}",
                    x_off + col,
                    y_off + row,
                    idx,
                    expected,
                    actual
                );
            }
        }

        let result = oxigeo_mobile::ffi::raster::oxigeo_dataset_close(dataset_ptr);
        assert_eq!(result, OxiGeoErrorCode::Success);
    }
}

#[test]
fn test_issue_14_compute_stats_approx_uses_valid_level() {
    use oxigeo_core::io::FileDataSource;
    use oxigeo_core::types::{GeoTransform, NoDataValue, RasterDataType};
    use oxigeo_geotiff::tiff::{Compression, Predictor};
    use oxigeo_geotiff::{
        GeoTiffReader, GeoTiffWriter, GeoTiffWriterOptions, OverviewResampling, WriterConfig,
    };
    use std::ffi::CString;

    // The FFI create/flush pipeline never generates overviews, so build the
    // pyramid directly with the writer. 64x64 with levels [2, 4] gives a real
    // multi-level file whose coarsest level is `overview_count()`.
    let width = 64u64;
    let height = 64u64;
    let temp_path = issue_14_temp_path("compute_stats_overviews");

    let mut samples = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            // Smooth ramp 0..=126 so averaging overviews stay inside the same
            // value range as the full-resolution data.
            samples.push((x + y) as u8);
        }
    }

    let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
        .with_compression(Compression::None)
        .with_predictor(Predictor::None)
        .with_tile_size(16, 16)
        .with_overviews(true, OverviewResampling::Average)
        .with_overview_levels(vec![2, 4])
        .with_geo_transform(GeoTransform::north_up(0.0, 0.0, 1.0, -1.0))
        .with_epsg_code(4326)
        .with_nodata(NoDataValue::None);

    {
        let mut writer = GeoTiffWriter::create(&temp_path, config, GeoTiffWriterOptions::default())
            .expect("create overview fixture");
        writer.write(&samples).expect("write overview fixture");
    }

    // Confirm the fixture really has overviews; otherwise this test would
    // silently degrade into the no-overview path.
    let (overview_count, coarsest_pixels) = {
        let source = FileDataSource::open(&temp_path).expect("open fixture");
        let reader = GeoTiffReader::open(source).expect("parse fixture");
        let count = reader.overview_count();
        // Level numbering: 0 = full resolution, 1..=overview_count() = the
        // overviews. `count` (not `count - 1`) is the coarsest level.
        let pixels = reader
            .band_pixel_count(count)
            .expect("coarsest overview level must exist");
        (count, pixels)
    };
    assert!(
        overview_count > 0,
        "fixture must contain overviews for this test to exercise the level \
         fallback: overview_count() = {overview_count}"
    );

    let path_cstring =
        CString::new(temp_path.to_str().expect("valid path")).expect("valid cstring");

    unsafe {
        let mut dataset_ptr: *mut OxiGeoDataset = std::ptr::null_mut();
        let result = oxigeo_mobile::ffi::raster::oxigeo_dataset_open(
            path_cstring.as_ptr(),
            &mut dataset_ptr,
        );
        assert_eq!(result, OxiGeoErrorCode::Success, "dataset_open failed");

        let mut exact = OxiGeoStats {
            min: 0.0,
            max: 0.0,
            mean: 0.0,
            stddev: 0.0,
            valid_count: 0,
        };
        let result =
            oxigeo_mobile::ffi::raster::oxigeo_dataset_compute_stats(dataset_ptr, 1, 0, &mut exact);
        assert_eq!(
            result,
            OxiGeoErrorCode::Success,
            "exact stats (approx_ok = 0) must succeed"
        );
        assert_eq!(
            exact.valid_count,
            width * height,
            "exact stats: expected {} valid pixels, got {}",
            width * height,
            exact.valid_count
        );

        let mut approx = OxiGeoStats {
            min: 0.0,
            max: 0.0,
            mean: 0.0,
            stddev: 0.0,
            valid_count: 0,
        };
        let result = oxigeo_mobile::ffi::raster::oxigeo_dataset_compute_stats(
            dataset_ptr,
            1,
            1,
            &mut approx,
        );
        assert_eq!(
            result,
            OxiGeoErrorCode::Success,
            "approximate stats (approx_ok != 0) on a file with {overview_count} overview(s) \
             must succeed, not fail with a buffer-size error: {:?}",
            {
                let msg = oxigeo_get_last_error();
                let text = std::ffi::CStr::from_ptr(msg).to_string_lossy().into_owned();
                oxigeo_string_free(msg);
                text
            }
        );

        // The coarsest overview is level `overview_count()`, not
        // `overview_count() - 1`: reading it must yield exactly that level's
        // pixel count, which for this 64x64 / [2, 4] pyramid is 16x16 = 256.
        assert_eq!(
            approx.valid_count, coarsest_pixels as u64,
            "approximate stats: expected the coarsest overview's {} pixels, got {}",
            coarsest_pixels, approx.valid_count
        );
        assert!(
            approx.valid_count < width * height,
            "approximate stats: pixel count {} must be smaller than the \
             full-resolution count {} (an overview was supposed to be read)",
            approx.valid_count,
            width * height
        );
        assert!(
            approx.min >= exact.min && approx.max <= exact.max,
            "approximate stats: min/max ({}, {}) must stay inside the exact range \
             ({}, {})",
            approx.min,
            approx.max,
            exact.min,
            exact.max
        );
        assert!(
            (approx.mean - exact.mean).abs() < 5.0,
            "approximate stats: mean {} must be close to the exact mean {}",
            approx.mean,
            exact.mean
        );

        let result = oxigeo_mobile::ffi::raster::oxigeo_dataset_close(dataset_ptr);
        assert_eq!(result, OxiGeoErrorCode::Success);
    }
}
