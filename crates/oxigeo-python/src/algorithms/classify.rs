//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use numpy::{PyArray2, PyArrayMethods, PyUntypedArrayMethods};
use oxigeo_algorithms::raster::{self};
use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::types::RasterDataType;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use super::edges::{invert_matrix, matrix_determinant};
use super::stats::{raster_buffer_to_vec2, slice_to_raster_buffer};

/// Performs supervised classification using training samples.
///
/// Args:
///     bands (list): List of band arrays (each 2D)
///     training_data (dict): Training samples with class labels
///     method (str): Classification method - "maximum_likelihood", "minimum_distance"
///
/// Returns:
///     numpy.ndarray: Class labels (2D array)
///
/// Example:
///     >>> training = {
///     ...     1: [(100, 200), (101, 201)],  # Class 1 samples (row, col)
///     ...     2: [(300, 400), (301, 401)]   # Class 2 samples
///     ... }
///     >>> classes = oxigeo.supervised_classify(bands, training, method="maximum_likelihood")
#[pyfunction]
#[pyo3(signature = (bands, training_data, method="maximum_likelihood"))]
pub fn supervised_classify<'py>(
    py: Python<'py>,
    bands: Vec<Bound<'_, PyArray2<f64>>>,
    training_data: &Bound<'_, PyDict>,
    method: &str,
) -> PyResult<Bound<'py, PyArray2<i64>>> {
    if bands.is_empty() {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "At least one band required",
        ));
    }

    let valid_methods = ["maximum_likelihood", "minimum_distance", "random_forest"];
    if !valid_methods.contains(&method) {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Invalid method '{}'. Valid options: {:?}",
            method, valid_methods
        )));
    }

    let first_shape = bands[0].shape();
    let rows = first_shape[0];
    let cols = first_shape[1];
    let n_bands = bands.len();

    // Read all band data
    let mut band_data: Vec<Vec<f64>> = Vec::with_capacity(n_bands);
    for band in &bands {
        let ro = band.readonly();
        let slice = ro.as_slice().map_err(|_| {
            pyo3::exceptions::PyValueError::new_err("Band array must be contiguous")
        })?;
        band_data.push(slice.to_vec());
    }

    // Parse training data: dict of {class_label: [(row, col), ...]}. Dict
    // iteration/extraction touches Python objects and needs the GIL, so it
    // happens up front, before the CPU-bound classification work below.
    let mut training_samples: Vec<(i64, Vec<(usize, usize)>)> = Vec::new();
    for item in training_data.iter() {
        let (key, value) = item;
        let class_id: i64 = key.extract::<i64>().map_err(|_| {
            pyo3::exceptions::PyValueError::new_err("Class labels must be integers")
        })?;

        let samples: Vec<(usize, usize)> =
            value.extract::<Vec<(usize, usize)>>().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err(
                    "Training samples must be list of (row, col) tuples",
                )
            })?;

        training_samples.push((class_id, samples));
    }

    // Per-class statistics (mean/covariance/inversion) and the pixel-by-pixel
    // classification pass are CPU-bound; release the GIL while they run.
    let result_labels = py.detach(|| -> PyResult<Vec<Vec<i64>>> {
        // Compute per-class mean (and covariance for maximum_likelihood)
        struct ClassStats {
            class_id: i64,
            mean: Vec<f64>,
            // Inverse covariance matrix and log-determinant for maximum_likelihood
            inv_cov: Option<Vec<Vec<f64>>>,
            log_det: f64,
        }

        let mut class_stats_list: Vec<ClassStats> = Vec::new();

        for (class_id, samples) in &training_samples {
            if samples.is_empty() {
                continue;
            }

            // Collect pixel values for this class
            let mut class_pixels: Vec<Vec<f64>> = Vec::with_capacity(samples.len());
            for &(row, col) in samples {
                if row >= rows || col >= cols {
                    return Err(pyo3::exceptions::PyValueError::new_err(format!(
                        "Training sample ({}, {}) out of bounds for array ({}, {})",
                        row, col, rows, cols
                    )));
                }
                let pixel_idx = row * cols + col;
                let pixel: Vec<f64> = (0..n_bands).map(|b| band_data[b][pixel_idx]).collect();
                class_pixels.push(pixel);
            }

            // Compute mean
            let n = class_pixels.len() as f64;
            let mean: Vec<f64> = (0..n_bands)
                .map(|b| class_pixels.iter().map(|p| p[b]).sum::<f64>() / n)
                .collect();

            // Compute covariance matrix for maximum_likelihood
            let (inv_cov, log_det) =
                if method == "maximum_likelihood" && class_pixels.len() > n_bands {
                    // Compute covariance matrix
                    let mut cov = vec![vec![0.0_f64; n_bands]; n_bands];
                    for p in &class_pixels {
                        for i in 0..n_bands {
                            for j in 0..n_bands {
                                cov[i][j] += (p[i] - mean[i]) * (p[j] - mean[j]);
                            }
                        }
                    }
                    let denom = if class_pixels.len() > 1 {
                        (class_pixels.len() - 1) as f64
                    } else {
                        1.0
                    };
                    for row in &mut cov {
                        for cell in row.iter_mut() {
                            *cell /= denom;
                        }
                    }

                    // Add regularization to diagonal
                    for (i, row) in cov.iter_mut().enumerate() {
                        row[i] += 1e-6;
                    }

                    // Invert the covariance matrix using Gauss-Jordan
                    let inv = invert_matrix(&cov);
                    let det = matrix_determinant(&cov);
                    let ld = if det > 0.0 { det.ln() } else { 0.0 };

                    (Some(inv), ld)
                } else {
                    (None, 0.0)
                };

            class_stats_list.push(ClassStats {
                class_id: *class_id,
                mean,
                inv_cov,
                log_det,
            });
        }

        if class_stats_list.is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "No valid training data provided",
            ));
        }

        // Classify each pixel
        let mut result_labels = vec![vec![0_i64; cols]; rows];

        for (y, row) in result_labels.iter_mut().enumerate() {
            for (x, cell) in row.iter_mut().enumerate() {
                let pixel_idx = y * cols + x;
                let pixel: Vec<f64> = (0..n_bands).map(|b| band_data[b][pixel_idx]).collect();

                let best_class = match method {
                    "minimum_distance" => {
                        // Assign to class with nearest mean (Euclidean distance)
                        let mut best_id = class_stats_list[0].class_id;
                        let mut best_dist = f64::INFINITY;
                        for cs in &class_stats_list {
                            let dist: f64 = pixel
                                .iter()
                                .zip(cs.mean.iter())
                                .map(|(&p, &m)| (p - m) * (p - m))
                                .sum();
                            if dist < best_dist {
                                best_dist = dist;
                                best_id = cs.class_id;
                            }
                        }
                        best_id
                    }
                    "maximum_likelihood" => {
                        // Maximum likelihood classification (multivariate Gaussian)
                        let mut best_id = class_stats_list[0].class_id;
                        let mut best_score = f64::NEG_INFINITY;
                        for cs in &class_stats_list {
                            if let Some(ref inv_cov) = cs.inv_cov {
                                // Mahalanobis distance squared
                                let diff: Vec<f64> = pixel
                                    .iter()
                                    .zip(cs.mean.iter())
                                    .map(|(&p, &m)| p - m)
                                    .collect();
                                let mut mahal = 0.0;
                                for i in 0..n_bands {
                                    for j in 0..n_bands {
                                        mahal += diff[i] * inv_cov[i][j] * diff[j];
                                    }
                                }
                                // Log-likelihood (ignoring constant terms)
                                let score = -0.5 * (mahal + cs.log_det);
                                if score > best_score {
                                    best_score = score;
                                    best_id = cs.class_id;
                                }
                            } else {
                                // Fallback to minimum distance if covariance not available
                                let dist: f64 = pixel
                                    .iter()
                                    .zip(cs.mean.iter())
                                    .map(|(&p, &m)| (p - m) * (p - m))
                                    .sum();
                                let score = -dist;
                                if score > best_score {
                                    best_score = score;
                                    best_id = cs.class_id;
                                }
                            }
                        }
                        best_id
                    }
                    // "random_forest" or other - fallback to minimum distance
                    _ => {
                        let mut best_id = class_stats_list[0].class_id;
                        let mut best_dist = f64::INFINITY;
                        for cs in &class_stats_list {
                            let dist: f64 = pixel
                                .iter()
                                .zip(cs.mean.iter())
                                .map(|(&p, &m)| (p - m) * (p - m))
                                .sum();
                            if dist < best_dist {
                                best_dist = dist;
                                best_id = cs.class_id;
                            }
                        }
                        best_id
                    }
                };

                *cell = best_class;
            }
        }

        Ok(result_labels)
    })?;

    numpy::PyArray2::from_vec2(py, &result_labels).map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to create result array: {}", e))
    })
}

