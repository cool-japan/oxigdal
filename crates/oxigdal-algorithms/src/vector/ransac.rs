//! RANSAC robust model fitting (lines and planes).
//!
//! RANSAC (RANdom SAmple Consensus) is an iterative method for estimating the
//! parameters of a mathematical model from a set of observed data that contains
//! a large fraction of outliers. This module provides robust fitting of:
//!
//! - 2D lines (`a x + b y + c = 0`, normalized `a² + b² = 1`) via
//!   [`ransac_fit_line`].
//! - 3D planes (`a x + b y + c z + d = 0`, normalized `a² + b² + c² = 1`) via
//!   [`ransac_fit_plane`].
//!
//! The implementation follows the classic Fischler–Bolles algorithm with an
//! adaptive stopping criterion: the required number of iterations is recomputed
//! whenever a better consensus set is found, so clean data terminates quickly
//! while noisy data exhausts the iteration budget.
//!
//! ## Robustness details
//!
//! The fit complements Slice 21's Weiszfeld `geometric_median`: where the
//! geometric median is robust to outliers in a *location* estimate, RANSAC is
//! robust to outliers in a *parametric model* estimate. After the consensus set
//! is found, the model is refined with total (orthogonal) least squares on the
//! inliers, which minimizes the perpendicular distances rather than the
//! vertical residuals — appropriate when there is no privileged axis.
//!
//! ## Determinism
//!
//! No external RNG is used. Sampling relies on a deterministic Knuth MMIX linear
//! congruential generator seeded by [`RansacOptions::seed`], so identical inputs
//! and seeds always produce identical results (see the seed-reproducibility
//! tests). This honors the project's SciRS2 policy (no `rand` / `rand_distr`).
//!
//! ## Example
//!
//! ```
//! use oxigdal_algorithms::vector::{Point, RansacOptions, ransac_fit_line};
//!
//! // Points on the line y = 2x + 1 with one gross outlier.
//! let points = [
//!     Point::new(0.0, 1.0),
//!     Point::new(1.0, 3.0),
//!     Point::new(2.0, 5.0),
//!     Point::new(3.0, 7.0),
//!     Point::new(4.0, 9.0),
//!     Point::new(2.0, 50.0), // outlier
//! ];
//! let result = ransac_fit_line(&points, &RansacOptions::default())?;
//! assert!(result.converged);
//! assert!(!result.inliers.contains(&5)); // outlier rejected
//! # Ok::<(), oxigdal_algorithms::error::AlgorithmError>(())
//! ```

use crate::error::AlgorithmError;
use oxigdal_core::vector::Point;

/// Knuth MMIX LCG multiplier (matches the convention used elsewhere in the
/// workspace, e.g. `oxigdal-streaming` retry jitter and the `oxigdal-index`
/// test harness).
const LCG_MULTIPLIER: u64 = 6_364_136_223_846_793_005;

/// Knuth MMIX LCG increment.
const LCG_INCREMENT: u64 = 1_442_695_040_888_963_407;

/// Tolerance for treating sampled points as coincident / collinear when forming
/// a candidate model.
const DEGENERATE_EPSILON: f64 = 1e-12;

/// Configuration for RANSAC fitting.
///
/// Use [`RansacOptions::default`] for sensible defaults and override individual
/// fields as needed.
#[derive(Debug, Clone, PartialEq)]
pub struct RansacOptions {
    /// Maximum number of sampling iterations.
    pub max_iterations: usize,
    /// Maximum distance from the model for a point to be counted as an inlier.
    pub inlier_threshold: f64,
    /// Minimum number of inliers required for the result to be considered valid.
    pub min_inlier_count: usize,
    /// Seed for the deterministic Knuth MMIX LCG used to draw samples.
    pub seed: u64,
    /// Desired probability that at least one outlier-free sample is drawn; used
    /// by the adaptive stopping criterion.
    pub confidence: f64,
}

