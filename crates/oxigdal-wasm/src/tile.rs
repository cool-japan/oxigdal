//! Tile management and caching system
//!
//! This module provides comprehensive tile coordinate systems, LRU caching,
//! pyramid management, prefetching, and memory management for WASM geospatial applications.
//!
//! # Overview
//!
//! The tile module implements the core tile management infrastructure for oxigdal-wasm:
//!
//! - **Tile Coordinates**: XYZ tile coordinate system (level, x, y)
//! - **Tile Pyramids**: Multi-resolution tile pyramids for efficient zooming
//! - **LRU Caching**: Least Recently Used cache with configurable size limits
//! - **Prefetching**: Intelligent prefetching strategies for smooth interaction
//! - **Memory Management**: Automatic eviction when memory limits are reached
//!
//! # Tile Coordinate System
//!
//! Tiles are addressed using a standard XYZ coordinate system:
//!
//! ```text
//! Level 0:     [0,0,0]                  (1 tile)
//!
//! Level 1:  [1,0,0] [1,1,0]             (4 tiles)
//!           [1,0,1] [1,1,1]
//!
//! Level 2:  [2,0,0] [2,1,0] [2,2,0] [2,3,0]  (16 tiles)
//!           [2,0,1] [2,1,1] [2,2,1] [2,3,1]
//!           [2,0,2] [2,1,2] [2,2,2] [2,3,2]
//!           [2,0,3] [2,1,3] [2,2,3] [2,3,3]
//! ```
//!
//! Each level has 4^level tiles. The tree structure allows efficient
//! parent/child relationships for multi-resolution rendering.
//!
//! # Tile Pyramid Structure
//!
//! A tile pyramid represents the complete multi-resolution structure:
//!
//! ```rust
//! use oxigdal_wasm::TilePyramid;
//!
//! // Create pyramid for a 4096x2048 image with 256x256 tiles
//! let pyramid = TilePyramid::new(4096, 2048, 256, 256);
//!
//! // Level 0: 16x8 tiles (full resolution)
//! // Level 1: 8x4 tiles (2x downsampled)
//! // Level 2: 4x2 tiles (4x downsampled)
//! // Level 3: 2x1 tiles (8x downsampled)
//! // Level 4: 1x1 tile (16x downsampled)
//!
//! println!("Levels: {}", pyramid.num_levels);
//! println!("Total tiles: {}", pyramid.total_tiles());
//! ```
//!
//! # LRU Cache Implementation
//!
//! The cache uses a Least Recently Used eviction policy:
//!
//! 1. Tiles are stored in a HashMap for O(1) lookups
//! 2. Access order is tracked in a VecDeque
//! 3. When cache is full, oldest tile is evicted
//! 4. Cache hits update the access time
//!
//! ```rust
//! use oxigdal_wasm::{TileCache, TileCoord};
//!
//! let mut cache = TileCache::new(100 * 1024 * 1024); // 100 MB
//!
//! let coord = TileCoord::new(0, 0, 0);
//! let tile_data = vec![0u8; 256 * 256 * 4];
//!
//! // Cache the tile
//! cache.put(coord, tile_data.clone(), 0.0).expect("Put failed");
//!
//! // Retrieve from cache
//! let cached = cache.get(&coord, 1.0).expect("Cache miss");
//!
//! // Check statistics
//! let stats = cache.stats();
//! println!("Hit rate: {:.2}%", stats.hit_rate() * 100.0);
//! ```
//!
//! # Prefetching Strategies
//!
//! Multiple prefetching strategies are supported:
//!
//! ## Neighbors
//! Prefetches 8 immediately adjacent tiles:
//! ```text
//! [NW] [N]  [NE]
//! [W]  [C]  [E]
//! [SW] [S]  [SE]
//! ```
//!
//! ## Radius
//! Prefetches all tiles within a given radius (Euclidean distance).
//!
//! ## Pyramid
//! Prefetches parent tile (for zoom out) and 4 child tiles (for zoom in).
//!
//! ## Directional
//! Prefetches tiles in a specific direction (for panning).
//!
//! # Performance Characteristics
//!
//! - Cache lookup: O(1) average, O(n) worst case
//! - Cache insertion: O(1) average, O(n) worst case
//! - Cache eviction: O(1)
//! - Pyramid traversal: O(log n) for parent/child lookup
//!
//! Memory overhead:
//! - TileCoord: 12 bytes (3 u32 values)
//! - CachedTile: ~48 bytes + data size
//! - Cache entry: ~80 bytes + data size
//!
//! # Example: Complete Tile Loading
//!
//! ```rust
//! use oxigdal_wasm::{TileCache, TileCoord, TilePrefetcher, PrefetchStrategy, TilePyramid};
//!
//! // Setup
//! let mut cache = TileCache::new(50 * 1024 * 1024); // 50 MB cache
//! let pyramid = TilePyramid::new(4096, 4096, 256, 256);
//! let prefetcher = TilePrefetcher::new(
//!     PrefetchStrategy::Neighbors,
//!     pyramid.clone()
//! );
//!
//! // Load center tile
//! let center = TileCoord::new(0, 8, 8); // Center of level 0
//! let tile_data = vec![0u8; 256 * 256 * 4]; // Simulated tile data
//!
//! cache.put(center, tile_data, 0.0).expect("Cache put failed");
//!
//! // Get tiles to prefetch
//! let to_prefetch = prefetcher.prefetch_for(&center);
//! println!("Prefetching {} tiles", to_prefetch.len());
//!
//! // Load prefetch tiles (in real code, fetch from network)
//! for coord in to_prefetch {
//!     if !cache.contains(&coord) {
//!         // Fetch and cache tile
//!         // let tile = fetch_tile(coord).await;
//!         // cache.put(coord, tile, timestamp)?;
//!     }
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use wasm_bindgen::prelude::*;

