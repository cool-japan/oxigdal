//! Geospatial-specific caching strategies and tile caching.

use crate::cache::{CacheManager, StrategyType};
use crate::error::{PwaError, Result};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, Response};

/// Tile coordinate for map tiles.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TileCoord {
    /// Zoom level
    pub z: u32,

    /// X coordinate
    pub x: u32,

    /// Y coordinate
    pub y: u32,
}

impl TileCoord {
    /// Create a new tile coordinate.
    pub fn new(z: u32, x: u32, y: u32) -> Self {
        Self { z, x, y }
    }

    /// Get the maximum tile coordinate for a zoom level.
    pub fn max_coord(z: u32) -> u32 {
        (1 << z) - 1
    }

    /// Check if this coordinate is valid for its zoom level.
    pub fn is_valid(&self) -> bool {
        let max = Self::max_coord(self.z);
        self.x <= max && self.y <= max
    }

    /// Get parent tile at zoom level z-1.
    pub fn parent(&self) -> Option<Self> {
        if self.z == 0 {
            None
        } else {
            Some(Self {
                z: self.z - 1,
                x: self.x / 2,
                y: self.y / 2,
            })
        }
    }

    /// Get child tiles at zoom level z+1.
    pub fn children(&self) -> [Self; 4] {
        let z = self.z + 1;
        let x = self.x * 2;
        let y = self.y * 2;

        [
            Self { z, x, y },
            Self { z, x: x + 1, y },
            Self { z, x, y: y + 1 },
            Self {
                z,
                x: x + 1,
                y: y + 1,
            },
        ]
    }

    /// Convert to standard tile URL format.
    pub fn to_url(&self, base_url: &str) -> String {
        format!("{}/{}/{}/{}", base_url, self.z, self.x, self.y)
    }
}

/// Bounding box for geospatial data.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BoundingBox {
    /// Minimum longitude
    pub min_lon: f64,

    /// Minimum latitude
    pub min_lat: f64,

    /// Maximum longitude
    pub max_lon: f64,

    /// Maximum latitude
    pub max_lat: f64,
}

impl BoundingBox {
    /// Create a new bounding box.
    pub fn new(min_lon: f64, min_lat: f64, max_lon: f64, max_lat: f64) -> Result<Self> {
        if min_lon >= max_lon || min_lat >= max_lat {
            return Err(PwaError::ConfigurationError(
                "Invalid bounding box coordinates".to_string(),
            ));
        }

        Ok(Self {
            min_lon,
            min_lat,
            max_lon,
            max_lat,
        })
    }

    /// Check if a point is inside the bounding box.
    pub fn contains(&self, lon: f64, lat: f64) -> bool {
        lon >= self.min_lon && lon <= self.max_lon && lat >= self.min_lat && lat <= self.max_lat
    }

    /// Get the center point of the bounding box.
    pub fn center(&self) -> (f64, f64) {
        (
            (self.min_lon + self.max_lon) / 2.0,
            (self.min_lat + self.max_lat) / 2.0,
        )
    }

    /// Get the width of the bounding box.
    pub fn width(&self) -> f64 {
        self.max_lon - self.min_lon
    }

    /// Get the height of the bounding box.
    pub fn height(&self) -> f64 {
        self.max_lat - self.min_lat
    }
}

/// Geospatial cache configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeospatialCacheConfig {
    /// Cache name for map tiles
    pub tile_cache_name: String,

    /// Cache name for vector data
    pub vector_cache_name: String,

    /// Cache name for raster data
    pub raster_cache_name: String,

    /// Maximum zoom level to cache
    pub max_zoom: u32,

    /// Minimum zoom level to cache
    pub min_zoom: u32,

    /// Tile cache strategy
    pub tile_strategy: StrategyType,

    /// Vector data cache strategy
    pub vector_strategy: StrategyType,

    /// Raster data cache strategy
    pub raster_strategy: StrategyType,
}

impl Default for GeospatialCacheConfig {
    fn default() -> Self {
        Self {
            tile_cache_name: "geo-tiles".to_string(),
            vector_cache_name: "geo-vector".to_string(),
            raster_cache_name: "geo-raster".to_string(),
            max_zoom: 18,
            min_zoom: 0,
            tile_strategy: StrategyType::CacheFirst,
            vector_strategy: StrategyType::NetworkFirst,
            raster_strategy: StrategyType::CacheFirst,
        }
    }
}

