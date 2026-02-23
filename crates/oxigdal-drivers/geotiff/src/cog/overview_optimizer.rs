//! Overview optimization for Cloud Optimized GeoTIFF
//!
//! This module determines optimal overview levels, resampling methods,
//! and compression settings for efficient multi-resolution access.

use oxigdal_core::error::{OxiGdalError, Result};
use oxigdal_core::types::RasterDataType;

use crate::tiff::{Compression, PhotometricInterpretation};
use crate::writer::OverviewResampling;

/// Overview optimization strategy
#[derive(Debug, Clone)]
pub struct OverviewStrategy {
    /// Overview levels (e.g., [2, 4, 8, 16] for power-of-2 downsampling)
    pub levels: Vec<u32>,
    /// Resampling method per level
    pub resampling_methods: Vec<OverviewResampling>,
    /// Compression per level
    pub compressions: Vec<Compression>,
    /// Estimated total file size increase (bytes)
    pub estimated_size_increase: u64,
    /// Reasoning for the strategy
    pub reasoning: String,
}

/// Overview optimization preferences
#[derive(Debug, Clone)]
pub struct OverviewPreferences {
    /// Maximum number of overview levels
    pub max_levels: Option<usize>,
    /// Minimum overview size (pixels)
    pub min_overview_size: u32,
    /// Target downsampling factor (typically 2)
    pub downsampling_factor: u32,
    /// Prefer quality over file size
    pub prefer_quality: bool,
    /// Prefer file size over quality
    pub prefer_size: bool,
    /// Maximum acceptable file size increase (percentage)
    pub max_size_increase_percent: Option<f64>,
}

impl Default for OverviewPreferences {
    fn default() -> Self {
        Self {
            max_levels: None,
            min_overview_size: 256,
            downsampling_factor: 2,
            prefer_quality: false,
            prefer_size: false,
            max_size_increase_percent: None,
        }
    }
}

/// Determines optimal overview configuration
pub fn optimize_overviews(
    width: u64,
    height: u64,
    data_type: RasterDataType,
    photometric: PhotometricInterpretation,
    base_compression: Compression,
    preferences: &OverviewPreferences,
) -> Result<OverviewStrategy> {
    // Calculate optimal levels
    let levels = calculate_optimal_levels(
        width,
        height,
        preferences.downsampling_factor,
        preferences.min_overview_size,
        preferences.max_levels,
    );

    if levels.is_empty() {
        return Err(OxiGdalError::InvalidParameter {
            parameter: "dimensions",
            message: format!("Image {}x{} too small to generate overviews", width, height),
        });
    }

    // Select resampling methods
    let resampling_methods =
        select_resampling_methods(&levels, data_type, &photometric, preferences.prefer_quality);

    // Select compression for each level
    let compressions = select_overview_compressions(
        &levels,
        base_compression,
        preferences.prefer_size,
        preferences.prefer_quality,
    );

    // Estimate size increase
    let estimated_size_increase =
        estimate_overview_size(width, height, &levels, data_type, &compressions);

    // Generate reasoning
    let reasoning = generate_reasoning(
        &levels,
        &resampling_methods,
        &compressions,
        estimated_size_increase,
        width * height * data_type.size_bytes() as u64,
    );

    Ok(OverviewStrategy {
        levels,
        resampling_methods,
        compressions,
        estimated_size_increase,
        reasoning,
    })
}

/// Calculates optimal overview levels
fn calculate_optimal_levels(
    width: u64,
    height: u64,
    downsampling_factor: u32,
    min_size: u32,
    max_levels: Option<usize>,
) -> Vec<u32> {
    let mut levels = Vec::new();
    let mut current_factor = downsampling_factor;

    loop {
        let ov_width = width / current_factor as u64;
        let ov_height = height / current_factor as u64;

        // Stop if overview would be too small
        if ov_width < min_size as u64 || ov_height < min_size as u64 {
            break;
        }

        // Stop if we've reached max levels
        if let Some(max) = max_levels {
            if levels.len() >= max {
                break;
            }
        }

        levels.push(current_factor);
        current_factor *= downsampling_factor;

        // Safety check to prevent infinite loop
        if levels.len() > 20 {
            break;
        }
    }

    levels
}

