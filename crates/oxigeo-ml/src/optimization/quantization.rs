//! Model quantization for reduced precision inference
//!
//! Quantization reduces model size and improves inference speed by converting
//! floating-point weights and activations to lower precision formats.

use crate::error::{MlError, Result};
use std::path::Path;
use tracing::{debug, info};

/// Quantization type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuantizationType {
    /// 8-bit signed integer quantization
    Int8,
    /// 8-bit unsigned integer quantization
    UInt8,
    /// 16-bit floating point quantization
    Float16,
    /// 4-bit quantization (experimental)
    Int4,
}

/// Quantization mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuantizationMode {
    /// Dynamic quantization (runtime calibration)
    Dynamic,
    /// Static quantization (pre-calibrated)
    Static,
    /// Quantization-aware training
    QAT,
}

/// Quantization configuration
#[derive(Debug, Clone)]
pub struct QuantizationConfig {
    /// Quantization type
    pub quantization_type: QuantizationType,
    /// Quantization mode
    pub mode: QuantizationMode,
    /// Per-channel quantization (more accurate)
    pub per_channel: bool,
    /// Symmetric vs asymmetric quantization
    pub symmetric: bool,
    /// Calibration dataset size (for static quantization)
    pub calibration_samples: usize,
}

impl Default for QuantizationConfig {
    fn default() -> Self {
        Self {
            quantization_type: QuantizationType::Int8,
            mode: QuantizationMode::Dynamic,
            per_channel: false,
            symmetric: true,
            calibration_samples: 100,
        }
    }
}

impl QuantizationConfig {
    /// Creates a configuration builder
    #[must_use]
    pub fn builder() -> QuantizationConfigBuilder {
        QuantizationConfigBuilder::default()
    }
}

/// Builder for quantization configuration
#[derive(Debug, Default)]
pub struct QuantizationConfigBuilder {
    quantization_type: Option<QuantizationType>,
    mode: Option<QuantizationMode>,
    per_channel: bool,
    symmetric: bool,
    calibration_samples: Option<usize>,
}

impl QuantizationConfigBuilder {
    /// Sets the quantization type
    #[must_use]
    pub fn quantization_type(mut self, qtype: QuantizationType) -> Self {
        self.quantization_type = Some(qtype);
        self
    }

    /// Sets the quantization mode
    #[must_use]
    pub fn mode(mut self, mode: QuantizationMode) -> Self {
        self.mode = Some(mode);
        self
    }

    /// Enables per-channel quantization
    #[must_use]
    pub fn per_channel(mut self, enable: bool) -> Self {
        self.per_channel = enable;
        self
    }

    /// Sets symmetric quantization
    #[must_use]
    pub fn symmetric(mut self, enable: bool) -> Self {
        self.symmetric = enable;
        self
    }

    /// Sets calibration sample count
    #[must_use]
    pub fn calibration_samples(mut self, count: usize) -> Self {
        self.calibration_samples = Some(count);
        self
    }

    /// Builds the configuration
    #[must_use]
    pub fn build(self) -> QuantizationConfig {
        QuantizationConfig {
            quantization_type: self.quantization_type.unwrap_or(QuantizationType::Int8),
            mode: self.mode.unwrap_or(QuantizationMode::Dynamic),
            per_channel: self.per_channel,
            symmetric: self.symmetric,
            calibration_samples: self.calibration_samples.unwrap_or(100),
        }
    }
}

/// Quantization parameters
#[derive(Debug, Clone)]
pub struct QuantizationParams {
    /// Scale factor
    pub scale: f32,
    /// Zero point
    pub zero_point: i32,
    /// Min value
    pub min: f32,
    /// Max value
    pub max: f32,
    /// Quantization type (for proper clamping)
    pub qtype: QuantizationType,
}

