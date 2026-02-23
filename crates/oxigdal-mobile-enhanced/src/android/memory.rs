//! Android memory management and low memory handling

use super::{AndroidApiLevel, PerformanceTier};
use crate::error::{MobileError, Result};
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Instant;

/// Low Memory Killer (LMK) priority level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LowMemoryKiller {
    /// Cached processes (killed first)
    Cached,
    /// Service processes
    Service,
    /// Visible processes
    Visible,
    /// Foreground processes (killed last)
    Foreground,
}

impl LowMemoryKiller {
    /// Get OOM adjustment score
    pub fn oom_score(&self) -> i32 {
        match self {
            Self::Cached => 15,
            Self::Service => 8,
            Self::Visible => 1,
            Self::Foreground => 0,
        }
    }

    /// Get recommended memory budget in bytes
    pub fn memory_budget(&self) -> usize {
        match self {
            Self::Foreground => 256 * 1024 * 1024, // 256 MB
            Self::Visible => 128 * 1024 * 1024,    // 128 MB
            Self::Service => 64 * 1024 * 1024,     // 64 MB
            Self::Cached => 32 * 1024 * 1024,      // 32 MB
        }
    }
}

/// Android memory trim level (from ComponentCallbacks2)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrimLevel {
    /// Trim memory completely (process in background)
    Complete,
    /// Trim memory moderately (process backgrounded)
    Moderate,
    /// Trim memory for background processes
    Background,
    /// Running low on memory
    RunningLow,
    /// Running moderately low on memory
    RunningModerate,
    /// Running critically low on memory
    RunningCritical,
    /// UI is hidden
    UiHidden,
}

impl TrimLevel {
    /// Get cache reduction factor
    pub fn cache_reduction_factor(&self) -> f32 {
        match self {
            Self::Complete => 0.0,
            Self::Moderate => 0.25,
            Self::Background => 0.5,
            Self::RunningLow => 0.6,
            Self::RunningModerate => 0.5,
            Self::RunningCritical => 0.25,
            Self::UiHidden => 0.75,
        }
    }

    /// Check if aggressive cleanup is needed
    pub fn needs_aggressive_cleanup(&self) -> bool {
        matches!(self, Self::Complete | Self::RunningCritical)
    }
}

/// Android memory statistics
#[derive(Debug, Clone)]
pub struct AndroidMemoryStats {
    /// Total device RAM in bytes
    pub total_ram: u64,
    /// Available RAM in bytes
    pub available_ram: u64,
    /// Low memory threshold in bytes
    pub low_memory_threshold: u64,
    /// App memory usage in bytes
    pub app_memory: u64,
    /// Native heap size in bytes
    pub native_heap: u64,
    /// Dalvik/ART heap size in bytes
    pub dalvik_heap: u64,
    /// Current trim level
    pub trim_level: Option<TrimLevel>,
    /// Timestamp
    pub timestamp: Instant,
}

impl AndroidMemoryStats {
    /// Check if device is low on memory
    pub fn is_low_memory(&self) -> bool {
        self.available_ram < self.low_memory_threshold
    }

    /// Get memory usage percentage
    pub fn usage_percentage(&self) -> f64 {
        if self.total_ram == 0 {
            return 0.0;
        }
        ((self.total_ram - self.available_ram) as f64 / self.total_ram as f64) * 100.0
    }

    /// Get recommended cache size
    pub fn recommended_cache_size(&self) -> usize {
        let base_size = 64 * 1024 * 1024; // 64 MB base

        if let Some(trim_level) = self.trim_level {
            let factor = trim_level.cache_reduction_factor();
            (base_size as f32 * factor) as usize
        } else {
            base_size
        }
    }
}

/// Android memory manager
pub struct AndroidMemoryManager {
    #[allow(dead_code)]
    api_level: AndroidApiLevel,
    performance_tier: PerformanceTier,
    current_stats: Arc<RwLock<Option<AndroidMemoryStats>>>,
}

impl AndroidMemoryManager {
    /// Create a new Android memory manager
    pub fn new(api_level: AndroidApiLevel, performance_tier: PerformanceTier) -> Self {
        Self {
            api_level,
            performance_tier,
            current_stats: Arc::new(RwLock::new(None)),
        }
    }

