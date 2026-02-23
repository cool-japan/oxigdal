//! Spatial join operations with spatial indexing
//!
//! Efficient spatial joins using R-tree and other spatial indices.

use crate::error::Result;
use oxigdal_core::vector::Point;
use rstar::{AABB, PointDistance, RTree, RTreeObject};

/// Spatial join predicate
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpatialJoinPredicate {
    /// Features intersect
    Intersects,
    /// First feature contains second
    Contains,
    /// First feature is within second
    Within,
    /// Features touch (share boundary)
    Touches,
    /// Features are within distance
    WithinDistance,
}

/// Options for spatial join
#[derive(Debug, Clone)]
pub struct SpatialJoinOptions {
    /// Spatial predicate
    pub predicate: SpatialJoinPredicate,
    /// Distance threshold (for WithinDistance predicate)
    pub distance: f64,
    /// Whether to build spatial index
    pub use_index: bool,
}

impl Default for SpatialJoinOptions {
    fn default() -> Self {
        Self {
            predicate: SpatialJoinPredicate::Intersects,
            distance: 0.0,
            use_index: true,
        }
    }
}

/// Result of spatial join
#[derive(Debug, Clone)]
pub struct SpatialJoinResult {
    /// Pairs of matching indices (left_idx, right_idx)
    pub matches: Vec<(usize, usize)>,
    /// Number of matches
    pub num_matches: usize,
}

/// Indexed point for R-tree
#[derive(Debug, Clone)]
struct IndexedPoint {
    point: Point,
    index: usize,
}

impl RTreeObject for IndexedPoint {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        AABB::from_point([self.point.coord.x, self.point.coord.y])
    }
}

impl PointDistance for IndexedPoint {
    fn distance_2(&self, point: &[f64; 2]) -> f64 {
        let dx = self.point.coord.x - point[0];
        let dy = self.point.coord.y - point[1];
        dx * dx + dy * dy
    }
}

/// Perform spatial join between two point sets
///
/// # Arguments
///
/// * `left_points` - First set of points
/// * `right_points` - Second set of points
/// * `options` - Spatial join options
///
/// # Returns
///
/// Join result with matching pairs
///
/// # Examples
///
/// ```
/// use oxigdal_algorithms::vector::spatial_join::{spatial_join_points, SpatialJoinOptions, SpatialJoinPredicate};
/// use oxigdal_algorithms::Point;
/// # use oxigdal_algorithms::error::Result;
///
/// # fn main() -> Result<()> {
/// let left = vec![
///     Point::new(0.0, 0.0),
///     Point::new(1.0, 1.0),
/// ];
///
/// let right = vec![
///     Point::new(0.1, 0.1),
///     Point::new(10.0, 10.0),
/// ];
///
/// let options = SpatialJoinOptions {
///     predicate: SpatialJoinPredicate::WithinDistance,
///     distance: 0.5,
///     use_index: true,
/// };
///
/// let result = spatial_join_points(&left, &right, &options)?;
/// assert!(result.num_matches >= 1);
/// # Ok(())
/// # }
/// ```
pub fn spatial_join_points(
    left_points: &[Point],
    right_points: &[Point],
    options: &SpatialJoinOptions,
) -> Result<SpatialJoinResult> {
    if left_points.is_empty() || right_points.is_empty() {
        return Ok(SpatialJoinResult {
            matches: Vec::new(),
            num_matches: 0,
        });
    }

    let matches = if options.use_index {
        // Build R-tree for right points
        let indexed_points: Vec<IndexedPoint> = right_points
            .iter()
            .enumerate()
            .map(|(idx, point)| IndexedPoint {
                point: point.clone(),
                index: idx,
            })
            .collect();

        let rtree = RTree::bulk_load(indexed_points);

        // Query R-tree for each left point
        let mut all_matches = Vec::new();

        for (left_idx, left_point) in left_points.iter().enumerate() {
            let nearby = match options.predicate {
                SpatialJoinPredicate::WithinDistance => {
                    // Query points within distance
                    let envelope = AABB::from_corners(
                        [
                            left_point.coord.x - options.distance,
                            left_point.coord.y - options.distance,
                        ],
                        [
                            left_point.coord.x + options.distance,
                            left_point.coord.y + options.distance,
                        ],
                    );

                    rtree
                        .locate_in_envelope(&envelope)
                        .filter(|indexed| {
                            point_distance(left_point, &indexed.point) <= options.distance
                        })
                        .map(|indexed| indexed.index)
                        .collect::<Vec<_>>()
                }
                SpatialJoinPredicate::Intersects => {
                    // For points, intersects means exactly coincident
                    let mut matches = Vec::new();
                    for indexed in rtree.locate_at_point(&[left_point.coord.x, left_point.coord.y])
                    {
                        matches.push(indexed.index);
                    }
                    matches
                }
                _ => {
                    // Other predicates not applicable to points
                    Vec::new()
                }
            };

            for right_idx in nearby {
                all_matches.push((left_idx, right_idx));
            }
        }

        all_matches
    } else {
        // Brute force comparison
        let mut all_matches = Vec::new();

        for (left_idx, left_point) in left_points.iter().enumerate() {
            for (right_idx, right_point) in right_points.iter().enumerate() {
                if matches_predicate(left_point, right_point, options) {
                    all_matches.push((left_idx, right_idx));
                }
            }
        }

        all_matches
    };

    Ok(SpatialJoinResult {
        num_matches: matches.len(),
        matches,
    })
}