impl QuantizationParams {
    /// Computes quantization parameters from min/max values
    #[must_use]
    pub fn from_min_max(min: f32, max: f32, qtype: QuantizationType, symmetric: bool) -> Self {
        let (qmin, qmax) = match qtype {
            QuantizationType::Int8 => (-128i32, 127i32),
            QuantizationType::UInt8 => (0i32, 255i32),
            QuantizationType::Int4 => (-8i32, 7i32),
            QuantizationType::Float16 => return Self::identity(),
        };

        if symmetric {
            let abs_max = min.abs().max(max.abs());
            let scale = abs_max / qmax as f32;
            Self {
                scale,
                zero_point: 0,
                min,
                max,
                qtype,
            }
        } else {
            let scale = (max - min) / (qmax - qmin) as f32;
            let zero_point = qmin - (min / scale).round() as i32;
            Self {
                scale,
                zero_point,
                min,
                max,
                qtype,
            }
        }
    }

    /// Creates identity parameters (no quantization)
    #[must_use]
    pub fn identity() -> Self {
        Self {
            scale: 1.0,
            zero_point: 0,
            min: 0.0,
            max: 1.0,
            qtype: QuantizationType::Float16,
        }
    }

    /// Quantizes a floating-point value
    #[must_use]
    pub fn quantize(&self, value: f32) -> i32 {
        let (qmin, qmax) = match self.qtype {
            QuantizationType::Int8 => (-128i32, 127i32),
            QuantizationType::UInt8 => (0i32, 255i32),
            QuantizationType::Int4 => (-8i32, 7i32),
            QuantizationType::Float16 => return value as i32,
        };

        let scaled = value / self.scale;
        (scaled.round() as i32 + self.zero_point).clamp(qmin, qmax)
    }

    /// Dequantizes a quantized value
    #[must_use]
    pub fn dequantize(&self, value: i32) -> f32 {
        (value - self.zero_point) as f32 * self.scale
    }
}

/// Number of bytes a single weight occupies at a given quantization type,
/// expressed as an (integer numerator, denominator) so `Int4` (0.5 bytes) is
/// exact.
fn quantized_bytes_per_weight(qtype: QuantizationType) -> (u64, u64) {
    match qtype {
        QuantizationType::Int8 | QuantizationType::UInt8 => (1, 1),
        QuantizationType::Float16 => (2, 1),
        QuantizationType::Int4 => (1, 2),
    }
}

/// Analyzes the real weight tensors of an ONNX model and reports the
/// *measured* compression achievable by quantizing every inline float32
/// initializer to `config.quantization_type`.
///
/// Unlike a fabricated ratio derived only from the requested type, the returned
/// [`QuantizationResult`] reflects the actual number of weight bytes in the
/// model, the exact quantized weight size (including per-tensor scale/zero-point
/// overhead), and the non-weight bytes that would remain unchanged.
///
/// This performs no file output — it measures what a real quantizer would
/// achieve. See [`quantize_model`] for why emitting a valid quantized ONNX file
/// is a separate, larger task.
///
/// # Errors
/// Returns an error if the input file cannot be read or is not a valid ONNX
/// model.
pub fn analyze_model_quantization<P: AsRef<Path>>(
    input_path: P,
    config: &QuantizationConfig,
) -> Result<QuantizationResult> {
    let input = input_path.as_ref();
    if !input.exists() {
        return Err(MlError::InvalidConfig(format!(
            "Input model not found: {}",
            input.display()
        )));
    }

    let file_data = std::fs::read(input)?;
    let original_size = file_data.len() as u64;

    let initializers = crate::optimization::onnx_weights::parse_float_initializers(&file_data)?;
    let total_weights: u64 = initializers.iter().map(|i| i.numel() as u64).sum();
    let weight_bytes_fp32 = total_weights * 4;

    let (num, den) = quantized_bytes_per_weight(config.quantization_type);
    // Round up quantized weight bytes (a 4-bit weight still costs a nibble).
    let quantized_weight_bytes = (total_weights * num).div_ceil(den);

    // Per-tensor scale (f32) + zero-point (i32) metadata for integer types.
    let per_tensor_overhead = match config.quantization_type {
        QuantizationType::Float16 => 0,
        _ => 8,
    };
    let overhead = initializers.len() as u64 * per_tensor_overhead;

    // Non-weight bytes (graph structure, metadata) are unchanged.
    let non_weight_bytes = original_size.saturating_sub(weight_bytes_fp32);
    let quantized_size = non_weight_bytes + quantized_weight_bytes + overhead;

    let compression_ratio = if quantized_size > 0 {
        original_size as f32 / quantized_size as f32
    } else {
        1.0
    };

    info!(
        "Quantization analysis: {} weights, {} -> {} bytes ({:.2}x measured compression)",
        total_weights, weight_bytes_fp32, quantized_weight_bytes, compression_ratio
    );

    Ok(QuantizationResult {
        original_size,
        quantized_size,
        compression_ratio,
        quantization_type: config.quantization_type,
    })
}

