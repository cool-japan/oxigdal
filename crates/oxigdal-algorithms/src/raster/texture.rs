//! Texture analysis using Gray-Level Co-occurrence Matrix (GLCM)
//!
//! This module provides texture analysis algorithms based on GLCM and Haralick features.
//! These features quantify the spatial arrangement of pixel intensities and are widely
//! used in image classification, segmentation, and pattern recognition.
//!
//! # GLCM (Gray-Level Co-occurrence Matrix)
//!
//! The GLCM is a statistical method of examining texture that considers the spatial
//! relationship of pixels. It calculates how often pairs of pixels with specific values
//! occur in a specified spatial relationship.
//!
//! # Haralick Features
//!
//! Haralick features are statistics computed from the GLCM that capture different
//! aspects of texture:
//!
//! - **Contrast**: Measures local intensity variation
//! - **Correlation**: Measures linear dependency of gray levels
//! - **Energy (Angular Second Moment)**: Measures textural uniformity
//! - **Homogeneity (Inverse Difference Moment)**: Measures smoothness
//! - **Entropy**: Measures randomness/complexity
//! - **Dissimilarity**: Similar to contrast but with linear weighting
//! - **Variance**: Measures dispersion around the mean
//! - **Sum Average**: Average of sum of probabilities
//! - **Sum Entropy**: Entropy of sum probabilities
//! - **Difference Entropy**: Entropy of difference probabilities
//! - **Information Measure of Correlation**: Mutual information measures
//! - **Maximum Correlation Coefficient**: Maximum correlation in GLCM
//!
//! # Example
//!
//! ```ignore
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use oxigdal_algorithms::raster::texture::{
//!     compute_glcm, compute_haralick_features, Direction, GlcmParams
//! };
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! let src = RasterBuffer::zeros(100, 100, RasterDataType::UInt8);
//! let params = GlcmParams::default();
//!
//! // Compute GLCM for horizontal direction
//! let glcm = compute_glcm(&src, Direction::Horizontal, 1, &params)?;
//!
//! // Compute Haralick features
//! let features = compute_haralick_features(&glcm);
//! println!("Contrast: {}", features.contrast);
//! println!("Energy: {}", features.energy);
//! # Ok(())
//! # }
//! ```

use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

/// Direction for GLCM computation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Horizontal (0°)
    Horizontal,

    /// Vertical (90°)
    Vertical,

    /// Diagonal down-right (45°)
    Diagonal45,

    /// Diagonal down-left (135°)
    Diagonal135,

    /// Custom direction (dx, dy)
    Custom(i64, i64),
}

impl Direction {
    /// Gets the offset for this direction
    #[must_use]
    pub fn offset(&self, distance: u32) -> (i64, i64) {
        let d = distance as i64;
        match self {
            Self::Horizontal => (d, 0),
            Self::Vertical => (0, d),
            Self::Diagonal45 => (d, -d),
            Self::Diagonal135 => (d, d),
            Self::Custom(dx, dy) => (*dx * d, *dy * d),
        }
    }

    /// Returns all standard directions
    #[must_use]
    pub fn all_standard() -> Vec<Self> {
        vec![
            Self::Horizontal,
            Self::Vertical,
            Self::Diagonal45,
            Self::Diagonal135,
        ]
    }
}

/// Parameters for GLCM computation
#[derive(Debug, Clone)]
pub struct GlcmParams {
    /// Number of gray levels to use (quantization)
    pub gray_levels: usize,

    /// Whether to normalize the GLCM
    pub normalize: bool,

    /// Whether to make the GLCM symmetric
    pub symmetric: bool,

    /// Window size for local GLCM computation (None = global)
    pub window_size: Option<usize>,
}

impl Default for GlcmParams {
    fn default() -> Self {
        Self {
            gray_levels: 256,
            normalize: true,
            symmetric: true,
            window_size: None,
        }
    }
}

/// Gray-Level Co-occurrence Matrix
#[derive(Debug, Clone)]
pub struct Glcm {
    /// The co-occurrence matrix (gray_levels x gray_levels)
    matrix: Vec<Vec<f64>>,

