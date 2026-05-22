//! Kriging Interpolation
//!
//! Kriging is a geostatistical interpolation method that uses variogram models
//! to provide Best Linear Unbiased Predictions (BLUP).

use crate::error::{AnalyticsError, Result};
use scirs2_core::ndarray::{Array1, Array2, ArrayView1};

/// Kriging types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KrigingType {
    /// Ordinary Kriging (constant mean)
    Ordinary,
    /// Universal Kriging (trend surface)
    Universal,
}

/// Variogram models
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariogramModel {
    /// Spherical variogram
    Spherical,
    /// Exponential variogram
    Exponential,
    /// Gaussian variogram
    Gaussian,
    /// Linear variogram
    Linear,
}

/// Variogram parameters
#[derive(Debug, Clone, Copy)]
pub struct Variogram {
    /// Nugget effect
    pub nugget: f64,
    /// Sill (total variance)
    pub sill: f64,
    /// Range parameter
    pub range: f64,
    /// Model type
    pub model: VariogramModel,
}

impl Variogram {
    /// Create a new variogram
    pub fn new(model: VariogramModel, nugget: f64, sill: f64, range: f64) -> Self {
        Self {
            nugget,
            sill,
            range,
            model,
        }
    }

    /// Evaluate variogram at distance h
    pub fn evaluate(&self, h: f64) -> f64 {
        if h < f64::EPSILON {
            return 0.0;
        }

        let partial_sill = self.sill - self.nugget;

        match self.model {
            VariogramModel::Spherical => {
                if h >= self.range {
                    self.sill
                } else {
                    let h_r = h / self.range;
                    self.nugget + partial_sill * (1.5 * h_r - 0.5 * h_r.powi(3))
                }
            }
            VariogramModel::Exponential => {
                self.nugget + partial_sill * (1.0 - (-h / self.range).exp())
            }
            VariogramModel::Gaussian => {
                self.nugget + partial_sill * (1.0 - (-(h * h) / (self.range * self.range)).exp())
            }
            VariogramModel::Linear => {
                let slope = self.sill / self.range;
                self.nugget + slope * h.min(self.range)
            }
        }
    }
}

/// Kriging result
#[derive(Debug, Clone)]
pub struct KrigingResult {
    /// Interpolated values
    pub values: Array1<f64>,
    /// Prediction variances
    pub variances: Array1<f64>,
    /// Target coordinates
    pub coordinates: Array2<f64>,
}

/// Kriging interpolator
pub struct KrigingInterpolator {
    kriging_type: KrigingType,
    variogram: Variogram,
}

impl KrigingInterpolator {
    /// Create a new Kriging interpolator
    ///
    /// # Arguments
    /// * `kriging_type` - Type of kriging
    /// * `variogram` - Variogram model
    pub fn new(kriging_type: KrigingType, variogram: Variogram) -> Self {
        Self {
            kriging_type,
            variogram,
        }
    }

