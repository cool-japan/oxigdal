//! Unstructured (element-wise) pruning implementation
//!
//! This module provides the core types and algorithms for unstructured pruning,
//! including weight tensors, pruning masks, importance computation methods,
//! and the `UnstructuredPruner` orchestrator.

use super::{PruningConfig, PruningSchedule, PruningStats};
use crate::error::{MlError, Result};
use std::cmp::Ordering;
use tracing::{debug, info, warn};

/// Weight tensor for pruning operations
///
/// Represents a named tensor with shape information, suitable for
/// layer-wise or global pruning operations.
#[derive(Debug, Clone)]
pub struct WeightTensor {
    /// Weight data in row-major order
    pub data: Vec<f32>,
    /// Shape of the tensor (e.g., [out_channels, in_channels, kernel_h, kernel_w])
    pub shape: Vec<usize>,
    /// Layer name for identification
    pub name: String,
}

impl WeightTensor {
    /// Creates a new weight tensor
    #[must_use]
    pub fn new(data: Vec<f32>, shape: Vec<usize>, name: String) -> Self {
        Self { data, shape, name }
    }

    /// Returns the total number of elements
    #[must_use]
    pub fn numel(&self) -> usize {
        self.data.len()
    }

    /// Returns true if the tensor is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Validates that shape matches data length
    ///
    /// # Errors
    /// Returns an error if shape product does not match data length
    pub fn validate(&self) -> Result<()> {
        let expected_len: usize = self.shape.iter().product();
        if expected_len != self.data.len() {
            return Err(MlError::InvalidConfig(format!(
                "Shape {:?} expects {} elements but got {}",
                self.shape,
                expected_len,
                self.data.len()
            )));
        }
        Ok(())
    }

    /// Computes sparsity (fraction of zero weights)
    #[must_use]
    pub fn sparsity(&self) -> f32 {
        if self.data.is_empty() {
            return 0.0;
        }
        let zero_count = self.data.iter().filter(|&&w| w == 0.0).count();
        zero_count as f32 / self.data.len() as f32
    }

    /// Returns the L1 norm (sum of absolute values)
    #[must_use]
    pub fn l1_norm(&self) -> f32 {
        self.data.iter().map(|w| w.abs()).sum()
    }

    /// Returns the L2 norm (Euclidean norm)
    #[must_use]
    pub fn l2_norm(&self) -> f32 {
        self.data.iter().map(|w| w * w).sum::<f32>().sqrt()
    }

    /// Returns statistics about the weight distribution
    #[must_use]
    pub fn statistics(&self) -> WeightStatistics {
        if self.data.is_empty() {
            return WeightStatistics::default();
        }

        let mut sorted = self.data.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));

        let min = sorted.first().copied().unwrap_or(0.0);
        let max = sorted.last().copied().unwrap_or(0.0);
        let mean = self.data.iter().sum::<f32>() / self.data.len() as f32;

        let variance =
            self.data.iter().map(|w| (w - mean).powi(2)).sum::<f32>() / self.data.len() as f32;
        let std = variance.sqrt();

        let median_idx = self.data.len() / 2;
        let median = if self.data.len() % 2 == 0 {
            (sorted
                .get(median_idx.saturating_sub(1))
                .copied()
                .unwrap_or(0.0)
                + sorted.get(median_idx).copied().unwrap_or(0.0))
                / 2.0
        } else {
            sorted.get(median_idx).copied().unwrap_or(0.0)
        };

        WeightStatistics {
            min,
            max,
            mean,
            std,
            median,
            sparsity: self.sparsity(),
        }
    }
}

/// Statistics about weight distribution
#[derive(Debug, Clone, Default)]
pub struct WeightStatistics {
    /// Minimum weight value
    pub min: f32,
    /// Maximum weight value
    pub max: f32,
    /// Mean weight value
    pub mean: f32,
    /// Standard deviation
    pub std: f32,
    /// Median weight value
    pub median: f32,
    /// Fraction of zero weights
    pub sparsity: f32,
}

/// Pruning mask indicating which weights are kept or pruned
///
/// The mask uses `true` to indicate weights that should be KEPT,
/// and `false` for weights that should be PRUNED (zeroed).
#[derive(Debug, Clone)]
pub struct PruningMask {
    /// Boolean mask (true = keep, false = prune)
    pub mask: Vec<bool>,
    /// Shape of the mask (same as weight tensor)
    pub shape: Vec<usize>,
    /// Optional layer name
    pub name: Option<String>,
}

