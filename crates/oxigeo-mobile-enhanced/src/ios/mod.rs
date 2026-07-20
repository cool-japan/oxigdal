//! iOS-specific optimizations and features
//!
//! This module provides iOS platform-specific optimizations including:
//! - Metal GPU acceleration hints
//! - Core Image integration support
//! - iOS memory pressure handling
//! - Background execution management
//! - iOS-specific performance tuning
//!
//! # Platform introspection
//!
//! Device and OS-version queries (`IOSPlatform::current`) and memory-pressure
//! sampling (`memory::IOSMemoryManager::update_stats`) read **live** state on
//! an actual iOS target (via `sysctl`/mach APIs). On non-iOS builds (desktop
//! development and CI) they fall back to documented best-effort defaults or an
//! explicit error, never to silently fabricated device data.

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

/// Map an iOS `hw.machine` model identifier (e.g. `"iPhone14,2"`) to a device
/// class. Returns [`IOSDevice::Unknown`] for unrecognized identifiers.
// Used by `current()` on iOS targets and by unit tests on all targets.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
fn parse_ios_device(machine: &str) -> IOSDevice {
    if machine.starts_with("iPhone") {
        IOSDevice::IPhone
    } else if machine.starts_with("iPad") {
        IOSDevice::IPad
    } else if machine.starts_with("iPod") {
        IOSDevice::IPod
    } else {
        IOSDevice::Unknown
    }
}

/// Parse a `kern.osproductversion` string (e.g. `"17.2"` or `"16.4.1"`) into an
/// [`IOSVersion`]. Missing components default to zero; returns `None` if no
/// numeric major component can be parsed.
// Used by `current()` on iOS targets and by unit tests on all targets.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
fn parse_ios_version(product_version: &str) -> Option<IOSVersion> {
    let mut parts = product_version
        .split('.')
        .map(|p| p.trim().parse::<u32>().ok());
    let major = parts.next().flatten()?;
    let minor = parts.next().flatten().unwrap_or(0);
    let patch = parts.next().flatten().unwrap_or(0);
    Some(IOSVersion::new(major, minor, patch))
}

impl IOSPlatform {
    /// Get current iOS platform information.
    ///
    /// On an actual iOS target this queries real device state through the
    /// `sysctl` interface (`hw.machine` for the device class and
    /// `kern.osproductversion` for the OS version), so an iPad is not
    /// misreported as an iPhone and version-gated feature checks are accurate.
    ///
    /// On non-iOS builds (desktop development / CI, where no iOS device exists)
    /// this returns documented best-effort defaults (iOS 17 / iPhone); such
    /// callers should not rely on the returned values reflecting real hardware.
    pub fn current() -> Result<Self> {
        #[cfg(target_os = "ios")]
        {
            let device = sysctl_string("hw.machine")
                .map(|m| parse_ios_device(&m))
                .unwrap_or(IOSDevice::Unknown);
            let version = sysctl_string("kern.osproductversion")
                .and_then(|v| parse_ios_version(&v))
                .unwrap_or(IOSVersion::IOS_13);
            return Ok(Self { version, device });
        }

        #[cfg(not(target_os = "ios"))]
        {
            // Best-effort defaults for non-iOS hosts (see doc comment).
            Ok(Self {
                version: IOSVersion::IOS_17,
                device: IOSDevice::IPhone,
            })
        }
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

/// Read a string-valued `sysctl` by name (e.g. `"hw.machine"`).
///
/// Uses the `sysctlbyname(3)` ABI via `libc`. This links no C library; it is a
/// raw platform-syscall binding consistent with the Pure-Rust policy. Returns
/// `None` on any failure rather than panicking.
#[cfg(target_os = "ios")]
#[allow(unsafe_code)]
fn sysctl_string(name: &str) -> Option<String> {
    use std::ffi::CString;

    let cname = CString::new(name).ok()?;
    let mut size: libc::size_t = 0;

    // First call determines the required buffer size.
    let rc = unsafe {
        libc::sysctlbyname(
            cname.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || size == 0 {
        return None;
    }

    let mut buf = vec![0u8; size];
    let rc = unsafe {
        libc::sysctlbyname(
            cname.as_ptr(),
            buf.as_mut_ptr().cast::<libc::c_void>(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }

    // The returned value is a C string; drop the trailing NUL and anything after.
    if let Some(pos) = buf.iter().position(|&b| b == 0) {
        buf.truncate(pos);
    }
    String::from_utf8(buf).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ios_device() {
        assert_eq!(parse_ios_device("iPhone14,2"), IOSDevice::IPhone);
        assert_eq!(parse_ios_device("iPad13,1"), IOSDevice::IPad);
        assert_eq!(parse_ios_device("iPod9,1"), IOSDevice::IPod);
        assert_eq!(parse_ios_device("x86_64"), IOSDevice::Unknown);
        assert_eq!(parse_ios_device(""), IOSDevice::Unknown);
    }

    #[test]
    fn test_parse_ios_version() {
        assert_eq!(parse_ios_version("17.2"), Some(IOSVersion::new(17, 2, 0)));
        assert_eq!(parse_ios_version("16.4.1"), Some(IOSVersion::new(16, 4, 1)));
        assert_eq!(parse_ios_version("13"), Some(IOSVersion::new(13, 0, 0)));
        assert_eq!(parse_ios_version("not-a-version"), None);
        assert_eq!(parse_ios_version(""), None);
    }

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

        // On non-iOS hosts (CI/dev), current() returns the documented best-effort
        // defaults rather than fabricated device state.
        #[cfg(not(target_os = "ios"))]
        {
            assert_eq!(platform.version(), IOSVersion::IOS_17);
            assert_eq!(platform.device(), IOSDevice::IPhone);
        }
    }
}
