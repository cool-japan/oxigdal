//! Resource quota management for users and tenants.
//!
//! This module implements quota enforcement to ensure fair resource allocation
//! and prevent resource exhaustion.

use crate::error::{ClusterError, Result};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// User or tenant identifier.
pub type QuotaId = String;

/// Resource quotas for a user or tenant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceQuota {
    /// Maximum CPU cores
    pub max_cpu_cores: f64,
    /// Maximum memory in MB
    pub max_memory_mb: usize,
    /// Maximum GPU count
    pub max_gpu_count: usize,
    /// Maximum disk space in MB
    pub max_disk_mb: usize,
    /// Maximum concurrent tasks
    pub max_concurrent_tasks: usize,
    /// Maximum task queue length
    pub max_queue_length: usize,
    /// Time window for quota enforcement
    pub time_window: Duration,
}

impl Default for ResourceQuota {
    fn default() -> Self {
        Self {
            max_cpu_cores: 4.0,
            max_memory_mb: 8192,
            max_gpu_count: 0,
            max_disk_mb: 10240,
            max_concurrent_tasks: 10,
            max_queue_length: 100,
            time_window: Duration::from_secs(3600),
        }
    }
}

/// Current resource usage for quota tracking.
#[derive(Debug, Clone)]
pub struct ResourceUsage {
    /// Current CPU cores in use
    pub current_cpu_cores: f64,
    /// Current memory in use (MB)
    pub current_memory_mb: usize,
    /// Current GPU count in use
    pub current_gpu_count: usize,
    /// Current disk space in use (MB)
    pub current_disk_mb: usize,
    /// Current concurrent tasks
    pub current_tasks: usize,
    /// Current queue length
    pub current_queue_length: usize,
    /// Last updated timestamp
    pub last_updated: Instant,
}

impl Default for ResourceUsage {
    fn default() -> Self {
        Self {
            current_cpu_cores: 0.0,
            current_memory_mb: 0,
            current_gpu_count: 0,
            current_disk_mb: 0,
            current_tasks: 0,
            current_queue_length: 0,
            last_updated: Instant::now(),
        }
    }
}

/// Quota manager for enforcing resource limits.
pub struct QuotaManager {
    /// Configured quotas per user/tenant
    quotas: Arc<DashMap<QuotaId, ResourceQuota>>,
    /// Current usage per user/tenant
    usage: Arc<DashMap<QuotaId, RwLock<ResourceUsage>>>,
    /// Default quota for new users
    default_quota: Arc<RwLock<ResourceQuota>>,
    /// Statistics
    stats: Arc<RwLock<QuotaStats>>,
}

/// Quota manager statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QuotaStats {
    /// Total number of quotas defined
    pub total_quotas: usize,
    /// Number of quota violations detected
    pub quota_violations: u64,
    /// Number of quota warnings issued
    pub quota_warnings: u64,
    /// Average resource utilization across all quotas
    pub average_utilization: f64,
}

impl QuotaManager {
    /// Create a new quota manager.
    pub fn new(default_quota: ResourceQuota) -> Self {
        Self {
            quotas: Arc::new(DashMap::new()),
            usage: Arc::new(DashMap::new()),
            default_quota: Arc::new(RwLock::new(default_quota)),
            stats: Arc::new(RwLock::new(QuotaStats::default())),
        }
    }

    /// Set quota for a user/tenant.
    pub fn set_quota(&self, id: QuotaId, quota: ResourceQuota) -> Result<()> {
        self.quotas.insert(id.clone(), quota);
        // Scope the DashMap entry ref so it's dropped before accessing
        // self.quotas.len() and self.stats.write().
        {
            self.usage
                .entry(id)
                .or_insert_with(|| RwLock::new(ResourceUsage::default()));
        }

        let mut stats = self.stats.write();
        stats.total_quotas = self.quotas.len();

        Ok(())
    }

    /// Get quota for a user/tenant.
    pub fn get_quota(&self, id: &QuotaId) -> ResourceQuota {
        self.quotas
            .get(id)
            .map(|q| q.clone())
            .unwrap_or_else(|| self.default_quota.read().clone())
    }

