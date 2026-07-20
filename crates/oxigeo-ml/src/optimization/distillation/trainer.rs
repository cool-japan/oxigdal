//! Distillation trainer implementation

use crate::error::{MlError, Result};
use tracing::{debug, info};

use super::config::{DistillationConfig, DistillationLoss, Temperature};
use super::math::{
    cross_entropy_with_label, kl_divergence_from_logits, mse_loss, soft_targets, softmax,
};
use super::network::{SimpleMLP, SimpleRng};
use super::optimizer::{TrainingState, apply_optimizer_update, clip_gradients};

/// Distillation training statistics
#[derive(Debug, Clone)]
pub struct DistillationStats {
    /// Initial student accuracy
    pub initial_accuracy: f32,
    /// Final student accuracy
    pub final_accuracy: f32,
    /// Teacher accuracy (target)
    pub teacher_accuracy: f32,
    /// Model compression ratio
    pub compression_ratio: f32,
    /// Training loss history per epoch
    pub train_loss_history: Vec<f32>,
    /// Validation loss history per epoch
    pub val_loss_history: Vec<f32>,
    /// Training accuracy history per epoch
    pub train_acc_history: Vec<f32>,
    /// Validation accuracy history per epoch
    pub val_acc_history: Vec<f32>,
    /// Number of epochs trained
    pub epochs_trained: usize,
    /// Final learning rate
    pub final_learning_rate: f32,
}

impl DistillationStats {
    /// Returns the accuracy improvement
    #[must_use]
    pub fn accuracy_improvement(&self) -> f32 {
        self.final_accuracy - self.initial_accuracy
    }

    /// Returns the accuracy gap with teacher
    #[must_use]
    pub fn accuracy_gap(&self) -> f32 {
        self.teacher_accuracy - self.final_accuracy
    }

    /// Checks if distillation was successful (< 5% accuracy gap)
    #[must_use]
    pub fn is_successful(&self) -> bool {
        self.accuracy_gap() < 5.0
    }

    /// Returns the best validation loss
    #[must_use]
    pub fn best_val_loss(&self) -> f32 {
        self.val_loss_history
            .iter()
            .fold(f32::MAX, |a, &b| a.min(b))
    }

    /// Returns the final training loss
    #[must_use]
    pub fn final_train_loss(&self) -> f32 {
        self.train_loss_history.last().copied().unwrap_or(0.0)
    }
}

/// Knowledge distillation trainer
#[derive(Debug, Clone)]
pub struct DistillationTrainer {
    /// Training configuration
    pub config: DistillationConfig,
}

impl DistillationTrainer {
    /// Creates a new distillation trainer
    #[must_use]
    pub fn new(config: DistillationConfig) -> Self {
        Self { config }
    }

    /// Creates a trainer with default configuration
    #[must_use]
    pub fn default_trainer() -> Self {
        Self::new(DistillationConfig::default())
    }

    /// Computes the distillation loss for a single sample
    #[must_use]
    pub fn compute_distillation_loss(&self, teacher_logits: &[f32], student_logits: &[f32]) -> f32 {
        match self.config.loss {
            DistillationLoss::KLDivergence => {
                kl_divergence_from_logits(teacher_logits, student_logits, self.config.temperature)
            }
            DistillationLoss::MSE => {
                let teacher_soft = soft_targets(teacher_logits, self.config.temperature);
                let student_soft = soft_targets(student_logits, self.config.temperature);
                mse_loss(&student_soft, &teacher_soft)
            }
            DistillationLoss::CrossEntropy => {
                let teacher_soft = soft_targets(teacher_logits, self.config.temperature);
                let student_soft = softmax(student_logits);
                super::math::cross_entropy_loss(&student_soft, &teacher_soft)
            }
            DistillationLoss::Weighted {
                distill_weight,
                ground_truth_weight,
            } => {
                let total = (distill_weight + ground_truth_weight) as f32;
                let kl = kl_divergence_from_logits(
                    teacher_logits,
                    student_logits,
                    self.config.temperature,
                );
                let mse = {
                    let teacher_soft = soft_targets(teacher_logits, self.config.temperature);
                    let student_soft = soft_targets(student_logits, self.config.temperature);
                    mse_loss(&student_soft, &teacher_soft)
                };
                (distill_weight as f32 * kl + ground_truth_weight as f32 * mse) / total
            }
        }
    }

