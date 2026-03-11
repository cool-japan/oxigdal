//! Tests for ndarray and rand integration
//!
//! Verifies that ndarray and random number generation work correctly
//! in the oxigdal-ml crate, using scirs2_core instead of direct ndarray/rand.

use scirs2_core::ndarray::{Array1, Array2, Array3, Array4, Axis, s};
use scirs2_core::random::prelude::{SeedableRng, StdRng, seeded_rng};

/// Test that ndarray types work correctly
#[test]
fn test_ndarray_basic() {
    // Test 1D array
    let a1 = Array1::<f32>::zeros(10);
    assert_eq!(a1.len(), 10);
    assert_eq!(a1.shape(), &[10]);

    // Test 2D array
    let a2 = Array2::<f64>::zeros((3, 4));
    assert_eq!(a2.shape(), &[3, 4]);
    assert_eq!(a2.len(), 12);

    // Test 3D array
    let a3 = Array3::<f32>::zeros((2, 3, 4));
    assert_eq!(a3.shape(), &[2, 3, 4]);
    assert_eq!(a3.len(), 24);

    // Test 4D array
    let a4 = Array4::<f32>::zeros((1, 2, 3, 4));
    assert_eq!(a4.shape(), &[1, 2, 3, 4]);
    assert_eq!(a4.len(), 24);
}

/// Test array operations directly
#[test]
fn test_ndarray_operations() {
    let mut arr = Array2::<f32>::zeros((3, 3));

    // Set values
    arr[[0, 0]] = 1.0;
    arr[[1, 1]] = 2.0;
    arr[[2, 2]] = 3.0;

    // Check values
    assert_eq!(arr[[0, 0]], 1.0);
    assert_eq!(arr[[1, 1]], 2.0);
    assert_eq!(arr[[2, 2]], 3.0);

    // Test sum
    let sum: f32 = arr.sum();
    assert_eq!(sum, 6.0);
}

/// Test array slicing directly
#[test]
fn test_ndarray_slicing() {
    let arr = Array3::<f32>::from_shape_fn((4, 5, 6), |(i, j, k)| (i * 100 + j * 10 + k) as f32);

    // Test slice macro with fixed index
    let slice = arr.slice(s![1_usize, .., ..]);
    assert_eq!(slice.shape(), &[5, 6]);
    assert_eq!(slice[[0, 0]], 100.0);

    // Test multi-dimensional slice
    let slice2 = arr.slice(s![.., 2_usize, ..]);
    assert_eq!(slice2.shape(), &[4, 6]);
    assert_eq!(slice2[[0, 0]], 20.0);

    // Test range slice
    let slice3 = arr.slice(s![1_usize..3, .., ..]);
    assert_eq!(slice3.shape(), &[2, 5, 6]);
}

/// Test axis operations directly
#[test]
fn test_ndarray_axis() {
    let arr = Array3::<f32>::ones((2, 3, 4));

    // Test insert_axis
    let expanded = arr.clone().insert_axis(Axis(0));
    assert_eq!(expanded.shape(), &[1, 2, 3, 4]);

    // Test index_axis
    let indexed = arr.index_axis(Axis(0), 0);
    assert_eq!(indexed.shape(), &[3, 4]);

    // Test sum along axis
    let sum_axis = arr.sum_axis(Axis(2));
    assert_eq!(sum_axis.shape(), &[2, 3]);
    assert_eq!(sum_axis[[0, 0]], 4.0); // sum of 4 ones
}

/// Test array creation methods directly
#[test]
fn test_ndarray_creation() {
    // Test zeros
    let zeros = Array2::<f64>::zeros((3, 3));
    assert_eq!(zeros.sum(), 0.0);

    // Test ones
    let ones = Array2::<f32>::ones((2, 3));
    assert_eq!(ones.sum(), 6.0);

    // Test from_vec
    let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
    let arr = Array2::<f64>::from_shape_vec((2, 3), data.clone());
    assert!(arr.is_ok());
    let arr = arr.expect("Should create array");
    assert_eq!(arr[[0, 0]], 1.0);
    assert_eq!(arr[[1, 2]], 6.0);

    // Test from_shape_fn
    let arr = Array2::<i32>::from_shape_fn((3, 3), |(i, j)| (i * 3 + j) as i32);
    assert_eq!(arr[[0, 0]], 0);
    assert_eq!(arr[[2, 2]], 8);
}

/// Test rand number generation
#[test]
fn test_random_basic() {
    // Create RNG with fixed seed for reproducibility
    let mut rng = seeded_rng(42);

    // Generate random numbers
    let r1: f64 = rng.random();
    let r2: f64 = rng.random();

    // Should be different
    assert_ne!(r1, r2);

    // Should be in range [0, 1)
    assert!((0.0..1.0).contains(&r1));
    assert!((0.0..1.0).contains(&r2));
}