    /// Interpolate values at target locations
    ///
    /// # Arguments
    /// * `points` - Known point coordinates (n_points × n_dim)
    /// * `values` - Known values (n_points)
    /// * `targets` - Target coordinates (n_targets × n_dim)
    ///
    /// # Errors
    /// Returns error if interpolation fails
    pub fn interpolate(
        &self,
        points: &Array2<f64>,
        values: &ArrayView1<f64>,
        targets: &Array2<f64>,
    ) -> Result<KrigingResult> {
        let n_points = points.nrows();
        let n_targets = targets.nrows();

        if values.len() != n_points {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{}", n_points),
                format!("{}", values.len()),
            ));
        }

        if targets.ncols() != points.ncols() {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{}", points.ncols()),
                format!("{}", targets.ncols()),
            ));
        }

        // Build covariance matrix
        let cov_matrix = self.build_covariance_matrix(points)?;

        // Solve kriging system once for efficiency
        let weights_matrix = self.solve_kriging_system(&cov_matrix)?;

        let mut interpolated = Array1::zeros(n_targets);
        let mut variances = Array1::zeros(n_targets);

        for i in 0..n_targets {
            let target = targets.row(i);
            let (value, variance) =
                self.interpolate_point(&target, points, values, &weights_matrix)?;
            interpolated[i] = value;
            variances[i] = variance;
        }

        Ok(KrigingResult {
            values: interpolated,
            variances,
            coordinates: targets.clone(),
        })
    }

    /// Build covariance matrix from variogram
    fn build_covariance_matrix(&self, points: &Array2<f64>) -> Result<Array2<f64>> {
        let n = points.nrows();
        let size = match self.kriging_type {
            KrigingType::Ordinary => n + 1,  // Add Lagrange multiplier
            KrigingType::Universal => n + 4, // Add trend terms (constant + x + y + xy)
        };

        let mut matrix = Array2::zeros((size, size));

        // Fill in covariances
        for i in 0..n {
            for j in 0..n {
                let dist = self.calculate_distance(&points.row(i), &points.row(j))?;
                let covariance = self.variogram.sill - self.variogram.evaluate(dist);
                matrix[[i, j]] = covariance;
            }
        }

        // Add constraint equations
        match self.kriging_type {
            KrigingType::Ordinary => {
                // Sum of weights = 1
                for i in 0..n {
                    matrix[[i, n]] = 1.0;
                    matrix[[n, i]] = 1.0;
                }
            }
            KrigingType::Universal => {
                // Trend surface constraints
                for i in 0..n {
                    let x = points[[i, 0]];
                    let y = points[[i, 1]];
                    matrix[[i, n]] = 1.0;
                    matrix[[n, i]] = 1.0;
                    matrix[[i, n + 1]] = x;
                    matrix[[n + 1, i]] = x;
                    matrix[[i, n + 2]] = y;
                    matrix[[n + 2, i]] = y;
                    matrix[[i, n + 3]] = x * y;
                    matrix[[n + 3, i]] = x * y;
                }
            }
        }

        Ok(matrix)
    }

    /// Solve kriging system using matrix inversion
    fn solve_kriging_system(&self, cov_matrix: &Array2<f64>) -> Result<Array2<f64>> {
        // For simplicity, use Gaussian elimination
        // In production, would use proper linear algebra library
        self.matrix_inverse(cov_matrix)
    }

    /// Simple matrix inversion using Gauss-Jordan elimination
    fn matrix_inverse(&self, matrix: &Array2<f64>) -> Result<Array2<f64>> {
        let n = matrix.nrows();
        if n != matrix.ncols() {
            return Err(AnalyticsError::matrix_error("Matrix must be square"));
        }

        // Create augmented matrix [A | I]
        let mut aug = Array2::zeros((n, 2 * n));
        for i in 0..n {
            for j in 0..n {
                aug[[i, j]] = matrix[[i, j]];
            }
            aug[[i, n + i]] = 1.0;
        }

        // Gauss-Jordan elimination
        for i in 0..n {
            // Find pivot
            let mut max_row = i;
            let mut max_val = aug[[i, i]].abs();
            for k in (i + 1)..n {
                if aug[[k, i]].abs() > max_val {
                    max_val = aug[[k, i]].abs();
                    max_row = k;
                }
            }

            if max_val < f64::EPSILON {
                return Err(AnalyticsError::matrix_error("Matrix is singular"));
            }

            // Swap rows
            if max_row != i {
                for j in 0..(2 * n) {
                    let tmp = aug[[i, j]];
                    aug[[i, j]] = aug[[max_row, j]];
                    aug[[max_row, j]] = tmp;
                }
            }

            // Eliminate column
            let pivot = aug[[i, i]];
            for j in 0..(2 * n) {
                aug[[i, j]] /= pivot;
            }

            for k in 0..n {
                if k != i {
                    let factor = aug[[k, i]];
                    for j in 0..(2 * n) {
                        aug[[k, j]] -= factor * aug[[i, j]];
                    }
                }
            }
        }

        // Extract inverse matrix
        let mut inverse = Array2::zeros((n, n));
        for i in 0..n {
            for j in 0..n {
                inverse[[i, j]] = aug[[i, n + j]];
            }
        }

        Ok(inverse)
    }

    /// Interpolate at a single point
    fn interpolate_point(
        &self,
        target: &scirs2_core::ndarray::ArrayView1<f64>,
        points: &Array2<f64>,
        values: &ArrayView1<f64>,
        weights_matrix: &Array2<f64>,
    ) -> Result<(f64, f64)> {
        let n = points.nrows();

        // Build right-hand side vector
        let rhs_size = match self.kriging_type {
            KrigingType::Ordinary => n + 1,
            KrigingType::Universal => n + 4,
        };

        let mut rhs = Array1::zeros(rhs_size);

        // Fill in covariances to target
        for i in 0..n {
            let dist = self.calculate_distance(&points.row(i), target)?;
            rhs[i] = self.variogram.sill - self.variogram.evaluate(dist);
        }

        // Add constraints
        match self.kriging_type {
            KrigingType::Ordinary => {
                rhs[n] = 1.0;
            }
            KrigingType::Universal => {
                rhs[n] = 1.0;
                rhs[n + 1] = target[0];
                rhs[n + 2] = target[1];
                rhs[n + 3] = target[0] * target[1];
            }
        }

        // Solve for weights
        let mut weights: Array1<f64> = Array1::zeros(rhs_size);
        for i in 0..rhs_size {
            for j in 0..rhs_size {
                weights[i] += weights_matrix[[i, j]] * rhs[j];
            }
        }

        // Calculate interpolated value
        let mut value: f64 = 0.0;
        for i in 0..n {
            value += weights[i] * values[i];
        }

        // Calculate kriging variance
        let mut variance = self.variogram.sill;
        for i in 0..rhs_size {
            variance -= weights[i] * rhs[i];
        }

        Ok((value, variance.max(0.0)))
    }

    /// Calculate distance between two points
    fn calculate_distance(
        &self,
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
}