impl Default for RansacOptions {
    fn default() -> Self {
        Self {
            max_iterations: 1000,
            inlier_threshold: 1e-3,
            min_inlier_count: 2,
            seed: 0xDEAD_BEEF,
            confidence: 0.99,
        }
    }
}

/// Result of a RANSAC fit.
///
/// `M` is the fitted model type ([`RansacLineModel`] or [`RansacPlaneModel`]).
#[derive(Debug, Clone)]
pub struct RansacResult<M> {
    /// The best model found.
    pub model: M,
    /// Indices (into the input slice) of the inliers supporting `model`.
    pub inliers: Vec<usize>,
    /// Number of sampling iterations actually performed.
    pub iterations: usize,
    /// Whether the consensus set reached `min_inlier_count`.
    pub converged: bool,
}

/// A 2D line in implicit form `a x + b y + c = 0` with the normal `(a, b)`
/// normalized so that `a² + b² = 1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RansacLineModel {
    /// `x` coefficient of the normal.
    pub a: f64,
    /// `y` coefficient of the normal.
    pub b: f64,
    /// Signed offset.
    pub c: f64,
}

impl RansacLineModel {
    /// Builds a line through two points.
    ///
    /// Returns `None` when the points are coincident (within
    /// `DEGENERATE_EPSILON`). The normal `(a, b)` is perpendicular to the
    /// direction `p2 - p1`.
    #[must_use]
    pub fn from_two_points(p1: Point, p2: Point) -> Option<Self> {
        let dx = p2.x() - p1.x();
        let dy = p2.y() - p1.y();
        let norm = (dx * dx + dy * dy).sqrt();
        if !norm.is_finite() || norm <= DEGENERATE_EPSILON {
            return None;
        }
        // Normal is perpendicular to the direction (dx, dy): (-dy, dx).
        let a = -dy / norm;
        let b = dx / norm;
        let c = -(a * p1.x() + b * p1.y());
        Some(Self { a, b, c })
    }

    /// Perpendicular distance from a point to the line (`a`, `b` normalized).
    #[must_use]
    pub fn distance_to(&self, p: Point) -> f64 {
        (self.a * p.x() + self.b * p.y() + self.c).abs()
    }

    /// Unit direction vector of the line: `(-b, a)`.
    #[must_use]
    pub fn direction(&self) -> (f64, f64) {
        (-self.b, self.a)
    }

    /// The point on the line closest to the origin: `(-a*c, -b*c)`.
    #[must_use]
    pub fn point_on_line(&self) -> (f64, f64) {
        (-self.a * self.c, -self.b * self.c)
    }
}

/// A 3D plane in implicit form `a x + b y + c z + d = 0` with the normal
/// `(a, b, c)` normalized so that `a² + b² + c² = 1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RansacPlaneModel {
    /// `x` coefficient of the normal.
    pub a: f64,
    /// `y` coefficient of the normal.
    pub b: f64,
    /// `z` coefficient of the normal.
    pub c: f64,
    /// Signed offset.
    pub d: f64,
}

impl RansacPlaneModel {
    /// Builds a plane through three points.
    ///
    /// Returns `None` when the points are collinear (cross product magnitude
    /// within `DEGENERATE_EPSILON`).
    #[must_use]
    pub fn from_three_points(
        p1: (f64, f64, f64),
        p2: (f64, f64, f64),
        p3: (f64, f64, f64),
    ) -> Option<Self> {
        let v1 = (p2.0 - p1.0, p2.1 - p1.1, p2.2 - p1.2);
        let v2 = (p3.0 - p1.0, p3.1 - p1.1, p3.2 - p1.2);
        // Normal = v1 × v2.
        let nx = v1.1 * v2.2 - v1.2 * v2.1;
        let ny = v1.2 * v2.0 - v1.0 * v2.2;
        let nz = v1.0 * v2.1 - v1.1 * v2.0;
        let norm = (nx * nx + ny * ny + nz * nz).sqrt();
        if !norm.is_finite() || norm <= DEGENERATE_EPSILON {
            return None;
        }
        let a = nx / norm;
        let b = ny / norm;
        let c = nz / norm;
        let d = -(a * p1.0 + b * p1.1 + c * p1.2);
        Some(Self { a, b, c, d })
    }

