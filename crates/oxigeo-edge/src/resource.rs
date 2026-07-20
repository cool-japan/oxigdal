//! Resource management for constrained edge devices
//!
//! Monitors and manages CPU, memory, and storage resources to ensure
//! efficient operation on resource-limited devices.

use crate::error::{EdgeError, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Resource constraints for edge devices
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceConstraints {
    /// Maximum memory usage in bytes
    pub max_memory_bytes: usize,
    /// Maximum CPU usage percentage (0-100)
    pub max_cpu_percent: f64,
    /// Maximum storage usage in bytes
    pub max_storage_bytes: usize,
    /// Maximum concurrent operations
    pub max_concurrent_ops: usize,
    /// Operation timeout in seconds
    pub operation_timeout_secs: u64,
}

impl Default for ResourceConstraints {
    fn default() -> Self {
        Self {
            max_memory_bytes: crate::MAX_MEMORY_USAGE,
            max_cpu_percent: 80.0,
            max_storage_bytes: 100 * 1024 * 1024, // 100 MB
            max_concurrent_ops: 10,
            operation_timeout_secs: 30,
        }
    }
}

impl ResourceConstraints {
    /// Create constraints for minimal embedded devices
    pub fn minimal() -> Self {
        Self {
            max_memory_bytes: 10 * 1024 * 1024, // 10 MB
            max_cpu_percent: 50.0,
            max_storage_bytes: 10 * 1024 * 1024, // 10 MB
            max_concurrent_ops: 3,
            operation_timeout_secs: 10,
        }
    }

    /// Create constraints for moderate edge devices
    pub fn moderate() -> Self {
        Self {
            max_memory_bytes: 50 * 1024 * 1024, // 50 MB
            max_cpu_percent: 70.0,
            max_storage_bytes: 100 * 1024 * 1024, // 100 MB
            max_concurrent_ops: 10,
            operation_timeout_secs: 30,
        }
    }

    /// Create constraints for powerful edge devices
    pub fn powerful() -> Self {
        Self {
            max_memory_bytes: 200 * 1024 * 1024, // 200 MB
            max_cpu_percent: 90.0,
            max_storage_bytes: 500 * 1024 * 1024, // 500 MB
            max_concurrent_ops: 50,
            operation_timeout_secs: 60,
        }
    }

    /// Validate constraints
    pub fn validate(&self) -> Result<()> {
        if self.max_memory_bytes == 0 {
            return Err(EdgeError::invalid_config("max_memory_bytes must be > 0"));
        }
        if self.max_cpu_percent <= 0.0 || self.max_cpu_percent > 100.0 {
            return Err(EdgeError::invalid_config(
                "max_cpu_percent must be between 0 and 100",
            ));
        }
        if self.max_storage_bytes == 0 {
            return Err(EdgeError::invalid_config("max_storage_bytes must be > 0"));
        }
        if self.max_concurrent_ops == 0 {
            return Err(EdgeError::invalid_config("max_concurrent_ops must be > 0"));
        }
        Ok(())
    }
}

/// Resource usage metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMetrics {
    /// Current memory usage in bytes
    pub memory_bytes: usize,
    /// Current CPU usage percentage
    pub cpu_percent: f64,
    /// Current storage usage in bytes
    pub storage_bytes: usize,
    /// Number of active operations
    pub active_operations: usize,
    /// Peak memory usage
    pub peak_memory_bytes: usize,
    /// Peak CPU usage
    pub peak_cpu_percent: f64,
    /// Total operations completed
    pub total_operations: u64,
    /// Failed operations count
    pub failed_operations: u64,
}

impl Default for ResourceMetrics {
    fn default() -> Self {
        Self {
            memory_bytes: 0,
            cpu_percent: 0.0,
            storage_bytes: 0,
            active_operations: 0,
            peak_memory_bytes: 0,
            peak_cpu_percent: 0.0,
            total_operations: 0,
            failed_operations: 0,
        }
    }
}

impl ResourceMetrics {
    /// Get memory usage percentage
    pub fn memory_percent(&self, max_memory: usize) -> f64 {
        if max_memory == 0 {
            return 0.0;
        }
        (self.memory_bytes as f64 / max_memory as f64) * 100.0
    }

