//! Android-specific dataset operations.
//!
//! Provides Android-optimized dataset handling with content provider integration
//! and Android file system conventions.

#![cfg(feature = "android")]

use crate::ffi::types::*;
use std::os::raw::{c_char, c_int};
use std::sync::atomic::{AtomicI32, Ordering};

/// Android memory class level (MB of available heap per app)
/// Defaults to 128MB which is a conservative estimate for most devices
static ANDROID_MEMORY_CLASS_MB: AtomicI32 = AtomicI32::new(128);

/// Sets the Android device memory class (in MB).
///
/// This should be called during initialization with the value from
/// `ActivityManager.getMemoryClass()`.
///
/// # Parameters
/// - `memory_class_mb`: Memory class in megabytes
#[unsafe(no_mangle)]
pub extern "C" fn oxigdal_android_set_memory_class(memory_class_mb: c_int) -> OxiGdalErrorCode {
    if memory_class_mb <= 0 {
        crate::ffi::error::set_last_error("Invalid memory class value".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }
    ANDROID_MEMORY_CLASS_MB.store(memory_class_mb, Ordering::Relaxed);
    apply_android_memory_policy();
    OxiGdalErrorCode::Success
}

/// Applies Android-specific memory and cache policies based on device memory class.
fn apply_android_memory_policy() {
    let memory_class = ANDROID_MEMORY_CLASS_MB.load(Ordering::Relaxed);

    // Allocate cache as a fraction of available memory class
    // Low-end: <64MB class => 10MB cache
    // Mid-range: 64-192MB class => 25MB cache
    // High-end: 192-384MB class => 50MB cache
    // Flagship: >384MB class => 100MB cache
    let cache_size_mb = if memory_class < 64 {
        10
    } else if memory_class < 192 {
        25
    } else if memory_class < 384 {
        50
    } else {
        100
    };

    // Initialize or resize the cache
    if let Err(e) = crate::common::cache::set_max_cache_size_mb(cache_size_mb as usize) {
        crate::ffi::error::set_last_error(format!("Failed to set Android cache policy: {}", e));
    }
}

/// Opens a dataset with Android-specific optimizations.
///
/// Applies Android-specific settings like Dalvik memory limits
/// and battery optimization.
///
/// # Safety
/// - path must be valid null-terminated string
/// - out_dataset must be valid pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_android_dataset_open(
    path: *const c_char,
    out_dataset: *mut *mut OxiGdalDataset,
) -> OxiGdalErrorCode {
    let result = unsafe { crate::ffi::raster::oxigdal_dataset_open(path, out_dataset) };

    if result == OxiGdalErrorCode::Success {
        // Apply Android-specific optimizations
        // Android devices typically have limited memory (1-8GB RAM)
        // We use conservative defaults suitable for most Android devices
        apply_android_memory_policy();
    }

    result
}

/// Opens a dataset from Android assets.
///
/// # Parameters
/// - `asset_path`: Path within assets directory
/// - `out_dataset`: Output dataset handle
///
/// # Safety
/// - asset_path must be valid string
/// - out_dataset must be valid pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_android_dataset_open_asset(
    asset_path: *const c_char,
    out_dataset: *mut *mut OxiGdalDataset,
) -> OxiGdalErrorCode {
    crate::check_null!(asset_path, "asset_path");
    crate::check_null!(out_dataset, "out_dataset");

    let path = match unsafe { std::ffi::CStr::from_ptr(asset_path) }.to_str() {
        Ok(s) => s,
        Err(_) => {
            crate::ffi::error::set_last_error("Invalid asset path".to_string());
            return OxiGdalErrorCode::InvalidUtf8;
        }
    };

    // Construct full asset path
    // In real implementation, would use Android AssetManager
    let full_path = format!("/android_asset/{}", path);
    let path_cstr = match std::ffi::CString::new(full_path) {
        Ok(s) => s,
        Err(_) => {
            crate::ffi::error::set_last_error("Failed to create path".to_string());
            return OxiGdalErrorCode::IoError;
        }
    };

    unsafe { oxigdal_android_dataset_open(path_cstr.as_ptr(), out_dataset) }
}

