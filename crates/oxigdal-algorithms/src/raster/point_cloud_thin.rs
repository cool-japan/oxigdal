//! Point-cloud thinning algorithms: grid (voxel), random (LCG), Poisson disk.
//!
//! Point-cloud thinning is the task of reducing the cardinality of a 3D point
//! set while preserving some structural property of the cloud. Three classical
//! strategies are provided here:
//!
//! - **Grid (voxel) thinning** ([`thin_grid`]) partitions space into cubic
//!   voxels of side `cell_size` and keeps the *first* point that falls into
//!   each voxel (in input order). This produces a roughly uniform sub-sample
//!   at the resolution of the voxel grid and is the standard approach used by
//!   PDAL's `filters.sample` and CloudCompare's "subsample with octree".
//! - **Random thinning** ([`thin_random`]) uniformly draws `target_count`
//!   indices without replacement via a deterministic Fisher-Yates shuffle.
//!   The shuffle is driven by a linear congruential generator (LCG) using
//!   Knuth's MMIX constants — this satisfies the project policy of *no
//!   `rand`-crate dependency* while remaining reproducible across runs and
//!   platforms.
//! - **Poisson-disk thinning** ([`thin_poisson_disk`]) implements Bridson's
//!   spatial-hash dart-throwing in 3D: every candidate point is accepted only
//!   if no previously kept point lies within `min_distance` of it. The spatial
//!   hash uses cubic buckets of side `min_distance`, so each acceptance test
//!   examines at most the 27 neighbouring buckets — giving expected O(N)
//!   runtime, an order of magnitude faster than a naive O(N^2) sweep.
//!
//! All three operators are deterministic functions of their inputs (the random
//! and Poisson-disk variants additionally take a `seed`), preserve input order
//! in their output and return owned `Vec<ThinPoint3>` so the caller may freely
//! mutate the result. Thinning statistics ([`ThinningStats`]) can be obtained
//! in one call via [`thin_with_stats`].
//!
//! # Examples
//!
//! ```
//! use oxigdal_algorithms::raster::{ThinPoint3, ThinningMethod, thin_with_stats};
//!
//! let points = vec![
//!     ThinPoint3::new(0.0, 0.0, 0.0),
//!     ThinPoint3::new(0.1, 0.2, 0.3),
//!     ThinPoint3::new(5.0, 5.0, 5.0),
//! ];
//! let (kept, stats) = thin_with_stats(
//!     &points,
//!     ThinningMethod::Grid { cell_size: 1.0 },
//! );
//! assert_eq!(stats.input_count, 3);
//! assert_eq!(stats.kept_count, kept.len());
//! ```

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A 3D sample point used as input to point-cloud thinning operators.
///
/// `ThinPoint3` is intentionally a Plain-Old-Data struct: it carries no
/// auxiliary attributes (intensity, classification, …) because the thinning
/// operators only depend on the geometric coordinates. Callers that need to
/// preserve attributes typically build a parallel `Vec<Attr>` indexed in the
/// same order as their `Vec<ThinPoint3>` and re-index it using the order of
/// the returned point vector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThinPoint3 {
    /// X coordinate (typically easting / planimetric east).
    pub x: f64,
    /// Y coordinate (typically northing / planimetric north).
    pub y: f64,
    /// Z coordinate (typically elevation).
    pub z: f64,
}

impl ThinPoint3 {
    /// Construct a new [`ThinPoint3`] from its three coordinates.
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
}