/// Test that seeded RNGs produce same sequence
#[test]
fn test_random_reproducibility() {
    let mut rng1 = StdRng::seed_from_u64(123);
    let mut rng2 = StdRng::seed_from_u64(123);

    // Generate 10 numbers from each
    for _ in 0..10 {
        let r1: f64 = rng1.random();
        let r2: f64 = rng2.random();
        assert_eq!(r1, r2, "Seeded RNGs should produce same sequence");
    }
}

/// Test random number generation with different types
#[test]
fn test_random_types() {
    let mut rng = seeded_rng(42);

    // Test f64
    let r_f64: f64 = rng.random();
    assert!((0.0..1.0).contains(&r_f64));

    // Test f32
    let r_f32: f32 = rng.random();
    assert!((0.0_f32..1.0_f32).contains(&r_f32));
}

/// Test Box-Muller transform for Gaussian random numbers
#[test]
fn test_gaussian_random() {
    use std::f64::consts::PI;

    let mut rng = seeded_rng(42);

    // Generate Gaussian random numbers using Box-Muller
    let mut samples = Vec::new();
    for _ in 0..1000 {
        let u1: f64 = rng.random();
        let u2: f64 = rng.random();
        let z0 = (-2.0_f64 * u1.ln()).sqrt() * (2.0_f64 * PI * u2).cos();
        samples.push(z0);
    }

    // Check that mean is approximately 0
    let mean: f64 = samples.iter().sum::<f64>() / samples.len() as f64;
    assert!(mean.abs() < 0.1, "Mean should be close to 0, got {}", mean);

    // Check that std dev is approximately 1
    let variance: f64 =
        samples.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / samples.len() as f64;
    let std_dev = variance.sqrt();
    assert!(
        (std_dev - 1.0).abs() < 0.1,
        "Std dev should be close to 1, got {}",
        std_dev
    );
}

/// Test ndarray and random together (typical ML use case)
#[test]
fn test_ndarray_random_combined() {
    let mut rng = seeded_rng(42);

    // Create array and fill with random values
    let mut arr = Array2::<f64>::zeros((10, 10));
    for i in 0..10 {
        for j in 0..10 {
            arr[[i, j]] = rng.random();
        }
    }

    // Check all values are in valid range
    for val in arr.iter() {
        assert!(*val >= 0.0 && *val < 1.0);
    }

    // Check that not all values are the same
    let first = arr[[0, 0]];
    let mut all_same = true;
    for val in arr.iter() {
        if (*val - first).abs() > 1e-10 {
            all_same = false;
            break;
        }
    }
    assert!(!all_same, "Random values should not all be identical");
}

/// Test array reshaping directly
#[test]
fn test_ndarray_reshape() {
    let arr = Array1::<f32>::from_vec(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);

    // Reshape to 2x3 using from_shape_vec
    let reshaped = Array2::<f32>::from_shape_vec((2, 3), arr.to_vec());
    assert!(reshaped.is_ok());
    let reshaped = reshaped.expect("Should reshape");
    assert_eq!(reshaped.shape(), &[2, 3]);
    assert_eq!(reshaped[[0, 0]], 1.0);
    assert_eq!(reshaped[[1, 2]], 6.0);

    // Reshape to 3x2
    let reshaped2 = Array2::<f32>::from_shape_vec((3, 2), arr.to_vec());
    assert!(reshaped2.is_ok());
    let reshaped2 = reshaped2.expect("Should reshape");
    assert_eq!(reshaped2.shape(), &[3, 2]);
    assert_eq!(reshaped2[[0, 0]], 1.0);
    assert_eq!(reshaped2[[2, 1]], 6.0);
}

/// Test array broadcasting directly
#[test]
fn test_ndarray_broadcasting() {
    let arr = Array2::<f32>::ones((3, 3));
    let scalar = 2.0;

    // Scalar multiplication
    let result = &arr * scalar;
    assert_eq!(result.sum(), 18.0); // 9 elements * 2.0

    // Element-wise addition
    let result2 = &arr + &arr;
    assert_eq!(result2.sum(), 18.0); // 9 elements * 2.0
}

/// Test that all ndarray functionality is available
#[test]
fn test_ndarray_functionality() {
    // This test verifies that we can use all necessary ndarray functionality
    // directly in oxigdal-ml

    // Array creation
    let _ = Array1::<f32>::zeros(10);
    let _ = Array2::<f64>::ones((5, 5));
    let _ = Array3::<f32>::zeros((2, 3, 4));
    let _ = Array4::<f32>::ones((1, 2, 3, 4));

    // Slicing
    let arr = Array2::<f32>::ones((5, 5));
    let _ = arr.slice(s![.., 0]);
    let _ = arr.slice(s![0, ..]);
    let _ = arr.slice(s![1..3, ..]);

    // Axis operations
    let _ = Axis(0);
    let _ = arr.sum_axis(Axis(0));

    // Random numbers via scirs2_core
    let mut rng = seeded_rng(42);
    let _: f64 = rng.random();

    // All ndarray and random functionality available via scirs2_core
}
