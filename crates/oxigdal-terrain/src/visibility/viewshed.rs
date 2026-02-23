//! Viewshed analysis.

use crate::error::{Result, TerrainError};
use num_traits::Float;
use scirs2_core::prelude::*;

/// Calculate binary viewshed (visible/not visible).
#[allow(clippy::too_many_arguments)]
pub fn viewshed_binary<T>(
    dem: &Array2<T>,
    cell_size: f64,
    observer_y: usize,
    observer_x: usize,
    observer_height: f64,
    target_height: f64,
    max_distance: Option<f64>,
    nodata: Option<T>,
) -> Result<Array2<u8>>
where
    T: Float + Into<f64> + Copy,
{
    let (height, width) = dem.dim();

    if observer_y >= height || observer_x >= width {
        return Err(TerrainError::InvalidObserverPosition {
            x: observer_x,
            y: observer_y,
        });
    }

    if observer_height < 0.0 {
        return Err(TerrainError::InvalidObserverHeight {
            height: observer_height,
        });
    }

    let mut viewshed = Array2::zeros((height, width));
    let observer_elev = dem[[observer_y, observer_x]].into() + observer_height;

    for y in 0..height {
        for x in 0..width {
            if y == observer_y && x == observer_x {
                viewshed[[y, x]] = 1;
                continue;
            }

            let target_val = dem[[y, x]];
            if let Some(nd) = nodata {
                if (target_val - nd).abs() < T::epsilon() {
                    continue;
                }
            }

            // Check distance
            let dy = (y as isize - observer_y as isize) as f64;
            let dx = (x as isize - observer_x as isize) as f64;
            let distance =
                (dy * dy * cell_size * cell_size + dx * dx * cell_size * cell_size).sqrt();

            if let Some(max_dist) = max_distance {
                if distance > max_dist {
                    continue;
                }
            }

            // Line of sight check
            if is_visible(
                dem,
                observer_y,
                observer_x,
                y,
                x,
                observer_elev,
                target_height,
            ) {
                viewshed[[y, x]] = 1;
            }
        }
    }

    Ok(viewshed)
}

fn is_visible<T: Float + Into<f64> + Copy>(
    dem: &Array2<T>,
    obs_y: usize,
    obs_x: usize,
    tgt_y: usize,
    tgt_x: usize,
    obs_elev: f64,
    tgt_height: f64,
) -> bool {
    let tgt_elev = dem[[tgt_y, tgt_x]].into() + tgt_height;
    let dy = tgt_y as isize - obs_y as isize;
    let dx = tgt_x as isize - obs_x as isize;
    let steps = dy.abs().max(dx.abs());

    for i in 1..steps {
        let t = i as f64 / steps as f64;
        let y = (obs_y as f64 + dy as f64 * t).round() as usize;
        let x = (obs_x as f64 + dx as f64 * t).round() as usize;

        let interp_elev = obs_elev + (tgt_elev - obs_elev) * t;
        let terrain_elev = dem[[y, x]].into();

        if terrain_elev > interp_elev {
            return false;
        }
    }

    true
}

/// Calculate cumulative viewshed from multiple observer points.
pub fn viewshed_cumulative<T>(
    dem: &Array2<T>,
    cell_size: f64,
    observers: &[(usize, usize, f64)], // (y, x, height)
    target_height: f64,
    max_distance: Option<f64>,
    nodata: Option<T>,
) -> Result<Array2<u32>>
where
    T: Float + Into<f64> + Copy,
{
    let (height, width) = dem.dim();
    let mut cumulative = Array2::zeros((height, width));

    for (obs_y, obs_x, obs_height) in observers {
        let viewshed = viewshed_binary(
            dem,
            cell_size,
            *obs_y,
            *obs_x,
            *obs_height,
            target_height,
            max_distance,
            nodata,
        )?;

        for y in 0..height {
            for x in 0..width {
                cumulative[[y, x]] += viewshed[[y, x]] as u32;
            }
        }
    }

    Ok(cumulative)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewshed_flat() {
        let dem = Array2::from_elem((10, 10), 100.0_f64);
        let vs = viewshed_binary(&dem, 10.0, 5, 5, 2.0, 0.0, None, None).expect("failed");

        // On flat terrain, all cells should be visible
        for y in 0..10 {
            for x in 0..10 {
                assert_eq!(vs[[y, x]], 1);
            }
        }
    }
}
