//! Tile cache data structures: TileKey, CachedTile, TileCache, TilePrefetcher.
//!
//! Implements an LRU tile cache with byte-budget eviction, staleness checking,
//! FNV-1a ETag generation, and a prefetcher that enumerates neighboring tiles.

use std::collections::{HashMap, VecDeque};

// ── TileFormat ────────────────────────────────────────────────────────────────

/// The serialization format of a tile.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TileFormat {
    /// Mapbox Vector Tile — `application/vnd.mapbox-vector-tile`
    Mvt,
    /// PNG raster tile — `image/png`
    Png,
    /// JPEG raster tile — `image/jpeg`
    Jpeg,
    /// WebP raster tile — `image/webp`
    Webp,
    /// JSON tile (e.g. UTFGrid) — `application/json`
    Json,
}

impl TileFormat {
    /// Returns the file extension used in tile paths.
    #[must_use]
    pub fn extension(&self) -> &'static str {
        match self {
            TileFormat::Mvt => "mvt",
            TileFormat::Png => "png",
            TileFormat::Jpeg => "jpg",
            TileFormat::Webp => "webp",
            TileFormat::Json => "json",
        }
    }

    /// Returns the MIME content-type string.
    #[must_use]
    pub fn content_type(&self) -> &'static str {
        match self {
            TileFormat::Mvt => "application/vnd.mapbox-vector-tile",
            TileFormat::Png => "image/png",
            TileFormat::Jpeg => "image/jpeg",
            TileFormat::Webp => "image/webp",
            TileFormat::Json => "application/json",
        }
    }
}

// ── TileKey ───────────────────────────────────────────────────────────────────

/// Uniquely identifies a tile by zoom level, column, row, layer name, and format.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TileKey {
    /// Zoom level (0–22).
    pub z: u8,
    /// Tile column.
    pub x: u32,
    /// Tile row.
    pub y: u32,
    /// Layer name.
    pub layer: String,
    /// Serialization format.
    pub format: TileFormat,
}

impl TileKey {
    /// Creates a new `TileKey`.
    pub fn new(z: u8, x: u32, y: u32, layer: impl Into<String>, format: TileFormat) -> Self {
        Self {
            z,
            x,
            y,
            layer: layer.into(),
            format,
        }
    }

    /// Returns the canonical path string `"{layer}/{z}/{x}/{y}.{ext}"`.
    #[must_use]
    pub fn path_string(&self) -> String {
        format!(
            "{}/{}/{}/{}.{}",
            self.layer,
            self.z,
            self.x,
            self.y,
            self.format.extension()
        )
    }

    /// Returns the MIME content-type for this tile's format.
    #[must_use]
    pub fn content_type(&self) -> &'static str {
        self.format.content_type()
    }
}

// ── TileEncoding ──────────────────────────────────────────────────────────────

/// Content-encoding applied to the tile bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TileEncoding {
    /// No content-encoding (identity).
    Identity,
    /// Gzip-compressed.
    Gzip,
    /// Brotli-compressed.
    Brotli,
}

// ── CachedTile ────────────────────────────────────────────────────────────────

/// A tile stored in the cache together with its metadata.
#[derive(Debug, Clone)]
pub struct CachedTile {
    /// The tile's unique key.
    pub key: TileKey,
    /// Raw tile bytes.
    pub data: Vec<u8>,
    /// ETag string (quoted FNV-1a hex).
    pub etag: String,
    /// Unix timestamp when the tile was cached.
    pub created_at: u64,
    /// Unix timestamp of the most recent access.
    pub accessed_at: u64,
    /// Number of times this tile has been accessed (starts at 1 on creation).
    pub access_count: u64,
    /// Size of `data` in bytes.
    pub size_bytes: u64,
    /// Content-encoding of `data`.
    pub encoding: TileEncoding,
}

impl CachedTile {
    /// Creates a new `CachedTile`, computing the ETag from `data`.
    pub fn new(key: TileKey, data: Vec<u8>, timestamp: u64) -> Self {
        let etag = Self::compute_etag(&data);
        let size_bytes = data.len() as u64;
        Self {
            key,
            data,
            etag,
            created_at: timestamp,
            accessed_at: timestamp,
            access_count: 1,
            size_bytes,
            encoding: TileEncoding::Identity,
        }
    }

