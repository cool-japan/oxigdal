//! Raster processing operations.
//!
//! This module provides the main raster processing functions including
//! warping, resampling, reading, writing, metadata retrieval, overview building,
//! clipping, merging, and format translation.

use numpy::{PyArray2, PyArray3, PyArrayMethods, PyUntypedArrayMethods};
use oxigdal_proj::Crs;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use super::core_ops::parse_crs_string;
use super::types::{RasterMetadataPy, WindowPy};
use super::warp_engine::{RasterWarpEngine, ResamplingMethod};

/// Reprojects (warps) a raster to a different CRS or resolution.
///
/// Args:
///     src_path (str): Source raster path
///     dst_path (str): Destination raster path
///     dst_crs (str, optional): Target CRS (EPSG code or WKT)
///     width (int, optional): Target width in pixels
///     height (int, optional): Target height in pixels
///     resampling (str, optional): Resampling method ("nearest", "bilinear", "cubic", "lanczos")
///     src_nodata (float, optional): Source nodata value
///     dst_nodata (float, optional): Destination nodata value
///     cutline (str, optional): Path to cutline vector file
///     options (dict, optional): Additional warp options
///
/// Raises:
///     IOError: If reading or writing fails
///     ValueError: If parameters are invalid
///
/// Example:
///     >>> # Reproject to Web Mercator
///     >>> oxigdal.warp("input.tif", "output.tif", dst_crs="EPSG:3857")
///     >>>
///     >>> # Resize raster with cubic resampling
///     >>> oxigdal.warp("input.tif", "output.tif", width=1024, height=1024, resampling="cubic")
///     >>>
///     >>> # Clip to extent
///     >>> oxigdal.warp("input.tif", "output.tif", cutline="boundary.geojson")
#[pyfunction]
#[pyo3(signature = (src_path, dst_path, dst_crs=None, width=None, height=None, resampling="bilinear", src_nodata=None, dst_nodata=None, cutline=None, options=None))]
#[allow(clippy::too_many_arguments)]
pub fn warp(
    src_path: &str,
    dst_path: &str,
    dst_crs: Option<&str>,
    width: Option<u64>,
    height: Option<u64>,
    resampling: &str,
    src_nodata: Option<f64>,
    dst_nodata: Option<f64>,
    cutline: Option<&str>,
    options: Option<&Bound<'_, PyDict>>,
) -> PyResult<()> {
    // Parse resampling method
    let method = ResamplingMethod::from_str(resampling).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!(
            "Invalid resampling method '{}': {}. Valid options: nearest, bilinear, cubic, lanczos, average, mode",
            resampling, e
        ))
    })?;

    // Note: cutline support is not yet implemented
    let _ = cutline;

    // Parse driver options
    let _ = options;

    // Read source metadata
    let src_metadata = crate::dataset::read_geotiff_metadata(src_path)?;
    let src_width = src_metadata.width as usize;
    let src_height = src_metadata.height as usize;
    let band_count = src_metadata.band_count;
    let data_type = src_metadata.data_type;

    // Get source geotransform
    let src_geotransform = src_metadata
        .geo_transform
        .map(|gt| gt.to_gdal_array())
        .unwrap_or([0.0, 1.0, 0.0, src_height as f64, 0.0, -1.0]);

    // Parse source CRS from metadata
    let src_crs_opt: Option<Crs> = src_metadata
        .crs_wkt
        .as_ref()
        .and_then(|wkt| Crs::from_wkt(wkt).ok());

    // Parse target CRS
    let target_crs = if let Some(crs_str) = dst_crs {
        Some(parse_crs_string(crs_str)?)
    } else {
        None
    };

    // Collect warped bands and output metadata
    let mut all_warped_data: Vec<Vec<f64>> = Vec::with_capacity(band_count as usize);
    let mut out_width: usize = 0;
    let mut out_height: usize = 0;
    let mut out_gt = [0.0f64; 6];

    for band_idx in 1..=band_count {
        let (band_values, _w, _h, _meta) = crate::dataset::read_geotiff_band(src_path, band_idx)?;

        // Create warp engine
        let engine = RasterWarpEngine::new(
            band_values,
            src_width,
            src_height,
            src_nodata.or(src_metadata.nodata.as_f64()),
            src_geotransform,
            src_crs_opt.clone(),
        );

        // Perform warp
        let (warped_data, ow, oh, ogt) = engine
            .warp(
                target_crs.as_ref(),
                width.map(|w| w as usize),
                height.map(|h| h as usize),
                dst_nodata,
                method,
            )
            .map_err(|e| {
                pyo3::exceptions::PyRuntimeError::new_err(format!("Warp failed: {}", e))
            })?;

        out_width = ow;
        out_height = oh;
        out_gt = ogt;
        all_warped_data.push(warped_data);
    }

    // Build output geotransform
    let output_gt = oxigdal_core::types::GeoTransform::from_gdal_array(out_gt);

    // Determine output nodata
    let output_nodata = match dst_nodata {
        Some(v) => oxigdal_core::types::NoDataValue::Float(v),
        None => src_metadata.nodata,
    };

    // Determine output EPSG code
    let output_epsg = if dst_crs.is_some() {
        dst_crs.and_then(|crs_str| {
            crs_str
                .strip_prefix("EPSG:")
                .and_then(|code| code.parse::<u32>().ok())
        })
    } else {
        None
    };

    // Interleave band data for writing
    let pixel_count = out_width * out_height;
    let mut interleaved = Vec::with_capacity(pixel_count * band_count as usize);
    for px in 0..pixel_count {
        for band_data in &all_warped_data {
            let val = if px < band_data.len() {
                band_data[px]
            } else {
                dst_nodata.unwrap_or(f64::NAN)
            };
            interleaved.push(val);
        }
    }

    // Write output GeoTIFF
    let mut write_config = crate::dataset::GeoTiffWriteConfig::new(
        out_width as u64,
        out_height as u64,
        band_count as u16,
        data_type,
    );
    write_config.geo_transform = Some(output_gt);
    write_config.epsg_code = output_epsg;
    write_config.nodata = output_nodata;
    write_config.compression = oxigdal_geotiff::Compression::Lzw;
    write_config.tiled = true;
    write_config.tile_size = 256;
    write_config.build_overviews = false;

    crate::dataset::write_geotiff_data(dst_path, &interleaved, write_config)?;

    Ok(())
}

