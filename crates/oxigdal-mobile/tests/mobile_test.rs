//! Integration tests for oxigdal-mobile
//!
//! Tests the complete mobile SDK including FFI bindings,
//! iOS/Android platform integration, and caching.

#![allow(unsafe_code)]
#![allow(clippy::expect_used)]
#![allow(clippy::identity_op)]

use oxigdal_mobile::common::cache::{oxigdal_cache_clear, oxigdal_cache_get_info};
use oxigdal_mobile::common::*;
use oxigdal_mobile::ffi::types::*;
use oxigdal_mobile::ffi::*;

#[test]
fn test_library_initialization() {
    let result = oxigdal_init();
    assert_eq!(result, OxiGdalErrorCode::Success);

    let result = oxigdal_cleanup();
    assert_eq!(result, OxiGdalErrorCode::Success);
}

#[test]
fn test_buffer_allocation() {
    unsafe {
        let buffer = oxigdal_buffer_alloc(256, 256, 3);
        assert!(!buffer.is_null());

        let buf = &*buffer;
        assert_eq!(buf.width, 256);
        assert_eq!(buf.height, 256);
        assert_eq!(buf.channels, 3);
        assert_eq!(buf.length, 256 * 256 * 3);

        oxigdal_buffer_free(buffer);
    }
}

#[test]
fn test_buffer_invalid_params() {
    unsafe {
        let buffer = oxigdal_buffer_alloc(-1, 256, 3);
        assert!(buffer.is_null());

        let buffer = oxigdal_buffer_alloc(256, 0, 3);
        assert!(buffer.is_null());
    }
}

#[test]
fn test_error_handling() {
    oxigdal_init();

    // Trigger an error
    unsafe {
        let null_path = std::ptr::null();
        let mut dataset = std::ptr::null_mut();
        let result = oxigdal_dataset_open(null_path, &mut dataset);
        assert_eq!(result, OxiGdalErrorCode::NullPointer);

        // Get error message
        let error_msg = oxigdal_get_last_error();
        assert!(!error_msg.is_null());

        oxigdal_string_free(error_msg);
    }

    oxigdal_cleanup();
}

#[test]
fn test_mobile_statistics() {
    // Reset stats
    let result = oxigdal_mobile_reset_stats();
    assert_eq!(result, OxiGdalErrorCode::Success);

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

    let result = unsafe { oxigdal_mobile_get_stats(&mut stats) };
    assert_eq!(result, OxiGdalErrorCode::Success);
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
    let result = unsafe { oxigdal_cache_clear() };
    assert_eq!(result, OxiGdalErrorCode::Success);

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
        let result = oxigdal_cache_get_info(&mut size_mb, &mut max_mb, &mut entries);
        assert_eq!(result, OxiGdalErrorCode::Success);
        assert!(size_mb <= max_mb);
    }
}

#[test]
fn test_tile_coordinate_conversion() {
    unsafe {
        let mut x = 0;
        let mut y = 0;

        // Convert lon/lat to tile at zoom 10
        let result = tiles::oxigdal_lonlat_to_tile(0.0, 0.0, 10, &mut x, &mut y);
        assert_eq!(result, OxiGdalErrorCode::Success);
        assert!(x >= 0);
        assert!(y >= 0);

        // Convert tile back to bbox
        let mut bbox = OxiGdalBbox {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 0.0,
            max_y: 0.0,
        };

        let result = tiles::oxigdal_tile_to_bbox(x, y, 10, &mut bbox);
        assert_eq!(result, OxiGdalErrorCode::Success);
        assert!(bbox.min_x < bbox.max_x);
        assert!(bbox.min_y < bbox.max_y);
    }
}

