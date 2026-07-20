//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use numpy::{PyArray2, PyArrayMethods, PyUntypedArrayMethods};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple};

use super::types::ConvBoundary;

/// Converts a flat f64 slice into a `RasterBuffer` with given dimensions.
pub(super) fn slice_to_raster_buffer(data: &[f64], width: usize, height: usize) -> RasterBuffer {
    let mut buf = RasterBuffer::zeros(width as u64, height as u64, RasterDataType::Float64);
    for y in 0..height {
        for x in 0..width {
            let _ = buf.set_pixel(x as u64, y as u64, data[y * width + x]);
        }
    }
    buf
}

/// Converts a flat f64 slice into a `RasterBuffer` with nodata value set.
pub(super) fn slice_to_raster_buffer_with_nodata(
    data: &[f64],
    width: usize,
    height: usize,
    nodata_val: f64,
) -> RasterBuffer {
    use oxigdal_core::types::NoDataValue;
    let mut buf = RasterBuffer::nodata_filled(
        width as u64,
        height as u64,
        RasterDataType::Float64,
        NoDataValue::Float(nodata_val),
    );
    for y in 0..height {
        for x in 0..width {
            let _ = buf.set_pixel(x as u64, y as u64, data[y * width + x]);
        }
    }
    buf
}

/// Converts a `RasterBuffer` into a Vec<Vec<f64>> suitable for PyArray2.
pub(super) fn raster_buffer_to_vec2(buf: &RasterBuffer) -> Result<Vec<Vec<f64>>, String> {
    let width = buf.width() as usize;
    let height = buf.height() as usize;
    let mut result = Vec::with_capacity(height);
    for y in 0..height {
        let mut row = Vec::with_capacity(width);
        for x in 0..width {
            let val = buf
                .get_pixel(x as u64, y as u64)
                .map_err(|e| format!("Failed to read pixel ({}, {}): {}", x, y, e))?;
            row.push(val);
        }
        result.push(row);
    }
    Ok(result)
}

/// Reflects an out-of-range index back into `[0, n)` using half-sample
/// symmetric reflection. Well-defined for any `i` when `n >= 1`.
pub(super) fn reflect_index(i: i64, n: i64) -> i64 {
    if n == 1 {
        return 0;
    }
    let period = 2 * n;
    let mut m = i % period;
    if m < 0 {
        m += period;
    }
    if m >= n { period - 1 - m } else { m }
}

/// Wraps an out-of-range index into `[0, n)` (periodic boundary).
pub(super) fn wrap_index(i: i64, n: i64) -> i64 {
    let mut m = i % n;
    if m < 0 {
        m += n;
    }
    m
}

/// Performs a 2D correlation (kernel applied without flipping, matching
/// `oxigdal_algorithms::raster::focal_convolve`'s interior convention) with
/// explicit boundary handling for out-of-range samples.
///
/// All computed sample indices are provably within `[0, width) x [0, height)`
/// (via clamp/reflect/wrap), so direct indexing cannot panic; when `width` or
/// `height` is zero the loops never execute.
#[allow(clippy::too_many_arguments)]
pub(super) fn convolve_with_boundary(
    data: &[f64],
    width: usize,
    height: usize,
    kernel: &[f64],
    kw: usize,
    kh: usize,
    normalize: bool,
    boundary: ConvBoundary,
    fill_value: f64,
) -> Vec<f64> {
    let hw = (kw / 2) as i64;
    let hh = (kh / 2) as i64;

    let kernel_sum: f64 = if normalize {
        let sum: f64 = kernel.iter().sum();
        if sum.abs() < 1e-10 { 1.0 } else { sum }
    } else {
        1.0
    };

    let w = width as i64;
    let h = height as i64;
    let mut out = vec![0.0f64; width * height];

    for y in 0..h {
        for x in 0..w {
            let mut sum = 0.0;
            for ky in 0..kh as i64 {
                for kx in 0..kw as i64 {
                    let sx = x + (kx - hw);
                    let sy = y + (ky - hh);

                    let sample = if sx >= 0 && sx < w && sy >= 0 && sy < h {
                        data[sy as usize * width + sx as usize]
                    } else {
                        match boundary {
                            ConvBoundary::Constant => fill_value,
                            ConvBoundary::Nearest => {
                                let cx = sx.clamp(0, w - 1);
                                let cy = sy.clamp(0, h - 1);
                                data[cy as usize * width + cx as usize]
                            }
                            ConvBoundary::Reflect => {
                                let rx = reflect_index(sx, w);
                                let ry = reflect_index(sy, h);
                                data[ry as usize * width + rx as usize]
                            }
                            ConvBoundary::Wrap => {
                                let rx = wrap_index(sx, w);
                                let ry = wrap_index(sy, h);
                                data[ry as usize * width + rx as usize]
                            }
                        }
                    };

                    sum += sample * kernel[ky as usize * kw + kx as usize];
                }
            }
            out[y as usize * width + x as usize] = sum / kernel_sum;
        }
    }

    out
}

