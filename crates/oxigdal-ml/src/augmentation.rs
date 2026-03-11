//! Data augmentation for geospatial imagery
//!
//! This module provides comprehensive data augmentation techniques specifically
//! designed for geospatial and satellite imagery.

use crate::error::{PreprocessingError, Result};
use oxigdal_core::buffer::RasterBuffer;
// use oxigdal_core::types::RasterDataType;
use scirs2_core::random::prelude::{StdRng, seeded_rng};
use tracing::debug;

/// Generates a Gaussian random number using Box-Muller transform
fn gaussian_random(rng: &mut StdRng, mean: f64, std_dev: f64) -> f64 {
    use std::f64::consts::PI;

    // Box-Muller transform
    let u1: f64 = rng.random();
    let u2: f64 = rng.random();

    let z0: f64 = (-2.0_f64 * u1.ln()).sqrt() * (2.0_f64 * PI * u2).cos();
    mean + std_dev * z0
}

/// Augmentation configuration
#[derive(Debug, Clone, Default)]
pub struct AugmentationConfig {
    /// Enable horizontal flip
    pub horizontal_flip: bool,
    /// Enable vertical flip
    pub vertical_flip: bool,
    /// Rotation angles (degrees)
    pub rotation_angles: Vec<f32>,
    /// Enable random crops
    pub random_crop: bool,
    /// Crop size (if random_crop enabled)
    pub crop_size: Option<(u64, u64)>,
    /// Brightness adjustment range
    pub brightness_range: Option<(f32, f32)>,
    /// Contrast adjustment range
    pub contrast_range: Option<(f32, f32)>,
    /// Saturation adjustment range (for RGB)
    pub saturation_range: Option<(f32, f32)>,
    /// Add Gaussian noise
    pub gaussian_noise: Option<f32>,
    /// Add salt and pepper noise
    pub salt_pepper_noise: Option<f32>,
    /// Gaussian blur kernel size
    pub blur_kernel: Option<usize>,
}

impl AugmentationConfig {
    /// Creates a builder for augmentation configuration
    #[must_use]
    pub fn builder() -> AugmentationConfigBuilder {
        AugmentationConfigBuilder::default()
    }

    /// Creates a standard augmentation configuration
    #[must_use]
    pub fn standard() -> Self {
        Self {
            horizontal_flip: true,
            vertical_flip: true,
            rotation_angles: vec![90.0, 180.0, 270.0],
            random_crop: false,
            crop_size: None,
            brightness_range: Some((-0.2, 0.2)),
            contrast_range: Some((0.8, 1.2)),
            saturation_range: Some((0.8, 1.2)),
            gaussian_noise: Some(0.01),
            salt_pepper_noise: None,
            blur_kernel: None,
        }
    }

    /// Creates an aggressive augmentation configuration
    #[must_use]
    pub fn aggressive() -> Self {
        Self {
            horizontal_flip: true,
            vertical_flip: true,
            rotation_angles: vec![45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0],
            random_crop: true,
            crop_size: Some((256, 256)),
            brightness_range: Some((-0.3, 0.3)),
            contrast_range: Some((0.7, 1.3)),
            saturation_range: Some((0.7, 1.3)),
            gaussian_noise: Some(0.02),
            salt_pepper_noise: Some(0.01),
            blur_kernel: Some(3),
        }
    }
}

/// Builder for augmentation configuration
#[derive(Debug, Default)]
pub struct AugmentationConfigBuilder {
    horizontal_flip: bool,
    vertical_flip: bool,
    rotation_angles: Vec<f32>,
    random_crop: bool,
    crop_size: Option<(u64, u64)>,
    brightness_range: Option<(f32, f32)>,
    contrast_range: Option<(f32, f32)>,
    saturation_range: Option<(f32, f32)>,
    gaussian_noise: Option<f32>,
    salt_pepper_noise: Option<f32>,
    blur_kernel: Option<usize>,
}

impl AugmentationConfigBuilder {
    /// Enables horizontal flip
    #[must_use]
    pub fn horizontal_flip(mut self, enable: bool) -> Self {
        self.horizontal_flip = enable;
        self
    }

    /// Enables vertical flip
    #[must_use]
    pub fn vertical_flip(mut self, enable: bool) -> Self {
        self.vertical_flip = enable;
        self
    }

    /// Sets rotation angles
    #[must_use]
    pub fn rotation_angles(mut self, angles: Vec<f32>) -> Self {
        self.rotation_angles = angles;
        self
    }

