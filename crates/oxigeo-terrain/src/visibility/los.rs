//! Line of sight analysis.

use crate::error::Result;
use num_traits::Float;
use scirs2_core::prelude::*;

/// Check line of sight between two points.
pub fn line_of_sight<T>(
    dem: &Array2<T>,
    from_y: usize,
    from_x: usize,
    to_y: usize,
    to_x: usize,
    from_height: f64,
    to_height: f64,
) -> Result<bool>
where
    T: Float + Into<f64> + Copy,
{
    let from_elev = dem[[from_y, from_x]].into() + from_height;
    let to_elev = dem[[to_y, to_x]].into() + to_height;

    let dy = to_y as isize - from_y as isize;
    let dx = to_x as isize - from_x as isize;
    let steps = dy.abs().max(dx.abs());

    for i in 1..steps {
        let t = i as f64 / steps as f64;
        let y = (from_y as f64 + dy as f64 * t).round() as usize;
        let x = (from_x as f64 + dx as f64 * t).round() as usize;

        let interp_elev = from_elev + (to_elev - from_elev) * t;
        let terrain_elev = dem[[y, x]].into();

        if terrain_elev > interp_elev {
            return Ok(false);
        }
    }

    Ok(true)
}