/// Geospatial cache for map tiles and geospatial data.
pub struct GeospatialCache {
    config: GeospatialCacheConfig,
    tile_cache: CacheManager,
    vector_cache: CacheManager,
    raster_cache: CacheManager,
}

impl GeospatialCache {
    /// Create a new geospatial cache.
    pub fn new(config: GeospatialCacheConfig) -> Self {
        let tile_cache = CacheManager::new(&config.tile_cache_name);
        let vector_cache = CacheManager::new(&config.vector_cache_name);
        let raster_cache = CacheManager::new(&config.raster_cache_name);

        Self {
            config,
            tile_cache,
            vector_cache,
            raster_cache,
        }
    }

    /// Create a geospatial cache with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(GeospatialCacheConfig::default())
    }

    /// Cache a map tile.
    pub async fn cache_tile(&self, coord: &TileCoord, url: &str) -> Result<Response> {
        if !self.is_zoom_cacheable(coord.z) {
            return self.fetch_tile(url).await;
        }

        let request = self.create_tile_request(url)?;

        // Use cache-first strategy for tiles
        if let Some(response) = self.tile_cache.match_request(&request).await? {
            return Ok(response);
        }

        let response = self.fetch_tile(url).await?;
        self.tile_cache.put(&request, &response).await?;

        Ok(response)
    }

    /// Prefetch tiles for a bounding box and zoom range.
    pub async fn prefetch_tiles(
        &self,
        bbox: &BoundingBox,
        zoom_range: std::ops::Range<u32>,
        base_url: &str,
    ) -> Result<Vec<TileCoord>> {
        let mut cached_tiles = Vec::new();

        for z in zoom_range {
            if !self.is_zoom_cacheable(z) {
                continue;
            }

            let tiles = self.get_tiles_in_bbox(bbox, z);

            for coord in tiles {
                let url = coord.to_url(base_url);
                if self.cache_tile(&coord, &url).await.is_ok() {
                    cached_tiles.push(coord);
                }
            }
        }

        Ok(cached_tiles)
    }

    /// Get all tile coordinates that intersect a bounding box at a given zoom.
    pub fn get_tiles_in_bbox(&self, bbox: &BoundingBox, zoom: u32) -> Vec<TileCoord> {
        let mut tiles = Vec::new();

        // Convert lat/lon to tile coordinates
        let min_tile = Self::lonlat_to_tile(bbox.min_lon, bbox.max_lat, zoom);
        let max_tile = Self::lonlat_to_tile(bbox.max_lon, bbox.min_lat, zoom);

        for x in min_tile.x..=max_tile.x {
            for y in min_tile.y..=max_tile.y {
                let coord = TileCoord::new(zoom, x, y);
                if coord.is_valid() {
                    tiles.push(coord);
                }
            }
        }

        tiles
    }

    /// Convert lon/lat to tile coordinates.
    fn lonlat_to_tile(lon: f64, lat: f64, zoom: u32) -> TileCoord {
        let n = 2_f64.powi(zoom as i32);
        let x = ((lon + 180.0) / 360.0 * n) as u32;
        let lat_rad = lat.to_radians();
        let y = ((1.0 - lat_rad.tan().asinh() / std::f64::consts::PI) / 2.0 * n) as u32;

        TileCoord::new(zoom, x, y)
    }

    /// Cache vector data (GeoJSON, etc.).
    pub async fn cache_vector_data(&self, url: &str, data: &Response) -> Result<()> {
        let request = self.create_request(url)?;
        self.vector_cache.put(&request, data).await
    }

    /// Get cached vector data.
    pub async fn get_vector_data(&self, url: &str) -> Result<Option<Response>> {
        let request = self.create_request(url)?;
        self.vector_cache.match_request(&request).await
    }

    /// Cache raster data (COG, GeoTIFF, etc.).
    pub async fn cache_raster_data(&self, url: &str, data: &Response) -> Result<()> {
        let request = self.create_request(url)?;
        self.raster_cache.put(&request, data).await
    }

    /// Get cached raster data.
    pub async fn get_raster_data(&self, url: &str) -> Result<Option<Response>> {
        let request = self.create_request(url)?;
        self.raster_cache.match_request(&request).await
    }

    /// Clear tile cache.
    pub async fn clear_tiles(&self) -> Result<()> {
        self.tile_cache.clear().await
    }

    /// Clear vector cache.
    pub async fn clear_vector(&self) -> Result<()> {
        self.vector_cache.clear().await
    }

    /// Clear raster cache.
    pub async fn clear_raster(&self) -> Result<()> {
        self.raster_cache.clear().await
    }

    /// Clear all geospatial caches.
    pub async fn clear_all(&self) -> Result<()> {
        self.clear_tiles().await?;
        self.clear_vector().await?;
        self.clear_raster().await?;
        Ok(())
    }

    /// Check if a zoom level is cacheable.
    fn is_zoom_cacheable(&self, zoom: u32) -> bool {
        zoom >= self.config.min_zoom && zoom <= self.config.max_zoom
    }

    /// Create a request for a URL.
    fn create_request(&self, url: &str) -> Result<Request> {
        Request::new_with_str(url)
            .map_err(|e| PwaError::InvalidUrl(format!("Failed to create request: {:?}", e)))
    }

    /// Create a tile request.
    fn create_tile_request(&self, url: &str) -> Result<Request> {
        self.create_request(url)
    }

    /// Fetch a tile from the network.
    async fn fetch_tile(&self, url: &str) -> Result<Response> {
        let window = web_sys::window()
            .ok_or_else(|| PwaError::InvalidState("No window available".to_string()))?;

        let promise = window.fetch_with_str(url);
        let result = JsFuture::from(promise)
            .await
            .map_err(|e| PwaError::FetchFailed(format!("Tile fetch failed: {:?}", e)))?;

        result
            .dyn_into::<Response>()
            .map_err(|_| PwaError::FetchFailed("Invalid response object".to_string()))
    }

    /// Get the configuration.
    pub fn config(&self) -> &GeospatialCacheConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_coord() {
        let coord = TileCoord::new(10, 512, 512);
        assert_eq!(coord.z, 10);
        assert_eq!(coord.x, 512);
        assert_eq!(coord.y, 512);
        assert!(coord.is_valid());
    }

    #[test]
    fn test_tile_parent() {
        let coord = TileCoord::new(10, 512, 512);
        let parent = coord
            .parent()
            .ok_or("")
            .unwrap_or_else(|_| TileCoord::new(0, 0, 0));
        assert_eq!(parent.z, 9);
        assert_eq!(parent.x, 256);
        assert_eq!(parent.y, 256);
    }

    #[test]
    fn test_tile_children() {
        let coord = TileCoord::new(5, 10, 10);
        let children = coord.children();
        assert_eq!(children.len(), 4);
        assert_eq!(children[0], TileCoord::new(6, 20, 20));
        assert_eq!(children[1], TileCoord::new(6, 21, 20));
        assert_eq!(children[2], TileCoord::new(6, 20, 21));
        assert_eq!(children[3], TileCoord::new(6, 21, 21));
    }

    #[test]
    fn test_tile_url() {
        let coord = TileCoord::new(10, 512, 512);
        let url = coord.to_url("https://tiles.example.com");
        assert_eq!(url, "https://tiles.example.com/10/512/512");
    }

    #[test]
    fn test_bounding_box() -> Result<()> {
        let bbox = BoundingBox::new(-180.0, -85.0, 180.0, 85.0)?;
        assert_eq!(bbox.width(), 360.0);
        assert_eq!(bbox.height(), 170.0);

        let (center_lon, center_lat) = bbox.center();
        assert_eq!(center_lon, 0.0);
        assert_eq!(center_lat, 0.0);

        assert!(bbox.contains(0.0, 0.0));
        assert!(bbox.contains(-100.0, 50.0));
        assert!(!bbox.contains(0.0, 90.0));

        Ok(())
    }

    #[test]
    fn test_lonlat_to_tile() {
        let coord = GeospatialCache::lonlat_to_tile(0.0, 0.0, 0);
        assert_eq!(coord.z, 0);

        let coord = GeospatialCache::lonlat_to_tile(0.0, 0.0, 1);
        assert_eq!(coord.z, 1);
    }

    #[test]
    fn test_geospatial_cache_config() {
        let config = GeospatialCacheConfig::default();
        assert_eq!(config.tile_cache_name, "geo-tiles");
        assert_eq!(config.max_zoom, 18);
        assert_eq!(config.min_zoom, 0);
    }
}
