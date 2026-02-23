//! Temporal Transformer models for long-sequence time series forecasting.
//!
//! This module provides Transformer-based models for geospatial time series analysis,
//! offering an alternative to LSTM for long sequences with better parallelization
//! and long-range dependency modeling.
//!
//! # Features
//!
//! - TemporalTransformer for sequence processing
//! - Multi-head self-attention
//! - Positional encoding for temporal information
//! - Feed-forward networks with residual connections
//! - Integration with scirs2 for training
//!
//! # Use Cases
//!
//! - Long-term NDVI forecasting (>50 timesteps)
//! - Multi-variate climate prediction
//! - Phenology modeling with attention visualization
//! - Temporal gap filling with context awareness
//!
//! # Examples
//!
//! ```rust,no_run
//! use oxigdal_ml_foundation::models::transformer::{TemporalTransformer, TransformerConfig};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a Transformer for multi-variate forecasting
//! let config = TransformerConfig {
//!     input_dim: 10,          // 10 input features (e.g., bands)
//!     hidden_dim: 256,        // 256 hidden units
//!     num_layers: 4,          // 4 Transformer layers
//!     num_heads: 8,           // 8 attention heads
//!     ff_dim: 1024,           // 1024 feed-forward dim
//!     dropout: 0.1,           // 10% dropout
//!     max_seq_len: 365,       // Max 365 timesteps (daily for 1 year)
//! };
//!
//! let transformer = TemporalTransformer::new(config)?;
//! # Ok(())
//! # }
//! ```

use crate::error::{Error, Result};
use ndarray::{Array1, Array2, Array3, Array4, s};
use scirs2_core::random::{SeedableRng, StdRng};
use serde::{Deserialize, Serialize};

/// Configuration for Transformer models.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformerConfig {
    /// Number of input features per timestep
    pub input_dim: usize,
    /// Hidden dimension (model dimension / d_model)
    pub hidden_dim: usize,
    /// Number of Transformer encoder layers
    pub num_layers: usize,
    /// Number of attention heads
    pub num_heads: usize,
    /// Feed-forward network dimension
    pub ff_dim: usize,
    /// Dropout rate (0.0 to 1.0)
    pub dropout: f32,
    /// Maximum sequence length for positional encoding
    pub max_seq_len: usize,
}

impl Default for TransformerConfig {
    fn default() -> Self {
        Self {
            input_dim: 1,
            hidden_dim: 128,
            num_layers: 2,
            num_heads: 4,
            ff_dim: 512,
            dropout: 0.1,
            max_seq_len: 100,
        }
    }
}

impl TransformerConfig {
    /// Creates a new Transformer configuration.
    ///
    /// # Arguments
    ///
    /// * `input_dim` - Number of input features per timestep
    /// * `hidden_dim` - Hidden dimension (must be divisible by num_heads)
    /// * `num_layers` - Number of Transformer encoder layers
    /// * `num_heads` - Number of attention heads
    /// * `ff_dim` - Feed-forward network dimension
    /// * `dropout` - Dropout rate (0.0 to 1.0)
    /// * `max_seq_len` - Maximum sequence length
    ///
    /// # Returns
    ///
    /// A new `TransformerConfig` instance.
    pub fn new(
        input_dim: usize,
        hidden_dim: usize,
        num_layers: usize,
        num_heads: usize,
        ff_dim: usize,
        dropout: f32,
        max_seq_len: usize,
    ) -> Self {
        Self {
            input_dim,
            hidden_dim,
            num_layers,
            num_heads,
            ff_dim,
            dropout,
            max_seq_len,
        }
    }

    /// Validates the configuration.
    pub fn validate(&self) -> Result<()> {
        if self.input_dim == 0 {
            return Err(Error::invalid_parameter(
                "input_dim",
                self.input_dim,
                "must be > 0",
            ));
        }

        if self.hidden_dim == 0 {
            return Err(Error::invalid_parameter(
                "hidden_dim",
                self.hidden_dim,
                "must be > 0",
            ));
        }

        if self.hidden_dim % self.num_heads != 0 {
            return Err(Error::invalid_parameter(
                "hidden_dim",
                format!("{} (num_heads={})", self.hidden_dim, self.num_heads),
                "hidden_dim must be divisible by num_heads",
            ));
        }

        if self.num_layers == 0 {
            return Err(Error::invalid_parameter(
                "num_layers",
                self.num_layers,
                "must be > 0",
            ));
        }

        if self.num_heads == 0 {
            return Err(Error::invalid_parameter(
                "num_heads",
                self.num_heads,
                "must be > 0",
            ));
        }

        if self.ff_dim == 0 {
            return Err(Error::invalid_parameter(
                "ff_dim",
                self.ff_dim,
                "must be > 0",
            ));
        }

        if !(0.0..=1.0).contains(&self.dropout) {
            return Err(Error::invalid_parameter(
                "dropout",
                self.dropout,
                "must be in [0.0, 1.0]",
            ));
        }

        if self.max_seq_len == 0 {
            return Err(Error::invalid_parameter(
                "max_seq_len",
                self.max_seq_len,
                "must be > 0",
            ));
        }

        Ok(())
    }
}

