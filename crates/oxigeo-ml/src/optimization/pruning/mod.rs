//! Model pruning for sparse neural networks
//!
//! Pruning removes unnecessary weights and connections from neural networks,
//! reducing model size and improving inference speed.
//!
//! # Unstructured Pruning
//!
//! This module provides comprehensive unstructured (element-wise) pruning with:
//!
//! - **Multiple importance methods**: L1 norm, L2 norm, gradient-based, Taylor expansion, random
//! - **Flexible mask creation**: By threshold or percentage
//! - **Iterative pruning**: Gradual sparsity increase with fine-tuning
//! - **Lottery Ticket Hypothesis**: Weight rewinding for finding winning tickets
//!
//! # Example: Basic Unstructured Pruning
//!
//! ```
//! use oxigeo_ml::optimization::pruning::{
//!     UnstructuredPruner, WeightTensor, ImportanceMethod, PruningConfig
//! };
//! use oxigeo_ml::error::Result;
//!
//! # fn main() -> Result<()> {
//! // Create weight tensor
//! let weights = WeightTensor::new(
//!     vec![0.1, -0.5, 0.3, -0.8, 0.2, -0.1, 0.7, -0.4],
//!     vec![2, 4],
//!     "layer1.weight".to_string(),
//! );
//!
//! // Create pruner with magnitude-based importance
//! let config = PruningConfig::builder()
//!     .sparsity_target(0.5)
//!     .build();
//! let mut pruner = UnstructuredPruner::new(config, ImportanceMethod::L1Norm);
//!
//! // Prune weights
//! let (pruned_weights, mask) = pruner.prune_tensor(&weights)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Example: Lottery Ticket Hypothesis
//!
//! ```
//! use oxigeo_ml::optimization::pruning::{
//!     UnstructuredPruner, WeightTensor, ImportanceMethod, PruningConfig, PruningSchedule,
//! };
//!
//! // Initial weights before training
//! let initial_weights = vec![
//!     WeightTensor::new(vec![0.1, 0.2, 0.3, 0.4], vec![2, 2], "layer1".to_string()),
//! ];
//!
//! // Create pruner with lottery ticket support
//! let config = PruningConfig::builder()
//!     .sparsity_target(0.5)
//!     .schedule(PruningSchedule::Iterative { iterations: 3 })
//!     .build();
//! let mut pruner = UnstructuredPruner::new(config, ImportanceMethod::L1Norm);
//!
//! // Enable lottery ticket rewinding
//! pruner.enable_lottery_ticket(initial_weights);
//!
//! // After training, you can rewind to initial weights with learned mask
//! if let Some(rewound) = pruner.rewind_to_initial() {
//!     // Use rewound weights for training from scratch
//! }
//! ```

#[cfg(test)]
mod tests;
mod unstructured;

pub use unstructured::{
    FineTuneCallback, GradientInfo, ImportanceMethod, LotteryTicketState, MaskCreationMode,
    NoOpFineTune, PruningMask, UnstructuredPruner, WeightStatistics, WeightTensor,
};

use crate::error::{MlError, ModelError, Result};
use crate::optimization::onnx_weights;
use std::cmp::Ordering;
use std::path::Path;
use tracing::{debug, info};

/// Pruning strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruningStrategy {
    /// Magnitude-based pruning (remove small weights)
    Magnitude,
    /// Structured pruning (remove entire filters/channels)
    Structured,
    /// Gradient-based pruning
    Gradient,
    /// Taylor expansion-based pruning
    Taylor,
    /// Random pruning (baseline)
    Random,
}

/// Pruning schedule
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruningSchedule {
    /// One-shot pruning
    OneShot,
    /// Iterative pruning with gradual sparsity increase
    Iterative {
        /// Number of iterations
        iterations: usize,
    },
    /// Polynomial decay schedule
    Polynomial {
        /// Initial sparsity
        initial_sparsity: u8,
        /// Final sparsity (0-100)
        final_sparsity: u8,
        /// Number of steps
        steps: usize,
    },
}