/// Selection of thinning algorithm and its parameters.
///
/// This enum is the input to [`thin_with_stats`]; each variant carries the
/// parameters needed by exactly one of the public thinning functions.
#[derive(Debug, Clone, Copy)]
pub enum ThinningMethod {
    /// Voxel-bucket thinning with cubic voxels of side `cell_size`. Equivalent
    /// to PDAL's `filters.sample` with a uniform grid.
    Grid {
        /// Voxel side length in the same units as the input coordinates. Must
        /// be strictly positive; non-positive values cause the input to be
        /// returned unchanged.
        cell_size: f64,
    },
    /// Random sub-sampling that keeps exactly `target_count` points (or fewer
    /// when the input is smaller) via a deterministic Fisher-Yates shuffle.
    Random {
        /// Desired number of output points. If `target_count >= points.len()`
        /// the input is returned unchanged.
        target_count: usize,
        /// LCG seed; identical seeds produce identical outputs.
        seed: u64,
    },
    /// Poisson-disk thinning ensuring every pair of kept points is at least
    /// `min_distance` apart in 3D Euclidean distance.
    PoissonDisk {
        /// Minimum Euclidean separation between any two kept points. Must be
        /// strictly positive; non-positive values cause the input to be
        /// returned unchanged.
        min_distance: f64,
        /// LCG seed used to permute input order before greedy acceptance.
        seed: u64,
    },
}

/// Summary statistics for a single thinning invocation.
#[derive(Debug, Clone, Copy, Default)]
pub struct ThinningStats {
    /// Number of points in the input cloud.
    pub input_count: usize,
    /// Number of points in the output cloud after thinning.
    pub kept_count: usize,
    /// Fraction of input points discarded, in `[0, 1]`. Defined as
    /// `1 - kept_count / input_count`; zero when `input_count == 0`.
    pub reduction_ratio: f64,
}

impl ThinningStats {
    /// Construct a [`ThinningStats`] from the input/output cardinalities,
    /// computing the reduction ratio.
    ///
    /// `reduction_ratio` is `0.0` when `input_count == 0` to avoid a
    /// division-by-zero, and is otherwise clamped to `[0, 1]` by construction
    /// (because `kept_count <= input_count` for every operator in this
    /// module).
    pub fn new(input_count: usize, kept_count: usize) -> Self {
        let reduction_ratio = if input_count == 0 {
            0.0
        } else {
            1.0 - (kept_count as f64 / input_count as f64)
        };
        Self {
            input_count,
            kept_count,
            reduction_ratio,
        }
    }
}

// ---------------------------------------------------------------------------
// Deterministic LCG (no `rand` crate dependency)
// ---------------------------------------------------------------------------

/// One step of Knuth's MMIX 64-bit linear congruential generator.
///
/// `x_{n+1} = a * x_n + c` with `a = 6364136223846793005` and
/// `c = 1442695040888963407`. Both constants are taken from Knuth's *Art of
/// Computer Programming* Vol. 2 §3.3.4, Table 1, line 26 ("MMIX") and yield a
/// full period of 2^64. Wrapping arithmetic is intentional: modular reduction
/// modulo 2^64 is part of the LCG definition.
#[inline]
fn lcg_next(state: u64) -> u64 {
    state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
}

/// In-place Fisher-Yates shuffle driven by [`lcg_next`].
///
/// The top 32 bits of the LCG state are used to select the swap target, since
/// the low-order bits of a power-of-two-modulus LCG have short periods (this
/// is a well-known weakness of LCGs documented by L'Ecuyer 1990 and others).
/// The `seed.wrapping_add(1)` step prevents the degenerate fixed point at
/// `state == 0`, which would otherwise collapse the shuffle to the identity.
fn lcg_shuffle<T>(slice: &mut [T], seed: u64) {
    if slice.len() < 2 {
        return;
    }
    let mut state = seed.wrapping_add(1);
    for i in (1..slice.len()).rev() {
        state = lcg_next(state);
        let j = ((state >> 32) as usize) % (i + 1);
        slice.swap(i, j);
    }
}

// ---------------------------------------------------------------------------
// Grid (voxel) thinning
// ---------------------------------------------------------------------------