/// Selects resampling method for each overview level
fn select_resampling_methods(
    levels: &[u32],
    data_type: RasterDataType,
    photometric: &PhotometricInterpretation,
    prefer_quality: bool,
) -> Vec<OverviewResampling> {
    levels
        .iter()
        .map(|&level| select_resampling_method(level, data_type, photometric, prefer_quality))
        .collect()
}

/// Selects resampling method for a single overview level
fn select_resampling_method(
    _level: u32,
    data_type: RasterDataType,
    photometric: &PhotometricInterpretation,
    prefer_quality: bool,
) -> OverviewResampling {
    // Categorical data (palette, class maps)
    if matches!(photometric, PhotometricInterpretation::Palette) {
        return OverviewResampling::Nearest;
    }

    // Integer data types might be categorical
    if matches!(
        data_type,
        RasterDataType::UInt8 | RasterDataType::UInt16 | RasterDataType::Int16
    ) {
        // For low bit-depth integer data, use nearest neighbor
        // to preserve exact values
        if !prefer_quality {
            return OverviewResampling::Nearest;
        }
    }

    // Photographic content
    if matches!(
        photometric,
        PhotometricInterpretation::Rgb | PhotometricInterpretation::YCbCr
    ) {
        if prefer_quality {
            return OverviewResampling::Bilinear;
        }
        return OverviewResampling::Average;
    }

    // Floating point data
    if matches!(data_type, RasterDataType::Float32 | RasterDataType::Float64) {
        if prefer_quality {
            return OverviewResampling::Bilinear;
        }
        return OverviewResampling::Average;
    }

    // Default: bilinear is a good balance
    OverviewResampling::Bilinear
}

/// Selects compression for overview levels
fn select_overview_compressions(
    levels: &[u32],
    base_compression: Compression,
    prefer_size: bool,
    prefer_quality: bool,
) -> Vec<Compression> {
    levels
        .iter()
        .map(|_| select_overview_compression(base_compression, prefer_size, prefer_quality))
        .collect()
}

/// Selects compression for a single overview level
fn select_overview_compression(
    base_compression: Compression,
    prefer_size: bool,
    prefer_quality: bool,
) -> Compression {
    // If base is uncompressed, suggest compression for overviews
    if base_compression == Compression::None {
        if prefer_size {
            return Compression::Zstd;
        }
        return Compression::Deflate;
    }

    // If base is JPEG, we can use higher quality for smaller overviews
    if base_compression == Compression::Jpeg && prefer_quality {
        return Compression::Deflate;
    }

    // Generally, use the same compression as base
    // Overviews compress better due to smoother data
    base_compression
}

/// Estimates total overview size
fn estimate_overview_size(
    width: u64,
    height: u64,
    levels: &[u32],
    data_type: RasterDataType,
    compressions: &[Compression],
) -> u64 {
    let bytes_per_sample = data_type.size_bytes() as u64;
    let mut total_size = 0u64;

    for (i, &level) in levels.iter().enumerate() {
        let ov_width = width / level as u64;
        let ov_height = height / level as u64;
        let uncompressed_size = ov_width * ov_height * bytes_per_sample;

        // Estimate compression ratio
        let compression_ratio =
            estimate_compression_ratio(compressions.get(i).copied().unwrap_or(Compression::None));

        let compressed_size = (uncompressed_size as f64 / compression_ratio) as u64;
        total_size += compressed_size;
    }

    total_size
}