/// Pruning granularity
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruningGranularity {
    /// Element-wise (unstructured)
    Element,
    /// Entire neurons
    Neuron,
    /// Entire filters/channels
    Channel,
    /// Blocks of weights
    Block {
        /// Block size
        size: usize,
    },
}

/// Pruning configuration
#[derive(Debug, Clone)]
pub struct PruningConfig {
    /// Pruning strategy
    pub strategy: PruningStrategy,
    /// Target sparsity (0.0 to 1.0)
    pub sparsity_target: f32,
    /// Pruning schedule
    pub schedule: PruningSchedule,
    /// Pruning granularity
    pub granularity: PruningGranularity,
    /// Fine-tune after pruning
    pub fine_tune: bool,
    /// Number of fine-tuning epochs
    pub fine_tune_epochs: usize,
}

impl Default for PruningConfig {
    fn default() -> Self {
        Self {
            strategy: PruningStrategy::Magnitude,
            sparsity_target: 0.5,
            schedule: PruningSchedule::OneShot,
            granularity: PruningGranularity::Element,
            fine_tune: true,
            fine_tune_epochs: 10,
        }
    }
}

impl PruningConfig {
    /// Creates a configuration builder
    #[must_use]
    pub fn builder() -> PruningConfigBuilder {
        PruningConfigBuilder::default()
    }
}

/// Builder for pruning configuration
#[derive(Debug, Default)]
pub struct PruningConfigBuilder {
    strategy: Option<PruningStrategy>,
    sparsity_target: Option<f32>,
    schedule: Option<PruningSchedule>,
    granularity: Option<PruningGranularity>,
    fine_tune: bool,
    fine_tune_epochs: Option<usize>,
}

impl PruningConfigBuilder {
    /// Sets the pruning strategy
    #[must_use]
    pub fn strategy(mut self, strategy: PruningStrategy) -> Self {
        self.strategy = Some(strategy);
        self
    }

    /// Sets the sparsity target
    #[must_use]
    pub fn sparsity_target(mut self, sparsity: f32) -> Self {
        self.sparsity_target = Some(sparsity.clamp(0.0, 1.0));
        self
    }

    /// Sets the pruning schedule
    #[must_use]
    pub fn schedule(mut self, schedule: PruningSchedule) -> Self {
        self.schedule = Some(schedule);
        self
    }

    /// Sets the pruning granularity
    #[must_use]
    pub fn granularity(mut self, granularity: PruningGranularity) -> Self {
        self.granularity = Some(granularity);
        self
    }

    /// Enables fine-tuning after pruning
    #[must_use]
    pub fn fine_tune(mut self, enable: bool) -> Self {
        self.fine_tune = enable;
        self
    }

    /// Sets fine-tuning epochs
    #[must_use]
    pub fn fine_tune_epochs(mut self, epochs: usize) -> Self {
        self.fine_tune_epochs = Some(epochs);
        self
    }

    /// Builds the configuration
    #[must_use]
    pub fn build(self) -> PruningConfig {
        PruningConfig {
            strategy: self.strategy.unwrap_or(PruningStrategy::Magnitude),
            sparsity_target: self.sparsity_target.unwrap_or(0.5),
            schedule: self.schedule.unwrap_or(PruningSchedule::OneShot),
            granularity: self.granularity.unwrap_or(PruningGranularity::Element),
            fine_tune: self.fine_tune,
            fine_tune_epochs: self.fine_tune_epochs.unwrap_or(10),
        }
    }
}

/// Prunes a model according to the configuration
///
/// # Errors
/// Returns an error if pruning fails
pub fn prune_model<P: AsRef<Path>>(
    input_path: P,
    output_path: P,
    config: &PruningConfig,
) -> Result<PruningStats> {
    let input = input_path.as_ref();
    let output = output_path.as_ref();

    info!(
        "Pruning model {:?} to {:?} (strategy: {:?}, sparsity: {:.1}%)",
        input,
        output,
        config.strategy,
        config.sparsity_target * 100.0
    );

    if !input.exists() {
        return Err(MlError::InvalidConfig(format!(
            "Input model not found: {}",
            input.display()
        )));
    }

    // Apply pruning based on strategy
    let stats = match config.strategy {
        PruningStrategy::Structured => structured_pruning(input, output, config)?,
        _ => unstructured_pruning(input, output, config)?,
    };

    info!(
        "Pruning complete: {:.1}% sparsity, {:.1}% size reduction",
        stats.actual_sparsity * 100.0,
        stats.size_reduction_percent()
    );

    Ok(stats)
}

