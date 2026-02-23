//! Focal (neighborhood) statistics operations
//!
//! This module provides focal/neighborhood operations that compute statistics
//! over moving windows. These are essential for many spatial analysis tasks.
//!
//! # Supported Operations
//!
//! - **Focal Mean**: Average value in neighborhood
//! - **Focal Median**: Median value in neighborhood
//! - **Focal Range**: Max - Min in neighborhood
//! - **Focal Variety**: Count of unique values
//! - **Focal Min/Max**: Minimum/Maximum in neighborhood
//! - **Focal Sum**: Sum of values in neighborhood
//! - **Focal Standard Deviation**: Standard deviation in neighborhood
//!
//! # Window Shapes
//!
//! - **Rectangular**: Standard rectangular window
//! - **Circular**: Circular window (pixels within radius)
//! - **Custom**: User-defined kernel mask
//!
//! # Example
//!
//! ```ignore
//! use oxigdal_algorithms::raster::focal::{focal_mean, WindowShape, BoundaryMode};
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//! # use oxigdal_algorithms::error::Result;
//!
//! # fn main() -> Result<()> {
//! let src = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
//! let window = WindowShape::rectangular(3, 3)?;
//! let boundary = BoundaryMode::Reflect;
//!
//! let result = focal_mean(&src, &window, &boundary)?;
//! # Ok(())
//! # }
//! ```

use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

/// Window shape for focal operations
#[derive(Debug, Clone, PartialEq)]
pub enum WindowShape {
    /// Rectangular window with given width and height (must be odd)
    Rectangular { width: usize, height: usize },

    /// Circular window with given radius
    Circular { radius: f64 },

    /// Custom window defined by a binary mask (1 = include, 0 = exclude)
    Custom {
        mask: Vec<bool>,
        width: usize,
        height: usize,
    },
}

impl WindowShape {
    /// Creates a rectangular window
    ///
    /// # Arguments
    ///
    /// * `width` - Window width (must be odd)
    /// * `height` - Window height (must be odd)
    ///
    /// # Errors
    ///
    /// Returns an error if width or height is even or zero
    pub fn rectangular(width: usize, height: usize) -> Result<Self> {
        use oxigdal_core::OxiGdalError;

        if width == 0 || height == 0 {
            return Err(OxiGdalError::invalid_parameter_builder(
                "window_size",
                format!(
                    "window dimensions must be greater than zero, got {}x{}",
                    width, height
                ),
            )
            .with_parameter("width", width.to_string())
            .with_parameter("height", height.to_string())
            .with_parameter("min", "1")
            .with_operation("focal_operation")
            .with_suggestion("Use positive window dimensions. Common values: 3, 5, 7, 9, 11")
            .build()
            .into());
        }
        if width % 2 == 0 || height % 2 == 0 {
            let suggested_width = if width % 2 == 0 { width + 1 } else { width };
            let suggested_height = if height % 2 == 0 { height + 1 } else { height };
            return Err(OxiGdalError::invalid_parameter_builder(
                "window_size",
                format!("window dimensions must be odd, got {}x{}", width, height),
            )
            .with_parameter("width", width.to_string())
            .with_parameter("height", height.to_string())
            .with_operation("focal_operation")
            .with_suggestion(format!(
                "Use odd window dimensions for symmetric neighborhood. Try {}x{} instead",
                suggested_width, suggested_height
            ))
            .build()
            .into());
        }
        Ok(Self::Rectangular { width, height })
    }

    /// Creates a circular window
    ///
    /// # Arguments
    ///
    /// * `radius` - Window radius (must be positive)
    ///
    /// # Errors
    ///
    /// Returns an error if radius is not positive
    pub fn circular(radius: f64) -> Result<Self> {
        use oxigdal_core::OxiGdalError;

        if radius <= 0.0 {
            return Err(OxiGdalError::invalid_parameter_builder(
                "radius",
                format!("window radius must be positive, got {}", radius),
            )
            .with_parameter("value", radius.to_string())
            .with_parameter("min", "0.0")
            .with_operation("focal_operation")
            .with_suggestion(
                "Use positive radius value. Common values: 1.0, 1.5, 2.0, 3.0 (in pixels)",
            )
            .build()
            .into());
        }
        Ok(Self::Circular { radius })
    }

