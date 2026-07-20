//! Watershed delineation.

use crate::error::Result;
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