/// Performs structured pruning (removes entire filters/channels)
///
/// # Errors
/// Returns an error if pruning fails
pub fn structured_pruning<P: AsRef<Path>>(
    input_path: P,
    output_path: P,
    config: &PruningConfig,
) -> Result<PruningStats> {
    let input = input_path.as_ref();
    let output = output_path.as_ref();

    debug!("Applying structured pruning");

    // Structured pruning removes whole output channels/filters. Physically
    // resizing weight tensors would require propagating the new shapes through
    // every downstream consumer (Conv -> BN -> next Conv, etc.), which is a
    // graph-wide transformation. Instead we perform *structured sparsity*: the
    // lowest-importance output channels (by L2 norm) of every ≥2-D weight
    // initializer are zeroed in place. The output remains a byte-for-byte valid
    // ONNX model and the reported statistics reflect the real weights that were
    // zeroed — no fabricated parameter counts.
    let mut file_data = std::fs::read(input)?;
    let initializers = onnx_weights::parse_float_initializers(&file_data)?;

    let mut original_params = 0usize;
    let mut kept_params = 0usize;
    let mut pruned_tensors = 0usize;

    for init in &initializers {
        let shape = init.normalized_shape();
        if shape.len() < 2 {
            // 1-D tensors (biases, scales) have no channel structure.
            continue;
        }
        let out_channels = shape[0];
        if out_channels == 0 {
            continue;
        }
        let channel_size = init.values.len() / out_channels;
        if channel_size == 0 || out_channels * channel_size != init.values.len() {
            continue;
        }

        // L2 norm of each output channel.
        let mut norms: Vec<(usize, f32)> = (0..out_channels)
            .map(|c| {
                let start = c * channel_size;
                let norm = init.values[start..start + channel_size]
                    .iter()
                    .map(|w| w * w)
                    .sum::<f32>()
                    .sqrt();
                (c, norm)
            })
            .collect();
        norms.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));

        let num_prune_channels =
            ((out_channels as f32 * config.sparsity_target).round() as usize).min(out_channels);

        let mut keep = vec![true; init.values.len()];
        for (channel, _) in norms.iter().take(num_prune_channels) {
            let start = channel * channel_size;
            for slot in keep.iter_mut().skip(start).take(channel_size) {
                *slot = false;
            }
        }

        let kept = keep.iter().filter(|&&k| k).count();
        onnx_weights::zero_values_in_place(&mut file_data, init, &keep);

        original_params += init.values.len();
        kept_params += kept;
        pruned_tensors += 1;
    }

    if pruned_tensors == 0 {
        return Err(ModelError::InvalidFormat {
            message: "structured pruning found no multi-dimensional float32 weight tensors \
                      (external weights or unsupported encoding)"
                .to_string(),
        }
        .into());
    }

    std::fs::write(output, &file_data)?;

    let actual_sparsity = if original_params > 0 {
        1.0 - (kept_params as f32 / original_params as f32)
    } else {
        0.0
    };

    info!(
        "Structured pruning applied to {} tensor(s): {} -> {} parameters ({:.1}% sparsity)",
        pruned_tensors,
        original_params,
        kept_params,
        actual_sparsity * 100.0
    );

    Ok(PruningStats {
        original_params,
        pruned_params: kept_params,
        actual_sparsity,
    })
}

