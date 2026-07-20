//! Optimizer implementations for knowledge distillation

use super::config::{EarlyStopping, LearningRateSchedule, OptimizerType};

/// Training state for tracking optimizer momentum and history
#[derive(Debug, Clone)]
pub struct TrainingState {
    /// Current epoch
    pub epoch: usize,
    /// Current batch within epoch
    pub batch: usize,
    /// Total batches processed
    pub total_batches: usize,
    /// Current learning rate
    pub current_lr: f32,
    /// Best validation loss seen
    pub best_val_loss: f32,
    /// Epochs since improvement (for early stopping)
    pub epochs_without_improvement: usize,
    /// Momentum buffer for SGD with momentum
    pub momentum_buffer: Vec<f32>,
    /// First moment estimate for Adam (m)
    pub adam_m: Vec<f32>,
    /// Second moment estimate for Adam (v)
    pub adam_v: Vec<f32>,
    /// Adam timestep
    pub adam_t: usize,
    /// Training loss history
    pub train_loss_history: Vec<f32>,
    /// Validation loss history
    pub val_loss_history: Vec<f32>,
    /// Training accuracy history
    pub train_acc_history: Vec<f32>,
    /// Validation accuracy history
    pub val_acc_history: Vec<f32>,
}

impl TrainingState {
    /// Creates a new training state
    #[must_use]
    pub fn new(num_params: usize, initial_lr: f32) -> Self {
        Self {
            epoch: 0,
            batch: 0,
            total_batches: 0,
            current_lr: initial_lr,
            best_val_loss: f32::MAX,
            epochs_without_improvement: 0,
            momentum_buffer: vec![0.0; num_params],
            adam_m: vec![0.0; num_params],
            adam_v: vec![0.0; num_params],
            adam_t: 0,
            train_loss_history: Vec::new(),
            val_loss_history: Vec::new(),
            train_acc_history: Vec::new(),
            val_acc_history: Vec::new(),
        }
    }

    /// Updates learning rate based on schedule
    pub fn update_learning_rate(
        &mut self,
        base_lr: f32,
        schedule: &LearningRateSchedule,
        total_epochs: usize,
    ) {
        self.current_lr = match schedule {
            LearningRateSchedule::Constant => base_lr,
            LearningRateSchedule::StepDecay {
                decay_factor,
                step_size,
            } => {
                let num_decays = self.epoch / step_size;
                base_lr * decay_factor.powi(num_decays as i32)
            }
            LearningRateSchedule::CosineAnnealing { min_lr } => {
                let progress = self.epoch as f32 / total_epochs as f32;
                let cos_value = (std::f32::consts::PI * progress).cos();
                min_lr + (base_lr - min_lr) * (1.0 + cos_value) / 2.0
            }
            LearningRateSchedule::WarmupDecay {
                warmup_epochs,
                decay_factor,
            } => {
                if self.epoch < *warmup_epochs {
                    base_lr * (self.epoch + 1) as f32 / *warmup_epochs as f32
                } else {
                    let epochs_after_warmup = self.epoch - warmup_epochs;
                    base_lr * decay_factor.powi(epochs_after_warmup as i32)
                }
            }
        };
    }

    /// Checks if early stopping should trigger
    pub fn should_stop(&self, config: &Option<EarlyStopping>) -> bool {
        if let Some(es) = config {
            self.epochs_without_improvement >= es.patience
        } else {
            false
        }
    }

    /// Updates early stopping state based on validation loss
    pub fn update_early_stopping(&mut self, val_loss: f32, config: &Option<EarlyStopping>) {
        if let Some(es) = config {
            if val_loss < self.best_val_loss - es.min_delta {
                self.best_val_loss = val_loss;
                self.epochs_without_improvement = 0;
            } else {
                self.epochs_without_improvement += 1;
            }
        }
    }
}

/// Applies SGD update to weights
pub fn sgd_update(weights: &mut [f32], gradients: &[f32], lr: f32) {
    for (w, g) in weights.iter_mut().zip(gradients.iter()) {
        *w -= lr * g;
    }
}

