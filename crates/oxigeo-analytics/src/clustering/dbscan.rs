//! DBSCAN Clustering Algorithm
//!
//! Density-Based Spatial Clustering of Applications with Noise (DBSCAN).
//! Useful for finding clusters of arbitrary shape and detecting spatial outliers.

use crate::error::{AnalyticsError, Result};
use scirs2_core::ndarray::{Array1, Array2, ArrayView2};
use std::collections::VecDeque;

/// DBSCAN clustering result
#[derive(Debug, Clone)]
pub struct DbscanResult {
    /// Cluster assignments for each point (-1 for noise)
    pub labels: Array1<i32>,
    /// Number of clusters found (excluding noise)
    pub n_clusters: usize,
    /// Number of noise points
    pub n_noise: usize,
    /// Core point mask (true if point is a core point)
    pub core_points: Array1<bool>,
}

/// DBSCAN clustering algorithm
pub struct DbscanClusterer {
    eps: f64,
    min_samples: usize,
}

impl DbscanClusterer {
    /// Create a new DBSCAN clusterer
    ///
    /// # Arguments
    /// * `eps` - Maximum distance between two points to be considered neighbors
    /// * `min_samples` - Minimum number of points to form a dense region (core point)
    pub fn new(eps: f64, min_samples: usize) -> Self {
        Self { eps, min_samples }
    }

    /// Fit DBSCAN clustering to data
    ///
    /// # Arguments
    /// * `data` - Feature matrix (n_samples × n_features)
    ///
    /// # Errors
    /// Returns error if clustering fails or data is invalid
    pub fn fit(&self, data: &ArrayView2<f64>) -> Result<DbscanResult> {
        let n_samples = data.nrows();

        if n_samples == 0 {
            return Err(AnalyticsError::insufficient_data("Data is empty"));
        }

        if self.eps <= 0.0 {
            return Err(AnalyticsError::invalid_parameter("eps", "must be positive"));
        }

        if self.min_samples == 0 {
            return Err(AnalyticsError::invalid_parameter(
                "min_samples",
                "must be positive",
            ));
        }

        // Build distance matrix (or use spatial index for large datasets)
        let distances = self.compute_distances(data)?;

        // Find neighbors for each point
        let neighbors = self.find_neighbors(&distances);

        // Initialize labels (-1 for unvisited/noise)
        let mut labels = Array1::from_elem(n_samples, -1);
        let mut core_points = Array1::from_elem(n_samples, false);
        let mut cluster_id = 0;

        // DBSCAN main loop
        for i in 0..n_samples {
            // Skip if already processed
            if labels[i] != -1 {
                continue;
            }

            // Check if this is a core point
            if neighbors[i].len() < self.min_samples {
                continue; // Leave as noise for now
            }

            // Start new cluster from core point
            core_points[i] = true;
            self.expand_cluster(i, cluster_id, &neighbors, &mut labels, &mut core_points)?;
            cluster_id += 1;
        }

        let n_clusters = cluster_id as usize;
        let n_noise = labels.iter().filter(|&&x| x == -1).count();

        Ok(DbscanResult {
            labels,
            n_clusters,
            n_noise,
            core_points,
        })
    }

    /// Compute pairwise distances between all points
    fn compute_distances(&self, data: &ArrayView2<f64>) -> Result<Array2<f64>> {
        let n_samples = data.nrows();
        let mut distances = Array2::zeros((n_samples, n_samples));

        for i in 0..n_samples {
            for j in (i + 1)..n_samples {
                let dist = euclidean_distance(&data.row(i), &data.row(j))?;
                distances[[i, j]] = dist;
                distances[[j, i]] = dist;
            }
        }

        Ok(distances)
    }

    /// Find neighbors within eps distance for each point
    fn find_neighbors(&self, distances: &Array2<f64>) -> Vec<Vec<usize>> {
        let n_samples = distances.nrows();
        let mut neighbors = Vec::with_capacity(n_samples);

        for i in 0..n_samples {
            let mut point_neighbors = Vec::new();
            for j in 0..n_samples {
                if i != j && distances[[i, j]] <= self.eps {
                    point_neighbors.push(j);
                }
            }
            neighbors.push(point_neighbors);
        }

        neighbors
    }

    /// Expand cluster from a core point using BFS
    fn expand_cluster(
        &self,
        start: usize,
        cluster_id: i32,
        neighbors: &[Vec<usize>],
        labels: &mut Array1<i32>,
        core_points: &mut Array1<bool>,
    ) -> Result<()> {
        let mut queue = VecDeque::new();
        queue.push_back(start);
        labels[start] = cluster_id;

        while let Some(point) = queue.pop_front() {
            // Check if this is a core point
            if neighbors[point].len() < self.min_samples {
                continue;
            }

            core_points[point] = true;

            // Add unvisited neighbors to cluster
            for &neighbor in &neighbors[point] {
                if labels[neighbor] == -1 {
                    // Unvisited point
                    labels[neighbor] = cluster_id;
                    queue.push_back(neighbor);
                } else if labels[neighbor] == -2 {
                    // Border point of another cluster (shouldn't happen in standard DBSCAN)
                    labels[neighbor] = cluster_id;
                }
            }
        }

        Ok(())
    }
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
    use scirs2_core::ndarray::array;

    #[test]
    fn test_dbscan_simple() {
        // Create data with 2 clear clusters and 1 outlier
        let data = array![
            [0.0, 0.0],
            [0.1, 0.1],
            [0.2, 0.0],
            [10.0, 10.0],
            [10.1, 10.1],
            [10.0, 10.2],
            [5.0, 5.0], // Outlier
        ];

        let clusterer = DbscanClusterer::new(0.5, 2);
        let result = clusterer
            .fit(&data.view())
            .expect("DBSCAN clustering should succeed for valid data");

        assert_eq!(result.n_clusters, 2);
        assert!(result.n_noise > 0); // Outlier should be noise
        assert!(result.labels.iter().any(|&x| x == -1)); // Has noise points
    }

    #[test]
    fn test_dbscan_all_noise() {
        // Points too far apart to form clusters
        let data = array![[0.0, 0.0], [10.0, 10.0], [20.0, 20.0]];

        let clusterer = DbscanClusterer::new(0.5, 2);
        let result = clusterer
            .fit(&data.view())
            .expect("DBSCAN should succeed even when all points are noise");

        assert_eq!(result.n_clusters, 0);
        assert_eq!(result.n_noise, 3);
    }

    #[test]
    fn test_dbscan_single_cluster() {
        // All points form one cluster
        let data = array![[0.0, 0.0], [0.1, 0.0], [0.2, 0.0], [0.3, 0.0], [0.4, 0.0],];

        let clusterer = DbscanClusterer::new(0.15, 2);
        let result = clusterer
            .fit(&data.view())
            .expect("DBSCAN should succeed for single cluster data");

        assert_eq!(result.n_clusters, 1);
        assert_eq!(result.n_noise, 0);
    }

    #[test]
    fn test_dbscan_invalid_params() {
        let data = array![[1.0, 2.0]];
        let clusterer = DbscanClusterer::new(-1.0, 2);
        let result = clusterer.fit(&data.view());

        assert!(result.is_err());
    }
}
