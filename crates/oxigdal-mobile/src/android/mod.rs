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
use jni::JNIEnv;
#[cfg(feature = "android")]
use jni::objects::{JClass, JObject, JString};
#[cfg(feature = "android")]
use jni::sys::{jbyteArray, jint, jlong, jstring};

/// Converts an OxiGDAL buffer to Android Bitmap format.
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
pub unsafe extern "C" fn oxigdal_buffer_to_android_argb(
    buffer: *const OxiGdalBuffer,
    out_buffer: *mut OxiGdalBuffer,
) -> OxiGdalErrorCode {
    crate::check_null!(buffer, "buffer");
    crate::check_null!(out_buffer, "out_buffer");

    let src = crate::deref_ptr!(buffer, OxiGdalBuffer, "buffer");
    let dst = crate::deref_ptr_mut!(out_buffer, OxiGdalBuffer, "out_buffer");

    if src.width != dst.width || src.height != dst.height {
        crate::ffi::error::set_last_error("Buffer dimensions mismatch".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    if dst.channels != 4 {
        crate::ffi::error::set_last_error("Output buffer must have 4 channels (ARGB)".to_string());
        return OxiGdalErrorCode::InvalidArgument;
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
                return OxiGdalErrorCode::UnsupportedFormat;
            }
        }
    }

    OxiGdalErrorCode::Success
}