use crate::error::{TileCacheError, WasmError, WasmResult};

/// Maximum cache size in bytes (default: 100MB)
pub const DEFAULT_MAX_CACHE_SIZE: usize = 100 * 1024 * 1024;

/// Default tile size in pixels
#[allow(dead_code)]
pub const DEFAULT_TILE_SIZE: u32 = 256;

/// Tile coordinate in a pyramid
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TileCoord {
    /// Zoom level (0 = most zoomed out)
    pub level: u32,
    /// Column index (x)
    pub x: u32,
    /// Row index (y)
    pub y: u32,
}

impl TileCoord {
    /// Creates a new tile coordinate
    pub const fn new(level: u32, x: u32, y: u32) -> Self {
        Self { level, x, y }
    }

    /// Returns the parent tile coordinate (one level up)
    pub const fn parent(self) -> Option<Self> {
        if self.level == 0 {
            return None;
        }

        Some(Self {
            level: self.level - 1,
            x: self.x / 2,
            y: self.y / 2,
        })
    }

    /// Returns the four child tile coordinates (one level down)
    pub const fn children(self) -> [Self; 4] {
        let level = self.level + 1;
        let x2 = self.x * 2;
        let y2 = self.y * 2;

        [
            Self {
                level,
                x: x2,
                y: y2,
            },
            Self {
                level,
                x: x2 + 1,
                y: y2,
            },
            Self {
                level,
                x: x2,
                y: y2 + 1,
            },
            Self {
                level,
                x: x2 + 1,
                y: y2 + 1,
            },
        ]
    }

    /// Returns the tile key for caching
    pub fn key(&self) -> String {
        format!("{}/{}/{}", self.level, self.x, self.y)
    }

    /// Parses a tile key
    pub fn from_key(key: &str) -> Option<Self> {
        let parts: Vec<&str> = key.split('/').collect();
        if parts.len() != 3 {
            return None;
        }

        let level = parts[0].parse().ok()?;
        let x = parts[1].parse().ok()?;
        let y = parts[2].parse().ok()?;

        Some(Self::new(level, x, y))
    }

    /// Checks if this tile is valid for the given bounds
    pub const fn is_valid(&self, max_level: u32, max_x: u32, max_y: u32) -> bool {
        self.level <= max_level && self.x < max_x && self.y < max_y
    }

