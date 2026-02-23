//! Android-specific optimizations and features
//!
//! This module provides Android platform-specific optimizations including:
//! - RenderScript acceleration hints
//! - Android Runtime (ART) optimizations
//! - Lifecycle-aware processing
//! - Background execution limits
//! - Android-specific performance tuning

pub mod memory;
pub mod performance;

pub use memory::{AndroidMemoryManager, LowMemoryKiller};
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

impl AndroidDevice {
    /// Get current Android device information
    pub fn current() -> Result<Self> {
        // In a real implementation, this would query Android APIs
        // For now, return mock values
        Ok(Self {
            api_level: AndroidApiLevel::TIRAMISU,
            performance_tier: PerformanceTier::High,
        })
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
    fn test_android_device() {
        let device = AndroidDevice::current().expect("Failed to get device");

        assert!(device.is_renderscript_available());
        assert!(device.has_background_limits());
        assert!(device.has_scoped_storage());
        assert!(device.is_job_scheduler_available());
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
