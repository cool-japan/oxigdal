//! Android-specific optimizations and features
//!
//! This module provides Android platform-specific optimizations including:
//! - RenderScript acceleration hints
//! - Android Runtime (ART) optimizations
//! - Lifecycle-aware processing
//! - Background execution limits
//! - Android-specific performance tuning
//!
//! # Platform introspection
//!
//! Device queries (`AndroidDevice::current`) and memory sampling
//! (`memory::AndroidMemoryManager::update_stats`) read **live** state on an
//! actual Android target: the API level from the `ro.build.version.sdk` system
//! property, and RAM figures from the OS. Java/ART-heap figures and the exact
//! `ActivityManager` low-memory threshold require JNI at the host-app call site
//! and can be injected via
//! `memory::AndroidMemoryManager::update_stats_with`. On non-Android builds
//! these fall back to documented conservative defaults, never to silently
//! fabricated device data.

pub mod memory;
pub mod performance;

pub use memory::{AndroidMemoryManager, AndroidMemorySample, LowMemoryKiller};
pub use performance::{AndroidPerformanceOptimizer, RenderScriptLevel};

use crate::error::Result;

/// Android API level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AndroidApiLevel(pub u32);

impl AndroidApiLevel {
    /// Android 7.0 Nougat (API 24)
    pub const NOUGAT: Self = Self(24);
    /// Android 8.0 Oreo (API 26)
    pub const OREO: Self = Self(26);
    /// Android 9.0 Pie (API 28)
    pub const PIE: Self = Self(28);
    /// Android 10 (API 29)
    pub const Q: Self = Self(29);
    /// Android 11 (API 30)
    pub const R: Self = Self(30);
    /// Android 12 (API 31)
    pub const S: Self = Self(31);
    /// Android 13 (API 33)
    pub const TIRAMISU: Self = Self(33);
    /// Android 14 (API 34)
    pub const U: Self = Self(34);

    /// Check if API level is at least the specified level
    pub fn is_at_least(&self, level: u32) -> bool {
        self.0 >= level
    }

    /// Get version name
    pub fn version_name(&self) -> &'static str {
        match self.0 {
            24..=25 => "Nougat",
            26..=27 => "Oreo",
            28 => "Pie",
            29 => "10",
            30 => "11",
            31..=32 => "12",
            33 => "13",
            34 => "14",
            _ => "Unknown",
        }
    }
}

impl std::fmt::Display for AndroidApiLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "API {} ({})", self.0, self.version_name())
    }
}

/// Android device performance tier
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PerformanceTier {
    /// Low-end device
    Low,
    /// Mid-range device
    Medium,
    /// High-end device
    High,
    /// Premium flagship device
    Premium,
}

impl PerformanceTier {
    /// Get recommended concurrent operations
    pub fn max_concurrent_operations(&self) -> usize {
        match self {
            Self::Low => 2,
            Self::Medium => 4,
            Self::High => 6,
            Self::Premium => 8,
        }
    }

    /// Get recommended cache size in bytes
    pub fn recommended_cache_size(&self) -> usize {
        match self {
            Self::Low => 32 * 1024 * 1024,      // 32 MB
            Self::Medium => 64 * 1024 * 1024,   // 64 MB
            Self::High => 128 * 1024 * 1024,    // 128 MB
            Self::Premium => 256 * 1024 * 1024, // 256 MB
        }
    }

    /// Check if GPU acceleration should be used
    pub fn should_use_gpu(&self) -> bool {
        matches!(self, Self::High | Self::Premium)
    }
}

/// Android device information
pub struct AndroidDevice {
    api_level: AndroidApiLevel,
    performance_tier: PerformanceTier,
}

