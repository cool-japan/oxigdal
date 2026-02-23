//! Algorithm bindings for Node.js
//!
//! This module provides geospatial algorithms including resampling,
//! calculator, terrain analysis, and statistical operations.

use napi::bindgen_prelude::*;
use napi_derive::napi;
use oxigdal_algorithms::raster::compute_zonal_stats as compute_zonal;
use oxigdal_algorithms::raster::{HillshadeParams, hillshade as compute_hillshade};
use oxigdal_algorithms::raster::{aspect as compute_aspect, slope as compute_slope};
use oxigdal_algorithms::resampling::{Resampler, ResamplingMethod as CoreResamplingMethod};
use oxigdal_algorithms::vector::{
    AreaMethod as CoreAreaMethod, BufferCapStyle, BufferJoinStyle,
    BufferOptions as CoreBufferOptions, SimplifyMethod as CoreSimplifyMethod, area_polygon,
    buffer_point, buffer_polygon, simplify_linestring,
};
use oxigdal_core::buffer::RasterBuffer;

use crate::buffer::BufferWrapper;
use crate::error::{NodeError, ToNapiResult};
use crate::vector::GeometryWrapper;

/// Resampling methods
#[napi]
pub enum ResamplingMethod {
    /// Nearest neighbor (fast, preserves exact values)
    NearestNeighbor,
    /// Bilinear interpolation (smooth, good for continuous data)
    Bilinear,
    /// Bicubic interpolation (high quality, slower)
    Bicubic,
    /// Lanczos resampling (highest quality, expensive)
    Lanczos,
}

impl From<ResamplingMethod> for CoreResamplingMethod {
    fn from(method: ResamplingMethod) -> Self {
        match method {
            ResamplingMethod::NearestNeighbor => CoreResamplingMethod::Nearest,
            ResamplingMethod::Bilinear => CoreResamplingMethod::Bilinear,
            ResamplingMethod::Bicubic => CoreResamplingMethod::Bicubic,
            ResamplingMethod::Lanczos => CoreResamplingMethod::Lanczos,
        }
    }
}

/// Resamples a raster buffer to a new size
#[napi]
pub fn resample(
    buffer: &BufferWrapper,
    new_width: u32,
    new_height: u32,
    method: ResamplingMethod,
) -> Result<BufferWrapper> {
    let resampler = Resampler::new(method.into());
    let resampled = resampler
        .resample(buffer.inner(), new_width as u64, new_height as u64)
        .to_napi()?;

    Ok(BufferWrapper::from_raster_buffer(resampled))
}

/// Raster calculator - evaluates an expression on raster bands
#[allow(dead_code)]
#[napi]
pub fn calculate(expression: String, bands: Vec<&BufferWrapper>) -> Result<BufferWrapper> {
    if bands.is_empty() {
        return Err(NodeError {
            code: "INVALID_INPUT".to_string(),
            message: "At least one band is required".to_string(),
        }
        .into());
    }

    // Get dimensions from first band
    let width = bands[0].width() as u64;
    let height = bands[0].height() as u64;

    // Verify all bands have same dimensions
    for (i, band) in bands.iter().enumerate() {
        if band.width() as u64 != width || band.height() as u64 != height {
            return Err(NodeError {
                code: "DIMENSION_MISMATCH".to_string(),
                message: format!(
                    "Band {} has different dimensions ({}x{}) than first band ({}x{})",
                    i,
                    band.width(),
                    band.height(),
                    width,
                    height
                ),
            }
            .into());
        }
    }

    // Simple expression evaluation (supports basic operations)
    let result = evaluate_expression(&expression, bands)?;
    Ok(BufferWrapper::from_raster_buffer(result))
}

/// Buffer operations - creates a buffer around geometries
#[napi]
pub fn buffer(geometry: &GeometryWrapper, distance: f64, segments: u32) -> Result<GeometryWrapper> {
    use oxigdal_core::vector::Geometry;

    let options = CoreBufferOptions {
        quadrant_segments: segments as usize,
        cap_style: BufferCapStyle::Round,
        join_style: BufferJoinStyle::Round,
        miter_limit: 5.0,
        simplify_tolerance: 0.0,
    };

    let buffered = match geometry.inner() {
        Geometry::Point(p) => {
            let polygon = buffer_point(p, distance, &options).to_napi()?;
            Geometry::Polygon(polygon)
        }
        Geometry::Polygon(p) => {
            let polygon = buffer_polygon(p, distance, &options).to_napi()?;
            Geometry::Polygon(polygon)
        }
        _ => {
            return Err(NodeError {
                code: "NOT_IMPLEMENTED".to_string(),
                message: "Buffer not implemented for this geometry type".to_string(),
            }
            .into());
        }
    };

    Ok(GeometryWrapper::from_geometry(buffered))
}

