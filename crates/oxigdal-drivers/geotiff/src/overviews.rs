//! GeoTIFF overview (pyramid) generation and reading.
//!
//! Provides resampling kernels for building reduced-resolution datasets (overviews)
//! from source rasters, plus statistics and histogram computation.

use std::collections::HashMap;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors produced by the overview subsystem.
#[derive(Debug, Error)]
pub enum OverviewError {
    /// Source raster is empty (zero width or height).
    #[error("empty raster: width or height is zero")]
    EmptyRaster,
    /// Downsample factor must be ≥ 2.
    #[error("invalid overview factor {0}: must be ≥ 2")]
    InvalidFactor(u32),
    /// Tile size must be a power of two and ≥ 1.
    #[error("invalid tile size {0}: must be ≥ 1")]
    InvalidTileSize(u32),
    /// A resampling operation failed.
    #[error("resample error: {0}")]
    ResampleError(String),
}

// ---------------------------------------------------------------------------
// ResampleMethod
// ---------------------------------------------------------------------------

/// Method used to downsample raster data when building overviews.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResampleMethod {
    /// Nearest-neighbour (fast, aliased).
    Nearest,
    /// 2×2 bilinear weighted average.
    Bilinear,
    /// 4×4 bicubic convolution.
    Bicubic,
    /// Arithmetic mean of source pixels covered by the output pixel.
    #[default]
    Average,
    /// Most-common value (suitable for categorical / thematic data).
    Mode,
    /// Lanczos windowed-sinc (a = 3).
    Lanczos,
    /// Gaussian blur then subsample.
    Gauss,
    /// Minimum value of source window.
    Min,
    /// Maximum value of source window.
    Max,
    /// Median value of source window.
    Median,
}

impl ResampleMethod {
    /// Returns `true` for methods that preserve exact source values
    /// (i.e. no interpolation artefacts).
    #[must_use]
    pub fn is_exact(self) -> bool {
        matches!(self, Self::Nearest | Self::Mode)
    }

    /// Convolution kernel half-width (full kernel = `kernel_size × kernel_size`).
    ///
    /// - `Nearest`  → 1
    /// - `Bilinear` → 2
    /// - `Bicubic`  → 4
    /// - `Lanczos`  → 6
    /// - everything else → 2
    #[must_use]
    pub fn kernel_size(self) -> u32 {
        match self {
            Self::Nearest => 1,
            Self::Bilinear => 2,
            Self::Bicubic => 4,
            Self::Lanczos => 6,
            _ => 2,
        }
    }
}

// ---------------------------------------------------------------------------
// OverviewLevel
// ---------------------------------------------------------------------------

/// A single reduced-resolution level of a raster.
#[derive(Debug, Clone)]
pub struct OverviewLevel {
    /// Downsample factor relative to the full-resolution source.
    pub factor: u32,
    /// Width of this overview in pixels.
    pub width: u32,
    /// Height of this overview in pixels.
    pub height: u32,
    /// Tile width used when tiling this overview.
    pub tile_width: u32,
    /// Tile height used when tiling this overview.
    pub tile_height: u32,
    /// Row-major pixel data normalised to `f64`.
    pub data: Vec<f64>,
}

impl OverviewLevel {
    /// Constructs a new empty `OverviewLevel`.
    ///
    /// `source_width / factor` and `source_height / factor` are clamped to at
    /// least 1 pixel.
    #[must_use]
    pub fn new(factor: u32, source_width: u32, source_height: u32, tile_size: u32) -> Self {
        let factor = factor.max(1);
        let width = (source_width / factor).max(1);
        let height = (source_height / factor).max(1);
        let tile = tile_size.max(1);
        Self {
            factor,
            width,
            height,
            tile_width: tile,
            tile_height: tile,
            data: Vec::new(),
        }
    }

    /// Returns the pixel value at `(x, y)` (column-major / x-first), or `None`
    /// if out of bounds.
    #[must_use]
    pub fn pixel_at(&self, x: u32, y: u32) -> Option<f64> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.data
            .get((y as usize) * (self.width as usize) + (x as usize))
            .copied()
    }

    /// Returns the number of tiles in `(x, y)` directions.
    #[must_use]
    pub fn tile_count(&self) -> (u32, u32) {
        let tx = self.width.div_ceil(self.tile_width);
        let ty = self.height.div_ceil(self.tile_height);
        (tx, ty)
    }
}

