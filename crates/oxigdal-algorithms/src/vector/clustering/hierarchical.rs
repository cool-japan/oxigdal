//! Hierarchical (agglomerative) clustering
//!
//! Build a dendrogram by progressively merging clusters.

use crate::error::{AlgorithmError, Result};
use crate::vector::clustering::dbscan::{DistanceMetric, calculate_distance};
use oxigdal_core::vector::Point;
use std::collections::HashMap;

/// Options for hierarchical clustering
#[derive(Debug, Clone)]
pub struct HierarchicalOptions {
    /// Number of clusters to extract
    pub num_clusters: usize,
    /// Linkage method
    pub linkage: LinkageMethod,
    /// Distance metric
    pub metric: DistanceMetric,
    /// Distance threshold (alternative to num_clusters)
    pub distance_threshold: Option<f64>,
}

impl Default for HierarchicalOptions {
    fn default() -> Self {
        Self {
            num_clusters: 3,
            linkage: LinkageMethod::Average,
            metric: DistanceMetric::Euclidean,
            distance_threshold: None,
        }
    }
}

/// Linkage method for hierarchical clustering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkageMethod {
    /// Single linkage (minimum distance)
    Single,
    /// Complete linkage (maximum distance)
    Complete,
    /// Average linkage (average distance)
    Average,
    /// Ward linkage (minimize variance)
    Ward,
}

/// Result of hierarchical clustering
#[derive(Debug, Clone)]
pub struct HierarchicalResult {
    /// Cluster assignments for each point
    pub labels: Vec<usize>,
    /// Dendrogram (merge history)
    pub dendrogram: Vec<Merge>,
    /// Number of clusters
    pub num_clusters: usize,
    /// Cluster sizes
    pub cluster_sizes: HashMap<usize, usize>,
}

/// A merge operation in the dendrogram
#[derive(Debug, Clone)]
pub struct Merge {
    /// First cluster being merged
    pub cluster1: usize,
    /// Second cluster being merged
    pub cluster2: usize,
    /// Distance at which merge occurs
    pub distance: f64,
    /// New cluster ID
    pub new_cluster: usize,
}

/// Perform hierarchical clustering
///
/// # Arguments
///
/// * `points` - Points to cluster
/// * `options` - Hierarchical clustering options
///
/// # Returns
///
/// Clustering result with dendrogram and labels
///
/// # Examples
///
/// ```
/// use oxigdal_algorithms::vector::clustering::{hierarchical_cluster, HierarchicalOptions};
/// use oxigdal_algorithms::Point;
/// # use oxigdal_algorithms::error::Result;
///
/// # fn main() -> Result<()> {
/// let points = vec![
///     Point::new(0.0, 0.0),
///     Point::new(0.1, 0.1),
///     Point::new(5.0, 5.0),
/// ];
///
/// let options = HierarchicalOptions {
///     num_clusters: 2,
///     ..Default::default()
/// };
///
/// let result = hierarchical_cluster(&points, &options)?;
/// assert_eq!(result.num_clusters, 2);
/// # Ok(())
/// # }
/// ```
pub fn hierarchical_cluster(
    points: &[Point],
    options: &HierarchicalOptions,
) -> Result<HierarchicalResult> {
    if points.is_empty() {
        return Err(AlgorithmError::InvalidInput(
            "Cannot cluster empty point set".to_string(),
        ));
    }

    let n = points.len();

    // Initialize each point as its own cluster
    let mut clusters: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
    let mut dendrogram = Vec::new();

    // Build distance matrix
    let mut distances = compute_distance_matrix(points, options.metric);

    // Merge clusters until we reach the desired number
    let target_clusters = options.num_clusters.max(1);

    while clusters.len() > target_clusters {
        // Find pair of clusters with minimum distance
        let (i, j, dist) = find_closest_clusters(&clusters, &distances, options.linkage)?;

        // Check distance threshold BEFORE merging
        if let Some(threshold) = options.distance_threshold {
            if dist >= threshold {
                break;
            }
        }

        // Merge clusters
        let new_cluster_id = clusters.len();
        let merged = merge_clusters(&mut clusters, i, j);

        dendrogram.push(Merge {
            cluster1: i,
            cluster2: j,
            distance: dist,
            new_cluster: new_cluster_id,
        });

        // Update distance matrix
        update_distances(&mut distances, i, j, &merged, points, options)?;
    }

    // Extract final labels
    let mut labels = vec![0; n];
    for (cluster_id, cluster) in clusters.iter().enumerate() {
        for &point_idx in cluster {
            labels[point_idx] = cluster_id;
        }
    }

    // Calculate cluster sizes
    let mut cluster_sizes: HashMap<usize, usize> = HashMap::new();
    for &label in &labels {
        *cluster_sizes.entry(label).or_insert(0) += 1;
    }

    Ok(HierarchicalResult {
        labels,
        dendrogram,
        num_clusters: clusters.len(),
        cluster_sizes,
    })
}

