//! XYZ tile handlers
//!
//! Simple tile serving compatible with Leaflet, MapLibre, and other web mapping libraries.
//! Provides a standard {z}/{x}/{y} endpoint for tile requests.

use crate::cache::{CacheKey, TileCache};
use crate::config::ImageFormat;
use crate::dataset_registry::DatasetRegistry;
use crate::handlers::rendering::{RasterRenderer, RenderStyle};
use axum::{
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::types::GeoTransform;
use oxigeo_proj::{Coordinate, Crs, Transformer};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, trace, warn};

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

/// `Cache-Control` value applied to rendered tiles.
///
/// Tiles are effectively immutable for a given layer/z/x/y/style, so a long
/// max-age with `public` lets shared CDN/proxy caches serve them, while
/// `stale-while-revalidate` avoids latency spikes on revalidation.
const TILE_CACHE_CONTROL: &str = "public, max-age=86400, stale-while-revalidate=604800";

/// Compute a stable, strong-ish ETag for a tile from its cache key.
///
/// The cache key uniquely identifies the tile content (layer, z/x/y, format,
/// style), so hashing it yields an identifier that changes only when the
/// addressed tile changes.
fn tile_etag(cache_key: &CacheKey) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    cache_key.to_string().hash(&mut hasher);
    format!("\"{:016x}\"", hasher.finish())
}

/// Check whether the request's `If-None-Match` header matches the tile ETag.
fn if_none_match_matches(headers: &axum::http::HeaderMap, etag: &str) -> bool {
    headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(|inm| {
            // Support the wildcard and comma-separated lists of ETags.
            inm == "*" || inm.split(',').any(|candidate| candidate.trim() == etag)
        })
        .unwrap_or(false)
}

/// Build a tile response with content-type, ETag and Cache-Control headers.
fn tile_response(image_format: ImageFormat, etag: &str, body: Bytes) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, image_format.mime_type()),
            (header::CACHE_CONTROL, TILE_CACHE_CONTROL),
            (header::ETAG, etag),
        ],
        body,
    )
        .into_response()
}

/// Build a `304 Not Modified` response carrying the validators.
fn not_modified_response(etag: &str) -> Response {
    (
        StatusCode::NOT_MODIFIED,
        [
            (header::CACHE_CONTROL, TILE_CACHE_CONTROL),
            (header::ETAG, etag),
        ],
    )
        .into_response()
}