/// Applies SGD with momentum update to weights
pub fn sgd_momentum_update(
    weights: &mut [f32],
    gradients: &[f32],
    momentum_buffer: &mut [f32],
    lr: f32,
    momentum: f32,
) {
    for ((w, g), m) in weights
        .iter_mut()
        .zip(gradients.iter())
        .zip(momentum_buffer.iter_mut())
    {
        *m = momentum * *m + g;
        *w -= lr * *m;
    }
}

/// Adam optimizer parameters
#[derive(Debug, Clone, Copy)]
pub struct AdamParams {
    /// Learning rate
    pub lr: f32,
    /// Beta1 parameter (first moment decay)
    pub beta1: f32,
    /// Beta2 parameter (second moment decay)
    pub beta2: f32,
    /// Epsilon for numerical stability
    pub epsilon: f32,
}

impl Default for AdamParams {
    fn default() -> Self {
        Self {
            lr: 0.001,
            beta1: 0.9,
            beta2: 0.999,
            epsilon: 1e-8,
        }
    }
}

/// Applies Adam optimizer update to weights
#[allow(clippy::too_many_arguments)]
pub fn adam_update(
    weights: &mut [f32],
    gradients: &[f32],
    m: &mut [f32],
    v: &mut [f32],
    t: usize,
    lr: f32,
    beta1: f32,
    beta2: f32,
    epsilon: f32,
) {
    let bias_correction1 = 1.0 - beta1.powi(t as i32);
    let bias_correction2 = 1.0 - beta2.powi(t as i32);

    for i in 0..weights.len() {
        // Update biased first moment estimate
        m[i] = beta1 * m[i] + (1.0 - beta1) * gradients[i];
        // Update biased second raw moment estimate
        v[i] = beta2 * v[i] + (1.0 - beta2) * gradients[i].powi(2);

        // Compute bias-corrected estimates
        let m_hat = m[i] / bias_correction1;
        let v_hat = v[i] / bias_correction2;

        // Update weights
        weights[i] -= lr * m_hat / (v_hat.sqrt() + epsilon);
    }
}

/// Applies AdamW optimizer update with decoupled weight decay
#[allow(clippy::too_many_arguments)]
pub fn adamw_update(
    weights: &mut [f32],
    gradients: &[f32],
    m: &mut [f32],
    v: &mut [f32],
    t: usize,
    lr: f32,
    beta1: f32,
    beta2: f32,
    epsilon: f32,
    weight_decay: f32,
) {
    let bias_correction1 = 1.0 - beta1.powi(t as i32);
    let bias_correction2 = 1.0 - beta2.powi(t as i32);

    for i in 0..weights.len() {
        // Decoupled weight decay
        weights[i] -= lr * weight_decay * weights[i];

        // Update biased first moment estimate
        m[i] = beta1 * m[i] + (1.0 - beta1) * gradients[i];
        // Update biased second raw moment estimate
        v[i] = beta2 * v[i] + (1.0 - beta2) * gradients[i].powi(2);

        // Compute bias-corrected estimates
        let m_hat = m[i] / bias_correction1;
        let v_hat = v[i] / bias_correction2;

        // Update weights
        weights[i] -= lr * m_hat / (v_hat.sqrt() + epsilon);
    }
}

/// Clips gradients by global norm
pub fn clip_gradients(gradients: &mut [f32], max_norm: f32) {
    let total_norm: f32 = gradients.iter().map(|g| g.powi(2)).sum::<f32>().sqrt();

    if total_norm > max_norm {
        let scale = max_norm / (total_norm + 1e-6);
        for g in gradients.iter_mut() {
            *g *= scale;
        }
    }
}

