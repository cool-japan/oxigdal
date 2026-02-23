//! Cloud removal using inpainting techniques

use ndarray::{Array2, Array3, s};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use rayon::prelude::*;
use tracing::{debug, instrument};

use super::{CloudConfig, CloudMask};
use crate::error::{InferenceError, MlError, Result};

/// Cloud remover using partial convolution inpainting
pub struct CloudRemover {
    /// Configuration
    config: CloudConfig,
}

impl CloudRemover {
    /// Create a new cloud remover
    #[must_use]
    pub fn new(config: CloudConfig) -> Self {
        Self { config }
    }

    /// Remove clouds from image using mask
    ///
    /// # Arguments
    /// * `image` - Original multi-band image
    /// * `mask` - Cloud mask (binary or confidence)
    ///
    /// # Returns
    /// Cloud-free image with inpainted regions
    ///
    /// # Errors
    /// Returns error if removal fails or invalid input
    #[instrument(skip(self, image, mask))]
    pub fn remove(&self, image: &RasterBuffer, mask: &CloudMask) -> Result<RasterBuffer> {
        debug!(
            "Removing clouds from {}x{} image",
            image.width(),
            image.height()
        );

        // Validate dimensions
        if image.width() != mask.width || image.height() != mask.height {
            return Err(MlError::Inference(InferenceError::InvalidInputShape {
                expected: vec![mask.height as usize, mask.width as usize],
                actual: vec![image.height() as usize, image.width() as usize],
            }));
        }

        // Dilate mask to include cloud shadows
        let mut dilated_mask = mask.clone();
        if self.config.dilation_radius > 0 {
            dilated_mask.dilate(self.config.dilation_radius)?;
        }

        // Perform inpainting
        let inpainted = self.inpaint(image, &dilated_mask)?;

        // Blend with original using alpha
        let result = self.blend(image, &inpainted, &dilated_mask)?;

        Ok(result)
    }

    /// Remove clouds using temporal interpolation from multiple images
    ///
    /// # Arguments
    /// * `images` - Time series of images
    /// * `masks` - Cloud masks for each image
    /// * `target_idx` - Index of image to clean
    ///
    /// # Errors
    /// Returns error if temporal interpolation fails
    pub fn remove_temporal(
        &self,
        images: &[RasterBuffer],
        masks: &[CloudMask],
        target_idx: usize,
    ) -> Result<RasterBuffer> {
        if images.len() != masks.len() {
            return Err(MlError::InvalidConfig(
                "Number of images and masks must match".to_string(),
            ));
        }

        if target_idx >= images.len() {
            return Err(MlError::InvalidConfig(format!(
                "Target index {} out of range (len={})",
                target_idx,
                images.len()
            )));
        }

        // Find cloud-free pixels in temporal neighbors
        let target = &images[target_idx];
        let target_mask = &masks[target_idx];

        let interpolated = self.temporal_interpolate(images, masks, target_idx)?;

        // Blend interpolated with inpainted
        let inpainted = self.inpaint(target, target_mask)?;
        let blended = self.weighted_blend(&interpolated, &inpainted, 0.7)?;

        Ok(blended)
    }

    /// Inpaint cloud regions using partial convolution
    fn inpaint(&self, image: &RasterBuffer, mask: &CloudMask) -> Result<RasterBuffer> {
        let height = image.height() as usize;
        let width = image.width() as usize;

        let image_data = buffer_to_array3(image)?;
        let mask_data = mask.mask.as_slice::<f32>().map_err(MlError::OxiGdal)?;

        let num_bands = image_data.shape()[0];
        let mut result = image_data.clone();

        // Iterative partial convolution inpainting
        let iterations = 5;
        let kernel_size = 7;

        for _iter in 0..iterations {
            for band in 0..num_bands {
                let band_data = result.slice(s![band, .., ..]).to_owned();
                let inpainted = partial_convolution_inpaint(&band_data, mask_data, kernel_size)?;
                result.slice_mut(s![band, .., ..]).assign(&inpainted);
            }
        }

        array3_to_buffer(result, width as u64, height as u64)
    }

    /// Blend inpainted image with original
    fn blend(
        &self,
        original: &RasterBuffer,
        inpainted: &RasterBuffer,
        mask: &CloudMask,
    ) -> Result<RasterBuffer> {
        let height = original.height() as usize;
        let width = original.width() as usize;

        let orig_data = buffer_to_array3(original)?;
        let inp_data = buffer_to_array3(inpainted)?;
        let mask_data = mask.mask.as_slice::<f32>().map_err(MlError::OxiGdal)?;

        let num_bands = orig_data.shape()[0];
        let alpha = self.config.blend_alpha;

        let mut blended = Array3::<f32>::zeros((num_bands, height, width));

        for band in 0..num_bands {
            for y in 0..height {
                for x in 0..width {
                    let mask_val = mask_data[y * width + x];
                    let orig_val = orig_data[[band, y, x]];
                    let inp_val = inp_data[[band, y, x]];

                    // If clouded, use inpainted; otherwise use original
                    let val = if mask_val > 0.5 {
                        inp_val * alpha + orig_val * (1.0 - alpha)
                    } else {
                        orig_val
                    };

                    blended[[band, y, x]] = val;
                }
            }
        }

        array3_to_buffer(blended, width as u64, height as u64)
    }

