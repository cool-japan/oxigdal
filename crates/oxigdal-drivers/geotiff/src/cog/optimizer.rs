//! COG optimizer for analyzing and optimizing Cloud Optimized GeoTIFF files
//!
//! This module provides comprehensive analysis of raster data and recommends
//! optimal COG configuration including tile size, compression, and overviews.

use oxigdal_core::error::Result;
use oxigdal_core::types::RasterDataType;

use crate::tiff::{Compression, PhotometricInterpretation};
use crate::writer::OverviewResampling;

use super::compression_selector::{CompressionPreferences, analyze_for_compression};
use super::overview_optimizer::{OverviewPreferences, optimize_overviews};

/// Complete COG optimization analysis
#[derive(Debug, Clone)]
pub struct CogOptimization {
    /// Recommended tile width
    pub optimal_tile_width: u32,
    /// Recommended tile height
    pub optimal_tile_height: u32,
    /// Recommended compression
    pub recommended_compression: Compression,
    /// Recommended overview levels
    pub recommended_overviews: Vec<u32>,
    /// Recommended resampling methods
    pub recommended_resampling: Vec<OverviewResampling>,
    /// Estimated final file size (bytes)
    pub estimated_file_size: u64,
    /// Estimated compression ratio
    pub estimated_compression_ratio: f64,
    /// Optimization goal used
    pub optimization_goal: OptimizationGoal,
    /// Detailed recommendations
    pub recommendations: Vec<String>,
}

/// Optimization goal for COG creation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptimizationGoal {
    /// Minimize file size
    MinimizeSize,
    /// Minimize access latency
    MinimizeLatency,
    /// Balance between size and speed
    Balanced,
    /// Optimize for cloud storage costs
    CloudCost,
    /// Optimize for web serving
    WebServing,
}

/// Access pattern prediction
#[derive(Debug, Clone)]
pub struct AccessPattern {
    /// Expected zoom levels (percentage of access at each overview level)
    pub zoom_distribution: Vec<f64>,
    /// Expected spatial access pattern
    pub spatial_pattern: SpatialAccessPattern,
    /// Expected sequential vs random access ratio (0-1, 1=all sequential)
    pub sequential_ratio: f64,
}

/// Spatial access patterns
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpatialAccessPattern {
    /// Random access across entire image
    Random,
    /// Focused on specific regions
    Regional,
    /// Sequential scanline reading
    Sequential,
    /// Tiled access (e.g., web maps)
    Tiled,
}

impl Default for AccessPattern {
    fn default() -> Self {
        Self {
            zoom_distribution: vec![0.1, 0.2, 0.3, 0.4], // Favor higher zoom
            spatial_pattern: SpatialAccessPattern::Tiled,
            sequential_ratio: 0.3,
        }
    }
}

/// Analyzes raster data and recommends optimal COG configuration
pub fn analyze_for_cog(
    data: &[u8],
    width: u64,
    height: u64,
    data_type: RasterDataType,
    samples_per_pixel: usize,
    photometric: PhotometricInterpretation,
    goal: OptimizationGoal,
    access_pattern: Option<AccessPattern>,
) -> Result<CogOptimization> {
    let access_pattern = access_pattern.unwrap_or_default();

    // Determine optimal tile size
    let (tile_width, tile_height) =
        determine_optimal_tile_size(width, height, goal, &access_pattern);

    // Analyze compression
    let compression_prefs = CompressionPreferences {
        prefer_lossless: !matches!(goal, OptimizationGoal::MinimizeSize),
        max_quality_loss: if matches!(goal, OptimizationGoal::MinimizeSize) {
            10
        } else {
            0
        },
        prefer_speed: matches!(goal, OptimizationGoal::MinimizeLatency),
        prefer_ratio: matches!(
            goal,
            OptimizationGoal::MinimizeSize | OptimizationGoal::CloudCost
        ),
        require_compatibility: false,
    };

    let compression_analysis = analyze_for_compression(
        data,
        data_type,
        width as usize,
        height as usize,
        samples_per_pixel,
        photometric,
        &compression_prefs,
    )?;

    // Optimize overviews
    let overview_prefs = OverviewPreferences {
        max_levels: None,
        min_overview_size: determine_min_overview_size(goal),
        downsampling_factor: 2,
        prefer_quality: matches!(goal, OptimizationGoal::MinimizeLatency),
        prefer_size: matches!(
            goal,
            OptimizationGoal::MinimizeSize | OptimizationGoal::CloudCost
        ),
        max_size_increase_percent: None,
    };

    let overview_strategy = optimize_overviews(
        width,
        height,
        data_type,
        photometric,
        compression_analysis.recommended_compression,
        &overview_prefs,
    )?;

    // Estimate file size
    let base_size = estimate_base_image_size(
        width,
        height,
        data_type,
        samples_per_pixel,
        compression_analysis.estimated_ratio,
    );
    let estimated_file_size = base_size + overview_strategy.estimated_size_increase;

    // Generate recommendations
    let recommendations =
        generate_recommendations(goal, &compression_analysis.characteristics, width, height);

    Ok(CogOptimization {
        optimal_tile_width: tile_width,
        optimal_tile_height: tile_height,
        recommended_compression: compression_analysis.recommended_compression,
        recommended_overviews: overview_strategy.levels,
        recommended_resampling: overview_strategy.resampling_methods,
        estimated_file_size,
        estimated_compression_ratio: compression_analysis.estimated_ratio,
        optimization_goal: goal,
        recommendations,
    })
}

