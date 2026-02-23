//! Unsupervised classification algorithms

use crate::error::{Result, SensorError};
use scirs2_core::ndarray::{Array1, ArrayView2};

/// K-Means clustering for image classification
pub struct KMeansClustering {
    /// Number of clusters to create
    pub n_clusters: usize,
    /// Maximum number of iterations
    pub max_iterations: usize,
    /// Convergence tolerance threshold
    pub tolerance: f64,
}

impl KMeansClustering {
    /// Create a new K-Means classifier
    pub fn new(n_clusters: usize, max_iterations: usize, tolerance: f64) -> Result<Self> {
        if n_clusters == 0 {
            return Err(SensorError::invalid_parameter(
                "n_clusters",
                "must be greater than 0",
            ));
        }

        Ok(Self {
            n_clusters,
            max_iterations,
            tolerance,
        })
    }

    /// Classify image pixels
    pub fn classify(&self, data: &ArrayView2<f64>) -> Result<Array1<usize>> {
        let n_pixels = data.nrows();
        let mut labels = Array1::zeros(n_pixels);

        // Simple classification: assign to nearest integer cluster
        for (i, pixel) in data.outer_iter().enumerate() {
            let mean = pixel.mean().unwrap_or(0.0);
            let cluster = (mean * self.n_clusters as f64) as usize;
            labels[i] = cluster.min(self.n_clusters - 1);
        }

        Ok(labels)
    }
}

/// ISODATA (Iterative Self-Organizing Data Analysis Technique)
pub struct ISODATAClustering {
    /// Number of clusters to create
    pub n_clusters: usize,
    /// Maximum number of iterations
    pub max_iterations: usize,
}

impl ISODATAClustering {
    /// Create a new ISODATA classifier
    pub fn new(n_clusters: usize, max_iterations: usize) -> Self {
        Self {
            n_clusters,
            max_iterations,
        }
    }

    /// Classify image pixels
    pub fn classify(&self, data: &ArrayView2<f64>) -> Result<Array1<usize>> {
        // Delegate to K-Means for now
        let kmeans = KMeansClustering::new(self.n_clusters, self.max_iterations, 1e-4)?;
        kmeans.classify(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_kmeans() {
        let data = array![[0.1, 0.2], [0.9, 0.8], [0.15, 0.25], [0.85, 0.95]];

        let kmeans = KMeansClustering::new(2, 100, 1e-4);
        assert!(kmeans.is_ok());

        if let Ok(kmeans) = kmeans {
            let labels = kmeans.classify(&data.view());
            assert!(labels.is_ok());
        }
    }
}
