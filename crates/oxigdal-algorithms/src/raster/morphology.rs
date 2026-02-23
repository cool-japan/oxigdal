//! Morphological operations for rasters
//!
//! This module provides mathematical morphology operations:
//! - Dilation (expands bright regions)
//! - Erosion (shrinks bright regions)
//! - Opening (erosion followed by dilation)
//! - Closing (dilation followed by erosion)
//! - Morphological gradient (difference between dilation and erosion)
//! - Top-hat and black-hat transforms

use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

/// Structuring element shape
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StructuringElement {
    /// Square/rectangular element
    Square {
        /// Size of the square
        size: usize,
    },
    /// Cross-shaped element
    Cross {
        /// Size of the cross
        size: usize,
    },
    /// Disk-shaped element (approximated)
    Disk {
        /// Radius of the disk
        radius: usize,
    },
    /// Horizontal line
    HorizontalLine {
        /// Length of the line
        length: usize,
    },
    /// Vertical line
    VerticalLine {
        /// Length of the line
        length: usize,
    },
}

impl StructuringElement {
    /// Generates the structuring element as a 2D mask
    fn generate(&self) -> Result<Vec<Vec<bool>>> {
        use oxigdal_core::OxiGdalError;

        match self {
            Self::Square { size } => {
                if *size == 0 {
                    return Err(OxiGdalError::invalid_parameter_builder(
                        "size",
                        "must be positive, got 0",
                    )
                    .with_parameter("value", "0")
                    .with_parameter("min", "1")
                    .with_operation("morphology_square")
                    .with_suggestion("Use positive size. Common values: 3, 5, 7 for standard morphological operations")
                    .build()
                    .into());
                }
                Ok(vec![vec![true; *size]; *size])
            }
            Self::Cross { size } => {
                if *size == 0 || *size % 2 == 0 {
                    let suggested = if *size == 0 { 3 } else { size + 1 };
                    return Err(OxiGdalError::invalid_parameter_builder(
                        "size",
                        format!("must be odd and positive, got {}", size),
                    )
                    .with_parameter("value", size.to_string())
                    .with_parameter("min", "1")
                    .with_operation("morphology_cross")
                    .with_suggestion(format!(
                        "Use odd positive size. Try {} instead. Common values: 3, 5, 7",
                        suggested
                    ))
                    .build()
                    .into());
                }

                let center = size / 2;
                let mut mask = vec![vec![false; *size]; *size];

                for i in 0..*size {
                    mask[center][i] = true;
                    mask[i][center] = true;
                }

                Ok(mask)
            }
            Self::Disk { radius } => {
                if *radius == 0 {
                    return Err(OxiGdalError::invalid_parameter_builder(
                        "radius",
                        "must be positive, got 0",
                    )
                    .with_parameter("value", "0")
                    .with_parameter("min", "1")
                    .with_operation("morphology_disk")
                    .with_suggestion("Use positive radius. Common values: 1, 2, 3, 5 (in pixels)")
                    .build()
                    .into());
                }

                let size = 2 * radius + 1;
                let center = *radius as i32;
                let mut mask = vec![vec![false; size]; size];
                let r_sq = (radius * radius) as i32;

                for y in 0..size {
                    for x in 0..size {
                        let dx = x as i32 - center;
                        let dy = y as i32 - center;
                        if dx * dx + dy * dy <= r_sq {
                            mask[y][x] = true;
                        }
                    }
                }

                Ok(mask)
            }
            Self::HorizontalLine { length } => {
                if *length == 0 {
                    return Err(OxiGdalError::invalid_parameter_builder(
                        "length",
                        "must be positive, got 0",
                    )
                    .with_parameter("value", "0")
                    .with_parameter("min", "1")
                    .with_operation("morphology_horizontal_line")
                    .with_suggestion(
                        "Use positive length. Common values: 3, 5, 7, 9 for edge detection",
                    )
                    .build()
                    .into());
                }

                Ok(vec![vec![true; *length]])
            }
            Self::VerticalLine { length } => {
                if *length == 0 {
                    return Err(OxiGdalError::invalid_parameter_builder(
                        "length",
                        "must be positive, got 0",
                    )
                    .with_parameter("value", "0")
                    .with_parameter("min", "1")
                    .with_operation("morphology_vertical_line")
                    .with_suggestion(
                        "Use positive length. Common values: 3, 5, 7, 9 for edge detection",
                    )
                    .build()
                    .into());
                }

                Ok(vec![vec![true]; *length])
            }
        }
    }

