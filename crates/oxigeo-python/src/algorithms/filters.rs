//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use numpy::{PyArray2, PyArrayMethods, PyUntypedArrayMethods};
use oxigeo_algorithms::raster::{self, StructuringElement};
use pyo3::prelude::*;

use super::stats::{
    convolve_with_boundary, raster_buffer_to_vec2, slice_to_raster_buffer,
    slice_to_raster_buffer_with_nodata,
};
use super::types::ConvBoundary;

/// Applies convolution filter to a raster array.
///
/// Args:
///     array (numpy.ndarray): Input array (2D)
///     kernel (numpy.ndarray): Convolution kernel (2D)
///     normalize (bool): Normalize kernel (default: False)
///     boundary (str): Boundary mode - "reflect", "constant", "nearest" (default: "reflect")
///     fill_value (float): Fill value for constant boundary (default: 0.0)
///
/// Returns:
///     numpy.ndarray: Filtered array
///
/// Example:
///     >>> # Apply 3x3 averaging filter
///     >>> kernel = np.ones((3, 3)) / 9
///     >>> filtered = oxigeo.convolve(data, kernel)
///     >>>
///     >>> # Sobel edge detection
///     >>> sobel_x = np.array([[-1, 0, 1], [-2, 0, 2], [-1, 0, 1]])
///     >>> edges_x = oxigeo.convolve(data, sobel_x)
#[pyfunction]
#[pyo3(signature = (array, kernel, normalize=false, boundary="reflect", fill_value=0.0))]
pub fn convolve<'py>(
    py: Python<'py>,
    array: &Bound<'_, PyArray2<f64>>,
    kernel: &Bound<'_, PyArray2<f64>>,
    normalize: bool,
    boundary: &str,
    fill_value: f64,
) -> PyResult<Bound<'py, PyArray2<f64>>> {
    // Validate and map the boundary mode. Every accepted mode is actually
    // applied below (previously boundary/fill_value were silently discarded and
    // all modes behaved identically to edge replication).
    let mode = match boundary {
        "reflect" => ConvBoundary::Reflect,
        "constant" => ConvBoundary::Constant,
        "nearest" => ConvBoundary::Nearest,
        "wrap" => ConvBoundary::Wrap,
        other => {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Invalid boundary mode '{}'. Valid options: [\"reflect\", \"constant\", \"nearest\", \"wrap\"]",
                other
            )));
        }
    };

    let arr_shape = array.shape();
    let kernel_shape = kernel.shape();

    if kernel_shape[0] % 2 == 0 || kernel_shape[1] % 2 == 0 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Kernel dimensions must be odd",
        ));
    }

    // Extract owned copies of the input/kernel data while the GIL is held so the
    // CPU-bound convolution can run with the GIL released.
    let readonly = array.readonly();
    let arr_data = readonly
        .as_slice()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Array must be contiguous"))?
        .to_vec();
    drop(readonly);

    let k_readonly = kernel.readonly();
    let k_data = k_readonly
        .as_slice()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Kernel must be contiguous"))?
        .to_vec();
    drop(k_readonly);

    let arr_width = arr_shape[1];
    let arr_height = arr_shape[0];
    let k_width = kernel_shape[1];
    let k_height = kernel_shape[0];

    // Run the actual convolution with the GIL released.
    let result_data = py.detach(|| {
        convolve_with_boundary(
            &arr_data, arr_width, arr_height, &k_data, k_width, k_height, normalize, mode,
            fill_value,
        )
    });

    let result: Vec<Vec<f64>> = result_data
        .chunks(arr_width)
        .map(|chunk| chunk.to_vec())
        .collect();

    numpy::PyArray2::from_vec2(py, &result).map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to create result array: {}", e))
    })
}

