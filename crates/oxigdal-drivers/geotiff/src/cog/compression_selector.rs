//! Smart compression selector for COG optimization
//!
//! This module analyzes raster data characteristics and recommends
//! the best compression algorithm for optimal file size and access speed.

use oxigdal_core::error::{OxiGdalError, Result};
use oxigdal_core::types::RasterDataType;

use crate::tiff::{Compression, PhotometricInterpretation};

/// Analysis results for compression selection
#[derive(Debug, Clone)]
pub struct CompressionAnalysis {
    /// Recommended compression method
    pub recommended_compression: Compression,
    /// Alternative compression methods (sorted by suitability)
    pub alternatives: Vec<Compression>,
    /// Estimated compression ratio (1.0 = no compression)
    pub estimated_ratio: f64,
    /// Reason for recommendation
    pub reason: String,
    /// Data characteristics that influenced the decision
    pub characteristics: DataCharacteristics,
}

/// Characteristics of raster data
#[derive(Debug, Clone)]
pub struct DataCharacteristics {
    /// Data type (int vs float)
    pub data_type: RasterDataType,
    /// Whether data is sparse (many zeros or nodata values)
    pub is_sparse: bool,
    /// Sparsity ratio (0.0 = dense, 1.0 = all zeros/nodata)
    pub sparsity_ratio: f64,
    /// Whether data has smooth gradients
    pub is_smooth: bool,
    /// Smoothness score (0.0 = noisy, 1.0 = very smooth)
    pub smoothness: f64,
    /// Entropy (bits per value, higher = less compressible)
    pub entropy: f64,
    /// Unique value count
    pub unique_values: usize,
    /// Whether data appears to be photographic
    pub is_photographic: bool,
    /// Photometric interpretation
    pub photometric: PhotometricInterpretation,
}

/// Compression preferences for optimization
#[derive(Debug, Clone)]
pub struct CompressionPreferences {
    /// Prefer lossless compression
    pub prefer_lossless: bool,
    /// Maximum acceptable quality loss (0-100, only for lossy)
    pub max_quality_loss: u8,
    /// Prefer faster compression over better ratios
    pub prefer_speed: bool,
    /// Prefer better ratios over faster compression
    pub prefer_ratio: bool,
    /// Require backward compatibility
    pub require_compatibility: bool,
}

impl Default for CompressionPreferences {
    fn default() -> Self {
        Self {
            prefer_lossless: true,
            max_quality_loss: 0,
            prefer_speed: false,
            prefer_ratio: false,
            require_compatibility: false,
        }
    }
}

/// Analyzes data and recommends optimal compression
pub fn analyze_for_compression(
    data: &[u8],
    data_type: RasterDataType,
    width: usize,
    height: usize,
    samples_per_pixel: usize,
    photometric: PhotometricInterpretation,
    preferences: &CompressionPreferences,
) -> Result<CompressionAnalysis> {
    // Analyze data characteristics
    let characteristics = analyze_data_characteristics(
        data,
        data_type,
        width,
        height,
        samples_per_pixel,
        photometric,
    )?;

    // Select compression based on characteristics and preferences
    let (recommended, alternatives, reason) = select_compression(&characteristics, preferences);

    // Estimate compression ratio
    let estimated_ratio = estimate_compression_ratio(&characteristics, recommended);

    Ok(CompressionAnalysis {
        recommended_compression: recommended,
        alternatives,
        estimated_ratio,
        reason,
        characteristics,
    })
}

