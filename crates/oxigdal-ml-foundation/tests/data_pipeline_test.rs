//! Integration tests for data pipeline and training infrastructure

#[cfg(feature = "ml")]
use oxigdal_ml_foundation::data::dataloader::DataLoader;
#[cfg(feature = "ml")]
use oxigdal_ml_foundation::data::dataset::Dataset;
use oxigdal_ml_foundation::data::dataset::GeoTiffDataset;
use oxigdal_ml_foundation::training::{TrainingConfig, TrainingHistory};
use std::path::PathBuf;

/// Test GeoTiffDataset creation
#[test]
#[cfg(feature = "ml")]
fn test_dataset_creation() {
    let files = vec![
        PathBuf::from("test1.tif"),
        PathBuf::from("test2.tif"),
        PathBuf::from("test3.tif"),
    ];

    let result = GeoTiffDataset::new(files.clone(), (256, 256));
    assert!(result.is_ok(), "Dataset creation should succeed");

    let dataset = result.expect("Should have dataset");
    assert_eq!(dataset.len(), 30); // 3 files * 10 patches (default)
}

/// Test dataset with labels
#[test]
#[cfg(feature = "ml")]
fn test_dataset_with_labels() {
    let files = vec![PathBuf::from("test1.tif"), PathBuf::from("test2.tif")];
    let labels = vec![PathBuf::from("label1.tif"), PathBuf::from("label2.tif")];

    let dataset = GeoTiffDataset::new(files.clone(), (256, 256)).expect("Failed to create dataset");

    let result = dataset.with_labels(labels.clone());
    assert!(result.is_ok(), "Adding labels should succeed");
}

/// Test dataset configuration
#[test]
#[cfg(feature = "ml")]
fn test_dataset_configuration() {
    let files = vec![PathBuf::from("test.tif")];

    let dataset = GeoTiffDataset::new(files, (128, 128))
        .expect("Failed to create dataset")
        .with_channels(4)
        .expect("Failed to set channels")
        .with_classes(10)
        .expect("Failed to set classes")
        .with_patches_per_image(20)
        .expect("Failed to set patches");

    assert_eq!(dataset.len(), 20); // 1 file * 20 patches
}

/// Test dataset validation
#[test]
fn test_dataset_validation() {
    // Empty file list should error
    let result = GeoTiffDataset::new(vec![], (256, 256));
    assert!(result.is_err(), "Empty file list should fail");

    // Zero patch size should error
    let files = vec![PathBuf::from("test.tif")];
    let result = GeoTiffDataset::new(files, (0, 256));
    assert!(result.is_err(), "Zero patch size should fail");
}

/// Test DataLoader creation
#[test]
#[cfg(feature = "ml")]
fn test_dataloader_creation() {
    use oxigdal_ml_foundation::training::training_loop::Dataset;
    use std::sync::Arc;

    struct MockDataset {
        size: usize,
    }

    impl Dataset for MockDataset {
        fn len(&self) -> usize {
            self.size
        }

        fn get_batch(
            &self,
            indices: &[usize],
        ) -> oxigdal_ml_foundation::Result<(Vec<f32>, Vec<f32>)> {
            let batch_size = indices.len();
            let inputs = vec![0.0f32; batch_size * 10];
            let targets = vec![0.0f32; batch_size * 10];
            Ok((inputs, targets))
        }

        fn shapes(&self) -> (Vec<usize>, Vec<usize>) {
            (vec![1, 10], vec![1, 10])
        }
    }

    let dataset = Arc::new(MockDataset { size: 100 });
    let loader = DataLoader::new(dataset, 8, false).expect("Failed to create data loader");

    assert_eq!(loader.batch_size(), 8);
    assert_eq!(loader.num_batches(), 13); // ceil(100/8)
}

