//! Specialized cache backends (Spatial, Tile, Persistent Disk)

#[cfg(feature = "cache")]
use dashmap::DashMap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[cfg(feature = "async")]
use tokio::sync::RwLock;

use bytes::Bytes;

use super::CacheConfig;
use super::metadata::{
    CacheEntry, CacheKey, CacheStats, DiskCacheMetadata, LevelStats, SpatialInfo, TileCoord,
};
use crate::error::{CacheError, CloudError, Result};

/// Spatial-aware cache for geospatial data
#[cfg(feature = "cache")]
pub struct SpatialCache {
    /// Main storage
    storage: Arc<DashMap<CacheKey, CacheEntry>>,
    /// Spatial index: maps grid cells to keys
    spatial_index: Arc<DashMap<(i64, i64), Vec<CacheKey>>>,
    /// Grid cell size
    cell_size: f64,
    /// Current size
    current_size: Arc<AtomicUsize>,
    /// Configuration
    config: CacheConfig,
    /// Statistics
    stats: CacheStats,
}

#[cfg(feature = "cache")]
impl SpatialCache {
    /// Creates a new spatial cache
    pub fn new(config: CacheConfig, cell_size: f64) -> Self {
        Self {
            storage: Arc::new(DashMap::new()),
            spatial_index: Arc::new(DashMap::new()),
            cell_size,
            current_size: Arc::new(AtomicUsize::new(0)),
            config,
            stats: CacheStats::default(),
        }
    }

    /// Gets cell coordinates for a point
    fn get_cell(&self, x: f64, y: f64) -> (i64, i64) {
        let cell_x = (x / self.cell_size).floor() as i64;
        let cell_y = (y / self.cell_size).floor() as i64;
        (cell_x, cell_y)
    }

    /// Gets entries intersecting a bounding box
    pub async fn get_by_bounds(
        &self,
        bounds: (f64, f64, f64, f64),
    ) -> Result<Vec<(CacheKey, Bytes)>> {
        let (min_x, min_y, max_x, max_y) = bounds;
        let query_bounds = SpatialInfo::new(bounds);

        let min_cell = self.get_cell(min_x, min_y);
        let max_cell = self.get_cell(max_x, max_y);

        let mut results = Vec::new();

        for cx in min_cell.0..=max_cell.0 {
            for cy in min_cell.1..=max_cell.1 {
                if let Some(keys) = self.spatial_index.get(&(cx, cy)) {
                    for key in keys.iter() {
                        if let Some(entry) = self.storage.get(key) {
                            if let Some(ref spatial) = entry.spatial_info {
                                if spatial.intersects(&query_bounds) && !entry.is_expired() {
                                    results.push((key.clone(), entry.data.clone()));
                                }
                            }
                        }
                    }
                }
            }
        }

        self.stats
            .hits
            .fetch_add(results.len() as u64, Ordering::Relaxed);
        Ok(results)
    }

