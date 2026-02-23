//! Change Detection Algorithms
//!
//! Implementations of various change detection methods for multi-temporal analysis.

use crate::error::{AnalyticsError, Result};
use scirs2_core::ndarray::{Array2, ArrayView2, ArrayView3};
use scirs2_core::num_traits::Float;

/// Change detection methods
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeMethod {
    /// Simple image differencing
    Differencing,
    /// Change Vector Analysis
    CVA,
    /// Principal Component Analysis
    PCA,
    /// Normalized difference
    NormalizedDifference,
}

/// Change detection result
#[derive(Debug, Clone)]
pub struct ChangeResult {
    /// Change magnitude map
    pub magnitude: Array2<f64>,
    /// Binary change map (based on threshold)
    pub binary_map: Array2<bool>,
    /// Threshold used for binary classification
    pub threshold: f64,
    /// Method used
    pub method: ChangeMethod,
    /// Additional statistics
    pub stats: ChangeStats,
}

/// Change detection statistics
#[derive(Debug, Clone)]
pub struct ChangeStats {
    /// Mean change magnitude
    pub mean_change: f64,
    /// Standard deviation of change
    pub std_change: f64,
    /// Minimum change value
    pub min_change: f64,
    /// Maximum change value
    pub max_change: f64,
    /// Number of changed pixels
    pub n_changed: usize,
    /// Percentage of changed pixels
    pub percent_changed: f64,
}

/// Change detector
pub struct ChangeDetector {
    method: ChangeMethod,
    threshold: Option<f64>,
}

impl ChangeDetector {
    /// Create a new change detector
    ///
    /// # Arguments
    /// * `method` - Change detection method
    pub fn new(method: ChangeMethod) -> Self {
        Self {
            method,
            threshold: None,
        }
    }

    /// Set threshold for binary classification
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.threshold = Some(threshold);
        self
    }

    /// Detect changes between two images
    ///
    /// # Arguments
    /// * `before` - Image before change (height × width × bands)
    /// * `after` - Image after change (height × width × bands)
    ///
    /// # Errors
    /// Returns error if images have different dimensions
    pub fn detect(
        &self,
        before: &ArrayView3<f64>,
        after: &ArrayView3<f64>,
    ) -> Result<ChangeResult> {
        if before.dim() != after.dim() {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{:?}", before.dim()),
                format!("{:?}", after.dim()),
            ));
        }

        let magnitude = match self.method {
            ChangeMethod::Differencing => self.image_differencing(before, after)?,
            ChangeMethod::CVA => self.change_vector_analysis(before, after)?,
            ChangeMethod::PCA => self.pca_change_detection(before, after)?,
            ChangeMethod::NormalizedDifference => self.normalized_difference(before, after)?,
        };

        // Determine threshold if not provided
        let threshold = self
            .threshold
            .unwrap_or_else(|| ThresholdOptimizer::otsu(&magnitude.view()).unwrap_or(0.0));

        // Create binary change map
        let binary_map = magnitude.mapv(|x| x > threshold);

        // Calculate statistics
        let stats = self.calculate_stats(&magnitude, &binary_map)?;

        Ok(ChangeResult {
            magnitude,
            binary_map,
            threshold,
            method: self.method,
            stats,
        })
    }

    /// Simple image differencing
    fn image_differencing(
        &self,
        before: &ArrayView3<f64>,
        after: &ArrayView3<f64>,
    ) -> Result<Array2<f64>> {
        let (height, width, bands) = before.dim();
        let mut magnitude = Array2::zeros((height, width));

        for i in 0..height {
            for j in 0..width {
                let mut sum_sq = 0.0;
                for b in 0..bands {
                    let diff = after[[i, j, b]] - before[[i, j, b]];
                    sum_sq += diff * diff;
                }
                magnitude[[i, j]] = sum_sq.sqrt();
            }
        }

        Ok(magnitude)
    }

    /// Change Vector Analysis (CVA)
    fn change_vector_analysis(
        &self,
        before: &ArrayView3<f64>,
        after: &ArrayView3<f64>,
    ) -> Result<Array2<f64>> {
        // CVA computes the magnitude of change vector in feature space
        let (height, width, bands) = before.dim();
        let mut magnitude = Array2::zeros((height, width));

        for i in 0..height {
            for j in 0..width {
                let mut sum_sq = 0.0;
                for b in 0..bands {
                    let diff = after[[i, j, b]] - before[[i, j, b]];
                    sum_sq += diff * diff;
                }
                magnitude[[i, j]] = sum_sq.sqrt();
            }
        }

        Ok(magnitude)
    }

    /// PCA-based change detection
    fn pca_change_detection(
        &self,
        before: &ArrayView3<f64>,
        after: &ArrayView3<f64>,
    ) -> Result<Array2<f64>> {
        let pca = PrincipalComponentAnalysis::new();
        pca.detect_change(before, after)
    }

    /// Normalized difference
    fn normalized_difference(
        &self,
        before: &ArrayView3<f64>,
        after: &ArrayView3<f64>,
    ) -> Result<Array2<f64>> {
        let (height, width, bands) = before.dim();
        let mut magnitude = Array2::zeros((height, width));

        for i in 0..height {
            for j in 0..width {
                let mut sum_diff = 0.0;
                let mut sum_sum = 0.0;

                for b in 0..bands {
                    let b_val = before[[i, j, b]];
                    let a_val = after[[i, j, b]];
                    sum_diff += (a_val - b_val).abs();
                    sum_sum += a_val + b_val;
                }

                magnitude[[i, j]] = if sum_sum > f64::EPSILON {
                    sum_diff / sum_sum
                } else {
                    0.0
                };
            }
        }

        Ok(magnitude)
    }

    /// Calculate change statistics
    fn calculate_stats(
        &self,
        magnitude: &Array2<f64>,
        binary_map: &Array2<bool>,
    ) -> Result<ChangeStats> {
        let n_pixels = magnitude.len();
        let n_changed = binary_map.iter().filter(|&&x| x).count();

        let mean_change = magnitude.sum() / (n_pixels as f64);
        let variance = magnitude
            .iter()
            .map(|&x| (x - mean_change).powi(2))
            .sum::<f64>()
            / (n_pixels as f64);
        let std_change = variance.sqrt();

        let min_change = magnitude
            .iter()
            .copied()
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(0.0);
        let max_change = magnitude
            .iter()
            .copied()
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(0.0);

        Ok(ChangeStats {
            mean_change,
            std_change,
            min_change,
            max_change,
            n_changed,
            percent_changed: (n_changed as f64 / n_pixels as f64) * 100.0,
        })
    }
}

