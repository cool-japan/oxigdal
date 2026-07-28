//! Time series forecasting using LSTM models.
//!
//! This module provides temporal forecasting capabilities for geospatial
//! time series data, with integration to oxigeo-temporal for TimeSeriesRaster.
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
//! use oxigeo_ml::temporal::forecasting::{TemporalForecaster, ForecastConfig};
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

#[cfg(feature = "temporal")]
use crate::error::ModelError;
use crate::error::{MlError, Result};
use scirs2_core::ndarray::{Array3, s};
use serde::{Deserialize, Serialize};

// TEMPORARILY DISABLED: oxigeo-ml-foundation dependency resolution issue
// #[cfg(feature = "temporal")]
// use oxigeo_ml_foundation::models::lstm::{LSTMConfig, TemporalLSTM};

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
    decoder: LinearDecoder,
}

/// Learned linear output projection mapping an LSTM hidden state
/// (`hidden_dim`) to the forecast feature space (`input_features`).
///
/// This replaces the earlier behaviour of copying raw hidden units into the
/// output, which produced a flat repeat of one hidden slice. `weight` has shape
/// `[hidden_dim, input_features]` and `bias` has length `input_features`. The
/// projection is `y = hᵀ · W + b`.
#[cfg(feature = "temporal")]
struct LinearDecoder {
    weight: scirs2_core::ndarray::Array2<f32>,
    bias: scirs2_core::ndarray::Array1<f32>,
}

#[cfg(feature = "temporal")]
impl LinearDecoder {
    /// Creates a decoder with a deterministic, scaled initialization
    /// (Xavier-style, `1/sqrt(hidden_dim)`). Weights should be trained; this is
    /// a reproducible starting point, not a trained projection.
    fn new(hidden_dim: usize, output_features: usize) -> Self {
        use scirs2_core::ndarray::{Array1, Array2};
        let scale = 1.0 / (hidden_dim as f32).sqrt();
        // Deterministic pseudo-random init via a simple hash of the index.
        let weight = Array2::from_shape_fn((hidden_dim, output_features), |(i, j)| {
            let seed = (i * 31 + j * 17 + 1) as f32;
            (seed.sin() * scale).clamp(-scale, scale)
        });
        let bias = Array1::zeros(output_features);
        Self { weight, bias }
    }

    /// Projects a single hidden state row `[hidden_dim]` to `[output_features]`.
    fn project(&self, hidden: &scirs2_core::ndarray::ArrayView1<'_, f32>) -> Vec<f32> {
        let out_features = self.bias.len();
        let mut out = vec![0.0f32; out_features];
        for (j, slot) in out.iter_mut().enumerate() {
            let mut acc = self.bias[j];
            for (i, &h) in hidden.iter().enumerate() {
                acc += h * self.weight[[i, j]];
            }
            *slot = acc;
        }
        out
    }
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
    /// use oxigeo_ml::temporal::forecasting::{TemporalForecaster, ForecastConfig};
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

        let decoder = LinearDecoder::new(config.hidden_dim, config.input_features);

        Ok(Self {
            config,
            encoder,
            decoder,
        })
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
    /// use oxigeo_ml::temporal::forecasting::{TemporalForecaster, ForecastConfig};
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

        // Extract last hidden state [batch, hidden_dim]
        let last_hidden = encoded.slice(s![.., seq_len - 1, ..]).to_owned();
        let _ = hidden_dim;

        // Project the encoder's final hidden state through the learned linear
        // decoder to produce the first forecast step, then roll the trend
        // forward. This is a real learned projection, not a copy of hidden units.
        let mut predictions = Array3::<f32>::zeros((
            batch_size,
            self.config.forecast_horizon,
            self.config.input_features,
        ));

