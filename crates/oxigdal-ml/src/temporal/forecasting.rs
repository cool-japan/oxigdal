//! Time series forecasting using LSTM models.
//!
//! This module provides temporal forecasting capabilities for geospatial
//! time series data, with integration to oxigdal-temporal for TimeSeriesRaster.
//!
//! # Use Cases
//!
//! - **NDVI Forecasting**: Predict future vegetation indices
//! - **Crop Yield Prediction**: Forecast agricultural production
//! - **Drought Monitoring**: Predict drought conditions
//! - **Phenology Prediction**: Forecast crop growth stages
//!
//! # Examples
//!
//! ```rust,no_run
//! use oxigdal_ml::temporal::forecasting::{TemporalForecaster, ForecastConfig};
//! use scirs2_core::ndarray::Array3;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let config = ForecastConfig {
//!     input_features: 1,
//!     hidden_dim: 64,
//!     num_layers: 2,
//!     forecast_horizon: 6,
//!     ..Default::default()
//! };
//!
//! let forecaster = TemporalForecaster::new(config)?;
//!
//! // Input: [batch_size, seq_len, features]
//! let input = Array3::<f32>::zeros((1, 12, 1));
//! let prediction = forecaster.predict(&input)?;
//! # Ok(())
//! # }
//! ```

use crate::error::{MlError, ModelError, Result};
use scirs2_core::ndarray::{Array3, ArrayView3, s};
use serde::{Deserialize, Serialize};

// TEMPORARILY DISABLED: oxigdal-ml-foundation dependency resolution issue
// #[cfg(feature = "temporal")]
// use oxigdal_ml_foundation::models::lstm::{LSTMConfig, TemporalLSTM};

/// Configuration for temporal forecasting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastConfig {
    /// Number of input features per timestep (e.g., 1 for NDVI only, >1 for multi-band)
    pub input_features: usize,
    /// Number of hidden units in LSTM
    pub hidden_dim: usize,
    /// Number of LSTM layers
    pub num_layers: usize,
    /// Number of future timesteps to forecast
    pub forecast_horizon: usize,
    /// Dropout rate for regularization
    pub dropout: f32,
    /// Use bidirectional LSTM
    pub bidirectional: bool,
    /// Sequence length for input
    pub sequence_length: usize,
}

impl Default for ForecastConfig {
    fn default() -> Self {
        Self {
            input_features: 1,
            hidden_dim: 64,
            num_layers: 2,
            forecast_horizon: 1,
            dropout: 0.2,
            bidirectional: false,
            sequence_length: 12,
        }
    }
}

impl ForecastConfig {
    /// Creates a new forecast configuration.
    ///
    /// # Arguments
    ///
    /// * `input_features` - Number of input features per timestep
    /// * `hidden_dim` - Number of hidden units
    /// * `num_layers` - Number of LSTM layers
    /// * `forecast_horizon` - Number of future timesteps to predict
    pub fn new(
        input_features: usize,
        hidden_dim: usize,
        num_layers: usize,
        forecast_horizon: usize,
    ) -> Self {
        Self {
            input_features,
            hidden_dim,
            num_layers,
            forecast_horizon,
            ..Default::default()
        }
    }

    /// Sets the dropout rate.
    pub fn with_dropout(mut self, dropout: f32) -> Self {
        self.dropout = dropout;
        self
    }

    /// Sets whether to use bidirectional LSTM.
    pub fn with_bidirectional(mut self, bidirectional: bool) -> Self {
        self.bidirectional = bidirectional;
        self
    }

    /// Sets the input sequence length.
    pub fn with_sequence_length(mut self, length: usize) -> Self {
        self.sequence_length = length;
        self
    }

    /// Validates the configuration.
    pub fn validate(&self) -> Result<()> {
        if self.input_features == 0 {
            return Err(MlError::InvalidConfig(
                "input_features must be greater than 0".to_string(),
            ));
        }
        if self.hidden_dim == 0 {
            return Err(MlError::InvalidConfig(
                "hidden_dim must be greater than 0".to_string(),
            ));
        }
        if self.num_layers == 0 {
            return Err(MlError::InvalidConfig(
                "num_layers must be greater than 0".to_string(),
            ));
        }
        if self.forecast_horizon == 0 {
            return Err(MlError::InvalidConfig(
                "forecast_horizon must be greater than 0".to_string(),
            ));
        }
        if self.sequence_length == 0 {
            return Err(MlError::InvalidConfig(
                "sequence_length must be greater than 0".to_string(),
            ));
        }
        if !(0.0..=1.0).contains(&self.dropout) {
            return Err(MlError::InvalidConfig(
                "dropout must be between 0.0 and 1.0".to_string(),
            ));
        }
        Ok(())
    }
}

/// Result of temporal forecasting.
#[derive(Debug, Clone)]
pub struct ForecastResult {
    /// Predicted values with shape [batch_size, forecast_horizon, features]
    pub predictions: Array3<f32>,
    /// Confidence intervals (if available)
    pub confidence_intervals: Option<(Array3<f32>, Array3<f32>)>,
}