/// Check if two points match the join predicate
fn matches_predicate(left: &Point, right: &Point, options: &SpatialJoinOptions) -> bool {
    match options.predicate {
        SpatialJoinPredicate::Intersects => {
            (left.coord.x - right.coord.x).abs() < 1e-10
                && (left.coord.y - right.coord.y).abs() < 1e-10
        }
        SpatialJoinPredicate::WithinDistance => point_distance(left, right) <= options.distance,
        _ => false,
    }
}

/// Calculate Euclidean distance between points
fn point_distance(p1: &Point, p2: &Point) -> f64 {
    let dx = p1.coord.x - p2.coord.x;
    let dy = p1.coord.y - p2.coord.y;
    (dx * dx + dy * dy).sqrt()
}

/// Nearest neighbor search
pub fn nearest_neighbor(query: &Point, points: &[Point]) -> Option<(usize, f64)> {
    let indexed_points: Vec<IndexedPoint> = points
        .iter()
        .enumerate()
        .map(|(idx, point)| IndexedPoint {
            point: point.clone(),
            index: idx,
        })
        .collect();

    if indexed_points.is_empty() {
        return None;
    }

    let rtree = RTree::bulk_load(indexed_points);
    let nearest = rtree.nearest_neighbor(&[query.coord.x, query.coord.y])?;

    let distance = point_distance(query, &nearest.point);

    Some((nearest.index, distance))
}

/// K-nearest neighbors search
pub fn k_nearest_neighbors(query: &Point, points: &[Point], k: usize) -> Vec<(usize, f64)> {
    let indexed_points: Vec<IndexedPoint> = points
        .iter()
        .enumerate()
        .map(|(idx, point)| IndexedPoint {
            point: point.clone(),
            index: idx,
        })
        .collect();

    if indexed_points.is_empty() {
        return Vec::new();
    }

    let rtree = RTree::bulk_load(indexed_points);

    rtree
        .nearest_neighbor_iter(&[query.coord.x, query.coord.y])
        .take(k)
        .map(|indexed| {
            let dist = point_distance(query, &indexed.point);
            (indexed.index, dist)
        })
        .collect()
}

/// Range query (all points within distance)
pub fn range_query(query: &Point, points: &[Point], distance: f64) -> Vec<usize> {
    let options = SpatialJoinOptions {
        predicate: SpatialJoinPredicate::WithinDistance,
        distance,
        use_index: true,
    };

    let result = spatial_join_points(std::slice::from_ref(query), points, &options);

    result
        .map(|r| {
            r.matches
                .into_iter()
                .map(|(_, right_idx)| right_idx)
                .collect()
        })
        .unwrap_or_else(|_| Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spatial_join_within_distance() {
        let left = vec![Point::new(0.0, 0.0), Point::new(10.0, 10.0)];

        let right = vec![Point::new(0.1, 0.1), Point::new(5.0, 5.0)];

        let options = SpatialJoinOptions {
            predicate: SpatialJoinPredicate::WithinDistance,
            distance: 0.5,
            use_index: true,
        };

        let result = spatial_join_points(&left, &right, &options);
        assert!(result.is_ok());

        let join_result = result.expect("Join failed");
        assert!(join_result.num_matches >= 1);
    }

    #[test]
    fn test_nearest_neighbor() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(5.0, 5.0),
            Point::new(10.0, 10.0),
        ];

        let query = Point::new(0.1, 0.1);
        let result = nearest_neighbor(&query, &points);

        assert!(result.is_some());

        let (idx, dist) = result.expect("Nearest neighbor failed");
        assert_eq!(idx, 0);
        assert!(dist < 0.2);
    }

    #[test]
    fn test_k_nearest_neighbors() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(2.0, 2.0),
            Point::new(10.0, 10.0),
        ];

        let query = Point::new(0.0, 0.0);
        let result = k_nearest_neighbors(&query, &points, 2);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, 0); // First is the query point itself
    }

    #[test]
    fn test_range_query() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(0.5, 0.5),
            Point::new(10.0, 10.0),
        ];

        let query = Point::new(0.0, 0.0);
        let result = range_query(&query, &points, 1.0);

        assert!(result.len() >= 2); // Should find points at 0.0 and 0.5
    }

    #[test]
    fn test_point_distance() {
        let p1 = Point::new(0.0, 0.0);
        let p2 = Point::new(3.0, 4.0);

        let dist = point_distance(&p1, &p2);
        assert!((dist - 5.0).abs() < 1e-6);
    }
}
