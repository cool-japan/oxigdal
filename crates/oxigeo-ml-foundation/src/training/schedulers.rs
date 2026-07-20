//! Learning rate schedulers for optimizers.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// Trait for learning rate schedulers.
pub trait LRScheduler: Send + Sync {
    /// Gets the learning rate for the given epoch.
    ///
    /// # Arguments
    /// * `epoch` - Current epoch number (0-indexed)
    /// * `base_lr` - Base learning rate
    fn get_lr(&self, epoch: usize, base_lr: f64) -> f64;

    /// Returns the name of the scheduler.
    fn name(&self) -> &str;
}

/// Constant learning rate (no scheduling).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ConstantLR;

impl LRScheduler for ConstantLR {
    fn get_lr(&self, _epoch: usize, base_lr: f64) -> f64 {
        base_lr
    }

    fn name(&self) -> &str {
        "ConstantLR"
    }
}

/// Step decay: reduce LR by a factor every N epochs.
///
/// lr = base_lr * (gamma ^ (epoch // step_size))
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct StepLR {
    /// Number of epochs between each LR reduction
    pub step_size: usize,
    /// Multiplicative factor of learning rate decay
    pub gamma: f64,
}

impl StepLR {
    /// Creates a new StepLR scheduler.
    pub fn new(step_size: usize, gamma: f64) -> Result<Self> {
        if step_size == 0 {
            return Err(Error::invalid_parameter(
                "step_size",
                step_size,
                "must be positive",
            ));
        }

        if !(0.0..=1.0).contains(&gamma) {
            return Err(Error::invalid_parameter(
                "gamma",
                gamma,
                "must be in (0, 1]",
            ));
        }

        Ok(Self { step_size, gamma })
    }
}

impl LRScheduler for StepLR {
    fn get_lr(&self, epoch: usize, base_lr: f64) -> f64 {
        let num_steps = epoch / self.step_size;
        base_lr * self.gamma.powi(num_steps as i32)
    }

    fn name(&self) -> &str {
        "StepLR"
    }
}

/// Exponential decay: lr = base_lr * (gamma ^ epoch)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ExponentialLR {
    /// Multiplicative factor of learning rate decay
    pub gamma: f64,
}

impl ExponentialLR {
    /// Creates a new ExponentialLR scheduler.
    pub fn new(gamma: f64) -> Result<Self> {
        if !(0.0..=1.0).contains(&gamma) {
            return Err(Error::invalid_parameter(
                "gamma",
                gamma,
                "must be in (0, 1]",
            ));
        }

        Ok(Self { gamma })
    }
}

impl LRScheduler for ExponentialLR {
    fn get_lr(&self, epoch: usize, base_lr: f64) -> f64 {
        base_lr * self.gamma.powi(epoch as i32)
    }

    fn name(&self) -> &str {
        "ExponentialLR"
    }
}

/// Cosine annealing: lr = min_lr + (base_lr - min_lr) * (1 + cos(π * epoch / T_max)) / 2
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CosineAnnealingLR {
    /// Maximum number of epochs
    pub t_max: usize,
    /// Minimum learning rate
    pub eta_min: f64,
}

impl CosineAnnealingLR {
    /// Creates a new CosineAnnealingLR scheduler.
    pub fn new(t_max: usize, eta_min: f64) -> Result<Self> {
        if t_max == 0 {
            return Err(Error::invalid_parameter("t_max", t_max, "must be positive"));
        }

        if eta_min < 0.0 {
            return Err(Error::invalid_parameter(
                "eta_min",
                eta_min,
                "must be non-negative",
            ));
        }

        Ok(Self { t_max, eta_min })
    }
}

impl LRScheduler for CosineAnnealingLR {
    fn get_lr(&self, epoch: usize, base_lr: f64) -> f64 {
        let t = (epoch % self.t_max) as f64;
        let t_max = self.t_max as f64;
        let cos_term = (1.0 + (std::f64::consts::PI * t / t_max).cos()) / 2.0;
        self.eta_min + (base_lr - self.eta_min) * cos_term
    }

    fn name(&self) -> &str {
        "CosineAnnealingLR"
    }
}

/// Cosine annealing with warm restarts.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CosineAnnealingWarmRestarts {
    /// Number of epochs for the first restart
    pub t_0: usize,
    /// Factor to increase t_i after each restart
    pub t_mult: usize,
    /// Minimum learning rate
    pub eta_min: f64,
}

impl CosineAnnealingWarmRestarts {
    /// Creates a new CosineAnnealingWarmRestarts scheduler.
    pub fn new(t_0: usize, t_mult: usize, eta_min: f64) -> Result<Self> {
        if t_0 == 0 {
            return Err(Error::invalid_parameter("t_0", t_0, "must be positive"));
        }

        if t_mult == 0 {
            return Err(Error::invalid_parameter(
                "t_mult",
                t_mult,
                "must be positive",
            ));
        }

        if eta_min < 0.0 {
            return Err(Error::invalid_parameter(
                "eta_min",
                eta_min,
                "must be non-negative",
            ));
        }

        Ok(Self {
            t_0,
            t_mult,
            eta_min,
        })
    }