/// Analyzes data characteristics
fn analyze_data_characteristics(
    data: &[u8],
    data_type: RasterDataType,
    width: usize,
    height: usize,
    samples_per_pixel: usize,
    photometric: PhotometricInterpretation,
) -> Result<DataCharacteristics> {
    let bytes_per_sample = data_type.size_bytes();
    let total_samples = width * height * samples_per_pixel;

    if data.len() < total_samples * bytes_per_sample {
        return Err(OxiGdalError::InvalidParameter {
            parameter: "data",
            message: format!(
                "Data too short: expected {} bytes, got {}",
                total_samples * bytes_per_sample,
                data.len()
            ),
        });
    }

    // Analyze sparsity
    let (is_sparse, sparsity_ratio) = analyze_sparsity(data, data_type);

    // Analyze smoothness
    let (is_smooth, smoothness) =
        analyze_smoothness(data, width, height, samples_per_pixel, bytes_per_sample);

    // Calculate entropy
    let entropy = calculate_entropy(data);

    // Count unique values (sample-based for large datasets)
    let unique_values = count_unique_values(data, bytes_per_sample);

    // Detect photographic content
    let is_photographic =
        detect_photographic_content(&photometric, smoothness, unique_values, total_samples);

    Ok(DataCharacteristics {
        data_type,
        is_sparse,
        sparsity_ratio,
        is_smooth,
        smoothness,
        entropy,
        unique_values,
        is_photographic,
        photometric,
    })
}

/// Analyzes data sparsity
fn analyze_sparsity(data: &[u8], _data_type: RasterDataType) -> (bool, f64) {
    let zero_count = data.iter().filter(|&&b| b == 0).count();
    let sparsity_ratio = zero_count as f64 / data.len() as f64;
    let is_sparse = sparsity_ratio > 0.5;
    (is_sparse, sparsity_ratio)
}

/// Analyzes data smoothness
fn analyze_smoothness(
    data: &[u8],
    width: usize,
    height: usize,
    samples_per_pixel: usize,
    bytes_per_sample: usize,
) -> (bool, f64) {
    if width < 2 || height < 2 {
        return (false, 0.0);
    }

    let row_bytes = width * samples_per_pixel * bytes_per_sample;
    let mut total_diff = 0u64;
    let mut sample_count = 0u64;

    // Sample every 8th row for performance
    for y in (0..height.saturating_sub(1)).step_by(8) {
        let row_start = y * row_bytes;
        let next_row_start = (y + 1) * row_bytes;

        if next_row_start + row_bytes > data.len() {
            break;
        }

        // Sample every 8th pixel in the row
        for x in (0..width.saturating_sub(1)).step_by(8) {
            for s in 0..samples_per_pixel {
                let idx = row_start + (x * samples_per_pixel + s) * bytes_per_sample;
                let next_idx = row_start + ((x + 1) * samples_per_pixel + s) * bytes_per_sample;
                let below_idx = next_row_start + (x * samples_per_pixel + s) * bytes_per_sample;

                if next_idx + bytes_per_sample <= data.len()
                    && below_idx + bytes_per_sample <= data.len()
                {
                    // Horizontal difference
                    let h_diff =
                        calculate_byte_diff(&data[idx..], &data[next_idx..], bytes_per_sample);
                    // Vertical difference
                    let v_diff =
                        calculate_byte_diff(&data[idx..], &data[below_idx..], bytes_per_sample);

                    total_diff += h_diff + v_diff;
                    sample_count += 2;
                }
            }
        }
    }

    if sample_count == 0 {
        return (false, 0.0);
    }

    let avg_diff = total_diff as f64 / sample_count as f64;
    let max_diff = 255.0 * bytes_per_sample as f64;
    let smoothness = 1.0 - (avg_diff / max_diff).min(1.0);
    let is_smooth = smoothness > 0.7;

    (is_smooth, smoothness)
}

/// Calculates difference between two byte sequences
fn calculate_byte_diff(a: &[u8], b: &[u8], bytes: usize) -> u64 {
    let mut diff = 0u64;
    for i in 0..bytes.min(a.len()).min(b.len()) {
        diff += (a[i] as i16 - b[i] as i16).unsigned_abs() as u64;
    }
    diff
}

/// Calculates Shannon entropy of data
fn calculate_entropy(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }

    let mut counts = [0u32; 256];
    for &byte in data {
        counts[byte as usize] += 1;
    }

    let total = data.len() as f64;
    let mut entropy = 0.0;

    for &count in &counts {
        if count > 0 {
            let p = count as f64 / total;
            entropy -= p * p.log2();
        }
    }

    entropy
}