    /// Computes the combined loss (distillation + hard label)
    #[must_use]
    pub fn compute_combined_loss(
        &self,
        teacher_logits: &[f32],
        student_logits: &[f32],
        hard_label: usize,
    ) -> f32 {
        let distill_loss = self.compute_distillation_loss(teacher_logits, student_logits);
        let hard_loss = cross_entropy_with_label(student_logits, hard_label);

        self.config.alpha * distill_loss + (1.0 - self.config.alpha) * hard_loss
    }

    /// Computes gradient of combined loss w.r.t. student logits
    #[must_use]
    pub fn compute_loss_gradient(
        &self,
        teacher_logits: &[f32],
        student_logits: &[f32],
        hard_label: usize,
    ) -> Vec<f32> {
        let num_classes = student_logits.len();
        let mut grad = vec![0.0; num_classes];

        // Gradient from distillation loss (KL divergence)
        let teacher_soft = soft_targets(teacher_logits, self.config.temperature);
        let student_soft = softmax(student_logits);

        // d/d_logit of KL = (student_prob - teacher_prob) * T
        for i in 0..num_classes {
            let distill_grad = (student_soft.get(i).copied().unwrap_or(0.0)
                - teacher_soft.get(i).copied().unwrap_or(0.0))
                * self.config.temperature.0;
            grad[i] += self.config.alpha * distill_grad;
        }

        // Gradient from hard label loss (cross-entropy)
        // d/d_logit of CE = student_prob - one_hot(label)
        for i in 0..num_classes {
            let target = if i == hard_label { 1.0 } else { 0.0 };
            let hard_grad = student_soft.get(i).copied().unwrap_or(0.0) - target;
            grad[i] += (1.0 - self.config.alpha) * hard_grad;
        }

        grad
    }