/// Grid (voxel) thinning: keep one point per cubic voxel of side `cell_size`.
///
/// The voxel containing a point `(x, y, z)` is identified by the integer
/// triple `(floor(x / s), floor(y / s), floor(z / s))` where `s == cell_size`.
/// The *first* point (in input order) that lands in each voxel is kept; later
/// arrivals are silently discarded. This is the same convention used by
/// PDAL's `filters.sample` and is preferred over "voxel centroid" thinning
/// when the caller needs to preserve original point identities (for example
/// to carry classification or RGB attributes through the thinning step).
///
/// Edge cases:
///
/// - Empty input returns an empty `Vec`.
/// - `cell_size <= 0.0` returns a copy of the input (no thinning).
/// - Points with non-finite coordinates are bucketed by `floor(NaN) as i64`,
///   which the standard library defines as `i64::MIN` for NaN — so all NaN
///   points map into the same degenerate voxel and only the first is kept.
///
/// # Examples
///
/// ```
/// use oxigdal_algorithms::raster::{ThinPoint3, thin_grid};
///
/// // Three points in the same unit voxel collapse to one.
/// let pts = vec![
///     ThinPoint3::new(0.1, 0.1, 0.1),
///     ThinPoint3::new(0.5, 0.5, 0.5),
///     ThinPoint3::new(0.9, 0.9, 0.9),
/// ];
/// let kept = thin_grid(&pts, 1.0);
/// assert_eq!(kept.len(), 1);
/// assert_eq!(kept[0], ThinPoint3::new(0.1, 0.1, 0.1));
/// ```
pub fn thin_grid(points: &[ThinPoint3], cell_size: f64) -> Vec<ThinPoint3> {
    if points.is_empty() || cell_size <= 0.0 || !cell_size.is_finite() {
        return points.to_vec();
    }
    let mut buckets: HashMap<(i64, i64, i64), usize> = HashMap::with_capacity(points.len());
    let mut keep_indices: Vec<usize> = Vec::new();
    for (i, p) in points.iter().enumerate() {
        let key = voxel_key(p, cell_size);
        if let std::collections::hash_map::Entry::Vacant(e) = buckets.entry(key) {
            e.insert(i);
            keep_indices.push(i);
        }
    }
    keep_indices.into_iter().map(|i| points[i]).collect()
}

/// Map a point to its containing voxel under voxel side `cell_size`.
#[inline]
fn voxel_key(p: &ThinPoint3, cell_size: f64) -> (i64, i64, i64) {
    (
        (p.x / cell_size).floor() as i64,
        (p.y / cell_size).floor() as i64,
        (p.z / cell_size).floor() as i64,
    )
}

// ---------------------------------------------------------------------------
// Random thinning
// ---------------------------------------------------------------------------

/// Random thinning: keep `target_count` points selected uniformly without
/// replacement via a deterministic Fisher-Yates shuffle.
///
/// The shuffle is driven by an internal LCG (`lcg_next`) rather than the
/// `rand` crate, in keeping with the project's *no `rand`* dependency policy.
/// Identical `seed` values produce identical outputs across runs and across
/// platforms (the LCG state is a `u64` and only `wrapping_*` arithmetic is
/// used). Output points are returned in input order — the shuffle is applied
/// to an index vector, the first `target_count` indices are retained, and the
/// final point vector is materialised in the original order.
///
/// Edge cases:
///
/// - Empty input returns an empty `Vec`.
/// - `target_count >= points.len()` returns a copy of the input (no thinning).
/// - `target_count == 0` returns an empty `Vec`.
///
/// # Examples
///
/// ```
/// use oxigdal_algorithms::raster::{ThinPoint3, thin_random};
///
/// let pts: Vec<_> = (0..100)
///     .map(|i| ThinPoint3::new(i as f64, 0.0, 0.0))
///     .collect();
/// let a = thin_random(&pts, 10, 0xC0FFEE);
/// let b = thin_random(&pts, 10, 0xC0FFEE);
/// assert_eq!(a, b); // deterministic for a given seed
/// assert_eq!(a.len(), 10);
/// ```
pub fn thin_random(points: &[ThinPoint3], target_count: usize, seed: u64) -> Vec<ThinPoint3> {
    if points.is_empty() {
        return Vec::new();
    }
    if target_count >= points.len() {
        return points.to_vec();
    }
    if target_count == 0 {
        return Vec::new();
    }

    let mut indices: Vec<usize> = (0..points.len()).collect();
    lcg_shuffle(&mut indices, seed);
    indices.truncate(target_count);
    indices.sort_unstable();
    indices.into_iter().map(|i| points[i]).collect()
}