/// Applies Gaussian blur filter.
///
/// Args:
///     array (numpy.ndarray): Input array (2D)
///     sigma (float): Standard deviation for Gaussian kernel
///     kernel_size (int, optional): Kernel size (auto-calculated if None)
///     truncate (float): Truncate kernel at this many standard deviations (default: 4.0)
///
/// Returns:
///     numpy.ndarray: Blurred array
///
/// Example:
///     >>> blurred = oxigeo.gaussian_blur(data, sigma=2.0)
///     >>>
///     >>> # Strong blur with large kernel
///     >>> very_blurred = oxigeo.gaussian_blur(data, sigma=5.0, kernel_size=31)
#[pyfunction]
#[pyo3(signature = (array, sigma, kernel_size=None, truncate=4.0))]
pub fn gaussian_blur<'py>(
    py: Python<'py>,
    array: &Bound<'_, PyArray2<f64>>,
    sigma: f64,
    kernel_size: Option<usize>,
    truncate: f64,
) -> PyResult<Bound<'py, PyArray2<f64>>> {
    if sigma <= 0.0 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Sigma must be positive",
        ));
    }

    if truncate <= 0.0 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Truncate must be positive",
        ));
    }

    // Calculate kernel size if not provided
    let _ksize = if let Some(ks) = kernel_size {
        if ks % 2 == 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Kernel size must be odd",
            ));
        }
        ks
    } else {
        let radius = (truncate * sigma).ceil() as usize;
        2 * radius + 1
    };

    let shape = array.shape();
    let (height, width) = (shape[0], shape[1]);
    let readonly = array.readonly();
    let owned: Vec<f64> = readonly
        .as_slice()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Array must be contiguous"))?
        .to_vec();
    drop(readonly);

    // The blur convolution is CPU-bound; release the GIL while it runs.
    let result = py.detach(|| -> PyResult<Vec<Vec<f64>>> {
        let src = slice_to_raster_buffer(&owned, width, height);

        let result_buf = raster::gaussian_blur(&src, sigma, Some(_ksize)).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Gaussian blur failed: {}", e))
        })?;

        raster_buffer_to_vec2(&result_buf).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to read result: {}", e))
        })
    })?;

    numpy::PyArray2::from_vec2(py, &result).map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to create result array: {}", e))
    })
}

/// Applies median filter.
///
/// Args:
///     array (numpy.ndarray): Input array (2D)
///     size (int): Filter window size (must be odd)
///     nodata (float, optional): NoData value to exclude
///
/// Returns:
///     numpy.ndarray: Filtered array
///
/// Example:
///     >>> # Remove salt-and-pepper noise
///     >>> denoised = oxigeo.median_filter(noisy_data, size=5)
#[pyfunction]
#[pyo3(signature = (array, size, nodata=None))]
pub fn median_filter<'py>(
    py: Python<'py>,
    array: &Bound<'_, PyArray2<f64>>,
    size: usize,
    nodata: Option<f64>,
) -> PyResult<Bound<'py, PyArray2<f64>>> {
    if size.is_multiple_of(2) {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Filter size must be odd",
        ));
    }

    if size < 3 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Filter size must be at least 3",
        ));
    }

    let shape = array.shape();
    let (height, width) = (shape[0], shape[1]);
    let readonly = array.readonly();
    let owned: Vec<f64> = readonly
        .as_slice()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Array must be contiguous"))?
        .to_vec();
    drop(readonly);

    // The median filter is CPU-bound; release the GIL while it runs.
    let result = py.detach(|| -> PyResult<Vec<Vec<f64>>> {
        let src = if let Some(nd) = nodata {
            slice_to_raster_buffer_with_nodata(&owned, width, height, nd)
        } else {
            slice_to_raster_buffer(&owned, width, height)
        };

        let result_buf = raster::median_filter(&src, size).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Median filter failed: {}", e))
        })?;

        raster_buffer_to_vec2(&result_buf).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to read result: {}", e))
        })
    })?;

    numpy::PyArray2::from_vec2(py, &result).map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to create result array: {}", e))
    })
}