/// Semivariogram calculator
pub struct SemivariogramCalculator;

impl SemivariogramCalculator {
    /// Calculate experimental semivariogram
    ///
    /// # Arguments
    /// * `points` - Point coordinates
    /// * `values` - Values at points
    /// * `n_bins` - Number of distance bins
    ///
    /// # Errors
    /// Returns error if calculation fails
    pub fn calculate(
        points: &Array2<f64>,
        values: &ArrayView1<f64>,
        n_bins: usize,
    ) -> Result<(Array1<f64>, Array1<f64>)> {
        let n = points.nrows();
        if values.len() != n {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{}", n),
                format!("{}", values.len()),
            ));
        }

        // Calculate all pairwise distances and semivariances
        let mut pairs = Vec::new();
        for i in 0..n {
            for j in (i + 1)..n {
                let mut dist_sq = 0.0;
                for k in 0..points.ncols() {
                    let diff = points[[i, k]] - points[[j, k]];
                    dist_sq += diff * diff;
                }
                let dist = dist_sq.sqrt();
                let semivar = 0.5 * (values[i] - values[j]).powi(2);
                pairs.push((dist, semivar));
            }
        }

        if pairs.is_empty() {
            return Err(AnalyticsError::insufficient_data("Need at least 2 points"));
        }

        // Find max distance for binning
        let max_dist = pairs
            .iter()
            .map(|(d, _)| *d)
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .ok_or_else(|| AnalyticsError::insufficient_data("No valid distances"))?;

        let bin_width = max_dist / (n_bins as f64);

        // Bin semivariances
        let mut bin_sums = vec![0.0; n_bins];
        let mut bin_counts = vec![0usize; n_bins];

        for (dist, semivar) in pairs {
            let bin = ((dist / bin_width).floor() as usize).min(n_bins - 1);
            bin_sums[bin] += semivar;
            bin_counts[bin] += 1;
        }

        // Calculate average semivariance for each bin
        let mut distances = Vec::new();
        let mut semivariances = Vec::new();

        for i in 0..n_bins {
            if bin_counts[i] > 0 {
                distances.push((i as f64 + 0.5) * bin_width);
                semivariances.push(bin_sums[i] / (bin_counts[i] as f64));
            }
        }

        Ok((Array1::from_vec(distances), Array1::from_vec(semivariances)))
    }
}

