//! Loss functions for training neural networks.

use crate::{Error, Result};
use scirs2_core::ndarray::{Array1, Array2, ArrayView2, Axis};

/// Trait for loss functions.
pub trait LossFunction: Send + Sync {
    /// Computes the loss value.
    ///
    /// # Arguments
    /// * `predictions` - Model predictions
    /// * `targets` - Ground truth targets
    fn compute(&self, predictions: ArrayView2<f32>, targets: ArrayView2<f32>) -> Result<f32>;

    /// Computes the gradient of the loss with respect to predictions.
    ///
    /// # Arguments
    /// * `predictions` - Model predictions
    /// * `targets` - Ground truth targets
    fn gradient(
        &self,
        predictions: ArrayView2<f32>,
        targets: ArrayView2<f32>,
    ) -> Result<Array2<f32>>;

    /// Returns the name of the loss function.
    fn name(&self) -> &str;
}

/// Mean Squared Error (MSE) loss for regression tasks.
///
/// L = (1/N) * Σ(y_pred - y_true)²
#[derive(Debug, Clone)]
pub struct MSELoss;

impl LossFunction for MSELoss {
    fn compute(&self, predictions: ArrayView2<f32>, targets: ArrayView2<f32>) -> Result<f32> {
        if predictions.shape() != targets.shape() {
            return Err(Error::invalid_dimensions(
                format!("{:?}", targets.shape()),
                format!("{:?}", predictions.shape()),
            ));
        }

        let diff = &predictions.to_owned() - &targets.to_owned();
        let squared = &diff * &diff;
        let sum = squared.sum();
        let n = predictions.len() as f32;

        Ok(sum / n)
    }

    fn gradient(
        &self,
        predictions: ArrayView2<f32>,
        targets: ArrayView2<f32>,
    ) -> Result<Array2<f32>> {
        if predictions.shape() != targets.shape() {
            return Err(Error::invalid_dimensions(
                format!("{:?}", targets.shape()),
                format!("{:?}", predictions.shape()),
            ));
        }

        let n = predictions.len() as f32;
        let diff = &predictions.to_owned() - &targets.to_owned();
        Ok(diff * (2.0 / n))
    }

    fn name(&self) -> &str {
        "MSELoss"
    }
}

/// Cross-entropy loss for classification tasks.
///
/// L = -(1/N) * Σ Σ y_true * log(y_pred + ε)
#[derive(Debug, Clone)]
pub struct CrossEntropyLoss {
    /// Small epsilon to avoid log(0)
    pub epsilon: f32,
}

impl Default for CrossEntropyLoss {
    fn default() -> Self {
        Self { epsilon: 1e-7 }
    }
}

impl CrossEntropyLoss {
    /// Creates a new cross-entropy loss with default epsilon.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new cross-entropy loss with custom epsilon.
    pub fn with_epsilon(epsilon: f32) -> Self {
        Self { epsilon }
    }

    /// Applies softmax activation to logits.
    fn softmax(&self, logits: ArrayView2<f32>) -> Array2<f32> {
        let mut result = Array2::<f32>::zeros(logits.dim());

        for (i, row) in logits.axis_iter(Axis(0)).enumerate() {
            let max_val = row.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));
            let exp_row: Array1<f32> = row.mapv(|x| (x - max_val).exp());
            let sum: f32 = exp_row.sum();
            result.row_mut(i).assign(&(exp_row / sum));
        }

        result
    }
}

impl LossFunction for CrossEntropyLoss {
    fn compute(&self, predictions: ArrayView2<f32>, targets: ArrayView2<f32>) -> Result<f32> {
        if predictions.shape() != targets.shape() {
            return Err(Error::invalid_dimensions(
                format!("{:?}", targets.shape()),
                format!("{:?}", predictions.shape()),
            ));
        }

        let probs = self.softmax(predictions);
        let clipped = probs.mapv(|x| (x + self.epsilon).ln());
        let product = &targets.to_owned() * &clipped;
        let sum = product.sum();
        let n = predictions.nrows() as f32;

        Ok(-sum / n)
    }

    fn gradient(
        &self,
        predictions: ArrayView2<f32>,
        targets: ArrayView2<f32>,
    ) -> Result<Array2<f32>> {
        if predictions.shape() != targets.shape() {
            return Err(Error::invalid_dimensions(
                format!("{:?}", targets.shape()),
                format!("{:?}", predictions.shape()),
            ));
        }

        let probs = self.softmax(predictions);
        let n = predictions.nrows() as f32;
        let diff = probs - targets.to_owned();
        Ok(diff / n)
    }