    /// Number of gray levels
    gray_levels: usize,

    /// Direction used
    direction: Direction,

    /// Distance used
    distance: u32,

    /// Whether the matrix is normalized
    normalized: bool,
}

impl Glcm {
    /// Creates a new GLCM with given size
    #[must_use]
    pub fn new(gray_levels: usize, direction: Direction, distance: u32) -> Self {
        Self {
            matrix: vec![vec![0.0; gray_levels]; gray_levels],
            gray_levels,
            direction,
            distance,
            normalized: false,
        }
    }

    /// Gets the value at position (i, j)
    #[must_use]
    pub fn get(&self, i: usize, j: usize) -> f64 {
        if i < self.gray_levels && j < self.gray_levels {
            self.matrix[i][j]
        } else {
            0.0
        }
    }

    /// Sets the value at position (i, j)
    pub fn set(&mut self, i: usize, j: usize, value: f64) {
        if i < self.gray_levels && j < self.gray_levels {
            self.matrix[i][j] = value;
        }
    }

    /// Increments the value at position (i, j)
    pub fn increment(&mut self, i: usize, j: usize) {
        if i < self.gray_levels && j < self.gray_levels {
            self.matrix[i][j] += 1.0;
        }
    }

    /// Normalizes the GLCM
    pub fn normalize(&mut self) {
        let sum: f64 = self.matrix.iter().flat_map(|row| row.iter()).sum();

        if sum > 0.0 {
            for row in &mut self.matrix {
                for val in row {
                    *val /= sum;
                }
            }
            self.normalized = true;
        }
    }

    /// Makes the GLCM symmetric
    pub fn make_symmetric(&mut self) {
        for i in 0..self.gray_levels {
            for j in 0..self.gray_levels {
                let avg = (self.matrix[i][j] + self.matrix[j][i]) / 2.0;
                self.matrix[i][j] = avg;
                self.matrix[j][i] = avg;
            }
        }
    }

    /// Returns the gray levels
    #[must_use]
    pub fn gray_levels(&self) -> usize {
        self.gray_levels
    }

    /// Returns the direction
    #[must_use]
    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// Returns the distance
    #[must_use]
    pub fn distance(&self) -> u32 {
        self.distance
    }

    /// Returns whether normalized
    #[must_use]
    pub fn is_normalized(&self) -> bool {
        self.normalized
    }

    /// Returns a reference to the matrix
    #[must_use]
    pub fn matrix(&self) -> &[Vec<f64>] {
        &self.matrix
    }
}

/// Haralick texture features computed from GLCM
#[derive(Debug, Clone, Default)]
pub struct HaralickFeatures {
    /// Contrast (variance of differences)
    pub contrast: f64,

    /// Correlation (linear dependency)
    pub correlation: f64,

    /// Energy/Angular Second Moment (uniformity)
    pub energy: f64,

    /// Homogeneity/Inverse Difference Moment (smoothness)
    pub homogeneity: f64,

    /// Entropy (randomness)
    pub entropy: f64,

    /// Dissimilarity (linear contrast)
    pub dissimilarity: f64,

    /// Variance
    pub variance: f64,

    /// Sum average
    pub sum_average: f64,

    /// Sum entropy
    pub sum_entropy: f64,

    /// Difference entropy
    pub difference_entropy: f64,

    /// Information measure of correlation 1
    pub info_measure_corr1: f64,

    /// Information measure of correlation 2
    pub info_measure_corr2: f64,

    /// Maximum correlation coefficient
    pub max_correlation_coeff: f64,

    /// Cluster shade
    pub cluster_shade: f64,

    /// Cluster prominence
    pub cluster_prominence: f64,
}