    /// Returns the size (width, height) of the structuring element
    fn size(&self) -> (usize, usize) {
        match self {
            Self::Square { size } => (*size, *size),
            Self::Cross { size } => (*size, *size),
            Self::Disk { radius } => (2 * radius + 1, 2 * radius + 1),
            Self::HorizontalLine { length } => (*length, 1),
            Self::VerticalLine { length } => (1, *length),
        }
    }
}

/// Applies morphological dilation
///
/// Expands bright regions in the image
///
/// # Arguments
///
/// * `src` - Source raster
/// * `element` - Structuring element
///
/// # Errors
///
/// Returns an error if operation fails
pub fn dilate(src: &RasterBuffer, element: StructuringElement) -> Result<RasterBuffer> {
    let mask = element.generate()?;
    let (se_width, se_height) = element.size();

    let width = src.width();
    let height = src.height();
    let mut dst = RasterBuffer::zeros(width, height, src.data_type());

    let offset_x = (se_width / 2) as i64;
    let offset_y = (se_height / 2) as i64;

    for y in 0..height {
        for x in 0..width {
            let mut max_val = f64::NEG_INFINITY;

            // Apply structuring element
            for (se_y, row) in mask.iter().enumerate() {
                for (se_x, &active) in row.iter().enumerate() {
                    if !active {
                        continue;
                    }

                    let px = x as i64 + se_x as i64 - offset_x;
                    let py = y as i64 + se_y as i64 - offset_y;

                    if px >= 0 && px < width as i64 && py >= 0 && py < height as i64 {
                        let val = src
                            .get_pixel(px as u64, py as u64)
                            .map_err(AlgorithmError::Core)?;

                        if val.is_finite() && !src.is_nodata(val) && val > max_val {
                            max_val = val;
                        }
                    }
                }
            }

            if max_val.is_finite() {
                dst.set_pixel(x, y, max_val).map_err(AlgorithmError::Core)?;
            }
        }
    }

    Ok(dst)
}

/// Applies morphological erosion
///
/// Shrinks bright regions in the image
///
/// # Arguments
///
/// * `src` - Source raster
/// * `element` - Structuring element
///
/// # Errors
///
/// Returns an error if operation fails
pub fn erode(src: &RasterBuffer, element: StructuringElement) -> Result<RasterBuffer> {
    let mask = element.generate()?;
    let (se_width, se_height) = element.size();

    let width = src.width();
    let height = src.height();
    let mut dst = RasterBuffer::zeros(width, height, src.data_type());

    let offset_x = (se_width / 2) as i64;
    let offset_y = (se_height / 2) as i64;

    for y in 0..height {
        for x in 0..width {
            let mut min_val = f64::INFINITY;

            // Apply structuring element
            for (se_y, row) in mask.iter().enumerate() {
                for (se_x, &active) in row.iter().enumerate() {
                    if !active {
                        continue;
                    }

                    let px = x as i64 + se_x as i64 - offset_x;
                    let py = y as i64 + se_y as i64 - offset_y;

                    if px >= 0 && px < width as i64 && py >= 0 && py < height as i64 {
                        let val = src
                            .get_pixel(px as u64, py as u64)
                            .map_err(AlgorithmError::Core)?;

                        if val.is_finite() && !src.is_nodata(val) && val < min_val {
                            min_val = val;
                        }
                    }
                }
            }

            if min_val.is_finite() {
                dst.set_pixel(x, y, min_val).map_err(AlgorithmError::Core)?;
            }
        }
    }

    Ok(dst)
}

