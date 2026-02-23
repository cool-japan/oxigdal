//! Foreign Function Interface (FFI) layer for mobile bindings.
//!
//! This module provides a C-compatible API that can be safely called from
//! iOS (Swift/Objective-C) and Android (Java/Kotlin) applications.
//!
//! # Safety
//!
//! All functions in this module are marked `unsafe` or `extern "C"` because
//! they cross FFI boundaries. However, they perform extensive validation
//! internally to ensure memory safety:
//!
//! - Null pointer checks on all pointer arguments
//! - Bounds checking on array accesses
//! - UTF-8 validation on string conversions
//! - Proper resource cleanup via close/free functions
//!
//! # Memory Management
//!
//! The FFI layer follows these conventions:
//!
//! - Functions that create handles (e.g., `*_open`, `*_create`) must be
//!   paired with corresponding cleanup functions (`*_close`, `*_free`)
//! - Strings returned by OxiGDAL must be freed with `oxigdal_string_free`
//! - Buffers are caller-allocated; OxiGDAL only writes to them
//! - Opaque handles must not be dereferenced on the FFI side
//!
//! # Error Handling
//!
//! All functions return `OxiGdalErrorCode`:
//! - `Success` (0) indicates success
//! - Non-zero values indicate specific error types
//! - Detailed error messages can be retrieved with `oxigdal_get_last_error`
//!
//! # Threading
//!
//! The FFI layer is designed to be thread-safe:
//! - Error messages are stored per-thread
//! - Handles can be used from different threads (with external synchronization)
//! - No global mutable state (except thread-local error storage)

pub mod error;
pub mod raster;
pub mod types;
pub mod vector;

// Re-export commonly used items
pub use error::{oxigdal_get_last_error, oxigdal_string_free};
pub use raster::*;
pub use types::*;
pub use vector::*;

/// Initializes the OxiGDAL library.
///
/// This should be called once before using any other OxiGDAL functions.
/// It is safe to call multiple times; subsequent calls are no-ops.
///
/// # Returns
/// - `Success` if initialization succeeds
/// - Error code on failure
#[unsafe(no_mangle)]
pub extern "C" fn oxigdal_init() -> OxiGdalErrorCode {
    // Initialize logging (only once)
    #[cfg(feature = "std")]
    {
        use std::sync::Once;
        static INIT: Once = Once::new();

        INIT.call_once(|| {
            // Logging setup would go here if needed
            // For now, keeping FFI layer logging-free to avoid dependencies
        });
    }

    OxiGdalErrorCode::Success
}

/// Cleans up resources used by OxiGDAL.
///
/// This should be called when the application is done using OxiGDAL.
/// After calling this, `oxigdal_init` must be called again before using
/// other functions.
///
/// # Safety
/// All dataset/layer/feature handles must be closed before calling this.
#[unsafe(no_mangle)]
pub extern "C" fn oxigdal_cleanup() -> OxiGdalErrorCode {
    error::clear_last_error();
    OxiGdalErrorCode::Success
}

/// Checks if a file format is supported.
///
/// # Parameters
/// - `path`: File path to check
///
/// # Returns
/// - 1 if format is supported
/// - 0 if not supported or on error
///
/// # Safety
/// - path must be a valid null-terminated string
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_is_format_supported(
    path: *const std::os::raw::c_char,
) -> std::os::raw::c_int {
    if path.is_null() {
        return 0;
    }

    let path_str = unsafe {
        match std::ffi::CStr::from_ptr(path).to_str() {
            Ok(s) => s,
            Err(_) => return 0,
        }
    };

    // Check file extension
    let ext = std::path::Path::new(path_str)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    // Supported formats (expand as drivers are implemented)
    match ext.to_lowercase().as_str() {
        "tif" | "tiff" | "geotiff" => 1,
        "json" | "geojson" => 1,
        "shp" | "shapefile" => 1,
        "gpkg" | "geopackage" => 1,
        "png" | "jpg" | "jpeg" => 1,
        _ => 0,
    }
}