/// Opens dataset from Android content URI.
///
/// Supports content:// URIs from DocumentProvider, MediaStore, etc.
///
/// # Safety
/// - uri must be valid null-terminated string
/// - out_dataset must be valid pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_android_dataset_open_content_uri(
    uri: *const c_char,
    out_dataset: *mut *mut OxiGdalDataset,
) -> OxiGdalErrorCode {
    crate::check_null!(uri, "uri");
    crate::check_null!(out_dataset, "out_dataset");

    let uri_str = match unsafe { std::ffi::CStr::from_ptr(uri) }.to_str() {
        Ok(s) => s,
        Err(_) => {
            crate::ffi::error::set_last_error("Invalid URI string".to_string());
            return OxiGdalErrorCode::InvalidUtf8;
        }
    };

    // Parse content:// URI scheme
    // Android content URIs follow the format: content://authority/path/id
    // Common authorities:
    //   - com.android.providers.media.documents (MediaStore)
    //   - com.android.externalstorage.documents (External storage)
    //   - com.android.providers.downloads.documents (Downloads)
    if !uri_str.starts_with("content://") {
        crate::ffi::error::set_last_error(format!(
            "Not a content URI: {}. Expected content:// scheme",
            uri_str
        ));
        return OxiGdalErrorCode::InvalidArgument;
    }

    // Extract the authority and path from the content URI
    let uri_body = &uri_str["content://".len()..];

    // Try to resolve to a file path based on known content providers
    let resolved_path = resolve_content_uri(uri_body);

    match resolved_path {
        Some(path) => {
            let path_cstr = match std::ffi::CString::new(path) {
                Ok(s) => s,
                Err(_) => {
                    crate::ffi::error::set_last_error(
                        "Failed to create resolved path string".to_string(),
                    );
                    return OxiGdalErrorCode::IoError;
                }
            };
            unsafe { oxigdal_android_dataset_open(path_cstr.as_ptr(), out_dataset) }
        }
        None => {
            crate::ffi::error::set_last_error(format!(
                "Could not resolve content URI: {}. \
                 The content provider may require JNI-based resolution via ContentResolver",
                uri_str
            ));
            OxiGdalErrorCode::FileNotFound
        }
    }
}

/// Attempts to resolve an Android content URI body to a file system path.
///
/// This handles common content provider patterns for external storage and
/// media documents. For more complex providers (e.g., Google Drive, custom
/// providers), JNI-based ContentResolver access is needed from the Java/Kotlin
/// layer.
fn resolve_content_uri(uri_body: &str) -> Option<String> {
    // Split authority from path
    let parts: Vec<&str> = uri_body.splitn(2, '/').collect();
    if parts.len() < 2 {
        return None;
    }

    let authority = parts[0];
    let path = parts[1];

    match authority {
        // External storage documents provider
        // URI format: content://com.android.externalstorage.documents/document/primary:path/to/file
        "com.android.externalstorage.documents" => {
            // Extract document ID which is typically "primary:relative/path"
            let doc_path = if let Some(stripped) = path.strip_prefix("document/") {
                stripped
            } else {
                path
            };

            // URL decode the path
            let decoded = url_decode(doc_path);

            // Handle "primary:" prefix (internal storage)
            if let Some(relative) = decoded.strip_prefix("primary:") {
                Some(format!("/storage/emulated/0/{}", relative))
            } else {
                // Could be a secondary storage volume like "1234-5678:path"
                // Try to map volume ID to mount point
                let colon_pos = decoded.find(':');
                if let Some(pos) = colon_pos {
                    let volume_id = &decoded[..pos];
                    let relative_path = &decoded[pos + 1..];
                    Some(format!("/storage/{}/{}", volume_id, relative_path))
                } else {
                    None
                }
            }
        }

        // Media documents provider
        // URI format: content://com.android.providers.media.documents/document/image:123
        "com.android.providers.media.documents" => {
            // Media documents are accessed by type:id
            // Without JNI, we cannot resolve these directly
            // Return None to indicate JNI resolution is needed
            let _ = path;
            None
        }

        // Downloads provider
        // URI format: content://com.android.providers.downloads.documents/document/123
        "com.android.providers.downloads.documents" => {
            // Downloads can sometimes be resolved to the Download directory
            let doc_id = if let Some(stripped) = path.strip_prefix("document/") {
                stripped
            } else {
                path
            };

            // Raw file URIs in the downloads directory
            doc_id.strip_prefix("raw:").map(url_decode)
        }

        // File-based content URI (some apps use file:// like paths)
        _ => {
            // For unknown authorities, check if the path looks like a direct file path
            if path.starts_with('/') {
                Some(path.to_string())
            } else {
                None
            }
        }
    }
}