    /// Enables random cropping
    #[must_use]
    pub fn random_crop(mut self, enable: bool, size: Option<(u64, u64)>) -> Self {
        self.random_crop = enable;
        self.crop_size = size;
        self
    }

    /// Sets brightness adjustment range
    #[must_use]
    pub fn brightness_range(mut self, min: f32, max: f32) -> Self {
        self.brightness_range = Some((min, max));
        self
    }

    /// Sets contrast adjustment range
    #[must_use]
    pub fn contrast_range(mut self, min: f32, max: f32) -> Self {
        self.contrast_range = Some((min, max));
        self
    }

    /// Sets saturation adjustment range
    #[must_use]
    pub fn saturation_range(mut self, min: f32, max: f32) -> Self {
        self.saturation_range = Some((min, max));
        self
    }

    /// Sets Gaussian noise standard deviation
    #[must_use]
    pub fn gaussian_noise(mut self, std_dev: f32) -> Self {
        self.gaussian_noise = Some(std_dev);
        self
    }

    /// Sets salt and pepper noise probability
    #[must_use]
    pub fn salt_pepper_noise(mut self, prob: f32) -> Self {
        self.salt_pepper_noise = Some(prob);
        self
    }

    /// Sets blur kernel size
    #[must_use]
    pub fn blur_kernel(mut self, size: usize) -> Self {
        self.blur_kernel = Some(size);
        self
    }

    /// Builds the configuration
    #[must_use]
    pub fn build(self) -> AugmentationConfig {
        AugmentationConfig {
            horizontal_flip: self.horizontal_flip,
            vertical_flip: self.vertical_flip,
            rotation_angles: self.rotation_angles,
            random_crop: self.random_crop,
            crop_size: self.crop_size,
            brightness_range: self.brightness_range,
            contrast_range: self.contrast_range,
            saturation_range: self.saturation_range,
            gaussian_noise: self.gaussian_noise,
            salt_pepper_noise: self.salt_pepper_noise,
            blur_kernel: self.blur_kernel,
        }
    }
}

/// Applies horizontal flip augmentation
///
/// # Errors
/// Returns an error if the operation fails
pub fn horizontal_flip(input: &RasterBuffer) -> Result<RasterBuffer> {
    debug!("Applying horizontal flip");
    let mut output = RasterBuffer::zeros(input.width(), input.height(), input.data_type());

    for y in 0..input.height() {
        for x in 0..input.width() {
            let flipped_x = input.width() - 1 - x;
            let value =
                input
                    .get_pixel(x, y)
                    .map_err(|e| PreprocessingError::AugmentationFailed {
                        reason: format!("Failed to read pixel: {}", e),
                    })?;
            output.set_pixel(flipped_x, y, value).map_err(|e| {
                PreprocessingError::AugmentationFailed {
                    reason: format!("Failed to write pixel: {}", e),
                }
            })?;
        }
    }

    Ok(output)
}

/// Applies vertical flip augmentation
///
/// # Errors
/// Returns an error if the operation fails
pub fn vertical_flip(input: &RasterBuffer) -> Result<RasterBuffer> {
    debug!("Applying vertical flip");
    let mut output = RasterBuffer::zeros(input.width(), input.height(), input.data_type());

    for y in 0..input.height() {
        for x in 0..input.width() {
            let flipped_y = input.height() - 1 - y;
            let value =
                input
                    .get_pixel(x, y)
                    .map_err(|e| PreprocessingError::AugmentationFailed {
                        reason: format!("Failed to read pixel: {}", e),
                    })?;
            output.set_pixel(x, flipped_y, value).map_err(|e| {
                PreprocessingError::AugmentationFailed {
                    reason: format!("Failed to write pixel: {}", e),
                }
            })?;
        }
    }

    Ok(output)
}

/// Applies rotation augmentation
///
/// # Errors
/// Returns an error if the operation fails
pub fn rotate(input: &RasterBuffer, angle_degrees: f32) -> Result<RasterBuffer> {
    debug!("Applying rotation: {} degrees", angle_degrees);

    // Simple rotation for 90-degree multiples
    if (angle_degrees % 90.0).abs() < 0.1 {
        let times = ((angle_degrees / 90.0).round() as i32).rem_euclid(4);
        return rotate_90_times(input, times as usize);
    }

    // General rotation with bilinear interpolation
    rotate_general(input, angle_degrees)
}

