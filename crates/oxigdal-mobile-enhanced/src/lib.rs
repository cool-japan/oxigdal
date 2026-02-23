//! OxiGDAL Mobile Enhanced - Advanced mobile platform optimizations
//!
//! This crate provides comprehensive mobile platform optimizations for iOS and Android,
//! focusing on performance, battery efficiency, network optimization, and mobile-specific
//! features.
//!
//! # Features
//!
//! - **Battery-Aware Processing**: Adaptive algorithms that adjust based on battery level
//! - **Network Optimization**: Efficient data transfer minimizing cellular usage
//! - **Storage Optimization**: Compression and caching strategies for limited storage
//! - **Background Processing**: Safe background task execution on mobile platforms
//! - **Memory Management**: Mobile-specific memory pressure handling
//! - **Performance Tuning**: Platform-specific optimizations for iOS and Android
//!
//! # Architecture
//!
//! The crate is organized into platform-specific and cross-platform modules:
//!
//! - **iOS Module** (`ios`): iOS-specific optimizations including Metal GPU acceleration hints,
//!   Core Image integration, and iOS memory management
//! - **Android Module** (`android`): Android-specific optimizations including RenderScript hints,
//!   Android Runtime optimizations, and lifecycle-aware processing
//! - **Battery Module** (`battery`): Cross-platform battery-aware algorithms
//! - **Network Module** (`network`): Mobile network optimization strategies
//! - **Storage Module** (`storage`): Mobile storage and caching strategies
//! - **Background Module** (`background`): Background task management
//!
//! # Usage Example
//!
//! ```rust,no_run
//! use oxigdal_mobile_enhanced::battery::{BatteryMonitor, ProcessingMode};
//! use oxigdal_mobile_enhanced::network::NetworkOptimizer;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create battery monitor
//! let monitor = BatteryMonitor::new()?;
//!
//! // Adjust processing based on battery level
//! let mode = monitor.recommended_processing_mode();
//! match mode {
//!     ProcessingMode::HighPerformance => {
//!         // Full processing enabled
//!     }
//!     ProcessingMode::Balanced => {
//!         // Moderate processing
//!     }
//!     ProcessingMode::PowerSaver => {
//!         // Minimal processing
//!     }
//! }
//!
//! // Optimize network transfers
//! let optimizer = NetworkOptimizer::new();/// let data = vec![0u8; 100];//! let compressed_data = optimizer.compress_for_transfer(&data)?;
//! # Ok(())
//! # }
//! ```
//!
//! # COOLJAPAN Policies
//!
//! This crate adheres to COOLJAPAN ecosystem policies:
//! - **Pure Rust**: 100% Pure Rust implementation
//! - **No Unwrap**: All error cases explicitly handled
//! - **Workspace**: Version management via workspace
//! - **Latest Crates**: Always use latest stable dependencies
//! - **No Warnings**: Code compiles without warnings
//!
//! # Platform Support
//!
//! ## iOS
//! - Minimum iOS version: 13.0
//! - Targets: `aarch64-apple-ios`, `x86_64-apple-ios` (simulator)
//! - Features: Metal GPU hints, Core Image optimization, iOS memory pressure handling
//!
//! ## Android
//! - Minimum Android version: API 24 (Android 7.0)
//! - Targets: `aarch64-linux-android`, `armv7-linux-androideabi`, `x86_64-linux-android`
//! - Features: RenderScript hints, ART optimization, Android lifecycle awareness

#![warn(missing_docs)]
#![cfg_attr(not(feature = "std"), no_std)]

// Alloc support for no_std environments
#[cfg(all(not(feature = "std"), feature = "alloc"))]
extern crate alloc;

/// Battery-aware processing and power management
pub mod battery;

/// Mobile network optimization
pub mod network;

/// Background task management
pub mod background;

/// Mobile storage optimization
pub mod storage;

/// iOS-specific optimizations
#[cfg(feature = "ios")]
pub mod ios;

/// Android-specific optimizations
#[cfg(feature = "android")]
pub mod android;

/// Common error types for mobile-enhanced operations
pub mod error;

// Re-export commonly used types
pub use error::{MobileError, Result};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_availability() {
        // Ensure all modules compile
        let _battery = battery::BatteryLevel::Full;
        let _network = network::NetworkType::WiFi;
    }

    #[test]
    fn test_error_types() {
        let err = MobileError::BatteryMonitoringNotSupported;
        assert!(format!("{}", err).contains("Battery monitoring"));
    }
}
