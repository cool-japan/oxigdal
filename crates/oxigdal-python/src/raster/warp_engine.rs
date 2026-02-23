//! Raster warping engine for coordinate transformation and resampling.
//!
//! This module provides the `RasterWarpEngine` for performing raster reprojection
//! and resampling operations with support for multiple interpolation methods.

use oxigdal_proj::{Coordinate, Crs, Transformer};
use std::collections::HashMap;

/// Resampling method for raster operations
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResamplingMethod {
    /// Nearest neighbor (fastest, no interpolation)
    Nearest,
    /// Bilinear interpolation (2x2 neighborhood)
    Bilinear,
    /// Cubic interpolation (4x4 neighborhood)
    Cubic,
    /// Lanczos interpolation (6x6 neighborhood, highest quality)
    Lanczos,
    /// Average of contributing pixels
    Average,
    /// Mode (most frequent value) for categorical data
    Mode,
}

impl ResamplingMethod {
    /// Parses a resampling method from a string.
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "nearest" => Ok(Self::Nearest),
            "bilinear" => Ok(Self::Bilinear),
            "cubic" => Ok(Self::Cubic),
            "lanczos" => Ok(Self::Lanczos),
            "average" => Ok(Self::Average),
            "mode" => Ok(Self::Mode),
            _ => Err(format!("Unknown resampling method: {}", s)),
        }
    }
}

/// Raster warping engine for coordinate transformation and resampling.
pub struct RasterWarpEngine {
    /// Source data (2D array, row-major order)
    pub(crate) src_data: Vec<f64>,
    /// Source width
    pub(crate) src_width: usize,
    /// Source height
    pub(crate) src_height: usize,
    /// Source nodata value
    pub(crate) src_nodata: Option<f64>,
    /// Source geotransform [origin_x, pixel_width, 0, origin_y, 0, -pixel_height]
    pub(crate) src_geotransform: [f64; 6],
    /// Source CRS
    pub(crate) src_crs: Option<Crs>,
}

impl RasterWarpEngine {
    /// Creates a new warp engine.
    pub fn new(
        src_data: Vec<f64>,
        src_width: usize,
        src_height: usize,
        src_nodata: Option<f64>,
        src_geotransform: [f64; 6],
        src_crs: Option<Crs>,
    ) -> Self {
        Self {
            src_data,
            src_width,
            src_height,
            src_nodata,
            src_geotransform,
            src_crs,
        }
    }

    /// Converts pixel coordinates to geographic coordinates.
    pub(crate) fn pixel_to_geo(&self, col: f64, row: f64) -> (f64, f64) {
        let x = self.src_geotransform[0]
            + col * self.src_geotransform[1]
            + row * self.src_geotransform[2];
        let y = self.src_geotransform[3]
            + col * self.src_geotransform[4]
            + row * self.src_geotransform[5];
        (x, y)
    }

    /// Converts geographic coordinates to pixel coordinates.
    pub(crate) fn geo_to_pixel(&self, x: f64, y: f64) -> (f64, f64) {
        // Inverse of affine transformation
        let det = self.src_geotransform[1] * self.src_geotransform[5]
            - self.src_geotransform[2] * self.src_geotransform[4];
        if det.abs() < 1e-12 {
            return (f64::NAN, f64::NAN);
        }

        let dx = x - self.src_geotransform[0];
        let dy = y - self.src_geotransform[3];

        let col = (self.src_geotransform[5] * dx - self.src_geotransform[2] * dy) / det;
        let row = (-self.src_geotransform[4] * dx + self.src_geotransform[1] * dy) / det;

        (col, row)
    }

    /// Gets source pixel value with bounds checking.
    pub(crate) fn get_pixel(&self, col: isize, row: isize) -> Option<f64> {
        if col < 0 || row < 0 {
            return None;
        }
        let col = col as usize;
        let row = row as usize;
        if col >= self.src_width || row >= self.src_height {
            return None;
        }

        let idx = row * self.src_width + col;
        let value = self.src_data.get(idx).copied()?;

        // Check for nodata
        if let Some(nodata) = self.src_nodata {
            if (value - nodata).abs() < 1e-10 || value.is_nan() {
                return None;
            }
        }

        Some(value)
    }