/// Determines optimal tile size based on goal and access pattern
fn determine_optimal_tile_size(
    width: u64,
    height: u64,
    goal: OptimizationGoal,
    access_pattern: &AccessPattern,
) -> (u32, u32) {
    // Default tile size based on access pattern
    let base_size = match access_pattern.spatial_pattern {
        SpatialAccessPattern::Random => 256,
        SpatialAccessPattern::Regional => 512,
        SpatialAccessPattern::Sequential => 1024,
        SpatialAccessPattern::Tiled => 256,
    };

    // Adjust based on optimization goal
    let adjusted_size = match goal {
        OptimizationGoal::MinimizeSize => base_size.max(512), // Larger tiles compress better
        OptimizationGoal::MinimizeLatency => base_size.min(512), // Smaller tiles reduce latency
        OptimizationGoal::CloudCost => 512,                   // Balance egress and storage
        OptimizationGoal::WebServing => 256,                  // Standard web tile size
        OptimizationGoal::Balanced => base_size,
    };

    // Ensure tiles don't exceed image dimensions
    let tile_width = adjusted_size.min(width as u32);
    let tile_height = adjusted_size.min(height as u32);

    // Ensure power of 2
    let tile_width = tile_width.next_power_of_two().min(1024);
    let tile_height = tile_height.next_power_of_two().min(1024);

    (tile_width, tile_height)
}

/// Determines minimum overview size based on goal
fn determine_min_overview_size(goal: OptimizationGoal) -> u32 {
    match goal {
        OptimizationGoal::MinimizeSize => 512,    // Fewer overviews
        OptimizationGoal::MinimizeLatency => 128, // More overviews
        OptimizationGoal::CloudCost => 256,
        OptimizationGoal::WebServing => 256,
        OptimizationGoal::Balanced => 256,
    }
}

/// Estimates base image size
fn estimate_base_image_size(
    width: u64,
    height: u64,
    data_type: RasterDataType,
    samples_per_pixel: usize,
    compression_ratio: f64,
) -> u64 {
    let uncompressed_size =
        width * height * data_type.size_bytes() as u64 * samples_per_pixel as u64;
    (uncompressed_size as f64 / compression_ratio) as u64
}

/// Generates optimization recommendations
fn generate_recommendations(
    goal: OptimizationGoal,
    characteristics: &super::compression_selector::DataCharacteristics,
    width: u64,
    height: u64,
) -> Vec<String> {
    let mut recommendations = Vec::new();

    // Image size recommendations
    let megapixels = (width * height) as f64 / 1_000_000.0;
    if megapixels > 100.0 {
        recommendations.push(format!(
            "Large image ({:.1} MP) - COG format highly recommended for efficient access",
            megapixels
        ));
    } else if megapixels < 1.0 {
        recommendations
            .push("Small image - COG overhead may not provide significant benefits".to_string());
    }

    // Compression recommendations
    if characteristics.is_sparse {
        recommendations
            .push("Sparse data detected - compression will be very effective".to_string());
    }

    if characteristics.is_smooth {
        recommendations.push(
            "Smooth data detected - consider using predictor for better compression".to_string(),
        );
    }

    if characteristics.is_photographic && characteristics.entropy > 6.0 {
        recommendations.push(
            "High-entropy photographic data - JPEG compression may reduce file size significantly"
                .to_string(),
        );
    }

    // Goal-specific recommendations
    match goal {
        OptimizationGoal::MinimizeSize => {
            recommendations.push(
                "Size optimization: Using aggressive compression and minimal overviews".to_string(),
            );
        }
        OptimizationGoal::MinimizeLatency => {
            recommendations.push(
                "Latency optimization: Using smaller tiles and more overview levels".to_string(),
            );
        }
        OptimizationGoal::CloudCost => {
            recommendations.push(
                "Cloud cost optimization: Balancing storage costs with egress fees".to_string(),
            );
        }
        OptimizationGoal::WebServing => {
            recommendations.push(
                "Web serving optimization: Using standard 256x256 tiles for compatibility"
                    .to_string(),
            );
        }
        OptimizationGoal::Balanced => {
            recommendations
                .push("Balanced optimization: Good compromise between size and speed".to_string());
        }
    }

    // Data type recommendations
    match characteristics.data_type {
        RasterDataType::Float32 | RasterDataType::Float64 => {
            recommendations
                .push("Floating-point data: Consider DEFLATE or ZSTD compression".to_string());
        }
        RasterDataType::UInt8 => {
            if characteristics.unique_values < 256 {
                recommendations.push(
                    "8-bit integer data with limited unique values - highly compressible"
                        .to_string(),
                );
            }
        }
        _ => {}
    }

    recommendations
}

