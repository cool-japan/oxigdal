//! Getis-Ord Gi* Statistic
//!
//! The Getis-Ord Gi* statistic identifies spatial clusters of high or low values.
//! Also known as hot spot analysis.

use crate::error::{AnalyticsError, Result};
use crate::hotspot::SpatialWeights;
use scirs2_core::ndarray::{Array1, ArrayView1, ArrayView2};

/// Getis-Ord Gi* result
#[derive(Debug, Clone)]
pub struct GetisOrdResult {
    /// Gi* z-scores for each location
    pub z_scores: Array1<f64>,
    /// P-values for each location
    pub p_values: Array1<f64>,
    /// Classification of each location
    pub classifications: Array1<HotspotClass>,
    /// Confidence level used
    pub confidence: f64,
}

/// Hotspot classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotspotClass {
    /// Statistically significant hot spot (high value cluster)
    HotSpot,
    /// Statistically significant cold spot (low value cluster)
    ColdSpot,
    /// Not statistically significant
    NotSignificant,
}

/// Getis-Ord Gi* calculator
pub struct GetisOrdGiStar {
    confidence: f64,
}

impl GetisOrdGiStar {
    /// Create a new Getis-Ord Gi* calculator
    ///
    /// # Arguments
    /// * `confidence` - Significance level (e.g., 0.05 for 95% confidence)
    pub fn new(confidence: f64) -> Self {
        Self { confidence }
    }

    /// Calculate Gi* statistic for all locations
    ///
    /// # Arguments
    /// * `values` - Values for each location
    /// * `weights` - Spatial weights matrix (should include self-weight)
    ///
    /// # Errors
    /// Returns error if computation fails or dimensions don't match
    pub fn calculate(
        &self,
        values: &ArrayView1<f64>,
        weights: &SpatialWeights,
    ) -> Result<GetisOrdResult> {
        let n = values.len();
        if n != weights.weights.nrows() {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{}", n),
                format!("{}", weights.weights.nrows()),
            ));
        }

        if n < 3 {
            return Err(AnalyticsError::insufficient_data(
                "Need at least 3 observations for Gi* statistic",
            ));
        }

        // Calculate global mean and variance
        let mean = values.sum() / (n as f64);
        let variance = values.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n as f64);
        let std_dev = variance.sqrt();

        if std_dev < f64::EPSILON {
            return Err(AnalyticsError::numerical_instability(
                "Standard deviation is too small",
            ));
        }

        let mut z_scores = Array1::zeros(n);
        let mut p_values = Array1::zeros(n);
        let mut classifications = Array1::from_elem(n, HotspotClass::NotSignificant);

        // Calculate Gi* for each location
        for i in 0..n {
            // Calculate weighted sum including self
            let mut weighted_sum = 0.0;
            let mut weight_sum = 0.0;
            let mut weight_sq_sum = 0.0;

            for j in 0..n {
                let w_ij = weights.weights[[i, j]];
                weighted_sum += w_ij * values[j];
                weight_sum += w_ij;
                weight_sq_sum += w_ij * w_ij;
            }

            // Calculate Gi* z-score
            let numerator = weighted_sum - mean * weight_sum;
            let denominator = std_dev
                * ((n as f64 * weight_sq_sum - weight_sum * weight_sum) / ((n - 1) as f64)).sqrt();

            if denominator.abs() < f64::EPSILON {
                z_scores[i] = 0.0;
            } else {
                z_scores[i] = numerator / denominator;
            }

            // Calculate p-value (two-tailed)
            p_values[i] = 2.0 * (1.0 - standard_normal_cdf(z_scores[i].abs()));

            // Classify
            if p_values[i] < self.confidence {
                classifications[i] = if z_scores[i] > 0.0 {
                    HotspotClass::HotSpot
                } else {
                    HotspotClass::ColdSpot
                };
            }
        }

        Ok(GetisOrdResult {
            z_scores,
            p_values,
            classifications,
            confidence: self.confidence,
        })
    }

    /// Calculate Gi* for spatial grid data
    ///
    /// # Arguments
    /// * `values` - Values arranged in 2D grid
    /// * `neighborhood_size` - Size of neighborhood (e.g., 1 for 3x3, 2 for 5x5)
    ///
    /// # Errors
    /// Returns error if computation fails
    pub fn calculate_grid(
        &self,
        values: &ArrayView2<f64>,
        neighborhood_size: usize,
    ) -> Result<GetisOrdResult> {
        let (nrows, ncols) = values.dim();
        let _n = nrows * ncols;

        // Flatten grid to 1D
        let flat_values = values.iter().copied().collect::<Vec<f64>>();
        let flat_array = Array1::from_vec(flat_values);

        // Build spatial weights for grid
        let weights = self.build_grid_weights(nrows, ncols, neighborhood_size)?;

        self.calculate(&flat_array.view(), &weights)
    }

    /// Build spatial weights for regular grid with neighborhood
    fn build_grid_weights(
        &self,
        nrows: usize,
        ncols: usize,
        neighborhood_size: usize,
    ) -> Result<SpatialWeights> {
        let n = nrows * ncols;
        let mut weights = scirs2_core::ndarray::Array2::zeros((n, n));

        for row in 0..nrows {
            for col in 0..ncols {
                let i = row * ncols + col;

                // Include self-weight
                weights[[i, i]] = 1.0;

                // Add neighbors
                for dr in 0..=(2 * neighborhood_size) {
                    for dc in 0..=(2 * neighborhood_size) {
                        if dr == neighborhood_size && dc == neighborhood_size {
                            continue; // Skip self (already added)
                        }

                        let nr = row as isize + dr as isize - neighborhood_size as isize;
                        let nc = col as isize + dc as isize - neighborhood_size as isize;

                        if nr >= 0 && nr < nrows as isize && nc >= 0 && nc < ncols as isize {
                            let j = (nr as usize) * ncols + (nc as usize);
                            weights[[i, j]] = 1.0;
                        }
                    }
                }
            }
        }

        SpatialWeights::from_adjacency(weights)
    }
}