    fn name(&self) -> &str {
        "CrossEntropyLoss"
    }
}

/// Dice loss for segmentation tasks.
///
/// L = 1 - (2 * |X ∩ Y| + ε) / (|X| + |Y| + ε)
#[derive(Debug, Clone)]
pub struct DiceLoss {
    /// Smooth factor to avoid division by zero
    pub smooth: f32,
}

impl Default for DiceLoss {
    fn default() -> Self {
        Self { smooth: 1.0 }
    }
}

impl DiceLoss {
    /// Creates a new Dice loss with default smoothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new Dice loss with custom smoothing factor.
    pub fn with_smooth(smooth: f32) -> Self {
        Self { smooth }
    }
}

impl LossFunction for DiceLoss {
    fn compute(&self, predictions: ArrayView2<f32>, targets: ArrayView2<f32>) -> Result<f32> {
        if predictions.shape() != targets.shape() {
            return Err(Error::invalid_dimensions(
                format!("{:?}", targets.shape()),
                format!("{:?}", predictions.shape()),
            ));
        }

        let pred = predictions.to_owned();
        let tgt = targets.to_owned();

        let intersection = (&pred * &tgt).sum();
        let pred_sum = pred.sum();
        let tgt_sum = tgt.sum();

        let dice = (2.0 * intersection + self.smooth) / (pred_sum + tgt_sum + self.smooth);

        Ok(1.0 - dice)
    }

    fn gradient(
        &self,
        predictions: ArrayView2<f32>,
        targets: ArrayView2<f32>,
    ) -> Result<Array2<f32>> {
        if predictions.shape() != targets.shape() {
            return Err(Error::invalid_dimensions(
                format!("{:?}", targets.shape()),
                format!("{:?}", predictions.shape()),
            ));
        }

        let pred = predictions.to_owned();
        let tgt = targets.to_owned();

        let intersection = (&pred * &tgt).sum();
        let pred_sum = pred.sum();
        let tgt_sum = tgt.sum();

        let numerator = 2.0 * intersection + self.smooth;
        let denominator = pred_sum + tgt_sum + self.smooth;

        // Gradient: d(1-dice)/dx = -2 * (target * denominator - numerator) / denominator^2
        let grad = (tgt * denominator - numerator) * (-2.0 / (denominator * denominator));

        Ok(grad)
    }

    fn name(&self) -> &str {
        "DiceLoss"
    }
}

/// Focal loss for handling class imbalance.
///
/// L = -α * (1-p)^γ * log(p)
#[derive(Debug, Clone)]
pub struct FocalLoss {
    /// Weighting factor for each class
    pub alpha: f32,
    /// Focusing parameter
    pub gamma: f32,
    /// Epsilon to avoid log(0)
    pub epsilon: f32,
}

impl Default for FocalLoss {
    fn default() -> Self {
        Self {
            alpha: 0.25,
            gamma: 2.0,
            epsilon: 1e-7,
        }
    }
}

impl FocalLoss {
    /// Creates a new focal loss with default parameters.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new focal loss with custom parameters.
    pub fn with_params(alpha: f32, gamma: f32) -> Self {
        Self {
            alpha,
            gamma,
            epsilon: 1e-7,
        }
    }
}

impl LossFunction for FocalLoss {
    fn compute(&self, predictions: ArrayView2<f32>, targets: ArrayView2<f32>) -> Result<f32> {
        if predictions.shape() != targets.shape() {
            return Err(Error::invalid_dimensions(
                format!("{:?}", targets.shape()),
                format!("{:?}", predictions.shape()),
            ));
        }

        let pred = predictions.to_owned();
        let tgt = targets.to_owned();

        // Clip predictions to avoid log(0)
        let pred_clipped = pred.mapv(|x| x.clamp(self.epsilon, 1.0 - self.epsilon));

        // Compute focal loss: -α * (1-p)^γ * log(p)
        let focal_weight = pred_clipped.mapv(|p| 1.0 - p).mapv(|x| x.powf(self.gamma));
        let log_probs = pred_clipped.mapv(|p| p.ln());
        let loss = &tgt * &focal_weight * &log_probs * (-self.alpha);

        let n = predictions.nrows() as f32;
        Ok(loss.sum() / n)
    }