impl PruningMask {
    /// Creates a new pruning mask
    #[must_use]
    pub fn new(mask: Vec<bool>, shape: Vec<usize>) -> Self {
        Self {
            mask,
            shape,
            name: None,
        }
    }

    /// Creates a new pruning mask with a name
    #[must_use]
    pub fn with_name(mask: Vec<bool>, shape: Vec<usize>, name: String) -> Self {
        Self {
            mask,
            shape,
            name: Some(name),
        }
    }

    /// Creates an all-ones mask (keep everything)
    #[must_use]
    pub fn ones(shape: &[usize]) -> Self {
        let size: usize = shape.iter().product();
        Self {
            mask: vec![true; size],
            shape: shape.to_vec(),
            name: None,
        }
    }

    /// Creates an all-zeros mask (prune everything)
    #[must_use]
    pub fn zeros(shape: &[usize]) -> Self {
        let size: usize = shape.iter().product();
        Self {
            mask: vec![false; size],
            shape: shape.to_vec(),
            name: None,
        }
    }

    /// Returns the number of elements in the mask
    #[must_use]
    pub fn numel(&self) -> usize {
        self.mask.len()
    }

    /// Returns the number of weights kept (not pruned)
    #[must_use]
    pub fn num_kept(&self) -> usize {
        self.mask.iter().filter(|&&m| m).count()
    }

    /// Returns the number of weights pruned
    #[must_use]
    pub fn num_pruned(&self) -> usize {
        self.mask.iter().filter(|&&m| !m).count()
    }

    /// Returns the sparsity (fraction of pruned weights)
    #[must_use]
    pub fn sparsity(&self) -> f32 {
        if self.mask.is_empty() {
            return 0.0;
        }
        self.num_pruned() as f32 / self.mask.len() as f32
    }

    /// Combines two masks with logical AND (both must keep)
    ///
    /// # Errors
    /// Returns an error if mask sizes don't match
    pub fn and(&self, other: &PruningMask) -> Result<PruningMask> {
        if self.mask.len() != other.mask.len() {
            return Err(MlError::InvalidConfig(format!(
                "Mask sizes don't match: {} vs {}",
                self.mask.len(),
                other.mask.len()
            )));
        }

        let combined: Vec<bool> = self
            .mask
            .iter()
            .zip(other.mask.iter())
            .map(|(&a, &b)| a && b)
            .collect();

        Ok(PruningMask::new(combined, self.shape.clone()))
    }

    /// Combines two masks with logical OR (either keeps)
    ///
    /// # Errors
    /// Returns an error if mask sizes don't match
    pub fn or(&self, other: &PruningMask) -> Result<PruningMask> {
        if self.mask.len() != other.mask.len() {
            return Err(MlError::InvalidConfig(format!(
                "Mask sizes don't match: {} vs {}",
                self.mask.len(),
                other.mask.len()
            )));
        }

        let combined: Vec<bool> = self
            .mask
            .iter()
            .zip(other.mask.iter())
            .map(|(&a, &b)| a || b)
            .collect();

        Ok(PruningMask::new(combined, self.shape.clone()))
    }

    /// Inverts the mask
    #[must_use]
    pub fn invert(&self) -> PruningMask {
        PruningMask::new(self.mask.iter().map(|&m| !m).collect(), self.shape.clone())
    }

    /// Applies the mask to a weight tensor, zeroing pruned weights
    ///
    /// # Errors
    /// Returns an error if sizes don't match
    pub fn apply(&self, weights: &WeightTensor) -> Result<WeightTensor> {
        if self.mask.len() != weights.data.len() {
            return Err(MlError::InvalidConfig(format!(
                "Mask size {} doesn't match weight size {}",
                self.mask.len(),
                weights.data.len()
            )));
        }

        let pruned_data: Vec<f32> = weights
            .data
            .iter()
            .zip(self.mask.iter())
            .map(|(&w, &keep)| if keep { w } else { 0.0 })
            .collect();

        Ok(WeightTensor::new(
            pruned_data,
            weights.shape.clone(),
            weights.name.clone(),
        ))
    }
}

