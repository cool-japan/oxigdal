//! Early stopping logic to prevent overfitting.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// Early stopping criterion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Criterion {
    /// Stop when validation loss stops improving
    ValidationLoss,
    /// Stop when validation accuracy stops improving
    ValidationAccuracy,
}

/// Early stopping monitor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EarlyStopping {
    /// Criterion to monitor
    pub criterion: Criterion,
    /// Number of epochs with no improvement after which to stop
    pub patience: usize,
    /// Minimum change to qualify as improvement
    pub min_delta: f64,
    /// Best value seen so far
    best_value: Option<f64>,
    /// Number of epochs without improvement
    epochs_without_improvement: usize,
    /// Whether early stopping has been triggered
    stopped: bool,
}

impl EarlyStopping {
    /// Creates a new early stopping monitor.
    ///
    /// # Arguments
    /// * `criterion` - Criterion to monitor
    /// * `patience` - Number of epochs with no improvement before stopping
    /// * `min_delta` - Minimum change to qualify as improvement
    pub fn new(criterion: Criterion, patience: usize, min_delta: f64) -> Result<Self> {
        if patience == 0 {
            return Err(Error::invalid_parameter(
                "patience",
                patience,
                "must be positive",
            ));
        }

        if min_delta < 0.0 {
            return Err(Error::invalid_parameter(
                "min_delta",
                min_delta,
                "must be non-negative",
            ));
        }

        Ok(Self {
            criterion,
            patience,
            min_delta,
            best_value: None,
            epochs_without_improvement: 0,
            stopped: false,
        })
    }

    /// Creates an early stopping monitor for validation loss.
    pub fn for_loss(patience: usize, min_delta: f64) -> Result<Self> {
        Self::new(Criterion::ValidationLoss, patience, min_delta)
    }

    /// Creates an early stopping monitor for validation accuracy.
    pub fn for_accuracy(patience: usize, min_delta: f64) -> Result<Self> {
        Self::new(Criterion::ValidationAccuracy, patience, min_delta)
    }

    /// Updates the early stopping state with a new metric value.
    ///
    /// Returns `true` if training should continue, `false` if it should stop.
    pub fn update(&mut self, value: f64) -> bool {
        if self.stopped {
            return false;
        }

        let is_improvement = match self.best_value {
            None => true,
            Some(best) => match self.criterion {
                Criterion::ValidationLoss => value < best - self.min_delta,
                Criterion::ValidationAccuracy => value > best + self.min_delta,
            },
        };

        if is_improvement {
            self.best_value = Some(value);
            self.epochs_without_improvement = 0;
        } else {
            self.epochs_without_improvement += 1;
        }

        if self.epochs_without_improvement > self.patience {
            self.stopped = true;
            return false;
        }

        true
    }

    /// Checks if early stopping has been triggered.
    pub fn should_stop(&self) -> bool {
        self.stopped
    }

    /// Gets the best value seen so far.
    pub fn best_value(&self) -> Option<f64> {
        self.best_value
    }

    /// Gets the number of epochs without improvement.
    pub fn epochs_without_improvement(&self) -> usize {
        self.epochs_without_improvement
    }

    /// Resets the early stopping state.
    pub fn reset(&mut self) {
        self.best_value = None;
        self.epochs_without_improvement = 0;
        self.stopped = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_early_stopping_loss() {
        let mut es = EarlyStopping::for_loss(3, 0.001).expect("Failed to create early stopping");

        // Improvement
        assert!(es.update(1.0));
        assert_eq!(es.epochs_without_improvement(), 0);

        // Improvement
        assert!(es.update(0.9));
        assert_eq!(es.epochs_without_improvement(), 0);

        // No improvement
        assert!(es.update(0.95));
        assert_eq!(es.epochs_without_improvement(), 1);

        // No improvement
        assert!(es.update(0.92));
        assert_eq!(es.epochs_without_improvement(), 2);

        // No improvement - should continue (patience not reached)
        assert!(es.update(0.91));
        assert_eq!(es.epochs_without_improvement(), 3);

        // Patience reached - should stop
        assert!(!es.update(0.90));
        assert!(es.should_stop());
    }

    #[test]
    fn test_early_stopping_accuracy() {
        let mut es = EarlyStopping::for_accuracy(2, 0.01).expect("Failed to create early stopping");

        // Improvement
        assert!(es.update(0.7));
        assert_eq!(es.epochs_without_improvement(), 0);

        // Improvement
        assert!(es.update(0.75));
        assert_eq!(es.epochs_without_improvement(), 0);

        // No improvement
        assert!(es.update(0.74));
        assert_eq!(es.epochs_without_improvement(), 1);

        // No improvement - patience reached, should stop
        assert!(es.update(0.73));
        assert_eq!(es.epochs_without_improvement(), 2);

        // Should stop now
        assert!(!es.update(0.72));
        assert!(es.should_stop());
    }

    #[test]
    fn test_early_stopping_reset() {
        let mut es = EarlyStopping::for_loss(2, 0.0).expect("Failed to create early stopping");

        es.update(1.0);
        es.update(1.1);
        es.update(1.2);
        es.update(1.3); // Third epoch without improvement - triggers stop
        assert!(es.should_stop());

        es.reset();
        assert!(!es.should_stop());
        assert_eq!(es.epochs_without_improvement(), 0);
        assert_eq!(es.best_value(), None);
    }

    #[test]
    fn test_min_delta() {
        let mut es = EarlyStopping::for_loss(2, 0.1).expect("Failed to create early stopping");

        es.update(1.0);
        assert_eq!(es.best_value(), Some(1.0));

        // Small improvement (less than min_delta) - should not count
        es.update(0.95);
        assert_eq!(es.epochs_without_improvement(), 1);

        // Large improvement (more than min_delta) - should count
        es.update(0.85);
        assert_eq!(es.epochs_without_improvement(), 0);
    }

    #[test]
    fn test_early_stopping_errors() {
        assert!(EarlyStopping::for_loss(0, 0.0).is_err());
        assert!(EarlyStopping::for_loss(3, -0.1).is_err());
    }
}
