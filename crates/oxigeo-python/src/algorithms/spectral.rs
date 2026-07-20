//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use numpy::{PyArray2, PyArrayMethods, PyUntypedArrayMethods};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use super::filters::{dilation, erosion};
use super::types::EviConfig;

/// Applies morphological opening (erosion followed by dilation).
///
/// Args:
///     array (numpy.ndarray): Input binary array (2D)
///     kernel (numpy.ndarray, optional): Structuring element (default: 3x3 square)
///     iterations (int): Number of iterations (default: 1)
///
/// Returns:
///     numpy.ndarray: Opened array
///
/// Example:
///     >>> # Remove small noise
///     >>> cleaned = oxigeo.opening(binary_mask.astype(float))
#[pyfunction]
#[pyo3(signature = (array, kernel=None, iterations=1))]
pub fn opening<'py>(
    py: Python<'py>,
    array: &Bound<'_, PyArray2<f64>>,
    kernel: Option<&Bound<'_, PyArray2<f64>>>,
    iterations: usize,
) -> PyResult<Bound<'py, PyArray2<f64>>> {
    // Opening = Erosion followed by Dilation
    let eroded = erosion(py, array, kernel, iterations)?;
    dilation(py, &eroded, kernel, iterations)
}

/// Applies morphological closing (dilation followed by erosion).
///
/// Args:
///     array (numpy.ndarray): Input binary array (2D)
///     kernel (numpy.ndarray, optional): Structuring element (default: 3x3 square)
///     iterations (int): Number of iterations (default: 1)
///
/// Returns:
///     numpy.ndarray: Closed array
///
/// Example:
///     >>> # Fill small holes
///     >>> filled = oxigeo.closing(binary_mask.astype(float))
#[pyfunction]
#[pyo3(signature = (array, kernel=None, iterations=1))]
pub fn closing<'py>(
    py: Python<'py>,
    array: &Bound<'_, PyArray2<f64>>,
    kernel: Option<&Bound<'_, PyArray2<f64>>>,
    iterations: usize,
) -> PyResult<Bound<'py, PyArray2<f64>>> {
    // Closing = Dilation followed by Erosion
    let dilated = dilation(py, array, kernel, iterations)?;
    erosion(py, &dilated, kernel, iterations)
}

/// Calculates NDVI (Normalized Difference Vegetation Index).
///
/// Args:
///     nir (numpy.ndarray): Near-infrared band (2D)
///     red (numpy.ndarray): Red band (2D)
///     nodata (float, optional): NoData value
///
/// Returns:
///     numpy.ndarray: NDVI values ranging from -1 to 1
///
/// Example:
///     >>> ndvi = oxigeo.ndvi(band4, band3)
///     >>> vegetation_mask = ndvi > 0.3
#[pyfunction]
#[pyo3(signature = (nir, red, nodata=None))]
pub fn ndvi<'py>(
    py: Python<'py>,
    nir: &Bound<'_, PyArray2<f64>>,
    red: &Bound<'_, PyArray2<f64>>,
    nodata: Option<f64>,
) -> PyResult<Bound<'py, PyArray2<f64>>> {
    let nir_shape = nir.shape();
    let red_shape = red.shape();

    if nir_shape != red_shape {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "NIR and RED bands must have the same shape",
        ));
    }

    let nir_readonly = nir.readonly();
    let red_readonly = red.readonly();
    let nir_owned: Vec<f64> = nir_readonly
        .as_slice()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Array must be contiguous"))?
        .to_vec();
    let red_owned: Vec<f64> = red_readonly
        .as_slice()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Array must be contiguous"))?
        .to_vec();
    drop(nir_readonly);
    drop(red_readonly);

    let width = nir_shape[1];

    // Elementwise index computation is CPU-bound; release the GIL.
    let result = py.detach(|| -> Vec<Vec<f64>> {
        let result_data: Vec<f64> = nir_owned
            .iter()
            .zip(red_owned.iter())
            .map(|(&nir_val, &red_val)| {
                // Check for nodata
                if let Some(nd) = nodata
                    && ((nir_val - nd).abs() < 1e-10 || (red_val - nd).abs() < 1e-10)
                {
                    return nd;
                }

                let sum = nir_val + red_val;
                if sum.abs() < 1e-10 {
                    0.0
                } else {
                    (nir_val - red_val) / sum
                }
            })
            .collect();

        result_data
            .chunks(width)
            .map(|chunk| chunk.to_vec())
            .collect()
    });

    numpy::PyArray2::from_vec2(py, &result).map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to create result array: {}", e))
    })
}

