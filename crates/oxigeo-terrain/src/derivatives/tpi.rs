//! Topographic Position Index (TPI) calculation.
//!
//! TPI compares the elevation of each cell to the mean elevation of a
//! neighborhood around it. Positive TPI indicates ridges, negative indicates
//! valleys, and near-zero indicates flat or mid-slope.

use crate::error::{Result, TerrainError};
use num_traits::Float;
use scirs2_core::prelude::*;

/// Calculate Topographic Position Index (TPI).
///
/// TPI = elevation - mean(neighborhood)
///
/// # Arguments
/// * `dem` - Input DEM as 2D array
/// * `radius` - Radius of neighborhood in cells
/// * `nodata` - Optional NoData value to skip
///
/// # Returns
/// 2D array of TPI values
pub fn tpi<T>(dem: &Array2<T>, radius: usize, nodata: Option<T>) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, radius)?;

    let (height, width) = dem.dim();
    let mut tpi_result = Array2::zeros((height, width));

    let diameter = 2 * radius + 1;

    for y in 0..height {
        for x in 0..width {
            let center = dem[[y, x]];

            if let Some(nd) = nodata
                && is_nodata(center, nd)
            {
                tpi_result[[y, x]] = f64::NAN;
                continue;
            }

            let mut sum = 0.0;
            let mut count = 0;

            // Calculate mean of neighborhood
            for dy in 0..diameter {
                for dx in 0..diameter {
                    let ny = y as isize + dy as isize - radius as isize;
                    let nx = x as isize + dx as isize - radius as isize;

                    if ny >= 0 && ny < height as isize && nx >= 0 && nx < width as isize {
                        let val = dem[[ny as usize, nx as usize]];

                        if let Some(nd) = nodata
                            && is_nodata(val, nd)
                        {
                            continue;
                        }

                        sum += val.into();
                        count += 1;
                    }
                }
            }

            if count > 0 {
                let mean = sum / (count as f64);
                tpi_result[[y, x]] = center.into() - mean;
            } else {
                tpi_result[[y, x]] = f64::NAN;
            }
        }
    }

    Ok(tpi_result)
}

/// Calculate TPI with optional parallelization.
#[cfg(feature = "parallel")]
pub fn tpi_parallel<T>(dem: &Array2<T>, radius: usize, nodata: Option<T>) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy + Send + Sync,
{
    use rayon::prelude::*;

    validate_inputs(dem, radius)?;

    let (height, width) = dem.dim();
    let diameter = 2 * radius + 1;

    let values: Vec<f64> = (0..height * width)
        .into_par_iter()
        .map(|idx| {
            let y = idx / width;
            let x = idx % width;
            let center = dem[[y, x]];

            if let Some(nd) = nodata
                && is_nodata(center, nd)
            {
                return f64::NAN;
            }

            let mut sum = 0.0;
            let mut count = 0;

            for dy in 0..diameter {
                for dx in 0..diameter {
                    let ny = y as isize + dy as isize - radius as isize;
                    let nx = x as isize + dx as isize - radius as isize;

                    if ny >= 0 && ny < height as isize && nx >= 0 && nx < width as isize {
                        let val = dem[[ny as usize, nx as usize]];

                        if let Some(nd) = nodata
                            && is_nodata(val, nd)
                        {
                            continue;
                        }

                        sum += val.into();
                        count += 1;
                    }
                }
            }

            if count > 0 {
                let mean = sum / (count as f64);
                center.into() - mean
            } else {
                f64::NAN
            }
        })
        .collect();

    Array2::from_shape_vec((height, width), values).map_err(|_| TerrainError::ComputationError {
        message: "Failed to create TPI array".to_string(),
    })
}

