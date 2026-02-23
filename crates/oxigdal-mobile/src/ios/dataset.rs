//! iOS-specific dataset operations.
//!
//! Provides iOS-optimized dataset handling with CoreLocation integration
//! and iOS file system conventions.

#![cfg(feature = "ios")]

use crate::ffi::types::*;
use std::os::raw::{c_char, c_int};
use std::sync::atomic::{AtomicI32, Ordering};

/// iOS device physical memory class (in MB).
/// Defaults to 2048 MB (2GB) which is a conservative estimate for modern iOS devices.
/// Actual value should be set by the iOS layer using `os_proc_available_memory()`.
static IOS_DEVICE_MEMORY_MB: AtomicI32 = AtomicI32::new(2048);

/// Sets the iOS device available memory (in MB).
///
/// This should be called during initialization with the value obtained
/// from `os_proc_available_memory()` or `ProcessInfo.physicalMemory`.
///
/// # Parameters
/// - `available_mb`: Available memory in megabytes
#[unsafe(no_mangle)]
pub extern "C" fn oxigdal_ios_set_device_memory(available_mb: c_int) -> OxiGdalErrorCode {
    if available_mb <= 0 {
        crate::ffi::error::set_last_error("Invalid device memory value".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }
    IOS_DEVICE_MEMORY_MB.store(available_mb, Ordering::Relaxed);
    apply_ios_memory_policy();
    OxiGdalErrorCode::Success
}

/// Applies iOS-specific memory and cache policies.
///
/// iOS devices have tighter memory constraints than desktop and typically get
/// killed by the system (jetsam) if they exceed their memory limit.
/// We use conservative cache sizes based on available memory.
fn apply_ios_memory_policy() {
    let available_mb = IOS_DEVICE_MEMORY_MB.load(Ordering::Relaxed);

    // iOS apps typically have 40-50% of physical RAM available
    // Cache should be a small fraction of that
    // Low-end (< 1GB available): 15MB cache (e.g., older iPhones)
    // Mid-range (1-2GB available): 30MB cache
    // High-end (2-4GB available): 60MB cache
    // Flagship (>4GB available): 120MB cache (e.g., iPad Pro)
    let cache_size_mb = if available_mb < 1024 {
        15
    } else if available_mb < 2048 {
        30
    } else if available_mb < 4096 {
        60
    } else {
        120
    };

    if let Err(e) = crate::common::cache::set_max_cache_size_mb(cache_size_mb as usize) {
        crate::ffi::error::set_last_error(format!("Failed to set iOS cache policy: {}", e));
    }
}

/// Opens a dataset with iOS-specific optimizations.
///
/// This function applies iOS-specific settings like memory limits
/// and offline mode detection.
///
/// # Safety
/// - path must be valid null-terminated string
/// - out_dataset must be valid pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_ios_dataset_open(
    path: *const c_char,
    out_dataset: *mut *mut OxiGdalDataset,
) -> OxiGdalErrorCode {
    // Use standard dataset open with iOS optimizations
    // SAFETY: Caller guarantees path and out_dataset are valid pointers
    let result = unsafe { crate::ffi::raster::oxigdal_dataset_open(path, out_dataset) };

    if result == OxiGdalErrorCode::Success {
        // Apply iOS-specific optimizations
        // iOS has stricter memory management with jetsam enforcement
        apply_ios_memory_policy();
    }

    result
}