/// Applies general rotation with bilinear interpolation
///
/// Uses affine transformation and bilinear interpolation for arbitrary angles.
/// Out-of-bounds pixels are filled with zeros.
///
/// # Errors
/// Returns an error if pixel access fails
fn rotate_general(input: &RasterBuffer, angle_degrees: f32) -> Result<RasterBuffer> {
    let width = input.width() as f64;
    let height = input.height() as f64;

    // Convert angle to radians
    let angle = angle_degrees as f64 * std::f64::consts::PI / 180.0;
    let cos_angle = angle.cos();
    let sin_angle = angle.sin();

    // Calculate output dimensions to fit the entire rotated image
    let corners = [(0.0, 0.0), (width, 0.0), (0.0, height), (width, height)];

    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    // Find bounding box of rotated image
    for (x, y) in &corners {
        let rotated_x = x * cos_angle - y * sin_angle;
        let rotated_y = x * sin_angle + y * cos_angle;
        min_x = min_x.min(rotated_x);
        max_x = max_x.max(rotated_x);
        min_y = min_y.min(rotated_y);
        max_y = max_y.max(rotated_y);
    }

    let out_width = (max_x - min_x).ceil() as u64;
    let out_height = (max_y - min_y).ceil() as u64;

    let mut output = RasterBuffer::zeros(out_width, out_height, input.data_type());

    // Center of input image
    let cx = width / 2.0;
    let cy = height / 2.0;

    // Center of output image
    let out_cx = (max_x - min_x) / 2.0;
    let out_cy = (max_y - min_y) / 2.0;

    // For each output pixel, find corresponding input pixel
    for out_y in 0..out_height {
        for out_x in 0..out_width {
            // Translate to center
            let x = out_x as f64 - out_cx;
            let y = out_y as f64 - out_cy;

            // Inverse rotation
            let src_x = x * cos_angle + y * sin_angle + cx;
            let src_y = -x * sin_angle + y * cos_angle + cy;

            // Apply bilinear interpolation
            let value = bilinear_interpolate(input, src_x, src_y)?;

            output.set_pixel(out_x, out_y, value).map_err(|e| {
                PreprocessingError::AugmentationFailed {
                    reason: format!("Failed to write pixel: {}", e),
                }
            })?;
        }
    }

    Ok(output)
}

/// Applies bilinear interpolation at the given coordinates
///
/// Returns 0.0 for out-of-bounds coordinates
///
/// # Errors
/// Returns an error if pixel access fails
fn bilinear_interpolate(input: &RasterBuffer, x: f64, y: f64) -> Result<f64> {
    // Check bounds
    if x < 0.0 || y < 0.0 || x >= input.width() as f64 - 1.0 || y >= input.height() as f64 - 1.0 {
        return Ok(0.0); // Fill with zeros for out-of-bounds
    }

    // Get the four surrounding pixels
    let x0 = x.floor() as u64;
    let y0 = y.floor() as u64;
    let x1 = x0 + 1;
    let y1 = y0 + 1;

    // Fractional parts
    let dx = x - x0 as f64;
    let dy = y - y0 as f64;

    // Get pixel values
    let p00 = input
        .get_pixel(x0, y0)
        .map_err(|e| PreprocessingError::AugmentationFailed {
            reason: format!("Failed to read pixel: {}", e),
        })?;
    let p10 = input
        .get_pixel(x1, y0)
        .map_err(|e| PreprocessingError::AugmentationFailed {
            reason: format!("Failed to read pixel: {}", e),
        })?;
    let p01 = input
        .get_pixel(x0, y1)
        .map_err(|e| PreprocessingError::AugmentationFailed {
            reason: format!("Failed to read pixel: {}", e),
        })?;
    let p11 = input
        .get_pixel(x1, y1)
        .map_err(|e| PreprocessingError::AugmentationFailed {
            reason: format!("Failed to read pixel: {}", e),
        })?;

    // Bilinear interpolation formula:
    // value = (1-dx)(1-dy)·p00 + dx(1-dy)·p10 + (1-dx)dy·p01 + dx·dy·p11
    let value = (1.0 - dx) * (1.0 - dy) * p00
        + dx * (1.0 - dy) * p10
        + (1.0 - dx) * dy * p01
        + dx * dy * p11;

    Ok(value)
}

/// Rotates image by 90 degrees multiple times
fn rotate_90_times(input: &RasterBuffer, times: usize) -> Result<RasterBuffer> {
    let mut current = input.clone();

    for _ in 0..times {
        current = rotate_90(&current)?;
    }

    Ok(current)
}