    /// Perpendicular distance from a point to the plane (`a`, `b`, `c`
    /// normalized).
    #[must_use]
    pub fn distance_to(&self, p: (f64, f64, f64)) -> f64 {
        (self.a * p.0 + self.b * p.1 + self.c * p.2 + self.d).abs()
    }

    /// Unit normal vector of the plane: `(a, b, c)`.
    #[must_use]
    pub fn normal(&self) -> (f64, f64, f64) {
        (self.a, self.b, self.c)
    }
}

/// Advances a Knuth MMIX LCG state in place and returns the new state.
///
/// `next = state * 6364136223846793005 + 1442695040888963407` (wrapping).
/// This matches the convention used by `oxigdal-streaming::cloud::retry` and
/// the `oxigdal-index` test harness.
#[inline]
fn lcg_next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(LCG_MULTIPLIER)
        .wrapping_add(LCG_INCREMENT);
    *state
}

/// Draws a uniform index in `[0, n)` from the LCG.
///
/// Uses the upper 31 bits of the advanced state (the best-mixed bits of an LCG)
/// before reducing modulo `n`. Precondition: `n >= 1`.
#[inline]
fn lcg_index(n: usize, state: &mut u64) -> usize {
    let raw = lcg_next(state);
    ((raw >> 33) as usize) % n
}

/// Draws two distinct indices in `[0, n)` via rejection sampling.
///
/// Precondition: `n >= 2`.
fn sample_two_distinct(n: usize, state: &mut u64) -> (usize, usize) {
    let i = lcg_index(n, state);
    let mut j = lcg_index(n, state);
    while j == i {
        j = lcg_index(n, state);
    }
    (i, j)
}

/// Draws three distinct indices in `[0, n)` via rejection sampling.
///
/// Precondition: `n >= 3`.
fn sample_three_distinct(n: usize, state: &mut u64) -> (usize, usize, usize) {
    let i = lcg_index(n, state);
    let mut j = lcg_index(n, state);
    while j == i {
        j = lcg_index(n, state);
    }
    let mut k = lcg_index(n, state);
    while k == i || k == j {
        k = lcg_index(n, state);
    }
    (i, j, k)
}

/// Computes the adaptive iteration count for RANSAC.
///
/// `N = log(1 - confidence) / log(1 - inlier_ratio^sample_size)`, clamped to
/// `[1, max]`.
///
/// Guards:
/// - `inlier_ratio >= 1.0` ⇒ `1` (a single clean sample suffices; avoids
///   `log(0) = -inf` and the resulting `NaN`).
/// - `inlier_ratio <= 0.0` ⇒ `max` (no inliers known; exhaust the budget).
/// - any non-finite intermediate ⇒ `max`.
fn adaptive_iteration_count(
    inlier_ratio: f64,
    sample_size: usize,
    confidence: f64,
    max: usize,
) -> usize {
    if max == 0 {
        return 0;
    }
    if !inlier_ratio.is_finite() || inlier_ratio <= 0.0 {
        return max;
    }
    if inlier_ratio >= 1.0 {
        return 1;
    }
    // Clamp confidence into a usable open interval to keep the logarithms finite.
    let conf = if !confidence.is_finite() {
        return max;
    } else {
        confidence.clamp(0.0, 1.0 - f64::EPSILON)
    };
    let prob_all_inliers = inlier_ratio.powi(sample_size as i32);
    let denom = (1.0 - prob_all_inliers).ln();
    let numer = (1.0 - conf).ln();
    if !denom.is_finite() || !numer.is_finite() || denom >= 0.0 {
        // denom should be negative; if not (e.g. prob_all_inliers ~ 0 making
        // 1 - p ~ 1, ln ~ 0), fall back to the full budget.
        return max;
    }
    let raw = numer / denom;
    if !raw.is_finite() {
        return max;
    }
    let needed = raw.ceil();
    if needed <= 1.0 {
        1
    } else if needed >= max as f64 {
        max
    } else {
        needed as usize
    }
}

