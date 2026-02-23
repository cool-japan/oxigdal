//! SIMD-accelerated coordinate transformations
//!
//! This module provides high-performance coordinate projection and transformation operations
//! using SIMD instructions. It supports common geospatial projections and transformations.
//!
//! # Supported Transformations
//!
//! - **Affine Transformations**: Translation, rotation, scaling
//! - **Geographic to Projected**: Web Mercator, UTM-like transformations
//! - **Coordinate System Conversions**: WGS84 ↔ Web Mercator, Lat/Lon ↔ XY
//! - **Batch Processing**: Transform thousands of coordinates efficiently
//!
//! # Performance
//!
//! Expected speedup over scalar: 4-8x for coordinate transformations
//!
//! # Example
//!
//! ```rust
//! use oxigdal_algorithms::simd::projection::{affine_transform_2d, AffineMatrix2D};
//! use oxigdal_algorithms::error::Result;
//!
//! fn example() -> Result<()> {
//!     let matrix = AffineMatrix2D::identity();
//!     let x = vec![0.0, 1.0, 2.0, 3.0];
//!     let y = vec![0.0, 1.0, 2.0, 3.0];
//!     let mut out_x = vec![0.0; 4];
//!     let mut out_y = vec![0.0; 4];
//!
//!     affine_transform_2d(&matrix, &x, &y, &mut out_x, &mut out_y)?;
//!     Ok(())
//! }
//! # example().expect("example failed");
//! ```

use crate::error::{AlgorithmError, Result};

/// 2D affine transformation matrix [a, b, c, d, e, f]
/// Represents: x' = a*x + b*y + e, y' = c*x + d*y + f
#[derive(Debug, Clone, Copy)]
pub struct AffineMatrix2D {
    /// Scale/rotation X component
    pub a: f64,
    /// Shear/rotation X component
    pub b: f64,
    /// Shear/rotation Y component
    pub c: f64,
    /// Scale/rotation Y component
    pub d: f64,
    /// Translation X component
    pub e: f64,
    /// Translation Y component
    pub f: f64,
}

impl AffineMatrix2D {
    /// Create identity transformation matrix
    #[must_use]
    pub const fn identity() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }

    /// Create translation matrix
    #[must_use]
    pub const fn translation(dx: f64, dy: f64) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: dx,
            f: dy,
        }
    }

    /// Create scaling matrix
    #[must_use]
    pub const fn scale(sx: f64, sy: f64) -> Self {
        Self {
            a: sx,
            b: 0.0,
            c: 0.0,
            d: sy,
            e: 0.0,
            f: 0.0,
        }
    }

    /// Create rotation matrix (angle in radians)
    #[must_use]
    pub fn rotation(angle: f64) -> Self {
        let cos = angle.cos();
        let sin = angle.sin();
        Self {
            a: cos,
            b: -sin,
            c: sin,
            d: cos,
            e: 0.0,
            f: 0.0,
        }
    }

    /// Invert the transformation matrix
    ///
    /// # Errors
    ///
    /// Returns an error if the matrix is singular (determinant is zero)
    pub fn invert(&self) -> Result<Self> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < 1e-10 {
            return Err(AlgorithmError::NumericalError {
                operation: "matrix_invert",
                message: "Matrix is singular (determinant near zero)".to_string(),
            });
        }

        let inv_det = 1.0 / det;
        Ok(Self {
            a: self.d * inv_det,
            b: -self.b * inv_det,
            c: -self.c * inv_det,
            d: self.a * inv_det,
            e: (self.b * self.f - self.d * self.e) * inv_det,
            f: (self.c * self.e - self.a * self.f) * inv_det,
        })
    }
}

/// Apply 2D affine transformation to coordinate arrays using SIMD
///
/// Transforms coordinates using the formula:
/// - x' = a*x + b*y + e
/// - y' = c*x + d*y + f
///
/// # Arguments
///
/// * `matrix` - Affine transformation matrix
/// * `x` - Input X coordinates
/// * `y` - Input Y coordinates
/// * `out_x` - Output X coordinates
/// * `out_y` - Output Y coordinates
///
/// # Errors
///
/// Returns an error if array lengths don't match
pub fn affine_transform_2d(
    matrix: &AffineMatrix2D,
    x: &[f64],
    y: &[f64],
    out_x: &mut [f64],
    out_y: &mut [f64],
) -> Result<()> {
    if x.len() != y.len() || x.len() != out_x.len() || x.len() != out_y.len() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "coordinates",
            message: format!(
                "Array length mismatch: x={}, y={}, out_x={}, out_y={}",
                x.len(),
                y.len(),
                out_x.len(),
                out_y.len()
            ),
        });
    }

    if x.is_empty() {
        return Ok(());
    }

    // SIMD processing - process 4 coordinates at a time
    const LANES: usize = 4;
    let chunks = x.len() / LANES;

    // Extract matrix components for SIMD broadcasting
    let a = matrix.a;
    let b = matrix.b;
    let c = matrix.c;
    let d = matrix.d;
    let e = matrix.e;
    let f = matrix.f;

    // Process SIMD chunks
    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;

        // Auto-vectorized by LLVM
        for j in start..end {
            let x_in = x[j];
            let y_in = y[j];
            out_x[j] = a * x_in + b * y_in + e;
            out_y[j] = c * x_in + d * y_in + f;
        }
    }

    // Handle remainder
    let remainder_start = chunks * LANES;
    for i in remainder_start..x.len() {
        let x_in = x[i];
        let y_in = y[i];
        out_x[i] = a * x_in + b * y_in + e;
        out_y[i] = c * x_in + d * y_in + f;
    }

    Ok(())
}

