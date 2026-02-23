//! Data transformation utilities for geospatial data.
//!
//! Provides functions to apply augmentation pipelines and normalization
//! to raster buffers and raw data arrays.

use crate::augmentation::AugmentationPipeline;
use crate::{Error, Result};
use ndarray::Array3;
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

/// Applies an augmentation pipeline to a raster buffer.
///
/// # Arguments
///
/// * `buffer` - Input raster buffer
/// * `pipeline` - Augmentation pipeline to apply
///
/// # Returns
///
/// Transformed raster buffer
///
/// # Errors
///
/// Returns an error if the buffer cannot be converted to the required format
/// or if any augmentation fails.
pub fn apply_transforms_to_buffer(
    buffer: &RasterBuffer,
    pipeline: &AugmentationPipeline,
) -> Result<RasterBuffer> {
    // Convert buffer to Array3<f32> (C x H x W)
    let width = buffer.width() as usize;
    let height = buffer.height() as usize;

    // Assume single-band for now, can be extended for multi-band
    let mut data = Vec::with_capacity(height * width);

    for y in 0..height {
        for x in 0..width {
            let value = buffer.get_pixel(x as u64, y as u64)?;
            data.push(value as f32);
        }
    }

    let array = Array3::from_shape_vec((1, height, width), data).map_err(|e| {
        Error::invalid_dimensions(
            format!("1x{}x{}", height, width),
            format!("array shape error: {}", e),
        )
    })?;

    // Apply augmentation pipeline
    let transformed = pipeline.apply(&array)?;

    // Convert back to RasterBuffer
    let transformed_data: Vec<f32> = transformed.iter().copied().collect();
    array_to_buffer(&transformed_data, width, height, buffer.data_type())
}

/// Normalizes a raster buffer to [0, 1] range.
///
/// # Arguments
///
/// * `buffer` - Input raster buffer
/// * `min_val` - Minimum value for normalization
/// * `max_val` - Maximum value for normalization
///
/// # Returns
///
/// Normalized raster buffer
pub fn normalize_buffer(buffer: &RasterBuffer, min_val: f64, max_val: f64) -> Result<RasterBuffer> {
    if min_val >= max_val {
        return Err(Error::invalid_parameter(
            "min_val/max_val",
            format!("{}/{}", min_val, max_val),
            "min_val must be < max_val",
        ));
    }

    let width = buffer.width();
    let height = buffer.height();
    let mut normalized = RasterBuffer::zeros(width, height, buffer.data_type());

    let range = max_val - min_val;

    for y in 0..height {
        for x in 0..width {
            let value = buffer.get_pixel(x, y)?;
            let normalized_value = (value - min_val) / range;
            normalized.set_pixel(x, y, normalized_value)?;
        }
    }

    Ok(normalized)
}

/// Standardizes a raster buffer using mean and standard deviation.
///
/// # Arguments
///
/// * `buffer` - Input raster buffer
/// * `mean` - Mean value for standardization
/// * `std_dev` - Standard deviation for standardization
///
/// # Returns
///
/// Standardized raster buffer
pub fn standardize_buffer(buffer: &RasterBuffer, mean: f64, std_dev: f64) -> Result<RasterBuffer> {
    if std_dev <= 0.0 {
        return Err(Error::invalid_parameter("std_dev", std_dev, "must be > 0"));
    }

    let width = buffer.width();
    let height = buffer.height();
    let mut standardized = RasterBuffer::zeros(width, height, buffer.data_type());

    for y in 0..height {
        for x in 0..width {
            let value = buffer.get_pixel(x, y)?;
            let standardized_value = (value - mean) / std_dev;
            standardized.set_pixel(x, y, standardized_value)?;
        }
    }

    Ok(standardized)
}

/// Converts a flat f32 array to a RasterBuffer.
///
/// # Arguments
///
/// * `data` - Flat array of pixel values
/// * `width` - Width in pixels
/// * `height` - Height in pixels
/// * `data_type` - Target data type
///
/// # Returns
///
/// RasterBuffer containing the data
fn array_to_buffer(
    data: &[f32],
    width: usize,
    height: usize,
    data_type: RasterDataType,
) -> Result<RasterBuffer> {
    if data.len() != width * height {
        return Err(Error::invalid_dimensions(
            format!("{}x{} = {} pixels", width, height, width * height),
            format!("{} values", data.len()),
        ));
    }

    let mut buffer = RasterBuffer::zeros(width as u64, height as u64, data_type);

    let mut idx = 0;
    for y in 0..height {
        for x in 0..width {
            buffer.set_pixel(x as u64, y as u64, data[idx] as f64)?;
            idx += 1;
        }
    }

    Ok(buffer)
}

/// Converts multiple raster buffers to a batched tensor format.
///
/// # Arguments
///
/// * `buffers` - Vector of raster buffers
/// * `num_channels` - Number of channels per buffer
///
/// # Returns
///
/// Flat vector in (N, C, H, W) format where N is batch size
pub fn buffers_to_tensor(buffers: &[RasterBuffer], num_channels: usize) -> Result<Vec<f32>> {
    if buffers.is_empty() {
        return Err(Error::invalid_parameter(
            "buffers",
            "empty",
            "at least one buffer required",
        ));
    }

    let width = buffers[0].width() as usize;
    let height = buffers[0].height() as usize;

    // Verify all buffers have the same dimensions
    for (i, buffer) in buffers.iter().enumerate() {
        if buffer.width() as usize != width || buffer.height() as usize != height {
            return Err(Error::invalid_dimensions(
                format!("{}x{}", width, height),
                format!("buffer[{}]: {}x{}", i, buffer.width(), buffer.height()),
            ));
        }
    }

    let batch_size = buffers.len();
    let total_size = batch_size * num_channels * height * width;
    let mut tensor = Vec::with_capacity(total_size);

    for buffer in buffers {
        for _c in 0..num_channels {
            for y in 0..height {
                for x in 0..width {
                    let value = buffer.get_pixel(x as u64, y as u64)?;
                    tensor.push(value as f32);
                }
            }
        }
    }

    Ok(tensor)
}