/// Calculates statistics for a raster array.
///
/// Args:
///     array (numpy.ndarray): Input array (2D)
///     nodata (float, optional): NoData value to exclude
///     compute_percentiles (bool): Compute percentiles (default: False)
///     percentiles (list, optional): Percentile values [25, 50, 75] (default)
///
/// Returns:
///     dict: Statistics dictionary with keys: min, max, mean, std, count, sum, median, etc.
///
/// Example:
///     >>> data = np.random.rand(512, 512)
///     >>> stats = oxigdal.statistics(data)
///     >>> print(f"Mean: {stats['mean']}, Std: {stats['std']}")
///     >>>
///     >>> # Compute with percentiles
///     >>> stats = oxigdal.statistics(data, compute_percentiles=True, percentiles=[10, 50, 90])
#[pyfunction]
#[pyo3(signature = (array, nodata=None, compute_percentiles=false, percentiles=None))]
pub fn statistics<'py>(
    py: Python<'py>,
    array: &Bound<'_, PyArray2<f64>>,
    nodata: Option<f64>,
    compute_percentiles: bool,
    percentiles: Option<Vec<f64>>,
) -> PyResult<Bound<'py, PyDict>> {
    let _shape = array.shape();
    let readonly = array.readonly();
    // Own the pixel data so the CPU-bound reduction can run with the GIL
    // released.
    let owned: Vec<f64> = readonly
        .as_slice()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Array must be contiguous"))?
        .to_vec();
    drop(readonly);

    /// Result of the statistics reduction, computed with the GIL released.
    struct StatsResult {
        min: f64,
        max: f64,
        mean: f64,
        std: f64,
        count: f64,
        sum: f64,
        variance: f64,
        median: Option<f64>,
        percentiles: Option<Vec<(i32, f64)>>,
    }

    let stats = py.detach(|| -> PyResult<StatsResult> {
        // Filter nodata values
        let mut valid_values: Vec<f64> = if let Some(nd) = nodata {
            owned
                .iter()
                .copied()
                .filter(|&v| (v - nd).abs() > 1e-10)
                .collect()
        } else {
            owned
        };

        if valid_values.is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "No valid values in array",
            ));
        }

        let count = valid_values.len() as f64;
        let min = valid_values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = valid_values
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        let sum: f64 = valid_values.iter().sum();
        let mean = sum / count;

        let variance: f64 = valid_values
            .iter()
            .map(|&v| (v - mean).powi(2))
            .sum::<f64>()
            / count;
        let std = variance.sqrt();

        let (median, percentiles) = if compute_percentiles {
            valid_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

            let percentile_values = percentiles.unwrap_or_else(|| vec![25.0, 50.0, 75.0]);
            let mut pairs: Vec<(i32, f64)> = Vec::with_capacity(percentile_values.len());

            for p in percentile_values {
                if !(0.0..=100.0).contains(&p) {
                    return Err(pyo3::exceptions::PyValueError::new_err(
                        "Percentiles must be between 0 and 100",
                    ));
                }
                let index = ((p / 100.0) * (count - 1.0)).round() as usize;
                let index = index.min(valid_values.len() - 1);
                pairs.push((p as i32, valid_values[index]));
            }

            let median_index = (count / 2.0).floor() as usize;
            let median = valid_values[median_index];
            (Some(median), Some(pairs))
        } else {
            (None, None)
        };

        Ok(StatsResult {
            min,
            max,
            mean,
            std,
            count,
            sum,
            variance,
            median,
            percentiles,
        })
    })?;

    // Create result dictionary
    let dict = PyDict::new(py);
    dict.set_item("min", stats.min)?;
    dict.set_item("max", stats.max)?;
    dict.set_item("mean", stats.mean)?;
    dict.set_item("std", stats.std)?;
    dict.set_item("count", stats.count)?;
    dict.set_item("sum", stats.sum)?;
    dict.set_item("variance", stats.variance)?;

    if let Some(median) = stats.median {
        dict.set_item("median", median)?;
    }
    if let Some(pairs) = stats.percentiles {
        let percentile_dict = PyDict::new(py);
        for (p, v) in pairs {
            percentile_dict.set_item(format!("p{}", p), v)?;
        }
        dict.set_item("percentiles", percentile_dict)?;
    }

    Ok(dict)
}

