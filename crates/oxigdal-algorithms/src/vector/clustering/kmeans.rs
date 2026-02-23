//! K-means clustering algorithm
//!
//! Partition points into K clusters by minimizing within-cluster variance.

use crate::error::{AlgorithmError, Result};
use crate::vector::clustering::dbscan::{DistanceMetric, calculate_distance};
use oxigdal_core::vector::Point;
use std::collections::HashMap;

/// Options for K-means clustering
#[derive(Debug, Clone)]
pub struct KmeansOptions {
    /// Number of clusters
    pub k: usize,
    /// Maximum number of iterations
    pub max_iterations: usize,
    /// Convergence tolerance
    pub tolerance: f64,
    /// Distance metric
    pub metric: DistanceMetric,
    /// Initialization method
    pub init_method: InitMethod,
    /// Random seed for reproducibility
    pub seed: Option<u64>,
}

impl Default for KmeansOptions {
    fn default() -> Self {
        Self {
            k: 3,
            max_iterations: 100,
            tolerance: 1e-6,
            metric: DistanceMetric::Euclidean,
            init_method: InitMethod::KMeansPlusPlus,
            seed: None,
        }
    }
}

/// Initialization method for cluster centroids
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitMethod {
    /// Random selection
    Random,
    /// K-means++ (better initialization)
    KMeansPlusPlus,
    /// Uniform grid initialization
    Grid,
}

/// Result of K-means clustering
#[derive(Debug, Clone)]
pub struct KmeansResult {
    /// Cluster assignments for each point
    pub labels: Vec<usize>,
    /// Final centroid positions
    pub centroids: Vec<Point>,
    /// Sum of squared distances (inertia)
    pub inertia: f64,
    /// Number of iterations performed
    pub iterations: usize,
    /// Whether algorithm converged
    pub converged: bool,
    /// Cluster sizes
    pub cluster_sizes: HashMap<usize, usize>,
}

/// Perform K-means clustering
///
/// # Arguments
///
/// * `points` - Points to cluster
/// * `options` - K-means options
///
/// # Returns
///
/// Clustering result with labels and centroids
///
/// # Examples
///
/// ```
/// use oxigdal_algorithms::vector::clustering::{kmeans_cluster, KmeansOptions};
/// use oxigdal_algorithms::Point;
/// # use oxigdal_algorithms::error::Result;
///
/// # fn main() -> Result<()> {
/// let points = vec![
///     Point::new(0.0, 0.0),
///     Point::new(0.1, 0.1),
///     Point::new(5.0, 5.0),
///     Point::new(5.1, 5.1),
/// ];
///
/// let options = KmeansOptions {
///     k: 2,
///     max_iterations: 100,
///     ..Default::default()
/// };
///
/// let result = kmeans_cluster(&points, &options)?;
/// assert_eq!(result.centroids.len(), 2);
/// # Ok(())
/// # }
/// ```
pub fn kmeans_cluster(points: &[Point], options: &KmeansOptions) -> Result<KmeansResult> {
    if points.is_empty() {
        return Err(AlgorithmError::InvalidInput(
            "Cannot cluster empty point set".to_string(),
        ));
    }

    if options.k == 0 {
        return Err(AlgorithmError::InvalidInput(
            "Number of clusters must be positive".to_string(),
        ));
    }

    if options.k > points.len() {
        return Err(AlgorithmError::InvalidInput(format!(
            "Number of clusters ({}) exceeds number of points ({})",
            options.k,
            points.len()
        )));
    }

    // Initialize centroids
    let mut centroids = match options.init_method {
        InitMethod::KMeansPlusPlus => kmeans_plus_plus_init(points, options.k, options.metric)?,
        InitMethod::Random => random_init(points, options.k),
        InitMethod::Grid => grid_init(points, options.k),
    };

    let mut labels = vec![0; points.len()];
    let mut converged = false;
    let mut iteration = 0;

    for iter in 0..options.max_iterations {
        iteration = iter + 1;

        // Assignment step: assign each point to nearest centroid
        let mut changed = false;
        for (i, point) in points.iter().enumerate() {
            let nearest = find_nearest_centroid(point, &centroids, options.metric);
            if labels[i] != nearest {
                labels[i] = nearest;
                changed = true;
            }
        }

        if !changed {
            converged = true;
            break;
        }

        // Update step: recalculate centroids
        let old_centroids = centroids.clone();
        centroids = update_centroids(points, &labels, options.k);

        // Check convergence
        let max_movement = old_centroids
            .iter()
            .zip(&centroids)
            .map(|(old, new)| calculate_distance(old, new, options.metric))
            .fold(0.0, f64::max);

        if max_movement < options.tolerance {
            converged = true;
            break;
        }
    }

    // Calculate inertia (sum of squared distances)
    let mut inertia = 0.0;
    for (point, &label) in points.iter().zip(&labels) {
        let centroid = &centroids[label];
        let dist = calculate_distance(point, centroid, options.metric);
        inertia += dist * dist;
    }

    // Calculate cluster sizes
    let mut cluster_sizes: HashMap<usize, usize> = HashMap::new();
    for &label in &labels {
        *cluster_sizes.entry(label).or_insert(0) += 1;
    }

    Ok(KmeansResult {
        labels,
        centroids,
        inertia,
        iterations: iteration,
        converged,
        cluster_sizes,
    })
}