    fn get_current_t(&self, epoch: usize) -> (usize, usize) {
        let mut t_i = self.t_0;
        let mut elapsed = 0;

        loop {
            if epoch < elapsed + t_i {
                return (epoch - elapsed, t_i);
            }
            elapsed += t_i;
            t_i *= self.t_mult;
        }
    }
}

impl LRScheduler for CosineAnnealingWarmRestarts {
    fn get_lr(&self, epoch: usize, base_lr: f64) -> f64 {
        let (t_cur, t_i) = self.get_current_t(epoch);
        let t_cur_f = t_cur as f64;
        let t_i_f = t_i as f64;
        let cos_term = (1.0 + (std::f64::consts::PI * t_cur_f / t_i_f).cos()) / 2.0;
        self.eta_min + (base_lr - self.eta_min) * cos_term
    }

    fn name(&self) -> &str {
        "CosineAnnealingWarmRestarts"
    }
}

/// Linear warmup followed by constant learning rate.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LinearWarmup {
    /// Number of warmup epochs
    pub warmup_epochs: usize,
}

impl LinearWarmup {
    /// Creates a new LinearWarmup scheduler.
    pub fn new(warmup_epochs: usize) -> Result<Self> {
        Ok(Self { warmup_epochs })
    }
}

impl LRScheduler for LinearWarmup {
    fn get_lr(&self, epoch: usize, base_lr: f64) -> f64 {
        if self.warmup_epochs == 0 {
            return base_lr;
        }

        if epoch < self.warmup_epochs {
            base_lr * (epoch + 1) as f64 / self.warmup_epochs as f64
        } else {
            base_lr
        }
    }

    fn name(&self) -> &str {
        "LinearWarmup"
    }
}

/// Polynomial decay: lr = base_lr * (1 - epoch / max_epochs)^power
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PolynomialLR {
    /// Total number of epochs
    pub total_epochs: usize,
    /// Power of the polynomial
    pub power: f64,
}

impl PolynomialLR {
    /// Creates a new PolynomialLR scheduler.
    pub fn new(total_epochs: usize, power: f64) -> Result<Self> {
        if total_epochs == 0 {
            return Err(Error::invalid_parameter(
                "total_epochs",
                total_epochs,
                "must be positive",
            ));
        }

        if power <= 0.0 {
            return Err(Error::invalid_parameter("power", power, "must be positive"));
        }

        Ok(Self {
            total_epochs,
            power,
        })
    }
}

impl LRScheduler for PolynomialLR {
    fn get_lr(&self, epoch: usize, base_lr: f64) -> f64 {
        if epoch >= self.total_epochs {
            return 0.0;
        }

        let factor = 1.0 - (epoch as f64 / self.total_epochs as f64);
        base_lr * factor.powf(self.power)
    }

    fn name(&self) -> &str {
        "PolynomialLR"
    }
}

/// One cycle policy: warmup + annealing.
///
/// Used in super-convergence training.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct OneCycleLR {
    /// Total number of epochs
    pub total_epochs: usize,
    /// Maximum learning rate (peak)
    pub max_lr: f64,
    /// Percentage of cycle spent increasing LR
    pub pct_start: f64,
    /// Anneal strategy ("cos" or "linear")
    pub anneal_strategy: AnnealStrategy,
}

/// Annealing strategy for learning rate decay.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AnnealStrategy {
    /// Cosine annealing (smooth decay following cosine curve)
    Cosine,
    /// Linear annealing (constant rate decay)
    Linear,
}

impl OneCycleLR {
    /// Creates a new OneCycleLR scheduler.
    pub fn new(total_epochs: usize, max_lr: f64) -> Result<Self> {
        if total_epochs == 0 {
            return Err(Error::invalid_parameter(
                "total_epochs",
                total_epochs,
                "must be positive",
            ));
        }

        if max_lr <= 0.0 {
            return Err(Error::invalid_parameter(
                "max_lr",
                max_lr,
                "must be positive",
            ));
        }

        Ok(Self {
            total_epochs,
            max_lr,
            pct_start: 0.3,
            anneal_strategy: AnnealStrategy::Cosine,
        })
    }

    /// Sets the percentage of the cycle spent increasing LR.
    pub fn with_pct_start(mut self, pct_start: f64) -> Result<Self> {
        if !(0.0..=1.0).contains(&pct_start) {
            return Err(Error::invalid_parameter(
                "pct_start",
                pct_start,
                "must be in [0, 1]",
            ));
        }
        self.pct_start = pct_start;
        Ok(self)
    }
}