/// Temporal Transformer model for time series forecasting.
///
/// Implements a Transformer encoder architecture for processing temporal sequences.
/// Provides better long-range dependency modeling than LSTM for sequences > 50 timesteps.
pub struct TemporalTransformer {
    config: TransformerConfig,
    positional_encoding: Vec<Vec<f32>>,
    input_projection: Array2<f32>,
    layers: Vec<TransformerEncoderLayer>,
}

impl TemporalTransformer {
    /// Creates a new Temporal Transformer model.
    ///
    /// # Arguments
    ///
    /// * `config` - Transformer configuration
    ///
    /// # Returns
    ///
    /// A new `TemporalTransformer` instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration is invalid.
    pub fn new(config: TransformerConfig) -> Result<Self> {
        config.validate()?;

        // Pre-compute positional encoding
        let positional_encoding = Self::compute_positional_encoding(&config);

        // Initialize input projection (input_dim -> hidden_dim)
        let mut rng = StdRng::seed_from_u64(42);
        let scale = (2.0 / config.input_dim as f32).sqrt();
        let input_projection = Array2::from_shape_fn((config.input_dim, config.hidden_dim), |_| {
            rng.gen_range(-scale..scale)
        });

        // Create transformer encoder layers
        let mut layers = Vec::with_capacity(config.num_layers);
        for _ in 0..config.num_layers {
            layers.push(TransformerEncoderLayer::new(&config)?);
        }

        tracing::info!(
            "Created TemporalTransformer: layers={}, heads={}, hidden_dim={}",
            config.num_layers,
            config.num_heads,
            config.hidden_dim
        );

        Ok(Self {
            config,
            positional_encoding,
            input_projection,
            layers,
        })
    }

    /// Computes sinusoidal positional encoding.
    ///
    /// # Arguments
    ///
    /// * `config` - Transformer configuration
    ///
    /// # Returns
    ///
    /// Positional encoding matrix (max_seq_len x hidden_dim)
    fn compute_positional_encoding(config: &TransformerConfig) -> Vec<Vec<f32>> {
        let mut pe = vec![vec![0.0; config.hidden_dim]; config.max_seq_len];

        for (pos, pe_row) in pe.iter_mut().enumerate() {
            for i in 0..config.hidden_dim / 2 {
                let div_term = (10000.0_f32).powf((2 * i) as f32 / config.hidden_dim as f32);
                let angle = pos as f32 / div_term;

                pe_row[2 * i] = angle.sin();
                pe_row[2 * i + 1] = angle.cos();
            }
        }

        pe
    }

    /// Forward pass through the Transformer.
    ///
    /// # Arguments
    ///
    /// * `input` - Input sequence (batch_size, seq_len, input_dim)
    ///
    /// # Returns
    ///
    /// Output sequence (batch_size, seq_len, hidden_dim)
    pub fn forward(&self, input: &Array3<f32>) -> Result<Array3<f32>> {
        let (batch_size, seq_len, input_dim) = input.dim();

        if input_dim != self.config.input_dim {
            return Err(Error::invalid_parameter(
                "input_dim",
                format!("{} (expected {})", input_dim, self.config.input_dim),
                "input dimension mismatch",
            ));
        }

        if seq_len > self.config.max_seq_len {
            return Err(Error::invalid_parameter(
                "seq_len",
                format!("{} (max {})", seq_len, self.config.max_seq_len),
                "sequence length exceeds maximum",
            ));
        }

        // Project input to hidden dimension and add positional encoding
        let mut x = Array3::<f32>::zeros((batch_size, seq_len, self.config.hidden_dim));
        for b in 0..batch_size {
            for t in 0..seq_len {
                let input_vec = input.slice(s![b, t, ..]);
                let proj = input_vec.dot(&self.input_projection);

                // Add positional encoding
                for d in 0..self.config.hidden_dim {
                    x[[b, t, d]] = proj[d] + self.positional_encoding[t][d];
                }
            }
        }

        // Pass through transformer encoder layers
        for layer in &self.layers {
            x = layer.forward(&x)?;
        }

        Ok(x)
    }

    /// Returns the model configuration.
    pub fn config(&self) -> &TransformerConfig {
        &self.config
    }

    /// Computes the number of trainable parameters.
    pub fn num_parameters(&self) -> usize {
        let d = self.config.hidden_dim;
        let n = self.config.num_layers;

        // Input projection: input_dim * hidden_dim
        let input_proj = self.config.input_dim * d;

        // Per layer parameters
        let layer_params = if n > 0 {
            self.layers[0].num_parameters() * n
        } else {
            0
        };

        input_proj + layer_params
    }
}

