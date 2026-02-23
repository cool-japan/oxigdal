//! XYZ tile handlers
//!
//! Simple tile serving compatible with Leaflet, MapLibre, and other web mapping libraries.
//! Provides a standard {z}/{x}/{y} endpoint for tile requests.

use crate::cache::{CacheKey, TileCache};
use crate::config::ImageFormat;
use crate::dataset_registry::DatasetRegistry;
use axum::{
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, trace};

/// XYZ tile errors
#[derive(Debug, Error)]
pub enum TileError {
    /// Layer not found
    #[error("Layer not found: {0}")]
    LayerNotFound(String),

    /// Invalid coordinates
    #[error("Invalid tile coordinates")]
    InvalidCoordinates,

    /// Tile out of bounds
    #[error("Tile coordinates out of bounds")]
    TileOutOfBounds,

    /// Rendering error
    #[error("Rendering error: {0}")]
    Rendering(String),

    /// Registry error
    #[error("Registry error: {0}")]
    Registry(#[from] crate::dataset_registry::RegistryError),

    /// Unsupported format
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),
}

impl IntoResponse for TileError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            TileError::LayerNotFound(_) | TileError::TileOutOfBounds => {
                (StatusCode::NOT_FOUND, self.to_string())
            }
            TileError::InvalidCoordinates => (StatusCode::BAD_REQUEST, self.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        (status, [(header::CONTENT_TYPE, "text/plain")], message).into_response()
    }
}

/// Shared tile server state
#[derive(Clone)]
pub struct TileState {
    /// Dataset registry
    pub registry: DatasetRegistry,

    /// Tile cache
    pub cache: TileCache,
}

/// Tile path parameters
#[derive(Debug)]
pub struct TilePath {
    /// Layer name
    pub layer: String,

    /// Zoom level
    pub z: u8,

    /// Tile X coordinate
    pub x: u32,

    /// Tile Y coordinate
    pub y: u32,

    /// Image format (extension)
    pub format: String,
}

/// Web Mercator tile bounds calculator
pub struct WebMercatorBounds {
    /// Zoom level
    pub z: u8,

    /// Tile X coordinate
    pub x: u32,

    /// Tile Y coordinate
    pub y: u32,
}

impl WebMercatorBounds {
    /// Create new bounds calculator
    pub fn new(z: u8, x: u32, y: u32) -> Self {
        Self { z, x, y }
    }

    /// Get the number of tiles at this zoom level
    pub fn num_tiles(&self) -> u32 {
        1 << self.z
    }

    /// Calculate the bounding box in Web Mercator coordinates
    pub fn bbox(&self) -> (f64, f64, f64, f64) {
        let n = self.num_tiles() as f64;
        let size = 20037508.34278925 * 2.0;

        let min_x = -20037508.34278925 + (self.x as f64 / n) * size;
        let max_x = -20037508.34278925 + ((self.x + 1) as f64 / n) * size;
        let min_y = 20037508.34278925 - ((self.y + 1) as f64 / n) * size;
        let max_y = 20037508.34278925 - (self.y as f64 / n) * size;

        (min_x, min_y, max_x, max_y)
    }

    /// Calculate bounding box in WGS84 (lon/lat)
    pub fn bbox_wgs84(&self) -> (f64, f64, f64, f64) {
        let (min_x, min_y, max_x, max_y) = self.bbox();

        // Convert from Web Mercator to WGS84
        let min_lon = (min_x / 20037508.34278925) * 180.0;
        let max_lon = (max_x / 20037508.34278925) * 180.0;

        let min_lat = (min_y / 20037508.34278925) * 180.0;
        let min_lat =
            (2.0 * min_lat.to_radians().exp().atan() - std::f64::consts::PI / 2.0).to_degrees();

        let max_lat = (max_y / 20037508.34278925) * 180.0;
        let max_lat =
            (2.0 * max_lat.to_radians().exp().atan() - std::f64::consts::PI / 2.0).to_degrees();

        (min_lon, min_lat, max_lon, max_lat)
    }

    /// Check if tile coordinates are valid for this zoom level
    pub fn is_valid(&self) -> bool {
        let max_tile = self.num_tiles();
        self.x < max_tile && self.y < max_tile && self.z <= 30
    }
}