// ------------------------------------------------------------------
// Universal Kriging with External Drift (Wackernagel 2003 §16,
// Cressie 1993 §3.4.2). Free-function API operating on shaped views;
// reuses the same Gauss-Jordan inversion style as KrigingInterpolator.
// ------------------------------------------------------------------

use scirs2_core::ndarray::ArrayView2;

/// Drift basis function for Universal Kriging with external drift.
///
/// Each variant contributes one or more columns to the design matrix `F`
/// (samples × drift). The composite drift is the concatenation of all
/// variants, in the order supplied to [`UniversalKrigingOptions`].
#[derive(Debug, Clone)]
pub enum DriftBasis {
    /// `f(x) = 1` — intercept only (1 column).
    Constant,
    /// `f(x) = (1, x, y)` — affine surface drift (3 columns).
    Linear,
    /// `f(x) = (1, x, y, x², xy, y²)` — second-order polynomial drift (6 columns).
    Quadratic,
    /// `f(x) = caller-supplied per-sample values` (1 column). The length of
    /// the vector must equal the number of samples.
    External(Vec<f64>),
}

/// Options for Universal Kriging with external drift.
///
/// The total drift column count `p` is the sum of each basis' contribution:
/// `Constant` = 1, `Linear` = 3, `Quadratic` = 6, `External(v)` = 1.
#[derive(Debug, Clone)]
pub struct UniversalKrigingOptions {
    /// Ordered list of drift bases composing the trend model.
    pub drift_bases: Vec<DriftBasis>,
    /// Variogram model used to populate `Γ` and `γ_0`.
    pub variogram: Variogram,
    /// Tikhonov regularisation added to the diagonal of `Γ`.
    pub regularization: f64,
}

impl Default for UniversalKrigingOptions {
    fn default() -> Self {
        Self {
            drift_bases: vec![DriftBasis::Constant],
            variogram: Variogram::new(VariogramModel::Spherical, 0.0, 1.0, 1.0),
            regularization: 1e-10,
        }
    }
}

/// Result of universal kriging prediction.
#[derive(Debug, Clone)]
pub struct UniversalKrigingResult {
    /// Predicted values at each query point (length `m`).
    pub predicted: Vec<f64>,
    /// Kriging variance at each query point (length `m`, clamped to ≥ 0).
    pub variance: Vec<f64>,
    /// Drift coefficients (Lagrange multipliers) from the final query
    /// point's solve (length `p`).
    pub drift_coefficients: Vec<f64>,
}

/// Number of drift columns contributed by a single basis.
fn drift_basis_columns(basis: &DriftBasis) -> usize {
    match basis {
        DriftBasis::Constant => 1,
        DriftBasis::Linear => 3,
        DriftBasis::Quadratic => 6,
        DriftBasis::External(_) => 1,
    }
}

/// Fill row `i` of `F` for sample at `(x, y)` using `bases`.
fn fill_drift_row(bases: &[DriftBasis], sample_index: usize, x: f64, y: f64, row: &mut [f64]) {
    let mut col = 0usize;
    for basis in bases {
        match basis {
            DriftBasis::Constant => {
                row[col] = 1.0;
                col += 1;
            }
            DriftBasis::Linear => {
                row[col] = 1.0;
                row[col + 1] = x;
                row[col + 2] = y;
                col += 3;
            }
            DriftBasis::Quadratic => {
                row[col] = 1.0;
                row[col + 1] = x;
                row[col + 2] = y;
                row[col + 3] = x * x;
                row[col + 4] = x * y;
                row[col + 5] = y * y;
                col += 6;
            }
            DriftBasis::External(values) => {
                row[col] = values[sample_index];
                col += 1;
            }
        }
    }
}