    /// Nearest neighbor resampling.
    fn resample_nearest(&self, src_col: f64, src_row: f64) -> Option<f64> {
        let col = src_col.round() as isize;
        let row = src_row.round() as isize;
        self.get_pixel(col, row)
    }

    /// Bilinear interpolation (2x2 neighborhood).
    fn resample_bilinear(&self, src_col: f64, src_row: f64) -> Option<f64> {
        let col0 = src_col.floor() as isize;
        let row0 = src_row.floor() as isize;
        let col1 = col0 + 1;
        let row1 = row0 + 1;

        let dx = src_col - col0 as f64;
        let dy = src_row - row0 as f64;

        // Get 4 corner values
        let v00 = self.get_pixel(col0, row0)?;
        let v10 = self.get_pixel(col1, row0)?;
        let v01 = self.get_pixel(col0, row1)?;
        let v11 = self.get_pixel(col1, row1)?;

        // Bilinear interpolation
        let v0 = v00 * (1.0 - dx) + v10 * dx;
        let v1 = v01 * (1.0 - dx) + v11 * dx;
        let result = v0 * (1.0 - dy) + v1 * dy;

        Some(result)
    }

    /// Cubic interpolation weight (Catmull-Rom spline).
    pub(crate) fn cubic_weight(t: f64) -> f64 {
        let t_abs = t.abs();
        if t_abs <= 1.0 {
            (1.5 * t_abs - 2.5) * t_abs * t_abs + 1.0
        } else if t_abs <= 2.0 {
            ((-0.5 * t_abs + 2.5) * t_abs - 4.0) * t_abs + 2.0
        } else {
            0.0
        }
    }

    /// Cubic interpolation (4x4 neighborhood).
    fn resample_cubic(&self, src_col: f64, src_row: f64) -> Option<f64> {
        let col0 = src_col.floor() as isize - 1;
        let row0 = src_row.floor() as isize - 1;

        let dx = src_col - (col0 + 1) as f64;
        let dy = src_row - (row0 + 1) as f64;

        let mut sum = 0.0;
        let mut weight_sum = 0.0;
        let mut valid_count = 0;

        for j in 0..4 {
            let wy = Self::cubic_weight(dy - (j - 1) as f64);
            for i in 0..4 {
                let wx = Self::cubic_weight(dx - (i - 1) as f64);
                let weight = wx * wy;

                if let Some(value) = self.get_pixel(col0 + i, row0 + j) {
                    sum += value * weight;
                    weight_sum += weight;
                    valid_count += 1;
                }
            }
        }

        // Require at least 8 valid pixels for cubic interpolation
        if valid_count >= 8 && weight_sum.abs() > 1e-10 {
            Some(sum / weight_sum)
        } else if valid_count > 0 {
            // Fall back to nearest neighbor
            self.resample_nearest(src_col, src_row)
        } else {
            None
        }
    }

    /// Lanczos sinc function.
    pub(crate) fn sinc(x: f64) -> f64 {
        if x.abs() < 1e-10 {
            1.0
        } else {
            let pi_x = std::f64::consts::PI * x;
            pi_x.sin() / pi_x
        }
    }

    /// Lanczos kernel weight (a=3).
    pub(crate) fn lanczos_weight(t: f64, a: f64) -> f64 {
        let t_abs = t.abs();
        if t_abs < a {
            Self::sinc(t) * Self::sinc(t / a)
        } else {
            0.0
        }
    }

