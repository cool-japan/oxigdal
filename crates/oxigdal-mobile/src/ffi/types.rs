//! C-compatible FFI types for mobile bindings.
//!
//! This module provides C-compatible type definitions that can be safely
//! used across FFI boundaries with iOS (Swift/Objective-C) and Android (Java/Kotlin).

use std::os::raw::{c_char, c_double, c_int, c_void};

/// FFI-safe error codes returned by all operations.
///
/// These codes can be directly mapped to platform-specific error types
/// in Swift and Kotlin wrappers.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OxiGdalErrorCode {
    /// Operation completed successfully
    Success = 0,
    /// Null pointer provided
    NullPointer = 1,
    /// Invalid argument
    InvalidArgument = 2,
    /// File not found or inaccessible
    FileNotFound = 3,
    /// IO error occurred
    IoError = 4,
    /// Unsupported format
    UnsupportedFormat = 5,
    /// Out of bounds access
    OutOfBounds = 6,
    /// Memory allocation failed
    AllocationFailed = 7,
    /// Invalid UTF-8 string
    InvalidUtf8 = 8,
    /// Driver error
    DriverError = 9,
    /// Projection error
    ProjectionError = 10,
    /// Unknown error
    Unknown = 99,
}

/// Opaque handle to a dataset.
///
/// This is a pointer to an internal Rust structure that should never
/// be dereferenced from the FFI side. Use provided functions to interact.
#[repr(C)]
pub struct OxiGdalDataset {
    _private: [u8; 0],
}

/// Opaque handle to a raster band.
#[repr(C)]
pub struct OxiGdalBand {
    _private: [u8; 0],
}

/// Opaque handle to a vector layer.
#[repr(C)]
pub struct OxiGdalLayer {
    _private: [u8; 0],
}

/// Opaque handle to a feature.
#[repr(C)]
pub struct OxiGdalFeature {
    _private: [u8; 0],
}

/// Opaque handle to a tile.
#[repr(C)]
pub struct OxiGdalTile {
    _private: [u8; 0],
}

/// Metadata about a dataset.
///
/// This structure is safe to pass across FFI boundaries as it contains
/// only primitive types and fixed-size arrays.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxiGdalMetadata {
    /// Width in pixels
    pub width: c_int,
    /// Height in pixels
    pub height: c_int,
    /// Number of bands/channels
    pub band_count: c_int,
    /// Data type code (0=byte, 1=uint16, 2=int16, etc.)
    pub data_type: c_int,
    /// Coordinate reference system EPSG code (0 if unknown)
    pub epsg_code: c_int,
    /// Geotransform coefficients [x_origin, pixel_width, rotation_x, y_origin, rotation_y, pixel_height]
    pub geotransform: [c_double; 6],
}

/// Bounding box in geographic coordinates.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxiGdalBbox {
    /// Minimum X coordinate (longitude)
    pub min_x: c_double,
    /// Minimum Y coordinate (latitude)
    pub min_y: c_double,
    /// Maximum X coordinate (longitude)
    pub max_x: c_double,
    /// Maximum Y coordinate (latitude)
    pub max_y: c_double,
}

/// Point in geographic coordinates.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxiGdalPoint {
    /// X coordinate (longitude)
    pub x: c_double,
    /// Y coordinate (latitude)
    pub y: c_double,
    /// Optional Z coordinate (elevation)
    pub z: c_double,
}

/// Tile coordinates in XYZ scheme.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxiGdalTileCoord {
    /// Zoom level
    pub z: c_int,
    /// Column index
    pub x: c_int,
    /// Row index
    pub y: c_int,
}

/// Image buffer containing pixel data.
///
/// The caller is responsible for allocating the buffer with sufficient size.
/// Buffer size should be: width * height * channels * bytes_per_pixel
#[repr(C)]
#[derive(Debug)]
pub struct OxiGdalBuffer {
    /// Pointer to pixel data
    pub data: *mut u8,
    /// Length of data in bytes
    pub length: usize,
    /// Width in pixels
    pub width: c_int,
    /// Height in pixels
    pub height: c_int,
    /// Number of channels (e.g., 3 for RGB, 4 for RGBA)
    pub channels: c_int,
}

