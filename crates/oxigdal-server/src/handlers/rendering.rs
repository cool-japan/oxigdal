//! Raster rendering utilities for WMS and WMTS handlers
//!
//! This module provides colormap application, min/max scaling,
//! and image generation for serving raster data.

use crate::config::{ImageFormat, StyleConfig};
use bytes::Bytes;
use oxigdal_algorithms::resampling::{Resampler, ResamplingMethod};
use oxigdal_core::buffer::RasterBuffer;
#[cfg(test)]
use oxigdal_core::types::RasterDataType;
use oxigdal_core::types::{BoundingBox, GeoTransform};
use thiserror::Error;

/// Rendering errors
#[derive(Debug, Error)]
pub enum RenderError {
    /// Invalid parameters
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    /// Data read error
    #[error("Failed to read data: {0}")]
    ReadError(String),

    /// Resampling error
    #[error("Resampling failed: {0}")]
    ResamplingError(String),

    /// Image encoding error
    #[error("Image encoding failed: {0}")]
    EncodingError(String),

    /// Unsupported operation
    #[error("Unsupported: {0}")]
    Unsupported(String),
}

impl From<oxigdal_core::OxiGdalError> for RenderError {
    fn from(e: oxigdal_core::OxiGdalError) -> Self {
        RenderError::ReadError(e.to_string())
    }
}

impl From<oxigdal_algorithms::AlgorithmError> for RenderError {
    fn from(e: oxigdal_algorithms::AlgorithmError) -> Self {
        RenderError::ResamplingError(e.to_string())
    }
}

/// Colormap types for raster visualization
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Colormap {
    /// Grayscale (black to white)
    Grayscale,
    /// Viridis (perceptually uniform, blue to yellow)
    Viridis,
    /// Terrain (green/brown, for elevation)
    Terrain,
    /// Jet (rainbow, blue to red)
    Jet,
    /// Hot (black to red to yellow to white)
    Hot,
    /// Cool (cyan to magenta)
    Cool,
    /// Spectral (diverging, red-yellow-blue)
    Spectral,
    /// NDVI (brown to green, for vegetation)
    Ndvi,
}