/// Calculates EVI (Enhanced Vegetation Index).
///
/// Args:
///     nir (numpy.ndarray): Near-infrared band (2D)
///     red (numpy.ndarray): Red band (2D)
///     blue (numpy.ndarray): Blue band (2D)
///     config (dict, optional): Configuration dictionary with keys: 'g', 'c1', 'c2', 'l'
///         - g (float): Gain factor (default: 2.5)
///         - c1 (float): Coefficient for aerosol resistance (default: 6.0)
///         - c2 (float): Coefficient for aerosol resistance (default: 7.5)
///         - l (float): Soil adjustment factor (default: 1.0)
///
/// Returns:
///     numpy.ndarray: EVI values
///
/// Example:
///     >>> # Use default parameters
///     >>> evi = oxigeo.evi(nir, red, blue)
///     >>>
///     >>> # Use custom parameters
///     >>> evi = oxigeo.evi(nir, red, blue, config={'g': 3.0, 'c1': 7.0, 'c2': 8.0, 'l': 1.5})
#[pyfunction]
#[pyo3(signature = (nir, red, blue, config=None))]
pub fn evi<'py>(
    py: Python<'py>,
    nir: &Bound<'_, PyArray2<f64>>,
    red: &Bound<'_, PyArray2<f64>>,
    blue: &Bound<'_, PyArray2<f64>>,
    config: Option<&Bound<'_, PyDict>>,
) -> PyResult<Bound<'py, PyArray2<f64>>> {
    let cfg = if let Some(dict) = config {
        EviConfig {
            g: dict
                .get_item("g")?
                .and_then(|v| v.extract().ok())
                .unwrap_or(2.5),
            c1: dict
                .get_item("c1")?
                .and_then(|v| v.extract().ok())
                .unwrap_or(6.0),
            c2: dict
                .get_item("c2")?
                .and_then(|v| v.extract().ok())
                .unwrap_or(7.5),
            l: dict
                .get_item("l")?
                .and_then(|v| v.extract().ok())
                .unwrap_or(1.0),
        }
    } else {
        EviConfig::default()
    };
    let shape = nir.shape();
    if red.shape() != shape || blue.shape() != shape {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "All bands must have the same shape",
        ));
    }

    let nir_readonly = nir.readonly();
    let red_readonly = red.readonly();
    let blue_readonly = blue.readonly();
    let nir_owned: Vec<f64> = nir_readonly
        .as_slice()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Array must be contiguous"))?
        .to_vec();
    let red_owned: Vec<f64> = red_readonly
        .as_slice()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Array must be contiguous"))?
        .to_vec();
    let blue_owned: Vec<f64> = blue_readonly
        .as_slice()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Array must be contiguous"))?
        .to_vec();
    drop(nir_readonly);
    drop(red_readonly);
    drop(blue_readonly);

    let width = shape[1];

    // Elementwise index computation is CPU-bound; release the GIL.
    let result = py.detach(|| -> Vec<Vec<f64>> {
        let result_data: Vec<f64> = nir_owned
            .iter()
            .zip(red_owned.iter())
            .zip(blue_owned.iter())
            .map(|((&nir_val, &red_val), &blue_val)| {
                let denominator = nir_val + cfg.c1 * red_val - cfg.c2 * blue_val + cfg.l;
                if denominator.abs() < 1e-10 {
                    0.0
                } else {
                    cfg.g * (nir_val - red_val) / denominator
                }
            })
            .collect();

        result_data
            .chunks(width)
            .map(|chunk| chunk.to_vec())
            .collect()
    });

    numpy::PyArray2::from_vec2(py, &result).map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to create result array: {}", e))
    })
}

/// Calculates NDWI (Normalized Difference Water Index).
///
/// Args:
///     green (numpy.ndarray): Green band (2D)
///     nir (numpy.ndarray): Near-infrared band (2D)
///     nodata (float, optional): NoData value
///
/// Returns:
///     numpy.ndarray: NDWI values ranging from -1 to 1
///
/// Example:
///     >>> ndwi = oxigeo.ndwi(green_band, nir_band)
///     >>> water_mask = ndwi > 0.3
#[pyfunction]
#[pyo3(signature = (green, nir, nodata=None))]
pub fn ndwi<'py>(
    py: Python<'py>,
    green: &Bound<'_, PyArray2<f64>>,
    nir: &Bound<'_, PyArray2<f64>>,
    nodata: Option<f64>,
) -> PyResult<Bound<'py, PyArray2<f64>>> {
    // NDWI = (Green - NIR) / (Green + NIR)
    ndvi(py, green, nir, nodata) // Reuse NDVI calculation logic
}