/// TPI computed over an annular neighbourhood (ring between `inner_r` and
/// `outer_r` cells, Chebyshev metric).
///
/// Only cells whose Chebyshev distance from the centre satisfies
/// `inner_r <= dist <= outer_r` contribute to the mean.  Setting `inner_r = 0`
/// reproduces the behaviour of the plain [`tpi`] function for the same
/// `outer_r`.
///
/// # Arguments
/// * `dem`     - Input DEM
/// * `inner_r` - Inner exclusion radius (cells closer than this are ignored)
/// * `outer_r` - Outer neighbourhood radius (must be > 0 and > inner_r)
/// * `nodata`  - Optional NoData value
pub fn tpi_annulus<T>(
    dem: &Array2<T>,
    inner_r: usize,
    outer_r: usize,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    if outer_r == 0 {
        return Err(TerrainError::InvalidRadius {
            radius: outer_r as f64,
        });
    }
    if inner_r > outer_r {
        return Err(TerrainError::InvalidRadius {
            radius: inner_r as f64,
        });
    }
    validate_inputs(dem, outer_r)?;

    let (height, width) = dem.dim();
    let mut result = Array2::zeros((height, width));

    let diameter = 2 * outer_r + 1;

    for y in 0..height {
        for x in 0..width {
            let center = dem[[y, x]];

            if let Some(nd) = nodata
                && is_nodata(center, nd)
            {
                result[[y, x]] = f64::NAN;
                continue;
            }

            let mut sum = 0.0_f64;
            let mut count = 0_usize;

            for dy in 0..diameter {
                for dx in 0..diameter {
                    let dr = dy as isize - outer_r as isize;
                    let dc = dx as isize - outer_r as isize;
                    let chebyshev = dr.unsigned_abs().max(dc.unsigned_abs());

                    // Annulus: must be within outer ring AND outside inner ring
                    if chebyshev < inner_r || chebyshev > outer_r {
                        continue;
                    }

                    let ny = y as isize + dr;
                    let nx = x as isize + dc;

                    if ny >= 0 && ny < height as isize && nx >= 0 && nx < width as isize {
                        let val = dem[[ny as usize, nx as usize]];

                        if let Some(nd) = nodata
                            && is_nodata(val, nd)
                        {
                            continue;
                        }

                        sum += val.into();
                        count += 1;
                    }
                }
            }

            result[[y, x]] = if count > 0 {
                center.into() - sum / count as f64
            } else {
                f64::NAN
            };
        }
    }

    Ok(result)
}

/// Standardized TPI: `(center − mean) / std_dev` of the neighbourhood window.
///
/// Uses the *population* standard deviation (divides by n, not n-1).
/// When the neighbourhood is flat (std_dev ≈ 0), returns 0.0.
///
/// # Arguments
/// * `dem`    - Input DEM
/// * `radius` - Neighbourhood radius (box window: 2*radius+1 × 2*radius+1)
/// * `nodata` - Optional NoData value
pub fn tpi_standardized<T>(dem: &Array2<T>, radius: usize, nodata: Option<T>) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, radius)?;

    let (height, width) = dem.dim();
    let mut result = Array2::zeros((height, width));
    let diameter = 2 * radius + 1;

    for y in 0..height {
        for x in 0..width {
            let center = dem[[y, x]];

            if let Some(nd) = nodata
                && is_nodata(center, nd)
            {
                result[[y, x]] = f64::NAN;
                continue;
            }

            // First pass: collect valid neighbourhood values
            let mut sum = 0.0_f64;
            let mut count = 0_usize;
            let mut values: Vec<f64> = Vec::with_capacity(diameter * diameter);

            for dy in 0..diameter {
                for dx in 0..diameter {
                    let ny = y as isize + dy as isize - radius as isize;
                    let nx = x as isize + dx as isize - radius as isize;

                    if ny >= 0 && ny < height as isize && nx >= 0 && nx < width as isize {
                        let val = dem[[ny as usize, nx as usize]];

                        if let Some(nd) = nodata
                            && is_nodata(val, nd)
                        {
                            continue;
                        }

                        let v: f64 = val.into();
                        sum += v;
                        count += 1;
                        values.push(v);
                    }
                }
            }

            if count == 0 {
                result[[y, x]] = f64::NAN;
                continue;
            }

            let mean = sum / count as f64;
            let variance =
                values.iter().map(|&v| (v - mean) * (v - mean)).sum::<f64>() / count as f64;
            let std_dev = variance.sqrt();

            result[[y, x]] = if std_dev < 1e-10 {
                0.0
            } else {
                (center.into() - mean) / std_dev
            };
        }
    }

    Ok(result)
}