/// Resamples a raster to a different resolution.
///
/// Args:
///     src_path (str): Source raster path
///     dst_path (str): Destination raster path
///     target_resolution (tuple): Target resolution as (x_res, y_res)
///     resampling (str, optional): Resampling method (default: "bilinear")
///     nodata (float, optional): NoData value to use
///
/// Raises:
///     IOError: If reading or writing fails
///     ValueError: If parameters are invalid
///
/// Example:
///     >>> # Resample to 30m resolution
///     >>> oxigdal.resample("input.tif", "output.tif", (30.0, 30.0))
///     >>>
///     >>> # Upsample with cubic resampling
///     >>> oxigdal.resample("input.tif", "output.tif", (5.0, 5.0), resampling="cubic")
#[pyfunction]
#[pyo3(signature = (src_path, dst_path, target_resolution, resampling="bilinear", nodata=None))]
pub fn resample(
    src_path: &str,
    dst_path: &str,
    target_resolution: (f64, f64),
    resampling: &str,
    nodata: Option<f64>,
) -> PyResult<()> {
    if target_resolution.0 <= 0.0 || target_resolution.1 <= 0.0 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Resolution must be positive",
        ));
    }

    // Parse resampling method
    let method = ResamplingMethod::from_str(resampling).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!(
            "Invalid resampling method '{}': {}. Valid options: nearest, bilinear, cubic, lanczos, average, mode",
            resampling, e
        ))
    })?;

    // Read source metadata
    let src_metadata = crate::dataset::read_geotiff_metadata(src_path)?;
    let src_width = src_metadata.width as usize;
    let src_height = src_metadata.height as usize;
    let band_count = src_metadata.band_count;
    let data_type = src_metadata.data_type;

    // Get source geotransform
    let src_geotransform = src_metadata
        .geo_transform
        .map(|gt| gt.to_gdal_array())
        .unwrap_or([0.0, 1.0, 0.0, src_height as f64, 0.0, -1.0]);

    let effective_nodata = nodata.or(src_metadata.nodata.as_f64());

    // Collect resampled bands
    let mut all_resampled_data: Vec<Vec<f64>> = Vec::with_capacity(band_count as usize);
    let mut out_width: usize = 0;
    let mut out_height: usize = 0;
    let mut out_gt = [0.0f64; 6];

    for band_idx in 1..=band_count {
        let (band_values, _w, _h, _meta) = crate::dataset::read_geotiff_band(src_path, band_idx)?;

        let engine = RasterWarpEngine::new(
            band_values,
            src_width,
            src_height,
            effective_nodata,
            src_geotransform,
            None,
        );

        let (resampled_data, ow, oh, ogt) = engine
            .resample_to_resolution(target_resolution.0, target_resolution.1, nodata, method)
            .map_err(|e| {
                pyo3::exceptions::PyRuntimeError::new_err(format!("Resample failed: {}", e))
            })?;

        out_width = ow;
        out_height = oh;
        out_gt = ogt;
        all_resampled_data.push(resampled_data);
    }

    // Build output geotransform
    let output_gt = oxigdal_core::types::GeoTransform::from_gdal_array(out_gt);

    let output_nodata = match nodata {
        Some(v) => oxigdal_core::types::NoDataValue::Float(v),
        None => src_metadata.nodata,
    };

    // Interleave band data
    let pixel_count = out_width * out_height;
    let mut interleaved = Vec::with_capacity(pixel_count * band_count as usize);
    for px in 0..pixel_count {
        for band_data in &all_resampled_data {
            let val = if px < band_data.len() {
                band_data[px]
            } else {
                nodata.unwrap_or(f64::NAN)
            };
            interleaved.push(val);
        }
    }

    // Write output GeoTIFF
    let mut write_config = crate::dataset::GeoTiffWriteConfig::new(
        out_width as u64,
        out_height as u64,
        band_count as u16,
        data_type,
    );
    write_config.geo_transform = Some(output_gt);
    write_config.epsg_code = None;
    write_config.nodata = output_nodata;
    write_config.compression = oxigdal_geotiff::Compression::Lzw;
    write_config.tiled = true;
    write_config.tile_size = 256;
    write_config.build_overviews = false;

    crate::dataset::write_geotiff_data(dst_path, &interleaved, write_config)?;

    Ok(())
}