impl Colormap {
    /// Parse colormap name from string
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "grayscale" | "gray" | "grey" => Some(Self::Grayscale),
            "viridis" => Some(Self::Viridis),
            "terrain" => Some(Self::Terrain),
            "jet" | "rainbow" => Some(Self::Jet),
            "hot" => Some(Self::Hot),
            "cool" => Some(Self::Cool),
            "spectral" => Some(Self::Spectral),
            "ndvi" | "vegetation" => Some(Self::Ndvi),
            _ => None,
        }
    }

    /// Apply colormap to a normalized value (0.0 to 1.0)
    /// Returns (R, G, B) values in range 0-255
    #[must_use]
    pub fn apply(&self, value: f64) -> (u8, u8, u8) {
        let v = value.clamp(0.0, 1.0);

        match self {
            Self::Grayscale => {
                let gray = (v * 255.0) as u8;
                (gray, gray, gray)
            }
            Self::Viridis => Self::viridis(v),
            Self::Terrain => Self::terrain(v),
            Self::Jet => Self::jet(v),
            Self::Hot => Self::hot(v),
            Self::Cool => Self::cool(v),
            Self::Spectral => Self::spectral(v),
            Self::Ndvi => Self::ndvi(v),
        }
    }

    fn viridis(t: f64) -> (u8, u8, u8) {
        // Viridis colormap approximation
        let r = ((0.267004 + t * (0.993248 - 0.267004)) * 255.0) as u8;
        let g = if t < 0.5 {
            (t * 2.0 * 0.7).clamp(0.0, 1.0) * 255.0
        } else {
            (0.7 + (t - 0.5) * 2.0 * 0.3).clamp(0.0, 1.0) * 255.0
        } as u8;
        let b = if t < 0.3 {
            ((0.33 + t / 0.3 * 0.37) * 255.0) as u8
        } else if t < 0.7 {
            ((0.7 - (t - 0.3) / 0.4 * 0.5) * 255.0) as u8
        } else {
            ((0.2 * (1.0 - (t - 0.7) / 0.3)) * 255.0) as u8
        };
        (r, g, b)
    }

    fn terrain(t: f64) -> (u8, u8, u8) {
        // Terrain colormap: blue -> green -> brown -> white
        if t < 0.1 {
            // Deep water (dark blue)
            (0, 0, (t / 0.1 * 128.0 + 64.0) as u8)
        } else if t < 0.25 {
            // Shallow water (light blue)
            let v = (t - 0.1) / 0.15;
            (0, (v * 128.0) as u8, (192.0 - v * 64.0) as u8)
        } else if t < 0.5 {
            // Lowland (green)
            let v = (t - 0.25) / 0.25;
            (
                (v * 100.0) as u8,
                (128.0 + v * 64.0) as u8,
                (128.0 - v * 64.0) as u8,
            )
        } else if t < 0.75 {
            // Highland (brown/tan)
            let v = (t - 0.5) / 0.25;
            (
                (100.0 + v * 100.0) as u8,
                (192.0 - v * 64.0) as u8,
                (64.0 + v * 32.0) as u8,
            )
        } else {
            // Mountain/snow (gray to white)
            let v = (t - 0.75) / 0.25;
            let c = (200.0 + v * 55.0) as u8;
            (c, c, c)
        }
    }

    fn jet(t: f64) -> (u8, u8, u8) {
        // Classic jet/rainbow colormap
        let r = if t < 0.35 {
            0.0
        } else if t < 0.65 {
            (t - 0.35) / 0.3
        } else {
            1.0
        };

        let g = if t < 0.125 {
            0.0
        } else if t < 0.375 {
            (t - 0.125) / 0.25
        } else if t < 0.625 {
            1.0
        } else if t < 0.875 {
            1.0 - (t - 0.625) / 0.25
        } else {
            0.0
        };

        let b = if t < 0.35 {
            0.5 + t / 0.35 * 0.5
        } else if t < 0.65 {
            1.0 - (t - 0.35) / 0.3
        } else {
            0.0
        };

        ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
    }

    fn hot(t: f64) -> (u8, u8, u8) {
        // Hot colormap: black -> red -> yellow -> white
        let r = if t < 0.4 { t / 0.4 } else { 1.0 };
        let g = if t < 0.4 {
            0.0
        } else if t < 0.8 {
            (t - 0.4) / 0.4
        } else {
            1.0
        };
        let b = if t < 0.8 { 0.0 } else { (t - 0.8) / 0.2 };

        ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
    }

    fn cool(t: f64) -> (u8, u8, u8) {
        // Cool colormap: cyan to magenta
        ((t * 255.0) as u8, ((1.0 - t) * 255.0) as u8, 255)
    }

    fn spectral(t: f64) -> (u8, u8, u8) {
        // Spectral diverging colormap
        if t < 0.2 {
            let v = t / 0.2;
            (
                (158.0 + v * 60.0) as u8,
                (1.0 + v * 102.0) as u8,
                (66.0) as u8,
            )
        } else if t < 0.4 {
            let v = (t - 0.2) / 0.2;
            (
                (213.0 + v * 40.0) as u8,
                (103.0 + v * 96.0) as u8,
                ((66.0 + v * 8.0) as u8),
            )
        } else if t < 0.6 {
            let v = (t - 0.4) / 0.2;
            (
                (253.0 - v * 82.0) as u8,
                (199.0 + v * 32.0) as u8,
                (74.0 + v * 92.0) as u8,
            )
        } else if t < 0.8 {
            let v = (t - 0.6) / 0.2;
            (
                (171.0 - v * 69.0) as u8,
                (231.0 - v * 42.0) as u8,
                (166.0 - v * 22.0) as u8,
            )
        } else {
            let v = (t - 0.8) / 0.2;
            (
                (102.0 - v * 49.0) as u8,
                (189.0 - v * 60.0) as u8,
                ((144.0 - v * 45.0) as u8),
            )
        }
    }

    fn ndvi(t: f64) -> (u8, u8, u8) {
        // NDVI colormap: brown -> yellow -> green
        if t < 0.2 {
            // Brown (sparse/no vegetation)
            let v = t / 0.2;
            (
                (139.0 - v * 30.0) as u8,
                (69.0 + v * 40.0) as u8,
                (19.0 + v * 30.0) as u8,
            )
        } else if t < 0.4 {
            // Yellow-brown
            let v = (t - 0.2) / 0.2;
            (
                (109.0 + v * 100.0) as u8,
                (109.0 + v * 90.0) as u8,
                (49.0 - v * 20.0) as u8,
            )
        } else if t < 0.6 {
            // Yellow-green
            let v = (t - 0.4) / 0.2;
            (
                (209.0 - v * 77.0) as u8,
                (199.0 - v * 30.0) as u8,
                (29.0 + v * 20.0) as u8,
            )
        } else if t < 0.8 {
            // Light green
            let v = (t - 0.6) / 0.2;
            (
                (132.0 - v * 66.0) as u8,
                (169.0 - v * 24.0) as u8,
                (49.0) as u8,
            )
        } else {
            // Dark green (dense vegetation)
            let v = (t - 0.8) / 0.2;
            (
                (66.0 - v * 32.0) as u8,
                ((145.0 - v * 45.0) as u8),
                ((49.0 - v * 20.0) as u8),
            )
        }
    }
}