    /// Update memory statistics
    pub fn update_stats(&self) -> Result<()> {
        // In a real implementation, this would use Android APIs
        // (ActivityManager.MemoryInfo, Debug.MemoryInfo, etc.)
        let total_ram = match self.performance_tier {
            PerformanceTier::Low => 2 * 1024 * 1024 * 1024u64, // 2 GB
            PerformanceTier::Medium => 4 * 1024 * 1024 * 1024u64, // 4 GB
            PerformanceTier::High => 6 * 1024 * 1024 * 1024u64, // 6 GB
            PerformanceTier::Premium => 8 * 1024 * 1024 * 1024u64, // 8 GB
        };

        let available_ram = total_ram / 3; // Mock: 33% available
        let low_memory_threshold = total_ram / 10; // 10% threshold

        let stats = AndroidMemoryStats {
            total_ram,
            available_ram,
            low_memory_threshold,
            app_memory: 128 * 1024 * 1024, // 128 MB app usage
            native_heap: 64 * 1024 * 1024, // 64 MB native
            dalvik_heap: 64 * 1024 * 1024, // 64 MB dalvik
            trim_level: None,
            timestamp: Instant::now(),
        };

        *self.current_stats.write() = Some(stats);
        Ok(())
    }

    /// Get current memory stats
    pub fn current_stats(&self) -> Result<AndroidMemoryStats> {
        self.update_stats()?;

        let stats = self.current_stats.read();
        stats.clone().ok_or(MobileError::MemoryPressureError(
            "No memory stats available".to_string(),
        ))
    }

    /// Handle memory trim callback
    pub fn on_trim_memory(&self, level: TrimLevel) -> Result<TrimResponse> {
        let mut stats = self.current_stats.write();
        if let Some(ref mut s) = *stats {
            s.trim_level = Some(level);
        }

        Ok(TrimResponse {
            should_clear_caches: level.needs_aggressive_cleanup(),
            should_release_bitmaps: matches!(level, TrimLevel::Complete | TrimLevel::Moderate),
            should_gc: level.needs_aggressive_cleanup(),
            cache_reduction_factor: level.cache_reduction_factor(),
        })
    }

    /// Check if allocation is safe
    pub fn can_allocate(&self, size: usize) -> bool {
        if let Ok(stats) = self.current_stats() {
            // Check against available memory
            (size as u64) < stats.available_ram / 2 // Use at most half of available
        } else {
            // Be conservative if we can't check
            size < 16 * 1024 * 1024 // 16 MB
        }
    }

    /// Get recommended bitmap config
    pub fn recommended_bitmap_config(&self) -> BitmapConfig {
        if let Ok(stats) = self.current_stats() {
            if stats.is_low_memory() {
                BitmapConfig::RGB565 // Lower memory usage
            } else {
                BitmapConfig::ARGB8888 // Full quality
            }
        } else {
            BitmapConfig::ARGB8888
        }
    }

    /// Get recommended inSampleSize for image loading
    pub fn recommended_sample_size(&self, image_size: usize) -> u32 {
        if let Ok(stats) = self.current_stats() {
            if stats.is_low_memory() {
                // More aggressive downsampling
                if image_size > 4 * 1024 * 1024 {
                    4 // 1/16 size
                } else if image_size > 1024 * 1024 {
                    2 // 1/4 size
                } else {
                    1
                }
            } else {
                1 // No downsampling
            }
        } else {
            1
        }
    }

    /// Calculate optimal chunk size for processing
    pub fn optimal_chunk_size(&self, total_size: usize) -> usize {
        let budget = LowMemoryKiller::Foreground.memory_budget();

        if total_size <= budget {
            total_size
        } else {
            let chunks = total_size.div_ceil(budget);
            total_size / chunks
        }
    }
}

/// Response to memory trim
#[derive(Debug, Clone)]
pub struct TrimResponse {
    /// Should clear all caches
    pub should_clear_caches: bool,
    /// Should release bitmap memory
    pub should_release_bitmaps: bool,
    /// Should trigger garbage collection
    pub should_gc: bool,
    /// Cache reduction factor (0.0 - 1.0)
    pub cache_reduction_factor: f32,
}

/// Android bitmap configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitmapConfig {
    /// 32-bit color with alpha (4 bytes per pixel)
    ARGB8888,
    /// 16-bit color without alpha (2 bytes per pixel)
    RGB565,
    /// 8-bit color with alpha (2 bytes per pixel)
    ARGB4444,
    /// 8-bit alpha only (1 byte per pixel)
    ALPHA8,
}

impl BitmapConfig {
    /// Get bytes per pixel
    pub fn bytes_per_pixel(&self) -> usize {
        match self {
            Self::ARGB8888 => 4,
            Self::RGB565 | Self::ARGB4444 => 2,
            Self::ALPHA8 => 1,
        }
    }

    /// Calculate memory size for bitmap
    pub fn calculate_size(&self, width: u32, height: u32) -> usize {
        (width as usize) * (height as usize) * self.bytes_per_pixel()
    }
}