/// Gets the Android external storage directory path.
///
/// # Returns
/// Path string (caller must free with oxigdal_string_free)
#[unsafe(no_mangle)]
pub extern "C" fn oxigdal_android_get_external_storage_path() -> *mut std::os::raw::c_char {
    match std::ffi::CString::new("/sdcard") {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Gets the Android cache directory path.
///
/// # Returns
/// Path string (caller must free with oxigdal_string_free)
#[unsafe(no_mangle)]
pub extern "C" fn oxigdal_android_get_cache_path() -> *mut std::os::raw::c_char {
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
pub extern "C" fn oxigdal_android_on_low_memory() -> OxiGdalErrorCode {
    // Low memory is critical - clear all caches
    let clear_result = unsafe { crate::common::cache::oxigdal_cache_clear() };
    if clear_result != OxiGdalErrorCode::Success {
        return clear_result;
    }

    // Reset statistics since cache is cleared
    crate::common::oxigdal_mobile_reset_stats();

    // Set minimum cache size to prevent re-filling
    if let Err(e) = crate::common::cache::set_max_cache_size_mb(5) {
        crate::ffi::error::set_last_error(format!("Failed to reduce cache on low memory: {}", e));
        return OxiGdalErrorCode::AllocationFailed;
    }

    OxiGdalErrorCode::Success
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
pub extern "C" fn oxigdal_android_on_trim_memory(level: std::os::raw::c_int) -> OxiGdalErrorCode {
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
        let clear_result = unsafe { crate::common::cache::oxigdal_cache_clear() };
        if clear_result != OxiGdalErrorCode::Success {
            return clear_result;
        }
        crate::common::oxigdal_mobile_reset_stats();

        // Set absolute minimum cache
        if let Err(e) = crate::common::cache::set_max_cache_size_mb(2) {
            crate::ffi::error::set_last_error(format!(
                "Failed to minimize cache at TRIM_MEMORY_COMPLETE: {}",
                e
            ));
            return OxiGdalErrorCode::AllocationFailed;
        }
    } else if level >= TRIM_MEMORY_MODERATE {
        // Moderate background pressure - reduce cache significantly
        if let Err(e) = crate::common::cache::set_max_cache_size_mb(5) {
            crate::ffi::error::set_last_error(format!(
                "Failed to reduce cache at TRIM_MEMORY_MODERATE: {}",
                e
            ));
            return OxiGdalErrorCode::AllocationFailed;
        }
    } else if level >= TRIM_MEMORY_BACKGROUND {
        // In background - reduce cache moderately
        if let Err(e) = crate::common::cache::set_max_cache_size_mb(10) {
            crate::ffi::error::set_last_error(format!(
                "Failed to reduce cache at TRIM_MEMORY_BACKGROUND: {}",
                e
            ));
            return OxiGdalErrorCode::AllocationFailed;
        }
    } else if level >= TRIM_MEMORY_UI_HIDDEN {
        // UI hidden - reduce cache slightly
        if let Err(e) = crate::common::cache::set_max_cache_size_mb(25) {
            crate::ffi::error::set_last_error(format!(
                "Failed to reduce cache at TRIM_MEMORY_UI_HIDDEN: {}",
                e
            ));
            return OxiGdalErrorCode::AllocationFailed;
        }
    } else if level >= TRIM_MEMORY_RUNNING_CRITICAL {
        // Running but critically low - reduce cache
        if let Err(e) = crate::common::cache::set_max_cache_size_mb(15) {
            crate::ffi::error::set_last_error(format!(
                "Failed to reduce cache at TRIM_MEMORY_RUNNING_CRITICAL: {}",
                e
            ));
            return OxiGdalErrorCode::AllocationFailed;
        }
    } else if level >= TRIM_MEMORY_RUNNING_LOW {
        // Running but low memory - reduce cache moderately
        if let Err(e) = crate::common::cache::set_max_cache_size_mb(25) {
            crate::ffi::error::set_last_error(format!(
                "Failed to reduce cache at TRIM_MEMORY_RUNNING_LOW: {}",
                e
            ));
            return OxiGdalErrorCode::AllocationFailed;
        }
    } else if level >= TRIM_MEMORY_RUNNING_MODERATE {
        // Running with moderate memory - slight reduction
        if let Err(e) = crate::common::cache::set_max_cache_size_mb(50) {
            crate::ffi::error::set_last_error(format!(
                "Failed to reduce cache at TRIM_MEMORY_RUNNING_MODERATE: {}",
                e
            ));
            return OxiGdalErrorCode::AllocationFailed;
        }
    }
    // Levels below RUNNING_MODERATE don't need action

    OxiGdalErrorCode::Success
}

// JNI bindings (when android feature is enabled)

#[cfg(feature = "android")]
#[unsafe(no_mangle)]
/// Initializes the OxiGDAL library for Android.
///
/// # Returns
/// 0 on success, non-zero error code on failure.
pub extern "system" fn Java_com_cooljapan_oxigdal_OxiGDAL_nativeInit(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    crate::ffi::oxigdal_init() as jint
}

#[cfg(feature = "android")]
#[unsafe(no_mangle)]
/// Gets the OxiGDAL version string.
///
/// # Returns
/// A JNI string containing the version information, or null on error.
pub extern "system" fn Java_com_cooljapan_oxigdal_OxiGDAL_nativeGetVersion(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let version_ptr = crate::ffi::raster::oxigdal_get_version_string();
    if version_ptr.is_null() {
        return std::ptr::null_mut();
    }

    unsafe {
        let version_cstr = std::ffi::CStr::from_ptr(version_ptr);
        let version_str = match version_cstr.to_str() {
            Ok(s) => s,
            Err(_) => {
                crate::ffi::error::oxigdal_string_free(version_ptr);
                return std::ptr::null_mut();
            }
        };

        let result = match env.new_string(version_str) {
            Ok(s) => s.into_raw(),
            Err(_) => std::ptr::null_mut(),
        };

        crate::ffi::error::oxigdal_string_free(version_ptr);
        result
    }
}

#[cfg(feature = "android")]
#[unsafe(no_mangle)]
/// Opens a geospatial dataset from the given file path.
///
/// # Returns
/// A dataset handle (pointer) on success, or 0 on failure.
pub extern "system" fn Java_com_cooljapan_oxigdal_OxiGDAL_nativeOpenDataset(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
) -> jlong {
    let path_str: String = match env.get_string(&path) {
        Ok(s) => s.into(),
        Err(_) => return 0,
    };

    let path_cstr = match std::ffi::CString::new(path_str) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let mut dataset: *mut OxiGdalDataset = std::ptr::null_mut();

    unsafe {
        let result = crate::ffi::raster::oxigdal_dataset_open(
            path_cstr.as_ptr(),
            &mut dataset as *mut *mut OxiGdalDataset,
        );

        if result != OxiGdalErrorCode::Success {
            return 0;
        }
    }

    dataset as jlong
}

/// JNI binding to close a dataset.
#[cfg(feature = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cooljapan_oxigdal_OxiGDAL_nativeCloseDataset(
    _env: JNIEnv,
    _class: JClass,
    dataset_ptr: jlong,
) {
    if dataset_ptr == 0 {
        return;
    }

    unsafe {
        crate::ffi::raster::oxigdal_dataset_close(dataset_ptr as *mut OxiGdalDataset);
    }
}

/// JNI binding to get dataset width.
#[cfg(feature = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cooljapan_oxigdal_OxiGDAL_nativeGetWidth(
    _env: JNIEnv,
    _class: JClass,
    dataset_ptr: jlong,
) -> jint {
    if dataset_ptr == 0 {
        return 0;
    }

    let mut metadata = OxiGdalMetadata {
        width: 0,
        height: 0,
        band_count: 0,
        data_type: 0,
        epsg_code: 0,
        geotransform: [0.0; 6],
    };

    unsafe {
        let result = crate::ffi::raster::oxigdal_dataset_get_metadata(
            dataset_ptr as *const OxiGdalDataset,
            &mut metadata,
        );

        if result != OxiGdalErrorCode::Success {
            return 0;
        }
    }

    metadata.width
}

/// JNI binding to get dataset height.
#[cfg(feature = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cooljapan_oxigdal_OxiGDAL_nativeGetHeight(
    _env: JNIEnv,
    _class: JClass,
    dataset_ptr: jlong,
) -> jint {
    if dataset_ptr == 0 {
        return 0;
    }

    let mut metadata = OxiGdalMetadata {
        width: 0,
        height: 0,
        band_count: 0,
        data_type: 0,
        epsg_code: 0,
        geotransform: [0.0; 6],
    };

    unsafe {
        let result = crate::ffi::raster::oxigdal_dataset_get_metadata(
            dataset_ptr as *const OxiGdalDataset,
            &mut metadata,
        );

        if result != OxiGdalErrorCode::Success {
            return 0;
        }
    }

    metadata.height
}

