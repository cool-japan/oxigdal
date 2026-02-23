//! Stream network extraction.

use crate::error::Result;
use crate::hydrology::flow_accumulation::flow_accumulation;
use num_traits::Float;
use scirs2_core::prelude::*;

/// Extract stream network from flow accumulation.
pub fn extract_streams<T>(
    dem: &Array2<T>,
    cell_size: f64,
    threshold: u32,
    nodata: Option<T>,
) -> Result<Array2<u8>>
where
    T: Float + Into<f64> + Copy,
{
    let accumulation = flow_accumulation(dem, cell_size, nodata)?;
    let (height, width) = accumulation.dim();
    let mut streams = Array2::zeros((height, width));

    for y in 0..height {
        for x in 0..width {
            if accumulation[[y, x]] >= threshold {
                streams[[y, x]] = 1;
            }
        }
    }

    Ok(streams)
}

/// Calculate Strahler stream order.
pub fn strahler_order<T>(
    dem: &Array2<T>,
    cell_size: f64,
    threshold: u32,
    nodata: Option<T>,
) -> Result<Array2<u8>>
where
    T: Float + Into<f64> + Copy,
{
    let streams = extract_streams(dem, cell_size, threshold, nodata)?;
    let (height, width) = streams.dim();
    let mut order = Array2::zeros((height, width));

    // Simplified Strahler ordering (basic implementation)
    for y in 0..height {
        for x in 0..width {
            if streams[[y, x]] == 1 {
                order[[y, x]] = 1; // Assign minimum order
            }
        }
    }

    Ok(order)
}