/// Computes the Gray-Level Co-occurrence Matrix
///
/// # Arguments
///
/// * `src` - Source raster buffer
/// * `direction` - Direction to compute co-occurrence
/// * `distance` - Distance between pixel pairs
/// * `params` - GLCM parameters
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn compute_glcm(
    src: &RasterBuffer,
    direction: Direction,
    distance: u32,
    params: &GlcmParams,
) -> Result<Glcm> {
    let width = src.width();
    let height = src.height();

    if params.gray_levels == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "gray_levels",
            message: "Gray levels must be greater than zero".to_string(),
        });
    }

    // Quantize image to specified gray levels
    let quantized = quantize_image(src, params.gray_levels)?;

    let (dx, dy) = direction.offset(distance);
    let mut glcm = Glcm::new(params.gray_levels, direction, distance);

    // Compute co-occurrence matrix
    for y in 0..height {
        for x in 0..width {
            let nx = x as i64 + dx;
            let ny = y as i64 + dy;

            if nx >= 0 && nx < width as i64 && ny >= 0 && ny < height as i64 {
                let i = quantized.get_pixel(x, y).map_err(AlgorithmError::Core)? as usize;
                let j = quantized
                    .get_pixel(nx as u64, ny as u64)
                    .map_err(AlgorithmError::Core)? as usize;

                glcm.increment(i, j);
            }
        }
    }

    // Make symmetric if requested
    if params.symmetric {
        glcm.make_symmetric();
    }

    // Normalize if requested
    if params.normalize {
        glcm.normalize();
    }

    Ok(glcm)
}

/// Computes GLCM for multiple directions and averages
///
/// # Arguments
///
/// * `src` - Source raster buffer
/// * `directions` - Directions to compute
/// * `distance` - Distance between pixel pairs
/// * `params` - GLCM parameters
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn compute_glcm_multi_direction(
    src: &RasterBuffer,
    directions: &[Direction],
    distance: u32,
    params: &GlcmParams,
) -> Result<Glcm> {
    if directions.is_empty() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "directions",
            message: "At least one direction required".to_string(),
        });
    }

    let mut avg_glcm = Glcm::new(params.gray_levels, directions[0], distance);

    for direction in directions {
        let glcm = compute_glcm(src, *direction, distance, params)?;

        for i in 0..params.gray_levels {
            for j in 0..params.gray_levels {
                let val = avg_glcm.get(i, j) + glcm.get(i, j);
                avg_glcm.set(i, j, val);
            }
        }
    }

    // Average
    for i in 0..params.gray_levels {
        for j in 0..params.gray_levels {
            let val = avg_glcm.get(i, j) / directions.len() as f64;
            avg_glcm.set(i, j, val);
        }
    }

    Ok(avg_glcm)
}