    /// Get storage usage percentage
    pub fn storage_percent(&self, max_storage: usize) -> f64 {
        if max_storage == 0 {
            return 0.0;
        }
        (self.storage_bytes as f64 / max_storage as f64) * 100.0
    }

    /// Get success rate
    pub fn success_rate(&self) -> f64 {
        if self.total_operations == 0 {
            return 100.0;
        }
        let successful = self.total_operations - self.failed_operations;
        (successful as f64 / self.total_operations as f64) * 100.0
    }
}

/// Resource manager for edge devices
pub struct ResourceManager {
    constraints: ResourceConstraints,
    memory_used: Arc<AtomicUsize>,
    storage_used: Arc<AtomicUsize>,
    active_ops: Arc<AtomicUsize>,
    total_ops: Arc<AtomicU64>,
    failed_ops: Arc<AtomicU64>,
    peak_memory: Arc<AtomicUsize>,
    cpu_samples: Arc<RwLock<Vec<f64>>>,
}

impl ResourceManager {
    /// Create new resource manager
    pub fn new(constraints: ResourceConstraints) -> Result<Self> {
        constraints.validate()?;

        Ok(Self {
            constraints,
            memory_used: Arc::new(AtomicUsize::new(0)),
            storage_used: Arc::new(AtomicUsize::new(0)),
            active_ops: Arc::new(AtomicUsize::new(0)),
            total_ops: Arc::new(AtomicU64::new(0)),
            failed_ops: Arc::new(AtomicU64::new(0)),
            peak_memory: Arc::new(AtomicUsize::new(0)),
            cpu_samples: Arc::new(RwLock::new(Vec::new())),
        })
    }

    /// Check if operation can be started
    pub fn can_start_operation(&self) -> Result<()> {
        // Check concurrent operations limit
        let active = self.active_ops.load(Ordering::Relaxed);
        if active >= self.constraints.max_concurrent_ops {
            return Err(EdgeError::resource_constraint(format!(
                "Maximum concurrent operations ({}) reached",
                self.constraints.max_concurrent_ops
            )));
        }

        // Check memory constraint
        let memory = self.memory_used.load(Ordering::Relaxed);
        if memory >= self.constraints.max_memory_bytes {
            return Err(EdgeError::resource_constraint(format!(
                "Memory limit ({} bytes) exceeded",
                self.constraints.max_memory_bytes
            )));
        }

        Ok(())
    }

    /// Start an operation
    pub fn start_operation(&self) -> Result<OperationGuard> {
        self.can_start_operation()?;

        self.active_ops.fetch_add(1, Ordering::Relaxed);
        self.total_ops.fetch_add(1, Ordering::Relaxed);

        Ok(OperationGuard {
            active_ops: Arc::clone(&self.active_ops),
        })
    }

    /// Record operation failure
    pub fn record_failure(&self) {
        self.failed_ops.fetch_add(1, Ordering::Relaxed);
    }

    /// Allocate memory
    pub fn allocate_memory(&self, bytes: usize) -> Result<MemoryGuard> {
        let current = self.memory_used.load(Ordering::Relaxed);
        let new_total = current.saturating_add(bytes);

        if new_total > self.constraints.max_memory_bytes {
            return Err(EdgeError::resource_constraint(format!(
                "Memory allocation of {} bytes would exceed limit of {} bytes",
                bytes, self.constraints.max_memory_bytes
            )));
        }

        self.memory_used.fetch_add(bytes, Ordering::Relaxed);

        // Update peak memory
        let mut peak = self.peak_memory.load(Ordering::Relaxed);
        while new_total > peak {
            match self.peak_memory.compare_exchange_weak(
                peak,
                new_total,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(current) => peak = current,
            }
        }

        Ok(MemoryGuard {
            bytes,
            memory_used: Arc::clone(&self.memory_used),
        })
    }

    /// Allocate storage
    pub fn allocate_storage(&self, bytes: usize) -> Result<StorageGuard> {
        let current = self.storage_used.load(Ordering::Relaxed);
        let new_total = current.saturating_add(bytes);

        if new_total > self.constraints.max_storage_bytes {
            return Err(EdgeError::resource_constraint(format!(
                "Storage allocation of {} bytes would exceed limit of {} bytes",
                bytes, self.constraints.max_storage_bytes
            )));
        }

        self.storage_used.fetch_add(bytes, Ordering::Relaxed);

        Ok(StorageGuard {
            bytes,
            storage_used: Arc::clone(&self.storage_used),
        })
    }

