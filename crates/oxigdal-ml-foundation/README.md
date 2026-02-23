# OxiGDAL ML Foundation

Deep learning training infrastructure and model architectures for geospatial machine learning.

## Features

- **Training Infrastructure**: Complete training loops, optimizers (SGD, Adam, AdamW), loss functions (MSE, Cross-Entropy, Dice, Focal), learning rate schedulers, early stopping, and checkpointing
- **Model Architectures**: UNet for segmentation, ResNet for classification, with flexible configurations
- **Transfer Learning**: Pre-trained model loading, layer freezing strategies, fine-tuning procedures
- **Data Augmentation**: Geometric (flip, rotate, crop), color (brightness, contrast, gamma), noise, and geospatial-specific augmentations
- **Evaluation Metrics**: Accuracy, precision, recall, F1-score, IoU, confusion matrix

## COOLJAPAN Compliance

- ✅ Pure Rust implementation (PyTorch bindings are feature-gated)
- ✅ No `unwrap()` calls in production code
- ✅ All files under 2000 lines
- ✅ Uses workspace dependencies
- ✅ Uses SciRS2-Core for numerical operations

## Usage

```rust
use oxigdal_ml_foundation::{
    training::{TrainingConfig, optimizers::Adam, losses::CrossEntropyLoss},
    models::unet::UNet,
    augmentation::{AugmentationPipeline, geometric::HorizontalFlip},
};

// Create a UNet model for segmentation
let model = UNet::standard(3, 10)?;

// Configure training
let config = TrainingConfig {
    learning_rate: 0.001,
    batch_size: 16,
    num_epochs: 100,
    ..Default::default()
};

// Setup augmentation pipeline
let mut pipeline = AugmentationPipeline::new();
pipeline.add(Box::new(HorizontalFlip));

// Create optimizer and loss
let optimizer = Adam::new(0.001)?;
let loss_fn = CrossEntropyLoss::new();
```

## Cargo Features

- `std` (default): Standard library support
- `pytorch`: PyTorch backend for training (requires libtorch)
- `onnx`: ONNX export support
- `cuda`: GPU acceleration (requires CUDA)

## Architecture

### Training Module (`training/`)

- **mod.rs**: Training configuration and history
- **training_loop.rs**: Core training loop implementation
- **losses.rs**: Loss functions (MSE, Cross-Entropy, Dice, Focal, Combined)
- **optimizers.rs**: Optimization algorithms (SGD, Adam, AdamW)
- **schedulers.rs**: Learning rate schedulers (Step, Exponential, Cosine, OneCycle)
- **early_stopping.rs**: Early stopping logic
- **checkpointing.rs**: Model checkpoint management

### Models Module (`models/`)

- **unet.rs**: UNet architecture for segmentation
- **resnet.rs**: ResNet variants (18, 34, 50, 101, 152)
- **layers.rs**: Common layers (Conv2D, BatchNorm, Pooling, Residual blocks)

### Transfer Learning Module (`transfer/`)

- **pretrained.rs**: Pre-trained model loading
- **freezing.rs**: Layer freezing strategies
- **finetuning.rs**: Fine-tuning procedures
- **feature_extraction.rs**: Feature extraction utilities

### Augmentation Module (`augmentation/`)

- **geometric.rs**: Flip, rotate, crop transformations
- **color.rs**: Brightness, contrast, gamma adjustments
- **noise.rs**: Gaussian noise, channel dropout
- **geospatial.rs**: Band selection, spectral normalization

## Examples

### Training Configuration

```rust
let config = TrainingConfig {
    learning_rate: 0.001,
    batch_size: 32,
    num_epochs: 100,
    weight_decay: 0.0001,
    gradient_clip: Some(1.0),
    mixed_precision: true,
    ..Default::default()
};
```

### Model Creation

```rust
// UNet variants
let small_unet = UNet::small(3, 10)?;
let standard_unet = UNet::standard(3, 10)?;
let deep_unet = UNet::deep(3, 10)?;

// ResNet variants
let resnet18 = ResNet::resnet18(3, 1000)?;
let resnet50 = ResNet::resnet50(3, 1000)?;
```

### Data Augmentation Pipeline

```rust
let mut pipeline = AugmentationPipeline::new();
pipeline
    .add(Box::new(HorizontalFlip))
    .add(Box::new(VerticalFlip))
    .add(Box::new(Brightness::new(1.2)?))
    .add(Box::new(GaussianNoise::new(0.0, 0.1)?));

let augmented = pipeline.apply(&image)?;
```

### Transfer Learning

```rust
let config = FineTuningConfig::fine_tune_top(5, 1e-4);
let freezer = LayerFreezer::new(config.freezing, total_layers)?;

// Check which layers are trainable
for idx in 0..total_layers {
    if !freezer.is_layer_frozen(idx) {
        println!("Layer {} is trainable", idx);
    }
}
```

## Testing

Run tests:
```bash
cargo test --all-features
```

Run benchmarks:
```bash
cargo bench
```

## License

Apache-2.0

## Authors

COOLJAPAN OU (Team Kitasan)
