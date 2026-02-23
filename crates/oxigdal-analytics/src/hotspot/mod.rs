//! Hotspot Analysis Module
//!
//! This module provides spatial statistics for hotspot and cluster analysis:
//! - Getis-Ord Gi* statistic
//! - Moran's I spatial autocorrelation
//! - Local Indicators of Spatial Association (LISA)

pub mod getis_ord;
pub mod moran;

pub use getis_ord::{GetisOrdGiStar, GetisOrdResult};
pub use moran::{LocalMoransI, LocalMoransIResult, MoransI, MoransIResult};

use crate::error::{AnalyticsError, Result};
use scirs2_core::ndarray::{Array1, Array2, ArrayView1, ArrayView2};

/// Spatial weights matrix
///
/// Represents spatial relationships between observations
#[derive(Debug, Clone)]
pub struct SpatialWeights {
    /// Weights matrix (n × n)
    pub weights: Array2<f64>,
    /// Whether weights are row-standardized
    pub row_standardized: bool,
}

impl SpatialWeights {
    /// Create spatial weights from adjacency matrix
    ///
    /// # Arguments
    /// * `adjacency` - Binary adjacency matrix (1 if neighbors, 0 otherwise)
    ///
    /// # Errors
    /// Returns error if matrix is not square
    pub fn from_adjacency(adjacency: Array2<f64>) -> Result<Self> {
        let (n, m) = adjacency.dim();
        if n != m {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{}x{}", n, n),
                format!("{}x{}", n, m),
            ));
        }

        Ok(Self {
            weights: adjacency,
            row_standardized: false,
        })
    }

    /// Create spatial weights from distance matrix with threshold
    ///
    /// # Arguments
    /// * `distances` - Distance matrix
    /// * `threshold` - Maximum distance to be considered neighbors
    ///
    /// # Errors
    /// Returns error if matrix is not square
    pub fn from_distance_threshold(distances: &ArrayView2<f64>, threshold: f64) -> Result<Self> {
        let (n, m) = distances.dim();
        if n != m {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{}x{}", n, n),
                format!("{}x{}", n, m),
            ));
        }

        let mut weights = Array2::zeros((n, n));
        for i in 0..n {
            for j in 0..n {
                if i != j && distances[[i, j]] <= threshold {
                    weights[[i, j]] = 1.0;
                }
            }
        }

        Ok(Self {
            weights,
            row_standardized: false,
        })
    }

    /// Create spatial weights using inverse distance
    ///
    /// # Arguments
    /// * `distances` - Distance matrix
    /// * `power` - Power parameter for inverse distance (typically 1 or 2)
    ///
    /// # Errors
    /// Returns error if matrix is not square or contains zero distances
    pub fn from_inverse_distance(distances: &ArrayView2<f64>, power: f64) -> Result<Self> {
        let (n, m) = distances.dim();
        if n != m {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{}x{}", n, n),
                format!("{}x{}", n, m),
            ));
        }

        let mut weights = Array2::zeros((n, n));
        for i in 0..n {
            for j in 0..n {
                if i != j {
                    let dist = distances[[i, j]];
                    if dist < f64::EPSILON {
                        return Err(AnalyticsError::invalid_input(
                            "Distance matrix contains zero distances",
                        ));
                    }
                    weights[[i, j]] = 1.0 / dist.powf(power);
                }
            }
        }

        Ok(Self {
            weights,
            row_standardized: false,
        })
    }

    /// Row-standardize the weights matrix
    ///
    /// Each row sums to 1.0 (or 0.0 if the row was all zeros)
    pub fn row_standardize(&mut self) {
        let n = self.weights.nrows();
        for i in 0..n {
            let row_sum: f64 = self.weights.row(i).sum();
            if row_sum > f64::EPSILON {
                for j in 0..n {
                    self.weights[[i, j]] /= row_sum;
                }
            }
        }
        self.row_standardized = true;
    }

    /// Get number of neighbors for each observation
    #[must_use]
    pub fn n_neighbors(&self) -> Array1<usize> {
        let n = self.weights.nrows();
        let mut neighbors = Array1::zeros(n);

        for i in 0..n {
            let count = self
                .weights
                .row(i)
                .iter()
                .filter(|&&w| w > f64::EPSILON)
                .count();
            neighbors[i] = count;
        }

        neighbors
    }

    /// Calculate spatial lag (weighted average of neighbors)
    ///
    /// # Arguments
    /// * `values` - Values for each observation
    ///
    /// # Errors
    /// Returns error if dimension mismatch
    pub fn spatial_lag(&self, values: &ArrayView1<f64>) -> Result<Array1<f64>> {
        let n = self.weights.nrows();
        if values.len() != n {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{}", n),
                format!("{}", values.len()),
            ));
        }

        let mut lag = Array1::zeros(n);
        for i in 0..n {
            let mut weighted_sum = 0.0;
            for j in 0..n {
                weighted_sum += self.weights[[i, j]] * values[j];
            }
            lag[i] = weighted_sum;
        }

        Ok(lag)
    }
}