/// Quantizes an ONNX model to disk.
///
/// Emitting a *valid, runnable* quantized ONNX model requires inserting
/// `QuantizeLinear`/`DequantizeLinear` (QDQ) nodes around each quantized weight
/// and rewiring the graph — a protobuf-serializing graph transformation that
/// `oxionnx` does not yet support (it can parse a model but not re-serialize
/// one). Rather than copy the file and report a fabricated compression ratio
/// (which would leave the model unquantized while claiming otherwise), this
/// function returns an honest [`MlError::Unsupported`] error.
///
/// To obtain the real, measured compression a quantizer would achieve on this
/// model's weights, use [`analyze_model_quantization`]; to quantize an
/// in-memory tensor, use [`quantize_tensor`] / [`QuantizationParams`].
///
/// # Errors
/// Always returns [`MlError::Unsupported`] (after validating the input is a
/// readable ONNX model), or an I/O / format error if the input cannot be read.
pub fn quantize_model<P: AsRef<Path>>(
    input_path: P,
    output_path: P,
    config: &QuantizationConfig,
) -> Result<QuantizationResult> {
    let input = input_path.as_ref();
    let output = output_path.as_ref();

    info!(
        "Quantizing model {:?} to {:?} (type: {:?}, mode: {:?})",
        input, output, config.quantization_type, config.mode
    );

    if !input.exists() {
        return Err(MlError::InvalidConfig(format!(
            "Input model not found: {}",
            input.display()
        )));
    }

    debug!(
        "Quantization config: per_channel={}, symmetric={}",
        config.per_channel, config.symmetric
    );

    // Validate the input is a real ONNX model and compute the achievable
    // compression so the error message carries honest, measured numbers.
    let analysis = analyze_model_quantization(input, config)?;

    Err(MlError::Unsupported {
        operation: "quantize_model (ONNX file emission)".to_string(),
        reason: format!(
            "writing a valid quantized ONNX model requires QuantizeLinear/DequantizeLinear \
             graph insertion + protobuf serialization, which is not yet available. \
             Measured achievable compression for this model is {:.2}x ({} -> {} bytes); \
             use analyze_model_quantization() for the measurement or quantize_tensor() for \
             in-memory tensor quantization",
            analysis.compression_ratio, analysis.original_size, analysis.quantized_size
        ),
    })
}

/// Result of model quantization
#[derive(Debug, Clone)]
pub struct QuantizationResult {
    /// Original model size in bytes
    pub original_size: u64,
    /// Quantized model size in bytes
    pub quantized_size: u64,
    /// Compression ratio achieved
    pub compression_ratio: f32,
    /// Quantization type used
    pub quantization_type: QuantizationType,
}

impl QuantizationResult {
    /// Returns the size reduction percentage
    #[must_use]
    pub fn size_reduction_percent(&self) -> f32 {
        if self.original_size > 0 {
            (1.0 - (self.quantized_size as f32 / self.original_size as f32)) * 100.0
        } else {
            0.0
        }
    }

    /// Returns the original size in megabytes
    #[must_use]
    pub fn original_size_mb(&self) -> f32 {
        self.original_size as f32 / (1024.0 * 1024.0)
    }

    /// Returns the quantized size in megabytes
    #[must_use]
    pub fn quantized_size_mb(&self) -> f32 {
        self.quantized_size as f32 / (1024.0 * 1024.0)
    }
}