    /// Computes a quoted FNV-1a 64-bit hex ETag for `data`.
    fn compute_etag(data: &[u8]) -> String {
        const FNV_OFFSET: u64 = 14_695_981_039_346_656_037;
        const FNV_PRIME: u64 = 1_099_511_628_211;
        let mut hash = FNV_OFFSET;
        for &byte in data {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        format!("\"{hash:016x}\"")
    }

    /// Returns `true` if the tile has exceeded `max_age_secs` since creation.
    #[must_use]
    pub fn is_stale(&self, max_age_secs: u64, now: u64) -> bool {
        now >= self.created_at.saturating_add(max_age_secs)
    }
}

// ── CacheStats ────────────────────────────────────────────────────────────────

/// A snapshot of `TileCache` statistics.
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Number of entries currently in the cache.
    pub entry_count: usize,
    /// Total bytes occupied by cached tile data.
    pub total_bytes: u64,
    /// Cumulative cache hits.
    pub hit_count: u64,
    /// Cumulative cache misses.
    pub miss_count: u64,
    /// Cumulative evictions.
    pub eviction_count: u64,
    /// Hit rate in the range `[0.0, 1.0]`.
    pub hit_rate: f64,
}

// ── TileCache ─────────────────────────────────────────────────────────────────

/// LRU tile cache with entry-count and byte-budget eviction.
///
/// Entries at the **back** of `access_order` are most recently used;
/// the **front** is evicted first.
pub struct TileCache {
    entries: HashMap<TileKey, CachedTile>,
    access_order: VecDeque<TileKey>,
    /// Maximum number of entries before LRU eviction.
    pub max_entries: usize,
    /// Maximum total bytes before LRU eviction.
    pub max_bytes: u64,
    /// Current total bytes of cached tile data.
    pub current_bytes: u64,
    /// Cumulative cache hits.
    pub hit_count: u64,
    /// Cumulative cache misses.
    pub miss_count: u64,
    /// Cumulative evictions.
    pub eviction_count: u64,
}

impl TileCache {
    /// Creates a new `TileCache` with the given capacity limits.
    pub fn new(max_entries: usize, max_bytes: u64) -> Self {
        Self {
            entries: HashMap::new(),
            access_order: VecDeque::new(),
            max_entries,
            max_bytes,
            current_bytes: 0,
            hit_count: 0,
            miss_count: 0,
            eviction_count: 0,
        }
    }

    /// Looks up `key` in the cache.  On a hit the access metadata is updated
    /// and the key is promoted to the back of the LRU queue.
    pub fn get(&mut self, key: &TileKey, now: u64) -> Option<&CachedTile> {
        if self.entries.contains_key(key) {
            self.hit_count += 1;
            // Promote to back of access_order
            if let Some(pos) = self.access_order.iter().position(|k| k == key) {
                self.access_order.remove(pos);
            }
            self.access_order.push_back(key.clone());
            // Update access metadata
            if let Some(tile) = self.entries.get_mut(key) {
                tile.accessed_at = now;
                tile.access_count += 1;
            }
            self.entries.get(key)
        } else {
            self.miss_count += 1;
            None
        }
    }

    /// Inserts `tile` into the cache, evicting LRU entries as needed.
    pub fn insert(&mut self, tile: CachedTile) {
        // If the key already exists, remove old size first
        if let Some(old) = self.entries.remove(&tile.key) {
            self.current_bytes = self.current_bytes.saturating_sub(old.size_bytes);
            if let Some(pos) = self.access_order.iter().position(|k| k == &old.key) {
                self.access_order.remove(pos);
            }
        }

        let key = tile.key.clone();
        self.current_bytes += tile.size_bytes;
        self.entries.insert(key.clone(), tile);
        self.access_order.push_back(key);

        // Evict until within limits
        while self.entries.len() > self.max_entries
            || (self.current_bytes > self.max_bytes && self.entries.len() > 1)
        {
            self.evict_lru();
        }
    }

    /// Removes `key` from the cache.  Returns `true` if the entry existed.
    pub fn invalidate(&mut self, key: &TileKey) -> bool {
        if let Some(tile) = self.entries.remove(key) {
            self.current_bytes = self.current_bytes.saturating_sub(tile.size_bytes);
            if let Some(pos) = self.access_order.iter().position(|k| k == key) {
                self.access_order.remove(pos);
            }
            true
        } else {
            false
        }
    }

    /// Removes all tiles belonging to `layer`.  Returns the number of entries removed.
    pub fn invalidate_layer(&mut self, layer: &str) -> u64 {
        let keys_to_remove: Vec<TileKey> = self
            .entries
            .keys()
            .filter(|k| k.layer == layer)
            .cloned()
            .collect();
        let count = keys_to_remove.len() as u64;
        for key in keys_to_remove {
            self.invalidate(&key);
        }
        count
    }

    /// Removes all tiles whose zoom level falls within `[min_z, max_z]`.
    /// Returns the number of entries removed.
    pub fn invalidate_zoom_range(&mut self, min_z: u8, max_z: u8) -> u64 {
        let keys_to_remove: Vec<TileKey> = self
            .entries
            .keys()
            .filter(|k| k.z >= min_z && k.z <= max_z)
            .cloned()
            .collect();
        let count = keys_to_remove.len() as u64;
        for key in keys_to_remove {
            self.invalidate(&key);
        }
        count
    }

