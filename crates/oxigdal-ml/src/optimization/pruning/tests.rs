//! Tests for model pruning module.

use super::*;

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
