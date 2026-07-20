//! Inverse Distance Weighting (IDW) Interpolation
//!
//! IDW is a deterministic interpolation method that estimates values
//! based on weighted average of nearby known points.

use crate::error::{AnalyticsError, Result};
use scirs2_core::ndarray::{Array1, Array2, ArrayView1};

/// IDW interpolation result
#[derive(Debug, Clone)]
pub struct IdwResult {
    /// Interpolated values at target locations
    pub values: Array1<f64>,
    /// Target coordinates
    pub coordinates: Array2<f64>,
}

/// Inverse Distance Weighting interpolator
pub struct IdwInterpolator {
    power: f64,
    min_neighbors: usize,
    max_neighbors: Option<usize>,
    max_distance: Option<f64>,
}

impl IdwInterpolator {
    /// Create a new IDW interpolator
    ///
    /// # Arguments
    /// * `power` - Power parameter (typically 1-3, default 2)
    pub fn new(power: f64) -> Self {
        Self {
            power,
            min_neighbors: 1,
            max_neighbors: None,
            max_distance: None,
        }
    }

    /// Set minimum number of neighbors
    pub fn with_min_neighbors(mut self, min: usize) -> Self {
        self.min_neighbors = min;
        self
    }

    /// Set maximum number of neighbors
    pub fn with_max_neighbors(mut self, max: usize) -> Self {
        self.max_neighbors = Some(max);
        self
    }

    /// Set maximum search distance
    pub fn with_max_distance(mut self, dist: f64) -> Self {
        self.max_distance = Some(dist);
        self
    }

    /// Interpolate values at target locations
    ///
    /// # Arguments
    /// * `points` - Known point coordinates (n_points × n_dim)
    /// * `values` - Known values at points (n_points)
    /// * `targets` - Target coordinates for interpolation (n_targets × n_dim)
    ///
    /// # Errors
    /// Returns error if dimensions don't match or interpolation fails
    pub fn interpolate(
        &self,
        points: &Array2<f64>,
        values: &ArrayView1<f64>,
        targets: &Array2<f64>,
    ) -> Result<IdwResult> {
        let n_points = points.nrows();
        let n_targets = targets.nrows();
        let n_dim = points.ncols();

        if values.len() != n_points {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{}", n_points),
                format!("{}", values.len()),
            ));
        }

        if targets.ncols() != n_dim {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{}", n_dim),
                format!("{}", targets.ncols()),
            ));
        }

        if n_points < self.min_neighbors {
            return Err(AnalyticsError::insufficient_data(format!(
                "Need at least {} points for interpolation",
                self.min_neighbors
            )));
        }

        let mut interpolated = Array1::zeros(n_targets);

        for i in 0..n_targets {
            let target = targets.row(i);
            interpolated[i] = self.interpolate_point(&target, points, values)?;
        }

        Ok(IdwResult {
            values: interpolated,
            coordinates: targets.clone(),
        })
    }

    /// Interpolate value at a single point
    fn interpolate_point(
        &self,
        target: &scirs2_core::ndarray::ArrayView1<f64>,
        points: &Array2<f64>,
        values: &ArrayView1<f64>,
    ) -> Result<f64> {
        let n_points = points.nrows();

        // Calculate distances to all points
        let mut distances = Vec::with_capacity(n_points);
        for i in 0..n_points {
            let point = points.row(i);
            let dist = euclidean_distance(target, &point)?;

            // Check if target is exactly at a known point
            if dist < f64::EPSILON {
                return Ok(values[i]);
            }

            // Filter by maximum distance if specified
            if let Some(max_dist) = self.max_distance {
                if dist <= max_dist {
                    distances.push((i, dist));
                }
            } else {
                distances.push((i, dist));
            }
        }

        if distances.is_empty() {
            return Err(AnalyticsError::insufficient_data(
                "No points within maximum distance",
            ));
        }

        // Sort by distance and limit to max_neighbors if specified
        distances.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        if let Some(max_n) = self.max_neighbors {
            distances.truncate(max_n);
        }

        if distances.len() < self.min_neighbors {
            return Err(AnalyticsError::insufficient_data(format!(
                "Found only {} neighbors, need at least {}",
                distances.len(),
                self.min_neighbors
            )));
        }

        // Calculate IDW weights and interpolated value
        let mut weighted_sum = 0.0;
        let mut weight_sum = 0.0;

        for (idx, dist) in distances {
            let weight = 1.0 / dist.powf(self.power);
            weighted_sum += weight * values[idx];
            weight_sum += weight;
        }

        if weight_sum < f64::EPSILON {
            return Err(AnalyticsError::numerical_instability(
                "Sum of weights is too small",
            ));
        }

        Ok(weighted_sum / weight_sum)
    }

    /// Perform leave-one-out cross-validation
    ///
    /// # Arguments
    /// * `points` - Known point coordinates
    /// * `values` - Known values
    ///
    /// # Errors
    /// Returns error if validation fails
    pub fn cross_validate(
        &self,
        points: &Array2<f64>,
        values: &ArrayView1<f64>,
    ) -> Result<CrossValidationResult> {
        let n = points.nrows();
        let mut predictions = Array1::zeros(n);
        let mut errors = Array1::zeros(n);

        for i in 0..n {
            // Create temporary arrays excluding point i
            let mut temp_points = Vec::new();
            let mut temp_values = Vec::new();

            for j in 0..n {
                if i != j {
                    temp_points.extend(points.row(j).iter());
                    temp_values.push(values[j]);
                }
            }

            let temp_points_array = Array2::from_shape_vec((n - 1, points.ncols()), temp_points)
                .map_err(|_| AnalyticsError::matrix_error("Failed to create temporary array"))?;

            let temp_values_array = Array1::from_vec(temp_values);

            // Predict value at point i
            let target = points.row(i).to_owned();
            let pred = self.interpolate_point(
                &target.view(),
                &temp_points_array,
                &temp_values_array.view(),
            )?;

            predictions[i] = pred;
            errors[i] = pred - values[i];
        }

        // Calculate validation metrics
        let mae = errors.iter().map(|x| x.abs()).sum::<f64>() / (n as f64);
        let rmse = (errors.iter().map(|x| x.powi(2)).sum::<f64>() / (n as f64)).sqrt();

        let mean_observed = values.sum() / (n as f64);
        let ss_tot: f64 = values.iter().map(|x| (x - mean_observed).powi(2)).sum();
        let ss_res: f64 = errors.iter().map(|x| x.powi(2)).sum();

        let r_squared = if ss_tot > f64::EPSILON {
            1.0 - (ss_res / ss_tot)
        } else {
            0.0
        };

        Ok(CrossValidationResult {
            predictions,
            errors,
            mae,
            rmse,
            r_squared,
        })
    }
}

