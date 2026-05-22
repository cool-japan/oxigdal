//! OPTICS Clustering Algorithm
//!
//! Ordering Points To Identify the Clustering Structure (OPTICS).
//! A density-based clustering algorithm that addresses one of DBSCAN's
//! major weaknesses: the inability to detect meaningful clusters when
//! the data set contains regions of widely varying density.
//!
//! Reference: Ankerst, M., Breunig, M. M., Kriegel, H.-P., & Sander, J.
//! (1999). "OPTICS: Ordering Points to Identify the Clustering Structure."
//! Proceedings of the 1999 ACM SIGMOD International Conference on
//! Management of Data, pp. 49–60.
//!
//! Unlike DBSCAN, OPTICS does not produce a clustering of the data set
//! explicitly. Instead, it creates an augmented ordering of the database
//! representing its density-based clustering structure — the
//! "reachability plot". Clusters can be extracted from this plot using:
//!   - The ξ (xi) steep-area method (variable density, default).
//!   - A DBSCAN-equivalent extraction with a chosen radius ε.

use crate::error::{AnalyticsError, Result};
use rstar::{AABB, PointDistance, RTree, RTreeObject};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

// ── Public Types ────────────────────────────────────────────────────────────

/// A 2-dimensional point used for OPTICS input.
///
/// This mirrors `oxigdal_noalloc::Point2D` but is intentionally local so
/// that the analytics crate avoids pulling extra workspace dependencies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point2D {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
}

impl Point2D {
    /// Creates a new [`Point2D`].
    #[must_use]
    #[inline]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// Options controlling the OPTICS algorithm.
#[derive(Debug, Clone, Copy)]
pub struct OpticsOptions {
    /// Minimum number of neighbours required for a point to be a "core"
    /// point. Smaller values produce noisier clusterings.
    pub min_samples: usize,
    /// Maximum neighbourhood radius. Setting `f64::INFINITY` (the default)
    /// means no upper bound is enforced.
    pub max_eps: f64,
    /// Steepness threshold used for ξ-extraction (Ankerst 1999 §4).
    /// Typical values are in `[0.01, 0.10]`.
    pub xi: f64,
}

impl Default for OpticsOptions {
    fn default() -> Self {
        Self {
            min_samples: 5,
            max_eps: f64::INFINITY,
            xi: 0.05,
        }
    }
}

/// A single cluster discovered in the reachability ordering.
///
/// The cluster spans positions `[start, end]` (inclusive) of the
/// `ordering` vector in [`OpticsResult`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpticsCluster {
    /// First position in the reachability ordering.
    pub start: usize,
    /// Last position in the reachability ordering (inclusive).
    pub end: usize,
    /// Steepness used during ξ-extraction (0.0 for DBSCAN-compat
    /// extraction).
    pub xi_steepness: f64,
}

/// Full output of an OPTICS run.
#[derive(Debug, Clone)]
pub struct OpticsResult {
    /// Visit order — `ordering[i]` is the original index of the point at
    /// position `i` in the reachability plot.
    pub ordering: Vec<usize>,
    /// Per-position reachability distance (`f64::INFINITY` for points
    /// whose reachability is undefined, typically the first member of a
    /// connected component).
    pub reachability: Vec<f64>,
    /// Per-position core distance (`f64::INFINITY` if the point at this
    /// position is not a core point).
    pub core_distances: Vec<f64>,
    /// Clusters extracted with the default ξ value supplied in
    /// [`OpticsOptions`]. Use [`extract_xi_clusters`] or
    /// [`extract_dbscan_clusters`] for alternative extractions over the
    /// same reachability plot.
    pub clusters: Vec<OpticsCluster>,
}

/// OPTICS clusterer object that pairs options with a fit method.
#[derive(Debug, Clone, Copy)]
pub struct OpticsClusterer {
    options: OpticsOptions,
}

impl OpticsClusterer {
    /// Construct a new clusterer with the given options.
    #[must_use]
    pub fn new(options: OpticsOptions) -> Self {
        Self { options }
    }

    /// Run the OPTICS algorithm on the supplied points.
    ///
    /// # Errors
    ///
    /// Returns an error when `min_samples == 0` or `max_eps <= 0`.
    pub fn fit(&self, points: &[Point2D]) -> Result<OpticsResult> {
        optics(points, &self.options)
    }

    /// Access the options used by this clusterer.
    #[must_use]
    pub fn options(&self) -> &OpticsOptions {
        &self.options
    }
}

// ── R-tree adapter ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
struct IndexedPoint {
    index: usize,
    x: f64,
    y: f64,
}

