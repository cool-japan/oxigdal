//! Moran's I Spatial Autocorrelation
//!
//! Global and local Moran's I statistics for detecting spatial clustering.

use crate::error::{AnalyticsError, Result};
use crate::hotspot::SpatialWeights;
use scirs2_core::ndarray::{Array1, ArrayView1};

/// Global Moran's I result
#[derive(Debug, Clone)]
pub struct MoransIResult {
    /// Moran's I statistic
    pub i_statistic: f64,
    /// Expected value under null hypothesis
    pub expected_i: f64,
    /// Variance of I
    pub variance_i: f64,
    /// Z-score
    pub z_score: f64,
    /// P-value (two-tailed)
    pub p_value: f64,
    /// Whether result is statistically significant
    pub significant: bool,
    /// Confidence level
    pub confidence: f64,
}

/// Local Moran's I result
#[derive(Debug, Clone)]
pub struct LocalMoransIResult {
    /// Local I statistics for each location
    pub local_i: Array1<f64>,
    /// Z-scores for each location
    pub z_scores: Array1<f64>,
    /// P-values for each location
    pub p_values: Array1<f64>,
    /// LISA classifications
    pub classifications: Array1<LisaClass>,
    /// Confidence level
    pub confidence: f64,
}

/// LISA (Local Indicators of Spatial Association) classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LisaClass {
    /// High-High cluster
    HH,
    /// Low-Low cluster
    LL,
    /// High-Low outlier
    HL,
    /// Low-High outlier
    LH,
    /// Not significant
    NotSignificant,
}

/// Global Moran's I calculator
pub struct MoransI {
    confidence: f64,
}

impl MoransI {
    /// Create a new Moran's I calculator
    ///
    /// # Arguments
    /// * `confidence` - Significance level (e.g., 0.05)
    pub fn new(confidence: f64) -> Self {
        Self { confidence }
    }

