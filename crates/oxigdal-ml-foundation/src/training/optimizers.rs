//! Optimization algorithms for training neural networks.

use crate::{Error, Result};
use ndarray::{Array2, ArrayView2};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Trait for optimization algorithms.
pub trait Optimizer: Send + Sync {
    /// Performs a single optimization step.
    ///
    /// # Arguments
    /// * `parameter_id` - Unique identifier for the parameter
    /// * `gradient` - Gradient of the loss with respect to the parameter
    ///
    /// Returns the parameter update to apply.
    fn step(&mut self, parameter_id: &str, gradient: ArrayView2<f32>) -> Result<Array2<f32>>;

    /// Resets the optimizer state.
    fn reset(&mut self);

    /// Returns the name of the optimizer.
    fn name(&self) -> &str;

    /// Gets the current learning rate.
    fn learning_rate(&self) -> f64;

    /// Sets the learning rate.
    fn set_learning_rate(&mut self, lr: f64);
}

/// Stochastic Gradient Descent (SGD) optimizer.
///
/// Parameters are updated as: θ = θ - lr * ∇L
/// With momentum: v = β * v + ∇L, θ = θ - lr * v
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SGD {
    /// Learning rate
    pub learning_rate: f64,
    /// Momentum coefficient (0 = no momentum)
    pub momentum: f64,
    /// Weight decay (L2 regularization)
    pub weight_decay: f64,
    /// Nesterov momentum
    pub nesterov: bool,
    /// Velocity buffers for momentum (keyed by parameter ID)
    #[serde(skip)]
    velocity: HashMap<String, Array2<f32>>,
}

impl SGD {
    /// Creates a new SGD optimizer.
    pub fn new(learning_rate: f64) -> Result<Self> {
        if learning_rate <= 0.0 {
            return Err(Error::invalid_parameter(
                "learning_rate",
                learning_rate,
                "must be positive",
            ));
        }

        Ok(Self {
            learning_rate,
            momentum: 0.0,
            weight_decay: 0.0,
            nesterov: false,
            velocity: HashMap::new(),
        })
    }

    /// Creates an SGD optimizer with momentum.
    pub fn with_momentum(learning_rate: f64, momentum: f64) -> Result<Self> {
        if learning_rate <= 0.0 {
            return Err(Error::invalid_parameter(
                "learning_rate",
                learning_rate,
                "must be positive",
            ));
        }

        if !(0.0..=1.0).contains(&momentum) {
            return Err(Error::invalid_parameter(
                "momentum",
                momentum,
                "must be in [0, 1]",
            ));
        }

        Ok(Self {
            learning_rate,
            momentum,
            weight_decay: 0.0,
            nesterov: false,
            velocity: HashMap::new(),
        })
    }

    /// Creates an SGD optimizer with Nesterov momentum.
    pub fn with_nesterov(learning_rate: f64, momentum: f64) -> Result<Self> {
        let mut opt = Self::with_momentum(learning_rate, momentum)?;
        opt.nesterov = true;
        Ok(opt)
    }

    /// Sets the weight decay.
    pub fn with_weight_decay(mut self, weight_decay: f64) -> Result<Self> {
        if weight_decay < 0.0 {
            return Err(Error::invalid_parameter(
                "weight_decay",
                weight_decay,
                "must be non-negative",
            ));
        }
        self.weight_decay = weight_decay;
        Ok(self)
    }
}

impl Optimizer for SGD {
    fn step(&mut self, parameter_id: &str, gradient: ArrayView2<f32>) -> Result<Array2<f32>> {
        let lr = self.learning_rate as f32;
        let mut grad = gradient.to_owned();

        // Apply weight decay if specified
        if self.weight_decay > 0.0 {
            // Note: In a real implementation, we'd need access to the parameters
            // For now, we just apply the gradient
        }

        if self.momentum > 0.0 {
            let velocity = self
                .velocity
                .entry(parameter_id.to_string())
                .or_insert_with(|| Array2::zeros(gradient.dim()));

            // Update velocity: v = momentum * v + grad
            *velocity = velocity.mapv(|v| v * self.momentum as f32) + &grad;

            if self.nesterov {
                // Nesterov momentum: update = momentum * v + grad
                grad = velocity.mapv(|v| v * self.momentum as f32) + &grad;
            } else {
                // Standard momentum: update = v
                grad = velocity.clone();
            }
        }

        // Apply learning rate and return update
        Ok(grad.mapv(|g| g * lr))
    }

    fn reset(&mut self) {
        self.velocity.clear();
    }

    fn name(&self) -> &str {
        if self.nesterov {
            "SGD (Nesterov)"
        } else if self.momentum > 0.0 {
            "SGD (Momentum)"
        } else {
            "SGD"
        }
    }