/// Derive a [`PerformanceTier`] from total device RAM in bytes.
///
/// This mirrors the intent of `ActivityManager.isLowRamDevice()` /
/// `getMemoryClass()`: less RAM implies a lower tier and more conservative
/// GPU/cache recommendations.
// Used by `current()` on Android targets and by unit tests on all targets.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn tier_from_total_ram(total_ram: u64) -> PerformanceTier {
    const GB: u64 = 1024 * 1024 * 1024;
    if total_ram < 2 * GB {
        PerformanceTier::Low
    } else if total_ram < 4 * GB {
        PerformanceTier::Medium
    } else if total_ram < 6 * GB {
        PerformanceTier::High
    } else {
        PerformanceTier::Premium
    }
}

impl AndroidDevice {
    /// Get current Android device information.
    ///
    /// On an actual Android target this reads real device state: the API level
    /// comes from the `ro.build.version.sdk` system property and the
    /// [`PerformanceTier`] is derived from the device's real total RAM
    /// (approximating `ActivityManager.isLowRamDevice()`/`getMemoryClass()`), so
    /// low-end devices are no longer told to enable GPU acceleration or use a
    /// 128 MB cache.
    ///
    /// On non-Android builds (desktop development / CI) it returns a documented,
    /// conservative fallback (API 24 / [`PerformanceTier::Medium`]); such callers
    /// should not treat the result as real hardware data.
    pub fn current() -> Result<Self> {
        #[cfg(target_os = "android")]
        {
            let api_level = system_property("ro.build.version.sdk")
                .and_then(|s| s.trim().parse::<u32>().ok())
                .map(AndroidApiLevel)
                .unwrap_or(AndroidApiLevel::NOUGAT);

            let performance_tier = match crate::sys_probe::sample_physical_memory() {
                Ok(mem) => tier_from_total_ram(mem.total),
                // Conservative default if RAM cannot be read.
                Err(_) => PerformanceTier::Medium,
            };

            return Ok(Self {
                api_level,
                performance_tier,
            });
        }

        #[cfg(not(target_os = "android"))]
        {
            // Conservative, documented fallback for non-Android hosts.
            Ok(Self {
                api_level: AndroidApiLevel::NOUGAT,
                performance_tier: PerformanceTier::Medium,
            })
        }
    }

    /// Get API level
    pub fn api_level(&self) -> AndroidApiLevel {
        self.api_level
    }

    /// Get performance tier
    pub fn performance_tier(&self) -> PerformanceTier {
        self.performance_tier
    }

    /// Check if RenderScript is available
    pub fn is_renderscript_available(&self) -> bool {
        // RenderScript deprecated in API 31+, but still available
        self.api_level.0 >= 17
    }

    /// Check if background execution limits apply
    pub fn has_background_limits(&self) -> bool {
        // Background execution limits introduced in Oreo (API 26)
        self.api_level.is_at_least(26)
    }

    /// Check if scoped storage is enforced
    pub fn has_scoped_storage(&self) -> bool {
        // Scoped storage enforced in Android 10 (API 29)
        self.api_level.is_at_least(29)
    }

    /// Check if JobScheduler is available
    pub fn is_job_scheduler_available(&self) -> bool {
        self.api_level.is_at_least(21)
    }
}

/// Android lifecycle state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    /// App is in foreground and active
    Foreground,
    /// App is visible but not focused
    Visible,
    /// App is in background
    Background,
    /// App is stopped
    Stopped,
}

impl LifecycleState {
    /// Check if intensive operations are allowed
    pub fn allows_intensive_operations(&self) -> bool {
        matches!(self, Self::Foreground | Self::Visible)
    }

    /// Check if background processing is allowed
    pub fn allows_background_processing(&self) -> bool {
        !matches!(self, Self::Stopped)
    }

    /// Get recommended quality level (0.0 - 1.0)
    pub fn quality_level(&self) -> f32 {
        match self {
            Self::Foreground => 1.0,
            Self::Visible => 0.8,
            Self::Background => 0.5,
            Self::Stopped => 0.3,
        }
    }
}