/// Calculates cost estimate for cloud storage
pub fn estimate_cloud_cost(
    file_size: u64,
    monthly_reads: u64,
    avg_tile_accesses_per_read: u32,
    tile_size: u32,
    compression_ratio: f64,
) -> CloudCostEstimate {
    // AWS S3 pricing (approximate, 2024)
    let storage_cost_per_gb_month = 0.023; // Standard storage
    let get_request_cost_per_1000 = 0.0004;
    let data_transfer_cost_per_gb = 0.09; // First 10 TB

    let file_size_gb = file_size as f64 / 1_073_741_824.0;
    let storage_cost = file_size_gb * storage_cost_per_gb_month;

    let total_requests = monthly_reads * avg_tile_accesses_per_read as u64;
    let request_cost = (total_requests as f64 / 1000.0) * get_request_cost_per_1000;

    let bytes_per_sample = tile_size as u64 * tile_size as u64;
    let compressed_tile_size = (bytes_per_sample as f64 / compression_ratio) as u64;
    let data_transfer_bytes = total_requests * compressed_tile_size;
    let data_transfer_gb = data_transfer_bytes as f64 / 1_073_741_824.0;
    let transfer_cost = data_transfer_gb * data_transfer_cost_per_gb;

    let total_cost = storage_cost + request_cost + transfer_cost;

    CloudCostEstimate {
        storage_cost,
        request_cost,
        transfer_cost,
        total_monthly_cost: total_cost,
        file_size_gb,
        requests_per_month: total_requests,
        data_transfer_gb,
    }
}

/// Cloud cost estimate
#[derive(Debug, Clone)]
pub struct CloudCostEstimate {
    /// Storage cost per month (USD)
    pub storage_cost: f64,
    /// Request cost per month (USD)
    pub request_cost: f64,
    /// Data transfer cost per month (USD)
    pub transfer_cost: f64,
    /// Total monthly cost (USD)
    pub total_monthly_cost: f64,
    /// File size in GB
    pub file_size_gb: f64,
    /// Number of requests per month
    pub requests_per_month: u64,
    /// Data transfer in GB per month
    pub data_transfer_gb: f64,
}

/// Compares two optimization strategies
pub fn compare_optimizations(
    opt1: &CogOptimization,
    opt2: &CogOptimization,
) -> OptimizationComparison {
    let size_diff_percent = if opt1.estimated_file_size > 0 {
        ((opt2.estimated_file_size as i64 - opt1.estimated_file_size as i64) as f64
            / opt1.estimated_file_size as f64)
            * 100.0
    } else {
        0.0
    };

    let compression_diff = opt2.estimated_compression_ratio - opt1.estimated_compression_ratio;

    let mut advantages_opt1 = Vec::new();
    let mut advantages_opt2 = Vec::new();

    if opt1.estimated_file_size < opt2.estimated_file_size {
        advantages_opt1.push("Smaller file size".to_string());
    } else if opt2.estimated_file_size < opt1.estimated_file_size {
        advantages_opt2.push("Smaller file size".to_string());
    }

    if opt1.optimal_tile_width < opt2.optimal_tile_width {
        advantages_opt1.push("Lower latency (smaller tiles)".to_string());
    } else if opt2.optimal_tile_width < opt1.optimal_tile_width {
        advantages_opt2.push("Lower latency (smaller tiles)".to_string());
    }

    if opt1.recommended_overviews.len() > opt2.recommended_overviews.len() {
        advantages_opt1.push("More overview levels".to_string());
    } else if opt2.recommended_overviews.len() > opt1.recommended_overviews.len() {
        advantages_opt2.push("More overview levels".to_string());
    }

    OptimizationComparison {
        size_difference_percent: size_diff_percent,
        compression_ratio_difference: compression_diff,
        advantages_option1: advantages_opt1,
        advantages_option2: advantages_opt2,
    }
}

