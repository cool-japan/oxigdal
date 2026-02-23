//! Multi-tier cache implementation
//!
//! Implements a three-tier caching system:
//! - L1: In-memory cache (fastest, smallest)
//! - L2: SSD cache (fast, medium size)
//! - L3: Network/disk cache (slower, largest)
//!
//! Features:
//! - Automatic promotion/demotion between tiers
//! - Per-tier eviction policies
//! - Tier usage statistics
//! - Async operations

use crate::compression::{AdaptiveCompressor, CompressedData, CompressionCodec, DataType};
use crate::error::{CacheError, Result};
use crate::eviction::{EvictionPolicy, LruEviction};
use crate::{CacheConfig, CacheStats};
use async_trait::async_trait;
use bytes::Bytes;
use dashmap::DashMap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::RwLock;

/// Cache key type
pub type CacheKey = String;

/// Cache value with metadata
#[derive(Debug, Clone)]
pub struct CacheValue {
    /// Actual data
    pub data: Bytes,
    /// Data type hint for compression
    pub data_type: DataType,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last access timestamp
    pub last_accessed: chrono::DateTime<chrono::Utc>,
    /// Access count
    pub access_count: u64,
    /// Size in bytes
    pub size: usize,
}

impl CacheValue {
    /// Create new cache value
    pub fn new(data: Bytes, data_type: DataType) -> Self {
        let now = chrono::Utc::now();
        let size = data.len();

        Self {
            data,
            data_type,
            created_at: now,
            last_accessed: now,
            access_count: 0,
            size,
        }
    }

    /// Record an access
    pub fn record_access(&mut self) {
        self.last_accessed = chrono::Utc::now();
        self.access_count += 1;
    }

    /// Get age in seconds
    pub fn age_seconds(&self) -> i64 {
        let now = chrono::Utc::now();
        (now - self.created_at).num_seconds()
    }

    /// Get time since last access in seconds
    pub fn idle_seconds(&self) -> i64 {
        let now = chrono::Utc::now();
        (now - self.last_accessed).num_seconds()
    }
}

/// Cache tier trait
#[async_trait]
pub trait CacheTier: Send + Sync {
    /// Get value from this tier
    async fn get(&self, key: &CacheKey) -> Result<Option<CacheValue>>;

    /// Put value into this tier
    async fn put(&self, key: CacheKey, value: CacheValue) -> Result<()>;

    /// Remove value from this tier
    async fn remove(&self, key: &CacheKey) -> Result<bool>;

    /// Check if key exists
    async fn contains(&self, key: &CacheKey) -> bool;

    /// Get tier statistics
    async fn stats(&self) -> CacheStats;

    /// Clear the tier
    async fn clear(&self) -> Result<()>;

    /// Get tier name
    fn name(&self) -> &str;

    /// Get tier capacity in bytes
    fn capacity(&self) -> usize;

    /// Get current size in bytes
    async fn current_size(&self) -> usize;
}

/// L1 in-memory cache tier
pub struct L1MemoryTier {
    /// Cache storage
    cache: Arc<DashMap<CacheKey, CacheValue>>,
    /// Maximum size in bytes
    max_size: usize,
    /// Current size in bytes
    current_size: Arc<RwLock<usize>>,
    /// Eviction policy
    eviction: Arc<RwLock<Box<dyn EvictionPolicy<CacheKey>>>>,
    /// Statistics
    stats: Arc<RwLock<CacheStats>>,
}

impl L1MemoryTier {
    /// Create new L1 memory tier
    pub fn new(max_size: usize) -> Self {
        Self {
            cache: Arc::new(DashMap::new()),
            max_size,
            current_size: Arc::new(RwLock::new(0)),
            eviction: Arc::new(RwLock::new(Box::new(LruEviction::new()))),
            stats: Arc::new(RwLock::new(CacheStats::new())),
        }
    }

    /// Evict items until we have enough space
    async fn make_space(&self, needed: usize) -> Result<()> {
        let mut current = self.current_size.write().await;

        while *current + needed > self.max_size {
            let mut eviction = self.eviction.write().await;

            if let Some(victim_key) = eviction.select_victim() {
                if let Some((_, victim_value)) = self.cache.remove(&victim_key) {
                    *current = current.saturating_sub(victim_value.size);
                    eviction.on_remove(&victim_key);

                    let mut stats = self.stats.write().await;
                    stats.evictions += 1;
                    stats.item_count = stats.item_count.saturating_sub(1);
                } else {
                    // Victim not in cache, try another
                    continue;
                }
            } else {
                // No victims available
                return Err(CacheError::CacheFull("L1 cache full".to_string()));
            }
        }

        Ok(())
    }
}