/// Single Transformer encoder layer.
struct TransformerEncoderLayer {
    attention: MultiHeadAttention,
    feed_forward: PositionWiseFeedForward,
    norm1: LayerNorm,
    norm2: LayerNorm,
}

impl TransformerEncoderLayer {
    /// Creates a new transformer encoder layer.
    fn new(config: &TransformerConfig) -> Result<Self> {
        Ok(Self {
            attention: MultiHeadAttention::new(
                config.hidden_dim,
                config.num_heads,
                config.dropout,
            )?,
            feed_forward: PositionWiseFeedForward::new(
                config.hidden_dim,
                config.ff_dim,
                config.dropout,
            )?,
            norm1: LayerNorm::new(config.hidden_dim)?,
            norm2: LayerNorm::new(config.hidden_dim)?,
        })
    }

    /// Forward pass through the encoder layer.
    fn forward(&self, input: &Array3<f32>) -> Result<Array3<f32>> {
        // Multi-head attention with residual connection and layer norm
        let attn_out = self.attention.forward(input, input, input)?;
        let x = self.norm1.forward(&add_arrays(input, &attn_out)?)?;

        // Feed-forward with residual connection and layer norm
        let ff_out = self.feed_forward.forward(&x)?;
        let output = self.norm2.forward(&add_arrays(&x, &ff_out)?)?;

        Ok(output)
    }

    /// Returns the number of trainable parameters.
    fn num_parameters(&self) -> usize {
        self.attention.num_parameters()
            + self.feed_forward.num_parameters()
            + self.norm1.num_parameters()
            + self.norm2.num_parameters()
    }
}

/// Multi-head self-attention mechanism.
///
/// Implements scaled dot-product attention with multiple attention heads.
struct MultiHeadAttention {
    num_heads: usize,
    head_dim: usize,
    hidden_dim: usize,
    #[allow(dead_code)]
    dropout: f32,
    // Linear projection weights: Q, K, V, O
    w_q: Array2<f32>,
    w_k: Array2<f32>,
    w_v: Array2<f32>,
    w_o: Array2<f32>,
}

impl MultiHeadAttention {
    /// Creates a new multi-head attention layer.
    fn new(hidden_dim: usize, num_heads: usize, dropout: f32) -> Result<Self> {
        if hidden_dim % num_heads != 0 {
            return Err(Error::invalid_parameter(
                "hidden_dim",
                hidden_dim,
                "must be divisible by num_heads",
            ));
        }

        let head_dim = hidden_dim / num_heads;

        // Xavier initialization for weight matrices
        let mut rng = StdRng::seed_from_u64(42);
        let scale = (2.0 / (hidden_dim + hidden_dim) as f32).sqrt();

        let w_q = Array2::from_shape_fn((hidden_dim, hidden_dim), |_| rng.gen_range(-scale..scale));
        let w_k = Array2::from_shape_fn((hidden_dim, hidden_dim), |_| rng.gen_range(-scale..scale));
        let w_v = Array2::from_shape_fn((hidden_dim, hidden_dim), |_| rng.gen_range(-scale..scale));
        let w_o = Array2::from_shape_fn((hidden_dim, hidden_dim), |_| rng.gen_range(-scale..scale));

        Ok(Self {
            num_heads,
            head_dim,
            hidden_dim,
            dropout,
            w_q,
            w_k,
            w_v,
            w_o,
        })
    }

    /// Computes scaled dot-product attention.
    ///
    /// # Arguments
    ///
    /// * `query` - Query matrix (batch, seq_len, hidden_dim)
    /// * `key` - Key matrix (batch, seq_len, hidden_dim)
    /// * `value` - Value matrix (batch, seq_len, hidden_dim)
    ///
    /// # Returns
    ///
    /// Attention output (batch, seq_len, hidden_dim)
    fn forward(
        &self,
        query: &Array3<f32>,
        key: &Array3<f32>,
        value: &Array3<f32>,
    ) -> Result<Array3<f32>> {
        let (batch_size, seq_len, _) = query.dim();

        // Linear projections: (batch, seq_len, hidden_dim)
        let q = self.project_batch(query, &self.w_q)?;
        let k = self.project_batch(key, &self.w_k)?;
        let v = self.project_batch(value, &self.w_v)?;

        // Reshape to (batch, num_heads, seq_len, head_dim)
        let q_heads = self.split_heads(&q, batch_size, seq_len)?;
        let k_heads = self.split_heads(&k, batch_size, seq_len)?;
        let v_heads = self.split_heads(&v, batch_size, seq_len)?;

        // Scaled dot-product attention for each head
        let mut attn_outputs = Vec::with_capacity(self.num_heads);
        for h in 0..self.num_heads {
            let q_h = q_heads.slice(s![.., h, .., ..]).to_owned();
            let k_h = k_heads.slice(s![.., h, .., ..]).to_owned();
            let v_h = v_heads.slice(s![.., h, .., ..]).to_owned();

            let attn_h = self.scaled_dot_product_attention(&q_h, &k_h, &v_h)?;
            attn_outputs.push(attn_h);
        }

        // Concatenate heads: (batch, seq_len, hidden_dim)
        let concat = self.concat_heads(&attn_outputs, batch_size, seq_len)?;

        // Output projection
        let output = self.project_batch(&concat, &self.w_o)?;

        Ok(output)
    }