/// Style parameters for rendering
#[derive(Debug, Clone)]
pub struct RenderStyle {
    /// Colormap for single-band data
    pub colormap: Option<Colormap>,
    /// Value range for normalization (min, max)
    pub value_range: Option<(f64, f64)>,
    /// Alpha/transparency (0.0 = transparent, 1.0 = opaque)
    pub alpha: f32,
    /// Gamma correction (1.0 = no correction)
    pub gamma: f32,
    /// Brightness adjustment (-1.0 to 1.0)
    pub brightness: f32,
    /// Contrast adjustment (0.0 to 2.0, 1.0 = normal)
    pub contrast: f32,
    /// Resampling method
    pub resampling: ResamplingMethod,
}

impl Default for RenderStyle {
    fn default() -> Self {
        Self {
            colormap: Some(Colormap::Grayscale),
            value_range: None,
            alpha: 1.0,
            gamma: 1.0,
            brightness: 0.0,
            contrast: 1.0,
            resampling: ResamplingMethod::Bilinear,
        }
    }
}

impl RenderStyle {
    /// Create from StyleConfig
    pub fn from_config(config: &StyleConfig) -> Self {
        let colormap = config
            .colormap
            .as_ref()
            .and_then(|name| Colormap::from_name(name))
            .or(Some(Colormap::Grayscale));

        Self {
            colormap,
            value_range: config.value_range,
            alpha: config.alpha,
            gamma: config.gamma,
            brightness: config.brightness,
            contrast: config.contrast,
            resampling: ResamplingMethod::Bilinear,
        }
    }
}

/// Raster renderer for generating images from raster data
pub struct RasterRenderer;

impl RasterRenderer {
    /// Render a RasterBuffer to an RGBA image buffer
    ///
    /// # Arguments
    /// * `buffer` - Source raster data
    /// * `style` - Rendering style parameters
    ///
    /// # Returns
    /// RGBA image data as `Vec<u8>` (4 bytes per pixel)
    pub fn render_to_rgba(
        buffer: &RasterBuffer,
        style: &RenderStyle,
    ) -> Result<Vec<u8>, RenderError> {
        let width = buffer.width() as usize;
        let height = buffer.height() as usize;
        let pixel_count = width * height;

        // First, compute min/max if not provided
        let (min_val, max_val) = if let Some((min, max)) = style.value_range {
            (min, max)
        } else {
            // Compute statistics from buffer
            let stats = buffer.compute_statistics().map_err(|e| {
                RenderError::ReadError(format!("Failed to compute statistics: {}", e))
            })?;
            (stats.min, stats.max)
        };

        let value_range = max_val - min_val;
        if value_range.abs() < f64::EPSILON {
            // All values are the same - return solid color
            let alpha = (style.alpha * 255.0) as u8;
            return Ok([128, 128, 128, alpha].repeat(pixel_count));
        }

        // Allocate output buffer
        let mut rgba = vec![0u8; pixel_count * 4];

        // Apply colormap based on data type
        let colormap = style.colormap.unwrap_or(Colormap::Grayscale);
        let gamma = style.gamma;
        let brightness = style.brightness;
        let contrast = style.contrast;
        let alpha = (style.alpha * 255.0) as u8;

        for y in 0..height {
            for x in 0..width {
                let pixel_idx = y * width + x;
                let rgba_idx = pixel_idx * 4;

                // Get pixel value
                let value = buffer.get_pixel(x as u64, y as u64).unwrap_or(f64::NAN);

                // Handle nodata
                if value.is_nan() || buffer.is_nodata(value) {
                    // Transparent
                    rgba[rgba_idx] = 0;
                    rgba[rgba_idx + 1] = 0;
                    rgba[rgba_idx + 2] = 0;
                    rgba[rgba_idx + 3] = 0;
                    continue;
                }

                // Normalize to 0-1
                let mut normalized = (value - min_val) / value_range;

                // Apply gamma correction
                if (gamma - 1.0).abs() > f32::EPSILON {
                    normalized = normalized.powf(gamma as f64);
                }

                // Apply brightness and contrast
                if contrast.abs() > f32::EPSILON || brightness.abs() > f32::EPSILON {
                    normalized = ((normalized - 0.5) * contrast as f64 + 0.5 + brightness as f64)
                        .clamp(0.0, 1.0);
                }

                // Apply colormap
                let (r, g, b) = colormap.apply(normalized);

                rgba[rgba_idx] = r;
                rgba[rgba_idx + 1] = g;
                rgba[rgba_idx + 2] = b;
                rgba[rgba_idx + 3] = alpha;
            }
        }

        Ok(rgba)
    }

