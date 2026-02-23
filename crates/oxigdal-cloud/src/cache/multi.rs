//! Multi-level caching and cache warming

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[cfg(feature = "async")]
use tokio::sync::RwLock;

use bytes::Bytes;

use super::backends::PersistentDiskCache;
use super::eviction::LruTtlCache;
use super::metadata::{CacheKey, CacheStats};
use super::{CacheConfig, WarmingStrategy};
use crate::error::{CacheError, CloudError, Result};

/// Cache warmer for proactive cache population
#[cfg(feature = "cache")]
pub struct CacheWarmer<C> {
    /// The cache to warm
    cache: Arc<C>,
    /// Warming strategy
    strategy: WarmingStrategy,
    /// Access history for pattern detection
    access_history: Arc<RwLock<VecDeque<CacheKey>>>,
    /// Maximum history size
    max_history: usize,
}

#[cfg(feature = "cache")]
impl<C> CacheWarmer<C> {
    /// Creates a new cache warmer
    pub fn new(cache: Arc<C>, strategy: WarmingStrategy) -> Self {
        Self {
            cache,
            strategy,
            access_history: Arc::new(RwLock::new(VecDeque::new())),
            max_history: 1000,
        }
    }

    /// Records an access for pattern detection
    pub async fn record_access(&self, key: &CacheKey) {
        let mut history = self.access_history.write().await;
        history.push_back(key.clone());
        while history.len() > self.max_history {
            history.pop_front();
        }
    }

    /// Gets warming targets based on strategy
    pub async fn get_warming_targets(&self, current_key: &CacheKey) -> Vec<CacheKey> {
        match self.strategy {
            WarmingStrategy::None => Vec::new(),
            WarmingStrategy::AccessPattern => self.get_pattern_targets(current_key).await,
            WarmingStrategy::SpatialAdjacent => Vec::new(), // Requires spatial info
            WarmingStrategy::PyramidLevels => Vec::new(),   // Requires tile info
            WarmingStrategy::Custom => Vec::new(),
        }
    }

    /// Gets targets based on access patterns
    async fn get_pattern_targets(&self, current_key: &CacheKey) -> Vec<CacheKey> {
        let history = self.access_history.read().await;

        // Find sequences that start with current_key
        let mut next_keys: HashMap<CacheKey, usize> = HashMap::new();

        for window in history.iter().collect::<Vec<_>>().windows(2) {
            if window[0] == current_key {
                *next_keys.entry(window[1].clone()).or_insert(0) += 1;
            }
        }

        // Return most common next keys
        let mut targets: Vec<_> = next_keys.into_iter().collect();
        targets.sort_by(|a, b| b.1.cmp(&a.1));
        targets.into_iter().take(5).map(|(k, _)| k).collect()
    }
}

/// Memory cache layer (simplified for compatibility)
#[cfg(feature = "cache")]
pub type MemoryCache = LruTtlCache;

/// Disk cache layer
pub type DiskCache = PersistentDiskCache;

/// Multi-level cache combining memory and disk tiers
#[cfg(feature = "cache")]
pub struct MultiLevelCache {
    /// Memory cache (L1)
    pub(crate) memory: LruTtlCache,
    /// Disk cache (L2)
    disk: Option<PersistentDiskCache>,
    /// Cache warmer
    warmer: Option<Arc<CacheWarmer<LruTtlCache>>>,
}

#[cfg(feature = "cache")]
impl MultiLevelCache {
    /// Creates a new multi-level cache
    pub fn new(config: CacheConfig) -> Result<Self> {
        let memory = LruTtlCache::new(config.clone())?;

        let disk = if config.persistent && config.cache_dir.is_some() {
            Some(PersistentDiskCache::new(config.clone())?)
        } else {
            None
        };

        let warmer = if config.warming_strategy != WarmingStrategy::None {
            Some(Arc::new(CacheWarmer::new(
                Arc::new(LruTtlCache::new(config.clone())?),
                config.warming_strategy,
            )))
        } else {
            None
        };

        Ok(Self {
            memory,
            disk,
            warmer,
        })
    }

    /// Gets an entry from the cache
    pub async fn get(&self, key: &CacheKey) -> Result<Bytes> {
        // Record access for warming
        if let Some(ref warmer) = self.warmer {
            warmer.record_access(key).await;
        }

        // Try L1 (memory) first
        if let Ok(data) = self.memory.get(key).await {
            tracing::trace!("Cache hit (memory): {}", key);
            return Ok(data);
        }

        // Try L2 (disk) if available
        if let Some(ref disk) = self.disk {
            if let Ok(data) = disk.get(key).await {
                tracing::trace!("Cache hit (disk): {}", key);

                // Promote to memory cache
                self.memory.put(key.clone(), data.clone(), None).await.ok();

                return Ok(data);
            }
        }

        tracing::trace!("Cache miss: {}", key);
        Err(CloudError::Cache(CacheError::Miss { key: key.clone() }))
    }

    /// Puts an entry into the cache
    pub async fn put(&self, key: CacheKey, data: Bytes) -> Result<()> {
        // Write to both levels
        self.memory.put(key.clone(), data.clone(), None).await?;

        if let Some(ref disk) = self.disk {
            disk.put(key, data, None).await?;
        }

        Ok(())
    }

    /// Puts an entry with TTL
    pub async fn put_with_ttl(&self, key: CacheKey, data: Bytes, ttl: Duration) -> Result<()> {
        self.memory
            .put(key.clone(), data.clone(), Some(ttl))
            .await?;

        if let Some(ref disk) = self.disk {
            disk.put(key, data, Some(ttl)).await?;
        }

        Ok(())
    }

    /// Removes an entry from the cache
    pub async fn remove(&self, key: &CacheKey) -> Result<()> {
        self.memory.remove(key).await?;

        if let Some(ref disk) = self.disk {
            disk.remove(key).await?;
        }

        Ok(())
    }

    /// Clears the cache
    pub async fn clear(&self) -> Result<()> {
        self.memory.clear().await?;

        if let Some(ref disk) = self.disk {
            disk.clear().await?;
        }

        Ok(())
    }

    /// Returns cache statistics
    #[must_use]
    pub fn stats(&self) -> &CacheStats {
        self.memory.stats()
    }

    /// Returns the current memory cache size
    pub fn memory_size(&self) -> usize {
        self.memory.current_size.load(Ordering::SeqCst)
    }
}
