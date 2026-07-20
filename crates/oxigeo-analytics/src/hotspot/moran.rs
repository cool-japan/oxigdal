//! Moran's I Spatial Autocorrelation
//!
//! Global and local Moran's I statistics for detecting spatial clustering.

use crate::error::{AnalyticsError, Result};
use crate::hotspot::SpatialWeights;
use scirs2_core::ndarray::{Array1, ArrayView1};

/// Global Moran's I result
#[derive(Debug, Clone)]
pub struct MoransIResult {
    /// Moran's I statistic
    pub i_statistic: f64,
    /// Expected value under null hypothesis
    pub expected_i: f64,
    /// Variance of I
    pub variance_i: f64,
    /// Z-score
    pub z_score: f64,
    /// P-value (two-tailed)
    pub p_value: f64,
    /// Whether result is statistically significant
    pub significant: bool,
    /// Confidence level
    pub confidence: f64,
}

/// Local Moran's I result
#[derive(Debug, Clone)]
pub struct LocalMoransIResult {
    /// Local I statistics for each location
    pub local_i: Array1<f64>,
    /// Z-scores for each location
    pub z_scores: Array1<f64>,
    /// P-values for each location
    pub p_values: Array1<f64>,
    /// LISA classifications
    pub classifications: Array1<LisaClass>,
    /// Confidence level
    pub confidence: f64,
}

/// LISA (Local Indicators of Spatial Association) classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LisaClass {
    /// High-High cluster
    HH,
    /// Low-Low cluster
    LL,
    /// High-Low outlier
    HL,
    /// Low-High outlier
    LH,
    /// Not significant
    NotSignificant,
}

/// Global Moran's I calculator
pub struct MoransI {
    confidence: f64,
}

impl MoransI {
    /// Create a new Moran's I calculator
    ///
    /// # Arguments
    /// * `confidence` - Significance level (e.g., 0.05)
    pub fn new(confidence: f64) -> Self {
        Self { confidence }
    }