/// Performs unstructured pruning (removes individual weights)
///
/// This function loads a model file, extracts weight tensors, applies
/// unstructured pruning based on the configuration, and saves the modified model.
///
/// # Arguments
/// * `input_path` - Path to the input model file
/// * `output_path` - Path to save the pruned model
/// * `config` - Pruning configuration
///
/// # Returns
/// Pruning statistics including original/pruned parameter counts and actual sparsity
///
/// # Errors
/// Returns an error if:
/// - Input file cannot be read
/// - Output file cannot be written
/// - Pruning computation fails
///
/// # Example
/// ```no_run
/// use oxigeo_ml::optimization::pruning::{unstructured_pruning, PruningConfig, PruningStrategy};
/// use oxigeo_ml::error::Result;
///
/// # fn main() -> Result<()> {
/// let config = PruningConfig::builder()
///     .strategy(PruningStrategy::Magnitude)
///     .sparsity_target(0.5)
///     .build();
///
/// let stats = unstructured_pruning("model.onnx", "model_pruned.onnx", &config)?;
/// println!("Achieved {:.1}% sparsity", stats.actual_sparsity * 100.0);
/// # Ok(())
/// # }
/// ```
pub fn unstructured_pruning<P: AsRef<Path>>(
    input_path: P,
    output_path: P,
    config: &PruningConfig,
) -> Result<PruningStats> {
    let input = input_path.as_ref();
    let output = output_path.as_ref();

    debug!(
        "Applying unstructured pruning with {:?} strategy",
        config.strategy
    );

    // Determine importance method based on strategy
    let importance_method = match config.strategy {
        PruningStrategy::Magnitude => ImportanceMethod::L1Norm,
        PruningStrategy::Gradient => ImportanceMethod::GradientWeighted,
        PruningStrategy::Taylor => ImportanceMethod::TaylorExpansion,
        PruningStrategy::Random => ImportanceMethod::Random { seed: 42 },
        PruningStrategy::Structured => {
            // Structured pruning should use structured_pruning function
            return structured_pruning(input_path, output_path, config);
        }
    };

    // Parse the real ONNX model and extract its actual float32 weight
    // initializers. This validates that the input is a genuine ONNX model and
    // never touches non-weight bytes.
    let mut file_data = std::fs::read(input)?;
    let initializers = onnx_weights::parse_float_initializers(&file_data)?;

    if initializers.is_empty() {
        return Err(ModelError::InvalidFormat {
            message: "no inline float32 weight initializers found to prune \
                      (external weights or unsupported encoding)"
                .to_string(),
        }
        .into());
    }

    // Build weight tensors from the real initializer data.
    let weights: Vec<WeightTensor> = initializers
        .iter()
        .map(|init| {
            WeightTensor::new(
                init.values.clone(),
                init.normalized_shape(),
                init.name.clone(),
            )
        })
        .collect();
    let original_params: usize = weights.iter().map(WeightTensor::numel).sum();

    // Create pruner with configuration and apply global magnitude pruning
    // across all weight tensors.
    let mut pruner = UnstructuredPruner::new(config.clone(), importance_method);
    let (_pruned_weights, masks) = pruner.prune_tensors_global(&weights)?;

    // Compute actual statistics from the real masks.
    let pruned_params: usize = masks.iter().map(PruningMask::num_kept).sum();
    let actual_sparsity = if original_params > 0 {
        1.0 - (pruned_params as f32 / original_params as f32)
    } else {
        0.0
    };

    // Write the pruned weights back into the ONNX file in place. Because we only
    // zero existing float values (no field length changes), the output remains a
    // valid, loadable ONNX model.
    for (init, mask) in initializers.iter().zip(masks.iter()) {
        onnx_weights::zero_values_in_place(&mut file_data, init, &mask.mask);
    }
    std::fs::write(output, &file_data)?;

    info!(
        "Unstructured pruning complete: {} -> {} parameters ({:.1}% sparsity)",
        original_params,
        pruned_params,
        actual_sparsity * 100.0
    );

    Ok(PruningStats {
        original_params,
        pruned_params,
        actual_sparsity,
    })
}

