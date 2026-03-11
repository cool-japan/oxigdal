//! Integration tests for training functionality.

use oxigdal_ml_foundation::training::{
    TrainingConfig,
    checkpointing::CheckpointManager,
    early_stopping::EarlyStopping,
    losses::{CrossEntropyLoss, DiceLoss, LossFunction, MSELoss},
    optimizers::{Adam, Optimizer, SGD},
    schedulers::{CosineAnnealingLR, LRScheduler, StepLR},
};
use scirs2_core::ndarray::arr2;
use tempfile::TempDir;

#[test]
fn test_training_config_validation() {
    let config = TrainingConfig::default();
    assert!(config.validate().is_ok());

    let mut invalid_config = config.clone();
    invalid_config.learning_rate = -0.001;
    assert!(invalid_config.validate().is_err());
}

#[test]
fn test_loss_functions() {
    let predictions = arr2(&[[0.8, 0.2], [0.3, 0.7]]);
    let targets = arr2(&[[1.0, 0.0], [0.0, 1.0]]);

    let mse = MSELoss;
    let mse_loss = mse
        .compute(predictions.view(), targets.view())
        .expect("Failed to compute MSE loss");
    assert!(mse_loss > 0.0);

    let ce = CrossEntropyLoss::new();
    let ce_loss = ce
        .compute(predictions.view(), targets.view())
        .expect("Failed to compute cross-entropy loss");
    assert!(ce_loss > 0.0);

    let dice = DiceLoss::new();
    let dice_loss = dice
        .compute(predictions.view(), targets.view())
        .expect("Failed to compute dice loss");
    assert!((0.0..=1.0).contains(&dice_loss));
}

#[test]
fn test_optimizers() {
    let mut sgd = SGD::new(0.01).expect("Failed to create SGD");
    let gradient = arr2(&[[1.0, 2.0], [3.0, 4.0]]);

    let update = sgd
        .step("param1", gradient.view())
        .expect("Failed to perform SGD step");
    assert_eq!(update.shape(), gradient.shape());

    let mut adam = Adam::new(0.001).expect("Failed to create Adam");
    let update = adam
        .step("param1", gradient.view())
        .expect("Failed to perform Adam step");
    assert_eq!(update.shape(), gradient.shape());
}

#[test]
fn test_lr_schedulers() {
    let step_lr = StepLR::new(10, 0.1).expect("Failed to create StepLR");
    let base_lr = 1.0;

    let lr_0 = step_lr.get_lr(0, base_lr);
    let lr_10 = step_lr.get_lr(10, base_lr);
    let lr_20 = step_lr.get_lr(20, base_lr);

    assert_eq!(lr_0, 1.0);
    assert!(lr_10 < lr_0);
    assert!(lr_20 < lr_10);

    let cosine = CosineAnnealingLR::new(100, 0.0).expect("Failed to create CosineAnnealingLR");
    let lr_0 = cosine.get_lr(0, base_lr);
    let lr_50 = cosine.get_lr(50, base_lr);

    assert_eq!(lr_0, 1.0);
    assert!(lr_50 < lr_0);
}

#[test]
fn test_early_stopping() {
    let mut es = EarlyStopping::for_loss(3, 0.001).expect("Failed to create early stopping");

    assert!(es.update(1.0));
    assert!(es.update(0.9));
    assert!(es.update(0.85));
    assert!(es.update(0.86)); // No improvement
    assert!(es.update(0.87)); // No improvement
    assert!(es.update(0.88)); // No improvement - patience reached

    assert!(!es.update(0.89)); // Should stop
    assert!(es.should_stop());
}

#[test]
fn test_checkpoint_manager() {
    use oxigdal_ml_foundation::training::checkpointing::CheckpointMetadata;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let manager = CheckpointManager::new(temp_dir.path(), Some(3), false)
        .expect("Failed to create checkpoint manager");

    let metadata = CheckpointMetadata::new(0, 0.5);
    let path = manager
        .save_metadata(&metadata)
        .expect("Failed to save checkpoint");
    assert!(path.exists());

    let loaded = CheckpointManager::load_metadata(&path).expect("Failed to load checkpoint");
    assert_eq!(loaded.epoch, 0);
    assert_eq!(loaded.train_loss, 0.5);
}

#[test]
fn test_gradient_clipping() {
    use oxigdal_ml_foundation::training::training_loop::utils::{
        clip_gradients_by_norm, compute_gradient_norm,
    };
    use scirs2_core::ndarray::arr2;

    let grad1 = arr2(&[[3.0, 4.0]]);
    let grad2 = arr2(&[[0.0, 0.0]]);

    let initial_norm = compute_gradient_norm(&[grad1.clone(), grad2.clone()]);
    assert_eq!(initial_norm, 5.0);

    let grad1_clipped = grad1.clone();
    let grad2_clipped = grad2.clone();
    let mut gradients = vec![grad1_clipped, grad2_clipped];
    clip_gradients_by_norm(&mut gradients, 2.5);
    let clipped_norm = compute_gradient_norm(&gradients);
    assert!((clipped_norm - 2.5).abs() < 1e-5);
}