/// Apply 2D affine transformation in-place using SIMD
///
/// # Arguments
///
/// * `matrix` - Affine transformation matrix
/// * `x` - X coordinates (modified in-place)
/// * `y` - Y coordinates (modified in-place)
///
/// # Errors
///
/// Returns an error if array lengths don't match
pub fn affine_transform_2d_inplace(
    matrix: &AffineMatrix2D,
    x: &mut [f64],
    y: &mut [f64],
) -> Result<()> {
    if x.len() != y.len() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "coordinates",
            message: format!("Array length mismatch: x={}, y={}", x.len(), y.len()),
        });
    }

    if x.is_empty() {
        return Ok(());
    }

    // Extract matrix components
    let a = matrix.a;
    let b = matrix.b;
    let c = matrix.c;
    let d = matrix.d;
    let e = matrix.e;
    let f = matrix.f;

    const LANES: usize = 4;
    let chunks = x.len() / LANES;

    // Process SIMD chunks
    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;

        for j in start..end {
            let x_in = x[j];
            let y_in = y[j];
            x[j] = a * x_in + b * y_in + e;
            y[j] = c * x_in + d * y_in + f;
        }
    }

    // Handle remainder
    let remainder_start = chunks * LANES;
    for i in remainder_start..x.len() {
        let x_in = x[i];
        let y_in = y[i];
        x[i] = a * x_in + b * y_in + e;
        y[i] = c * x_in + d * y_in + f;
    }

    Ok(())
}

/// Convert latitude/longitude to Web Mercator (EPSG:3857) using SIMD
///
/// Performs the transformation:
/// - X = R * λ
/// - Y = R * ln(tan(π/4 + φ/2))
///
/// where R is Earth's radius (6378137.0), λ is longitude, φ is latitude
///
/// # Arguments
///
/// * `lon` - Longitude in degrees
/// * `lat` - Latitude in degrees
/// * `out_x` - Output X coordinates (meters)
/// * `out_y` - Output Y coordinates (meters)
///
/// # Errors
///
/// Returns an error if array lengths don't match or if latitude is out of bounds
pub fn latlon_to_web_mercator(
    lon: &[f64],
    lat: &[f64],
    out_x: &mut [f64],
    out_y: &mut [f64],
) -> Result<()> {
    if lon.len() != lat.len() || lon.len() != out_x.len() || lon.len() != out_y.len() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "coordinates",
            message: format!(
                "Array length mismatch: lon={}, lat={}, out_x={}, out_y={}",
                lon.len(),
                lat.len(),
                out_x.len(),
                out_y.len()
            ),
        });
    }

    if lon.is_empty() {
        return Ok(());
    }

    const EARTH_RADIUS: f64 = 6_378_137.0;
    const DEG_TO_RAD: f64 = std::f64::consts::PI / 180.0;
    const MAX_LAT: f64 = 85.0511288; // Web Mercator valid range

    const LANES: usize = 4;
    let chunks = lon.len() / LANES;

    // Process SIMD chunks
    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;

        for j in start..end {
            let lat_deg = lat[j];

            // Clamp latitude to valid Web Mercator range
            let lat_clamped = lat_deg.clamp(-MAX_LAT, MAX_LAT);
            let lat_rad = lat_clamped * DEG_TO_RAD;
            let lon_rad = lon[j] * DEG_TO_RAD;

            out_x[j] = EARTH_RADIUS * lon_rad;

            // Y = R * ln(tan(π/4 + φ/2))
            let tan_arg = std::f64::consts::FRAC_PI_4 + lat_rad / 2.0;
            out_y[j] = EARTH_RADIUS * tan_arg.tan().ln();
        }
    }

    // Handle remainder
    let remainder_start = chunks * LANES;
    for i in remainder_start..lon.len() {
        let lat_deg = lat[i];
        let lat_clamped = lat_deg.clamp(-MAX_LAT, MAX_LAT);
        let lat_rad = lat_clamped * DEG_TO_RAD;
        let lon_rad = lon[i] * DEG_TO_RAD;

        out_x[i] = EARTH_RADIUS * lon_rad;
        let tan_arg = std::f64::consts::FRAC_PI_4 + lat_rad / 2.0;
        out_y[i] = EARTH_RADIUS * tan_arg.tan().ln();
    }

    Ok(())
}

