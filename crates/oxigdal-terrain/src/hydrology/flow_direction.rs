//! Flow direction calculation for hydrological analysis.
//!
//! Implements D8 (8-direction) and D-Infinity algorithms for determining
//! flow direction from each cell based on elevation gradients.

use crate::error::{Result, TerrainError};
use num_traits::Float;
use scirs2_core::prelude::*;

/// Flow direction algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowAlgorithm {
    /// D8: 8 cardinal and diagonal directions
    D8,
    /// D-Infinity: continuous flow direction
    DInfinity,
}

/// D8 flow direction codes (ArcGIS convention)
/// 1=E, 2=SE, 4=S, 8=SW, 16=W, 32=NW, 64=N, 128=NE
pub(crate) const D8_DIRS: [(isize, isize, u8); 8] = [
    (0, 1, 1),    // East
    (1, 1, 2),    // Southeast
    (1, 0, 4),    // South
    (1, -1, 8),   // Southwest
    (0, -1, 16),  // West
    (-1, -1, 32), // Northwest
    (-1, 0, 64),  // North
    (-1, 1, 128), // Northeast
];

/// Calculate D8 flow direction.
///
/// Returns an array where each cell contains a power-of-2 value indicating
/// the direction of steepest descent.
pub fn flow_direction_d8<T>(
    dem: &Array2<T>,
    cell_size: f64,
    nodata: Option<T>,
) -> Result<Array2<u8>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, cell_size)?;

    let (height, width) = dem.dim();
    let mut flow_dir = Array2::zeros((height, width));

    // Cell size for diagonal directions
    let diag_size = cell_size * 2.0_f64.sqrt();

    for y in 0..height {
        for x in 0..width {
            let center = dem[[y, x]];

            if let Some(nd) = nodata {
                if is_nodata(center, nd) {
                    flow_dir[[y, x]] = 0;
                    continue;
                }
            }

            let center_val = center.into();
            let mut max_slope = f64::NEG_INFINITY;
            let mut max_dir = 0_u8;

            // Check all 8 neighbors
            for (dy, dx, dir_code) in &D8_DIRS {
                let ny = y as isize + dy;
                let nx = x as isize + dx;

                if ny >= 0 && ny < height as isize && nx >= 0 && nx < width as isize {
                    let neighbor = dem[[ny as usize, nx as usize]];

                    if let Some(nd) = nodata {
                        if is_nodata(neighbor, nd) {
                            continue;
                        }
                    }

                    let neighbor_val = neighbor.into();
                    let elevation_diff = center_val - neighbor_val;

                    // Calculate distance
                    let distance = if dy.abs() == 1 && dx.abs() == 1 {
                        diag_size
                    } else {
                        cell_size
                    };

                    let slope = elevation_diff / distance;

                    if slope > max_slope {
                        max_slope = slope;
                        max_dir = *dir_code;
                    }
                }
            }

            // If no downslope direction found, mark as sink
            flow_dir[[y, x]] = max_dir;
        }
    }

    Ok(flow_dir)
}

/// Calculate D-Infinity flow direction.
///
/// Returns an array where each cell contains a continuous direction angle
/// in radians (0 to 2π).
pub fn flow_direction_dinf<T>(
    dem: &Array2<T>,
    cell_size: f64,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, cell_size)?;

    let (height, width) = dem.dim();
    let mut flow_dir = Array2::zeros((height, width));

    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let center = dem[[y, x]];

            if let Some(nd) = nodata {
                if is_nodata(center, nd) {
                    flow_dir[[y, x]] = -1.0;
                    continue;
                }
            }

            let z = center.into();

            // Get 3x3 neighborhood
            let e0 = dem[[y, x + 1]].into();
            let e1 = dem[[y - 1, x + 1]].into();
            let e2 = dem[[y - 1, x]].into();
            let e3 = dem[[y - 1, x - 1]].into();
            let e4 = dem[[y, x - 1]].into();
            let e5 = dem[[y + 1, x - 1]].into();
            let e6 = dem[[y + 1, x]].into();
            let e7 = dem[[y + 1, x + 1]].into();

            // Calculate slopes in 8 facets
            let facets = [
                (e0, e1, 0.0),                               // Facet 0
                (e2, e1, std::f64::consts::FRAC_PI_4),       // Facet 1
                (e2, e3, std::f64::consts::FRAC_PI_2),       // Facet 2
                (e4, e3, 3.0 * std::f64::consts::FRAC_PI_4), // Facet 3
                (e4, e5, std::f64::consts::PI),              // Facet 4
                (e6, e5, 5.0 * std::f64::consts::FRAC_PI_4), // Facet 5
                (e6, e7, 3.0 * std::f64::consts::FRAC_PI_2), // Facet 6
                (e0, e7, 7.0 * std::f64::consts::FRAC_PI_4), // Facet 7
            ];

            let mut max_slope = f64::NEG_INFINITY;
            let mut flow_angle = 0.0;

            for (e1_val, e2_val, base_angle) in &facets {
                let s1 = (z - e1_val) / cell_size;
                let s2 = (z - e2_val) / (cell_size * 2.0_f64.sqrt());

                let r = (s1 * s1 + s2 * s2).sqrt();
                let angle = s2.atan2(s1);

                if r > max_slope {
                    max_slope = r;
                    flow_angle = base_angle + angle;
                }
            }

            // Normalize to 0-2π
            if flow_angle < 0.0 {
                flow_angle += 2.0 * std::f64::consts::PI;
            }
            if flow_angle >= 2.0 * std::f64::consts::PI {
                flow_angle -= 2.0 * std::f64::consts::PI;
            }

            flow_dir[[y, x]] = flow_angle;
        }
    }

    Ok(flow_dir)
}

