//! DBSCAN (Density-Based Spatial Clustering of Applications with Noise)
//!
//! Finds clusters of arbitrary shape based on density connectivity.

use crate::error::{AlgorithmError, Result};
use oxigdal_core::vector::Point;
use std::collections::{HashMap, HashSet, VecDeque};

/// Options for DBSCAN clustering
#[derive(Debug, Clone)]
pub struct DbscanOptions {
    /// Maximum distance for neighborhood (epsilon)
    pub epsilon: f64,
    /// Minimum number of points to form a dense region
    pub min_points: usize,
    /// Distance metric
    pub metric: DistanceMetric,
}

impl Default for DbscanOptions {
    fn default() -> Self {
        Self {
            epsilon: 0.5,
            min_points: 5,
            metric: DistanceMetric::Euclidean,
        }
    }
}

/// Distance metric for clustering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistanceMetric {
    /// Euclidean distance
    Euclidean,
    /// Manhattan distance
    Manhattan,
    /// Haversine distance (for geographic coordinates)
    Haversine,
}

/// Result of DBSCAN clustering
#[derive(Debug, Clone)]
pub struct DbscanResult {
    /// Cluster assignments (-1 for noise points)
    pub labels: Vec<i32>,
    /// Number of clusters found
    pub num_clusters: usize,
    /// Indices of noise points
    pub noise_points: Vec<usize>,
    /// Cluster statistics
    pub cluster_sizes: HashMap<i32, usize>,
}

/// Perform DBSCAN clustering on points
///
/// # Arguments
///
/// * `points` - Points to cluster
/// * `options` - DBSCAN options
///
/// # Returns
///
/// Clustering result with labels for each point
///
/// # Examples
///
/// ```
/// use oxigdal_algorithms::vector::clustering::{dbscan_cluster, DbscanOptions};
/// use oxigdal_algorithms::Point;
/// # use oxigdal_algorithms::error::Result;
///
/// # fn main() -> Result<()> {
/// let points = vec![
///     Point::new(0.0, 0.0),
///     Point::new(0.1, 0.1),
///     Point::new(0.2, 0.1),
///     Point::new(5.0, 5.0),
///     Point::new(5.1, 5.0),
/// ];
///
/// let options = DbscanOptions {
///     epsilon: 0.5,
///     min_points: 2,
///     ..Default::default()
/// };
///
/// let result = dbscan_cluster(&points, &options)?;
/// assert!(result.num_clusters >= 1);
/// # Ok(())
/// # }
/// ```
pub fn dbscan_cluster(points: &[Point], options: &DbscanOptions) -> Result<DbscanResult> {
    if points.is_empty() {
        return Err(AlgorithmError::InvalidInput(
            "Cannot cluster empty point set".to_string(),
        ));
    }

    let n = points.len();
    let mut labels = vec![-1; n]; // -1 = unvisited, 0 = noise, >0 = cluster ID
    let mut cluster_id = 0;

    for i in 0..n {
        if labels[i] != -1 {
            continue; // Already processed
        }

        let neighbors = range_query(points, i, options.epsilon, options.metric);

        if neighbors.len() < options.min_points {
            labels[i] = 0; // Mark as noise
            continue;
        }

        // Start new cluster
        cluster_id += 1;
        expand_cluster(points, i, &neighbors, cluster_id, &mut labels, options)?;
    }

    // Build result
    let mut noise_points = Vec::new();
    let mut cluster_sizes: HashMap<i32, usize> = HashMap::new();

    for (idx, &label) in labels.iter().enumerate() {
        if label == 0 {
            noise_points.push(idx);
        } else if label > 0 {
            *cluster_sizes.entry(label).or_insert(0) += 1;
        }
    }

    Ok(DbscanResult {
        labels,
        num_clusters: cluster_id as usize,
        noise_points,
        cluster_sizes,
    })
}