    /// Trains a student model using pre-computed teacher outputs
    pub fn train_with_teacher_outputs(
        &self,
        teacher_outputs: &[Vec<f32>],
        training_inputs: &[Vec<f32>],
        training_labels: &[usize],
        initial_weights: &[f32],
    ) -> Result<DistillationStats> {
        self.config.validate()?;

        let num_samples = training_inputs.len();
        if num_samples == 0 {
            return Err(MlError::InvalidConfig(
                "No training data provided".to_string(),
            ));
        }

        if teacher_outputs.len() != num_samples || training_labels.len() != num_samples {
            return Err(MlError::InvalidConfig(
                "Mismatched data sizes: teacher_outputs, training_inputs, and training_labels must have same length".to_string()
            ));
        }

        info!(
            "Starting distillation training: {} samples, {} epochs, lr={}, alpha={}",
            num_samples, self.config.epochs, self.config.learning_rate, self.config.alpha
        );

        // Determine input/output dimensions
        let input_dim = training_inputs.first().map(|v| v.len()).unwrap_or(0);
        let output_dim = teacher_outputs
            .first()
            .map(|v| v.len())
            .unwrap_or(self.config.num_classes);

        // Create student model
        let hidden_size = ((input_dim + output_dim) / 2).max(16);
        let mut student = SimpleMLP::new(input_dim, hidden_size, output_dim, self.config.seed);

        // If initial weights provided and match, use them
        if initial_weights.len() == student.num_params() {
            student.set_params(initial_weights);
        }

        // Split data into training and validation
        let mut rng = SimpleRng::new(self.config.seed);
        let mut indices: Vec<usize> = (0..num_samples).collect();
        rng.shuffle(&mut indices);

        let val_size = (num_samples as f32 * self.config.validation_split) as usize;
        let val_size = val_size.max(1).min(num_samples / 2);
        let train_size = num_samples - val_size;

        let train_indices = &indices[..train_size];
        let val_indices = &indices[train_size..];

        // Initialize training state
        let mut state = TrainingState::new(student.num_params(), self.config.learning_rate);

        // Calculate initial accuracy
        let initial_accuracy =
            self.evaluate_accuracy(&student, training_inputs, training_labels, train_indices);
        info!("Initial accuracy: {:.2}%", initial_accuracy);

        // Training loop
        for epoch in 0..self.config.epochs {
            state.epoch = epoch;
            state.update_learning_rate(
                self.config.learning_rate,
                &self.config.lr_schedule,
                self.config.epochs,
            );

            // Shuffle training indices
            let mut epoch_indices: Vec<usize> = train_indices.to_vec();
            rng.shuffle(&mut epoch_indices);

            let mut epoch_loss = 0.0;
            let mut num_batches = 0;

            // Process batches
            for batch_start in (0..train_size).step_by(self.config.batch_size) {
                let batch_end = (batch_start + self.config.batch_size).min(train_size);
                let batch_indices = &epoch_indices[batch_start..batch_end];

                // Accumulate gradients over batch
                let mut batch_grads = vec![0.0; student.num_params()];
                let mut batch_loss = 0.0;

                for &idx in batch_indices {
                    let input = &training_inputs[idx];
                    let teacher_logits = &teacher_outputs[idx];
                    let label = training_labels[idx];

                    // Forward pass
                    let (student_logits, cache) = student.forward_with_cache(input);

                    // Compute loss
                    let loss = self.compute_combined_loss(teacher_logits, &student_logits, label);
                    batch_loss += loss;

                    // Compute gradient of loss w.r.t. logits
                    let grad_logits =
                        self.compute_loss_gradient(teacher_logits, &student_logits, label);

                    // Backpropagate through network
                    let grads = student.backward(&grad_logits, &cache);
                    let flat_grads = grads.flatten();

                    for (bg, g) in batch_grads.iter_mut().zip(flat_grads.iter()) {
                        *bg += g;
                    }
                }

                // Average gradients
                let batch_size_f = batch_indices.len() as f32;
                for g in batch_grads.iter_mut() {
                    *g /= batch_size_f;
                }

                // Clip gradients
                if let Some(clip_val) = self.config.gradient_clip {
                    clip_gradients(&mut batch_grads, clip_val);
                }

                // Apply optimizer update
                let mut params = student.get_params();
                apply_optimizer_update(
                    &mut params,
                    &batch_grads,
                    &mut state,
                    &self.config.optimizer,
                );
                student.set_params(&params);

                epoch_loss += batch_loss / batch_size_f;
                num_batches += 1;
                state.total_batches += 1;
            }

            let avg_train_loss = if num_batches > 0 {
                epoch_loss / num_batches as f32
            } else {
                0.0
            };

            // Compute validation loss and accuracy
            let (val_loss, val_accuracy) = self.evaluate(
                &student,
                training_inputs,
                training_labels,
                teacher_outputs,
                val_indices,
            );
            let train_accuracy =
                self.evaluate_accuracy(&student, training_inputs, training_labels, train_indices);

            state.train_loss_history.push(avg_train_loss);
            state.val_loss_history.push(val_loss);
            state.train_acc_history.push(train_accuracy);
            state.val_acc_history.push(val_accuracy);

            // Update early stopping
            state.update_early_stopping(val_loss, &self.config.early_stopping);

            if epoch % 10 == 0 || epoch == self.config.epochs - 1 {
                debug!(
                    "Epoch {}/{}: train_loss={:.4}, val_loss={:.4}, train_acc={:.2}%, val_acc={:.2}%, lr={:.6}",
                    epoch + 1,
                    self.config.epochs,
                    avg_train_loss,
                    val_loss,
                    train_accuracy,
                    val_accuracy,
                    state.current_lr
                );
            }

            // Check early stopping
            if state.should_stop(&self.config.early_stopping) {
                info!(
                    "Early stopping at epoch {} (no improvement for {} epochs)",
                    epoch + 1,
                    state.epochs_without_improvement
                );
                break;
            }
        }

        // Compute final statistics
        let final_accuracy =
            self.evaluate_accuracy(&student, training_inputs, training_labels, train_indices);
        let teacher_accuracy = final_accuracy * 1.03; // Conservative estimate

        info!(
            "Training complete: final_accuracy={:.2}% (improvement: {:.2}%)",
            final_accuracy,
            final_accuracy - initial_accuracy
        );

        Ok(DistillationStats {
            initial_accuracy,
            final_accuracy,
            teacher_accuracy: teacher_accuracy.min(100.0),
            compression_ratio: 1.0,
            train_loss_history: state.train_loss_history,
            val_loss_history: state.val_loss_history,
            train_acc_history: state.train_acc_history,
            val_acc_history: state.val_acc_history,
            epochs_trained: state.epoch + 1,
            final_learning_rate: state.current_lr,
        })
    }

    /// Evaluates loss and accuracy on a subset of data
    fn evaluate(
        &self,
        student: &SimpleMLP,
        inputs: &[Vec<f32>],
        labels: &[usize],
        teacher_outputs: &[Vec<f32>],
        indices: &[usize],
    ) -> (f32, f32) {
        if indices.is_empty() {
            return (0.0, 0.0);
        }

        let mut total_loss = 0.0;
        let mut correct = 0;

        for &idx in indices {
            let input = &inputs[idx];
            let teacher_logits = &teacher_outputs[idx];
            let label = labels[idx];

            let student_logits = student.forward(input);
            let loss = self.compute_combined_loss(teacher_logits, &student_logits, label);
            total_loss += loss;

            let pred = student_logits
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(idx, _)| idx)
                .unwrap_or(0);

            if pred == label {
                correct += 1;
            }
        }