    /// Check if resource request is within quota.
    pub fn check_quota(
        &self,
        id: &QuotaId,
        cpu: f64,
        memory_mb: usize,
        gpu: usize,
        disk_mb: usize,
    ) -> Result<()> {
        let quota = self.get_quota(id);

        // Read current usage and snapshot it, then drop the DashMap entry ref
        // and inner RwLock read guard before acquiring stats.write() to avoid
        // holding multiple locks simultaneously.
        let (current_cpu, current_mem, current_gpu, current_disk, current_tasks) = {
            let usage_entry = self
                .usage
                .entry(id.clone())
                .or_insert_with(|| RwLock::new(ResourceUsage::default()));
            let usage = usage_entry.read();
            (
                usage.current_cpu_cores,
                usage.current_memory_mb,
                usage.current_gpu_count,
                usage.current_disk_mb,
                usage.current_tasks,
            )
        };

        // Check CPU quota
        if current_cpu + cpu > quota.max_cpu_cores {
            let mut stats = self.stats.write();
            stats.quota_violations += 1;
            return Err(ClusterError::QuotaExceeded(format!(
                "CPU quota exceeded: {} + {} > {}",
                current_cpu, cpu, quota.max_cpu_cores
            )));
        }

        // Check memory quota
        if current_mem + memory_mb > quota.max_memory_mb {
            let mut stats = self.stats.write();
            stats.quota_violations += 1;
            return Err(ClusterError::QuotaExceeded(format!(
                "Memory quota exceeded: {} + {} > {}",
                current_mem, memory_mb, quota.max_memory_mb
            )));
        }

        // Check GPU quota
        if current_gpu + gpu > quota.max_gpu_count {
            let mut stats = self.stats.write();
            stats.quota_violations += 1;
            return Err(ClusterError::QuotaExceeded(format!(
                "GPU quota exceeded: {} + {} > {}",
                current_gpu, gpu, quota.max_gpu_count
            )));
        }

        // Check disk quota
        if current_disk + disk_mb > quota.max_disk_mb {
            let mut stats = self.stats.write();
            stats.quota_violations += 1;
            return Err(ClusterError::QuotaExceeded(format!(
                "Disk quota exceeded: {} + {} > {}",
                current_disk, disk_mb, quota.max_disk_mb
            )));
        }

        // Check concurrent tasks quota
        if current_tasks + 1 > quota.max_concurrent_tasks {
            let mut stats = self.stats.write();
            stats.quota_violations += 1;
            return Err(ClusterError::QuotaExceeded(format!(
                "Concurrent tasks quota exceeded: {} >= {}",
                current_tasks, quota.max_concurrent_tasks
            )));
        }

        Ok(())
    }

    /// Allocate resources from quota.
    pub fn allocate(
        &self,
        id: &QuotaId,
        cpu: f64,
        memory_mb: usize,
        gpu: usize,
        disk_mb: usize,
    ) -> Result<()> {
        // First check quota
        self.check_quota(id, cpu, memory_mb, gpu, disk_mb)?;

        // Allocate resources - scoped to drop the DashMap entry ref and
        // inner RwLock write guard before calling update_utilization_stats(),
        // which needs to iterate the same DashMap.
        {
            let usage_entry = self
                .usage
                .entry(id.clone())
                .or_insert_with(|| RwLock::new(ResourceUsage::default()));
            let mut usage = usage_entry.write();

            usage.current_cpu_cores += cpu;
            usage.current_memory_mb += memory_mb;
            usage.current_gpu_count += gpu;
            usage.current_disk_mb += disk_mb;
            usage.current_tasks += 1;
            usage.last_updated = Instant::now();
        }

        self.update_utilization_stats();

        Ok(())
    }

    /// Release resources back to quota.
    pub fn release(
        &self,
        id: &QuotaId,
        cpu: f64,
        memory_mb: usize,
        gpu: usize,
        disk_mb: usize,
    ) -> Result<()> {
        // Scope the DashMap entry ref and inner RwLock write guard so they
        // are dropped before calling update_utilization_stats(), which needs
        // to iterate the same DashMap.
        {
            let usage_entry = self
                .usage
                .entry(id.clone())
                .or_insert_with(|| RwLock::new(ResourceUsage::default()));
            let mut usage = usage_entry.write();

            usage.current_cpu_cores = (usage.current_cpu_cores - cpu).max(0.0);
            usage.current_memory_mb = usage.current_memory_mb.saturating_sub(memory_mb);
            usage.current_gpu_count = usage.current_gpu_count.saturating_sub(gpu);
            usage.current_disk_mb = usage.current_disk_mb.saturating_sub(disk_mb);
            usage.current_tasks = usage.current_tasks.saturating_sub(1);
            usage.last_updated = Instant::now();
        }

        self.update_utilization_stats();

        Ok(())
    }

