//! Cache metadata and entry types

use bytes::Bytes;
use std::time::{Duration, Instant};

/// Cache key type
pub type CacheKey = String;

/// Cache entry with metadata
#[derive(Debug, Clone)]
pub struct CacheEntry {
    /// Cached data
    pub data: Bytes,
    /// Entry size in bytes
    pub size: usize,
    /// Access count for LFU
    pub access_count: u64,
    /// Last access time
    pub last_access: Instant,
    /// Creation time
    pub created_at: Instant,
    /// TTL expiration time
    pub expires_at: Option<Instant>,
    /// Whether data is compressed
    pub compressed: bool,
    /// Spatial metadata for geospatial caching
    pub spatial_info: Option<SpatialInfo>,
}

impl CacheEntry {
    /// Creates a new cache entry
    #[must_use]
    pub fn new(data: Bytes, compressed: bool) -> Self {
        let size = data.len();
        let now = Instant::now();

        Self {
            data,
            size,
            access_count: 1,
            last_access: now,
            created_at: now,
            expires_at: None,
            compressed,
            spatial_info: None,
        }
    }

    /// Creates a new cache entry with TTL
    #[must_use]
    pub fn with_ttl(data: Bytes, compressed: bool, ttl: Duration) -> Self {
        let mut entry = Self::new(data, compressed);
        entry.expires_at = Some(Instant::now() + ttl);
        entry
    }

    /// Creates a new cache entry with spatial info
    #[must_use]
    pub fn with_spatial_info(data: Bytes, compressed: bool, spatial_info: SpatialInfo) -> Self {
        let mut entry = Self::new(data, compressed);
        entry.spatial_info = Some(spatial_info);
        entry
    }

    /// Records an access
    pub fn record_access(&mut self) {
        self.access_count += 1;
        self.last_access = Instant::now();
    }

    /// Returns the age of the entry
    #[must_use]
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Checks if the entry is expired
    #[must_use]
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            Instant::now() >= expires_at
        } else {
            false
        }
    }

    /// Returns remaining TTL if set
    #[must_use]
    pub fn remaining_ttl(&self) -> Option<Duration> {
        self.expires_at.map(|expires_at| {
            let now = Instant::now();
            if now >= expires_at {
                Duration::ZERO
            } else {
                expires_at - now
            }
        })
    }
}

/// Spatial information for geospatial caching
#[derive(Debug, Clone)]
pub struct SpatialInfo {
    /// Bounding box: (min_x, min_y, max_x, max_y)
    pub bounds: (f64, f64, f64, f64),
    /// Coordinate reference system (EPSG code)
    pub crs: Option<u32>,
    /// Resolution in CRS units
    pub resolution: Option<(f64, f64)>,
    /// Zoom level for tile caching
    pub zoom_level: Option<u8>,
}

impl SpatialInfo {
    /// Creates new spatial info with bounding box
    #[must_use]
    pub fn new(bounds: (f64, f64, f64, f64)) -> Self {
        Self {
            bounds,
            crs: None,
            resolution: None,
            zoom_level: None,
        }
    }

    /// Sets the CRS
    #[must_use]
    pub fn with_crs(mut self, crs: u32) -> Self {
        self.crs = Some(crs);
        self
    }

    /// Sets the resolution
    #[must_use]
    pub fn with_resolution(mut self, res_x: f64, res_y: f64) -> Self {
        self.resolution = Some((res_x, res_y));
        self
    }

    /// Sets the zoom level
    #[must_use]
    pub fn with_zoom_level(mut self, zoom: u8) -> Self {
        self.zoom_level = Some(zoom);
        self
    }

    /// Checks if this bounding box intersects with another
    #[must_use]
    pub fn intersects(&self, other: &SpatialInfo) -> bool {
        let (min_x1, min_y1, max_x1, max_y1) = self.bounds;
        let (min_x2, min_y2, max_x2, max_y2) = other.bounds;

        min_x1 <= max_x2 && max_x1 >= min_x2 && min_y1 <= max_y2 && max_y1 >= min_y2
    }

    /// Checks if this bounding box contains a point
    #[must_use]
    pub fn contains_point(&self, x: f64, y: f64) -> bool {
        let (min_x, min_y, max_x, max_y) = self.bounds;
        x >= min_x && x <= max_x && y >= min_y && y <= max_y
    }
}

/// Tile coordinates for tile-based caching
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct TileCoord {
    /// Zoom level
    pub z: u8,
    /// X coordinate
    pub x: u32,
    /// Y coordinate
    pub y: u32,
}

impl TileCoord {
    /// Creates new tile coordinates
    #[must_use]
    pub const fn new(z: u8, x: u32, y: u32) -> Self {
        Self { z, x, y }
    }

    /// Generates cache key for this tile
    #[must_use]
    pub fn to_cache_key(&self, prefix: &str) -> String {
        format!("{prefix}/tiles/{}/{}/{}", self.z, self.x, self.y)
    }

    /// Returns parent tile coordinates
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        if self.z == 0 {
            return None;
        }
        Some(Self {
            z: self.z - 1,
            x: self.x / 2,
            y: self.y / 2,
        })
    }

    /// Returns children tile coordinates
    #[must_use]
    pub fn children(&self) -> [Self; 4] {
        let z = self.z + 1;
        let x = self.x * 2;
        let y = self.y * 2;
        [
            Self::new(z, x, y),
            Self::new(z, x + 1, y),
            Self::new(z, x, y + 1),
            Self::new(z, x + 1, y + 1),
        ]
    }
}

/// Metadata for disk cache entries
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiskCacheMetadata {
    /// File path relative to cache dir
    pub path: String,
    /// Entry size
    pub size: usize,
    /// Creation timestamp
    pub created_at_ms: u64,
    /// Expiration timestamp
    pub expires_at_ms: Option<u64>,
    /// Access count
    pub access_count: u64,
    /// Whether compressed
    pub compressed: bool,
}

/// Cache statistics
#[derive(Debug, Default)]
pub struct CacheStats {
    /// Cache hits
    pub hits: std::sync::atomic::AtomicU64,
    /// Cache misses
    pub misses: std::sync::atomic::AtomicU64,
    /// Cache writes
    pub writes: std::sync::atomic::AtomicU64,
    /// Cache evictions
    pub evictions: std::sync::atomic::AtomicU64,
}

impl CacheStats {
    /// Returns hit ratio
    #[must_use]
    pub fn hit_ratio(&self) -> f64 {
        use std::sync::atomic::Ordering;
        let hits = self.hits.load(Ordering::Relaxed) as f64;
        let misses = self.misses.load(Ordering::Relaxed) as f64;
        if hits + misses == 0.0 {
            0.0
        } else {
            hits / (hits + misses)
        }
    }

    /// Resets statistics
    pub fn reset(&self) {
        use std::sync::atomic::Ordering;
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
        self.writes.store(0, Ordering::Relaxed);
        self.evictions.store(0, Ordering::Relaxed);
    }
}

/// Statistics per zoom level
#[derive(Debug, Default)]
pub struct LevelStats {
    /// Number of tiles at this level
    pub tile_count: std::sync::atomic::AtomicUsize,
    /// Total size at this level
    pub total_size: std::sync::atomic::AtomicUsize,
    /// Hit count at this level
    pub hits: std::sync::atomic::AtomicU64,
}