/// Clips values in a buffer to a specified range.
///
/// # Arguments
///
/// * `buffer` - Input raster buffer
/// * `min_val` - Minimum value
/// * `max_val` - Maximum value
///
/// # Returns
///
/// Clipped raster buffer
pub fn clip_buffer(buffer: &RasterBuffer, min_val: f64, max_val: f64) -> Result<RasterBuffer> {
    if min_val >= max_val {
        return Err(Error::invalid_parameter(
            "min_val/max_val",
            format!("{}/{}", min_val, max_val),
            "min_val must be < max_val",
        ));
    }

    let width = buffer.width();
    let height = buffer.height();
    let mut clipped = RasterBuffer::zeros(width, height, buffer.data_type());

    for y in 0..height {
        for x in 0..width {
            let value = buffer.get_pixel(x, y)?;
            let clipped_value = value.clamp(min_val, max_val);
            clipped.set_pixel(x, y, clipped_value)?;
        }
    }

    Ok(clipped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use oxigdal_core::buffer::RasterBuffer;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_normalize_buffer() {
        let mut buffer = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Set some test values
        for y in 0..10 {
            for x in 0..10 {
                buffer
                    .set_pixel(x, y, (x * 10 + y) as f64)
                    .expect("Failed to set pixel");
            }
        }

        let normalized = normalize_buffer(&buffer, 0.0, 99.0).expect("Failed to normalize");

        // Check that values are in [0, 1]
        for y in 0..10 {
            for x in 0..10 {
                let value = normalized.get_pixel(x, y).expect("Failed to get pixel");
                assert!((0.0..=1.0).contains(&value));
            }
        }

        // Check specific values
        let val_0_0 = normalized.get_pixel(0, 0).expect("Failed to get pixel");
        assert_relative_eq!(val_0_0, 0.0, epsilon = 1e-6);

        let val_9_9 = normalized.get_pixel(9, 9).expect("Failed to get pixel");
        assert_relative_eq!(val_9_9, 1.0, epsilon = 1e-6);
    }

    #[test]
    fn test_standardize_buffer() {
        let mut buffer = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        // Fill with constant value
        for y in 0..5 {
            for x in 0..5 {
                buffer.set_pixel(x, y, 10.0).expect("Failed to set pixel");
            }
        }

        let standardized = standardize_buffer(&buffer, 10.0, 2.0).expect("Failed to standardize");

        // All values should be 0 after standardizing with mean=10
        for y in 0..5 {
            for x in 0..5 {
                let value = standardized.get_pixel(x, y).expect("Failed to get pixel");
                assert_relative_eq!(value, 0.0, epsilon = 1e-6);
            }
        }
    }

    #[test]
    fn test_clip_buffer() {
        let mut buffer = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        // Set values outside the clip range
        buffer.set_pixel(0, 0, -100.0).expect("Failed to set pixel");
        buffer.set_pixel(1, 1, 50.0).expect("Failed to set pixel");
        buffer.set_pixel(2, 2, 1000.0).expect("Failed to set pixel");

        let clipped = clip_buffer(&buffer, 0.0, 100.0).expect("Failed to clip");

        assert_relative_eq!(
            clipped.get_pixel(0, 0).expect("Failed to get pixel"),
            0.0,
            epsilon = 1e-6
        );
        assert_relative_eq!(
            clipped.get_pixel(1, 1).expect("Failed to get pixel"),
            50.0,
            epsilon = 1e-6
        );
        assert_relative_eq!(
            clipped.get_pixel(2, 2).expect("Failed to get pixel"),
            100.0,
            epsilon = 1e-6
        );
    }

    #[test]
    fn test_array_to_buffer() {
        let data: Vec<f32> = (0..100).map(|i| i as f32).collect();
        let buffer = array_to_buffer(&data, 10, 10, RasterDataType::Float32);

        assert!(buffer.is_ok());
        let buffer = buffer.expect("Failed to create buffer");

        assert_eq!(buffer.width(), 10);
        assert_eq!(buffer.height(), 10);

        // Verify values
        for i in 0..100 {
            let x = i % 10;
            let y = i / 10;
            let value = buffer.get_pixel(x, y).expect("Failed to get pixel");
            assert_relative_eq!(value, i as f64, epsilon = 1e-6);
        }
    }

    #[test]
    fn test_buffers_to_tensor() {
        let buffer1 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        let buffer2 = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        let buffers = vec![buffer1, buffer2];
        let tensor = buffers_to_tensor(&buffers, 1);

        assert!(tensor.is_ok());
        let tensor = tensor.expect("Failed to convert to tensor");

        // Batch size 2, 1 channel, 5x5 = 50 elements
        assert_eq!(tensor.len(), 2 * 5 * 5);
    }

    #[test]
    fn test_validation_errors() {
        let buffer = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        // Invalid normalization range
        let result = normalize_buffer(&buffer, 100.0, 50.0);
        assert!(result.is_err());

        // Invalid standardization std_dev
        let result = standardize_buffer(&buffer, 0.0, -1.0);
        assert!(result.is_err());

        // Invalid clip range
        let result = clip_buffer(&buffer, 100.0, 50.0);
        assert!(result.is_err());

        // Empty buffers
        let result = buffers_to_tensor(&[], 1);
        assert!(result.is_err());
    }
}