    /// Creates a custom window from a binary mask
    ///
    /// # Arguments
    ///
    /// * `mask` - Binary mask (true = include pixel)
    /// * `width` - Mask width
    /// * `height` - Mask height
    ///
    /// # Errors
    ///
    /// Returns an error if dimensions don't match mask size or are even
    pub fn custom(mask: Vec<bool>, width: usize, height: usize) -> Result<Self> {
        use oxigdal_core::OxiGdalError;

        if mask.len() != width * height {
            let expected = width * height;
            return Err(OxiGdalError::invalid_parameter_builder(
                "mask",
                format!(
                    "mask size must match width * height, got {} but expected {}",
                    mask.len(),
                    expected
                ),
            )
            .with_parameter("mask_length", mask.len().to_string())
            .with_parameter("width", width.to_string())
            .with_parameter("height", height.to_string())
            .with_parameter("expected_length", expected.to_string())
            .with_operation("focal_operation")
            .with_suggestion(format!(
                "Provide a mask with exactly {} elements ({}x{})",
                expected, width, height
            ))
            .build()
            .into());
        }
        if width % 2 == 0 || height % 2 == 0 {
            let suggested_width = if width % 2 == 0 { width + 1 } else { width };
            let suggested_height = if height % 2 == 0 { height + 1 } else { height };
            return Err(OxiGdalError::invalid_parameter_builder(
                "window_size",
                format!("window dimensions must be odd, got {}x{}", width, height),
            )
            .with_parameter("width", width.to_string())
            .with_parameter("height", height.to_string())
            .with_operation("focal_operation")
            .with_suggestion(format!(
                "Use odd window dimensions for symmetric neighborhood. Try {}x{} instead",
                suggested_width, suggested_height
            ))
            .build()
            .into());
        }
        Ok(Self::Custom {
            mask,
            width,
            height,
        })
    }

    /// Gets the window dimensions
    #[must_use]
    pub fn dimensions(&self) -> (usize, usize) {
        match self {
            Self::Rectangular { width, height } => (*width, *height),
            Self::Circular { radius } => {
                let size = (radius.ceil() * 2.0 + 1.0) as usize;
                (size, size)
            }
            Self::Custom { width, height, .. } => (*width, *height),
        }
    }

    /// Checks if a pixel at given offset is included in the window
    #[must_use]
    pub fn includes(&self, dx: i64, dy: i64) -> bool {
        match self {
            Self::Rectangular { width, height } => {
                let hw = (*width / 2) as i64;
                let hh = (*height / 2) as i64;
                dx.abs() <= hw && dy.abs() <= hh
            }
            Self::Circular { radius } => {
                let dist = ((dx * dx + dy * dy) as f64).sqrt();
                dist <= *radius
            }
            Self::Custom {
                mask,
                width,
                height,
            } => {
                let hw = (*width / 2) as i64;
                let hh = (*height / 2) as i64;
                if dx.abs() > hw || dy.abs() > hh {
                    return false;
                }
                let x = (dx + hw) as usize;
                let y = (dy + hh) as usize;
                mask[y * width + x]
            }
        }
    }

    /// Returns all offsets included in the window
    #[must_use]
    pub fn offsets(&self) -> Vec<(i64, i64)> {
        let (width, height) = self.dimensions();
        let hw = (width / 2) as i64;
        let hh = (height / 2) as i64;

        let mut result = Vec::new();
        for dy in -hh..=hh {
            for dx in -hw..=hw {
                if self.includes(dx, dy) {
                    result.push((dx, dy));
                }
            }
        }
        result
    }

    /// Returns the number of cells in the window
    #[must_use]
    pub fn cell_count(&self) -> usize {
        self.offsets().len()
    }
}

