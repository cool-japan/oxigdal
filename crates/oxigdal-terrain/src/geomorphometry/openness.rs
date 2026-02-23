//! Topographic openness calculation.

use crate::error::Result;
use num_traits::Float;
use scirs2_core::prelude::*;

/// Calculate positive openness (view from above).
pub fn positive_openness<T>(
    dem: &Array2<T>,
    cell_size: f64,
    radius: usize,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    let (height, width) = dem.dim();
    let mut openness = Array2::zeros((height, width));

    for y in 0..height {
        for x in 0..width {
            let center = dem[[y, x]];
            if let Some(nd) = nodata {
                if (center - nd).abs() < T::epsilon() {
                    continue;
                }
            }

            let mut angle_sum = 0.0;
            let n_dirs = 8;

            // Calculate zenith angles in 8 directions
            for dir in 0..n_dirs {
                let angle = (dir as f64) * std::f64::consts::PI / (n_dirs as f64);
                let dy = (angle.sin() * radius as f64).round() as isize;
                let dx = (angle.cos() * radius as f64).round() as isize;

                let ny = (y as isize + dy) as usize;
                let nx = (x as isize + dx) as usize;

                if ny < height && nx < width {
                    let neighbor = dem[[ny, nx]];
                    let dz = neighbor.into() - center.into();
                    let dist = ((dy * dy + dx * dx) as f64).sqrt() * cell_size;
                    let zenith = (dz / dist).atan();
                    angle_sum += 90.0 - zenith.to_degrees();
                }
            }

            openness[[y, x]] = angle_sum / n_dirs as f64;
        }
    }

    Ok(openness)
}

/// Calculate negative openness (view from below).
pub fn negative_openness<T>(
    dem: &Array2<T>,
    cell_size: f64,
    radius: usize,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    let (height, width) = dem.dim();
    let mut openness = Array2::zeros((height, width));

    for y in 0..height {
        for x in 0..width {
            let center = dem[[y, x]];
            if let Some(nd) = nodata {
                if (center - nd).abs() < T::epsilon() {
                    continue;
                }
            }

            let mut angle_sum = 0.0;
            let n_dirs = 8;

            for dir in 0..n_dirs {
                let angle = (dir as f64) * std::f64::consts::PI / (n_dirs as f64);
                let dy = (angle.sin() * radius as f64).round() as isize;
                let dx = (angle.cos() * radius as f64).round() as isize;

                let ny = (y as isize + dy) as usize;
                let nx = (x as isize + dx) as usize;

                if ny < height && nx < width {
                    let neighbor = dem[[ny, nx]];
                    let dz = center.into() - neighbor.into(); // Reversed
                    let dist = ((dy * dy + dx * dx) as f64).sqrt() * cell_size;
                    let nadir = (dz / dist).atan();
                    angle_sum += 90.0 - nadir.to_degrees();
                }
            }

            openness[[y, x]] = angle_sum / n_dirs as f64;
        }
    }

    Ok(openness)
}