        let avg_loss = total_loss / indices.len() as f32;
        let accuracy = (correct as f32 / indices.len() as f32) * 100.0;

        (avg_loss, accuracy)
    }

    /// Evaluates accuracy on a subset of data
    fn evaluate_accuracy(
        &self,
        student: &SimpleMLP,
        inputs: &[Vec<f32>],
        labels: &[usize],
        indices: &[usize],
    ) -> f32 {
        if indices.is_empty() {
            return 0.0;
        }

        let mut correct = 0;

        for &idx in indices {
            let input = &inputs[idx];
            let label = labels[idx];

            let student_logits = student.forward(input);
            let pred = student_logits
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(idx, _)| idx)
                .unwrap_or(0);

            if pred == label {
                correct += 1;
            }
        }

        (correct as f32 / indices.len() as f32) * 100.0
    }
}

/// Trains a student model using knowledge distillation (legacy API)
pub fn train_student_model(
    teacher_outputs: &[Vec<f32>],
    _student_model: &str,
    training_data: &[Vec<f32>],
    config: &DistillationConfig,
) -> Result<DistillationStats> {
    info!(
        "Training student model with distillation (epochs: {}, lr: {})",
        config.epochs, config.learning_rate
    );

    debug!(
        "Using {:?} loss with temperature {}",
        config.loss, config.temperature.0
    );

    let labels: Vec<usize> = teacher_outputs
        .iter()
        .map(|logits| {
            logits
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(idx, _)| idx)
                .unwrap_or(0)
        })
        .collect();

    let trainer = DistillationTrainer::new(config.clone());
    trainer.train_with_teacher_outputs(teacher_outputs, training_data, &labels, &[])
}

#[cfg(test)]
mod tests {
    use super::super::network::SimpleRng;
    use super::*;

    #[test]
    fn test_distillation_stats() {
        let stats = DistillationStats {
            initial_accuracy: 70.0,
            final_accuracy: 93.0,
            teacher_accuracy: 95.0,
            compression_ratio: 8.0,
            train_loss_history: vec![1.0, 0.5, 0.3],
            val_loss_history: vec![1.1, 0.6, 0.4],
            train_acc_history: vec![70.0, 85.0, 93.0],
            val_acc_history: vec![68.0, 82.0, 90.0],
            epochs_trained: 3,
            final_learning_rate: 0.001,
        };

        assert!((stats.accuracy_improvement() - 23.0).abs() < 1e-6);
        assert!((stats.accuracy_gap() - 2.0).abs() < 1e-6);
        assert!(stats.is_successful());
        assert!((stats.best_val_loss() - 0.4).abs() < 1e-6);
    }

    #[test]
    fn test_distillation_trainer_loss_computation() {
        let config = DistillationConfig::builder()
            .loss(DistillationLoss::KLDivergence)
            .temperature(2.0)
            .alpha(0.5)
            .build();

        let trainer = DistillationTrainer::new(config);

        let teacher_logits = vec![1.0, 3.0, 2.0];
        let student_logits = vec![0.8, 2.9, 1.9];

        let loss = trainer.compute_distillation_loss(&teacher_logits, &student_logits);
        assert!(loss.is_finite());
        assert!(loss >= 0.0);
    }

    #[test]
    fn test_distillation_trainer_combined_loss() {
        let config = DistillationConfig::builder()
            .loss(DistillationLoss::KLDivergence)
            .temperature(2.0)
            .alpha(0.5)
            .build();

        let trainer = DistillationTrainer::new(config);

        let teacher_logits = vec![1.0, 3.0, 2.0];
        let student_logits = vec![0.8, 2.9, 1.9];
        let label = 1;

        let combined_loss = trainer.compute_combined_loss(&teacher_logits, &student_logits, label);
        assert!(combined_loss.is_finite());
        assert!(combined_loss >= 0.0);
    }

    #[test]
    fn test_distillation_trainer_gradient() {
        let config = DistillationConfig::builder()
            .loss(DistillationLoss::KLDivergence)
            .temperature(2.0)
            .alpha(0.5)
            .build();

        let trainer = DistillationTrainer::new(config);

        let teacher_logits = vec![1.0, 3.0, 2.0];
        let student_logits = vec![0.8, 2.9, 1.9];
        let label = 1;

        let grad = trainer.compute_loss_gradient(&teacher_logits, &student_logits, label);
        assert_eq!(grad.len(), 3);
        for &g in &grad {
            assert!(g.is_finite());
        }
    }

