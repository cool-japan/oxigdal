//! Android-specific bindings and utilities.
//!
//! This module provides Android-specific functionality including JNI bindings,
//! Bitmap conversion, and Android-optimized memory management.

// Module-level cfg is handled by #[cfg(feature = "android")] in lib.rs

pub mod dataset;
pub mod raster;
pub mod vector;

use crate::ffi::types::*;

#[cfg(feature = "android")]
use jni::EnvUnowned;
#[cfg(feature = "android")]
use jni::Outcome;
#[cfg(feature = "android")]
use jni::objects::{JClass, JObject, JString};
#[cfg(feature = "android")]
use jni::sys::{jbyteArray, jint, jlong, jstring};

/// Converts an OxiGeo buffer to Android Bitmap format.
///
/// Android Bitmaps use ARGB_8888 format by default.
///
/// # Parameters
/// - `buffer`: Source buffer
/// - `out_buffer`: Output buffer (must be pre-allocated with 4 channels)
///
/// # Safety
/// - Both buffers must be valid and properly sized
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigeo_buffer_to_android_argb(
    buffer: *const OxiGeoBuffer,
    out_buffer: *mut OxiGeoBuffer,
) -> OxiGeoErrorCode {
    crate::check_null!(buffer, "buffer");
    crate::check_null!(out_buffer, "out_buffer");

    let src = crate::deref_ptr!(buffer, OxiGeoBuffer, "buffer");
    let dst = crate::deref_ptr_mut!(out_buffer, OxiGeoBuffer, "out_buffer");

    if src.width != dst.width || src.height != dst.height {
        crate::ffi::error::set_last_error("Buffer dimensions mismatch".to_string());
        return OxiGeoErrorCode::InvalidArgument;
    }

    if dst.channels != 4 {
        crate::ffi::error::set_last_error("Output buffer must have 4 channels (ARGB)".to_string());
        return OxiGeoErrorCode::InvalidArgument;
    }

    let pixel_count = (src.width * src.height) as usize;

    // Convert to ARGB format
    // SAFETY: We've validated buffer dimensions and channels
    unsafe {
        match src.channels {
            1 => {
                // Grayscale to ARGB
                for i in 0..pixel_count {
                    let gray = *src.data.add(i);
                    let dst_offset = i * 4;
                    *dst.data.add(dst_offset) = 255; // Alpha
                    *dst.data.add(dst_offset + 1) = gray; // Red
                    *dst.data.add(dst_offset + 2) = gray; // Green
                    *dst.data.add(dst_offset + 3) = gray; // Blue
                }
            }
            3 => {
                // RGB to ARGB
                for i in 0..pixel_count {
                    let src_offset = i * 3;
                    let dst_offset = i * 4;
                    *dst.data.add(dst_offset) = 255; // Alpha
                    *dst.data.add(dst_offset + 1) = *src.data.add(src_offset); // Red
                    *dst.data.add(dst_offset + 2) = *src.data.add(src_offset + 1); // Green
                    *dst.data.add(dst_offset + 3) = *src.data.add(src_offset + 2); // Blue
                }
            }
            4 => {
                // RGBA to ARGB (swap R and B channels, move A)
                for i in 0..pixel_count {
                    let src_offset = i * 4;
                    let dst_offset = i * 4;
                    let r = *src.data.add(src_offset);
                    let g = *src.data.add(src_offset + 1);
                    let b = *src.data.add(src_offset + 2);
                    let a = *src.data.add(src_offset + 3);
                    *dst.data.add(dst_offset) = a; // Alpha
                    *dst.data.add(dst_offset + 1) = r; // Red
                    *dst.data.add(dst_offset + 2) = g; // Green
                    *dst.data.add(dst_offset + 3) = b; // Blue
                }
            }
            _ => {
                crate::ffi::error::set_last_error(format!(
                    "Unsupported channel count: {}",
                    src.channels
                ));
                return OxiGeoErrorCode::UnsupportedFormat;
            }
        }
    }

    OxiGeoErrorCode::Success
}