    /// Returns neighboring tile coordinates
    pub const fn neighbors(&self) -> [Option<Self>; 8] {
        let level = self.level;
        let x = self.x;
        let y = self.y;

        [
            // Top-left
            if x > 0 && y > 0 {
                Some(Self::new(level, x - 1, y - 1))
            } else {
                None
            },
            // Top
            if y > 0 {
                Some(Self::new(level, x, y - 1))
            } else {
                None
            },
            // Top-right
            if y > 0 {
                Some(Self::new(level, x + 1, y - 1))
            } else {
                None
            },
            // Left
            if x > 0 {
                Some(Self::new(level, x - 1, y))
            } else {
                None
            },
            // Right
            Some(Self::new(level, x + 1, y)),
            // Bottom-left
            if x > 0 {
                Some(Self::new(level, x - 1, y + 1))
            } else {
                None
            },
            // Bottom
            Some(Self::new(level, x, y + 1)),
            // Bottom-right
            Some(Self::new(level, x + 1, y + 1)),
        ]
    }
}

/// Tile bounds in the pyramid
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileBounds {
    /// Minimum x coordinate
    pub min_x: u32,
    /// Minimum y coordinate
    pub min_y: u32,
    /// Maximum x coordinate (exclusive)
    pub max_x: u32,
    /// Maximum y coordinate (exclusive)
    pub max_y: u32,
}

impl TileBounds {
    /// Creates new tile bounds
    pub const fn new(min_x: u32, min_y: u32, max_x: u32, max_y: u32) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    /// Checks if a coordinate is within bounds
    pub const fn contains(&self, coord: &TileCoord) -> bool {
        coord.x >= self.min_x
            && coord.x < self.max_x
            && coord.y >= self.min_y
            && coord.y < self.max_y
    }

    /// Returns the number of tiles in these bounds
    pub const fn count(&self) -> u64 {
        (self.max_x - self.min_x) as u64 * (self.max_y - self.min_y) as u64
    }

    /// Returns an iterator over tile coordinates in these bounds
    pub fn iter(&self) -> TileBoundsIter {
        TileBoundsIter {
            bounds: *self,
            current_x: self.min_x,
            current_y: self.min_y,
        }
    }
}

/// Iterator over tile coordinates in bounds
pub struct TileBoundsIter {
    bounds: TileBounds,
    current_x: u32,
    current_y: u32,
}

impl Iterator for TileBoundsIter {
    type Item = (u32, u32);

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_y >= self.bounds.max_y {
            return None;
        }

        let result = (self.current_x, self.current_y);

        self.current_x += 1;
        if self.current_x >= self.bounds.max_x {
            self.current_x = self.bounds.min_x;
            self.current_y += 1;
        }

        Some(result)
    }
}

/// Tile pyramid metadata
#[derive(Debug, Clone)]
pub struct TilePyramid {
    /// Image width in pixels
    pub width: u64,
    /// Image height in pixels
    pub height: u64,
    /// Tile width in pixels
    pub tile_width: u32,
    /// Tile height in pixels
    pub tile_height: u32,
    /// Number of pyramid levels
    pub num_levels: u32,
    /// Tiles per level (width, height)
    pub tiles_per_level: Vec<(u32, u32)>,
}

impl TilePyramid {
    /// Creates a new tile pyramid
    pub fn new(width: u64, height: u64, tile_width: u32, tile_height: u32) -> Self {
        let mut tiles_per_level = Vec::new();
        let mut level_width = width;
        let mut level_height = height;
        let mut num_levels = 0;

        while level_width > 0 && level_height > 0 {
            let tiles_x = level_width.div_ceil(u64::from(tile_width)) as u32;
            let tiles_y = level_height.div_ceil(u64::from(tile_height)) as u32;
            tiles_per_level.push((tiles_x, tiles_y));
            num_levels += 1;

            // Stop when we reach a single tile (pyramid top)
            if tiles_x == 1 && tiles_y == 1 {
                break;
            }

            level_width /= 2;
            level_height /= 2;
        }

        Self {
            width,
            height,
            tile_width,
            tile_height,
            num_levels,
            tiles_per_level,
        }
    }