/// Importance score computation method for unstructured pruning
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ImportanceMethod {
    /// L1 norm (absolute value): |w|
    #[default]
    L1Norm,
    /// L2 norm (squared magnitude): w^2
    L2Norm,
    /// Gradient-weighted importance: |w * g|
    GradientWeighted,
    /// Taylor expansion: |w * g * a| or first-order Taylor approximation
    TaylorExpansion,
    /// Random scores (baseline for comparison)
    Random {
        /// Seed for reproducibility
        seed: u64,
    },
    /// Movement-based: tracks weight changes during training
    Movement,
    /// Fisher information approximation: g^2 (gradient squared)
    Fisher,
}

/// Gradient information for gradient-based pruning methods
#[derive(Debug, Clone)]
pub struct GradientInfo {
    /// Gradients for each weight
    pub gradients: Vec<f32>,
    /// Optional activations (for Taylor expansion)
    pub activations: Option<Vec<f32>>,
}

impl GradientInfo {
    /// Creates gradient info with gradients only
    #[must_use]
    pub fn new(gradients: Vec<f32>) -> Self {
        Self {
            gradients,
            activations: None,
        }
    }

    /// Creates gradient info with gradients and activations
    #[must_use]
    pub fn with_activations(gradients: Vec<f32>, activations: Vec<f32>) -> Self {
        Self {
            gradients,
            activations: Some(activations),
        }
    }
}

/// State for Lottery Ticket Hypothesis rewinding
///
/// The Lottery Ticket Hypothesis (Frankle & Carlin, 2019) suggests that
/// randomly initialized networks contain sparse subnetworks ("winning tickets")
/// that can achieve comparable accuracy when trained in isolation.
#[derive(Debug, Clone)]
pub struct LotteryTicketState {
    /// Initial weights before any training (for rewinding)
    pub initial_weights: Vec<WeightTensor>,
    /// Current pruning masks learned through training
    pub masks: Vec<PruningMask>,
    /// Current pruning iteration
    pub iteration: usize,
    /// Sparsity at each iteration
    pub sparsity_history: Vec<f32>,
    /// Whether rewinding is enabled
    pub enabled: bool,
}

impl LotteryTicketState {
    /// Creates a new lottery ticket state
    #[must_use]
    pub fn new(initial_weights: Vec<WeightTensor>) -> Self {
        let num_layers = initial_weights.len();
        Self {
            initial_weights,
            masks: Vec::with_capacity(num_layers),
            iteration: 0,
            sparsity_history: Vec::new(),
            enabled: true,
        }
    }

    /// Rewinds weights to initial values while applying current masks
    ///
    /// Returns the rewound weights with masks applied
    pub fn rewind(&self) -> Vec<WeightTensor> {
        if self.masks.is_empty() {
            return self.initial_weights.clone();
        }

        self.initial_weights
            .iter()
            .zip(self.masks.iter())
            .map(|(weights, mask)| {
                // Apply mask, falling back to original if apply fails
                mask.apply(weights).unwrap_or_else(|_| weights.clone())
            })
            .collect()
    }

    /// Updates masks after a pruning iteration
    pub fn update_masks(&mut self, new_masks: Vec<PruningMask>, sparsity: f32) {
        self.masks = new_masks;
        self.iteration += 1;
        self.sparsity_history.push(sparsity);
    }

    /// Returns the current overall sparsity
    #[must_use]
    pub fn current_sparsity(&self) -> f32 {
        if self.masks.is_empty() {
            return 0.0;
        }

        let total_pruned: usize = self.masks.iter().map(|m| m.num_pruned()).sum();
        let total_elements: usize = self.masks.iter().map(|m| m.numel()).sum();

        if total_elements == 0 {
            0.0
        } else {
            total_pruned as f32 / total_elements as f32
        }
    }
}

/// Mask creation mode for pruning
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MaskCreationMode {
    /// Prune weights below a fixed threshold
    Threshold(f32),
    /// Prune a percentage of weights globally
    GlobalPercentage(f32),
    /// Prune a percentage of weights per layer
    LayerWisePercentage(f32),
    /// Keep only top-k weights globally
    TopK(usize),
    /// Keep only top-k weights per layer
    TopKPerLayer(usize),
}

impl Default for MaskCreationMode {
    fn default() -> Self {
        Self::GlobalPercentage(0.5)
    }
}