/// Handle XYZ tile request
pub async fn get_tile(
    State(state): State<Arc<TileState>>,
    headers: axum::http::HeaderMap,
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
    let etag = tile_etag(&cache_key);

    // Conditional GET: if the client already has this exact tile, short-circuit.
    if if_none_match_matches(&headers, &etag) {
        trace!("Conditional GET hit (304) for tile: {}", cache_key);
        return Ok(not_modified_response(&etag));
    }

    if let Some(cached_tile) = state.cache.get(&cache_key) {
        trace!("Cache hit for tile: {}", cache_key.to_string());
        let image_format = parse_format(&format)?;
        return Ok(tile_response(image_format, &etag, cached_tile));
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

    // Build the rendering style from the layer configuration (colormap, value
    // range, resampling, etc.), matching the WMS/WMTS handlers' styling path.
    let render_style = if let Some(ref style_cfg) = layer_info.config.style {
        RenderStyle::from_config(style_cfg)
    } else {
        RenderStyle::default()
    };

    // Render tile
    let tile_data = render_tile(
        &dataset,
        &bounds,
        layer_info.config.tile_size,
        image_format,
        &render_style,
    )?;

    // Cache the tile. A failure here is non-fatal but must be visible in logs.
    if let Err(e) = state.cache.put(cache_key, tile_data.clone()) {
        warn!("Failed to cache rendered tile: {}", e);
    }

    Ok(tile_response(image_format, &etag, tile_data))
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

/// Render a tile from the dataset.
///
/// This reads the real raster window that intersects the requested Web Mercator
/// (XYZ / WebMercatorQuad) tile, reprojects the source pixels into the tile grid
/// when the dataset's native CRS differs from the tile CRS (EPSG:3857), applies
/// the layer's style (colormap / RGB composition), and encodes the result. Areas
/// of the tile that fall outside the dataset footprint (or over nodata) are
/// rendered fully transparent.
fn render_tile(
    dataset: &Arc<crate::dataset_registry::Dataset>,
    bounds: &WebMercatorBounds,
    tile_size: u32,
    format: ImageFormat,
    style: &RenderStyle,
) -> Result<Bytes, TileError> {
    debug!(
        "Rendering tile: z={}, x={}, y={}, size={}x{}, format={:?}",
        bounds.z, bounds.x, bounds.y, tile_size, tile_size, format
    );

    let geo_transform = dataset
        .geo_transform_obj()
        .ok_or_else(|| TileError::Rendering("Dataset has no geotransform".to_string()))?;

    let ds_width = dataset.width();
    let ds_height = dataset.height();
    let band_count = dataset.raster_count();

    // Tile footprint in Web Mercator (EPSG:3857) meters.
    let merc = bounds.bbox();

    // Determine the dataset's native CRS and build a tile(3857)->dataset
    // transformer when a real reprojection is required.
    let ds_epsg = dataset_epsg(dataset);
    let transformer = build_tile_transformer(ds_epsg)?;

    // Compute the source pixel window in the dataset covering the tile.
    let window = compute_source_window(
        geo_transform,
        ds_width,
        ds_height,
        merc,
        transformer.as_ref(),
    );

    let tile_px = tile_size as u64;

    let rgba = match window {
        Some((src_x, src_y, src_w, src_h)) if src_w > 0 && src_h > 0 => {
            debug!(
                "Source window: x={}, y={}, w={}, h={} (reproject={})",
                src_x,
                src_y,
                src_w,
                src_h,
                transformer.is_some()
            );
            match transformer {
                Some(ref tr) => warp_reproject(
                    dataset,
                    geo_transform,
                    (src_x, src_y, src_w, src_h),
                    merc,
                    tile_px,
                    band_count,
                    tr,
                    style,
                )?,
                None => render_aligned(
                    dataset,
                    (src_x, src_y, src_w, src_h),
                    tile_px,
                    band_count,
                    style,
                )?,
            }
        }
        // No overlap between the tile and the dataset: fully transparent tile.
        _ => vec![0u8; (tile_px * tile_px * 4) as usize],
    };

    // Encode based on format
    let encoded = match format {
        ImageFormat::Png => encode_png(&rgba, tile_size, tile_size)?,
        ImageFormat::Jpeg => encode_jpeg(&rgba, tile_size, tile_size)?,
        ImageFormat::Webp => encode_webp(&rgba, tile_size, tile_size)?,
        ImageFormat::Geotiff => {
            return Err(TileError::UnsupportedFormat(
                "GeoTIFF not supported for tiles".to_string(),
            ));
        }
    };

    Ok(Bytes::from(encoded))
}

/// Parse the dataset's native EPSG code from its projection string.
///
/// Returns `None` when the code cannot be determined (e.g. a non-EPSG WKT),
/// in which case callers assume the dataset is already in the tile CRS.
fn dataset_epsg(dataset: &crate::dataset_registry::Dataset) -> Option<u32> {
    let proj = dataset.projection().ok()?;
    let upper = proj.trim().to_uppercase();
    let code = upper.strip_prefix("EPSG:")?;
    code.trim().parse::<u32>().ok()
}

/// Build a transformer from the tile CRS (EPSG:3857) to the dataset CRS.
///
/// Returns `Ok(None)` when the dataset is already in the Web Mercator tile CRS
/// (or its CRS is unknown, in which case we assume alignment) and `Ok(Some(_))`
/// when a real reprojection is required.
fn build_tile_transformer(ds_epsg: Option<u32>) -> Result<Option<Transformer>, TileError> {
    match ds_epsg {
        // EPSG:3857 (and its historical aliases) are the tile CRS itself.
        None | Some(3857) | Some(900913) | Some(3785) => Ok(None),
        Some(code) => {
            let src = Crs::from_epsg(3857)
                .map_err(|e| TileError::Rendering(format!("Tile CRS EPSG:3857 error: {}", e)))?;
            let dst = Crs::from_epsg(code).map_err(|e| {
                TileError::Rendering(format!("Dataset CRS EPSG:{} error: {}", code, e))
            })?;
            let tr = Transformer::new(src, dst).map_err(|e| {
                TileError::Rendering(format!(
                    "Failed to build reprojection EPSG:3857 -> EPSG:{}: {}",
                    code, e
                ))
            })?;
            Ok(Some(tr))
        }
    }
}

/// Compute the source pixel window in the dataset covering the tile footprint.
///
/// Samples a grid of points across the tile (in Web Mercator), optionally
/// reprojects them into the dataset CRS, maps them to source pixel coordinates
/// via the geotransform, and returns the clamped bounding window
/// `(x, y, width, height)`. Returns `None` when the tile does not overlap the
/// dataset at all.
fn compute_source_window(
    geo_transform: &GeoTransform,
    ds_width: u64,
    ds_height: u64,
    merc: (f64, f64, f64, f64),
    transformer: Option<&Transformer>,
) -> Option<(u64, u64, u64, u64)> {
    let (min_x, min_y, max_x, max_y) = merc;
    // A grid (rather than only the 4 corners) keeps the window correct even when
    // the reprojected tile edges are curved.
    const SAMPLES: usize = 9;

    let mut px_min = f64::INFINITY;
    let mut px_max = f64::NEG_INFINITY;
    let mut py_min = f64::INFINITY;
    let mut py_max = f64::NEG_INFINITY;
    let mut any = false;

    for iy in 0..SAMPLES {
        let fy = iy as f64 / (SAMPLES - 1) as f64;
        let wy = min_y + fy * (max_y - min_y);
        for ix in 0..SAMPLES {
            let fx = ix as f64 / (SAMPLES - 1) as f64;
            let wx = min_x + fx * (max_x - min_x);

            let (dsx, dsy) = match transformer {
                Some(tr) => match tr.transform(&Coordinate::new(wx, wy)) {
                    Ok(c) if c.x.is_finite() && c.y.is_finite() => (c.x, c.y),
                    _ => continue,
                },
                None => (wx, wy),
            };

            if let Ok((px, py)) = geo_transform.world_to_pixel(dsx, dsy)
                && px.is_finite()
                && py.is_finite()
            {
                px_min = px_min.min(px);
                px_max = px_max.max(px);
                py_min = py_min.min(py);
                py_max = py_max.max(py);
                any = true;
            }
        }
    }

    if !any {
        return None;
    }

    // Clamp to dataset bounds, expanding by 1px to avoid seams at tile edges.
    let x0 = px_min.floor().max(0.0) as u64;
    let y0 = py_min.floor().max(0.0) as u64;
    let x1 = (((px_max.ceil() + 1.0).max(0.0)) as u64).min(ds_width);
    let y1 = (((py_max.ceil() + 1.0).max(0.0)) as u64).min(ds_height);

    if x1 <= x0 || y1 <= y0 {
        return None;
    }

    Some((x0, y0, x1 - x0, y1 - y0))
}

/// Render a tile when the dataset is already in the tile CRS: read the source
/// window, resample to the tile size, and style it. This is the exact real-data
/// path used by the WMS/WMTS handlers.
fn render_aligned(
    dataset: &crate::dataset_registry::Dataset,
    window: (u64, u64, u64, u64),
    tile_px: u64,
    band_count: usize,
    style: &RenderStyle,
) -> Result<Vec<u8>, TileError> {
    let (src_x, src_y, src_w, src_h) = window;

    if band_count >= 3 {
        let red = dataset
            .read_window(src_x, src_y, src_w, src_h)
            .map_err(|e| TileError::Rendering(format!("Failed to read window: {}", e)))?;
        let green = read_band_window(dataset, 1, window);
        let blue = read_band_window(dataset, 2, window);

        let (green, blue) = match (green, blue) {
            (Ok(g), Ok(b)) => (g, b),
            _ => {
                let gray = red.clone();
                (gray.clone(), gray)
            }
        };

        let red = resample_to(&red, tile_px, style)?;
        let green = resample_to(&green, tile_px, style)?;
        let blue = resample_to(&blue, tile_px, style)?;

        RasterRenderer::render_rgb_to_rgba(&red, &green, &blue, style)
            .map_err(|e| TileError::Rendering(e.to_string()))
    } else {
        let src = dataset
            .read_window(src_x, src_y, src_w, src_h)
            .map_err(|e| TileError::Rendering(format!("Failed to read window: {}", e)))?;
        let src = resample_to(&src, tile_px, style)?;
        RasterRenderer::render_to_rgba(&src, style).map_err(|e| TileError::Rendering(e.to_string()))
    }
}

/// Reproject the source window into the Web Mercator tile grid, sampling each
/// output pixel from the dataset (inverse warp) and applying the layer style.
#[allow(clippy::too_many_arguments)]
fn warp_reproject(
    dataset: &crate::dataset_registry::Dataset,
    geo_transform: &GeoTransform,
    window: (u64, u64, u64, u64),
    merc: (f64, f64, f64, f64),
    tile_px: u64,
    band_count: usize,
    transformer: &Transformer,
    style: &RenderStyle,
) -> Result<Vec<u8>, TileError> {
    let (src_x, src_y, src_w, src_h) = window;
    let data_type = dataset.data_type();
    let nodata = dataset.nodata().as_f64();

    // Read source windows for the bands we need.
    let src_r = dataset
        .read_window(src_x, src_y, src_w, src_h)
        .map_err(|e| TileError::Rendering(format!("Failed to read window: {}", e)))?;
    let (src_g, src_b) = if band_count >= 3 {
        match (
            read_band_window(dataset, 1, window),
            read_band_window(dataset, 2, window),
        ) {
            (Ok(g), Ok(b)) => (Some(g), Some(b)),
            _ => (None, None),
        }
    } else {
        (None, None)
    };

    let pixel_count = (tile_px * tile_px) as usize;
    let mut r_buf = RasterBuffer::zeros(tile_px, tile_px, data_type);
    let mut g_buf = RasterBuffer::zeros(tile_px, tile_px, data_type);
    let mut b_buf = RasterBuffer::zeros(tile_px, tile_px, data_type);
    let mut mask = vec![false; pixel_count];

    let (min_x, min_y, max_x, max_y) = merc;
    let sx = src_x as f64;
    let sy = src_y as f64;
    let sw = src_w as f64;
    let sh = src_h as f64;

    // Track covered-pixel statistics so single-band normalization is driven only
    // by real data, not by transparent filler pixels.
    let mut cov_min = f64::INFINITY;
    let mut cov_max = f64::NEG_INFINITY;

    for ty in 0..tile_px {
        // Top tile row corresponds to the northern edge (max_y).
        let fy = (ty as f64 + 0.5) / tile_px as f64;
        let wy = max_y - fy * (max_y - min_y);
        for tx in 0..tile_px {
            let fx = (tx as f64 + 0.5) / tile_px as f64;
            let wx = min_x + fx * (max_x - min_x);

            let coord = match transformer.transform(&Coordinate::new(wx, wy)) {
                Ok(c) if c.x.is_finite() && c.y.is_finite() => c,
                _ => continue,
            };
            let (px, py) = match geo_transform.world_to_pixel(coord.x, coord.y) {
                Ok(p) => p,
                Err(_) => continue,
            };

            let lx = px - sx;
            let ly = py - sy;
            if lx < 0.0 || ly < 0.0 || lx >= sw || ly >= sh {
                continue;
            }
            let nx = lx as u64;
            let ny = ly as u64;

            let rv = match src_r.get_pixel(nx, ny) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if rv.is_nan() || is_nodata(rv, nodata) {
                continue;
            }

            let idx = (ty * tile_px + tx) as usize;
            let _ = r_buf.set_pixel(tx, ty, rv);
            mask[idx] = true;

            if band_count >= 3 {
                if let (Some(g), Some(b)) = (&src_g, &src_b) {
                    let gv = g.get_pixel(nx, ny).unwrap_or(rv);
                    let bv = b.get_pixel(nx, ny).unwrap_or(rv);
                    let _ = g_buf.set_pixel(tx, ty, gv);
                    let _ = b_buf.set_pixel(tx, ty, bv);
                } else {
                    let _ = g_buf.set_pixel(tx, ty, rv);
                    let _ = b_buf.set_pixel(tx, ty, rv);
                }
            } else {
                cov_min = cov_min.min(rv);
                cov_max = cov_max.max(rv);
            }
        }
    }

    let mut rgba = if band_count >= 3 {
        RasterRenderer::render_rgb_to_rgba(&r_buf, &g_buf, &b_buf, style)
            .map_err(|e| TileError::Rendering(e.to_string()))?
    } else {
        let mut s = style.clone();
        if s.value_range.is_none() && cov_max > cov_min {
            s.value_range = Some((cov_min, cov_max));
        }
        RasterRenderer::render_to_rgba(&r_buf, &s)
            .map_err(|e| TileError::Rendering(e.to_string()))?
    };

    // Apply the coverage mask: any pixel not sampled from real data is fully
    // transparent.
    for (i, covered) in mask.iter().enumerate() {
        if !covered {
            let o = i * 4;
            rgba[o] = 0;
            rgba[o + 1] = 0;
            rgba[o + 2] = 0;
            rgba[o + 3] = 0;
        }
    }

    Ok(rgba)
}

/// Check whether a value matches the dataset's nodata value (if any).
fn is_nodata(value: f64, nodata: Option<f64>) -> bool {
    match nodata {
        Some(nd) if nd.is_finite() => (value - nd).abs() < 1e-9,
        Some(_) => value.is_nan(),
        None => false,
    }
}

/// Read a window of one band, touching only the blocks that overlap it.
///
/// `Dataset::read_window` is this with `band = 0`, so the red channel and the
/// green/blue channels now come from the same code path; they used to disagree,
/// with red going through a tile-stitching loop that silently produced an
/// all-zero buffer for multi-band datasets.
/// See <https://github.com/cool-japan/oxigeo/issues/14>.
fn read_band_window(
    dataset: &crate::dataset_registry::Dataset,
    band: usize,
    window: (u64, u64, u64, u64),
) -> Result<RasterBuffer, TileError> {
    let (src_x, src_y, src_w, src_h) = window;
    dataset
        .read_band_window(0, band, src_x, src_y, src_w, src_h)
        .map_err(|e| TileError::Rendering(format!("Failed to read band {}: {}", band, e)))
}

/// Resample a buffer to `tile_px` x `tile_px` if it is not already that size.
fn resample_to(
    buffer: &RasterBuffer,
    tile_px: u64,
    style: &RenderStyle,
) -> Result<RasterBuffer, TileError> {
    if buffer.width() != tile_px || buffer.height() != tile_px {
        RasterRenderer::resample(buffer, tile_px, tile_px, style.resampling)
            .map_err(|e| TileError::Rendering(e.to_string()))
    } else {
        Ok(buffer.clone())
    }
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

/// Encode RGBA data to lossless WebP format
fn encode_webp(data: &[u8], width: u32, height: u32) -> Result<Vec<u8>, TileError> {
    use image::ExtendedColorType;
    use image::codecs::webp::WebPEncoder;

    let mut output = Vec::new();
    let encoder = WebPEncoder::new_lossless(&mut output);
    encoder
        .encode(data, width, height, ExtendedColorType::Rgba8)
        .map_err(|e| TileError::Rendering(e.to_string()))?;
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

    #[test]
    fn test_build_tile_transformer_web_mercator_is_none() {
        // Datasets already in the tile CRS (or its aliases) need no reprojection.
        assert!(
            build_tile_transformer(Some(3857))
                .expect("transformer build")
                .is_none()
        );
        assert!(
            build_tile_transformer(Some(900913))
                .expect("transformer build")
                .is_none()
        );
        assert!(
            build_tile_transformer(None)
                .expect("transformer build")
                .is_none()
        );
    }

    #[test]
    fn test_build_tile_transformer_wgs84_is_some() {
        // A WGS84 dataset must be reprojected from the Web Mercator tile CRS.
        let tr = build_tile_transformer(Some(4326)).expect("transformer build");
        assert!(tr.is_some(), "EPSG:4326 dataset requires a transformer");
    }

    #[test]
    fn test_is_nodata() {
        assert!(is_nodata(0.0, Some(0.0)));
        assert!(!is_nodata(1.0, Some(0.0)));
        assert!(!is_nodata(0.0, None));
        assert!(!is_nodata(f64::NAN, None));
        // A NaN-configured nodata treats NaN pixels as nodata.
        assert!(is_nodata(f64::NAN, Some(f64::NAN)));
    }

    #[test]
    fn test_compute_source_window_aligned_full_overlap() {
        // Dataset in EPSG:3857 covering the whole world at 256x256 pixels.
        // Zoom-0 tile covers the same extent -> the window is the full raster.
        let world = 20_037_508.342_789_244_f64;
        let px = (2.0 * world) / 256.0;
        let gt = GeoTransform::north_up(-world, world, px, -px);

        let bounds = WebMercatorBounds::new(0, 0, 0);
        let window = compute_source_window(&gt, 256, 256, bounds.bbox(), None)
            .expect("zoom-0 tile should overlap a whole-world raster");

        let (x, y, w, h) = window;
        assert_eq!(x, 0);
        assert_eq!(y, 0);
        // Full width/height (allowing for the +1px expansion clamp).
        assert_eq!(w, 256);
        assert_eq!(h, 256);
    }

    #[test]
    fn test_compute_source_window_no_overlap() {
        // A tiny dataset near the origin; a far-away high-zoom tile does not touch it.
        let gt = GeoTransform::north_up(0.0, 100.0, 1.0, -1.0);
        // High-zoom tile at column 0/row 0 (far north-west corner, ~-20037508 x).
        let bounds = WebMercatorBounds::new(10, 0, 0);
        let window = compute_source_window(&gt, 100, 100, bounds.bbox(), None);
        assert!(
            window.is_none(),
            "a non-overlapping tile must yield no source window"
        );
    }

    #[test]
    fn test_compute_source_window_reproject_wgs84() {
        // A global WGS84 (EPSG:4326) dataset: 360x180 deg over 360x180 px.
        let gt = GeoTransform::north_up(-180.0, 90.0, 1.0, -1.0);
        let transformer = build_tile_transformer(Some(4326))
            .expect("transformer build")
            .expect("EPSG:4326 needs reprojection");

        // Zoom-0 tile covers the full Web Mercator extent (~+/-85 deg lat).
        let bounds = WebMercatorBounds::new(0, 0, 0);
        let window = compute_source_window(&gt, 360, 180, bounds.bbox(), Some(&transformer))
            .expect("reprojected zoom-0 tile should overlap the global raster");

        let (x, _y, w, _h) = window;
        // Longitude spans the whole raster width.
        assert_eq!(x, 0);
        assert!(w >= 359, "expected near-full width, got {}", w);
    }
}
