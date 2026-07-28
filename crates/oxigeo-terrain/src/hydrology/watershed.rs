//! Watershed delineation.

use crate::error::{Result, TerrainError};
use crate::hydrology::flow_direction::flow_direction_d8;
use num_traits::Float;
use scirs2_core::prelude::*;
use std::collections::VecDeque;

/// Delineate watershed from pour point.
pub fn watershed_from_point<T>(
    dem: &Array2<T>,
    cell_size: f64,
    pour_y: usize,
    pour_x: usize,
    nodata: Option<T>,
) -> Result<Array2<u8>>
where
    T: Float + Into<f64> + Copy,
{
    let (height, width) = dem.dim();

    // Validate the pour point against the DEM bounds before indexing so an
    // out-of-range pour point returns a typed error rather than panicking
    // inside ndarray's `IndexMut`.
    if pour_y >= height || pour_x >= width {
        return Err(TerrainError::WatershedError {
            message: format!(
                "pour point (y={pour_y}, x={pour_x}) is outside DEM bounds \
                 (height={height}, width={width})"
            ),
        });
    }

    let flow_dir = flow_direction_d8(dem, cell_size, nodata)?;
    let mut watershed = Array2::zeros((height, width));

    // Trace upstream from pour point
    let mut queue = VecDeque::new();
    queue.push_back((pour_y, pour_x));
    watershed[[pour_y, pour_x]] = 1;

    while let Some((y, x)) = queue.pop_front() {
        // Check all neighbors
        for dy in -1..=1_isize {
            for dx in -1..=1_isize {
                if dy == 0 && dx == 0 {
                    continue;
                }
                let ny = (y as isize + dy) as usize;
                let nx = (x as isize + dx) as usize;

                if ny < height && nx < width && watershed[[ny, nx]] == 0 {
                    // Check if this cell flows to (y, x)
                    if flows_to(&flow_dir, ny, nx, y, x) {
                        watershed[[ny, nx]] = 1;
                        queue.push_back((ny, nx));
                    }
                }
            }
        }
    }

    Ok(watershed)
}

fn flows_to(flow_dir: &Array2<u8>, from_y: usize, from_x: usize, to_y: usize, to_x: usize) -> bool {
    let dir = flow_dir[[from_y, from_x]];
    match dir {
        1 => from_y == to_y && from_x + 1 == to_x,     // E
        2 => from_y + 1 == to_y && from_x + 1 == to_x, // SE
        4 => from_y + 1 == to_y && from_x == to_x,     // S
        8 => from_y + 1 == to_y && from_x.wrapping_sub(1) == to_x, // SW
        16 => from_y == to_y && from_x.wrapping_sub(1) == to_x, // W
        32 => from_y.wrapping_sub(1) == to_y && from_x.wrapping_sub(1) == to_x, // NW
        64 => from_y.wrapping_sub(1) == to_y && from_x == to_x, // N
        128 => from_y.wrapping_sub(1) == to_y && from_x + 1 == to_x, // NE
        _ => false,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use scirs2_core::ndarray::Array2;

    /// A simple 3x3 DEM sloping down toward the south-east corner so flow
    /// converges to a single pour point.
    fn sloping_dem() -> Array2<f64> {
        Array2::from_shape_vec(
            (3, 3),
            vec![
                9.0, 8.0, 7.0, //
                8.0, 7.0, 6.0, //
                7.0, 6.0, 5.0, //
            ],
        )
        .expect("valid 3x3 DEM")
    }

    #[test]
    fn test_watershed_valid_pour_point() {
        let dem = sloping_dem();
        // Pour point at the low corner (2, 2).
        let result = watershed_from_point(&dem, 1.0, 2, 2, None);
        assert!(
            result.is_ok(),
            "valid pour point should succeed: {result:?}"
        );
        let watershed = result.expect("watershed");
        assert_eq!(watershed.dim(), (3, 3));
        // The pour point itself must always be marked as part of the watershed.
        assert_eq!(watershed[[2, 2]], 1, "pour point must be in the watershed");
        // At least one cell is included.
        assert!(watershed.iter().any(|&v| v == 1));
    }

    #[test]
    fn test_watershed_out_of_bounds_row() {
        let dem = sloping_dem();
        let result = watershed_from_point(&dem, 1.0, 3, 0, None);
        assert!(
            matches!(result, Err(TerrainError::WatershedError { .. })),
            "out-of-bounds row must return a typed WatershedError, got {result:?}"
        );
    }

    #[test]
    fn test_watershed_out_of_bounds_col() {
        let dem = sloping_dem();
        let result = watershed_from_point(&dem, 1.0, 0, 5, None);
        assert!(
            matches!(result, Err(TerrainError::WatershedError { .. })),
            "out-of-bounds column must return a typed WatershedError, got {result:?}"
        );
    }

    #[test]
    fn test_watershed_boundary_pour_point_ok() {
        let dem = sloping_dem();
        // Last valid indices (height-1, width-1) must not error.
        let result = watershed_from_point(&dem, 1.0, 2, 2, None);
        assert!(result.is_ok());
    }
}