    /// Puts an entry with spatial info
    pub async fn put(
        &self,
        key: CacheKey,
        data: Bytes,
        spatial_info: SpatialInfo,
        ttl: Option<Duration>,
    ) -> Result<()> {
        let entry = if let Some(ttl_duration) = ttl.or(self.config.default_ttl) {
            let mut e = CacheEntry::with_ttl(data, false, ttl_duration);
            e.spatial_info = Some(spatial_info.clone());
            e
        } else {
            CacheEntry::with_spatial_info(data, false, spatial_info.clone())
        };

        let entry_size = entry.size;

        // Evict if necessary
        while self.current_size.load(Ordering::SeqCst) + entry_size > self.config.max_memory_size
            && !self.storage.is_empty()
        {
            self.evict_oldest().await;
        }

        // Index in spatial grid
        let (min_x, min_y, max_x, max_y) = spatial_info.bounds;
        let min_cell = self.get_cell(min_x, min_y);
        let max_cell = self.get_cell(max_x, max_y);

        for cx in min_cell.0..=max_cell.0 {
            for cy in min_cell.1..=max_cell.1 {
                self.spatial_index
                    .entry((cx, cy))
                    .or_default()
                    .push(key.clone());
            }
        }

        if let Some(old) = self.storage.insert(key, entry) {
            self.current_size.fetch_sub(old.size, Ordering::SeqCst);
        }
        self.current_size.fetch_add(entry_size, Ordering::SeqCst);
        self.stats.writes.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// Evicts oldest entry
    async fn evict_oldest(&self) {
        let mut oldest_key: Option<String> = None;
        let mut oldest_time = Instant::now();

        for entry in self.storage.iter() {
            if entry.created_at < oldest_time {
                oldest_time = entry.created_at;
                oldest_key = Some(entry.key().clone());
            }
        }

        if let Some(key) = oldest_key {
            if let Some((_, entry)) = self.storage.remove(&key) {
                self.current_size.fetch_sub(entry.size, Ordering::SeqCst);
                self.stats.evictions.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Clears the cache
    pub async fn clear(&self) -> Result<()> {
        self.storage.clear();
        self.spatial_index.clear();
        self.current_size.store(0, Ordering::SeqCst);
        Ok(())
    }
}

/// Tile-based cache for COG and tile pyramids
#[cfg(feature = "cache")]
pub struct TileCache {
    /// Main storage
    storage: Arc<DashMap<TileCoord, CacheEntry>>,
    /// Level statistics
    level_stats: Arc<DashMap<u8, LevelStats>>,
    /// Current size
    current_size: Arc<AtomicUsize>,
    /// Configuration
    config: CacheConfig,
    /// Statistics
    stats: CacheStats,
}

#[cfg(feature = "cache")]
impl TileCache {
    /// Creates a new tile cache
    pub fn new(config: CacheConfig) -> Self {
        Self {
            storage: Arc::new(DashMap::new()),
            level_stats: Arc::new(DashMap::new()),
            current_size: Arc::new(AtomicUsize::new(0)),
            config,
            stats: CacheStats::default(),
        }
    }

    /// Gets a tile
    pub async fn get(&self, coord: &TileCoord) -> Result<Bytes> {
        if let Some(mut entry) = self.storage.get_mut(coord) {
            if entry.is_expired() {
                drop(entry);
                self.remove(coord).await?;
                return Err(CloudError::Cache(CacheError::Miss {
                    key: coord.to_cache_key("tile"),
                }));
            }

            entry.record_access();

            if let Some(level_stat) = self.level_stats.get(&coord.z) {
                level_stat.hits.fetch_add(1, Ordering::Relaxed);
            }

            self.stats.hits.fetch_add(1, Ordering::Relaxed);
            Ok(entry.data.clone())
        } else {
            self.stats.misses.fetch_add(1, Ordering::Relaxed);
            Err(CloudError::Cache(CacheError::Miss {
                key: coord.to_cache_key("tile"),
            }))
        }
    }

    /// Puts a tile
    pub async fn put(&self, coord: TileCoord, data: Bytes, ttl: Option<Duration>) -> Result<()> {
        let entry = if let Some(ttl_duration) = ttl.or(self.config.default_ttl) {
            CacheEntry::with_ttl(data, false, ttl_duration)
        } else {
            CacheEntry::new(data, false)
        };

        let entry_size = entry.size;

        // Evict if necessary
        while self.current_size.load(Ordering::SeqCst) + entry_size > self.config.max_memory_size
            && !self.storage.is_empty()
        {
            self.evict_tile().await;
        }

        // Update level stats
        let level_stat = self.level_stats.entry(coord.z).or_default();
        level_stat.tile_count.fetch_add(1, Ordering::SeqCst);
        level_stat
            .total_size
            .fetch_add(entry_size, Ordering::SeqCst);

        if let Some(old) = self.storage.insert(coord, entry) {
            self.current_size.fetch_sub(old.size, Ordering::SeqCst);
        }
        self.current_size.fetch_add(entry_size, Ordering::SeqCst);
        self.stats.writes.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// Evicts a tile using the configured strategy
    async fn evict_tile(&self) {
        // Prefer evicting from higher zoom levels (more tiles, less important)
        let mut max_level = 0u8;
        for entry in self.level_stats.iter() {
            if *entry.key() > max_level && entry.tile_count.load(Ordering::SeqCst) > 0 {
                max_level = *entry.key();
            }
        }

        // Find an entry at this level
        let mut key_to_remove: Option<TileCoord> = None;
        for entry in self.storage.iter() {
            if entry.key().z == max_level {
                key_to_remove = Some(entry.key().clone());
                break;
            }
        }

        if let Some(coord) = key_to_remove {
            if let Some((_, entry)) = self.storage.remove(&coord) {
                self.current_size.fetch_sub(entry.size, Ordering::SeqCst);

                if let Some(level_stat) = self.level_stats.get(&coord.z) {
                    level_stat.tile_count.fetch_sub(1, Ordering::SeqCst);
                    level_stat
                        .total_size
                        .fetch_sub(entry.size, Ordering::SeqCst);
                }

                self.stats.evictions.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Removes a tile
    pub async fn remove(&self, coord: &TileCoord) -> Result<()> {
        if let Some((_, entry)) = self.storage.remove(coord) {
            self.current_size.fetch_sub(entry.size, Ordering::SeqCst);

            if let Some(level_stat) = self.level_stats.get(&coord.z) {
                level_stat.tile_count.fetch_sub(1, Ordering::SeqCst);
                level_stat
                    .total_size
                    .fetch_sub(entry.size, Ordering::SeqCst);
            }
        }
        Ok(())
    }

    /// Prefetches adjacent tiles
    pub fn get_prefetch_targets(&self, coord: &TileCoord) -> Vec<TileCoord> {
        let mut targets = Vec::new();

        // Adjacent tiles at same level
        let x = coord.x;
        let y = coord.y;
        let z = coord.z;

        let offsets: [(i32, i32); 8] = [
            (-1, -1),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ];

        for (dx, dy) in offsets {
            let nx = x as i64 + dx as i64;
            let ny = y as i64 + dy as i64;
            if nx >= 0 && ny >= 0 {
                targets.push(TileCoord::new(z, nx as u32, ny as u32));
            }
        }

        // Parent tile
        if let Some(parent) = coord.parent() {
            targets.push(parent);
        }

        targets
    }

    /// Clears the cache
    pub async fn clear(&self) -> Result<()> {
        self.storage.clear();
        self.level_stats.clear();
        self.current_size.store(0, Ordering::SeqCst);
        Ok(())
    }

    /// Returns cache statistics
    #[must_use]
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }
}

/// Persistent disk cache with metadata
pub struct PersistentDiskCache {
    /// Cache directory
    cache_dir: PathBuf,
    /// Metadata storage
    metadata: Arc<RwLock<HashMap<CacheKey, DiskCacheMetadata>>>,
    /// Current size
    current_size: Arc<AtomicUsize>,
    /// Configuration
    config: CacheConfig,
    /// Statistics
    stats: CacheStats,
}

impl PersistentDiskCache {
    /// Creates a new persistent disk cache
    pub fn new(config: CacheConfig) -> Result<Self> {
        let cache_dir = config.cache_dir.clone().ok_or_else(|| {
            CloudError::Cache(CacheError::WriteError {
                message: "Cache directory not configured".to_string(),
            })
        })?;

        std::fs::create_dir_all(&cache_dir).map_err(|e| {
            CloudError::Cache(CacheError::WriteError {
                message: format!("Failed to create cache directory: {e}"),
            })
        })?;

        let mut cache = Self {
            cache_dir,
            metadata: Arc::new(RwLock::new(HashMap::new())),
            current_size: Arc::new(AtomicUsize::new(0)),
            config,
            stats: CacheStats::default(),
        };

        // Load existing metadata
        cache.load_metadata_blocking()?;

        Ok(cache)
    }

    /// Loads metadata from disk
    fn load_metadata_blocking(&mut self) -> Result<()> {
        let metadata_path = self.cache_dir.join("metadata.json");
        if metadata_path.exists() {
            let content = std::fs::read_to_string(&metadata_path).map_err(|e| {
                CloudError::Cache(CacheError::ReadError {
                    message: format!("Failed to read metadata: {e}"),
                })
            })?;

            let metadata: HashMap<CacheKey, DiskCacheMetadata> = serde_json::from_str(&content)
                .map_err(|e| {
                    CloudError::Cache(CacheError::ReadError {
                        message: format!("Failed to parse metadata: {e}"),
                    })
                })?;

            let total_size: usize = metadata.values().map(|m| m.size).sum();
            self.current_size.store(total_size, Ordering::SeqCst);
            *self.metadata.blocking_write() = metadata;
        }
        Ok(())
    }

    /// Saves metadata to disk
    async fn save_metadata(&self) -> Result<()> {
        let metadata_path = self.cache_dir.join("metadata.json");
        let metadata = self.metadata.read().await;
        let content = serde_json::to_string_pretty(&*metadata).map_err(|e| {
            CloudError::Cache(CacheError::WriteError {
                message: format!("Failed to serialize metadata: {e}"),
            })
        })?;

        tokio::fs::write(&metadata_path, content)
            .await
            .map_err(|e| {
                CloudError::Cache(CacheError::WriteError {
                    message: format!("Failed to write metadata: {e}"),
                })
            })?;

        Ok(())
    }

    /// Gets the file path for a cache key
    fn get_path(&self, key: &CacheKey) -> PathBuf {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        let hash = hasher.finalize();
        let filename = format!("{:x}", hash);
        self.cache_dir.join(&filename[..2]).join(filename)
    }

    /// Gets an entry from disk
    pub async fn get(&self, key: &CacheKey) -> Result<Bytes> {
        let metadata = self.metadata.read().await;

        if let Some(meta) = metadata.get(key) {
            // Check expiration
            if let Some(expires_at_ms) = meta.expires_at_ms {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);

                if now_ms >= expires_at_ms {
                    drop(metadata);
                    self.remove(key).await?;
                    return Err(CloudError::Cache(CacheError::Miss { key: key.clone() }));
                }
            }

            let path = self.get_path(key);
            let data = tokio::fs::read(&path)
                .await
                .map_err(|_| CloudError::Cache(CacheError::Miss { key: key.clone() }))?;

            self.stats.hits.fetch_add(1, Ordering::Relaxed);
            Ok(Bytes::from(data))
        } else {
            self.stats.misses.fetch_add(1, Ordering::Relaxed);
            Err(CloudError::Cache(CacheError::Miss { key: key.clone() }))
        }
    }

    /// Puts an entry to disk
    pub async fn put(&self, key: CacheKey, data: Bytes, ttl: Option<Duration>) -> Result<()> {
        let path = self.get_path(&key);

        // Create parent directory
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                CloudError::Cache(CacheError::WriteError {
                    message: format!("Failed to create directory: {e}"),
                })
            })?;
        }

        let size = data.len();

        // Evict if necessary
        while self.current_size.load(Ordering::SeqCst) + size > self.config.max_disk_size {
            self.evict_oldest().await?;
        }

        // Write data
        tokio::fs::write(&path, &data).await.map_err(|e| {
            CloudError::Cache(CacheError::WriteError {
                message: format!("Failed to write file: {e}"),
            })
        })?;

        // Update metadata
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let expires_at_ms = ttl
            .or(self.config.default_ttl)
            .map(|d| now_ms + d.as_millis() as u64);

        let meta = DiskCacheMetadata {
            path: path
                .strip_prefix(&self.cache_dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string(),
            size,
            created_at_ms: now_ms,
            expires_at_ms,
            access_count: 1,
            compressed: false,
        };

        {
            let mut metadata = self.metadata.write().await;
            if let Some(old) = metadata.insert(key, meta) {
                self.current_size.fetch_sub(old.size, Ordering::SeqCst);
            }
        }

        self.current_size.fetch_add(size, Ordering::SeqCst);
        self.stats.writes.fetch_add(1, Ordering::Relaxed);

        // Save metadata periodically
        if self.stats.writes.load(Ordering::Relaxed) % 10 == 0 {
            self.save_metadata().await?;
        }

        Ok(())
    }

    /// Evicts oldest entry
    async fn evict_oldest(&self) -> Result<()> {
        let mut oldest_key: Option<String> = None;
        let mut oldest_time = u64::MAX;

        {
            let metadata = self.metadata.read().await;
            for (key, meta) in metadata.iter() {
                if meta.created_at_ms < oldest_time {
                    oldest_time = meta.created_at_ms;
                    oldest_key = Some(key.clone());
                }
            }
        }

        if let Some(key) = oldest_key {
            self.remove(&key).await?;
        }

        Ok(())
    }

    /// Removes an entry
    pub async fn remove(&self, key: &CacheKey) -> Result<()> {
        let path = self.get_path(key);
        tokio::fs::remove_file(&path).await.ok();

        let mut metadata = self.metadata.write().await;
        if let Some(meta) = metadata.remove(key) {
            self.current_size.fetch_sub(meta.size, Ordering::SeqCst);
            self.stats.evictions.fetch_add(1, Ordering::Relaxed);
        }

        Ok(())
    }

    /// Clears the cache
    pub async fn clear(&self) -> Result<()> {
        let metadata = self.metadata.read().await;
        for key in metadata.keys() {
            let path = self.get_path(key);
            tokio::fs::remove_file(&path).await.ok();
        }
        drop(metadata);

        self.metadata.write().await.clear();
        self.current_size.store(0, Ordering::SeqCst);

        self.save_metadata().await?;

        Ok(())
    }

    /// Returns cache statistics
    #[must_use]
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }
}