/// Image differencing utility
pub struct ImageDifferencing;

impl ImageDifferencing {
    /// Compute absolute difference between two images
    pub fn absolute_difference(
        before: &ArrayView2<f64>,
        after: &ArrayView2<f64>,
    ) -> Result<Array2<f64>> {
        if before.dim() != after.dim() {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{:?}", before.dim()),
                format!("{:?}", after.dim()),
            ));
        }

        Ok((after - before).mapv(|x| x.abs()))
    }

    /// Compute ratio between two images
    pub fn ratio(before: &ArrayView2<f64>, after: &ArrayView2<f64>) -> Result<Array2<f64>> {
        if before.dim() != after.dim() {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{:?}", before.dim()),
                format!("{:?}", after.dim()),
            ));
        }

        let mut ratio = Array2::zeros(before.dim());
        for ((i, j), &b_val) in before.indexed_iter() {
            let a_val = after[[i, j]];
            ratio[[i, j]] = if b_val.abs() > f64::EPSILON {
                a_val / b_val
            } else {
                0.0
            };
        }

        Ok(ratio)
    }
}

/// Change Vector Analysis
pub struct ChangeVectorAnalysis;

impl ChangeVectorAnalysis {
    /// Compute change magnitude and direction
    pub fn analyze(
        before: &ArrayView3<f64>,
        after: &ArrayView3<f64>,
    ) -> Result<(Array2<f64>, Array2<f64>)> {
        if before.dim() != after.dim() {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{:?}", before.dim()),
                format!("{:?}", after.dim()),
            ));
        }

        let (height, width, bands) = before.dim();
        let mut magnitude = Array2::zeros((height, width));
        let mut direction = Array2::zeros((height, width));

        for i in 0..height {
            for j in 0..width {
                let mut sum_sq = 0.0;
                let mut diff_vec = Vec::with_capacity(bands);

                for b in 0..bands {
                    let diff = after[[i, j, b]] - before[[i, j, b]];
                    diff_vec.push(diff);
                    sum_sq += diff * diff;
                }

                magnitude[[i, j]] = sum_sq.sqrt();

                // Calculate direction (angle in radians) for 2-band case
                if bands == 2 {
                    direction[[i, j]] = diff_vec[1].atan2(diff_vec[0]);
                } else if bands >= 2 {
                    // For multi-band, use first two bands
                    direction[[i, j]] = diff_vec[1].atan2(diff_vec[0]);
                }
            }
        }

        Ok((magnitude, direction))
    }
}

/// Principal Component Analysis for change detection
pub struct PrincipalComponentAnalysis;

impl PrincipalComponentAnalysis {
    /// Create new PCA change detector
    pub fn new() -> Self {
        Self
    }