impl RTreeObject for IndexedPoint {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        AABB::from_point([self.x, self.y])
    }
}

impl PointDistance for IndexedPoint {
    fn distance_2(&self, p: &[f64; 2]) -> f64 {
        let dx = self.x - p[0];
        let dy = self.y - p[1];
        dx * dx + dy * dy
    }
}

// ── Priority queue entry ───────────────────────────────────────────────────

/// Entry of the OPTICS priority queue. The natural ordering is the
/// reverse of the reachability so that [`BinaryHeap`] (a max-heap)
/// behaves like a min-heap on reachability distance. `total_cmp` is
/// used so that NaN never sneaks into the comparison — although in
/// practice all values pushed here are finite.
#[derive(Debug, Clone, Copy)]
struct HeapEntry {
    reachability: f64,
    index: usize,
}

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.reachability.total_cmp(&other.reachability) == Ordering::Equal
            && self.index == other.index
    }
}

impl Eq for HeapEntry {}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse so BinaryHeap pops smallest reachability first.
        other
            .reachability
            .total_cmp(&self.reachability)
            .then_with(|| other.index.cmp(&self.index))
    }
}

// ── Free functions ─────────────────────────────────────────────────────────

/// Run the OPTICS algorithm on `points` using the supplied `options`.
///
/// The returned [`OpticsResult`] contains the reachability plot from
/// which clusters at any density level can be extracted; the field
/// `clusters` is populated with the ξ extraction at `options.xi`.
///
/// # Errors
///
/// Returns an error when `min_samples == 0` or `max_eps <= 0`.
pub fn optics(points: &[Point2D], options: &OpticsOptions) -> Result<OpticsResult> {
    if options.min_samples == 0 {
        return Err(AnalyticsError::invalid_parameter(
            "min_samples",
            "must be positive",
        ));
    }
    if options.max_eps <= 0.0 || options.max_eps.is_nan() {
        return Err(AnalyticsError::invalid_parameter(
            "max_eps",
            "must be positive (or f64::INFINITY)",
        ));
    }
    if !(0.0..1.0).contains(&options.xi) {
        return Err(AnalyticsError::invalid_parameter(
            "xi",
            "must lie in [0, 1)",
        ));
    }

    let n = points.len();
    if n == 0 {
        return Ok(OpticsResult {
            ordering: Vec::new(),
            reachability: Vec::new(),
            core_distances: Vec::new(),
            clusters: Vec::new(),
        });
    }

    // Build the R-tree.
    let indexed: Vec<IndexedPoint> = points
        .iter()
        .enumerate()
        .map(|(i, p)| IndexedPoint {
            index: i,
            x: p.x,
            y: p.y,
        })
        .collect();
    let tree = RTree::bulk_load(indexed);

    // Working state. `reach_by_idx` and `core_by_idx` are indexed by the
    // ORIGINAL point index (not by ordering position) until we finalise
    // the result; this matches the textbook OPTICS pseudocode.
    let mut processed: Vec<bool> = vec![false; n];
    let mut reach_by_idx: Vec<f64> = vec![f64::INFINITY; n];
    let mut core_by_idx: Vec<f64> = vec![f64::INFINITY; n];

    let mut ordering: Vec<usize> = Vec::with_capacity(n);
    let mut reachability: Vec<f64> = Vec::with_capacity(n);
    let mut core_distances: Vec<f64> = Vec::with_capacity(n);

    for start in 0..n {
        if processed[start] {
            continue;
        }

        // Compute neighbours and core distance for the seed.
        let seed_neighbours = neighbours_within(&tree, points[start], options.max_eps);
        let seed_core = core_distance(&seed_neighbours, options.min_samples);
        core_by_idx[start] = seed_core;

        // Emit the seed point with its current reachability
        // (typically INFINITY for the first point of a new component).
        processed[start] = true;
        ordering.push(start);
        reachability.push(reach_by_idx[start]);
        core_distances.push(seed_core);

        if !seed_core.is_finite() {
            // Seed is not a core point — move on without expanding.
            continue;
        }

        // Seed the priority queue from the seed's neighbourhood.
        let mut queue: BinaryHeap<HeapEntry> = BinaryHeap::new();
        update_seeds(
            &seed_neighbours,
            start,
            seed_core,
            &mut reach_by_idx,
            &processed,
            &mut queue,
        );

        // Expand.
        while let Some(HeapEntry {
            index: q,
            reachability: r_q,
        }) = queue.pop()
        {
            if processed[q] {
                // The same point can be queued more than once when a
                // shorter reachability is discovered; stale entries are
                // dropped here.
                continue;
            }
            if (reach_by_idx[q] - r_q).abs() > 0.0 && reach_by_idx[q] < r_q {
                // A better reachability was found after this entry was
                // pushed; the canonical entry will be popped later.
                continue;
            }

            let q_neighbours = neighbours_within(&tree, points[q], options.max_eps);
            let q_core = core_distance(&q_neighbours, options.min_samples);
            core_by_idx[q] = q_core;

            processed[q] = true;
            ordering.push(q);
            reachability.push(reach_by_idx[q]);
            core_distances.push(q_core);

            if q_core.is_finite() {
                update_seeds(
                    &q_neighbours,
                    q,
                    q_core,
                    &mut reach_by_idx,
                    &processed,
                    &mut queue,
                );
            }
        }
    }

    let mut result = OpticsResult {
        ordering,
        reachability,
        core_distances,
        clusters: Vec::new(),
    };
    result.clusters = extract_xi_clusters(&result, options.xi);
    Ok(result)
}