/// Expand a cluster from a core point
fn expand_cluster(
    points: &[Point],
    point_idx: usize,
    neighbors: &[usize],
    cluster_id: i32,
    labels: &mut [i32],
    options: &DbscanOptions,
) -> Result<()> {
    labels[point_idx] = cluster_id;

    let mut queue = VecDeque::from(neighbors.to_vec());
    let mut processed = HashSet::new();
    processed.insert(point_idx);

    while let Some(neighbor_idx) = queue.pop_front() {
        if processed.contains(&neighbor_idx) {
            continue;
        }
        processed.insert(neighbor_idx);

        if labels[neighbor_idx] == 0 {
            // Change noise to border point
            labels[neighbor_idx] = cluster_id;
        }

        if labels[neighbor_idx] != -1 {
            continue; // Already in a cluster
        }

        labels[neighbor_idx] = cluster_id;

        let neighbor_neighbors = range_query(points, neighbor_idx, options.epsilon, options.metric);

        if neighbor_neighbors.len() >= options.min_points {
            // This is also a core point
            for &nn in &neighbor_neighbors {
                if !processed.contains(&nn) {
                    queue.push_back(nn);
                }
            }
        }
    }

    Ok(())
}

/// Find all points within epsilon distance of a given point
fn range_query(
    points: &[Point],
    point_idx: usize,
    epsilon: f64,
    metric: DistanceMetric,
) -> Vec<usize> {
    let query_point = &points[point_idx];
    let mut neighbors = Vec::new();

    for (i, point) in points.iter().enumerate() {
        let dist = calculate_distance(query_point, point, metric);
        if dist <= epsilon {
            neighbors.push(i);
        }
    }

    neighbors
}

/// Calculate distance between two points
pub fn calculate_distance(p1: &Point, p2: &Point, metric: DistanceMetric) -> f64 {
    match metric {
        DistanceMetric::Euclidean => {
            let dx = p1.coord.x - p2.coord.x;
            let dy = p1.coord.y - p2.coord.y;
            (dx * dx + dy * dy).sqrt()
        }
        DistanceMetric::Manhattan => {
            let dx = (p1.coord.x - p2.coord.x).abs();
            let dy = (p1.coord.y - p2.coord.y).abs();
            dx + dy
        }
        DistanceMetric::Haversine => haversine_distance(p1, p2),
    }
}

/// Calculate Haversine distance between two geographic points (in meters)
fn haversine_distance(p1: &Point, p2: &Point) -> f64 {
    const EARTH_RADIUS: f64 = 6371000.0; // meters

    let lat1 = p1.coord.y.to_radians();
    let lat2 = p2.coord.y.to_radians();
    let delta_lat = (p2.coord.y - p1.coord.y).to_radians();
    let delta_lon = (p2.coord.x - p1.coord.x).to_radians();

    let a =
        (delta_lat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (delta_lon / 2.0).sin().powi(2);

    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

    EARTH_RADIUS * c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dbscan_simple() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(0.1, 0.1),
            Point::new(0.2, 0.1),
            Point::new(5.0, 5.0),
            Point::new(5.1, 5.0),
        ];

        let options = DbscanOptions {
            epsilon: 0.5,
            min_points: 2,
            ..Default::default()
        };

        let result = dbscan_cluster(&points, &options);
        assert!(result.is_ok());

        let clustering = result.expect("Clustering failed");
        assert!(clustering.num_clusters >= 1);
    }

    #[test]
    fn test_dbscan_all_noise() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(20.0, 20.0),
        ];

        let options = DbscanOptions {
            epsilon: 0.5,
            min_points: 2,
            ..Default::default()
        };

        let result = dbscan_cluster(&points, &options);
        assert!(result.is_ok());

        let clustering = result.expect("Clustering failed");
        assert_eq!(clustering.num_clusters, 0);
        assert_eq!(clustering.noise_points.len(), 3);
    }

    #[test]
    fn test_haversine_distance() {
        let p1 = Point::new(0.0, 0.0);
        let p2 = Point::new(0.0, 0.01); // ~1.11 km

        let dist = haversine_distance(&p1, &p2);
        assert!(dist > 1000.0 && dist < 1200.0); // Approximately 1.11 km
    }

    #[test]
    fn test_euclidean_distance() {
        let p1 = Point::new(0.0, 0.0);
        let p2 = Point::new(3.0, 4.0);

        let dist = calculate_distance(&p1, &p2, DistanceMetric::Euclidean);
        assert!((dist - 5.0).abs() < 1e-6);
    }

    #[test]
    fn test_manhattan_distance() {
        let p1 = Point::new(0.0, 0.0);
        let p2 = Point::new(3.0, 4.0);

        let dist = calculate_distance(&p1, &p2, DistanceMetric::Manhattan);
        assert!((dist - 7.0).abs() < 1e-6);
    }
}