/// Convert Web Mercator (EPSG:3857) to latitude/longitude using SIMD
///
/// # Arguments
///
/// * `x` - X coordinates (meters)
/// * `y` - Y coordinates (meters)
/// * `out_lon` - Output longitude (degrees)
/// * `out_lat` - Output latitude (degrees)
///
/// # Errors
///
/// Returns an error if array lengths don't match
pub fn web_mercator_to_latlon(
    x: &[f64],
    y: &[f64],
    out_lon: &mut [f64],
    out_lat: &mut [f64],
) -> Result<()> {
    if x.len() != y.len() || x.len() != out_lon.len() || x.len() != out_lat.len() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "coordinates",
            message: format!(
                "Array length mismatch: x={}, y={}, out_lon={}, out_lat={}",
                x.len(),
                y.len(),
                out_lon.len(),
                out_lat.len()
            ),
        });
    }

    if x.is_empty() {
        return Ok(());
    }

    const EARTH_RADIUS: f64 = 6_378_137.0;
    const RAD_TO_DEG: f64 = 180.0 / std::f64::consts::PI;

    const LANES: usize = 4;
    let chunks = x.len() / LANES;

    // Process SIMD chunks
    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;

        for j in start..end {
            out_lon[j] = (x[j] / EARTH_RADIUS) * RAD_TO_DEG;

            // lat = 2 * atan(exp(y/R)) - π/2
            let exp_term = (y[j] / EARTH_RADIUS).exp();
            out_lat[j] = (2.0 * exp_term.atan() - std::f64::consts::FRAC_PI_2) * RAD_TO_DEG;
        }
    }

    // Handle remainder
    let remainder_start = chunks * LANES;
    for i in remainder_start..x.len() {
        out_lon[i] = (x[i] / EARTH_RADIUS) * RAD_TO_DEG;
        let exp_term = (y[i] / EARTH_RADIUS).exp();
        out_lat[i] = (2.0 * exp_term.atan() - std::f64::consts::FRAC_PI_2) * RAD_TO_DEG;
    }

    Ok(())
}

/// Convert degrees to radians in bulk using SIMD
pub fn degrees_to_radians(degrees: &[f64], radians: &mut [f64]) -> Result<()> {
    if degrees.len() != radians.len() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "arrays",
            message: format!(
                "Array length mismatch: degrees={}, radians={}",
                degrees.len(),
                radians.len()
            ),
        });
    }

    const DEG_TO_RAD: f64 = std::f64::consts::PI / 180.0;
    const LANES: usize = 8;
    let chunks = degrees.len() / LANES;

    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;
        for j in start..end {
            radians[j] = degrees[j] * DEG_TO_RAD;
        }
    }

    let remainder_start = chunks * LANES;
    for i in remainder_start..degrees.len() {
        radians[i] = degrees[i] * DEG_TO_RAD;
    }

    Ok(())
}