#[async_trait]
impl CacheTier for L1MemoryTier {
    async fn get(&self, key: &CacheKey) -> Result<Option<CacheValue>> {
        let mut stats = self.stats.write().await;

        if let Some(mut entry) = self.cache.get_mut(key) {
            entry.record_access();
            stats.hits += 1;

            let mut eviction = self.eviction.write().await;
            eviction.on_access(key);

            Ok(Some(entry.clone()))
        } else {
            stats.misses += 1;
            Ok(None)
        }
    }

    async fn put(&self, key: CacheKey, value: CacheValue) -> Result<()> {
        let size = value.size;

        // Make space if needed
        self.make_space(size).await?;

        // Insert into cache
        self.cache.insert(key.clone(), value);

        // Update size
        let mut current_size = self.current_size.write().await;
        *current_size += size;

        // Update eviction policy
        let mut eviction = self.eviction.write().await;
        eviction.on_insert(key.clone(), size);

        // Update stats
        let mut stats = self.stats.write().await;
        stats.bytes_stored = *current_size as u64;
        stats.item_count += 1;

        Ok(())
    }

    async fn remove(&self, key: &CacheKey) -> Result<bool> {
        if let Some((_, value)) = self.cache.remove(key) {
            let mut current_size = self.current_size.write().await;
            *current_size = current_size.saturating_sub(value.size);

            let mut eviction = self.eviction.write().await;
            eviction.on_remove(key);

            let mut stats = self.stats.write().await;
            stats.bytes_stored = *current_size as u64;
            stats.item_count = stats.item_count.saturating_sub(1);

            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn contains(&self, key: &CacheKey) -> bool {
        self.cache.contains_key(key)
    }

    async fn stats(&self) -> CacheStats {
        self.stats.read().await.clone()
    }

    async fn clear(&self) -> Result<()> {
        self.cache.clear();

        let mut current_size = self.current_size.write().await;
        *current_size = 0;

        let mut eviction = self.eviction.write().await;
        eviction.clear();

        let mut stats = self.stats.write().await;
        *stats = CacheStats::new();

        Ok(())
    }

    fn name(&self) -> &str {
        "L1-Memory"
    }

    fn capacity(&self) -> usize {
        self.max_size
    }

    async fn current_size(&self) -> usize {
        *self.current_size.read().await
    }
}

/// L2 SSD/disk cache tier
pub struct L2DiskTier {
    /// Cache directory
    cache_dir: PathBuf,
    /// Maximum size in bytes
    max_size: usize,
    /// Index of cached files
    index: Arc<DashMap<CacheKey, CacheValue>>,
    /// Current size in bytes
    current_size: Arc<RwLock<usize>>,
    /// Eviction policy
    eviction: Arc<RwLock<Box<dyn EvictionPolicy<CacheKey>>>>,
    /// Compressor
    compressor: Arc<RwLock<AdaptiveCompressor>>,
    /// Statistics
    stats: Arc<RwLock<CacheStats>>,
}

impl L2DiskTier {
    /// Create new L2 disk tier
    pub async fn new(cache_dir: PathBuf, max_size: usize) -> Result<Self> {
        // Create cache directory
        fs::create_dir_all(&cache_dir).await?;

        let tier = Self {
            cache_dir,
            max_size,
            index: Arc::new(DashMap::new()),
            current_size: Arc::new(RwLock::new(0)),
            eviction: Arc::new(RwLock::new(Box::new(LruEviction::new()))),
            compressor: Arc::new(RwLock::new(AdaptiveCompressor::new())),
            stats: Arc::new(RwLock::new(CacheStats::new())),
        };

        // Load existing cache files
        tier.load_index().await?;

        Ok(tier)
    }

    /// Load cache index from disk
    async fn load_index(&self) -> Result<()> {
        let mut entries = fs::read_dir(&self.cache_dir).await?;
        let mut total_size = 0;

        while let Some(entry) = entries.next_entry().await? {
            if let Ok(metadata) = entry.metadata().await {
                if metadata.is_file() {
                    let file_size = metadata.len() as usize;
                    total_size += file_size;

                    // Extract key from filename (remove .cache extension)
                    if let Some(file_name) = entry.file_name().to_str() {
                        if file_name.ends_with(".cache") {
                            let key = file_name.trim_end_matches(".cache").to_string();

                            // Create minimal cache value for index
                            let value = CacheValue {
                                data: Bytes::new(),
                                data_type: DataType::Binary,
                                created_at: chrono::Utc::now(),
                                last_accessed: chrono::Utc::now(),
                                access_count: 0,
                                size: file_size,
                            };

                            self.index.insert(key.clone(), value);

                            let mut eviction = self.eviction.write().await;
                            eviction.on_insert(key, file_size);
                        }
                    }
                }
            }
        }

        let mut current_size = self.current_size.write().await;
        *current_size = total_size;

        let mut stats = self.stats.write().await;
        stats.bytes_stored = total_size as u64;
        stats.item_count = self.index.len();

        Ok(())
    }

    /// Get file path for key
    fn get_file_path(&self, key: &CacheKey) -> PathBuf {
        self.cache_dir.join(format!("{}.cache", key))
    }

    /// Evict items until we have enough space
    async fn make_space(&self, needed: usize) -> Result<()> {
        let mut current = self.current_size.write().await;

        while *current + needed > self.max_size {
            let mut eviction = self.eviction.write().await;

            if let Some(victim_key) = eviction.select_victim() {
                let file_path = self.get_file_path(&victim_key);

                if let Some((_, victim_value)) = self.index.remove(&victim_key) {
                    // Delete file
                    let _ = fs::remove_file(file_path).await;

                    *current = current.saturating_sub(victim_value.size);
                    eviction.on_remove(&victim_key);

                    let mut stats = self.stats.write().await;
                    stats.evictions += 1;
                    stats.item_count = stats.item_count.saturating_sub(1);
                } else {
                    continue;
                }
            } else {
                return Err(CacheError::CacheFull("L2 cache full".to_string()));
            }
        }

        Ok(())
    }
}

#[async_trait]
impl CacheTier for L2DiskTier {
    async fn get(&self, key: &CacheKey) -> Result<Option<CacheValue>> {
        let mut stats = self.stats.write().await;

        if let Some(mut index_entry) = self.index.get_mut(key) {
            let file_path = self.get_file_path(key);

            // Read from disk
            let mut file = fs::File::open(file_path).await?;
            let mut compressed_bytes = Vec::new();
            file.read_to_end(&mut compressed_bytes).await?;

            // Deserialize compressed data
            let compressed: CompressedData = serde_json::from_slice(&compressed_bytes)?;

            // Decompress
            let mut compressor = self.compressor.write().await;
            let data = compressed.decompress(&mut compressor)?;

            index_entry.record_access();
            stats.hits += 1;

            let mut eviction = self.eviction.write().await;
            eviction.on_access(key);

            Ok(Some(CacheValue {
                data,
                data_type: index_entry.data_type,
                created_at: index_entry.created_at,
                last_accessed: index_entry.last_accessed,
                access_count: index_entry.access_count,
                size: index_entry.size,
            }))
        } else {
            stats.misses += 1;
            Ok(None)
        }
    }

    async fn put(&self, key: CacheKey, value: CacheValue) -> Result<()> {
        // Compress data
        let mut compressor = self.compressor.write().await;
        let codec = compressor.select_codec(value.data_type);
        let compressed_data = compressor.compress(&value.data, codec, value.data_type)?;

        // If data was too small to compress, use CompressionCodec::None
        let actual_codec = if compressed_data.len() == value.data.len() && value.data.len() < 1024 {
            CompressionCodec::None
        } else {
            codec
        };
        drop(compressor);

        let compressed =
            CompressedData::new(compressed_data.to_vec(), actual_codec, value.data.len());

        // Serialize
        let serialized = serde_json::to_vec(&compressed)?;
        let file_size = serialized.len();

        // Make space
        self.make_space(file_size).await?;

        // Write to disk
        let file_path = self.get_file_path(&key);
        let mut file = fs::File::create(file_path).await?;
        file.write_all(&serialized).await?;
        file.flush().await?;

        // Update index
        let index_value = CacheValue {
            data: Bytes::new(),
            data_type: value.data_type,
            created_at: value.created_at,
            last_accessed: value.last_accessed,
            access_count: value.access_count,
            size: file_size,
        };

        self.index.insert(key.clone(), index_value);

        // Update size
        let mut current_size = self.current_size.write().await;
        *current_size += file_size;

        // Update eviction policy
        let mut eviction = self.eviction.write().await;
        eviction.on_insert(key, file_size);

        // Update stats
        let mut stats = self.stats.write().await;
        stats.bytes_stored = *current_size as u64;
        stats.item_count += 1;

        Ok(())
    }

    async fn remove(&self, key: &CacheKey) -> Result<bool> {
        if let Some((_, value)) = self.index.remove(key) {
            let file_path = self.get_file_path(key);
            let _ = fs::remove_file(file_path).await;

            let mut current_size = self.current_size.write().await;
            *current_size = current_size.saturating_sub(value.size);

            let mut eviction = self.eviction.write().await;
            eviction.on_remove(key);

            let mut stats = self.stats.write().await;
            stats.bytes_stored = *current_size as u64;
            stats.item_count = stats.item_count.saturating_sub(1);

            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn contains(&self, key: &CacheKey) -> bool {
        self.index.contains_key(key)
    }

    async fn stats(&self) -> CacheStats {
        self.stats.read().await.clone()
    }

    async fn clear(&self) -> Result<()> {
        // Remove all cache files
        let mut entries = fs::read_dir(&self.cache_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            if entry.path().extension().and_then(|s| s.to_str()) == Some("cache") {
                let _ = fs::remove_file(entry.path()).await;
            }
        }

        self.index.clear();

        let mut current_size = self.current_size.write().await;
        *current_size = 0;

        let mut eviction = self.eviction.write().await;
        eviction.clear();

        let mut stats = self.stats.write().await;
        *stats = CacheStats::new();

        Ok(())
    }

    fn name(&self) -> &str {
        "L2-Disk"
    }

    fn capacity(&self) -> usize {
        self.max_size
    }

    async fn current_size(&self) -> usize {
        *self.current_size.read().await
    }
}

/// Multi-tier cache
pub struct MultiTierCache {
    /// L1 tier
    l1: Arc<dyn CacheTier>,
    /// L2 tier
    l2: Option<Arc<dyn CacheTier>>,
    /// L3 tier (optional)
    l3: Option<Arc<dyn CacheTier>>,
    /// Configuration
    #[allow(dead_code)]
    config: CacheConfig,
    /// Global statistics
    global_stats: Arc<RwLock<CacheStats>>,
}

impl MultiTierCache {
    /// Create new multi-tier cache
    pub async fn new(config: CacheConfig) -> Result<Self> {
        let l1 = Arc::new(L1MemoryTier::new(config.l1_size)) as Arc<dyn CacheTier>;

        let l2 = if config.l2_size > 0 {
            if let Some(cache_dir) = &config.cache_dir {
                let l2_dir = cache_dir.join("l2");
                Some(Arc::new(L2DiskTier::new(l2_dir, config.l2_size).await?) as Arc<dyn CacheTier>)
            } else {
                None
            }
        } else {
            None
        };

        Ok(Self {
            l1,
            l2,
            l3: None, // L3 network tier can be added later
            config,
            global_stats: Arc::new(RwLock::new(CacheStats::new())),
        })
    }

    /// Get value from cache (checks all tiers)
    pub async fn get(&self, key: &CacheKey) -> Result<Option<CacheValue>> {
        // Try L1 first
        if let Some(value) = self.l1.get(key).await? {
            let mut stats = self.global_stats.write().await;
            stats.hits += 1;
            return Ok(Some(value));
        }

        // Try L2
        if let Some(l2) = &self.l2 {
            if let Some(value) = l2.get(key).await? {
                // Promote to L1
                let _ = self.l1.put(key.clone(), value.clone()).await;

                let mut stats = self.global_stats.write().await;
                stats.hits += 1;
                return Ok(Some(value));
            }
        }

        // Try L3
        if let Some(l3) = &self.l3 {
            if let Some(value) = l3.get(key).await? {
                // Promote to L2 and L1
                if let Some(l2) = &self.l2 {
                    let _ = l2.put(key.clone(), value.clone()).await;
                }
                let _ = self.l1.put(key.clone(), value.clone()).await;

                let mut stats = self.global_stats.write().await;
                stats.hits += 1;
                return Ok(Some(value));
            }
        }

        let mut stats = self.global_stats.write().await;
        stats.misses += 1;
        Ok(None)
    }

    /// Put value into cache (writes to all tiers)
    pub async fn put(&self, key: CacheKey, value: CacheValue) -> Result<()> {
        // Write to L1
        self.l1.put(key.clone(), value.clone()).await?;

        // Write to L2
        if let Some(l2) = &self.l2 {
            let _ = l2.put(key.clone(), value.clone()).await;
        }

        // Write to L3
        if let Some(l3) = &self.l3 {
            let _ = l3.put(key, value).await;
        }

        Ok(())
    }

    /// Remove value from all tiers
    pub async fn remove(&self, key: &CacheKey) -> Result<bool> {
        let mut removed = false;

        removed |= self.l1.remove(key).await?;

        if let Some(l2) = &self.l2 {
            removed |= l2.remove(key).await?;
        }

        if let Some(l3) = &self.l3 {
            removed |= l3.remove(key).await?;
        }

        Ok(removed)
    }

    /// Check if key exists in any tier
    pub async fn contains(&self, key: &CacheKey) -> bool {
        if self.l1.contains(key).await {
            return true;
        }

        if let Some(l2) = &self.l2 {
            if l2.contains(key).await {
                return true;
            }
        }

        if let Some(l3) = &self.l3 {
            if l3.contains(key).await {
                return true;
            }
        }

        false
    }

    /// Get global statistics
    pub async fn stats(&self) -> CacheStats {
        self.global_stats.read().await.clone()
    }

    /// Get per-tier statistics
    pub async fn tier_stats(&self) -> HashMap<String, CacheStats> {
        let mut stats = HashMap::new();

        stats.insert(self.l1.name().to_string(), self.l1.stats().await);

        if let Some(l2) = &self.l2 {
            stats.insert(l2.name().to_string(), l2.stats().await);
        }

        if let Some(l3) = &self.l3 {
            stats.insert(l3.name().to_string(), l3.stats().await);
        }

        stats
    }

    /// Clear all tiers
    pub async fn clear(&self) -> Result<()> {
        self.l1.clear().await?;

        if let Some(l2) = &self.l2 {
            l2.clear().await?;
        }

        if let Some(l3) = &self.l3 {
            l3.clear().await?;
        }

        let mut stats = self.global_stats.write().await;
        *stats = CacheStats::new();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_l1_memory_tier() {
        let tier = L1MemoryTier::new(1024 * 1024); // 1MB

        let key = "test_key".to_string();
        let value = CacheValue::new(Bytes::from("test data"), DataType::Binary);

        // Put and get
        tier.put(key.clone(), value.clone())
            .await
            .expect("put failed");
        let retrieved = tier.get(&key).await.expect("get failed");

        assert!(retrieved.is_some());
        assert_eq!(retrieved.as_ref().map(|v| &v.data), Some(&value.data));

        // Stats
        let stats = tier.stats().await;
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.item_count, 1);
    }

    #[tokio::test]
    async fn test_l1_eviction() {
        let tier = L1MemoryTier::new(100); // Very small cache

        let value1 = CacheValue::new(Bytes::from("a".repeat(40)), DataType::Binary);
        let value2 = CacheValue::new(Bytes::from("b".repeat(40)), DataType::Binary);
        let value3 = CacheValue::new(Bytes::from("c".repeat(40)), DataType::Binary);

        tier.put("key1".to_string(), value1)
            .await
            .expect("put failed");
        tier.put("key2".to_string(), value2)
            .await
            .expect("put failed");

        // This should trigger eviction
        tier.put("key3".to_string(), value3)
            .await
            .expect("put failed");

        let stats = tier.stats().await;
        assert!(stats.evictions > 0);
    }

    #[tokio::test]
    async fn test_multi_tier_cache() {
        let temp_dir = std::env::temp_dir().join("oxigdal_cache_test");
        let config = CacheConfig {
            l1_size: 1024,
            l2_size: 4096,
            l3_size: 0,
            enable_compression: true,
            enable_prefetch: false,
            enable_distributed: false,
            cache_dir: Some(temp_dir.clone()),
        };

        let cache = MultiTierCache::new(config)
            .await
            .expect("cache creation failed");

        let key = "test_multi".to_string();
        let value = CacheValue::new(Bytes::from("multi-tier test data"), DataType::Text);

        // Put
        cache
            .put(key.clone(), value.clone())
            .await
            .expect("put failed");

        // Get
        let retrieved = cache.get(&key).await.expect("get failed");
        assert!(retrieved.is_some());

        // Clean up
        let _ = tokio::fs::remove_dir_all(temp_dir).await;
    }
}
