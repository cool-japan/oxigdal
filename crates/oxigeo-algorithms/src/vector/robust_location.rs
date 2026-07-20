//! Robust spatial location estimators.
//!
//! Provides the geometric median (L1 median in 2-D and 3-D) via the Weiszfeld
//! algorithm with Vardi-Zhang coincidence handling, the coordinate-wise L1
//! median, and the arithmetic spatial mean.
//!
//! # References
//!
//! * Weiszfeld, E. (1937). Sur le point pour lequel la somme des distances de
//!   n points donnés est minimum. *Tôhoku Math. J.*, 43, 355-386.
//! * Vardi, Y. & Zhang, C.-H. (2000). The multivariate L1-median and associated
//!   data depth. *PNAS*, 97(4), 1423-1426.

/// Convergence options for the Weiszfeld iterative algorithm.
#[derive(Debug, Clone)]
pub struct RobustLocationOptions {
    /// Maximum number of Weiszfeld iterations.  Default: 200.
    pub max_iter: usize,

    /// Convergence tolerance (L2 norm of the step vector). Default: 1e-10.
    pub tol: f64,

    /// Distance below which a data point is considered coincident with the
    /// current estimate (triggers a perturbation to avoid division by zero).
    /// Default: 1e-12.
    pub coincidence_eps: f64,
}

impl Default for RobustLocationOptions {
    fn default() -> Self {
        Self {
            max_iter: 200,
            tol: 1e-10,
            coincidence_eps: 1e-12,
        }
    }
}

impl RobustLocationOptions {
    /// Set the maximum number of Weiszfeld iterations.
    pub fn with_max_iter(mut self, v: usize) -> Self {
        self.max_iter = v;
        self
    }

    /// Set the convergence tolerance.
    pub fn with_tol(mut self, v: f64) -> Self {
        self.tol = v;
        self
    }