/// Applies morphological erosion.
///
/// Args:
///     array (numpy.ndarray): Input binary array (2D)
///     kernel (numpy.ndarray, optional): Structuring element (default: 3x3 square)
///     iterations (int): Number of iterations (default: 1)
///
/// Returns:
///     numpy.ndarray: Eroded array
///
/// Example:
///     >>> binary_mask = data > 0.5
///     >>> eroded = oxigeo.erosion(binary_mask.astype(float))
#[pyfunction]
#[pyo3(signature = (array, kernel=None, iterations=1))]
pub fn erosion<'py>(
    py: Python<'py>,
    array: &Bound<'_, PyArray2<f64>>,
    kernel: Option<&Bound<'_, PyArray2<f64>>>,
    iterations: usize,
) -> PyResult<Bound<'py, PyArray2<f64>>> {
    if iterations < 1 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Iterations must be at least 1",
        ));
    }

    let shape = array.shape();
    let (height, width) = (shape[0], shape[1]);
    let readonly = array.readonly();
    let owned: Vec<f64> = readonly
        .as_slice()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Array must be contiguous"))?
        .to_vec();
    drop(readonly);

    // Determine structuring element: use custom kernel if provided, else default 3x3 square
    let se = if let Some(k) = kernel {
        let k_shape = k.shape();
        if k_shape[0] != k_shape[1] {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Kernel must be square",
            ));
        }
        StructuringElement::Square { size: k_shape[0] }
    } else {
        StructuringElement::Square { size: 3 }
    };

    // Apply erosion iteratively; this is CPU-bound, release the GIL.
    let result = py.detach(|| -> PyResult<Vec<Vec<f64>>> {
        let src = slice_to_raster_buffer(&owned, width, height);
        let mut current = src;
        for _ in 0..iterations {
            current = raster::erode(&current, se).map_err(|e| {
                pyo3::exceptions::PyRuntimeError::new_err(format!("Erosion failed: {}", e))
            })?;
        }

        raster_buffer_to_vec2(&current).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to read result: {}", e))
        })
    })?;

    numpy::PyArray2::from_vec2(py, &result).map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to create result array: {}", e))
    })
}

/// Applies morphological dilation.
///
/// Args:
///     array (numpy.ndarray): Input binary array (2D)
///     kernel (numpy.ndarray, optional): Structuring element (default: 3x3 square)
///     iterations (int): Number of iterations (default: 1)
///
/// Returns:
///     numpy.ndarray: Dilated array
///
/// Example:
///     >>> binary_mask = data > 0.5
///     >>> dilated = oxigeo.dilation(binary_mask.astype(float))
#[pyfunction]
#[pyo3(signature = (array, kernel=None, iterations=1))]
pub fn dilation<'py>(
    py: Python<'py>,
    array: &Bound<'_, PyArray2<f64>>,
    kernel: Option<&Bound<'_, PyArray2<f64>>>,
    iterations: usize,
) -> PyResult<Bound<'py, PyArray2<f64>>> {
    if iterations < 1 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Iterations must be at least 1",
        ));
    }

    let shape = array.shape();
    let (height, width) = (shape[0], shape[1]);
    let readonly = array.readonly();
    let owned: Vec<f64> = readonly
        .as_slice()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Array must be contiguous"))?
        .to_vec();
    drop(readonly);

    // Determine structuring element: use custom kernel if provided, else default 3x3 square
    let se = if let Some(k) = kernel {
        let k_shape = k.shape();
        if k_shape[0] != k_shape[1] {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Kernel must be square",
            ));
        }
        StructuringElement::Square { size: k_shape[0] }
    } else {
        StructuringElement::Square { size: 3 }
    };

    // Apply dilation iteratively; this is CPU-bound, release the GIL.
    let result = py.detach(|| -> PyResult<Vec<Vec<f64>>> {
        let src = slice_to_raster_buffer(&owned, width, height);
        let mut current = src;
        for _ in 0..iterations {
            current = raster::dilate(&current, se).map_err(|e| {
                pyo3::exceptions::PyRuntimeError::new_err(format!("Dilation failed: {}", e))
            })?;
        }

        raster_buffer_to_vec2(&current).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to read result: {}", e))
        })
    })?;

    numpy::PyArray2::from_vec2(py, &result).map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to create result array: {}", e))
    })
}