/// Weiss (2001) 10-class landform classification derived from standardized TPI
/// at two neighbourhood scales.
///
/// Uses the decision table from Weiss (2001) with threshold = 1.0 standard
/// deviation and slope threshold = 5°.  Slope is computed internally using
/// Horn's method.
///
/// # Arguments
/// * `dem`       - Input DEM
/// * `small_r`   - Radius for the small-scale TPI neighbourhood
/// * `large_r`   - Radius for the large-scale TPI neighbourhood
/// * `cell_size` - Cell size in map units (for slope calculation)
/// * `nodata`    - Optional NoData value
///
/// # Returns
/// Array of class values in the range 1–10.
pub fn landform_classification_tpi<T>(
    dem: &Array2<T>,
    small_r: usize,
    large_r: usize,
    cell_size: f64,
    nodata: Option<T>,
) -> Result<Array2<u8>>
where
    T: Float + Into<f64> + Copy,
{
    if large_r <= small_r {
        return Err(TerrainError::InvalidRadius {
            radius: large_r as f64,
        });
    }
    validate_inputs(dem, large_r)?;
    if cell_size <= 0.0 {
        return Err(TerrainError::InvalidCellSize { size: cell_size });
    }

    let ts_arr = tpi_standardized(dem, small_r, nodata)?;
    let tl_arr = tpi_standardized(dem, large_r, nodata)?;

    let (height, width) = dem.dim();

    // Compute slope in radians using Horn's method (inline to avoid re-export
    // coupling — we only need radians here).
    let mut slope_rad = Array2::<f64>::zeros((height, width));
    for y in 0..height {
        for x in 0..width {
            let a: f64 = get_clamped_value(dem, y, x, -1, -1).into();
            let b: f64 = get_clamped_value(dem, y, x, -1, 0).into();
            let c: f64 = get_clamped_value(dem, y, x, -1, 1).into();
            let d: f64 = get_clamped_value(dem, y, x, 0, -1).into();
            let f: f64 = get_clamped_value(dem, y, x, 0, 1).into();
            let g: f64 = get_clamped_value(dem, y, x, 1, -1).into();
            let h: f64 = get_clamped_value(dem, y, x, 1, 0).into();
            let i: f64 = get_clamped_value(dem, y, x, 1, 1).into();
            let dzdx = ((c + 2.0 * f + i) - (a + 2.0 * d + g)) / (8.0 * cell_size);
            let dzdy = ((g + 2.0 * h + i) - (a + 2.0 * b + c)) / (8.0 * cell_size);
            slope_rad[[y, x]] = (dzdx * dzdx + dzdy * dzdy).sqrt().atan();
        }
    }

    const STD_THRESHOLD: f64 = 1.0;
    let slope_threshold: f64 = 5_f64.to_radians();

    let mut classes = Array2::<u8>::zeros((height, width));

    for y in 0..height {
        for x in 0..width {
            let ts = ts_arr[[y, x]];
            let tl = tl_arr[[y, x]];
            let s = slope_rad[[y, x]];

            // If any input is NaN (nodata region), assign class 0 (invalid)
            if ts.is_nan() || tl.is_nan() {
                classes[[y, x]] = 0;
                continue;
            }

            classes[[y, x]] = weiss_class(ts, tl, s, STD_THRESHOLD, slope_threshold);
        }
    }

    Ok(classes)
}

/// Evaluate the Weiss (2001) 10-class decision table.
///
/// Returns a class in 1..=10.  This is a pure function with no I/O, exposed
/// for testing and compositional use.
fn weiss_class(ts: f64, tl: f64, s: f64, t: f64, slope_t: f64) -> u8 {
    if ts <= -t && tl <= -t {
        1 // canyons / deeply incised streams
    } else if ts <= -t && tl > -t && tl < t {
        2 // midslope drainages / shallow valleys
    } else if ts <= -t && tl >= t {
        3 // upland drainages / headwaters
    } else if ts > -t && ts < t && tl <= -t && s >= slope_t {
        4 // U-shaped valleys
    } else if ts > -t && ts < t && tl <= -t && s < slope_t {
        5 // plains / open slopes (low)
    } else if ts > -t && ts < t && tl > -t && tl < t && s < slope_t {
        6 // flat / gentle plains
    } else if ts > -t && ts < t && tl > -t && tl < t && s >= slope_t {
        7 // open slopes
    } else if ts > -t && ts < t && tl >= t && s >= slope_t {
        8 // upper slopes / mesas
    } else if ts >= t && tl <= -t {
        9 // local ridges / hills in valleys
    } else {
        // ts >= t && (tl > -t)
        10 // midslope ridges, small hills in plains
    }
}

/// Read a DEM value at (y + dy, x + dx), clamping to valid bounds.
fn get_clamped_value<T: Copy>(dem: &Array2<T>, y: usize, x: usize, dy: isize, dx: isize) -> T {
    let (height, width) = dem.dim();
    let ny = (y as isize + dy).clamp(0, height as isize - 1) as usize;
    let nx = (x as isize + dx).clamp(0, width as isize - 1) as usize;
    dem[[ny, nx]]
}