impl LRScheduler for OneCycleLR {
    fn get_lr(&self, epoch: usize, base_lr: f64) -> f64 {
        let progress = epoch as f64 / self.total_epochs as f64;
        let warmup_end = self.pct_start;

        if progress < warmup_end {
            // Warmup phase
            let warmup_progress = progress / warmup_end;
            base_lr + (self.max_lr - base_lr) * warmup_progress
        } else {
            // Annealing phase
            let anneal_progress = (progress - warmup_end) / (1.0 - warmup_end);
            match self.anneal_strategy {
                AnnealStrategy::Cosine => {
                    let cos_term = (1.0 + (std::f64::consts::PI * anneal_progress).cos()) / 2.0;
                    base_lr + (self.max_lr - base_lr) * cos_term
                }
                AnnealStrategy::Linear => self.max_lr - (self.max_lr - base_lr) * anneal_progress,
            }
        }
    }

    fn name(&self) -> &str {
        "OneCycleLR"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_constant_lr() {
        let scheduler = ConstantLR;
        assert_eq!(scheduler.get_lr(0, 0.01), 0.01);
        assert_eq!(scheduler.get_lr(10, 0.01), 0.01);
        assert_eq!(scheduler.get_lr(100, 0.01), 0.01);
    }

    #[test]
    fn test_step_lr() {
        let scheduler = StepLR::new(10, 0.1).expect("Failed to create StepLR");
        let base_lr = 0.1;

        assert_relative_eq!(scheduler.get_lr(0, base_lr), 0.1);
        assert_relative_eq!(scheduler.get_lr(9, base_lr), 0.1);
        assert_relative_eq!(scheduler.get_lr(10, base_lr), 0.01);
        assert_relative_eq!(scheduler.get_lr(20, base_lr), 0.001, epsilon = 1e-10);
    }

    #[test]
    fn test_exponential_lr() {
        let scheduler = ExponentialLR::new(0.9).expect("Failed to create ExponentialLR");
        let base_lr = 0.1;

        assert_relative_eq!(scheduler.get_lr(0, base_lr), 0.1);
        assert_relative_eq!(scheduler.get_lr(1, base_lr), 0.09);
        assert!(scheduler.get_lr(10, base_lr) < 0.05);
    }

    #[test]
    fn test_cosine_annealing_lr() {
        let scheduler =
            CosineAnnealingLR::new(10, 0.0).expect("Failed to create CosineAnnealingLR");
        let base_lr = 1.0;

        let lr_0 = scheduler.get_lr(0, base_lr);
        let lr_5 = scheduler.get_lr(5, base_lr);
        let lr_10 = scheduler.get_lr(10, base_lr);

        assert_relative_eq!(lr_0, 1.0, epsilon = 1e-6);
        assert!(lr_5 < 0.6);
        assert_relative_eq!(lr_10, 1.0, epsilon = 1e-6); // Restart
    }

    #[test]
    fn test_linear_warmup() {
        let scheduler = LinearWarmup::new(5).expect("Failed to create LinearWarmup");
        let base_lr = 1.0;

        assert_relative_eq!(scheduler.get_lr(0, base_lr), 0.2);
        assert_relative_eq!(scheduler.get_lr(2, base_lr), 0.6);
        assert_relative_eq!(scheduler.get_lr(4, base_lr), 1.0);
        assert_relative_eq!(scheduler.get_lr(10, base_lr), 1.0);
    }

    #[test]
    fn test_polynomial_lr() {
        let scheduler = PolynomialLR::new(10, 2.0).expect("Failed to create PolynomialLR");
        let base_lr = 1.0;

        let lr_0 = scheduler.get_lr(0, base_lr);
        let lr_5 = scheduler.get_lr(5, base_lr);
        let lr_10 = scheduler.get_lr(10, base_lr);

        assert_relative_eq!(lr_0, 1.0, epsilon = 1e-6);
        assert_relative_eq!(lr_5, 0.25, epsilon = 1e-6);
        assert_relative_eq!(lr_10, 0.0, epsilon = 1e-6);
    }

    #[test]
    fn test_one_cycle_lr() {
        let scheduler = OneCycleLR::new(100, 0.1).expect("Failed to create OneCycleLR");
        let base_lr = 0.01;

        let lr_0 = scheduler.get_lr(0, base_lr);
        let lr_30 = scheduler.get_lr(30, base_lr);
        let lr_50 = scheduler.get_lr(50, base_lr);

        assert!(lr_0 < lr_30); // Should be increasing during warmup
        assert!(lr_30 > lr_50); // Should be decreasing during annealing
    }

    #[test]
    fn test_scheduler_errors() {
        assert!(StepLR::new(0, 0.1).is_err());
        assert!(StepLR::new(10, 1.5).is_err());
        assert!(ExponentialLR::new(1.5).is_err());
        assert!(CosineAnnealingLR::new(0, 0.0).is_err());
        assert!(PolynomialLR::new(10, 0.0).is_err());
        assert!(OneCycleLR::new(0, 0.1).is_err());
    }
}