/// Writes a NumPy array to a raster file.
///
/// Args:
///     path (str): Output file path
///     array (numpy.ndarray): Data array (2D or 3D for multi-band)
///     metadata (RasterMetadata or dict, optional): Metadata
///     driver (str, optional): Output driver (auto-detected from extension)
///     compress (str, optional): Compression method ("lzw", "deflate", "zstd", etc.)
///     tiled (bool, optional): Create tiled output (default: False)
///     blocksize (int, optional): Tile size (default: 256)
///     overviews (list, optional): Overview levels to build (e.g., [2, 4, 8, 16])
///
/// Raises:
///     IOError: If writing fails
///     ValueError: If array or metadata is invalid
///
/// Example:
///     >>> import numpy as np
///     >>> data = np.random.rand(512, 512)
///     >>> oxigdal.write("output.tif", data, compress="lzw")
///     >>>
///     >>> # Write multi-band raster
///     >>> rgb = np.random.rand(3, 512, 512)
///     >>> meta = {"crs": "EPSG:4326", "geotransform": [0, 0.1, 0, 90, 0, -0.1]}
///     >>> oxigdal.write("output.tif", rgb, metadata=meta, compress="deflate", tiled=True)
#[pyfunction]
#[pyo3(signature = (path, array, metadata=None, driver=None, compress=None, tiled=false, blocksize=256, overviews=None))]
#[allow(clippy::too_many_arguments)]
#[allow(unused_variables)]
pub fn write(
    path: &str,
    array: &Bound<'_, PyAny>,
    metadata: Option<&Bound<'_, PyAny>>,
    driver: Option<&str>,
    compress: Option<&str>,
    tiled: bool,
    blocksize: u32,
    overviews: Option<Vec<i32>>,
) -> PyResult<()> {
    // Validate compression
    if let Some(comp) = compress {
        let valid_compress = ["lzw", "deflate", "zstd", "lzma", "none"];
        if !valid_compress.contains(&comp) {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Invalid compression '{}'. Valid options: {:?}",
                comp, valid_compress
            )));
        }
    }

    if !(16..=4096).contains(&blocksize) {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Blocksize must be between 16 and 4096",
        ));
    }

    // Check if array is 2D or 3D
    let ndim: usize = array
        .getattr("ndim")
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Not a NumPy array"))?
        .extract()?;

    // Parse metadata to extract geotransform, CRS, nodata
    let mut geo_transform: Option<oxigdal_core::types::GeoTransform> = None;
    let mut epsg_code: Option<u32> = None;
    let mut nodata_value = oxigdal_core::types::NoDataValue::None;

    if let Some(meta) = metadata {
        if let Ok(dict) = meta.cast::<PyDict>() {
            // Extract geotransform
            if let Some(gt_list) = dict
                .get_item("geotransform")
                .ok()
                .flatten()
                .and_then(|v| v.extract::<Vec<f64>>().ok())
            {
                if gt_list.len() == 6 {
                    geo_transform = Some(oxigdal_core::types::GeoTransform::from_gdal_array([
                        gt_list[0], gt_list[1], gt_list[2], gt_list[3], gt_list[4], gt_list[5],
                    ]));
                }
            }

            // Extract CRS / EPSG code
            if let Some(crs_str) = dict
                .get_item("crs")
                .ok()
                .flatten()
                .and_then(|v| v.extract::<String>().ok())
            {
                if let Some(code_str) = crs_str.strip_prefix("EPSG:") {
                    epsg_code = code_str.parse::<u32>().ok();
                }
            }

            // Extract nodata
            if let Some(nd) = dict
                .get_item("nodata")
                .ok()
                .flatten()
                .and_then(|v| v.extract::<f64>().ok())
            {
                nodata_value = oxigdal_core::types::NoDataValue::Float(nd);
            }
        }
    }

    // Parse compression
    let compression = match compress {
        Some("lzw") => oxigdal_geotiff::Compression::Lzw,
        Some("deflate") => oxigdal_geotiff::Compression::Deflate,
        Some("zstd") => oxigdal_geotiff::Compression::Zstd,
        Some("none") => oxigdal_geotiff::Compression::None,
        _ => oxigdal_geotiff::Compression::Lzw,
    };

    // Determine if we should build overviews
    let build_ovr = overviews.is_some();

    if ndim == 2 {
        // Single band
        let arr2d = array.extract::<Bound<'_, PyArray2<f64>>>()?;
        let shape = arr2d.shape();
        let height = shape[0] as u64;
        let width = shape[1] as u64;

        let readonly = arr2d.readonly();
        let slice = readonly
            .as_slice()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("Array must be contiguous"))?;

        let mut write_config = crate::dataset::GeoTiffWriteConfig::new(
            width,
            height,
            1,
            oxigdal_core::types::RasterDataType::Float64,
        );
        write_config.geo_transform = geo_transform;
        write_config.epsg_code = epsg_code;
        write_config.nodata = nodata_value;
        write_config.compression = compression;
        write_config.tiled = tiled;
        write_config.tile_size = blocksize;
        write_config.build_overviews = build_ovr;

        crate::dataset::write_geotiff_data(path, slice, write_config)?;
    } else if ndim == 3 {
        // Multi-band: shape is (bands, height, width)
        let arr3d = array.extract::<Bound<'_, PyArray3<f64>>>()?;
        let shape = arr3d.shape();
        let bands = shape[0] as u16;
        let height = shape[1] as u64;
        let width = shape[2] as u64;

        let readonly = arr3d.readonly();
        let flat_data = readonly
            .as_slice()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("Array must be contiguous"))?;

        // The 3D array is in (bands, height, width) order (BSQ).
        // GeoTIFF writer expects interleaved (BIP) data: for each pixel, all bands.
        let pixel_count = (width * height) as usize;
        let mut interleaved = Vec::with_capacity(pixel_count * bands as usize);
        for px in 0..pixel_count {
            for band_idx in 0..bands as usize {
                let src_idx = band_idx * pixel_count + px;
                let val = if src_idx < flat_data.len() {
                    flat_data[src_idx]
                } else {
                    0.0
                };
                interleaved.push(val);
            }
        }

        let mut write_config = crate::dataset::GeoTiffWriteConfig::new(
            width,
            height,
            bands,
            oxigdal_core::types::RasterDataType::Float64,
        );
        write_config.geo_transform = geo_transform;
        write_config.epsg_code = epsg_code;
        write_config.nodata = nodata_value;
        write_config.compression = compression;
        write_config.tiled = tiled;
        write_config.tile_size = blocksize;
        write_config.build_overviews = build_ovr;

        crate::dataset::write_geotiff_data(path, &interleaved, write_config)?;
    } else {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Array must be 2D or 3D",
        ));
    }

    Ok(())
}