/// Rotates image by 90 degrees clockwise
fn rotate_90(input: &RasterBuffer) -> Result<RasterBuffer> {
    let mut output = RasterBuffer::zeros(input.height(), input.width(), input.data_type());

    for y in 0..input.height() {
        for x in 0..input.width() {
            let new_x = y;
            let new_y = input.width() - 1 - x;
            let value =
                input
                    .get_pixel(x, y)
                    .map_err(|e| PreprocessingError::AugmentationFailed {
                        reason: format!("Failed to read pixel: {}", e),
                    })?;
            output.set_pixel(new_x, new_y, value).map_err(|e| {
                PreprocessingError::AugmentationFailed {
                    reason: format!("Failed to write pixel: {}", e),
                }
            })?;
        }
    }

    Ok(output)
}

/// Applies random crop augmentation
///
/// # Errors
/// Returns an error if the operation fails
pub fn random_crop(
    input: &RasterBuffer,
    crop_width: u64,
    crop_height: u64,
) -> Result<RasterBuffer> {
    debug!("Applying random crop: {}x{}", crop_width, crop_height);

    if crop_width > input.width() || crop_height > input.height() {
        return Err(PreprocessingError::AugmentationFailed {
            reason: format!(
                "Crop size ({}x{}) larger than input ({}x{})",
                crop_width,
                crop_height,
                input.width(),
                input.height()
            ),
        }
        .into());
    }

    // Use SciRS2-Core RNG for random offset
    let mut rng = seeded_rng(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    );
    let max_x_offset = input.width() - crop_width;
    let max_y_offset = input.height() - crop_height;

    let x_offset = if max_x_offset > 0 {
        let random_val: f64 = rng.random();
        (random_val * (max_x_offset + 1) as f64) as u64
    } else {
        0
    };
    let y_offset = if max_y_offset > 0 {
        let random_val: f64 = rng.random();
        (random_val * (max_y_offset + 1) as f64) as u64
    } else {
        0
    };

    let mut output = RasterBuffer::zeros(crop_width, crop_height, input.data_type());

    for y in 0..crop_height {
        for x in 0..crop_width {
            let value = input.get_pixel(x + x_offset, y + y_offset).map_err(|e| {
                PreprocessingError::AugmentationFailed {
                    reason: format!("Failed to read pixel: {}", e),
                }
            })?;
            output
                .set_pixel(x, y, value)
                .map_err(|e| PreprocessingError::AugmentationFailed {
                    reason: format!("Failed to write pixel: {}", e),
                })?;
        }
    }

    Ok(output)
}

/// Adjusts brightness
///
/// # Errors
/// Returns an error if the operation fails
pub fn adjust_brightness(input: &RasterBuffer, delta: f32) -> Result<RasterBuffer> {
    debug!("Adjusting brightness by {}", delta);
    let mut output = input.clone();

    for y in 0..input.height() {
        for x in 0..input.width() {
            let value =
                input
                    .get_pixel(x, y)
                    .map_err(|e| PreprocessingError::AugmentationFailed {
                        reason: format!("Failed to read pixel: {}", e),
                    })?;
            let adjusted = (value as f32 + delta).clamp(0.0, 1.0) as f64;
            output.set_pixel(x, y, adjusted).map_err(|e| {
                PreprocessingError::AugmentationFailed {
                    reason: format!("Failed to write pixel: {}", e),
                }
            })?;
        }
    }

    Ok(output)
}

/// Adjusts contrast
///
/// # Errors
/// Returns an error if the operation fails
pub fn adjust_contrast(input: &RasterBuffer, factor: f32) -> Result<RasterBuffer> {
    debug!("Adjusting contrast by factor {}", factor);
    let mut output = input.clone();

    // Calculate mean
    let mut sum = 0.0;
    let mut count = 0u64;
    for y in 0..input.height() {
        for x in 0..input.width() {
            let value =
                input
                    .get_pixel(x, y)
                    .map_err(|e| PreprocessingError::AugmentationFailed {
                        reason: format!("Failed to read pixel: {}", e),
                    })?;
            sum += value;
            count += 1;
        }
    }
    let mean = sum / count as f64;

    // Adjust contrast
    for y in 0..input.height() {
        for x in 0..input.width() {
            let value =
                input
                    .get_pixel(x, y)
                    .map_err(|e| PreprocessingError::AugmentationFailed {
                        reason: format!("Failed to read pixel: {}", e),
                    })?;
            let adjusted = (mean + (value - mean) * factor as f64).clamp(0.0, 1.0);
            output.set_pixel(x, y, adjusted).map_err(|e| {
                PreprocessingError::AugmentationFailed {
                    reason: format!("Failed to write pixel: {}", e),
                }
            })?;
        }
    }

    Ok(output)
}

