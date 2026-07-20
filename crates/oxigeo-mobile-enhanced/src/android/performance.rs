//! Android performance optimization strategies

use super::{AndroidApiLevel, LifecycleState, PerformanceTier};
use std::time::Duration;

/// RenderScript acceleration level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderScriptLevel {
    /// No RenderScript usage (deprecated API)
    None,
    /// Use RenderScript for specific operations
    Selective,
    /// Use RenderScript aggressively
    Aggressive,
}

impl RenderScriptLevel {
    /// Check if RenderScript should be used for operation size
    pub fn should_use_for_size(&self, size: usize) -> bool {
        match self {
            Self::None => false,
            Self::Selective => size > 2 * 1024 * 1024, // > 2 MB
            Self::Aggressive => size > 512 * 1024,     // > 512 KB
        }
    }

    /// Get recommended for API level
    pub fn for_api_level(level: AndroidApiLevel) -> Self {
        if level.is_at_least(31) {
            // RenderScript deprecated in API 31, prefer Vulkan/OpenGL
            Self::None
        } else {
            Self::Selective
        }
    }
}

/// Vulkan/OpenGL acceleration level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuAcceleration {
    /// No GPU acceleration
    None,
    /// OpenGL ES acceleration
    OpenGL,
    /// Vulkan acceleration (preferred)
    Vulkan,
}

impl GpuAcceleration {
    /// Get recommended for performance tier
    pub fn for_performance_tier(tier: PerformanceTier) -> Self {
        match tier {
            PerformanceTier::Low => Self::None,
            PerformanceTier::Medium => Self::OpenGL,
            PerformanceTier::High | PerformanceTier::Premium => Self::Vulkan,
        }
    }