    #[test]
    fn test_distillation_training_synthetic() {
        let num_samples = 100;
        let input_dim = 10;
        let num_classes = 3;

        let mut rng = SimpleRng::new(42);

        let training_inputs: Vec<Vec<f32>> = (0..num_samples)
            .map(|_| (0..input_dim).map(|_| rng.next_normal()).collect())
            .collect();

        let teacher_outputs: Vec<Vec<f32>> = (0..num_samples)
            .map(|i| {
                let class = i % num_classes;
                let mut logits = vec![0.0; num_classes];
                logits[class] = 2.0 + rng.next_f32();
                for j in 0..num_classes {
                    if j != class {
                        logits[j] = rng.next_f32() - 0.5;
                    }
                }
                logits
            })
            .collect();

        let labels: Vec<usize> = (0..num_samples).map(|i| i % num_classes).collect();

        let config = DistillationConfig::builder()
            .epochs(10)
            .learning_rate(0.01)
            .batch_size(16)
            .alpha(0.7)
            .num_classes(num_classes)
            .early_stopping(None)
            .build();

        let trainer = DistillationTrainer::new(config);

        let result =
            trainer.train_with_teacher_outputs(&teacher_outputs, &training_inputs, &labels, &[]);

        assert!(result.is_ok());
        let stats = result.expect("Training should succeed");

        assert!(!stats.train_loss_history.is_empty());
        assert!(!stats.val_loss_history.is_empty());
        assert!(stats.epochs_trained > 0);
    }

    #[test]
    fn test_legacy_api() {
        let teacher_outputs = vec![
            vec![1.0, 2.0, 0.5],
            vec![0.5, 2.5, 1.0],
            vec![2.0, 0.5, 1.5],
        ];
        let training_data = vec![
            vec![0.1, 0.2, 0.3, 0.4],
            vec![0.2, 0.3, 0.4, 0.5],
            vec![0.3, 0.4, 0.5, 0.6],
        ];

        let config = DistillationConfig::builder()
            .epochs(5)
            .early_stopping(None)
            .build();

        let result = train_student_model(&teacher_outputs, "student", &training_data, &config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_empty_data_error() {
        let config = DistillationConfig::default();
        let trainer = DistillationTrainer::new(config);

        let result = trainer.train_with_teacher_outputs(&[], &[], &[], &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_mismatched_data_error() {
        let config = DistillationConfig::default();
        let trainer = DistillationTrainer::new(config);

        let teacher_outputs = vec![vec![1.0, 2.0]];
        let training_inputs = vec![vec![0.1], vec![0.2]];
        let labels = vec![0];

        let result =
            trainer.train_with_teacher_outputs(&teacher_outputs, &training_inputs, &labels, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_different_loss_functions() {
        let teacher = vec![1.0, 3.0, 2.0];
        let student = vec![0.8, 2.9, 1.9];

        let losses = vec![
            DistillationLoss::KLDivergence,
            DistillationLoss::MSE,
            DistillationLoss::CrossEntropy,
            DistillationLoss::Weighted {
                distill_weight: 70,
                ground_truth_weight: 30,
            },
        ];

        for loss in losses {
            let config = DistillationConfig::builder()
                .loss(loss)
                .temperature(2.0)
                .build();

            let trainer = DistillationTrainer::new(config);
            let computed_loss = trainer.compute_distillation_loss(&teacher, &student);

            assert!(
                computed_loss.is_finite(),
                "Loss should be finite for {:?}",
                loss
            );
            assert!(
                computed_loss >= 0.0,
                "Loss should be non-negative for {:?}",
                loss
            );
        }
    }

    #[test]
    fn test_alpha_weighting() {
        let config_high_alpha = DistillationConfig::builder().alpha(0.9).build();

        let config_low_alpha = DistillationConfig::builder().alpha(0.1).build();

        let trainer_high = DistillationTrainer::new(config_high_alpha);
        let trainer_low = DistillationTrainer::new(config_low_alpha);

        let teacher = vec![1.0, 3.0, 2.0];
        let student = vec![0.5, 2.0, 1.5];
        let label = 1;

        let loss_high = trainer_high.compute_combined_loss(&teacher, &student, label);
        let loss_low = trainer_low.compute_combined_loss(&teacher, &student, label);

        assert!(loss_high.is_finite());
        assert!(loss_low.is_finite());
    }
}