    /// Projects a batch of sequences through a weight matrix.
    fn project_batch(&self, input: &Array3<f32>, weight: &Array2<f32>) -> Result<Array3<f32>> {
        let (batch_size, seq_len, _) = input.dim();
        let output_dim = weight.ncols();

        let mut output = Array3::<f32>::zeros((batch_size, seq_len, output_dim));
        for b in 0..batch_size {
            for t in 0..seq_len {
                let input_vec = input.slice(s![b, t, ..]);
                let proj = input_vec.dot(weight);
                for d in 0..output_dim {
                    output[[b, t, d]] = proj[d];
                }
            }
        }

        Ok(output)
    }

    /// Splits hidden dimension into multiple attention heads.
    fn split_heads(
        &self,
        x: &Array3<f32>,
        batch_size: usize,
        seq_len: usize,
    ) -> Result<Array4<f32>> {
        let mut output = Array4::<f32>::zeros((batch_size, self.num_heads, seq_len, self.head_dim));

        for b in 0..batch_size {
            for t in 0..seq_len {
                for h in 0..self.num_heads {
                    for d in 0..self.head_dim {
                        output[[b, h, t, d]] = x[[b, t, h * self.head_dim + d]];
                    }
                }
            }
        }

        Ok(output)
    }

    /// Scaled dot-product attention for a single head.
    fn scaled_dot_product_attention(
        &self,
        q: &Array3<f32>,
        k: &Array3<f32>,
        v: &Array3<f32>,
    ) -> Result<Array3<f32>> {
        let (batch_size, seq_len, _) = q.dim();
        let scale = (self.head_dim as f32).sqrt();

        let mut output = Array3::<f32>::zeros((batch_size, seq_len, self.head_dim));

        for b in 0..batch_size {
            // Compute attention scores: Q @ K^T / sqrt(d_k)
            let mut scores = Array2::<f32>::zeros((seq_len, seq_len));
            for i in 0..seq_len {
                for j in 0..seq_len {
                    let mut score = 0.0;
                    for d in 0..self.head_dim {
                        score += q[[b, i, d]] * k[[b, j, d]];
                    }
                    scores[[i, j]] = score / scale;
                }
            }

            // Apply softmax row-wise
            let attn_weights = softmax_2d(&scores)?;

            // Apply attention to values: attention_weights @ V
            for i in 0..seq_len {
                for d in 0..self.head_dim {
                    let mut weighted_sum = 0.0;
                    for j in 0..seq_len {
                        weighted_sum += attn_weights[[i, j]] * v[[b, j, d]];
                    }
                    output[[b, i, d]] = weighted_sum;
                }
            }
        }

        Ok(output)
    }

    /// Concatenates multiple attention heads.
    fn concat_heads(
        &self,
        heads: &[Array3<f32>],
        batch_size: usize,
        seq_len: usize,
    ) -> Result<Array3<f32>> {
        let mut output = Array3::<f32>::zeros((batch_size, seq_len, self.hidden_dim));

        for b in 0..batch_size {
            for t in 0..seq_len {
                for (h, head) in heads.iter().enumerate() {
                    for d in 0..self.head_dim {
                        output[[b, t, h * self.head_dim + d]] = head[[b, t, d]];
                    }
                }
            }
        }

        Ok(output)
    }

    /// Returns the number of trainable parameters.
    fn num_parameters(&self) -> usize {
        // 4 weight matrices: Q, K, V, O (each hidden_dim x hidden_dim)
        4 * self.hidden_dim * self.hidden_dim
    }
}

/// Position-wise feed-forward network.
///
/// Two-layer feed-forward network with ReLU activation:
/// FFN(x) = ReLU(xW1 + b1)W2 + b2
struct PositionWiseFeedForward {
    hidden_dim: usize,
    ff_dim: usize,
    #[allow(dead_code)]
    dropout: f32,
    w1: Array2<f32>,
    b1: Array1<f32>,
    w2: Array2<f32>,
    b2: Array1<f32>,
}

impl PositionWiseFeedForward {
    /// Creates a new feed-forward network.
    fn new(hidden_dim: usize, ff_dim: usize, dropout: f32) -> Result<Self> {
        // Xavier initialization
        let mut rng = StdRng::seed_from_u64(42);
        let scale1 = (2.0 / (hidden_dim + ff_dim) as f32).sqrt();
        let scale2 = (2.0 / (ff_dim + hidden_dim) as f32).sqrt();

        let w1 = Array2::from_shape_fn((hidden_dim, ff_dim), |_| rng.gen_range(-scale1..scale1));
        let b1 = Array1::zeros(ff_dim);

        let w2 = Array2::from_shape_fn((ff_dim, hidden_dim), |_| rng.gen_range(-scale2..scale2));
        let b2 = Array1::zeros(hidden_dim);

        Ok(Self {
            hidden_dim,
            ff_dim,
            dropout,
            w1,
            b1,
            w2,
            b2,
        })
    }