/// Comparison between two optimization strategies
#[derive(Debug, Clone)]
pub struct OptimizationComparison {
    /// Size difference as percentage (negative = opt1 smaller)
    pub size_difference_percent: f64,
    /// Compression ratio difference
    pub compression_ratio_difference: f64,
    /// Advantages of first optimization
    pub advantages_option1: Vec<String>,
    /// Advantages of second optimization
    pub advantages_option2: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determine_optimal_tile_size() {
        let access_pattern = AccessPattern::default();

        let (w, h) =
            determine_optimal_tile_size(4096, 4096, OptimizationGoal::WebServing, &access_pattern);
        assert_eq!(w, 256);
        assert_eq!(h, 256);

        let (w, h) = determine_optimal_tile_size(
            4096,
            4096,
            OptimizationGoal::MinimizeSize,
            &access_pattern,
        );
        assert!(w >= 512);
        assert!(h >= 512);
    }

    #[test]
    fn test_tile_size_power_of_two() {
        let access_pattern = AccessPattern::default();
        let (w, h) =
            determine_optimal_tile_size(4096, 4096, OptimizationGoal::Balanced, &access_pattern);

        assert_eq!(w, w.next_power_of_two());
        assert_eq!(h, h.next_power_of_two());
    }

    #[test]
    fn test_tile_size_bounds() {
        let access_pattern = AccessPattern::default();

        // Small image
        let (w, h) =
            determine_optimal_tile_size(128, 128, OptimizationGoal::Balanced, &access_pattern);
        assert!(w <= 128);
        assert!(h <= 128);

        // Large image
        let (w, h) =
            determine_optimal_tile_size(16384, 16384, OptimizationGoal::Balanced, &access_pattern);
        assert!(w <= 1024);
        assert!(h <= 1024);
    }

    #[test]
    fn test_cloud_cost_estimation() {
        let estimate = estimate_cloud_cost(
            1_000_000_000, // 1 GB file
            10_000,        // 10k reads per month
            10,            // 10 tiles per read
            256,           // 256x256 tiles
            3.0,           // 3:1 compression
        );

        assert!(estimate.storage_cost > 0.0);
        assert!(estimate.request_cost > 0.0);
        assert!(estimate.transfer_cost > 0.0);
        assert!(estimate.total_monthly_cost > 0.0);
    }

    #[test]
    fn test_full_optimization() {
        let width = 1024;
        let height = 1024;
        let data_type = RasterDataType::UInt8;
        let samples_per_pixel = 1;
        let data = vec![128u8; width * height * samples_per_pixel];

        let result = analyze_for_cog(
            &data,
            width as u64,
            height as u64,
            data_type,
            samples_per_pixel,
            PhotometricInterpretation::BlackIsZero,
            OptimizationGoal::Balanced,
            None,
        );

        assert!(result.is_ok());
        let opt = result.expect("optimization should succeed");
        assert!(opt.optimal_tile_width > 0);
        assert!(opt.optimal_tile_height > 0);
        assert!(opt.estimated_file_size > 0);
    }

    #[test]
    fn test_optimization_comparison() {
        let opt1 = CogOptimization {
            optimal_tile_width: 256,
            optimal_tile_height: 256,
            recommended_compression: Compression::Deflate,
            recommended_overviews: vec![2, 4, 8],
            recommended_resampling: vec![
                OverviewResampling::Average,
                OverviewResampling::Average,
                OverviewResampling::Average,
            ],
            estimated_file_size: 1_000_000,
            estimated_compression_ratio: 2.5,
            optimization_goal: OptimizationGoal::Balanced,
            recommendations: vec![],
        };

        let opt2 = CogOptimization {
            optimal_tile_width: 512,
            optimal_tile_height: 512,
            recommended_compression: Compression::Zstd,
            recommended_overviews: vec![2, 4],
            recommended_resampling: vec![OverviewResampling::Average, OverviewResampling::Average],
            estimated_file_size: 800_000,
            estimated_compression_ratio: 3.0,
            optimization_goal: OptimizationGoal::MinimizeSize,
            recommendations: vec![],
        };

        let comparison = compare_optimizations(&opt1, &opt2);
        assert!(comparison.size_difference_percent < 0.0); // opt2 is smaller
        assert!(comparison.compression_ratio_difference > 0.0); // opt2 compresses better
    }
}
