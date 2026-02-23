//! iOS memory management and pressure handling

use crate::error::{MobileError, Result};
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Instant;

/// Memory pressure level on iOS
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MemoryPressureLevel {
    /// Normal memory conditions
    Normal,
    /// Warning - some memory pressure
    Warning,
    /// Critical - severe memory pressure
    Critical,
}

impl MemoryPressureLevel {
    /// Get recommended cache reduction factor
    pub fn cache_reduction_factor(&self) -> f32 {
        match self {
            Self::Normal => 1.0,
            Self::Warning => 0.5,
            Self::Critical => 0.25,
        }
    }

    /// Check if large allocations should be avoided
    pub fn should_avoid_large_allocations(&self) -> bool {
        matches!(self, Self::Warning | Self::Critical)
    }

    /// Get maximum allocation size in bytes
    pub fn max_allocation_size(&self) -> usize {
        match self {
            Self::Normal => 64 * 1024 * 1024,  // 64 MB
            Self::Warning => 16 * 1024 * 1024, // 16 MB
            Self::Critical => 4 * 1024 * 1024, // 4 MB
        }
    }
}

/// Memory usage statistics
#[derive(Debug, Clone)]
pub struct MemoryStats {
    /// Total physical memory in bytes
    pub total_physical: u64,
    /// Available memory in bytes
    pub available: u64,
    /// Used memory in bytes
    pub used: u64,
    /// App memory usage in bytes
    pub app_usage: u64,
    /// Current pressure level
    pub pressure_level: MemoryPressureLevel,
    /// Timestamp of measurement
    pub timestamp: Instant,
}

impl MemoryStats {
    /// Get memory usage percentage
    pub fn usage_percentage(&self) -> f64 {
        if self.total_physical == 0 {
            return 0.0;
        }
        (self.used as f64 / self.total_physical as f64) * 100.0
    }

    /// Check if memory is running low
    pub fn is_low_memory(&self) -> bool {
        matches!(
            self.pressure_level,
            MemoryPressureLevel::Warning | MemoryPressureLevel::Critical
        )
    }

    /// Get recommended cache size
    pub fn recommended_cache_size(&self) -> usize {
        let base_size = 64 * 1024 * 1024; // 64 MB base
        let factor = self.pressure_level.cache_reduction_factor();
        (base_size as f32 * factor) as usize
    }
}

/// iOS memory manager
pub struct IOSMemoryManager {
    current_stats: Arc<RwLock<Option<MemoryStats>>>,
    warning_threshold: f64,
    critical_threshold: f64,
}

impl IOSMemoryManager {
    /// Create a new iOS memory manager
    pub fn new() -> Self {
        Self {
            current_stats: Arc::new(RwLock::new(None)),
            warning_threshold: 75.0,  // 75% usage
            critical_threshold: 90.0, // 90% usage
        }
    }

    /// Update memory statistics
    pub fn update_stats(&self) -> Result<()> {
        // In a real implementation, this would use iOS APIs to get memory info
        // For now, create mock statistics
        let total_physical = 4 * 1024 * 1024 * 1024u64; // 4 GB
        let used = 2 * 1024 * 1024 * 1024u64; // 2 GB
        let available = total_physical - used;
        let app_usage = 256 * 1024 * 1024u64; // 256 MB

        let usage_pct = (used as f64 / total_physical as f64) * 100.0;
        let pressure_level = if usage_pct >= self.critical_threshold {
            MemoryPressureLevel::Critical
        } else if usage_pct >= self.warning_threshold {
            MemoryPressureLevel::Warning
        } else {
            MemoryPressureLevel::Normal
        };

        let stats = MemoryStats {
            total_physical,
            available,
            used,
            app_usage,
            pressure_level,
            timestamp: Instant::now(),
        };

        *self.current_stats.write() = Some(stats);
        Ok(())
    }

    /// Get current memory stats
    pub fn current_stats(&self) -> Result<MemoryStats> {
        self.update_stats()?;

        let stats = self.current_stats.read();
        stats.clone().ok_or(MobileError::MemoryPressureError(
            "No memory stats available".to_string(),
        ))
    }

    /// Get current pressure level
    pub fn pressure_level(&self) -> Result<MemoryPressureLevel> {
        let stats = self.current_stats()?;
        Ok(stats.pressure_level)
    }

    /// Check if allocation is safe given current memory pressure
    pub fn can_allocate(&self, size: usize) -> bool {
        if let Ok(level) = self.pressure_level() {
            size <= level.max_allocation_size()
        } else {
            // If we can't determine pressure, be conservative
            size <= 16 * 1024 * 1024 // 16 MB
        }
    }

    /// Handle memory warning
    pub fn handle_memory_warning(&self) -> Result<MemoryWarningResponse> {
        let stats = self.current_stats()?;

        Ok(MemoryWarningResponse {
            should_clear_caches: true,
            should_reduce_quality: stats.pressure_level >= MemoryPressureLevel::Warning,
            should_suspend_background: stats.pressure_level >= MemoryPressureLevel::Critical,
            recommended_cache_size: stats.recommended_cache_size(),
        })
    }

    /// Get memory budget for operation
    pub fn memory_budget(&self) -> usize {
        if let Ok(level) = self.pressure_level() {
            level.max_allocation_size()
        } else {
            32 * 1024 * 1024 // 32 MB default
        }
    }

    /// Calculate optimal chunk size for processing
    pub fn optimal_chunk_size(&self, total_size: usize) -> usize {
        let budget = self.memory_budget();

        if total_size <= budget {
            total_size
        } else {
            // Split into reasonable chunks
            let chunks = total_size.div_ceil(budget);
            total_size / chunks
        }
    }
}