/// Prunes weight tensors directly (in-memory operation)
///
/// This is a convenience function for direct tensor pruning without file I/O.
///
/// # Arguments
/// * `weights` - Weight tensors to prune
/// * `config` - Pruning configuration
///
/// # Returns
/// Tuple of (pruned tensors, masks, stats)
///
/// # Errors
/// Returns an error if pruning fails
///
/// # Example
/// ```
/// use oxigeo_ml::optimization::pruning::{
///     prune_weights_direct, WeightTensor, PruningConfig, PruningStrategy
/// };
/// use oxigeo_ml::error::Result;
///
/// # fn main() -> Result<()> {
/// let weights = vec![
///     WeightTensor::new(vec![0.1, -0.5, 0.3, -0.8], vec![2, 2], "layer1".to_string()),
///     WeightTensor::new(vec![0.2, -0.1, 0.7, -0.4], vec![2, 2], "layer2".to_string()),
/// ];
///
/// let config = PruningConfig::builder()
///     .strategy(PruningStrategy::Magnitude)
///     .sparsity_target(0.5)
///     .build();
///
/// let (pruned, masks, stats) = prune_weights_direct(&weights, &config)?;
/// println!("Sparsity: {:.1}%", stats.actual_sparsity * 100.0);
/// # Ok(())
/// # }
/// ```
pub fn prune_weights_direct(
    weights: &[WeightTensor],
    config: &PruningConfig,
) -> Result<(Vec<WeightTensor>, Vec<PruningMask>, PruningStats)> {
    let importance_method = match config.strategy {
        PruningStrategy::Magnitude => ImportanceMethod::L1Norm,
        PruningStrategy::Gradient => ImportanceMethod::GradientWeighted,
        PruningStrategy::Taylor => ImportanceMethod::TaylorExpansion,
        PruningStrategy::Random => ImportanceMethod::Random { seed: 42 },
        PruningStrategy::Structured => ImportanceMethod::L2Norm, // Use L2 for structured
    };

    let mut pruner = UnstructuredPruner::new(config.clone(), importance_method);
    let (pruned_weights, masks) = pruner.prune_tensors_global(weights)?;
    let stats = pruner.compute_stats(weights);

    Ok((pruned_weights, masks, stats))
}

/// Prunes weight tensors with gradient information
///
/// Use this function when gradient information is available for
/// gradient-based or Taylor expansion pruning.
///
/// # Arguments
/// * `weights` - Weight tensors to prune
/// * `gradients` - Gradient information for each tensor
/// * `config` - Pruning configuration
///
/// # Returns
/// Tuple of (pruned tensors, masks, stats)
///
/// # Errors
/// Returns an error if pruning fails
pub fn prune_weights_with_gradients(
    weights: &[WeightTensor],
    gradients: &[GradientInfo],
    config: &PruningConfig,
) -> Result<(Vec<WeightTensor>, Vec<PruningMask>, PruningStats)> {
    let importance_method = match config.strategy {
        PruningStrategy::Magnitude => ImportanceMethod::L1Norm,
        PruningStrategy::Gradient => ImportanceMethod::GradientWeighted,
        PruningStrategy::Taylor => ImportanceMethod::TaylorExpansion,
        PruningStrategy::Random => ImportanceMethod::Random { seed: 42 },
        PruningStrategy::Structured => ImportanceMethod::L2Norm,
    };

    let mut pruner = UnstructuredPruner::new(config.clone(), importance_method);
    let (pruned_weights, masks) = pruner.prune_tensors_global_with_gradients(weights, gradients)?;
    let stats = pruner.compute_stats(weights);

    Ok((pruned_weights, masks, stats))
}

/// Pruning statistics
#[derive(Debug, Clone)]
pub struct PruningStats {
    /// Original parameter count
    pub original_params: usize,
    /// Remaining parameters after pruning
    pub pruned_params: usize,
    /// Actual sparsity achieved
    pub actual_sparsity: f32,
}

impl PruningStats {
    /// Returns the number of parameters removed
    #[must_use]
    pub fn params_removed(&self) -> usize {
        self.original_params.saturating_sub(self.pruned_params)
    }

    /// Returns the size reduction percentage
    #[must_use]
    pub fn size_reduction_percent(&self) -> f32 {
        if self.original_params > 0 {
            (self.params_removed() as f32 / self.original_params as f32) * 100.0
        } else {
            0.0
        }
    }
}

// ============================================================================
// Standalone helper functions
// ============================================================================

/// Computes importance scores for weights using magnitude
#[must_use]
pub fn compute_magnitude_importance(weights: &[f32]) -> Vec<f32> {
    weights.iter().map(|w| w.abs()).collect()
}

