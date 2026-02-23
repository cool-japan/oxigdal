//! Knowledge distillation for model compression
//!
//! Knowledge distillation transfers knowledge from a large "teacher" model
//! to a smaller "student" model, maintaining accuracy while reducing size.
//!
//! # Overview
//!
//! Knowledge distillation uses soft probability distributions from a teacher model
//! to train a student model. The soft targets contain "dark knowledge" about class
//! relationships that hard labels cannot capture.
//!
//! # Example
//!
//! ```no_run
//! use oxigdal_ml::optimization::distillation::*;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create configuration
//! let config = DistillationConfig::builder()
//!     .loss(DistillationLoss::KLDivergence)
//!     .temperature(3.0)
//!     .alpha(0.7)  // 70% distillation loss, 30% hard label loss
//!     .epochs(100)
//!     .learning_rate(0.001)
//!     .batch_size(32)
//!     .build();
//!
//! // Create trainer
//! let trainer = DistillationTrainer::new(config);
//!
//! // Prepare data
//! let teacher_outputs = vec![vec![1.0, 2.0, 0.5]];
//! let training_inputs = vec![vec![0.1, 0.2, 0.3]];
//! let training_labels = vec![1usize];
//! let initial_student_weights: Vec<f32> = vec![];
//!
//! // Train
//! let stats = trainer.train_with_teacher_outputs(
//!     &teacher_outputs,
//!     &training_inputs,
//!     &training_labels,
//!     &initial_student_weights,
//! )?;
//!
//! println!("Final accuracy: {:.2}%", stats.final_accuracy);
//! # Ok(())
//! # }
//! ```

mod config;
mod math;
mod network;
mod optimizer;
mod trainer;

// Re-export all public types
pub use config::{
    DistillationConfig, DistillationConfigBuilder, DistillationLoss, EarlyStopping,
    LearningRateSchedule, OptimizerType, Temperature,
};

pub use math::{
    cross_entropy_loss, cross_entropy_with_label, kl_divergence, kl_divergence_from_logits,
    log_softmax, mse_loss, soft_targets, softmax,
};

pub use network::{DenseLayer, ForwardCache, MLPGradients, SimpleMLP, SimpleRng};

pub use optimizer::{
    TrainingState, adam_update, adamw_update, apply_optimizer_update, clip_gradients,
    sgd_momentum_update, sgd_update,
};

pub use trainer::{DistillationStats, DistillationTrainer, train_student_model};