    /// Forward pass: FFN(x) = ReLU(xW1 + b1)W2 + b2
    fn forward(&self, input: &Array3<f32>) -> Result<Array3<f32>> {
        let (batch_size, seq_len, _) = input.dim();

        let mut output = Array3::<f32>::zeros((batch_size, seq_len, self.hidden_dim));

        for b in 0..batch_size {
            for t in 0..seq_len {
                let x = input.slice(s![b, t, ..]);

                // First linear layer: x @ W1 + b1
                let hidden = x.dot(&self.w1) + &self.b1;

                // ReLU activation
                let activated = hidden.mapv(|v| v.max(0.0));

                // Second linear layer: activated @ W2 + b2
                let out = activated.dot(&self.w2) + &self.b2;

                for d in 0..self.hidden_dim {
                    output[[b, t, d]] = out[d];
                }
            }
        }

        Ok(output)
    }

    /// Returns the number of trainable parameters.
    fn num_parameters(&self) -> usize {
        // W1: hidden_dim * ff_dim, b1: ff_dim
        // W2: ff_dim * hidden_dim, b2: hidden_dim
        self.hidden_dim * self.ff_dim
            + self.ff_dim
            + self.ff_dim * self.hidden_dim
            + self.hidden_dim
    }
}

/// Layer normalization.
///
/// Normalizes inputs across the feature dimension with learnable scale and shift.
struct LayerNorm {
    normalized_shape: usize,
    gamma: Array1<f32>,
    beta: Array1<f32>,
    eps: f32,
}

impl LayerNorm {
    /// Creates a new layer normalization layer.
    fn new(normalized_shape: usize) -> Result<Self> {
        Ok(Self {
            normalized_shape,
            gamma: Array1::ones(normalized_shape),
            beta: Array1::zeros(normalized_shape),
            eps: 1e-5,
        })
    }

    /// Forward pass: LayerNorm(x) = gamma * (x - mean) / sqrt(var + eps) + beta
    fn forward(&self, input: &Array3<f32>) -> Result<Array3<f32>> {
        let (batch_size, seq_len, feature_dim) = input.dim();

        if feature_dim != self.normalized_shape {
            return Err(Error::invalid_parameter(
                "feature_dim",
                format!("{} (expected {})", feature_dim, self.normalized_shape),
                "feature dimension mismatch",
            ));
        }

        let mut output = Array3::<f32>::zeros((batch_size, seq_len, feature_dim));

        for b in 0..batch_size {
            for t in 0..seq_len {
                let x = input.slice(s![b, t, ..]);

                // Compute mean
                let mean = x.mean().ok_or_else(|| {
                    Error::Backend("Failed to compute mean in layer norm".to_string())
                })?;

                // Compute variance
                let variance = x.mapv(|v| (v - mean).powi(2)).mean().ok_or_else(|| {
                    Error::Backend("Failed to compute variance in layer norm".to_string())
                })?;

                // Normalize: (x - mean) / sqrt(var + eps)
                let std = (variance + self.eps).sqrt();
                let normalized = (x.to_owned() - mean) / std;

                // Scale and shift: gamma * normalized + beta
                let scaled = &normalized * &self.gamma + &self.beta;

                for d in 0..feature_dim {
                    output[[b, t, d]] = scaled[d];
                }
            }
        }

        Ok(output)
    }

    /// Returns the number of trainable parameters.
    fn num_parameters(&self) -> usize {
        // gamma and beta
        2 * self.normalized_shape
    }
}

/// Helper function: Applies softmax to each row of a 2D array.
fn softmax_2d(scores: &Array2<f32>) -> Result<Array2<f32>> {
    let (rows, cols) = scores.dim();
    let mut output = Array2::<f32>::zeros((rows, cols));

    for i in 0..rows {
        let row = scores.slice(s![i, ..]);

        // Find max for numerical stability
        let max_val = row.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));

        // Compute exp(x - max)
        let exp_vals: Array1<f32> = row.mapv(|v| (v - max_val).exp());

        // Compute sum
        let sum_exp: f32 = exp_vals.sum();

        if sum_exp <= 0.0 {
            return Err(Error::Backend(
                "Softmax sum is zero or negative".to_string(),
            ));
        }

        // Normalize
        for j in 0..cols {
            output[[i, j]] = exp_vals[j] / sum_exp;
        }
    }

    Ok(output)
}