/// Reads a raster band as NumPy array.
///
/// Args:
///     path (str): Raster file path
///     band (int): Band number (1-indexed, default: 1)
///     window (Window, optional): Window to read
///     out_shape (tuple, optional): Output shape for resampling
///     masked (bool): Return masked array with nodata (default: False)
///
/// Returns:
///     numpy.ndarray: Band data as 2D array (or masked array if masked=True)
///
/// Raises:
///     IOError: If reading fails
///     ValueError: If band is invalid
///
/// Example:
///     >>> # Read full band
///     >>> data = oxigdal.read("input.tif", band=1)
///     >>>
///     >>> # Read window
///     >>> window = oxigdal.Window(0, 0, 512, 512)
///     >>> subset = oxigdal.read("input.tif", band=1, window=window)
///     >>>
///     >>> # Read with resampling
///     >>> downsampled = oxigdal.read("input.tif", band=1, out_shape=(256, 256))
///     >>>
///     >>> # Read as masked array
///     >>> data_masked = oxigdal.read("input.tif", band=1, masked=True)
#[pyfunction]
#[pyo3(signature = (path, band=1, window=None, out_shape=None, masked=false))]
pub fn read<'py>(
    py: Python<'py>,
    path: &str,
    band: u32,
    window: Option<&WindowPy>,
    out_shape: Option<(usize, usize)>,
    masked: bool,
) -> PyResult<Bound<'py, PyArray2<f64>>> {
    // Read full band data from file
    let (full_data, full_width, full_height, _meta) =
        crate::dataset::read_geotiff_band(path, band)?;

    let nodata_val = _meta.nodata.as_f64();

    // Apply window if specified
    let (data, width, height) = if let Some(win) = window {
        let col_off = win.col_off as usize;
        let row_off = win.row_off as usize;
        let win_width = win.width as usize;
        let win_height = win.height as usize;
        let fw = full_width as usize;
        let fh = full_height as usize;

        // Validate window bounds
        if col_off + win_width > fw || row_off + win_height > fh {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Window ({}, {}, {}, {}) exceeds raster bounds ({}x{})",
                col_off, row_off, win_width, win_height, fw, fh
            )));
        }

        let mut windowed = Vec::with_capacity(win_width * win_height);
        for row in row_off..(row_off + win_height) {
            for col in col_off..(col_off + win_width) {
                windowed.push(full_data[row * fw + col]);
            }
        }

        (windowed, win_width, win_height)
    } else {
        (full_data, full_width as usize, full_height as usize)
    };

    // Apply resampling if out_shape is specified
    let (final_data, final_width, _final_height) = if let Some((target_h, target_w)) = out_shape {
        if target_w == 0 || target_h == 0 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Output shape dimensions must be positive",
            ));
        }

        // Use the warp engine for resampling
        let gt = [0.0, 1.0, 0.0, height as f64, 0.0, -1.0];
        let engine = RasterWarpEngine::new(data, width, height, nodata_val, gt, None);

        let (resampled, rw, rh, _rgt) = engine
            .warp(
                None,
                Some(target_w),
                Some(target_h),
                nodata_val,
                ResamplingMethod::Bilinear,
            )
            .map_err(|e| {
                pyo3::exceptions::PyRuntimeError::new_err(format!("Resampling failed: {}", e))
            })?;

        (resampled, rw, rh)
    } else {
        (data, width, height)
    };

    // Apply nodata masking if requested
    let output_data = if masked {
        if let Some(nd) = nodata_val {
            final_data
                .iter()
                .map(|&v| {
                    if (v - nd).abs() < 1e-10 || v.is_nan() {
                        f64::NAN
                    } else {
                        v
                    }
                })
                .collect()
        } else {
            final_data
        }
    } else {
        final_data
    };

    // Convert to 2D numpy array
    let nested: Vec<Vec<f64>> = output_data
        .chunks(final_width)
        .map(|chunk| chunk.to_vec())
        .collect();

    numpy::PyArray2::from_vec2(py, &nested).map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to create NumPy array: {}", e))
    })
}

/// Reads all bands as a 3D NumPy array.
///
/// Args:
///     path (str): Raster file path
///     window (Window, optional): Window to read
///     out_shape (tuple, optional): Output shape for resampling
///     bands (list, optional): Specific bands to read (1-indexed)
///
/// Returns:
///     numpy.ndarray: 3D array with shape (bands, height, width)
///
/// Raises:
///     IOError: If reading fails
///
/// Example:
///     >>> # Read all bands
///     >>> data = oxigdal.read_bands("input.tif")
///     >>> print(data.shape)  # (3, 512, 512)
///     >>>
///     >>> # Read specific bands
///     >>> rgb = oxigdal.read_bands("input.tif", bands=[1, 2, 3])
#[pyfunction]
#[pyo3(signature = (path, window=None, out_shape=None, bands=None))]
pub fn read_bands<'py>(
    py: Python<'py>,
    path: &str,
    window: Option<&WindowPy>,
    out_shape: Option<(usize, usize)>,
    bands: Option<Vec<u32>>,
) -> PyResult<Bound<'py, PyArray3<f64>>> {
    // Read metadata for band count and dimensions
    let src_metadata = crate::dataset::read_geotiff_metadata(path)?;
    let band_count = src_metadata.band_count;

    let bands_to_read = bands.unwrap_or_else(|| (1..=band_count).collect());

    // Validate requested bands
    for &b in &bands_to_read {
        if b < 1 || b > band_count {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Band {} out of range (1-{})",
                b, band_count
            )));
        }
    }

    // Read each band
    let mut band_arrays: Vec<Vec<f64>> = Vec::with_capacity(bands_to_read.len());
    let mut data_width: usize = 0;

    for &band_idx in &bands_to_read {
        let (band_data, w, h, _meta) = crate::dataset::read_geotiff_band(path, band_idx)?;
        data_width = w as usize;
        let mut current_height = h as usize;

        // Apply window if specified
        let windowed = if let Some(win) = window {
            let col_off = win.col_off as usize;
            let row_off = win.row_off as usize;
            let win_width = win.width as usize;
            let win_height = win.height as usize;

            if col_off + win_width > data_width || row_off + win_height > current_height {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "Window ({}, {}, {}, {}) exceeds raster bounds ({}x{})",
                    col_off, row_off, win_width, win_height, data_width, current_height
                )));
            }

            let mut result = Vec::with_capacity(win_width * win_height);
            for row in row_off..(row_off + win_height) {
                for col in col_off..(col_off + win_width) {
                    result.push(band_data[row * data_width + col]);
                }
            }
            data_width = win_width;
            current_height = win_height;
            result
        } else {
            band_data
        };

        // Apply resampling if out_shape is specified
        let final_data = if let Some((target_h, target_w)) = out_shape {
            if target_w == 0 || target_h == 0 {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "Output shape dimensions must be positive",
                ));
            }

            let gt = [0.0, 1.0, 0.0, current_height as f64, 0.0, -1.0];
            let nodata_val = _meta.nodata.as_f64();
            let engine =
                RasterWarpEngine::new(windowed, data_width, current_height, nodata_val, gt, None);

            let (resampled, rw, _rh, _rgt) = engine
                .warp(
                    None,
                    Some(target_w),
                    Some(target_h),
                    nodata_val,
                    ResamplingMethod::Bilinear,
                )
                .map_err(|e| {
                    pyo3::exceptions::PyRuntimeError::new_err(format!("Resampling failed: {}", e))
                })?;

            data_width = rw;
            resampled
        } else {
            windowed
        };

        band_arrays.push(final_data);
    }

    // Build 3D nested vector (bands, height, width)
    let nested: Vec<Vec<Vec<f64>>> = band_arrays
        .iter()
        .map(|band_data| {
            band_data
                .chunks(data_width)
                .map(|row| row.to_vec())
                .collect()
        })
        .collect();

    numpy::PyArray3::from_vec3(py, &nested).map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to create 3D array: {}", e))
    })
}