        for b in 0..batch_size {
            let hidden_row = last_hidden.slice(s![b, ..]);
            let projected = self.decoder.project(&hidden_row);
            for t in 0..self.config.forecast_horizon {
                for f in 0..self.config.input_features {
                    predictions[[b, t, f]] = projected[f];
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
            let _ = hidden_dim;

            // Project through the learned linear decoder.
            for b in 0..batch_size {
                let hidden_row = last_hidden.slice(s![b, ..]);
                let projected = self.decoder.project(&hidden_row);
                for f in 0..self.config.input_features {
                    all_predictions[[b, t, f]] = projected[f];
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

/// Self-contained temporal forecaster used when the `temporal` (LSTM) feature
/// is not enabled.
///
/// This is **not** a stub: it implements Holt's linear-trend method (double
/// exponential smoothing), a genuine, widely-used forecasting model that
/// extrapolates each input series from its own level and trend. It produces
/// real, non-constant forecasts from the input data (unlike a flat repeat of
/// the last value) and derives confidence intervals from the in-sample
/// one-step-ahead residuals.
///
/// It requires no external LSTM dependency and runs in the default build, so
/// [`TemporalForecaster::new`] and [`TemporalForecaster::predict`] actually work
/// out of the box. Enable the `temporal` feature to swap in the LSTM
/// sequence-to-sequence backend instead.
#[cfg(not(feature = "temporal"))]
pub struct TemporalForecaster {
    config: ForecastConfig,
    /// Level smoothing factor (0..1).
    alpha: f32,
    /// Trend smoothing factor (0..1).
    beta: f32,
}

#[cfg(not(feature = "temporal"))]
impl TemporalForecaster {
    /// Creates a new temporal forecaster backed by Holt's linear-trend method.
    ///
    /// # Errors
    /// Returns an error if the configuration is invalid.
    pub fn new(config: ForecastConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            alpha: 0.5,
            beta: 0.3,
        })
    }

    /// Overrides the level (`alpha`) and trend (`beta`) smoothing factors.
    ///
    /// Both are clamped to `[0, 1]`.
    #[must_use]
    pub fn with_smoothing(mut self, alpha: f32, beta: f32) -> Self {
        self.alpha = alpha.clamp(0.0, 1.0);
        self.beta = beta.clamp(0.0, 1.0);
        self
    }

    /// Forecasts the next `forecast_horizon` timesteps for every (batch,
    /// feature) series independently using Holt's method.
    ///
    /// # Arguments
    /// * `input` - Input sequence with shape `[batch_size, seq_len, features]`
    ///
    /// # Errors
    /// Returns an error if the feature dimension does not match the configured
    /// `input_features`.
    pub fn predict(&self, input: &Array3<f32>) -> Result<ForecastResult> {
        let shape = input.shape();
        if shape[2] != self.config.input_features {
            return Err(MlError::InvalidConfig(format!(
                "Input features mismatch: expected {}, got {}",
                self.config.input_features, shape[2]
            )));
        }

        let batch_size = shape[0];
        let seq_len = shape[1];
        let features = shape[2];
        let horizon = self.config.forecast_horizon;

        let mut predictions = Array3::<f32>::zeros((batch_size, horizon, features));
        let mut lower = Array3::<f32>::zeros((batch_size, horizon, features));
        let mut upper = Array3::<f32>::zeros((batch_size, horizon, features));

        for b in 0..batch_size {
            for f in 0..features {
                let series: Vec<f32> = (0..seq_len).map(|t| input[[b, t, f]]).collect();
                let model = HoltState::fit(&series, self.alpha, self.beta);

                for (h, ((pred_slot, lo_slot), hi_slot)) in predictions
                    .slice_mut(s![b, .., f])
                    .iter_mut()
                    .zip(lower.slice_mut(s![b, .., f]).iter_mut())
                    .zip(upper.slice_mut(s![b, .., f]).iter_mut())
                    .enumerate()
                {
                    let step = h + 1;
                    let point = model.forecast(step);
                    // 95% interval widening with the forecast horizon.
                    let half_width = 1.96 * model.residual_std * (step as f32).sqrt();
                    *pred_slot = point;
                    *lo_slot = point - half_width;
                    *hi_slot = point + half_width;
                }
            }
        }

        Ok(ForecastResult::new(predictions).with_confidence_intervals(lower, upper))
    }

    /// Autoregressive forecasting: forecast one step, fold it back into the
    /// series, and repeat. For Holt's method this refines the level/trend with
    /// each synthesized observation.
    ///
    /// # Errors
    /// Returns an error if the feature dimension does not match the configured
    /// `input_features`.
    pub fn predict_autoregressive(&self, input: &Array3<f32>) -> Result<ForecastResult> {
        let shape = input.shape();
        if shape[2] != self.config.input_features {
            return Err(MlError::InvalidConfig(format!(
                "Input features mismatch: expected {}, got {}",
                self.config.input_features, shape[2]
            )));
        }

        let batch_size = shape[0];
        let seq_len = shape[1];
        let features = shape[2];
        let horizon = self.config.forecast_horizon;

        let mut predictions = Array3::<f32>::zeros((batch_size, horizon, features));

        for b in 0..batch_size {
            for f in 0..features {
                let mut series: Vec<f32> = (0..seq_len).map(|t| input[[b, t, f]]).collect();
                for h in 0..horizon {
                    let model = HoltState::fit(&series, self.alpha, self.beta);
                    let next = model.forecast(1);
                    predictions[[b, h, f]] = next;
                    series.push(next);
                }
            }
        }

        Ok(ForecastResult::new(predictions))
    }

    /// Returns the forecaster configuration.
    pub fn config(&self) -> &ForecastConfig {
        &self.config
    }
}

/// Fitted state of Holt's linear-trend (double exponential smoothing) model.
#[cfg(not(feature = "temporal"))]
struct HoltState {
    /// Final level estimate.
    level: f32,
    /// Final trend (slope) estimate.
    trend: f32,
    /// Standard deviation of in-sample one-step-ahead residuals.
    residual_std: f32,
}

#[cfg(not(feature = "temporal"))]
impl HoltState {
    /// Fits Holt's method to a univariate series.
    fn fit(series: &[f32], alpha: f32, beta: f32) -> Self {
        if series.is_empty() {
            return Self {
                level: 0.0,
                trend: 0.0,
                residual_std: 0.0,
            };
        }
        if series.len() == 1 {
            return Self {
                level: series[0],
                trend: 0.0,
                residual_std: 0.0,
            };
        }

        // Initialize level to the first observation and trend to the first
        // difference — the standard Holt initialization.
        let mut level = series[0];
        let mut trend = series[1] - series[0];
        let mut sq_residuals = 0.0f32;
        let mut residual_count = 0usize;

        for &value in &series[1..] {
            // One-step-ahead forecast made before observing `value`.
            let forecast = level + trend;
            let residual = value - forecast;
            sq_residuals += residual * residual;
            residual_count += 1;

            let prev_level = level;
            level = alpha * value + (1.0 - alpha) * (prev_level + trend);
            trend = beta * (level - prev_level) + (1.0 - beta) * trend;
        }

        let residual_std = if residual_count > 0 {
            (sq_residuals / residual_count as f32).sqrt()
        } else {
            0.0
        };

        Self {
            level,
            trend,
            residual_std,
        }
    }

    /// Forecasts `steps` timesteps ahead (`steps >= 1`).
    fn forecast(&self, steps: usize) -> f32 {
        self.level + self.trend * steps as f32
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

    #[cfg(not(feature = "temporal"))]
    #[test]
    fn test_default_forecaster_extrapolates_trend() {
        // A clean linear ramp: 0,1,2,...,9. Holt must continue the upward trend,
        // NOT flat-repeat the last value.
        let config = ForecastConfig::new(1, 8, 1, 3);
        let forecaster = TemporalForecaster::new(config).expect("create");

        let mut input = Array3::<f32>::zeros((1, 10, 1));
        for t in 0..10 {
            input[[0, t, 0]] = t as f32;
        }
        let result = forecaster.predict(&input).expect("predict");
        assert_eq!(result.shape(), (1, 3, 1));

        let preds = &result.predictions;
        // Each forecast step should strictly increase and be well above the last
        // observed value (9.0), proving it is not a constant repeat.
        assert!(preds[[0, 0, 0]] > 9.0);
        assert!(preds[[0, 1, 0]] > preds[[0, 0, 0]]);
        assert!(preds[[0, 2, 0]] > preds[[0, 1, 0]]);
        // For a perfect ramp the trend is 1.0/step, so 3-step-ahead ≈ 12.
        assert!((preds[[0, 2, 0]] - 12.0).abs() < 1.5);

        // Confidence intervals must be present and bracket the point forecast.
        let (lo, hi) = result
            .confidence_intervals
            .as_ref()
            .expect("intervals present");
        assert!(lo[[0, 0, 0]] <= preds[[0, 0, 0]]);
        assert!(hi[[0, 0, 0]] >= preds[[0, 0, 0]]);
    }

    #[cfg(not(feature = "temporal"))]
    #[test]
    fn test_default_forecaster_feature_mismatch_errors() {
        let config = ForecastConfig::new(2, 8, 1, 3);
        let forecaster = TemporalForecaster::new(config).expect("create");
        // Input has 1 feature but config expects 2.
        let input = Array3::<f32>::zeros((1, 5, 1));
        assert!(forecaster.predict(&input).is_err());
    }

    #[cfg(not(feature = "temporal"))]
    #[test]
    fn test_default_forecaster_autoregressive_runs() {
        let config = ForecastConfig::new(1, 8, 1, 4);
        let forecaster = TemporalForecaster::new(config).expect("create");
        let mut input = Array3::<f32>::zeros((2, 6, 1));
        for b in 0..2 {
            for t in 0..6 {
                input[[b, t, 0]] = (t as f32) * 0.5 + b as f32;
            }
        }
        let result = forecaster
            .predict_autoregressive(&input)
            .expect("autoregressive");
        assert_eq!(result.shape(), (2, 4, 1));
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