    /// Calculate global Moran's I
    ///
    /// # Arguments
    /// * `values` - Values for each location
    /// * `weights` - Spatial weights matrix (should be row-standardized)
    ///
    /// # Errors
    /// Returns error if computation fails
    pub fn calculate(
        &self,
        values: &ArrayView1<f64>,
        weights: &SpatialWeights,
    ) -> Result<MoransIResult> {
        let n = values.len();
        if n != weights.weights.nrows() {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{}", n),
                format!("{}", weights.weights.nrows()),
            ));
        }

        if n < 3 {
            return Err(AnalyticsError::insufficient_data(
                "Need at least 3 observations for Moran's I",
            ));
        }

        // Calculate mean and deviations
        let mean = values.sum() / (n as f64);
        let deviations: Vec<f64> = values.iter().map(|&x| x - mean).collect();

        // Calculate sum of squared deviations
        let s2 = deviations.iter().map(|&d| d * d).sum::<f64>() / (n as f64);

        if s2 < f64::EPSILON {
            return Err(AnalyticsError::numerical_instability(
                "Variance is too small",
            ));
        }

        // Calculate spatial lag
        let spatial_lag = weights.spatial_lag(values)?;

        // Calculate numerator (sum of cross-products)
        let mut numerator = 0.0;
        for i in 0..n {
            numerator += deviations[i] * (spatial_lag[i] - mean);
        }

        // Calculate sum of weights
        let s0: f64 = weights.weights.iter().sum();

        if s0 < f64::EPSILON {
            return Err(AnalyticsError::numerical_instability(
                "Sum of weights is too small",
            ));
        }

        // Calculate Moran's I
        let i_statistic =
            (n as f64 / s0) * (numerator / deviations.iter().map(|&d| d * d).sum::<f64>());

        // Calculate expected value and variance
        let expected_i = -1.0 / ((n - 1) as f64);

        // Calculate variance (under normality assumption)
        let s1 = self.calculate_s1(&weights.weights);
        let s2_stat = self.calculate_s2(&weights.weights, n);
        let _s3 = self.calculate_s3(&deviations, n);
        let s4 = self.calculate_s4(&deviations, n);
        let s5 = self.calculate_s5(&deviations, n);

        let n_f64 = n as f64;
        let variance_i = ((n_f64 * s1 - n_f64 * s2_stat + 3.0 * s0 * s0) * s4)
            / ((n_f64 - 1.0) * (n_f64 + 1.0) * s5)
            - (expected_i * expected_i);

        // Calculate z-score and p-value
        let z_score = if variance_i > f64::EPSILON {
            (i_statistic - expected_i) / variance_i.sqrt()
        } else {
            0.0
        };

        let p_value = 2.0 * (1.0 - standard_normal_cdf(z_score.abs()));
        let significant = p_value < self.confidence;

        Ok(MoransIResult {
            i_statistic,
            expected_i,
            variance_i,
            z_score,
            p_value,
            significant,
            confidence: self.confidence,
        })
    }

    fn calculate_s1(&self, weights: &scirs2_core::ndarray::Array2<f64>) -> f64 {
        let n = weights.nrows();
        let mut s1 = 0.0;
        for i in 0..n {
            for j in 0..n {
                let w_ij = weights[[i, j]];
                let w_ji = weights[[j, i]];
                s1 += (w_ij + w_ji) * (w_ij + w_ji);
            }
        }
        s1 / 2.0
    }

    fn calculate_s2(&self, weights: &scirs2_core::ndarray::Array2<f64>, n: usize) -> f64 {
        let mut s2 = 0.0;
        for i in 0..n {
            let mut sum_i = 0.0;
            let mut sum_j = 0.0;
            for j in 0..n {
                sum_i += weights[[i, j]];
                sum_j += weights[[j, i]];
            }
            s2 += (sum_i + sum_j) * (sum_i + sum_j);
        }
        s2
    }

    fn calculate_s3(&self, deviations: &[f64], n: usize) -> f64 {
        let m2 = deviations.iter().map(|&d| d * d).sum::<f64>() / (n as f64);
        let m4 = deviations.iter().map(|&d| d.powi(4)).sum::<f64>() / (n as f64);
        (n as f64) * m4 / (m2 * m2)
    }

    fn calculate_s4(&self, deviations: &[f64], n: usize) -> f64 {
        deviations.iter().map(|&d| d * d).sum::<f64>() / (n as f64)
    }

    fn calculate_s5(&self, deviations: &[f64], n: usize) -> f64 {
        let m2 = deviations.iter().map(|&d| d * d).sum::<f64>() / (n as f64);
        m2 * m2
    }
}

/// Local Moran's I calculator (LISA)
pub struct LocalMoransI {
    confidence: f64,
}

impl LocalMoransI {
    /// Create a new Local Moran's I calculator
    ///
    /// # Arguments
    /// * `confidence` - Significance level (e.g., 0.05)
    pub fn new(confidence: f64) -> Self {
        Self { confidence }
    }