/// Fine-tuning callback for iterative pruning
///
/// Implement this trait to perform fine-tuning between pruning iterations.
pub trait FineTuneCallback: Send + Sync {
    /// Called after each pruning iteration for fine-tuning
    ///
    /// # Arguments
    /// * `weights` - Current pruned weights
    /// * `masks` - Current pruning masks
    /// * `iteration` - Current pruning iteration (0-indexed)
    /// * `sparsity` - Current sparsity level
    ///
    /// # Returns
    /// Fine-tuned weights
    fn fine_tune(
        &mut self,
        weights: Vec<WeightTensor>,
        masks: &[PruningMask],
        iteration: usize,
        sparsity: f32,
    ) -> Result<Vec<WeightTensor>>;

    /// Returns the number of fine-tuning epochs
    fn epochs(&self) -> usize;
}

/// No-op fine-tuning callback (skips fine-tuning)
pub struct NoOpFineTune;

impl FineTuneCallback for NoOpFineTune {
    fn fine_tune(
        &mut self,
        weights: Vec<WeightTensor>,
        _masks: &[PruningMask],
        _iteration: usize,
        _sparsity: f32,
    ) -> Result<Vec<WeightTensor>> {
        Ok(weights)
    }

    fn epochs(&self) -> usize {
        0
    }
}

/// Unstructured pruner for element-wise weight removal
///
/// This is the main orchestrator for unstructured pruning operations,
/// supporting various importance methods, mask creation modes, and
/// advanced features like lottery ticket rewinding.
pub struct UnstructuredPruner {
    /// Pruning configuration
    config: PruningConfig,
    /// Importance computation method
    importance_method: ImportanceMethod,
    /// Current pruning masks for each layer
    masks: Vec<PruningMask>,
    /// Lottery ticket state for rewinding
    lottery_ticket_state: Option<LotteryTicketState>,
    /// Mask creation mode
    mask_mode: MaskCreationMode,
    /// Current pruning iteration
    current_iteration: usize,
    /// RNG state for random pruning (simple LCG)
    rng_state: u64,
}

impl UnstructuredPruner {
    /// Creates a new unstructured pruner
    #[must_use]
    pub fn new(config: PruningConfig, importance_method: ImportanceMethod) -> Self {
        let seed = match importance_method {
            ImportanceMethod::Random { seed } => seed,
            _ => 42,
        };
        let sparsity_target = config.sparsity_target;

        Self {
            config,
            importance_method,
            masks: Vec::new(),
            lottery_ticket_state: None,
            mask_mode: MaskCreationMode::GlobalPercentage(sparsity_target),
            current_iteration: 0,
            rng_state: seed,
        }
    }

    /// Sets the mask creation mode
    #[must_use]
    pub fn with_mask_mode(mut self, mode: MaskCreationMode) -> Self {
        self.mask_mode = mode;
        self
    }

    /// Enables lottery ticket hypothesis support
    pub fn enable_lottery_ticket(&mut self, initial_weights: Vec<WeightTensor>) {
        self.lottery_ticket_state = Some(LotteryTicketState::new(initial_weights));
    }

    /// Disables lottery ticket support
    pub fn disable_lottery_ticket(&mut self) {
        self.lottery_ticket_state = None;
    }

    /// Returns the current masks
    #[must_use]
    pub fn masks(&self) -> &[PruningMask] {
        &self.masks
    }

    /// Returns the current iteration
    #[must_use]
    pub fn current_iteration(&self) -> usize {
        self.current_iteration
    }

    /// Returns the lottery ticket state if enabled
    #[must_use]
    pub fn lottery_ticket_state(&self) -> Option<&LotteryTicketState> {
        self.lottery_ticket_state.as_ref()
    }

    /// Rewinds to initial weights with current masks (lottery ticket)
    #[must_use]
    pub fn rewind_to_initial(&self) -> Option<Vec<WeightTensor>> {
        self.lottery_ticket_state
            .as_ref()
            .map(|state| state.rewind())
    }

    /// Generates a pseudo-random number (simple LCG)
    fn next_random(&mut self) -> f32 {
        // Linear Congruential Generator: a = 1103515245, c = 12345, m = 2^31
        self.rng_state = self.rng_state.wrapping_mul(1103515245).wrapping_add(12345) % (1u64 << 31);
        (self.rng_state as f32) / ((1u64 << 31) as f32)
    }