/// Gets the Android external storage directory path.
///
/// # Returns
/// Path string (caller must free with oxigeo_string_free)
#[unsafe(no_mangle)]
pub extern "C" fn oxigeo_android_get_external_storage_path() -> *mut std::os::raw::c_char {
    match std::ffi::CString::new("/sdcard") {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Gets the Android cache directory path.
///
/// # Returns
/// Path string (caller must free with oxigeo_string_free)
#[unsafe(no_mangle)]
pub extern "C" fn oxigeo_android_get_cache_path() -> *mut std::os::raw::c_char {
    match std::ffi::CString::new("/data/data/cache") {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Handles Android low memory situation.
///
/// This should be called when `onLowMemory()` is triggered.
/// Performs aggressive cache clearing and memory optimization.
#[unsafe(no_mangle)]
pub extern "C" fn oxigeo_android_on_low_memory() -> OxiGeoErrorCode {
    // Low memory is critical - clear all caches
    let clear_result = unsafe { crate::common::cache::oxigeo_cache_clear() };
    if clear_result != OxiGeoErrorCode::Success {
        return clear_result;
    }

    // Reset statistics since cache is cleared
    crate::common::oxigeo_mobile_reset_stats();

    // Set minimum cache size to prevent re-filling
    if let Err(e) = crate::common::cache::set_max_cache_size_mb(5) {
        crate::ffi::error::set_last_error(format!("Failed to reduce cache on low memory: {}", e));
        return OxiGeoErrorCode::AllocationFailed;
    }

    OxiGeoErrorCode::Success
}

/// Handles Android trim memory request.
///
/// Implements graduated memory trimming based on Android TRIM_MEMORY levels.
/// Higher levels indicate more urgent need to free memory.
///
/// # Parameters
/// - `level`: TRIM_MEMORY level from Android
///
/// # TRIM_MEMORY Levels
/// - 5 (RUNNING_MODERATE): App is not killable, running with moderate memory
/// - 10 (RUNNING_LOW): App is not killable, running with low memory
/// - 15 (RUNNING_CRITICAL): App is not killable, memory critically low
/// - 20 (UI_HIDDEN): App's UI is no longer visible
/// - 40 (BACKGROUND): App is in background LRU list
/// - 60 (MODERATE): App is in middle of background LRU list
/// - 80 (COMPLETE): App will be killed soon if memory is not freed
#[unsafe(no_mangle)]
pub extern "C" fn oxigeo_android_on_trim_memory(level: std::os::raw::c_int) -> OxiGeoErrorCode {
    // Android TRIM_MEMORY level constants
    const TRIM_MEMORY_RUNNING_MODERATE: i32 = 5;
    const TRIM_MEMORY_RUNNING_LOW: i32 = 10;
    const TRIM_MEMORY_RUNNING_CRITICAL: i32 = 15;
    const TRIM_MEMORY_UI_HIDDEN: i32 = 20;
    const TRIM_MEMORY_BACKGROUND: i32 = 40;
    const TRIM_MEMORY_MODERATE: i32 = 60;
    const TRIM_MEMORY_COMPLETE: i32 = 80;

    // Determine cache action based on trim level
    if level >= TRIM_MEMORY_COMPLETE {
        // About to be killed - release everything
        let clear_result = unsafe { crate::common::cache::oxigeo_cache_clear() };
        if clear_result != OxiGeoErrorCode::Success {
            return clear_result;
        }
        crate::common::oxigeo_mobile_reset_stats();

        // Set absolute minimum cache
        if let Err(e) = crate::common::cache::set_max_cache_size_mb(2) {
            crate::ffi::error::set_last_error(format!(
                "Failed to minimize cache at TRIM_MEMORY_COMPLETE: {}",
                e
            ));
            return OxiGeoErrorCode::AllocationFailed;
        }
    } else if level >= TRIM_MEMORY_MODERATE {
        // Moderate background pressure - reduce cache significantly
        if let Err(e) = crate::common::cache::set_max_cache_size_mb(5) {
            crate::ffi::error::set_last_error(format!(
                "Failed to reduce cache at TRIM_MEMORY_MODERATE: {}",
                e
            ));
            return OxiGeoErrorCode::AllocationFailed;
        }
    } else if level >= TRIM_MEMORY_BACKGROUND {
        // In background - reduce cache moderately
        if let Err(e) = crate::common::cache::set_max_cache_size_mb(10) {
            crate::ffi::error::set_last_error(format!(
                "Failed to reduce cache at TRIM_MEMORY_BACKGROUND: {}",
                e
            ));
            return OxiGeoErrorCode::AllocationFailed;
        }
    } else if level >= TRIM_MEMORY_UI_HIDDEN {
        // UI hidden - reduce cache slightly
        if let Err(e) = crate::common::cache::set_max_cache_size_mb(25) {
            crate::ffi::error::set_last_error(format!(
                "Failed to reduce cache at TRIM_MEMORY_UI_HIDDEN: {}",
                e
            ));
            return OxiGeoErrorCode::AllocationFailed;
        }
    } else if level >= TRIM_MEMORY_RUNNING_CRITICAL {
        // Running but critically low - reduce cache
        if let Err(e) = crate::common::cache::set_max_cache_size_mb(15) {
            crate::ffi::error::set_last_error(format!(
                "Failed to reduce cache at TRIM_MEMORY_RUNNING_CRITICAL: {}",
                e
            ));
            return OxiGeoErrorCode::AllocationFailed;
        }
    } else if level >= TRIM_MEMORY_RUNNING_LOW {
        // Running but low memory - reduce cache moderately
        if let Err(e) = crate::common::cache::set_max_cache_size_mb(25) {
            crate::ffi::error::set_last_error(format!(
                "Failed to reduce cache at TRIM_MEMORY_RUNNING_LOW: {}",
                e
            ));
            return OxiGeoErrorCode::AllocationFailed;
        }
    } else if level >= TRIM_MEMORY_RUNNING_MODERATE {
        // Running with moderate memory - slight reduction
        if let Err(e) = crate::common::cache::set_max_cache_size_mb(50) {
            crate::ffi::error::set_last_error(format!(
                "Failed to reduce cache at TRIM_MEMORY_RUNNING_MODERATE: {}",
                e
            ));
            return OxiGeoErrorCode::AllocationFailed;
        }
    }
    // Levels below RUNNING_MODERATE don't need action

    OxiGeoErrorCode::Success
}

// JNI bindings (when android feature is enabled)

#[cfg(feature = "android")]
#[unsafe(no_mangle)]
/// Initializes the OxiGeo library for Android.
///
/// # Returns
/// 0 on success, non-zero error code on failure.
pub extern "system" fn Java_com_cooljapan_oxigeo_OxiGeo_nativeInit(
    _env: EnvUnowned,
    _class: JClass,
) -> jint {
    crate::ffi::oxigeo_init() as jint
}

#[cfg(feature = "android")]
#[unsafe(no_mangle)]
/// Gets the OxiGeo version string.
///
/// # Returns
/// A JNI string containing the version information, or null on error.
pub extern "system" fn Java_com_cooljapan_oxigeo_OxiGeo_nativeGetVersion(
    mut unowned_env: EnvUnowned,
    _class: JClass,
) -> jstring {
    let version_ptr = crate::ffi::raster::oxigeo_get_version_string();
    if version_ptr.is_null() {
        return std::ptr::null_mut();
    }

    unsafe {
        let version_cstr = std::ffi::CStr::from_ptr(version_ptr);
        let version_str = match version_cstr.to_str() {
            Ok(s) => s,
            Err(_) => {
                crate::ffi::error::oxigeo_string_free(version_ptr);
                return std::ptr::null_mut();
            }
        };

        let result = match unowned_env
            .with_env(|env| JString::new(env, version_str))
            .into_outcome()
        {
            Outcome::Ok(s) => s.into_raw(),
            _ => std::ptr::null_mut(),
        };

        crate::ffi::error::oxigeo_string_free(version_ptr);
        result
    }
}

#[cfg(feature = "android")]
#[unsafe(no_mangle)]
/// Opens a geospatial dataset from the given file path.
///
/// # Returns
/// A dataset handle (pointer) on success, or 0 on failure.
pub extern "system" fn Java_com_cooljapan_oxigeo_OxiGeo_nativeOpenDataset(
    mut unowned_env: EnvUnowned,
    _class: JClass,
    path: JString,
) -> jlong {
    let path_str: String = match unsafe {
        unowned_env
            .with_env(|env| {
                let chars = path.mutf8_chars(env)?;
                Ok::<_, jni::errors::Error>(chars.to_string())
            })
            .into_outcome()
    } {
        Outcome::Ok(s) => s,
        _ => return 0,
    };

    let path_cstr = match std::ffi::CString::new(path_str) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let mut dataset: *mut OxiGeoDataset = std::ptr::null_mut();

    unsafe {
        let result = crate::ffi::raster::oxigeo_dataset_open(
            path_cstr.as_ptr(),
            &mut dataset as *mut *mut OxiGeoDataset,
        );

        if result != OxiGeoErrorCode::Success {
            return 0;
        }
    }

    dataset as jlong
}

/// JNI binding to close a dataset.
#[cfg(feature = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cooljapan_oxigeo_OxiGeo_nativeCloseDataset(
    _env: EnvUnowned,
    _class: JClass,
    dataset_ptr: jlong,
) {
    if dataset_ptr == 0 {
        return;
    }

    unsafe {
        crate::ffi::raster::oxigeo_dataset_close(dataset_ptr as *mut OxiGeoDataset);
    }
}

/// JNI binding to get dataset width.
#[cfg(feature = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cooljapan_oxigeo_OxiGeo_nativeGetWidth(
    _env: EnvUnowned,
    _class: JClass,
    dataset_ptr: jlong,
) -> jint {
    if dataset_ptr == 0 {
        return 0;
    }

    let mut metadata = OxiGeoMetadata {
        width: 0,
        height: 0,
        band_count: 0,
        data_type: 0,
        epsg_code: 0,
        geotransform: [0.0; 6],
    };

    unsafe {
        let result = crate::ffi::raster::oxigeo_dataset_get_metadata(
            dataset_ptr as *const OxiGeoDataset,
            &mut metadata,
        );

        if result != OxiGeoErrorCode::Success {
            return 0;
        }
    }

    metadata.width
}

/// JNI binding to get dataset height.
#[cfg(feature = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cooljapan_oxigeo_OxiGeo_nativeGetHeight(
    _env: EnvUnowned,
    _class: JClass,
    dataset_ptr: jlong,
) -> jint {
    if dataset_ptr == 0 {
        return 0;
    }

    let mut metadata = OxiGeoMetadata {
        width: 0,
        height: 0,
        band_count: 0,
        data_type: 0,
        epsg_code: 0,
        geotransform: [0.0; 6],
    };

    unsafe {
        let result = crate::ffi::raster::oxigeo_dataset_get_metadata(
            dataset_ptr as *const OxiGeoDataset,
            &mut metadata,
        );

        if result != OxiGeoErrorCode::Success {
            return 0;
        }
    }

    metadata.height
}

/// JNI binding to read a region from dataset.
#[cfg(feature = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cooljapan_oxigeo_OxiGeo_nativeReadRegion(
    mut unowned_env: EnvUnowned,
    _class: JClass,
    dataset_ptr: jlong,
    x_off: jint,
    y_off: jint,
    width: jint,
    height: jint,
    band: jint,
) -> jbyteArray {
    if dataset_ptr == 0 {
        return std::ptr::null_mut();
    }

    let channels = 3; // RGB
    let buffer_size = (width * height * channels) as usize;

    // Allocate buffer
    let buffer_ptr = unsafe { crate::ffi::oxigeo_buffer_alloc(width, height, channels) };
    if buffer_ptr.is_null() {
        return std::ptr::null_mut();
    }

    // Read data
    unsafe {
        let result = crate::ffi::raster::oxigeo_dataset_read_region(
            dataset_ptr as *const OxiGeoDataset,
            x_off,
            y_off,
            width,
            height,
            band,
            buffer_ptr,
        );

        if result != OxiGeoErrorCode::Success {
            crate::ffi::oxigeo_buffer_free(buffer_ptr);
            return std::ptr::null_mut();
        }

        let buffer = &*buffer_ptr;

        // Create Java byte array and copy data via with_env
        let slice = std::slice::from_raw_parts(buffer.data as *const i8, buffer_size);
        let array_result = unowned_env
            .with_env(|env| {
                let byte_array = jni::objects::JByteArray::new(env, buffer_size)?;
                byte_array.set_region(env, 0, slice)?;
                Ok::<_, jni::errors::Error>(byte_array.into_raw())
            })
            .into_outcome();

        crate::ffi::oxigeo_buffer_free(buffer_ptr);

        match array_result {
            Outcome::Ok(raw) => raw,
            _ => std::ptr::null_mut(),
        }
    }
}