    /// Check if GPU should be used
    pub fn is_enabled(&self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Android performance configuration
#[derive(Debug, Clone)]
pub struct AndroidPerformanceConfig {
    /// RenderScript level (deprecated but still used)
    pub renderscript: RenderScriptLevel,
    /// GPU acceleration level
    pub gpu_acceleration: GpuAcceleration,
    /// Enable aggressive prefetching
    pub prefetch_enabled: bool,
    /// Maximum concurrent operations
    pub max_concurrent_ops: usize,
    /// Enable hardware bitmap decoding
    pub hardware_bitmaps: bool,
    /// Current lifecycle state
    pub lifecycle_state: LifecycleState,
}

impl Default for AndroidPerformanceConfig {
    fn default() -> Self {
        Self {
            renderscript: RenderScriptLevel::None,
            gpu_acceleration: GpuAcceleration::OpenGL,
            prefetch_enabled: true,
            max_concurrent_ops: 4,
            hardware_bitmaps: true,
            lifecycle_state: LifecycleState::Foreground,
        }
    }
}

impl AndroidPerformanceConfig {
    /// Create high-performance configuration
    pub fn high_performance(tier: PerformanceTier) -> Self {
        Self {
            renderscript: RenderScriptLevel::None, // Deprecated
            gpu_acceleration: GpuAcceleration::for_performance_tier(tier),
            prefetch_enabled: true,
            max_concurrent_ops: tier.max_concurrent_operations(),
            hardware_bitmaps: true,
            lifecycle_state: LifecycleState::Foreground,
        }
    }

    /// Create balanced configuration
    pub fn balanced(tier: PerformanceTier) -> Self {
        Self {
            renderscript: RenderScriptLevel::None,
            gpu_acceleration: GpuAcceleration::OpenGL,
            prefetch_enabled: true,
            max_concurrent_ops: tier.max_concurrent_operations() / 2,
            hardware_bitmaps: true,
            lifecycle_state: LifecycleState::Foreground,
        }
    }

    /// Create battery-saving configuration
    pub fn battery_saver() -> Self {
        Self {
            renderscript: RenderScriptLevel::None,
            gpu_acceleration: GpuAcceleration::None,
            prefetch_enabled: false,
            max_concurrent_ops: 1,
            hardware_bitmaps: false,
            lifecycle_state: LifecycleState::Background,
        }
    }

    /// Update for lifecycle change
    pub fn update_for_lifecycle(&mut self, state: LifecycleState) {
        self.lifecycle_state = state;

        match state {
            LifecycleState::Foreground => {
                // Full performance
                self.prefetch_enabled = true;
            }
            LifecycleState::Visible => {
                // Slightly reduced
                self.prefetch_enabled = true;
            }
            LifecycleState::Background => {
                // Minimal processing
                self.prefetch_enabled = false;
                self.max_concurrent_ops = 1;
            }
            LifecycleState::Stopped => {
                // No processing
                self.max_concurrent_ops = 0;
            }
        }
    }
}

/// Android performance optimizer
pub struct AndroidPerformanceOptimizer {
    config: AndroidPerformanceConfig,
    metrics: AndroidPerformanceMetrics,
}

impl AndroidPerformanceOptimizer {
    /// Create a new Android performance optimizer
    pub fn new(config: AndroidPerformanceConfig) -> Self {
        Self {
            config,
            metrics: AndroidPerformanceMetrics::default(),
        }
    }

    /// Get current configuration
    pub fn config(&self) -> &AndroidPerformanceConfig {
        &self.config
    }

    /// Update configuration
    pub fn set_config(&mut self, config: AndroidPerformanceConfig) {
        self.config = config;
    }

    /// Handle lifecycle state change
    pub fn on_lifecycle_changed(&mut self, state: LifecycleState) {
        self.config.update_for_lifecycle(state);
    }

    /// Check if GPU should be used
    pub fn should_use_gpu(&self, data_size: usize) -> bool {
        if !self.config.gpu_acceleration.is_enabled() {
            return false;
        }

        // Only use GPU for larger operations in foreground
        self.config.lifecycle_state.allows_intensive_operations() && data_size > 1024 * 1024 // > 1 MB
    }

    /// Get recommended tile size
    pub fn recommended_tile_size(&self) -> (u32, u32) {
        match self.config.gpu_acceleration {
            GpuAcceleration::Vulkan => (512, 512),
            GpuAcceleration::OpenGL => (256, 256),
            GpuAcceleration::None => (128, 128),
        }
    }

    /// Check if prefetching should be enabled
    pub fn should_prefetch(&self) -> bool {
        self.config.prefetch_enabled && self.config.lifecycle_state.allows_intensive_operations()
    }

    /// Get prefetch distance
    pub fn prefetch_distance(&self) -> usize {
        if !self.should_prefetch() {
            return 0;
        }

        match self.config.gpu_acceleration {
            GpuAcceleration::Vulkan => 8,
            GpuAcceleration::OpenGL => 4,
            GpuAcceleration::None => 2,
        }
    }

    /// Record operation timing
    pub fn record_operation(&mut self, operation: &str, duration: Duration) {
        self.metrics.record_operation(operation, duration);
    }

    /// Get performance metrics
    pub fn metrics(&self) -> &AndroidPerformanceMetrics {
        &self.metrics
    }

    /// Auto-tune based on metrics
    pub fn auto_tune(&mut self) {
        let avg_latency = self.metrics.average_operation_time();

        // Adjust based on performance
        if avg_latency > Duration::from_millis(150) {
            // Reduce quality
            if self.config.max_concurrent_ops > 1 {
                self.config.max_concurrent_ops = self.config.max_concurrent_ops.saturating_sub(1);
            }
            if self.config.gpu_acceleration == GpuAcceleration::Vulkan {
                self.config.gpu_acceleration = GpuAcceleration::OpenGL;
            }
        } else if avg_latency < Duration::from_millis(30) {
            // Increase quality
            if self.config.max_concurrent_ops < 8 {
                self.config.max_concurrent_ops = self.config.max_concurrent_ops.saturating_add(1);
            }
        }
    }

    /// Check if hardware bitmaps should be used
    pub fn should_use_hardware_bitmaps(&self) -> bool {
        self.config.hardware_bitmaps && self.config.lifecycle_state == LifecycleState::Foreground
    }
}

impl Default for AndroidPerformanceOptimizer {
    fn default() -> Self {
        Self::new(AndroidPerformanceConfig::default())
    }
}

/// Android performance metrics
#[derive(Debug, Clone, Default)]
pub struct AndroidPerformanceMetrics {
    operations_count: u64,
    total_duration: Duration,
    gpu_operations: u64,
    cpu_operations: u64,
    renderscript_operations: u64,
}

impl AndroidPerformanceMetrics {
    /// Record an operation
    pub fn record_operation(&mut self, operation: &str, duration: Duration) {
        self.operations_count = self.operations_count.saturating_add(1);
        self.total_duration = self.total_duration.saturating_add(duration);

        if operation.contains("gpu") || operation.contains("vulkan") || operation.contains("opengl")
        {
            self.gpu_operations = self.gpu_operations.saturating_add(1);
        } else if operation.contains("renderscript") {
            self.renderscript_operations = self.renderscript_operations.saturating_add(1);
        } else {
            self.cpu_operations = self.cpu_operations.saturating_add(1);
        }
    }

    /// Get average operation time
    pub fn average_operation_time(&self) -> Duration {
        if self.operations_count == 0 {
            return Duration::ZERO;
        }
        self.total_duration / self.operations_count as u32
    }

    /// Get GPU usage percentage
    pub fn gpu_usage_percentage(&self) -> f64 {
        if self.operations_count == 0 {
            return 0.0;
        }
        (self.gpu_operations as f64 / self.operations_count as f64) * 100.0
    }

    /// Get total operations
    pub fn total_operations(&self) -> u64 {
        self.operations_count
    }

    /// Reset metrics
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// ART (Android Runtime) optimization hints
pub struct ARTOptimizer {
    api_level: AndroidApiLevel,
}

impl ARTOptimizer {
    /// Create new ART optimizer
    pub fn new(api_level: AndroidApiLevel) -> Self {
        Self { api_level }
    }

    /// Check if JIT compilation is available
    pub fn has_jit(&self) -> bool {
        // ART with JIT available in Android 7.0+
        self.api_level.is_at_least(24)
    }

    /// Check if profile-guided optimization is available
    pub fn has_pgo(&self) -> bool {
        // Profile-guided optimization improved in Android 8.0+
        self.api_level.is_at_least(26)
    }

    /// Get recommended warmup iterations
    pub fn warmup_iterations(&self) -> usize {
        if self.has_jit() {
            10 // Fewer iterations needed with JIT
        } else {
            50 // More iterations for older versions
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renderscript_level() {
        let selective = RenderScriptLevel::Selective;
        assert!(!selective.should_use_for_size(1024 * 1024));
        assert!(selective.should_use_for_size(3 * 1024 * 1024));

        let api33 = AndroidApiLevel::TIRAMISU;
        assert_eq!(
            RenderScriptLevel::for_api_level(api33),
            RenderScriptLevel::None
        );
    }

    #[test]
    fn test_gpu_acceleration() {
        let gpu = GpuAcceleration::for_performance_tier(PerformanceTier::High);
        assert_eq!(gpu, GpuAcceleration::Vulkan);

        let gpu = GpuAcceleration::for_performance_tier(PerformanceTier::Low);
        assert_eq!(gpu, GpuAcceleration::None);

        assert!(GpuAcceleration::Vulkan.is_enabled());
        assert!(!GpuAcceleration::None.is_enabled());
    }

    #[test]
    fn test_performance_config() {
        let high_perf = AndroidPerformanceConfig::high_performance(PerformanceTier::Premium);
        assert_eq!(high_perf.gpu_acceleration, GpuAcceleration::Vulkan);
        assert!(high_perf.prefetch_enabled);

        let battery = AndroidPerformanceConfig::battery_saver();
        assert_eq!(battery.gpu_acceleration, GpuAcceleration::None);
        assert!(!battery.prefetch_enabled);
    }

    #[test]
    fn test_lifecycle_update() {
        let mut config = AndroidPerformanceConfig::default();

        config.update_for_lifecycle(LifecycleState::Background);
        assert!(!config.prefetch_enabled);
        assert_eq!(config.max_concurrent_ops, 1);

        config.update_for_lifecycle(LifecycleState::Foreground);
        assert!(config.prefetch_enabled);
    }

    #[test]
    fn test_performance_optimizer() {
        let mut optimizer = AndroidPerformanceOptimizer::default();

        assert!(optimizer.should_prefetch());

        optimizer.on_lifecycle_changed(LifecycleState::Background);
        assert!(!optimizer.should_prefetch());

        optimizer.record_operation("test", Duration::from_millis(10));
        assert_eq!(optimizer.metrics().total_operations(), 1);
    }

    #[test]
    fn test_performance_metrics() {
        let mut metrics = AndroidPerformanceMetrics::default();

        metrics.record_operation("cpu_op", Duration::from_millis(10));
        metrics.record_operation("gpu_op", Duration::from_millis(5));

        assert_eq!(metrics.total_operations(), 2);
        assert!(metrics.average_operation_time() > Duration::ZERO);
        assert!(metrics.gpu_usage_percentage() > 0.0);
    }

    #[test]
    fn test_art_optimizer() {
        let art = ARTOptimizer::new(AndroidApiLevel::TIRAMISU);

        assert!(art.has_jit());
        assert!(art.has_pgo());
        assert_eq!(art.warmup_iterations(), 10);
    }

    #[test]
    fn test_auto_tune() {
        let mut optimizer = AndroidPerformanceOptimizer::new(
            AndroidPerformanceConfig::high_performance(PerformanceTier::Premium),
        );

        // Record slow operations
        for _ in 0..10 {
            optimizer.record_operation("slow_op", Duration::from_millis(200));
        }

        let initial_ops = optimizer.config().max_concurrent_ops;
        optimizer.auto_tune();

        // Should have reduced concurrent ops
        assert!(optimizer.config().max_concurrent_ops <= initial_ops);
    }
}
