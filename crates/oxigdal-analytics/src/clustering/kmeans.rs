//! K-Means Clustering Algorithm
//!
//! Implementation of the K-means clustering algorithm using Lloyd's algorithm.
//! Suitable for image classification and general clustering tasks.

use crate::error::{AnalyticsError, Result};
use scirs2_core::ndarray::{Array1, Array2, ArrayView2, Axis};
use scirs2_core::random::Rng;

/// K-means clustering result
#[derive(Debug, Clone)]
pub struct KMeansResult {
    /// Cluster assignments for each point
    pub labels: Array1<i32>,
    /// Cluster centers (k × n_features)
    pub centers: Array2<f64>,
    /// Within-cluster sum of squares
    pub inertia: f64,
    /// Number of iterations performed
    pub n_iterations: usize,
    /// Whether the algorithm converged
    pub converged: bool,
}

/// K-means clustering algorithm
pub struct KMeansClusterer {
    n_clusters: usize,
    max_iterations: usize,
    tolerance: f64,
    init_method: InitMethod,
}

/// Initialization methods for K-means
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitMethod {
    /// Random initialization
    Random,
    /// K-means++ initialization (better initial centers)
    KMeansPlusPlus,
}

impl KMeansClusterer {
    /// Create a new K-means clusterer
    ///
    /// # Arguments
    /// * `n_clusters` - Number of clusters
    /// * `max_iterations` - Maximum number of iterations
    /// * `tolerance` - Convergence tolerance for center movement
    pub fn new(n_clusters: usize, max_iterations: usize, tolerance: f64) -> Self {
        Self {
            n_clusters,
            max_iterations,
            tolerance,
            init_method: InitMethod::KMeansPlusPlus,
        }
    }

    /// Set initialization method
    pub fn with_init_method(mut self, method: InitMethod) -> Self {
        self.init_method = method;
        self
    }

    /// Fit K-means clustering to data
    ///
    /// # Arguments
    /// * `data` - Feature matrix (n_samples × n_features)
    ///
    /// # Errors
    /// Returns error if clustering fails or data is invalid
    pub fn fit(&self, data: &ArrayView2<f64>) -> Result<KMeansResult> {
        let (n_samples, _n_features) = data.dim();

        if n_samples < self.n_clusters {
            return Err(AnalyticsError::insufficient_data(format!(
                "Need at least {} samples for {} clusters",
                self.n_clusters, self.n_clusters
            )));
        }

        // Initialize centers
        let mut centers = match self.init_method {
            InitMethod::Random => self.initialize_random(data)?,
            InitMethod::KMeansPlusPlus => self.initialize_kmeans_plus_plus(data)?,
        };

        let mut labels = Array1::zeros(n_samples);
        let mut converged = false;

        // Lloyd's algorithm
        for iteration in 0..self.max_iterations {
            // Assignment step: assign each point to nearest center
            let mut changed = false;
            for i in 0..n_samples {
                let point = data.row(i);
                let nearest = self.find_nearest_center(&point, &centers)?;
                if labels[i] != nearest {
                    labels[i] = nearest;
                    changed = true;
                }
            }

            if !changed {
                converged = true;
                tracing::debug!("K-means converged after {} iterations", iteration);
                break;
            }

            // Update step: recalculate centers
            let old_centers = centers.clone();
            centers = self.update_centers(data, &labels)?;

            // Check convergence based on center movement
            let max_movement = self.max_center_movement(&old_centers, &centers)?;
            if max_movement < self.tolerance {
                converged = true;
                tracing::debug!(
                    "K-means converged after {} iterations (max movement: {})",
                    iteration,
                    max_movement
                );
                break;
            }
        }

        // Calculate inertia (within-cluster sum of squares)
        let inertia = self.calculate_inertia(data, &labels, &centers)?;

        Ok(KMeansResult {
            labels,
            centers,
            inertia,
            n_iterations: self.max_iterations,
            converged,
        })
    }

    /// Initialize centers randomly
    fn initialize_random(&self, data: &ArrayView2<f64>) -> Result<Array2<f64>> {
        let (n_samples, n_features) = data.dim();
        let mut rng = scirs2_core::random::thread_rng();

        let mut centers = Array2::zeros((self.n_clusters, n_features));
        let mut used_indices = Vec::new();

        for i in 0..self.n_clusters {
            // Select a random sample that hasn't been used
            let idx = loop {
                let candidate = rng.gen_range(0..n_samples);
                if !used_indices.contains(&candidate) {
                    break candidate;
                }
            };
            used_indices.push(idx);

            centers.row_mut(i).assign(&data.row(idx));
        }

        Ok(centers)
    }

