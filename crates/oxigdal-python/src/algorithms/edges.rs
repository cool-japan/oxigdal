//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use numpy::{PyArray2, PyArrayMethods, PyUntypedArrayMethods};
use oxigdal_algorithms::raster::{self};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use pyo3::prelude::*;

use super::stats::{raster_buffer_to_vec2, slice_to_raster_buffer};
// The remaining helpers are only exercised by this module's test suite
// (they moved here from the original single-file `algorithms.rs`, which
// co-located these helper unit tests with `canny_edges`); gate the imports
// accordingly so a non-test build doesn't warn about unused imports.
#[cfg(test)]
use super::stats::{convolve_with_boundary, histogram, reflect_index, wrap_index};
#[cfg(test)]
use super::types::ConvBoundary;

/// Applies Canny edge detection.
///
/// Args:
///     array (numpy.ndarray): Input array (2D)
///     low_threshold (float): Low threshold for hysteresis
///     high_threshold (float): High threshold for hysteresis
///     sigma (float): Gaussian blur sigma (default: 1.0)
///
/// Returns:
///     numpy.ndarray: Binary edge map
///
/// Example:
///     >>> edges = oxigdal.canny_edges(data, low_threshold=0.1, high_threshold=0.3)
#[pyfunction]
#[pyo3(signature = (array, low_threshold, high_threshold, sigma=1.0))]
pub fn canny_edges<'py>(
    py: Python<'py>,
    array: &Bound<'_, PyArray2<f64>>,
    low_threshold: f64,
    high_threshold: f64,
    sigma: f64,
) -> PyResult<Bound<'py, PyArray2<f64>>> {
    if low_threshold >= high_threshold {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Low threshold must be less than high threshold",
        ));
    }

    if sigma <= 0.0 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Sigma must be positive",
        ));
    }

    let shape = array.shape();
    let (height_usize, width_usize) = (shape[0], shape[1]);
    let readonly = array.readonly();
    let owned: Vec<f64> = readonly
        .as_slice()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Array must be contiguous"))?
        .to_vec();
    drop(readonly);

    // Gaussian blur + Canny edge detection is CPU-bound; release the GIL
    // while it runs.
    let result = py.detach(|| -> PyResult<Vec<Vec<f64>>> {
        let src = slice_to_raster_buffer(&owned, width_usize, height_usize);

        // Step 1: Apply Gaussian blur to reduce noise
        let ksize = {
            let radius = (4.0 * sigma).ceil() as usize;
            let s = 2 * radius + 1;
            if s.is_multiple_of(2) { s + 1 } else { s }
        };
        let blurred = raster::gaussian_blur(&src, sigma, Some(ksize)).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!(
                "Canny edge detection (blur step) failed: {}",
                e
            ))
        })?;

        // Step 2: Use detect_edges with Canny detector from oxigdal-algorithms
        let edge_detector = raster::EdgeDetector::Canny {
            low_threshold,
            high_threshold,
        };
        let edges_buf = raster::detect_edges(&blurred, edge_detector).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Canny edge detection failed: {}", e))
        })?;

        // Step 3: Normalize to binary (0.0 or 1.0)
        let width = width_usize as u64;
        let height = height_usize as u64;
        let mut binary_edges = RasterBuffer::zeros(width, height, RasterDataType::Float64);
        for y in 0..height {
            for x in 0..width {
                let val = edges_buf.get_pixel(x, y).map_err(|e| {
                    pyo3::exceptions::PyRuntimeError::new_err(format!(
                        "Failed to read edge pixel: {}",
                        e
                    ))
                })?;
                let out = if val > 0.0 { 1.0 } else { 0.0 };
                binary_edges.set_pixel(x, y, out).map_err(|e| {
                    pyo3::exceptions::PyRuntimeError::new_err(format!(
                        "Failed to write edge pixel: {}",
                        e
                    ))
                })?;
            }
        }

        raster_buffer_to_vec2(&binary_edges).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to read result: {}", e))
        })
    })?;

    numpy::PyArray2::from_vec2(py, &result).map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to create result array: {}", e))
    })
}

/// Inverts a square matrix using Gauss-Jordan elimination.
/// Returns the identity matrix if inversion fails (singular matrix).
pub(super) fn invert_matrix(matrix: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = matrix.len();
    // Create augmented matrix [A | I]
    let mut augmented: Vec<Vec<f64>> = matrix
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let mut aug_row = row.clone();
            aug_row.resize(2 * n, 0.0);
            aug_row[n + i] = 1.0;
            aug_row
        })
        .collect();

    // Forward elimination
    for col in 0..n {
        // Find pivot
        let mut max_row = col;
        let mut max_val = augmented[col][col].abs();
        for (row, aug_row) in augmented.iter().enumerate().skip(col + 1) {
            let val = aug_row[col].abs();
            if val > max_val {
                max_val = val;
                max_row = row;
            }
        }

        if max_val < 1e-12 {
            // Singular matrix, return identity
            return (0..n)
                .map(|i| {
                    let mut row = vec![0.0; n];
                    row[i] = 1.0;
                    row
                })
                .collect();
        }

        // Swap rows
        if max_row != col {
            augmented.swap(col, max_row);
        }

        // Scale pivot row
        let pivot = augmented[col][col];
        for cell in augmented[col].iter_mut() {
            *cell /= pivot;
        }

        // Eliminate column
        let col_row = augmented[col].clone();
        for (row, aug_row) in augmented.iter_mut().enumerate() {
            if row != col {
                let factor = aug_row[col];
                for (cell, &col_val) in aug_row.iter_mut().zip(col_row.iter()) {
                    *cell -= factor * col_val;
                }
            }
        }
    }

    // Extract inverse
    augmented.iter().map(|row| row[n..].to_vec()).collect()
}

