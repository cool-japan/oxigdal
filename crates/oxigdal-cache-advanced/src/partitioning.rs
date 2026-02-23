//! Cache partitioning for QoS and tenant isolation
//!
//! Provides mechanisms for:
//! - Quality-of-Service (QoS) partitioning
//! - Priority-based cache allocation
//! - Tenant isolation
//! - Dynamic partition resizing
//! - Performance monitoring per partition

use crate::error::{CacheError, Result};
use crate::multi_tier::CacheKey;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Partition priority level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Critical priority (highest)
    Critical = 4,
    /// High priority
    High = 3,
    /// Normal priority
    Normal = 2,
    /// Low priority
    Low = 1,
    /// Best effort (lowest)
    BestEffort = 0,
}

impl Priority {
    /// Get priority from integer
    pub fn from_level(level: usize) -> Self {
        match level {
            4 => Priority::Critical,
            3 => Priority::High,
            2 => Priority::Normal,
            1 => Priority::Low,
            _ => Priority::BestEffort,
        }
    }

    /// Get integer level
    pub fn to_level(&self) -> usize {
        *self as usize
    }
}

/// Cache partition definition
#[derive(Debug, Clone)]
pub struct Partition {
    /// Partition ID
    pub id: String,
    /// Priority level
    pub priority: Priority,
    /// Minimum guaranteed size (bytes)
    pub min_size: usize,
    /// Maximum size limit (bytes)
    pub max_size: usize,
    /// Current size (bytes)
    pub current_size: usize,
    /// Tenant ID (for multi-tenancy)
    pub tenant_id: Option<String>,
}

impl Partition {
    /// Create new partition
    pub fn new(id: String, priority: Priority, min_size: usize, max_size: usize) -> Self {
        Self {
            id,
            priority,
            min_size,
            max_size,
            current_size: 0,
            tenant_id: None,
        }
    }

    /// Set tenant ID
    pub fn with_tenant(mut self, tenant_id: String) -> Self {
        self.tenant_id = Some(tenant_id);
        self
    }

    /// Check if partition can accommodate bytes
    pub fn can_fit(&self, bytes: usize) -> bool {
        self.current_size + bytes <= self.max_size
    }

    /// Get available space
    pub fn available_space(&self) -> usize {
        self.max_size.saturating_sub(self.current_size)
    }

    /// Get utilization percentage
    pub fn utilization(&self) -> f64 {
        if self.max_size == 0 {
            0.0
        } else {
            (self.current_size as f64 / self.max_size as f64) * 100.0
        }
    }

    /// Check if partition is under minimum guarantee
    pub fn under_minimum(&self) -> bool {
        self.current_size < self.min_size
    }
}

/// Partition statistics
#[derive(Debug, Clone)]
pub struct PartitionStats {
    /// Number of cache hits
    pub hits: u64,
    /// Number of cache misses
    pub misses: u64,
    /// Number of evictions
    pub evictions: u64,
    /// Total access time (microseconds)
    pub total_access_time_us: u64,
    /// Number of accesses
    pub access_count: u64,
}

impl PartitionStats {
    /// Create new stats
    pub fn new() -> Self {
        Self {
            hits: 0,
            misses: 0,
            evictions: 0,
            total_access_time_us: 0,
            access_count: 0,
        }
    }

    /// Calculate hit rate
    pub fn hit_rate(&self) -> f64 {
        if self.hits + self.misses == 0 {
            0.0
        } else {
            (self.hits as f64) / ((self.hits + self.misses) as f64) * 100.0
        }
    }

    /// Calculate average access time
    pub fn avg_access_time_us(&self) -> f64 {
        if self.access_count == 0 {
            0.0
        } else {
            self.total_access_time_us as f64 / self.access_count as f64
        }
    }
}

impl Default for PartitionStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Cache partitioning manager
pub struct PartitionManager {
    /// Partitions by ID
    partitions: Arc<RwLock<HashMap<String, Partition>>>,
    /// Key to partition mapping
    key_partitions: Arc<RwLock<HashMap<CacheKey, String>>>,
    /// Partition statistics
    stats: Arc<RwLock<HashMap<String, PartitionStats>>>,
    /// Total cache size
    total_size: usize,
}