/// Enhancement parameters for image processing.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxiGdalEnhanceParams {
    /// Brightness multiplier (1.0 = no change)
    pub brightness: c_double,
    /// Contrast multiplier (1.0 = no change)
    pub contrast: c_double,
    /// Saturation multiplier (1.0 = no change, 0.0 = grayscale)
    pub saturation: c_double,
    /// Gamma correction (1.0 = no change)
    pub gamma: c_double,
}

impl Default for OxiGdalEnhanceParams {
    fn default() -> Self {
        Self {
            brightness: 1.0,
            contrast: 1.0,
            saturation: 1.0,
            gamma: 1.0,
        }
    }
}

/// FFI-safe string type.
///
/// Strings returned from Rust must be freed using `oxigdal_string_free`.
pub type OxiGdalString = *mut c_char;

/// Callback type for progress reporting.
///
/// # Parameters
/// - `progress`: Progress value between 0.0 and 1.0
/// - `message`: Optional status message (can be null)
/// - `user_data`: User-provided context pointer
///
/// # Returns
/// - 0 to continue operation
/// - non-zero to cancel operation
pub type OxiGdalProgressCallback =
    extern "C" fn(progress: c_double, message: *const c_char, user_data: *mut c_void) -> c_int;

/// Version information.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxiGdalVersion {
    /// Major version number
    pub major: c_int,
    /// Minor version number
    pub minor: c_int,
    /// Patch version number
    pub patch: c_int,
}

/// Statistics for a raster band.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OxiGdalStats {
    /// Minimum value
    pub min: c_double,
    /// Maximum value
    pub max: c_double,
    /// Mean value
    pub mean: c_double,
    /// Standard deviation
    pub stddev: c_double,
    /// Number of valid (non-nodata) pixels
    pub valid_count: u64,
}

/// Resampling algorithms for image operations.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OxiGdalResampling {
    /// Nearest neighbor (fastest)
    Nearest = 0,
    /// Bilinear interpolation
    #[default]
    Bilinear = 1,
    /// Cubic convolution
    Cubic = 2,
    /// Lanczos windowed sinc
    Lanczos = 3,
    /// Average of contributing pixels
    Average = 4,
}

/// Data type enumeration.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OxiGdalDataType {
    /// Unsigned 8-bit integer
    Byte = 0,
    /// Unsigned 16-bit integer
    UInt16 = 1,
    /// Signed 16-bit integer
    Int16 = 2,
    /// Unsigned 32-bit integer
    UInt32 = 3,
    /// Signed 32-bit integer
    Int32 = 4,
    /// 32-bit floating point
    Float32 = 5,
    /// 64-bit floating point
    Float64 = 6,
}

/// Filter types for image enhancement.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OxiGdalFilterType {
    /// Gaussian blur
    GaussianBlur = 0,
    /// Sharpen filter
    Sharpen = 1,
    /// Edge detection
    EdgeDetect = 2,
    /// Emboss effect
    Emboss = 3,
}

/// Compression type for output.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OxiGdalCompression {
    /// No compression
    None = 0,
    /// DEFLATE compression
    Deflate = 1,
    /// LZW compression
    Lzw = 2,
    /// JPEG compression
    Jpeg = 3,
    /// WebP compression
    Webp = 4,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_code_values() {
        assert_eq!(OxiGdalErrorCode::Success as i32, 0);
        assert_eq!(OxiGdalErrorCode::NullPointer as i32, 1);
        assert_eq!(OxiGdalErrorCode::Unknown as i32, 99);
    }

    #[test]
    fn test_metadata_size() {
        // Ensure metadata is reasonably sized for FFI
        assert!(std::mem::size_of::<OxiGdalMetadata>() < 256);
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
    fn test_types_are_repr_c() {
        // These should all compile - verifying #[repr(C)]
        let _: OxiGdalMetadata = unsafe { std::mem::zeroed() };
        let _: OxiGdalBbox = unsafe { std::mem::zeroed() };
        let _: OxiGdalPoint = unsafe { std::mem::zeroed() };
    }
}