/// Read an Android system property (e.g. `"ro.build.version.sdk"`).
///
/// Uses the `__system_property_get(3)` ABI via `libc`. This links no C library;
/// it is a raw platform-syscall binding consistent with the Pure-Rust policy.
/// Returns `None` on any failure rather than panicking.
#[cfg(target_os = "android")]
#[allow(unsafe_code)]
fn system_property(name: &str) -> Option<String> {
    use std::ffi::CString;

    let cname = CString::new(name).ok()?;
    // Android's PROP_VALUE_MAX is 92 bytes (including the trailing NUL).
    let mut buf = vec![0u8; 92];
    let len = unsafe {
        libc::__system_property_get(cname.as_ptr(), buf.as_mut_ptr().cast::<libc::c_char>())
    };
    if len <= 0 {
        return None;
    }
    buf.truncate(len as usize);
    String::from_utf8(buf).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_android_api_level() {
        let api24 = AndroidApiLevel::NOUGAT;
        let api33 = AndroidApiLevel::TIRAMISU;

        assert!(api33 > api24);
        assert!(api33.is_at_least(24));
        assert!(!api24.is_at_least(33));

        assert_eq!(api24.version_name(), "Nougat");
        assert_eq!(api33.version_name(), "13");
    }

    #[test]
    fn test_api_level_display() {
        let api = AndroidApiLevel::TIRAMISU;
        assert_eq!(api.to_string(), "API 33 (13)");
    }

    #[test]
    fn test_performance_tier() {
        assert_eq!(PerformanceTier::Low.max_concurrent_operations(), 2);
        assert_eq!(PerformanceTier::Premium.max_concurrent_operations(), 8);

        assert!(!PerformanceTier::Low.should_use_gpu());
        assert!(PerformanceTier::High.should_use_gpu());
    }

    #[test]
    fn test_tier_from_total_ram() {
        const GB: u64 = 1024 * 1024 * 1024;
        assert_eq!(
            tier_from_total_ram(1024 * 1024 * 1024),
            PerformanceTier::Low
        ); // 1 GB
        assert_eq!(tier_from_total_ram(3 * GB), PerformanceTier::Medium);
        assert_eq!(tier_from_total_ram(5 * GB), PerformanceTier::High);
        assert_eq!(tier_from_total_ram(8 * GB), PerformanceTier::Premium);
        // Boundary checks.
        assert_eq!(tier_from_total_ram(2 * GB), PerformanceTier::Medium);
        assert_eq!(tier_from_total_ram(6 * GB), PerformanceTier::Premium);
    }

    #[test]
    fn test_android_device() {
        let device = AndroidDevice::current().expect("Failed to get device");

        // Derived helpers must be consistent with the reported API level rather
        // than a fabricated high-end constant.
        assert_eq!(
            device.has_background_limits(),
            device.api_level().is_at_least(26)
        );
        assert_eq!(
            device.has_scoped_storage(),
            device.api_level().is_at_least(29)
        );

        // On non-Android hosts (CI/dev) current() returns the documented
        // conservative fallback: API 24 (Nougat) / Medium tier.
        #[cfg(not(target_os = "android"))]
        {
            assert_eq!(device.api_level(), AndroidApiLevel::NOUGAT);
            assert_eq!(device.performance_tier(), PerformanceTier::Medium);
            assert!(device.is_renderscript_available());
            assert!(device.is_job_scheduler_available());
        }
    }

    #[test]
    fn test_lifecycle_state() {
        assert!(LifecycleState::Foreground.allows_intensive_operations());
        assert!(!LifecycleState::Background.allows_intensive_operations());

        assert!(LifecycleState::Background.allows_background_processing());
        assert!(!LifecycleState::Stopped.allows_background_processing());

        assert_eq!(LifecycleState::Foreground.quality_level(), 1.0);
        assert_eq!(LifecycleState::Background.quality_level(), 0.5);
    }
}