    /// Calculate global Moran's I
    ///
    /// # Arguments
    /// * `values` - Values for each location
    /// * `weights` - Spatial weights matrix (should be row-standardized)
    ///
    /// # Errors
    /// Returns error if computation fails
    pub fn calculate(
        &self,
        values: &ArrayView1<f64>,
        weights: &SpatialWeights,
    ) -> Result<MoransIResult> {
        let n = values.len();
        if n != weights.weights.nrows() {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{}", n),
                format!("{}", weights.weights.nrows()),
            ));
        }

        if n < 3 {
            return Err(AnalyticsError::insufficient_data(
                "Need at least 3 observations for Moran's I",
            ));
        }

        // Calculate mean and deviations
        let mean = values.sum() / (n as f64);
        let deviations: Vec<f64> = values.iter().map(|&x| x - mean).collect();

        // Calculate sum of squared deviations
        let s2 = deviations.iter().map(|&d| d * d).sum::<f64>() / (n as f64);

        if s2 < f64::EPSILON {
            return Err(AnalyticsError::numerical_instability(
                "Variance is too small",
            ));
        }

        // Calculate spatial lag
        let spatial_lag = weights.spatial_lag(values)?;

        // Calculate numerator (sum of cross-products)
        let mut numerator = 0.0;
        for i in 0..n {
            numerator += deviations[i] * (spatial_lag[i] - mean);
        }

        // Calculate sum of weights
        let s0: f64 = weights.weights.iter().sum();

        if s0 < f64::EPSILON {
            return Err(AnalyticsError::numerical_instability(
                "Sum of weights is too small",
            ));
        }

        // Calculate Moran's I
        let i_statistic =
            (n as f64 / s0) * (numerator / deviations.iter().map(|&d| d * d).sum::<f64>());

        // Calculate expected value and variance
        let expected_i = -1.0 / ((n - 1) as f64);

        let s1 = self.calculate_s1(&weights.weights);
        let s2_stat = self.calculate_s2(&weights.weights, n);

        let n_f64 = n as f64;
        let n2 = n_f64 * n_f64;
        let s0_sq = s0 * s0;

        // Var(I) closed forms (Cliff & Ord 1981), matching the formulas used
        // by GeoDa/PySAL's `esda.Moran`. The randomization-assumption
        // formula (which does not require the values to be normally
        // distributed) is preferred and uses the kurtosis coefficient
        // `b2 = m4 / m2^2` of the deviations; its denominator vanishes at
        // n == 3, so that boundary case falls back to the normality
        // formula.
        let variance_i = if n >= 4 {
            let b2 = self.calculate_kurtosis(&deviations, n);
            let numerator = n_f64 * ((n2 - 3.0 * n_f64 + 3.0) * s1 - n_f64 * s2_stat + 3.0 * s0_sq)
                - b2 * ((n2 - n_f64) * s1 - 2.0 * n_f64 * s2_stat + 6.0 * s0_sq);
            let denominator = (n_f64 - 1.0) * (n_f64 - 2.0) * (n_f64 - 3.0) * s0_sq;
            numerator / denominator - expected_i * expected_i
        } else {
            let numerator = n2 * s1 - n_f64 * s2_stat + 3.0 * s0_sq;
            let denominator = (n2 - 1.0) * s0_sq;
            numerator / denominator - expected_i * expected_i
        };

        // Calculate z-score and p-value
        let z_score = if variance_i > f64::EPSILON {
            (i_statistic - expected_i) / variance_i.sqrt()
        } else {
            0.0
        };

        let p_value = 2.0 * (1.0 - standard_normal_cdf(z_score.abs()));
        let significant = p_value < self.confidence;

        Ok(MoransIResult {
            i_statistic,
            expected_i,
            variance_i,
            z_score,
            p_value,
            significant,
            confidence: self.confidence,
        })
    }

    fn calculate_s1(&self, weights: &scirs2_core::ndarray::Array2<f64>) -> f64 {
        let n = weights.nrows();
        let mut s1 = 0.0;
        for i in 0..n {
            for j in 0..n {
                let w_ij = weights[[i, j]];
                let w_ji = weights[[j, i]];
                s1 += (w_ij + w_ji) * (w_ij + w_ji);
            }
        }
        s1 / 2.0
    }

    fn calculate_s2(&self, weights: &scirs2_core::ndarray::Array2<f64>, n: usize) -> f64 {
        let mut s2 = 0.0;
        for i in 0..n {
            let mut sum_i = 0.0;
            let mut sum_j = 0.0;
            for j in 0..n {
                sum_i += weights[[i, j]];
                sum_j += weights[[j, i]];
            }
            s2 += (sum_i + sum_j) * (sum_i + sum_j);
        }
        s2
    }

    /// Kurtosis coefficient `b2 = m4 / m2^2` of the deviations, where
    /// `m2`/`m4` are the (biased, population) second and fourth central
    /// moments. Used by the randomization-assumption variance formula for
    /// Moran's I (Cliff & Ord 1981).
    fn calculate_kurtosis(&self, deviations: &[f64], n: usize) -> f64 {
        let n_f64 = n as f64;
        let m2 = deviations.iter().map(|&d| d * d).sum::<f64>() / n_f64;
        let m4 = deviations.iter().map(|&d| d.powi(4)).sum::<f64>() / n_f64;
        m4 / (m2 * m2)
    }
}

/// Local Moran's I calculator (LISA)
pub struct LocalMoransI {
    confidence: f64,
}

impl LocalMoransI {
    /// Create a new Local Moran's I calculator
    ///
    /// # Arguments
    /// * `confidence` - Significance level (e.g., 0.05)
    pub fn new(confidence: f64) -> Self {
        Self { confidence }
    }

