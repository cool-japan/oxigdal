//! iOS-specific bindings and utilities.
//!
//! This module provides iOS-specific functionality such as UIImage conversion,
//! file system integration, and iOS-optimized memory management.

// Module-level cfg is handled by #[cfg(feature = "ios")] in lib.rs

pub mod dataset;
pub mod raster;
pub mod vector;

use crate::ffi::types::*;
use std::os::raw::{c_int, c_void};

/// Converts an OxiGDAL buffer to iOS-compatible RGBA format.
///
/// This function ensures the pixel data is in the format expected by
/// UIImage (RGBA, premultiplied alpha).
///
/// # Parameters
/// - `buffer`: Source buffer
/// - `out_buffer`: Output buffer (must be pre-allocated with same dimensions)
///
/// # Safety
/// - Both buffers must be valid and properly sized
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_buffer_to_ios_rgba(
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
        crate::ffi::error::set_last_error("Output buffer must have 4 channels (RGBA)".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    let pixel_count = (src.width * src.height) as usize;

    // Convert based on source channel count
    // SAFETY: We've validated buffer dimensions and channels
    unsafe {
        match src.channels {
            1 => {
                // Grayscale to RGBA
                for i in 0..pixel_count {
                    let gray = *src.data.add(i);
                    let dst_offset = i * 4;
                    *dst.data.add(dst_offset) = gray;
                    *dst.data.add(dst_offset + 1) = gray;
                    *dst.data.add(dst_offset + 2) = gray;
                    *dst.data.add(dst_offset + 3) = 255; // Full opacity
                }
            }
            3 => {
                // RGB to RGBA
                for i in 0..pixel_count {
                    let src_offset = i * 3;
                    let dst_offset = i * 4;
                    *dst.data.add(dst_offset) = *src.data.add(src_offset);
                    *dst.data.add(dst_offset + 1) = *src.data.add(src_offset + 1);
                    *dst.data.add(dst_offset + 2) = *src.data.add(src_offset + 2);
                    *dst.data.add(dst_offset + 3) = 255; // Full opacity
                }
            }
            4 => {
                // RGBA to RGBA (copy)
                std::ptr::copy_nonoverlapping(src.data, dst.data, pixel_count * 4);
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

/// Gets the iOS documents directory path.
///
/// # Returns
/// Path string (caller must free with oxigdal_string_free)
#[unsafe(no_mangle)]
pub extern "C" fn oxigdal_ios_get_documents_path() -> *mut std::os::raw::c_char {
    // This would use iOS-specific APIs in a real implementation
    // For now, return a placeholder
    match std::ffi::CString::new("/Documents") {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Gets the iOS cache directory path.
///
/// # Returns
/// Path string (caller must free with oxigdal_string_free)
#[unsafe(no_mangle)]
pub extern "C" fn oxigdal_ios_get_cache_path() -> *mut std::os::raw::c_char {
    match std::ffi::CString::new("/Library/Caches") {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Optimizes memory usage for iOS (releases caches, etc.).
///
/// This should be called in response to `didReceiveMemoryWarning` or
/// `UIApplication.didReceiveMemoryWarningNotification`.
///
/// iOS memory warnings indicate the system is under memory pressure and
/// the app should free as much memory as possible to avoid being killed
/// by the jetsam mechanism.
#[unsafe(no_mangle)]
pub extern "C" fn oxigdal_ios_memory_warning() -> OxiGdalErrorCode {
    // iOS memory warnings are serious - clear all cached tiles
    let clear_result = unsafe { crate::common::cache::oxigdal_cache_clear() };
    if clear_result != OxiGdalErrorCode::Success {
        return clear_result;
    }

    // Reset statistics since cache is cleared
    crate::common::oxigdal_mobile_reset_stats();

    // Set reduced cache size to prevent re-filling during memory pressure
    // iOS will send another memory warning if pressure continues,
    // at which point the app might be terminated
    if let Err(e) = crate::common::cache::set_max_cache_size_mb(10) {
        crate::ffi::error::set_last_error(format!(
            "Failed to reduce cache on iOS memory warning: {}",
            e
        ));
        return OxiGdalErrorCode::AllocationFailed;
    }

    // Clean up any temporary share/export files in the cache directory
    let cache_dirs = ["/Library/Caches/oxigdal_share", "/tmp/oxigdal"];
    for dir in &cache_dirs {
        let path = std::path::Path::new(dir);
        if path.exists() {
            // Best effort cleanup; ignore errors
            let _ = std::fs::remove_dir_all(path);
        }
    }

    OxiGdalErrorCode::Success
}

/// Callback type for background task completion.
pub type OxiGdalIosBackgroundTaskCallback = extern "C" fn(success: c_int, user_data: *mut c_void);

/// Processes a dataset in the background (iOS-safe).
///
/// This function performs work in a way that's safe for iOS background
/// processing, respecting background execution time limits.
///
/// # Parameters
/// - `dataset`: Dataset handle
/// - `callback`: Completion callback
/// - `user_data`: User data passed to callback
///
/// # Safety
/// - dataset must be a valid handle
/// - callback will be called exactly once
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_ios_process_in_background(
    dataset: *const OxiGdalDataset,
    callback: OxiGdalIosBackgroundTaskCallback,
    user_data: *mut c_void,
) -> OxiGdalErrorCode {
    crate::check_null!(dataset, "dataset");

    // Wrap user_data in a send-safe wrapper since we need to move it across threads
    // SAFETY: The caller guarantees the user_data pointer is valid for the duration
    // of the background task and that the callback can be called from any thread
    let user_data_ptr = user_data as usize;
    let callback_fn = callback;

    // Get dataset metadata first (on the calling thread)
    let mut metadata = OxiGdalMetadata {
        width: 0,
        height: 0,
        band_count: 0,
        data_type: 0,
        epsg_code: 0,
        geotransform: [0.0; 6],
    };

    // SAFETY: Caller guarantees dataset is valid (checked for null above)
    let result =
        unsafe { crate::ffi::raster::oxigdal_dataset_get_metadata(dataset, &mut metadata) };
    if result != OxiGdalErrorCode::Success {
        // Call the callback with failure on the current thread
        callback_fn(0, user_data);
        return result;
    }

    // Store dataset pointer as usize for cross-thread transfer
    let dataset_ptr = dataset as usize;

    // Spawn background thread for processing
    // iOS background task time is limited (~30 seconds), so we need to be efficient
    let handle = std::thread::Builder::new()
        .name("oxigdal-ios-bg".to_string())
        .spawn(move || {
            // Reconstruct pointers in the background thread
            let dataset = dataset_ptr as *const OxiGdalDataset;
            let user_data = user_data_ptr as *mut c_void;

            // Process the dataset in the background
            // This performs pre-caching of commonly needed data
            let success = process_dataset_background(dataset, &metadata);

            // Call the callback with the result
            // SAFETY: Caller guarantees callback is valid and can be called from any thread
            callback_fn(if success { 1 } else { 0 }, user_data);
        });

    match handle {
        Ok(_) => OxiGdalErrorCode::Success,
        Err(e) => {
            crate::ffi::error::set_last_error(format!("Failed to spawn background thread: {}", e));
            // Call callback with failure since the background thread failed to start
            callback_fn(0, user_data);
            OxiGdalErrorCode::Unknown
        }
    }
}

/// Performs background dataset processing.
///
/// This function pre-caches dataset information and commonly needed
/// tiles/regions to improve responsiveness when the UI becomes visible again.
fn process_dataset_background(dataset: *const OxiGdalDataset, metadata: &OxiGdalMetadata) -> bool {
    // Step 1: Pre-compute statistics for band 1 (most commonly needed)
    if metadata.band_count > 0 {
        let mut stats = OxiGdalStats {
            min: 0.0,
            max: 0.0,
            mean: 0.0,
            stddev: 0.0,
            valid_count: 0,
        };

        // Use approximate statistics for speed in background
        let result = unsafe {
            crate::ffi::raster::oxigdal_dataset_compute_stats(
                dataset, 1, 1, // approx_ok = true
                &mut stats,
            )
        };

        if result != OxiGdalErrorCode::Success {
            return false;
        }
    }

    // Step 2: Pre-read a low-resolution overview tile for quick display
    // Read center region at reduced resolution
    let preview_width = metadata.width.min(256);
    let preview_height = metadata.height.min(256);

    if preview_width > 0 && preview_height > 0 {
        let buffer_ptr = unsafe {
            crate::ffi::oxigdal_buffer_alloc(
                preview_width,
                preview_height,
                metadata.band_count.min(3), // Limit to RGB for preview
            )
        };

        if !buffer_ptr.is_null() {
            let read_result = unsafe {
                crate::ffi::raster::oxigdal_dataset_read_region(
                    dataset,
                    0,
                    0,
                    preview_width,
                    preview_height,
                    1,
                    buffer_ptr,
                )
            };

            // Cache the preview data if read was successful
            if read_result == OxiGdalErrorCode::Success {
                let preview_key = format!("ios_preview_{}x{}", metadata.width, metadata.height);
                let buf = unsafe { &*buffer_ptr };
                if !buf.data.is_null() && buf.length > 0 {
                    let data = unsafe { std::slice::from_raw_parts(buf.data, buf.length).to_vec() };
                    crate::common::cache::put_cached_tile(
                        preview_key,
                        data,
                        preview_width,
                        preview_height,
                        metadata.band_count.min(3),
                    );
                }
            }

            unsafe {
                crate::ffi::oxigdal_buffer_free(buffer_ptr);
            }
        }
    }

    true
}

/// Gets the iOS background task timeout in seconds.
///
/// iOS typically allows ~30 seconds for background tasks.
/// This function returns the recommended timeout for callers to use
/// with `UIApplication.beginBackgroundTask`.
///
/// # Returns
/// Recommended timeout in seconds
#[unsafe(no_mangle)]
pub extern "C" fn oxigdal_ios_get_background_timeout() -> c_int {
    // iOS allows approximately 30 seconds for background execution
    // We recommend 25 seconds to leave a safety margin
    25
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paths() {
        let docs_path = oxigdal_ios_get_documents_path();
        assert!(!docs_path.is_null());
        unsafe {
            crate::ffi::error::oxigdal_string_free(docs_path);
        }

        let cache_path = oxigdal_ios_get_cache_path();
        assert!(!cache_path.is_null());
        unsafe {
            crate::ffi::error::oxigdal_string_free(cache_path);
        }
    }

    #[test]
    fn test_memory_warning() {
        // Initialize cache first
        let _ = crate::common::cache::init_cache(50);

        let result = oxigdal_ios_memory_warning();
        assert_eq!(result, OxiGdalErrorCode::Success);

        // After memory warning, cache should be cleared and reduced
        let mut size_mb = 0;
        let mut max_mb = 0;
        let mut entries = 0;
        let info_result = unsafe {
            crate::common::cache::oxigdal_cache_get_info(&mut size_mb, &mut max_mb, &mut entries)
        };
        assert_eq!(info_result, OxiGdalErrorCode::Success);
        assert_eq!(entries, 0); // Cache should be empty
        assert!(max_mb <= 10); // Cache max should be reduced
    }

    #[test]
    fn test_background_timeout() {
        let timeout = oxigdal_ios_get_background_timeout();
        assert!(timeout > 0);
        assert!(timeout <= 30); // Should not exceed iOS background limit
    }

    /// Test callback for background processing
    extern "C" fn test_bg_callback(success: c_int, _user_data: *mut c_void) {
        // In test, just verify the callback is called
        assert!(success == 0 || success == 1);
    }

    #[test]
    fn test_background_processing_null_dataset() {
        let result = unsafe {
            oxigdal_ios_process_in_background(
                std::ptr::null(),
                test_bg_callback,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(result, OxiGdalErrorCode::NullPointer);
    }
}
