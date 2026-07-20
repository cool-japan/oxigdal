//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use numpy::{PyArray2, PyArrayMethods, PyUntypedArrayMethods};
use pyo3::prelude::*;

/// Performs unsupervised k-means classification.
///
/// Args:
///     bands (list): List of band arrays (each 2D)
///     n_clusters (int): Number of clusters
///     max_iter (int): Maximum iterations (default: 100)
///     tolerance (float): Convergence tolerance (default: 0.001)
///     nodata (float, optional): NoData value to exclude
///
/// Returns:
///     numpy.ndarray: Class labels (2D array)
///
/// Example:
///     >>> bands = [band1, band2, band3, band4]
///     >>> classes = oxigdal.kmeans_classify(bands, n_clusters=5)
#[pyfunction]
#[pyo3(signature = (bands, n_clusters, max_iter=100, tolerance=0.001, nodata=None))]
pub fn kmeans_classify<'py>(
    py: Python<'py>,
    bands: Vec<Bound<'_, PyArray2<f64>>>,
    n_clusters: usize,
    max_iter: usize,
    tolerance: f64,
    nodata: Option<f64>,
) -> PyResult<Bound<'py, PyArray2<i64>>> {
    if bands.is_empty() {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "At least one band required",
        ));
    }

    if n_clusters < 2 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Number of clusters must be at least 2",
        ));
    }

    if max_iter < 1 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Max iterations must be at least 1",
        ));
    }

    // Check all bands have same shape
    let first_shape = bands[0].shape();
    for band in &bands {
        if band.shape() != first_shape {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "All bands must have the same shape",
            ));
        }
    }

    let rows = first_shape[0];
    let cols = first_shape[1];
    let n_bands = bands.len();
    let n_pixels = rows * cols;

    // Read all band data into a matrix: n_pixels x n_bands
    let mut band_data: Vec<Vec<f64>> = Vec::with_capacity(n_bands);
    for band in &bands {
        let ro = band.readonly();
        let slice = ro.as_slice().map_err(|_| {
            pyo3::exceptions::PyValueError::new_err("Band array must be contiguous")
        })?;
        band_data.push(slice.to_vec());
    }

    // The clustering iteration below is CPU-bound and can run over large
    // rasters; release the GIL while it runs.
    let result_labels = py.detach(|| -> PyResult<Vec<Vec<i64>>> {
        // Track nodata mask
        let nodata_mask: Vec<bool> = if let Some(nd) = nodata {
            (0..n_pixels)
                .map(|i| band_data.iter().any(|b| (b[i] - nd).abs() < 1e-10))
                .collect()
        } else {
            vec![false; n_pixels]
        };

        // Collect valid pixel indices and their feature vectors
        let valid_indices: Vec<usize> = (0..n_pixels).filter(|&i| !nodata_mask[i]).collect();

        if valid_indices.len() < n_clusters {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Not enough valid pixels for the number of clusters",
            ));
        }

        // Initialize centroids using k-means++ style: pick first, then farthest
        let mut centroids: Vec<Vec<f64>> = Vec::with_capacity(n_clusters);

        // First centroid: pick from evenly spaced valid pixels
        let first_idx = valid_indices[0];
        centroids.push((0..n_bands).map(|b| band_data[b][first_idx]).collect());

        // Remaining centroids: pick the point farthest from all existing centroids
        for _ in 1..n_clusters {
            let mut max_dist = f64::NEG_INFINITY;
            let mut best_idx = valid_indices[0];
            for &vi in &valid_indices {
                let min_dist_to_centroid = centroids
                    .iter()
                    .map(|c| {
                        (0..n_bands)
                            .map(|b| {
                                let diff = band_data[b][vi] - c[b];
                                diff * diff
                            })
                            .sum::<f64>()
                    })
                    .fold(f64::INFINITY, f64::min);
                if min_dist_to_centroid > max_dist {
                    max_dist = min_dist_to_centroid;
                    best_idx = vi;
                }
            }
            centroids.push((0..n_bands).map(|b| band_data[b][best_idx]).collect());
        }

        // K-means iteration
        let mut labels = vec![0_usize; n_pixels];

        for _iter in 0..max_iter {
            let mut changed = false;

            // Assignment step: assign each valid pixel to nearest centroid
            for &vi in &valid_indices {
                let pixel: Vec<f64> = (0..n_bands).map(|b| band_data[b][vi]).collect();
                let mut best_cluster = 0;
                let mut best_dist = f64::INFINITY;
                for (c_idx, centroid) in centroids.iter().enumerate() {
                    let dist: f64 = pixel
                        .iter()
                        .zip(centroid.iter())
                        .map(|(&p, &c)| (p - c) * (p - c))
                        .sum();
                    if dist < best_dist {
                        best_dist = dist;
                        best_cluster = c_idx;
                    }
                }
                if labels[vi] != best_cluster {
                    labels[vi] = best_cluster;
                    changed = true;
                }
            }

            if !changed {
                break;
            }

            // Update step: recalculate centroids
            let mut sums = vec![vec![0.0_f64; n_bands]; n_clusters];
            let mut counts = vec![0_usize; n_clusters];

            for &vi in &valid_indices {
                let cluster = labels[vi];
                counts[cluster] += 1;
                for b in 0..n_bands {
                    sums[cluster][b] += band_data[b][vi];
                }
            }

            let mut max_movement = 0.0_f64;
            for c in 0..n_clusters {
                if counts[c] > 0 {
                    let new_centroid: Vec<f64> = (0..n_bands)
                        .map(|b| sums[c][b] / counts[c] as f64)
                        .collect();
                    let movement: f64 = new_centroid
                        .iter()
                        .zip(centroids[c].iter())
                        .map(|(&a, &b)| (a - b) * (a - b))
                        .sum::<f64>()
                        .sqrt();
                    if movement > max_movement {
                        max_movement = movement;
                    }
                    centroids[c] = new_centroid;
                }
            }

            if max_movement < tolerance {
                break;
            }
        }

        // Build output as i64 labels
        let mut result_labels = vec![vec![0_i64; cols]; rows];
        for (y, row) in result_labels.iter_mut().enumerate() {
            for (x, cell) in row.iter_mut().enumerate() {
                let idx = y * cols + x;
                if nodata_mask[idx] {
                    *cell = -1; // nodata pixels get -1
                } else {
                    *cell = labels[idx] as i64;
                }
            }
        }

        Ok(result_labels)
    })?;

    numpy::PyArray2::from_vec2(py, &result_labels).map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to create result array: {}", e))
    })
}
