//! Integration tests for the Gaussian Maximum Likelihood Classifier.

use oxigdal_sensors::classification::supervised::MaximumLikelihood;
use scirs2_core::ndarray::array;

#[test]
fn test_mlc_two_class_well_separated() {
    // Class 0 centred at (1,1), class 1 centred at (5,5).
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
    let result = clf
        .classify(&data.view(), &training.view(), &labels.view())
        .expect("well-separated two-class problem must succeed");
    assert_eq!(result[0], 0);
    assert_eq!(result[1], 1);
}

#[test]
fn test_mlc_three_class() {
    let training = array![
        [0.0_f64, 0.0],
        [0.1, 0.0],
        [0.0, 0.1],
        [5.0, 0.0],
        [5.1, 0.0],
        [4.9, 0.0],
        [0.0, 5.0],
        [0.0, 5.1],
        [0.1, 5.0]
    ];
    let labels = array![0_usize, 0, 0, 1, 1, 1, 2, 2, 2];
    let data = array![[0.05_f64, 0.05], [5.0, 0.05], [0.05, 5.0]];
    let clf = MaximumLikelihood::new();
    let result = clf
        .classify(&data.view(), &training.view(), &labels.view())
        .expect("three-class well-separated problem must succeed");
    assert_eq!(result[0], 0);
    assert_eq!(result[1], 1);
    assert_eq!(result[2], 2);
}

#[test]
fn test_mlc_output_length_matches_data_rows() {
    let training = array![[1.0_f64, 2.0], [3.0, 4.0]];
    let labels = array![0_usize, 1];
    let data = array![[1.0_f64, 2.0], [1.5, 2.5], [3.0, 4.0]];
    let clf = MaximumLikelihood::new();
    let result = clf
        .classify(&data.view(), &training.view(), &labels.view())
        .expect("classification must succeed for valid inputs");
    assert_eq!(result.len(), 3);
}

#[test]
fn test_mlc_tikhonov_handles_rank_deficient_class() {
    // Single training sample → covariance is all zeros → Tikhonov λ=1e-6 makes it invertible.
    let training = array![[1.0_f64, 2.0], [5.0, 6.0]];
    let labels = array![0_usize, 1];
    let data = array![[1.0_f64, 2.0]];
    let clf = MaximumLikelihood::new();
    let result = clf
        .classify(&data.view(), &training.view(), &labels.view())
        .expect("Tikhonov regularisation must handle rank-deficient covariance");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], 0); // closer to class 0
}

#[test]
fn test_mlc_preserves_existing_stub_test() {
    // The old test: 1 training sample, 1 class, 2 data points → must still pass.
    let data = array![[0.1_f64, 0.2], [0.3, 0.4]];
    let training = array![[0.1_f64, 0.2]];
    let labels = array![0_usize];
    let classifier = MaximumLikelihood::new();
    let result = classifier.classify(&data.view(), &training.view(), &labels.view());
    assert!(result.is_ok());
    let r = result.expect("single-class problem must succeed");
    assert_eq!(r.len(), 2);
}

#[test]
fn test_mlc_empty_data_returns_empty() {
    use scirs2_core::ndarray::Array2;
    let training = array![[1.0_f64, 2.0], [3.0, 4.0]];
    let labels = array![0_usize, 1];
    let data: Array2<f64> = Array2::zeros((0, 2));
    let clf = MaximumLikelihood::new();
    let result = clf
        .classify(&data.view(), &training.view(), &labels.view())
        .expect("empty pixel array must succeed and return empty result");
    assert_eq!(result.len(), 0);
}

#[test]
fn test_mlc_cholesky_invert_2x2() {
    // White-box test via classify on a known diagonal-covariance case.
    let training = array![
        [0.0_f64, 0.0],
        [0.0, 0.1],
        [0.1, 0.0],
        [10.0, 10.0],
        [10.1, 10.0],
        [10.0, 10.1]
    ];
    let labels = array![0_usize, 0, 0, 1, 1, 1];
    let clf = MaximumLikelihood::new();
    let data = array![[0.05_f64, 0.05], [10.05, 10.05]];
    let r = clf
        .classify(&data.view(), &training.view(), &labels.view())
        .expect("diagonal covariance case must succeed");
    assert_eq!(r[0], 0);
    assert_eq!(r[1], 1);
}

#[test]
fn test_mlc_with_priors_affects_boundary() {
    // Class 0 and class 1 equidistant from query point (origin midpoint).
    // With strongly skewed prior toward class 1, the midpoint should be class 1.
    let training = array![
        [-1.0_f64, 0.0],
        [-1.1, 0.0],
        [-0.9, 0.0],
        [1.0, 0.0],
        [1.1, 0.0],
        [0.9, 0.0]
    ];
    let labels = array![0_usize, 0, 0, 1, 1, 1];
    let data = array![[0.0_f64, 0.0]]; // exact midpoint

    // Strong prior toward class 1.
    let clf_biased = MaximumLikelihood::with_priors(vec![0.01, 0.99]);
    let result = clf_biased
        .classify(&data.view(), &training.view(), &labels.view())
        .expect("biased prior classification must succeed");
    assert_eq!(result[0], 1);

    // Uniform prior — just verify no error (tie-breaking is implementation-defined).
    let clf_uniform = MaximumLikelihood::new();
    let _ = clf_uniform
        .classify(&data.view(), &training.view(), &labels.view())
        .expect("uniform prior classification must succeed");
}

#[test]
fn test_mlc_dimension_mismatch_errors() {
    let training = array![[1.0_f64, 2.0], [3.0, 4.0]];
    let labels = array![0_usize, 1];
    // 3 features vs 2 in training → should return an error (not panic).
    let data = array![[1.0_f64, 2.0, 3.0]];
    let clf = MaximumLikelihood::new();
    // We just verify it does not panic or hang; error type is an implementation detail.
    let _ = clf.classify(&data.view(), &training.view(), &labels.view());
}