/// Update reachability of `centre`'s neighbours and push improved
/// entries into the priority queue.
fn update_seeds(
    neighbours: &[NeighbourInfo],
    centre: usize,
    centre_core: f64,
    reach_by_idx: &mut [f64],
    processed: &[bool],
    queue: &mut BinaryHeap<HeapEntry>,
) {
    for nb in neighbours {
        if nb.index == centre || processed[nb.index] {
            continue;
        }
        let new_reach = centre_core.max(nb.distance);
        if new_reach < reach_by_idx[nb.index] {
            reach_by_idx[nb.index] = new_reach;
            queue.push(HeapEntry {
                reachability: new_reach,
                index: nb.index,
            });
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct NeighbourInfo {
    index: usize,
    /// Euclidean distance (not squared) from the query point.
    distance: f64,
}

/// Collect neighbours of `point` within `max_eps` (inclusive), including
/// the point itself if it is in the tree.
fn neighbours_within(
    tree: &RTree<IndexedPoint>,
    point: Point2D,
    max_eps: f64,
) -> Vec<NeighbourInfo> {
    let mut out: Vec<NeighbourInfo> = Vec::new();
    let query = [point.x, point.y];

    if max_eps.is_finite() {
        let radius_sq = max_eps * max_eps;
        for candidate in tree.locate_within_distance(query, radius_sq) {
            let dx = candidate.x - query[0];
            let dy = candidate.y - query[1];
            let dist = (dx * dx + dy * dy).sqrt();
            if dist <= max_eps {
                out.push(NeighbourInfo {
                    index: candidate.index,
                    distance: dist,
                });
            }
        }
    } else {
        // `locate_within_distance` requires a finite radius, so for
        // unbounded queries we walk the iterator of nearest neighbours.
        for candidate in tree.nearest_neighbor_iter(&query) {
            let dx = candidate.x - query[0];
            let dy = candidate.y - query[1];
            let dist = (dx * dx + dy * dy).sqrt();
            out.push(NeighbourInfo {
                index: candidate.index,
                distance: dist,
            });
        }
    }
    out
}

/// Core distance = distance to the `min_samples`-th nearest neighbour
/// (where the centre point itself counts as one). Returns
/// `f64::INFINITY` when fewer than `min_samples` neighbours are
/// available.
fn core_distance(neighbours: &[NeighbourInfo], min_samples: usize) -> f64 {
    if neighbours.len() < min_samples {
        return f64::INFINITY;
    }
    let mut sorted: Vec<f64> = neighbours.iter().map(|n| n.distance).collect();
    sorted.sort_by(|a, b| a.total_cmp(b));
    // 1-indexed: the `min_samples`-th smallest distance.
    sorted[min_samples - 1]
}

// ── Cluster extraction ─────────────────────────────────────────────────────

/// Extract clusters from the reachability plot via the ξ steep-area
/// method (Ankerst 1999 §4).
///
/// A *steep-down area* at position `i` is where the curve drops by at
/// least a factor of `(1 - ξ)`, formally `reach[i+1] <= reach[i] * (1 - ξ)`
/// with `reach[i] >= reach[i+1]`. A *steep-up area* is the symmetric
/// case where the curve rises by at least the same factor:
/// `reach[i] <= reach[i+1] * (1 - ξ)` with `reach[i] <= reach[i+1]`.
///
/// Each steep-down area is paired with the next compatible steep-up
/// area to form a cluster: the cluster spans `[start_of_down,
/// end_of_up]`. The resulting span must contain at least three points
/// to be retained.
#[must_use]
pub fn extract_xi_clusters(result: &OpticsResult, xi: f64) -> Vec<OpticsCluster> {
    let reach = &result.reachability;
    let n = reach.len();
    if n < 3 || !(0.0..1.0).contains(&xi) {
        return Vec::new();
    }
    let factor = 1.0 - xi;

    // Identify steep-down and steep-up runs. A run extends as long as
    // each adjacent pair satisfies the inequality (and points in between
    // do not violate monotonicity beyond what ξ allows). We keep the
    // runs as `(start, end)` inclusive position pairs.
    let mut downs: Vec<(usize, usize)> = Vec::new();
    let mut ups: Vec<(usize, usize)> = Vec::new();

    let is_steep_down =
        |a: f64, b: f64| -> bool { a.is_finite() && b.is_finite() && a >= b && b <= a * factor };
    let is_steep_up =
        |a: f64, b: f64| -> bool { a.is_finite() && b.is_finite() && a <= b && a <= b * factor };

    let mut i = 0;
    while i + 1 < n {
        let r0 = reach[i];
        let r1 = reach[i + 1];
        if is_steep_down(r0, r1) {
            let s = i;
            let mut j = i + 1;
            while j + 1 < n {
                let a = reach[j];
                let b = reach[j + 1];
                if !is_steep_down(a, b) {
                    break;
                }
                j += 1;
            }
            downs.push((s, j));
            i = j + 1;
        } else if is_steep_up(r0, r1) {
            let s = i;
            let mut j = i + 1;
            while j + 1 < n {
                let a = reach[j];
                let b = reach[j + 1];
                if !is_steep_up(a, b) {
                    break;
                }
                j += 1;
            }
            ups.push((s, j));
            i = j + 1;
        } else {
            i += 1;
        }
    }

    // Pair each steep-down with the next compatible steep-up that
    // begins at or after the end of the down area.
    let mut clusters: Vec<OpticsCluster> = Vec::new();
    for &(d_start, d_end) in &downs {
        if let Some(&(_u_start, u_end)) = ups.iter().find(|&&(u_s, _)| u_s >= d_end) {
            let cluster_start = d_start;
            let cluster_end = u_end;
            let span = cluster_end.saturating_sub(cluster_start).saturating_add(1);
            if span < 3 {
                continue;
            }

            let r_start = reach[cluster_start];
            let r_end = reach[cluster_end];
            let steepness = if r_start.is_finite() && r_end.is_finite() {
                let max_r = r_start.max(r_end).max(f64::EPSILON);
                (r_start - r_end).abs() / max_r
            } else {
                xi
            };

            clusters.push(OpticsCluster {
                start: cluster_start,
                end: cluster_end,
                xi_steepness: steepness,
            });
        }
    }

    clusters
}

/// Extract clusters using the DBSCAN-equivalent rule: a cluster spans a
/// maximal contiguous slice of the reachability plot in which every
/// entry is ≤ `eps`. Reachability values that exceed `eps` (including
/// `f64::INFINITY`) act as separators.
#[must_use]
pub fn extract_dbscan_clusters(result: &OpticsResult, eps: f64) -> Vec<OpticsCluster> {
    if !(eps > 0.0 && eps.is_finite()) {
        return Vec::new();
    }
    let reach = &result.reachability;
    let n = reach.len();
    if n == 0 {
        return Vec::new();
    }

    let mut clusters: Vec<OpticsCluster> = Vec::new();
    let mut start: Option<usize> = None;
    for i in 0..n {
        if reach[i] <= eps {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start.take() {
            if i > s {
                clusters.push(OpticsCluster {
                    start: s,
                    end: i - 1,
                    xi_steepness: 0.0,
                });
            }
        }
    }
    if let Some(s) = start {
        clusters.push(OpticsCluster {
            start: s,
            end: n - 1,
            xi_steepness: 0.0,
        });
    }
    clusters
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_default_values() {
        let opts = OpticsOptions::default();
        assert_eq!(opts.min_samples, 5);
        assert!((opts.xi - 0.05).abs() < 1e-12);
        assert!(opts.max_eps.is_infinite());
    }

    #[test]
    fn empty_input_returns_empty_result() {
        let res = optics(&[], &OpticsOptions::default()).expect("empty");
        assert!(res.ordering.is_empty());
        assert!(res.reachability.is_empty());
        assert!(res.core_distances.is_empty());
        assert!(res.clusters.is_empty());
    }

    #[test]
    fn invalid_min_samples_zero_fails() {
        let opts = OpticsOptions {
            min_samples: 0,
            ..OpticsOptions::default()
        };
        let pts = [Point2D::new(0.0, 0.0)];
        assert!(optics(&pts, &opts).is_err());
    }
}