/// Simple URL decoding for content URI paths.
///
/// Handles %XX hex-encoded characters commonly found in Android content URIs.
fn url_decode(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars();

    while let Some(ch) = chars.next() {
        if ch == '%' {
            let hex_high = chars.next();
            let hex_low = chars.next();
            if let (Some(h), Some(l)) = (hex_high, hex_low) {
                let hex_str: String = [h, l].iter().collect();
                if let Ok(byte_val) = u8::from_str_radix(&hex_str, 16) {
                    output.push(byte_val as char);
                } else {
                    // Invalid hex escape, pass through literally
                    output.push('%');
                    output.push(h);
                    output.push(l);
                }
            } else {
                // Incomplete escape, pass through
                output.push('%');
                if let Some(h) = hex_high {
                    output.push(h);
                }
            }
        } else {
            output.push(ch);
        }
    }
    output
}

/// Checks if dataset fits in Android device memory constraints.
///
/// # Returns
/// - 1 if loadable
/// - 0 if not loadable or on error
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_android_dataset_check_memory(
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

    let result =
        unsafe { crate::ffi::raster::oxigdal_dataset_get_metadata(dataset, &mut metadata) };
    if result != OxiGdalErrorCode::Success {
        return 0;
    }

    // Conservative estimate: check if uncompressed size fits in memory
    let bytes_per_pixel = match metadata.data_type {
        0 => 1,     // Byte
        1 | 2 => 2, // UInt16, Int16
        3 | 4 => 4, // UInt32, Int32
        5 => 4,     // Float32
        6 => 8,     // Float64
        _ => 4,     // Default
    };

    let total_bytes = metadata.width as i64
        * metadata.height as i64
        * metadata.band_count as i64
        * bytes_per_pixel;

    // Android devices typically have 100MB-2GB app memory
    // Be conservative and limit to 50MB for a single dataset
    let max_bytes = 50 * 1024 * 1024i64;

    if total_bytes > max_bytes { 0 } else { 1 }
}