/// JNI binding to read a map tile in XYZ scheme.
///
/// Mirrors the Kotlin `OxiGeo.Dataset.readTile` contract (`OxiGeo.kt`'s
/// `nativeReadTile` `external fun`): reads the real dataset pixels for tile
/// `(z, x, y)` at `tile_size` through the same overview-selection pipeline
/// used by [`crate::android::raster::oxigeo_android_read_tile`], then
/// converts down to a 3-channel (RGB) byte layout to match
/// `nativeReadRegion`'s `channels = 3` contract that the Kotlin
/// `ImageBuffer` assumes.
///
/// Returns `null` (rather than a Java exception) on any failure; the Kotlin
/// wrapper (`OxiGeo.Dataset.readTile`) turns a `null` return into an
/// `IOErrorException`.
#[cfg(feature = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cooljapan_oxigeo_OxiGeo_nativeReadTile(
    mut unowned_env: EnvUnowned,
    _class: JClass,
    dataset_ptr: jlong,
    z: jint,
    x: jint,
    y: jint,
    tile_size: jint,
) -> jbyteArray {
    if dataset_ptr == 0 {
        return std::ptr::null_mut();
    }

    if z < 0 || x < 0 || y < 0 || tile_size <= 0 || tile_size > 4096 {
        return std::ptr::null_mut();
    }

    // Read the tile as ARGB via the shared, already-tested Android tile
    // pipeline (real GeoTIFF overview-selection logic; see
    // `android::raster::oxigeo_android_read_tile`).
    let argb_len = (tile_size as usize) * (tile_size as usize) * 4;
    let mut argb_data = vec![0u8; argb_len];
    let mut argb_buffer = OxiGeoBuffer {
        data: argb_data.as_mut_ptr(),
        length: argb_data.len(),
        width: tile_size,
        height: tile_size,
        channels: 4,
    };

    let result = unsafe {
        crate::android::raster::oxigeo_android_read_tile(
            dataset_ptr as *const OxiGeoDataset,
            z,
            x,
            y,
            tile_size,
            &mut argb_buffer,
        )
    };

    if result != OxiGeoErrorCode::Success {
        return std::ptr::null_mut();
    }

    // Convert ARGB -> RGB (drop the alpha channel) so the returned byte
    // array matches `nativeReadRegion`'s 3-channel contract.
    let pixel_count = (tile_size as usize) * (tile_size as usize);
    let mut rgb_data = vec![0u8; pixel_count * 3];
    for i in 0..pixel_count {
        let src = i * 4;
        let dst = i * 3;
        // argb_data[src] is alpha; [src+1, src+2, src+3] are R, G, B.
        rgb_data[dst] = argb_data[src + 1];
        rgb_data[dst + 1] = argb_data[src + 2];
        rgb_data[dst + 2] = argb_data[src + 3];
    }

    // Create Java byte array and copy data via with_env, same pattern as
    // `nativeReadRegion` above.
    let array_result = unowned_env
        .with_env(|env| {
            let byte_array = jni::objects::JByteArray::new(env, rgb_data.len())?;
            // SAFETY: `rgb_data` is a valid, fully-initialized `Vec<u8>` of
            // exactly `rgb_data.len()` bytes; reinterpreting as `&[i8]` for
            // `set_region` is a same-size, same-layout reborrow (JNI byte
            // arrays are signed 8-bit), matching `nativeReadRegion`'s
            // identical conversion.
            let signed_slice = unsafe {
                std::slice::from_raw_parts(rgb_data.as_ptr().cast::<i8>(), rgb_data.len())
            };
            byte_array.set_region(env, 0, signed_slice)?;
            Ok::<_, jni::errors::Error>(byte_array.into_raw())
        })
        .into_outcome();

    match array_result {
        Outcome::Ok(raw) => raw,
        _ => std::ptr::null_mut(),
    }
}