// ---------------------------------------------------------------------------
// Poisson-disk thinning
// ---------------------------------------------------------------------------

/// Poisson-disk thinning: keep a maximal greedy subset such that every pair
/// of kept points is at least `min_distance` apart in 3D Euclidean distance.
///
/// This is the spatial-hash variant of Bridson's 2007 dart-throwing
/// algorithm specialised to the case where the candidate pool is a fixed,
/// pre-existing point set (so the "active list" reduces to the shuffled
/// input order). The spatial hash uses cubic buckets of side `min_distance`:
/// any kept point within `min_distance` of a candidate must lie in one of the
/// 27 buckets adjacent to the candidate's own bucket (including the
/// candidate's bucket itself), so each acceptance test inspects at most 27
/// buckets of constant expected occupancy — yielding expected `O(N)`
/// runtime on uniformly distributed inputs.
///
/// The input order is randomised by an LCG-driven Fisher-Yates shuffle
/// (see `lcg_shuffle`) before greedy acceptance, so identical `seed`
/// values produce identical outputs while avoiding the directional bias
/// of always favouring early input points. Output points are returned in
/// original input order: after the greedy pass the kept indices are
/// sorted ascending and the point vector is materialised from that order.
///
/// Edge cases:
///
/// - Empty input returns an empty `Vec`.
/// - `min_distance <= 0.0` returns a copy of the input (no thinning).
///
/// # Complexity
///
/// Expected `O(N)` time and `O(N)` extra space (the spatial hash and kept-index
/// vector together). Worst case (all points colliding into one bucket) is
/// `O(N^2)`, but this requires pathological clustering at the bucket scale.
///
/// # Examples
///
/// ```
/// use oxigdal_algorithms::raster::{ThinPoint3, thin_poisson_disk};
///
/// // Eight points along a line, separated by 0.1: at min_distance=0.25 we
/// // keep one in three (roughly).
/// let pts: Vec<_> = (0..8)
///     .map(|i| ThinPoint3::new(0.1 * i as f64, 0.0, 0.0))
///     .collect();
/// let kept = thin_poisson_disk(&pts, 0.25, 42);
/// assert!(kept.len() < pts.len());
/// // Every pair is at least 0.25 apart.
/// for (i, p) in kept.iter().enumerate() {
///     for q in &kept[i + 1..] {
///         let d = ((p.x - q.x).powi(2) + (p.y - q.y).powi(2) + (p.z - q.z).powi(2)).sqrt();
///         assert!(d >= 0.25);
///     }
/// }
/// ```
pub fn thin_poisson_disk(points: &[ThinPoint3], min_distance: f64, seed: u64) -> Vec<ThinPoint3> {
    if points.is_empty() {
        return Vec::new();
    }
    if min_distance <= 0.0 || !min_distance.is_finite() {
        return points.to_vec();
    }

    let cell_size = min_distance;
    let min_dist_sq = min_distance * min_distance;

    let mut shuffled_indices: Vec<usize> = (0..points.len()).collect();
    lcg_shuffle(&mut shuffled_indices, seed);

    let mut buckets: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    let mut kept: Vec<usize> = Vec::new();

    for &i in &shuffled_indices {
        let p = points[i];
        let (kx, ky, kz) = voxel_key(&p, cell_size);

        // Check the 27 buckets adjacent to (kx, ky, kz) (including itself) for
        // any kept point closer than `min_distance`.
        let mut too_close = false;
        'outer: for dx in -1..=1i64 {
            for dy in -1..=1i64 {
                for dz in -1..=1i64 {
                    if let Some(bucket) = buckets.get(&(kx + dx, ky + dy, kz + dz)) {
                        for &j in bucket {
                            let q = points[j];
                            let ex = p.x - q.x;
                            let ey = p.y - q.y;
                            let ez = p.z - q.z;
                            if ex * ex + ey * ey + ez * ez < min_dist_sq {
                                too_close = true;
                                break 'outer;
                            }
                        }
                    }
                }
            }
        }

        if !too_close {
            buckets.entry((kx, ky, kz)).or_default().push(i);
            kept.push(i);
        }
    }

    // Preserve the original input order in the output.
    kept.sort_unstable();
    kept.into_iter().map(|i| points[i]).collect()
}