    /// Calculate local Moran's I for all locations using the Anselin (1995)
    /// analytical randomization variance.
    ///
    /// For each location `i`, `z_scores[i]` and `p_values[i]` are derived
    /// from the closed-form randomization variance `Var(I_i)` (Anselin
    /// 1995, eq. 12), which depends on the local spatial weights (`Σw_ij`,
    /// `Σw_ij²`) and the leave-one-out kurtosis coefficient of the
    /// deviations. This gives an approximate analytical p-value; for exact
    /// inference (recommended whenever performance allows), prefer
    /// [`Self::calculate_with_permutations`], which computes an empirical
    /// null distribution via conditional permutation and does not rely on
    /// this normal-theory approximation.
    ///
    /// # Arguments
    /// * `values` - Values for each location
    /// * `weights` - Spatial weights matrix
    ///
    /// # Errors
    /// Returns error if computation fails
    pub fn calculate(
        &self,
        values: &ArrayView1<f64>,
        weights: &SpatialWeights,
    ) -> Result<LocalMoransIResult> {
        let n = values.len();
        if n != weights.weights.nrows() {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{}", n),
                format!("{}", weights.weights.nrows()),
            ));
        }

        if n < 3 {
            return Err(AnalyticsError::insufficient_data(
                "Need at least 3 observations for Local Moran's I",
            ));
        }

        // Calculate mean and deviations
        let mean = values.sum() / (n as f64);
        let deviations: Vec<f64> = values.iter().map(|&x| x - mean).collect();

        // Calculate variance
        let s2 = deviations.iter().map(|&d| d * d).sum::<f64>() / (n as f64);

        if s2 < f64::EPSILON {
            return Err(AnalyticsError::numerical_instability(
                "Variance is too small",
            ));
        }

        let mut local_i = Array1::zeros(n);
        let mut z_scores = Array1::zeros(n);
        let mut p_values = Array1::zeros(n);
        let mut classifications = Array1::from_elem(n, LisaClass::NotSignificant);

        // Calculate spatial lag
        let spatial_lag = weights.spatial_lag(values)?;

        let n_f64 = n as f64;
        let n1 = n_f64 - 1.0;
        let n2 = n_f64 - 2.0;
        let deviations_sq: Vec<f64> = deviations.iter().map(|&d| d * d).collect();
        let sum_dev2: f64 = deviations_sq.iter().sum();
        let sum_dev4: f64 = deviations.iter().map(|&d| d.powi(4)).sum();

        // Calculate Local I for each location
        for i in 0..n {
            let zi = deviations[i] / s2.sqrt();
            let mut sum_wij_zj = 0.0;
            let mut wi_dot = 0.0;
            let mut wi_dot2 = 0.0;

            for j in 0..n {
                if i != j {
                    let w_ij = weights.weights[[i, j]];
                    let zj = deviations[j] / s2.sqrt();
                    sum_wij_zj += w_ij * zj;
                    wi_dot += w_ij;
                    wi_dot2 += w_ij * w_ij;
                }
            }

            local_i[i] = zi * sum_wij_zj;

            // Anselin (1995) eq. 11: E[I_i] under randomization.
            let expected_i = -wi_dot / n1;

            // Leave-one-out kurtosis coefficient b2_(i): the fourth/second
            // central moments of the n-1 deviations excluding location i
            // (Anselin 1995, eq. 12-13).
            let sum_dev2_loo = sum_dev2 - deviations_sq[i];
            let sum_dev4_loo = sum_dev4 - deviations_sq[i] * deviations_sq[i];

            z_scores[i] = if sum_dev2_loo > f64::EPSILON {
                let m2_loo = sum_dev2_loo / n1;
                let m4_loo = sum_dev4_loo / n1;
                let b2 = m4_loo / (m2_loo * m2_loo);

                let variance_i = (wi_dot2 * (n_f64 - b2)) / n1
                    + ((wi_dot * wi_dot - wi_dot2) * (2.0 * b2 - n_f64)) / (n1 * n2)
                    - (wi_dot * wi_dot) / (n1 * n1);

                if variance_i > f64::EPSILON {
                    (local_i[i] - expected_i) / variance_i.sqrt()
                } else {
                    0.0
                }
            } else {
                0.0
            };

            // Calculate p-value
            p_values[i] = 2.0 * (1.0 - standard_normal_cdf(z_scores[i].abs()));

            // Classify LISA
            if p_values[i] < self.confidence {
                let lag_mean = spatial_lag[i];
                classifications[i] = match (values[i] > mean, lag_mean > mean) {
                    (true, true) => LisaClass::HH,   // High-High
                    (false, false) => LisaClass::LL, // Low-Low
                    (true, false) => LisaClass::HL,  // High-Low
                    (false, true) => LisaClass::LH,  // Low-High
                };
            }
        }

        Ok(LocalMoransIResult {
            local_i,
            z_scores,
            p_values,
            classifications,
            confidence: self.confidence,
        })
    }
}