    /// Get current usage for a user/tenant.
    pub fn get_usage(&self, id: &QuotaId) -> Option<ResourceUsage> {
        self.usage.get(id).map(|u| u.read().clone())
    }

    /// Get utilization percentage (0.0 to 1.0).
    pub fn get_utilization(&self, id: &QuotaId) -> f64 {
        let quota = self.get_quota(id);
        let usage = match self.get_usage(id) {
            Some(u) => u,
            None => return 0.0,
        };

        let cpu_util = usage.current_cpu_cores / quota.max_cpu_cores;
        let mem_util = usage.current_memory_mb as f64 / quota.max_memory_mb as f64;
        let task_util = usage.current_tasks as f64 / quota.max_concurrent_tasks as f64;

        (cpu_util + mem_util + task_util) / 3.0
    }

    /// Check if quota is near limit (>80% utilized).
    pub fn is_near_limit(&self, id: &QuotaId) -> bool {
        self.get_utilization(id) > 0.8
    }

    fn update_utilization_stats(&self) {
        // Collect keys first, then drop the DashMap iterator before
        // calling get_utilization() which re-accesses the same DashMap.
        // Iterating DashMap while calling get() on it causes a deadlock
        // when both access the same shard.
        let keys: Vec<QuotaId> = self.usage.iter().map(|entry| entry.key().clone()).collect();
        let count = keys.len();

        if count == 0 {
            return;
        }

        let mut total_util = 0.0;
        for id in &keys {
            total_util += self.get_utilization(id);
        }

        let mut stats = self.stats.write();
        stats.average_utilization = total_util / count as f64;
    }

    /// Get quota statistics.
    pub fn get_stats(&self) -> QuotaStats {
        self.stats.read().clone()
    }

    /// List all quotas.
    pub fn list_quotas(&self) -> Vec<(QuotaId, ResourceQuota, ResourceUsage)> {
        self.quotas
            .iter()
            .filter_map(|entry| {
                let id = entry.key().clone();
                let quota = entry.value().clone();
                let usage = self.get_usage(&id)?;
                Some((id, quota, usage))
            })
            .collect()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_quota_allocation() {
        let quota = ResourceQuota {
            max_cpu_cores: 8.0,
            max_memory_mb: 16384,
            max_gpu_count: 2,
            max_disk_mb: 102400,
            max_concurrent_tasks: 10,
            max_queue_length: 100,
            time_window: Duration::from_secs(3600),
        };

        let manager = QuotaManager::new(quota);
        let user_id = "user1".to_string();

        // Allocate within quota
        let result = manager.allocate(&user_id, 4.0, 8192, 1, 10240);
        assert!(result.is_ok());

        // Check usage
        let usage = manager
            .get_usage(&user_id)
            .expect("usage should exist for allocated user");
        assert_eq!(usage.current_cpu_cores, 4.0);
        assert_eq!(usage.current_memory_mb, 8192);

        // Try to exceed quota
        let result = manager.allocate(&user_id, 5.0, 9000, 0, 0);
        assert!(result.is_err());

        // Release resources
        let result = manager.release(&user_id, 2.0, 4096, 0, 5120);
        assert!(result.is_ok());

        let usage = manager
            .get_usage(&user_id)
            .expect("Failed to get resource usage in test");
        assert_eq!(usage.current_cpu_cores, 2.0);
    }

    #[test]
    fn test_quota_utilization() {
        let quota = ResourceQuota::default();
        let manager = QuotaManager::new(quota);
        let user_id = "user1".to_string();

        manager.allocate(&user_id, 2.0, 4096, 0, 0).ok();

        let utilization = manager.get_utilization(&user_id);
        assert!(utilization > 0.0 && utilization < 1.0);

        let near_limit = manager.is_near_limit(&user_id);
        assert!(!near_limit);
    }
}