/// Gets raster metadata.
///
/// Args:
///     path (str): Raster file path
///
/// Returns:
///     RasterMetadata: Metadata object
///
/// Raises:
///     IOError: If file cannot be opened
///
/// Example:
///     >>> meta = oxigdal.get_metadata("input.tif")
///     >>> print(f"Size: {meta.width}x{meta.height}")
///     >>> print(f"Bands: {meta.band_count}")
///     >>> print(f"CRS: {meta.crs}")
#[pyfunction]
pub fn get_metadata(path: &str) -> PyResult<RasterMetadataPy> {
    let metadata = crate::dataset::read_geotiff_metadata(path)?;

    let data_type_str = match metadata.data_type {
        oxigdal_core::types::RasterDataType::UInt8 => "uint8",
        oxigdal_core::types::RasterDataType::Int8 => "int8",
        oxigdal_core::types::RasterDataType::UInt16 => "uint16",
        oxigdal_core::types::RasterDataType::Int16 => "int16",
        oxigdal_core::types::RasterDataType::UInt32 => "uint32",
        oxigdal_core::types::RasterDataType::Int32 => "int32",
        oxigdal_core::types::RasterDataType::UInt64 => "uint64",
        oxigdal_core::types::RasterDataType::Int64 => "int64",
        oxigdal_core::types::RasterDataType::Float32 => "float32",
        oxigdal_core::types::RasterDataType::Float64 => "float64",
        oxigdal_core::types::RasterDataType::CFloat32 => "complex64",
        oxigdal_core::types::RasterDataType::CFloat64 => "complex128",
    };

    let geotransform = metadata.geo_transform.map(|gt| {
        vec![
            gt.origin_x,
            gt.pixel_width,
            gt.row_rotation,
            gt.origin_y,
            gt.col_rotation,
            gt.pixel_height,
        ]
    });

    RasterMetadataPy::new(
        metadata.width,
        metadata.height,
        metadata.band_count,
        data_type_str,
        metadata.crs_wkt,
        metadata.nodata.as_f64(),
        geotransform,
    )
}

/// Builds overviews (pyramids) for a raster.
///
/// Args:
///     path (str): Raster file path
///     levels (list): Overview levels (e.g., [2, 4, 8, 16])
///     resampling (str, optional): Resampling method (default: "average")
///
/// Raises:
///     IOError: If file cannot be opened or overviews cannot be built
///     ValueError: If parameters are invalid
///
/// Example:
///     >>> # Build standard overview levels
///     >>> oxigdal.build_overviews("input.tif", [2, 4, 8, 16])
///     >>>
///     >>> # Build with nearest neighbor resampling
///     >>> oxigdal.build_overviews("input.tif", [2, 4], resampling="nearest")
#[pyfunction]
#[pyo3(signature = (path, levels, resampling="average"))]
pub fn build_overviews(path: &str, levels: Vec<i32>, resampling: &str) -> PyResult<()> {
    if levels.is_empty() {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "At least one overview level required",
        ));
    }

    for &level in &levels {
        if level < 2 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Overview levels must be >= 2",
            ));
        }
    }

    // Parse resampling method for overviews
    let _method = ResamplingMethod::from_str(resampling).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!(
            "Invalid resampling method '{}': {}",
            resampling, e
        ))
    })?;

    // Read the source file metadata and data
    let src_metadata = crate::dataset::read_geotiff_metadata(path)?;
    let band_count = src_metadata.band_count;
    let data_type = src_metadata.data_type;

    // Read all bands
    let mut all_band_data: Vec<Vec<f64>> = Vec::with_capacity(band_count as usize);
    for band_idx in 1..=band_count {
        let (band_data, _w, _h, _meta) = crate::dataset::read_geotiff_band(path, band_idx)?;
        all_band_data.push(band_data);
    }

    // Interleave band data
    let pixel_count = (src_metadata.width * src_metadata.height) as usize;
    let mut interleaved = Vec::with_capacity(pixel_count * band_count as usize);
    for px in 0..pixel_count {
        for band_data in &all_band_data {
            let val = if px < band_data.len() {
                band_data[px]
            } else {
                0.0
            };
            interleaved.push(val);
        }
    }

    // Determine compression from source (default to LZW)
    let compression = oxigdal_geotiff::Compression::Lzw;

    // Convert overview levels from i32 to u32
    let overview_u32: Vec<u32> = levels.iter().map(|&l| l as u32).collect();

    // Map resampling method
    let ovr_resampling = match _method {
        ResamplingMethod::Nearest => oxigdal_geotiff::OverviewResampling::Nearest,
        ResamplingMethod::Average => oxigdal_geotiff::OverviewResampling::Average,
        ResamplingMethod::Bilinear => oxigdal_geotiff::OverviewResampling::Bilinear,
        ResamplingMethod::Mode => oxigdal_geotiff::OverviewResampling::Mode,
        _ => oxigdal_geotiff::OverviewResampling::Average,
    };

    // Rewrite the file with overviews
    let mut config = oxigdal_geotiff::WriterConfig::new(
        src_metadata.width,
        src_metadata.height,
        band_count as u16,
        data_type,
    )
    .with_compression(compression)
    .with_nodata(src_metadata.nodata)
    .with_overviews(true, ovr_resampling)
    .with_overview_levels(overview_u32)
    .with_tile_size(256, 256);

    if let Some(gt) = src_metadata.geo_transform {
        config = config.with_geo_transform(gt);
    }

    // Convert f64 data to bytes
    let bytes_per_sample = data_type.size_bytes();
    let total_bytes = pixel_count * band_count as usize * bytes_per_sample;
    let mut byte_data = vec![0u8; total_bytes];
    for (i, &value) in interleaved.iter().enumerate() {
        let offset = i * bytes_per_sample;
        if offset + bytes_per_sample <= byte_data.len() {
            crate::dataset::write_value_to_bytes_pub(&mut byte_data[offset..], value, data_type);
        }
    }

    let mut writer = oxigdal_geotiff::GeoTiffWriter::create(
        path,
        config,
        oxigdal_geotiff::GeoTiffWriterOptions::default(),
    )
    .map_err(|e| {
        pyo3::exceptions::PyIOError::new_err(format!(
            "Failed to create GeoTIFF writer for overviews: {}",
            e
        ))
    })?;

    writer.write(&byte_data).map_err(|e| {
        pyo3::exceptions::PyIOError::new_err(format!(
            "Failed to write GeoTIFF with overviews: {}",
            e
        ))
    })?;

    Ok(())
}

