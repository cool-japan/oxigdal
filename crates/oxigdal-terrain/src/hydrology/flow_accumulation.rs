//! Flow accumulation calculation.

use crate::error::Result;
use crate::hydrology::flow_direction::{D8_DIRS, flow_direction_d8};
use num_traits::Float;
use scirs2_core::prelude::*;

/// Calculate flow accumulation from D8 flow direction.
pub fn flow_accumulation<T>(
    dem: &Array2<T>,
    cell_size: f64,
    nodata: Option<T>,
) -> Result<Array2<u32>>
where
    T: Float + Into<f64> + Copy,
{
    let (height, width) = dem.dim();
    let flow_dir = flow_direction_d8(dem, cell_size, nodata)?;
    let mut accumulation = Array2::zeros((height, width));

    // Count upstream cells for each cell
    for y in 0..height {
        for x in 0..width {
            accumulation[[y, x]] = 1; // Each cell contributes 1
        }
    }

    // Process cells from highest to lowest elevation
    let mut cells: Vec<(usize, usize, f64)> = Vec::new();
    for y in 0..height {
        for x in 0..width {
            if let Some(nd) = nodata {
                if (dem[[y, x]] - nd).abs() < T::epsilon() {
                    continue;
                }
            }
            cells.push((y, x, dem[[y, x]].into()));
        }
    }
    cells.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(core::cmp::Ordering::Equal));

    // Accumulate flow
    for (y, x, _) in cells {
        let dir = flow_dir[[y, x]];
        if dir == 0 {
            continue; // Sink or no data
        }

        // Find downstream cell
        if let Some((dy, dx, _)) = D8_DIRS.iter().find(|(_, _, code)| *code == dir) {
            let ny = (y as isize + dy) as usize;
            let nx = (x as isize + dx) as usize;
            if ny < height && nx < width {
                accumulation[[ny, nx]] += accumulation[[y, x]];
            }
        }
    }

    Ok(accumulation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flow_accumulation() {
        let mut dem = Array2::zeros((5, 5));
        for y in 0..5 {
            for x in 0..5 {
                dem[[y, x]] = 100.0 - (x as f64);
            }
        }

        let accum = flow_accumulation(&dem, 10.0, None).expect("failed");
        // Eastward flow should accumulate
        assert!(accum[[2, 4]] > accum[[2, 0]]);
    }
}