/// Computes the determinant of a square matrix using LU decomposition-style approach.
pub(super) fn matrix_determinant(matrix: &[Vec<f64>]) -> f64 {
    let n = matrix.len();
    if n == 0 {
        return 1.0;
    }
    if n == 1 {
        return matrix[0][0];
    }
    if n == 2 {
        return matrix[0][0] * matrix[1][1] - matrix[0][1] * matrix[1][0];
    }

    // Use Gaussian elimination to compute determinant
    let mut work: Vec<Vec<f64>> = matrix.to_vec();
    let mut det = 1.0;

    for col in 0..n {
        // Find pivot
        let mut max_row = col;
        let mut max_val = work[col][col].abs();
        for (row, work_row) in work.iter().enumerate().skip(col + 1) {
            let val = work_row[col].abs();
            if val > max_val {
                max_val = val;
                max_row = row;
            }
        }

        if max_val < 1e-15 {
            return 0.0;
        }

        if max_row != col {
            work.swap(col, max_row);
            det = -det; // Row swap changes sign
        }

        det *= work[col][col];

        let col_row = work[col].clone();
        for (_, work_row) in work.iter_mut().enumerate().skip(col + 1) {
            let factor = work_row[col] / col_row[col];
            for (j, cell) in work_row.iter_mut().enumerate().skip(col + 1) {
                *cell -= factor * col_row[j];
            }
        }
    }

    det
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    #[test]
    fn test_reflect_and_wrap_index() {
        // Reflect (half-sample symmetric) on n=4: indices -1,-2 -> 0,1; 4,5 -> 3,2
        assert_eq!(reflect_index(-1, 4), 0);
        assert_eq!(reflect_index(-2, 4), 1);
        assert_eq!(reflect_index(4, 4), 3);
        assert_eq!(reflect_index(5, 4), 2);
        assert_eq!(reflect_index(2, 4), 2);
        // n=1 degenerate
        assert_eq!(reflect_index(-3, 1), 0);
        assert_eq!(reflect_index(7, 1), 0);

        // Wrap on n=4
        assert_eq!(wrap_index(-1, 4), 3);
        assert_eq!(wrap_index(4, 4), 0);
        assert_eq!(wrap_index(5, 4), 1);
    }

    #[test]
    fn test_convolve_boundary_modes_differ_at_edges() {
        // 3x3 input with a strong left column so edge handling matters.
        let data = vec![
            10.0, 0.0, 0.0, //
            10.0, 0.0, 0.0, //
            10.0, 0.0, 0.0,
        ];
        // Asymmetric horizontal-difference kernel (samples the left neighbor).
        let kernel = vec![
            0.0, 0.0, 0.0, //
            1.0, 0.0, 0.0, //
            0.0, 0.0, 0.0,
        ];

        let nearest = convolve_with_boundary(
            &data,
            3,
            3,
            &kernel,
            3,
            3,
            false,
            ConvBoundary::Nearest,
            0.0,
        );
        let constant0 = convolve_with_boundary(
            &data,
            3,
            3,
            &kernel,
            3,
            3,
            false,
            ConvBoundary::Constant,
            0.0,
        );
        let wrap =
            convolve_with_boundary(&data, 3, 3, &kernel, 3, 3, false, ConvBoundary::Wrap, 0.0);
        let reflect = convolve_with_boundary(
            &data,
            3,
            3,
            &kernel,
            3,
            3,
            false,
            ConvBoundary::Reflect,
            0.0,
        );

        // Output pixel (row 0, col 0) samples the left neighbor of column 0.
        // nearest: replicate col 0 (value 10) -> 10
        assert_eq!(nearest[0], 10.0);
        // constant 0: out-of-range -> fill 0 -> 0
        assert_eq!(constant0[0], 0.0);
        // wrap: left of col 0 wraps to col 2 (value 0) -> 0
        assert_eq!(wrap[0], 0.0);
        // reflect: left of col 0 mirrors to col 0 (value 10) -> 10
        assert_eq!(reflect[0], 10.0);

        // Constant differs from nearest at the edge (the key regression: modes
        // are no longer all identical).
        assert_ne!(constant0[0], nearest[0]);
        assert_ne!(wrap[0], nearest[0]);

        // Interior column-1 pixel samples column 0 (value 10) identically in all
        // modes since no boundary is involved.
        assert_eq!(nearest[1], 10.0);
        assert_eq!(constant0[1], 10.0);
        assert_eq!(wrap[1], 10.0);
        assert_eq!(reflect[1], 10.0);
    }

    #[test]
    fn test_convolve_constant_fill_value_applied() {
        let data = vec![5.0];
        let kernel = vec![
            0.0, 0.0, 0.0, //
            1.0, 0.0, 0.0, //
            0.0, 0.0, 0.0,
        ];
        // 1x1 image: the left neighbor is always out of range, so the output is
        // exactly the fill value.
        let out = convolve_with_boundary(
            &data,
            1,
            1,
            &kernel,
            3,
            3,
            false,
            ConvBoundary::Constant,
            -99.0,
        );
        assert_eq!(out[0], -99.0);
    }

    #[test]
    fn test_histogram_validation() {
        // Test that histogram rejects invalid bin count.
        // Initialize the Python interpreter so pyo3 APIs work in unit tests.
        Python::initialize();
        Python::attach(|py| {
            let array = numpy::PyArray2::zeros(py, [10, 10], false);
            let result = histogram(py, &array, 1, None, None);
            // bins=1 is too few, should return an error
            assert!(result.is_err());
        });
    }
}