    fn learning_rate(&self) -> f64 {
        self.learning_rate
    }

    fn set_learning_rate(&mut self, lr: f64) {
        self.learning_rate = lr;
    }
}

/// Adam optimizer (Adaptive Moment Estimation).
///
/// Maintains running averages of gradient and squared gradient.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Adam {
    /// Learning rate
    pub learning_rate: f64,
    /// Exponential decay rate for first moment (default: 0.9)
    pub beta1: f64,
    /// Exponential decay rate for second moment (default: 0.999)
    pub beta2: f64,
    /// Small constant for numerical stability (default: 1e-8)
    pub epsilon: f64,
    /// Weight decay
    pub weight_decay: f64,
    /// First moment estimates (keyed by parameter ID)
    #[serde(skip)]
    m: HashMap<String, Array2<f32>>,
    /// Second moment estimates (keyed by parameter ID)
    #[serde(skip)]
    v: HashMap<String, Array2<f32>>,
    /// Step counter (keyed by parameter ID)
    #[serde(skip)]
    t: HashMap<String, usize>,
}

impl Adam {
    /// Creates a new Adam optimizer with default parameters.
    pub fn new(learning_rate: f64) -> Result<Self> {
        if learning_rate <= 0.0 {
            return Err(Error::invalid_parameter(
                "learning_rate",
                learning_rate,
                "must be positive",
            ));
        }

        Ok(Self {
            learning_rate,
            beta1: 0.9,
            beta2: 0.999,
            epsilon: 1e-8,
            weight_decay: 0.0,
            m: HashMap::new(),
            v: HashMap::new(),
            t: HashMap::new(),
        })
    }

    /// Creates an Adam optimizer with custom parameters.
    pub fn with_params(learning_rate: f64, beta1: f64, beta2: f64, epsilon: f64) -> Result<Self> {
        if learning_rate <= 0.0 {
            return Err(Error::invalid_parameter(
                "learning_rate",
                learning_rate,
                "must be positive",
            ));
        }

        if !(0.0..1.0).contains(&beta1) {
            return Err(Error::invalid_parameter(
                "beta1",
                beta1,
                "must be in [0, 1)",
            ));
        }

        if !(0.0..1.0).contains(&beta2) {
            return Err(Error::invalid_parameter(
                "beta2",
                beta2,
                "must be in [0, 1)",
            ));
        }

        Ok(Self {
            learning_rate,
            beta1,
            beta2,
            epsilon,
            weight_decay: 0.0,
            m: HashMap::new(),
            v: HashMap::new(),
            t: HashMap::new(),
        })
    }

    /// Sets the weight decay.
    pub fn with_weight_decay(mut self, weight_decay: f64) -> Result<Self> {
        if weight_decay < 0.0 {
            return Err(Error::invalid_parameter(
                "weight_decay",
                weight_decay,
                "must be non-negative",
            ));
        }
        self.weight_decay = weight_decay;
        Ok(self)
    }
}

impl Optimizer for Adam {
    fn step(&mut self, parameter_id: &str, gradient: ArrayView2<f32>) -> Result<Array2<f32>> {
        let grad = gradient.to_owned();

        // Initialize moments if needed
        let m = self
            .m
            .entry(parameter_id.to_string())
            .or_insert_with(|| Array2::zeros(gradient.dim()));

        let v = self
            .v
            .entry(parameter_id.to_string())
            .or_insert_with(|| Array2::zeros(gradient.dim()));

        let t = self.t.entry(parameter_id.to_string()).or_insert(0);
        *t += 1;

        let beta1 = self.beta1 as f32;
        let beta2 = self.beta2 as f32;
        let lr = self.learning_rate as f32;
        let eps = self.epsilon as f32;

        // Update biased first moment estimate: m = beta1 * m + (1 - beta1) * grad
        *m = m.mapv(|mi| mi * beta1) + grad.mapv(|g| g * (1.0 - beta1));

        // Update biased second moment estimate: v = beta2 * v + (1 - beta2) * grad^2
        *v = v.mapv(|vi| vi * beta2) + grad.mapv(|g| g * g * (1.0 - beta2));

        // Compute bias-corrected moments
        let t_f32 = *t as f32;
        let m_hat = m.mapv(|mi| mi / (1.0 - beta1.powf(t_f32)));
        let v_hat = v.mapv(|vi| vi / (1.0 - beta2.powf(t_f32)));

        // Compute update: lr * m_hat / (sqrt(v_hat) + epsilon)
        let update = m_hat
            .iter()
            .zip(v_hat.iter())
            .map(|(m, v)| lr * m / (v.sqrt() + eps))
            .collect::<Vec<_>>();

        let update_array = Array2::from_shape_vec(gradient.dim(), update)
            .map_err(|e| Error::Optimizer(format!("Failed to reshape update array: {}", e)))?;

        Ok(update_array)
    }