/// Test DataLoader shuffling
#[test]
#[cfg(feature = "ml")]
fn test_dataloader_shuffling() {
    use oxigdal_ml_foundation::training::training_loop::Dataset;
    use std::sync::Arc;

    struct MockDataset {
        size: usize,
    }

    impl Dataset for MockDataset {
        fn len(&self) -> usize {
            self.size
        }

        fn get_batch(
            &self,
            indices: &[usize],
        ) -> oxigdal_ml_foundation::Result<(Vec<f32>, Vec<f32>)> {
            let batch_size = indices.len();
            Ok((vec![0.0; batch_size * 10], vec![0.0; batch_size * 10]))
        }

        fn shapes(&self) -> (Vec<usize>, Vec<usize>) {
            (vec![1, 10], vec![1, 10])
        }
    }

    let dataset = Arc::new(MockDataset { size: 50 });
    let loader = DataLoader::new(dataset, 10, true).expect("Failed to create data loader");

    // Should be able to iterate
    let mut count = 0;
    for _batch in loader.iter() {
        count += 1;
    }
    assert_eq!(count, 5); // 50 samples / 10 batch_size = 5 batches
}

/// Test training configuration validation
#[test]
fn test_training_config_validation() {
    let valid_config = TrainingConfig::default();
    assert!(
        valid_config.validate().is_ok(),
        "Default config should be valid"
    );

    let mut invalid_config = TrainingConfig {
        learning_rate: -0.001,
        ..TrainingConfig::default()
    };
    assert!(
        invalid_config.validate().is_err(),
        "Negative LR should be invalid"
    );

    invalid_config.learning_rate = 0.001;
    invalid_config.batch_size = 0;
    assert!(
        invalid_config.validate().is_err(),
        "Zero batch size should be invalid"
    );
}

/// Test training history tracking
#[test]
fn test_training_history() {
    use oxigdal_ml_foundation::training::EpochStats;

    let mut history = TrainingHistory::new();

    // Add some epochs
    for i in 0..5 {
        history.add_epoch(EpochStats {
            epoch: i,
            train_loss: 1.0 - (i as f64 * 0.1),
            val_loss: Some(0.9 - (i as f64 * 0.1)),
            train_accuracy: Some(0.7 + (i as f64 * 0.05)),
            val_accuracy: Some(0.75 + (i as f64 * 0.04)),
            learning_rate: 0.001,
            epoch_time: 10.0,
        });
    }

    assert_eq!(history.epochs.len(), 5);

    // Check best val loss
    let (best_epoch, best_loss) = history.best_val_loss().expect("Should have best val loss");
    assert_eq!(best_epoch, 4);
    assert!((best_loss - 0.5).abs() < 1e-6);

    // Check best val accuracy
    let (best_epoch, _best_acc) = history
        .best_val_accuracy()
        .expect("Should have best val accuracy");
    assert_eq!(best_epoch, 4);
}

/// Test training history improvement tracking
#[test]
fn test_is_improving() {
    use oxigdal_ml_foundation::training::EpochStats;

    let mut history = TrainingHistory::new();

    // Add improving epochs
    for i in 0..5 {
        history.add_epoch(EpochStats {
            epoch: i,
            train_loss: 1.0 - (i as f64 * 0.1),
            val_loss: Some(1.0 - (i as f64 * 0.1)),
            train_accuracy: None,
            val_accuracy: None,
            learning_rate: 0.001,
            epoch_time: 10.0,
        });
    }

    assert!(
        history.is_improving(3),
        "Should be improving with patience=3"
    );

    // Add non-improving epochs
    for i in 5..10 {
        history.add_epoch(EpochStats {
            epoch: i,
            train_loss: 0.6,
            val_loss: Some(0.6),
            train_accuracy: None,
            val_accuracy: None,
            learning_rate: 0.001,
            epoch_time: 10.0,
        });
    }

    assert!(
        !history.is_improving(3),
        "Should not be improving after 5 epochs without improvement"
    );
}

/// Test early stopping
#[test]
fn test_early_stopping() {
    use oxigdal_ml_foundation::training::early_stopping::{Criterion, EarlyStopping};

    let mut es = EarlyStopping::new(Criterion::ValidationLoss, 3, 0.01)
        .expect("Failed to create early stopping");

    // Improving losses
    assert!(es.update(1.0), "Should continue");
    assert!(es.update(0.9), "Should continue");
    assert!(es.update(0.8), "Should continue");

    // No improvement for 3 epochs
    assert!(es.update(0.85), "Should continue (1/3)");
    assert!(es.update(0.84), "Should continue (2/3)");
    assert!(es.update(0.83), "Should continue (3/3)");

    // Should stop now
    assert!(!es.update(0.82), "Should stop");
    assert!(es.should_stop(), "Should be stopped");
}