/// Computes Haralick texture features from a GLCM
///
/// # Arguments
///
/// * `glcm` - Gray-Level Co-occurrence Matrix
///
/// # Returns
///
/// Haralick features structure with all computed features
#[must_use]
pub fn compute_haralick_features(glcm: &Glcm) -> HaralickFeatures {
    let n = glcm.gray_levels();
    let matrix = glcm.matrix();

    // Compute marginal probabilities
    let mut px = vec![0.0; n];
    let mut py = vec![0.0; n];

    for i in 0..n {
        for j in 0..n {
            px[i] += matrix[i][j];
            py[j] += matrix[i][j];
        }
    }

    // Compute means and standard deviations
    let mut mu_x = 0.0;
    let mut mu_y = 0.0;

    for i in 0..n {
        mu_x += i as f64 * px[i];
        mu_y += i as f64 * py[i];
    }

    let mut sigma_x = 0.0;
    let mut sigma_y = 0.0;

    for i in 0..n {
        sigma_x += (i as f64 - mu_x).powi(2) * px[i];
        sigma_y += (i as f64 - mu_y).powi(2) * py[i];
    }

    sigma_x = sigma_x.sqrt();
    sigma_y = sigma_y.sqrt();

    // Compute sum and difference probabilities
    let max_sum = 2 * (n - 1);
    let mut p_x_plus_y = vec![0.0; max_sum + 1];
    let mut p_x_minus_y = vec![0.0; n];

    for i in 0..n {
        for j in 0..n {
            p_x_plus_y[i + j] += matrix[i][j];
            let diff = (i as i64 - j as i64).unsigned_abs() as usize;
            p_x_minus_y[diff] += matrix[i][j];
        }
    }

    let mut features = HaralickFeatures::default();

    // 1. Contrast
    features.contrast = (0..n).map(|k| k.pow(2) as f64 * p_x_minus_y[k]).sum();

    // 2. Correlation
    if sigma_x > 0.0 && sigma_y > 0.0 {
        features.correlation = (0..n)
            .flat_map(|i| {
                (0..n).map(move |j| {
                    (i as f64 - mu_x) * (j as f64 - mu_y) * matrix[i][j] / (sigma_x * sigma_y)
                })
            })
            .sum();
    }

    // 3. Energy (Angular Second Moment)
    features.energy = (0..n)
        .flat_map(|i| (0..n).map(move |j| matrix[i][j].powi(2)))
        .sum();

    // 4. Homogeneity (Inverse Difference Moment)
    features.homogeneity = (0..n)
        .flat_map(|i| (0..n).map(move |j| matrix[i][j] / (1.0 + (i as f64 - j as f64).powi(2))))
        .sum();

    // 5. Entropy
    features.entropy = -(0..n)
        .flat_map(|i| {
            (0..n).map(move |j| {
                let p = matrix[i][j];
                if p > 0.0 { p * p.ln() } else { 0.0 }
            })
        })
        .sum::<f64>();

    // 6. Dissimilarity
    features.dissimilarity = (0..n)
        .flat_map(|i| (0..n).map(move |j| (i as f64 - j as f64).abs() * matrix[i][j]))
        .sum();

    // 7. Variance
    let mu = (0..n)
        .flat_map(|i| (0..n).map(move |j| (i + j) as f64 * matrix[i][j]))
        .sum::<f64>()
        / 2.0;

    features.variance = (0..n)
        .flat_map(|i| (0..n).map(move |j| ((i + j) as f64 / 2.0 - mu).powi(2) * matrix[i][j]))
        .sum();

    // 8. Sum Average
    features.sum_average = (0..=max_sum).map(|k| k as f64 * p_x_plus_y[k]).sum();

    // 9. Sum Entropy
    features.sum_entropy = -(0..=max_sum)
        .map(|k| {
            let p = p_x_plus_y[k];
            if p > 0.0 { p * p.ln() } else { 0.0 }
        })
        .sum::<f64>();

    // 10. Difference Entropy
    features.difference_entropy = -(0..n)
        .map(|k| {
            let p = p_x_minus_y[k];
            if p > 0.0 { p * p.ln() } else { 0.0 }
        })
        .sum::<f64>();

    // 11. Information Measures of Correlation
    let hx = -px
        .iter()
        .map(|&p| if p > 0.0 { p * p.ln() } else { 0.0 })
        .sum::<f64>();

    let hy = -py
        .iter()
        .map(|&p| if p > 0.0 { p * p.ln() } else { 0.0 })
        .sum::<f64>();

    let hxy = features.entropy;

    let px_clone = px.clone();
    let py_clone = py.clone();
    let hxy1 = -(0..n)
        .flat_map(|i| {
            let px = px_clone.clone();
            let py = py_clone.clone();
            (0..n).map(move |j| {
                let p = matrix[i][j];
                if p > 0.0 && px[i] > 0.0 && py[j] > 0.0 {
                    p * (px[i] * py[j]).ln()
                } else {
                    0.0
                }
            })
        })
        .sum::<f64>();

    let hxy2 = -(0..n)
        .flat_map(|i| {
            let px = px.clone();
            let py = py.clone();
            (0..n).map(move |j| {
                let p = px[i] * py[j];
                if p > 0.0 { p * p.ln() } else { 0.0 }
            })
        })
        .sum::<f64>();

    let max_hxy = hx.max(hy);
    if max_hxy > 0.0 {
        features.info_measure_corr1 = (hxy - hxy1) / max_hxy;
    }

    if hxy2 > hxy {
        features.info_measure_corr2 = (1.0 - (-2.0 * (hxy2 - hxy)).exp()).sqrt();
    }

    // 12. Maximum Correlation Coefficient
    // This is computationally expensive (requires eigenvalues)
    // Simplified approximation
    features.max_correlation_coeff = features.correlation.abs();

    // 13. Cluster Shade
    features.cluster_shade = (0..n)
        .flat_map(|i| {
            (0..n).map(move |j| (i as f64 + j as f64 - mu_x - mu_y).powi(3) * matrix[i][j])
        })
        .sum();

    // 14. Cluster Prominence
    features.cluster_prominence = (0..n)
        .flat_map(|i| {
            (0..n).map(move |j| (i as f64 + j as f64 - mu_x - mu_y).powi(4) * matrix[i][j])
        })
        .sum();

    features
}

