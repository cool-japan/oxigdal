//! Temporal LSTM models for time series forecasting.
//!
//! This module provides LSTM-based models for geospatial time series analysis,
//! including NDVI forecasting, crop yield prediction, and temporal pattern learning.
//!
//! # Features
//!
//! - TemporalLSTM for sequence processing
//! - Bidirectional LSTM support
//! - Multi-layer stacking
//! - Integration with scirs2-autograd for training
//!
//! # Examples
//!
//! ```rust,no_run
//! use oxigdal_ml_foundation::models::lstm::{TemporalLSTM, LSTMConfig};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create an LSTM for NDVI forecasting
//! let config = LSTMConfig {
//!     input_dim: 10,      // 10 input features (e.g., bands)
//!     hidden_dim: 128,    // 128 hidden units
//!     num_layers: 2,      // 2 LSTM layers
//!     dropout: 0.2,       // 20% dropout
//!     bidirectional: false,
//! };
//!
//! let lstm = TemporalLSTM::new(config)?;
//! # Ok(())
//! # }
//! ```

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

#[cfg(feature = "ml")]
use scirs2_core::random::SeedableRng;
#[cfg(feature = "ml")]
use scirs2_neural::layers::{LSTM, Layer};

/// Configuration for LSTM models.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LSTMConfig {
    /// Number of input features per timestep
    pub input_dim: usize,
    /// Number of hidden units in LSTM
    pub hidden_dim: usize,
    /// Number of stacked LSTM layers
    pub num_layers: usize,
    /// Dropout rate (0.0 to 1.0)
    pub dropout: f32,
    /// Whether to use bidirectional LSTM
    pub bidirectional: bool,
}

impl Default for LSTMConfig {
    fn default() -> Self {
        Self {
            input_dim: 1,
            hidden_dim: 64,
            num_layers: 1,
            dropout: 0.0,
            bidirectional: false,
        }
    }
}

impl LSTMConfig {
    /// Creates a new LSTM configuration.
    ///
    /// # Arguments
    ///
    /// * `input_dim` - Number of input features per timestep
    /// * `hidden_dim` - Number of hidden units
    /// * `num_layers` - Number of stacked LSTM layers
    /// * `dropout` - Dropout rate (0.0 to 1.0)
    ///
    /// # Returns
    ///
    /// A new `LSTMConfig` instance.
    pub fn new(input_dim: usize, hidden_dim: usize, num_layers: usize, dropout: f32) -> Self {
        Self {
            input_dim,
            hidden_dim,
            num_layers,
            dropout,
            bidirectional: false,
        }
    }

    /// Sets whether to use bidirectional LSTM.
    pub fn with_bidirectional(mut self, bidirectional: bool) -> Self {
        self.bidirectional = bidirectional;
        self
    }

    /// Validates the configuration.
    pub fn validate(&self) -> Result<()> {
        if self.input_dim == 0 {
            return Err(Error::invalid_parameter(
                "input_dim",
                self.input_dim,
                "must be greater than 0",
            ));
        }
        if self.hidden_dim == 0 {
            return Err(Error::invalid_parameter(
                "hidden_dim",
                self.hidden_dim,
                "must be greater than 0",
            ));
        }
        if self.num_layers == 0 {
            return Err(Error::invalid_parameter(
                "num_layers",
                self.num_layers,
                "must be greater than 0",
            ));
        }
        if !(0.0..=1.0).contains(&self.dropout) {
            return Err(Error::invalid_parameter(
                "dropout",
                self.dropout,
                "must be between 0.0 and 1.0",
            ));
        }
        Ok(())
    }

    /// Returns the output dimension.
    ///
    /// For bidirectional LSTM, output dimension is 2 * hidden_dim.
    pub fn output_dim(&self) -> usize {
        if self.bidirectional {
            2 * self.hidden_dim
        } else {
            self.hidden_dim
        }
    }
}

/// Temporal LSTM model for time series forecasting.
///
/// This model processes sequential geospatial data (e.g., NDVI time series)
/// and can be used for forecasting, anomaly detection, and pattern recognition.
#[cfg(feature = "ml")]
pub struct TemporalLSTM {
    config: LSTMConfig,
    lstm_layers: Vec<LSTM<f32>>,
}

#[cfg(feature = "ml")]
impl TemporalLSTM {
    /// Creates a new temporal LSTM model.
    ///
    /// # Arguments
    ///
    /// * `config` - LSTM configuration
    ///
    /// # Returns
    ///
    /// A new `TemporalLSTM` instance or an error if creation fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use oxigdal_ml_foundation::models::lstm::{TemporalLSTM, LSTMConfig};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = LSTMConfig::new(10, 128, 2, 0.2);
    /// let lstm = TemporalLSTM::new(config)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(config: LSTMConfig) -> Result<Self> {
        config.validate()?;

        let mut rng = scirs2_core::random::rngs::StdRng::seed_from_u64(42);
        let mut lstm_layers = Vec::with_capacity(config.num_layers);

        for i in 0..config.num_layers {
            let input_size = if i == 0 {
                config.input_dim
            } else {
                config.output_dim()
            };

            let lstm = LSTM::new(input_size, config.hidden_dim, &mut rng).map_err(|e| {
                Error::ModelArchitecture(format!("Failed to create LSTM layer {}: {}", i, e))
            })?;

            lstm_layers.push(lstm);
        }