/// Standard normal cumulative distribution function
fn standard_normal_cdf(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / 2_f64.sqrt()))
}

/// Error function approximation
fn erf(x: f64) -> f64 {
    let sign = x.signum();
    let x = x.abs();

    let a1 = 0.254_829_592;
    let a2 = -0.284_496_736;
    let a3 = 1.421_413_741;
    let a4 = -1.453_152_027;
    let a5 = 1.061_405_429;
    let p = 0.327_591_100;

    let t = 1.0 / (1.0 + p * x);
    let t2 = t * t;
    let t3 = t2 * t;
    let t4 = t3 * t;
    let t5 = t4 * t;

    let result = 1.0 - (a1 * t + a2 * t2 + a3 * t3 + a4 * t4 + a5 * t5) * (-x * x).exp();

    sign * result
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_getis_ord_hotspot() {
        // Create data with a hot spot
        let values = array![1.0, 1.0, 1.0, 10.0, 10.0, 10.0, 1.0, 1.0, 1.0];

        // Create spatial weights (3x3 grid with neighbors)
        let mut weights_matrix = scirs2_core::ndarray::Array2::zeros((9, 9));
        // Simple adjacency for demonstration
        for i in 0..9 {
            weights_matrix[[i, i]] = 1.0;
            if i > 0 {
                weights_matrix[[i, i - 1]] = 1.0;
            }
            if i < 8 {
                weights_matrix[[i, i + 1]] = 1.0;
            }
        }

        let weights = SpatialWeights::from_adjacency(weights_matrix)
            .expect("Failed to create spatial weights for hotspot test");
        let gi_star = GetisOrdGiStar::new(0.05);
        let result = gi_star
            .calculate(&values.view(), &weights)
            .expect("Failed to calculate Gi* statistic for hotspot test");

        assert_eq!(result.z_scores.len(), 9);
        // Middle values (hot spot) should have positive z-scores
        assert!(result.z_scores[3] > 0.0);
        assert!(result.z_scores[4] > 0.0);
        assert!(result.z_scores[5] > 0.0);
    }

    #[test]
    fn test_getis_ord_coldspot() {
        let values = array![10.0, 10.0, 1.0, 1.0, 10.0, 10.0];

        let mut weights_matrix = scirs2_core::ndarray::Array2::zeros((6, 6));
        for i in 0..6 {
            weights_matrix[[i, i]] = 1.0;
            if i > 0 {
                weights_matrix[[i, i - 1]] = 1.0;
            }
            if i < 5 {
                weights_matrix[[i, i + 1]] = 1.0;
            }
        }

        let weights = SpatialWeights::from_adjacency(weights_matrix)
            .expect("Failed to create spatial weights for coldspot test");
        let gi_star = GetisOrdGiStar::new(0.05);
        let result = gi_star
            .calculate(&values.view(), &weights)
            .expect("Failed to calculate Gi* statistic for coldspot test");

        // Middle values (cold spot) should have negative z-scores
        assert!(result.z_scores[2] < 0.0);
        assert!(result.z_scores[3] < 0.0);
    }

    #[test]
    fn test_getis_ord_grid() {
        // Use a 5x5 grid so neighborhood doesn't cover all cells
        let values = array![
            [1.0, 1.0, 1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0, 1.0, 1.0],
            [1.0, 1.0, 10.0, 1.0, 1.0],
            [1.0, 1.0, 1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0, 1.0, 1.0]
        ];

        let gi_star = GetisOrdGiStar::new(0.05);
        let result = gi_star
            .calculate_grid(&values.view(), 1)
            .expect("Failed to calculate Gi* statistic for grid test");

        assert_eq!(result.z_scores.len(), 25);
        // Center cell (index 12: row 2, col 2) should be a hot spot
        assert!(
            result.z_scores[12] > 0.0,
            "Center hotspot should have positive z-score, got: {}",
            result.z_scores[12]
        );
    }
}
