//! Supervised classification algorithms
//!
//! Implements the Gaussian Maximum Likelihood Classifier (MLC) as described in
//! Richards (1999) "Remote Sensing Digital Image Analysis", §8.3.
//!
//! Each class is modelled by a multivariate Gaussian distribution.  For a
//! *d*-band pixel **x**, the log-discriminant for class *c* is:
//!
//! ```text
//! g_c(x) = -0.5 * ln|Σ_c|
//!          - 0.5 * (x - μ_c)^T  Σ_c^{-1}  (x - μ_c)
//!          + ln P(c)
//! ```
//!
//! where `Σ_c` is the class covariance (biased estimator), `μ_c` is the class
//! mean, and `P(c)` is the prior probability.  The pixel is assigned the class
//! with the maximum discriminant.

use crate::classification::gaussian::invert_spd_with_logdet;
use crate::error::{Result, SensorError};
use scirs2_core::ndarray::{Array1, Array2, ArrayView1, ArrayView2};

// ---------------------------------------------------------------------------
// Internal per-class model
// ---------------------------------------------------------------------------

/// Fitted Gaussian model for one training class.
struct ClassModel {
    /// *d*-dimensional empirical mean vector.
    mean: Vec<f64>,
    /// *d×d* inverse of the regularised sample covariance matrix.
    cov_inv: Array2<f64>,
    /// `ln |Σ_c|`  (natural log of the covariance determinant).
    log_det: f64,
    /// `ln P(c)` — log prior probability for this class.
    log_prior: f64,
}

// ---------------------------------------------------------------------------
// Public classifier struct
// ---------------------------------------------------------------------------

/// Gaussian Maximum Likelihood Classifier.
///
/// Training and prediction happen in a single call to [`classify`], which
/// keeps the public API minimal while following the remote-sensing convention
/// where training data are passed alongside the image to be classified.
///
/// # Prior probabilities
///
/// By default all classes share equal priors.  Use [`with_priors`] to supply
/// custom class priors (they need not sum to 1 — they will be normalised
/// during fit).
///
/// [`classify`]: MaximumLikelihood::classify
/// [`with_priors`]: MaximumLikelihood::with_priors
pub struct MaximumLikelihood {
    /// Optional custom prior probabilities; `None` → equal priors.
    priors: Option<Vec<f64>>,
}

impl MaximumLikelihood {
    /// Create a new Maximum Likelihood classifier with equal class priors.
    pub fn new() -> Self {
        Self { priors: None }
    }

    /// Create a classifier with explicit prior probabilities.
    ///
    /// The vector length must equal the number of classes inferred from the
    /// training labels at classification time.  Priors need not be normalised
    /// — only their ratios matter.
    pub fn with_priors(priors: Vec<f64>) -> Self {
        Self {
            priors: Some(priors),
        }
    }

    /// Classify pixels in `data` using Gaussian Maximum Likelihood.
    ///
    /// # Arguments
    /// - `data`            — `(n_pixels × d)` array of test observations.
    /// - `training_data`   — `(n_train × d)` array of labelled training observations.
    /// - `training_labels` — `(n_train,)` integer class labels `0 .. n_classes-1`.
    ///
    /// # Returns
    /// A `(n_pixels,)` array of predicted class labels.
    pub fn classify(
        &self,
        data: &ArrayView2<f64>,
        training_data: &ArrayView2<f64>,
        training_labels: &ArrayView1<usize>,
    ) -> Result<Array1<usize>> {
        let n_pixels = data.nrows();
        if n_pixels == 0 {
            return Ok(Array1::zeros(0));
        }

        let models = fit(training_data, training_labels, self.priors.as_deref())?;

        // Validate that the test data dimensionality matches training.
        let d = training_data.ncols();
        if data.ncols() != d {
            return Err(SensorError::dimension_mismatch(
                format!("{d} spectral bands (from training data)"),
                format!("{} spectral bands (in data)", data.ncols()),
            ));
        }

        let mut labels = Array1::<usize>::zeros(n_pixels);

        for px in 0..n_pixels {
            let pixel = data.row(px);
            let mut best_class = 0_usize;
            let mut best_score = f64::NEG_INFINITY;

            for (class_idx, model) in models.iter().enumerate() {
                // diff = x - μ_c  (convert ArrayView1 → owned Array1 for arithmetic)
                let diff: Array1<f64> = &pixel - &Array1::from(model.mean.clone());
                // tmp = Σ_c^{-1} * diff
                let tmp = model.cov_inv.dot(&diff);
                // Mahalanobis squared distance
                let mahal_sq = diff.dot(&tmp);
                let score = -0.5 * model.log_det - 0.5 * mahal_sq + model.log_prior;

                if score > best_score {
                    best_score = score;
                    best_class = class_idx;
                }
            }

            labels[px] = best_class;
        }

        Ok(labels)
    }
}

