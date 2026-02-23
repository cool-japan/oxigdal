//! iOS-specific optimizations and features
//!
//! This module provides iOS platform-specific optimizations including:
//! - Metal GPU acceleration hints
//! - Core Image integration support
//! - iOS memory pressure handling
//! - Background execution management
//! - iOS-specific performance tuning

pub mod memory;
pub mod performance;

pub use memory::{IOSMemoryManager, MemoryPressureLevel};
pub use performance::{IOSPerformanceOptimizer, MetalAcceleration};

use crate::error::Result;

/// iOS platform version
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct IOSVersion {
    /// Major version number
    pub major: u32,
    /// Minor version number
    pub minor: u32,
    /// Patch version number
    pub patch: u32,
}

impl IOSVersion {
    /// Create a new iOS version
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Check if this version is at least the specified version
    pub fn is_at_least(&self, major: u32, minor: u32) -> bool {
        if self.major > major {
            return true;
        }
        if self.major == major && self.minor >= minor {
            return true;
        }
        false
    }

    /// Common iOS versions
    pub const IOS_13: Self = Self::new(13, 0, 0);
    /// iOS 14 version constant
    pub const IOS_14: Self = Self::new(14, 0, 0);
    /// iOS 15 version constant
    pub const IOS_15: Self = Self::new(15, 0, 0);
    /// iOS 16 version constant
    pub const IOS_16: Self = Self::new(16, 0, 0);
    /// iOS 17 version constant
    pub const IOS_17: Self = Self::new(17, 0, 0);
}

impl std::fmt::Display for IOSVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// iOS device type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IOSDevice {
    /// iPhone
    IPhone,
    /// iPad
    IPad,
    /// iPod Touch
    IPod,
    /// Unknown device
    Unknown,
}

impl IOSDevice {
    /// Check if device is an iPad (larger screen)
    pub fn is_tablet(&self) -> bool {
        matches!(self, Self::IPad)
    }

    /// Get recommended cache size for device
    pub fn recommended_cache_size(&self) -> usize {
        match self {
            Self::IPad => 128 * 1024 * 1024,   // 128 MB
            Self::IPhone => 64 * 1024 * 1024,  // 64 MB
            Self::IPod => 32 * 1024 * 1024,    // 32 MB
            Self::Unknown => 64 * 1024 * 1024, // 64 MB
        }
    }
}

/// iOS platform information
pub struct IOSPlatform {
    version: IOSVersion,
    device: IOSDevice,
}

impl IOSPlatform {
    /// Get current iOS platform information
    pub fn current() -> Result<Self> {
        // In a real implementation, this would use iOS APIs
        // For now, return mock values
        Ok(Self {
            version: IOSVersion::IOS_17,
            device: IOSDevice::IPhone,
        })
    }

    /// Get iOS version
    pub fn version(&self) -> IOSVersion {
        self.version
    }

    /// Get device type
    pub fn device(&self) -> IOSDevice {
        self.device
    }

    /// Check if Metal is available
    pub fn is_metal_available(&self) -> bool {
        // Metal is available on iOS 8.0+
        self.version.is_at_least(8, 0)
    }

    /// Check if background processing is available
    pub fn is_background_processing_available(&self) -> bool {
        // Background processing APIs available in iOS 13.0+
        self.version.is_at_least(13, 0)
    }

    /// Check if low power mode is likely enabled
    pub fn is_low_power_mode(&self) -> Result<bool> {
        // In a real implementation, this would check iOS power state
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ios_version() {
        let v13 = IOSVersion::IOS_13;
        let v14 = IOSVersion::IOS_14;
        let v17 = IOSVersion::IOS_17;

        assert!(v14 > v13);
        assert!(v17 > v14);

        assert!(v17.is_at_least(13, 0));
        assert!(v17.is_at_least(17, 0));
        assert!(!v13.is_at_least(14, 0));
    }

    #[test]
    fn test_ios_version_display() {
        let version = IOSVersion::new(17, 2, 1);
        assert_eq!(version.to_string(), "17.2.1");
    }

    #[test]
    fn test_ios_device() {
        assert!(IOSDevice::IPad.is_tablet());
        assert!(!IOSDevice::IPhone.is_tablet());

        assert_eq!(IOSDevice::IPad.recommended_cache_size(), 128 * 1024 * 1024);
        assert_eq!(IOSDevice::IPhone.recommended_cache_size(), 64 * 1024 * 1024);
    }

    #[test]
    fn test_ios_platform() {
        let platform = IOSPlatform::current().expect("Failed to get platform");

        assert!(platform.is_metal_available());
        assert!(platform.is_background_processing_available());
    }
}