/// Fits a line to `points` using total (orthogonal) least squares restricted to
/// `indices`.
///
/// Returns `None` if fewer than two indices are supplied or the covariance is
/// degenerate (all selected points coincide), so the caller can fall back to the
/// sampled model.
fn least_squares_line(points: &[Point], indices: &[usize]) -> Option<RansacLineModel> {
    let n = indices.len();
    if n < 2 {
        return None;
    }
    let inv_n = 1.0 / n as f64;
    let mut mean_x = 0.0;
    let mut mean_y = 0.0;
    for &idx in indices {
        mean_x += points[idx].x();
        mean_y += points[idx].y();
    }
    mean_x *= inv_n;
    mean_y *= inv_n;

    let mut sxx = 0.0;
    let mut sxy = 0.0;
    let mut syy = 0.0;
    for &idx in indices {
        let dx = points[idx].x() - mean_x;
        let dy = points[idx].y() - mean_y;
        sxx += dx * dx;
        sxy += dx * dy;
        syy += dy * dy;
    }

    // Degenerate spread: all points effectively coincide.
    if sxx + syy <= DEGENERATE_EPSILON {
        return None;
    }

    // Eigen-decomposition of the symmetric 2×2 covariance
    // [[sxx, sxy], [sxy, syy]]. The line normal is the eigenvector of the
    // *smaller* eigenvalue (the direction of least variance).
    let trace_half = 0.5 * (sxx + syy);
    let diff_half = 0.5 * (sxx - syy);
    let radius = (diff_half * diff_half + sxy * sxy).sqrt();
    let lambda_min = trace_half - radius;

    // Eigenvector for lambda_min: solve (C - lambda_min I) v = 0. Use the more
    // numerically stable of the two rows.
    let (mut a, mut b) = {
        let r0x = sxx - lambda_min;
        let r0y = sxy;
        let r1x = sxy;
        let r1y = syy - lambda_min;
        // The eigenvector is orthogonal to a non-zero row of (C - lambda I).
        if r0x * r0x + r0y * r0y >= r1x * r1x + r1y * r1y {
            (-r0y, r0x)
        } else {
            (-r1y, r1x)
        }
    };

    let norm = (a * a + b * b).sqrt();
    if !norm.is_finite() || norm <= DEGENERATE_EPSILON {
        return None;
    }
    a /= norm;
    b /= norm;
    let c = -(a * mean_x + b * mean_y);
    Some(RansacLineModel { a, b, c })
}

/// Multiplies a symmetric 3×3 matrix (stored as `[xx, xy, xz, yy, yz, zz]`) by a
/// vector.
#[inline]
fn sym3_mul(m: &[f64; 6], v: (f64, f64, f64)) -> (f64, f64, f64) {
    (
        m[0] * v.0 + m[1] * v.1 + m[2] * v.2,
        m[1] * v.0 + m[3] * v.1 + m[4] * v.2,
        m[2] * v.0 + m[4] * v.1 + m[5] * v.2,
    )
}

#[inline]
fn vec3_norm(v: (f64, f64, f64)) -> f64 {
    (v.0 * v.0 + v.1 * v.1 + v.2 * v.2).sqrt()
}

#[inline]
fn vec3_normalize(v: (f64, f64, f64)) -> Option<(f64, f64, f64)> {
    let n = vec3_norm(v);
    if !n.is_finite() || n <= DEGENERATE_EPSILON {
        None
    } else {
        Some((v.0 / n, v.1 / n, v.2 / n))
    }
}