    /// Render RGB bands to RGBA image
    ///
    /// # Arguments
    /// * `red` - Red band buffer
    /// * `green` - Green band buffer
    /// * `blue` - Blue band buffer
    /// * `style` - Rendering style parameters
    pub fn render_rgb_to_rgba(
        red: &RasterBuffer,
        green: &RasterBuffer,
        blue: &RasterBuffer,
        style: &RenderStyle,
    ) -> Result<Vec<u8>, RenderError> {
        let width = red.width() as usize;
        let height = red.height() as usize;

        if green.width() as usize != width
            || green.height() as usize != height
            || blue.width() as usize != width
            || blue.height() as usize != height
        {
            return Err(RenderError::InvalidParameter(
                "RGB bands must have same dimensions".to_string(),
            ));
        }

        let pixel_count = width * height;
        let mut rgba = vec![0u8; pixel_count * 4];

        let alpha = (style.alpha * 255.0) as u8;
        let gamma = style.gamma;
        let brightness = style.brightness;
        let contrast = style.contrast;

        // Get value ranges for each band
        let (r_min, r_max) = if let Some((min, max)) = style.value_range {
            (min, max)
        } else {
            let stats = red.compute_statistics().map_err(|e| {
                RenderError::ReadError(format!("Failed to compute red stats: {}", e))
            })?;
            (stats.min, stats.max)
        };
        let r_range = (r_max - r_min).max(1.0);

        let g_stats = green
            .compute_statistics()
            .map_err(|e| RenderError::ReadError(format!("Failed to compute green stats: {}", e)))?;
        let g_range = (g_stats.max - g_stats.min).max(1.0);
        let g_min = g_stats.min;

        let b_stats = blue
            .compute_statistics()
            .map_err(|e| RenderError::ReadError(format!("Failed to compute blue stats: {}", e)))?;
        let b_range = (b_stats.max - b_stats.min).max(1.0);
        let b_min = b_stats.min;

        for y in 0..height {
            for x in 0..width {
                let pixel_idx = y * width + x;
                let rgba_idx = pixel_idx * 4;

                let r_val = red.get_pixel(x as u64, y as u64).unwrap_or(0.0);
                let g_val = green.get_pixel(x as u64, y as u64).unwrap_or(0.0);
                let b_val = blue.get_pixel(x as u64, y as u64).unwrap_or(0.0);

                // Normalize
                let mut r_norm = (r_val - r_min) / r_range;
                let mut g_norm = (g_val - g_min) / g_range;
                let mut b_norm = (b_val - b_min) / b_range;

                // Apply gamma
                if (gamma - 1.0).abs() > f32::EPSILON {
                    let g = gamma as f64;
                    r_norm = r_norm.powf(g);
                    g_norm = g_norm.powf(g);
                    b_norm = b_norm.powf(g);
                }

                // Apply brightness/contrast
                if contrast.abs() > f32::EPSILON || brightness.abs() > f32::EPSILON {
                    let c = contrast as f64;
                    let b_adj = brightness as f64;
                    r_norm = ((r_norm - 0.5) * c + 0.5 + b_adj).clamp(0.0, 1.0);
                    g_norm = ((g_norm - 0.5) * c + 0.5 + b_adj).clamp(0.0, 1.0);
                    b_norm = ((b_norm - 0.5) * c + 0.5 + b_adj).clamp(0.0, 1.0);
                }

                rgba[rgba_idx] = (r_norm * 255.0).clamp(0.0, 255.0) as u8;
                rgba[rgba_idx + 1] = (g_norm * 255.0).clamp(0.0, 255.0) as u8;
                rgba[rgba_idx + 2] = (b_norm * 255.0).clamp(0.0, 255.0) as u8;
                rgba[rgba_idx + 3] = alpha;
            }
        }

        Ok(rgba)
    }