impl ForecastResult {
    /// Creates a new forecast result.
    pub fn new(predictions: Array3<f32>) -> Self {
        Self {
            predictions,
            confidence_intervals: None,
        }
    }

    /// Adds confidence intervals to the result.
    pub fn with_confidence_intervals(mut self, lower: Array3<f32>, upper: Array3<f32>) -> Self {
        self.confidence_intervals = Some((lower, upper));
        self
    }

    /// Returns the shape of predictions.
    pub fn shape(&self) -> (usize, usize, usize) {
        let shape = self.predictions.shape();
        (shape[0], shape[1], shape[2])
    }
}

/// Temporal forecaster using LSTM.
///
/// This forecaster uses a sequence-to-sequence LSTM architecture
/// to predict future values in geospatial time series.
#[cfg(feature = "temporal")]
pub struct TemporalForecaster {
    config: ForecastConfig,
    encoder: TemporalLSTM,
}

#[cfg(feature = "temporal")]
impl TemporalForecaster {
    /// Creates a new temporal forecaster.
    ///
    /// # Arguments
    ///
    /// * `config` - Forecast configuration
    ///
    /// # Returns
    ///
    /// A new `TemporalForecaster` instance or an error.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use oxigdal_ml::temporal::forecasting::{TemporalForecaster, ForecastConfig};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = ForecastConfig::new(1, 64, 2, 6);
    /// let forecaster = TemporalForecaster::new(config)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(config: ForecastConfig) -> Result<Self> {
        config.validate()?;

        let lstm_config = LSTMConfig::new(
            config.input_features,
            config.hidden_dim,
            config.num_layers,
            config.dropout,
        )
        .with_bidirectional(config.bidirectional);

        let encoder =
            TemporalLSTM::new(lstm_config).map_err(|e| ModelError::InitializationFailed {
                reason: format!("Failed to create LSTM encoder: {}", e),
            })?;

        Ok(Self { config, encoder })
    }

    /// Predicts future values for the given input sequence.
    ///
    /// # Arguments
    ///
    /// * `input` - Input sequence with shape [batch_size, seq_len, features]
    ///
    /// # Returns
    ///
    /// Forecast result containing predictions and optional confidence intervals.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Input shape is invalid
    /// - Feature dimension mismatch
    /// - LSTM forward pass fails
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use oxigdal_ml::temporal::forecasting::{TemporalForecaster, ForecastConfig};
    /// use scirs2_core::ndarray::Array3;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let forecaster = TemporalForecaster::new(ForecastConfig::default())?;
    /// let input = Array3::<f32>::zeros((1, 12, 1));
    /// let result = forecaster.predict(&input)?;
    /// println!("Predicted shape: {:?}", result.shape());
    /// # Ok(())
    /// # }
    /// ```
    pub fn predict(&self, input: &Array3<f32>) -> Result<ForecastResult> {
        let input_shape = input.shape();

        // Validate input shape
        if input_shape[2] != self.config.input_features {
            return Err(MlError::InvalidConfig(format!(
                "Input features mismatch: expected {}, got {}",
                self.config.input_features, input_shape[2]
            )));
        }

        let batch_size = input_shape[0];

        // Encode input sequence
        let encoded = self
            .encoder
            .forward(&input.clone().into_dyn())
            .map_err(|e| ModelError::InitializationFailed {
                reason: format!("LSTM encoding failed: {}", e),
            })?;

        // For now, use the last hidden state for multi-step forecasting
        // In a full implementation, this would use a decoder LSTM
        let encoded_shape = encoded.shape();
        let seq_len = encoded_shape[1];
        let hidden_dim = encoded_shape[2];

        // Extract last hidden state
        let last_hidden = encoded.slice(s![.., seq_len - 1, ..]).to_owned();

        // Repeat last hidden state for forecast horizon
        // This is a simplified approach; a full seq2seq would use decoder
        let mut predictions = Array3::<f32>::zeros((
            batch_size,
            self.config.forecast_horizon,
            self.config.input_features,
        ));

        for t in 0..self.config.forecast_horizon {
            for b in 0..batch_size {
                for f in 0..self.config.input_features {
                    // Simple linear projection from hidden state
                    // In practice, this would be a learned linear layer
                    let hidden_idx = f.min(hidden_dim - 1);
                    predictions[[b, t, f]] = last_hidden[[b, hidden_idx]];
                }
            }
        }

        Ok(ForecastResult::new(predictions))
    }

    /// Performs multi-step forecasting with autoregressive prediction.
    ///
    /// Each predicted timestep is fed back as input for the next prediction.
    ///
    /// # Arguments
    ///
    /// * `input` - Input sequence with shape [batch_size, seq_len, features]
    ///
    /// # Returns
    ///
    /// Forecast result with multi-step predictions.
    pub fn predict_autoregressive(&self, input: &Array3<f32>) -> Result<ForecastResult> {
        let input_shape = input.shape();
        if input_shape[2] != self.config.input_features {
            return Err(MlError::InvalidConfig(format!(
                "Input features mismatch: expected {}, got {}",
                self.config.input_features, input_shape[2]
            )));
        }

        let batch_size = input_shape[0];
        let seq_len = input_shape[1];

        // Start with input sequence
        let mut current_seq = input.clone();
        let mut all_predictions = Array3::<f32>::zeros((
            batch_size,
            self.config.forecast_horizon,
            self.config.input_features,
        ));

        // Autoregressively predict each timestep
        for t in 0..self.config.forecast_horizon {
            // Predict next timestep
            let encoded = self
                .encoder
                .forward(&current_seq.clone().into_dyn())
                .map_err(|e| ModelError::InitializationFailed {
                    reason: format!("LSTM encoding failed at step {}: {}", t, e),
                })?;

            let encoded_shape = encoded.shape();
            let current_seq_len = encoded_shape[1];
            let hidden_dim = encoded_shape[2];

            // Get last hidden state
            let last_hidden = encoded.slice(s![.., current_seq_len - 1, ..]).to_owned();

            // Project to output
            for b in 0..batch_size {
                for f in 0..self.config.input_features {
                    let hidden_idx = f.min(hidden_dim - 1);
                    all_predictions[[b, t, f]] = last_hidden[[b, hidden_idx]];
                }
            }

            // Update sequence by appending prediction and removing oldest
            if t < self.config.forecast_horizon - 1 {
                let mut new_seq =
                    Array3::<f32>::zeros((batch_size, seq_len, self.config.input_features));
                // Copy all but first timestep from current sequence
                for b in 0..batch_size {
                    for s in 1..seq_len {
                        for f in 0..self.config.input_features {
                            new_seq[[b, s - 1, f]] = current_seq[[b, s, f]];
                        }
                    }
                    // Append prediction as last timestep
                    for f in 0..self.config.input_features {
                        new_seq[[b, seq_len - 1, f]] = all_predictions[[b, t, f]];
                    }
                }
                current_seq = new_seq;
            }
        }

        Ok(ForecastResult::new(all_predictions))
    }

    /// Returns the forecaster configuration.
    pub fn config(&self) -> &ForecastConfig {
        &self.config
    }
}