        Ok(Self {
            config,
            lstm_layers,
        })
    }

    /// Returns the model configuration.
    pub fn config(&self) -> &LSTMConfig {
        &self.config
    }

    /// Performs forward pass through the LSTM.
    ///
    /// # Arguments
    ///
    /// * `input` - Input tensor of shape [batch_size, seq_len, input_dim]
    ///
    /// # Returns
    ///
    /// Output tensor of shape [batch_size, seq_len, output_dim] or an error.
    pub fn forward(
        &self,
        input: &scirs2_core::ndarray::Array<f32, scirs2_core::ndarray::IxDyn>,
    ) -> Result<scirs2_core::ndarray::Array<f32, scirs2_core::ndarray::IxDyn>> {
        // Validate input shape
        let input_shape = input.shape();
        if input_shape.len() != 3 {
            return Err(Error::invalid_dimensions(
                "[batch_size, seq_len, features]",
                format!("{:?}", input_shape),
            ));
        }

        if input_shape[2] != self.config.input_dim {
            return Err(Error::invalid_parameter(
                "input_features",
                input_shape[2],
                format!("expected {}", self.config.input_dim),
            ));
        }

        // Process through LSTM layers
        let mut output = input.clone();
        for (i, lstm) in self.lstm_layers.iter().enumerate() {
            output = lstm.forward(&output).map_err(|e| {
                Error::ModelArchitecture(format!("LSTM layer {} forward pass failed: {}", i, e))
            })?;

            // Apply dropout if configured (except last layer during inference)
            // Note: Dropout should be handled by training loop
        }

        Ok(output)
    }

    /// Returns the number of parameters in the model.
    pub fn num_parameters(&self) -> usize {
        // Each LSTM layer has 4 gates, each with:
        // - input-to-hidden weights: hidden_dim * input_dim
        // - hidden-to-hidden weights: hidden_dim * hidden_dim
        // - bias: hidden_dim
        let mut total = 0;
        for i in 0..self.config.num_layers {
            let input_size = if i == 0 {
                self.config.input_dim
            } else {
                self.config.output_dim()
            };

            // 4 gates * (W_ih + W_hh + b_ih + b_hh)
            let layer_params = 4
                * (self.config.hidden_dim * input_size
                    + self.config.hidden_dim * self.config.hidden_dim
                    + self.config.hidden_dim
                    + self.config.hidden_dim);
            total += layer_params;
        }
        total
    }
}

/// Stub for when ml feature is not enabled
#[cfg(not(feature = "ml"))]
pub struct TemporalLSTM {
    config: LSTMConfig,
}

#[cfg(not(feature = "ml"))]
impl TemporalLSTM {
    /// Creates a new temporal LSTM model (stub).
    pub fn new(config: LSTMConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self { config })
    }

    /// Returns the model configuration.
    pub fn config(&self) -> &LSTMConfig {
        &self.config
    }

    /// Returns the number of parameters (stub).
    pub fn num_parameters(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lstm_config_default() {
        let config = LSTMConfig::default();
        assert_eq!(config.input_dim, 1);
        assert_eq!(config.hidden_dim, 64);
        assert_eq!(config.num_layers, 1);
        assert_eq!(config.dropout, 0.0);
        assert!(!config.bidirectional);
    }

    #[test]
    fn test_lstm_config_new() {
        let config = LSTMConfig::new(10, 128, 2, 0.2);
        assert_eq!(config.input_dim, 10);
        assert_eq!(config.hidden_dim, 128);
        assert_eq!(config.num_layers, 2);
        assert_eq!(config.dropout, 0.2);
        assert!(!config.bidirectional);
    }

    #[test]
    fn test_lstm_config_bidirectional() {
        let config = LSTMConfig::new(10, 128, 2, 0.2).with_bidirectional(true);
        assert!(config.bidirectional);
        assert_eq!(config.output_dim(), 256); // 2 * 128
    }

    #[test]
    fn test_lstm_config_validation() {
        let valid_config = LSTMConfig::new(10, 128, 2, 0.2);
        assert!(valid_config.validate().is_ok());

        let invalid_config = LSTMConfig::new(0, 128, 2, 0.2);
        assert!(invalid_config.validate().is_err());

        let invalid_dropout = LSTMConfig::new(10, 128, 2, 1.5);
        assert!(invalid_dropout.validate().is_err());
    }

    #[cfg(feature = "ml")]
    #[test]
    fn test_temporal_lstm_creation() {
        let config = LSTMConfig::new(10, 64, 2, 0.1);
        let lstm = TemporalLSTM::new(config);
        assert!(lstm.is_ok());
    }

    #[cfg(feature = "ml")]
    #[test]
    fn test_temporal_lstm_forward() {
        use scirs2_core::ndarray::Array3;

        let config = LSTMConfig::new(10, 64, 1, 0.0);
        let lstm = TemporalLSTM::new(config).expect("Failed to create LSTM");

        // Create input: [batch=2, seq_len=5, features=10]
        let input = Array3::<f32>::zeros((2, 5, 10)).into_dyn();

        let output = lstm.forward(&input);
        assert!(output.is_ok());

        let output = output.expect("Forward pass failed");
        assert_eq!(output.shape(), &[2, 5, 64]);
    }

    #[cfg(feature = "ml")]
    #[test]
    fn test_temporal_lstm_num_parameters() {
        let config = LSTMConfig::new(10, 64, 2, 0.1);
        let lstm = TemporalLSTM::new(config).expect("Failed to create LSTM");
        let num_params = lstm.num_parameters();
        assert!(num_params > 0);
    }
}