impl PartitionManager {
    /// Create new partition manager
    pub fn new(total_size: usize) -> Self {
        Self {
            partitions: Arc::new(RwLock::new(HashMap::new())),
            key_partitions: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(HashMap::new())),
            total_size,
        }
    }

    /// Add partition
    pub async fn add_partition(&self, partition: Partition) -> Result<()> {
        let mut partitions = self.partitions.write().await;
        let mut stats = self.stats.write().await;

        // Validate total size doesn't exceed limit
        let total_min: usize = partitions.values().map(|p| p.min_size).sum();
        if total_min + partition.min_size > self.total_size {
            return Err(CacheError::InvalidConfig(
                "Total minimum partition sizes exceed cache size".to_string(),
            ));
        }

        stats.insert(partition.id.clone(), PartitionStats::new());
        partitions.insert(partition.id.clone(), partition);

        Ok(())
    }

    /// Remove partition
    pub async fn remove_partition(&self, partition_id: &str) -> Result<()> {
        let mut partitions = self.partitions.write().await;
        let mut stats = self.stats.write().await;

        partitions.remove(partition_id);
        stats.remove(partition_id);

        Ok(())
    }

    /// Assign key to partition
    pub async fn assign_key(&self, key: CacheKey, partition_id: String, size: usize) -> Result<()> {
        let mut partitions = self.partitions.write().await;
        let mut key_partitions = self.key_partitions.write().await;

        let partition = partitions
            .get_mut(&partition_id)
            .ok_or_else(|| CacheError::InvalidConfig("Partition not found".to_string()))?;

        if !partition.can_fit(size) {
            return Err(CacheError::CacheFull(format!(
                "Partition {} is full",
                partition_id
            )));
        }

        partition.current_size += size;
        key_partitions.insert(key, partition_id);

        Ok(())
    }

    /// Remove key from partition
    pub async fn remove_key(&self, key: &CacheKey, size: usize) -> Result<()> {
        let mut partitions = self.partitions.write().await;
        let mut key_partitions = self.key_partitions.write().await;

        if let Some(partition_id) = key_partitions.remove(key) {
            if let Some(partition) = partitions.get_mut(&partition_id) {
                partition.current_size = partition.current_size.saturating_sub(size);
            }
        }

        Ok(())
    }

    /// Get partition for key
    pub async fn get_partition(&self, key: &CacheKey) -> Option<String> {
        self.key_partitions.read().await.get(key).cloned()
    }

    /// Record cache hit
    pub async fn record_hit(&self, partition_id: &str, access_time_us: u64) {
        let mut stats = self.stats.write().await;
        if let Some(s) = stats.get_mut(partition_id) {
            s.hits += 1;
            s.total_access_time_us += access_time_us;
            s.access_count += 1;
        }
    }

    /// Record cache miss
    pub async fn record_miss(&self, partition_id: &str) {
        let mut stats = self.stats.write().await;
        if let Some(s) = stats.get_mut(partition_id) {
            s.misses += 1;
        }
    }

    /// Record eviction
    pub async fn record_eviction(&self, partition_id: &str) {
        let mut stats = self.stats.write().await;
        if let Some(s) = stats.get_mut(partition_id) {
            s.evictions += 1;
        }
    }

    /// Get partition statistics
    pub async fn get_stats(&self, partition_id: &str) -> Option<PartitionStats> {
        self.stats.read().await.get(partition_id).cloned()
    }

    /// Get all partition info
    pub async fn get_all_partitions(&self) -> Vec<Partition> {
        self.partitions.read().await.values().cloned().collect()
    }

    /// Rebalance partitions based on usage
    pub async fn rebalance(&self) -> Result<()> {
        let mut partitions = self.partitions.write().await;
        let _stats = self.stats.read().await;

        // Calculate priority-weighted sizes
        let total_priority: usize = partitions.values().map(|p| p.priority.to_level()).sum();

        if total_priority == 0 {
            return Ok(());
        }

        // Available space after guarantees
        let total_min: usize = partitions.values().map(|p| p.min_size).sum();
        let available = self.total_size.saturating_sub(total_min);

        // Distribute available space by priority
        for partition in partitions.values_mut() {
            let priority_share = partition.priority.to_level() as f64 / total_priority as f64;
            let additional = (available as f64 * priority_share) as usize;
            partition.max_size = partition.min_size + additional;
        }

        Ok(())
    }
}

/// QoS policy for automatic partition assignment
pub struct QoSPolicy {
    /// Priority mappings (tenant -> priority)
    tenant_priorities: Arc<RwLock<HashMap<String, Priority>>>,
    /// Default priority
    default_priority: Priority,
}