impl LocalMoransI {
    /// Compute local Moran's I with conditional permutation inference (Anselin 1995).
    ///
    /// For each location i, holds z_i fixed while randomly permuting the other n-1
    /// standardized values, computing I_i^(perm) each time. Pseudo p-value =
    /// (C + 1) / (M + 1) where C = #{|I_i^(perm)| >= |I_i^(obs)|}.
    ///
    /// Z-score is derived from the permutation null distribution:
    /// z_i = (I_i^(obs) - mean_perm) / sd_perm; guarded: sd_perm < 1e-12 -> z=0, p=1.
    ///
    /// # Parameters
    /// - `n_permutations`: number of random permutations per location (suggested: 999)
    /// - `seed`: LCG seed for deterministic reproducibility
    ///
    /// # Errors
    /// Returns error if n < 3, n_permutations == 0, or dimensions mismatch.
    pub fn calculate_with_permutations(
        &self,
        values: &ArrayView1<f64>,
        weights: &SpatialWeights,
        n_permutations: usize,
        seed: u64,
    ) -> Result<LocalMoransIResult> {
        let n = values.len();

        if n != weights.weights.nrows() {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{}", n),
                format!("{}", weights.weights.nrows()),
            ));
        }

        if n < 3 {
            return Err(AnalyticsError::insufficient_data(
                "Need at least 3 observations for Local Moran's I with permutations",
            ));
        }

        if n_permutations == 0 {
            return Err(AnalyticsError::insufficient_data(
                "n_permutations must be at least 1",
            ));
        }

        // Step 1: Standardize values z = (x - mean) / std
        let mean = values.sum() / (n as f64);
        let variance = values.iter().map(|&x| (x - mean) * (x - mean)).sum::<f64>() / (n as f64);

        if variance < f64::EPSILON {
            return Err(AnalyticsError::numerical_instability(
                "Variance is too small for permutation inference",
            ));
        }

        let std_dev = variance.sqrt();
        let z_vals: Vec<f64> = values.iter().map(|&x| (x - mean) / std_dev).collect();

        // Step 2: Compute observed local I_i^(obs) = z[i] * sum_j(w_ij * z[j])
        let mut local_i_obs: Vec<f64> = vec![0.0; n];
        for i in 0..n {
            let mut sum_wij_zj = 0.0;
            for j in 0..n {
                if i != j {
                    sum_wij_zj += weights.weights[[i, j]] * z_vals[j];
                }
            }
            local_i_obs[i] = z_vals[i] * sum_wij_zj;
        }

        // Compute spatial lag for quadrant classification (using raw values as in calculate)
        let spatial_lag = weights.spatial_lag(values)?;

        let mut z_scores_out = Array1::zeros(n);
        let mut p_values_out = Array1::zeros(n);
        let mut classifications = Array1::from_elem(n, LisaClass::NotSignificant);

        // Step 3: Per-location conditional permutation
        // We use independent LCG states per location seeded from seed + i
        // to maintain reproducibility without correlating adjacent runs.
        for i in 0..n {
            // Build the "other" buffer: z values for all j != i
            let others: Vec<f64> = (0..n).filter(|&j| j != i).map(|j| z_vals[j]).collect();
            // Row i's weights for positions in `others` (j != i in order)
            let row_weights: Vec<f64> = (0..n)
                .filter(|&j| j != i)
                .map(|j| weights.weights[[i, j]])
                .collect();

            let obs_i = local_i_obs[i];
            let obs_abs = obs_i.abs();

            // LCG state seeded per location for independence + reproducibility
            let mut rng_state: u64 = seed.wrapping_add(i as u64).wrapping_add(1);

            let mut perm_buf = others.clone();
            let mut count_extreme: usize = 0;
            let mut perm_sum = 0.0;
            let mut perm_sum_sq = 0.0;

            for _ in 0..n_permutations {
                permutation_fisher_yates(&mut perm_buf, &mut rng_state);
                // Compute I_i^(perm) = z[i] * sum_j w_ij * permuted_z[j]
                let lag_perm: f64 = row_weights
                    .iter()
                    .zip(perm_buf.iter())
                    .map(|(&w, &z)| w * z)
                    .sum();
                let i_perm = z_vals[i] * lag_perm;

                perm_sum += i_perm;
                perm_sum_sq += i_perm * i_perm;

                if i_perm.abs() >= obs_abs {
                    count_extreme += 1;
                }
            }

            // Pseudo p-value per Anselin 1995: (C + 1) / (M + 1)
            let pseudo_pvalue = (count_extreme as f64 + 1.0) / (n_permutations as f64 + 1.0);
            p_values_out[i] = pseudo_pvalue;

            // Z-score from permutation null distribution
            let perm_mean = perm_sum / (n_permutations as f64);
            let perm_var = (perm_sum_sq / (n_permutations as f64)) - perm_mean * perm_mean;
            let perm_sd = perm_var.max(0.0).sqrt();

            z_scores_out[i] = if perm_sd < 1e-12 {
                0.0
            } else {
                (obs_i - perm_mean) / perm_sd
            };

            // Quadrant classification gated by pseudo p-value < confidence
            if pseudo_pvalue < self.confidence {
                let lag_val = spatial_lag[i];
                classifications[i] = match (values[i] > mean, lag_val > mean) {
                    (true, true) => LisaClass::HH,
                    (false, false) => LisaClass::LL,
                    (true, false) => LisaClass::HL,
                    (false, true) => LisaClass::LH,
                };
            }
        }

        let local_i_array = Array1::from_vec(local_i_obs);

        Ok(LocalMoransIResult {
            local_i: local_i_array,
            z_scores: z_scores_out,
            p_values: p_values_out,
            classifications,
            confidence: self.confidence,
        })
    }
}