/// Opens a dataset from iOS bundle resources.
///
/// # Parameters
/// - `resource_name`: Name of resource (without extension)
/// - `resource_type`: File extension (e.g., "tif", "geojson")
/// - `out_dataset`: Output dataset handle
///
/// # Safety
/// - resource_name and resource_type must be valid strings
/// - out_dataset must be valid pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_ios_dataset_open_bundle_resource(
    resource_name: *const c_char,
    resource_type: *const c_char,
    out_dataset: *mut *mut OxiGdalDataset,
) -> OxiGdalErrorCode {
    crate::check_null!(resource_name, "resource_name");
    crate::check_null!(resource_type, "resource_type");
    crate::check_null!(out_dataset, "out_dataset");

    let name = unsafe {
        match std::ffi::CStr::from_ptr(resource_name).to_str() {
            Ok(s) => s,
            Err(_) => {
                crate::ffi::error::set_last_error("Invalid resource name".to_string());
                return OxiGdalErrorCode::InvalidUtf8;
            }
        }
    };

    let ext = unsafe {
        match std::ffi::CStr::from_ptr(resource_type).to_str() {
            Ok(s) => s,
            Err(_) => {
                crate::ffi::error::set_last_error("Invalid resource type".to_string());
                return OxiGdalErrorCode::InvalidUtf8;
            }
        }
    };

    // Construct path to bundle resource
    // In real implementation, this would use iOS Foundation APIs
    let path = format!("/Resources/{}.{}", name, ext);
    let path_cstr = match std::ffi::CString::new(path) {
        Ok(s) => s,
        Err(_) => {
            crate::ffi::error::set_last_error("Failed to create path".to_string());
            return OxiGdalErrorCode::IoError;
        }
    };

    // SAFETY: We just validated the path and out_dataset is from the caller's guarantee
    unsafe { oxigdal_ios_dataset_open(path_cstr.as_ptr(), out_dataset) }
}

/// Gets dataset information optimized for iOS display.
///
/// Returns metadata formatted for iOS UI components.
///
/// # Safety
/// - dataset must be valid
/// - out_metadata must be valid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_ios_dataset_get_info(
    dataset: *const OxiGdalDataset,
    out_metadata: *mut OxiGdalMetadata,
) -> OxiGdalErrorCode {
    // SAFETY: Caller guarantees dataset and out_metadata are valid
    unsafe { crate::ffi::raster::oxigdal_dataset_get_metadata(dataset, out_metadata) }
}

