//! R-tree-accelerated DBSCAN spatial clustering.
//!
//! DBSCAN (Density-Based Spatial Clustering of Applications with Noise) groups
//! points into clusters based on density reachability.  This implementation
//! replaces the naïve O(N²) range query with an R-tree window prefilter +
//! exact Euclidean distance check, yielding O(N log N) average-case behaviour
//! on well-indexed point sets.
//!
//! # Algorithm outline
//!
//! 1. Build (or accept) an R-tree of all input points, keyed by point index.
//! 2. For each unvisited point `p`:
//!    a. Issue a window query for the bbox `[cx-ε, cy-ε, cx+ε, cy+ε]`.
//!    b. Filter candidates to those within Euclidean distance ε (exact check).
//!    c. If `|neighbors| < min_points`, mark `p` as noise and continue.
//!    d. Otherwise start a new cluster and BFS-expand over reachable core pts.
//! 3. Return per-point cluster labels, total cluster count, and noise count.
//!
//! # Example
//!
//! ```rust
//! use oxigeo_index::clustering::dbscan::{dbscan_rtree, DbscanOptions};
//!
//! let points = vec![
//!     (0.0_f64, 0.0_f64), (0.1, 0.0), (0.0, 0.1),
//!     (10.0, 10.0), (10.1, 10.0), (10.0, 10.1),
//!     (99.0, 99.0), // noise
//! ];
//! let opts = DbscanOptions { eps: 1.0, min_points: 3 };
//! let result = dbscan_rtree(&points, &opts);
//! assert_eq!(result.num_clusters, 2);
//! assert_eq!(result.noise_count, 1);
//! ```

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use crate::bbox::Bbox2D;
use crate::rtree::RTree;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Signed integer label for a clustered point.
///
/// * `>= 0` — cluster identifier (0-based, assigned in BFS expansion order)
/// * [`NOISE`] (`-1`) — point is not part of any cluster
/// * [`UNVISITED`] (`-2`) — internal sentinel, never appears in output
pub type ClusterLabel = i32;

/// Sentinel: point is noise (no cluster).
pub const NOISE: ClusterLabel = -1;

/// Internal sentinel: point has not yet been processed.
pub const UNVISITED: ClusterLabel = -2;

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/// Configuration for R-tree-accelerated DBSCAN.
#[derive(Debug, Clone)]
pub struct DbscanOptions {
    /// Neighbourhood radius ε.  Two points are considered neighbours when
    /// their Euclidean distance is ≤ `eps`.
    pub eps: f64,

    /// Minimum number of points (including the point itself) required for a
    /// point to be considered a *core point*.  If a point's ε-neighbourhood
    /// contains fewer than `min_points` points it is labelled noise unless it
    /// is reachable from another core point.
    pub min_points: usize,
}

impl Default for DbscanOptions {
    fn default() -> Self {
        Self {
            eps: 1.0,
            min_points: 4,
        }
    }
}

// ---------------------------------------------------------------------------
// Result
// ---------------------------------------------------------------------------

/// Output of an R-tree-accelerated DBSCAN run.
#[derive(Debug, Clone)]
pub struct DbscanResult {
    /// Per-point cluster label.  `labels[i]` corresponds to `points[i]`.
    ///
    /// * `>= 0` — cluster id (0-based)
    /// * `-1`   — noise point
    pub labels: Vec<ClusterLabel>,

    /// Total number of distinct clusters discovered (≥ 0).
    pub num_clusters: usize,

    /// Number of points labelled as noise.
    pub noise_count: usize,
}

// ---------------------------------------------------------------------------
// Core algorithms
// ---------------------------------------------------------------------------