// ---------------------------------------------------------------------------
// OverviewBuilder
// ---------------------------------------------------------------------------

/// Builds multi-level overview pyramids from a source raster band.
#[derive(Debug, Clone)]
pub struct OverviewBuilder {
    /// Resampling method.
    pub method: ResampleMethod,
    /// Downsample factors to generate (e.g. `[2, 4, 8, 16, 32]`).
    pub factors: Vec<u32>,
    /// Tile size for each overview level (default 256).
    pub tile_size: u32,
    /// NoData sentinel value (excluded from statistics and averaging).
    pub nodata: Option<f64>,
}

impl Default for OverviewBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl OverviewBuilder {
    /// Creates a new builder with default settings (Average method, tile 256,
    /// no nodata).
    #[must_use]
    pub fn new() -> Self {
        Self {
            method: ResampleMethod::Average,
            factors: vec![2, 4, 8, 16, 32],
            tile_size: 256,
            nodata: None,
        }
    }

    /// Sets the resampling method.
    #[must_use]
    pub fn with_method(mut self, method: ResampleMethod) -> Self {
        self.method = method;
        self
    }

    /// Overrides the list of downsample factors.
    #[must_use]
    pub fn with_factors(mut self, factors: Vec<u32>) -> Self {
        self.factors = factors;
        self
    }

    /// Sets the tile size for all generated overview levels.
    #[must_use]
    pub fn with_tile_size(mut self, size: u32) -> Self {
        self.tile_size = size;
        self
    }

    /// Sets the nodata sentinel value.
    #[must_use]
    pub fn with_nodata(mut self, nodata: f64) -> Self {
        self.nodata = Some(nodata);
        self
    }

    /// Builds all overview levels from a single-band source raster.
    ///
    /// `source` must contain `width * height` pixels in row-major order.
    ///
    /// # Errors
    /// Returns an empty `Vec` when any factor is < 2; callers should validate
    /// via [`OverviewBuilder::standard_factors`] when needed.
    #[must_use]
    pub fn build(&self, source: &[f64], width: u32, height: u32) -> Vec<OverviewLevel> {
        if width == 0 || height == 0 || source.is_empty() {
            return Vec::new();
        }
        let mut levels = Vec::with_capacity(self.factors.len());
        for &factor in &self.factors {
            if factor < 2 {
                continue;
            }
            let dst_w = (width / factor).max(1);
            let dst_h = (height / factor).max(1);
            let data = match self.method {
                ResampleMethod::Nearest => resample_nearest(source, width, height, dst_w, dst_h),
                ResampleMethod::Bilinear => resample_bilinear(source, width, height, dst_w, dst_h),
                ResampleMethod::Bicubic => resample_bicubic(source, width, height, dst_w, dst_h),
                ResampleMethod::Average => {
                    resample_average(source, width, height, dst_w, dst_h, self.nodata)
                }
                ResampleMethod::Mode => resample_mode(source, width, height, dst_w, dst_h),
                ResampleMethod::Lanczos => resample_lanczos(source, width, height, dst_w, dst_h),
                ResampleMethod::Gauss => {
                    resample_gauss(source, width, height, dst_w, dst_h, self.nodata)
                }
                ResampleMethod::Min => {
                    resample_min(source, width, height, dst_w, dst_h, self.nodata)
                }
                ResampleMethod::Max => {
                    resample_max(source, width, height, dst_w, dst_h, self.nodata)
                }
                ResampleMethod::Median => {
                    resample_median(source, width, height, dst_w, dst_h, self.nodata)
                }
            };
            let mut level = OverviewLevel::new(factor, width, height, self.tile_size);
            level.data = data;
            levels.push(level);
        }
        levels
    }