/// Stub for when temporal feature is not enabled
#[cfg(not(feature = "temporal"))]
pub struct TemporalForecaster {
    config: ForecastConfig,
}

#[cfg(not(feature = "temporal"))]
impl TemporalForecaster {
    /// Creates a new temporal forecaster (stub).
    pub fn new(config: ForecastConfig) -> Result<Self> {
        config.validate()?;
        Err(MlError::FeatureNotAvailable {
            feature: "Temporal forecasting".to_string(),
            flag: "temporal".to_string(),
        })
    }

    /// Returns the forecaster configuration (stub).
    pub fn config(&self) -> &ForecastConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_forecast_config_default() {
        let config = ForecastConfig::default();
        assert_eq!(config.input_features, 1);
        assert_eq!(config.hidden_dim, 64);
        assert_eq!(config.num_layers, 2);
        assert_eq!(config.forecast_horizon, 1);
        assert_eq!(config.sequence_length, 12);
    }

    #[test]
    fn test_forecast_config_new() {
        let config = ForecastConfig::new(10, 128, 3, 6);
        assert_eq!(config.input_features, 10);
        assert_eq!(config.hidden_dim, 128);
        assert_eq!(config.num_layers, 3);
        assert_eq!(config.forecast_horizon, 6);
    }

    #[test]
    fn test_forecast_config_validation() {
        let valid_config = ForecastConfig::new(10, 128, 2, 6);
        assert!(valid_config.validate().is_ok());

        let invalid_config = ForecastConfig::new(0, 128, 2, 6);
        assert!(invalid_config.validate().is_err());

        let invalid_horizon = ForecastConfig::new(10, 128, 2, 0);
        assert!(invalid_horizon.validate().is_err());
    }

    #[test]
    fn test_forecast_result() {
        let predictions = Array3::<f32>::zeros((2, 6, 1));
        let result = ForecastResult::new(predictions);
        assert_eq!(result.shape(), (2, 6, 1));
        assert!(result.confidence_intervals.is_none());
    }

    #[cfg(feature = "temporal")]
    #[test]
    fn test_temporal_forecaster_creation() {
        let config = ForecastConfig::new(1, 64, 2, 6);
        let forecaster = TemporalForecaster::new(config);
        assert!(forecaster.is_ok());
    }

    #[cfg(feature = "temporal")]
    #[test]
    fn test_temporal_forecaster_predict() {
        let config = ForecastConfig::new(1, 64, 2, 6);
        let forecaster = TemporalForecaster::new(config).expect("Failed to create forecaster");

        let input = Array3::<f32>::zeros((2, 12, 1));
        let result = forecaster.predict(&input);
        assert!(result.is_ok());

        let result = result.expect("Prediction failed");
        assert_eq!(result.shape(), (2, 6, 1));
    }
}