/// Estimates compression ratio for a given compression method
fn estimate_compression_ratio(compression: Compression) -> f64 {
    match compression {
        Compression::None => 1.0,
        Compression::Lzw => 2.0,
        Compression::Deflate | Compression::AdobeDeflate => 2.5,
        Compression::Zstd => 3.0,
        Compression::Jpeg => 10.0,
        _ => 1.5,
    }
}

/// Generates reasoning for the strategy
fn generate_reasoning(
    levels: &[u32],
    resampling_methods: &[OverviewResampling],
    compressions: &[Compression],
    estimated_size: u64,
    base_size: u64,
) -> String {
    let level_str = levels
        .iter()
        .map(|l| format!("1:{}", l))
        .collect::<Vec<_>>()
        .join(", ");

    let size_increase_percent = if base_size > 0 {
        (estimated_size as f64 / base_size as f64) * 100.0
    } else {
        0.0
    };

    let primary_resampling = resampling_methods
        .first()
        .copied()
        .unwrap_or(OverviewResampling::Nearest);

    let primary_compression = compressions.first().copied().unwrap_or(Compression::None);

    format!(
        "Generated {} overview levels ({}). \
         Using {:?} resampling for optimal quality. \
         Compression: {:?}. \
         Estimated size increase: {:.1}% ({} MB).",
        levels.len(),
        level_str,
        primary_resampling,
        primary_compression,
        size_increase_percent,
        estimated_size / 1_000_000
    )
}

/// Progressive overview generation config
#[derive(Debug, Clone)]
pub struct ProgressiveOverviewConfig {
    /// Number of overview levels to generate at once
    pub batch_size: usize,
    /// Whether to validate each overview after generation
    pub validate_after_gen: bool,
    /// Maximum memory usage for overview generation (bytes)
    pub max_memory_usage: Option<u64>,
}

impl Default for ProgressiveOverviewConfig {
    fn default() -> Self {
        Self {
            batch_size: 1,
            validate_after_gen: false,
            max_memory_usage: Some(1024 * 1024 * 1024), // 1 GB
        }
    }
}

/// Optimizes overview generation order for progressive rendering
pub fn optimize_progressive_order(levels: &[u32]) -> Vec<u32> {
    // For progressive rendering, generate overviews from coarsest to finest
    // This allows users to see a low-res version quickly
    let mut ordered = levels.to_vec();
    ordered.sort_by(|a, b| b.cmp(a)); // Descending order
    ordered
}

/// Calculates optimal batch size for overview generation
pub fn calculate_optimal_batch_size(
    width: u64,
    height: u64,
    data_type: RasterDataType,
    available_memory: u64,
) -> usize {
    let bytes_per_sample = data_type.size_bytes() as u64;
    let base_image_size = width * height * bytes_per_sample;

    // Estimate memory needed per overview level
    // We need space for source and destination
    let memory_per_level = base_image_size * 2;

    if memory_per_level == 0 {
        return 1;
    }

    let batch_size = (available_memory / memory_per_level) as usize;
    batch_size.max(1).min(10) // At least 1, at most 10
}