/// Counts unique values in data
fn count_unique_values(data: &[u8], bytes_per_sample: usize) -> usize {
    // For performance, sample the data if it's large
    let sample_size = 10000.min(data.len() / bytes_per_sample);
    let mut seen = std::collections::HashSet::new();

    for i in 0..sample_size {
        let idx = i * bytes_per_sample;
        if idx + bytes_per_sample <= data.len() {
            let value = &data[idx..idx + bytes_per_sample];
            seen.insert(value.to_vec());
        }
    }

    seen.len()
}

/// Detects if content is photographic
fn detect_photographic_content(
    photometric: &PhotometricInterpretation,
    smoothness: f64,
    unique_values: usize,
    total_samples: usize,
) -> bool {
    matches!(
        photometric,
        PhotometricInterpretation::Rgb | PhotometricInterpretation::YCbCr
    ) && smoothness > 0.6
        && unique_values as f64 / total_samples as f64 > 0.1
}

/// Selects compression based on characteristics and preferences
fn select_compression(
    characteristics: &DataCharacteristics,
    preferences: &CompressionPreferences,
) -> (Compression, Vec<Compression>, String) {
    // Floating point data
    if matches!(
        characteristics.data_type,
        RasterDataType::Float32 | RasterDataType::Float64
    ) {
        if characteristics.is_sparse {
            return (
                Compression::Zstd,
                vec![Compression::Deflate, Compression::Lzw],
                "Sparse floating-point data: ZSTD provides best compression".to_string(),
            );
        }
        return (
            Compression::Deflate,
            vec![Compression::Zstd, Compression::Lzw],
            "Floating-point data: DEFLATE balances speed and compression".to_string(),
        );
    }

    // Photographic content
    if characteristics.is_photographic && !preferences.prefer_lossless {
        return (
            Compression::Jpeg,
            vec![Compression::Deflate, Compression::Zstd],
            "Photographic RGB data: JPEG offers excellent lossy compression".to_string(),
        );
    }

    // Very sparse data
    if characteristics.sparsity_ratio > 0.8 {
        return (
            Compression::Zstd,
            vec![Compression::Deflate, Compression::Lzw],
            "Very sparse data: ZSTD excels at highly compressible data".to_string(),
        );
    }

    // Smooth data
    if characteristics.is_smooth {
        if preferences.prefer_ratio {
            return (
                Compression::Zstd,
                vec![Compression::Deflate, Compression::Lzw],
                "Smooth data with preference for best ratio: ZSTD recommended".to_string(),
            );
        }
        return (
            Compression::Deflate,
            vec![Compression::Zstd, Compression::Lzw],
            "Smooth data: DEFLATE provides good balance".to_string(),
        );
    }

    // Low entropy (highly compressible)
    if characteristics.entropy < 4.0 {
        return (
            Compression::Zstd,
            vec![Compression::Deflate, Compression::Lzw],
            "Low entropy data: ZSTD provides best compression".to_string(),
        );
    }

    // Compatibility mode
    if preferences.require_compatibility {
        return (
            Compression::Lzw,
            vec![Compression::Deflate],
            "Compatibility required: LZW has broadest support".to_string(),
        );
    }

    // Speed preference
    if preferences.prefer_speed {
        return (
            Compression::Lzw,
            vec![Compression::Deflate, Compression::Zstd],
            "Speed preference: LZW is faster than DEFLATE/ZSTD".to_string(),
        );
    }

    // Default: DEFLATE for general purpose
    (
        Compression::Deflate,
        vec![Compression::Zstd, Compression::Lzw],
        "General purpose data: DEFLATE is widely supported and efficient".to_string(),
    )
}