    /// Record CPU sample
    pub fn record_cpu_sample(&self, cpu_percent: f64) {
        let mut samples = self.cpu_samples.write();
        samples.push(cpu_percent);

        // Keep only last 100 samples
        if samples.len() > 100 {
            samples.remove(0);
        }
    }

    /// Get current CPU usage (averaged over recent samples)
    pub fn current_cpu(&self) -> f64 {
        let samples = self.cpu_samples.read();
        if samples.is_empty() {
            return 0.0;
        }

        let sum: f64 = samples.iter().sum();
        sum / samples.len() as f64
    }

    /// Check if CPU is overloaded
    pub fn is_cpu_overloaded(&self) -> bool {
        self.current_cpu() > self.constraints.max_cpu_percent
    }

    /// Get current metrics
    pub fn metrics(&self) -> ResourceMetrics {
        let memory = self.memory_used.load(Ordering::Relaxed);
        let storage = self.storage_used.load(Ordering::Relaxed);
        let active_ops = self.active_ops.load(Ordering::Relaxed);
        let total_ops = self.total_ops.load(Ordering::Relaxed);
        let failed_ops = self.failed_ops.load(Ordering::Relaxed);
        let peak_memory = self.peak_memory.load(Ordering::Relaxed);

        let cpu_samples = self.cpu_samples.read();
        let (cpu_current, cpu_peak) = if cpu_samples.is_empty() {
            (0.0, 0.0)
        } else {
            let sum: f64 = cpu_samples.iter().sum();
            let avg = sum / cpu_samples.len() as f64;
            let peak = cpu_samples.iter().copied().fold(0.0, f64::max);
            (avg, peak)
        };

        ResourceMetrics {
            memory_bytes: memory,
            cpu_percent: cpu_current,
            storage_bytes: storage,
            active_operations: active_ops,
            peak_memory_bytes: peak_memory,
            peak_cpu_percent: cpu_peak,
            total_operations: total_ops,
            failed_operations: failed_ops,
        }
    }

    /// Get constraints
    pub fn constraints(&self) -> &ResourceConstraints {
        &self.constraints
    }

    /// Reset metrics
    pub fn reset_metrics(&self) {
        self.total_ops.store(0, Ordering::Relaxed);
        self.failed_ops.store(0, Ordering::Relaxed);
        self.peak_memory.store(0, Ordering::Relaxed);
        self.cpu_samples.write().clear();
    }

    /// Check overall health
    pub fn health_check(&self) -> HealthStatus {
        let metrics = self.metrics();

        let memory_ok = metrics.memory_percent(self.constraints.max_memory_bytes) < 90.0;
        let cpu_ok = metrics.cpu_percent < self.constraints.max_cpu_percent * 0.9;
        let storage_ok = metrics.storage_percent(self.constraints.max_storage_bytes) < 90.0;
        let success_ok = metrics.success_rate() > 95.0;

        if memory_ok && cpu_ok && storage_ok && success_ok {
            HealthStatus::Healthy
        } else if !memory_ok || !cpu_ok || !storage_ok {
            HealthStatus::Critical
        } else {
            HealthStatus::Degraded
        }
    }
}

/// Health status of edge device
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    /// All resources operating normally
    Healthy,
    /// Some resources under pressure but operational
    Degraded,
    /// Critical resource constraints
    Critical,
}

/// RAII guard for operations
pub struct OperationGuard {
    active_ops: Arc<AtomicUsize>,
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        self.active_ops.fetch_sub(1, Ordering::Relaxed);
    }
}

/// RAII guard for memory allocation
pub struct MemoryGuard {
    bytes: usize,
    memory_used: Arc<AtomicUsize>,
}