/// Fill `f_0` for query point at `(x, y)` using `bases` and optional
/// per-query external drift row.
fn fill_query_drift_row(
    bases: &[DriftBasis],
    x: f64,
    y: f64,
    external_row: Option<&[f64]>,
    f0: &mut [f64],
) -> Result<()> {
    let mut col = 0usize;
    let mut external_used = 0usize;
    for basis in bases {
        match basis {
            DriftBasis::Constant => {
                f0[col] = 1.0;
                col += 1;
            }
            DriftBasis::Linear => {
                f0[col] = 1.0;
                f0[col + 1] = x;
                f0[col + 2] = y;
                col += 3;
            }
            DriftBasis::Quadratic => {
                f0[col] = 1.0;
                f0[col + 1] = x;
                f0[col + 2] = y;
                f0[col + 3] = x * x;
                f0[col + 4] = x * y;
                f0[col + 5] = y * y;
                col += 6;
            }
            DriftBasis::External(_) => {
                let row = external_row.ok_or_else(|| {
                    AnalyticsError::invalid_input(
                        "query_external_drift required for DriftBasis::External",
                    )
                })?;
                if external_used >= row.len() {
                    return Err(AnalyticsError::dimension_mismatch(
                        format!(">= {}", external_used + 1),
                        format!("{}", row.len()),
                    ));
                }
                f0[col] = row[external_used];
                external_used += 1;
                col += 1;
            }
        }
    }
    Ok(())
}

/// Squared Euclidean distance between two `(x, y)` points.
fn squared_distance_2d(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = ax - bx;
    let dy = ay - by;
    dx * dx + dy * dy
}

/// Gauss-Jordan inversion mirroring the style used by
/// `KrigingInterpolator::matrix_inverse`. Returns `None` if rank-deficient.
fn gauss_jordan_invert_uked(matrix: &Array2<f64>) -> Option<Array2<f64>> {
    let n = matrix.nrows();
    if n != matrix.ncols() || n == 0 {
        return None;
    }

    let mut aug = Array2::<f64>::zeros((n, 2 * n));
    for i in 0..n {
        for j in 0..n {
            aug[[i, j]] = matrix[[i, j]];
        }
        aug[[i, n + i]] = 1.0;
    }

    for i in 0..n {
        let mut max_row = i;
        let mut max_val = aug[[i, i]].abs();
        for k in (i + 1)..n {
            let v = aug[[k, i]].abs();
            if v > max_val {
                max_val = v;
                max_row = k;
            }
        }

        if max_val < f64::EPSILON {
            return None;
        }

        if max_row != i {
            for j in 0..(2 * n) {
                let tmp = aug[[i, j]];
                aug[[i, j]] = aug[[max_row, j]];
                aug[[max_row, j]] = tmp;
            }
        }

        let pivot = aug[[i, i]];
        for j in 0..(2 * n) {
            aug[[i, j]] /= pivot;
        }

        for k in 0..n {
            if k != i {
                let factor = aug[[k, i]];
                if factor != 0.0 {
                    for j in 0..(2 * n) {
                        aug[[k, j]] -= factor * aug[[i, j]];
                    }
                }
            }
        }
    }

    let mut inverse = Array2::<f64>::zeros((n, n));
    for i in 0..n {
        for j in 0..n {
            inverse[[i, j]] = aug[[i, n + j]];
        }
    }
    Some(inverse)
}