/// Boundary handling mode for focal operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryMode {
    /// Ignore pixels outside boundary (smaller neighborhood at edges)
    Ignore,

    /// Use a constant value for pixels outside boundary
    Constant(i64),

    /// Reflect values at the boundary
    Reflect,

    /// Wrap around to opposite edge
    Wrap,

    /// Extend edge values
    Edge,
}

/// Focal operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocalOperation {
    /// Mean (average) value
    Mean,

    /// Median value
    Median,

    /// Range (max - min)
    Range,

    /// Variety (count of unique values)
    Variety,

    /// Minimum value
    Min,

    /// Maximum value
    Max,

    /// Sum of values
    Sum,

    /// Standard deviation
    StdDev,

    /// Majority (most common value)
    Majority,
}

/// Computes focal mean over a raster
///
/// # Arguments
///
/// * `src` - Source raster buffer
/// * `window` - Window shape
/// * `boundary` - Boundary handling mode
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn focal_mean(
    src: &RasterBuffer,
    window: &WindowShape,
    boundary: &BoundaryMode,
) -> Result<RasterBuffer> {
    focal_operation(src, window, boundary, FocalOperation::Mean)
}

/// Computes focal median over a raster
///
/// # Arguments
///
/// * `src` - Source raster buffer
/// * `window` - Window shape
/// * `boundary` - Boundary handling mode
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn focal_median(
    src: &RasterBuffer,
    window: &WindowShape,
    boundary: &BoundaryMode,
) -> Result<RasterBuffer> {
    focal_operation(src, window, boundary, FocalOperation::Median)
}

/// Computes focal range (max - min) over a raster
///
/// # Arguments
///
/// * `src` - Source raster buffer
/// * `window` - Window shape
/// * `boundary` - Boundary handling mode
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn focal_range(
    src: &RasterBuffer,
    window: &WindowShape,
    boundary: &BoundaryMode,
) -> Result<RasterBuffer> {
    focal_operation(src, window, boundary, FocalOperation::Range)
}

/// Computes focal variety (unique value count) over a raster
///
/// # Arguments
///
/// * `src` - Source raster buffer
/// * `window` - Window shape
/// * `boundary` - Boundary handling mode
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn focal_variety(
    src: &RasterBuffer,
    window: &WindowShape,
    boundary: &BoundaryMode,
) -> Result<RasterBuffer> {
    focal_operation(src, window, boundary, FocalOperation::Variety)
}

/// Computes focal minimum over a raster
///
/// # Arguments
///
/// * `src` - Source raster buffer
/// * `window` - Window shape
/// * `boundary` - Boundary handling mode
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn focal_min(
    src: &RasterBuffer,
    window: &WindowShape,
    boundary: &BoundaryMode,
) -> Result<RasterBuffer> {
    focal_operation(src, window, boundary, FocalOperation::Min)
}

/// Computes focal maximum over a raster
///
/// # Arguments
///
/// * `src` - Source raster buffer
/// * `window` - Window shape
/// * `boundary` - Boundary handling mode
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn focal_max(
    src: &RasterBuffer,
    window: &WindowShape,
    boundary: &BoundaryMode,
) -> Result<RasterBuffer> {
    focal_operation(src, window, boundary, FocalOperation::Max)
}

/// Computes focal sum over a raster
///
/// # Arguments
///
/// * `src` - Source raster buffer
/// * `window` - Window shape
/// * `boundary` - Boundary handling mode
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn focal_sum(
    src: &RasterBuffer,
    window: &WindowShape,
    boundary: &BoundaryMode,
) -> Result<RasterBuffer> {
    focal_operation(src, window, boundary, FocalOperation::Sum)
}

/// Computes focal standard deviation over a raster
///
/// # Arguments
///
/// * `src` - Source raster buffer
/// * `window` - Window shape
/// * `boundary` - Boundary handling mode
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn focal_stddev(
    src: &RasterBuffer,
    window: &WindowShape,
    boundary: &BoundaryMode,
) -> Result<RasterBuffer> {
    focal_operation(src, window, boundary, FocalOperation::StdDev)
}