/// JNI binding to read a region from dataset.
#[cfg(feature = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cooljapan_oxigdal_OxiGDAL_nativeReadRegion(
    env: JNIEnv,
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
    let buffer_ptr = unsafe { crate::ffi::oxigdal_buffer_alloc(width, height, channels) };
    if buffer_ptr.is_null() {
        return std::ptr::null_mut();
    }

    // Read data
    unsafe {
        let result = crate::ffi::raster::oxigdal_dataset_read_region(
            dataset_ptr as *const OxiGdalDataset,
            x_off,
            y_off,
            width,
            height,
            band,
            buffer_ptr,
        );

        if result != OxiGdalErrorCode::Success {
            crate::ffi::oxigdal_buffer_free(buffer_ptr);
            return std::ptr::null_mut();
        }

        let buffer = &*buffer_ptr;

        // Create Java byte array
        let byte_array = match env.new_byte_array(buffer_size as i32) {
            Ok(arr) => arr,
            Err(_) => {
                crate::ffi::oxigdal_buffer_free(buffer_ptr);
                return std::ptr::null_mut();
            }
        };

        // Copy data to Java array
        let slice = std::slice::from_raw_parts(buffer.data as *const i8, buffer_size);
        if env.set_byte_array_region(&byte_array, 0, slice).is_err() {
            crate::ffi::oxigdal_buffer_free(buffer_ptr);
            return std::ptr::null_mut();
        }

        crate::ffi::oxigdal_buffer_free(buffer_ptr);
        byte_array.into_raw()
    }
}