/// Computes histogram for a raster array.
///
/// Args:
///     array (numpy.ndarray): Input array (2D)
///     bins (int): Number of bins (default: 256)
///     range (tuple, optional): Value range as (min, max)
///     nodata (float, optional): NoData value to exclude
///
/// Returns:
///     tuple: (hist, bin_edges) where hist is counts and bin_edges are bin boundaries
///
/// Example:
///     >>> data = np.random.rand(512, 512)
///     >>> hist, bins = oxigdal.histogram(data, bins=100)
///     >>> print(f"Histogram shape: {len(hist)}")
#[pyfunction]
#[pyo3(signature = (array, bins=256, range=None, nodata=None))]
pub fn histogram<'py>(
    py: Python<'py>,
    array: &Bound<'_, PyArray2<f64>>,
    bins: usize,
    range: Option<(f64, f64)>,
    nodata: Option<f64>,
) -> PyResult<Bound<'py, PyTuple>> {
    if bins < 2 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Number of bins must be at least 2",
        ));
    }

    // Own the pixel data so the CPU-bound binning can run with the GIL
    // released.
    let readonly = array.readonly();
    let owned: Vec<f64> = readonly
        .as_slice()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Array must be contiguous"))?
        .to_vec();
    drop(readonly);

    let (hist, bin_edges) = py.detach(|| -> PyResult<(Vec<u64>, Vec<f64>)> {
        // Filter nodata values
        let valid_values: Vec<f64> = if let Some(nd) = nodata {
            owned
                .iter()
                .copied()
                .filter(|&v| (v - nd).abs() > 1e-10)
                .collect()
        } else {
            owned
        };

        if valid_values.is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "No valid values in array",
            ));
        }

        // Determine range
        let (min_val, max_val) = if let Some((min, max)) = range {
            (min, max)
        } else {
            let min = valid_values.iter().copied().fold(f64::INFINITY, f64::min);
            let max = valid_values
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);
            (min, max)
        };

        if max_val <= min_val {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Maximum must be greater than minimum",
            ));
        }

        // Create histogram
        let mut hist = vec![0_u64; bins];
        let bin_width = (max_val - min_val) / bins as f64;

        for &value in &valid_values {
            if value >= min_val && value <= max_val {
                let bin_index = ((value - min_val) / bin_width).floor() as usize;
                let bin_index = bin_index.min(bins - 1);
                hist[bin_index] += 1;
            }
        }

        // Create bin edges
        let bin_edges: Vec<f64> = (0..=bins).map(|i| min_val + i as f64 * bin_width).collect();

        Ok((hist, bin_edges))
    })?;

    let hist_list = PyList::new(py, hist)?;
    let edges_list = PyList::new(py, bin_edges)?;

    PyTuple::new(py, &[hist_list.into_any(), edges_list.into_any()])
}