#[inline]
fn vec3_cross(a: (f64, f64, f64), b: (f64, f64, f64)) -> (f64, f64, f64) {
    (
        a.1 * b.2 - a.2 * b.1,
        a.2 * b.0 - a.0 * b.2,
        a.0 * b.1 - a.1 * b.0,
    )
}

#[inline]
fn vec3_dot(a: (f64, f64, f64), b: (f64, f64, f64)) -> f64 {
    a.0 * b.0 + a.1 * b.1 + a.2 * b.2
}

/// Dominant eigenvector of a symmetric 3×3 matrix via power iteration.
///
/// Returns the (unit) eigenvector and its eigenvalue, or `None` if the matrix is
/// effectively zero.
fn dominant_eigenvector(m: &[f64; 6], seed: (f64, f64, f64)) -> Option<((f64, f64, f64), f64)> {
    const MAX_ITER: usize = 50;
    const TOL: f64 = 1e-12;
    let mut v = vec3_normalize(seed).unwrap_or((1.0, 0.0, 0.0));
    let mut prev_eigenvalue = 0.0;
    for _ in 0..MAX_ITER {
        let mv = sym3_mul(m, v);
        // `?` propagates `None` when the matrix maps `v` to ~0 (near-zero matrix).
        let next = vec3_normalize(mv)?;
        // Rayleigh quotient gives the eigenvalue estimate.
        let eigenvalue = vec3_dot(next, sym3_mul(m, next));
        if (eigenvalue - prev_eigenvalue).abs() <= TOL * eigenvalue.abs().max(1.0) {
            return Some((next, eigenvalue));
        }
        prev_eigenvalue = eigenvalue;
        v = next;
    }
    let eigenvalue = vec3_dot(v, sym3_mul(m, v));
    Some((v, eigenvalue))
}

/// Fits a plane to `points` using total least squares restricted to `indices`.
///
/// The plane normal is the eigenvector of the *smallest* eigenvalue of the 3×3
/// covariance. Rather than risk an ill-conditioned inverse-power iteration, the
/// two dominant eigenvectors are found by deflated power iteration and the
/// normal is recovered as their cross product (which is, up to sign, the
/// least-variance direction). Returns `None` for fewer than three indices or a
/// degenerate (rank-deficient) covariance, so the caller can fall back.
fn least_squares_plane(points: &[(f64, f64, f64)], indices: &[usize]) -> Option<RansacPlaneModel> {
    let n = indices.len();
    if n < 3 {
        return None;
    }
    let inv_n = 1.0 / n as f64;
    let mut mean = (0.0, 0.0, 0.0);
    for &idx in indices {
        let p = points[idx];
        mean.0 += p.0;
        mean.1 += p.1;
        mean.2 += p.2;
    }
    mean.0 *= inv_n;
    mean.1 *= inv_n;
    mean.2 *= inv_n;

    // Symmetric covariance stored as [xx, xy, xz, yy, yz, zz].
    let mut cov = [0.0_f64; 6];
    for &idx in indices {
        let p = points[idx];
        let dx = p.0 - mean.0;
        let dy = p.1 - mean.1;
        let dz = p.2 - mean.2;
        cov[0] += dx * dx;
        cov[1] += dx * dy;
        cov[2] += dx * dz;
        cov[3] += dy * dy;
        cov[4] += dy * dz;
        cov[5] += dz * dz;
    }

    let total_spread = cov[0] + cov[3] + cov[5];
    if total_spread <= DEGENERATE_EPSILON {
        return None;
    }

    // First dominant eigenvector.
    let (v1, lambda1) = dominant_eigenvector(&cov, (1.0, 0.0, 0.0))?;

    // Deflate: cov' = cov - lambda1 * v1 v1ᵀ, then find the next dominant
    // eigenvector. The deflated matrix is still symmetric.
    let deflated = [
        cov[0] - lambda1 * v1.0 * v1.0,
        cov[1] - lambda1 * v1.0 * v1.1,
        cov[2] - lambda1 * v1.0 * v1.2,
        cov[3] - lambda1 * v1.1 * v1.1,
        cov[4] - lambda1 * v1.1 * v1.2,
        cov[5] - lambda1 * v1.2 * v1.2,
    ];
    // Seed the second iteration with a vector orthogonal to v1 for faster and
    // more reliable convergence away from v1.
    let helper = if v1.0.abs() <= 0.9 {
        (1.0, 0.0, 0.0)
    } else {
        (0.0, 1.0, 0.0)
    };
    let seed2 = vec3_cross(v1, helper);
    let (v2, _lambda2) = dominant_eigenvector(&deflated, seed2)?;

    // The least-variance direction is orthogonal to the two dominant ones.
    let normal = vec3_normalize(vec3_cross(v1, v2))?;
    let d = -(normal.0 * mean.0 + normal.1 * mean.1 + normal.2 * mean.2);
    Some(RansacPlaneModel {
        a: normal.0,
        b: normal.1,
        c: normal.2,
        d,
    })
}