/// JNI binding to get dataset band count.
#[cfg(feature = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cooljapan_oxigdal_OxiGDAL_nativeGetBandCount(
    _env: JNIEnv,
    _class: JClass,
    dataset_ptr: jlong,
) -> jint {
    if dataset_ptr == 0 {
        return 0;
    }

    let mut metadata = OxiGdalMetadata {
        width: 0,
        height: 0,
        band_count: 0,
        data_type: 0,
        epsg_code: 0,
        geotransform: [0.0; 6],
    };

    unsafe {
        let result = crate::ffi::raster::oxigdal_dataset_get_metadata(
            dataset_ptr as *const OxiGdalDataset,
            &mut metadata,
        );

        if result != OxiGdalErrorCode::Success {
            return 0;
        }
    }

    metadata.band_count
}

/// JNI binding to get dataset data type.
#[cfg(feature = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cooljapan_oxigdal_OxiGDAL_nativeGetDataType(
    _env: JNIEnv,
    _class: JClass,
    dataset_ptr: jlong,
) -> jint {
    if dataset_ptr == 0 {
        return 0;
    }

    let mut metadata = OxiGdalMetadata {
        width: 0,
        height: 0,
        band_count: 0,
        data_type: 0,
        epsg_code: 0,
        geotransform: [0.0; 6],
    };

    unsafe {
        let result = crate::ffi::raster::oxigdal_dataset_get_metadata(
            dataset_ptr as *const OxiGdalDataset,
            &mut metadata,
        );

        if result != OxiGdalErrorCode::Success {
            return 0;
        }
    }

    metadata.data_type
}