/// Knuth MMIX LCG step — advances the state and returns the new value.
#[inline]
fn lcg_next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *state
}

/// Fisher-Yates in-place shuffle driven by the Knuth MMIX LCG.
fn permutation_fisher_yates(buf: &mut [f64], state: &mut u64) {
    let n = buf.len();
    for i in (1..n).rev() {
        let j = (lcg_next(state) as usize) % (i + 1);
        buf.swap(i, j);
    }
}

/// Standard normal CDF
fn standard_normal_cdf(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / 2_f64.sqrt()))
}

/// Error function
fn erf(x: f64) -> f64 {
    let sign = x.signum();
    let x = x.abs();

    let a1 = 0.254_829_592;
    let a2 = -0.284_496_736;
    let a3 = 1.421_413_741;
    let a4 = -1.453_152_027;
    let a5 = 1.061_405_429;
    let p = 0.327_591_100;

    let t = 1.0 / (1.0 + p * x);
    let result = 1.0
        - (a1 * t + a2 * t * t + a3 * t.powi(3) + a4 * t.powi(4) + a5 * t.powi(5)) * (-x * x).exp();

    sign * result
}

#[cfg(test)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_global_morans_i() {
        let values = array![1.0, 2.0, 3.0, 4.0, 5.0];
        let mut weights_matrix = scirs2_core::ndarray::Array2::zeros((5, 5));
        for i in 0..4 {
            weights_matrix[[i, i + 1]] = 1.0;
            weights_matrix[[i + 1, i]] = 1.0;
        }

        let mut weights = SpatialWeights::from_adjacency(weights_matrix)
            .expect("Creating spatial weights from adjacency matrix should succeed");
        weights.row_standardize();

        let morans_i = MoransI::new(0.05);
        let result = morans_i
            .calculate(&values.view(), &weights)
            .expect("Global Moran's I calculation should succeed");

        // Should show positive spatial autocorrelation
        assert!(result.i_statistic > result.expected_i);
    }

    #[test]
    fn test_local_morans_i() {
        let values = array![1.0, 1.0, 1.0, 10.0, 10.0, 10.0];
        let mut weights_matrix = scirs2_core::ndarray::Array2::zeros((6, 6));
        for i in 0..5 {
            weights_matrix[[i, i + 1]] = 1.0;
            weights_matrix[[i + 1, i]] = 1.0;
        }

        let weights = SpatialWeights::from_adjacency(weights_matrix)
            .expect("Creating spatial weights from adjacency matrix should succeed");
        let local_i = LocalMoransI::new(0.05);
        let result = local_i
            .calculate(&values.view(), &weights)
            .expect("Local Moran's I calculation should succeed");

        assert_eq!(result.local_i.len(), 6);

        // Regression coverage for the Anselin (1995) analytical variance:
        // z_scores/p_values must be finite valid probabilities for every
        // location.
        for i in 0..6 {
            assert!(
                result.z_scores[i].is_finite(),
                "z_scores[{i}] must be finite"
            );
            assert!(
                (0.0..=1.0).contains(&result.p_values[i]),
                "p_values[{i}] = {} must be a valid probability",
                result.p_values[i]
            );
        }
        // Endpoints (1 neighbor -> wi_dot = wi_dot2 = 1) have a hand-derived
        // Var(I_0) = 0.96 != 1.0, so under the old `variance_i = 1.0`
        // hardcode z_scores[0] was *exactly* local_i[0] (division by 1.0);
        // this pins the fix directly. (Interior locations 1..4 have
        // wi_dot = 2 and, for this particular symmetric +/-4.5 dataset,
        // z_score coincidentally also comes out to 2.0 = local_i, so they
        // are not usable as a regression signal here.)
        assert_eq!(result.local_i[0], 1.0);
        assert!(
            (result.z_scores[0] - 1.224_744_871_391_589).abs() < 1e-9,
            "z_scores[0] = {}, expected (local_i[0] - E[I_0]) / sqrt(0.96)",
            result.z_scores[0]
        );
        // Local I itself (unaffected by the variance fix) should still
        // show the expected clustering pattern.
        assert!(result.local_i[0] > 0.0, "location 0 sits in an LL cluster");
        assert!(result.local_i[5] > 0.0, "location 5 sits in an HH cluster");
    }

    /// Locks down the exact Var(I) closed form (Cliff & Ord 1981
    /// randomization assumption) against a hand-computed value, so a
    /// regression back to the dimensionally-wrong formula (`n*s1` instead
    /// of `n^2*s1`, dividing by `s5` instead of `s0^2`) cannot ship
    /// silently again.
    #[test]
    fn test_global_morans_i_variance_matches_closed_form() {
        let values = array![1.0, 2.0, 3.0, 4.0];
        // Path graph 0-1-2-3 (symmetric binary adjacency, NOT
        // row-standardized so S0/S1/S2 are easy to hand-verify).
        let mut weights_matrix = scirs2_core::ndarray::Array2::zeros((4, 4));
        for i in 0..3 {
            weights_matrix[[i, i + 1]] = 1.0;
            weights_matrix[[i + 1, i]] = 1.0;
        }
        let weights = SpatialWeights::from_adjacency(weights_matrix)
            .expect("Creating spatial weights from adjacency matrix should succeed");

        let morans_i = MoransI::new(0.05);
        let result = morans_i
            .calculate(&values.view(), &weights)
            .expect("Global Moran's I calculation should succeed");

        // Hand-derived: S0=6, S1=12, S2=40, b2=41/25, E[I]=-1/3 =>
        // Var(I) = 8/45.
        let expected_variance = 8.0 / 45.0;
        assert!(
            (result.variance_i - expected_variance).abs() < 1e-9,
            "variance_i = {}, expected {}",
            result.variance_i,
            expected_variance
        );

        let expected_i_statistic = 1.0 / 3.0;
        assert!(
            (result.i_statistic - expected_i_statistic).abs() < 1e-9,
            "i_statistic = {}, expected {}",
            result.i_statistic,
            expected_i_statistic
        );

        let expected_z = (2.0f64 / 3.0) / expected_variance.sqrt();
        assert!(
            (result.z_score - expected_z).abs() < 1e-6,
            "z_score = {}, expected {}",
            result.z_score,
            expected_z
        );
    }

    /// The n == 3 boundary makes the randomization-assumption denominator
    /// `(n-1)(n-2)(n-3)` vanish; `calculate()` must fall back to the
    /// normality-assumption formula instead of dividing by zero /
    /// producing NaN.
    #[test]
    fn test_global_morans_i_n_equals_three_no_nan() {
        let values = array![1.0, 5.0, 2.0];
        let mut weights_matrix = scirs2_core::ndarray::Array2::zeros((3, 3));
        weights_matrix[[0, 1]] = 1.0;
        weights_matrix[[1, 0]] = 1.0;
        weights_matrix[[1, 2]] = 1.0;
        weights_matrix[[2, 1]] = 1.0;

        let weights = SpatialWeights::from_adjacency(weights_matrix)
            .expect("Creating spatial weights from adjacency matrix should succeed");
        let morans_i = MoransI::new(0.05);
        let result = morans_i
            .calculate(&values.view(), &weights)
            .expect("Global Moran's I calculation should succeed for n == 3");

        assert!(result.variance_i.is_finite());
        assert!(result.variance_i >= 0.0);
        assert!(result.z_score.is_finite());
        assert!((0.0..=1.0).contains(&result.p_value));
    }

    /// The analytical `calculate()` z-scores should broadly agree in sign
    /// and order of magnitude with the Monte Carlo
    /// `calculate_with_permutations()` z-scores, since both target the same
    /// randomization null distribution. This guards against the analytical
    /// formula drifting to a statistically nonsensical result even though
    /// the closed-form regression test above already pins the exact
    /// arithmetic for a specific case.
    #[test]
    fn test_local_morans_i_analytical_agrees_with_permutations() {
        let values = array![1.0, 1.2, 0.9, 8.0, 8.3, 7.8, 1.1, 8.1];
        let n = values.len();
        let mut weights_matrix = scirs2_core::ndarray::Array2::zeros((n, n));
        for i in 0..n - 1 {
            weights_matrix[[i, i + 1]] = 1.0;
            weights_matrix[[i + 1, i]] = 1.0;
        }
        let weights = SpatialWeights::from_adjacency(weights_matrix)
            .expect("Creating spatial weights from adjacency matrix should succeed");

        let local_i = LocalMoransI::new(0.05);
        let analytical = local_i
            .calculate(&values.view(), &weights)
            .expect("Analytical local Moran's I should succeed");
        let permuted = local_i
            .calculate_with_permutations(&values.view(), &weights, 4999, 42)
            .expect("Permutation-based local Moran's I should succeed");

        for i in 0..n {
            assert!(analytical.z_scores[i].is_finite());
            // Same sign (or both ~0) — the two inference methods must at
            // least agree on the direction of local association.
            assert!(
                analytical.z_scores[i] * permuted.z_scores[i] >= -1e-6,
                "z_scores disagree in sign at {i}: analytical={}, permuted={}",
                analytical.z_scores[i],
                permuted.z_scores[i]
            );
        }
    }
}