/// K-means++ initialization for better starting centroids
pub fn kmeans_plus_plus_init(
    points: &[Point],
    k: usize,
    metric: DistanceMetric,
) -> Result<Vec<Point>> {
    if k > points.len() {
        return Err(AlgorithmError::InvalidInput(
            "k exceeds number of points".to_string(),
        ));
    }

    let mut centroids = Vec::with_capacity(k);

    // Choose first centroid randomly
    // Using first point as deterministic "random" choice
    centroids.push(points[0].clone());

    // Choose remaining centroids
    for _ in 1..k {
        // Calculate D^2 for each point (squared distance to nearest centroid)
        let mut weights: Vec<f64> = points
            .iter()
            .map(|point| {
                let min_dist = centroids
                    .iter()
                    .map(|centroid| calculate_distance(point, centroid, metric))
                    .fold(f64::INFINITY, f64::min);
                min_dist * min_dist
            })
            .collect();

        // Normalize weights
        let total_weight: f64 = weights.iter().sum();
        if total_weight > 0.0 {
            for w in &mut weights {
                *w /= total_weight;
            }
        }

        // Choose next centroid with probability proportional to D^2
        // For deterministic behavior, choose the point with maximum weight
        let next_idx = weights
            .iter()
            .enumerate()
            .max_by(|(_, a): &(usize, &f64), (_, b): &(usize, &f64)| {
                a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(idx, _)| idx)
            .unwrap_or(centroids.len());

        centroids.push(points[next_idx].clone());
    }

    Ok(centroids)
}

/// Random initialization (first k points)
fn random_init(points: &[Point], k: usize) -> Vec<Point> {
    points.iter().take(k).cloned().collect()
}

/// Grid-based initialization
fn grid_init(points: &[Point], k: usize) -> Vec<Point> {
    if points.is_empty() {
        return Vec::new();
    }

    // Find bounding box
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for point in points {
        min_x = min_x.min(point.coord.x);
        max_x = max_x.max(point.coord.x);
        min_y = min_y.min(point.coord.y);
        max_y = max_y.max(point.coord.y);
    }

    // Create grid of k centroids
    let grid_size = (k as f64).sqrt().ceil() as usize;
    let mut centroids = Vec::new();

    for i in 0..grid_size {
        for j in 0..grid_size {
            if centroids.len() >= k {
                break;
            }

            let x = min_x + (max_x - min_x) * (i as f64 + 0.5) / grid_size as f64;
            let y = min_y + (max_y - min_y) * (j as f64 + 0.5) / grid_size as f64;

            centroids.push(Point::new(x, y));
        }

        if centroids.len() >= k {
            break;
        }
    }

    centroids
}

/// Find nearest centroid for a point
fn find_nearest_centroid(point: &Point, centroids: &[Point], metric: DistanceMetric) -> usize {
    centroids
        .iter()
        .enumerate()
        .map(|(idx, centroid)| (idx, calculate_distance(point, centroid, metric)))
        .min_by(|(_, d1): &(usize, f64), (_, d2): &(usize, f64)| {
            d1.partial_cmp(d2).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}

/// Update centroids based on current assignments
fn update_centroids(points: &[Point], labels: &[usize], k: usize) -> Vec<Point> {
    let mut sums_x = vec![0.0; k];
    let mut sums_y = vec![0.0; k];
    let mut counts = vec![0; k];

    for (point, &label) in points.iter().zip(labels) {
        sums_x[label] += point.coord.x;
        sums_y[label] += point.coord.y;
        counts[label] += 1;
    }

    (0..k)
        .map(|i| {
            if counts[i] > 0 {
                Point::new(sums_x[i] / counts[i] as f64, sums_y[i] / counts[i] as f64)
            } else {
                // Empty cluster, keep old centroid or use first point
                Point::new(
                    sums_x[0] / counts[0].max(1) as f64,
                    sums_y[0] / counts[0].max(1) as f64,
                )
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kmeans_simple() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(0.1, 0.1),
            Point::new(5.0, 5.0),
            Point::new(5.1, 5.1),
        ];

        let options = KmeansOptions {
            k: 2,
            max_iterations: 100,
            ..Default::default()
        };

        let result = kmeans_cluster(&points, &options);
        assert!(result.is_ok());

        let clustering = result.expect("Clustering failed");
        assert_eq!(clustering.centroids.len(), 2);
        assert_eq!(clustering.labels.len(), 4);
    }

    #[test]
    fn test_kmeans_plus_plus() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(0.1, 0.1),
            Point::new(5.0, 5.0),
            Point::new(5.1, 5.1),
        ];

        let centroids = kmeans_plus_plus_init(&points, 2, DistanceMetric::Euclidean);
        assert!(centroids.is_ok());

        let init = centroids.expect("Init failed");
        assert_eq!(init.len(), 2);
    }

    #[test]
    fn test_grid_init() {
        let points = vec![Point::new(0.0, 0.0), Point::new(10.0, 10.0)];

        let centroids = grid_init(&points, 4);
        assert_eq!(centroids.len(), 4);
    }

    #[test]
    fn test_kmeans_convergence() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(0.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(10.0, 10.0),
        ];

        let options = KmeansOptions {
            k: 2,
            tolerance: 0.01,
            ..Default::default()
        };

        let result = kmeans_cluster(&points, &options);
        assert!(result.is_ok());

        let clustering = result.expect("Clustering failed");
        assert!(clustering.converged);
    }
}