/// Helper function: Element-wise addition of two 3D arrays.
fn add_arrays(a: &Array3<f32>, b: &Array3<f32>) -> Result<Array3<f32>> {
    if a.dim() != b.dim() {
        return Err(Error::invalid_parameter(
            "array dimensions",
            format!("{:?} vs {:?}", a.dim(), b.dim()),
            "arrays must have the same shape for addition",
        ));
    }

    Ok(a + b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::arr3;

    #[test]
    fn test_transformer_config_validation() {
        let valid_config = TransformerConfig {
            input_dim: 10,
            hidden_dim: 128,
            num_layers: 2,
            num_heads: 4,
            ff_dim: 512,
            dropout: 0.1,
            max_seq_len: 100,
        };
        assert!(valid_config.validate().is_ok());

        // Hidden dim not divisible by num_heads
        let invalid_config = TransformerConfig {
            input_dim: 10,
            hidden_dim: 127,
            num_layers: 2,
            num_heads: 4,
            ff_dim: 512,
            dropout: 0.1,
            max_seq_len: 100,
        };
        assert!(invalid_config.validate().is_err());
    }

    #[test]
    fn test_transformer_creation() {
        let config = TransformerConfig::default();
        let transformer = TemporalTransformer::new(config.clone());
        assert!(transformer.is_ok());

        let transformer = transformer.expect("Failed to create transformer");
        assert_eq!(transformer.config().hidden_dim, config.hidden_dim);
        assert_eq!(transformer.config().num_layers, config.num_layers);
    }

    #[test]
    fn test_positional_encoding() {
        let config = TransformerConfig {
            input_dim: 10,
            hidden_dim: 128,
            num_layers: 2,
            num_heads: 4,
            ff_dim: 512,
            dropout: 0.1,
            max_seq_len: 50,
        };

        let pe = TemporalTransformer::compute_positional_encoding(&config);

        // Check dimensions
        assert_eq!(pe.len(), 50);
        assert_eq!(pe[0].len(), 128);

        // Check that first position is not all zeros
        let sum: f32 = pe[0].iter().sum();
        assert!(sum.abs() > 0.01);

        // Check that different positions have different encodings
        let diff: f32 = pe[0].iter().zip(&pe[10]).map(|(a, b)| (a - b).abs()).sum();
        assert!(diff > 1.0);
    }

    #[test]
    fn test_num_parameters() {
        let config = TransformerConfig {
            input_dim: 10,
            hidden_dim: 128,
            num_layers: 2,
            num_heads: 4,
            ff_dim: 512,
            dropout: 0.1,
            max_seq_len: 100,
        };

        let transformer = TemporalTransformer::new(config).expect("Failed to create");
        let num_params = transformer.num_parameters();

        // Rough check: should be in reasonable range for this config
        assert!(num_params > 100_000);
        assert!(num_params < 1_000_000);
    }

    #[test]
    fn test_config_builder() {
        let config = TransformerConfig::new(10, 256, 4, 8, 1024, 0.1, 365);
        assert_eq!(config.input_dim, 10);
        assert_eq!(config.hidden_dim, 256);
        assert_eq!(config.num_layers, 4);
        assert_eq!(config.num_heads, 8);
        assert_eq!(config.ff_dim, 1024);
        assert_eq!(config.dropout, 0.1);
        assert_eq!(config.max_seq_len, 365);
    }

    #[test]
    fn test_softmax_2d() {
        let scores = Array2::from_shape_vec((2, 3), vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0])
            .expect("Failed to create array");

        let result = softmax_2d(&scores).expect("Softmax failed");

        // Check that rows sum to 1.0
        for i in 0..2 {
            let row_sum: f32 = result.slice(s![i, ..]).sum();
            assert!((row_sum - 1.0).abs() < 1e-5, "Row {} sum is {}", i, row_sum);
        }

        // Check that all values are positive
        for &val in result.iter() {
            assert!(val > 0.0 && val < 1.0);
        }
    }

    #[test]
    fn test_layer_norm_creation() {
        let layer_norm = LayerNorm::new(128).expect("Failed to create LayerNorm");
        assert_eq!(layer_norm.normalized_shape, 128);
        assert_eq!(layer_norm.num_parameters(), 256); // gamma + beta
    }

    #[test]
    fn test_layer_norm_forward() {
        let layer_norm = LayerNorm::new(4).expect("Failed to create LayerNorm");

        // Create input: (batch=1, seq_len=2, features=4)
        let input = arr3(&[[[1.0, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0]]]);

        let output = layer_norm
            .forward(&input)
            .expect("LayerNorm forward failed");

        // Check output shape
        assert_eq!(output.dim(), (1, 2, 4));

        // Check that each position is normalized (mean ≈ 0, std ≈ 1)
        for b in 0..1 {
            for t in 0..2 {
                let out_slice = output.slice(s![b, t, ..]);
                let mean = out_slice.mean().expect("Failed to compute mean");
                let variance = out_slice
                    .mapv(|v| (v - mean).powi(2))
                    .mean()
                    .expect("Failed to compute variance");
                let std = variance.sqrt();

                assert!(mean.abs() < 0.1, "Mean should be close to 0, got {}", mean);
                assert!(
                    (std - 1.0).abs() < 0.1,
                    "Std should be close to 1, got {}",
                    std
                );
            }
        }
    }

    #[test]
    fn test_feed_forward_creation() {
        let ff = PositionWiseFeedForward::new(128, 512, 0.1).expect("Failed to create FFN");
        assert_eq!(ff.hidden_dim, 128);
        assert_eq!(ff.ff_dim, 512);

        let params = ff.num_parameters();
        // W1: 128*512, b1: 512, W2: 512*128, b2: 128
        assert_eq!(params, 128 * 512 + 512 + 512 * 128 + 128);
    }

    #[test]
    fn test_feed_forward_forward() {
        let ff = PositionWiseFeedForward::new(8, 32, 0.0).expect("Failed to create FFN");

        // Create input: (batch=2, seq_len=3, hidden_dim=8)
        let input = Array3::<f32>::ones((2, 3, 8));

        let output = ff.forward(&input).expect("FFN forward failed");

        // Check output shape
        assert_eq!(output.dim(), (2, 3, 8));

        // Output should not be all zeros
        let sum: f32 = output.iter().sum();
        assert!(sum.abs() > 0.01);
    }

    #[test]
    fn test_multi_head_attention_creation() {
        let attn = MultiHeadAttention::new(128, 4, 0.1).expect("Failed to create attention");
        assert_eq!(attn.num_heads, 4);
        assert_eq!(attn.head_dim, 32);
        assert_eq!(attn.hidden_dim, 128);

        let params = attn.num_parameters();
        // 4 weight matrices: Q, K, V, O (each 128 x 128)
        assert_eq!(params, 4 * 128 * 128);
    }

    #[test]
    fn test_multi_head_attention_forward() {
        let attn = MultiHeadAttention::new(8, 2, 0.0).expect("Failed to create attention");

        // Create input: (batch=1, seq_len=4, hidden_dim=8)
        let input =
            Array3::<f32>::from_shape_fn((1, 4, 8), |(_, t, d)| (t as f32 + d as f32) * 0.1);

        let output = attn
            .forward(&input, &input, &input)
            .expect("Attention forward failed");

        // Check output shape
        assert_eq!(output.dim(), (1, 4, 8));

        // Output should not be all zeros
        let sum: f32 = output.iter().sum();
        assert!(sum.abs() > 0.01);
    }

    #[test]
    fn test_multi_head_attention_different_seq_lengths() {
        let attn = MultiHeadAttention::new(16, 4, 0.0).expect("Failed to create attention");

        // Test with different sequence lengths
        for seq_len in &[1, 5, 10, 20] {
            let input = Array3::<f32>::ones((2, *seq_len, 16));
            let output = attn
                .forward(&input, &input, &input)
                .expect("Attention forward failed");

            assert_eq!(output.dim(), (2, *seq_len, 16));
        }
    }

    #[test]
    fn test_transformer_encoder_layer() {
        let config = TransformerConfig {
            input_dim: 10,
            hidden_dim: 16,
            num_layers: 1,
            num_heads: 2,
            ff_dim: 32,
            dropout: 0.0,
            max_seq_len: 50,
        };

        let layer = TransformerEncoderLayer::new(&config).expect("Failed to create layer");

        // Create input with varying values: (batch=2, seq_len=5, hidden_dim=16)
        let input = Array3::<f32>::from_shape_fn((2, 5, 16), |(b, t, d)| {
            (b as f32 + t as f32 * 0.1 + d as f32 * 0.01) + 1.0
        });

        let output = layer.forward(&input).expect("Layer forward failed");

        // Check output shape
        assert_eq!(output.dim(), (2, 5, 16));

        // Check that output contains valid values (not NaN or Inf)
        for &val in output.iter() {
            assert!(val.is_finite(), "Output contains non-finite values");
        }

        // Output should have some variation
        let mean = output.mean().expect("Failed to compute mean");
        let variance = output
            .mapv(|v| (v - mean).powi(2))
            .mean()
            .expect("Failed to compute variance");
        assert!(variance > 0.0, "Output has zero variance");
    }

    #[test]
    fn test_transformer_full_forward_pass() {
        let config = TransformerConfig {
            input_dim: 10,
            hidden_dim: 32,
            num_layers: 2,
            num_heads: 4,
            ff_dim: 64,
            dropout: 0.0,
            max_seq_len: 50,
        };

        let transformer =
            TemporalTransformer::new(config.clone()).expect("Failed to create transformer");

        // Create input: (batch=2, seq_len=10, input_dim=10)
        let input = Array3::<f32>::from_shape_fn((2, 10, 10), |(b, t, d)| {
            (b as f32 + t as f32 + d as f32) * 0.1
        });

        let output = transformer
            .forward(&input)
            .expect("Transformer forward failed");

        // Check output shape: (batch=2, seq_len=10, hidden_dim=32)
        assert_eq!(output.dim(), (2, 10, 32));

        // Check that output contains valid values (not NaN or Inf)
        for &val in output.iter() {
            assert!(val.is_finite(), "Output contains non-finite values");
        }

        // Output should have some variation
        let mean = output.mean().expect("Failed to compute mean");
        let variance = output
            .mapv(|v| (v - mean).powi(2))
            .mean()
            .expect("Failed to compute variance");
        assert!(variance > 0.0, "Output has zero variance");
    }

    #[test]
    fn test_transformer_different_batch_sizes() {
        let config = TransformerConfig {
            input_dim: 5,
            hidden_dim: 16,
            num_layers: 1,
            num_heads: 2,
            ff_dim: 32,
            dropout: 0.0,
            max_seq_len: 100,
        };

        let transformer = TemporalTransformer::new(config).expect("Failed to create transformer");

        // Test with different batch sizes
        for batch_size in &[1, 2, 4, 8] {
            let input = Array3::<f32>::ones((*batch_size, 10, 5));
            let output = transformer.forward(&input).expect("Forward failed");
            assert_eq!(output.dim(), (*batch_size, 10, 16));
        }
    }

    #[test]
    fn test_transformer_different_seq_lengths() {
        let config = TransformerConfig {
            input_dim: 5,
            hidden_dim: 16,
            num_layers: 1,
            num_heads: 2,
            ff_dim: 32,
            dropout: 0.0,
            max_seq_len: 100,
        };

        let transformer = TemporalTransformer::new(config).expect("Failed to create transformer");

        // Test with different sequence lengths
        for seq_len in &[1, 5, 20, 50, 100] {
            let input = Array3::<f32>::ones((2, *seq_len, 5));
            let output = transformer.forward(&input).expect("Forward failed");
            assert_eq!(output.dim(), (2, *seq_len, 16));
        }
    }

    #[test]
    fn test_transformer_seq_len_exceeds_max() {
        let config = TransformerConfig {
            input_dim: 5,
            hidden_dim: 16,
            num_layers: 1,
            num_heads: 2,
            ff_dim: 32,
            dropout: 0.0,
            max_seq_len: 10,
        };

        let transformer = TemporalTransformer::new(config).expect("Failed to create transformer");

        // Try with sequence length exceeding max
        let input = Array3::<f32>::ones((1, 15, 5));
        let result = transformer.forward(&input);

        assert!(result.is_err());
    }

    #[test]
    fn test_transformer_input_dim_mismatch() {
        let config = TransformerConfig {
            input_dim: 10,
            hidden_dim: 16,
            num_layers: 1,
            num_heads: 2,
            ff_dim: 32,
            dropout: 0.0,
            max_seq_len: 50,
        };

        let transformer = TemporalTransformer::new(config).expect("Failed to create transformer");

        // Try with wrong input dimension
        let input = Array3::<f32>::ones((1, 5, 20)); // Wrong: should be 10
        let result = transformer.forward(&input);

        assert!(result.is_err());
    }

    #[test]
    fn test_transformer_parameter_count() {
        let config = TransformerConfig {
            input_dim: 10,
            hidden_dim: 32,
            num_layers: 2,
            num_heads: 4,
            ff_dim: 64,
            dropout: 0.0,
            max_seq_len: 100,
        };

        let transformer = TemporalTransformer::new(config).expect("Failed to create transformer");
        let num_params = transformer.num_parameters();

        // Should have a reasonable number of parameters
        assert!(num_params > 10000);
        assert!(num_params < 1000000);

        // Input projection: 10 * 32 = 320
        // Per layer:
        //   - Attention: 4 * 32 * 32 = 4096
        //   - FFN: 32*64 + 64 + 64*32 + 32 = 2048 + 64 + 2048 + 32 = 4192
        //   - LayerNorm1: 2 * 32 = 64
        //   - LayerNorm2: 2 * 32 = 64
        //   Total per layer: 4096 + 4192 + 64 + 64 = 8416
        // Total: 320 + 2 * 8416 = 17152
        assert_eq!(num_params, 17152);
    }

    #[test]
    fn test_add_arrays_helper() {
        let a = arr3(&[[[1.0, 2.0], [3.0, 4.0]]]);
        let b = arr3(&[[[5.0, 6.0], [7.0, 8.0]]]);

        let result = add_arrays(&a, &b).expect("add_arrays failed");

        assert_eq!(result[[0, 0, 0]], 6.0);
        assert_eq!(result[[0, 0, 1]], 8.0);
        assert_eq!(result[[0, 1, 0]], 10.0);
        assert_eq!(result[[0, 1, 1]], 12.0);
    }

    #[test]
    fn test_add_arrays_shape_mismatch() {
        let a = arr3(&[[[1.0, 2.0]]]);
        let b = arr3(&[[[1.0, 2.0], [3.0, 4.0]]]);

        let result = add_arrays(&a, &b);
        assert!(result.is_err());
    }
}