impl Drop for MemoryGuard {
    fn drop(&mut self) {
        self.memory_used.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}

/// RAII guard for storage allocation
pub struct StorageGuard {
    bytes: usize,
    storage_used: Arc<AtomicUsize>,
}

impl Drop for StorageGuard {
    fn drop(&mut self) {
        self.storage_used.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constraints_creation() {
        let constraints = ResourceConstraints::default();
        assert!(constraints.validate().is_ok());

        let minimal = ResourceConstraints::minimal();
        assert!(minimal.validate().is_ok());
        assert!(minimal.max_memory_bytes < constraints.max_memory_bytes);
    }

    #[test]
    fn test_constraints_validation() {
        let mut invalid = ResourceConstraints {
            max_memory_bytes: 0,
            ..Default::default()
        };
        assert!(invalid.validate().is_err());

        invalid.max_memory_bytes = 1024;
        invalid.max_cpu_percent = 150.0;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_resource_manager_creation() {
        let constraints = ResourceConstraints::minimal();
        let manager = ResourceManager::new(constraints);
        assert!(manager.is_ok());
    }

    #[test]
    fn test_operation_guard() -> Result<()> {
        let constraints = ResourceConstraints::minimal();
        let manager = ResourceManager::new(constraints)?;

        let metrics_before = manager.metrics();
        assert_eq!(metrics_before.active_operations, 0);

        {
            let _guard = manager.start_operation()?;
            let metrics_during = manager.metrics();
            assert_eq!(metrics_during.active_operations, 1);
        }

        let metrics_after = manager.metrics();
        assert_eq!(metrics_after.active_operations, 0);

        Ok(())
    }

    #[test]
    fn test_memory_allocation() -> Result<()> {
        let constraints = ResourceConstraints::minimal();
        let manager = ResourceManager::new(constraints)?;

        let _guard = manager.allocate_memory(1024)?;
        let metrics = manager.metrics();
        assert_eq!(metrics.memory_bytes, 1024);

        Ok(())
    }

    #[test]
    fn test_memory_limit() -> Result<()> {
        let mut constraints = ResourceConstraints::minimal();
        constraints.max_memory_bytes = 2048;
        let manager = ResourceManager::new(constraints)?;

        let _guard1 = manager.allocate_memory(1024)?;
        let _guard2 = manager.allocate_memory(512)?;

        // This should fail
        let result = manager.allocate_memory(1024);
        assert!(result.is_err());

        Ok(())
    }

    #[test]
    fn test_concurrent_operations_limit() -> Result<()> {
        let mut constraints = ResourceConstraints::minimal();
        constraints.max_concurrent_ops = 2;
        let manager = ResourceManager::new(constraints)?;

        let _guard1 = manager.start_operation()?;
        let _guard2 = manager.start_operation()?;

        // This should fail
        let result = manager.start_operation();
        assert!(result.is_err());

        Ok(())
    }

    #[test]
    fn test_cpu_tracking() -> Result<()> {
        let constraints = ResourceConstraints::minimal();
        let manager = ResourceManager::new(constraints)?;

        manager.record_cpu_sample(10.0);
        manager.record_cpu_sample(20.0);
        manager.record_cpu_sample(30.0);

        let cpu = manager.current_cpu();
        assert!((cpu - 20.0).abs() < 1.0);

        Ok(())
    }

    #[test]
    fn test_metrics() -> Result<()> {
        let constraints = ResourceConstraints::minimal();
        let manager = ResourceManager::new(constraints)?;

        let _guard = manager.start_operation()?;
        manager.record_failure();

        let metrics = manager.metrics();
        assert_eq!(metrics.active_operations, 1);
        assert_eq!(metrics.total_operations, 1);
        assert_eq!(metrics.failed_operations, 1);

        Ok(())
    }

    #[test]
    fn test_health_check() -> Result<()> {
        let constraints = ResourceConstraints::minimal();
        let max_memory = constraints.max_memory_bytes;
        let manager = ResourceManager::new(constraints)?;

        assert_eq!(manager.health_check(), HealthStatus::Healthy);

        // Fill up memory
        let _guard = manager.allocate_memory(max_memory - 100)?;
        assert_eq!(manager.health_check(), HealthStatus::Critical);

        Ok(())
    }

    #[test]
    fn test_peak_memory() -> Result<()> {
        let constraints = ResourceConstraints::minimal();
        let manager = ResourceManager::new(constraints)?;

        {
            let _guard = manager.allocate_memory(1000)?;
            let metrics = manager.metrics();
            assert_eq!(metrics.peak_memory_bytes, 1000);
        }

        let metrics = manager.metrics();
        assert_eq!(metrics.memory_bytes, 0);
        assert_eq!(metrics.peak_memory_bytes, 1000);

        Ok(())
    }
}