/// R-tree-accelerated DBSCAN that builds its own internal R-tree from
/// `points`.
///
/// Each entry in the R-tree maps a degenerate (point) bounding box to the
/// corresponding index into `points`.  The index is then used during BFS
/// expansion to recover coordinates for the exact distance check.
///
/// # Parameters
///
/// * `points`  – slice of `(x, y)` coordinates.
/// * `options` – DBSCAN hyperparameters (ε and min_points).
///
/// # Returns
///
/// A [`DbscanResult`] whose `labels` vector has the same length as `points`.
/// Returns an empty result for an empty input rather than panicking.
pub fn dbscan_rtree(points: &[(f64, f64)], options: &DbscanOptions) -> DbscanResult {
    if points.is_empty() {
        return DbscanResult {
            labels: Vec::new(),
            num_clusters: 0,
            noise_count: 0,
        };
    }

    // Build the R-tree once, storing point indices as values.
    let mut rtree: RTree<usize> = RTree::new();
    for (i, &(x, y)) in points.iter().enumerate() {
        let bbox = Bbox2D::point(x, y);
        rtree.insert(bbox, i);
    }

    dbscan_with_rtree(&rtree, points, options)
}

/// R-tree-accelerated DBSCAN using a *caller-supplied* R-tree.
///
/// This variant is useful when the caller has already built (or can reuse) an
/// R-tree for other purposes, avoiding a redundant build step.
///
/// **Contract:** every value stored in `rtree` must be a valid index into
/// `points`.  Violating this contract is safe (no undefined behaviour) but
/// will produce incorrect clustering results.
///
/// # Parameters
///
/// * `rtree`   – pre-built R-tree mapping point bboxes to `points` indices.
/// * `points`  – slice of `(x, y)` coordinates matching the R-tree entries.
/// * `options` – DBSCAN hyperparameters.
pub fn dbscan_with_rtree(
    rtree: &RTree<usize>,
    points: &[(f64, f64)],
    options: &DbscanOptions,
) -> DbscanResult {
    let n = points.len();
    let mut labels: Vec<ClusterLabel> = vec![UNVISITED; n];
    let mut cluster_id: i32 = -1;

    for i in 0..n {
        if labels[i] != UNVISITED {
            continue;
        }

        let (cx, cy) = points[i];
        let neighbors = range_query_eps(rtree, points, cx, cy, options.eps);

        if neighbors.len() < options.min_points {
            labels[i] = NOISE;
            continue;
        }

        // Open a new cluster.
        cluster_id += 1;
        labels[i] = cluster_id;

        // BFS expansion: seed with all neighbors except point i itself
        // (i is already labelled; it will be skipped below).
        let mut queue: Vec<usize> = neighbors.into_iter().filter(|&j| j != i).collect();
        let mut qi: usize = 0;

        while qi < queue.len() {
            let j = queue[qi];
            qi += 1;

            // Absorb noise border points into the current cluster.
            if labels[j] == NOISE {
                labels[j] = cluster_id;
            }

            // Skip if already committed to any cluster (including this one).
            if labels[j] != UNVISITED {
                continue;
            }

            labels[j] = cluster_id;

            // If j is itself a core point, enqueue its unvisited / noise
            // neighbors so they can be absorbed or further expanded.
            let (jx, jy) = points[j];
            let j_neighbors = range_query_eps(rtree, points, jx, jy, options.eps);
            if j_neighbors.len() >= options.min_points {
                for k in j_neighbors {
                    if labels[k] == UNVISITED || labels[k] == NOISE {
                        queue.push(k);
                    }
                }
            }
        }
    }

    let noise_count = labels.iter().filter(|&&l| l == NOISE).count();
    let num_clusters = if cluster_id < 0 {
        0
    } else {
        (cluster_id + 1) as usize
    };

    DbscanResult {
        labels,
        num_clusters,
        noise_count,
    }
}

// ---------------------------------------------------------------------------
// Range query
// ---------------------------------------------------------------------------

