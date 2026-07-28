//! Tile caching for improved performance.

use super::protocol::{TileCoordinate, TileResponse};
use crate::error::{Result, StreamingError};
use dashmap::DashMap;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};
use tokio::fs;
use tokio::sync::RwLock;
use tracing::debug;

/// zlib compression level used for the disk cache when `compress` is enabled.
const DISK_COMPRESS_LEVEL: u8 = 6;

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

/// Metadata for a tile persisted to the disk cache.
#[derive(Clone)]
struct DiskEntry {
    /// On-disk path of the (possibly compressed) tile bytes.
    path: PathBuf,
    /// The tile's real content type, preserved so reloads report the correct MIME.
    content_type: String,
    /// When the tile was written (for TTL enforcement on disk hits).
    cached_at: SystemTime,
    /// Number of bytes actually stored on disk (after optional compression).
    stored_bytes: u64,
    /// Whether the on-disk bytes are zlib-compressed.
    compressed: bool,
}

/// Tile cache implementation.
pub struct TileCache {
    config: TileCacheConfig,
    memory_cache: Arc<RwLock<LruCache<TileCoordinate, CachedTile>>>,
    disk_cache_map: Arc<DashMap<TileCoordinate, DiskEntry>>,
    /// Cumulative bytes currently held by the disk cache.
    disk_bytes: Arc<AtomicU64>,
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
            disk_bytes: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Get a tile from cache.
    pub async fn get(&self, coord: &TileCoordinate) -> Option<TileResponse> {
        // Check memory cache
        let mut cache = self.memory_cache.write().await;
        if let Some(cached) = cache.get(coord)
            && !self.is_expired(&cached.cached_at)
        {
            debug!("Memory cache hit for tile {}", coord);
            return Some(cached.response.clone());
        }
        drop(cache);

        // Check disk cache — but honor the TTL for disk hits too.
        let entry = self.disk_cache_map.get(coord).map(|e| e.value().clone());
        if let Some(entry) = entry {
            if self.is_disk_expired(entry.cached_at) {
                debug!("Disk cache entry for tile {} expired; evicting", coord);
                self.evict_disk(coord).await;
                return None;
            }
            if let Ok(response) = self.load_from_disk(coord, &entry).await {
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
        cache.put(
            *coord,
            CachedTile {
                response,
                cached_at: std::time::Instant::now(),
            },
        );
        Ok(())
    }

    /// Put a tile in disk cache.
    ///
    /// The tile's real content type is preserved, the bytes are optionally
    /// zlib-compressed (when `config.compress` is set), and the cumulative disk
    /// usage is accounted for and bounded by `config.max_disk_bytes` via
    /// oldest-first eviction.
    async fn put_disk(&self, coord: &TileCoordinate, response: TileResponse) -> Result<()> {
        let cache_dir =
            self.config.disk_cache_dir.as_ref().ok_or_else(|| {
                StreamingError::ConfigError("Disk cache not configured".to_string())
            })?;

        let ext = ext_for_content_type(&response.content_type);
        let compressed = self.config.compress;
        let file_name = if compressed {
            format!("{}/{}/{}.{}.z", coord.z, coord.x, coord.y, ext)
        } else {
            format!("{}/{}/{}.{}", coord.z, coord.x, coord.y, ext)
        };
        let path = cache_dir.join(file_name);

        // Create parent directory
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(StreamingError::Io)?;
        }

        // Optionally compress the payload (Pure-Rust zlib via oxiarc-deflate).
        let bytes_to_write: Vec<u8> = if compressed {
            oxiarc_deflate::zlib_compress(&response.data, DISK_COMPRESS_LEVEL).map_err(|e| {
                StreamingError::Other(format!("disk cache zlib compression failed: {e}"))
            })?
        } else {
            response.data.to_vec()
        };
        let stored_bytes = bytes_to_write.len() as u64;

        // Write tile data
        fs::write(&path, &bytes_to_write)
            .await
            .map_err(StreamingError::Io)?;

        // If this coordinate already had a disk entry, remove its old file and
        // reclaim its byte accounting before inserting the new one.
        if let Some((_, old)) = self.disk_cache_map.remove(coord) {
            if old.path != path {
                fs::remove_file(&old.path).await.ok();
            }
            self.disk_bytes.fetch_sub(
                old.stored_bytes
                    .min(self.disk_bytes.load(Ordering::Relaxed)),
                Ordering::Relaxed,
            );
        }

        self.disk_cache_map.insert(
            *coord,
            DiskEntry {
                path,
                content_type: response.content_type.clone(),
                cached_at: SystemTime::now(),
                stored_bytes,
                compressed,
            },
        );
        self.disk_bytes.fetch_add(stored_bytes, Ordering::Relaxed);

        // Enforce the disk-size budget, evicting oldest entries first.
        self.enforce_disk_limit().await;

        Ok(())
    }

    /// Evict oldest disk entries until the cumulative size is within
    /// `config.max_disk_bytes`.
    async fn enforce_disk_limit(&self) {
        while self.disk_bytes.load(Ordering::Relaxed) > self.config.max_disk_bytes {
            // Find the oldest entry (smallest `cached_at`).
            let oldest = self
                .disk_cache_map
                .iter()
                .min_by_key(|e| e.value().cached_at)
                .map(|e| *e.key());
            match oldest {
                Some(coord) => self.evict_disk(&coord).await,
                None => break, // nothing left to evict
            }
        }
    }

    /// Remove a single tile from the disk cache (file + accounting + map entry).
    async fn evict_disk(&self, coord: &TileCoordinate) {
        if let Some((_, entry)) = self.disk_cache_map.remove(coord) {
            fs::remove_file(&entry.path).await.ok();
            let current = self.disk_bytes.load(Ordering::Relaxed);
            self.disk_bytes
                .fetch_sub(entry.stored_bytes.min(current), Ordering::Relaxed);
        }
    }

    /// Load a tile from disk cache, decompressing and restoring the real
    /// content type recorded when it was stored.
    async fn load_from_disk(
        &self,
        coord: &TileCoordinate,
        entry: &DiskEntry,
    ) -> Result<TileResponse> {
        let raw = fs::read(&entry.path).await.map_err(StreamingError::Io)?;
        let data = if entry.compressed {
            oxiarc_deflate::zlib_decompress(&raw).map_err(|e| {
                StreamingError::Other(format!("disk cache zlib decompression failed: {e}"))
            })?
        } else {
            raw
        };

        Ok(TileResponse::new(
            *coord,
            bytes::Bytes::from(data),
            entry.content_type.clone(),
        ))
    }

    /// Check if a cached tile is expired.
    fn is_expired(&self, cached_at: &std::time::Instant) -> bool {
        cached_at.elapsed() > Duration::from_secs(self.config.ttl_seconds)
    }

    /// Check if a disk-cached tile is expired (based on wall-clock `cached_at`).
    fn is_disk_expired(&self, cached_at: SystemTime) -> bool {
        SystemTime::now()
            .duration_since(cached_at)
            .unwrap_or(Duration::ZERO)
            > Duration::from_secs(self.config.ttl_seconds)
    }

    /// Clear all caches.
    pub async fn clear(&self) -> Result<()> {
        let mut cache = self.memory_cache.write().await;
        cache.clear();
        drop(cache);

        self.disk_cache_map.clear();
        self.disk_bytes.store(0, Ordering::Relaxed);

        if let Some(cache_dir) = &self.config.disk_cache_dir
            && cache_dir.exists()
        {
            fs::remove_dir_all(cache_dir)
                .await
                .map_err(StreamingError::Io)?;
        }

        Ok(())
    }

    /// Get cache statistics.
    pub async fn stats(&self) -> CacheStats {
        let cache = self.memory_cache.read().await;
        CacheStats {
            memory_tiles: cache.len(),
            disk_tiles: self.disk_cache_map.len(),
            disk_bytes: self.disk_bytes.load(Ordering::Relaxed),
            max_memory_tiles: self.config.max_memory_tiles,
        }
    }
}

/// Map a tile content type to a file extension for the disk cache.
fn ext_for_content_type(content_type: &str) -> &'static str {
    // Ignore any parameters after ';' (e.g. "image/png; charset=binary").
    let base = content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim();
    match base {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "application/x-protobuf" | "application/vnd.mapbox-vector-tile" => "pbf",
        "application/geo+json" => "geojson",
        "application/json" => "json",
        _ => "bin",
    }
}