/// Calculate flow direction with specified algorithm.
pub fn flow_direction<T>(
    dem: &Array2<T>,
    cell_size: f64,
    algorithm: FlowAlgorithm,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    match algorithm {
        FlowAlgorithm::D8 => {
            let d8 = flow_direction_d8(dem, cell_size, nodata)?;
            let (height, width) = d8.dim();
            let mut result = Array2::zeros((height, width));
            for y in 0..height {
                for x in 0..width {
                    result[[y, x]] = d8[[y, x]] as f64;
                }
            }
            Ok(result)
        }
        FlowAlgorithm::DInfinity => flow_direction_dinf(dem, cell_size, nodata),
    }
}

// Helper functions

fn validate_inputs<T>(dem: &Array2<T>, cell_size: f64) -> Result<()> {
    let (height, width) = dem.dim();

    if height < 3 || width < 3 {
        return Err(TerrainError::InvalidDimensions { width, height });
    }

    if cell_size <= 0.0 {
        return Err(TerrainError::InvalidCellSize { size: cell_size });
    }

    Ok(())
}

fn is_nodata<T: Float>(value: T, nodata: T) -> bool {
    if value.is_nan() && nodata.is_nan() {
        true
    } else {
        (value - nodata).abs() < T::epsilon()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_d8_simple_slope() {
        // Create simple east-facing slope
        let mut dem = Array2::zeros((5, 5));
        for y in 0..5 {
            for x in 0..5 {
                dem[[y, x]] = 100.0 - (x as f64) * 10.0; // Decreases eastward
            }
        }

        let flow_dir = flow_direction_d8(&dem, 10.0, None).expect("flow direction failed");

        // Most cells should flow east (code 1)
        for y in 1..4 {
            for x in 1..3 {
                assert_eq!(flow_dir[[y, x]], 1, "should flow east");
            }
        }
    }

    #[test]
    fn test_d8_directions() {
        // Create a pit at center
        let mut dem = Array2::from_elem((5, 5), 100.0);
        dem[[2, 2]] = 50.0; // Central pit

        let flow_dir = flow_direction_d8(&dem, 10.0, None).expect("flow direction failed");

        // Neighbors should flow toward center
        assert!(flow_dir[[2, 1]] > 0); // West neighbor
        assert!(flow_dir[[2, 3]] > 0); // East neighbor
        assert!(flow_dir[[1, 2]] > 0); // North neighbor
        assert!(flow_dir[[3, 2]] > 0); // South neighbor
    }

    #[test]
    fn test_dinf_continuous() {
        let mut dem = Array2::zeros((5, 5));
        for y in 0..5 {
            for x in 0..5 {
                dem[[y, x]] = 100.0 - (x as f64) * 10.0;
            }
        }

        let flow_dir = flow_direction_dinf(&dem, 10.0, None).expect("D-Infinity failed");

        // Flow directions should be continuous values
        for y in 1..4 {
            for x in 1..4 {
                assert!(flow_dir[[y, x]] >= 0.0 && flow_dir[[y, x]] < 2.0 * std::f64::consts::PI);
            }
        }
    }
}