/// Exports dataset to Android-compatible format.
///
/// # Parameters
/// - `dataset`: Source dataset
/// - `output_path`: Output file path
/// - `format`: Output format
///
/// # Safety
/// - All pointers must be valid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_android_dataset_export(
    dataset: *const OxiGdalDataset,
    output_path: *const c_char,
    format: *const c_char,
) -> OxiGdalErrorCode {
    crate::check_null!(dataset, "dataset");
    crate::check_null!(output_path, "output_path");
    crate::check_null!(format, "format");

    let out_path = match unsafe { std::ffi::CStr::from_ptr(output_path) }.to_str() {
        Ok(s) => s,
        Err(_) => {
            crate::ffi::error::set_last_error("Invalid output path encoding".to_string());
            return OxiGdalErrorCode::InvalidUtf8;
        }
    };

    let fmt = match unsafe { std::ffi::CStr::from_ptr(format) }.to_str() {
        Ok(s) => s,
        Err(_) => {
            crate::ffi::error::set_last_error("Invalid format string encoding".to_string());
            return OxiGdalErrorCode::InvalidUtf8;
        }
    };

    // Validate format is Android-compatible
    let supported_formats = ["geotiff", "tif", "tiff", "png", "jpeg", "jpg", "geojson"];
    let fmt_lower = fmt.to_lowercase();
    if !supported_formats.contains(&fmt_lower.as_str()) {
        crate::ffi::error::set_last_error(format!(
            "Unsupported Android export format: '{}'. Supported: {:?}",
            fmt, supported_formats
        ));
        return OxiGdalErrorCode::UnsupportedFormat;
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

    let result =
        unsafe { crate::ffi::raster::oxigdal_dataset_get_metadata(dataset, &mut metadata) };
    if result != OxiGdalErrorCode::Success {
        return result;
    }

    // Check memory constraints before export
    let bytes_per_pixel = match metadata.data_type {
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

    // Conservative Android limit: don't export if dataset > 100MB uncompressed
    let max_export_bytes = 100 * 1024 * 1024i64;
    if total_bytes > max_export_bytes {
        crate::ffi::error::set_last_error(format!(
            "Dataset too large for Android export: {} bytes (max: {} bytes). \
             Consider exporting a sub-region instead.",
            total_bytes, max_export_bytes
        ));
        return OxiGdalErrorCode::AllocationFailed;
    }

    // Validate output directory exists (or can be created)
    let output_dir = std::path::Path::new(out_path)
        .parent()
        .unwrap_or(std::path::Path::new("/"));
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

    let mut out_dataset: *mut OxiGdalDataset = std::ptr::null_mut();
    let create_result = unsafe {
        crate::ffi::raster::oxigdal_dataset_create(
            output_cstr.as_ptr(),
            metadata.width,
            metadata.height,
            metadata.band_count,
            data_type,
            &mut out_dataset,
        )
    };

    if create_result != OxiGdalErrorCode::Success {
        return create_result;
    }

    // Set geotransform and projection on output
    let gt_result = unsafe {
        crate::ffi::raster::oxigdal_dataset_set_geotransform(
            out_dataset,
            metadata.geotransform.as_ptr(),
        )
    };
    if gt_result != OxiGdalErrorCode::Success {
        unsafe { crate::ffi::raster::oxigdal_dataset_close(out_dataset) };
        return gt_result;
    }

    if metadata.epsg_code > 0 {
        let proj_result = unsafe {
            crate::ffi::raster::oxigdal_dataset_set_projection_epsg(out_dataset, metadata.epsg_code)
        };
        if proj_result != OxiGdalErrorCode::Success {
            unsafe { crate::ffi::raster::oxigdal_dataset_close(out_dataset) };
            return proj_result;
        }
    }

    // Copy raster data band by band in scanline chunks to limit memory usage
    let chunk_height = 256.min(metadata.height);
    let chunk_size = (metadata.width * chunk_height * metadata.band_count) as usize;
    let buffer_ptr = unsafe {
        crate::ffi::oxigdal_buffer_alloc(metadata.width, chunk_height, metadata.band_count)
    };

    if buffer_ptr.is_null() {
        unsafe { crate::ffi::raster::oxigdal_dataset_close(out_dataset) };
        crate::ffi::error::set_last_error("Failed to allocate export buffer".to_string());
        return OxiGdalErrorCode::AllocationFailed;
    }

    let mut y_off = 0;
    while y_off < metadata.height {
        let rows_to_read = chunk_height.min(metadata.height - y_off);

        for band in 1..=metadata.band_count {
            let read_result = unsafe {
                crate::ffi::raster::oxigdal_dataset_read_region(
                    dataset,
                    0,
                    y_off,
                    metadata.width,
                    rows_to_read,
                    band,
                    buffer_ptr,
                )
            };

            if read_result != OxiGdalErrorCode::Success {
                unsafe { crate::ffi::oxigdal_buffer_free(buffer_ptr) };
                unsafe { crate::ffi::raster::oxigdal_dataset_close(out_dataset) };
                return read_result;
            }

            let write_result = unsafe {
                crate::ffi::raster::oxigdal_dataset_write_region(
                    out_dataset,
                    0,
                    y_off,
                    metadata.width,
                    rows_to_read,
                    band,
                    buffer_ptr,
                )
            };

            if write_result != OxiGdalErrorCode::Success {
                unsafe { crate::ffi::oxigdal_buffer_free(buffer_ptr) };
                unsafe { crate::ffi::raster::oxigdal_dataset_close(out_dataset) };
                return write_result;
            }
        }

        y_off += rows_to_read;
    }

    unsafe { crate::ffi::oxigdal_buffer_free(buffer_ptr) };

    // Flush the output dataset to disk
    let flush_result = unsafe { crate::ffi::raster::oxigdal_dataset_flush(out_dataset) };
    unsafe { crate::ffi::raster::oxigdal_dataset_close(out_dataset) };

    flush_result
}

/// Result of share preparation containing the path and MIME type.
///
/// Shares dataset via Android share intent.
///
/// # Safety
/// - dataset must be valid
/// - title must be valid string
#[repr(C)]
pub struct AndroidShareInfo {
    /// Path to the shared file (caller must free with oxigdal_string_free)
    pub file_path: *mut c_char,
    /// MIME type string (caller must free with oxigdal_string_free)
    pub mime_type: *mut c_char,
    /// Title string for the share dialog (caller must free with oxigdal_string_free)
    pub title: *mut c_char,
}

/// Android FFI function to share a dataset.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_android_dataset_share(
    dataset: *const OxiGdalDataset,
    title: *const c_char,
) -> OxiGdalErrorCode {
    crate::check_null!(dataset, "dataset");
    crate::check_null!(title, "title");

    let title_str = match unsafe { std::ffi::CStr::from_ptr(title) }.to_str() {
        Ok(s) => s,
        Err(_) => {
            crate::ffi::error::set_last_error("Invalid title encoding".to_string());
            return OxiGdalErrorCode::InvalidUtf8;
        }
    };

    // Get dataset metadata to determine appropriate share format
    let mut metadata = OxiGdalMetadata {
        width: 0,
        height: 0,
        band_count: 0,
        data_type: 0,
        epsg_code: 0,
        geotransform: [0.0; 6],
    };

    let result =
        unsafe { crate::ffi::raster::oxigdal_dataset_get_metadata(dataset, &mut metadata) };
    if result != OxiGdalErrorCode::Success {
        return result;
    }

    // Prepare a shareable copy in the Android cache directory
    // The Java/Kotlin layer should use FileProvider to create a content:// URI
    // from this path for actual sharing
    let cache_dir = "/data/data/cache/oxigdal_share";
    if std::fs::create_dir_all(cache_dir).is_err() {
        crate::ffi::error::set_last_error(format!(
            "Failed to create share cache directory: {}",
            cache_dir
        ));
        return OxiGdalErrorCode::IoError;
    }

    // Sanitize title for use as filename
    let safe_title: String = title_str
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();

    // Export as GeoTIFF for sharing (most compatible geospatial format)
    let share_path = format!("{}/{}.tif", cache_dir, safe_title);
    let share_path_cstr = match std::ffi::CString::new(share_path.as_str()) {
        Ok(s) => s,
        Err(_) => {
            crate::ffi::error::set_last_error("Failed to create share path".to_string());
            return OxiGdalErrorCode::IoError;
        }
    };

    let format_cstr = match std::ffi::CString::new("geotiff") {
        Ok(s) => s,
        Err(_) => {
            crate::ffi::error::set_last_error("Failed to create format string".to_string());
            return OxiGdalErrorCode::IoError;
        }
    };

    // Use our export function to create the shareable file
    let export_result = unsafe {
        oxigdal_android_dataset_export(dataset, share_path_cstr.as_ptr(), format_cstr.as_ptr())
    };

    if export_result != OxiGdalErrorCode::Success {
        crate::ffi::error::set_last_error(format!(
            "Failed to export dataset for sharing. \
             The Java/Kotlin layer should use Intent.ACTION_SEND with \
             FileProvider.getUriForFile() on the exported file at: {}",
            share_path
        ));
        return export_result;
    }

    // Store share info in last error for retrieval by the Java/Kotlin layer
    // In practice, the JNI wrapper would read this info and construct the share Intent
    crate::ffi::error::set_last_error(format!(
        "SHARE_READY:path={};mime=image/tiff;title={}",
        share_path, title_str
    ));

    OxiGdalErrorCode::Success
}

/// Gets the path where share data was prepared.
///
/// Should be called after a successful `oxigdal_android_dataset_share` to get
/// the file path for constructing a share intent.
///
/// # Returns
/// Path string (caller must free with oxigdal_string_free), or null on failure
#[unsafe(no_mangle)]
pub extern "C" fn oxigdal_android_get_share_path() -> *mut c_char {
    let last_error = crate::ffi::error::oxigdal_get_last_error();
    if last_error.is_null() {
        return std::ptr::null_mut();
    }

    // Parse share info from the last error message
    let error_str = unsafe {
        let cstr = std::ffi::CStr::from_ptr(last_error);
        let s = match cstr.to_str() {
            Ok(s) => s.to_string(),
            Err(_) => {
                crate::ffi::error::oxigdal_string_free(last_error);
                return std::ptr::null_mut();
            }
        };
        crate::ffi::error::oxigdal_string_free(last_error);
        s
    };

    if !error_str.starts_with("SHARE_READY:") {
        return std::ptr::null_mut();
    }

    // Extract path from "SHARE_READY:path=/some/path;mime=...;title=..."
    let info = &error_str["SHARE_READY:".len()..];
    for part in info.split(';') {
        if let Some(path) = part.strip_prefix("path=") {
            return match std::ffi::CString::new(path) {
                Ok(s) => s.into_raw(),
                Err(_) => std::ptr::null_mut(),
            };
        }
    }

    std::ptr::null_mut()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_check() {
        let dataset = std::ptr::null::<OxiGdalDataset>();
        let can_load = unsafe { oxigdal_android_dataset_check_memory(dataset) };
        assert_eq!(can_load, 0); // Null dataset should fail
    }

    #[test]
    fn test_set_memory_class() {
        let result = oxigdal_android_set_memory_class(256);
        assert_eq!(result, OxiGdalErrorCode::Success);
        assert_eq!(ANDROID_MEMORY_CLASS_MB.load(Ordering::Relaxed), 256);

        // Invalid value
        let result = oxigdal_android_set_memory_class(0);
        assert_eq!(result, OxiGdalErrorCode::InvalidArgument);

        let result = oxigdal_android_set_memory_class(-1);
        assert_eq!(result, OxiGdalErrorCode::InvalidArgument);
    }

    #[test]
    fn test_apply_android_memory_policy() {
        // Test low-end device
        ANDROID_MEMORY_CLASS_MB.store(32, Ordering::Relaxed);
        apply_android_memory_policy();

        // Test mid-range device
        ANDROID_MEMORY_CLASS_MB.store(128, Ordering::Relaxed);
        apply_android_memory_policy();

        // Test high-end device
        ANDROID_MEMORY_CLASS_MB.store(256, Ordering::Relaxed);
        apply_android_memory_policy();

        // Test flagship device
        ANDROID_MEMORY_CLASS_MB.store(512, Ordering::Relaxed);
        apply_android_memory_policy();
    }

    #[test]
    fn test_content_uri_resolution_external_storage() {
        // Test primary storage resolution
        let resolved = resolve_content_uri(
            "com.android.externalstorage.documents/document/primary:Documents/test.tif",
        );
        assert_eq!(
            resolved,
            Some("/storage/emulated/0/Documents/test.tif".to_string())
        );

        // Test secondary storage resolution
        let resolved = resolve_content_uri(
            "com.android.externalstorage.documents/document/1234-5678:Maps/data.tif",
        );
        assert_eq!(
            resolved,
            Some("/storage/1234-5678/Maps/data.tif".to_string())
        );
    }

    #[test]
    fn test_content_uri_resolution_downloads() {
        // Test raw download path
        let resolved = resolve_content_uri(
            "com.android.providers.downloads.documents/document/raw:/storage/emulated/0/Download/file.tif",
        );
        assert_eq!(
            resolved,
            Some("/storage/emulated/0/Download/file.tif".to_string())
        );

        // Test numeric download ID (cannot resolve without JNI)
        let resolved =
            resolve_content_uri("com.android.providers.downloads.documents/document/12345");
        assert!(resolved.is_none());
    }

    #[test]
    fn test_content_uri_resolution_media() {
        // Media URIs cannot be resolved without JNI
        let resolved =
            resolve_content_uri("com.android.providers.media.documents/document/image:12345");
        assert!(resolved.is_none());
    }

    #[test]
    fn test_content_uri_invalid() {
        // Test with null pointer
        let result = unsafe {
            oxigdal_android_dataset_open_content_uri(std::ptr::null(), std::ptr::null_mut())
        };
        assert_eq!(result, OxiGdalErrorCode::NullPointer);

        // Test non-content URI
        let file_uri = std::ffi::CString::new("file:///some/path").expect("valid cstring");
        let mut dataset: *mut OxiGdalDataset = std::ptr::null_mut();
        let result =
            unsafe { oxigdal_android_dataset_open_content_uri(file_uri.as_ptr(), &mut dataset) };
        assert_eq!(result, OxiGdalErrorCode::InvalidArgument);
    }

    #[test]
    fn test_url_decode() {
        assert_eq!(url_decode("hello%20world"), "hello world");
        assert_eq!(url_decode("path%2Fto%2Ffile"), "path/to/file");
        assert_eq!(url_decode("no_escapes"), "no_escapes");
        assert_eq!(url_decode("end%20"), "end ");
        assert_eq!(url_decode("%41%42%43"), "ABC");
    }

    #[test]
    fn test_export_null_checks() {
        let result = unsafe {
            oxigdal_android_dataset_export(std::ptr::null(), std::ptr::null(), std::ptr::null())
        };
        assert_eq!(result, OxiGdalErrorCode::NullPointer);
    }

    #[test]
    fn test_share_null_checks() {
        let result = unsafe { oxigdal_android_dataset_share(std::ptr::null(), std::ptr::null()) };
        assert_eq!(result, OxiGdalErrorCode::NullPointer);
    }
}