/// Dalvik/ART heap monitor
pub struct HeapMonitor {
    api_level: AndroidApiLevel,
}

impl HeapMonitor {
    /// Create new heap monitor
    pub fn new(api_level: AndroidApiLevel) -> Self {
        Self { api_level }
    }

    /// Check if large heap is available
    pub fn has_large_heap(&self) -> bool {
        // Large heap flag available on all supported versions
        true
    }

    /// Get recommended GC strategy
    pub fn gc_strategy(&self) -> GCStrategy {
        if self.api_level.is_at_least(26) {
            // ART with concurrent copying GC
            GCStrategy::ConcurrentCopying
        } else {
            // Older GC algorithms
            GCStrategy::ConcurrentMarkSweep
        }
    }
}

/// Garbage collection strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GCStrategy {
    /// Concurrent mark-sweep (older Android)
    ConcurrentMarkSweep,
    /// Concurrent copying (Android 8.0+)
    ConcurrentCopying,
}

impl GCStrategy {
    /// Check if strategy supports large objects well
    pub fn handles_large_objects_well(&self) -> bool {
        matches!(self, Self::ConcurrentCopying)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_low_memory_killer() {
        assert_eq!(LowMemoryKiller::Foreground.oom_score(), 0);
        assert_eq!(LowMemoryKiller::Cached.oom_score(), 15);

        assert!(
            LowMemoryKiller::Foreground.memory_budget() > LowMemoryKiller::Cached.memory_budget()
        );
    }

    #[test]
    fn test_trim_level() {
        assert!(TrimLevel::Complete.needs_aggressive_cleanup());
        assert!(!TrimLevel::UiHidden.needs_aggressive_cleanup());

        assert_eq!(TrimLevel::Complete.cache_reduction_factor(), 0.0);
        assert_eq!(TrimLevel::UiHidden.cache_reduction_factor(), 0.75);
    }

    #[test]
    fn test_memory_stats() {
        let stats = AndroidMemoryStats {
            total_ram: 4 * 1024 * 1024 * 1024,
            available_ram: 300 * 1024 * 1024, // Less than low_memory_threshold
            low_memory_threshold: 400 * 1024 * 1024,
            app_memory: 128 * 1024 * 1024,
            native_heap: 64 * 1024 * 1024,
            dalvik_heap: 64 * 1024 * 1024,
            trim_level: None,
            timestamp: Instant::now(),
        };

        assert!(stats.is_low_memory());
        assert!(stats.usage_percentage() > 80.0);
    }

    #[test]
    fn test_memory_manager() {
        let manager = AndroidMemoryManager::new(AndroidApiLevel::TIRAMISU, PerformanceTier::High);

        let stats = manager.current_stats().expect("Failed to get stats");
        assert!(stats.total_ram > 0);

        assert!(manager.can_allocate(1024 * 1024)); // 1 MB should be safe
    }

    #[test]
    fn test_trim_response() {
        let manager = AndroidMemoryManager::new(AndroidApiLevel::TIRAMISU, PerformanceTier::Medium);

        let response = manager
            .on_trim_memory(TrimLevel::Complete)
            .expect("Failed to handle trim");

        assert!(response.should_clear_caches);
        assert!(response.should_gc);
    }

    #[test]
    fn test_bitmap_config() {
        assert_eq!(BitmapConfig::ARGB8888.bytes_per_pixel(), 4);
        assert_eq!(BitmapConfig::RGB565.bytes_per_pixel(), 2);

        let size = BitmapConfig::ARGB8888.calculate_size(1920, 1080);
        assert_eq!(size, 1920 * 1080 * 4);
    }

    #[test]
    fn test_recommended_sample_size() {
        let manager = AndroidMemoryManager::new(AndroidApiLevel::TIRAMISU, PerformanceTier::Low);

        // Force low memory state
        let sample_size = manager.recommended_sample_size(5 * 1024 * 1024);
        assert!(sample_size >= 1);
    }

    #[test]
    fn test_heap_monitor() {
        let monitor = HeapMonitor::new(AndroidApiLevel::TIRAMISU);

        assert!(monitor.has_large_heap());
        assert_eq!(monitor.gc_strategy(), GCStrategy::ConcurrentCopying);

        let old_monitor = HeapMonitor::new(AndroidApiLevel::NOUGAT);
        assert_eq!(old_monitor.gc_strategy(), GCStrategy::ConcurrentMarkSweep);
    }

    #[test]
    fn test_gc_strategy() {
        assert!(GCStrategy::ConcurrentCopying.handles_large_objects_well());
        assert!(!GCStrategy::ConcurrentMarkSweep.handles_large_objects_well());
    }
}