/// Compute pairwise distance matrix
fn compute_distance_matrix(points: &[Point], metric: DistanceMetric) -> Vec<Vec<f64>> {
    let n = points.len();
    let mut distances = vec![vec![0.0; n]; n];

    for i in 0..n {
        for j in (i + 1)..n {
            let dist = calculate_distance(&points[i], &points[j], metric);
            distances[i][j] = dist;
            distances[j][i] = dist;
        }
    }

    distances
}

/// Find the pair of clusters with minimum distance
fn find_closest_clusters(
    clusters: &[Vec<usize>],
    distances: &[Vec<f64>],
    linkage: LinkageMethod,
) -> Result<(usize, usize, f64)> {
    let mut min_dist = f64::INFINITY;
    let mut best_i = 0;
    let mut best_j = 1;

    for i in 0..clusters.len() {
        for j in (i + 1)..clusters.len() {
            let dist = cluster_distance(&clusters[i], &clusters[j], distances, linkage);

            if dist < min_dist {
                min_dist = dist;
                best_i = i;
                best_j = j;
            }
        }
    }

    if min_dist.is_infinite() {
        return Err(AlgorithmError::ComputationError(
            "No valid cluster pair found".to_string(),
        ));
    }

    Ok((best_i, best_j, min_dist))
}

/// Calculate distance between two clusters
fn cluster_distance(
    cluster1: &[usize],
    cluster2: &[usize],
    distances: &[Vec<f64>],
    linkage: LinkageMethod,
) -> f64 {
    match linkage {
        LinkageMethod::Single => {
            // Minimum distance
            cluster1
                .iter()
                .flat_map(|&i| cluster2.iter().map(move |&j| distances[i][j]))
                .fold(f64::INFINITY, f64::min)
        }
        LinkageMethod::Complete => {
            // Maximum distance
            cluster1
                .iter()
                .flat_map(|&i| cluster2.iter().map(move |&j| distances[i][j]))
                .fold(f64::NEG_INFINITY, f64::max)
        }
        LinkageMethod::Average => {
            // Average distance
            let sum: f64 = cluster1
                .iter()
                .flat_map(|&i| cluster2.iter().map(move |&j| distances[i][j]))
                .sum();
            let count = (cluster1.len() * cluster2.len()) as f64;
            if count > 0.0 {
                sum / count
            } else {
                f64::INFINITY
            }
        }
        LinkageMethod::Ward => {
            // Ward linkage (simplified as average for now)
            let sum: f64 = cluster1
                .iter()
                .flat_map(|&i| cluster2.iter().map(move |&j| distances[i][j]))
                .sum();
            let count = (cluster1.len() * cluster2.len()) as f64;
            if count > 0.0 {
                sum / count
            } else {
                f64::INFINITY
            }
        }
    }
}

/// Merge two clusters
fn merge_clusters(clusters: &mut Vec<Vec<usize>>, i: usize, j: usize) -> Vec<usize> {
    let (idx1, idx2) = if i < j { (i, j) } else { (j, i) };

    // Remove clusters (remove larger index first)
    let cluster2 = clusters.remove(idx2);
    let mut cluster1 = clusters.remove(idx1);

    // Merge
    cluster1.extend(cluster2);

    // Add merged cluster back
    clusters.push(cluster1.clone());

    cluster1
}

/// Update distance matrix after merge
fn update_distances(
    _distances: &mut Vec<Vec<f64>>,
    _i: usize,
    _j: usize,
    _merged: &[usize],
    _points: &[Point],
    _options: &HierarchicalOptions,
) -> Result<()> {
    // Simplified: distances remain unchanged as we recalculate cluster distances on-the-fly
    // A full implementation would update the distance matrix here
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hierarchical_simple() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(0.1, 0.1),
            Point::new(5.0, 5.0),
        ];

        let options = HierarchicalOptions {
            num_clusters: 2,
            ..Default::default()
        };

        let result = hierarchical_cluster(&points, &options);
        assert!(result.is_ok());

        let clustering = result.expect("Clustering failed");
        assert_eq!(clustering.num_clusters, 2);
    }

    #[test]
    fn test_linkage_methods() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(10.0, 0.0),
        ];

        for linkage in [
            LinkageMethod::Single,
            LinkageMethod::Complete,
            LinkageMethod::Average,
            LinkageMethod::Ward,
        ] {
            let options = HierarchicalOptions {
                num_clusters: 2,
                linkage,
                ..Default::default()
            };

            let result = hierarchical_cluster(&points, &options);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_distance_threshold() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(0.5, 0.0),
            Point::new(10.0, 0.0),
        ];

        let options = HierarchicalOptions {
            num_clusters: 1,
            distance_threshold: Some(2.0),
            ..Default::default()
        };

        let result = hierarchical_cluster(&points, &options);
        assert!(result.is_ok());

        let clustering = result.expect("Clustering failed");
        assert!(clustering.num_clusters >= 2); // Should stop merging due to threshold
    }
}