    /// Temporal interpolation from neighboring images
    fn temporal_interpolate(
        &self,
        images: &[RasterBuffer],
        masks: &[CloudMask],
        target_idx: usize,
    ) -> Result<RasterBuffer> {
        let height = images[target_idx].height() as usize;
        let width = images[target_idx].width() as usize;

        let target_data = buffer_to_array3(&images[target_idx])?;
        let num_bands = target_data.shape()[0];

        let mut interpolated = Array3::<f32>::zeros((num_bands, height, width));
        let mut weight_sum = Array2::<f32>::zeros((height, width));

        // Accumulate cloud-free pixels from all images
        for (idx, (img, mask)) in images.iter().zip(masks.iter()).enumerate() {
            let img_data = buffer_to_array3(img)?;
            let mask_data = mask.mask.as_slice::<f32>().map_err(MlError::OxiGdal)?;

            // Weight based on temporal distance
            let temporal_dist = (idx as i32 - target_idx as i32).abs() as f32;
            let weight = 1.0 / (1.0 + temporal_dist);

            for y in 0..height {
                for x in 0..width {
                    let mask_val = mask_data[y * width + x];
                    // Only use cloud-free pixels
                    if mask_val < 0.5 {
                        let w = weight;
                        weight_sum[[y, x]] += w;
                        for band in 0..num_bands {
                            interpolated[[band, y, x]] += img_data[[band, y, x]] * w;
                        }
                    }
                }
            }
        }

        // Normalize by weight sum
        for y in 0..height {
            for x in 0..width {
                let w = weight_sum[[y, x]];
                if w > 0.0 {
                    for band in 0..num_bands {
                        interpolated[[band, y, x]] /= w;
                    }
                }
            }
        }

        array3_to_buffer(interpolated, width as u64, height as u64)
    }

    /// Weighted blend of two images
    fn weighted_blend(
        &self,
        img1: &RasterBuffer,
        img2: &RasterBuffer,
        weight1: f32,
    ) -> Result<RasterBuffer> {
        let height = img1.height() as usize;
        let width = img1.width() as usize;

        let data1 = buffer_to_array3(img1)?;
        let data2 = buffer_to_array3(img2)?;

        let weight2 = 1.0 - weight1;
        let blended = &data1 * weight1 + &data2 * weight2;

        array3_to_buffer(blended, width as u64, height as u64)
    }
}

/// Partial convolution inpainting for a single band
fn partial_convolution_inpaint(
    band: &Array2<f32>,
    mask: &[f32],
    kernel_size: usize,
) -> Result<Array2<f32>> {
    let height = band.shape()[0];
    let width = band.shape()[1];
    let radius = kernel_size / 2;

    let output: Vec<f32> = (0..height * width)
        .into_par_iter()
        .map(|idx| {
            let y = idx / width;
            let x = idx % width;

            let mask_val = mask[idx];

            // If pixel is not masked, keep original value
            if mask_val < 0.5 {
                return band[[y, x]];
            }

            // Compute weighted average of unmasked neighbors
            let y_start = y.saturating_sub(radius);
            let y_end = (y + radius + 1).min(height);
            let x_start = x.saturating_sub(radius);
            let x_end = (x + radius + 1).min(width);

            let mut sum = 0.0_f32;
            let mut weight_sum = 0.0_f32;

            for ny in y_start..y_end {
                for nx in x_start..x_end {
                    let neighbor_mask = mask[ny * width + nx];
                    // Only use unmasked neighbors
                    if neighbor_mask < 0.5 {
                        let val = band[[ny, nx]];
                        let dist =
                            ((ny as i32 - y as i32).pow(2) + (nx as i32 - x as i32).pow(2)) as f32;
                        let weight = 1.0 / (1.0 + dist);
                        sum += val * weight;
                        weight_sum += weight;
                    }
                }
            }

            if weight_sum > 0.0 {
                sum / weight_sum
            } else {
                // No valid neighbors, keep original (or 0)
                band[[y, x]]
            }
        })
        .collect();

    Array2::from_shape_vec((height, width), output).map_err(|e| {
        MlError::Inference(InferenceError::OutputParsingFailed {
            reason: format!("Failed to create array: {}", e),
        })
    })
}

/// Convert RasterBuffer to Array3 (C, H, W)
fn buffer_to_array3(buffer: &RasterBuffer) -> Result<Array3<f32>> {
    let height = buffer.height() as usize;
    let width = buffer.width() as usize;

    let data = buffer.as_slice::<f32>().map_err(MlError::OxiGdal)?;

    let total_pixels = height * width;
    let num_bands = data.len() / total_pixels;

    Array3::from_shape_vec((num_bands, height, width), data.to_vec()).map_err(|e| {
        MlError::Inference(InferenceError::OutputParsingFailed {
            reason: format!("Failed to create array: {}", e),
        })
    })
}

/// Convert Array3 to RasterBuffer
fn array3_to_buffer(array: Array3<f32>, width: u64, height: u64) -> Result<RasterBuffer> {
    let data: Vec<f32> = array.into_iter().collect();
    let bytes: Vec<u8> = data.iter().flat_map(|&f: &f32| f.to_le_bytes()).collect();

    Ok(RasterBuffer::new(
        bytes,
        width,
        height,
        RasterDataType::Float32,
        oxigdal_core::types::NoDataValue::None,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cloud_remover_creation() {
        let config = CloudConfig::default();
        let remover = CloudRemover::new(config);
        assert!(remover.config.blend_alpha > 0.0);
    }

    #[test]
    fn test_partial_convolution() {
        let band = Array2::<f32>::zeros((100, 100));
        let mask = vec![0.0; 100 * 100];

        let result = partial_convolution_inpaint(&band, &mask, 5);
        assert!(result.is_ok());
    }

    #[test]
    fn test_temporal_interpolation_validation() {
        let config = CloudConfig::default();
        let remover = CloudRemover::new(config);

        // Empty arrays should fail
        let result = remover.remove_temporal(&[], &[], 0);
        assert!(result.is_err());
    }
}