    /// Detect change using PCA
    pub fn detect_change(
        &self,
        before: &ArrayView3<f64>,
        after: &ArrayView3<f64>,
    ) -> Result<Array2<f64>> {
        if before.dim() != after.dim() {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{:?}", before.dim()),
                format!("{:?}", after.dim()),
            ));
        }

        let (height, width, bands) = before.dim();

        // Stack images for PCA
        let n_pixels = height * width;
        let mut stacked = Array2::zeros((n_pixels, bands * 2));

        for b in 0..bands {
            let before_band = before.slice(s![.., .., b]);
            let after_band = after.slice(s![.., .., b]);

            for (idx, (b_val, a_val)) in before_band.iter().zip(after_band.iter()).enumerate() {
                stacked[[idx, b]] = *b_val;
                stacked[[idx, b + bands]] = *a_val;
            }
        }

        // Simplified PCA: compute variance along time dimension
        let mut magnitude = Array2::zeros((height, width));
        for i in 0..height {
            for j in 0..width {
                let idx = i * width + j;
                let mut sum_sq = 0.0;
                for b in 0..bands {
                    let diff = stacked[[idx, b + bands]] - stacked[[idx, b]];
                    sum_sq += diff * diff;
                }
                magnitude[[i, j]] = sum_sq.sqrt();
            }
        }

        Ok(magnitude)
    }
}

impl Default for PrincipalComponentAnalysis {
    fn default() -> Self {
        Self::new()
    }
}

/// Threshold optimization
pub struct ThresholdOptimizer;

impl ThresholdOptimizer {
    /// Otsu's method for automatic threshold selection
    ///
    /// # Arguments
    /// * `data` - Change magnitude map
    ///
    /// # Errors
    /// Returns error if computation fails
    pub fn otsu(data: &ArrayView2<f64>) -> Result<f64> {
        // Normalize data to 0-255 range for histogram
        let min = data
            .iter()
            .copied()
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .ok_or_else(|| AnalyticsError::insufficient_data("Empty data"))?;
        let max = data
            .iter()
            .copied()
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .ok_or_else(|| AnalyticsError::insufficient_data("Empty data"))?;

        if (max - min).abs() < f64::EPSILON {
            return Ok(min);
        }

        // Build histogram
        const N_BINS: usize = 256;
        let mut histogram = vec![0usize; N_BINS];

        for &value in data.iter() {
            let normalized = ((value - min) / (max - min) * 255.0).clamp(0.0, 255.0);
            let bin = normalized as usize;
            if bin < N_BINS {
                histogram[bin] += 1;
            }
        }

        // Find optimal threshold using Otsu's method
        let total_pixels = data.len();
        let mut sum = 0.0;
        for (i, &count) in histogram.iter().enumerate() {
            sum += (i as f64) * (count as f64);
        }

        let mut sum_b = 0.0;
        let mut weight_b = 0;
        let mut max_variance = 0.0;
        let mut threshold_idx = 0;

        for (t, &count) in histogram.iter().enumerate() {
            weight_b += count;
            if weight_b == 0 {
                continue;
            }

            let weight_f = total_pixels - weight_b;
            if weight_f == 0 {
                break;
            }

            sum_b += (t as f64) * (count as f64);

            let mean_b = sum_b / (weight_b as f64);
            let mean_f = (sum - sum_b) / (weight_f as f64);

            let variance = (weight_b as f64) * (weight_f as f64) * (mean_b - mean_f).powi(2);

            if variance > max_variance {
                max_variance = variance;
                threshold_idx = t;
            }
        }

        // Convert threshold back to original scale
        let threshold = min + (threshold_idx as f64 / 255.0) * (max - min);

        Ok(threshold)
    }
}

// Import ndarray slice macro
use scirs2_core::ndarray::s;

#[cfg(test)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::Array;

    #[test]
    fn test_image_differencing() {
        let before = Array::from_shape_vec((2, 2, 1), vec![1.0, 2.0, 3.0, 4.0])
            .expect("Failed to create before array with shape (2, 2, 1)");
        let after = Array::from_shape_vec((2, 2, 1), vec![2.0, 3.0, 4.0, 5.0])
            .expect("Failed to create after array with shape (2, 2, 1)");

        let detector = ChangeDetector::new(ChangeMethod::Differencing).with_threshold(0.5);
        let result = detector
            .detect(&before.view(), &after.view())
            .expect("Change detection should succeed with valid inputs");

        assert_eq!(result.magnitude.dim(), (2, 2));
        assert!(result.stats.n_changed > 0);
    }

    #[test]
    fn test_absolute_difference() {
        let before = Array::from_shape_vec((2, 2), vec![1.0, 2.0, 3.0, 4.0])
            .expect("Failed to create before array with shape (2, 2)");
        let after = Array::from_shape_vec((2, 2), vec![2.0, 3.0, 4.0, 5.0])
            .expect("Failed to create after array with shape (2, 2)");

        let diff = ImageDifferencing::absolute_difference(&before.view(), &after.view())
            .expect("Absolute difference computation should succeed");

        assert_eq!(diff[[0, 0]], 1.0);
        assert_eq!(diff[[1, 1]], 1.0);
    }

    #[test]
    fn test_otsu_threshold() {
        let data =
            Array::from_shape_vec((3, 3), vec![1.0, 1.0, 1.0, 5.0, 5.0, 5.0, 10.0, 10.0, 10.0])
                .expect("Failed to create data array with shape (3, 3)");

        let threshold = ThresholdOptimizer::otsu(&data.view())
            .expect("Otsu threshold computation should succeed");

        assert!(threshold > 1.0 && threshold < 10.0);
    }
}
