//! Curvature calculation algorithms

use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

use super::{CurvatureType, slope_aspect::get_3x3_window};

pub fn compute_curvature(
    dem: &RasterBuffer,
    cell_size: f64,
    curvature_type: CurvatureType,
) -> Result<RasterBuffer> {
    let width = dem.width();
    let height = dem.height();
    let mut curvature = RasterBuffer::zeros(width, height, RasterDataType::Float64);

    #[cfg(feature = "parallel")]
    {
        let results: Result<Vec<_>> = (1..(height - 1))
            .into_par_iter()
            .map(|y| {
                let mut row_data = Vec::new();
                for x in 1..(width - 1) {
                    let value = compute_local_curvature(dem, x, y, cell_size, curvature_type)?;
                    row_data.push((x, value));
                }
                Ok((y, row_data))
            })
            .collect();

        for (y, row_data) in results? {
            for (x, value) in row_data {
                curvature
                    .set_pixel(x, y, value)
                    .map_err(AlgorithmError::Core)?;
            }
        }
    }

    #[cfg(not(feature = "parallel"))]
    {
        for y in 1..(height - 1) {
            for x in 1..(width - 1) {
                let value = compute_local_curvature(dem, x, y, cell_size, curvature_type)?;
                curvature
                    .set_pixel(x, y, value)
                    .map_err(AlgorithmError::Core)?;
            }
        }
    }

    Ok(curvature)
}

/// Computes curvature for a single pixel using 3x3 window
///
/// Uses the partial derivative formulation:
///   p = dz/dx, q = dz/dy
///   r = d2z/dx2, s = d2z/dxdy, t = d2z/dy2
fn compute_local_curvature(
    dem: &RasterBuffer,
    x: u64,
    y: u64,
    cell_size: f64,
    curvature_type: CurvatureType,
) -> Result<f64> {
    // Get 3x3 window values
    let z = get_3x3_window(dem, x, y)?;

    // Compute partial derivatives using finite differences
    let l_squared = cell_size * cell_size;

    // First derivatives (central differences)
    let p = (z[1][2] - z[1][0]) / (2.0 * cell_size);
    let q = (z[2][1] - z[0][1]) / (2.0 * cell_size);

    // Second derivatives
    let r = (z[1][2] - 2.0 * z[1][1] + z[1][0]) / l_squared;
    let s = (z[2][2] - z[2][0] - z[0][2] + z[0][0]) / (4.0 * l_squared);
    let t = (z[2][1] - 2.0 * z[1][1] + z[0][1]) / l_squared;

    let p2 = p * p;
    let q2 = q * q;
    let p2_q2 = p2 + q2;

    let result = match curvature_type {
        CurvatureType::Profile => {
            // Profile curvature: rate of change of slope in the direction of gradient
            // Kp = -(p^2*r + 2pqs + q^2*t) / ((p^2+q^2) * sqrt((1+p^2+q^2)^3))
            if p2_q2 < 1e-15 {
                0.0
            } else {
                let numerator = p2 * r + 2.0 * p * q * s + q2 * t;
                let denominator: f64 = p2_q2 * (1.0 + p2_q2).sqrt();
                -numerator / denominator
            }
        }
        CurvatureType::Planform => {
            // Planform curvature: curvature of contour line (flow convergence)
            // Kc = (q^2*r - 2pqs + p^2*t) / ((p^2+q^2)^(3/2))
            if p2_q2 < 1e-15 {
                0.0
            } else {
                let numerator = q2 * r - 2.0 * p * q * s + p2 * t;
                let denominator = p2_q2.powf(1.5);
                -numerator / denominator
            }
        }
        CurvatureType::Total => {
            // Total curvature (Laplacian): -(r + t)
            -(r + t)
        }
        CurvatureType::Mean => {
            // Mean curvature: average of principal curvatures
            // H = -((1+q^2)*r - 2pqs + (1+p^2)*t) / (2*(1+p^2+q^2)^(3/2))
            let denominator: f64 = 2.0 * (1.0 + p2_q2).powf(1.5);
            if denominator.abs() < 1e-15 {
                0.0
            } else {
                let numerator = (1.0 + q2) * r - 2.0 * p * q * s + (1.0 + p2) * t;
                -numerator / denominator
            }
        }
        CurvatureType::Gaussian => {
            // Gaussian curvature: product of principal curvatures
            // K = (r*t - s^2) / (1+p^2+q^2)^2
            let denominator: f64 = (1.0 + p2_q2).powi(2);
            if denominator.abs() < 1e-15 {
                0.0
            } else {
                (r * t - s * s) / denominator
            }
        }
        CurvatureType::Tangential => {
            // Tangential curvature: curvature of a normal section tangent to a contour
            // Kt = (q^2*r - 2pqs + p^2*t) / ((p^2+q^2) * sqrt(1+p^2+q^2))
            if p2_q2 < 1e-15 {
                0.0
            } else {
                let numerator = q2 * r - 2.0 * p * q * s + p2 * t;
                let denominator: f64 = p2_q2 * (1.0 + p2_q2).sqrt();
                -numerator / denominator
            }
        }
    };

    Ok(result)
}

// Note: Convergence index computation is implemented in the roughness.rs module.
// See `compute_convergence_index()` which measures flow convergence/divergence based
// on aspect directions of surrounding cells. Negative values indicate convergence (valleys),
// positive values indicate divergence (ridges).
// Uses the method of Koethe & Lehmeier (1996).