    /// Initialize centers using K-means++ algorithm
    ///
    /// This gives better initial centers by choosing them probabilistically
    /// based on distance from existing centers.
    fn initialize_kmeans_plus_plus(&self, data: &ArrayView2<f64>) -> Result<Array2<f64>> {
        let (n_samples, n_features) = data.dim();
        let mut rng = scirs2_core::random::thread_rng();

        let mut centers = Array2::zeros((self.n_clusters, n_features));

        // Choose first center randomly
        let first_idx = rng.gen_range(0..n_samples);
        centers.row_mut(0).assign(&data.row(first_idx));

        // Choose remaining centers
        for i in 1..self.n_clusters {
            // Calculate distances to nearest existing center
            let mut distances = Vec::with_capacity(n_samples);
            let mut distance_sum = 0.0;

            for j in 0..n_samples {
                let point = data.row(j);
                let mut min_dist = f64::INFINITY;

                for k in 0..i {
                    let center = centers.row(k);
                    let dist = euclidean_distance_squared(&point, &center)?;
                    min_dist = min_dist.min(dist);
                }

                distances.push(min_dist);
                distance_sum += min_dist;
            }

            // Choose next center with probability proportional to squared distance
            let threshold = rng.gen_range(0.0..distance_sum);
            let mut cumsum = 0.0;
            let mut next_idx = 0;

            for (j, &dist) in distances.iter().enumerate() {
                cumsum += dist;
                if cumsum >= threshold {
                    next_idx = j;
                    break;
                }
            }

            centers.row_mut(i).assign(&data.row(next_idx));
        }

        Ok(centers)
    }

    /// Find nearest center for a point
    fn find_nearest_center(
        &self,
        point: &scirs2_core::ndarray::ArrayView1<f64>,
        centers: &Array2<f64>,
    ) -> Result<i32> {
        let mut min_dist = f64::INFINITY;
        let mut nearest = 0;

        for (i, center) in centers.axis_iter(Axis(0)).enumerate() {
            let dist = euclidean_distance_squared(point, &center)?;
            if dist < min_dist {
                min_dist = dist;
                nearest = i;
            }
        }

        Ok(nearest as i32)
    }

    /// Update cluster centers based on current assignments
    fn update_centers(&self, data: &ArrayView2<f64>, labels: &Array1<i32>) -> Result<Array2<f64>> {
        let (n_samples, n_features) = data.dim();
        let mut new_centers = Array2::zeros((self.n_clusters, n_features));
        let mut counts = vec![0; self.n_clusters];

        // Sum up points in each cluster
        for i in 0..n_samples {
            let cluster = labels[i] as usize;
            if cluster < self.n_clusters {
                for j in 0..n_features {
                    new_centers[[cluster, j]] += data[[i, j]];
                }
                counts[cluster] += 1;
            }
        }

        // Average to get new centers
        for i in 0..self.n_clusters {
            if counts[i] > 0 {
                for j in 0..n_features {
                    new_centers[[i, j]] /= counts[i] as f64;
                }
            } else {
                // Handle empty cluster by reinitializing
                tracing::warn!("Cluster {} is empty, reinitializing", i);
                // Keep the old center or initialize randomly
            }
        }

        Ok(new_centers)
    }

    /// Calculate maximum movement of centers
    fn max_center_movement(
        &self,
        old_centers: &Array2<f64>,
        new_centers: &Array2<f64>,
    ) -> Result<f64> {
        let mut max_dist: f64 = 0.0;

        for i in 0..self.n_clusters {
            let dist = euclidean_distance_squared(&old_centers.row(i), &new_centers.row(i))?;
            max_dist = max_dist.max(dist);
        }

        Ok(max_dist.sqrt())
    }

    /// Calculate within-cluster sum of squares (inertia)
    fn calculate_inertia(
        &self,
        data: &ArrayView2<f64>,
        labels: &Array1<i32>,
        centers: &Array2<f64>,
    ) -> Result<f64> {
        let mut inertia = 0.0;

        for (i, &label) in labels.iter().enumerate() {
            let cluster = label as usize;
            if cluster < self.n_clusters {
                let point = data.row(i);
                let center = centers.row(cluster);
                inertia += euclidean_distance_squared(&point, &center)?;
            }
        }

        Ok(inertia)
    }
}

/// Calculate squared euclidean distance between two points
fn euclidean_distance_squared(
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

    Ok(dist_sq)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_kmeans_simple() {
        // Create simple 2-cluster data
        let data = array![
            [0.0, 0.0],
            [0.1, 0.1],
            [0.2, 0.0],
            [10.0, 10.0],
            [10.1, 10.1],
            [10.0, 10.2],
        ];

        let clusterer = KMeansClusterer::new(2, 100, 1e-4);
        let result = clusterer
            .fit(&data.view())
            .expect("K-means clustering should succeed for valid data");

        assert_eq!(result.labels.len(), 6);
        assert_eq!(result.centers.nrows(), 2);
        assert!(result.converged);

        // Check that similar points are in same cluster
        assert_eq!(result.labels[0], result.labels[1]);
        assert_eq!(result.labels[3], result.labels[4]);
        assert_ne!(result.labels[0], result.labels[3]);
    }

    #[test]
    fn test_kmeans_insufficient_data() {
        let data = array![[1.0, 2.0]];
        let clusterer = KMeansClusterer::new(2, 100, 1e-4);
        let result = clusterer.fit(&data.view());

        assert!(result.is_err());
    }

    #[test]
    fn test_kmeans_plus_plus_init() {
        let data = array![[0.0, 0.0], [1.0, 1.0], [10.0, 10.0], [11.0, 11.0],];

        let clusterer =
            KMeansClusterer::new(2, 100, 1e-4).with_init_method(InitMethod::KMeansPlusPlus);
        let result = clusterer
            .fit(&data.view())
            .expect("K-means++ initialization should succeed");

        assert!(result.converged);
        assert_eq!(result.labels.len(), 4);
    }
}