/// Computes focal majority (most common value) over a raster
///
/// # Arguments
///
/// * `src` - Source raster buffer
/// * `window` - Window shape
/// * `boundary` - Boundary handling mode
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn focal_majority(
    src: &RasterBuffer,
    window: &WindowShape,
    boundary: &BoundaryMode,
) -> Result<RasterBuffer> {
    focal_operation(src, window, boundary, FocalOperation::Majority)
}

/// Generic focal operation
fn focal_operation(
    src: &RasterBuffer,
    window: &WindowShape,
    boundary: &BoundaryMode,
    operation: FocalOperation,
) -> Result<RasterBuffer> {
    let width = src.width();
    let height = src.height();
    let mut dst = RasterBuffer::zeros(width, height, src.data_type());

    let offsets = window.offsets();

    #[cfg(feature = "parallel")]
    {
        let results: Result<Vec<_>> = (0..height)
            .into_par_iter()
            .map(|y| {
                let mut row_data = Vec::with_capacity(width as usize);
                for x in 0..width {
                    let value = compute_focal_value(src, x, y, &offsets, boundary, operation)?;
                    row_data.push(value);
                }
                Ok((y, row_data))
            })
            .collect();

        for (y, row_data) in results? {
            for (x, value) in row_data.into_iter().enumerate() {
                dst.set_pixel(x as u64, y, value)
                    .map_err(AlgorithmError::Core)?;
            }
        }
    }

    #[cfg(not(feature = "parallel"))]
    {
        for y in 0..height {
            for x in 0..width {
                let value = compute_focal_value(src, x, y, &offsets, boundary, operation)?;
                dst.set_pixel(x, y, value).map_err(AlgorithmError::Core)?;
            }
        }
    }

    Ok(dst)
}