    /// Computes importance scores for a weight tensor
    ///
    /// # Arguments
    /// * `weights` - The weight tensor to compute importance for
    /// * `gradient_info` - Optional gradient information for gradient-based methods
    ///
    /// # Returns
    /// Vector of importance scores (same length as weights)
    pub fn compute_importance(
        &mut self,
        weights: &WeightTensor,
        gradient_info: Option<&GradientInfo>,
    ) -> Vec<f32> {
        match self.importance_method {
            ImportanceMethod::L1Norm => weights.data.iter().map(|w| w.abs()).collect(),
            ImportanceMethod::L2Norm => weights.data.iter().map(|w| w * w).collect(),
            ImportanceMethod::GradientWeighted => {
                if let Some(info) = gradient_info {
                    if info.gradients.len() == weights.data.len() {
                        weights
                            .data
                            .iter()
                            .zip(info.gradients.iter())
                            .map(|(w, g)| (w * g).abs())
                            .collect()
                    } else {
                        warn!(
                            "Gradient size mismatch, falling back to L1 norm. \
                            Weights: {}, Gradients: {}",
                            weights.data.len(),
                            info.gradients.len()
                        );
                        weights.data.iter().map(|w| w.abs()).collect()
                    }
                } else {
                    warn!("No gradient info provided, falling back to L1 norm");
                    weights.data.iter().map(|w| w.abs()).collect()
                }
            }
            ImportanceMethod::TaylorExpansion => {
                if let Some(info) = gradient_info {
                    if info.gradients.len() == weights.data.len() {
                        if let Some(ref activations) = info.activations {
                            if activations.len() == weights.data.len() {
                                // Full Taylor: |w * g * a|
                                weights
                                    .data
                                    .iter()
                                    .zip(info.gradients.iter())
                                    .zip(activations.iter())
                                    .map(|((w, g), a)| (w * g * a).abs())
                                    .collect()
                            } else {
                                // First-order Taylor: |w * g|
                                weights
                                    .data
                                    .iter()
                                    .zip(info.gradients.iter())
                                    .map(|(w, g)| (w * g).abs())
                                    .collect()
                            }
                        } else {
                            // First-order Taylor: |w * g|
                            weights
                                .data
                                .iter()
                                .zip(info.gradients.iter())
                                .map(|(w, g)| (w * g).abs())
                                .collect()
                        }
                    } else {
                        warn!("Gradient size mismatch, falling back to L1 norm");
                        weights.data.iter().map(|w| w.abs()).collect()
                    }
                } else {
                    warn!("No gradient info for Taylor, falling back to L1 norm");
                    weights.data.iter().map(|w| w.abs()).collect()
                }
            }
            ImportanceMethod::Random { .. } => (0..weights.data.len())
                .map(|_| self.next_random())
                .collect(),
            ImportanceMethod::Movement => {
                // Movement pruning: importance based on weight magnitude change
                // Without historical data, fall back to L1 norm
                weights.data.iter().map(|w| w.abs()).collect()
            }
            ImportanceMethod::Fisher => {
                // Fisher information: gradient squared
                if let Some(info) = gradient_info {
                    if info.gradients.len() == weights.data.len() {
                        info.gradients.iter().map(|g| g * g).collect()
                    } else {
                        warn!("Gradient size mismatch for Fisher, falling back to L1");
                        weights.data.iter().map(|w| w.abs()).collect()
                    }
                } else {
                    warn!("No gradient info for Fisher, falling back to L1 norm");
                    weights.data.iter().map(|w| w.abs()).collect()
                }
            }
        }
    }