    /// Lanczos interpolation (6x6 neighborhood for a=3).
    fn resample_lanczos(&self, src_col: f64, src_row: f64) -> Option<f64> {
        const A: f64 = 3.0;
        let a_i = A as isize;

        let col0 = src_col.floor() as isize - a_i + 1;
        let row0 = src_row.floor() as isize - a_i + 1;

        let dx = src_col - src_col.floor();
        let dy = src_row - src_row.floor();

        let mut sum = 0.0;
        let mut weight_sum = 0.0;
        let mut valid_count = 0;
        let kernel_size = 2 * a_i;

        for j in 0..kernel_size {
            let wy = Self::lanczos_weight(dy - (j - a_i + 1) as f64, A);
            for i in 0..kernel_size {
                let wx = Self::lanczos_weight(dx - (i - a_i + 1) as f64, A);
                let weight = wx * wy;

                if let Some(value) = self.get_pixel(col0 + i, row0 + j) {
                    sum += value * weight;
                    weight_sum += weight;
                    valid_count += 1;
                }
            }
        }

        // Require at least half the kernel size for Lanczos
        let min_valid = (kernel_size * kernel_size / 2) as i32;
        if valid_count >= min_valid && weight_sum.abs() > 1e-10 {
            Some(sum / weight_sum)
        } else if valid_count > 0 {
            // Fall back to bilinear
            self.resample_bilinear(src_col, src_row)
        } else {
            None
        }
    }

    /// Average resampling (all contributing pixels).
    fn resample_average(&self, src_col: f64, src_row: f64) -> Option<f64> {
        // For average resampling, use a 2x2 box around the target point
        let col0 = src_col.floor() as isize;
        let row0 = src_row.floor() as isize;

        let mut sum = 0.0;
        let mut count = 0;

        for dr in 0..2 {
            for dc in 0..2 {
                if let Some(value) = self.get_pixel(col0 + dc, row0 + dr) {
                    sum += value;
                    count += 1;
                }
            }
        }

        if count > 0 {
            Some(sum / count as f64)
        } else {
            None
        }
    }

    /// Mode resampling (most frequent value in neighborhood).
    fn resample_mode(&self, src_col: f64, src_row: f64) -> Option<f64> {
        let col0 = src_col.floor() as isize;
        let row0 = src_row.floor() as isize;

        // Collect values from 3x3 neighborhood
        let mut values: Vec<f64> = Vec::with_capacity(9);
        for dr in -1..=1 {
            for dc in -1..=1 {
                if let Some(value) = self.get_pixel(col0 + dc, row0 + dr) {
                    values.push(value);
                }
            }
        }

        if values.is_empty() {
            return None;
        }

        // For mode, we round to nearest integer and find most common
        let mut counts: HashMap<i64, usize> = HashMap::new();
        for &v in &values {
            let key = v.round() as i64;
            *counts.entry(key).or_insert(0) += 1;
        }

        counts
            .into_iter()
            .max_by_key(|&(_, count)| count)
            .map(|(key, _)| key as f64)
    }

    /// Resamples a single pixel using the specified method.
    pub fn resample_pixel(
        &self,
        src_col: f64,
        src_row: f64,
        method: ResamplingMethod,
    ) -> Option<f64> {
        match method {
            ResamplingMethod::Nearest => self.resample_nearest(src_col, src_row),
            ResamplingMethod::Bilinear => self.resample_bilinear(src_col, src_row),
            ResamplingMethod::Cubic => self.resample_cubic(src_col, src_row),
            ResamplingMethod::Lanczos => self.resample_lanczos(src_col, src_row),
            ResamplingMethod::Average => self.resample_average(src_col, src_row),
            ResamplingMethod::Mode => self.resample_mode(src_col, src_row),
        }
    }