/// JNI binding to get dataset band count.
#[cfg(feature = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cooljapan_oxigeo_OxiGeo_nativeGetBandCount(
    _env: EnvUnowned,
    _class: JClass,
    dataset_ptr: jlong,
) -> jint {
    if dataset_ptr == 0 {
        return 0;
    }

    let mut metadata = OxiGeoMetadata {
        width: 0,
        height: 0,
        band_count: 0,
        data_type: 0,
        epsg_code: 0,
        geotransform: [0.0; 6],
    };

    unsafe {
        let result = crate::ffi::raster::oxigeo_dataset_get_metadata(
            dataset_ptr as *const OxiGeoDataset,
            &mut metadata,
        );

        if result != OxiGeoErrorCode::Success {
            return 0;
        }
    }

    metadata.band_count
}

/// JNI binding to get dataset data type.
#[cfg(feature = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cooljapan_oxigeo_OxiGeo_nativeGetDataType(
    _env: EnvUnowned,
    _class: JClass,
    dataset_ptr: jlong,
) -> jint {
    if dataset_ptr == 0 {
        return 0;
    }

    let mut metadata = OxiGeoMetadata {
        width: 0,
        height: 0,
        band_count: 0,
        data_type: 0,
        epsg_code: 0,
        geotransform: [0.0; 6],
    };

    unsafe {
        let result = crate::ffi::raster::oxigeo_dataset_get_metadata(
            dataset_ptr as *const OxiGeoDataset,
            &mut metadata,
        );

        if result != OxiGeoErrorCode::Success {
            return 0;
        }
    }

    metadata.data_type
}