// ---------------------------------------------------------------------------
// Dispatcher with stats
// ---------------------------------------------------------------------------

/// Dispatch [`ThinningMethod`] and return both the thinned point set and a
/// [`ThinningStats`] summary.
///
/// This is the recommended entry point for callers that need to report on
/// how aggressive the thinning was, or that want to switch algorithms based
/// on configuration without duplicating the dispatching boilerplate.
///
/// # Examples
///
/// ```
/// use oxigdal_algorithms::raster::{ThinPoint3, ThinningMethod, thin_with_stats};
///
/// let pts: Vec<_> = (0..1000)
///     .map(|i| ThinPoint3::new(i as f64, 0.0, 0.0))
///     .collect();
/// let (kept, stats) = thin_with_stats(
///     &pts,
///     ThinningMethod::Random { target_count: 100, seed: 7 },
/// );
/// assert_eq!(stats.input_count, 1000);
/// assert_eq!(stats.kept_count, 100);
/// assert!((stats.reduction_ratio - 0.9).abs() < 1e-12);
/// assert_eq!(kept.len(), 100);
/// ```
pub fn thin_with_stats(
    points: &[ThinPoint3],
    method: ThinningMethod,
) -> (Vec<ThinPoint3>, ThinningStats) {
    let out = match method {
        ThinningMethod::Grid { cell_size } => thin_grid(points, cell_size),
        ThinningMethod::Random { target_count, seed } => thin_random(points, target_count, seed),
        ThinningMethod::PoissonDisk { min_distance, seed } => {
            thin_poisson_disk(points, min_distance, seed)
        }
    };
    let stats = ThinningStats::new(points.len(), out.len());
    (out, stats)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lcg_next_is_deterministic() {
        let a = lcg_next(42);
        let b = lcg_next(42);
        assert_eq!(a, b);
    }

    #[test]
    fn lcg_shuffle_is_seed_deterministic() {
        let mut a: Vec<usize> = (0..100).collect();
        let mut b: Vec<usize> = (0..100).collect();
        lcg_shuffle(&mut a, 0xDEADBEEF);
        lcg_shuffle(&mut b, 0xDEADBEEF);
        assert_eq!(a, b);
    }

    #[test]
    fn lcg_shuffle_permutes_values() {
        let mut a: Vec<usize> = (0..100).collect();
        lcg_shuffle(&mut a, 7);
        let mut sorted = a.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..100).collect::<Vec<_>>());
    }

    #[test]
    fn thinning_stats_zero_input() {
        let s = ThinningStats::new(0, 0);
        assert_eq!(s.input_count, 0);
        assert_eq!(s.kept_count, 0);
        assert_eq!(s.reduction_ratio, 0.0);
    }

    #[test]
    fn thinning_stats_half_kept() {
        let s = ThinningStats::new(100, 50);
        assert!((s.reduction_ratio - 0.5).abs() < 1e-12);
    }

    #[test]
    fn grid_negative_cell_size_returns_input() {
        let pts = vec![
            ThinPoint3::new(0.0, 0.0, 0.0),
            ThinPoint3::new(1.0, 1.0, 1.0),
        ];
        let out = thin_grid(&pts, -1.0);
        assert_eq!(out, pts);
    }
}