/// Computes importance scores using gradient information
#[must_use]
pub fn compute_gradient_importance(weights: &[f32], gradients: &[f32]) -> Vec<f32> {
    weights
        .iter()
        .zip(gradients.iter())
        .map(|(w, g)| (w * g).abs())
        .collect()
}

/// Computes channel importance for structured pruning
///
/// Uses L2 norm of channel weights as importance metric
#[must_use]
pub fn compute_channel_importance(channel_weights: &[Vec<f32>]) -> Vec<f32> {
    channel_weights
        .iter()
        .map(|channel| {
            // L2 norm of channel
            channel.iter().map(|w| w * w).sum::<f32>().sqrt()
        })
        .collect()
}

/// Applies iterative pruning with gradual sparsity increase
///
/// # Errors
/// Returns an error if pruning fails
pub fn iterative_pruning<P: AsRef<Path>>(
    input_path: P,
    output_path: P,
    config: &PruningConfig,
) -> Result<Vec<PruningStats>> {
    let iterations = match config.schedule {
        PruningSchedule::Iterative { iterations } => iterations,
        PruningSchedule::Polynomial { steps, .. } => steps,
        PruningSchedule::OneShot => 1,
    };

    let mut stats_history = Vec::with_capacity(iterations);
    let temp_dir = std::env::temp_dir();

    for i in 0..iterations {
        let current_sparsity = match config.schedule {
            PruningSchedule::Polynomial {
                initial_sparsity,
                final_sparsity,
                steps,
            } => {
                // Polynomial decay: s_t = s_f + (s_i - s_f) * (1 - t/T)^3
                let t = i as f32;
                let total = steps as f32;
                let s_i = initial_sparsity as f32 / 100.0;
                let s_f = final_sparsity as f32 / 100.0;
                s_f + (s_i - s_f) * (1.0 - t / total).powi(3)
            }
            PruningSchedule::Iterative { iterations: n } => {
                // Linear increase
                config.sparsity_target * ((i + 1) as f32 / n as f32)
            }
            PruningSchedule::OneShot => config.sparsity_target,
        };

        info!(
            "Iteration {}/{}: target sparsity {:.1}%",
            i + 1,
            iterations,
            current_sparsity * 100.0
        );

        let iter_config = PruningConfig {
            sparsity_target: current_sparsity,
            ..config.clone()
        };

        let input_file = if i == 0 {
            input_path.as_ref().to_path_buf()
        } else {
            temp_dir.join(format!("pruned_iter_{}.onnx", i - 1))
        };

        let output_file = if i == iterations - 1 {
            output_path.as_ref().to_path_buf()
        } else {
            temp_dir.join(format!("pruned_iter_{}.onnx", i))
        };

        let stats = prune_model(&input_file, &output_file, &iter_config)?;
        stats_history.push(stats);

        // Clean up intermediate files
        if i > 0 {
            let _ = std::fs::remove_file(&input_file);
        }
    }

    Ok(stats_history)
}

/// Applies pruning with Taylor expansion-based importance
#[must_use]
pub fn compute_taylor_importance(
    weights: &[f32],
    gradients: &[f32],
    activations: &[f32],
) -> Vec<f32> {
    weights
        .iter()
        .zip(gradients.iter())
        .zip(activations.iter())
        .map(|((w, g), a)| {
            // Taylor expansion: |w * g * a|
            (w * g * a).abs()
        })
        .collect()
}

/// Selects weights to prune based on importance scores
#[must_use]
pub fn select_weights_to_prune(importance: &[f32], sparsity: f32) -> Vec<bool> {
    let num_to_prune = (importance.len() as f32 * sparsity) as usize;

    // Create indexed importance scores
    let mut indexed: Vec<_> = importance
        .iter()
        .enumerate()
        .map(|(i, &score)| (i, score))
        .collect();

    // Sort by importance (ascending)
    indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    // Create pruning mask
    let mut mask = vec![false; importance.len()];
    for (idx, _) in indexed.iter().take(num_to_prune) {
        mask[*idx] = true;
    }

    mask
}