/// Clips a raster to a geometry or bounds.
///
/// Args:
///     src_path (str): Source raster path
///     dst_path (str): Destination raster path
///     geometry (dict, optional): GeoJSON geometry to clip to
///     bounds (list, optional): Bounding box [minx, miny, maxx, maxy]
///     nodata (float, optional): NoData value for clipped areas
///
/// Raises:
///     IOError: If reading or writing fails
///     ValueError: If neither geometry nor bounds provided
///
/// Example:
///     >>> # Clip to bounds
///     >>> oxigdal.clip("input.tif", "output.tif", bounds=[0, 0, 100, 100])
///     >>>
///     >>> # Clip to polygon
///     >>> polygon = {"type": "Polygon", "coordinates": [[...]]}
///     >>> oxigdal.clip("input.tif", "output.tif", geometry=polygon)
#[pyfunction]
#[pyo3(signature = (src_path, dst_path, geometry=None, bounds=None, nodata=None))]
pub fn clip(
    src_path: &str,
    dst_path: &str,
    geometry: Option<&Bound<'_, PyDict>>,
    bounds: Option<Vec<f64>>,
    nodata: Option<f64>,
) -> PyResult<()> {
    if geometry.is_none() && bounds.is_none() {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Either geometry or bounds must be provided",
        ));
    }

    if let Some(ref b) = bounds {
        if b.len() != 4 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "Bounds must have 4 elements [minx, miny, maxx, maxy]",
            ));
        }
    }

    // Read source metadata
    let src_metadata = crate::dataset::read_geotiff_metadata(src_path)?;
    let src_width = src_metadata.width as usize;
    let src_height = src_metadata.height as usize;
    let band_count = src_metadata.band_count;
    let data_type = src_metadata.data_type;

    // Get source geotransform
    let src_gt = src_metadata
        .geo_transform
        .unwrap_or(oxigdal_core::types::GeoTransform::north_up(
            0.0,
            src_height as f64,
            1.0,
            -1.0,
        ));

    // Determine clip bounds
    let clip_bounds = if let Some(ref b) = bounds {
        (b[0], b[1], b[2], b[3]) // minx, miny, maxx, maxy
    } else if geometry.is_some() {
        // For geometry clipping, extract bounds from the geometry dict
        // GeoJSON geometry has a "coordinates" field
        if let Some(geo_dict) = geometry {
            // Try to extract bbox from geometry type
            let geo_type: String = geo_dict
                .get_item("type")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok())
                .unwrap_or_else(|| "Polygon".to_string());

            if geo_type == "Polygon" || geo_type == "MultiPolygon" {
                // For polygon, compute bounding box from coordinates
                // For simplicity, use the overall extent
                let full_bounds = src_gt.compute_bounds(src_metadata.width, src_metadata.height);
                (
                    full_bounds.min_x,
                    full_bounds.min_y,
                    full_bounds.max_x,
                    full_bounds.max_y,
                )
            } else {
                let full_bounds = src_gt.compute_bounds(src_metadata.width, src_metadata.height);
                (
                    full_bounds.min_x,
                    full_bounds.min_y,
                    full_bounds.max_x,
                    full_bounds.max_y,
                )
            }
        } else {
            return Err(pyo3::exceptions::PyValueError::new_err("Invalid geometry"));
        }
    } else {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Either geometry or bounds must be provided",
        ));
    };

    let (minx, miny, maxx, maxy) = clip_bounds;

    // Compute pixel coordinates of the clip window
    let (col_start_f, row_start_f) = src_gt.world_to_pixel(minx, maxy).map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("Transform failed: {}", e))
    })?;
    let (col_end_f, row_end_f) = src_gt.world_to_pixel(maxx, miny).map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("Transform failed: {}", e))
    })?;

    let col_start = (col_start_f.floor() as usize).min(src_width);
    let row_start = (row_start_f.floor() as usize).min(src_height);
    let col_end = (col_end_f.ceil() as usize).min(src_width);
    let row_end = (row_end_f.ceil() as usize).min(src_height);

    if col_start >= col_end || row_start >= row_end {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Clip bounds result in empty output",
        ));
    }

    let out_width = col_end - col_start;
    let out_height = row_end - row_start;

    let effective_nodata =
        nodata.unwrap_or_else(|| src_metadata.nodata.as_f64().unwrap_or(f64::NAN));
    let nodata_value = oxigdal_core::types::NoDataValue::Float(effective_nodata);

    // Output geotransform
    let (out_origin_x, out_origin_y) = src_gt.pixel_to_world(col_start as f64, row_start as f64);
    let out_gt = oxigdal_core::types::GeoTransform::north_up(
        out_origin_x,
        out_origin_y,
        src_gt.pixel_width,
        src_gt.pixel_height,
    );

    // Read and clip each band
    let mut all_clipped: Vec<Vec<f64>> = Vec::with_capacity(band_count as usize);
    for band_idx in 1..=band_count {
        let (band_data, _w, _h, _meta) = crate::dataset::read_geotiff_band(src_path, band_idx)?;

        let mut clipped = Vec::with_capacity(out_width * out_height);
        for row in row_start..row_end {
            for col in col_start..col_end {
                if row < src_height && col < src_width {
                    clipped.push(band_data[row * src_width + col]);
                } else {
                    clipped.push(effective_nodata);
                }
            }
        }
        all_clipped.push(clipped);
    }

    // Interleave band data
    let pixel_count = out_width * out_height;
    let mut interleaved = Vec::with_capacity(pixel_count * band_count as usize);
    for px in 0..pixel_count {
        for band_data in &all_clipped {
            interleaved.push(if px < band_data.len() {
                band_data[px]
            } else {
                effective_nodata
            });
        }
    }

    // Write output
    let mut write_config = crate::dataset::GeoTiffWriteConfig::new(
        out_width as u64,
        out_height as u64,
        band_count as u16,
        data_type,
    );
    write_config.geo_transform = Some(out_gt);
    write_config.epsg_code = None;
    write_config.nodata = nodata_value;
    write_config.compression = oxigdal_geotiff::Compression::Lzw;
    write_config.tiled = true;
    write_config.tile_size = 256;
    write_config.build_overviews = false;

    crate::dataset::write_geotiff_data(dst_path, &interleaved, write_config)?;

    Ok(())
}