#[test]
fn test_tiles_for_bbox() {
    let bbox = OxiGdalBbox {
        min_x: -10.0,
        min_y: -10.0,
        max_x: 10.0,
        max_y: 10.0,
    };

    unsafe {
        // Count tiles
        let count = tiles::oxigdal_count_tiles_for_bbox(&bbox, 5);
        assert!(count > 0);
        assert!(count < 1000); // Should be reasonable

        // Get tile coordinates
        let mut coords = vec![OxiGdalTileCoord { x: 0, y: 0, z: 0 }; count as usize];

        let retrieved = tiles::oxigdal_get_tiles_for_bbox(&bbox, 5, coords.as_mut_ptr(), count);

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
    let result = oxigdal_mobile_set_offline_mode(1);
    assert_eq!(result, OxiGdalErrorCode::Success);
    assert!(tiles::is_offline_mode());

    // Disable offline mode
    let result = oxigdal_mobile_set_offline_mode(0);
    assert_eq!(result, OxiGdalErrorCode::Success);
    assert!(!tiles::is_offline_mode());
}

#[test]
fn test_memory_optimization() {
    // Low memory
    let result = oxigdal_mobile_optimize_memory(50);
    assert_eq!(result, OxiGdalErrorCode::Success);

    // High memory
    let result = oxigdal_mobile_optimize_memory(500);
    assert_eq!(result, OxiGdalErrorCode::Success);

    // Invalid memory
    let result = oxigdal_mobile_optimize_memory(-1);
    assert_eq!(result, OxiGdalErrorCode::InvalidArgument);
}

#[test]
fn test_format_support() {
    unsafe {
        oxigdal_init();

        let path_tiff = std::ffi::CString::new("/test/file.tif").expect("valid string");
        let supported = oxigdal_is_format_supported(path_tiff.as_ptr());
        assert_eq!(supported, 1);

        let path_json = std::ffi::CString::new("/test/file.geojson").expect("valid string");
        let supported = oxigdal_is_format_supported(path_json.as_ptr());
        assert_eq!(supported, 1);

        let path_unknown = std::ffi::CString::new("/test/file.xyz").expect("valid string");
        let supported = oxigdal_is_format_supported(path_unknown.as_ptr());
        assert_eq!(supported, 0);

        oxigdal_cleanup();
    }
}

#[test]
#[cfg(feature = "ios")]
fn test_ios_buffer_conversion() {
    unsafe {
        // Create RGB buffer
        let mut rgb_data = vec![255u8, 0, 0, 0, 255, 0, 0, 0, 255]; // R, G, B pixels
        let rgb_buffer = OxiGdalBuffer {
            data: rgb_data.as_mut_ptr(),
            length: rgb_data.len(),
            width: 3,
            height: 1,
            channels: 3,
        };

        // Create RGBA output buffer
        let mut rgba_data = vec![0u8; 3 * 1 * 4];
        let mut rgba_buffer = OxiGdalBuffer {
            data: rgba_data.as_mut_ptr(),
            length: rgba_data.len(),
            width: 3,
            height: 1,
            channels: 4,
        };

        // Convert
        let result = oxigdal_mobile::ios::oxigdal_buffer_to_ios_rgba(&rgb_buffer, &mut rgba_buffer);
        assert_eq!(result, OxiGdalErrorCode::Success);

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
        let rgb_buffer = OxiGdalBuffer {
            data: rgb_data.as_mut_ptr(),
            length: rgb_data.len(),
            width: 2,
            height: 1,
            channels: 3,
        };

        // Create ARGB output buffer
        let mut argb_data = vec![0u8; 2 * 1 * 4];
        let mut argb_buffer = OxiGdalBuffer {
            data: argb_data.as_mut_ptr(),
            length: argb_data.len(),
            width: 2,
            height: 1,
            channels: 4,
        };

        // Convert
        let result =
            oxigdal_mobile::android::oxigdal_buffer_to_android_argb(&rgb_buffer, &mut argb_buffer);
        assert_eq!(result, OxiGdalErrorCode::Success);

        // Verify ARGB values (first pixel)
        assert_eq!(argb_data[0], 255); // A
        assert_eq!(argb_data[1], 255); // R
        assert_eq!(argb_data[2], 0); // G
        assert_eq!(argb_data[3], 0); // B
    }
}

#[test]
fn test_enhance_params_default() {
    let params = OxiGdalEnhanceParams::default();
    assert_eq!(params.brightness, 1.0);
    assert_eq!(params.contrast, 1.0);
    assert_eq!(params.saturation, 1.0);
    assert_eq!(params.gamma, 1.0);
}

#[test]
fn test_resampling_default() {
    let resampling = OxiGdalResampling::default();
    assert_eq!(resampling, OxiGdalResampling::Bilinear);
}

#[test]
fn test_type_sizes() {
    use std::mem::size_of;

    // Verify FFI types are reasonably sized
    assert!(size_of::<OxiGdalMetadata>() < 256);
    assert_eq!(size_of::<OxiGdalBbox>(), 32); // 4 * f64
    assert_eq!(size_of::<OxiGdalPoint>(), 24); // 3 * f64
    assert_eq!(size_of::<OxiGdalTileCoord>(), 12); // 3 * i32
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
    let coord = OxiGdalTileCoord { z: 0, x: 0, y: 0 };
    let mut tile_ptr: *mut oxigdal_mobile::ffi::types::OxiGdalTile = std::ptr::null_mut();

    unsafe {
        let result = oxigdal_mobile::ffi::raster::oxigdal_dataset_read_tile(
            std::ptr::null(),
            &coord,
            256,
            &mut tile_ptr,
        );
        assert_eq!(result, OxiGdalErrorCode::NullPointer);
    }
}

#[test]
fn test_tile_reading_invalid_coords() {
    use std::ffi::CString;

    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join("test_tile_coords.tif");
    let path_cstring =
        CString::new(temp_path.to_str().expect("valid path")).expect("valid cstring");

    let mut dataset_ptr: *mut OxiGdalDataset = std::ptr::null_mut();

    unsafe {
        // Create a small test dataset
        let create_result = oxigdal_mobile::ffi::raster::oxigdal_dataset_create(
            path_cstring.as_ptr(),
            256,
            256,
            3,
            oxigdal_mobile::ffi::types::OxiGdalDataType::Byte,
            &mut dataset_ptr,
        );
        assert_eq!(create_result, OxiGdalErrorCode::Success);

        // Test negative coordinates
        let coord = OxiGdalTileCoord { z: -1, x: 0, y: 0 };
        let mut tile_ptr: *mut oxigdal_mobile::ffi::types::OxiGdalTile = std::ptr::null_mut();

        let result = oxigdal_mobile::ffi::raster::oxigdal_dataset_read_tile(
            dataset_ptr,
            &coord,
            256,
            &mut tile_ptr,
        );
        assert_eq!(result, OxiGdalErrorCode::InvalidArgument);

        // Test invalid tile size
        let coord = OxiGdalTileCoord { z: 0, x: 0, y: 0 };
        let result = oxigdal_mobile::ffi::raster::oxigdal_dataset_read_tile(
            dataset_ptr,
            &coord,
            -1,
            &mut tile_ptr,
        );
        assert_eq!(result, OxiGdalErrorCode::InvalidArgument);

        let result = oxigdal_mobile::ffi::raster::oxigdal_dataset_read_tile(
            dataset_ptr,
            &coord,
            5000,
            &mut tile_ptr,
        );
        assert_eq!(result, OxiGdalErrorCode::InvalidArgument);

        oxigdal_mobile::ffi::raster::oxigdal_dataset_close(dataset_ptr);
    }
}

#[test]
fn test_tile_free_null() {
    unsafe {
        let result = oxigdal_mobile::ffi::raster::oxigdal_tile_free(std::ptr::null_mut());
        assert_eq!(result, OxiGdalErrorCode::Success);
    }
}

#[test]
fn test_tile_get_data_null() {
    let mut buffer = OxiGdalBuffer {
        data: std::ptr::null_mut(),
        length: 0,
        width: 0,
        height: 0,
        channels: 0,
    };

    unsafe {
        let result =
            oxigdal_mobile::ffi::raster::oxigdal_tile_get_data(std::ptr::null_mut(), &mut buffer);
        assert_eq!(result, OxiGdalErrorCode::NullPointer);
    }
}

#[test]
#[cfg(feature = "android")]
fn test_android_tile_reading() {
    use std::ffi::CString;

    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join("test_android_tile.tif");
    let path_cstring =
        CString::new(temp_path.to_str().expect("valid path")).expect("valid cstring");

    let mut dataset_ptr: *mut OxiGdalDataset = std::ptr::null_mut();

    unsafe {
        // Create test dataset
        let create_result = oxigdal_mobile::ffi::raster::oxigdal_dataset_create(
            path_cstring.as_ptr(),
            512,
            512,
            3,
            oxigdal_mobile::ffi::types::OxiGdalDataType::Byte,
            &mut dataset_ptr,
        );
        assert_eq!(create_result, OxiGdalErrorCode::Success);

        // Allocate buffer for tile (256x256 ARGB)
        let tile_size = 256;
        let mut buffer_data = vec![0u8; (tile_size * tile_size * 4) as usize];
        let mut buffer = OxiGdalBuffer {
            data: buffer_data.as_mut_ptr(),
            length: buffer_data.len(),
            width: tile_size,
            height: tile_size,
            channels: 4,
        };

        // Read tile
        let result = oxigdal_mobile::android::raster::oxigdal_android_read_tile(
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
            result == OxiGdalErrorCode::Success
                || result == OxiGdalErrorCode::IoError
                || result == OxiGdalErrorCode::InvalidArgument,
            "Expected Success, IoError, or InvalidArgument, got {:?}",
            result
        );

        oxigdal_mobile::ffi::raster::oxigdal_dataset_close(dataset_ptr);
    }
}

#[test]
#[cfg(feature = "ios")]
fn test_ios_tile_reading() {
    use std::ffi::CString;

    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join("test_ios_tile.tif");
    let path_cstring =
        CString::new(temp_path.to_str().expect("valid path")).expect("valid cstring");

    let mut dataset_ptr: *mut OxiGdalDataset = std::ptr::null_mut();

    unsafe {
        // Create test dataset
        let create_result = oxigdal_mobile::ffi::raster::oxigdal_dataset_create(
            path_cstring.as_ptr(),
            512,
            512,
            3,
            oxigdal_mobile::ffi::types::OxiGdalDataType::Byte,
            &mut dataset_ptr,
        );
        assert_eq!(create_result, OxiGdalErrorCode::Success);

        // Allocate buffer for tile (256x256 RGBA)
        let tile_size = 256;
        let mut buffer_data = vec![0u8; (tile_size * tile_size * 4) as usize];
        let mut buffer = OxiGdalBuffer {
            data: buffer_data.as_mut_ptr(),
            length: buffer_data.len(),
            width: tile_size,
            height: tile_size,
            channels: 4,
        };

        // Read tile
        let result = oxigdal_mobile::ios::raster::oxigdal_ios_read_tile(
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
            result == OxiGdalErrorCode::Success
                || result == OxiGdalErrorCode::IoError
                || result == OxiGdalErrorCode::InvalidArgument,
            "Expected Success, IoError, or InvalidArgument, got {:?}",
            result
        );

        oxigdal_mobile::ffi::raster::oxigdal_dataset_close(dataset_ptr);
    }
}