    /// Calculate local Moran's I for all locations
    ///
    /// # Arguments
    /// * `values` - Values for each location
    /// * `weights` - Spatial weights matrix
    ///
    /// # Errors
    /// Returns error if computation fails
    pub fn calculate(
        &self,
        values: &ArrayView1<f64>,
        weights: &SpatialWeights,
    ) -> Result<LocalMoransIResult> {
        let n = values.len();
        if n != weights.weights.nrows() {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{}", n),
                format!("{}", weights.weights.nrows()),
            ));
        }

        if n < 3 {
            return Err(AnalyticsError::insufficient_data(
                "Need at least 3 observations for Local Moran's I",
            ));
        }

        // Calculate mean and deviations
        let mean = values.sum() / (n as f64);
        let deviations: Vec<f64> = values.iter().map(|&x| x - mean).collect();

        // Calculate variance
        let s2 = deviations.iter().map(|&d| d * d).sum::<f64>() / (n as f64);

        if s2 < f64::EPSILON {
            return Err(AnalyticsError::numerical_instability(
                "Variance is too small",
            ));
        }

        let mut local_i = Array1::zeros(n);
        let mut z_scores = Array1::zeros(n);
        let mut p_values = Array1::zeros(n);
        let mut classifications = Array1::from_elem(n, LisaClass::NotSignificant);

        // Calculate spatial lag
        let spatial_lag = weights.spatial_lag(values)?;

        // Calculate Local I for each location
        for i in 0..n {
            let zi = deviations[i] / s2.sqrt();
            let mut sum_wij_zj = 0.0;

            for j in 0..n {
                if i != j {
                    let zj = deviations[j] / s2.sqrt();
                    sum_wij_zj += weights.weights[[i, j]] * zj;
                }
            }

            local_i[i] = zi * sum_wij_zj;

            // Calculate variance and z-score (simplified)
            // For proper inference, should use permutation tests or analytical variance
            let variance_i: f64 = 1.0; // Simplified - assumes standardization
            z_scores[i] = local_i[i] / variance_i.sqrt();

            // Calculate p-value
            p_values[i] = 2.0 * (1.0 - standard_normal_cdf(z_scores[i].abs()));

            // Classify LISA
            if p_values[i] < self.confidence {
                let lag_mean = spatial_lag[i];
                classifications[i] = match (values[i] > mean, lag_mean > mean) {
                    (true, true) => LisaClass::HH,   // High-High
                    (false, false) => LisaClass::LL, // Low-Low
                    (true, false) => LisaClass::HL,  // High-Low
                    (false, true) => LisaClass::LH,  // Low-High
                };
            }
        }

        Ok(LocalMoransIResult {
            local_i,
            z_scores,
            p_values,
            classifications,
            confidence: self.confidence,
        })
    }
}

/// Standard normal CDF
fn standard_normal_cdf(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / 2_f64.sqrt()))
}

/// Error function
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
    let result = 1.0
        - (a1 * t + a2 * t * t + a3 * t.powi(3) + a4 * t.powi(4) + a5 * t.powi(5)) * (-x * x).exp();

    sign * result
}

#[cfg(test)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_global_morans_i() {
        let values = array![1.0, 2.0, 3.0, 4.0, 5.0];
        let mut weights_matrix = scirs2_core::ndarray::Array2::zeros((5, 5));
        for i in 0..4 {
            weights_matrix[[i, i + 1]] = 1.0;
            weights_matrix[[i + 1, i]] = 1.0;
        }

        let mut weights = SpatialWeights::from_adjacency(weights_matrix)
            .expect("Creating spatial weights from adjacency matrix should succeed");
        weights.row_standardize();

        let morans_i = MoransI::new(0.05);
        let result = morans_i
            .calculate(&values.view(), &weights)
            .expect("Global Moran's I calculation should succeed");

        // Should show positive spatial autocorrelation
        assert!(result.i_statistic > result.expected_i);
    }

    #[test]
    fn test_local_morans_i() {
        let values = array![1.0, 1.0, 1.0, 10.0, 10.0, 10.0];
        let mut weights_matrix = scirs2_core::ndarray::Array2::zeros((6, 6));
        for i in 0..5 {
            weights_matrix[[i, i + 1]] = 1.0;
            weights_matrix[[i + 1, i]] = 1.0;
        }

        let weights = SpatialWeights::from_adjacency(weights_matrix)
            .expect("Creating spatial weights from adjacency matrix should succeed");
        let local_i = LocalMoransI::new(0.05);
        let result = local_i
            .calculate(&values.view(), &weights)
            .expect("Local Moran's I calculation should succeed");

        assert_eq!(result.local_i.len(), 6);
        // First cluster should be LL, second cluster should be HH
    }
}