    /// Warps the raster to a new CRS and/or resolution.
    ///
    /// Returns (output_data, output_width, output_height, output_geotransform).
    pub fn warp(
        &self,
        dst_crs: Option<&Crs>,
        dst_width: Option<usize>,
        dst_height: Option<usize>,
        dst_nodata: Option<f64>,
        method: ResamplingMethod,
    ) -> Result<(Vec<f64>, usize, usize, [f64; 6]), String> {
        // Compute source extent in source CRS
        let (src_min_x, src_max_y) = self.pixel_to_geo(0.0, 0.0);
        let (src_max_x, src_min_y) =
            self.pixel_to_geo(self.src_width as f64, self.src_height as f64);

        // Determine output parameters
        let (out_width, out_height, out_geotransform, transformer) =
            if let Some(target_crs) = dst_crs {
                // Transform extent to target CRS
                let transformer = if let Some(ref src) = self.src_crs {
                    Some(
                        Transformer::new(src.clone(), target_crs.clone())
                            .map_err(|e| format!("Failed to create transformer: {}", e))?,
                    )
                } else {
                    None
                };

                // Transform corner points to get output extent
                let corners = [
                    (src_min_x, src_min_y),
                    (src_max_x, src_min_y),
                    (src_max_x, src_max_y),
                    (src_min_x, src_max_y),
                ];

                let transformed_corners: Vec<(f64, f64)> = if let Some(ref t) = transformer {
                    let mut result = Vec::with_capacity(4);
                    for (x, y) in corners {
                        let coord = Coordinate::new(x, y);
                        match t.transform(&coord) {
                            Ok(tc) => result.push((tc.x, tc.y)),
                            Err(e) => return Err(format!("Failed to transform corner: {}", e)),
                        }
                    }
                    result
                } else {
                    corners.to_vec()
                };

                // Find output extent
                let out_min_x = transformed_corners
                    .iter()
                    .map(|c| c.0)
                    .fold(f64::INFINITY, f64::min);
                let out_max_x = transformed_corners
                    .iter()
                    .map(|c| c.0)
                    .fold(f64::NEG_INFINITY, f64::max);
                let out_min_y = transformed_corners
                    .iter()
                    .map(|c| c.1)
                    .fold(f64::INFINITY, f64::min);
                let out_max_y = transformed_corners
                    .iter()
                    .map(|c| c.1)
                    .fold(f64::NEG_INFINITY, f64::max);

                // Compute output resolution and dimensions
                let (ow, oh) = match (dst_width, dst_height) {
                    (Some(w), Some(h)) => (w, h),
                    (Some(w), None) => {
                        let aspect = (out_max_y - out_min_y) / (out_max_x - out_min_x);
                        (w, (w as f64 * aspect).round() as usize)
                    }
                    (None, Some(h)) => {
                        let aspect = (out_max_x - out_min_x) / (out_max_y - out_min_y);
                        ((h as f64 * aspect).round() as usize, h)
                    }
                    (None, None) => {
                        // Preserve approximate pixel count
                        let src_pixel_count = self.src_width * self.src_height;
                        let out_aspect = (out_max_x - out_min_x) / (out_max_y - out_min_y);
                        let oh = (src_pixel_count as f64 / out_aspect).sqrt().round() as usize;
                        let ow = (src_pixel_count as f64 * out_aspect).sqrt().round() as usize;
                        (ow.max(1), oh.max(1))
                    }
                };

                let pixel_width = (out_max_x - out_min_x) / ow as f64;
                let pixel_height = (out_max_y - out_min_y) / oh as f64;

                let gt = [out_min_x, pixel_width, 0.0, out_max_y, 0.0, -pixel_height];

                (ow, oh, gt, transformer)
            } else {
                // No CRS transformation, just resize
                let (ow, oh) = match (dst_width, dst_height) {
                    (Some(w), Some(h)) => (w, h),
                    (Some(w), None) => {
                        let aspect = self.src_height as f64 / self.src_width as f64;
                        (w, (w as f64 * aspect).round() as usize)
                    }
                    (None, Some(h)) => {
                        let aspect = self.src_width as f64 / self.src_height as f64;
                        ((h as f64 * aspect).round() as usize, h)
                    }
                    (None, None) => (self.src_width, self.src_height),
                };

                let pixel_width = (src_max_x - src_min_x) / ow as f64;
                let pixel_height = (src_max_y - src_min_y) / oh as f64;

                let gt = [src_min_x, pixel_width, 0.0, src_max_y, 0.0, -pixel_height];

                (ow, oh, gt, None)
            };

        // Create inverse transformer for output -> source
        let inverse_transformer = if let Some(ref t) = transformer {
            Some(
                Transformer::new(t.target_crs().clone(), t.source_crs().clone())
                    .map_err(|e| format!("Failed to create inverse transformer: {}", e))?,
            )
        } else {
            None
        };

        // Allocate output buffer
        let nodata = dst_nodata.or(self.src_nodata).unwrap_or(f64::NAN);
        let mut out_data = vec![nodata; out_width * out_height];

        // Process each output pixel
        for out_row in 0..out_height {
            for out_col in 0..out_width {
                // Output pixel center in output CRS
                let out_x = out_geotransform[0]
                    + (out_col as f64 + 0.5) * out_geotransform[1]
                    + (out_row as f64 + 0.5) * out_geotransform[2];
                let out_y = out_geotransform[3]
                    + (out_col as f64 + 0.5) * out_geotransform[4]
                    + (out_row as f64 + 0.5) * out_geotransform[5];

                // Transform to source CRS
                let (src_x, src_y) = if let Some(ref t) = inverse_transformer {
                    let coord = Coordinate::new(out_x, out_y);
                    match t.transform(&coord) {
                        Ok(tc) => (tc.x, tc.y),
                        Err(_) => continue, // Skip pixels that can't be transformed
                    }
                } else {
                    (out_x, out_y)
                };

                // Convert to source pixel coordinates
                let (src_col, src_row) = self.geo_to_pixel(src_x, src_y);

                // Skip if outside source bounds
                if src_col < -0.5
                    || src_row < -0.5
                    || src_col > self.src_width as f64 - 0.5
                    || src_row > self.src_height as f64 - 0.5
                {
                    continue;
                }

                // Resample
                if let Some(value) = self.resample_pixel(src_col, src_row, method) {
                    out_data[out_row * out_width + out_col] = value;
                }
            }
        }

        Ok((out_data, out_width, out_height, out_geotransform))
    }

