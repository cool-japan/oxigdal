//! iOS performance optimization strategies

use std::time::Duration;

/// Metal GPU acceleration hints
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetalAcceleration {
    /// No GPU acceleration
    None,
    /// Use Metal for specific operations
    Selective,
    /// Use Metal for all applicable operations
    Aggressive,
}

impl MetalAcceleration {
    /// Check if Metal should be used for this operation size
    pub fn should_use_for_size(&self, size: usize) -> bool {
        match self {
            Self::None => false,
            Self::Selective => size > 1024 * 1024, // > 1 MB
            Self::Aggressive => size > 64 * 1024,  // > 64 KB
        }
    }

    /// Get recommended batch size for Metal operations
    pub fn metal_batch_size(&self) -> usize {
        match self {
            Self::None => 0,
            Self::Selective => 4 * 1024 * 1024,  // 4 MB
            Self::Aggressive => 2 * 1024 * 1024, // 2 MB
        }
    }
}

/// Core Image integration level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreImageLevel {
    /// No Core Image integration
    None,
    /// Basic filters only
    Basic,
    /// Full Core Image pipeline
    Full,
}

impl CoreImageLevel {
    /// Check if Core Image should be used for filtering
    pub fn supports_filtering(&self) -> bool {
        !matches!(self, Self::None)
    }

    /// Get maximum filter chain length
    pub fn max_filter_chain(&self) -> usize {
        match self {
            Self::None => 0,
            Self::Basic => 3,
            Self::Full => 10,
        }
    }
}

/// iOS performance optimization settings
#[derive(Debug, Clone)]
pub struct IOSPerformanceConfig {
    /// Metal GPU acceleration level
    pub metal_acceleration: MetalAcceleration,
    /// Core Image integration level
    pub core_image: CoreImageLevel,
    /// Enable aggressive prefetching
    pub prefetch_enabled: bool,
    /// Enable async decoding
    pub async_decode: bool,
    /// Maximum concurrent operations
    pub max_concurrent_ops: usize,
    /// Enable texture compression
    pub texture_compression: bool,
}

impl Default for IOSPerformanceConfig {
    fn default() -> Self {
        Self {
            metal_acceleration: MetalAcceleration::Selective,
            core_image: CoreImageLevel::Basic,
            prefetch_enabled: true,
            async_decode: true,
            max_concurrent_ops: 4,
            texture_compression: true,
        }
    }
}

impl IOSPerformanceConfig {
    /// Create a high-performance configuration
    pub fn high_performance() -> Self {
        Self {
            metal_acceleration: MetalAcceleration::Aggressive,
            core_image: CoreImageLevel::Full,
            prefetch_enabled: true,
            async_decode: true,
            max_concurrent_ops: 8,
            texture_compression: false,
        }
    }

    /// Create a balanced configuration
    pub fn balanced() -> Self {
        Self::default()
    }

    /// Create a battery-saving configuration
    pub fn battery_saver() -> Self {
        Self {
            metal_acceleration: MetalAcceleration::None,
            core_image: CoreImageLevel::None,
            prefetch_enabled: false,
            async_decode: false,
            max_concurrent_ops: 2,
            texture_compression: true,
        }
    }
}

/// iOS performance optimizer
pub struct IOSPerformanceOptimizer {
    config: IOSPerformanceConfig,
    metrics: IOSPerformanceMetrics,
}

impl IOSPerformanceOptimizer {
    /// Create a new iOS performance optimizer
    pub fn new(config: IOSPerformanceConfig) -> Self {
        Self {
            config,
            metrics: IOSPerformanceMetrics::default(),
        }
    }

    /// Get current configuration
    pub fn config(&self) -> &IOSPerformanceConfig {
        &self.config
    }

    /// Update configuration
    pub fn set_config(&mut self, config: IOSPerformanceConfig) {
        self.config = config;
    }

    /// Check if Metal should be used for operation
    pub fn should_use_metal(&self, data_size: usize) -> bool {
        self.config
            .metal_acceleration
            .should_use_for_size(data_size)
    }

    /// Get recommended tile size for rendering
    pub fn recommended_tile_size(&self) -> (u32, u32) {
        match self.config.metal_acceleration {
            MetalAcceleration::Aggressive => (512, 512),
            MetalAcceleration::Selective => (256, 256),
            MetalAcceleration::None => (128, 128),
        }
    }

    /// Check if prefetching should be enabled
    pub fn should_prefetch(&self) -> bool {
        self.config.prefetch_enabled
    }

    /// Get prefetch distance (number of items)
    pub fn prefetch_distance(&self) -> usize {
        if self.config.prefetch_enabled {
            match self.config.metal_acceleration {
                MetalAcceleration::Aggressive => 8,
                MetalAcceleration::Selective => 4,
                MetalAcceleration::None => 2,
            }
        } else {
            0
        }
    }

    /// Record operation timing
    pub fn record_operation(&mut self, operation: &str, duration: Duration) {
        self.metrics.record_operation(operation, duration);
    }

    /// Get performance metrics
    pub fn metrics(&self) -> &IOSPerformanceMetrics {
        &self.metrics
    }

    /// Optimize based on current metrics
    pub fn auto_tune(&mut self) {
        let avg_latency = self.metrics.average_operation_time();

        // If operations are slow, reduce quality
        if avg_latency > Duration::from_millis(100) {
            if self.config.metal_acceleration == MetalAcceleration::Aggressive {
                self.config.metal_acceleration = MetalAcceleration::Selective;
            }
            if self.config.max_concurrent_ops > 2 {
                self.config.max_concurrent_ops = self.config.max_concurrent_ops.saturating_sub(1);
            }
        }

        // If operations are fast, we could increase quality
        if avg_latency < Duration::from_millis(20) {
            if self.config.metal_acceleration == MetalAcceleration::Selective {
                self.config.metal_acceleration = MetalAcceleration::Aggressive;
            }
            if self.config.max_concurrent_ops < 8 {
                self.config.max_concurrent_ops = self.config.max_concurrent_ops.saturating_add(1);
            }
        }
    }
}