/// Detects edges using Sobel operator.
///
/// Args:
///     array (numpy.ndarray): Input array (2D)
///     direction (str): Edge direction - "both", "horizontal", "vertical" (default: "both")
///     threshold (float, optional): Threshold for edge detection
///
/// Returns:
///     numpy.ndarray: Edge magnitude array
///
/// Example:
///     >>> edges = oxigeo.sobel_edges(data)
///     >>>
///     >>> # Detect horizontal edges only
///     >>> h_edges = oxigeo.sobel_edges(data, direction="horizontal")
#[pyfunction]
#[pyo3(signature = (array, direction="both", threshold=None))]
pub fn sobel_edges<'py>(
    py: Python<'py>,
    array: &Bound<'_, PyArray2<f64>>,
    direction: &str,
    threshold: Option<f64>,
) -> PyResult<Bound<'py, PyArray2<f64>>> {
    let valid_directions = ["both", "horizontal", "vertical"];
    if !valid_directions.contains(&direction) {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Invalid direction '{}'. Valid options: {:?}",
            direction, valid_directions
        )));
    }

    let shape = array.shape();
    let (height_usize, width_usize) = (shape[0], shape[1]);
    let readonly = array.readonly();
    let owned: Vec<f64> = readonly
        .as_slice()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Array must be contiguous"))?
        .to_vec();
    drop(readonly);

    // The convolution/edge-magnitude computation is CPU-bound; release the
    // GIL while it runs.
    let result = py.detach(|| -> PyResult<Vec<Vec<f64>>> {
        let src = slice_to_raster_buffer(&owned, width_usize, height_usize);

        // Sobel kernels
        let sobel_x: [f64; 9] = [-1.0, 0.0, 1.0, -2.0, 0.0, 2.0, -1.0, 0.0, 1.0];
        let sobel_y: [f64; 9] = [-1.0, -2.0, -1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 1.0];

        let width = width_usize as u64;
        let height = height_usize as u64;

        let result_buf = match direction {
            "horizontal" => {
                // Only horizontal edges (sobel_y kernel)
                raster::focal_convolve(&src, &sobel_y, 3, 3, false).map_err(|e| {
                    pyo3::exceptions::PyRuntimeError::new_err(format!(
                        "Sobel edge detection failed: {}",
                        e
                    ))
                })?
            }
            "vertical" => {
                // Only vertical edges (sobel_x kernel)
                raster::focal_convolve(&src, &sobel_x, 3, 3, false).map_err(|e| {
                    pyo3::exceptions::PyRuntimeError::new_err(format!(
                        "Sobel edge detection failed: {}",
                        e
                    ))
                })?
            }
            _ => {
                // "both" - compute full gradient magnitude using sobel_edge_detection
                raster::sobel_edge_detection(&src).map_err(|e| {
                    pyo3::exceptions::PyRuntimeError::new_err(format!(
                        "Sobel edge detection failed: {}",
                        e
                    ))
                })?
            }
        };

        // Apply threshold if specified
        let final_buf = if let Some(thresh) = threshold {
            let mut thresholded = RasterBuffer::zeros(width, height, RasterDataType::Float64);
            for y in 0..height {
                for x in 0..width {
                    let val = result_buf.get_pixel(x, y).map_err(|e| {
                        pyo3::exceptions::PyRuntimeError::new_err(format!(
                            "Failed to read pixel: {}",
                            e
                        ))
                    })?;
                    let out = if val >= thresh { val } else { 0.0 };
                    thresholded.set_pixel(x, y, out).map_err(|e| {
                        pyo3::exceptions::PyRuntimeError::new_err(format!(
                            "Failed to write pixel: {}",
                            e
                        ))
                    })?;
                }
            }
            thresholded
        } else {
            result_buf
        };

        raster_buffer_to_vec2(&final_buf).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to read result: {}", e))
        })
    })?;

    numpy::PyArray2::from_vec2(py, &result).map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to create result array: {}", e))
    })
}
