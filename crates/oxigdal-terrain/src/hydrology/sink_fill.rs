//! Depression (sink) filling using priority-flood algorithm.

use crate::error::Result;
use num_traits::Float;
use scirs2_core::prelude::*;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Copy, Clone, PartialEq)]
struct Cell {
    y: usize,
    x: usize,
    elevation: f64,
}

impl Eq for Cell {}

impl PartialOrd for Cell {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Cell {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap_or(Ordering::Equal)
    }
}

/// Fill depressions in DEM using priority-flood algorithm.
pub fn fill_sinks<T>(dem: &Array2<T>, nodata: Option<T>) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    let (height, width) = dem.dim();
    let mut filled = Array2::from_elem((height, width), f64::INFINITY);
    let mut closed = Array2::from_elem((height, width), false);
    let mut open = BinaryHeap::new();

    // Initialize with edge cells
    for y in 0..height {
        for x in 0..width {
            if y == 0 || y == height - 1 || x == 0 || x == width - 1 {
                let val = dem[[y, x]];
                if let Some(nd) = nodata {
                    if (val - nd).abs() < T::epsilon() {
                        continue;
                    }
                }
                filled[[y, x]] = val.into();
                open.push(Cell {
                    y,
                    x,
                    elevation: val.into(),
                });
            }
        }
    }

    // Priority-flood
    while let Some(cell) = open.pop() {
        if closed[[cell.y, cell.x]] {
            continue;
        }
        closed[[cell.y, cell.x]] = true;

        for dy in -1..=1_isize {
            for dx in -1..=1_isize {
                if dy == 0 && dx == 0 {
                    continue;
                }
                let ny = (cell.y as isize + dy) as usize;
                let nx = (cell.x as isize + dx) as usize;

                if ny < height && nx < width && !closed[[ny, nx]] {
                    let neighbor_elev = dem[[ny, nx]].into();
                    let new_elev = neighbor_elev.max(cell.elevation);
                    if new_elev < filled[[ny, nx]] {
                        filled[[ny, nx]] = new_elev;
                        open.push(Cell {
                            y: ny,
                            x: nx,
                            elevation: new_elev,
                        });
                    }
                }
            }
        }
    }

    Ok(filled)
}