    /// Returns the number of tiles at a given level
    pub fn tiles_at_level(&self, level: u32) -> Option<(u32, u32)> {
        self.tiles_per_level.get(level as usize).copied()
    }

    /// Returns the tile bounds for a given level
    pub fn bounds_at_level(&self, level: u32) -> Option<TileBounds> {
        self.tiles_at_level(level)
            .map(|(tiles_x, tiles_y)| TileBounds::new(0, 0, tiles_x, tiles_y))
    }

    /// Checks if a tile coordinate is valid
    pub fn is_valid_coord(&self, coord: &TileCoord) -> bool {
        if coord.level >= self.num_levels {
            return false;
        }

        if let Some((tiles_x, tiles_y)) = self.tiles_at_level(coord.level) {
            coord.x < tiles_x && coord.y < tiles_y
        } else {
            false
        }
    }

    /// Returns the total number of tiles in the pyramid
    pub fn total_tiles(&self) -> u64 {
        self.tiles_per_level
            .iter()
            .map(|(x, y)| u64::from(*x) * u64::from(*y))
            .sum()
    }
}

/// Cached tile data
#[derive(Debug, Clone)]
pub struct CachedTile {
    /// Tile coordinate
    pub coord: TileCoord,
    /// Tile data
    pub data: Vec<u8>,
    /// Size in bytes
    pub size: usize,
    /// Access timestamp (for LRU)
    pub last_access: f64,
    /// Load timestamp
    pub loaded_at: f64,
    /// Number of accesses
    pub access_count: u64,
}

impl CachedTile {
    /// Creates a new cached tile
    pub fn new(coord: TileCoord, data: Vec<u8>, timestamp: f64) -> Self {
        let size = data.len();
        Self {
            coord,
            data,
            size,
            last_access: timestamp,
            loaded_at: timestamp,
            access_count: 1,
        }
    }

    /// Updates access information
    pub fn access(&mut self, timestamp: f64) {
        self.last_access = timestamp;
        self.access_count += 1;
    }
}

/// LRU tile cache
pub struct TileCache {
    /// Cache entries
    cache: HashMap<String, CachedTile>,
    /// LRU queue (tile keys in access order)
    lru_queue: VecDeque<String>,
    /// Current cache size in bytes
    current_size: usize,
    /// Maximum cache size in bytes
    max_size: usize,
    /// Cache hit count
    hit_count: u64,
    /// Cache miss count
    miss_count: u64,
}

impl TileCache {
    /// Creates a new tile cache
    pub fn new(max_size: usize) -> Self {
        Self {
            cache: HashMap::new(),
            lru_queue: VecDeque::new(),
            current_size: 0,
            max_size,
            hit_count: 0,
            miss_count: 0,
        }
    }

    /// Creates a new tile cache with default size
    pub fn with_default_size() -> Self {
        Self::new(DEFAULT_MAX_CACHE_SIZE)
    }

    /// Gets a tile from the cache
    pub fn get(&mut self, coord: &TileCoord, timestamp: f64) -> Option<Vec<u8>> {
        let key = coord.key();

        if let Some(tile) = self.cache.get_mut(&key) {
            tile.access(timestamp);
            self.hit_count += 1;

            // Move to end of LRU queue
            if let Some(pos) = self.lru_queue.iter().position(|k| k == &key) {
                self.lru_queue.remove(pos);
            }
            self.lru_queue.push_back(key);

            Some(tile.data.clone())
        } else {
            self.miss_count += 1;
            None
        }
    }

    /// Puts a tile into the cache
    pub fn put(&mut self, coord: TileCoord, data: Vec<u8>, timestamp: f64) -> WasmResult<()> {
        let key = coord.key();
        let tile_size = data.len();

        // Evict tiles if necessary when adding would exceed or reach capacity
        // Using >= because cache should maintain strict size limit
        while !self.lru_queue.is_empty() && self.current_size + tile_size >= self.max_size {
            self.evict_oldest()?;
        }

        // Check if we can fit the tile
        if tile_size > self.max_size {
            return Err(WasmError::TileCache(TileCacheError::Full {
                current_size: self.current_size,
                max_size: self.max_size,
            }));
        }

        // Add or update tile
        if let Some(old_tile) = self.cache.remove(&key) {
            self.current_size -= old_tile.size;
            if let Some(pos) = self.lru_queue.iter().position(|k| k == &key) {
                self.lru_queue.remove(pos);
            }
        }

        let tile = CachedTile::new(coord, data, timestamp);
        self.current_size += tile_size;
        self.cache.insert(key.clone(), tile);
        self.lru_queue.push_back(key);

        Ok(())
    }

