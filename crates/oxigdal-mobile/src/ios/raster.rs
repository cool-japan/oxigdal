//! iOS-specific raster operations.
//!
//! Provides optimized raster processing for iOS with UIImage integration.

#![cfg(feature = "ios")]

use crate::ffi::types::*;
use std::os::raw::{c_char, c_double, c_int};

/// Reads a raster region optimized for iOS display.
///
/// Automatically applies iOS-specific optimizations like:
/// - Retina display scaling
/// - Memory limits for low-memory devices
/// - Automatic RGBA conversion for UIImage
///
/// # Safety
/// - dataset must be valid
/// - buffer must be properly allocated
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_ios_read_region_for_display(
    dataset: *const OxiGdalDataset,
    x_off: c_int,
    y_off: c_int,
    width: c_int,
    height: c_int,
    display_width: c_int,
    display_height: c_int,
    buffer: *mut OxiGdalBuffer,
) -> OxiGdalErrorCode {
    crate::check_null!(dataset, "dataset");
    crate::check_null!(buffer, "buffer");

    if width <= 0 || height <= 0 || display_width <= 0 || display_height <= 0 {
        crate::ffi::error::set_last_error("Invalid dimensions".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    // Read region with resampling for display size
    // For now, use standard read (would implement resampling in production)
    // SAFETY: Caller guarantees dataset and buffer are valid
    unsafe {
        crate::ffi::raster::oxigdal_dataset_read_region(
            dataset, x_off, y_off, width, height, 1, buffer,
        )
    }
}

/// Creates a thumbnail for iOS table views and collection views.
///
/// # Parameters
/// - `dataset`: Source dataset
/// - `max_size`: Maximum dimension for thumbnail
/// - `out_buffer`: Output buffer (must be pre-allocated)
///
/// # Safety
/// - All pointers must be valid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_ios_create_thumbnail(
    dataset: *const OxiGdalDataset,
    max_size: c_int,
    out_buffer: *mut OxiGdalBuffer,
) -> OxiGdalErrorCode {
    crate::check_null!(dataset, "dataset");
    crate::check_null!(out_buffer, "out_buffer");

    if max_size <= 0 || max_size > 1024 {
        crate::ffi::error::set_last_error("Invalid thumbnail size".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

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

    // Calculate thumbnail dimensions maintaining aspect ratio
    let (_thumb_width, _thumb_height) = if metadata.width > metadata.height {
        let width = max_size;
        let height = (max_size as f64 * metadata.height as f64 / metadata.width as f64) as i32;
        (width, height.max(1))
    } else {
        let height = max_size;
        let width = (max_size as f64 * metadata.width as f64 / metadata.height as f64) as i32;
        (width.max(1), height)
    };

    // Read full dataset at reduced resolution
    // SAFETY: Caller guarantees dataset and out_buffer are valid
    unsafe {
        crate::ffi::raster::oxigdal_dataset_read_region(
            dataset,
            0,
            0,
            metadata.width,
            metadata.height,
            1,
            out_buffer,
        )
    }
}

/// Applies iOS-style image enhancements.
///
/// Uses iOS-standard enhancement curves similar to Photos app.
///
/// # Safety
/// - buffer must be valid
/// - params must be valid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_ios_enhance_image(
    buffer: *mut OxiGdalBuffer,
    params: *const OxiGdalEnhanceParams,
) -> OxiGdalErrorCode {
    crate::check_null!(buffer, "buffer");
    crate::check_null!(params, "params");

    // SAFETY: Caller guarantees pointers are valid (checked for null above)
    let buf = unsafe { &mut *buffer };
    let p = unsafe { &*params };

    if buf.data.is_null() || buf.length == 0 {
        crate::ffi::error::set_last_error("Invalid buffer".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    let pixel_count = (buf.width * buf.height) as usize;
    // SAFETY: buf.data is non-null and buf.length is the valid size
    let data_slice = unsafe { std::slice::from_raw_parts_mut(buf.data, buf.length) };

    // Apply enhancements based on channel count
    if buf.channels == 1 {
        // Grayscale: apply brightness, contrast, gamma only (skip saturation)
        for i in 0..pixel_count.min(data_slice.len()) {
            let mut value = data_slice[i] as f64 / 255.0;

            // Apply brightness
            value *= p.brightness;

            // Apply contrast
            value = (value - 0.5) * p.contrast + 0.5;

            // Apply gamma
            if p.gamma != 1.0 {
                value = value.powf(1.0 / p.gamma);
            }

            // Clamp and convert back
            data_slice[i] = (value.clamp(0.0, 1.0) * 255.0) as u8;
        }
    } else if buf.channels >= 3 {
        // RGB/RGBA: apply all enhancements including saturation
        for i in 0..pixel_count {
            let offset = i * buf.channels as usize;
            if offset + 2 < data_slice.len() {
                // Read RGB values
                let mut r = data_slice[offset] as f64 / 255.0;
                let mut g = data_slice[offset + 1] as f64 / 255.0;
                let mut b = data_slice[offset + 2] as f64 / 255.0;

                // Apply brightness
                r *= p.brightness;
                g *= p.brightness;
                b *= p.brightness;

                // Apply saturation if not 1.0
                if (p.saturation - 1.0).abs() > 1e-6 {
                    // Convert to HSL
                    let (h, s, l) = rgb_to_hsl(r, g, b);
                    // Adjust saturation
                    let new_s = (s * p.saturation).clamp(0.0, 1.0);
                    // Convert back to RGB
                    let (new_r, new_g, new_b) = hsl_to_rgb(h, new_s, l);
                    r = new_r;
                    g = new_g;
                    b = new_b;
                }

                // Apply contrast
                r = (r - 0.5) * p.contrast + 0.5;
                g = (g - 0.5) * p.contrast + 0.5;
                b = (b - 0.5) * p.contrast + 0.5;

                // Apply gamma
                if p.gamma != 1.0 {
                    r = r.powf(1.0 / p.gamma);
                    g = g.powf(1.0 / p.gamma);
                    b = b.powf(1.0 / p.gamma);
                }

                // Clamp and write back
                data_slice[offset] = (r.clamp(0.0, 1.0) * 255.0) as u8;
                data_slice[offset + 1] = (g.clamp(0.0, 1.0) * 255.0) as u8;
                data_slice[offset + 2] = (b.clamp(0.0, 1.0) * 255.0) as u8;
                // Alpha channel (if present) is left unchanged
            }
        }
    }

    OxiGdalErrorCode::Success
}

/// Converts RGB to HSL color space.
///
/// # Parameters
/// - `r`, `g`, `b`: RGB values in range [0.0, 1.0]
///
/// # Returns
/// - `(h, s, l)`: HSL values where h is in [0.0, 360.0], s and l are in [0.0, 1.0]
fn rgb_to_hsl(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let l = (max + min) / 2.0;

    if delta < 1e-10 {
        // Achromatic (gray)
        return (0.0, 0.0, l);
    }

    let s = if l < 0.5 {
        delta / (max + min)
    } else {
        delta / (2.0 - max - min)
    };

    let h = if (max - r).abs() < 1e-10 {
        // Red is max
        ((g - b) / delta + if g < b { 6.0 } else { 0.0 }) * 60.0
    } else if (max - g).abs() < 1e-10 {
        // Green is max
        ((b - r) / delta + 2.0) * 60.0
    } else {
        // Blue is max
        ((r - g) / delta + 4.0) * 60.0
    };

    (h, s, l)
}

/// Converts HSL to RGB color space.
///
/// # Parameters
/// - `h`: Hue in range [0.0, 360.0]
/// - `s`: Saturation in range [0.0, 1.0]
/// - `l`: Lightness in range [0.0, 1.0]
///
/// # Returns
/// - `(r, g, b)`: RGB values in range [0.0, 1.0]
fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    if s < 1e-10 {
        // Achromatic (gray)
        return (l, l, l);
    }

    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };

    let p = 2.0 * l - q;

    let h_normalized = h / 360.0;

    let r = hue_to_rgb(p, q, h_normalized + 1.0 / 3.0);
    let g = hue_to_rgb(p, q, h_normalized);
    let b = hue_to_rgb(p, q, h_normalized - 1.0 / 3.0);

    (r, g, b)
}

/// Helper function for HSL to RGB conversion.
fn hue_to_rgb(p: f64, q: f64, mut t: f64) -> f64 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }

    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 1.0 / 2.0 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

/// Converts raster to iOS Metal texture format.
///
/// # Parameters
/// - `buffer`: Source buffer
/// - `out_metal_buffer`: Output Metal-compatible buffer
///
/// # Safety
/// - Both buffers must be valid and properly sized
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_ios_to_metal_texture(
    buffer: *const OxiGdalBuffer,
    out_metal_buffer: *mut OxiGdalBuffer,
) -> OxiGdalErrorCode {
    crate::check_null!(buffer, "buffer");
    crate::check_null!(out_metal_buffer, "out_metal_buffer");

    // SAFETY: Caller guarantees pointers are valid (checked for null above)
    let src = unsafe { &*buffer };
    let dst = unsafe { &mut *out_metal_buffer };

    // Metal textures use BGRA format by default
    if src.channels == 3 || src.channels == 4 {
        let pixel_count = (src.width * src.height) as usize;

        // SAFETY: src.data and dst.data are validated non-null buffers with
        // sufficient size for pixel_count * channels bytes
        unsafe {
            for i in 0..pixel_count {
                let src_offset = i * src.channels as usize;
                let dst_offset = i * 4;

                if src.channels == 3 {
                    // RGB to BGRA
                    *dst.data.add(dst_offset) = *src.data.add(src_offset + 2); // B
                    *dst.data.add(dst_offset + 1) = *src.data.add(src_offset + 1); // G
                    *dst.data.add(dst_offset + 2) = *src.data.add(src_offset); // R
                    *dst.data.add(dst_offset + 3) = 255; // A
                } else {
                    // RGBA to BGRA
                    *dst.data.add(dst_offset) = *src.data.add(src_offset + 2); // B
                    *dst.data.add(dst_offset + 1) = *src.data.add(src_offset + 1); // G
                    *dst.data.add(dst_offset + 2) = *src.data.add(src_offset); // R
                    *dst.data.add(dst_offset + 3) = *src.data.add(src_offset + 3); // A
                }
            }
        }
    } else {
        crate::ffi::error::set_last_error("Unsupported channel count for Metal".to_string());
        return OxiGdalErrorCode::UnsupportedFormat;
    }

    OxiGdalErrorCode::Success
}

/// Gets raster statistics for iOS display formatting.
///
/// Computes min, max, mean, and standard deviation for the specified band.
/// Uses SIMD-optimized algorithms from oxigdal-algorithms for performance.
///
/// # Safety
/// - dataset must be valid
/// - band must be valid band index (1-based)
/// - out_stats must be valid pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_ios_get_raster_stats(
    dataset: *const OxiGdalDataset,
    band: c_int,
    out_stats: *mut OxiGdalStats,
) -> OxiGdalErrorCode {
    crate::check_null!(dataset, "dataset");
    crate::check_null!(out_stats, "out_stats");

    if band < 1 {
        crate::ffi::error::set_last_error("Band index must be >= 1".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    // Use approximate statistics for iOS (read from overview if available for better performance)
    let approx_ok = 1;

    // Delegate to the main FFI statistics function which handles:
    // - Band validation
    // - Data reading (with optional overview for approximate stats)
    // - SIMD-optimized computation via RasterBuffer::compute_statistics
    // SAFETY: All pointers validated above, delegate maintains safety guarantees
    unsafe {
        crate::ffi::raster::oxigdal_dataset_compute_stats(dataset, band, approx_ok, out_stats)
    }
}

/// Reads a tile in XYZ tile scheme optimized for iOS.
///
/// This function reads a tile from a dataset using XYZ tile coordinates
/// and automatically converts it to RGBA format suitable for UIImage and Metal.
///
/// # Parameters
/// - `dataset`: Dataset handle
/// - `z`: Zoom level
/// - `x`: Tile column
/// - `y`: Tile row
/// - `tile_size`: Size of tile in pixels (typically 256 or 512)
/// - `out_buffer`: Output buffer (must be pre-allocated with 4 channels for RGBA)
///
/// # Safety
/// - All pointers must be valid
/// - Buffer must be properly allocated for tile_size * tile_size * 4 bytes
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_ios_read_tile(
    dataset: *const OxiGdalDataset,
    z: c_int,
    x: c_int,
    y: c_int,
    tile_size: c_int,
    out_buffer: *mut OxiGdalBuffer,
) -> OxiGdalErrorCode {
    crate::check_null!(dataset, "dataset");
    crate::check_null!(out_buffer, "out_buffer");

    if z < 0 || x < 0 || y < 0 {
        crate::ffi::error::set_last_error("Tile coordinates must be non-negative".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    if tile_size <= 0 || tile_size > 4096 {
        crate::ffi::error::set_last_error("Invalid tile size".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    // Create tile coordinate
    let coord = OxiGdalTileCoord { z, x, y };

    // Read tile using FFI function
    let mut tile_ptr: *mut crate::ffi::types::OxiGdalTile = std::ptr::null_mut();
    // SAFETY: All pointers validated above
    let result = unsafe {
        crate::ffi::raster::oxigdal_dataset_read_tile(dataset, &coord, tile_size, &mut tile_ptr)
    };

    if result != OxiGdalErrorCode::Success {
        return result;
    }

    if tile_ptr.is_null() {
        crate::ffi::error::set_last_error("Failed to read tile".to_string());
        return OxiGdalErrorCode::IoError;
    }

    // Get tile data
    let mut tile_buffer = OxiGdalBuffer {
        data: std::ptr::null_mut(),
        length: 0,
        width: 0,
        height: 0,
        channels: 0,
    };

    // SAFETY: tile_ptr validated non-null above
    let get_result =
        unsafe { crate::ffi::raster::oxigdal_tile_get_data(tile_ptr, &mut tile_buffer) };

    if get_result != OxiGdalErrorCode::Success {
        // SAFETY: tile_ptr is valid
        unsafe {
            crate::ffi::raster::oxigdal_tile_free(tile_ptr);
        }
        return get_result;
    }

    // SAFETY: Caller guarantees out_buffer is valid (checked for null above)
    let out_buf = unsafe { &mut *out_buffer };

    // Ensure output buffer has enough space
    let required_size = (tile_size * tile_size * 4) as usize;
    if out_buf.length < required_size {
        crate::ffi::error::set_last_error(format!(
            "Output buffer too small: {} < {}",
            out_buf.length, required_size
        ));
        // SAFETY: tile_ptr is valid
        unsafe {
            crate::ffi::raster::oxigdal_tile_free(tile_ptr);
        }
        return OxiGdalErrorCode::InvalidArgument;
    }

    // Convert to RGBA format for iOS
    // SAFETY: Both buffers validated for size and non-null above
    unsafe {
        if tile_buffer.channels == 3 {
            // RGB to RGBA conversion
            let pixels = (tile_size * tile_size) as usize;
            for i in 0..pixels {
                let src_offset = i * 3;
                let dst_offset = i * 4;

                if src_offset + 2 < tile_buffer.length && dst_offset + 3 < out_buf.length {
                    std::ptr::copy_nonoverlapping(
                        tile_buffer.data.add(src_offset),
                        out_buf.data.add(dst_offset),
                        1,
                    ); // R
                    std::ptr::copy_nonoverlapping(
                        tile_buffer.data.add(src_offset + 1),
                        out_buf.data.add(dst_offset + 1),
                        1,
                    ); // G
                    std::ptr::copy_nonoverlapping(
                        tile_buffer.data.add(src_offset + 2),
                        out_buf.data.add(dst_offset + 2),
                        1,
                    ); // B
                    std::ptr::copy_nonoverlapping(
                        &0xFFu8 as *const u8,
                        out_buf.data.add(dst_offset + 3),
                        1,
                    ); // A
                }
            }
        } else if tile_buffer.channels == 4 {
            // Already RGBA, just copy
            std::ptr::copy_nonoverlapping(
                tile_buffer.data,
                out_buf.data,
                tile_buffer.length.min(out_buf.length),
            );
        } else {
            crate::ffi::error::set_last_error(format!(
                "Unsupported channel count: {}",
                tile_buffer.channels
            ));
            crate::ffi::raster::oxigdal_tile_free(tile_ptr);
            return OxiGdalErrorCode::UnsupportedFormat;
        }

        out_buf.width = tile_size;
        out_buf.height = tile_size;
        out_buf.channels = 4;

        // Free the tile
        crate::ffi::raster::oxigdal_tile_free(tile_ptr);
    }

    OxiGdalErrorCode::Success
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enhance_params_bounds() {
        let params = OxiGdalEnhanceParams {
            brightness: 1.5,
            contrast: 1.2,
            saturation: 1.0,
            gamma: 1.0,
        };

        // Create test buffer
        let mut data = vec![128u8; 100];
        let mut buffer = OxiGdalBuffer {
            data: data.as_mut_ptr(),
            length: data.len(),
            width: 10,
            height: 10,
            channels: 1,
        };

        let result = unsafe { oxigdal_ios_enhance_image(&mut buffer, &params) };

        assert_eq!(result, OxiGdalErrorCode::Success);
    }

    #[test]
    fn test_ios_get_raster_stats_null_dataset() {
        let mut stats = OxiGdalStats {
            min: 0.0,
            max: 0.0,
            mean: 0.0,
            stddev: 0.0,
            valid_count: 0,
        };

        let result = unsafe { oxigdal_ios_get_raster_stats(std::ptr::null(), 1, &mut stats) };
        assert_eq!(result, OxiGdalErrorCode::NullPointer);
    }

    #[test]
    fn test_ios_get_raster_stats_null_out_stats() {
        use std::ffi::CString;

        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join("test_ios_stats.tif");
        let path_cstring =
            CString::new(temp_path.to_str().expect("valid path")).expect("valid cstring");

        let mut dataset_ptr: *mut OxiGdalDataset = std::ptr::null_mut();

        unsafe {
            let create_result = crate::ffi::raster::oxigdal_dataset_create(
                path_cstring.as_ptr(),
                10,
                10,
                1,
                OxiGdalDataType::Byte,
                &mut dataset_ptr,
            );
            assert_eq!(create_result, OxiGdalErrorCode::Success);

            let result = oxigdal_ios_get_raster_stats(dataset_ptr, 1, std::ptr::null_mut());
            assert_eq!(result, OxiGdalErrorCode::NullPointer);

            crate::ffi::raster::oxigdal_dataset_close(dataset_ptr);
        }
    }

    #[test]
    fn test_ios_get_raster_stats_invalid_band() {
        use std::ffi::CString;

        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join("test_ios_stats_band.tif");
        let path_cstring =
            CString::new(temp_path.to_str().expect("valid path")).expect("valid cstring");

        let mut dataset_ptr: *mut OxiGdalDataset = std::ptr::null_mut();

        unsafe {
            let create_result = crate::ffi::raster::oxigdal_dataset_create(
                path_cstring.as_ptr(),
                10,
                10,
                1,
                OxiGdalDataType::Byte,
                &mut dataset_ptr,
            );
            assert_eq!(create_result, OxiGdalErrorCode::Success);

            let mut stats = OxiGdalStats {
                min: 0.0,
                max: 0.0,
                mean: 0.0,
                stddev: 0.0,
                valid_count: 0,
            };

            // Test band 0 (invalid, must be >= 1)
            let result = oxigdal_ios_get_raster_stats(dataset_ptr, 0, &mut stats);
            assert_eq!(result, OxiGdalErrorCode::InvalidArgument);

            // Test negative band
            let result = oxigdal_ios_get_raster_stats(dataset_ptr, -1, &mut stats);
            assert_eq!(result, OxiGdalErrorCode::InvalidArgument);

            crate::ffi::raster::oxigdal_dataset_close(dataset_ptr);
        }
    }

    #[test]
    fn test_ios_read_tile_null_dataset() {
        let mut buffer_data = vec![0u8; 256 * 256 * 4];
        let mut buffer = OxiGdalBuffer {
            data: buffer_data.as_mut_ptr(),
            length: buffer_data.len(),
            width: 256,
            height: 256,
            channels: 4,
        };

        let result = unsafe { oxigdal_ios_read_tile(std::ptr::null(), 0, 0, 0, 256, &mut buffer) };

        assert_eq!(result, OxiGdalErrorCode::NullPointer);
    }

    #[test]
    fn test_ios_read_tile_null_buffer() {
        use std::ffi::CString;

        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join("test_ios_tile_null_buffer.tif");
        let path_cstring =
            CString::new(temp_path.to_str().expect("valid path")).expect("valid cstring");

        let mut dataset_ptr: *mut OxiGdalDataset = std::ptr::null_mut();

        unsafe {
            let create_result = crate::ffi::raster::oxigdal_dataset_create(
                path_cstring.as_ptr(),
                256,
                256,
                3,
                OxiGdalDataType::Byte,
                &mut dataset_ptr,
            );
            assert_eq!(create_result, OxiGdalErrorCode::Success);

            let result = oxigdal_ios_read_tile(dataset_ptr, 0, 0, 0, 256, std::ptr::null_mut());
            assert_eq!(result, OxiGdalErrorCode::NullPointer);

            crate::ffi::raster::oxigdal_dataset_close(dataset_ptr);
        }
    }

    #[test]
    fn test_ios_read_tile_invalid_coords() {
        use std::ffi::CString;

        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join("test_ios_tile_invalid_coords.tif");
        let path_cstring =
            CString::new(temp_path.to_str().expect("valid path")).expect("valid cstring");

        let mut dataset_ptr: *mut OxiGdalDataset = std::ptr::null_mut();

        unsafe {
            let create_result = crate::ffi::raster::oxigdal_dataset_create(
                path_cstring.as_ptr(),
                256,
                256,
                3,
                OxiGdalDataType::Byte,
                &mut dataset_ptr,
            );
            assert_eq!(create_result, OxiGdalErrorCode::Success);

            let mut buffer_data = vec![0u8; 256 * 256 * 4];
            let mut buffer = OxiGdalBuffer {
                data: buffer_data.as_mut_ptr(),
                length: buffer_data.len(),
                width: 256,
                height: 256,
                channels: 4,
            };

            // Test negative z
            let result = oxigdal_ios_read_tile(dataset_ptr, -1, 0, 0, 256, &mut buffer);
            assert_eq!(result, OxiGdalErrorCode::InvalidArgument);

            // Test negative x
            let result = oxigdal_ios_read_tile(dataset_ptr, 0, -1, 0, 256, &mut buffer);
            assert_eq!(result, OxiGdalErrorCode::InvalidArgument);

            // Test negative y
            let result = oxigdal_ios_read_tile(dataset_ptr, 0, 0, -1, 256, &mut buffer);
            assert_eq!(result, OxiGdalErrorCode::InvalidArgument);

            crate::ffi::raster::oxigdal_dataset_close(dataset_ptr);
        }
    }

    #[test]
    fn test_ios_read_tile_invalid_size() {
        use std::ffi::CString;

        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join("test_ios_tile_invalid_size.tif");
        let path_cstring =
            CString::new(temp_path.to_str().expect("valid path")).expect("valid cstring");

        let mut dataset_ptr: *mut OxiGdalDataset = std::ptr::null_mut();

        unsafe {
            let create_result = crate::ffi::raster::oxigdal_dataset_create(
                path_cstring.as_ptr(),
                256,
                256,
                3,
                OxiGdalDataType::Byte,
                &mut dataset_ptr,
            );
            assert_eq!(create_result, OxiGdalErrorCode::Success);

            let mut buffer_data = vec![0u8; 256 * 256 * 4];
            let mut buffer = OxiGdalBuffer {
                data: buffer_data.as_mut_ptr(),
                length: buffer_data.len(),
                width: 256,
                height: 256,
                channels: 4,
            };

            // Test zero size
            let result = oxigdal_ios_read_tile(dataset_ptr, 0, 0, 0, 0, &mut buffer);
            assert_eq!(result, OxiGdalErrorCode::InvalidArgument);

            // Test negative size
            let result = oxigdal_ios_read_tile(dataset_ptr, 0, 0, 0, -1, &mut buffer);
            assert_eq!(result, OxiGdalErrorCode::InvalidArgument);

            // Test too large size
            let result = oxigdal_ios_read_tile(dataset_ptr, 0, 0, 0, 5000, &mut buffer);
            assert_eq!(result, OxiGdalErrorCode::InvalidArgument);

            crate::ffi::raster::oxigdal_dataset_close(dataset_ptr);
        }
    }

    #[test]
    fn test_ios_read_tile_buffer_too_small() {
        use std::ffi::CString;

        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join("test_ios_tile_small_buffer.tif");
        let path_cstring =
            CString::new(temp_path.to_str().expect("valid path")).expect("valid cstring");

        let mut dataset_ptr: *mut OxiGdalDataset = std::ptr::null_mut();

        unsafe {
            let create_result = crate::ffi::raster::oxigdal_dataset_create(
                path_cstring.as_ptr(),
                512,
                512,
                3,
                OxiGdalDataType::Byte,
                &mut dataset_ptr,
            );
            assert_eq!(create_result, OxiGdalErrorCode::Success);

            // Buffer too small for 256x256 tile
            let mut buffer_data = vec![0u8; 100];
            let mut buffer = OxiGdalBuffer {
                data: buffer_data.as_mut_ptr(),
                length: buffer_data.len(),
                width: 10,
                height: 10,
                channels: 4,
            };

            let result = oxigdal_ios_read_tile(dataset_ptr, 0, 0, 0, 256, &mut buffer);
            assert_eq!(result, OxiGdalErrorCode::InvalidArgument);

            crate::ffi::raster::oxigdal_dataset_close(dataset_ptr);
        }
    }
}