/// Calculate spatial lag for raw weights (utility function)
///
/// # Arguments
/// * `values` - Values for each observation
/// * `weights` - Spatial weights matrix
///
/// # Errors
/// Returns error if dimensions don't match
pub fn calculate_spatial_lag(
    values: &ArrayView1<f64>,
    weights: &ArrayView2<f64>,
) -> Result<Array1<f64>> {
    let n = weights.nrows();
    if values.len() != n || weights.ncols() != n {
        return Err(AnalyticsError::dimension_mismatch(
            format!("{}", n),
            format!("{}x{}", weights.nrows(), weights.ncols()),
        ));
    }

    let mut lag = Array1::zeros(n);
    for i in 0..n {
        let mut sum = 0.0;
        for j in 0..n {
            sum += weights[[i, j]] * values[j];
        }
        lag[i] = sum;
    }

    Ok(lag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_spatial_weights_from_adjacency() {
        let adj = array![[0.0, 1.0, 0.0], [1.0, 0.0, 1.0], [0.0, 1.0, 0.0]];

        let weights = SpatialWeights::from_adjacency(adj)
            .expect("Failed to create spatial weights from adjacency matrix");
        assert_eq!(weights.weights.nrows(), 3);
        assert!(!weights.row_standardized);
    }

    #[test]
    fn test_spatial_weights_distance_threshold() {
        let distances = array![[0.0, 1.0, 5.0], [1.0, 0.0, 2.0], [5.0, 2.0, 0.0]];

        let weights = SpatialWeights::from_distance_threshold(&distances.view(), 2.5)
            .expect("Failed to create spatial weights from distance threshold");

        assert_eq!(weights.weights[[0, 1]], 1.0);
        assert_eq!(weights.weights[[1, 2]], 1.0);
        assert_eq!(weights.weights[[0, 2]], 0.0);
    }

    #[test]
    fn test_row_standardize() {
        let adj = array![[0.0, 1.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 0.0]];

        let mut weights = SpatialWeights::from_adjacency(adj)
            .expect("Failed to create spatial weights for row standardization test");
        weights.row_standardize();

        assert!(weights.row_standardized);
        assert_abs_diff_eq!(weights.weights.row(0).sum(), 1.0, epsilon = 1e-10);
        assert_abs_diff_eq!(weights.weights.row(1).sum(), 1.0, epsilon = 1e-10);
    }

    #[test]
    fn test_spatial_lag() {
        let adj = array![[0.0, 1.0, 0.0], [1.0, 0.0, 1.0], [0.0, 1.0, 0.0]];
        let values = array![1.0, 2.0, 3.0];

        let weights = SpatialWeights::from_adjacency(adj)
            .expect("Failed to create spatial weights for spatial lag test");
        let lag = weights
            .spatial_lag(&values.view())
            .expect("Failed to calculate spatial lag");

        assert_abs_diff_eq!(lag[0], 2.0, epsilon = 1e-10); // Neighbor value
        assert_abs_diff_eq!(lag[1], 4.0, epsilon = 1e-10); // Sum of neighbors
        assert_abs_diff_eq!(lag[2], 2.0, epsilon = 1e-10); // Neighbor value
    }

    #[test]
    fn test_n_neighbors() {
        let adj = array![[0.0, 1.0, 1.0], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0]];

        let weights = SpatialWeights::from_adjacency(adj)
            .expect("Failed to create spatial weights for n_neighbors test");
        let neighbors = weights.n_neighbors();

        assert_eq!(neighbors[0], 2);
        assert_eq!(neighbors[1], 1);
        assert_eq!(neighbors[2], 1);
    }
}