    /// Resample buffer to target dimensions
    pub fn resample(
        buffer: &RasterBuffer,
        target_width: u64,
        target_height: u64,
        method: ResamplingMethod,
    ) -> Result<RasterBuffer, RenderError> {
        let resampler = Resampler::new(method);
        resampler
            .resample(buffer, target_width, target_height)
            .map_err(RenderError::from)
    }

    /// Read a window from a buffer (subset extraction)
    pub fn read_window(
        buffer: &RasterBuffer,
        src_x: u64,
        src_y: u64,
        src_width: u64,
        src_height: u64,
    ) -> Result<RasterBuffer, RenderError> {
        let width = buffer.width();
        let height = buffer.height();

        // Validate bounds
        if src_x >= width || src_y >= height {
            return Err(RenderError::InvalidParameter(format!(
                "Window start ({}, {}) is outside buffer bounds ({}x{})",
                src_x, src_y, width, height
            )));
        }

        // Clamp window to buffer bounds
        let actual_width = (src_width).min(width - src_x);
        let actual_height = (src_height).min(height - src_y);

        // Create output buffer
        let data_type = buffer.data_type();
        let mut output = RasterBuffer::zeros(actual_width, actual_height, data_type);

        // Copy pixels
        for dy in 0..actual_height {
            for dx in 0..actual_width {
                let value = buffer
                    .get_pixel(src_x + dx, src_y + dy)
                    .map_err(|e| RenderError::ReadError(e.to_string()))?;
                output
                    .set_pixel(dx, dy, value)
                    .map_err(|e| RenderError::ReadError(e.to_string()))?;
            }
        }

        Ok(output)
    }
}

/// Encode RGBA data to PNG format
pub fn encode_png(data: &[u8], width: u32, height: u32) -> Result<Vec<u8>, RenderError> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);

        let mut writer = encoder
            .write_header()
            .map_err(|e| RenderError::EncodingError(e.to_string()))?;

        writer
            .write_image_data(data)
            .map_err(|e| RenderError::EncodingError(e.to_string()))?;
    }

    Ok(output)
}

/// Encode RGBA data to JPEG format
pub fn encode_jpeg(data: &[u8], width: u32, height: u32) -> Result<Vec<u8>, RenderError> {
    // Convert RGBA to RGB (JPEG doesn't support alpha)
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
        .map_err(|e| RenderError::EncodingError(e.to_string()))?;

    Ok(jpeg_buffer)
}

/// Encode RGBA data to the specified format
pub fn encode_image(
    data: &[u8],
    width: u32,
    height: u32,
    format: ImageFormat,
) -> Result<Bytes, RenderError> {
    let encoded = match format {
        ImageFormat::Png => encode_png(data, width, height)?,
        ImageFormat::Jpeg => encode_jpeg(data, width, height)?,
        ImageFormat::Webp => {
            return Err(RenderError::Unsupported(
                "WebP encoding not yet implemented".to_string(),
            ));
        }
        ImageFormat::Geotiff => {
            return Err(RenderError::Unsupported(
                "GeoTIFF encoding not yet implemented".to_string(),
            ));
        }
    };

    Ok(Bytes::from(encoded))
}

/// Calculate pixel coordinates from world coordinates
pub fn world_to_pixel(
    geo_transform: &GeoTransform,
    world_x: f64,
    world_y: f64,
) -> Result<(u64, u64), RenderError> {
    let (px, py) = geo_transform
        .world_to_pixel(world_x, world_y)
        .map_err(|e| RenderError::InvalidParameter(format!("Transform error: {}", e)))?;

    if px < 0.0 || py < 0.0 {
        return Err(RenderError::InvalidParameter(format!(
            "Negative pixel coordinates: ({}, {})",
            px, py
        )));
    }

    Ok((px as u64, py as u64))
}