/// Computes the focal value for a single pixel
fn compute_focal_value(
    src: &RasterBuffer,
    x: u64,
    y: u64,
    offsets: &[(i64, i64)],
    boundary: &BoundaryMode,
    operation: FocalOperation,
) -> Result<f64> {
    let width = src.width() as i64;
    let height = src.height() as i64;
    let x_i64 = x as i64;
    let y_i64 = y as i64;

    let mut values = Vec::with_capacity(offsets.len());

    for &(dx, dy) in offsets {
        let nx = x_i64 + dx;
        let ny = y_i64 + dy;

        let value = if nx >= 0 && nx < width && ny >= 0 && ny < height {
            src.get_pixel(nx as u64, ny as u64)
                .map_err(AlgorithmError::Core)?
        } else {
            match boundary {
                BoundaryMode::Ignore => continue,
                BoundaryMode::Constant(c) => *c as f64,
                BoundaryMode::Reflect => {
                    let rx = if nx < 0 {
                        -nx - 1
                    } else if nx >= width {
                        2 * width - nx - 1
                    } else {
                        nx
                    };
                    let ry = if ny < 0 {
                        -ny - 1
                    } else if ny >= height {
                        2 * height - ny - 1
                    } else {
                        ny
                    };
                    src.get_pixel(rx as u64, ry as u64)
                        .map_err(AlgorithmError::Core)?
                }
                BoundaryMode::Wrap => {
                    let wx = ((nx % width) + width) % width;
                    let wy = ((ny % height) + height) % height;
                    src.get_pixel(wx as u64, wy as u64)
                        .map_err(AlgorithmError::Core)?
                }
                BoundaryMode::Edge => {
                    let ex = nx.clamp(0, width - 1);
                    let ey = ny.clamp(0, height - 1);
                    src.get_pixel(ex as u64, ey as u64)
                        .map_err(AlgorithmError::Core)?
                }
            }
        };

        values.push(value);
    }

    if values.is_empty() {
        return Ok(0.0);
    }

    let result = match operation {
        FocalOperation::Mean => {
            let sum: f64 = values.iter().sum();
            sum / values.len() as f64
        }
        FocalOperation::Median => {
            values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
            let mid = values.len() / 2;
            if values.len() % 2 == 0 {
                (values[mid - 1] + values[mid]) / 2.0
            } else {
                values[mid]
            }
        }
        FocalOperation::Range => {
            // Compute min and max in a single pass using fold (panic-free)
            // This is safe because values is guaranteed non-empty by check at line 590
            let (min, max) = values.iter().copied().fold(
                (f64::INFINITY, f64::NEG_INFINITY),
                |(min_val, max_val), val| {
                    let new_min = if val < min_val { val } else { min_val };
                    let new_max = if val > max_val { val } else { max_val };
                    (new_min, new_max)
                },
            );
            max - min
        }
        FocalOperation::Variety => {
            let mut unique = values.clone();
            unique.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
            unique.dedup_by(|a, b| (*a - *b).abs() < 1e-10);
            unique.len() as f64
        }
        FocalOperation::Min => {
            // Use fold to find minimum (panic-free)
            // This is safe because values is guaranteed non-empty by check at line 590
            values
                .iter()
                .copied()
                .fold(f64::INFINITY, |acc, val| if val < acc { val } else { acc })
        }
        FocalOperation::Max => {
            // Use fold to find maximum (panic-free)
            // This is safe because values is guaranteed non-empty by check at line 590
            values.iter().copied().fold(
                f64::NEG_INFINITY,
                |acc, val| if val > acc { val } else { acc },
            )
        }
        FocalOperation::Sum => values.iter().sum(),
        FocalOperation::StdDev => {
            let mean = values.iter().sum::<f64>() / values.len() as f64;
            let variance = values
                .iter()
                .map(|v| {
                    let diff = v - mean;
                    diff * diff
                })
                .sum::<f64>()
                / values.len() as f64;
            variance.sqrt()
        }
        FocalOperation::Majority => {
            // Count occurrences (with tolerance for floating point)
            let mut counts: Vec<(f64, usize)> = Vec::new();
            for &val in &values {
                if let Some(entry) = counts.iter_mut().find(|(v, _)| (v - val).abs() < 1e-10) {
                    entry.1 += 1;
                } else {
                    counts.push((val, 1));
                }
            }
            // Use fold to find value with maximum count (panic-free)
            // This is safe because counts is guaranteed non-empty since values is non-empty (checked at line 590)
            let (majority_val, _) =
                counts
                    .into_iter()
                    .fold((0.0, 0), |(acc_val, acc_count), (val, count)| {
                        if count > acc_count {
                            (val, count)
                        } else {
                            (acc_val, acc_count)
                        }
                    });
            majority_val
        }
    };

    Ok(result)
}