/// Calibrates quantization parameters using a dataset
///
/// # Errors
/// Returns an error if calibration fails
pub fn calibrate_quantization(
    calibration_data: &[Vec<f32>],
    config: &QuantizationConfig,
) -> Result<Vec<QuantizationParams>> {
    info!(
        "Calibrating quantization with {} samples",
        calibration_data.len()
    );

    if calibration_data.is_empty() {
        return Err(MlError::InvalidConfig(
            "Calibration data cannot be empty".to_string(),
        ));
    }

    let mut params_list = Vec::new();

    // Compute min/max for each channel
    for channel_idx in 0..calibration_data[0].len() {
        let mut min = f32::MAX;
        let mut max = f32::MIN;

        for sample in calibration_data {
            if let Some(&value) = sample.get(channel_idx) {
                min = min.min(value);
                max = max.max(value);
            }
        }

        let params =
            QuantizationParams::from_min_max(min, max, config.quantization_type, config.symmetric);
        params_list.push(params);
    }

    debug!("Calibrated {} channels", params_list.len());
    Ok(params_list)
}

/// Quantizes a tensor using the provided parameters
#[must_use]
pub fn quantize_tensor(tensor: &[f32], params: &QuantizationParams) -> Vec<i8> {
    tensor.iter().map(|&v| params.quantize(v) as i8).collect()
}

/// Dequantizes a tensor using the provided parameters
#[must_use]
pub fn dequantize_tensor(tensor: &[i8], params: &QuantizationParams) -> Vec<f32> {
    tensor
        .iter()
        .map(|&v| params.dequantize(i32::from(v)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quantization_config_builder() {
        let config = QuantizationConfig::builder()
            .quantization_type(QuantizationType::Int8)
            .mode(QuantizationMode::Static)
            .per_channel(true)
            .symmetric(false)
            .calibration_samples(200)
            .build();

        assert_eq!(config.quantization_type, QuantizationType::Int8);
        assert_eq!(config.mode, QuantizationMode::Static);
        assert!(config.per_channel);
        assert!(!config.symmetric);
        assert_eq!(config.calibration_samples, 200);
    }

    #[test]
    fn test_quantization_params_symmetric() {
        let params = QuantizationParams::from_min_max(-10.0, 10.0, QuantizationType::Int8, true);

        assert_eq!(params.zero_point, 0);
        assert!((params.scale - 10.0 / 127.0).abs() < 1e-6);

        // Test quantize/dequantize round-trip
        let value = 5.0;
        let quantized = params.quantize(value);
        let dequantized = params.dequantize(quantized);
        assert!((dequantized - value).abs() < 0.1);
    }

    #[test]
    fn test_quantization_params_asymmetric() {
        let params = QuantizationParams::from_min_max(0.0, 255.0, QuantizationType::UInt8, false);

        assert!((params.scale - 1.0).abs() < 1e-6);

        let value = 128.0;
        let quantized = params.quantize(value);
        let dequantized = params.dequantize(quantized);
        assert!((dequantized - value).abs() < 1.0);
    }

    #[test]
    fn test_quantize_tensor() {
        let tensor = vec![0.0, 1.0, 2.0, 3.0, 4.0];
        let params = QuantizationParams::from_min_max(0.0, 4.0, QuantizationType::Int8, true);

        let quantized = quantize_tensor(&tensor, &params);
        assert_eq!(quantized.len(), tensor.len());

        let dequantized = dequantize_tensor(&quantized, &params);
        for (orig, deq) in tensor.iter().zip(dequantized.iter()) {
            assert!((orig - deq).abs() < 0.1);
        }
    }

    #[test]
    fn test_calibrate_quantization() {
        let calibration_data = vec![
            vec![0.0, 1.0, 2.0],
            vec![0.5, 1.5, 2.5],
            vec![1.0, 2.0, 3.0],
        ];

        let config = QuantizationConfig::default();
        let params =
            calibrate_quantization(&calibration_data, &config).expect("Calibration should succeed");

        assert_eq!(params.len(), 3);
        assert!(params[0].min <= 0.0);
        assert!(params[2].max >= 3.0);
    }
}
