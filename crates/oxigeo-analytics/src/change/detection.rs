//! Change Detection Algorithms
//!
//! Implementations of various change detection methods for multi-temporal analysis.

use crate::error::{AnalyticsError, Result};
use scirs2_core::linalg::eig_symmetric;
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

    /// Detect change using PCA (Deng et al., 2008; Celik, 2009 -- the classic
    /// PCA change-vector technique used in remote-sensing change detection).
    ///
    /// The before/after bands are stacked into a single `2 * bands`
    /// dimensional observation per pixel. The covariance matrix of the
    /// mean-centered stacked observations is eigendecomposed, and the
    /// pixel-wise projection onto the *minor* eigenvector (the component
    /// with the smallest eigenvalue) is used as the change score.
    ///
    /// The intuition: for pixels that did not change, the corresponding
    /// before/after band values are highly correlated, so that correlated
    /// ("no-change") variance is captured by the major components. Variance
    /// that is *not* explained by that shared before/after correlation --
    /// i.e. genuine change -- is pushed into the minor components, so the
    /// minor-component score is a strong change indicator.
    ///
    /// # Errors
    /// Returns an error if images have different dimensions, there are
    /// fewer than 2 pixels (a covariance matrix cannot be formed), or the
    /// covariance matrix eigendecomposition fails to converge.
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
        let n_pixels = height * width;
        let n_features = bands * 2;

        if n_pixels < 2 {
            return Err(AnalyticsError::insufficient_data(
                "PCA change detection requires at least 2 pixels",
            ));
        }
        if bands == 0 {
            return Err(AnalyticsError::insufficient_data(
                "PCA change detection requires at least 1 band",
            ));
        }

        // Stack before/after bands into a single (n_pixels x 2*bands) matrix.
        let mut stacked = Array2::zeros((n_pixels, n_features));
        for b in 0..bands {
            let before_band = before.slice(s![.., .., b]);
            let after_band = after.slice(s![.., .., b]);

            for (idx, (b_val, a_val)) in before_band.iter().zip(after_band.iter()).enumerate() {
                stacked[[idx, b]] = *b_val;
                stacked[[idx, b + bands]] = *a_val;
            }
        }

        // Mean-center each feature column.
        let mut centered = stacked;
        for f in 0..n_features {
            let mean = centered.column(f).sum() / n_pixels as f64;
            for i in 0..n_pixels {
                centered[[i, f]] -= mean;
            }
        }

        // Sample covariance matrix of the stacked observations: C = Xᵀ·X / (n-1).
        let denom = (n_pixels - 1) as f64;
        let covariance = centered.t().dot(&centered).mapv(|x| x / denom);

        // Eigendecompose the covariance matrix; eigenvalues come back sorted
        // ascending, so column 0 is the minor (smallest-eigenvalue) component.
        let evd = eig_symmetric(&covariance).map_err(|e| {
            AnalyticsError::matrix_error(format!("PCA covariance eigendecomposition failed: {e}"))
        })?;
        let minor_eigenvector = evd.eigenvectors.column(0);

        // Project every pixel's centered observation onto the minor
        // component; its magnitude is the PCA change score.
        let mut magnitude = Array2::zeros((height, width));
        for i in 0..height {
            for j in 0..width {
                let idx = i * width + j;
                let mut score = 0.0;
                for f in 0..n_features {
                    score += centered[[idx, f]] * minor_eigenvector[f];
                }
                magnitude[[i, j]] = score.abs();
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

    /// PCA change detection must actually perform PCA (covariance
    /// eigendecomposition + minor-component projection), not just Euclidean
    /// before/after distance. This test builds a scene where 19 of 20
    /// pixels have `before == after` (perfectly correlated, lying on the
    /// diagonal `after = before`), and a single pixel has a real jump
    /// (`after` far from `before`). Since the vast majority of pixels
    /// define the dominant (major) component, real PCA must concentrate the
    /// outlier's deviation into the minor component and give it by far the
    /// largest magnitude of any pixel.
    #[test]
    fn test_pca_change_detection_flags_decorrelated_outlier() {
        let height = 5;
        let width = 4;
        let bands = 1;
        let n_pixels = height * width;
        let outlier_idx = 10usize; // pixel (2, 2)

        let mut before_data = vec![0.0f64; n_pixels];
        let mut after_data = vec![0.0f64; n_pixels];
        for (idx, (b, a)) in before_data
            .iter_mut()
            .zip(after_data.iter_mut())
            .enumerate()
        {
            let v = idx as f64;
            *b = v;
            *a = v;
        }
        // Introduce a single genuine, decorrelated change.
        after_data[outlier_idx] = before_data[outlier_idx] + 30.0;

        let before = Array::from_shape_vec((height, width, bands), before_data)
            .expect("Failed to build before array");
        let after = Array::from_shape_vec((height, width, bands), after_data)
            .expect("Failed to build after array");

        let pca = PrincipalComponentAnalysis::new();
        let magnitude = pca
            .detect_change(&before.view(), &after.view())
            .expect("PCA change detection should succeed");

        assert_eq!(magnitude.dim(), (height, width));

        let outlier_i = outlier_idx / width;
        let outlier_j = outlier_idx % width;
        let outlier_magnitude = magnitude[[outlier_i, outlier_j]];

        for i in 0..height {
            for j in 0..width {
                if (i, j) == (outlier_i, outlier_j) {
                    continue;
                }
                assert!(
                    magnitude[[i, j]] < outlier_magnitude,
                    "unchanged pixel ({i},{j}) magnitude {} should be far below the \
                     decorrelated outlier's magnitude {outlier_magnitude}",
                    magnitude[[i, j]]
                );
            }
        }

        // Real PCA must diverge from plain Euclidean CVA on this scene: CVA
        // would report the exact same |after-before| = 30.0 at the outlier
        // regardless of the surrounding correlation structure, while PCA's
        // score depends on the fitted covariance eigenvectors.
        let detector = ChangeDetector::new(ChangeMethod::CVA);
        let cva_result = detector
            .detect(&before.view(), &after.view())
            .expect("CVA detection should succeed");
        assert!(
            (cva_result.magnitude[[outlier_i, outlier_j]] - outlier_magnitude).abs() > 1e-6,
            "PCA magnitude should not equal the naive CVA Euclidean magnitude"
        );
    }

    #[test]
    fn test_pca_change_detection_dimension_mismatch() {
        let before = Array::from_shape_vec((2, 2, 1), vec![1.0, 2.0, 3.0, 4.0])
            .expect("Failed to create before array");
        let after =
            Array::from_shape_vec((2, 3, 1), vec![0.0; 6]).expect("Failed to create after array");

        let pca = PrincipalComponentAnalysis::new();
        let result = pca.detect_change(&before.view(), &after.view());
        assert!(result.is_err());
    }

    #[test]
    fn test_pca_change_detection_requires_min_pixels() {
        let before =
            Array::from_shape_vec((1, 1, 1), vec![1.0]).expect("Failed to create before array");
        let after =
            Array::from_shape_vec((1, 1, 1), vec![2.0]).expect("Failed to create after array");

        let pca = PrincipalComponentAnalysis::new();
        let result = pca.detect_change(&before.view(), &after.view());
        assert!(result.is_err());
    }
}