impl Default for IOSMemoryManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Response to memory warning
#[derive(Debug, Clone)]
pub struct MemoryWarningResponse {
    /// Should clear all non-essential caches
    pub should_clear_caches: bool,
    /// Should reduce processing quality
    pub should_reduce_quality: bool,
    /// Should suspend background tasks
    pub should_suspend_background: bool,
    /// Recommended cache size after cleanup
    pub recommended_cache_size: usize,
}

/// Memory pool for efficient allocations
pub struct IOSMemoryPool {
    manager: IOSMemoryManager,
    pool_size: usize,
    allocated: Arc<RwLock<usize>>,
}

impl IOSMemoryPool {
    /// Create a new memory pool
    pub fn new(pool_size: usize) -> Self {
        Self {
            manager: IOSMemoryManager::new(),
            pool_size,
            allocated: Arc::new(RwLock::new(0)),
        }
    }

    /// Try to allocate from pool
    pub fn allocate(&self, size: usize) -> Result<PoolAllocation> {
        // Check memory pressure
        if !self.manager.can_allocate(size) {
            return Err(MobileError::MemoryPressureError(format!(
                "Cannot allocate {} bytes under current memory pressure",
                size
            )));
        }

        let mut allocated = self.allocated.write();
        if *allocated + size > self.pool_size {
            return Err(MobileError::MemoryPressureError(format!(
                "Pool exhausted: {} + {} > {}",
                *allocated, size, self.pool_size
            )));
        }

        *allocated = allocated.saturating_add(size);

        Ok(PoolAllocation {
            size,
            pool: self.allocated.clone(),
        })
    }

    /// Get current pool usage
    pub fn usage(&self) -> usize {
        *self.allocated.read()
    }

    /// Get available space
    pub fn available(&self) -> usize {
        self.pool_size.saturating_sub(self.usage())
    }

    /// Get usage percentage
    pub fn usage_percentage(&self) -> f64 {
        (self.usage() as f64 / self.pool_size as f64) * 100.0
    }
}

/// RAII guard for pool allocation
pub struct PoolAllocation {
    size: usize,
    pool: Arc<RwLock<usize>>,
}

impl Drop for PoolAllocation {
    fn drop(&mut self) {
        let mut allocated = self.pool.write();
        *allocated = allocated.saturating_sub(self.size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_pressure_level() {
        assert_eq!(MemoryPressureLevel::Normal.cache_reduction_factor(), 1.0);
        assert_eq!(MemoryPressureLevel::Warning.cache_reduction_factor(), 0.5);
        assert_eq!(MemoryPressureLevel::Critical.cache_reduction_factor(), 0.25);

        assert!(!MemoryPressureLevel::Normal.should_avoid_large_allocations());
        assert!(MemoryPressureLevel::Critical.should_avoid_large_allocations());
    }

    #[test]
    fn test_memory_stats() {
        let stats = MemoryStats {
            total_physical: 4 * 1024 * 1024 * 1024,
            available: 1024 * 1024 * 1024,
            used: 3 * 1024 * 1024 * 1024,
            app_usage: 256 * 1024 * 1024,
            pressure_level: MemoryPressureLevel::Warning,
            timestamp: Instant::now(),
        };

        assert_eq!(stats.usage_percentage(), 75.0);
        assert!(stats.is_low_memory());
        assert!(stats.recommended_cache_size() < 64 * 1024 * 1024);
    }

    #[test]
    fn test_memory_manager() {
        let manager = IOSMemoryManager::new();

        let stats = manager.current_stats().expect("Failed to get stats");
        assert!(stats.total_physical > 0);

        let level = manager
            .pressure_level()
            .expect("Failed to get pressure level");
        assert!(matches!(
            level,
            MemoryPressureLevel::Normal
                | MemoryPressureLevel::Warning
                | MemoryPressureLevel::Critical
        ));

        assert!(manager.can_allocate(1024 * 1024)); // 1 MB should be safe
    }

    #[test]
    fn test_memory_warning_response() {
        let manager = IOSMemoryManager::new();
        let response = manager
            .handle_memory_warning()
            .expect("Failed to handle warning");

        assert!(response.should_clear_caches);
        assert!(response.recommended_cache_size > 0);
    }

    #[test]
    fn test_memory_pool() {
        let pool = IOSMemoryPool::new(10 * 1024 * 1024); // 10 MB pool

        let alloc1 = pool.allocate(1024 * 1024).expect("Allocation failed");
        assert_eq!(pool.usage(), 1024 * 1024);

        let alloc2 = pool.allocate(2 * 1024 * 1024).expect("Allocation failed");
        assert_eq!(pool.usage(), 3 * 1024 * 1024);

        drop(alloc1);
        assert_eq!(pool.usage(), 2 * 1024 * 1024);

        drop(alloc2);
        assert_eq!(pool.usage(), 0);
    }

    #[test]
    fn test_memory_pool_exhaustion() {
        let pool = IOSMemoryPool::new(1024); // 1 KB pool

        let _alloc = pool.allocate(512).expect("Allocation failed");
        let _alloc2 = pool.allocate(256).expect("Allocation failed");

        // This should fail
        let result = pool.allocate(512);
        assert!(result.is_err());
    }

    #[test]
    fn test_optimal_chunk_size() {
        let manager = IOSMemoryManager::new();

        let chunk_size = manager.optimal_chunk_size(100 * 1024 * 1024);
        assert!(chunk_size > 0);
        assert!(chunk_size <= manager.memory_budget());
    }
}