    /// Evicts the oldest tile from the cache
    fn evict_oldest(&mut self) -> WasmResult<()> {
        if let Some(key) = self.lru_queue.pop_front() {
            if let Some(tile) = self.cache.remove(&key) {
                self.current_size -= tile.size;
                Ok(())
            } else {
                Err(WasmError::TileCache(TileCacheError::EvictionFailed {
                    message: format!("Tile {key} not found in cache"),
                }))
            }
        } else {
            Err(WasmError::TileCache(TileCacheError::EvictionFailed {
                message: "No tiles to evict".to_string(),
            }))
        }
    }

    /// Checks if a tile is in the cache
    pub fn contains(&self, coord: &TileCoord) -> bool {
        self.cache.contains_key(&coord.key())
    }

    /// Removes a tile from the cache
    pub fn remove(&mut self, coord: &TileCoord) -> Option<Vec<u8>> {
        let key = coord.key();
        if let Some(tile) = self.cache.remove(&key) {
            self.current_size -= tile.size;
            if let Some(pos) = self.lru_queue.iter().position(|k| k == &key) {
                self.lru_queue.remove(pos);
            }
            Some(tile.data)
        } else {
            None
        }
    }

    /// Clears the entire cache
    pub fn clear(&mut self) {
        self.cache.clear();
        self.lru_queue.clear();
        self.current_size = 0;
    }

    /// Returns cache statistics
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            current_size: self.current_size,
            max_size: self.max_size,
            entry_count: self.cache.len(),
            hit_count: self.hit_count,
            miss_count: self.miss_count,
        }
    }

    /// Returns the cache hit rate
    pub fn hit_rate(&self) -> f64 {
        let total = self.hit_count + self.miss_count;
        if total == 0 {
            0.0
        } else {
            self.hit_count as f64 / total as f64
        }
    }

    /// Prefetches tiles in the given coordinates
    pub fn prefetch_list(&self, coords: &[TileCoord]) -> Vec<TileCoord> {
        coords
            .iter()
            .filter(|coord| !self.contains(coord))
            .copied()
            .collect()
    }
}

/// Cache statistics
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CacheStats {
    /// Current cache size in bytes
    pub current_size: usize,
    /// Maximum cache size in bytes
    pub max_size: usize,
    /// Number of cached entries
    pub entry_count: usize,
    /// Number of cache hits
    pub hit_count: u64,
    /// Number of cache misses
    pub miss_count: u64,
}

impl CacheStats {
    /// Returns the cache hit rate
    pub fn hit_rate(&self) -> f64 {
        let total = self.hit_count + self.miss_count;
        if total == 0 {
            0.0
        } else {
            self.hit_count as f64 / total as f64
        }
    }

    /// Returns the cache utilization as a fraction
    pub fn utilization(&self) -> f64 {
        if self.max_size == 0 {
            0.0
        } else {
            self.current_size as f64 / self.max_size as f64
        }
    }
}

/// Prefetching strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefetchStrategy {
    /// No prefetching
    None,
    /// Prefetch immediate neighbors
    Neighbors,
    /// Prefetch in a radius
    Radius(u32),
    /// Prefetch pyramid (parent and children)
    Pyramid,
    /// Prefetch along a direction
    Directional {
        /// X direction delta
        dx: i32,
        /// Y direction delta
        dy: i32,
        /// Number of tiles to prefetch
        count: u32,
    },
}

/// Tile prefetcher
pub struct TilePrefetcher {
    /// Prefetch strategy
    strategy: PrefetchStrategy,
    /// Pyramid metadata
    pyramid: TilePyramid,
}