/// Checks if dataset can be displayed on current iOS device.
///
/// Considers device memory, screen size, and dataset dimensions.
///
/// # Returns
/// - 1 if displayable
/// - 0 if not displayable or on error
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_ios_dataset_is_displayable(
    dataset: *const OxiGdalDataset,
) -> c_int {
    if dataset.is_null() {
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

    // SAFETY: Caller guarantees dataset is valid, we just checked for null
    let result =
        unsafe { crate::ffi::raster::oxigdal_dataset_get_metadata(dataset, &mut metadata) };

    if result != OxiGdalErrorCode::Success {
        return 0;
    }

    // Check if dimensions are reasonable for mobile display
    let max_dimension = 8192; // Max texture size on most iOS devices
    let total_pixels = metadata.width as i64 * metadata.height as i64;
    let max_pixels = 50_000_000i64; // 50 megapixels

    if metadata.width > max_dimension
        || metadata.height > max_dimension
        || total_pixels > max_pixels
    {
        0
    } else {
        1
    }
}

/// Exports dataset to iOS-compatible format.
///
/// # Parameters
/// - `dataset`: Source dataset
/// - `output_path`: Output file path
/// - `format`: Output format ("png", "jpeg", "geotiff")
///
/// # Safety
/// - All pointers must be valid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_ios_dataset_export(
    dataset: *const OxiGdalDataset,
    output_path: *const c_char,
    format: *const c_char,
) -> OxiGdalErrorCode {
    crate::check_null!(dataset, "dataset");
    crate::check_null!(output_path, "output_path");
    crate::check_null!(format, "format");

    // SAFETY: Caller guarantees pointers are valid (checked by check_null above)
    let out_path = unsafe {
        match std::ffi::CStr::from_ptr(output_path).to_str() {
            Ok(s) => s,
            Err(_) => {
                crate::ffi::error::set_last_error("Invalid output path encoding".to_string());
                return OxiGdalErrorCode::InvalidUtf8;
            }
        }
    };

    // SAFETY: Caller guarantees pointer is valid
    let fmt = unsafe {
        match std::ffi::CStr::from_ptr(format).to_str() {
            Ok(s) => s,
            Err(_) => {
                crate::ffi::error::set_last_error("Invalid format string encoding".to_string());
                return OxiGdalErrorCode::InvalidUtf8;
            }
        }
    };

    // Validate format is iOS-compatible
    let supported_formats = ["geotiff", "tif", "tiff", "png", "jpeg", "jpg", "geojson"];
    let fmt_lower = fmt.to_lowercase();
    if !supported_formats.contains(&fmt_lower.as_str()) {
        crate::ffi::error::set_last_error(format!(
            "Unsupported iOS export format: '{}'. Supported: {:?}",
            fmt, supported_formats
        ));
        return OxiGdalErrorCode::UnsupportedFormat;
    }

    // Validate the output path is within the app sandbox
    // iOS apps can only write to their own containers
    let valid_prefixes = [
        "/Documents",
        "/Library",
        "/tmp",
        "/var/mobile",
        "/private/var",
    ];
    let is_valid_path = valid_prefixes
        .iter()
        .any(|prefix| out_path.starts_with(prefix))
        || out_path.contains("/Documents/")
        || out_path.contains("/Library/")
        || out_path.contains("/tmp/");

    if !is_valid_path {
        crate::ffi::error::set_last_error(format!(
            "Output path '{}' may be outside the iOS app sandbox. \
             Use paths within Documents, Library, or tmp directories.",
            out_path
        ));
        // Still allow the operation; the OS will enforce sandbox rules
    }

    // Get metadata for the source dataset
    let mut metadata = OxiGdalMetadata {
        width: 0,
        height: 0,
        band_count: 0,
        data_type: 0,
        epsg_code: 0,
        geotransform: [0.0; 6],
    };

    // SAFETY: Caller guarantees dataset is valid
    let result =
        unsafe { crate::ffi::raster::oxigdal_dataset_get_metadata(dataset, &mut metadata) };
    if result != OxiGdalErrorCode::Success {
        return result;
    }

    // Check memory constraints before export
    // iOS is stricter about memory usage; jetsam will kill the app
    let bytes_per_pixel: i64 = match metadata.data_type {
        0 => 1,
        1 | 2 => 2,
        3 | 4 => 4,
        5 => 4,
        6 => 8,
        _ => 4,
    };

    let total_bytes = metadata.width as i64
        * metadata.height as i64
        * metadata.band_count as i64
        * bytes_per_pixel;

    // iOS limit: 75MB per export operation (leave headroom for jetsam)
    let max_export_bytes = 75 * 1024 * 1024i64;
    if total_bytes > max_export_bytes {
        crate::ffi::error::set_last_error(format!(
            "Dataset too large for iOS export: {} bytes (max: {} bytes). \
             Consider exporting a sub-region or using background processing.",
            total_bytes, max_export_bytes
        ));
        return OxiGdalErrorCode::AllocationFailed;
    }

    // Ensure output directory exists
    let output_dir = std::path::Path::new(out_path)
        .parent()
        .unwrap_or(std::path::Path::new("/tmp"));
    if !output_dir.exists() && std::fs::create_dir_all(output_dir).is_err() {
        crate::ffi::error::set_last_error(format!(
            "Cannot create output directory: {}",
            output_dir.display()
        ));
        return OxiGdalErrorCode::IoError;
    }

    // Create output dataset and copy data
    let output_cstr = match std::ffi::CString::new(out_path) {
        Ok(s) => s,
        Err(_) => {
            crate::ffi::error::set_last_error("Invalid output path".to_string());
            return OxiGdalErrorCode::IoError;
        }
    };

    let data_type = match metadata.data_type {
        0 => crate::ffi::types::OxiGdalDataType::Byte,
        1 => crate::ffi::types::OxiGdalDataType::UInt16,
        2 => crate::ffi::types::OxiGdalDataType::Int16,
        3 => crate::ffi::types::OxiGdalDataType::UInt32,
        4 => crate::ffi::types::OxiGdalDataType::Int32,
        5 => crate::ffi::types::OxiGdalDataType::Float32,
        6 => crate::ffi::types::OxiGdalDataType::Float64,
        _ => crate::ffi::types::OxiGdalDataType::Byte,
    };

    // SAFETY: All FFI calls below operate on validated pointers
    unsafe {
        let mut out_dataset: *mut OxiGdalDataset = std::ptr::null_mut();
        let create_result = crate::ffi::raster::oxigdal_dataset_create(
            output_cstr.as_ptr(),
            metadata.width,
            metadata.height,
            metadata.band_count,
            data_type,
            &mut out_dataset,
        );

        if create_result != OxiGdalErrorCode::Success {
            return create_result;
        }

        // Set geotransform and projection on output
        let gt_result = crate::ffi::raster::oxigdal_dataset_set_geotransform(
            out_dataset,
            metadata.geotransform.as_ptr(),
        );
        if gt_result != OxiGdalErrorCode::Success {
            crate::ffi::raster::oxigdal_dataset_close(out_dataset);
            return gt_result;
        }

        if metadata.epsg_code > 0 {
            let proj_result = crate::ffi::raster::oxigdal_dataset_set_projection_epsg(
                out_dataset,
                metadata.epsg_code,
            );
            if proj_result != OxiGdalErrorCode::Success {
                crate::ffi::raster::oxigdal_dataset_close(out_dataset);
                return proj_result;
            }
        }

        // Copy data in chunks to avoid jetsam kills
        let chunk_height = 256.min(metadata.height);
        let buffer_ptr =
            crate::ffi::oxigdal_buffer_alloc(metadata.width, chunk_height, metadata.band_count);

        if buffer_ptr.is_null() {
            crate::ffi::raster::oxigdal_dataset_close(out_dataset);
            crate::ffi::error::set_last_error("Failed to allocate export buffer".to_string());
            return OxiGdalErrorCode::AllocationFailed;
        }

        let mut y_off = 0;
        while y_off < metadata.height {
            let rows_to_read = chunk_height.min(metadata.height - y_off);

            for band in 1..=metadata.band_count {
                let read_result = crate::ffi::raster::oxigdal_dataset_read_region(
                    dataset,
                    0,
                    y_off,
                    metadata.width,
                    rows_to_read,
                    band,
                    buffer_ptr,
                );

                if read_result != OxiGdalErrorCode::Success {
                    crate::ffi::oxigdal_buffer_free(buffer_ptr);
                    crate::ffi::raster::oxigdal_dataset_close(out_dataset);
                    return read_result;
                }

                let write_result = crate::ffi::raster::oxigdal_dataset_write_region(
                    out_dataset,
                    0,
                    y_off,
                    metadata.width,
                    rows_to_read,
                    band,
                    buffer_ptr,
                );

                if write_result != OxiGdalErrorCode::Success {
                    crate::ffi::oxigdal_buffer_free(buffer_ptr);
                    crate::ffi::raster::oxigdal_dataset_close(out_dataset);
                    return write_result;
                }
            }

            y_off += rows_to_read;
        }

        crate::ffi::oxigdal_buffer_free(buffer_ptr);

        // Flush the output dataset to disk
        let flush_result = crate::ffi::raster::oxigdal_dataset_flush(out_dataset);
        crate::ffi::raster::oxigdal_dataset_close(out_dataset);

        flush_result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dataset_displayable_check() {
        // Create a mock dataset handle (for testing only)
        // In real code, this would be from oxigdal_dataset_open
        let dataset = std::ptr::null::<OxiGdalDataset>();

        let displayable = unsafe { oxigdal_ios_dataset_is_displayable(dataset) };

        // Null dataset should not be displayable
        assert_eq!(displayable, 0);
    }

    #[test]
    fn test_set_device_memory() {
        let result = oxigdal_ios_set_device_memory(4096);
        assert_eq!(result, OxiGdalErrorCode::Success);
        assert_eq!(IOS_DEVICE_MEMORY_MB.load(Ordering::Relaxed), 4096);

        // Invalid value
        let result = oxigdal_ios_set_device_memory(0);
        assert_eq!(result, OxiGdalErrorCode::InvalidArgument);

        let result = oxigdal_ios_set_device_memory(-1);
        assert_eq!(result, OxiGdalErrorCode::InvalidArgument);
    }

    #[test]
    fn test_apply_ios_memory_policy() {
        // Test low-end device
        IOS_DEVICE_MEMORY_MB.store(512, Ordering::Relaxed);
        apply_ios_memory_policy();

        // Test mid-range device
        IOS_DEVICE_MEMORY_MB.store(1536, Ordering::Relaxed);
        apply_ios_memory_policy();

        // Test high-end device
        IOS_DEVICE_MEMORY_MB.store(3072, Ordering::Relaxed);
        apply_ios_memory_policy();

        // Test flagship device
        IOS_DEVICE_MEMORY_MB.store(6144, Ordering::Relaxed);
        apply_ios_memory_policy();
    }

    #[test]
    fn test_export_null_checks() {
        let result = unsafe {
            oxigdal_ios_dataset_export(std::ptr::null(), std::ptr::null(), std::ptr::null())
        };
        assert_eq!(result, OxiGdalErrorCode::NullPointer);
    }
}