/// Optimized focal mean for rectangular windows using separable filtering
///
/// This is much faster than the generic implementation for large rectangular windows.
///
/// # Arguments
///
/// * `src` - Source raster buffer
/// * `width` - Window width (must be odd)
/// * `height` - Window height (must be odd)
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn focal_mean_separable(
    src: &RasterBuffer,
    width: usize,
    height: usize,
) -> Result<RasterBuffer> {
    if width % 2 == 0 || height % 2 == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "window_size",
            message: "Window dimensions must be odd".to_string(),
        });
    }

    let img_width = src.width();
    let img_height = src.height();

    // Horizontal pass
    let mut temp = RasterBuffer::zeros(img_width, img_height, RasterDataType::Float64);
    let hw = (width / 2) as i64;

    for y in 0..img_height {
        for x in 0..img_width {
            let mut sum = 0.0;
            let mut count = 0;

            for dx in -hw..=hw {
                let nx = (x as i64 + dx).clamp(0, img_width as i64 - 1);
                sum += src.get_pixel(nx as u64, y).map_err(AlgorithmError::Core)?;
                count += 1;
            }

            temp.set_pixel(x, y, sum / count as f64)
                .map_err(AlgorithmError::Core)?;
        }
    }

    // Vertical pass
    let mut dst = RasterBuffer::zeros(img_width, img_height, src.data_type());
    let hh = (height / 2) as i64;

    for y in 0..img_height {
        for x in 0..img_width {
            let mut sum = 0.0;
            let mut count = 0;

            for dy in -hh..=hh {
                let ny = (y as i64 + dy).clamp(0, img_height as i64 - 1);
                sum += temp.get_pixel(x, ny as u64).map_err(AlgorithmError::Core)?;
                count += 1;
            }

            dst.set_pixel(x, y, sum / count as f64)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(dst)
}

/// Applies a custom kernel convolution
///
/// # Arguments
///
/// * `src` - Source raster buffer
/// * `kernel` - Convolution kernel weights
/// * `width` - Kernel width
/// * `height` - Kernel height
/// * `normalize` - Whether to normalize by kernel sum
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn focal_convolve(
    src: &RasterBuffer,
    kernel: &[f64],
    width: usize,
    height: usize,
    normalize: bool,
) -> Result<RasterBuffer> {
    if kernel.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "kernel",
            message: "Kernel size must match width * height".to_string(),
        });
    }

    let img_width = src.width();
    let img_height = src.height();
    let mut dst = RasterBuffer::zeros(img_width, img_height, src.data_type());

    let hw = (width / 2) as i64;
    let hh = (height / 2) as i64;

    let kernel_sum: f64 = if normalize {
        let sum: f64 = kernel.iter().sum();
        if sum.abs() < 1e-10 { 1.0 } else { sum }
    } else {
        1.0
    };

    for y in 0..img_height {
        for x in 0..img_width {
            let mut sum = 0.0;

            for ky in 0..height {
                for kx in 0..width {
                    let dx = kx as i64 - hw;
                    let dy = ky as i64 - hh;
                    let nx = (x as i64 + dx).clamp(0, img_width as i64 - 1);
                    let ny = (y as i64 + dy).clamp(0, img_height as i64 - 1);

                    let pixel_val = src
                        .get_pixel(nx as u64, ny as u64)
                        .map_err(AlgorithmError::Core)?;
                    sum += pixel_val * kernel[ky * width + kx];
                }
            }

            dst.set_pixel(x, y, sum / kernel_sum)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(dst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    #[test]
    fn test_window_shape_rectangular() {
        let window = WindowShape::rectangular(3, 3)
            .expect("3x3 rectangular window creation should succeed in test");
        assert_eq!(window.dimensions(), (3, 3));
        assert_eq!(window.cell_count(), 9);
        assert!(window.includes(0, 0));
        assert!(window.includes(1, 1));
        assert!(!window.includes(2, 2));
    }

    #[test]
    fn test_window_shape_circular() {
        let window = WindowShape::circular(1.5)
            .expect("circular window with radius 1.5 should succeed in test");
        assert!(window.includes(0, 0));
        assert!(window.includes(1, 0));
        assert!(window.includes(0, 1));
        assert!(!window.includes(2, 2));
    }

    #[test]
    fn test_window_shape_custom() {
        let mask = vec![false, true, false, true, true, true, false, true, false];
        let window = WindowShape::custom(mask, 3, 3)
            .expect("custom 3x3 window creation should succeed in test");
        assert_eq!(window.cell_count(), 5);
        assert!(window.includes(0, 0));
        assert!(!window.includes(1, 1));
    }

    #[test]
    fn test_focal_mean() {
        let mut src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        // Set center to 5, rest to 0
        src.set_pixel(2, 2, 5.0)
            .expect("setting pixel (2,2) should succeed in test");

        let window = WindowShape::rectangular(3, 3)
            .expect("3x3 rectangular window creation should succeed in test");
        let result = focal_mean(&src, &window, &BoundaryMode::Edge)
            .expect("focal_mean operation should succeed in test");

        // Center should be ~0.55 (5/9)
        let center = result
            .get_pixel(2, 2)
            .expect("getting pixel (2,2) from result should succeed in test");
        assert_abs_diff_eq!(center, 5.0 / 9.0, epsilon = 0.01);
    }

    #[test]
    fn test_focal_median() {
        let mut src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        src.set_pixel(2, 2, 9.0)
            .expect("setting pixel (2,2) should succeed in test");

        let window = WindowShape::rectangular(3, 3)
            .expect("3x3 rectangular window creation should succeed in test");
        let result = focal_median(&src, &window, &BoundaryMode::Edge)
            .expect("focal_median operation should succeed in test");

        // Median should be 0 (8 zeros, 1 nine)
        let center = result
            .get_pixel(2, 2)
            .expect("getting pixel (2,2) from result should succeed in test");
        assert_abs_diff_eq!(center, 0.0, epsilon = 0.01);
    }

    #[test]
    fn test_focal_range() {
        let mut src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        src.set_pixel(2, 2, 10.0)
            .expect("setting pixel (2,2) should succeed in test");

        let window = WindowShape::rectangular(3, 3)
            .expect("3x3 rectangular window creation should succeed in test");
        let result = focal_range(&src, &window, &BoundaryMode::Edge)
            .expect("focal_range operation should succeed in test");

        // Range should be 10
        let center = result
            .get_pixel(2, 2)
            .expect("getting pixel (2,2) from result should succeed in test");
        assert_abs_diff_eq!(center, 10.0, epsilon = 0.01);
    }

    #[test]
    fn test_focal_variety() {
        let mut src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        src.set_pixel(2, 2, 1.0)
            .expect("setting pixel (2,2) should succeed in test");
        src.set_pixel(2, 3, 2.0)
            .expect("setting pixel (2,3) should succeed in test");

        let window = WindowShape::rectangular(3, 3)
            .expect("3x3 rectangular window creation should succeed in test");
        let result = focal_variety(&src, &window, &BoundaryMode::Edge)
            .expect("focal_variety operation should succeed in test");

        // Should have 3 unique values (0, 1, 2)
        let center = result
            .get_pixel(2, 2)
            .expect("getting pixel (2,2) from result should succeed in test");
        assert_abs_diff_eq!(center, 3.0, epsilon = 0.01);
    }

    #[test]
    fn test_focal_stddev() {
        let mut src = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        for y in 0..5 {
            for x in 0..5 {
                src.set_pixel(x, y, (x + y) as f64)
                    .expect("setting pixel should succeed in test");
            }
        }

        let window = WindowShape::rectangular(3, 3)
            .expect("3x3 rectangular window creation should succeed in test");
        let result = focal_stddev(&src, &window, &BoundaryMode::Edge)
            .expect("focal_stddev operation should succeed in test");

        // Should have non-zero standard deviation
        let center = result
            .get_pixel(2, 2)
            .expect("getting pixel (2,2) from result should succeed in test");
        assert!(center > 0.0);
    }

    #[test]
    fn test_focal_mean_separable() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, (x + y) as f64)
                    .expect("setting pixel should succeed in test");
            }
        }

        let result = focal_mean_separable(&src, 3, 3)
            .expect("focal_mean_separable operation should succeed in test");

        // Compare with generic version
        let window = WindowShape::rectangular(3, 3)
            .expect("3x3 rectangular window creation should succeed in test");
        let expected = focal_mean(&src, &window, &BoundaryMode::Edge)
            .expect("focal_mean operation should succeed in test");

        for y in 1..9 {
            for x in 1..9 {
                let val = result
                    .get_pixel(x, y)
                    .expect("getting pixel from result should succeed in test");
                let exp = expected
                    .get_pixel(x, y)
                    .expect("getting pixel from expected should succeed in test");
                assert_abs_diff_eq!(val, exp, epsilon = 0.1);
            }
        }
    }

    #[test]
    fn test_boundary_modes() {
        let src = RasterBuffer::zeros(3, 3, RasterDataType::Float32);
        let window = WindowShape::rectangular(3, 3)
            .expect("3x3 rectangular window creation should succeed in test");

        // Test all boundary modes - they should not panic
        let _ = focal_mean(&src, &window, &BoundaryMode::Ignore);
        let _ = focal_mean(&src, &window, &BoundaryMode::Constant(0));
        let _ = focal_mean(&src, &window, &BoundaryMode::Reflect);
        let _ = focal_mean(&src, &window, &BoundaryMode::Wrap);
        let _ = focal_mean(&src, &window, &BoundaryMode::Edge);
    }
}