/// Computes a texture feature image for a single feature
///
/// # Arguments
///
/// * `src` - Source raster buffer
/// * `feature_name` - Name of feature to compute
/// * `direction` - Direction for GLCM
/// * `distance` - Distance for GLCM
/// * `window_size` - Size of moving window
/// * `params` - GLCM parameters
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn compute_texture_feature_image(
    src: &RasterBuffer,
    feature_name: &str,
    direction: Direction,
    distance: u32,
    window_size: usize,
    params: &GlcmParams,
) -> Result<RasterBuffer> {
    if window_size % 2 == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "window_size",
            message: "Window size must be odd".to_string(),
        });
    }

    let width = src.width();
    let height = src.height();
    let mut dst = RasterBuffer::zeros(width, height, oxigdal_core::types::RasterDataType::Float64);

    let hw = (window_size / 2) as i64;

    #[cfg(feature = "parallel")]
    {
        let results: Result<Vec<_>> = (hw as u64..(height - hw as u64))
            .into_par_iter()
            .map(|y| {
                let mut row_data = Vec::new();
                for x in hw as u64..(width - hw as u64) {
                    let value = compute_local_texture_feature(
                        src,
                        x,
                        y,
                        window_size,
                        feature_name,
                        direction,
                        distance,
                        params,
                    )?;
                    row_data.push((x, value));
                }
                Ok((y, row_data))
            })
            .collect();

        for (y, row_data) in results? {
            for (x, value) in row_data {
                dst.set_pixel(x, y, value).map_err(AlgorithmError::Core)?;
            }
        }
    }

    #[cfg(not(feature = "parallel"))]
    {
        for y in hw as u64..(height - hw as u64) {
            for x in hw as u64..(width - hw as u64) {
                let value = compute_local_texture_feature(
                    src,
                    x,
                    y,
                    window_size,
                    feature_name,
                    direction,
                    distance,
                    params,
                )?;
                dst.set_pixel(x, y, value).map_err(AlgorithmError::Core)?;
            }
        }
    }

    Ok(dst)
}

/// Computes texture feature for a local window
fn compute_local_texture_feature(
    src: &RasterBuffer,
    cx: u64,
    cy: u64,
    window_size: usize,
    feature_name: &str,
    direction: Direction,
    distance: u32,
    params: &GlcmParams,
) -> Result<f64> {
    use oxigdal_core::types::RasterDataType;

    let hw = (window_size / 2) as i64;

    // Extract window
    let mut window = RasterBuffer::zeros(
        window_size as u64,
        window_size as u64,
        RasterDataType::Float64,
    );

    for wy in 0..window_size {
        for wx in 0..window_size {
            let sx = (cx as i64 + wx as i64 - hw) as u64;
            let sy = (cy as i64 + wy as i64 - hw) as u64;
            let val = src.get_pixel(sx, sy).map_err(AlgorithmError::Core)?;
            window
                .set_pixel(wx as u64, wy as u64, val)
                .map_err(AlgorithmError::Core)?;
        }
    }

    // Compute GLCM for window
    let glcm = compute_glcm(&window, direction, distance, params)?;
    let features = compute_haralick_features(&glcm);

    // Extract requested feature
    let value = match feature_name {
        "contrast" => features.contrast,
        "correlation" => features.correlation,
        "energy" => features.energy,
        "homogeneity" => features.homogeneity,
        "entropy" => features.entropy,
        "dissimilarity" => features.dissimilarity,
        "variance" => features.variance,
        "sum_average" => features.sum_average,
        "sum_entropy" => features.sum_entropy,
        "difference_entropy" => features.difference_entropy,
        "info_measure_corr1" => features.info_measure_corr1,
        "info_measure_corr2" => features.info_measure_corr2,
        "max_correlation_coeff" => features.max_correlation_coeff,
        "cluster_shade" => features.cluster_shade,
        "cluster_prominence" => features.cluster_prominence,
        _ => {
            return Err(AlgorithmError::InvalidParameter {
                parameter: "feature_name",
                message: format!("Unknown feature: {}", feature_name),
            });
        }
    };

    Ok(value)
}