/// Validates overview configuration
pub fn validate_overview_config(
    strategy: &OverviewStrategy,
    max_file_size_increase: Option<f64>,
    base_file_size: u64,
) -> Result<()> {
    // Check if levels are valid
    if strategy.levels.is_empty() {
        return Err(OxiGdalError::InvalidParameter {
            parameter: "levels",
            message: "No overview levels specified".to_string(),
        });
    }

    // Check if levels are in ascending order
    for i in 1..strategy.levels.len() {
        if strategy.levels[i] <= strategy.levels[i - 1] {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "levels",
                message: format!(
                    "Overview levels {:?} must be in ascending order",
                    strategy.levels
                ),
            });
        }
    }

    // Check file size increase
    if let Some(max_increase) = max_file_size_increase {
        let increase_percent = if base_file_size > 0 {
            (strategy.estimated_size_increase as f64 / base_file_size as f64) * 100.0
        } else {
            0.0
        };

        if increase_percent > max_increase {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "size",
                message: format!(
                    "Estimated size increase {:.1}% exceeds maximum {:.1}%",
                    increase_percent, max_increase
                ),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_optimal_levels() {
        let levels = calculate_optimal_levels(8192, 8192, 2, 256, None);
        assert!(!levels.is_empty());
        assert!(levels.contains(&2));
        assert!(levels.contains(&4));
        assert!(levels.contains(&8));
    }

    #[test]
    fn test_small_image_no_overviews() {
        let levels = calculate_optimal_levels(128, 128, 2, 256, None);
        assert!(levels.is_empty());
    }

    #[test]
    fn test_max_levels_limit() {
        let levels = calculate_optimal_levels(16384, 16384, 2, 256, Some(3));
        assert_eq!(levels.len(), 3);
    }

    #[test]
    fn test_resampling_method_selection() {
        // Categorical data
        let method = select_resampling_method(
            2,
            RasterDataType::UInt8,
            &PhotometricInterpretation::Palette,
            false,
        );
        assert_eq!(method, OverviewResampling::Nearest);

        // Photographic data with quality preference
        let method = select_resampling_method(
            2,
            RasterDataType::UInt8,
            &PhotometricInterpretation::Rgb,
            true,
        );
        assert_eq!(method, OverviewResampling::Bilinear);

        // Floating point data
        let method = select_resampling_method(
            2,
            RasterDataType::Float32,
            &PhotometricInterpretation::BlackIsZero,
            false,
        );
        assert_eq!(method, OverviewResampling::Average);
    }

    #[test]
    fn test_overview_optimization() {
        let preferences = OverviewPreferences::default();
        let strategy = optimize_overviews(
            4096,
            4096,
            RasterDataType::UInt8,
            PhotometricInterpretation::BlackIsZero,
            Compression::Deflate,
            &preferences,
        );

        assert!(strategy.is_ok());
        let strategy = strategy.expect("strategy should be valid");
        assert!(!strategy.levels.is_empty());
        assert_eq!(strategy.levels.len(), strategy.resampling_methods.len());
        assert_eq!(strategy.levels.len(), strategy.compressions.len());
    }

    #[test]
    fn test_progressive_order() {
        let levels = vec![2, 4, 8, 16];
        let ordered = optimize_progressive_order(&levels);
        assert_eq!(ordered, vec![16, 8, 4, 2]);
    }

    #[test]
    fn test_batch_size_calculation() {
        let batch_size = calculate_optimal_batch_size(
            1024,
            1024,
            RasterDataType::UInt8,
            100 * 1024 * 1024, // 100 MB
        );
        assert!(batch_size >= 1);
        assert!(batch_size <= 10);
    }

    #[test]
    fn test_validate_overview_config() {
        let strategy = OverviewStrategy {
            levels: vec![2, 4, 8],
            resampling_methods: vec![
                OverviewResampling::Average,
                OverviewResampling::Average,
                OverviewResampling::Average,
            ],
            compressions: vec![
                Compression::Deflate,
                Compression::Deflate,
                Compression::Deflate,
            ],
            estimated_size_increase: 1000000,
            reasoning: "Test".to_string(),
        };

        assert!(validate_overview_config(&strategy, None, 10000000).is_ok());
        assert!(validate_overview_config(&strategy, Some(5.0), 10000000).is_err());
    }

    #[test]
    fn test_invalid_level_order() {
        let strategy = OverviewStrategy {
            levels: vec![4, 2, 8], // Wrong order
            resampling_methods: vec![
                OverviewResampling::Average,
                OverviewResampling::Average,
                OverviewResampling::Average,
            ],
            compressions: vec![
                Compression::Deflate,
                Compression::Deflate,
                Compression::Deflate,
            ],
            estimated_size_increase: 1000000,
            reasoning: "Test".to_string(),
        };

        assert!(validate_overview_config(&strategy, None, 10000000).is_err());
    }
}