/// Handle XYZ tile request
pub async fn get_tile(
    State(state): State<Arc<TileState>>,
    Path((layer, z, x, y_with_ext)): Path<(String, u8, u32, String)>,
) -> Result<Response, TileError> {
    // Parse y coordinate and format from "y.ext"
    let (y, format) = parse_y_and_format(&y_with_ext)?;

    debug!("XYZ tile request: {}/{}/{}/{}.{}", layer, z, x, y, format);

    // Validate coordinates
    let bounds = WebMercatorBounds::new(z, x, y);
    if !bounds.is_valid() {
        return Err(TileError::InvalidCoordinates);
    }

    // Check cache first
    let cache_key = CacheKey::new(layer.clone(), z, x, y, format.clone());

    if let Some(cached_tile) = state.cache.get(&cache_key) {
        trace!("Cache hit for tile: {}", cache_key.to_string());
        let image_format = parse_format(&format)?;
        return Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, image_format.mime_type())],
            cached_tile,
        )
            .into_response());
    }

    // Get layer
    let layer_info = state.registry.get_layer(&layer)?;

    // Validate zoom level
    if z < layer_info.config.min_zoom || z > layer_info.config.max_zoom {
        return Err(TileError::TileOutOfBounds);
    }

    // Parse image format
    let image_format = parse_format(&format)?;

    // Check if format is supported by this layer
    if !layer_info.config.formats.contains(&image_format) {
        return Err(TileError::UnsupportedFormat(format.clone()));
    }

    // Get dataset
    let dataset = state.registry.get_dataset(&layer)?;

    // Render tile
    let tile_data = render_tile(&dataset, &bounds, layer_info.config.tile_size, image_format)?;

    // Cache the tile
    let _ = state.cache.put(cache_key, tile_data.clone());

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, image_format.mime_type())],
        tile_data,
    )
        .into_response())
}

/// Parse y coordinate and format from string like "123.png"
fn parse_y_and_format(y_with_ext: &str) -> Result<(u32, String), TileError> {
    let parts: Vec<&str> = y_with_ext.rsplitn(2, '.').collect();

    if parts.len() != 2 {
        return Err(TileError::InvalidCoordinates);
    }

    let format = parts[0].to_string();
    let y = parts[1]
        .parse::<u32>()
        .map_err(|_| TileError::InvalidCoordinates)?;

    Ok((y, format))
}

/// Parse image format from file extension
fn parse_format(ext: &str) -> Result<ImageFormat, TileError> {
    ext.parse::<ImageFormat>()
        .map_err(|_| TileError::UnsupportedFormat(ext.to_string()))
}

/// Render a tile from the dataset
fn render_tile(
    dataset: &Arc<crate::dataset_registry::Dataset>,
    bounds: &WebMercatorBounds,
    tile_size: u32,
    format: ImageFormat,
) -> Result<Bytes, TileError> {
    debug!(
        "Rendering tile: z={}, x={}, y={}, size={}x{}, format={:?}",
        bounds.z, bounds.x, bounds.y, tile_size, tile_size, format
    );

    // Get dataset metadata
    let (raster_width, raster_height) = dataset.raster_size();
    let _band_count = dataset.raster_count();

    // Calculate geographic bounds from Web Mercator
    let tile_bounds = calculate_tile_geographic_bounds(bounds);

    // Get dataset geotransform to map geographic to pixel coordinates
    let geotransform = dataset
        .geotransform()
        .map_err(|e| TileError::Rendering(e.to_string()))?;

    // Calculate pixel window in source dataset
    let (src_x, src_y, src_width, src_height) =
        calculate_pixel_window(&tile_bounds, &geotransform, raster_width, raster_height);

    debug!(
        "Pixel window: x={}, y={}, w={}, h={}",
        src_x, src_y, src_width, src_height
    );

    // Create image buffer (RGBA)
    let mut img_buffer = vec![0u8; (tile_size * tile_size * 4) as usize];

    // Create a checkerboard pattern based on coordinates
    let checker_size = tile_size / 8;
    for y in 0..tile_size {
        for x in 0..tile_size {
            let idx = ((y * tile_size + x) * 4) as usize;

            let checker_x = (x / checker_size) % 2;
            let checker_y = (y / checker_size) % 2;
            let is_dark = (checker_x + checker_y) % 2 == 0;

            let base_color: u8 = if is_dark { 100 } else { 200 };

            // Add some variation based on tile coordinates (using saturating_add to avoid overflow)
            let r = base_color.saturating_add((bounds.x % 50) as u8);
            let g = base_color.saturating_add((bounds.y % 50) as u8);
            let b = base_color.saturating_add(bounds.z.saturating_mul(10));

            img_buffer[idx] = r;
            img_buffer[idx + 1] = g;
            img_buffer[idx + 2] = b;
            img_buffer[idx + 3] = 255;
        }
    }

    // Encode based on format
    let encoded = match format {
        ImageFormat::Png => encode_png(&img_buffer, tile_size, tile_size)?,
        ImageFormat::Jpeg => encode_jpeg(&img_buffer, tile_size, tile_size)?,
        ImageFormat::Webp => {
            return Err(TileError::UnsupportedFormat(
                "WebP not yet supported".to_string(),
            ));
        }
        ImageFormat::Geotiff => {
            return Err(TileError::UnsupportedFormat(
                "GeoTIFF not supported for tiles".to_string(),
            ));
        }
    };

    Ok(Bytes::from(encoded))
}