    /// Returns the cache hit rate in `[0.0, 1.0]`, or `0.0` if no requests.
    #[must_use]
    pub fn hit_rate(&self) -> f64 {
        let total = self.hit_count + self.miss_count;
        if total == 0 {
            0.0
        } else {
            self.hit_count as f64 / total as f64
        }
    }

    /// Returns a snapshot of current cache statistics.
    #[must_use]
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entry_count: self.entries.len(),
            total_bytes: self.current_bytes,
            hit_count: self.hit_count,
            miss_count: self.miss_count,
            eviction_count: self.eviction_count,
            hit_rate: self.hit_rate(),
        }
    }

    /// Evicts the least-recently-used entry (front of `access_order`).
    fn evict_lru(&mut self) {
        if let Some(key) = self.access_order.pop_front() {
            if let Some(tile) = self.entries.remove(&key) {
                self.current_bytes = self.current_bytes.saturating_sub(tile.size_bytes);
                self.eviction_count += 1;
            }
        }
    }
}

// ── TilePrefetcher ────────────────────────────────────────────────────────────

/// Pre-fetches neighboring tiles based on the current access pattern.
pub struct TilePrefetcher {
    /// How many tiles around the current tile to prefetch (Chebyshev radius).
    pub radius: u8,
    /// Prefetch this many zoom levels above and below the current zoom.
    pub max_zoom_delta: u8,
}

impl TilePrefetcher {
    /// Creates a new `TilePrefetcher` with the given `radius` and a default
    /// `max_zoom_delta` of 1.
    pub fn new(radius: u8) -> Self {
        Self {
            radius,
            max_zoom_delta: 1,
        }
    }

    /// Returns the set of tiles to prefetch for a given `key`.
    ///
    /// Includes the ring of neighbors at the same zoom level plus rings at
    /// `zoom ± 1 .. max_zoom_delta`.  The tile itself is excluded.
    pub fn neighbors(&self, key: &TileKey) -> Vec<TileKey> {
        let mut result: Vec<TileKey> = Vec::new();

        // Same zoom level
        let same_zoom_ring = self.ring_at_zoom(key, key.z, self.radius);
        result.extend(same_zoom_ring);

        // Adjacent zoom levels
        for delta in 1..=self.max_zoom_delta {
            if key.z >= delta {
                let lower_zoom = key.z - delta;
                // Scale coordinates down
                let scaled_x = key.x >> delta;
                let scaled_y = key.y >> delta;
                let parent_key = TileKey::new(
                    lower_zoom,
                    scaled_x,
                    scaled_y,
                    key.layer.clone(),
                    key.format.clone(),
                );
                let ring = self.ring_at_zoom(&parent_key, lower_zoom, self.radius);
                for t in ring {
                    if !result.iter().any(|r| r == &t) {
                        result.push(t);
                    }
                }
            }
            let upper_zoom = key.z.saturating_add(delta);
            if upper_zoom != key.z {
                // Scale coordinates up
                let scaled_x = key.x << delta;
                let scaled_y = key.y << delta;
                let child_key = TileKey::new(
                    upper_zoom,
                    scaled_x,
                    scaled_y,
                    key.layer.clone(),
                    key.format.clone(),
                );
                let ring = self.ring_at_zoom(&child_key, upper_zoom, self.radius);
                for t in ring {
                    if !result.iter().any(|r| r == &t) {
                        result.push(t);
                    }
                }
            }
        }

        // Remove the key itself
        result.retain(|t| t != key);
        result
    }

    /// Returns all tiles in a square ring of `radius` around `key` at `zoom`.
    ///
    /// Coordinates are clamped to zero via saturating arithmetic so that boundary
    /// tiles are valid (x, y ≥ 0).
    pub fn ring_at_zoom(&self, key: &TileKey, zoom: u8, radius: u8) -> Vec<TileKey> {
        let r = radius as i64;
        let mut tiles = Vec::new();
        for dx in -r..=r {
            for dy in -r..=r {
                let nx = if dx < 0 {
                    key.x.saturating_sub((-dx) as u32)
                } else {
                    key.x.saturating_add(dx as u32)
                };
                let ny = if dy < 0 {
                    key.y.saturating_sub((-dy) as u32)
                } else {
                    key.y.saturating_add(dy as u32)
                };
                tiles.push(TileKey::new(
                    zoom,
                    nx,
                    ny,
                    key.layer.clone(),
                    key.format.clone(),
                ));
            }
        }
        tiles
    }
}