/// Adds Gaussian noise
///
/// # Errors
/// Returns an error if the operation fails
pub fn add_gaussian_noise(input: &RasterBuffer, std_dev: f32) -> Result<RasterBuffer> {
    debug!("Adding Gaussian noise with std_dev={}", std_dev);
    let mut output = input.clone();

    // Use SciRS2-Core RNG with Box-Muller transform for Gaussian noise
    let mut rng = seeded_rng(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    );

    for y in 0..input.height() {
        for x in 0..input.width() {
            let value =
                input
                    .get_pixel(x, y)
                    .map_err(|e| PreprocessingError::AugmentationFailed {
                        reason: format!("Failed to read pixel: {}", e),
                    })?;

            // Generate Gaussian noise using Box-Muller transform
            let noise = gaussian_random(&mut rng, 0.0, std_dev as f64);
            let noisy = (value + noise).clamp(0.0, 1.0);

            output
                .set_pixel(x, y, noisy)
                .map_err(|e| PreprocessingError::AugmentationFailed {
                    reason: format!("Failed to write pixel: {}", e),
                })?;
        }
    }

    Ok(output)
}

/// Applies Gaussian blur
///
/// Uses separable Gaussian convolution for efficiency (O(n·k) instead of O(n·k²)).
/// Applies horizontal blur pass followed by vertical blur pass.
/// Edges are handled with mirror padding.
///
/// # Errors
/// Returns an error if the operation fails
pub fn gaussian_blur(input: &RasterBuffer, kernel_size: usize) -> Result<RasterBuffer> {
    debug!("Applying Gaussian blur with kernel size {}", kernel_size);

    if kernel_size % 2 == 0 {
        return Err(PreprocessingError::AugmentationFailed {
            reason: "Kernel size must be odd".to_string(),
        }
        .into());
    }

    if kernel_size < 3 {
        return Err(PreprocessingError::AugmentationFailed {
            reason: "Kernel size must be at least 3".to_string(),
        }
        .into());
    }

    // Calculate sigma from kernel size using standard formula
    // sigma = 0.3 * ((kernel_size - 1) * 0.5 - 1) + 0.8
    let sigma = 0.3 * ((kernel_size as f64 - 1.0) * 0.5 - 1.0) + 0.8;

    // Generate 1D Gaussian kernel
    let kernel = generate_gaussian_kernel(kernel_size, sigma)?;

    // Apply horizontal blur
    let horizontal = apply_horizontal_blur(input, &kernel)?;

    // Apply vertical blur
    apply_vertical_blur(&horizontal, &kernel)
}

/// Generates a 1D Gaussian kernel
///
/// Uses the formula: G(x) = exp(-x²/(2σ²)) / √(2πσ²)
/// Normalizes the kernel to sum to 1.0
///
/// # Errors
/// Returns an error if sigma is invalid
fn generate_gaussian_kernel(size: usize, sigma: f64) -> Result<Vec<f64>> {
    if sigma <= 0.0 {
        return Err(PreprocessingError::AugmentationFailed {
            reason: "Sigma must be positive".to_string(),
        }
        .into());
    }

    let radius = (size / 2) as i32;
    let mut kernel = Vec::with_capacity(size);
    let mut sum = 0.0;

    // Generate Gaussian values
    for i in -radius..=radius {
        let x = i as f64;
        let value = (-x * x / (2.0 * sigma * sigma)).exp();
        kernel.push(value);
        sum += value;
    }

    // Normalize to sum to 1.0
    for value in &mut kernel {
        *value /= sum;
    }

    Ok(kernel)
}

/// Applies horizontal blur using the given kernel
///
/// Uses mirror padding at image edges
///
/// # Errors
/// Returns an error if pixel access fails
fn apply_horizontal_blur(input: &RasterBuffer, kernel: &[f64]) -> Result<RasterBuffer> {
    let width = input.width();
    let height = input.height();
    let radius = (kernel.len() / 2) as i64;

    let mut output = RasterBuffer::zeros(width, height, input.data_type());

    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0;

            // Apply convolution
            for (k_idx, &k_val) in kernel.iter().enumerate() {
                let offset = k_idx as i64 - radius;
                let src_x = x as i64 + offset;

                // Mirror padding for edges
                let safe_x = if src_x < 0 {
                    (-src_x) as u64
                } else if src_x >= width as i64 {
                    (2 * width as i64 - src_x - 2) as u64
                } else {
                    src_x as u64
                };

                // Clamp to valid range
                let clamped_x = safe_x.min(width - 1);

                let pixel = input.get_pixel(clamped_x, y).map_err(|e| {
                    PreprocessingError::AugmentationFailed {
                        reason: format!("Failed to read pixel: {}", e),
                    }
                })?;

                sum += pixel * k_val;
            }

            output
                .set_pixel(x, y, sum)
                .map_err(|e| PreprocessingError::AugmentationFailed {
                    reason: format!("Failed to write pixel: {}", e),
                })?;
        }
    }

    Ok(output)
}

