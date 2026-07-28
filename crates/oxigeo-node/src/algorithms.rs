//! Algorithm bindings for Node.js
//!
//! This module provides geospatial algorithms including resampling,
//! calculator, terrain analysis, and statistical operations.

use napi::bindgen_prelude::*;
use napi_derive::napi;
use oxigeo_algorithms::raster::RasterCalculator;
use oxigeo_algorithms::raster::compute_zonal_stats as compute_zonal;
use oxigeo_algorithms::raster::{HillshadeParams, hillshade as compute_hillshade};
use oxigeo_algorithms::raster::{
    SlopeAspectConfig, SlopeUnits, aspect_advanced as compute_aspect_advanced,
    slope_advanced as compute_slope_advanced,
};
use oxigeo_algorithms::resampling::{Resampler, ResamplingMethod as CoreResamplingMethod};
use oxigeo_algorithms::vector::{
    AreaMethod as CoreAreaMethod, BufferCapStyle, BufferJoinStyle,
    BufferOptions as CoreBufferOptions, SimplifyMethod as CoreSimplifyMethod, area_polygon,
    buffer_point, buffer_polygon, simplify_linestring,
};
use oxigeo_core::buffer::RasterBuffer;

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
    use oxigeo_core::vector::Geometry;

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
    use oxigeo_core::vector::Geometry;

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
    use oxigeo_core::vector::Geometry;

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

/// Validates that a ground-unit pixel size is usable for terrain algorithms.
///
/// A non-positive or non-finite pixel size would silently produce `NaN`/`Inf`
/// slope, aspect, or hillshade values (since it is used as a division/scale
/// factor), so it is rejected explicitly instead of propagating garbage data.
fn validate_pixel_size(pixel_size: f64) -> Result<()> {
    if pixel_size.is_finite() && pixel_size > 0.0 {
        Ok(())
    } else {
        Err(NodeError {
            code: "INVALID_PARAMETER".to_string(),
            message: format!(
                "pixel_size must be a positive, finite number (in the DEM's ground units), got {}",
                pixel_size
            ),
        }
        .into())
    }
}

/// Computes hillshade from a DEM
///
/// `pixel_size` is the ground distance covered by one pixel (e.g. meters or
/// degrees, matching the DEM's coordinate reference system) and must be
/// supplied by the caller since raw pixel buffers carry no georeferencing.
#[napi]
pub fn hillshade(
    dem: &BufferWrapper,
    azimuth: f64,
    altitude: f64,
    z_factor: f64,
    pixel_size: f64,
) -> Result<BufferWrapper> {
    validate_pixel_size(pixel_size)?;

    let params = HillshadeParams {
        azimuth,
        altitude,
        z_factor,
        pixel_size,
        scale: 255.0,
    };

    let result = compute_hillshade(dem.inner(), params).to_napi()?;
    Ok(BufferWrapper::from_raster_buffer(result))
}

/// Computes slope from a DEM
///
/// `pixel_size` is the ground distance covered by one pixel (e.g. meters or
/// degrees, matching the DEM's coordinate reference system). When
/// `as_percent` is `true`, the result is percent rise instead of degrees.
#[napi]
pub fn slope(
    dem: &BufferWrapper,
    pixel_size: f64,
    z_factor: f64,
    as_percent: bool,
) -> Result<BufferWrapper> {
    validate_pixel_size(pixel_size)?;

    let config = SlopeAspectConfig {
        slope_units: if as_percent {
            SlopeUnits::Percent
        } else {
            SlopeUnits::Degrees
        },
        z_factor,
        ..Default::default()
    };

    let result = compute_slope_advanced(dem.inner(), pixel_size, &config).to_napi()?;
    Ok(BufferWrapper::from_raster_buffer(result))
}

/// Computes aspect from a DEM
///
/// `pixel_size` is the ground distance covered by one pixel (e.g. meters or
/// degrees, matching the DEM's coordinate reference system). Aspect (the
/// compass direction a slope faces) does not depend on the vertical
/// exaggeration factor since it is a ratio of the x/y gradients.
#[napi]
pub fn aspect(dem: &BufferWrapper, pixel_size: f64) -> Result<BufferWrapper> {
    validate_pixel_size(pixel_size)?;

    let result = compute_aspect_advanced(dem.inner(), pixel_size, &SlopeAspectConfig::default())
        .to_napi()?;
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
    pub(crate) fn from_geometry(geom: oxigeo_core::vector::Geometry) -> Self {
        Self { inner: geom }
    }
}