/// Test checkpointing configuration
#[test]
fn test_checkpoint_manager() {
    use oxigdal_ml_foundation::training::checkpointing::CheckpointManager;
    use std::env;

    let checkpoint_dir = env::temp_dir().join("oxigdal_test_checkpoints");
    let manager = CheckpointManager::new(checkpoint_dir.clone(), None, true)
        .expect("Failed to create checkpoint manager");

    assert!(manager.save_best_only);

    // Test should_save logic
    let mut manager = CheckpointManager::new(checkpoint_dir, None, true)
        .expect("Failed to create checkpoint manager");

    assert!(
        manager.should_save(Some(1.0)),
        "Should save first checkpoint"
    );
    assert!(
        manager.should_save(Some(0.9)),
        "Should save better checkpoint"
    );
    assert!(
        !manager.should_save(Some(1.1)),
        "Should not save worse checkpoint"
    );
}

/// Test loss function computation
#[test]
fn test_loss_functions() {
    use oxigdal_ml_foundation::training::losses::LossFunctionType;

    let loss_fn = LossFunctionType::MSE;

    // Simple test with 2D data
    let outputs = vec![1.0, 2.0, 3.0, 4.0];
    let targets = vec![1.5, 2.5, 3.5, 4.5];
    let shape = vec![2, 2];

    let result = loss_fn.compute(&outputs, &targets, &shape);
    assert!(result.is_ok(), "MSE computation should succeed");

    let loss = result.expect("Should have loss value");
    assert!(loss > 0.0, "Loss should be positive");
    assert!((loss - 0.25).abs() < 1e-5, "MSE should be 0.25");
}

/// Test gradient computation
#[test]
fn test_loss_gradients() {
    use oxigdal_ml_foundation::training::losses::LossFunctionType;

    let loss_fn = LossFunctionType::MSE;

    let outputs = vec![1.0, 2.0, 3.0, 4.0];
    let targets = vec![1.5, 2.5, 3.5, 4.5];
    let shape = vec![2, 2];

    let result = loss_fn.backward(&outputs, &targets, &shape);
    assert!(result.is_ok(), "Gradient computation should succeed");

    let gradients = result.expect("Should have gradients");
    assert_eq!(gradients.len(), 4, "Should have 4 gradients");
}

/// Test model configuration for testing
#[test]
fn test_config_for_testing() {
    let config = TrainingConfig::for_testing();

    assert_eq!(config.batch_size, 4);
    assert_eq!(config.num_epochs, 2);
    assert!(config.validate().is_ok());
}

/// Test LSTM configuration
#[test]
fn test_lstm_config() {
    use oxigdal_ml_foundation::models::lstm::LSTMConfig;

    let config = LSTMConfig::new(10, 128, 2, 0.2);
    assert!(config.validate().is_ok());

    let bidirectional = config.with_bidirectional(true);
    assert!(bidirectional.bidirectional);
}

/// Test Transformer configuration
#[test]
fn test_transformer_config() {
    use oxigdal_ml_foundation::models::transformer::TransformerConfig;

    // Valid config
    let config = TransformerConfig::new(10, 128, 4, 8, 512, 0.1, 100);
    assert!(config.validate().is_ok());

    // Invalid: hidden_dim not divisible by num_heads
    let invalid = TransformerConfig::new(10, 127, 4, 8, 512, 0.1, 100);
    assert!(invalid.validate().is_err());
}

/// Test Transformer positional encoding
#[test]
fn test_transformer_positional_encoding() {
    use oxigdal_ml_foundation::models::transformer::TemporalTransformer;

    let config = oxigdal_ml_foundation::models::transformer::TransformerConfig {
        input_dim: 10,
        hidden_dim: 128,
        num_layers: 2,
        num_heads: 4,
        ff_dim: 512,
        dropout: 0.1,
        max_seq_len: 100,
    };

    let result = TemporalTransformer::new(config);
    assert!(result.is_ok(), "Transformer creation should succeed");

    let transformer = result.expect("Should have transformer");
    assert!(
        transformer.num_parameters() > 0,
        "Should have trainable parameters"
    );
}
