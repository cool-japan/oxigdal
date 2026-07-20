//! OxiGeo Mobile SDK - FFI bindings for iOS and Android
//!
//! This crate provides C-compatible FFI bindings that enable OxiGeo to be used
//! from iOS (Swift/Objective-C) and Android (Kotlin/Java) applications.
//!
//! # Architecture
//!
//! The mobile SDK is organized into several layers:
//!
//! - **FFI Layer** (`ffi` module): C-compatible types and functions
//! - **Platform Layer** (`ios`/`android` modules): Platform-specific utilities
//! - **Language Bindings** (Swift/Kotlin): High-level wrappers (in `bindings/` directory)
//!
//! # Safety
//!
//! While this crate uses `unsafe` extensively due to FFI requirements, it provides
//! safety guarantees through:
//!
//! - Extensive null pointer validation
//! - Bounds checking on all array operations
//! - UTF-8 validation on string conversions
//! - Proper resource lifecycle management
//! - Thread-safe error handling
//!
//! # Memory Management
//!
//! The FFI layer follows these conventions:
//!
//! - **Handles**: Created by `*_open` or `*_create`, freed by `*_close` or `*_free`
//! - **Strings**: Returned strings must be freed with `oxigeo_string_free`
//! - **Buffers**: Caller-allocated, OxiGeo only writes to them
//! - **Opaque Types**: Must not be dereferenced on the foreign side
//!
//! # Error Handling
//!
//! All FFI functions return `OxiGeoErrorCode`:
//! - `Success` (0) indicates success
//! - Non-zero values indicate specific error types
//! - Detailed messages available via `oxigeo_get_last_error()`
//!
//! # Example (C API)
//!
//! ```c
//! // Initialize
//! oxigeo_init();
//!
//! // Open dataset
//! OxiGeoDataset* dataset;
//! if (oxigeo_dataset_open("/path/to/file.tif", &dataset) != Success) {
//!     char* error = oxigeo_get_last_error();
//!     printf("Error: %s\n", error);
//!     oxigeo_string_free(error);
//!     return;
//! }
//!
//! // Get metadata
//! OxiGeoMetadata metadata;
//! oxigeo_dataset_get_metadata(dataset, &metadata);
//! printf("Size: %d x %d\n", metadata.width, metadata.height);
//!
//! // Read region
//! OxiGeoBuffer* buffer = oxigeo_buffer_alloc(256, 256, 3);
//! oxigeo_dataset_read_region(dataset, 0, 0, 256, 256, 1, buffer);
//!
//! // Cleanup
//! oxigeo_buffer_free(buffer);
//! oxigeo_dataset_close(dataset);
//! oxigeo_cleanup();
//! ```
//!
//! # Features
//!
//! - `std` (default): Enable standard library support
//! - `ios`: Enable iOS-specific bindings
//! - `android`: Enable Android JNI bindings
//! - `offline`: Enable offline COG reading
//! - `filters`: Enable image enhancement filters
//! - `tiles`: Enable map tile generation
//!
//! # Platform Support
//!
//! ## iOS
//! - Target: `aarch64-apple-ios`, `x86_64-apple-ios` (simulator)
//! - Swift bindings available in `bindings/ios/`
//! - Integration: CocoaPods, Swift Package Manager
//!
//! ## Android
//! - Targets: `aarch64-linux-android`, `armv7-linux-androideabi`, `x86_64-linux-android`
//! - Kotlin bindings available in `bindings/android/`
//! - Integration: Gradle, AAR library
//!
//! # COOLJAPAN Policies
//!
//! This crate adheres to COOLJAPAN ecosystem policies:
//! - **Pure Rust**: No C/C++ dependencies by default
//! - **No Unwrap**: All error cases explicitly handled
//! - **Workspace**: Version management via workspace
//! - **Latest Crates**: Always use latest stable dependencies

// FFI code requires unsafe functions, unsafe blocks, and no_mangle symbols
#![allow(unsafe_code)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]
// FFI code requires no_mangle for C symbol export
#![allow(clippy::no_mangle_with_rust_abi)]
// FFI code uses expect() for internal invariant checks
#![allow(clippy::expect_used)]
// Allow unnecessary unsafe blocks (wrapped in outer unsafe fn)
#![allow(unused_unsafe)]
// Allow unused variables in FFI (may be used conditionally)
#![allow(unused_variables)]
// Allow unused imports in platform-specific code
#![allow(unused_imports)]
// Allow manual div_ceil for compatibility
#![allow(clippy::manual_div_ceil)]
// Allow complex types in FFI interfaces
#![allow(clippy::type_complexity)]
// Allow match collapsing warnings - explicit matches preferred in FFI
#![allow(clippy::collapsible_match)]
// Allow first element access with get(0)
#![allow(clippy::get_first)]
// Allow too many arguments for complex FFI operations
#![allow(clippy::too_many_arguments)]
#![warn(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![cfg_attr(not(feature = "std"), no_std)]
#![allow(unsafe_attr_outside_unsafe)]
// Allow unexpected cfg for conditional compilation
#![allow(unexpected_cfgs)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod common;
pub mod ffi;

#[cfg(feature = "ios")]
pub mod ios;

#[cfg(feature = "android")]
pub mod android;

// Re-export main types for convenience
pub use ffi::types::*;
pub use ffi::{oxigeo_cleanup, oxigeo_init};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_library_init() {
        let result = oxigeo_init();
        assert_eq!(result, OxiGeoErrorCode::Success);

        let result = oxigeo_cleanup();
        assert_eq!(result, OxiGeoErrorCode::Success);
    }

    #[test]
    fn test_error_codes_are_unique() {
        // Ensure error codes don't overlap
        let codes = vec![
            OxiGeoErrorCode::Success as i32,
            OxiGeoErrorCode::NullPointer as i32,
            OxiGeoErrorCode::InvalidArgument as i32,
            OxiGeoErrorCode::FileNotFound as i32,
            OxiGeoErrorCode::IoError as i32,
            OxiGeoErrorCode::UnsupportedFormat as i32,
            OxiGeoErrorCode::OutOfBounds as i32,
            OxiGeoErrorCode::AllocationFailed as i32,
            OxiGeoErrorCode::InvalidUtf8 as i32,
            OxiGeoErrorCode::DriverError as i32,
            OxiGeoErrorCode::ProjectionError as i32,
            OxiGeoErrorCode::Unknown as i32,
        ];

        let mut unique_codes = codes.clone();
        unique_codes.sort();
        unique_codes.dedup();

        assert_eq!(
            codes.len(),
            unique_codes.len(),
            "Error codes must be unique"
        );
    }

    #[test]
    fn test_repr_c_sizes() {
        use std::mem::size_of;

        // Ensure FFI types are reasonably sized
        assert!(size_of::<OxiGeoMetadata>() < 256);
        assert!(size_of::<OxiGeoBbox>() == 32); // 4 * f64
        assert!(size_of::<OxiGeoPoint>() == 24); // 3 * f64
        assert!(size_of::<OxiGeoTileCoord>() == 12); // 3 * i32
        assert!(size_of::<OxiGeoEnhanceParams>() == 32); // 4 * f64
    }

    #[test]
    fn test_default_values() {
        let enhance = OxiGeoEnhanceParams::default();
        assert_eq!(enhance.brightness, 1.0);
        assert_eq!(enhance.contrast, 1.0);
        assert_eq!(enhance.saturation, 1.0);
        assert_eq!(enhance.gamma, 1.0);

        let resampling = OxiGeoResampling::default();
        assert_eq!(resampling, OxiGeoResampling::Bilinear);
    }
}