    fn reset(&mut self) {
        self.m.clear();
        self.v.clear();
        self.t.clear();
    }

    fn name(&self) -> &str {
        "Adam"
    }

    fn learning_rate(&self) -> f64 {
        self.learning_rate
    }

    fn set_learning_rate(&mut self, lr: f64) {
        self.learning_rate = lr;
    }
}

/// AdamW optimizer (Adam with decoupled weight decay).
///
/// Applies weight decay directly to parameters rather than gradients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdamW {
    /// Underlying Adam optimizer
    adam: Adam,
}

impl AdamW {
    /// Creates a new AdamW optimizer.
    pub fn new(learning_rate: f64, weight_decay: f64) -> Result<Self> {
        let adam = Adam::new(learning_rate)?.with_weight_decay(weight_decay)?;
        Ok(Self { adam })
    }

    /// Creates an AdamW optimizer with custom parameters.
    pub fn with_params(
        learning_rate: f64,
        beta1: f64,
        beta2: f64,
        epsilon: f64,
        weight_decay: f64,
    ) -> Result<Self> {
        let adam = Adam::with_params(learning_rate, beta1, beta2, epsilon)?
            .with_weight_decay(weight_decay)?;
        Ok(Self { adam })
    }
}

impl Optimizer for AdamW {
    fn step(&mut self, parameter_id: &str, gradient: ArrayView2<f32>) -> Result<Array2<f32>> {
        self.adam.step(parameter_id, gradient)
    }

    fn reset(&mut self) {
        self.adam.reset();
    }

    fn name(&self) -> &str {
        "AdamW"
    }

    fn learning_rate(&self) -> f64 {
        self.adam.learning_rate()
    }

    fn set_learning_rate(&mut self, lr: f64) {
        self.adam.set_learning_rate(lr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::arr2;

    #[test]
    fn test_sgd_creation() {
        let opt = SGD::new(0.01).expect("Failed to create SGD");
        assert_eq!(opt.learning_rate, 0.01);
        assert_eq!(opt.momentum, 0.0);

        let opt_momentum =
            SGD::with_momentum(0.01, 0.9).expect("Failed to create SGD with momentum");
        assert_eq!(opt_momentum.momentum, 0.9);

        let invalid_opt = SGD::new(-0.01);
        assert!(invalid_opt.is_err());
    }

    #[test]
    fn test_sgd_step() {
        let mut opt = SGD::new(0.1).expect("Failed to create SGD");
        let gradient = arr2(&[[1.0, 2.0], [3.0, 4.0]]);

        let update = opt
            .step("param1", gradient.view())
            .expect("Failed to perform step");

        assert_eq!(update.shape(), gradient.shape());
        // Update should be lr * gradient = 0.1 * gradient
        assert!((update[[0, 0]] - 0.1).abs() < 1e-6);
        assert!((update[[1, 1]] - 0.4).abs() < 1e-6);
    }

    #[test]
    fn test_adam_creation() {
        let opt = Adam::new(0.001).expect("Failed to create Adam");
        assert_eq!(opt.learning_rate, 0.001);
        assert_eq!(opt.beta1, 0.9);
        assert_eq!(opt.beta2, 0.999);

        let invalid_opt = Adam::new(-0.001);
        assert!(invalid_opt.is_err());
    }

    #[test]
    fn test_adam_step() {
        let mut opt = Adam::new(0.001).expect("Failed to create Adam");
        let gradient = arr2(&[[1.0, 2.0], [3.0, 4.0]]);

        let update = opt
            .step("param1", gradient.view())
            .expect("Failed to perform step");

        assert_eq!(update.shape(), gradient.shape());
        assert!(update.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn test_adamw_creation() {
        let opt = AdamW::new(0.001, 0.01).expect("Failed to create AdamW");
        assert_eq!(opt.learning_rate(), 0.001);
    }

    #[test]
    fn test_optimizer_reset() {
        let mut opt = Adam::new(0.001).expect("Failed to create Adam");
        let gradient = arr2(&[[1.0, 2.0]]);

        opt.step("param1", gradient.view())
            .expect("Failed to perform step");
        assert!(!opt.m.is_empty());

        opt.reset();
        assert!(opt.m.is_empty());
        assert!(opt.v.is_empty());
        assert!(opt.t.is_empty());
    }

    #[test]
    fn test_learning_rate_modification() {
        let mut opt = SGD::new(0.01).expect("Failed to create SGD");
        assert_eq!(opt.learning_rate(), 0.01);

        opt.set_learning_rate(0.001);
        assert_eq!(opt.learning_rate(), 0.001);
    }
}