/// JNI binding to get dataset EPSG code.
#[cfg(feature = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cooljapan_oxigeo_OxiGeo_nativeGetEpsgCode(
    _env: EnvUnowned,
    _class: JClass,
    dataset_ptr: jlong,
) -> jint {
    if dataset_ptr == 0 {
        return 0;
    }

    let mut metadata = OxiGeoMetadata {
        width: 0,
        height: 0,
        band_count: 0,
        data_type: 0,
        epsg_code: 0,
        geotransform: [0.0; 6],
    };

    unsafe {
        let result = crate::ffi::raster::oxigeo_dataset_get_metadata(
            dataset_ptr as *const OxiGeoDataset,
            &mut metadata,
        );

        if result != OxiGeoErrorCode::Success {
            return 0;
        }
    }

    metadata.epsg_code
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_paths() {
        let storage_path = oxigeo_android_get_external_storage_path();
        assert!(!storage_path.is_null());
        unsafe {
            crate::ffi::error::oxigeo_string_free(storage_path);
        }

        let cache_path = oxigeo_android_get_cache_path();
        assert!(!cache_path.is_null());
        unsafe {
            crate::ffi::error::oxigeo_string_free(cache_path);
        }
    }

    #[test]
    fn test_memory_callbacks() {
        let result = oxigeo_android_on_low_memory();
        assert_eq!(result, OxiGeoErrorCode::Success);

        let result = oxigeo_android_on_trim_memory(20);
        assert_eq!(result, OxiGeoErrorCode::Success);
    }

    #[test]
    fn test_metadata_getters_null_safety() {
        use std::ptr;

        // Test that getting metadata from null dataset pointer returns safe defaults
        let null_dataset: *const OxiGeoDataset = ptr::null();

        let mut metadata = OxiGeoMetadata {
            width: 0,
            height: 0,
            band_count: 0,
            data_type: 0,
            epsg_code: 0,
            geotransform: [0.0; 6],
        };

        // Getting metadata from null pointer should fail safely
        unsafe {
            let result =
                crate::ffi::raster::oxigeo_dataset_get_metadata(null_dataset, &mut metadata);
            // Should return an error code (not Success)
            assert_ne!(result, OxiGeoErrorCode::Success);
        }
    }

    #[test]
    fn test_metadata_getters_with_dataset() {
        use std::ffi::CString;
        use std::ptr;

        // Create a test dataset
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join("test_android_metadata_getters.tif");
        let path_cstring = match CString::new(temp_path.to_str().expect("valid path")) {
            Ok(s) => s,
            Err(_) => {
                panic!("Failed to create CString");
            }
        };

        let mut dataset_ptr: *mut OxiGeoDataset = ptr::null_mut();

        unsafe {
            // Create a dataset with known metadata
            let result = crate::ffi::raster::oxigeo_dataset_create(
                path_cstring.as_ptr(),
                256, // width
                256, // height
                3,   // bands (RGB)
                OxiGeoDataType::Byte,
                &mut dataset_ptr,
            );
            assert_eq!(result, OxiGeoErrorCode::Success);
            assert!(!dataset_ptr.is_null());

            // Set EPSG code
            let epsg_result = crate::ffi::raster::oxigeo_dataset_set_projection_epsg(
                dataset_ptr,
                4326, // WGS84
            );
            assert_eq!(epsg_result, OxiGeoErrorCode::Success);

            // Get metadata using FFI
            let mut metadata = OxiGeoMetadata {
                width: 0,
                height: 0,
                band_count: 0,
                data_type: 0,
                epsg_code: 0,
                geotransform: [0.0; 6],
            };

            let result = crate::ffi::raster::oxigeo_dataset_get_metadata(
                dataset_ptr as *const OxiGeoDataset,
                &mut metadata,
            );
            assert_eq!(result, OxiGeoErrorCode::Success);

            // Verify metadata values
            assert_eq!(metadata.width, 256);
            assert_eq!(metadata.height, 256);
            assert_eq!(metadata.band_count, 3);
            assert_eq!(metadata.data_type, OxiGeoDataType::Byte as i32);
            assert_eq!(metadata.epsg_code, 4326);

            // Clean up
            let close_result = crate::ffi::raster::oxigeo_dataset_close(dataset_ptr);
            assert_eq!(close_result, OxiGeoErrorCode::Success);
        }

        // Clean up temp file
        let _ = std::fs::remove_file(temp_path);
    }

    #[test]
    fn test_metadata_getters_different_data_types() {
        use std::ffi::CString;
        use std::ptr;

        let data_types = vec![
            (OxiGeoDataType::Byte, "byte"),
            (OxiGeoDataType::UInt16, "uint16"),
            (OxiGeoDataType::Int16, "int16"),
            (OxiGeoDataType::Float32, "float32"),
        ];

        for (data_type, type_name) in data_types {
            let temp_dir = std::env::temp_dir();
            let temp_path = temp_dir.join(format!("test_android_metadata_{}.tif", type_name));
            let path_cstring = match CString::new(temp_path.to_str().expect("valid path")) {
                Ok(s) => s,
                Err(_) => {
                    panic!("Failed to create CString");
                }
            };

            let mut dataset_ptr: *mut OxiGeoDataset = ptr::null_mut();

            unsafe {
                let result = crate::ffi::raster::oxigeo_dataset_create(
                    path_cstring.as_ptr(),
                    100,
                    100,
                    1,
                    data_type,
                    &mut dataset_ptr,
                );
                assert_eq!(result, OxiGeoErrorCode::Success);
                assert!(!dataset_ptr.is_null());

                // Get metadata
                let mut metadata = OxiGeoMetadata {
                    width: 0,
                    height: 0,
                    band_count: 0,
                    data_type: 0,
                    epsg_code: 0,
                    geotransform: [0.0; 6],
                };

                let result = crate::ffi::raster::oxigeo_dataset_get_metadata(
                    dataset_ptr as *const OxiGeoDataset,
                    &mut metadata,
                );
                assert_eq!(result, OxiGeoErrorCode::Success);

                // Verify data type is correct
                assert_eq!(
                    metadata.data_type, data_type as i32,
                    "Data type mismatch for {}",
                    type_name
                );
                assert_eq!(metadata.band_count, 1);

                // Clean up
                let close_result = crate::ffi::raster::oxigeo_dataset_close(dataset_ptr);
                assert_eq!(close_result, OxiGeoErrorCode::Success);
            }

            // Clean up temp file
            let _ = std::fs::remove_file(temp_path);
        }
    }

    #[test]
    fn test_metadata_getters_multi_band() {
        use std::ffi::CString;
        use std::ptr;

        let band_counts = vec![1, 3, 4, 6];

        for band_count in band_counts {
            let temp_dir = std::env::temp_dir();
            let temp_path =
                temp_dir.join(format!("test_android_metadata_{}_bands.tif", band_count));
            let path_cstring = match CString::new(temp_path.to_str().expect("valid path")) {
                Ok(s) => s,
                Err(_) => {
                    panic!("Failed to create CString");
                }
            };

            let mut dataset_ptr: *mut OxiGeoDataset = ptr::null_mut();

            unsafe {
                let result = crate::ffi::raster::oxigeo_dataset_create(
                    path_cstring.as_ptr(),
                    50,
                    50,
                    band_count,
                    OxiGeoDataType::Byte,
                    &mut dataset_ptr,
                );
                assert_eq!(result, OxiGeoErrorCode::Success);
                assert!(!dataset_ptr.is_null());

                // Get metadata
                let mut metadata = OxiGeoMetadata {
                    width: 0,
                    height: 0,
                    band_count: 0,
                    data_type: 0,
                    epsg_code: 0,
                    geotransform: [0.0; 6],
                };

                let result = crate::ffi::raster::oxigeo_dataset_get_metadata(
                    dataset_ptr as *const OxiGeoDataset,
                    &mut metadata,
                );
                assert_eq!(result, OxiGeoErrorCode::Success);

                // Verify band count is correct
                assert_eq!(metadata.band_count, band_count, "Band count mismatch");

                // Clean up
                let close_result = crate::ffi::raster::oxigeo_dataset_close(dataset_ptr);
                assert_eq!(close_result, OxiGeoErrorCode::Success);
            }

            // Clean up temp file
            let _ = std::fs::remove_file(temp_path);
        }
    }
}