/// Cross-validation result
#[derive(Debug, Clone)]
pub struct CrossValidationResult {
    /// Predicted values
    pub predictions: Array1<f64>,
    /// Prediction errors
    pub errors: Array1<f64>,
    /// Mean Absolute Error
    pub mae: f64,
    /// Root Mean Squared Error
    pub rmse: f64,
    /// R-squared coefficient
    pub r_squared: f64,
}

/// Calculate euclidean distance between two points
fn euclidean_distance(
    p1: &scirs2_core::ndarray::ArrayView1<f64>,
    p2: &scirs2_core::ndarray::ArrayView1<f64>,
) -> Result<f64> {
    if p1.len() != p2.len() {
        return Err(AnalyticsError::dimension_mismatch(
            format!("{}", p1.len()),
            format!("{}", p2.len()),
        ));
    }

    let dist_sq: f64 = p1.iter().zip(p2.iter()).map(|(a, b)| (a - b).powi(2)).sum();

    Ok(dist_sq.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use scirs2_core::ndarray::{Array, array};

    #[test]
    fn test_idw_simple() {
        let points = array![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
        let values = array![1.0, 2.0, 3.0, 4.0];
        let targets = array![[0.5, 0.5]]; // Center point

        let interpolator = IdwInterpolator::new(2.0);
        let result = interpolator
            .interpolate(&points, &values.view(), &targets)
            .expect("IDW interpolation should succeed for valid data");

        assert_eq!(result.values.len(), 1);
        // Center should be approximately the average
        assert!(result.values[0] > 2.0 && result.values[0] < 3.0);
    }

    #[test]
    fn test_idw_exact_point() {
        let points = array![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let values = array![1.0, 2.0, 3.0];
        let targets = array![[0.0, 0.0]]; // Exact match with first point

        let interpolator = IdwInterpolator::new(2.0);
        let result = interpolator
            .interpolate(&points, &values.view(), &targets)
            .expect("IDW interpolation should succeed for exact point match");

        assert_abs_diff_eq!(result.values[0], 1.0, epsilon = 1e-10);
    }

    #[test]
    fn test_idw_max_neighbors() {
        let points = array![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0], [2.0, 2.0]];
        let values = array![1.0, 2.0, 3.0, 4.0, 5.0];
        let targets = array![[0.5, 0.5]];

        let interpolator = IdwInterpolator::new(2.0).with_max_neighbors(2);
        let result = interpolator
            .interpolate(&points, &values.view(), &targets)
            .expect("IDW interpolation should succeed with max neighbors constraint");

        assert_eq!(result.values.len(), 1);
    }

    #[test]
    fn test_cross_validation() {
        // Use more points with better spatial pattern for IDW
        let points = array![
            [0.0, 0.0],
            [1.0, 0.0],
            [2.0, 0.0],
            [0.0, 1.0],
            [1.0, 1.0],
            [2.0, 1.0]
        ];
        // Values increase smoothly in x direction (good for IDW)
        let values = array![1.0, 2.0, 3.0, 1.0, 2.0, 3.0];

        let interpolator = IdwInterpolator::new(2.0);
        let cv_result = interpolator
            .cross_validate(&points, &values.view())
            .expect("Cross-validation should succeed for valid data");

        assert_eq!(cv_result.predictions.len(), 6);
        assert!(cv_result.rmse > 0.0);
        // R-squared can be negative if model performs poorly, so just check it's reasonable
        assert!(cv_result.r_squared >= -1.0 && cv_result.r_squared <= 1.0);
    }
}