/// Estimates compression ratio
fn estimate_compression_ratio(
    characteristics: &DataCharacteristics,
    compression: Compression,
) -> f64 {
    // Base ratio estimate from entropy
    // Lower entropy = better compressibility
    // Entropy close to 0 means very uniform data (excellent compression)
    // Entropy close to 8 means random data (poor compression)
    let entropy_ratio = if characteristics.entropy > 0.1 {
        8.0 / characteristics.entropy
    } else {
        // Very low entropy (uniform data) - excellent compression
        // Return high ratio for nearly uniform data
        100.0
    };

    // Adjust based on compression algorithm
    let algo_multiplier = match compression {
        Compression::None => 1.0,
        Compression::Lzw => 0.8,
        Compression::Deflate | Compression::AdobeDeflate => 0.9,
        Compression::Zstd => 1.0,
        Compression::Jpeg => 0.6, // Much better for photographic content
        _ => 0.85,
    };

    // Adjust for sparsity
    let sparsity_bonus = if characteristics.is_sparse { 1.2 } else { 1.0 };

    // Adjust for smoothness
    let smoothness_bonus = if characteristics.is_smooth { 1.1 } else { 1.0 };

    (entropy_ratio * algo_multiplier * sparsity_bonus * smoothness_bonus).max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_sparse_data() {
        let mut data = vec![0u8; 1000];
        data[100] = 255;
        data[500] = 128;

        let (is_sparse, ratio) = analyze_sparsity(&data, RasterDataType::UInt8);
        assert!(is_sparse);
        assert!(ratio > 0.9);
    }

    #[test]
    fn test_analyze_dense_data() {
        let data: Vec<u8> = (0..=255).cycle().take(1000).collect();
        let (is_sparse, ratio) = analyze_sparsity(&data, RasterDataType::UInt8);
        assert!(!is_sparse);
        assert!(ratio < 0.1);
    }

    #[test]
    fn test_entropy_uniform() {
        let data = vec![0u8; 1000];
        let entropy = calculate_entropy(&data);
        assert!(entropy < 0.1); // Very low entropy
    }

    #[test]
    fn test_entropy_random() {
        let data: Vec<u8> = (0..=255).cycle().take(1000).collect();
        let entropy = calculate_entropy(&data);
        assert!(entropy > 5.0); // High entropy
    }

    #[test]
    fn test_compression_selection_float() {
        let chars = DataCharacteristics {
            data_type: RasterDataType::Float32,
            is_sparse: false,
            sparsity_ratio: 0.1,
            is_smooth: false,
            smoothness: 0.3,
            entropy: 6.0,
            unique_values: 1000,
            is_photographic: false,
            photometric: PhotometricInterpretation::BlackIsZero,
        };

        let prefs = CompressionPreferences::default();
        let (compression, _, _) = select_compression(&chars, &prefs);
        assert!(matches!(
            compression,
            Compression::Deflate | Compression::Zstd
        ));
    }

    #[test]
    fn test_compression_selection_sparse() {
        let chars = DataCharacteristics {
            data_type: RasterDataType::UInt8,
            is_sparse: true,
            sparsity_ratio: 0.9,
            is_smooth: false,
            smoothness: 0.3,
            entropy: 2.0,
            unique_values: 10,
            is_photographic: false,
            photometric: PhotometricInterpretation::BlackIsZero,
        };

        let prefs = CompressionPreferences::default();
        let (compression, _, _) = select_compression(&chars, &prefs);
        assert_eq!(compression, Compression::Zstd);
    }

    #[test]
    fn test_full_analysis() {
        let width = 100;
        let height = 100;
        let samples_per_pixel = 1;
        let data = vec![128u8; width * height * samples_per_pixel];

        let result = analyze_for_compression(
            &data,
            RasterDataType::UInt8,
            width,
            height,
            samples_per_pixel,
            PhotometricInterpretation::BlackIsZero,
            &CompressionPreferences::default(),
        );

        assert!(result.is_ok());
        let analysis = result.expect("analysis should succeed");
        assert!(analysis.estimated_ratio > 1.0);
    }
}