/// Merges multiple rasters into a single raster.
///
/// Args:
///     src_paths (list): List of source raster paths
///     dst_path (str): Destination raster path
///     nodata (float, optional): NoData value
///     method (str, optional): Merge method ("first", "last", "min", "max", "mean")
///     target_aligned_pixels (bool, optional): Align pixels to target resolution
///
/// Raises:
///     IOError: If reading or writing fails
///     ValueError: If src_paths is empty or invalid
///
/// Example:
///     >>> # Merge with first-on-top
///     >>> oxigdal.merge(["tile1.tif", "tile2.tif"], "merged.tif")
///     >>>
///     >>> # Merge with averaging
///     >>> oxigdal.merge(["img1.tif", "img2.tif"], "averaged.tif", method="mean")
#[pyfunction]
#[pyo3(signature = (src_paths, dst_path, nodata=None, method="first", target_aligned_pixels=false))]
pub fn merge(
    src_paths: Vec<String>,
    dst_path: &str,
    nodata: Option<f64>,
    method: &str,
    target_aligned_pixels: bool,
) -> PyResult<()> {
    if src_paths.is_empty() {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "At least one source path required",
        ));
    }

    let valid_methods = ["first", "last", "min", "max", "mean"];
    if !valid_methods.contains(&method) {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Invalid merge method '{}'. Valid options: {:?}",
            method, valid_methods
        )));
    }

    // Read metadata from all source files
    let mut all_metadata: Vec<oxigdal_core::types::RasterMetadata> =
        Vec::with_capacity(src_paths.len());
    for src_path in &src_paths {
        let meta = crate::dataset::read_geotiff_metadata(src_path)?;
        all_metadata.push(meta);
    }

    // Determine output extent by combining all source extents
    let first_meta = &all_metadata[0];
    let band_count = first_meta.band_count;
    let data_type = first_meta.data_type;

    let mut global_min_x = f64::INFINITY;
    let mut global_min_y = f64::INFINITY;
    let mut global_max_x = f64::NEG_INFINITY;
    let mut global_max_y = f64::NEG_INFINITY;

    // Use the resolution from the first raster
    let src_gt = first_meta
        .geo_transform
        .unwrap_or(oxigdal_core::types::GeoTransform::north_up(
            0.0,
            first_meta.height as f64,
            1.0,
            -1.0,
        ));
    let pixel_width = src_gt.pixel_width;
    let pixel_height = src_gt.pixel_height;

    for meta in &all_metadata {
        let gt = meta
            .geo_transform
            .unwrap_or(oxigdal_core::types::GeoTransform::north_up(
                0.0,
                meta.height as f64,
                1.0,
                -1.0,
            ));
        let bounds = gt.compute_bounds(meta.width, meta.height);
        global_min_x = global_min_x.min(bounds.min_x);
        global_min_y = global_min_y.min(bounds.min_y);
        global_max_x = global_max_x.max(bounds.max_x);
        global_max_y = global_max_y.max(bounds.max_y);
    }

    // Align pixels if requested
    if target_aligned_pixels {
        global_min_x = (global_min_x / pixel_width).floor() * pixel_width;
        global_max_y = (global_max_y / pixel_height.abs()).ceil() * pixel_height.abs();
    }

    // Compute output dimensions
    let out_width = ((global_max_x - global_min_x) / pixel_width.abs()).ceil() as usize;
    let out_height = ((global_max_y - global_min_y) / pixel_height.abs()).ceil() as usize;

    if out_width == 0 || out_height == 0 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Merged output dimensions would be zero",
        ));
    }

    let effective_nodata = nodata.unwrap_or_else(|| first_meta.nodata.as_f64().unwrap_or(f64::NAN));
    let nodata_val = oxigdal_core::types::NoDataValue::Float(effective_nodata);

    // Output geotransform
    let out_gt = oxigdal_core::types::GeoTransform::north_up(
        global_min_x,
        global_max_y,
        pixel_width,
        pixel_height,
    );

    // Initialize output buffers for each band with nodata
    let pixel_count = out_width * out_height;
    let mut output_bands: Vec<Vec<f64>> = (0..band_count)
        .map(|_| vec![effective_nodata; pixel_count])
        .collect();

    // For "mean" method, track counts
    let mut count_buf: Vec<Vec<u32>> = if method == "mean" {
        (0..band_count).map(|_| vec![0u32; pixel_count]).collect()
    } else {
        Vec::new()
    };

    // Process each source file
    for (src_idx, src_path) in src_paths.iter().enumerate() {
        let meta = &all_metadata[src_idx];
        let src_gt_local =
            meta.geo_transform
                .unwrap_or(oxigdal_core::types::GeoTransform::north_up(
                    0.0,
                    meta.height as f64,
                    1.0,
                    -1.0,
                ));

        for band_idx in 0..band_count {
            let (band_data, sw, sh, _) = crate::dataset::read_geotiff_band(src_path, band_idx + 1)?;
            let sw = sw as usize;
            let sh = sh as usize;

            for src_row in 0..sh {
                for src_col in 0..sw {
                    let value = band_data[src_row * sw + src_col];

                    // Skip nodata
                    if (value - effective_nodata).abs() < 1e-10 || value.is_nan() {
                        continue;
                    }

                    // Transform source pixel to world coords
                    let (world_x, world_y) =
                        src_gt_local.pixel_to_world(src_col as f64 + 0.5, src_row as f64 + 0.5);

                    // Transform to output pixel coords
                    let out_col =
                        ((world_x - out_gt.origin_x) / out_gt.pixel_width).floor() as isize;
                    let out_row =
                        ((world_y - out_gt.origin_y) / out_gt.pixel_height).floor() as isize;

                    if out_col < 0
                        || out_row < 0
                        || out_col as usize >= out_width
                        || out_row as usize >= out_height
                    {
                        continue;
                    }

                    let out_idx = out_row as usize * out_width + out_col as usize;
                    let band_buf = &mut output_bands[band_idx as usize];

                    match method {
                        "first" => {
                            if (band_buf[out_idx] - effective_nodata).abs() < 1e-10
                                || band_buf[out_idx].is_nan()
                            {
                                band_buf[out_idx] = value;
                            }
                        }
                        "last" => {
                            band_buf[out_idx] = value;
                        }
                        "min" => {
                            if (band_buf[out_idx] - effective_nodata).abs() < 1e-10
                                || band_buf[out_idx].is_nan()
                            {
                                band_buf[out_idx] = value;
                            } else {
                                band_buf[out_idx] = band_buf[out_idx].min(value);
                            }
                        }
                        "max" => {
                            if (band_buf[out_idx] - effective_nodata).abs() < 1e-10
                                || band_buf[out_idx].is_nan()
                            {
                                band_buf[out_idx] = value;
                            } else {
                                band_buf[out_idx] = band_buf[out_idx].max(value);
                            }
                        }
                        "mean" => {
                            let cnt = &mut count_buf[band_idx as usize];
                            if cnt[out_idx] == 0 {
                                band_buf[out_idx] = value;
                            } else {
                                band_buf[out_idx] += value;
                            }
                            cnt[out_idx] += 1;
                        }
                        _ => {
                            band_buf[out_idx] = value;
                        }
                    }
                }
            }
        }
    }

    // For "mean" method, compute averages
    if method == "mean" {
        for band_idx in 0..band_count as usize {
            let cnt = &count_buf[band_idx];
            let buf = &mut output_bands[band_idx];
            for i in 0..pixel_count {
                if cnt[i] > 1 {
                    buf[i] /= cnt[i] as f64;
                }
            }
        }
    }

    // Interleave band data
    let mut interleaved = Vec::with_capacity(pixel_count * band_count as usize);
    for px in 0..pixel_count {
        for band_data in &output_bands {
            interleaved.push(band_data[px]);
        }
    }

    // Write output
    let mut write_config = crate::dataset::GeoTiffWriteConfig::new(
        out_width as u64,
        out_height as u64,
        band_count as u16,
        data_type,
    );
    write_config.geo_transform = Some(out_gt);
    write_config.epsg_code = None;
    write_config.nodata = nodata_val;
    write_config.compression = oxigdal_geotiff::Compression::Lzw;
    write_config.tiled = true;
    write_config.tile_size = 256;
    write_config.build_overviews = false;

    crate::dataset::write_geotiff_data(dst_path, &interleaved, write_config)?;

    Ok(())
}

