//! Spatial Clustering Module
//!
//! This module provides clustering algorithms for geospatial data including:
//! - K-means clustering for image classification
//! - DBSCAN for spatial outlier detection
//! - Cluster validation and quality metrics

pub mod dbscan;
pub mod kmeans;

pub use dbscan::{DbscanClusterer, DbscanResult};
pub use kmeans::{KMeansClusterer, KMeansResult};

use crate::error::{AnalyticsError, Result};
use scirs2_core::ndarray::{Array1, Array2, ArrayView2};

/// Cluster assignment and statistics
#[derive(Debug, Clone)]
pub struct ClusterInfo {
    /// Cluster assignments for each point (-1 for noise/unassigned)
    pub labels: Array1<i32>,
    /// Number of clusters found
    pub n_clusters: usize,
    /// Cluster centers (if applicable)
    pub centers: Option<Array2<f64>>,
}

/// Calculate silhouette score for clustering quality
///
/// Silhouette coefficient is a measure of how similar an object is to its own cluster
/// compared to other clusters. Range: [-1, 1], higher is better.
///
/// # Arguments
/// * `data` - Feature matrix (n_samples × n_features)
/// * `labels` - Cluster assignments
///
/// # Errors
/// Returns error if computation fails or data is invalid
pub fn silhouette_score(data: &ArrayView2<f64>, labels: &Array1<i32>) -> Result<f64> {
    let n_samples = data.nrows();

    if n_samples != labels.len() {
        return Err(AnalyticsError::dimension_mismatch(
            format!("{}", n_samples),
            format!("{}", labels.len()),
        ));
    }

    if n_samples < 2 {
        return Err(AnalyticsError::insufficient_data(
            "Need at least 2 samples for silhouette score",
        ));
    }

    // Find unique clusters (excluding noise label -1)
    let mut unique_labels: Vec<i32> = labels.iter().copied().filter(|&x| x >= 0).collect();
    unique_labels.sort_unstable();
    unique_labels.dedup();

    if unique_labels.len() < 2 {
        return Err(AnalyticsError::insufficient_data(
            "Need at least 2 clusters for silhouette score",
        ));
    }

    let mut silhouette_sum = 0.0;
    let mut n_valid = 0;

    for i in 0..n_samples {
        let label_i = labels[i];
        if label_i < 0 {
            continue; // Skip noise points
        }

        // Calculate a(i): average distance to points in same cluster
        let mut a_sum = 0.0;
        let mut a_count = 0;
        for j in 0..n_samples {
            if i != j && labels[j] == label_i {
                a_sum += euclidean_distance(&data.row(i), &data.row(j))?;
                a_count += 1;
            }
        }
        let a_i = if a_count > 0 {
            a_sum / (a_count as f64)
        } else {
            0.0
        };

        // Calculate b(i): minimum average distance to points in other clusters
        let mut b_i = f64::INFINITY;
        for &other_label in &unique_labels {
            if other_label == label_i {
                continue;
            }

            let mut b_sum = 0.0;
            let mut b_count = 0;
            for j in 0..n_samples {
                if labels[j] == other_label {
                    b_sum += euclidean_distance(&data.row(i), &data.row(j))?;
                    b_count += 1;
                }
            }

            if b_count > 0 {
                let avg_dist = b_sum / (b_count as f64);
                b_i = b_i.min(avg_dist);
            }
        }

        // Calculate silhouette coefficient for point i
        if b_i.is_finite() && a_i.is_finite() {
            let max_val = a_i.max(b_i);
            if max_val > f64::EPSILON {
                silhouette_sum += (b_i - a_i) / max_val;
                n_valid += 1;
            }
        }
    }

    if n_valid == 0 {
        return Err(AnalyticsError::clustering_error(
            "No valid silhouette coefficients computed",
        ));
    }

    Ok(silhouette_sum / (n_valid as f64))
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

/// Calculate Davies-Bouldin Index for clustering quality
///
/// Lower values indicate better clustering.
///
/// # Arguments
/// * `data` - Feature matrix (n_samples × n_features)
/// * `labels` - Cluster assignments
/// * `centers` - Cluster centers
///
/// # Errors
/// Returns error if computation fails
pub fn davies_bouldin_index(
    data: &ArrayView2<f64>,
    labels: &Array1<i32>,
    centers: &Array2<f64>,
) -> Result<f64> {
    let n_clusters = centers.nrows();
    if n_clusters < 2 {
        return Err(AnalyticsError::insufficient_data(
            "Need at least 2 clusters for Davies-Bouldin index",
        ));
    }

    // Calculate average within-cluster distances
    let mut s = vec![0.0; n_clusters];
    let mut counts = vec![0; n_clusters];

    for (i, &label) in labels.iter().enumerate() {
        if label >= 0 {
            let cluster_idx = label as usize;
            if cluster_idx < n_clusters {
                let dist = euclidean_distance(&data.row(i), &centers.row(cluster_idx))?;
                s[cluster_idx] += dist;
                counts[cluster_idx] += 1;
            }
        }
    }

    for i in 0..n_clusters {
        if counts[i] > 0 {
            s[i] /= counts[i] as f64;
        }
    }

    // Calculate Davies-Bouldin index
    let mut db_sum = 0.0;
    for i in 0..n_clusters {
        let mut max_ratio: f64 = 0.0;
        for j in 0..n_clusters {
            if i != j {
                let m_ij = euclidean_distance(&centers.row(i), &centers.row(j))?;
                if m_ij > f64::EPSILON {
                    let ratio = (s[i] + s[j]) / m_ij;
                    max_ratio = max_ratio.max(ratio);
                }
            }
        }
        db_sum += max_ratio;
    }

    Ok(db_sum / (n_clusters as f64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_euclidean_distance() {
        let p1 = array![0.0, 0.0];
        let p2 = array![3.0, 4.0];
        let dist = euclidean_distance(&p1.view(), &p2.view())
            .expect("Euclidean distance calculation should succeed");
        assert_abs_diff_eq!(dist, 5.0, epsilon = 1e-10);
    }

    #[test]
    fn test_silhouette_score() {
        // Create simple 2-cluster data
        let data = array![[0.0, 0.0], [0.1, 0.1], [10.0, 10.0], [10.1, 10.1],];
        let labels = array![0, 0, 1, 1];

        let score = silhouette_score(&data.view(), &labels)
            .expect("Silhouette score calculation should succeed");
        assert!(score > 0.5); // Should have good separation
    }

    #[test]
    fn test_davies_bouldin_index() {
        let data = array![[0.0, 0.0], [0.1, 0.1], [10.0, 10.0], [10.1, 10.1],];
        let labels = array![0, 0, 1, 1];
        let centers = array![[0.05, 0.05], [10.05, 10.05]];

        let db_index = davies_bouldin_index(&data.view(), &labels, &centers)
            .expect("Davies-Bouldin index calculation should succeed");
        assert!(db_index < 1.0); // Should be relatively low for well-separated clusters
    }
}