/// Calculates the area of a polygon
#[napi]
pub fn area(geometry: &GeometryWrapper, method: String) -> Result<f64> {
    use oxigdal_core::vector::Geometry;

    let area_method = match method.to_lowercase().as_str() {
        "planar" => CoreAreaMethod::Planar,
        "geodetic" => CoreAreaMethod::Geodetic,
        _ => {
            return Err(NodeError {
                code: "INVALID_PARAMETER".to_string(),
                message: format!("Unknown area method: {}", method),
            }
            .into());
        }
    };

    match geometry.inner() {
        Geometry::Polygon(p) => area_polygon(p, area_method).to_napi(),
        _ => Err(NodeError {
            code: "INVALID_GEOMETRY".to_string(),
            message: "Area calculation requires a Polygon geometry".to_string(),
        }
        .into()),
    }
}

/// Simplifies a geometry using the specified method
#[napi]
pub fn simplify(
    geometry: &GeometryWrapper,
    tolerance: f64,
    method: String,
) -> Result<GeometryWrapper> {
    use oxigdal_core::vector::Geometry;

    let simplify_method = match method.to_lowercase().as_str() {
        "douglas-peucker" | "dp" => CoreSimplifyMethod::DouglasPeucker,
        "visvalingam-whyatt" | "vw" => CoreSimplifyMethod::VisvalingamWhyatt,
        _ => {
            return Err(NodeError {
                code: "INVALID_PARAMETER".to_string(),
                message: format!("Unknown simplify method: {}", method),
            }
            .into());
        }
    };

    let simplified = match geometry.inner() {
        Geometry::LineString(ls) => {
            let simple_ls = simplify_linestring(ls, tolerance, simplify_method).to_napi()?;
            Geometry::LineString(simple_ls)
        }
        _ => {
            return Err(NodeError {
                code: "INVALID_GEOMETRY".to_string(),
                message: "Simplify currently only supports LineString geometry".to_string(),
            }
            .into());
        }
    };

    Ok(GeometryWrapper::from_geometry(simplified))
}

/// Computes hillshade from a DEM
#[napi]
pub fn hillshade(
    dem: &BufferWrapper,
    azimuth: f64,
    altitude: f64,
    z_factor: f64,
) -> Result<BufferWrapper> {
    let params = HillshadeParams {
        azimuth,
        altitude,
        z_factor,
        pixel_size: 1.0,
        scale: 255.0,
    };

    let result = compute_hillshade(dem.inner(), params).to_napi()?;
    Ok(BufferWrapper::from_raster_buffer(result))
}

/// Computes slope from a DEM
#[napi]
pub fn slope(dem: &BufferWrapper, z_factor: f64, _as_percent: bool) -> Result<BufferWrapper> {
    // Note: pixel_size is assumed to be 1.0 for now
    let result = compute_slope(dem.inner(), 1.0, z_factor).to_napi()?;
    Ok(BufferWrapper::from_raster_buffer(result))
}

/// Computes aspect from a DEM
#[napi]
pub fn aspect(dem: &BufferWrapper) -> Result<BufferWrapper> {
    // Note: pixel_size and z_factor are assumed to be 1.0 for now
    let result = compute_aspect(dem.inner(), 1.0, 1.0).to_napi()?;
    Ok(BufferWrapper::from_raster_buffer(result))
}

/// Computes zonal statistics
#[napi]
pub fn zonal_stats(raster: &BufferWrapper, zones: &BufferWrapper) -> Result<Vec<ZonalStatistics>> {
    let core_stats = compute_zonal(raster.inner(), zones.inner()).to_napi()?;

    let results = core_stats
        .into_iter()
        .map(|s| ZonalStatistics {
            zone_id: s.zone_id,
            count: s.count as u32,
            min: s.min,
            max: s.max,
            mean: s.mean,
            stddev: s.std_dev,
            sum: s.sum,
        })
        .collect();

    Ok(results)
}

/// Zonal statistics result
#[napi(object)]
pub struct ZonalStatistics {
    pub zone_id: i32,
    pub count: u32,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub stddev: f64,
    pub sum: f64,
}

// Helper functions

impl GeometryWrapper {
    #[allow(dead_code)]
    pub(crate) fn from_geometry(geom: oxigdal_core::vector::Geometry) -> Self {
        Self { inner: geom }
    }
}

/// Simple expression evaluator for raster calculator
#[allow(dead_code)]
fn evaluate_expression(expr: &str, bands: Vec<&BufferWrapper>) -> Result<RasterBuffer> {
    // For now, use the RasterCalculator from oxigdal-algorithms
    // Simple single-band pass-through
    if expr.len() == 1 && expr.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        let band_idx = (expr.as_bytes()[0] - b'A') as usize;
        if band_idx >= bands.len() {
            return Err(NodeError {
                code: "INVALID_EXPRESSION".to_string(),
                message: format!("Band {} not found", expr),
            }
            .into());
        }
        Ok(bands[band_idx].inner().clone())
    } else {
        Err(NodeError {
            code: "NOT_IMPLEMENTED".to_string(),
            message: "Complex expressions not yet supported".to_string(),
        }
        .into())
    }
}