    /// Resamples the raster to a new resolution.
    pub fn resample_to_resolution(
        &self,
        target_res_x: f64,
        target_res_y: f64,
        dst_nodata: Option<f64>,
        method: ResamplingMethod,
    ) -> Result<(Vec<f64>, usize, usize, [f64; 6]), String> {
        if target_res_x <= 0.0 || target_res_y <= 0.0 {
            return Err("Resolution must be positive".to_string());
        }

        // Compute source extent
        let (src_min_x, src_max_y) = self.pixel_to_geo(0.0, 0.0);
        let (src_max_x, src_min_y) =
            self.pixel_to_geo(self.src_width as f64, self.src_height as f64);

        // Compute output dimensions
        let out_width = ((src_max_x - src_min_x) / target_res_x).ceil() as usize;
        let out_height = ((src_max_y - src_min_y) / target_res_y).ceil() as usize;

        if out_width == 0 || out_height == 0 {
            return Err("Output dimensions would be zero".to_string());
        }

        // Use warp with no CRS transformation
        self.warp(None, Some(out_width), Some(out_height), dst_nodata, method)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== Resampling Method Tests ==========

    #[test]
    fn test_resampling_method_from_str() {
        assert_eq!(
            ResamplingMethod::from_str("nearest").expect("should parse"),
            ResamplingMethod::Nearest
        );
        assert_eq!(
            ResamplingMethod::from_str("bilinear").expect("should parse"),
            ResamplingMethod::Bilinear
        );
        assert_eq!(
            ResamplingMethod::from_str("cubic").expect("should parse"),
            ResamplingMethod::Cubic
        );
        assert_eq!(
            ResamplingMethod::from_str("lanczos").expect("should parse"),
            ResamplingMethod::Lanczos
        );
        assert_eq!(
            ResamplingMethod::from_str("average").expect("should parse"),
            ResamplingMethod::Average
        );
        assert_eq!(
            ResamplingMethod::from_str("mode").expect("should parse"),
            ResamplingMethod::Mode
        );

        // Case insensitive
        assert_eq!(
            ResamplingMethod::from_str("BILINEAR").expect("should parse"),
            ResamplingMethod::Bilinear
        );

        // Invalid method
        assert!(ResamplingMethod::from_str("invalid").is_err());
    }

    // ========== RasterWarpEngine Tests ==========

    fn create_test_raster(width: usize, height: usize) -> Vec<f64> {
        (0..(width * height)).map(|i| (i % 256) as f64).collect()
    }

    fn create_gradient_raster(width: usize, height: usize) -> Vec<f64> {
        (0..height)
            .flat_map(|row| (0..width).map(move |col| (row + col) as f64))
            .collect()
    }

    #[test]
    fn test_warp_engine_creation() {
        let data = create_test_raster(10, 10);
        let gt = [0.0, 1.0, 0.0, 10.0, 0.0, -1.0];
        let engine = RasterWarpEngine::new(data.clone(), 10, 10, None, gt, None);

        assert_eq!(engine.src_width, 10);
        assert_eq!(engine.src_height, 10);
        assert!(engine.src_nodata.is_none());
    }

    #[test]
    fn test_pixel_to_geo_conversion() {
        let data = create_test_raster(10, 10);
        let gt = [100.0, 10.0, 0.0, 500.0, 0.0, -10.0];
        let engine = RasterWarpEngine::new(data, 10, 10, None, gt, None);

        // Test corner coordinates
        let (x, y) = engine.pixel_to_geo(0.0, 0.0);
        assert!((x - 100.0).abs() < 1e-10);
        assert!((y - 500.0).abs() < 1e-10);

        let (x, y) = engine.pixel_to_geo(10.0, 10.0);
        assert!((x - 200.0).abs() < 1e-10);
        assert!((y - 400.0).abs() < 1e-10);

        // Test center
        let (x, y) = engine.pixel_to_geo(5.0, 5.0);
        assert!((x - 150.0).abs() < 1e-10);
        assert!((y - 450.0).abs() < 1e-10);
    }

    #[test]
    fn test_geo_to_pixel_conversion() {
        let data = create_test_raster(10, 10);
        let gt = [100.0, 10.0, 0.0, 500.0, 0.0, -10.0];
        let engine = RasterWarpEngine::new(data, 10, 10, None, gt, None);

        // Test corner coordinates
        let (col, row) = engine.geo_to_pixel(100.0, 500.0);
        assert!((col - 0.0).abs() < 1e-10);
        assert!((row - 0.0).abs() < 1e-10);

        let (col, row) = engine.geo_to_pixel(200.0, 400.0);
        assert!((col - 10.0).abs() < 1e-10);
        assert!((row - 10.0).abs() < 1e-10);

        // Test center
        let (col, row) = engine.geo_to_pixel(150.0, 450.0);
        assert!((col - 5.0).abs() < 1e-10);
        assert!((row - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_get_pixel_bounds_checking() {
        let data = create_test_raster(5, 5);
        let gt = [0.0, 1.0, 0.0, 5.0, 0.0, -1.0];
        let engine = RasterWarpEngine::new(data, 5, 5, None, gt, None);

        // Valid pixels
        assert!(engine.get_pixel(0, 0).is_some());
        assert!(engine.get_pixel(4, 4).is_some());

        // Out of bounds
        assert!(engine.get_pixel(-1, 0).is_none());
        assert!(engine.get_pixel(0, -1).is_none());
        assert!(engine.get_pixel(5, 0).is_none());
        assert!(engine.get_pixel(0, 5).is_none());
    }

    #[test]
    fn test_get_pixel_nodata_handling() {
        let mut data = create_test_raster(5, 5);
        data[12] = -9999.0; // Set center pixel to nodata
        let gt = [0.0, 1.0, 0.0, 5.0, 0.0, -1.0];
        let engine = RasterWarpEngine::new(data, 5, 5, Some(-9999.0), gt, None);

        // Normal pixel
        assert!(engine.get_pixel(0, 0).is_some());

        // Nodata pixel
        assert!(engine.get_pixel(2, 2).is_none());
    }

    #[test]
    fn test_resample_nearest() {
        let data = create_gradient_raster(10, 10);
        let gt = [0.0, 1.0, 0.0, 10.0, 0.0, -1.0];
        let engine = RasterWarpEngine::new(data, 10, 10, None, gt, None);

        // At pixel center
        let result = engine.resample_pixel(5.0, 5.0, ResamplingMethod::Nearest);
        assert!(result.is_some());
        assert!((result.expect("should have value") - 10.0).abs() < 1e-10); // row=5, col=5 => 5+5=10

        // At pixel edge (should round)
        let result = engine.resample_pixel(5.5, 5.5, ResamplingMethod::Nearest);
        assert!(result.is_some());
        assert!((result.expect("should have value") - 12.0).abs() < 1e-10); // rounds to 6,6 => 6+6=12
    }

    #[test]
    fn test_resample_bilinear() {
        // Create a simple gradient for predictable interpolation
        let data: Vec<f64> = vec![
            0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0,
        ];
        let gt = [0.0, 1.0, 0.0, 4.0, 0.0, -1.0];
        let engine = RasterWarpEngine::new(data, 4, 4, None, gt, None);

        // At pixel center - should return exact value
        let result = engine.resample_pixel(1.0, 1.0, ResamplingMethod::Bilinear);
        assert!(result.is_some());
        assert!((result.expect("should have value") - 5.0).abs() < 1e-10);

        // Between pixels - should interpolate
        let result = engine.resample_pixel(1.5, 1.5, ResamplingMethod::Bilinear);
        assert!(result.is_some());
        // Should be average of 5, 6, 9, 10 = 7.5
        assert!((result.expect("should have value") - 7.5).abs() < 1e-10);
    }

    #[test]
    fn test_resample_cubic() {
        // Create a larger raster for cubic (needs 4x4 neighborhood)
        let data = create_gradient_raster(10, 10);
        let gt = [0.0, 1.0, 0.0, 10.0, 0.0, -1.0];
        let engine = RasterWarpEngine::new(data, 10, 10, None, gt, None);

        // In the interior where we have a full 4x4 neighborhood
        let result = engine.resample_pixel(5.0, 5.0, ResamplingMethod::Cubic);
        assert!(result.is_some());

        // At pixel center, cubic should be close to the actual value
        let value = result.expect("should have value");
        assert!((value - 10.0).abs() < 1.0); // Should be close to row+col = 10
    }

    #[test]
    fn test_resample_lanczos() {
        // Create a larger raster for lanczos (needs 6x6 neighborhood)
        let data = create_gradient_raster(20, 20);
        let gt = [0.0, 1.0, 0.0, 20.0, 0.0, -1.0];
        let engine = RasterWarpEngine::new(data, 20, 20, None, gt, None);

        // In the interior where we have a full neighborhood
        let result = engine.resample_pixel(10.0, 10.0, ResamplingMethod::Lanczos);
        assert!(result.is_some());

        let value = result.expect("should have value");
        assert!((value - 20.0).abs() < 1.0); // Should be close to row+col = 20
    }

    #[test]
    fn test_resample_average() {
        let data: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let gt = [0.0, 1.0, 0.0, 3.0, 0.0, -1.0];
        let engine = RasterWarpEngine::new(data, 3, 3, None, gt, None);

        // Average of 2x2 neighborhood starting at (0,0)
        let result = engine.resample_pixel(0.5, 0.5, ResamplingMethod::Average);
        assert!(result.is_some());
        // Average of 1, 2, 4, 5 = 3.0
        assert!((result.expect("should have value") - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_resample_mode() {
        // Create data with repeated values
        let data: Vec<f64> = vec![1.0, 1.0, 2.0, 1.0, 1.0, 2.0, 3.0, 3.0, 2.0];
        let gt = [0.0, 1.0, 0.0, 3.0, 0.0, -1.0];
        let engine = RasterWarpEngine::new(data, 3, 3, None, gt, None);

        // Mode around center should be 1.0 (appears 4 times vs 3 times for 2.0)
        let result = engine.resample_pixel(1.0, 1.0, ResamplingMethod::Mode);
        assert!(result.is_some());
        assert!((result.expect("should have value") - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_warp_resize_only() {
        let data = create_gradient_raster(10, 10);
        let gt = [0.0, 1.0, 0.0, 10.0, 0.0, -1.0];
        let engine = RasterWarpEngine::new(data, 10, 10, None, gt, None);

        // Resize to 5x5
        let result = engine.warp(None, Some(5), Some(5), None, ResamplingMethod::Nearest);
        assert!(result.is_ok());

        let (out_data, out_width, out_height, _out_gt) = result.expect("should succeed");
        assert_eq!(out_width, 5);
        assert_eq!(out_height, 5);
        assert_eq!(out_data.len(), 25);
    }

    #[test]
    fn test_warp_with_aspect_preservation() {
        let data = create_gradient_raster(20, 10);
        let gt = [0.0, 1.0, 0.0, 10.0, 0.0, -1.0];
        let engine = RasterWarpEngine::new(data, 20, 10, None, gt, None);

        // Specify only width, height should be computed
        let result = engine.warp(None, Some(10), None, None, ResamplingMethod::Bilinear);
        assert!(result.is_ok());

        let (_, out_width, out_height, _) = result.expect("should succeed");
        assert_eq!(out_width, 10);
        // Height should be approximately 5 (aspect ratio 2:1)
        assert!((4..=6).contains(&out_height));
    }

    #[test]
    fn test_warp_nodata_handling() {
        let mut data = create_gradient_raster(10, 10);
        // Set some nodata values
        data[0] = -9999.0;
        data[50] = -9999.0;

        let gt = [0.0, 1.0, 0.0, 10.0, 0.0, -1.0];
        let engine = RasterWarpEngine::new(data, 10, 10, Some(-9999.0), gt, None);

        let result = engine.warp(
            None,
            Some(10),
            Some(10),
            Some(-9999.0),
            ResamplingMethod::Nearest,
        );
        assert!(result.is_ok());

        let (out_data, _, _, _) = result.expect("should succeed");
        // Check that nodata is propagated
        assert!((out_data[0] - (-9999.0)).abs() < 1e-10 || out_data[0].is_nan());
    }

    #[test]
    fn test_resample_to_resolution() {
        let data = create_gradient_raster(100, 100);
        let gt = [0.0, 1.0, 0.0, 100.0, 0.0, -1.0];
        let engine = RasterWarpEngine::new(data, 100, 100, None, gt, None);

        // Resample to 2.0 resolution (should halve the dimensions)
        let result = engine.resample_to_resolution(2.0, 2.0, None, ResamplingMethod::Bilinear);
        assert!(result.is_ok());

        let (_, out_width, out_height, out_gt) = result.expect("should succeed");
        assert_eq!(out_width, 50);
        assert_eq!(out_height, 50);
        assert!((out_gt[1] - 2.0).abs() < 1e-10);
        assert!((out_gt[5] - (-2.0)).abs() < 1e-10);
    }

    #[test]
    fn test_resample_to_resolution_invalid() {
        let data = create_gradient_raster(10, 10);
        let gt = [0.0, 1.0, 0.0, 10.0, 0.0, -1.0];
        let engine = RasterWarpEngine::new(data, 10, 10, None, gt, None);

        // Invalid resolution
        assert!(
            engine
                .resample_to_resolution(0.0, 1.0, None, ResamplingMethod::Nearest)
                .is_err()
        );
        assert!(
            engine
                .resample_to_resolution(1.0, -1.0, None, ResamplingMethod::Nearest)
                .is_err()
        );
    }

    #[test]
    fn test_cubic_weight() {
        // Test cubic weight function
        assert!((RasterWarpEngine::cubic_weight(0.0) - 1.0).abs() < 1e-10);
        assert!(RasterWarpEngine::cubic_weight(0.5) > 0.0);
        assert!(RasterWarpEngine::cubic_weight(1.0) < 1e-10);
        assert!(RasterWarpEngine::cubic_weight(2.0).abs() < 1e-10);
        assert!(RasterWarpEngine::cubic_weight(3.0).abs() < 1e-10);
    }

    #[test]
    fn test_lanczos_weight() {
        // Test lanczos weight function
        assert!((RasterWarpEngine::lanczos_weight(0.0, 3.0) - 1.0).abs() < 1e-10);
        assert!(RasterWarpEngine::lanczos_weight(1.0, 3.0).abs() < 0.5);
        assert!(RasterWarpEngine::lanczos_weight(3.0, 3.0).abs() < 1e-10);
        assert!((RasterWarpEngine::lanczos_weight(4.0, 3.0) - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_sinc() {
        // Test sinc function
        assert!((RasterWarpEngine::sinc(0.0) - 1.0).abs() < 1e-10);
        assert!(RasterWarpEngine::sinc(1.0).abs() < 1e-10);
        assert!(RasterWarpEngine::sinc(0.5).abs() > 0.0);
    }
}
