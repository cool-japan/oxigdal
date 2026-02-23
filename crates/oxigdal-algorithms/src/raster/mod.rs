//! Raster processing operations
//!
//! This module provides various raster processing algorithms including:
//!
//! - Terrain analysis (hillshade, slope, aspect, TPI, TRI, curvature, VRM)
//! - Terrain classification (landform classification, TWI, SPI)
//! - Raster calculator (map algebra with expression parsing)
//! - Classification (equal interval, quantile, natural breaks)
//! - Statistics (mean, median, percentiles, histograms, zonal stats)
//! - Spatial filters (Gaussian, median, edge detection, sharpening)
//! - Morphological operations (dilation, erosion, opening, closing)
//! - Reclassification
//! - Focal operations (filters, convolution, neighborhood statistics)
//! - Viewshed analysis (R1/R2/R3 algorithms, earth curvature, Fresnel zone)
//! - Cost-distance analysis (isotropic, anisotropic, A*, corridor)

mod calculator;
mod classify;
mod cost_distance;
mod filters;
mod focal;
mod hillshade;
pub mod hydrology;
mod morphology;
mod reclassify;
mod slope_aspect;
mod statistics;
mod terrain;
mod texture;
mod viewshed;
mod zonal_stats;

// Calculator
pub use calculator::{RasterCalculator, RasterExpression};

// Classification
pub use classify::{ClassificationMethod, ClassificationRule, classify, reclassify, threshold};

// Filters
pub use filters::{
    BoundaryMode as FilterBoundaryMode, EdgeDetector, detect_edges, gaussian_blur,
    high_pass_filter, laplacian_edge_detection, low_pass_filter, median_filter,
    prewitt_edge_detection, sharpen, sobel_edge_detection,
};

// Focal operations
pub use focal::{
    BoundaryMode as FocalBoundaryMode, FocalOperation, WindowShape, focal_convolve, focal_majority,
    focal_max, focal_mean, focal_mean_separable, focal_median, focal_min, focal_range,
    focal_stddev, focal_sum, focal_variety,
};

// Hillshade
pub use hillshade::{
    CombinedHillshadeParams, CombinedHillshadeStyle, HillshadeParams, combined_hillshade,
    hillshade, multidirectional_hillshade, swiss_hillshade,
};

// Morphology
pub use morphology::{
    StructuringElement, black_hat, close, dilate, erode, external_gradient, internal_gradient,
    morphological_gradient, open, top_hat,
};

// Reclassify (legacy)
pub use reclassify::ReclassRule;

// Slope/Aspect (enhanced with multiple algorithms, units, edge handling)
pub use slope_aspect::{
    EdgeHandling, SlopeAlgorithm, SlopeAspectConfig, SlopeAspectOutput, SlopeUnits, aspect,
    aspect_advanced, compute_slope_aspect, compute_slope_aspect_advanced, convert_slope_degrees,
    slope, slope_advanced,
};

// Statistics
pub use statistics::{
    Histogram, Percentiles, RasterStatistics, Zone, compute_histogram, compute_mode,
    compute_percentiles, compute_statistics, compute_zonal_statistics,
};

// Terrain analysis (enhanced with TRI methods, roughness methods, classification, TWI/SPI)
pub use terrain::{
    CurvatureType, LandformClass, RoughnessMethod, TpiNeighborhood, TriMethod, classify_landforms,
    classify_landforms_multiscale, compute_aspect_degrees, compute_convergence_index,
    compute_curvature, compute_roughness, compute_roughness_advanced, compute_slope_degrees,
    compute_spi, compute_terrain_shape_index, compute_tpi, compute_tpi_advanced, compute_tri,
    compute_tri_advanced, compute_twi, compute_vrm,
};

// Texture analysis
pub use texture::{
    Direction as TextureDirection, Glcm, GlcmParams, HaralickFeatures,
    compute_all_texture_features, compute_glcm, compute_glcm_multi_direction,
    compute_haralick_features, compute_texture_feature_image,
};

// Viewshed analysis (enhanced with R2/R3 algorithms, curvature, Fresnel)
pub use viewshed::{
    CurvatureCorrection, ObserverPoint, ViewshedAlgorithm, ViewshedConfig, ViewshedResult,
    compute_cumulative_viewshed, compute_cumulative_viewshed_advanced, compute_fresnel_clearance,
    compute_los_profile, compute_viewshed, compute_viewshed_advanced,
};

// Cost-distance analysis (enhanced with anisotropic, barriers, A*, corridor)
pub use cost_distance::{
    CostDistanceResult, Direction as CostDirection, FrictionModel, astar_path, compute_corridor,
    compute_corridor_normalized, cost_distance, cost_distance_anisotropic, cost_distance_full,
    euclidean_distance, least_cost_path, least_cost_path_from_direction,
};

// Zonal stats (legacy)
pub use zonal_stats::{ZonalStatistics, compute_zonal_stats};

use crate::error::Result;
use oxigdal_core::buffer::RasterBuffer;

/// Applies a 3x3 convolution kernel to a raster
///
/// # Arguments
///
/// * `src` - Source raster buffer
/// * `kernel` - 3x3 convolution kernel (row-major order)
/// * `scale` - Scale factor applied to result
/// * `offset` - Offset added to result
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn convolve_3x3(
    src: &RasterBuffer,
    kernel: &[f64; 9],
    scale: f64,
    offset: f64,
) -> Result<RasterBuffer> {
    let width = src.width();
    let height = src.height();
    let mut dst = RasterBuffer::zeros(width, height, src.data_type());

    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            let mut sum = 0.0;
            let mut idx = 0;

            for dy in -1..=1i64 {
                for dx in -1..=1i64 {
                    let px = (x as i64 + dx) as u64;
                    let py = (y as i64 + dy) as u64;
                    let value = src
                        .get_pixel(px, py)
                        .map_err(crate::error::AlgorithmError::Core)?;
                    sum += value * kernel[idx];
                    idx += 1;
                }
            }

            let result = sum * scale + offset;
            dst.set_pixel(x, y, result)
                .map_err(crate::error::AlgorithmError::Core)?;
        }
    }

    Ok(dst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_convolve_identity() {
        let src = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let kernel = [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        let result = convolve_3x3(&src, &kernel, 1.0, 0.0);
        assert!(result.is_ok());
    }
}