impl QoSPolicy {
    /// Create new QoS policy
    pub fn new(default_priority: Priority) -> Self {
        Self {
            tenant_priorities: Arc::new(RwLock::new(HashMap::new())),
            default_priority,
        }
    }

    /// Set tenant priority
    pub async fn set_tenant_priority(&self, tenant_id: String, priority: Priority) {
        self.tenant_priorities
            .write()
            .await
            .insert(tenant_id, priority);
    }

    /// Get partition ID for tenant
    pub async fn get_partition_for_tenant(&self, tenant_id: &str) -> String {
        let priorities = self.tenant_priorities.read().await;
        let priority = priorities
            .get(tenant_id)
            .copied()
            .unwrap_or(self.default_priority);

        format!("partition_{}", priority.to_level())
    }

    /// Get priority for tenant
    pub async fn get_priority(&self, tenant_id: &str) -> Priority {
        self.tenant_priorities
            .read()
            .await
            .get(tenant_id)
            .copied()
            .unwrap_or(self.default_priority)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_partition_creation() {
        let partition = Partition::new(
            "test".to_string(),
            Priority::High,
            1024 * 1024,
            10 * 1024 * 1024,
        );

        assert_eq!(partition.id, "test");
        assert_eq!(partition.priority, Priority::High);
        assert!(partition.can_fit(5 * 1024 * 1024));
    }

    #[test]
    fn test_priority_levels() {
        assert_eq!(Priority::Critical.to_level(), 4);
        assert_eq!(Priority::High.to_level(), 3);
        assert_eq!(Priority::Normal.to_level(), 2);
        assert_eq!(Priority::Low.to_level(), 1);
        assert_eq!(Priority::BestEffort.to_level(), 0);
    }

    #[tokio::test]
    async fn test_partition_manager() {
        let manager = PartitionManager::new(100 * 1024 * 1024);

        let partition = Partition::new(
            "high".to_string(),
            Priority::High,
            10 * 1024 * 1024,
            50 * 1024 * 1024,
        );

        manager.add_partition(partition).await.unwrap_or_default();

        let key = "test_key".to_string();
        manager
            .assign_key(key.clone(), "high".to_string(), 1024)
            .await
            .unwrap_or_default();

        let partition_id = manager.get_partition(&key).await;
        assert_eq!(partition_id, Some("high".to_string()));
    }

    #[tokio::test]
    async fn test_partition_stats() {
        let manager = PartitionManager::new(100 * 1024 * 1024);

        let partition = Partition::new(
            "test".to_string(),
            Priority::Normal,
            10 * 1024 * 1024,
            50 * 1024 * 1024,
        );

        manager.add_partition(partition).await.unwrap_or_default();

        manager.record_hit("test", 100).await;
        manager.record_hit("test", 150).await;
        manager.record_miss("test").await;

        let stats = manager.get_stats("test").await;
        assert!(stats.is_some());

        let stats = stats.unwrap_or_default();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert!(stats.hit_rate() > 0.0);
    }

    #[tokio::test]
    async fn test_qos_policy() {
        let policy = QoSPolicy::new(Priority::Normal);

        policy
            .set_tenant_priority("tenant1".to_string(), Priority::High)
            .await;

        let priority = policy.get_priority("tenant1").await;
        assert_eq!(priority, Priority::High);

        let priority = policy.get_priority("tenant2").await;
        assert_eq!(priority, Priority::Normal);
    }

    #[tokio::test]
    async fn test_partition_rebalance() {
        let manager = PartitionManager::new(100 * 1024 * 1024);

        let p1 = Partition::new(
            "high".to_string(),
            Priority::High,
            10 * 1024 * 1024,
            30 * 1024 * 1024,
        );

        let p2 = Partition::new(
            "low".to_string(),
            Priority::Low,
            10 * 1024 * 1024,
            20 * 1024 * 1024,
        );

        manager.add_partition(p1).await.unwrap_or_default();
        manager.add_partition(p2).await.unwrap_or_default();

        manager.rebalance().await.unwrap_or_default();

        let partitions = manager.get_all_partitions().await;
        assert_eq!(partitions.len(), 2);

        // High priority partition should get more space
        let high = partitions.iter().find(|p| p.id == "high");
        let low = partitions.iter().find(|p| p.id == "low");

        if let (Some(h), Some(l)) = (high, low) {
            assert!(h.max_size >= l.max_size);
        }
    }
}