    /// Returns the standard GDAL-style overview factor list for a raster of
    /// the given dimensions: `[2, 4, 8, 16, …]` until the overview would be
    /// smaller than 256 pixels in both dimensions.
    #[must_use]
    pub fn standard_factors(width: u32, height: u32) -> Vec<u32> {
        let mut factors = Vec::new();
        let mut f = 2u32;
        loop {
            let ow = (width / f).max(1);
            let oh = (height / f).max(1);
            factors.push(f);
            if ow <= 1 && oh <= 1 {
                break;
            }
            // Stop when both dimensions are below 256
            if ow < 256 && oh < 256 {
                break;
            }
            f = f.saturating_mul(2);
            if f == 0 {
                break;
            }
        }
        factors
    }
}

// ---------------------------------------------------------------------------
// Resampling kernels — public API
// ---------------------------------------------------------------------------

/// Nearest-neighbour resampling.
///
/// Each output pixel takes the value of the nearest source pixel.
#[must_use]
pub fn resample_nearest(src: &[f64], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<f64> {
    let mut dst = vec![0.0_f64; (dst_w * dst_h) as usize];
    let scale_x = src_w as f64 / dst_w as f64;
    let scale_y = src_h as f64 / dst_h as f64;
    for py in 0..dst_h {
        for px in 0..dst_w {
            let sx = ((px as f64 + 0.5) * scale_x) as u32;
            let sy = ((py as f64 + 0.5) * scale_y) as u32;
            let sx = sx.min(src_w - 1);
            let sy = sy.min(src_h - 1);
            dst[(py * dst_w + px) as usize] = src[(sy * src_w + sx) as usize];
        }
    }
    dst
}

/// Bilinear resampling.
///
/// Each output pixel is computed as a weighted average of the four nearest
/// source pixels (2×2 neighbourhood).
#[must_use]
pub fn resample_bilinear(src: &[f64], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<f64> {
    let mut dst = vec![0.0_f64; (dst_w * dst_h) as usize];
    let scale_x = src_w as f64 / dst_w as f64;
    let scale_y = src_h as f64 / dst_h as f64;

    for py in 0..dst_h {
        for px in 0..dst_w {
            let sx = (px as f64 + 0.5) * scale_x - 0.5;
            let sy = (py as f64 + 0.5) * scale_y - 0.5;

            let x0 = sx.floor() as i64;
            let y0 = sy.floor() as i64;
            let tx = sx - x0 as f64;
            let ty = sy - y0 as f64;

            let get = |col: i64, row: i64| -> f64 {
                let col = col.clamp(0, (src_w - 1) as i64) as u32;
                let row = row.clamp(0, (src_h - 1) as i64) as u32;
                src[(row * src_w + col) as usize]
            };

            let v00 = get(x0, y0);
            let v10 = get(x0 + 1, y0);
            let v01 = get(x0, y0 + 1);
            let v11 = get(x0 + 1, y0 + 1);

            let val = v00 * (1.0 - tx) * (1.0 - ty)
                + v10 * tx * (1.0 - ty)
                + v01 * (1.0 - tx) * ty
                + v11 * tx * ty;

            dst[(py * dst_w + px) as usize] = val;
        }
    }
    dst
}

/// Bicubic resampling using the Keys cubic convolution kernel.
#[must_use]
pub fn resample_bicubic(src: &[f64], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<f64> {
    let mut dst = vec![0.0_f64; (dst_w * dst_h) as usize];
    let scale_x = src_w as f64 / dst_w as f64;
    let scale_y = src_h as f64 / dst_h as f64;

    let cubic_weight = |t: f64| -> f64 {
        let a = -0.5_f64; // Keys a=-0.5
        let t = t.abs();
        if t < 1.0 {
            (a + 2.0) * t * t * t - (a + 3.0) * t * t + 1.0
        } else if t < 2.0 {
            a * t * t * t - 5.0 * a * t * t + 8.0 * a * t - 4.0 * a
        } else {
            0.0
        }
    };

    let get = |col: i64, row: i64| -> f64 {
        let col = col.clamp(0, (src_w - 1) as i64) as u32;
        let row = row.clamp(0, (src_h - 1) as i64) as u32;
        src[(row * src_w + col) as usize]
    };

    for py in 0..dst_h {
        for px in 0..dst_w {
            let sx = (px as f64 + 0.5) * scale_x - 0.5;
            let sy = (py as f64 + 0.5) * scale_y - 0.5;
            let x0 = sx.floor() as i64;
            let y0 = sy.floor() as i64;
            let tx = sx - x0 as f64;
            let ty = sy - y0 as f64;

            let mut val = 0.0_f64;
            let mut wsum = 0.0_f64;
            for m in -1..=2_i64 {
                let wy = cubic_weight(ty - m as f64);
                for n in -1..=2_i64 {
                    let wx = cubic_weight(tx - n as f64);
                    let w = wx * wy;
                    val += w * get(x0 + n, y0 + m);
                    wsum += w;
                }
            }
            dst[(py * dst_w + px) as usize] = if wsum.abs() > 1e-15 { val / wsum } else { 0.0 };
        }
    }
    dst
}

/// Average (arithmetic mean) resampling.
///
/// Computes the mean of all source pixels mapped to each output pixel.
/// Pixels matching `nodata` are excluded from the average.
#[must_use]
pub fn resample_average(
    src: &[f64],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    nodata: Option<f64>,
) -> Vec<f64> {
    let mut dst = vec![nodata.unwrap_or(0.0); (dst_w * dst_h) as usize];
    let scale_x = src_w as f64 / dst_w as f64;
    let scale_y = src_h as f64 / dst_h as f64;

    for py in 0..dst_h {
        for px in 0..dst_w {
            let x_start = (px as f64 * scale_x) as u32;
            let y_start = (py as f64 * scale_y) as u32;
            let x_end = (((px + 1) as f64) * scale_x).ceil() as u32;
            let y_end = (((py + 1) as f64) * scale_y).ceil() as u32;
            let x_end = x_end.min(src_w);
            let y_end = y_end.min(src_h);

            let mut sum = 0.0_f64;
            let mut count = 0u64;
            for sy in y_start..y_end {
                for sx in x_start..x_end {
                    let v = src[(sy * src_w + sx) as usize];
                    if let Some(nd) = nodata {
                        if (v - nd).abs() < 1e-10 {
                            continue;
                        }
                    }
                    sum += v;
                    count += 1;
                }
            }
            if count > 0 {
                dst[(py * dst_w + px) as usize] = sum / count as f64;
            }
        }
    }
    dst
}

/// Mode resampling — most common value in the source window.
///
/// Values are rounded to 3 decimal places for bucketing.
#[must_use]
pub fn resample_mode(src: &[f64], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<f64> {
    let mut dst = vec![0.0_f64; (dst_w * dst_h) as usize];
    let scale_x = src_w as f64 / dst_w as f64;
    let scale_y = src_h as f64 / dst_h as f64;

    for py in 0..dst_h {
        for px in 0..dst_w {
            let x_start = (px as f64 * scale_x) as u32;
            let y_start = (py as f64 * scale_y) as u32;
            let x_end = (((px + 1) as f64) * scale_x).ceil() as u32;
            let y_end = (((py + 1) as f64) * scale_y).ceil() as u32;
            let x_end = x_end.min(src_w);
            let y_end = y_end.min(src_h);

            // Bucket by rounding to 3 dp → multiply by 1000, cast to i64
            let mut counts: HashMap<i64, u64> = HashMap::new();
            let mut first = 0.0_f64;
            let mut any = false;
            for sy in y_start..y_end {
                for sx in x_start..x_end {
                    let v = src[(sy * src_w + sx) as usize];
                    let key = (v * 1000.0).round() as i64;
                    *counts.entry(key).or_insert(0) += 1;
                    if !any {
                        first = v;
                        any = true;
                    }
                }
            }
            let mode_key = counts
                .iter()
                .max_by_key(|&(_, &c)| c)
                .map(|(&k, _)| k)
                .unwrap_or(0);
            let result = if any { mode_key as f64 / 1000.0 } else { first };
            dst[(py * dst_w + px) as usize] = result;
        }
    }
    dst
}

// ---------------------------------------------------------------------------
// Lanczos kernel
// ---------------------------------------------------------------------------

/// Lanczos windowed-sinc kernel weight (a = 3).
#[inline]
fn lanczos_kernel(x: f64, a: f64) -> f64 {
    if x.abs() < 1e-10 {
        return 1.0;
    }
    if x.abs() >= a {
        return 0.0;
    }
    let pi_x = std::f64::consts::PI * x;
    let pi_x_a = std::f64::consts::PI * x / a;
    (pi_x.sin() / pi_x) * (pi_x_a.sin() / pi_x_a)
}

/// Lanczos resampling (a = 3).
#[must_use]
pub fn resample_lanczos(src: &[f64], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<f64> {
    const A: f64 = 3.0;
    let mut dst = vec![0.0_f64; (dst_w * dst_h) as usize];
    let scale_x = src_w as f64 / dst_w as f64;
    let scale_y = src_h as f64 / dst_h as f64;

    let get = |col: i64, row: i64| -> f64 {
        let col = col.clamp(0, (src_w - 1) as i64) as u32;
        let row = row.clamp(0, (src_h - 1) as i64) as u32;
        src[(row * src_w + col) as usize]
    };

    for py in 0..dst_h {
        for px in 0..dst_w {
            let sx = (px as f64 + 0.5) * scale_x - 0.5;
            let sy = (py as f64 + 0.5) * scale_y - 0.5;
            let x0 = sx.floor() as i64;
            let y0 = sy.floor() as i64;

            let mut val = 0.0_f64;
            let mut wsum = 0.0_f64;
            let radius = A.ceil() as i64;
            for m in -radius + 1..=radius {
                let wy = lanczos_kernel(sy - (y0 + m) as f64, A);
                if wy.abs() < 1e-15 {
                    continue;
                }
                for n in -radius + 1..=radius {
                    let wx = lanczos_kernel(sx - (x0 + n) as f64, A);
                    let w = wx * wy;
                    if w.abs() < 1e-15 {
                        continue;
                    }
                    val += w * get(x0 + n, y0 + m);
                    wsum += w;
                }
            }
            dst[(py * dst_w + px) as usize] = if wsum.abs() > 1e-15 { val / wsum } else { 0.0 };
        }
    }
    dst
}

// ---------------------------------------------------------------------------
// Gaussian resample
// ---------------------------------------------------------------------------

/// Gaussian-blur-then-subsample resampling.
#[must_use]
pub fn resample_gauss(
    src: &[f64],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    nodata: Option<f64>,
) -> Vec<f64> {
    // Simple Gaussian with sigma = scale_factor / 2
    let scale_x = src_w as f64 / dst_w as f64;
    let scale_y = src_h as f64 / dst_h as f64;
    let sigma_x = scale_x / 2.0;
    let sigma_y = scale_y / 2.0;
    let radius_x = (3.0 * sigma_x).ceil() as i64;
    let radius_y = (3.0 * sigma_y).ceil() as i64;

    let mut dst = vec![nodata.unwrap_or(0.0); (dst_w * dst_h) as usize];

    let get = |col: i64, row: i64| -> Option<f64> {
        if col < 0 || row < 0 || col >= src_w as i64 || row >= src_h as i64 {
            return None;
        }
        let v = src[(row as u32 * src_w + col as u32) as usize];
        if let Some(nd) = nodata {
            if (v - nd).abs() < 1e-10 {
                return None;
            }
        }
        Some(v)
    };

    for py in 0..dst_h {
        for px in 0..dst_w {
            let cx = (px as f64 + 0.5) * scale_x - 0.5;
            let cy = (py as f64 + 0.5) * scale_y - 0.5;
            let x0 = cx.round() as i64;
            let y0 = cy.round() as i64;

            let mut sum = 0.0_f64;
            let mut wsum = 0.0_f64;
            for m in -radius_y..=radius_y {
                let dy = (cy - (y0 + m) as f64) / sigma_y;
                let wy = (-0.5 * dy * dy).exp();
                for n in -radius_x..=radius_x {
                    let dx = (cx - (x0 + n) as f64) / sigma_x;
                    let wx = (-0.5 * dx * dx).exp();
                    let w = wx * wy;
                    if let Some(v) = get(x0 + n, y0 + m) {
                        sum += w * v;
                        wsum += w;
                    }
                }
            }
            if wsum > 1e-15 {
                dst[(py * dst_w + px) as usize] = sum / wsum;
            }
        }
    }
    dst
}

// ---------------------------------------------------------------------------
// Min / Max / Median
// ---------------------------------------------------------------------------

/// Minimum-value resampling.
#[must_use]
pub fn resample_min(
    src: &[f64],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    nodata: Option<f64>,
) -> Vec<f64> {
    let mut dst = vec![nodata.unwrap_or(0.0); (dst_w * dst_h) as usize];
    let scale_x = src_w as f64 / dst_w as f64;
    let scale_y = src_h as f64 / dst_h as f64;

    for py in 0..dst_h {
        for px in 0..dst_w {
            let x_start = (px as f64 * scale_x) as u32;
            let y_start = (py as f64 * scale_y) as u32;
            let x_end = (((px + 1) as f64) * scale_x).ceil() as u32;
            let y_end = (((py + 1) as f64) * scale_y).ceil() as u32;
            let x_end = x_end.min(src_w);
            let y_end = y_end.min(src_h);

            let mut min = f64::INFINITY;
            for sy in y_start..y_end {
                for sx in x_start..x_end {
                    let v = src[(sy * src_w + sx) as usize];
                    if let Some(nd) = nodata {
                        if (v - nd).abs() < 1e-10 {
                            continue;
                        }
                    }
                    if v < min {
                        min = v;
                    }
                }
            }
            if min.is_finite() {
                dst[(py * dst_w + px) as usize] = min;
            }
        }
    }
    dst
}

/// Maximum-value resampling.
#[must_use]
pub fn resample_max(
    src: &[f64],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    nodata: Option<f64>,
) -> Vec<f64> {
    let mut dst = vec![nodata.unwrap_or(0.0); (dst_w * dst_h) as usize];
    let scale_x = src_w as f64 / dst_w as f64;
    let scale_y = src_h as f64 / dst_h as f64;

    for py in 0..dst_h {
        for px in 0..dst_w {
            let x_start = (px as f64 * scale_x) as u32;
            let y_start = (py as f64 * scale_y) as u32;
            let x_end = (((px + 1) as f64) * scale_x).ceil() as u32;
            let y_end = (((py + 1) as f64) * scale_y).ceil() as u32;
            let x_end = x_end.min(src_w);
            let y_end = y_end.min(src_h);

            let mut max = f64::NEG_INFINITY;
            for sy in y_start..y_end {
                for sx in x_start..x_end {
                    let v = src[(sy * src_w + sx) as usize];
                    if let Some(nd) = nodata {
                        if (v - nd).abs() < 1e-10 {
                            continue;
                        }
                    }
                    if v > max {
                        max = v;
                    }
                }
            }
            if max.is_finite() {
                dst[(py * dst_w + px) as usize] = max;
            }
        }
    }
    dst
}

/// Median-value resampling.
#[must_use]
pub fn resample_median(
    src: &[f64],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    nodata: Option<f64>,
) -> Vec<f64> {
    let mut dst = vec![nodata.unwrap_or(0.0); (dst_w * dst_h) as usize];
    let scale_x = src_w as f64 / dst_w as f64;
    let scale_y = src_h as f64 / dst_h as f64;

    for py in 0..dst_h {
        for px in 0..dst_w {
            let x_start = (px as f64 * scale_x) as u32;
            let y_start = (py as f64 * scale_y) as u32;
            let x_end = (((px + 1) as f64) * scale_x).ceil() as u32;
            let y_end = (((py + 1) as f64) * scale_y).ceil() as u32;
            let x_end = x_end.min(src_w);
            let y_end = y_end.min(src_h);

            let mut vals: Vec<f64> = Vec::new();
            for sy in y_start..y_end {
                for sx in x_start..x_end {
                    let v = src[(sy * src_w + sx) as usize];
                    if let Some(nd) = nodata {
                        if (v - nd).abs() < 1e-10 {
                            continue;
                        }
                    }
                    vals.push(v);
                }
            }
            if !vals.is_empty() {
                vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let mid = vals.len() / 2;
                let median = if vals.len() % 2 == 0 {
                    (vals[mid - 1] + vals[mid]) / 2.0
                } else {
                    vals[mid]
                };
                dst[(py * dst_w + px) as usize] = median;
            }
        }
    }
    dst
}

// ---------------------------------------------------------------------------
// BandHistogram
// ---------------------------------------------------------------------------

/// A histogram of raster band values.
#[derive(Debug, Clone)]
pub struct BandHistogram {
    /// Frequency counts per bucket.
    pub buckets: Vec<u64>,
    /// Minimum value of the lowest bucket.
    pub bucket_min: f64,
    /// Maximum value of the highest bucket (exclusive upper bound).
    pub bucket_max: f64,
    /// Width of each bucket in value units.
    pub bucket_size: f64,
}

impl BandHistogram {
    /// Builds a histogram from `data`.
    ///
    /// Returns `None` when `data` is empty or contains only nodata values.
    #[must_use]
    pub fn compute(data: &[f64], bucket_count: u32, nodata: Option<f64>) -> Option<Self> {
        if data.is_empty() || bucket_count == 0 {
            return None;
        }
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        for &v in data {
            if let Some(nd) = nodata {
                if (v - nd).abs() < 1e-10 {
                    continue;
                }
            }
            if v < min {
                min = v;
            }
            if v > max {
                max = v;
            }
        }
        if !min.is_finite() || !max.is_finite() {
            return None;
        }
        // Handle degenerate case where all values are identical
        let range = if (max - min).abs() < 1e-15 {
            1.0
        } else {
            max - min
        };
        let bucket_size = range / bucket_count as f64;
        let mut buckets = vec![0u64; bucket_count as usize];

        for &v in data {
            if let Some(nd) = nodata {
                if (v - nd).abs() < 1e-10 {
                    continue;
                }
            }
            let idx = ((v - min) / bucket_size) as usize;
            let idx = idx.min(bucket_count as usize - 1);
            buckets[idx] += 1;
        }

        Some(Self {
            buckets,
            bucket_min: min,
            bucket_max: max + bucket_size,
            bucket_size,
        })
    }

    /// Returns the bucket index for a value, or `None` if outside [min, max).
    #[must_use]
    pub fn bucket_for_value(&self, v: f64) -> Option<usize> {
        if v < self.bucket_min || v >= self.bucket_max {
            return None;
        }
        let idx = ((v - self.bucket_min) / self.bucket_size) as usize;
        Some(idx.min(self.buckets.len().saturating_sub(1)))
    }

    /// Returns the value at the given percentile (0..=100).
    ///
    /// Uses linear interpolation within the mode bucket.
    #[must_use]
    pub fn value_at_percentile(&self, percentile: f64) -> f64 {
        let percentile = percentile.clamp(0.0, 100.0);
        let total: u64 = self.buckets.iter().sum();
        if total == 0 {
            return self.bucket_min;
        }
        let target = (percentile / 100.0 * total as f64).round() as u64;
        let mut cumulative = 0u64;
        for (i, &count) in self.buckets.iter().enumerate() {
            cumulative += count;
            if cumulative >= target || i == self.buckets.len() - 1 {
                // Linearly interpolate within the bucket
                let bucket_start = self.bucket_min + i as f64 * self.bucket_size;
                let bucket_end = bucket_start + self.bucket_size;
                if count == 0 {
                    return bucket_start;
                }
                let before = cumulative - count;
                let frac = if target > before {
                    (target - before) as f64 / count as f64
                } else {
                    0.0
                };
                return bucket_start + frac * (bucket_end - bucket_start);
            }
        }
        self.bucket_max - self.bucket_size
    }

    /// Returns the centre value of the most-filled bucket.
    #[must_use]
    pub fn mode_value(&self) -> f64 {
        let idx = self
            .buckets
            .iter()
            .enumerate()
            .max_by_key(|&(_, &c)| c)
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.bucket_min + idx as f64 * self.bucket_size + self.bucket_size / 2.0
    }
}

// ---------------------------------------------------------------------------
// RasterStatistics
// ---------------------------------------------------------------------------

/// Descriptive statistics for a raster band.
#[derive(Debug, Clone)]
pub struct RasterStatistics {
    /// Minimum valid value.
    pub min: f64,
    /// Maximum valid value.
    pub max: f64,
    /// Arithmetic mean.
    pub mean: f64,
    /// Population standard deviation.
    pub std_dev: f64,
    /// Number of valid (non-nodata) pixels.
    pub valid_count: u64,
    /// Number of nodata pixels.
    pub nodata_count: u64,
    /// 25th-percentile value (approximate, from 256-bucket histogram).
    pub percentile_25: f64,
    /// 50th-percentile value (approximate).
    pub percentile_50: f64,
    /// 75th-percentile value (approximate).
    pub percentile_75: f64,
}

impl RasterStatistics {
    /// Computes statistics over the entire `data` slice.
    ///
    /// Returns `None` when `data` is empty or every pixel is nodata.
    #[must_use]
    pub fn compute(data: &[f64], nodata: Option<f64>) -> Option<Self> {
        Self::compute_inner(data, nodata, 1)
    }

    /// Computes approximate statistics by sampling every `sample_stride` pixels.
    ///
    /// Returns `None` when no valid samples exist.
    #[must_use]
    pub fn compute_approximate(
        data: &[f64],
        nodata: Option<f64>,
        sample_stride: usize,
    ) -> Option<Self> {
        let stride = sample_stride.max(1);
        Self::compute_inner(data, nodata, stride)
    }

    fn compute_inner(data: &[f64], nodata: Option<f64>, stride: usize) -> Option<Self> {
        // --- Pass 1: min, max, count ---
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        let mut valid_count = 0u64;
        let mut nodata_count = 0u64;

        for (i, &v) in data.iter().enumerate() {
            if i % stride != 0 {
                continue;
            }
            if let Some(nd) = nodata {
                if (v - nd).abs() < 1e-10 {
                    nodata_count += 1;
                    continue;
                }
            }
            if v < min {
                min = v;
            }
            if v > max {
                max = v;
            }
            valid_count += 1;
        }

        if valid_count == 0 || !min.is_finite() {
            return None;
        }

        // --- Pass 2: mean and variance (Welford online algorithm) ---
        let mut mean = 0.0_f64;
        let mut m2 = 0.0_f64;
        let mut n = 0u64;

        for (i, &v) in data.iter().enumerate() {
            if i % stride != 0 {
                continue;
            }
            if let Some(nd) = nodata {
                if (v - nd).abs() < 1e-10 {
                    continue;
                }
            }
            n += 1;
            let delta = v - mean;
            mean += delta / n as f64;
            let delta2 = v - mean;
            m2 += delta * delta2;
        }

        let std_dev = if n > 1 { (m2 / n as f64).sqrt() } else { 0.0 };

        // --- Histogram for percentiles ---
        const BUCKET_COUNT: u32 = 256;
        let hist = BandHistogram::compute(data, BUCKET_COUNT, nodata)?;
        let percentile_25 = hist.value_at_percentile(25.0);
        let percentile_50 = hist.value_at_percentile(50.0);
        let percentile_75 = hist.value_at_percentile(75.0);

        Some(Self {
            min,
            max,
            mean,
            std_dev,
            valid_count,
            nodata_count,
            percentile_25,
            percentile_50,
            percentile_75,
        })
    }
}

// ---------------------------------------------------------------------------
// Unit tests (basic smoke tests; comprehensive tests live in tests/)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_size_all_variants() {
        assert_eq!(ResampleMethod::Nearest.kernel_size(), 1);
        assert_eq!(ResampleMethod::Bilinear.kernel_size(), 2);
        assert_eq!(ResampleMethod::Bicubic.kernel_size(), 4);
        assert_eq!(ResampleMethod::Lanczos.kernel_size(), 6);
        assert_eq!(ResampleMethod::Average.kernel_size(), 2);
        assert_eq!(ResampleMethod::Mode.kernel_size(), 2);
        assert_eq!(ResampleMethod::Gauss.kernel_size(), 2);
        assert_eq!(ResampleMethod::Min.kernel_size(), 2);
        assert_eq!(ResampleMethod::Max.kernel_size(), 2);
        assert_eq!(ResampleMethod::Median.kernel_size(), 2);
    }

    #[test]
    fn is_exact() {
        assert!(ResampleMethod::Nearest.is_exact());
        assert!(ResampleMethod::Mode.is_exact());
        assert!(!ResampleMethod::Bilinear.is_exact());
        assert!(!ResampleMethod::Average.is_exact());
    }
}