/// Evaluates a raster-calculator expression across the supplied bands.
///
/// The full map-algebra expression language from `oxigeo-algorithms`
/// ([`RasterCalculator`]) is supported: arithmetic (`+ - * / ^`), math
/// functions (`sqrt`, `log`, `exp`, `sin`, `min`, `max`, ...), comparisons,
/// logical `and`/`or`, and `if/then/else`. Bands are referenced positionally as
/// `B1`, `B2`, ... (1-indexed), matching the Python `oxigeo.calc()` binding, so
/// an NDVI is written `"(B1 - B2) / (B1 + B2)"`.
///
/// For backward compatibility, a bare single uppercase letter (`"A"`..`"Z"`)
/// is still treated as a direct 0-indexed band pass-through (`A` == first band).
#[allow(dead_code)]
fn evaluate_expression(expr: &str, bands: Vec<&BufferWrapper>) -> Result<RasterBuffer> {
    // Legacy single-letter pass-through (A = band 0, B = band 1, ...). This is
    // kept for API stability; the general path below uses B1/B2 references.
    let trimmed = expr.trim();
    if trimmed.len() == 1
        && trimmed
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_uppercase())
    {
        let band_idx = (trimmed.as_bytes()[0] - b'A') as usize;
        if band_idx >= bands.len() {
            return Err(NodeError {
                code: "INVALID_EXPRESSION".to_string(),
                message: format!("Band {} not found", trimmed),
            }
            .into());
        }
        return Ok(bands[band_idx].inner().clone());
    }

    // General case: delegate to the shared raster calculator, which owns its
    // input buffers, so materialize owned clones of each band.
    let owned: Vec<RasterBuffer> = bands.iter().map(|b| b.inner().clone()).collect();
    RasterCalculator::evaluate(trimmed, &owned).to_napi()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use oxigeo_core::types::RasterDataType;

    /// Builds a 5x5 float32 DEM ramping linearly along x: elevation = x * 10.0.
    fn ramp_dem() -> BufferWrapper {
        let mut buf = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        for y in 0..5 {
            for x in 0..5 {
                buf.set_pixel(x, y, (x as f64) * 10.0)
                    .expect("set_pixel should succeed for in-bounds coordinates");
            }
        }
        BufferWrapper::from_raster_buffer(buf)
    }

    /// Asserts a `Result<BufferWrapper>` is an error and returns its message.
    ///
    /// `BufferWrapper` does not implement `Debug` (it wraps a large pixel
    /// buffer), so `Result::expect_err`/`unwrap_err` cannot be used directly.
    fn expect_error_message(result: Result<BufferWrapper>) -> String {
        match result {
            Ok(_) => panic!("expected an error, got Ok(..)"),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn slope_rejects_non_positive_pixel_size() {
        let dem = ramp_dem();

        let message = expect_error_message(slope(&dem, 0.0, 1.0, false));
        assert!(message.contains("INVALID_PARAMETER"));

        let message = expect_error_message(slope(&dem, -5.0, 1.0, false));
        assert!(message.contains("INVALID_PARAMETER"));

        let message = expect_error_message(slope(&dem, f64::NAN, 1.0, false));
        assert!(message.contains("INVALID_PARAMETER"));
    }

    #[test]
    fn hillshade_rejects_non_positive_pixel_size() {
        let dem = ramp_dem();
        let message = expect_error_message(hillshade(&dem, 315.0, 45.0, 1.0, 0.0));
        assert!(message.contains("INVALID_PARAMETER"));
    }

    #[test]
    fn aspect_rejects_non_positive_pixel_size() {
        let dem = ramp_dem();
        let message = expect_error_message(aspect(&dem, f64::INFINITY));
        assert!(message.contains("INVALID_PARAMETER"));
    }

    #[test]
    fn slope_scales_inversely_with_pixel_size() {
        let dem = ramp_dem();

        let slope_1m = slope(&dem, 1.0, 1.0, false).expect("slope at 1.0 pixel_size");
        let slope_10m = slope(&dem, 10.0, 1.0, false).expect("slope at 10.0 pixel_size");

        // At an interior pixel, the same elevation ramp spread over a larger
        // pixel_size means a gentler rise/run gradient, hence a smaller slope
        // angle. This is exactly the bug: previously pixel_size was hardcoded
        // to 1.0 regardless of what was passed in, so both calls would have
        // produced identical output.
        let center_1m = slope_1m.get_pixel(2, 2).expect("center pixel readable");
        let center_10m = slope_10m.get_pixel(2, 2).expect("center pixel readable");

        assert!(
            center_1m > center_10m,
            "slope with pixel_size=1.0 ({center_1m}) should exceed slope with pixel_size=10.0 ({center_10m})"
        );
    }

    #[test]
    fn slope_as_percent_flag_changes_units() {
        let dem = ramp_dem();

        let degrees = slope(&dem, 1.0, 1.0, false).expect("slope in degrees");
        let percent = slope(&dem, 1.0, 1.0, true).expect("slope in percent");

        let degrees_value = degrees.get_pixel(2, 2).expect("center pixel readable");
        let percent_value = percent.get_pixel(2, 2).expect("center pixel readable");

        // Previously `as_percent` was discarded entirely, so both calls would
        // yield identical (degree-based) output regardless of the flag.
        assert!(
            (degrees_value - percent_value).abs() > 1e-6,
            "as_percent=true must change the output units: degrees={degrees_value}, percent={percent_value}"
        );

        // tan(degrees) * 100 == percent, cross-checking against the known
        // conversion formula rather than just asserting inequality. The
        // buffers are float32, so allow for that reduced precision rather
        // than requiring f64-level exactness.
        let expected_percent = degrees_value.to_radians().tan() * 100.0;
        assert!(
            (expected_percent - percent_value).abs() < 1e-2,
            "percent value {percent_value} should equal tan(degrees) * 100 = {expected_percent}"
        );
    }

    #[test]
    fn hillshade_pixel_size_affects_output() {
        let dem = ramp_dem();

        let shade_1m = hillshade(&dem, 315.0, 45.0, 1.0, 1.0).expect("hillshade at 1.0");
        let shade_10m = hillshade(&dem, 315.0, 45.0, 1.0, 10.0).expect("hillshade at 10.0");

        let value_1m = shade_1m.get_pixel(2, 2).expect("center pixel readable");
        let value_10m = shade_10m.get_pixel(2, 2).expect("center pixel readable");

        assert!(
            (value_1m - value_10m).abs() > 1e-6,
            "hillshade should differ when pixel_size changes: {value_1m} vs {value_10m}"
        );
    }

    #[test]
    fn aspect_pixel_size_accepted_for_valid_input() {
        let dem = ramp_dem();
        let result = aspect(&dem, 30.0);
        assert!(result.is_ok(), "aspect should accept a valid pixel_size");
    }

    /// Builds a constant-valued float32 band.
    fn const_band(width: u32, height: u32, value: f64) -> BufferWrapper {
        let mut buf = RasterBuffer::zeros(width as u64, height as u64, RasterDataType::Float32);
        for y in 0..height as u64 {
            for x in 0..width as u64 {
                buf.set_pixel(x, y, value).expect("set pixel");
            }
        }
        BufferWrapper::from_raster_buffer(buf)
    }

    #[test]
    fn calculate_supports_band_algebra_ndvi() {
        let nir = const_band(4, 4, 100.0);
        let red = const_band(4, 4, 50.0);
        let result = calculate("(B1 - B2) / (B1 + B2)".to_string(), vec![&nir, &red])
            .expect("NDVI expression should evaluate");
        let expected = (100.0 - 50.0) / (100.0 + 50.0);
        let value = result.get_pixel(0, 0).expect("read pixel");
        assert!(
            (value - expected).abs() < 1e-3,
            "NDVI expected {expected}, got {value}"
        );
    }

    #[test]
    fn calculate_supports_math_functions() {
        let band = const_band(3, 3, 16.0);
        let result = calculate("sqrt(B1)".to_string(), vec![&band]).expect("sqrt should evaluate");
        let value = result.get_pixel(1, 1).expect("read pixel");
        assert!(
            (value - 4.0).abs() < 1e-3,
            "sqrt(16) should be 4, got {value}"
        );
    }

    #[test]
    fn calculate_supports_conditionals() {
        let mut buf = RasterBuffer::zeros(3, 1, RasterDataType::Float32);
        buf.set_pixel(0, 0, 10.0).expect("set");
        buf.set_pixel(1, 0, 30.0).expect("set");
        buf.set_pixel(2, 0, 50.0).expect("set");
        let band = BufferWrapper::from_raster_buffer(buf);

        let result = calculate("if B1 > 20 then 1 else 0".to_string(), vec![&band])
            .expect("conditional should evaluate");
        assert_eq!(result.get_pixel(0, 0).expect("read"), 0.0);
        assert_eq!(result.get_pixel(1, 0).expect("read"), 1.0);
        assert_eq!(result.get_pixel(2, 0).expect("read"), 1.0);
    }

    #[test]
    fn calculate_legacy_single_letter_passthrough_still_works() {
        let a = const_band(2, 2, 7.0);
        let b = const_band(2, 2, 9.0);
        let result = calculate("B".to_string(), vec![&a, &b]).expect("single-letter passthrough");
        // "B" selects the second band (index 1).
        assert_eq!(result.get_pixel(0, 0).expect("read"), 9.0);
    }

    #[test]
    fn calculate_reports_invalid_expression() {
        let band = const_band(2, 2, 1.0);
        assert!(calculate("B1 +".to_string(), vec![&band]).is_err());
        assert!(calculate("undefined_fn(B1)".to_string(), vec![&band]).is_err());
    }
}