    fn gradient(
        &self,
        predictions: ArrayView2<f32>,
        targets: ArrayView2<f32>,
    ) -> Result<Array2<f32>> {
        if predictions.shape() != targets.shape() {
            return Err(Error::invalid_dimensions(
                format!("{:?}", targets.shape()),
                format!("{:?}", predictions.shape()),
            ));
        }

        let pred = predictions.to_owned();
        let tgt = targets.to_owned();
        let pred_clipped = pred.mapv(|x| x.clamp(self.epsilon, 1.0 - self.epsilon));

        // Approximate gradient (simplified)
        let n = predictions.nrows() as f32;
        let one_minus_p = pred_clipped.mapv(|p| 1.0 - p);
        let focal_weight = one_minus_p.mapv(|x| x.powf(self.gamma));

        let grad = &tgt * &focal_weight * (-self.alpha / n);

        Ok(grad)
    }

    fn name(&self) -> &str {
        "FocalLoss"
    }
}

/// Combined loss that sums multiple loss functions.
#[derive(Default)]
pub struct CombinedLoss {
    /// List of (loss_function, weight) pairs
    pub losses: Vec<(Box<dyn LossFunction>, f32)>,
}

impl std::fmt::Debug for CombinedLoss {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CombinedLoss")
            .field(
                "losses",
                &self
                    .losses
                    .iter()
                    .map(|(l, w)| (l.name(), *w))
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl CombinedLoss {
    /// Creates a new empty combined loss.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a loss function with a weight.
    pub fn add_loss(&mut self, loss: Box<dyn LossFunction>, weight: f32) {
        self.losses.push((loss, weight));
    }
}

impl LossFunction for CombinedLoss {
    fn compute(&self, predictions: ArrayView2<f32>, targets: ArrayView2<f32>) -> Result<f32> {
        let mut total_loss = 0.0;

        for (loss, weight) in &self.losses {
            let loss_value = loss.compute(predictions, targets)?;
            total_loss += loss_value * weight;
        }

        Ok(total_loss)
    }

    fn gradient(
        &self,
        predictions: ArrayView2<f32>,
        targets: ArrayView2<f32>,
    ) -> Result<Array2<f32>> {
        if self.losses.is_empty() {
            return Err(Error::LossFunction("No losses added".to_string()));
        }

        let mut total_grad = Array2::<f32>::zeros(predictions.dim());

        for (loss, weight) in &self.losses {
            let grad = loss.gradient(predictions, targets)?;
            total_grad = total_grad + grad * *weight;
        }

        Ok(total_grad)
    }

    fn name(&self) -> &str {
        "CombinedLoss"
    }
}

/// Loss function enum wrapper for training loop interface
#[derive(Debug, Clone, Copy)]
pub enum LossFunctionType {
    /// Mean Squared Error loss
    MSE,
    /// Cross-Entropy loss
    CrossEntropy,
    /// Dice loss
    Dice,
    /// Focal loss
    Focal,
}

impl LossFunctionType {
    /// Compute loss from flat vectors
    ///
    /// # Arguments
    ///
    /// * `outputs` - Model outputs as flat vector
    /// * `targets` - Target values as flat vector
    /// * `shape` - Shape of the tensors (batch_size, num_features)
    pub fn compute(&self, outputs: &[f32], targets: &[f32], shape: &[usize]) -> Result<f64> {
        if outputs.len() != targets.len() {
            return Err(Error::invalid_dimensions(
                format!("{}", targets.len()),
                format!("{}", outputs.len()),
            ));
        }

        // Reshape to 2D arrays
        let outputs_array = Array2::from_shape_vec((shape[0], shape[1]), outputs.to_vec())
            .map_err(|_| Error::InvalidDimensions {
                expected: format!("{:?}", shape),
                actual: format!("{}", outputs.len()),
            })?;

        let targets_array = Array2::from_shape_vec((shape[0], shape[1]), targets.to_vec())
            .map_err(|_| Error::InvalidDimensions {
                expected: format!("{:?}", shape),
                actual: format!("{}", targets.len()),
            })?;

        // Compute loss using trait implementation
        let loss_value = match self {
            LossFunctionType::MSE => {
                let loss = MSELoss;
                loss.compute(outputs_array.view(), targets_array.view())?
            }
            LossFunctionType::CrossEntropy => {
                let loss = CrossEntropyLoss::new();
                loss.compute(outputs_array.view(), targets_array.view())?
            }
            LossFunctionType::Dice => {
                let loss = DiceLoss::new();
                loss.compute(outputs_array.view(), targets_array.view())?
            }
            LossFunctionType::Focal => {
                let loss = FocalLoss::new();
                loss.compute(outputs_array.view(), targets_array.view())?
            }
        };

        Ok(loss_value as f64)
    }

    /// Compute gradient (backward pass) from flat vectors
    ///
    /// # Arguments
    ///
    /// * `outputs` - Model outputs as flat vector
    /// * `targets` - Target values as flat vector
    /// * `shape` - Shape of the tensors (batch_size, num_features)
    pub fn backward(&self, outputs: &[f32], targets: &[f32], shape: &[usize]) -> Result<Vec<f32>> {
        if outputs.len() != targets.len() {
            return Err(Error::invalid_dimensions(
                format!("{}", targets.len()),
                format!("{}", outputs.len()),
            ));
        }

        // Reshape to 2D arrays
        let outputs_array = Array2::from_shape_vec((shape[0], shape[1]), outputs.to_vec())
            .map_err(|_| Error::InvalidDimensions {
                expected: format!("{:?}", shape),
                actual: format!("{}", outputs.len()),
            })?;

        let targets_array = Array2::from_shape_vec((shape[0], shape[1]), targets.to_vec())
            .map_err(|_| Error::InvalidDimensions {
                expected: format!("{:?}", shape),
                actual: format!("{}", targets.len()),
            })?;

        // Compute gradient using trait implementation
        let gradient = match self {
            LossFunctionType::MSE => {
                let loss = MSELoss;
                loss.gradient(outputs_array.view(), targets_array.view())?
            }
            LossFunctionType::CrossEntropy => {
                let loss = CrossEntropyLoss::new();
                loss.gradient(outputs_array.view(), targets_array.view())?
            }
            LossFunctionType::Dice => {
                let loss = DiceLoss::new();
                loss.gradient(outputs_array.view(), targets_array.view())?
            }
            LossFunctionType::Focal => {
                let loss = FocalLoss::new();
                loss.gradient(outputs_array.view(), targets_array.view())?
            }
        };

        // Flatten back to vector
        let (vec, _offset) = gradient.into_raw_vec_and_offset();
        Ok(vec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use scirs2_core::ndarray::arr2;

    #[test]
    fn test_mse_loss() {
        let loss = MSELoss;
        let predictions = arr2(&[[1.0, 2.0], [3.0, 4.0]]);
        let targets = arr2(&[[1.5, 2.5], [3.5, 4.5]]);

        let loss_value = loss
            .compute(predictions.view(), targets.view())
            .expect("Failed to compute MSE loss");

        // Expected: mean of (0.5^2, 0.5^2, 0.5^2, 0.5^2) = 0.25
        assert_relative_eq!(loss_value, 0.25, epsilon = 1e-5);
    }

    #[test]
    fn test_cross_entropy_loss() {
        let loss = CrossEntropyLoss::new();
        let predictions = arr2(&[[2.0, 1.0, 0.1], [1.0, 3.0, 0.2]]);
        let targets = arr2(&[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);

        let loss_value = loss
            .compute(predictions.view(), targets.view())
            .expect("Failed to compute cross-entropy loss");

        assert!(loss_value > 0.0);
        assert!(loss_value < 10.0);
    }

    #[test]
    fn test_dice_loss() {
        let loss = DiceLoss::new();
        let predictions = arr2(&[[0.9, 0.1], [0.8, 0.2]]);
        let targets = arr2(&[[1.0, 0.0], [1.0, 0.0]]);

        let loss_value = loss
            .compute(predictions.view(), targets.view())
            .expect("Failed to compute Dice loss");

        assert!(loss_value >= 0.0);
        assert!(loss_value <= 1.0);
    }

    #[test]
    fn test_focal_loss() {
        let loss = FocalLoss::new();
        let predictions = arr2(&[[0.9, 0.1], [0.8, 0.2]]);
        let targets = arr2(&[[1.0, 0.0], [1.0, 0.0]]);

        let loss_value = loss
            .compute(predictions.view(), targets.view())
            .expect("Failed to compute focal loss");

        assert!(loss_value.is_finite());
    }

    #[test]
    fn test_combined_loss() {
        let mut combined = CombinedLoss::new();
        combined.add_loss(Box::new(MSELoss), 0.5);
        combined.add_loss(Box::new(DiceLoss::new()), 0.5);

        let predictions = arr2(&[[0.9, 0.1], [0.8, 0.2]]);
        let targets = arr2(&[[1.0, 0.0], [1.0, 0.0]]);

        let loss_value = combined
            .compute(predictions.view(), targets.view())
            .expect("Failed to compute combined loss");

        assert!(loss_value.is_finite());
    }

    #[test]
    fn test_gradient_shapes() {
        let loss = MSELoss;
        let predictions = arr2(&[[1.0, 2.0], [3.0, 4.0]]);
        let targets = arr2(&[[1.5, 2.5], [3.5, 4.5]]);

        let grad = loss
            .gradient(predictions.view(), targets.view())
            .expect("Failed to compute gradient");

        assert_eq!(grad.shape(), predictions.shape());
    }
}