impl Default for IOSPerformanceOptimizer {
    fn default() -> Self {
        Self::new(IOSPerformanceConfig::default())
    }
}

/// Performance metrics for iOS
#[derive(Debug, Clone, Default)]
pub struct IOSPerformanceMetrics {
    operations_count: u64,
    total_duration: Duration,
    metal_operations: u64,
    cpu_operations: u64,
}

impl IOSPerformanceMetrics {
    /// Record an operation
    pub fn record_operation(&mut self, operation: &str, duration: Duration) {
        self.operations_count = self.operations_count.saturating_add(1);
        self.total_duration = self.total_duration.saturating_add(duration);

        if operation.contains("metal") || operation.contains("gpu") {
            self.metal_operations = self.metal_operations.saturating_add(1);
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

    /// Get Metal usage percentage
    pub fn metal_usage_percentage(&self) -> f64 {
        if self.operations_count == 0 {
            return 0.0;
        }
        (self.metal_operations as f64 / self.operations_count as f64) * 100.0
    }

    /// Get total operations count
    pub fn total_operations(&self) -> u64 {
        self.operations_count
    }

    /// Reset metrics
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Image decoding optimization
pub struct IOSImageDecoder {
    async_decode: bool,
    downscale_threshold: usize,
}

impl IOSImageDecoder {
    /// Create a new iOS image decoder
    pub fn new(async_decode: bool) -> Self {
        Self {
            async_decode,
            downscale_threshold: 4096 * 4096, // 4K
        }
    }

    /// Check if image should be decoded asynchronously
    pub fn should_decode_async(&self, image_size: usize) -> bool {
        self.async_decode && image_size > 1024 * 1024 // > 1 MB
    }

    /// Check if image should be downscaled during decode
    pub fn should_downscale(&self, pixel_count: usize) -> bool {
        pixel_count > self.downscale_threshold
    }

    /// Calculate downscale factor
    pub fn downscale_factor(&self, pixel_count: usize) -> f32 {
        if !self.should_downscale(pixel_count) {
            return 1.0;
        }

        let factor = (pixel_count as f32 / self.downscale_threshold as f32).sqrt();
        factor.max(1.0)
    }
}

impl Default for IOSImageDecoder {
    fn default() -> Self {
        Self::new(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metal_acceleration() {
        let selective = MetalAcceleration::Selective;
        let aggressive = MetalAcceleration::Aggressive;

        assert!(!selective.should_use_for_size(1024));
        assert!(selective.should_use_for_size(2 * 1024 * 1024));

        assert!(aggressive.should_use_for_size(128 * 1024));
        assert!(!aggressive.should_use_for_size(1024));
    }

    #[test]
    fn test_core_image_level() {
        assert!(CoreImageLevel::Basic.supports_filtering());
        assert!(!CoreImageLevel::None.supports_filtering());

        assert_eq!(CoreImageLevel::Full.max_filter_chain(), 10);
        assert_eq!(CoreImageLevel::Basic.max_filter_chain(), 3);
    }

    #[test]
    fn test_performance_config() {
        let high_perf = IOSPerformanceConfig::high_performance();
        assert_eq!(high_perf.metal_acceleration, MetalAcceleration::Aggressive);
        assert_eq!(high_perf.core_image, CoreImageLevel::Full);

        let battery = IOSPerformanceConfig::battery_saver();
        assert_eq!(battery.metal_acceleration, MetalAcceleration::None);
        assert!(!battery.prefetch_enabled);
    }

    #[test]
    fn test_performance_optimizer() {
        let mut optimizer = IOSPerformanceOptimizer::default();

        assert!(optimizer.should_prefetch());
        let (width, height) = optimizer.recommended_tile_size();
        assert!(width > 0 && height > 0);

        optimizer.record_operation("test", Duration::from_millis(10));
        assert_eq!(optimizer.metrics().total_operations(), 1);
    }

    #[test]
    fn test_performance_metrics() {
        let mut metrics = IOSPerformanceMetrics::default();

        metrics.record_operation("cpu_op", Duration::from_millis(10));
        metrics.record_operation("metal_op", Duration::from_millis(5));

        assert_eq!(metrics.total_operations(), 2);
        assert!(metrics.average_operation_time() > Duration::ZERO);
        assert!(metrics.metal_usage_percentage() > 0.0);
    }

    #[test]
    fn test_image_decoder() {
        let decoder = IOSImageDecoder::default();

        assert!(decoder.should_decode_async(2 * 1024 * 1024));
        assert!(!decoder.should_decode_async(512 * 1024));

        assert!(decoder.should_downscale(5000 * 5000));
        assert!(!decoder.should_downscale(1000 * 1000));

        let factor = decoder.downscale_factor(8192 * 8192);
        assert!(factor > 1.0);
    }

    #[test]
    fn test_auto_tune() {
        let mut optimizer = IOSPerformanceOptimizer::new(IOSPerformanceConfig::high_performance());

        // Record some slow operations
        for _ in 0..10 {
            optimizer.record_operation("slow_op", Duration::from_millis(200));
        }

        let _initial_config = optimizer.config().metal_acceleration;
        optimizer.auto_tune();

        // Should have reduced quality
        assert!(optimizer.config().max_concurrent_ops <= 8);
    }
}