/// Encode image as PNG
fn encode_png(data: &[u8], width: u32, height: u32) -> Result<Vec<u8>, TileError> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);

        let mut writer = encoder
            .write_header()
            .map_err(|e| TileError::Rendering(e.to_string()))?;

        writer
            .write_image_data(data)
            .map_err(|e| TileError::Rendering(e.to_string()))?;
    }

    Ok(output)
}

/// Encode image as JPEG
fn encode_jpeg(data: &[u8], width: u32, height: u32) -> Result<Vec<u8>, TileError> {
    // Convert RGBA to RGB
    let rgb_data: Vec<u8> = data
        .chunks(4)
        .flat_map(|rgba| &rgba[0..3])
        .copied()
        .collect();

    let mut jpeg_buffer = Vec::new();
    let mut encoder = jpeg_encoder::Encoder::new(&mut jpeg_buffer, 90);
    encoder.set_progressive(true);
    encoder
        .encode(
            &rgb_data,
            width as u16,
            height as u16,
            jpeg_encoder::ColorType::Rgb,
        )
        .map_err(|e| TileError::Rendering(e.to_string()))?;

    Ok(jpeg_buffer)
}

/// Handle tile metadata request (TileJSON format)
pub async fn get_tilejson(
    State(state): State<Arc<TileState>>,
    Path(layer): Path<String>,
) -> Result<Response, TileError> {
    debug!("TileJSON request for layer: {}", layer);

    // Get layer info
    let layer_info = state.registry.get_layer(&layer)?;

    // Generate TileJSON
    let tilejson = serde_json::json!({
        "tilejson": "2.2.0",
        "name": layer_info.title,
        "description": layer_info.abstract_,
        "version": "1.0.0",
        "scheme": "xyz",
        "tiles": [
            format!("/tiles/{}/{{z}}/{{x}}/{{y}}.png", layer)
        ],
        "minzoom": layer_info.config.min_zoom,
        "maxzoom": layer_info.config.max_zoom,
        "bounds": layer_info.metadata.bbox.map(|(min_x, min_y, max_x, max_y)| {
            vec![min_x, min_y, max_x, max_y]
        }).unwrap_or_else(|| vec![-180.0, -85.0511, 180.0, 85.0511]),
        "center": layer_info.metadata.bbox.map(|(min_x, min_y, max_x, max_y)| {
            let center_lon = (min_x + max_x) / 2.0;
            let center_lat = (min_y + max_y) / 2.0;
            let zoom = layer_info.config.min_zoom +
                       ((layer_info.config.max_zoom - layer_info.config.min_zoom) / 2);
            vec![center_lon, center_lat, zoom as f64]
        }),
    });

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string_pretty(&tilejson)
            .map_err(|e: serde_json::Error| TileError::Rendering(e.to_string()))?,
    )
        .into_response())
}