/// Quantizes an image to specified number of gray levels
fn quantize_image(src: &RasterBuffer, gray_levels: usize) -> Result<RasterBuffer> {
    use oxigdal_core::types::RasterDataType;

    let width = src.width();
    let height = src.height();

    // Find min and max values
    let mut min_val = f64::INFINITY;
    let mut max_val = f64::NEG_INFINITY;

    for y in 0..height {
        for x in 0..width {
            let val = src.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            min_val = min_val.min(val);
            max_val = max_val.max(val);
        }
    }

    let range = max_val - min_val;
    if range == 0.0 {
        return Ok(RasterBuffer::zeros(width, height, RasterDataType::UInt8));
    }

    // Quantize
    let mut quantized = RasterBuffer::zeros(width, height, RasterDataType::UInt8);
    let scale = (gray_levels - 1) as f64 / range;

    for y in 0..height {
        for x in 0..width {
            let val = src.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let level = ((val - min_val) * scale).round() as u8;
            let clamped = level.min((gray_levels - 1) as u8);
            quantized
                .set_pixel(x, y, f64::from(clamped))
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(quantized)
}

/// Computes all Haralick features as images
///
/// # Arguments
///
/// * `src` - Source raster buffer
/// * `direction` - Direction for GLCM
/// * `distance` - Distance for GLCM
/// * `window_size` - Size of moving window
/// * `params` - GLCM parameters
///
/// # Errors
///
/// Returns an error if the operation fails
#[allow(clippy::type_complexity)]
pub fn compute_all_texture_features(
    src: &RasterBuffer,
    direction: Direction,
    distance: u32,
    window_size: usize,
    params: &GlcmParams,
) -> Result<Vec<(&'static str, RasterBuffer)>> {
    let features = [
        "contrast",
        "correlation",
        "energy",
        "homogeneity",
        "entropy",
        "dissimilarity",
        "variance",
        "sum_average",
        "sum_entropy",
        "difference_entropy",
    ];

    let mut results = Vec::new();

    for &feature in &features {
        let image =
            compute_texture_feature_image(src, feature, direction, distance, window_size, params)?;
        results.push((feature, image));
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_direction_offset() {
        assert_eq!(Direction::Horizontal.offset(1), (1, 0));
        assert_eq!(Direction::Vertical.offset(1), (0, 1));
        assert_eq!(Direction::Diagonal45.offset(1), (1, -1));
        assert_eq!(Direction::Diagonal135.offset(1), (1, 1));
    }

    #[test]
    fn test_glcm_params_default() {
        let params = GlcmParams::default();
        assert_eq!(params.gray_levels, 256);
        assert!(params.normalize);
        assert!(params.symmetric);
        assert!(params.window_size.is_none());
    }

    #[test]
    fn test_glcm_creation() {
        let glcm = Glcm::new(256, Direction::Horizontal, 1);
        assert_eq!(glcm.gray_levels(), 256);
        assert_eq!(glcm.direction(), Direction::Horizontal);
        assert_eq!(glcm.distance(), 1);
        assert!(!glcm.is_normalized());
    }

    #[test]
    fn test_glcm_get_set() {
        let mut glcm = Glcm::new(8, Direction::Horizontal, 1);
        glcm.set(2, 3, 5.0);
        assert_abs_diff_eq!(glcm.get(2, 3), 5.0);
    }

    #[test]
    fn test_glcm_normalize() {
        let mut glcm = Glcm::new(2, Direction::Horizontal, 1);
        glcm.set(0, 0, 2.0);
        glcm.set(0, 1, 3.0);
        glcm.set(1, 0, 3.0);
        glcm.set(1, 1, 2.0);

        glcm.normalize();

        assert!(glcm.is_normalized());
        assert_abs_diff_eq!(glcm.get(0, 0), 0.2);
        assert_abs_diff_eq!(glcm.get(0, 1), 0.3);
    }

    #[test]
    fn test_compute_glcm() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::UInt8);

        // Create a simple pattern
        for y in 0..10 {
            for x in 0..10 {
                let val = if (x + y) % 2 == 0 { 0.0 } else { 255.0 };
                src.set_pixel(x, y, val)
                    .expect("setting pixel should succeed in test");
            }
        }

        let params = GlcmParams {
            gray_levels: 2,
            normalize: true,
            symmetric: true,
            window_size: None,
        };

        let glcm = compute_glcm(&src, Direction::Horizontal, 1, &params)
            .expect("compute_glcm should succeed in test");

        assert!(glcm.is_normalized());
        assert_eq!(glcm.gray_levels(), 2);
    }

    #[test]
    fn test_haralick_features() {
        let mut glcm = Glcm::new(2, Direction::Horizontal, 1);

        // Create a simple GLCM
        glcm.set(0, 0, 0.25);
        glcm.set(0, 1, 0.25);
        glcm.set(1, 0, 0.25);
        glcm.set(1, 1, 0.25);

        let features = compute_haralick_features(&glcm);

        // Uniform distribution should have maximum entropy
        assert!(features.energy > 0.0);
        assert!(features.entropy > 0.0);
        assert_abs_diff_eq!(features.energy, 0.25, epsilon = 0.01);
    }

    #[test]
    fn test_quantize_image() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::Float64);

        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, (x * 10 + y) as f64)
                    .expect("setting pixel should succeed in test");
            }
        }

        let quantized = quantize_image(&src, 8).expect("quantize_image should succeed in test");

        // Check that all values are within range
        for y in 0..10 {
            for x in 0..10 {
                let val = quantized
                    .get_pixel(x, y)
                    .expect("getting pixel should succeed in test");
                assert!((0.0..8.0).contains(&val));
            }
        }
    }

    #[test]
    fn test_multi_direction_glcm() {
        let mut src = RasterBuffer::zeros(10, 10, RasterDataType::UInt8);

        for y in 0..10 {
            for x in 0..10 {
                src.set_pixel(x, y, ((x + y) % 4 * 64) as f64)
                    .expect("setting pixel should succeed in test");
            }
        }

        let params = GlcmParams {
            gray_levels: 4,
            normalize: true,
            symmetric: true,
            window_size: None,
        };

        let directions = Direction::all_standard();
        let glcm = compute_glcm_multi_direction(&src, &directions, 1, &params)
            .expect("compute_glcm_multi_direction should succeed in test");

        assert_eq!(glcm.gray_levels(), 4);
    }

    #[test]
    fn test_texture_feature_names() {
        let mut glcm = Glcm::new(4, Direction::Horizontal, 1);

        // Create a simple non-uniform GLCM
        glcm.set(0, 0, 0.5);
        glcm.set(1, 1, 0.3);
        glcm.set(2, 2, 0.2);

        let features = compute_haralick_features(&glcm);

        // All features should be finite
        assert!(features.contrast.is_finite());
        assert!(features.correlation.is_finite());
        assert!(features.energy.is_finite());
        assert!(features.homogeneity.is_finite());
        assert!(features.entropy.is_finite());
        assert!(features.dissimilarity.is_finite());
        assert!(features.variance.is_finite());
    }
}