/// Annular-TPI with Rayon parallelism.
#[cfg(feature = "parallel")]
pub fn tpi_annulus_parallel<T>(
    dem: &Array2<T>,
    inner_r: usize,
    outer_r: usize,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy + Send + Sync,
{
    use rayon::prelude::*;

    if outer_r == 0 {
        return Err(TerrainError::InvalidRadius {
            radius: outer_r as f64,
        });
    }
    if inner_r > outer_r {
        return Err(TerrainError::InvalidRadius {
            radius: inner_r as f64,
        });
    }
    validate_inputs(dem, outer_r)?;

    let (height, width) = dem.dim();
    let diameter = 2 * outer_r + 1;

    let values: Vec<f64> = (0..height * width)
        .into_par_iter()
        .map(|idx| {
            let y = idx / width;
            let x = idx % width;
            let center = dem[[y, x]];

            if let Some(nd) = nodata
                && is_nodata(center, nd)
            {
                return f64::NAN;
            }

            let mut sum = 0.0_f64;
            let mut count = 0_usize;

            for dy in 0..diameter {
                for dx in 0..diameter {
                    let dr = dy as isize - outer_r as isize;
                    let dc = dx as isize - outer_r as isize;
                    let chebyshev = dr.unsigned_abs().max(dc.unsigned_abs());

                    if chebyshev < inner_r || chebyshev > outer_r {
                        continue;
                    }

                    let ny = y as isize + dr;
                    let nx = x as isize + dc;

                    if ny >= 0 && ny < height as isize && nx >= 0 && nx < width as isize {
                        let val = dem[[ny as usize, nx as usize]];

                        if let Some(nd) = nodata
                            && is_nodata(val, nd)
                        {
                            continue;
                        }

                        sum += val.into();
                        count += 1;
                    }
                }
            }

            if count > 0 {
                center.into() - sum / count as f64
            } else {
                f64::NAN
            }
        })
        .collect();

    Array2::from_shape_vec((height, width), values).map_err(|_| TerrainError::ComputationError {
        message: "Failed to create tpi_annulus array".to_string(),
    })
}

/// Standardized-TPI with Rayon parallelism.
#[cfg(feature = "parallel")]
pub fn tpi_standardized_parallel<T>(
    dem: &Array2<T>,
    radius: usize,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy + Send + Sync,
{
    use rayon::prelude::*;

    validate_inputs(dem, radius)?;

    let (height, width) = dem.dim();
    let diameter = 2 * radius + 1;

    let values: Vec<f64> = (0..height * width)
        .into_par_iter()
        .map(|idx| {
            let y = idx / width;
            let x = idx % width;
            let center = dem[[y, x]];

            if let Some(nd) = nodata
                && is_nodata(center, nd)
            {
                return f64::NAN;
            }

            let mut sum = 0.0_f64;
            let mut count = 0_usize;
            let mut sq_sum = 0.0_f64;

            for dy in 0..diameter {
                for dx in 0..diameter {
                    let ny = y as isize + dy as isize - radius as isize;
                    let nx = x as isize + dx as isize - radius as isize;

                    if ny >= 0 && ny < height as isize && nx >= 0 && nx < width as isize {
                        let val = dem[[ny as usize, nx as usize]];

                        if let Some(nd) = nodata
                            && is_nodata(val, nd)
                        {
                            continue;
                        }

                        let v: f64 = val.into();
                        sum += v;
                        sq_sum += v * v;
                        count += 1;
                    }
                }
            }

            if count == 0 {
                return f64::NAN;
            }

            let mean = sum / count as f64;
            // population variance: E[x²] - (E[x])²
            let variance = (sq_sum / count as f64) - mean * mean;
            let std_dev = variance.max(0.0).sqrt();

            if std_dev < 1e-10 {
                0.0
            } else {
                (center.into() - mean) / std_dev
            }
        })
        .collect();

    Array2::from_shape_vec((height, width), values).map_err(|_| TerrainError::ComputationError {
        message: "Failed to create tpi_standardized array".to_string(),
    })
}

// Helper functions

fn validate_inputs<T>(dem: &Array2<T>, radius: usize) -> Result<()> {
    let (height, width) = dem.dim();

    if height < 3 || width < 3 {
        return Err(TerrainError::InvalidDimensions { width, height });
    }

    if radius == 0 {
        return Err(TerrainError::InvalidRadius {
            radius: radius as f64,
        });
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
    use approx::assert_relative_eq;

    #[test]
    fn test_tpi_flat() {
        let dem = Array2::from_elem((10, 10), 100.0_f64);
        let result = tpi(&dem, 1, None).expect("TPI calculation failed");

        // Flat surface should have TPI of 0
        for &val in result.iter() {
            assert_relative_eq!(val, 0.0, epsilon = 1e-10);
        }
    }

    #[test]
    fn test_tpi_ridge() {
        // Create a ridge (high center)
        let mut dem = Array2::from_elem((5, 5), 100.0_f64);
        dem[[2, 2]] = 150.0; // Center is elevated

        let result = tpi(&dem, 1, None).expect("TPI calculation failed");

        // Center should have positive TPI
        assert!(result[[2, 2]] > 0.0, "ridge should have positive TPI");
    }

    #[test]
    fn test_tpi_valley() {
        // Create a valley (low center)
        let mut dem = Array2::from_elem((5, 5), 100.0_f64);
        dem[[2, 2]] = 50.0; // Center is depressed

        let result = tpi(&dem, 1, None).expect("TPI calculation failed");

        // Center should have negative TPI
        assert!(result[[2, 2]] < 0.0, "valley should have negative TPI");
    }

    #[test]
    fn test_tpi_radius() {
        let mut dem = Array2::from_elem((10, 10), 100.0_f64);
        dem[[5, 5]] = 150.0;

        let tpi1 = tpi(&dem, 1, None).expect("failed");
        let tpi2 = tpi(&dem, 2, None).expect("failed");

        // Different radius should give different results
        assert_ne!(tpi1[[5, 5]], tpi2[[5, 5]]);
    }

    #[test]
    fn test_invalid_radius() {
        let dem = Array2::from_elem((10, 10), 100.0_f64);
        let result = tpi(&dem, 0, None);
        assert!(result.is_err());
    }

    #[cfg(feature = "parallel")]
    #[test]
    fn test_tpi_parallel() {
        let mut dem = Array2::from_elem((20, 20), 100.0_f64);
        dem[[10, 10]] = 150.0;

        let result_seq = tpi(&dem, 2, None).expect("sequential TPI failed");
        let result_par = tpi_parallel(&dem, 2, None).expect("parallel TPI failed");

        // Results should be identical
        for y in 0..20 {
            for x in 0..20 {
                assert_relative_eq!(result_seq[[y, x]], result_par[[y, x]], epsilon = 1e-10);
            }
        }
    }

    // ── tpi_annulus ─────────────────────────────────────────────────────────

    #[test]
    fn test_tpi_annulus_vs_tpi_inner_zero() {
        // With inner_r = 0 the annulus should include the same cells as tpi
        let mut dem = Array2::from_elem((15, 15), 100.0_f64);
        dem[[7, 7]] = 150.0;

        let tpi_box = tpi(&dem, 2, None).expect("tpi failed");
        let tpi_ann = tpi_annulus(&dem, 0, 2, None).expect("tpi_annulus failed");

        for y in 0..15 {
            for x in 0..15 {
                assert_relative_eq!(tpi_box[[y, x]], tpi_ann[[y, x]], epsilon = 1e-10);
            }
        }
    }

    #[test]
    fn test_tpi_annulus_excludes_inner_ring() {
        // Flat DEM except the very centre; annulus that excludes centre cell
        // differences should not see the spike at all.
        let mut dem = Array2::from_elem((15, 15), 100.0_f64);
        dem[[7, 7]] = 200.0; // large spike at centre

        // inner_r = 1 excludes the 3×3 inner box (including the spike itself as
        // a neighbour when evaluated from nearby cells).
        let tpi_ann = tpi_annulus(&dem, 1, 3, None).expect("tpi_annulus failed");
        let tpi_box = tpi(&dem, 3, None).expect("tpi failed");

        // The two results must differ at (7, 7) because annulus excludes inner
        // cells from the neighbourhood mean.
        // For tpi_box the mean includes the spike; for annulus it does not
        // (the inner 3×3 ring is excluded from the average).
        let box_val = tpi_box[[7, 7]];
        let ann_val = tpi_ann[[7, 7]];
        // box TPI includes itself in mean → mean is pulled up → TPI is smaller
        // ann TPI excludes inner ring (only outer ring is flat 100) → TPI is larger
        assert!(
            (ann_val - box_val).abs() > 1e-6,
            "annulus TPI ({ann_val}) should differ from box TPI ({box_val}) when inner ring is excluded"
        );
    }

    // ── tpi_standardized ────────────────────────────────────────────────────

    #[test]
    fn test_tpi_standardized_hilltop() {
        // A raised centre on a flat background → centre has positive z-score
        let mut dem = Array2::from_elem((11, 11), 100.0_f64);
        dem[[5, 5]] = 200.0;

        let result = tpi_standardized(&dem, 2, None).expect("tpi_standardized failed");
        assert!(
            result[[5, 5]] > 0.0,
            "hilltop centre must have positive standardized TPI, got {}",
            result[[5, 5]]
        );
    }

    #[test]
    fn test_tpi_standardized_flat() {
        // Perfectly flat DEM → std_dev = 0 → all values must be 0.0
        let dem = Array2::from_elem((10, 10), 100.0_f64);
        let result = tpi_standardized(&dem, 2, None).expect("tpi_standardized failed");

        for &v in result.iter() {
            assert_relative_eq!(v, 0.0, epsilon = 1e-10);
        }
    }

    // ── landform_classification_tpi ─────────────────────────────────────────

    #[test]
    fn test_weiss_classification_valley() {
        // Build a synthetic valley: low trough running through the middle,
        // surrounded by higher terrain.
        let size = 25_usize;
        let mut dem = Array2::from_elem((size, size), 200.0_f64);
        // Depress the centre row to form a valley
        for x in 0..size {
            dem[[12, x]] = 50.0;
            dem[[11, x]] = 80.0;
            dem[[13, x]] = 80.0;
        }

        let classes = landform_classification_tpi(&dem, 2, 5, 30.0, None)
            .expect("landform classification failed");

        // The very bottom of the valley (centre row, away from edges) should be
        // class 1 (deeply incised) or 2 (shallow valley)
        let c = classes[[12, 12]];
        assert!(
            c == 1 || c == 2,
            "valley bottom expected class 1 or 2, got {c}"
        );
    }

    #[test]
    fn test_weiss_classification_ridge() {
        // Build a synthetic ridge: elevated spine down the middle
        let size = 25_usize;
        let mut dem = Array2::from_elem((size, size), 50.0_f64);
        for x in 0..size {
            dem[[12, x]] = 200.0;
            dem[[11, x]] = 150.0;
            dem[[13, x]] = 150.0;
        }

        let classes = landform_classification_tpi(&dem, 2, 5, 30.0, None)
            .expect("landform classification failed");

        let c = classes[[12, 12]];
        assert!(
            c == 9 || c == 10,
            "ridge spine expected class 9 or 10, got {c}"
        );
    }

    #[test]
    fn test_weiss_output_range() {
        // All non-zero output values must be in [1, 10]
        let mut dem = Array2::zeros((20, 20));
        for y in 0..20_usize {
            for x in 0..20_usize {
                dem[[y, x]] = (y as f64 * 7.3) + (x as f64 * 3.1);
            }
        }

        let classes = landform_classification_tpi(&dem, 1, 3, 10.0, None)
            .expect("landform classification failed");

        for &c in classes.iter() {
            assert!((1..=10).contains(&c) || c == 0, "class out of range: {c}");
        }
    }

    #[cfg(feature = "parallel")]
    #[test]
    fn test_tpi_annulus_parallel_matches_sequential() {
        let mut dem = Array2::from_elem((20, 20), 100.0_f64);
        dem[[10, 10]] = 150.0;

        let seq = tpi_annulus(&dem, 1, 3, None).expect("sequential failed");
        let par = tpi_annulus_parallel(&dem, 1, 3, None).expect("parallel failed");

        for y in 0..20 {
            for x in 0..20 {
                assert_relative_eq!(seq[[y, x]], par[[y, x]], epsilon = 1e-10);
            }
        }
    }

    #[cfg(feature = "parallel")]
    #[test]
    fn test_tpi_standardized_parallel_matches_sequential() {
        let mut dem = Array2::from_elem((20, 20), 100.0_f64);
        dem[[10, 10]] = 150.0;

        let seq = tpi_standardized(&dem, 2, None).expect("sequential failed");
        let par = tpi_standardized_parallel(&dem, 2, None).expect("parallel failed");

        for y in 0..20 {
            for x in 0..20 {
                assert_relative_eq!(seq[[y, x]], par[[y, x]], epsilon = 1e-3);
            }
        }
    }
}