/// Allocates a buffer for image data.
///
/// # Parameters
/// - `width`: Width in pixels
/// - `height`: Height in pixels
/// - `channels`: Number of channels (e.g., 3 for RGB, 4 for RGBA)
///
/// # Returns
/// Pointer to allocated buffer, or null on failure
///
/// # Safety
/// Caller must free the buffer with `oxigdal_buffer_free`
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_buffer_alloc(
    width: std::os::raw::c_int,
    height: std::os::raw::c_int,
    channels: std::os::raw::c_int,
) -> *mut OxiGdalBuffer {
    if width <= 0 || height <= 0 || channels <= 0 {
        error::set_last_error("Invalid buffer dimensions".to_string());
        return std::ptr::null_mut();
    }

    let length = (width * height * channels) as usize;

    // Allocate pixel data
    let layout = std::alloc::Layout::from_size_align(length, 1);
    let data = match layout {
        Ok(layout) => unsafe { std::alloc::alloc(layout) },
        Err(_) => {
            error::set_last_error("Failed to create buffer layout".to_string());
            return std::ptr::null_mut();
        }
    };

    if data.is_null() {
        error::set_last_error("Failed to allocate buffer memory".to_string());
        return std::ptr::null_mut();
    }

    // Zero-initialize
    unsafe {
        std::ptr::write_bytes(data, 0, length);
    }

    // Create buffer structure
    let buffer = Box::new(OxiGdalBuffer {
        data,
        length,
        width,
        height,
        channels,
    });

    Box::into_raw(buffer)
}

/// Frees a buffer allocated with `oxigdal_buffer_alloc`.
///
/// # Safety
/// - buffer must have been allocated with `oxigdal_buffer_alloc`
/// - Must not be used after calling this function
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_buffer_free(buffer: *mut OxiGdalBuffer) {
    if buffer.is_null() {
        return;
    }

    unsafe {
        let buf = Box::from_raw(buffer);

        if !buf.data.is_null() {
            let layout = std::alloc::Layout::from_size_align_unchecked(buf.length, 1);
            std::alloc::dealloc(buf.data, layout);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_cleanup() {
        let result = oxigdal_init();
        assert_eq!(result, OxiGdalErrorCode::Success);

        let result = oxigdal_cleanup();
        assert_eq!(result, OxiGdalErrorCode::Success);
    }

    #[test]
    fn test_format_support() {
        unsafe {
            let path_tiff = std::ffi::CString::new("/path/to/file.tif").expect("valid string");
            let supported = oxigdal_is_format_supported(path_tiff.as_ptr());
            assert_eq!(supported, 1);

            let path_json = std::ffi::CString::new("/path/to/file.geojson").expect("valid string");
            let supported = oxigdal_is_format_supported(path_json.as_ptr());
            assert_eq!(supported, 1);

            let path_unknown = std::ffi::CString::new("/path/to/file.xyz").expect("valid string");
            let supported = oxigdal_is_format_supported(path_unknown.as_ptr());
            assert_eq!(supported, 0);
        }
    }

    #[test]
    fn test_buffer_alloc_free() {
        unsafe {
            let buffer = oxigdal_buffer_alloc(256, 256, 3);
            assert!(!buffer.is_null());

            let buf = &*buffer;
            assert_eq!(buf.width, 256);
            assert_eq!(buf.height, 256);
            assert_eq!(buf.channels, 3);
            assert_eq!(buf.length, 256 * 256 * 3);
            assert!(!buf.data.is_null());

            oxigdal_buffer_free(buffer);
        }
    }

    #[test]
    fn test_buffer_alloc_invalid() {
        unsafe {
            let buffer = oxigdal_buffer_alloc(-1, 256, 3);
            assert!(buffer.is_null());

            let buffer = oxigdal_buffer_alloc(256, -1, 3);
            assert!(buffer.is_null());

            let buffer = oxigdal_buffer_alloc(256, 256, 0);
            assert!(buffer.is_null());
        }
    }
}