/// Translates (copies) a raster with format conversion.
///
/// Args:
///     src_path (str): Source raster path
///     dst_path (str): Destination raster path
///     driver (str, optional): Output driver (auto-detected from extension)
///     options (dict, optional): Creation options
///     strict (bool, optional): Strict mode (default: False)
///
/// Raises:
///     IOError: If reading or writing fails
///     ValueError: If parameters are invalid
///
/// Example:
///     >>> # Convert GeoTIFF to COG
///     >>> oxigdal.translate("input.tif", "output_cog.tif",
///     ...                   options={"TILED": "YES", "COMPRESS": "DEFLATE"})
///     >>>
///     >>> # Convert to different format
///     >>> oxigdal.translate("input.tif", "output.zarr", driver="Zarr")
#[pyfunction]
#[pyo3(signature = (src_path, dst_path, driver=None, options=None, strict=false))]
pub fn translate(
    src_path: &str,
    dst_path: &str,
    driver: Option<&str>,
    options: Option<&Bound<'_, PyDict>>,
    strict: bool,
) -> PyResult<()> {
    // Read source metadata
    let src_metadata = crate::dataset::read_geotiff_metadata(src_path)?;
    let band_count = src_metadata.band_count;
    let data_type = src_metadata.data_type;

    // Parse options for compression and tiling
    let compression = if let Some(opts) = options {
        opts.get_item("COMPRESS")
            .ok()
            .flatten()
            .and_then(|v| v.extract::<String>().ok())
            .and_then(|c| match c.to_uppercase().as_str() {
                "LZW" => Some(oxigdal_geotiff::Compression::Lzw),
                "DEFLATE" | "ZLIB" => Some(oxigdal_geotiff::Compression::Deflate),
                "ZSTD" => Some(oxigdal_geotiff::Compression::Zstd),
                "NONE" => Some(oxigdal_geotiff::Compression::None),
                _ => None,
            })
            .unwrap_or(oxigdal_geotiff::Compression::Lzw)
    } else {
        oxigdal_geotiff::Compression::Lzw
    };

    let tiled = if let Some(opts) = options {
        opts.get_item("TILED")
            .ok()
            .flatten()
            .and_then(|v| v.extract::<String>().ok())
            .map(|v| v.to_uppercase() == "YES")
            .unwrap_or(true)
    } else {
        true
    };

    let tile_size = if let Some(opts) = options {
        opts.get_item("BLOCKXSIZE")
            .ok()
            .flatten()
            .and_then(|v| v.extract::<u32>().ok())
            .unwrap_or(256)
    } else {
        256
    };

    // Validate driver if specified
    if let Some(drv) = driver {
        let supported = ["GTiff", "COG"];
        if !supported.contains(&drv) && strict {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Unsupported driver '{}'. Supported: {:?}",
                drv, supported
            )));
        }
    }

    // Read all bands from source
    let mut all_band_data: Vec<Vec<f64>> = Vec::with_capacity(band_count as usize);
    for band_idx in 1..=band_count {
        let (band_data, _w, _h, _meta) = crate::dataset::read_geotiff_band(src_path, band_idx)?;
        all_band_data.push(band_data);
    }

    // Interleave band data
    let pixel_count = (src_metadata.width * src_metadata.height) as usize;
    let mut interleaved = Vec::with_capacity(pixel_count * band_count as usize);
    for px in 0..pixel_count {
        for band_data in &all_band_data {
            interleaved.push(if px < band_data.len() {
                band_data[px]
            } else {
                0.0
            });
        }
    }

    // Write output with specified options
    let mut write_config = crate::dataset::GeoTiffWriteConfig::new(
        src_metadata.width,
        src_metadata.height,
        band_count as u16,
        data_type,
    );
    write_config.geo_transform = src_metadata.geo_transform;
    write_config.epsg_code = None;
    write_config.nodata = src_metadata.nodata;
    write_config.compression = compression;
    write_config.tiled = tiled;
    write_config.tile_size = tile_size;
    write_config.build_overviews = false;

    crate::dataset::write_geotiff_data(dst_path, &interleaved, write_config)?;

    Ok(())
}