/// Validates that every supplied 2D point has finite coordinates.
fn all_points_finite_2d(points: &[Point]) -> bool {
    points
        .iter()
        .all(|p| p.x().is_finite() && p.y().is_finite())
}

/// Validates that every supplied 3D point has finite coordinates.
fn all_points_finite_3d(points: &[(f64, f64, f64)]) -> bool {
    points
        .iter()
        .all(|p| p.0.is_finite() && p.1.is_finite() && p.2.is_finite())
}

/// Robustly fits a 2D line to `points` using RANSAC.
///
/// # Errors
///
/// Returns [`AlgorithmError::InvalidInput`] when fewer than two points are
/// supplied or any coordinate is non-finite.
///
/// # Behavior
///
/// Each iteration samples two distinct points, builds a candidate line, counts
/// inliers (perpendicular distance `<= inlier_threshold`), and keeps the model
/// with the most inliers (requiring at least `min_inlier_count`). The required
/// iteration count is recomputed adaptively after each improvement so clean data
/// terminates early. The best consensus set is then refined with total least
/// squares; if the refinement is degenerate, the sampled model is retained.
pub fn ransac_fit_line(
    points: &[Point],
    options: &RansacOptions,
) -> Result<RansacResult<RansacLineModel>, AlgorithmError> {
    if points.len() < 2 {
        return Err(AlgorithmError::InvalidInput(format!(
            "ransac_fit_line requires at least 2 points, got {}",
            points.len()
        )));
    }
    if !all_points_finite_2d(points) {
        return Err(AlgorithmError::InvalidInput(
            "ransac_fit_line requires all point coordinates to be finite".to_string(),
        ));
    }

    let n = points.len();
    let threshold = options.inlier_threshold;
    let min_inliers = options.min_inlier_count.max(2);
    let mut state = options.seed;

    let mut best_model: Option<RansacLineModel> = None;
    let mut best_inlier_count = 0usize;
    let mut iterations = 0usize;
    // Start by requiring the full budget; tighten as better models are found.
    let mut needed_iterations = options.max_iterations;

    while iterations < needed_iterations && iterations < options.max_iterations {
        iterations += 1;
        let (i, j) = sample_two_distinct(n, &mut state);
        let candidate = match RansacLineModel::from_two_points(points[i].clone(), points[j].clone())
        {
            Some(model) => model,
            None => continue,
        };

        let mut inlier_count = 0usize;
        for p in points {
            if candidate.distance_to(p.clone()) <= threshold {
                inlier_count += 1;
            }
        }

        if inlier_count > best_inlier_count {
            best_inlier_count = inlier_count;
            best_model = Some(candidate);
            let inlier_ratio = inlier_count as f64 / n as f64;
            needed_iterations = adaptive_iteration_count(
                inlier_ratio,
                2,
                options.confidence,
                options.max_iterations,
            );
        }
    }

    let converged = best_inlier_count >= min_inliers;

    // Resolve the best model. If nothing met even a single valid sample (all
    // candidates degenerate), fall back to a model through the first two points
    // or, failing that, an axis-aligned default so we always return something.
    let sampled_model = best_model.unwrap_or_else(|| {
        RansacLineModel::from_two_points(points[0].clone(), points[1].clone()).unwrap_or(
            RansacLineModel {
                a: 0.0,
                b: 1.0,
                c: -points[0].y(),
            },
        )
    });

    // Recompute the inlier set against the best sampled model.
    let inliers: Vec<usize> = points
        .iter()
        .enumerate()
        .filter(|(_, p)| sampled_model.distance_to((*p).clone()) <= threshold)
        .map(|(idx, _)| idx)
        .collect();

    // Refit via total least squares on the consensus set, falling back to the
    // sampled model if the covariance is degenerate.
    let model = match least_squares_line(points, &inliers) {
        Some(refined) => {
            // Guard against a refit that loses inliers due to numerical drift:
            // keep the refit only if it retains at least as many inliers.
            let refit_inliers = points
                .iter()
                .filter(|p| refined.distance_to((*p).clone()) <= threshold)
                .count();
            if refit_inliers >= inliers.len() {
                refined
            } else {
                sampled_model
            }
        }
        None => sampled_model,
    };

    // Final inlier set against the model we are returning.
    let final_inliers: Vec<usize> = points
        .iter()
        .enumerate()
        .filter(|(_, p)| model.distance_to((*p).clone()) <= threshold)
        .map(|(idx, _)| idx)
        .collect();

    Ok(RansacResult {
        model,
        inliers: final_inliers,
        iterations,
        converged,
    })
}