    /// Set the coincidence epsilon.
    pub fn with_coincidence_eps(mut self, v: f64) -> Self {
        self.coincidence_eps = v;
        self
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Compute the arithmetic mean of a slice of 2-D points.
///
/// Returns `None` if `points` is empty.
fn mean_2d(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    let n = points.len();
    if n == 0 {
        return None;
    }
    let mut sx = 0.0_f64;
    let mut sy = 0.0_f64;
    for &(x, y) in points {
        sx += x;
        sy += y;
    }
    let n_f = n as f64;
    Some((sx / n_f, sy / n_f))
}

/// Compute the arithmetic mean of a slice of 3-D points.
///
/// Returns `None` if `points` is empty.
fn mean_3d(points: &[(f64, f64, f64)]) -> Option<(f64, f64, f64)> {
    let n = points.len();
    if n == 0 {
        return None;
    }
    let mut sx = 0.0_f64;
    let mut sy = 0.0_f64;
    let mut sz = 0.0_f64;
    for &(x, y, z) in points {
        sx += x;
        sy += y;
        sz += z;
    }
    let n_f = n as f64;
    Some((sx / n_f, sy / n_f, sz / n_f))
}

/// One Weiszfeld step in 2-D with per-point weights.
///
/// Returns `(new_estimate, coincidence_detected)`.
fn weiszfeld_step_2d(
    cx: f64,
    cy: f64,
    points: &[(f64, f64)],
    weights: &[f64],
    coincidence_eps: f64,
) -> ((f64, f64), bool) {
    let mut num_x = 0.0_f64;
    let mut num_y = 0.0_f64;
    let mut denom = 0.0_f64;
    let mut coincident = false;

    for (i, &(px, py)) in points.iter().enumerate() {
        let w = weights[i];
        let dx = px - cx;
        let dy = py - cy;
        let dist = (dx * dx + dy * dy).sqrt();
        if dist < coincidence_eps {
            coincident = true;
            // Skip the coincident point — the Vardi-Zhang perturbation is
            // applied at the call site before the next iteration.
            continue;
        }
        let inv_d = w / dist;
        num_x += px * inv_d;
        num_y += py * inv_d;
        denom += inv_d;
    }

    if denom == 0.0 {
        // All points are coincident with the current estimate; return as-is.
        return ((cx, cy), true);
    }

    ((num_x / denom, num_y / denom), coincident)
}

/// One Weiszfeld step in 3-D (unit weight for every point).
///
/// Returns `(new_estimate, coincidence_detected)`.
fn weiszfeld_step_3d(
    cx: f64,
    cy: f64,
    cz: f64,
    points: &[(f64, f64, f64)],
    coincidence_eps: f64,
) -> ((f64, f64, f64), bool) {
    let mut num_x = 0.0_f64;
    let mut num_y = 0.0_f64;
    let mut num_z = 0.0_f64;
    let mut denom = 0.0_f64;
    let mut coincident = false;

    for &(px, py, pz) in points {
        let dx = px - cx;
        let dy = py - cy;
        let dz = pz - cz;
        let dist = (dx * dx + dy * dy + dz * dz).sqrt();
        if dist < coincidence_eps {
            coincident = true;
            continue;
        }
        let inv_d = 1.0 / dist;
        num_x += px * inv_d;
        num_y += py * inv_d;
        num_z += pz * inv_d;
        denom += inv_d;
    }

    if denom == 0.0 {
        return ((cx, cy, cz), true);
    }

    ((num_x / denom, num_y / denom, num_z / denom), coincident)
}

// ---------------------------------------------------------------------------
// Public API — 2-D
// ---------------------------------------------------------------------------

/// Compute the geometric median of a set of 2-D points using default options.
///
/// The geometric median minimises the sum of Euclidean distances to all input
/// points (L1 median in 2-D).  It is significantly more robust to outliers
/// than the arithmetic mean.
///
/// Returns `None` for an empty input.  For a single point, that point is
/// returned directly.  Convergence is guaranteed for non-coincident data; for
/// coincident data, a Vardi-Zhang perturbation is applied automatically.
pub fn geometric_median(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    geometric_median_with_options(points, &RobustLocationOptions::default())
}

/// Compute the geometric median with custom convergence options.
///
/// See [`geometric_median`] for full documentation.
pub fn geometric_median_with_options(
    points: &[(f64, f64)],
    options: &RobustLocationOptions,
) -> Option<(f64, f64)> {
    let n = points.len();
    match n {
        0 => return None,
        1 => return Some(points[0]),
        _ => {}
    }

    // Unit weights.
    let weights: Vec<f64> = vec![1.0; n];
    weighted_geometric_median(points, &weights, options)
}

/// Compute the weighted geometric median of a set of 2-D points.
///
/// Each point `points[i]` carries the non-negative weight `weights[i]`.  The
/// algorithm is equivalent to Weiszfeld's method but with per-point scaling of
/// the distance reciprocals.
///
/// Returns `None` if:
/// * `points` and `weights` have different lengths, or
/// * the combined slice is empty.
pub fn weighted_geometric_median(
    points: &[(f64, f64)],
    weights: &[f64],
    options: &RobustLocationOptions,
) -> Option<(f64, f64)> {
    let n = points.len();
    if n != weights.len() {
        return None;
    }
    match n {
        0 => return None,
        1 => return Some(points[0]),
        _ => {}
    }

    // Initial estimate: weighted arithmetic mean.
    let total_w: f64 = weights.iter().sum();
    let (mut cx, mut cy) = if total_w == 0.0 {
        mean_2d(points)?
    } else {
        let mut sx = 0.0_f64;
        let mut sy = 0.0_f64;
        for (i, &(px, py)) in points.iter().enumerate() {
            sx += weights[i] * px;
            sy += weights[i] * py;
        }
        (sx / total_w, sy / total_w)
    };

    // Weiszfeld iterations.
    for _ in 0..options.max_iter {
        let ((nx, ny), coincident) =
            weiszfeld_step_2d(cx, cy, points, weights, options.coincidence_eps);

        if coincident {
            // Vardi-Zhang practical approximation: perturb by coincidence_eps
            // along the x-axis to escape the singular point, then retry.
            let ((nx2, ny2), _) = weiszfeld_step_2d(
                cx + options.coincidence_eps,
                cy,
                points,
                weights,
                options.coincidence_eps,
            );
            let dx = nx2 - cx;
            let dy = ny2 - cy;
            let step = (dx * dx + dy * dy).sqrt();
            cx = nx2;
            cy = ny2;
            if step < options.tol {
                break;
            }
            continue;
        }

        let dx = nx - cx;
        let dy = ny - cy;
        let step = (dx * dx + dy * dy).sqrt();
        cx = nx;
        cy = ny;
        if step < options.tol {
            break;
        }
    }

    Some((cx, cy))
}

// ---------------------------------------------------------------------------
// Public API — 3-D
// ---------------------------------------------------------------------------

/// Compute the geometric median of a set of 3-D points.
///
/// Equivalent to [`geometric_median`] but operating in three dimensions.
/// Returns `None` for an empty input.
pub fn geometric_median_3d(
    points: &[(f64, f64, f64)],
    options: &RobustLocationOptions,
) -> Option<(f64, f64, f64)> {
    let n = points.len();
    match n {
        0 => return None,
        1 => return Some(points[0]),
        _ => {}
    }

    // Initial estimate: arithmetic mean.
    let (mut cx, mut cy, mut cz) = mean_3d(points)?;

    // Weiszfeld iterations.
    for _ in 0..options.max_iter {
        let ((nx, ny, nz), coincident) =
            weiszfeld_step_3d(cx, cy, cz, points, options.coincidence_eps);

        if coincident {
            // Perturb along x-axis.
            let ((nx2, ny2, nz2), _) = weiszfeld_step_3d(
                cx + options.coincidence_eps,
                cy,
                cz,
                points,
                options.coincidence_eps,
            );
            let dx = nx2 - cx;
            let dy = ny2 - cy;
            let dz = nz2 - cz;
            let step = (dx * dx + dy * dy + dz * dz).sqrt();
            cx = nx2;
            cy = ny2;
            cz = nz2;
            if step < options.tol {
                break;
            }
            continue;
        }

        let dx = nx - cx;
        let dy = ny - cy;
        let dz = nz - cz;
        let step = (dx * dx + dy * dy + dz * dz).sqrt();
        cx = nx;
        cy = ny;
        cz = nz;
        if step < options.tol {
            break;
        }
    }

    Some((cx, cy, cz))
}

// ---------------------------------------------------------------------------
// Public API — coordinate-wise L1 and arithmetic mean
// ---------------------------------------------------------------------------

/// Coordinate-wise L1 median (median-of-x, median-of-y).
///
/// This is **not** the true geometric median because it is computed
/// independently per coordinate axis and is therefore not rotation-invariant.
/// It is, however, extremely fast (O(n log n)) and provides a useful,
/// breakdown-point-50% robust location estimate.
///
/// For an odd number of points, the median is the middle sorted value.  For an
/// even number, the median is the arithmetic mean of the two middle values.
///
/// Returns `None` for an empty input.
pub fn l1_median(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    let n = points.len();
    if n == 0 {
        return None;
    }

    let mut xs: Vec<f64> = points.iter().map(|&(x, _)| x).collect();
    let mut ys: Vec<f64> = points.iter().map(|&(_, y)| y).collect();

    // Partial sort is sufficient, but a full sort keeps this straightforward
    // and is O(n log n) — acceptable for the sizes found in geospatial work.
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let median_x = coordinate_median(&xs);
    let median_y = coordinate_median(&ys);

    Some((median_x, median_y))
}

/// Return the median of a **sorted** slice of `f64` values.
///
/// For odd length: the middle element.
/// For even length: the average of the two middle elements.
///
/// # Panics
///
/// Panics if `sorted` is empty (callers must guard against this).
fn coordinate_median(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    debug_assert!(n > 0, "coordinate_median called on empty slice");
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    }
}

/// Arithmetic spatial mean (centroid) of a set of 2-D points.
///
/// Equivalent to the centre of mass when every point has equal unit mass.
/// Provided primarily to allow callers to compare the geometric median's
/// robustness against outliers with that of the arithmetic mean.
///
/// Returns `None` for an empty input.
pub fn spatial_mean(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    mean_2d(points)
}

// ---------------------------------------------------------------------------
// Unit tests (kept internal; the full integration test suite lives under
// `tests/robust_location_test.rs`).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_mean_2d_empty() {
        assert!(mean_2d(&[]).is_none());
    }

    #[test]
    fn unit_mean_2d_single() {
        let r = mean_2d(&[(3.0, 4.0)]).expect("non-empty slice should return Some");
        assert!((r.0 - 3.0).abs() < 1e-12 && (r.1 - 4.0).abs() < 1e-12);
    }

    #[test]
    fn unit_mean_2d_symmetric() {
        let pts = [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)];
        let r = mean_2d(&pts).expect("non-empty slice should return Some");
        assert!(r.0.abs() < 1e-12 && r.1.abs() < 1e-12);
    }

    #[test]
    fn unit_coordinate_median_odd() {
        let v = [1.0, 3.0, 5.0];
        assert!((coordinate_median(&v) - 3.0).abs() < 1e-12);
    }

    #[test]
    fn unit_coordinate_median_even() {
        let v = [1.0, 3.0];
        assert!((coordinate_median(&v) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn unit_geometric_median_options_builder() {
        let opts = RobustLocationOptions::default()
            .with_max_iter(50)
            .with_tol(1e-8)
            .with_coincidence_eps(1e-6);
        assert_eq!(opts.max_iter, 50);
        assert!((opts.tol - 1e-8).abs() < f64::EPSILON);
        assert!((opts.coincidence_eps - 1e-6).abs() < f64::EPSILON);
    }
}
