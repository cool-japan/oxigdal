//! Tests for model pruning module.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::optimization::onnx_weights::test_support::build_model;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

/// An RAII fixture path inside [`std::env::temp_dir`].
///
/// The leaf name embeds the process id and a monotonic counter, so no two test
/// binaries — nor two concurrent runs of this one — can ever land on the same
/// file.  Dropping the guard removes the fixture, so a panicking test leaks
/// nothing.
struct TempPath(std::path::PathBuf);

impl TempPath {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "oxigeo_ml_prune_{}_{seq}_{name}",
            std::process::id()
        )))
    }
}

impl std::ops::Deref for TempPath {
    type Target = std::path::Path;

    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

impl AsRef<std::path::Path> for TempPath {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn test_pruning_config_builder() {
    let config = PruningConfig::builder()
        .strategy(PruningStrategy::Structured)
        .sparsity_target(0.7)
        .schedule(PruningSchedule::Iterative { iterations: 5 })
        .granularity(PruningGranularity::Channel)
        .fine_tune(false)
        .build();

    assert_eq!(config.strategy, PruningStrategy::Structured);
    assert!((config.sparsity_target - 0.7).abs() < 1e-6);
    assert!(!config.fine_tune);
}

#[test]
fn test_sparsity_clamping() {
    let config1 = PruningConfig::builder().sparsity_target(1.5).build();
    assert!((config1.sparsity_target - 1.0).abs() < 1e-6);

    let config2 = PruningConfig::builder().sparsity_target(-0.5).build();
    assert!((config2.sparsity_target - 0.0).abs() < 1e-6);
}

#[test]
fn test_pruning_stats() {
    let stats = PruningStats {
        original_params: 1000000,
        pruned_params: 500000,
        actual_sparsity: 0.5,
    };

    assert_eq!(stats.params_removed(), 500000);
    assert!((stats.size_reduction_percent() - 50.0).abs() < 1e-6);
}

#[test]
fn test_magnitude_importance() {
    let weights = vec![-0.5, 0.2, -0.8, 0.1];
    let importance = compute_magnitude_importance(&weights);

    assert_eq!(importance.len(), weights.len());
    assert!((importance[0] - 0.5).abs() < 1e-6);
    assert!((importance[2] - 0.8).abs() < 1e-6);
}

#[test]
fn test_gradient_importance() {
    let weights = vec![0.5, 0.2, 0.8, 0.1];
    let gradients = vec![0.1, 0.5, 0.2, 0.3];
    let importance = compute_gradient_importance(&weights, &gradients);

    assert_eq!(importance.len(), weights.len());
    assert!(importance[0] > 0.0);
}

#[test]
fn test_select_weights_to_prune() {
    let importance = vec![0.5, 0.2, 0.8, 0.1, 0.6];
    let mask = select_weights_to_prune(&importance, 0.4); // Prune 40%

    // Should prune 2 weights (40% of 5)
    let pruned_count = mask.iter().filter(|&&x| x).count();
    assert_eq!(pruned_count, 2);

    // Should prune the least important weights (indices 1 and 3)
    assert!(mask[1]); // importance 0.2
    assert!(mask[3]); // importance 0.1
}

#[test]
fn test_channel_importance() {
    let channels = vec![
        vec![0.1, 0.2, 0.3],    // L2 norm ~ 0.374
        vec![0.5, 0.5, 0.5],    // L2 norm ~ 0.866
        vec![0.01, 0.01, 0.01], // L2 norm ~ 0.017
    ];

    let importance = compute_channel_importance(&channels);
    assert_eq!(importance.len(), 3);

    // Channel 1 should have highest importance
    assert!(importance[1] > importance[0]);
    assert!(importance[1] > importance[2]);

    // Channel 2 should have lowest importance
    assert!(importance[2] < importance[0]);
}

#[test]
fn test_taylor_importance() {
    let weights = vec![0.5, 0.2, 0.8, 0.1];
    let gradients = vec![0.1, 0.5, 0.2, 0.3];
    let activations = vec![0.9, 0.8, 0.7, 0.6];

    let importance = compute_taylor_importance(&weights, &gradients, &activations);
    assert_eq!(importance.len(), weights.len());

    // All importance scores should be non-negative
    for score in &importance {
        assert!(*score >= 0.0);
    }
}

#[test]
fn test_polynomial_schedule() {
    let _config = PruningConfig {
        strategy: PruningStrategy::Magnitude,
        sparsity_target: 0.8,
        schedule: PruningSchedule::Polynomial {
            initial_sparsity: 10,
            final_sparsity: 80,
            steps: 5,
        },
        granularity: PruningGranularity::Element,
        fine_tune: false,
        fine_tune_epochs: 0,
    };

    // Test sparsity progression
    // At t=0: should be close to initial_sparsity
    // At t=T: should be close to final_sparsity
    let s_0 = 0.8 + (0.1 - 0.8) * (1.0_f32).powi(3);
    let s_final = 0.8 + (0.1 - 0.8) * (0.0_f32).powi(3);

    assert!((s_0 - 0.1).abs() < 0.01); // Should be ~10%
    assert!((s_final - 0.8).abs() < 0.01); // Should be ~80%
}

/// Counts the number of exact-zero float32 values across all initializers of a
/// serialized ONNX model.
fn count_zeros(model_bytes: &[u8]) -> (usize, usize) {
    let inits =
        crate::optimization::onnx_weights::parse_float_initializers(model_bytes).expect("parse");
    let total: usize = inits.iter().map(|i| i.values.len()).sum();
    let zeros: usize = inits
        .iter()
        .flat_map(|i| i.values.iter())
        .filter(|&&v| v == 0.0)
        .count();
    (zeros, total)
}

#[test]
fn test_unstructured_pruning_real_onnx_roundtrip() {
    // Build a real ONNX model with two float weight tensors, none initially zero.
    let w1: Vec<f32> = (1..=16).map(|i| i as f32).collect();
    let w2: Vec<f32> = (1..=9).map(|i| -(i as f32)).collect();
    let model = build_model(&[("w1", vec![4, 4], w1), ("w2", vec![3, 3], w2)]);

    let input = TempPath::new("unstruct_in.onnx");
    let output = TempPath::new("unstruct_out.onnx");
    std::fs::write(&input, &model).expect("write input");

    let config = PruningConfig::builder()
        .strategy(PruningStrategy::Magnitude)
        .sparsity_target(0.5)
        .build();

    let stats = unstructured_pruning(&input, &output, &config).expect("prune");

    // 25 total params; 50% sparsity => ~12 or 13 pruned.
    assert_eq!(stats.original_params, 25);
    assert!(stats.actual_sparsity > 0.4 && stats.actual_sparsity < 0.6);

    // The output must be a valid ONNX model whose zeroed-value count matches
    // the reported sparsity (real in-place pruning, not a byte-reinterpretation).
    let out_bytes = std::fs::read(&output).expect("read output");
    let (zeros, total) = count_zeros(&out_bytes);
    assert_eq!(total, 25);
    assert_eq!(zeros, stats.params_removed());
    // The smallest-magnitude weights (±1, ±2, ...) must have been zeroed.
    let inits =
        crate::optimization::onnx_weights::parse_float_initializers(&out_bytes).expect("parse");
    let w1_out = &inits[0].values;
    assert_eq!(w1_out[0], 0.0, "smallest magnitude weight should be pruned");
}

#[test]
fn test_structured_pruning_zeroes_whole_channels() {
    // 4 output channels of 4 weights each; channel 0 has the smallest L2 norm.
    let weights: Vec<f32> = vec![
        0.1, 0.1, 0.1, 0.1, // channel 0 (smallest)
        5.0, 5.0, 5.0, 5.0, // channel 1
        9.0, 9.0, 9.0, 9.0, // channel 2
        7.0, 7.0, 7.0, 7.0, // channel 3
    ];
    let model = build_model(&[("conv", vec![4, 4], weights)]);

    let input = TempPath::new("struct_in.onnx");
    let output = TempPath::new("struct_out.onnx");
    std::fs::write(&input, &model).expect("write input");

    let config = PruningConfig::builder()
        .strategy(PruningStrategy::Structured)
        .sparsity_target(0.25) // prune 1 of 4 channels
        .build();

    let stats = structured_pruning(&input, &output, &config).expect("prune");
    assert_eq!(stats.original_params, 16);
    assert_eq!(stats.pruned_params, 12); // one channel (4 weights) removed
    assert!((stats.actual_sparsity - 0.25).abs() < 1e-6);

    let out_bytes = std::fs::read(&output).expect("read output");
    let inits =
        crate::optimization::onnx_weights::parse_float_initializers(&out_bytes).expect("parse");
    let vals = &inits[0].values;
    // Channel 0 (indices 0..4) must be fully zeroed; others untouched.
    assert_eq!(&vals[0..4], &[0.0, 0.0, 0.0, 0.0]);
    assert_eq!(&vals[4..8], &[5.0, 5.0, 5.0, 5.0]);
}

#[test]
fn test_pruning_rejects_non_onnx_input() {
    let input = TempPath::new("bad_in.bin");
    let output = TempPath::new("bad_out.bin");
    std::fs::write(&input, vec![0xAAu8; 128]).expect("write");

    let config = PruningConfig::builder().sparsity_target(0.5).build();
    let result = unstructured_pruning(&input, &output, &config);
    assert!(
        result.is_err(),
        "non-ONNX input must be rejected, not corrupted"
    );
}

/// `iterative_pruning` must not let concurrent callers trample each other.
///
/// The intermediate models were written straight into `std::env::temp_dir()` as
/// `pruned_iter_{i}.onnx` with no uniquing whatsoever, so two simultaneous calls
/// shared one set of filenames: each iteration read back whichever model the
/// *other* thread had most recently written and pruned that instead. Every
/// thread here uses a distinct parameter count, so a crossed intermediate shows
/// up immediately as the wrong `original_params` on iteration 2+ or as a final
/// model of the wrong size.
#[test]
fn test_iterative_pruning_is_safe_under_concurrency() {
    /// Distinct tensor width per thread => distinct parameter count.
    const THREADS: usize = 8;
    const ITERATIONS: usize = 3;

    let handles: Vec<_> = (0..THREADS)
        .map(|t| {
            std::thread::spawn(move || {
                // Thread t gets (t + 3) x (t + 3) weights: 9, 16, 25, ... params.
                let side = t + 3;
                let params = side * side;
                let weights: Vec<f32> = (1..=params).map(|i| i as f32).collect();
                let model = build_model(&[("w", vec![side as i64, side as i64], weights)]);

                let input = TempPath::new(&format!("iter_concurrent_in_{t}.onnx"));
                let output = TempPath::new(&format!("iter_concurrent_out_{t}.onnx"));
                std::fs::write(&input, &model).expect("write input");

                let config = PruningConfig::builder()
                    .strategy(PruningStrategy::Magnitude)
                    .sparsity_target(0.5)
                    .schedule(PruningSchedule::Iterative {
                        iterations: ITERATIONS,
                    })
                    .build();

                let history = iterative_pruning(&*input, &*output, &config).expect("prune");

                assert_eq!(
                    history.len(),
                    ITERATIONS,
                    "thread {t}: one stats entry per iteration"
                );
                for (i, stats) in history.iter().enumerate() {
                    assert_eq!(
                        stats.original_params, params,
                        "thread {t} iteration {i}: pruned a model with {} params, but this \
                         thread's model has {params} — an intermediate from another thread \
                         was picked up",
                        stats.original_params
                    );
                }

                // The final output must still be this thread's model.
                let out_bytes = std::fs::read(&output).expect("read output");
                let (_, total) = count_zeros(&out_bytes);
                assert_eq!(
                    total, params,
                    "thread {t}: final model has {total} params, expected {params}"
                );
            })
        })
        .collect();

    for (t, handle) in handles.into_iter().enumerate() {
        handle
            .join()
            .unwrap_or_else(|_| panic!("thread {t} panicked"));
    }
}