/// Robustly fits a 3D plane to `points` using RANSAC.
///
/// # Errors
///
/// Returns [`AlgorithmError::InvalidInput`] when fewer than three points are
/// supplied or any coordinate is non-finite.
///
/// # Behavior
///
/// Analogous to [`ransac_fit_line`] but samples three distinct points per
/// iteration and refines with a 3×3 total-least-squares fit (the plane normal is
/// the least-variance eigenvector of the covariance). `min_inlier_count` is
/// effectively at least three.
pub fn ransac_fit_plane(
    points: &[(f64, f64, f64)],
    options: &RansacOptions,
) -> Result<RansacResult<RansacPlaneModel>, AlgorithmError> {
    if points.len() < 3 {
        return Err(AlgorithmError::InvalidInput(format!(
            "ransac_fit_plane requires at least 3 points, got {}",
            points.len()
        )));
    }
    if !all_points_finite_3d(points) {
        return Err(AlgorithmError::InvalidInput(
            "ransac_fit_plane requires all point coordinates to be finite".to_string(),
        ));
    }

    let n = points.len();
    let threshold = options.inlier_threshold;
    let min_inliers = options.min_inlier_count.max(3);
    let mut state = options.seed;

    let mut best_model: Option<RansacPlaneModel> = None;
    let mut best_inlier_count = 0usize;
    let mut iterations = 0usize;
    let mut needed_iterations = options.max_iterations;

    while iterations < needed_iterations && iterations < options.max_iterations {
        iterations += 1;
        let (i, j, k) = sample_three_distinct(n, &mut state);
        let candidate = match RansacPlaneModel::from_three_points(points[i], points[j], points[k]) {
            Some(model) => model,
            None => continue,
        };

        let mut inlier_count = 0usize;
        for &p in points {
            if candidate.distance_to(p) <= threshold {
                inlier_count += 1;
            }
        }

        if inlier_count > best_inlier_count {
            best_inlier_count = inlier_count;
            best_model = Some(candidate);
            let inlier_ratio = inlier_count as f64 / n as f64;
            needed_iterations = adaptive_iteration_count(
                inlier_ratio,
                3,
                options.confidence,
                options.max_iterations,
            );
        }
    }

    let converged = best_inlier_count >= min_inliers;

    let sampled_model = best_model.unwrap_or_else(|| {
        RansacPlaneModel::from_three_points(points[0], points[1], points[2]).unwrap_or(
            RansacPlaneModel {
                a: 0.0,
                b: 0.0,
                c: 1.0,
                d: -points[0].2,
            },
        )
    });

    let inliers: Vec<usize> = points
        .iter()
        .enumerate()
        .filter(|(_, p)| sampled_model.distance_to(**p) <= threshold)
        .map(|(idx, _)| idx)
        .collect();

    let model = match least_squares_plane(points, &inliers) {
        Some(refined) => {
            let refit_inliers = points
                .iter()
                .filter(|p| refined.distance_to(**p) <= threshold)
                .count();
            if refit_inliers >= inliers.len() {
                refined
            } else {
                sampled_model
            }
        }
        None => sampled_model,
    };

    let final_inliers: Vec<usize> = points
        .iter()
        .enumerate()
        .filter(|(_, p)| model.distance_to(**p) <= threshold)
        .map(|(idx, _)| idx)
        .collect();

    Ok(RansacResult {
        model,
        inliers: final_inliers,
        iterations,
        converged,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lcg_is_deterministic_and_advances() {
        let mut s1 = 12345;
        let mut s2 = 12345;
        let a = lcg_next(&mut s1);
        let b = lcg_next(&mut s2);
        assert_eq!(a, b);
        let c = lcg_next(&mut s1);
        assert_ne!(a, c);
    }

    #[test]
    fn lcg_index_in_range() {
        let mut state = 7;
        for _ in 0..1000 {
            let idx = lcg_index(10, &mut state);
            assert!(idx < 10);
        }
    }

    #[test]
    fn sample_two_distinct_yields_distinct() {
        let mut state = 99;
        for _ in 0..1000 {
            let (i, j) = sample_two_distinct(5, &mut state);
            assert_ne!(i, j);
            assert!(i < 5 && j < 5);
        }
    }

    #[test]
    fn sample_three_distinct_yields_distinct() {
        let mut state = 99;
        for _ in 0..1000 {
            let (i, j, k) = sample_three_distinct(6, &mut state);
            assert_ne!(i, j);
            assert_ne!(i, k);
            assert_ne!(j, k);
            assert!(i < 6 && j < 6 && k < 6);
        }
    }

    #[test]
    fn adaptive_count_guards() {
        // inlier_ratio >= 1 ⇒ 1
        assert_eq!(adaptive_iteration_count(1.0, 2, 0.99, 1000), 1);
        assert_eq!(adaptive_iteration_count(1.5, 2, 0.99, 1000), 1);
        // inlier_ratio <= 0 ⇒ max
        assert_eq!(adaptive_iteration_count(0.0, 2, 0.99, 1000), 1000);
        assert_eq!(adaptive_iteration_count(-0.5, 2, 0.99, 1000), 1000);
        // non-finite ⇒ max
        assert_eq!(adaptive_iteration_count(f64::NAN, 2, 0.99, 1000), 1000);
        assert_eq!(adaptive_iteration_count(f64::INFINITY, 2, 0.99, 1000), 1000);
        // non-finite confidence ⇒ max
        assert_eq!(adaptive_iteration_count(0.5, 2, f64::NAN, 1000), 1000);
        // intermediate ratio ⇒ finite, clamped
        let mid = adaptive_iteration_count(0.5, 2, 0.99, 1000);
        assert!((1..=1000).contains(&mid));
    }

    #[test]
    fn line_from_coincident_points_is_none() {
        let p = Point::new(1.0, 1.0);
        assert!(RansacLineModel::from_two_points(p.clone(), p).is_none());
    }

    #[test]
    fn plane_from_collinear_points_is_none() {
        let p1 = (0.0, 0.0, 0.0);
        let p2 = (1.0, 1.0, 1.0);
        let p3 = (2.0, 2.0, 2.0);
        assert!(RansacPlaneModel::from_three_points(p1, p2, p3).is_none());
    }
}