/// Calculate bounding box for a tile
pub fn tile_to_bbox(
    tile_matrix_set: &str,
    z: u32,
    x: u32,
    y: u32,
) -> Result<BoundingBox, RenderError> {
    match tile_matrix_set {
        "WebMercatorQuad" => {
            let world_extent = 20_037_508.342_789_244;
            let tile_count = 1u64 << z;
            let tile_size = (2.0 * world_extent) / tile_count as f64;

            let min_x = -world_extent + (x as f64) * tile_size;
            let max_x = min_x + tile_size;
            let max_y = world_extent - (y as f64) * tile_size;
            let min_y = max_y - tile_size;

            BoundingBox::new(min_x, min_y, max_x, max_y)
                .map_err(|e| RenderError::InvalidParameter(format!("Invalid bbox: {}", e)))
        }
        "WorldCRS84Quad" => {
            let tiles_x = 2u64 << z;
            let tiles_y = 1u64 << z;
            let tile_width = 360.0 / tiles_x as f64;
            let tile_height = 180.0 / tiles_y as f64;

            let min_x = -180.0 + (x as f64) * tile_width;
            let max_x = min_x + tile_width;
            let max_y = 90.0 - (y as f64) * tile_height;
            let min_y = max_y - tile_height;

            BoundingBox::new(min_x, min_y, max_x, max_y)
                .map_err(|e| RenderError::InvalidParameter(format!("Invalid bbox: {}", e)))
        }
        _ => Err(RenderError::InvalidParameter(format!(
            "Unknown tile matrix set: {}",
            tile_matrix_set
        ))),
    }
}

/// Create a synthetic test buffer for testing
#[cfg(test)]
pub fn create_test_buffer(width: u64, height: u64) -> RasterBuffer {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let value = (x as f64 + y as f64 * width as f64) / (width * height) as f64 * 100.0;
            let _ = buffer.set_pixel(x, y, value);
        }
    }

    buffer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_colormap_grayscale() {
        let cm = Colormap::Grayscale;
        assert_eq!(cm.apply(0.0), (0, 0, 0));
        assert_eq!(cm.apply(1.0), (255, 255, 255));
        assert_eq!(cm.apply(0.5), (127, 127, 127));
    }

    #[test]
    fn test_colormap_from_name() {
        assert!(Colormap::from_name("viridis").is_some());
        assert!(Colormap::from_name("VIRIDIS").is_some());
        assert!(Colormap::from_name("terrain").is_some());
        assert!(Colormap::from_name("unknown").is_none());
    }

    #[test]
    fn test_render_to_rgba() {
        let buffer = create_test_buffer(10, 10);
        let style = RenderStyle::default();

        let result = RasterRenderer::render_to_rgba(&buffer, &style);
        assert!(result.is_ok());

        let rgba = result.expect("render should succeed");
        assert_eq!(rgba.len(), 10 * 10 * 4);
    }

    #[test]
    fn test_encode_png() {
        let rgba = vec![128u8; 4 * 4 * 4]; // 4x4 image
        let result = encode_png(&rgba, 4, 4);
        assert!(result.is_ok());
    }

    #[test]
    fn test_tile_to_bbox_web_mercator() {
        let bbox = tile_to_bbox("WebMercatorQuad", 0, 0, 0);
        assert!(bbox.is_ok());

        let bbox = bbox.expect("bbox should be valid");
        assert!(bbox.min_x < bbox.max_x);
        assert!(bbox.min_y < bbox.max_y);
    }

    #[test]
    fn test_tile_to_bbox_wgs84() {
        let bbox = tile_to_bbox("WorldCRS84Quad", 0, 0, 0);
        assert!(bbox.is_ok());

        let bbox = bbox.expect("bbox should be valid");
        assert_eq!(bbox.min_x, -180.0);
        assert_eq!(bbox.max_y, 90.0);
    }

    #[test]
    fn test_read_window() {
        let buffer = create_test_buffer(100, 100);
        let window = RasterRenderer::read_window(&buffer, 10, 10, 20, 20);

        assert!(window.is_ok());
        let window = window.expect("window should succeed");
        assert_eq!(window.width(), 20);
        assert_eq!(window.height(), 20);
    }

    #[test]
    fn test_resample() {
        let buffer = create_test_buffer(100, 100);
        let resampled = RasterRenderer::resample(&buffer, 50, 50, ResamplingMethod::Bilinear);

        assert!(resampled.is_ok());
        let resampled = resampled.expect("resample should succeed");
        assert_eq!(resampled.width(), 50);
        assert_eq!(resampled.height(), 50);
    }
}