/// Calculate geographic bounds from Web Mercator tile coordinates
fn calculate_tile_geographic_bounds(bounds: &WebMercatorBounds) -> GeographicBounds {
    const EARTH_RADIUS: f64 = 6378137.0; // WGS84 Earth radius in meters

    // Calculate Web Mercator bounds
    let tile_size_meters = 2.0 * std::f64::consts::PI * EARTH_RADIUS / (1 << bounds.z) as f64;
    let min_x_meters = bounds.x as f64 * tile_size_meters - std::f64::consts::PI * EARTH_RADIUS;
    let max_y_meters = std::f64::consts::PI * EARTH_RADIUS - bounds.y as f64 * tile_size_meters;

    let max_x_meters = min_x_meters + tile_size_meters;
    let min_y_meters = max_y_meters - tile_size_meters;

    // Convert Web Mercator to WGS84 latitude/longitude
    let min_lon = (min_x_meters / EARTH_RADIUS).to_degrees();
    let max_lon = (max_x_meters / EARTH_RADIUS).to_degrees();

    let min_lat = ((std::f64::consts::PI / 2.0)
        - 2.0 * ((-min_y_meters / EARTH_RADIUS).exp()).atan())
    .to_degrees();
    let max_lat = ((std::f64::consts::PI / 2.0)
        - 2.0 * ((-max_y_meters / EARTH_RADIUS).exp()).atan())
    .to_degrees();

    GeographicBounds {
        min_lon,
        max_lon,
        min_lat,
        max_lat,
    }
}

/// Calculate pixel window in source dataset from geographic bounds
fn calculate_pixel_window(
    bounds: &GeographicBounds,
    geotransform: &[f64; 6],
    raster_width: usize,
    raster_height: usize,
) -> (i32, i32, u32, u32) {
    // Geotransform: [top_left_x, pixel_width, rotation_x, top_left_y, rotation_y, pixel_height]
    // Standard non-rotated: [x_origin, pixel_width, 0, y_origin, 0, -pixel_height]

    let x_origin = geotransform[0];
    let pixel_width = geotransform[1];
    let y_origin = geotransform[3];
    let pixel_height = geotransform[5]; // Usually negative

    // Calculate pixel coordinates
    let x_min = ((bounds.min_lon - x_origin) / pixel_width).floor() as i32;
    let x_max = ((bounds.max_lon - x_origin) / pixel_width).ceil() as i32;
    let y_min = ((bounds.max_lat - y_origin) / pixel_height).floor() as i32;
    let y_max = ((bounds.min_lat - y_origin) / pixel_height).ceil() as i32;

    // Clamp to raster bounds
    let x_min = x_min.max(0).min(raster_width as i32);
    let x_max = x_max.max(0).min(raster_width as i32);
    let y_min = y_min.max(0).min(raster_height as i32);
    let y_max = y_max.max(0).min(raster_height as i32);

    let width = (x_max - x_min).max(0) as u32;
    let height = (y_max - y_min).max(0) as u32;

    (x_min, y_min, width, height)
}

/// Geographic bounds structure
struct GeographicBounds {
    min_lon: f64,
    max_lon: f64,
    min_lat: f64,
    max_lat: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_web_mercator_bounds() {
        // Test zoom 0 (single tile)
        let bounds = WebMercatorBounds::new(0, 0, 0);
        assert_eq!(bounds.num_tiles(), 1);
        assert!(bounds.is_valid());

        let (min_x, min_y, max_x, max_y) = bounds.bbox();
        assert!(min_x < max_x);
        assert!(min_y < max_y);

        // Test zoom 1 (2x2 tiles)
        let bounds = WebMercatorBounds::new(1, 0, 0);
        assert_eq!(bounds.num_tiles(), 2);
        assert!(bounds.is_valid());

        // Test invalid coordinates
        let bounds = WebMercatorBounds::new(1, 2, 0);
        assert!(!bounds.is_valid());

        let bounds = WebMercatorBounds::new(1, 0, 2);
        assert!(!bounds.is_valid());
    }

    #[test]
    fn test_parse_y_and_format() {
        assert_eq!(
            parse_y_and_format("123.png").ok(),
            Some((123, "png".to_string()))
        );
        assert_eq!(
            parse_y_and_format("0.jpg").ok(),
            Some((0, "jpg".to_string()))
        );
        assert_eq!(
            parse_y_and_format("999.webp").ok(),
            Some((999, "webp".to_string()))
        );

        assert!(parse_y_and_format("invalid").is_err());
        assert!(parse_y_and_format("abc.png").is_err());
    }

    #[test]
    fn test_parse_format() {
        assert_eq!(parse_format("png").ok(), Some(ImageFormat::Png));
        assert_eq!(parse_format("jpg").ok(), Some(ImageFormat::Jpeg));
        assert_eq!(parse_format("jpeg").ok(), Some(ImageFormat::Jpeg));

        assert!(parse_format("invalid").is_err());
    }
}