    /// Creates a pruning mask based on importance scores and mask mode
    ///
    /// # Arguments
    /// * `importance` - Importance scores for each weight
    /// * `shape` - Shape of the weight tensor
    pub fn create_mask(&self, importance: &[f32], shape: &[usize]) -> PruningMask {
        let num_weights = importance.len();
        if num_weights == 0 {
            return PruningMask::new(Vec::new(), shape.to_vec());
        }

        match self.mask_mode {
            MaskCreationMode::Threshold(threshold) => {
                let mask: Vec<bool> = importance.iter().map(|&s| s >= threshold).collect();
                PruningMask::new(mask, shape.to_vec())
            }
            MaskCreationMode::GlobalPercentage(sparsity)
            | MaskCreationMode::LayerWisePercentage(sparsity) => {
                let num_to_prune =
                    ((num_weights as f32 * sparsity).round() as usize).min(num_weights);

                // Create indexed importance scores
                let mut indexed: Vec<(usize, f32)> = importance
                    .iter()
                    .enumerate()
                    .map(|(i, &s)| (i, s))
                    .collect();

                // Sort by importance (ascending - lowest importance first)
                indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));

                // Create mask: prune the num_to_prune lowest importance weights
                let mut mask = vec![true; num_weights];
                for (idx, _) in indexed.iter().take(num_to_prune) {
                    mask[*idx] = false;
                }

                PruningMask::new(mask, shape.to_vec())
            }
            MaskCreationMode::TopK(k) | MaskCreationMode::TopKPerLayer(k) => {
                let num_to_keep = k.min(num_weights);

                // Create indexed importance scores
                let mut indexed: Vec<(usize, f32)> = importance
                    .iter()
                    .enumerate()
                    .map(|(i, &s)| (i, s))
                    .collect();

                // Sort by importance (descending - highest importance first)
                indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));

                // Create mask: keep only top-k
                let mut mask = vec![false; num_weights];
                for (idx, _) in indexed.iter().take(num_to_keep) {
                    mask[*idx] = true;
                }

                PruningMask::new(mask, shape.to_vec())
            }
        }
    }

    /// Prunes a single weight tensor
    ///
    /// # Arguments
    /// * `weights` - The weight tensor to prune
    ///
    /// # Returns
    /// Tuple of (pruned weights, mask)
    ///
    /// # Errors
    /// Returns an error if pruning fails
    pub fn prune_tensor(&mut self, weights: &WeightTensor) -> Result<(WeightTensor, PruningMask)> {
        self.prune_tensor_with_gradients(weights, None)
    }

    /// Prunes a single weight tensor with gradient information
    ///
    /// # Arguments
    /// * `weights` - The weight tensor to prune
    /// * `gradient_info` - Optional gradient information
    ///
    /// # Returns
    /// Tuple of (pruned weights, mask)
    ///
    /// # Errors
    /// Returns an error if pruning fails
    pub fn prune_tensor_with_gradients(
        &mut self,
        weights: &WeightTensor,
        gradient_info: Option<&GradientInfo>,
    ) -> Result<(WeightTensor, PruningMask)> {
        // Validate input
        weights.validate()?;

        if weights.is_empty() {
            return Ok((
                weights.clone(),
                PruningMask::new(Vec::new(), weights.shape.clone()),
            ));
        }

        // Compute importance scores
        let importance = self.compute_importance(weights, gradient_info);

        // Create mask
        let mask = self.create_mask(&importance, &weights.shape);

        // Apply mask
        let pruned = mask.apply(weights)?;

        debug!(
            "Pruned tensor '{}': {:.1}% sparsity ({} -> {} non-zero)",
            weights.name,
            mask.sparsity() * 100.0,
            weights.numel(),
            mask.num_kept()
        );

        Ok((pruned, mask))
    }

    /// Prunes multiple weight tensors globally
    ///
    /// For global pruning, importance scores are computed across all tensors
    /// and a single threshold is applied.
    ///
    /// # Arguments
    /// * `tensors` - Weight tensors to prune
    ///
    /// # Returns
    /// Tuple of (pruned tensors, masks)
    ///
    /// # Errors
    /// Returns an error if pruning fails
    pub fn prune_tensors_global(
        &mut self,
        tensors: &[WeightTensor],
    ) -> Result<(Vec<WeightTensor>, Vec<PruningMask>)> {
        self.prune_tensors_global_with_gradients(tensors, &[])
    }

    /// Prunes multiple weight tensors globally with gradient information
    ///
    /// # Arguments
    /// * `tensors` - Weight tensors to prune
    /// * `gradient_infos` - Gradient information for each tensor
    ///
    /// # Returns
    /// Tuple of (pruned tensors, masks)
    ///
    /// # Errors
    /// Returns an error if pruning fails
    pub fn prune_tensors_global_with_gradients(
        &mut self,
        tensors: &[WeightTensor],
        gradient_infos: &[GradientInfo],
    ) -> Result<(Vec<WeightTensor>, Vec<PruningMask>)> {
        if tensors.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }

        // Validate all tensors
        for tensor in tensors {
            tensor.validate()?;
        }

        // Collect all importance scores with their tensor and element indices
        let mut all_scores: Vec<(usize, usize, f32)> = Vec::new();

        for (tensor_idx, tensor) in tensors.iter().enumerate() {
            let gradient_info = gradient_infos.get(tensor_idx);
            let importance = self.compute_importance(tensor, gradient_info);

            for (elem_idx, &score) in importance.iter().enumerate() {
                all_scores.push((tensor_idx, elem_idx, score));
            }
        }

        // Sort by importance (ascending)
        all_scores.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(Ordering::Equal));

        // Determine how many to prune based on mode
        let total_weights = all_scores.len();
        let num_to_prune = match self.mask_mode {
            MaskCreationMode::GlobalPercentage(sparsity) => {
                ((total_weights as f32 * sparsity).round() as usize).min(total_weights)
            }
            MaskCreationMode::TopK(k) => total_weights.saturating_sub(k),
            MaskCreationMode::Threshold(threshold) => {
                all_scores.iter().filter(|(_, _, s)| *s < threshold).count()
            }
            MaskCreationMode::LayerWisePercentage(_) | MaskCreationMode::TopKPerLayer(_) => {
                // For layer-wise modes, fall back to per-tensor pruning
                return self.prune_tensors_layerwise_with_gradients(tensors, gradient_infos);
            }
        };

        // Create masks for each tensor
        let mut masks: Vec<Vec<bool>> = tensors.iter().map(|t| vec![true; t.data.len()]).collect();

        // Mark weights to prune
        for (tensor_idx, elem_idx, _) in all_scores.iter().take(num_to_prune) {
            if let Some(mask) = masks.get_mut(*tensor_idx) {
                if let Some(elem) = mask.get_mut(*elem_idx) {
                    *elem = false;
                }
            }
        }

        // Create PruningMask objects and apply to tensors
        let mut result_tensors = Vec::with_capacity(tensors.len());
        let mut result_masks = Vec::with_capacity(tensors.len());

        for (tensor, mask_vec) in tensors.iter().zip(masks) {
            let mask = PruningMask::with_name(mask_vec, tensor.shape.clone(), tensor.name.clone());
            let pruned = mask.apply(tensor)?;
            result_tensors.push(pruned);
            result_masks.push(mask);
        }

        // Update internal masks
        self.masks = result_masks.clone();

        // Calculate sparsity before borrowing lottery_ticket_state mutably
        let overall_sparsity = self.current_sparsity();

        // Update lottery ticket state if enabled
        if let Some(ref mut lts) = self.lottery_ticket_state {
            lts.update_masks(result_masks.clone(), overall_sparsity);
        }

        info!(
            "Global pruning complete: {:.1}% overall sparsity ({} tensors)",
            overall_sparsity * 100.0,
            tensors.len()
        );

        Ok((result_tensors, result_masks))
    }

    /// Prunes multiple weight tensors layer-wise
    ///
    /// Each tensor is pruned independently with the same sparsity target.
    ///
    /// # Arguments
    /// * `tensors` - Weight tensors to prune
    ///
    /// # Returns
    /// Tuple of (pruned tensors, masks)
    ///
    /// # Errors
    /// Returns an error if pruning fails
    pub fn prune_tensors_layerwise(
        &mut self,
        tensors: &[WeightTensor],
    ) -> Result<(Vec<WeightTensor>, Vec<PruningMask>)> {
        self.prune_tensors_layerwise_with_gradients(tensors, &[])
    }

    /// Prunes multiple weight tensors layer-wise with gradient information
    ///
    /// # Arguments
    /// * `tensors` - Weight tensors to prune
    /// * `gradient_infos` - Gradient information for each tensor
    ///
    /// # Returns
    /// Tuple of (pruned tensors, masks)
    ///
    /// # Errors
    /// Returns an error if pruning fails
    pub fn prune_tensors_layerwise_with_gradients(
        &mut self,
        tensors: &[WeightTensor],
        gradient_infos: &[GradientInfo],
    ) -> Result<(Vec<WeightTensor>, Vec<PruningMask>)> {
        let mut result_tensors = Vec::with_capacity(tensors.len());
        let mut result_masks = Vec::with_capacity(tensors.len());

        for (i, tensor) in tensors.iter().enumerate() {
            let gradient_info = gradient_infos.get(i);
            let (pruned, mask) = self.prune_tensor_with_gradients(tensor, gradient_info)?;
            result_tensors.push(pruned);
            result_masks.push(mask);
        }

        // Update internal masks
        self.masks = result_masks.clone();

        // Calculate sparsity before borrowing lottery_ticket_state mutably
        let overall_sparsity = self.current_sparsity();

        // Update lottery ticket state if enabled
        if let Some(ref mut lts) = self.lottery_ticket_state {
            lts.update_masks(result_masks.clone(), overall_sparsity);
        }

        info!(
            "Layer-wise pruning complete: {:.1}% overall sparsity ({} tensors)",
            overall_sparsity * 100.0,
            tensors.len()
        );

        Ok((result_tensors, result_masks))
    }

    /// Returns the current overall sparsity
    #[must_use]
    pub fn current_sparsity(&self) -> f32 {
        if self.masks.is_empty() {
            return 0.0;
        }

        let total_pruned: usize = self.masks.iter().map(|m| m.num_pruned()).sum();
        let total_elements: usize = self.masks.iter().map(|m| m.numel()).sum();

        if total_elements == 0 {
            0.0
        } else {
            total_pruned as f32 / total_elements as f32
        }
    }

    /// Performs iterative pruning with fine-tuning
    ///
    /// # Arguments
    /// * `initial_weights` - Initial weight tensors
    /// * `callback` - Fine-tuning callback
    ///
    /// # Returns
    /// Final pruned weights and masks after all iterations
    ///
    /// # Errors
    /// Returns an error if pruning or fine-tuning fails
    pub fn iterative_prune<F: FineTuneCallback>(
        &mut self,
        initial_weights: Vec<WeightTensor>,
        callback: &mut F,
    ) -> Result<(Vec<WeightTensor>, Vec<PruningMask>)> {
        let iterations = match self.config.schedule {
            PruningSchedule::Iterative { iterations } => iterations,
            PruningSchedule::Polynomial { steps, .. } => steps,
            PruningSchedule::OneShot => 1,
        };

        let mut current_weights = initial_weights;

        for i in 0..iterations {
            // Compute current target sparsity
            let target_sparsity = match self.config.schedule {
                PruningSchedule::Polynomial {
                    initial_sparsity,
                    final_sparsity,
                    steps,
                } => {
                    let t = i as f32;
                    let total = steps as f32;
                    let s_i = initial_sparsity as f32 / 100.0;
                    let s_f = final_sparsity as f32 / 100.0;
                    s_f + (s_i - s_f) * (1.0 - t / total).powi(3)
                }
                PruningSchedule::Iterative { iterations: n } => {
                    self.config.sparsity_target * ((i + 1) as f32 / n as f32)
                }
                PruningSchedule::OneShot => self.config.sparsity_target,
            };

            // Update mask mode with current sparsity target
            self.mask_mode = MaskCreationMode::GlobalPercentage(target_sparsity);

            info!(
                "Iteration {}/{}: target sparsity {:.1}%",
                i + 1,
                iterations,
                target_sparsity * 100.0
            );

            // Prune
            let (pruned, masks) = self.prune_tensors_global(&current_weights)?;
            self.current_iteration = i + 1;

            // Fine-tune if not the last iteration (or if configured)
            current_weights = if self.config.fine_tune && i < iterations - 1 {
                let actual_sparsity = self.current_sparsity();
                callback.fine_tune(pruned, &masks, i, actual_sparsity)?
            } else {
                pruned
            };
        }

        let final_masks = self.masks.clone();
        Ok((current_weights, final_masks))
    }

    /// Computes pruning statistics
    #[must_use]
    pub fn compute_stats(&self, original_tensors: &[WeightTensor]) -> PruningStats {
        let original_params: usize = original_tensors.iter().map(|t| t.numel()).sum();

        let pruned_params = if self.masks.is_empty() {
            original_params
        } else {
            self.masks.iter().map(|m| m.num_kept()).sum()
        };

        let actual_sparsity = self.current_sparsity();

        PruningStats {
            original_params,
            pruned_params,
            actual_sparsity,
        }
    }
}
