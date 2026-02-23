//! Tile caching for improved performance.

use super::protocol::{TileCoordinate, TileResponse};
use crate::error::{Result, StreamingError};
use dashmap::DashMap;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Configuration for tile cache.
#[derive(Debug, Clone)]
pub struct TileCacheConfig {
    /// Maximum number of tiles in memory cache
    pub max_memory_tiles: usize,

    /// Maximum size of disk cache in bytes
    pub max_disk_bytes: u64,

    /// Directory for disk cache
    pub disk_cache_dir: Option<PathBuf>,

    /// Enable compression for disk cache
    pub compress: bool,

    /// TTL for cached tiles in seconds
    pub ttl_seconds: u64,
}

impl Default for TileCacheConfig {
    fn default() -> Self {
        Self {
            max_memory_tiles: 1000,
            max_disk_bytes: 1024 * 1024 * 1024, // 1GB
            disk_cache_dir: None,
            compress: false,
            ttl_seconds: 3600, // 1 hour
        }
    }
}

/// Tile cache implementation.
pub struct TileCache {
    config: TileCacheConfig,
    memory_cache: Arc<RwLock<LruCache<TileCoordinate, CachedTile>>>,
    disk_cache_map: Arc<DashMap<TileCoordinate, PathBuf>>,
}

struct CachedTile {
    response: TileResponse,
    cached_at: std::time::Instant,
}

impl TileCache {
    /// Create a new tile cache.
    pub fn new(config: TileCacheConfig) -> Result<Self> {
        let max_size = NonZeroUsize::new(config.max_memory_tiles)
            .ok_or_else(|| StreamingError::ConfigError("Invalid cache size".to_string()))?;

        Ok(Self {
            config,
            memory_cache: Arc::new(RwLock::new(LruCache::new(max_size))),
            disk_cache_map: Arc::new(DashMap::new()),
        })
    }

    /// Get a tile from cache.
    pub async fn get(&self, coord: &TileCoordinate) -> Option<TileResponse> {
        // Check memory cache
        let mut cache = self.memory_cache.write().await;
        if let Some(cached) = cache.get(coord) {
            if !self.is_expired(&cached.cached_at) {
                debug!("Memory cache hit for tile {}", coord);
                return Some(cached.response.clone());
            }
        }
        drop(cache);

        // Check disk cache
        if let Some(path) = self.disk_cache_map.get(coord) {
            if let Ok(response) = self.load_from_disk(coord, path.value()).await {
                debug!("Disk cache hit for tile {}", coord);
                // Promote to memory cache
                self.put_memory(coord, response.clone()).await.ok();
                return Some(response);
            }
        }

        None
    }

    /// Put a tile in cache.
    pub async fn put(&self, response: TileResponse) -> Result<()> {
        let coord = response.coord;

        // Store in memory cache
        self.put_memory(&coord, response.clone()).await?;

        // Store in disk cache if enabled
        if self.config.disk_cache_dir.is_some() {
            self.put_disk(&coord, response).await?;
        }

        Ok(())
    }

    /// Put a tile in memory cache.
    async fn put_memory(&self, coord: &TileCoordinate, response: TileResponse) -> Result<()> {
        let mut cache = self.memory_cache.write().await;
        cache.put(*coord, CachedTile {
            response,
            cached_at: std::time::Instant::now(),
        });
        Ok(())
    }

    /// Put a tile in disk cache.
    async fn put_disk(&self, coord: &TileCoordinate, response: TileResponse) -> Result<()> {
        let cache_dir = self.config.disk_cache_dir.as_ref()
            .ok_or_else(|| StreamingError::ConfigError("Disk cache not configured".to_string()))?;

        let path = self.tile_path(cache_dir, coord);

        // Create parent directory
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| StreamingError::Io(e))?;
        }

        // Write tile data
        fs::write(&path, &response.data)
            .await
            .map_err(|e| StreamingError::Io(e))?;

        self.disk_cache_map.insert(*coord, path);

        Ok(())
    }

    /// Load a tile from disk cache.
    async fn load_from_disk(&self, coord: &TileCoordinate, path: &Path) -> Result<TileResponse> {
        let data = fs::read(path)
            .await
            .map_err(|e| StreamingError::Io(e))?;

        Ok(TileResponse::new(
            *coord,
            bytes::Bytes::from(data),
            "image/png".to_string(),
        ))
    }

    /// Get the file path for a tile.
    fn tile_path(&self, base_dir: &Path, coord: &TileCoordinate) -> PathBuf {
        base_dir.join(format!("{}/{}/{}.png", coord.z, coord.x, coord.y))
    }

    /// Check if a cached tile is expired.
    fn is_expired(&self, cached_at: &std::time::Instant) -> bool {
        cached_at.elapsed().as_secs() > self.config.ttl_seconds
    }

    /// Clear all caches.
    pub async fn clear(&self) -> Result<()> {
        let mut cache = self.memory_cache.write().await;
        cache.clear();
        drop(cache);

        self.disk_cache_map.clear();

        if let Some(cache_dir) = &self.config.disk_cache_dir {
            if cache_dir.exists() {
                fs::remove_dir_all(cache_dir)
                    .await
                    .map_err(|e| StreamingError::Io(e))?;
            }
        }

        Ok(())
    }

    /// Get cache statistics.
    pub async fn stats(&self) -> CacheStats {
        let cache = self.memory_cache.read().await;
        CacheStats {
            memory_tiles: cache.len(),
            disk_tiles: self.disk_cache_map.len(),
            max_memory_tiles: self.config.max_memory_tiles,
        }
    }
}

/// Cache statistics.
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Number of tiles in memory cache
    pub memory_tiles: usize,

    /// Number of tiles in disk cache
    pub disk_tiles: usize,

    /// Maximum memory cache size
    pub max_memory_tiles: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    #[tokio::test]
    async fn test_memory_cache() {
        let config = TileCacheConfig {
            max_memory_tiles: 10,
            ..Default::default()
        };

        let cache = TileCache::new(config).ok();
        assert!(cache.is_some());

        if let Some(cache) = cache {
            let coord = TileCoordinate::new(10, 512, 384);
            let response = TileResponse::new(
                coord,
                Bytes::from(vec![0u8; 1024]),
                "image/png".to_string(),
            );

            cache.put(response).await.ok();

            let retrieved = cache.get(&coord).await;
            assert!(retrieved.is_some());
        }
    }
}