/// Applies vertical blur using the given kernel
///
/// Uses mirror padding at image edges
///
/// # Errors
/// Returns an error if pixel access fails
fn apply_vertical_blur(input: &RasterBuffer, kernel: &[f64]) -> Result<RasterBuffer> {
    let width = input.width();
    let height = input.height();
    let radius = (kernel.len() / 2) as i64;

    let mut output = RasterBuffer::zeros(width, height, input.data_type());

    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0;

            // Apply convolution
            for (k_idx, &k_val) in kernel.iter().enumerate() {
                let offset = k_idx as i64 - radius;
                let src_y = y as i64 + offset;

                // Mirror padding for edges
                let safe_y = if src_y < 0 {
                    (-src_y) as u64
                } else if src_y >= height as i64 {
                    (2 * height as i64 - src_y - 2) as u64
                } else {
                    src_y as u64
                };

                // Clamp to valid range
                let clamped_y = safe_y.min(height - 1);

                let pixel = input.get_pixel(x, clamped_y).map_err(|e| {
                    PreprocessingError::AugmentationFailed {
                        reason: format!("Failed to read pixel: {}", e),
                    }
                })?;

                sum += pixel * k_val;
            }

            output
                .set_pixel(x, y, sum)
                .map_err(|e| PreprocessingError::AugmentationFailed {
                    reason: format!("Failed to write pixel: {}", e),
                })?;
        }
    }

    Ok(output)
}