/// Fit and predict using Universal Kriging with external drift.
///
/// Solves the augmented system
///
/// ```text
///     | Γ   F | | λ |   | γ_0 |
///     | Fᵀ  0 | | μ | = | f_0 |
/// ```
///
/// once per query point by inverting the shared `(n+p) × (n+p)` design
/// matrix and multiplying by each RHS.
///
/// # Arguments
/// * `coords` — sample coordinates of shape `(n, 2)`.
/// * `values` — sample observations of length `n`.
/// * `options` — variogram, regularisation, and drift bases.
/// * `query_points` — prediction coordinates of shape `(m, 2)`.
/// * `query_external_drift` — optional per-query external drift values of
///   shape `(m, k_ext)`; required iff any [`DriftBasis::External`] is
///   present. `k_ext` must equal the count of `External` bases.
///
/// # Errors
/// * Mismatched array dimensions.
/// * `DriftBasis::External(v)` length differing from sample count.
/// * Singular augmented matrix (insufficient samples vs. drift columns).
pub fn universal_kriging_fit(
    coords: ArrayView2<f64>,
    values: ArrayView1<f64>,
    options: &UniversalKrigingOptions,
    query_points: ArrayView2<f64>,
    query_external_drift: Option<ArrayView2<f64>>,
) -> Result<UniversalKrigingResult> {
    let n = values.len();
    if coords.nrows() != n {
        return Err(AnalyticsError::dimension_mismatch(
            format!("{}", n),
            format!("{}", coords.nrows()),
        ));
    }
    if coords.ncols() != 2 {
        return Err(AnalyticsError::dimension_mismatch(
            "2",
            format!("{}", coords.ncols()),
        ));
    }
    if query_points.ncols() != 2 {
        return Err(AnalyticsError::dimension_mismatch(
            "2",
            format!("{}", query_points.ncols()),
        ));
    }
    if options.drift_bases.is_empty() {
        return Err(AnalyticsError::invalid_input(
            "UniversalKrigingOptions.drift_bases must not be empty",
        ));
    }
    if !options.regularization.is_finite() || options.regularization < 0.0 {
        return Err(AnalyticsError::invalid_parameter(
            "regularization",
            "must be a non-negative finite number",
        ));
    }

    // Validate every External basis has length n and count them.
    let mut external_basis_count = 0usize;
    for basis in &options.drift_bases {
        if let DriftBasis::External(v) = basis {
            if v.len() != n {
                return Err(AnalyticsError::dimension_mismatch(
                    format!("{}", n),
                    format!("{}", v.len()),
                ));
            }
            external_basis_count += 1;
        }
    }

    // Compute total drift columns p.
    let p: usize = options.drift_bases.iter().map(drift_basis_columns).sum();
    if p == 0 {
        return Err(AnalyticsError::invalid_input(
            "drift bases must contribute at least one column",
        ));
    }
    if n < p {
        return Err(AnalyticsError::insufficient_data(format!(
            "universal kriging needs at least p={p} samples, got n={n}"
        )));
    }

    // Validate query_external_drift shape when External is present.
    let m = query_points.nrows();
    if external_basis_count > 0 {
        let qed = query_external_drift.ok_or_else(|| {
            AnalyticsError::invalid_input(
                "query_external_drift required when DriftBasis::External is used",
            )
        })?;
        if qed.nrows() != m {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{}", m),
                format!("{}", qed.nrows()),
            ));
        }
        if qed.ncols() != external_basis_count {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{}", external_basis_count),
                format!("{}", qed.ncols()),
            ));
        }
    }

    // Build Γ (n × n) and F (n × p).
    let mut gamma = Array2::<f64>::zeros((n, n));
    for i in 0..n {
        let xi = coords[[i, 0]];
        let yi = coords[[i, 1]];
        for j in 0..n {
            if i == j {
                gamma[[i, j]] = 0.0;
            } else {
                let xj = coords[[j, 0]];
                let yj = coords[[j, 1]];
                let h = squared_distance_2d(xi, yi, xj, yj).sqrt();
                gamma[[i, j]] = options.variogram.evaluate(h);
            }
        }
    }

    let mut f_matrix = Array2::<f64>::zeros((n, p));
    for i in 0..n {
        let xi = coords[[i, 0]];
        let yi = coords[[i, 1]];
        let mut row_buf = vec![0.0f64; p];
        fill_drift_row(&options.drift_bases, i, xi, yi, &mut row_buf);
        for col in 0..p {
            f_matrix[[i, col]] = row_buf[col];
        }
    }

    // Assemble M = [[Γ + λI, F], [Fᵀ, 0]] of size (n+p) × (n+p).
    let size = n + p;
    let mut m_design = Array2::<f64>::zeros((size, size));
    for i in 0..n {
        for j in 0..n {
            m_design[[i, j]] = gamma[[i, j]];
        }
        m_design[[i, i]] += options.regularization;
    }
    for i in 0..n {
        for j in 0..p {
            m_design[[i, n + j]] = f_matrix[[i, j]];
            m_design[[n + j, i]] = f_matrix[[i, j]];
        }
    }
    // The lower-right p × p block is zero by initialisation.

    let m_inv = gauss_jordan_invert_uked(&m_design).ok_or_else(|| {
        AnalyticsError::matrix_error("UKED design matrix singular (rank-deficient solve)")
    })?;

    let mut predicted = Vec::with_capacity(m);
    let mut variance = Vec::with_capacity(m);
    let mut drift_coefficients = vec![0.0f64; p];

    for q in 0..m {
        let qx = query_points[[q, 0]];
        let qy = query_points[[q, 1]];

        // Build RHS b = [γ_0; f_0].
        let mut b = vec![0.0f64; size];
        for i in 0..n {
            let xi = coords[[i, 0]];
            let yi = coords[[i, 1]];
            let h = squared_distance_2d(xi, yi, qx, qy).sqrt();
            b[i] = options.variogram.evaluate(h);
        }
        let ext_row_owned: Option<Vec<f64>> = if external_basis_count > 0 {
            let qed = query_external_drift.ok_or_else(|| {
                AnalyticsError::invalid_input(
                    "query_external_drift required when DriftBasis::External is used",
                )
            })?;
            let mut row = Vec::with_capacity(external_basis_count);
            for k in 0..external_basis_count {
                row.push(qed[[q, k]]);
            }
            Some(row)
        } else {
            None
        };
        let mut f0 = vec![0.0f64; p];
        fill_query_drift_row(
            &options.drift_bases,
            qx,
            qy,
            ext_row_owned.as_deref(),
            &mut f0,
        )?;
        b[n..n + p].copy_from_slice(&f0[..p]);

        // z = M⁻¹ b.
        let mut z = vec![0.0f64; size];
        for i in 0..size {
            let mut acc = 0.0f64;
            for j in 0..size {
                acc += m_inv[[i, j]] * b[j];
            }
            z[i] = acc;
        }

        // Prediction = λᵀ values.
        let mut value = 0.0f64;
        for i in 0..n {
            value += z[i] * values[i];
        }
        predicted.push(value);

        // Variance = λᵀ γ_0 + μᵀ f_0.
        let mut var_acc = 0.0f64;
        for i in 0..n {
            var_acc += z[i] * b[i];
        }
        for j in 0..p {
            var_acc += z[n + j] * f0[j];
        }
        variance.push(var_acc.max(0.0));

        // Drift coefficients = μ (from last query's solve).
        drift_coefficients[..p].copy_from_slice(&z[n..n + p]);
    }

    Ok(UniversalKrigingResult {
        predicted,
        variance,
        drift_coefficients,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_variogram_spherical() {
        let var = Variogram::new(VariogramModel::Spherical, 0.1, 1.0, 10.0);

        assert_abs_diff_eq!(var.evaluate(0.0), 0.0, epsilon = 1e-10);
        assert_abs_diff_eq!(var.evaluate(10.0), 1.0, epsilon = 1e-10);
        assert_abs_diff_eq!(var.evaluate(20.0), 1.0, epsilon = 1e-10);
    }

    #[test]
    fn test_kriging_simple() {
        let points = array![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
        let values = array![1.0, 2.0, 3.0, 4.0];
        let targets = array![[0.5, 0.5]];

        let var = Variogram::new(VariogramModel::Spherical, 0.0, 1.0, 2.0);
        let interpolator = KrigingInterpolator::new(KrigingType::Ordinary, var);

        let result = interpolator
            .interpolate(&points, &values.view(), &targets)
            .expect("Kriging interpolation should succeed for valid data");

        assert_eq!(result.values.len(), 1);
        assert_eq!(result.variances.len(), 1);
        assert!(result.values[0] > 2.0 && result.values[0] < 3.0);
    }

    #[test]
    fn test_semivariogram_calculation() {
        let points = array![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let values = array![1.0, 2.0, 3.0];

        let (distances, semivariances) =
            SemivariogramCalculator::calculate(&points, &values.view(), 3)
                .expect("Semivariogram calculation should succeed");

        assert!(!distances.is_empty());
        assert_eq!(distances.len(), semivariances.len());
    }
}