/// Return the indices of all points in `points` that lie within Euclidean
/// distance `eps` from the centre `(cx, cy)`.
///
/// Uses `rtree` as a spatial prefilter: a square window of half-side `eps` is
/// queried first, then each candidate is verified with an exact distance
/// check.  Points at distance exactly `eps` are included (closed ball).
///
/// # Parameters
///
/// * `rtree`  – R-tree whose values are indices into `points`.
/// * `points` – slice of `(x, y)` coordinates.
/// * `cx`, `cy` – centre of the query ball.
/// * `eps`    – radius of the query ball.
///
/// # Returns
///
/// A `Vec<usize>` of matching point indices (order unspecified).
pub fn range_query_eps(
    rtree: &RTree<usize>,
    points: &[(f64, f64)],
    cx: f64,
    cy: f64,
    eps: f64,
) -> Vec<usize> {
    // Conservative bounding square: any point within Euclidean distance eps
    // must lie within this axis-aligned square.
    let query_bbox = Bbox2D {
        min_x: cx - eps,
        min_y: cy - eps,
        max_x: cx + eps,
        max_y: cy + eps,
    };

    let eps_sq = eps * eps;

    // `rtree.search` returns `Vec<&usize>`.  We dereference each ref to get
    // the stored index, then apply the exact distance predicate.
    rtree
        .search(&query_bbox)
        .into_iter()
        .filter_map(|idx_ref| {
            let idx = *idx_ref;
            let (px, py) = points[idx];
            let dx = px - cx;
            let dy = py - cy;
            if dx * dx + dy * dy <= eps_sq {
                Some(idx)
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- helpers -----------------------------------------------------------

    fn make_rtree(points: &[(f64, f64)]) -> RTree<usize> {
        let mut rtree: RTree<usize> = RTree::new();
        for (i, &(x, y)) in points.iter().enumerate() {
            rtree.insert(Bbox2D::point(x, y), i);
        }
        rtree
    }

    // ---- range_query_eps ---------------------------------------------------

    #[test]
    fn range_query_includes_border_point() {
        // Point exactly at distance eps must be included (closed ball).
        let points = vec![(0.0, 0.0), (1.0, 0.0)];
        let rtree = make_rtree(&points);
        let result = range_query_eps(&rtree, &points, 0.0, 0.0, 1.0);
        assert!(result.contains(&0));
        assert!(result.contains(&1));
    }

    #[test]
    fn range_query_excludes_outside_radius() {
        let points = vec![(0.0, 0.0), (0.5, 0.0), (2.0, 0.0)];
        let rtree = make_rtree(&points);
        let result = range_query_eps(&rtree, &points, 0.0, 0.0, 1.0);
        assert!(result.contains(&0));
        assert!(result.contains(&1));
        assert!(!result.contains(&2), "distance-2 point must be excluded");
    }

    #[test]
    fn range_query_diagonal_corner_excluded() {
        // A point at (ε/√2, ε/√2) is exactly on the circle — should be in.
        // A point at (ε, ε) lies outside the circle (dist = ε√2) but inside
        // the bounding square, so the prefilter matches it but the exact check
        // rejects it.
        let eps: f64 = 1.0;
        let diag = eps / 2.0_f64.sqrt();
        let outside = eps + 0.001; // just beyond the circle at 45°
        let points = vec![(0.0, 0.0), (diag, diag), (eps, eps)];
        let rtree = make_rtree(&points);
        let result = range_query_eps(&rtree, &points, 0.0, 0.0, eps);
        assert!(result.contains(&0));
        assert!(
            result.contains(&1),
            "point on circle boundary must be included"
        );
        assert!(
            !result.contains(&2),
            "corner outside circle must be excluded"
        );
        // suppress unused warning
        let _ = outside;
    }

    // ---- dbscan_rtree -------------------------------------------------------

    #[test]
    fn empty_input_returns_empty_result() {
        let result = dbscan_rtree(&[], &DbscanOptions::default());
        assert!(result.labels.is_empty());
        assert_eq!(result.num_clusters, 0);
        assert_eq!(result.noise_count, 0);
    }

    #[test]
    fn single_dense_cluster_all_labelled_zero() {
        let points: Vec<(f64, f64)> =
            vec![(0.0, 0.0), (0.1, 0.0), (0.0, 0.1), (0.1, 0.1), (0.05, 0.05)];
        let result = dbscan_rtree(
            &points,
            &DbscanOptions {
                eps: 1.0,
                min_points: 3,
            },
        );
        assert_eq!(result.num_clusters, 1);
        assert_eq!(result.noise_count, 0);
        assert!(result.labels.iter().all(|&l| l == 0));
    }

    #[test]
    fn two_separated_clusters_distinct_labels() {
        let points: Vec<(f64, f64)> = vec![
            (0.0, 0.0),
            (0.1, 0.0),
            (0.0, 0.1), // cluster A
            (10.0, 10.0),
            (10.1, 10.0),
            (10.0, 10.1), // cluster B
        ];
        let result = dbscan_rtree(
            &points,
            &DbscanOptions {
                eps: 1.0,
                min_points: 3,
            },
        );
        assert_eq!(result.num_clusters, 2);
        assert_eq!(result.noise_count, 0);
        let label_a = result.labels[0];
        let label_b = result.labels[3];
        assert_ne!(label_a, label_b, "clusters A and B must have different ids");
        assert!(label_a >= 0 && label_b >= 0);
    }

    #[test]
    fn isolated_noise_point_labelled_minus_one() {
        let points: Vec<(f64, f64)> = vec![
            (0.0, 0.0),
            (0.1, 0.0),
            (0.0, 0.1),     // dense group
            (100.0, 100.0), // isolated noise
        ];
        let result = dbscan_rtree(
            &points,
            &DbscanOptions {
                eps: 0.5,
                min_points: 3,
            },
        );
        assert_eq!(
            result.labels[3], NOISE,
            "isolated point must be labelled NOISE"
        );
        assert_eq!(result.noise_count, 1);
    }

    #[test]
    fn below_min_points_threshold_all_noise() {
        let points: Vec<(f64, f64)> = vec![(0.0, 0.0), (0.1, 0.0)];
        let result = dbscan_rtree(
            &points,
            &DbscanOptions {
                eps: 1.0,
                min_points: 5,
            },
        );
        assert_eq!(result.num_clusters, 0);
        assert_eq!(result.noise_count, 2);
        assert!(result.labels.iter().all(|&l| l == NOISE));
    }

    #[test]
    fn eps_too_small_all_noise() {
        let points: Vec<(f64, f64)> = vec![(0.0, 0.0), (5.0, 0.0), (10.0, 0.0)];
        let result = dbscan_rtree(
            &points,
            &DbscanOptions {
                eps: 0.001,
                min_points: 2,
            },
        );
        assert_eq!(result.num_clusters, 0);
        assert_eq!(result.noise_count, 3);
    }

    #[test]
    fn single_point_cluster_when_min_points_is_one() {
        // With min_points = 1 every point is its own core point.
        let points: Vec<(f64, f64)> = vec![(0.0, 0.0), (100.0, 100.0), (200.0, 200.0)];
        let result = dbscan_rtree(
            &points,
            &DbscanOptions {
                eps: 1.0,
                min_points: 1,
            },
        );
        assert_eq!(result.num_clusters, 3);
        assert_eq!(result.noise_count, 0);
    }

    #[test]
    fn border_point_absorbed_into_cluster() {
        // core1: (0,0),(0.1,0),(0,0.1)  — min_points=3, all within eps=0.5
        // border: (0.4,0) — has only 2 eps-neighbors (itself + core point),
        //         but is reachable from the core cluster, so gets its label.
        let points: Vec<(f64, f64)> = vec![
            (0.0, 0.0),
            (0.1, 0.0),
            (0.0, 0.1), // core group
            (0.4, 0.0), // border point reachable from core
        ];
        let result = dbscan_rtree(
            &points,
            &DbscanOptions {
                eps: 0.5,
                min_points: 3,
            },
        );
        assert_eq!(result.num_clusters, 1);
        // border point must be in the cluster, not noise
        assert_ne!(result.labels[3], NOISE, "border point must be absorbed");
        assert_eq!(result.noise_count, 0);
    }
}