/// Applies a sequence of augmentations according to configuration
///
/// # Errors
/// Returns an error if any augmentation fails
pub fn apply_augmentation(
    input: &RasterBuffer,
    config: &AugmentationConfig,
) -> Result<Vec<RasterBuffer>> {
    let mut augmented = vec![input.clone()]; // Original

    // Horizontal flip
    if config.horizontal_flip {
        augmented.push(horizontal_flip(input)?);
    }

    // Vertical flip
    if config.vertical_flip {
        augmented.push(vertical_flip(input)?);
    }

    // Rotations
    for angle in &config.rotation_angles {
        augmented.push(rotate(input, *angle)?);
    }

    // Brightness adjustments
    if let Some((min, max)) = config.brightness_range {
        augmented.push(adjust_brightness(input, min)?);
        augmented.push(adjust_brightness(input, max)?);
    }

    // Contrast adjustments
    if let Some((min, max)) = config.contrast_range {
        augmented.push(adjust_contrast(input, min)?);
        augmented.push(adjust_contrast(input, max)?);
    }

    // Noise
    if let Some(std_dev) = config.gaussian_noise {
        augmented.push(add_gaussian_noise(input, std_dev)?);
    }

    // Blur
    if let Some(kernel_size) = config.blur_kernel {
        augmented.push(gaussian_blur(input, kernel_size)?);
    }

    Ok(augmented)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_augmentation_config_builder() {
        let config = AugmentationConfig::builder()
            .horizontal_flip(true)
            .vertical_flip(true)
            .rotation_angles(vec![90.0, 180.0])
            .brightness_range(-0.2, 0.2)
            .build();

        assert!(config.horizontal_flip);
        assert!(config.vertical_flip);
        assert_eq!(config.rotation_angles.len(), 2);
    }

    #[test]
    fn test_standard_config() {
        let config = AugmentationConfig::standard();
        assert!(config.horizontal_flip);
        assert!(config.brightness_range.is_some());
    }

    #[test]
    fn test_aggressive_config() {
        let config = AugmentationConfig::aggressive();
        assert!(config.random_crop);
        assert!(config.salt_pepper_noise.is_some());
        assert!(config.rotation_angles.len() > 3);
    }

    #[test]
    fn test_horizontal_flip() {
        let input = RasterBuffer::zeros(4, 4, RasterDataType::Float32);
        let result = horizontal_flip(&input);
        assert!(result.is_ok());

        let output = result.expect("Should succeed");
        assert_eq!(output.width(), input.width());
        assert_eq!(output.height(), input.height());
    }

    #[test]
    fn test_vertical_flip() {
        let input = RasterBuffer::zeros(4, 4, RasterDataType::Float32);
        let result = vertical_flip(&input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_rotate_90() {
        let input = RasterBuffer::zeros(4, 4, RasterDataType::Float32);
        let result = rotate(&input, 90.0);
        assert!(result.is_ok());

        let output = result.expect("Should succeed");
        assert_eq!(output.width(), input.height());
        assert_eq!(output.height(), input.width());
    }

    #[test]
    fn test_random_crop() {
        let input = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
        let result = random_crop(&input, 64, 64);
        assert!(result.is_ok());

        let output = result.expect("Should succeed");
        assert_eq!(output.width(), 64);
        assert_eq!(output.height(), 64);
    }

    #[test]
    fn test_random_crop_invalid_size() {
        let input = RasterBuffer::zeros(50, 50, RasterDataType::Float32);
        let result = random_crop(&input, 100, 100);
        assert!(result.is_err());
    }

    #[test]
    fn test_rotate_45_degrees() {
        // Test rotation at 45 degrees
        let mut input = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Set a specific pixel to test rotation
        let _ = input.set_pixel(5, 5, 1.0);

        let result = rotate(&input, 45.0);
        assert!(result.is_ok());

        let output = result.expect("Should succeed");
        // Output dimensions should be larger to accommodate rotation
        assert!(output.width() > input.width() || output.height() > input.height());
    }

    #[test]
    fn test_rotate_180_degrees() {
        // Test rotation at 180 degrees (should use optimized path)
        let mut input = RasterBuffer::zeros(8, 8, RasterDataType::Float32);

        // Set corner pixels
        let _ = input.set_pixel(0, 0, 1.0);
        let _ = input.set_pixel(7, 7, 0.5);

        let result = rotate(&input, 180.0);
        assert!(result.is_ok());

        let output = result.expect("Should succeed");
        assert_eq!(output.width(), input.width());
        assert_eq!(output.height(), input.height());

        // Check that corners are swapped
        let top_left = output.get_pixel(0, 0).expect("Should succeed");
        let bottom_right = output.get_pixel(7, 7).expect("Should succeed");

        assert!((top_left - 0.5).abs() < 0.01, "Top-left should be ~0.5");
        assert!(
            (bottom_right - 1.0).abs() < 0.01,
            "Bottom-right should be ~1.0"
        );
    }

    #[test]
    fn test_rotate_arbitrary_angle() {
        // Test rotation at arbitrary angle
        let input = RasterBuffer::zeros(16, 16, RasterDataType::Float32);

        let result = rotate(&input, 30.0);
        assert!(result.is_ok());

        let output = result.expect("Should succeed");
        // Verify output is created (dimensions may vary)
        assert!(output.width() > 0);
        assert!(output.height() > 0);
    }

    #[test]
    fn test_bilinear_interpolation() {
        // Test bilinear interpolation function
        let mut input = RasterBuffer::zeros(4, 4, RasterDataType::Float32);

        // Set up a gradient
        for y in 0..4 {
            for x in 0..4 {
                let value = (x + y) as f64 * 0.1;
                let _ = input.set_pixel(x, y, value);
            }
        }

        // Test interpolation at fractional coordinates
        let result = bilinear_interpolate(&input, 1.5, 1.5);
        assert!(result.is_ok());

        let value = result.expect("Should succeed");
        // Value should be between surrounding pixels
        assert!(
            (0.2..=0.4).contains(&value),
            "Value {} not in expected range",
            value
        );
    }

    #[test]
    fn test_bilinear_interpolation_out_of_bounds() {
        let input = RasterBuffer::zeros(4, 4, RasterDataType::Float32);

        // Test out-of-bounds coordinates
        let result = bilinear_interpolate(&input, -1.0, 2.0);
        assert!(result.is_ok());

        let value = result.expect("Should succeed");
        assert_eq!(value, 0.0, "Out-of-bounds should return 0.0");
    }

    #[test]
    fn test_gaussian_blur_basic() {
        // Test basic Gaussian blur
        let input = RasterBuffer::zeros(16, 16, RasterDataType::Float32);

        let result = gaussian_blur(&input, 3);
        assert!(result.is_ok());

        let output = result.expect("Should succeed");
        assert_eq!(output.width(), input.width());
        assert_eq!(output.height(), input.height());
    }

    #[test]
    fn test_gaussian_blur_larger_kernel() {
        // Test with larger kernel
        let input = RasterBuffer::zeros(20, 20, RasterDataType::Float32);

        let result = gaussian_blur(&input, 5);
        assert!(result.is_ok());

        let output = result.expect("Should succeed");
        assert_eq!(output.width(), input.width());
        assert_eq!(output.height(), input.height());
    }

    #[test]
    fn test_gaussian_blur_even_kernel_fails() {
        // Test that even kernel size fails
        let input = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        let result = gaussian_blur(&input, 4);
        assert!(result.is_err());
    }

    #[test]
    fn test_gaussian_blur_too_small_kernel_fails() {
        // Test that kernel size < 3 fails
        let input = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        let result = gaussian_blur(&input, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_gaussian_blur_smoothing() {
        // Test that blur actually smooths the image
        let mut input = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Create a sharp edge
        for y in 0..10 {
            for x in 0..5 {
                let _ = input.set_pixel(x, y, 1.0);
            }
        }

        let result = gaussian_blur(&input, 3);
        assert!(result.is_ok());

        let output = result.expect("Should succeed");

        // Check that the edge is smoothed (pixel at boundary should be between 0 and 1)
        let edge_value = output.get_pixel(4, 5).expect("Should succeed");
        assert!(
            edge_value > 0.4 && edge_value < 0.85,
            "Edge value {} should be smoothed",
            edge_value
        );

        // Check that the transition area has smoothing
        let inside_value = output.get_pixel(2, 5).expect("Should succeed");
        let outside_value = output.get_pixel(7, 5).expect("Should succeed");

        // Inside should be close to 1, outside should be close to 0
        assert!(
            inside_value > 0.9,
            "Inside value {} should be high",
            inside_value
        );
        assert!(
            outside_value < 0.15,
            "Outside value {} should be low",
            outside_value
        );
    }

    #[test]
    fn test_generate_gaussian_kernel() {
        // Test kernel generation
        let result = generate_gaussian_kernel(5, 1.0);
        assert!(result.is_ok());

        let kernel = result.expect("Should succeed");
        assert_eq!(kernel.len(), 5);

        // Kernel should sum to approximately 1.0
        let sum: f64 = kernel.iter().sum();
        assert!(
            (sum - 1.0).abs() < 1e-10,
            "Kernel sum {} should be ~1.0",
            sum
        );

        // Kernel should be symmetric
        assert!((kernel[0] - kernel[4]).abs() < 1e-10);
        assert!((kernel[1] - kernel[3]).abs() < 1e-10);

        // Center should be largest value
        assert!(kernel[2] > kernel[1]);
        assert!(kernel[2] > kernel[0]);
    }

    #[test]
    fn test_generate_gaussian_kernel_invalid_sigma() {
        // Test that invalid sigma fails
        let result = generate_gaussian_kernel(5, 0.0);
        assert!(result.is_err());

        let result = generate_gaussian_kernel(5, -1.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_horizontal_blur() {
        // Test horizontal blur function
        let mut input = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Set middle row to 1.0
        for x in 0..10 {
            let _ = input.set_pixel(x, 5, 1.0);
        }

        let kernel = vec![0.25, 0.5, 0.25]; // Simple box-like kernel
        let result = apply_horizontal_blur(&input, &kernel);
        assert!(result.is_ok());

        let output = result.expect("Should succeed");

        // Middle row should still be high, but slightly smoothed
        let value = output.get_pixel(5, 5).expect("Should succeed");
        assert!(value > 0.4, "Value {} should be high", value);
    }

    #[test]
    fn test_vertical_blur() {
        // Test vertical blur function
        let mut input = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Set middle column to 1.0
        for y in 0..10 {
            let _ = input.set_pixel(5, y, 1.0);
        }

        let kernel = vec![0.25, 0.5, 0.25]; // Simple box-like kernel
        let result = apply_vertical_blur(&input, &kernel);
        assert!(result.is_ok());

        let output = result.expect("Should succeed");

        // Middle column should still be high, but slightly smoothed
        let value = output.get_pixel(5, 5).expect("Should succeed");
        assert!(value > 0.4, "Value {} should be high", value);
    }

    #[test]
    fn test_blur_edge_handling() {
        // Test that edge handling works correctly
        let mut input = RasterBuffer::zeros(5, 5, RasterDataType::Float32);

        // Set corner pixel
        let _ = input.set_pixel(0, 0, 1.0);

        let result = gaussian_blur(&input, 3);
        assert!(result.is_ok());

        let output = result.expect("Should succeed");

        // Corner should be smoothed but not zero (due to mirror padding)
        let corner = output.get_pixel(0, 0).expect("Should succeed");
        assert!(
            corner > 0.0,
            "Corner should be non-zero due to mirror padding"
        );
    }
}