/// JNI binding to get dataset EPSG code.
#[cfg(feature = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cooljapan_oxigdal_OxiGDAL_nativeGetEpsgCode(
    _env: JNIEnv,
    _class: JClass,
    dataset_ptr: jlong,
) -> jint {
    if dataset_ptr == 0 {
        return 0;
    }

    let mut metadata = OxiGdalMetadata {
        width: 0,
        height: 0,
        band_count: 0,
        data_type: 0,
        epsg_code: 0,
        geotransform: [0.0; 6],
    };

    unsafe {
        let result = crate::ffi::raster::oxigdal_dataset_get_metadata(
            dataset_ptr as *const OxiGdalDataset,
            &mut metadata,
        );

        if result != OxiGdalErrorCode::Success {
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
        let storage_path = oxigdal_android_get_external_storage_path();
        assert!(!storage_path.is_null());
        unsafe {
            crate::ffi::error::oxigdal_string_free(storage_path);
        }

        let cache_path = oxigdal_android_get_cache_path();
        assert!(!cache_path.is_null());
        unsafe {
            crate::ffi::error::oxigdal_string_free(cache_path);
        }
    }

    #[test]
    fn test_memory_callbacks() {
        let result = oxigdal_android_on_low_memory();
        assert_eq!(result, OxiGdalErrorCode::Success);

        let result = oxigdal_android_on_trim_memory(20);
        assert_eq!(result, OxiGdalErrorCode::Success);
    }

    #[test]
    fn test_metadata_getters_null_safety() {
        use std::ptr;

        // Test that getting metadata from null dataset pointer returns safe defaults
        let null_dataset: *const OxiGdalDataset = ptr::null();

        let mut metadata = OxiGdalMetadata {
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
                crate::ffi::raster::oxigdal_dataset_get_metadata(null_dataset, &mut metadata);
            // Should return an error code (not Success)
            assert_ne!(result, OxiGdalErrorCode::Success);
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

        let mut dataset_ptr: *mut OxiGdalDataset = ptr::null_mut();

        unsafe {
            // Create a dataset with known metadata
            let result = crate::ffi::raster::oxigdal_dataset_create(
                path_cstring.as_ptr(),
                256, // width
                256, // height
                3,   // bands (RGB)
                OxiGdalDataType::Byte,
                &mut dataset_ptr,
            );
            assert_eq!(result, OxiGdalErrorCode::Success);
            assert!(!dataset_ptr.is_null());

            // Set EPSG code
            let epsg_result = crate::ffi::raster::oxigdal_dataset_set_projection_epsg(
                dataset_ptr,
                4326, // WGS84
            );
            assert_eq!(epsg_result, OxiGdalErrorCode::Success);

            // Get metadata using FFI
            let mut metadata = OxiGdalMetadata {
                width: 0,
                height: 0,
                band_count: 0,
                data_type: 0,
                epsg_code: 0,
                geotransform: [0.0; 6],
            };

            let result = crate::ffi::raster::oxigdal_dataset_get_metadata(
                dataset_ptr as *const OxiGdalDataset,
                &mut metadata,
            );
            assert_eq!(result, OxiGdalErrorCode::Success);

            // Verify metadata values
            assert_eq!(metadata.width, 256);
            assert_eq!(metadata.height, 256);
            assert_eq!(metadata.band_count, 3);
            assert_eq!(metadata.data_type, OxiGdalDataType::Byte as i32);
            assert_eq!(metadata.epsg_code, 4326);

            // Clean up
            let close_result = crate::ffi::raster::oxigdal_dataset_close(dataset_ptr);
            assert_eq!(close_result, OxiGdalErrorCode::Success);
        }

        // Clean up temp file
        let _ = std::fs::remove_file(temp_path);
    }

    #[test]
    fn test_metadata_getters_different_data_types() {
        use std::ffi::CString;
        use std::ptr;

        let data_types = vec![
            (OxiGdalDataType::Byte, "byte"),
            (OxiGdalDataType::UInt16, "uint16"),
            (OxiGdalDataType::Int16, "int16"),
            (OxiGdalDataType::Float32, "float32"),
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

            let mut dataset_ptr: *mut OxiGdalDataset = ptr::null_mut();

            unsafe {
                let result = crate::ffi::raster::oxigdal_dataset_create(
                    path_cstring.as_ptr(),
                    100,
                    100,
                    1,
                    data_type,
                    &mut dataset_ptr,
                );
                assert_eq!(result, OxiGdalErrorCode::Success);
                assert!(!dataset_ptr.is_null());

                // Get metadata
                let mut metadata = OxiGdalMetadata {
                    width: 0,
                    height: 0,
                    band_count: 0,
                    data_type: 0,
                    epsg_code: 0,
                    geotransform: [0.0; 6],
                };

                let result = crate::ffi::raster::oxigdal_dataset_get_metadata(
                    dataset_ptr as *const OxiGdalDataset,
                    &mut metadata,
                );
                assert_eq!(result, OxiGdalErrorCode::Success);

                // Verify data type is correct
                assert_eq!(
                    metadata.data_type, data_type as i32,
                    "Data type mismatch for {}",
                    type_name
                );
                assert_eq!(metadata.band_count, 1);

                // Clean up
                let close_result = crate::ffi::raster::oxigdal_dataset_close(dataset_ptr);
                assert_eq!(close_result, OxiGdalErrorCode::Success);
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

            let mut dataset_ptr: *mut OxiGdalDataset = ptr::null_mut();

            unsafe {
                let result = crate::ffi::raster::oxigdal_dataset_create(
                    path_cstring.as_ptr(),
                    50,
                    50,
                    band_count,
                    OxiGdalDataType::Byte,
                    &mut dataset_ptr,
                );
                assert_eq!(result, OxiGdalErrorCode::Success);
                assert!(!dataset_ptr.is_null());

                // Get metadata
                let mut metadata = OxiGdalMetadata {
                    width: 0,
                    height: 0,
                    band_count: 0,
                    data_type: 0,
                    epsg_code: 0,
                    geotransform: [0.0; 6],
                };

                let result = crate::ffi::raster::oxigdal_dataset_get_metadata(
                    dataset_ptr as *const OxiGdalDataset,
                    &mut metadata,
                );
                assert_eq!(result, OxiGdalErrorCode::Success);

                // Verify band count is correct
                assert_eq!(metadata.band_count, band_count, "Band count mismatch");

                // Clean up
                let close_result = crate::ffi::raster::oxigdal_dataset_close(dataset_ptr);
                assert_eq!(close_result, OxiGdalErrorCode::Success);
            }

            // Clean up temp file
            let _ = std::fs::remove_file(temp_path);
        }
    }
}