impl TilePrefetcher {
    /// Creates a new tile prefetcher
    pub const fn new(strategy: PrefetchStrategy, pyramid: TilePyramid) -> Self {
        Self { strategy, pyramid }
    }

    /// Returns tiles to prefetch for a given center tile
    pub fn prefetch_for(&self, center: &TileCoord) -> Vec<TileCoord> {
        match self.strategy {
            PrefetchStrategy::None => vec![],
            PrefetchStrategy::Neighbors => self.prefetch_neighbors(center),
            PrefetchStrategy::Radius(radius) => self.prefetch_radius(center, radius),
            PrefetchStrategy::Pyramid => self.prefetch_pyramid(center),
            PrefetchStrategy::Directional { dx, dy, count } => {
                self.prefetch_directional(center, dx, dy, count)
            }
        }
    }

    /// Prefetches immediate neighbors
    fn prefetch_neighbors(&self, center: &TileCoord) -> Vec<TileCoord> {
        center
            .neighbors()
            .iter()
            .filter_map(|&n| n)
            .filter(|coord| self.pyramid.is_valid_coord(coord))
            .collect()
    }

    /// Prefetches tiles in a radius
    fn prefetch_radius(&self, center: &TileCoord, radius: u32) -> Vec<TileCoord> {
        let mut tiles = Vec::new();
        let radius_i32 = radius as i32;

        for dy in -radius_i32..=radius_i32 {
            for dx in -radius_i32..=radius_i32 {
                // Skip the center tile itself
                if dx == 0 && dy == 0 {
                    continue;
                }

                // Calculate Euclidean distance
                let dist_sq = (dx * dx + dy * dy) as f64;
                if dist_sq > (radius as f64 * radius as f64) {
                    continue;
                }

                let x = center.x as i32 + dx;
                let y = center.y as i32 + dy;

                if x >= 0 && y >= 0 {
                    let coord = TileCoord::new(center.level, x as u32, y as u32);
                    if self.pyramid.is_valid_coord(&coord) {
                        tiles.push(coord);
                    }
                }
            }
        }

        tiles
    }

    /// Prefetches parent and children tiles
    fn prefetch_pyramid(&self, center: &TileCoord) -> Vec<TileCoord> {
        let mut tiles = Vec::new();

        // Add parent
        if let Some(parent) = center.parent() {
            if self.pyramid.is_valid_coord(&parent) {
                tiles.push(parent);
            }
        }

        // Add children
        for child in center.children() {
            if self.pyramid.is_valid_coord(&child) {
                tiles.push(child);
            }
        }

        tiles
    }

    /// Prefetches tiles in a direction
    fn prefetch_directional(
        &self,
        center: &TileCoord,
        dx: i32,
        dy: i32,
        count: u32,
    ) -> Vec<TileCoord> {
        let mut tiles = Vec::new();

        for i in 1..=count {
            let x = center.x as i32 + dx * i as i32;
            let y = center.y as i32 + dy * i as i32;

            if x >= 0 && y >= 0 {
                let coord = TileCoord::new(center.level, x as u32, y as u32);
                if self.pyramid.is_valid_coord(&coord) {
                    tiles.push(coord);
                } else {
                    break;
                }
            }
        }

        tiles
    }
}

/// WASM bindings for tile management
#[wasm_bindgen]
pub struct WasmTileCache {
    cache: TileCache,
}

#[wasm_bindgen]
impl WasmTileCache {
    /// Creates a new tile cache
    #[wasm_bindgen(constructor)]
    pub fn new(max_size_mb: usize) -> Self {
        let max_size = max_size_mb * 1024 * 1024;
        Self {
            cache: TileCache::new(max_size),
        }
    }

    /// Gets cache statistics as JSON
    #[wasm_bindgen(js_name = getStats)]
    pub fn get_stats(&self) -> String {
        serde_json::to_string(&self.cache.stats()).unwrap_or_default()
    }