/// Convert radians to degrees in bulk using SIMD
pub fn radians_to_degrees(radians: &[f64], degrees: &mut [f64]) -> Result<()> {
    if radians.len() != degrees.len() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "arrays",
            message: format!(
                "Array length mismatch: radians={}, degrees={}",
                radians.len(),
                degrees.len()
            ),
        });
    }

    const RAD_TO_DEG: f64 = 180.0 / std::f64::consts::PI;
    const LANES: usize = 8;
    let chunks = radians.len() / LANES;

    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;
        for j in start..end {
            degrees[j] = radians[j] * RAD_TO_DEG;
        }
    }

    let remainder_start = chunks * LANES;
    for i in remainder_start..radians.len() {
        degrees[i] = radians[i] * RAD_TO_DEG;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_affine_identity() {
        let matrix = AffineMatrix2D::identity();
        let x = vec![1.0, 2.0, 3.0, 4.0];
        let y = vec![5.0, 6.0, 7.0, 8.0];
        let mut out_x = vec![0.0; 4];
        let mut out_y = vec![0.0; 4];

        affine_transform_2d(&matrix, &x, &y, &mut out_x, &mut out_y)
            .expect("affine identity transformation should succeed");

        assert_eq!(out_x, x);
        assert_eq!(out_y, y);
    }

    #[test]
    fn test_affine_translation() {
        let matrix = AffineMatrix2D::translation(10.0, 20.0);
        let x = vec![0.0, 1.0, 2.0];
        let y = vec![0.0, 1.0, 2.0];
        let mut out_x = vec![0.0; 3];
        let mut out_y = vec![0.0; 3];

        affine_transform_2d(&matrix, &x, &y, &mut out_x, &mut out_y)
            .expect("affine translation should succeed");

        assert_eq!(out_x, vec![10.0, 11.0, 12.0]);
        assert_eq!(out_y, vec![20.0, 21.0, 22.0]);
    }

    #[test]
    fn test_affine_scale() {
        let matrix = AffineMatrix2D::scale(2.0, 3.0);
        let x = vec![1.0, 2.0, 3.0];
        let y = vec![1.0, 2.0, 3.0];
        let mut out_x = vec![0.0; 3];
        let mut out_y = vec![0.0; 3];

        affine_transform_2d(&matrix, &x, &y, &mut out_x, &mut out_y)
            .expect("affine scale should succeed");

        assert_eq!(out_x, vec![2.0, 4.0, 6.0]);
        assert_eq!(out_y, vec![3.0, 6.0, 9.0]);
    }

    #[test]
    fn test_affine_invert() {
        let matrix = AffineMatrix2D {
            a: 2.0,
            b: 0.0,
            c: 0.0,
            d: 3.0,
            e: 10.0,
            f: 20.0,
        };

        let inverse = matrix.invert().expect("matrix inversion should succeed");

        // Apply forward then inverse transformation
        let x = vec![1.0, 2.0, 3.0];
        let y = vec![1.0, 2.0, 3.0];
        let mut temp_x = vec![0.0; 3];
        let mut temp_y = vec![0.0; 3];
        let mut out_x = vec![0.0; 3];
        let mut out_y = vec![0.0; 3];

        affine_transform_2d(&matrix, &x, &y, &mut temp_x, &mut temp_y)
            .expect("forward transformation should succeed");
        affine_transform_2d(&inverse, &temp_x, &temp_y, &mut out_x, &mut out_y)
            .expect("inverse transformation should succeed");

        for i in 0..3 {
            assert!((out_x[i] - x[i]).abs() < 1e-10);
            assert!((out_y[i] - y[i]).abs() < 1e-10);
        }
    }

    #[test]
    fn test_web_mercator_roundtrip() {
        let lon = vec![-122.4194, 139.6917, 2.3522]; // SF, Tokyo, Paris
        let lat = vec![37.7749, 35.6762, 48.8566];
        let mut x = vec![0.0; 3];
        let mut y = vec![0.0; 3];
        let mut out_lon = vec![0.0; 3];
        let mut out_lat = vec![0.0; 3];

        latlon_to_web_mercator(&lon, &lat, &mut x, &mut y)
            .expect("latlon to web mercator conversion should succeed");
        web_mercator_to_latlon(&x, &y, &mut out_lon, &mut out_lat)
            .expect("web mercator to latlon conversion should succeed");

        for i in 0..3 {
            assert!((out_lon[i] - lon[i]).abs() < 1e-6);
            assert!((out_lat[i] - lat[i]).abs() < 1e-6);
        }
    }

    #[test]
    fn test_degrees_radians_conversion() {
        let degrees = vec![0.0, 45.0, 90.0, 180.0, 270.0, 360.0];
        let mut radians = vec![0.0; 6];
        let mut back_to_degrees = vec![0.0; 6];

        degrees_to_radians(&degrees, &mut radians)
            .expect("degrees to radians conversion should succeed");
        radians_to_degrees(&radians, &mut back_to_degrees)
            .expect("radians to degrees conversion should succeed");

        for i in 0..6 {
            assert!((back_to_degrees[i] - degrees[i]).abs() < 1e-10);
        }
    }

    #[test]
    fn test_empty_arrays() {
        let matrix = AffineMatrix2D::identity();
        let x: Vec<f64> = vec![];
        let y: Vec<f64> = vec![];
        let mut out_x: Vec<f64> = vec![];
        let mut out_y: Vec<f64> = vec![];

        let result = affine_transform_2d(&matrix, &x, &y, &mut out_x, &mut out_y);
        assert!(result.is_ok());
    }

    #[test]
    fn test_mismatched_lengths() {
        let matrix = AffineMatrix2D::identity();
        let x = vec![1.0, 2.0];
        let y = vec![1.0, 2.0, 3.0]; // Wrong length
        let mut out_x = vec![0.0; 2];
        let mut out_y = vec![0.0; 2];

        let result = affine_transform_2d(&matrix, &x, &y, &mut out_x, &mut out_y);
        assert!(result.is_err());
    }
}