impl Default for MaximumLikelihood {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Fit: estimate per-class Gaussian parameters from labelled training data
// ---------------------------------------------------------------------------

/// Fit one [`ClassModel`] per class from labelled training observations.
///
/// # Arguments
/// - `training_data`   — `(n_train × d)` array.
/// - `training_labels` — `(n_train,)` class indices in `0 .. n_classes-1`.
/// - `priors` — optional slice of raw prior weights (length must equal
///   `n_classes` if provided); will be log-normalised.
///
/// # Errors
/// - [`SensorError::InvalidParameter`] if training data is empty or dimensions
///   are inconsistent.
/// - [`SensorError::ClassificationError`] if any class has no training samples.
/// - [`SensorError::SingularCovariance`] if a class covariance remains
///   non-positive-definite even after Tikhonov regularisation.
fn fit(
    training_data: &ArrayView2<f64>,
    training_labels: &ArrayView1<usize>,
    priors: Option<&[f64]>,
) -> Result<Vec<ClassModel>> {
    let n_train = training_data.nrows();
    let d = training_data.ncols();

    // --- basic validation ---
    if n_train == 0 {
        return Err(SensorError::invalid_parameter(
            "training_data",
            "must contain at least one sample",
        ));
    }
    if training_labels.len() != n_train {
        return Err(SensorError::dimension_mismatch(
            format!("{n_train} rows (from training_data)"),
            format!("{} elements (in training_labels)", training_labels.len()),
        ));
    }

    // Number of classes is 1 + max label value.
    let n_classes = training_labels
        .iter()
        .copied()
        .max()
        .map(|m| m + 1)
        .unwrap_or(1);

    // --- collect row indices per class ---
    let mut class_rows: Vec<Vec<usize>> = vec![Vec::new(); n_classes];
    for (row_idx, &label) in training_labels.iter().enumerate() {
        if label >= n_classes {
            return Err(SensorError::invalid_parameter(
                "training_labels",
                format!("label {label} is out of range [0, {n_classes})"),
            ));
        }
        class_rows[label].push(row_idx);
    }

    // --- validate priors length if provided ---
    if let Some(p) = priors
        && p.len() != n_classes
    {
        return Err(SensorError::invalid_parameter(
            "priors",
            format!(
                "length {} does not match number of classes {n_classes}",
                p.len()
            ),
        ));
    }

    // --- log-normalise priors ---
    // Compute ln P(c) for each class.
    let log_priors: Vec<f64> = match priors {
        Some(p) => {
            let total: f64 = p.iter().sum();
            if total <= 0.0 {
                return Err(SensorError::invalid_parameter(
                    "priors",
                    "prior weights must sum to a positive value",
                ));
            }
            p.iter().map(|&pi| (pi / total).ln()).collect()
        }
        None => {
            let log_uniform = (1.0 / n_classes as f64).ln();
            vec![log_uniform; n_classes]
        }
    };

    // --- fit each class ---
    let tikhonov_lambda = 1e-6_f64;
    let mut models = Vec::with_capacity(n_classes);

    for class_idx in 0..n_classes {
        let rows = &class_rows[class_idx];
        if rows.is_empty() {
            return Err(SensorError::classification_error(format!(
                "class {class_idx} has no training samples"
            )));
        }

        let n_c = rows.len() as f64;

        // --- compute mean ---
        let mut mean = vec![0.0_f64; d];
        for &row_idx in rows {
            let row = training_data.row(row_idx);
            for band in 0..d {
                mean[band] += row[band];
            }
        }
        for v in &mut mean {
            *v /= n_c;
        }

        // --- compute biased covariance  Σ = (1/n_c) Σ_i (x_i - μ)(x_i - μ)^T ---
        let mut cov = Array2::<f64>::zeros((d, d));
        for &row_idx in rows {
            let row = training_data.row(row_idx);
            for band_i in 0..d {
                let di = row[band_i] - mean[band_i];
                for band_j in 0..=band_i {
                    let dj = row[band_j] - mean[band_j];
                    cov[[band_i, band_j]] += di * dj;
                    if band_i != band_j {
                        cov[[band_j, band_i]] += di * dj;
                    }
                }
            }
        }
        for v in cov.iter_mut() {
            *v /= n_c;
        }

        // --- invert with Tikhonov regularisation ---
        let (cov_inv, log_det) = invert_spd_with_logdet(&cov, tikhonov_lambda)?;

        models.push(ClassModel {
            mean,
            cov_inv,
            log_det,
            log_prior: log_priors[class_idx],
        });
    }

    Ok(models)
}

// ---------------------------------------------------------------------------
// Unit tests (in-module)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_maximum_likelihood() {
        let data = array![[0.1_f64, 0.2], [0.3, 0.4]];
        let training = array![[0.1_f64, 0.2]];
        let labels = array![0_usize];

        let classifier = MaximumLikelihood::new();
        let result = classifier
            .classify(&data.view(), &training.view(), &labels.view())
            .ok();
        assert!(result.is_some());
    }

    #[test]
    fn test_with_priors_constructor() {
        let clf = MaximumLikelihood::with_priors(vec![0.3, 0.7]);
        assert!(clf.priors.is_some());
    }

    #[test]
    fn test_two_class_separation() {
        let training = array![
            [1.0_f64, 1.0],
            [1.1, 0.9],
            [0.9, 1.1],
            [5.0, 5.0],
            [5.1, 4.9],
            [4.9, 5.1]
        ];
        let labels = array![0_usize, 0, 0, 1, 1, 1];
        let data = array![[1.0_f64, 1.0], [5.0, 5.0]];
        let clf = MaximumLikelihood::new();
        let r = clf
            .classify(&data.view(), &training.view(), &labels.view())
            .expect("well-separated two-class problem must succeed");
        assert_eq!(r[0], 0);
        assert_eq!(r[1], 1);
    }
}