    /// Clears the cache
    #[wasm_bindgen]
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Returns the cache hit rate
    #[wasm_bindgen(js_name = hitRate)]
    pub fn hit_rate(&self) -> f64 {
        self.cache.hit_rate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_coord() {
        let coord = TileCoord::new(5, 10, 20);
        assert_eq!(coord.level, 5);
        assert_eq!(coord.x, 10);
        assert_eq!(coord.y, 20);
        assert_eq!(coord.key(), "5/10/20");
    }

    #[test]
    fn test_tile_coord_parent() {
        let coord = TileCoord::new(5, 10, 20);
        let parent = coord.parent().expect("Should have parent");
        assert_eq!(parent.level, 4);
        assert_eq!(parent.x, 5);
        assert_eq!(parent.y, 10);

        let root = TileCoord::new(0, 0, 0);
        assert!(root.parent().is_none());
    }

    #[test]
    fn test_tile_coord_children() {
        let coord = TileCoord::new(5, 10, 20);
        let children = coord.children();
        assert_eq!(children.len(), 4);
        assert_eq!(children[0], TileCoord::new(6, 20, 40));
        assert_eq!(children[1], TileCoord::new(6, 21, 40));
        assert_eq!(children[2], TileCoord::new(6, 20, 41));
        assert_eq!(children[3], TileCoord::new(6, 21, 41));
    }

    #[test]
    fn test_tile_pyramid() {
        let pyramid = TilePyramid::new(4096, 2048, 256, 256);
        assert_eq!(pyramid.tile_width, 256);
        assert_eq!(pyramid.tile_height, 256);

        let (tiles_x, tiles_y) = pyramid.tiles_at_level(0).expect("Level 0 exists");
        assert_eq!(tiles_x, 16); // 4096 / 256
        assert_eq!(tiles_y, 8); // 2048 / 256
    }

    #[test]
    fn test_tile_cache() {
        let mut cache = TileCache::new(1024);
        let coord = TileCoord::new(0, 0, 0);
        let data = vec![1, 2, 3, 4];

        // Put and get
        cache
            .put(coord, data.clone(), 0.0)
            .expect("Put should succeed");
        let retrieved = cache.get(&coord, 0.0).expect("Should find tile");
        assert_eq!(retrieved, data);

        // Check stats
        let stats = cache.stats();
        assert_eq!(stats.entry_count, 1);
        assert_eq!(stats.hit_count, 1);
    }

    #[test]
    fn test_cache_eviction() {
        let mut cache = TileCache::new(10); // Very small cache
        let coord1 = TileCoord::new(0, 0, 0);
        let coord2 = TileCoord::new(0, 0, 1);

        cache
            .put(coord1, vec![1, 2, 3, 4, 5], 0.0)
            .expect("Put should succeed");
        cache
            .put(coord2, vec![6, 7, 8, 9, 10], 1.0)
            .expect("Put should succeed");

        // First tile should be evicted
        assert!(!cache.contains(&coord1));
        assert!(cache.contains(&coord2));
    }

    #[test]
    fn test_tile_bounds() {
        let bounds = TileBounds::new(0, 0, 10, 10);
        assert_eq!(bounds.count(), 100);

        let coord_in = TileCoord::new(0, 5, 5);
        let coord_out = TileCoord::new(0, 15, 15);

        assert!(bounds.contains(&coord_in));
        assert!(!bounds.contains(&coord_out));
    }

    #[test]
    fn test_prefetch_neighbors() {
        let pyramid = TilePyramid::new(1024, 1024, 256, 256);
        let prefetcher = TilePrefetcher::new(PrefetchStrategy::Neighbors, pyramid);

        let center = TileCoord::new(0, 1, 1);
        let to_prefetch = prefetcher.prefetch_for(&center);

        // Should prefetch up to 8 neighbors
        assert!(!to_prefetch.is_empty());
        assert!(to_prefetch.len() <= 8);
    }

    #[test]
    fn test_from_key() {
        let coord = TileCoord::new(5, 10, 20);
        let key = coord.key();
        let parsed = TileCoord::from_key(&key).expect("Should parse");
        assert_eq!(parsed, coord);

        assert!(TileCoord::from_key("invalid").is_none());
        assert!(TileCoord::from_key("1/2").is_none());
    }
}