/// Applies optimizer update based on configuration
pub fn apply_optimizer_update(
    params: &mut [f32],
    gradients: &[f32],
    state: &mut TrainingState,
    optimizer: &OptimizerType,
) {
    match optimizer {
        OptimizerType::SGD => {
            sgd_update(params, gradients, state.current_lr);
        }
        OptimizerType::SGDMomentum { momentum } => {
            let momentum_f = *momentum as f32 / 100.0;
            sgd_momentum_update(
                params,
                gradients,
                &mut state.momentum_buffer,
                state.current_lr,
                momentum_f,
            );
        }
        OptimizerType::Adam => {
            state.adam_t += 1;
            adam_update(
                params,
                gradients,
                &mut state.adam_m,
                &mut state.adam_v,
                state.adam_t,
                state.current_lr,
                0.9,
                0.999,
                1e-8,
            );
        }
        OptimizerType::AdamW { weight_decay } => {
            state.adam_t += 1;
            let wd = *weight_decay as f32 / 100.0;
            adamw_update(
                params,
                gradients,
                &mut state.adam_m,
                &mut state.adam_v,
                state.adam_t,
                state.current_lr,
                0.9,
                0.999,
                1e-8,
                wd,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gradient_clipping() {
        let mut grads = vec![10.0, 20.0, 30.0];
        clip_gradients(&mut grads, 1.0);

        let norm: f32 = grads.iter().map(|g| g.powi(2)).sum::<f32>().sqrt();
        assert!(norm <= 1.0 + 1e-6);
    }

    #[test]
    fn test_optimizer_sgd() {
        let mut weights = vec![1.0, 2.0, 3.0];
        let gradients = vec![0.1, 0.2, 0.3];

        sgd_update(&mut weights, &gradients, 0.1);

        assert!((weights[0] - 0.99).abs() < 1e-6);
        assert!((weights[1] - 1.98).abs() < 1e-6);
        assert!((weights[2] - 2.97).abs() < 1e-6);
    }

    #[test]
    fn test_optimizer_adam() {
        let mut weights = vec![1.0, 2.0, 3.0];
        let gradients = vec![0.1, 0.2, 0.3];
        let mut m = vec![0.0; 3];
        let mut v = vec![0.0; 3];

        adam_update(
            &mut weights,
            &gradients,
            &mut m,
            &mut v,
            1,
            0.001,
            0.9,
            0.999,
            1e-8,
        );

        assert!(weights[0] < 1.0);
        assert!(weights[1] < 2.0);
        assert!(weights[2] < 3.0);
    }

    #[test]
    fn test_training_state_lr_schedule() {
        let mut state = TrainingState::new(100, 0.1);

        state.epoch = 50;
        state.update_learning_rate(0.1, &LearningRateSchedule::Constant, 100);
        assert!((state.current_lr - 0.1).abs() < 1e-6);

        state.update_learning_rate(
            0.1,
            &LearningRateSchedule::StepDecay {
                decay_factor: 0.5,
                step_size: 10,
            },
            100,
        );
        assert!((state.current_lr - 0.003125).abs() < 1e-6);

        state.epoch = 50;
        state.update_learning_rate(
            0.1,
            &LearningRateSchedule::CosineAnnealing { min_lr: 0.0 },
            100,
        );
        assert!(state.current_lr > 0.0 && state.current_lr < 0.1);
    }

    #[test]
    fn test_early_stopping() {
        let mut state = TrainingState::new(100, 0.1);
        let early_stopping = Some(EarlyStopping {
            patience: 3,
            min_delta: 0.01,
        });

        assert!(!state.should_stop(&early_stopping));

        state.update_early_stopping(1.0, &early_stopping);
        assert_eq!(state.epochs_without_improvement, 0);

        state.update_early_stopping(1.0, &early_stopping);
        assert_eq!(state.epochs_without_improvement, 1);

        state.update_early_stopping(0.995, &early_stopping);
        assert_eq!(state.epochs_without_improvement, 2);

        state.update_early_stopping(1.0, &early_stopping);
        assert_eq!(state.epochs_without_improvement, 3);

        assert!(state.should_stop(&early_stopping));
    }
}