/// Applies morphological opening
///
/// Opening = Erosion followed by Dilation
/// Removes small bright features while preserving larger ones
///
/// # Arguments
///
/// * `src` - Source raster
/// * `element` - Structuring element
///
/// # Errors
///
/// Returns an error if operation fails
pub fn open(src: &RasterBuffer, element: StructuringElement) -> Result<RasterBuffer> {
    let eroded = erode(src, element)?;
    dilate(&eroded, element)
}

/// Applies morphological closing
///
/// Closing = Dilation followed by Erosion
/// Fills small dark features while preserving larger ones
///
/// # Arguments
///
/// * `src` - Source raster
/// * `element` - Structuring element
///
/// # Errors
///
/// Returns an error if operation fails
pub fn close(src: &RasterBuffer, element: StructuringElement) -> Result<RasterBuffer> {
    let dilated = dilate(src, element)?;
    erode(&dilated, element)
}

/// Computes morphological gradient
///
/// Gradient = Dilation - Erosion
/// Highlights edges and boundaries
///
/// # Arguments
///
/// * `src` - Source raster
/// * `element` - Structuring element
///
/// # Errors
///
/// Returns an error if operation fails
pub fn morphological_gradient(
    src: &RasterBuffer,
    element: StructuringElement,
) -> Result<RasterBuffer> {
    let dilated = dilate(src, element)?;
    let eroded = erode(src, element)?;

    let width = src.width();
    let height = src.height();
    let mut gradient = RasterBuffer::zeros(width, height, src.data_type());

    for y in 0..height {
        for x in 0..width {
            let dil_val = dilated.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let ero_val = eroded.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let grad = dil_val - ero_val;

            gradient
                .set_pixel(x, y, grad)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(gradient)
}

/// Computes top-hat transform
///
/// Top-hat = Original - Opening
/// Extracts small bright features
///
/// # Arguments
///
/// * `src` - Source raster
/// * `element` - Structuring element
///
/// # Errors
///
/// Returns an error if operation fails
pub fn top_hat(src: &RasterBuffer, element: StructuringElement) -> Result<RasterBuffer> {
    let opened = open(src, element)?;

    let width = src.width();
    let height = src.height();
    let mut result = RasterBuffer::zeros(width, height, src.data_type());

    for y in 0..height {
        for x in 0..width {
            let orig = src.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let open_val = opened.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let top = orig - open_val;

            result.set_pixel(x, y, top).map_err(AlgorithmError::Core)?;
        }
    }

    Ok(result)
}

/// Computes black-hat transform
///
/// Black-hat = Closing - Original
/// Extracts small dark features
///
/// # Arguments
///
/// * `src` - Source raster
/// * `element` - Structuring element
///
/// # Errors
///
/// Returns an error if operation fails
pub fn black_hat(src: &RasterBuffer, element: StructuringElement) -> Result<RasterBuffer> {
    let closed = close(src, element)?;

    let width = src.width();
    let height = src.height();
    let mut result = RasterBuffer::zeros(width, height, src.data_type());

    for y in 0..height {
        for x in 0..width {
            let close_val = closed.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let orig = src.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let black = close_val - orig;

            result
                .set_pixel(x, y, black)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(result)
}

/// Computes internal gradient
///
/// Internal gradient = Original - Erosion
///
/// # Arguments
///
/// * `src` - Source raster
/// * `element` - Structuring element
///
/// # Errors
///
/// Returns an error if operation fails
pub fn internal_gradient(src: &RasterBuffer, element: StructuringElement) -> Result<RasterBuffer> {
    let eroded = erode(src, element)?;

    let width = src.width();
    let height = src.height();
    let mut gradient = RasterBuffer::zeros(width, height, src.data_type());

    for y in 0..height {
        for x in 0..width {
            let orig = src.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let ero_val = eroded.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let grad = orig - ero_val;

            gradient
                .set_pixel(x, y, grad)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(gradient)
}

/// Computes external gradient
///
/// External gradient = Dilation - Original
///
/// # Arguments
///
/// * `src` - Source raster
/// * `element` - Structuring element
///
/// # Errors
///
/// Returns an error if operation fails
pub fn external_gradient(src: &RasterBuffer, element: StructuringElement) -> Result<RasterBuffer> {
    let dilated = dilate(src, element)?;

    let width = src.width();
    let height = src.height();
    let mut gradient = RasterBuffer::zeros(width, height, src.data_type());

    for y in 0..height {
        for x in 0..width {
            let dil_val = dilated.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let orig = src.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let grad = dil_val - orig;

            gradient
                .set_pixel(x, y, grad)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(gradient)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_structuring_elements() {
        let square = StructuringElement::Square { size: 3 };
        let mask = square.generate();
        assert!(mask.is_ok());
        let m = mask.expect("Should generate");
        assert_eq!(m.len(), 3);
        assert_eq!(m[0].len(), 3);

        let cross = StructuringElement::Cross { size: 3 };
        let mask = cross.generate();
        assert!(mask.is_ok());

        let disk = StructuringElement::Disk { radius: 2 };
        let mask = disk.generate();
        assert!(mask.is_ok());
    }

    #[test]
    fn test_dilate() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Set center pixel
        src.set_pixel(5, 5, 100.0).ok();

        let element = StructuringElement::Square { size: 3 };
        let result = dilate(&src, element);
        assert!(result.is_ok());

        let dilated = result.expect("Should succeed");

        // Check that dilation expanded the bright region
        let val = dilated.get_pixel(4, 4).expect("Should get pixel");
        assert!((val - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_erode() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Fill a 5x5 region
        for y in 3..8 {
            for x in 3..8 {
                src.set_pixel(x, y, 100.0).ok();
            }
        }

        let element = StructuringElement::Square { size: 3 };
        let result = erode(&src, element);
        assert!(result.is_ok());
    }

    #[test]
    fn test_opening() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, 50.0).ok();
            }
        }

        // Add small bright spot
        src.set_pixel(5, 5, 100.0).ok();

        let element = StructuringElement::Square { size: 3 };
        let result = open(&src, element);
        assert!(result.is_ok());
    }

    #[test]
    fn test_closing() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, 100.0).ok();
            }
        }

        // Add small dark spot
        src.set_pixel(5, 5, 0.0).ok();

        let element = StructuringElement::Square { size: 3 };
        let result = close(&src, element);
        assert!(result.is_ok());
    }

    #[test]
    fn test_morphological_gradient() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Create a rectangle
        for y in 3..7 {
            for x in 3..7 {
                src.set_pixel(x, y, 100.0).ok();
            }
        }

        let element = StructuringElement::Square { size: 3 };
        let result = morphological_gradient(&src, element);
        assert!(result.is_ok());
    }

    #[test]
    fn test_top_hat() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, 50.0).ok();
            }
        }

        src.set_pixel(5, 5, 100.0).ok();

        let element = StructuringElement::Square { size: 3 };
        let result = top_hat(&src, element);
        assert!(result.is_ok());
    }

    // ========== Additional Morphological Operations ==========

    #[test]
    fn test_black_hat() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, 100.0).ok();
            }
        }

        // Add small dark spot
        src.set_pixel(5, 5, 0.0).ok();

        let element = StructuringElement::Square { size: 3 };
        let result = black_hat(&src, element);
        assert!(result.is_ok());
    }

    #[test]
    fn test_internal_gradient() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Create a filled circle
        for y in 0..10 {
            for x in 0..10 {
                let dx = x as f64 - 5.0;
                let dy = y as f64 - 5.0;
                if (dx * dx + dy * dy).sqrt() < 3.0 {
                    src.set_pixel(x, y, 100.0).ok();
                }
            }
        }

        let element = StructuringElement::Square { size: 3 };
        let result = internal_gradient(&src, element);
        assert!(result.is_ok());
    }

    #[test]
    fn test_external_gradient() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Create a filled square
        for y in 3..7 {
            for x in 3..7 {
                src.set_pixel(x, y, 100.0).ok();
            }
        }

        let element = StructuringElement::Square { size: 3 };
        let result = external_gradient(&src, element);
        assert!(result.is_ok());
    }

    // ========== Different Structuring Elements ==========

    #[test]
    fn test_disk_element() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        src.set_pixel(5, 5, 100.0).ok();

        let element = StructuringElement::Disk { radius: 2 };
        let result = dilate(&src, element);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cross_element() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        src.set_pixel(5, 5, 100.0).ok();

        let element = StructuringElement::Cross { size: 5 };
        let result = dilate(&src, element);
        assert!(result.is_ok());
    }

    #[test]
    fn test_horizontal_line_element() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        src.set_pixel(5, 5, 100.0).ok();

        let element = StructuringElement::HorizontalLine { length: 5 };
        let result = dilate(&src, element);
        assert!(result.is_ok());
    }

    #[test]
    fn test_vertical_line_element() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        src.set_pixel(5, 5, 100.0).ok();

        let element = StructuringElement::VerticalLine { length: 5 };
        let result = dilate(&src, element);
        assert!(result.is_ok());
    }

    // ========== Edge Cases ==========

    #[test]
    fn test_structuring_element_zero_size() {
        let element = StructuringElement::Square { size: 0 };
        let result = element.generate();
        assert!(result.is_err());
    }

    #[test]
    fn test_cross_even_size() {
        let element = StructuringElement::Cross { size: 4 };
        let result = element.generate();
        assert!(result.is_err());
    }

    #[test]
    fn test_disk_zero_radius() {
        let element = StructuringElement::Disk { radius: 0 };
        let result = element.generate();
        assert!(result.is_err());
    }

    #[test]
    fn test_dilate_single_pixel() {
        let mut src = RasterBuffer::zeros(1, 1, RasterDataType::Float32);
        src.set_pixel(0, 0, 100.0).ok();

        let element = StructuringElement::Square { size: 1 };
        let result = dilate(&src, element);
        assert!(result.is_ok());
        let dilated = result.expect("Should succeed");
        let val = dilated.get_pixel(0, 0).expect("Should get pixel");
        assert!((val - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_erode_single_pixel() {
        let mut src = RasterBuffer::zeros(1, 1, RasterDataType::Float32);
        src.set_pixel(0, 0, 100.0).ok();

        let element = StructuringElement::Square { size: 1 };
        let result = erode(&src, element);
        assert!(result.is_ok());
    }

    // ========== Complex Patterns ==========

    #[test]
    fn test_opening_closing_inverse() {
        let mut src = RasterBuffer::zeros(15, 15, RasterDataType::Float32);

        // Create pattern with small bright and dark features
        for y in 0..15 {
            for x in 0..15 {
                src.set_pixel(x, y, 50.0).ok();
            }
        }

        // Add small bright spot
        src.set_pixel(5, 5, 100.0).ok();
        // Add small dark spot
        src.set_pixel(10, 10, 0.0).ok();

        let element = StructuringElement::Square { size: 3 };

        let opened = open(&src, element);
        assert!(opened.is_ok());

        let closed = close(&src, element);
        assert!(closed.is_ok());
    }

    #[test]
    fn test_gradient_types_comparison() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Create a square
        for y in 3..7 {
            for x in 3..7 {
                src.set_pixel(x, y, 100.0).ok();
            }
        }

        let element = StructuringElement::Square { size: 3 };

        let morph_grad = morphological_gradient(&src, element);
        assert!(morph_grad.is_ok());

        let int_grad = internal_gradient(&src, element);
        assert!(int_grad.is_ok());

        let ext_grad = external_gradient(&src, element);
        assert!(ext_grad.is_ok());
    }

    #[test]
    fn test_dilation_erosion_properties() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 4..6 {
            for x in 4..6 {
                src.set_pixel(x, y, 100.0).ok();
            }
        }

        let element = StructuringElement::Square { size: 3 };

        let dilated = dilate(&src, element);
        assert!(dilated.is_ok());
        let dil = dilated.expect("Should succeed");

        // Dilated region should be larger
        let val = dil.get_pixel(3, 3).expect("Should get pixel");
        assert!((val - 100.0).abs() < f64::EPSILON);

        let eroded = erode(&src, element);
        assert!(eroded.is_ok());
        let ero = eroded.expect("Should succeed");

        // Eroded region should be smaller or gone
        let val_ero = ero.get_pixel(4, 4).expect("Should get pixel");
        assert!(val_ero <= 100.0);
    }

    #[test]
    fn test_top_hat_black_hat_duality() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, 50.0).ok();
            }
        }

        // Add both bright and dark features
        src.set_pixel(3, 3, 100.0).ok();
        src.set_pixel(7, 7, 0.0).ok();

        let element = StructuringElement::Square { size: 3 };

        let top = top_hat(&src, element);
        assert!(top.is_ok());

        let black = black_hat(&src, element);
        assert!(black.is_ok());
    }

    // ========== Larger Elements ==========

    #[test]
    fn test_large_disk_element() {
        let mut src = RasterBuffer::zeros(20, 20, RasterDataType::Float32);
        src.set_pixel(10, 10, 100.0).ok();

        let element = StructuringElement::Disk { radius: 5 };
        let result = dilate(&src, element);
        assert!(result.is_ok());
    }

    #[test]
    fn test_large_square_element() {
        let mut src = RasterBuffer::zeros(20, 20, RasterDataType::Float32);

        for y in 8..12 {
            for x in 8..12 {
                src.set_pixel(x, y, 100.0).ok();
            }
        }

        let element = StructuringElement::Square { size: 7 };
        let result = erode(&src, element);
        assert!(result.is_ok());
    }

    // ========== Real-world Applications ==========

    #[test]
    fn test_binary_image_cleanup() {
        let mut src = RasterBuffer::zeros(15, 15, RasterDataType::Float32);

        // Create noisy binary image
        for y in 0..15 {
            for x in 0..15 {
                if (x > 3 && x < 12) && (y > 3 && y < 12) {
                    src.set_pixel(x, y, 1.0).ok();
                } else {
                    src.set_pixel(x, y, 0.0).ok();
                }
            }
        }

        // Add some noise
        src.set_pixel(1, 1, 1.0).ok(); // Salt
        src.set_pixel(7, 7, 0.0).ok(); // Pepper

        let element = StructuringElement::Square { size: 3 };

        // Opening removes small bright features
        let opened = open(&src, element);
        assert!(opened.is_ok());

        // Closing removes small dark features
        let closed = close(&src, element);
        assert!(closed.is_ok());
    }

    #[test]
    fn test_boundary_extraction() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Create a filled square
        for y in 3..7 {
            for x in 3..7 {
                src.set_pixel(x, y, 100.0).ok();
            }
        }

        let element = StructuringElement::Square { size: 3 };

        // Internal gradient should extract the inner boundary
        let boundary = internal_gradient(&src, element);
        assert!(boundary.is_ok());
        let bound = boundary.expect("Should succeed");

        // Check that boundary pixels are non-zero
        let val = bound.get_pixel(3, 3).expect("Should get pixel");
        assert!(val > 0.0);
    }
}