/// Cache statistics.
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Number of tiles in memory cache
    pub memory_tiles: usize,

    /// Number of tiles in disk cache
    pub disk_tiles: usize,

    /// Cumulative bytes held by the disk cache (after optional compression).
    pub disk_bytes: u64,

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
            let response =
                TileResponse::new(coord, Bytes::from(vec![0u8; 1024]), "image/png".to_string());

            cache.put(response).await.ok();

            let retrieved = cache.get(&coord).await;
            assert!(retrieved.is_some());
        }
    }

    fn temp_cache_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "oxigeo_streaming_cache_{}_{}_{}",
            tag,
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ))
    }

    #[tokio::test]
    async fn test_disk_cache_preserves_content_type_and_compresses() {
        let dir = temp_cache_dir("ct");
        let config = TileCacheConfig {
            max_memory_tiles: 1, // force eviction from memory so disk is exercised
            disk_cache_dir: Some(dir.clone()),
            compress: true,
            ttl_seconds: 3600,
            ..Default::default()
        };
        let cache = TileCache::new(config).expect("cache");

        let coord = TileCoordinate::new(4, 2, 3);
        // A highly compressible JPEG payload with a non-png content type.
        let payload = Bytes::from(vec![7u8; 4096]);
        let response = TileResponse::new(coord, payload.clone(), "image/jpeg".to_string());
        cache.put(response).await.expect("put");

        // Evict from memory by inserting another tile (max_memory_tiles = 1).
        let other = TileCoordinate::new(4, 9, 9);
        cache
            .put(TileResponse::new(
                other,
                Bytes::from(vec![1u8; 8]),
                "image/png".to_string(),
            ))
            .await
            .expect("put other");

        // Now the original comes from disk: content type preserved (NOT png),
        // and the decompressed bytes are intact.
        let got = cache.get(&coord).await.expect("disk hit");
        assert_eq!(got.content_type, "image/jpeg");
        assert_eq!(&got.data[..], &payload[..]);

        // Compression actually shrank the on-disk footprint.
        let stats = cache.stats().await;
        assert!(
            stats.disk_bytes < 4096,
            "compressed disk bytes should be < raw"
        );

        cache.clear().await.ok();
    }

    #[tokio::test]
    async fn test_disk_cache_enforces_max_disk_bytes() {
        let dir = temp_cache_dir("limit");
        let config = TileCacheConfig {
            max_memory_tiles: 100,
            max_disk_bytes: 3000, // room for ~2 of the 1024-byte tiles below
            disk_cache_dir: Some(dir.clone()),
            compress: false,
            ttl_seconds: 3600,
        };
        let cache = TileCache::new(config).expect("cache");

        for i in 0..5u32 {
            let coord = TileCoordinate::new(5, i, 0);
            cache
                .put(TileResponse::new(
                    coord,
                    Bytes::from(vec![i as u8; 1024]),
                    "application/octet-stream".to_string(),
                ))
                .await
                .expect("put");
        }

        let stats = cache.stats().await;
        assert!(
            stats.disk_bytes <= 3000,
            "disk cache must stay within max_disk_bytes, got {}",
            stats.disk_bytes
        );
        assert!(
            stats.disk_tiles < 5,
            "some tiles must have been evicted, have {}",
            stats.disk_tiles
        );

        cache.clear().await.ok();
    }

    #[tokio::test]
    async fn test_disk_cache_respects_ttl() {
        let dir = temp_cache_dir("ttl");
        let config = TileCacheConfig {
            max_memory_tiles: 1,
            disk_cache_dir: Some(dir.clone()),
            compress: false,
            ttl_seconds: 0, // everything is immediately stale
            ..Default::default()
        };
        let cache = TileCache::new(config).expect("cache");

        let coord = TileCoordinate::new(3, 1, 1);
        cache
            .put(TileResponse::new(
                coord,
                Bytes::from(vec![9u8; 16]),
                "image/png".to_string(),
            ))
            .await
            .expect("put");

        // Push out of memory cache.
        cache
            .put(TileResponse::new(
                TileCoordinate::new(3, 2, 2),
                Bytes::from(vec![0u8; 4]),
                "image/png".to_string(),
            ))
            .await
            .expect("put other");

        // With ttl_seconds = 0 the disk entry is expired and must not be served.
        assert!(
            cache.get(&coord).await.is_none(),
            "expired disk tile must not be returned"
        );

        cache.clear().await.ok();
    }
}
