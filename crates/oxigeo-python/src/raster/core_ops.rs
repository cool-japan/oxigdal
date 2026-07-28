//! Core raster operations: open, create, calculator, and CRS parsing.
//!
//! This module provides the fundamental raster I/O functions and the
//! expression-based raster calculator.

use numpy::{PyArray2, PyArrayMethods, PyUntypedArrayMethods};
use oxigeo_proj::Crs;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::HashMap;

use crate::dataset::{Dataset, DatasetCreateConfig};
use crate::expression::{Evaluator, parse_expression};

/// Opens a raster file.
///
/// Args:
///     path (str): Path to the raster file (local or remote URL)
///     mode (str, optional): Open mode - "r" for read (default), "r+" for read/write
///     driver (str, optional): Specific driver to use (auto-detected if None)
///     options (dict, optional): Driver-specific options. For remote URLs
///         (s3://, gs://, az://, http(s)://), recognized keys are honored as
///         real cloud-auth configuration: "AWS_REGION"/"AWS_DEFAULT_REGION",
///         "AWS_ENDPOINT_URL"/"AWS_S3_ENDPOINT" (S3-compatible endpoints such
///         as MinIO), "AWS_ACCESS_KEY_ID"/"AWS_SECRET_ACCESS_KEY"/
///         "AWS_SESSION_TOKEN" (S3), and "HTTP_BEARER_TOKEN"/
///         "HTTP_BASIC_USER"+"HTTP_BASIC_PASSWORD"/"HEADER_<Name>" (HTTP).
///         "AWS_NO_SIGN_REQUEST" (truly unsigned S3 access) is not yet
///         supported and raises NotImplementedError rather than silently
///         falling back to signed/ambient credentials.
///
/// Returns:
///     Dataset: Opened dataset
///
/// Raises:
///     IOError: If file cannot be opened
///     ValueError: If driver is not supported
///     NotImplementedError: If the "VRT" driver is requested (not yet wired
///         into the Python Dataset reader), or a remote URL is opened in a
///         build without the `cloud` Cargo feature, or unsigned S3 access is
///         requested without oxigeo-cloud support for it
///
/// Example:
///     >>> ds = oxigeo.open_raster("input.tif")
///     >>> data = ds.read_band(1)
///     >>>
///     >>> # Open a public/credentialed object straight from S3 (requires
///     >>> # oxigeo-python built with `--features cloud`)
///     >>> ds = oxigeo.open_raster("s3://bucket/file.tif", driver="GTiff",
///     ...     options={"AWS_ACCESS_KEY_ID": "...", "AWS_SECRET_ACCESS_KEY": "..."})
#[pyfunction]
#[pyo3(signature = (path, mode="r", driver=None, options=None))]
pub fn open_raster(
    py: Python<'_>,
    path: &str,
    mode: &str,
    driver: Option<&str>,
    options: Option<&Bound<'_, PyDict>>,
) -> PyResult<Dataset> {
    // Validate mode
    let valid_modes = ["r", "r+", "w"];
    if !valid_modes.contains(&mode) {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Invalid mode '{}'. Valid options: {:?}",
            mode, valid_modes
        )));
    }

    // Parse driver-specific / cloud-auth options if provided. These are
    // actually consumed below (remote URL credentials), not dead data.
    let mut parsed_options: HashMap<String, String> = HashMap::new();
    if let Some(opts) = options {
        for (key, value) in opts {
            let key_str: String = key.extract().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err("Option key must be string")
            })?;
            let val_str: String = value.extract().map_err(|_| {
                pyo3::exceptions::PyValueError::new_err("Option value must be string")
            })?;
            parsed_options.insert(key_str, val_str);
        }
    }

    // Validate driver if specified
    if let Some(drv) = driver {
        let supported_drivers = ["GTiff", "COG", "VRT"];
        if !supported_drivers.contains(&drv) {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Unsupported driver '{}'. Supported: {:?}",
                drv, supported_drivers
            )));
        }
    }

    let path_owned = path.to_string();
    let mode_owned = mode.to_string();
    let driver_owned = driver.map(|d| d.to_string());

    // Opening for read eagerly loads and parses file metadata -- for local
    // files this is blocking disk I/O, and for remote URLs it is a
    // synchronous network fetch (see `crate::remote`); release the GIL
    // while either runs.
    py.detach(|| {
        Dataset::open_with_driver_options(
            &path_owned,
            &mode_owned,
            driver_owned.as_deref(),
            &parsed_options,
        )
    })
}

/// Creates a new raster file.
///
/// Args:
///     path (str): Output path
///     width (int): Width in pixels
///     height (int): Height in pixels
///     bands (int): Number of bands (default: 1)
///     dtype (str): Data type (default: "float32")
///     crs (str, optional): CRS as WKT or EPSG code
///     nodata (float, optional): NoData value
///     geotransform (list, optional): GeoTransform as [x_min, pixel_width, 0, y_max, 0, -pixel_height]
///     driver (str, optional): Output driver (auto-detected from extension if None)
///     options (dict, optional): Driver-specific creation options
///
/// Returns:
///     Dataset: Created dataset opened for writing
///
/// Raises:
///     IOError: If file cannot be created
///     ValueError: If parameters are invalid
///
/// Example:
///     >>> ds = oxigeo.create_raster("output.tif", 512, 512, bands=3, dtype="uint8")
///     >>> ds.write_band(1, red_data)
///     >>> ds.write_band(2, green_data)
///     >>> ds.write_band(3, blue_data)
///     >>> ds.close()
///     >>>
///     >>> # Create with geotransform
///     >>> gt = [0.0, 1.0, 0.0, 100.0, 0.0, -1.0]
///     >>> ds = oxigeo.create_raster("output.tif", 100, 100, geotransform=gt, crs="EPSG:4326")
#[pyfunction]
#[pyo3(signature = (path, width, height, bands=1, dtype="float32", crs=None, nodata=None, geotransform=None, driver=None, options=None))]
#[allow(clippy::too_many_arguments)]
pub fn create_raster(
    path: &str,
    width: u64,
    height: u64,
    bands: u32,
    dtype: &str,
    crs: Option<&str>,
    nodata: Option<f64>,
    geotransform: Option<Vec<f64>>,
    driver: Option<&str>,
    options: Option<&Bound<'_, PyDict>>,
) -> PyResult<Dataset> {
    // Validate parameters
    if width == 0 || height == 0 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Width and height must be positive",
        ));
    }

    if bands == 0 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Band count must be positive",
        ));
    }

    if let Some(ref gt) = geotransform
        && gt.len() != 6
    {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "GeoTransform must have 6 elements",
        ));
    }

    // Validate dtype
    let valid_dtypes = [
        "uint8",
        "int8",
        "uint16",
        "int16",
        "uint32",
        "int32",
        "uint64",
        "int64",
        "float32",
        "float64",
        "complex64",
        "complex128",
    ];
    if !valid_dtypes.contains(&dtype) {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Invalid dtype '{}'. Valid options: {:?}",
            dtype, valid_dtypes
        )));
    }

    // Parse compression from options if provided
    let compress = options.and_then(|opts| {
        opts.get_item("COMPRESS")
            .ok()
            .flatten()
            .and_then(|v| v.extract::<String>().ok())
    });

    // Parse tiling from options
    let tiled = options
        .and_then(|opts| {
            opts.get_item("TILED")
                .ok()
                .flatten()
                .and_then(|v| v.extract::<String>().ok())
        })
        .map(|v| v.to_uppercase() == "YES")
        .unwrap_or(false);

    let blocksize = options
        .and_then(|opts| {
            opts.get_item("BLOCKXSIZE")
                .ok()
                .flatten()
                .and_then(|v| v.extract::<u32>().ok())
        })
        .unwrap_or(256);

    // Validate driver if specified
    if let Some(drv) = driver {
        let supported_drivers = ["GTiff", "COG"];
        if !supported_drivers.contains(&drv) {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Unsupported driver '{}'. Supported: {:?}",
                drv, supported_drivers
            )));
        }
    }

    // A Cloud-Optimized GeoTIFF is, at minimum, a tiled GeoTIFF with
    // overviews: requesting driver="COG" now has a real effect on the
    // output (previously it was validated and then silently discarded).
    // "OVERVIEWS" (YES/NO) lets the caller override the COG default
    // explicitly; absent that, overviews are built iff driver="COG".
    let is_cog = driver == Some("COG");
    let overviews_override = options.and_then(|opts| {
        opts.get_item("OVERVIEWS")
            .ok()
            .flatten()
            .and_then(|v| v.extract::<String>().ok())
            .map(|v| v.eq_ignore_ascii_case("YES"))
    });
    let build_overviews = overviews_override.unwrap_or(is_cog);
    let tiled = tiled || is_cog;

    let crs_wkt = crs.map(|c| c.to_string());

    let config = DatasetCreateConfig {
        width,
        height,
        bands,
        dtype: dtype.to_string(),
        crs_wkt,
        nodata,
        geotransform,
        compress: compress.clone(),
        tiled,
        blocksize,
        overviews: build_overviews,
    };

    Dataset::create(path, config)
}

/// Raster calculator - evaluates expressions on raster data.
///
/// Performs pixel-wise calculations using algebraic expressions.
/// Variables A-Z can reference input arrays. Supports standard operators
/// and mathematical functions.
///
/// Args:
///     expression (str): Mathematical expression (e.g., "(A - B) / (A + B)")
///     **arrays: Named NumPy arrays (A=array1, B=array2, etc.)
///
/// Returns:
///     numpy.ndarray: Result array
///
/// Raises:
///     ValueError: If expression is invalid or arrays have different shapes
///
/// Example:
///     >>> # Calculate NDVI
///     >>> ndvi = oxigeo.calc("(NIR - RED) / (NIR + RED)", NIR=band4, RED=band3)
///     >>>
///     >>> # Simple arithmetic
///     >>> scaled = oxigeo.calc("A * 2 + 10", A=data)
///     >>>
///     >>> # Complex expression with multiple bands
///     >>> result = oxigeo.calc("(A + B + C) / 3", A=band1, B=band2, C=band3)
///     >>>
///     >>> # Conditional expression
///     >>> masked = oxigeo.calc("A if A > 0 else 0", A=data)
#[pyfunction]
#[pyo3(signature = (expression, **kwargs))]
pub fn calc<'py>(
    py: Python<'py>,
    expression: &str,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Bound<'py, PyArray2<f64>>> {
    let arrays = kwargs
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("No input arrays provided"))?;

    if arrays.is_empty() {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "At least one input array required",
        ));
    }

    // Extract arrays and validate shapes
    let mut array_map: HashMap<String, Bound<'_, PyArray2<f64>>> = HashMap::new();
    let mut first_shape: Option<[usize; 2]> = None;

    for (key, value) in arrays {
        let key_str: String = key.extract()?;
        if let Ok(arr) = value.extract::<Bound<'_, PyArray2<f64>>>() {
            let shape = arr.shape();
            let shape_array = [shape[0], shape[1]];

            if let Some(ref first) = first_shape {
                if shape_array != *first {
                    return Err(pyo3::exceptions::PyValueError::new_err(format!(
                        "Array '{}' shape {:?} doesn't match first array shape {:?}",
                        key_str, shape_array, first
                    )));
                }
            } else {
                first_shape = Some(shape_array);
            }

            array_map.insert(key_str, arr);
        } else {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Value for '{}' is not a NumPy array",
                key_str
            )));
        }
    }

    if array_map.is_empty() {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "No valid NumPy arrays provided",
        ));
    }

    // Parse the expression using our expression parser
    let parsed_expr = parse_expression(expression).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("Failed to parse expression: {}", e))
    })?;

    // Get the first array to determine shape
    let first_arr = array_map
        .values()
        .next()
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err("No arrays available"))?;
    let shape = first_arr.shape();
    let width = shape[1];
    let height = shape[0];

    // Collect variable names and their order
    let mut var_names: Vec<String> = array_map.keys().cloned().collect();
    var_names.sort(); // Ensure consistent ordering

    // Convert arrays to owned slices in the same order as var_names. This
    // touches NumPy-managed memory and needs the GIL.
    let mut array_slices: Vec<Vec<f64>> = Vec::with_capacity(var_names.len());
    for name in &var_names {
        let arr = array_map.get(name).ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err(format!("Array '{}' not found", name))
        })?;
        let readonly = arr.readonly();
        let slice = readonly
            .as_slice()
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("Array must be contiguous"))?;
        array_slices.push(slice.to_vec());
    }

    // Expression evaluation over the full raster is CPU-bound; release the
    // GIL while it runs.
    let result = py.detach(|| -> PyResult<Vec<Vec<f64>>> {
        // Create the evaluator with variable mapping
        let evaluator = Evaluator::with_variables(&var_names);

        // Build refs after all data is collected
        let array_refs: Vec<&[f64]> = array_slices.iter().map(|s| s.as_slice()).collect();

        // Evaluate the expression
        let result_data = evaluator
            .evaluate(&parsed_expr, &array_refs, width, height)
            .map_err(|e| {
                pyo3::exceptions::PyRuntimeError::new_err(format!("Evaluation error: {}", e))
            })?;

        // Convert to 2D array
        Ok(result_data
            .chunks(width)
            .map(|chunk| chunk.to_vec())
            .collect())
    })?;

    numpy::PyArray2::from_vec2(py, &result).map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to create result array: {}", e))
    })
}

/// Parses a CRS string (EPSG code or WKT) into a Crs object.
pub fn parse_crs_string(crs_str: &str) -> PyResult<Crs> {
    // Check for EPSG:XXXX format
    if let Some(code_str) = crs_str.strip_prefix("EPSG:") {
        let code: u32 = code_str.parse().map_err(|_| {
            pyo3::exceptions::PyValueError::new_err(format!("Invalid EPSG code: {}", code_str))
        })?;
        return Crs::from_epsg(code).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!(
                "Failed to create CRS from EPSG:{}: {}",
                code, e
            ))
        });
    }

    // Check for numeric EPSG code
    if let Ok(code) = crs_str.parse::<u32>() {
        return Crs::from_epsg(code).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!(
                "Failed to create CRS from EPSG:{}: {}",
                code, e
            ))
        });
    }

    // Try as WKT
    if crs_str.starts_with("GEOGCS[") || crs_str.starts_with("PROJCS[") {
        return Crs::from_wkt(crs_str).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Failed to parse WKT CRS: {}", e))
        });
    }

    // Try as PROJ string
    if crs_str.contains("+proj=") {
        return Crs::from_proj(crs_str).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Failed to parse PROJ string: {}", e))
        });
    }

    Err(pyo3::exceptions::PyValueError::new_err(format!(
        "Could not parse CRS string: {}. Expected EPSG:XXXX, WKT, or PROJ string",
        crs_str
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_raster_validation() {
        let test_path = std::env::temp_dir().join("oxigeo_create_raster_test.tif");
        let test_path_str = test_path.to_string_lossy();
        assert!(
            create_raster(
                test_path_str.as_ref(),
                0,
                100,
                1,
                "float32",
                None,
                None,
                None,
                None,
                None
            )
            .is_err()
        );
        assert!(
            create_raster(
                test_path_str.as_ref(),
                100,
                0,
                1,
                "float32",
                None,
                None,
                None,
                None,
                None
            )
            .is_err()
        );
        assert!(
            create_raster(
                test_path_str.as_ref(),
                100,
                100,
                0,
                "float32",
                None,
                None,
                None,
                None,
                None
            )
            .is_err()
        );
    }

    // ========== driver/options honoring tests ==========

    /// `driver="COG"` must have a real effect: a Cloud-Optimized GeoTIFF is,
    /// at minimum, a tiled GeoTIFF with overview levels. Previously `driver`
    /// was validated and then completely discarded (dead data), so COG and
    /// plain GTiff output were byte-for-byte identical modulo the requested
    /// `options`. This exercises the full create -> write -> close -> reopen
    /// round trip and checks the *actual on-disk* overview count differs.
    #[test]
    fn test_create_raster_cog_driver_builds_real_overviews() {
        pyo3::Python::initialize();
        pyo3::Python::attach(|py| {
            let cog_path = std::env::temp_dir().join("oxigeo_create_raster_cog_test.tif");
            let plain_path = std::env::temp_dir().join("oxigeo_create_raster_plain_test.tif");
            let cog_path_str = cog_path.to_string_lossy().to_string();
            let plain_path_str = plain_path.to_string_lossy().to_string();

            // Large enough that the overview builder actually produces at
            // least one reduced-resolution level.
            let size: u64 = 512;
            let row: Vec<f64> = (0..size).map(|v| v as f64).collect();
            let data: Vec<Vec<f64>> = (0..size).map(|_| row.clone()).collect();

            for (path_str, driver) in [
                (cog_path_str.as_str(), Some("COG")),
                (plain_path_str.as_str(), None),
            ] {
                let ds = create_raster(
                    path_str, size, size, 1, "float32", None, None, None, driver, None,
                )
                .expect("create_raster should succeed");
                let ds_cell = pyo3::Py::new(py, ds).expect("wrap dataset");
                let arr = numpy::PyArray2::from_vec2(py, &data).expect("build input array");
                ds_cell
                    .borrow_mut(py)
                    .write_band(1, &arr)
                    .expect("write_band should succeed");
                ds_cell
                    .borrow_mut(py)
                    .close(py)
                    .expect("close/flush should succeed");
            }

            let cog_source =
                oxigeo_core::io::FileDataSource::open(&cog_path).expect("open COG file");
            let cog_reader =
                oxigeo_geotiff::GeoTiffReader::open(cog_source).expect("read COG file");
            let plain_source =
                oxigeo_core::io::FileDataSource::open(&plain_path).expect("open plain file");
            let plain_reader =
                oxigeo_geotiff::GeoTiffReader::open(plain_source).expect("read plain file");

            assert!(
                cog_reader.overview_count() > 0,
                "driver=\"COG\" must produce at least one overview level, got {}",
                cog_reader.overview_count()
            );
            assert_eq!(
                plain_reader.overview_count(),
                0,
                "driver=None must not silently build overviews"
            );

            let _ = std::fs::remove_file(&cog_path);
            let _ = std::fs::remove_file(&plain_path);
        });
    }

    /// `driver="VRT"` is a real, physically-existing format in the OxiGeo
    /// ecosystem (oxigeo-drivers/vrt), but `Dataset` only implements GeoTIFF
    /// reads. Requesting it must raise a clear, honest error rather than
    /// silently opening the file as if it were a GeoTIFF.
    #[test]
    fn test_open_raster_vrt_driver_is_honest_not_implemented_error() {
        pyo3::Python::initialize();
        pyo3::Python::attach(|py| {
            let test_path = std::env::temp_dir().join("oxigeo_open_raster_vrt_test.tif");
            // Reuse whatever create_raster leaves behind from the COG test if
            // present, else create a minimal file so the "path exists" check
            // doesn't shadow the driver check.
            let data: Vec<Vec<f64>> = vec![vec![0.0, 0.0], vec![0.0, 0.0]];
            let ds = create_raster(
                &test_path.to_string_lossy(),
                2,
                2,
                1,
                "float32",
                None,
                None,
                None,
                None,
                None,
            )
            .expect("create_raster should succeed");
            let ds_cell = pyo3::Py::new(py, ds).expect("wrap dataset");
            let arr = numpy::PyArray2::from_vec2(py, &data).expect("build input array");
            ds_cell
                .borrow_mut(py)
                .write_band(1, &arr)
                .expect("write_band should succeed");
            ds_cell.borrow_mut(py).close(py).expect("close");

            let err = match open_raster(py, &test_path.to_string_lossy(), "r", Some("VRT"), None) {
                Ok(_) => panic!("VRT driver must not silently succeed as GeoTIFF"),
                Err(e) => e,
            };
            let message = err.to_string();
            assert!(
                message.contains("VRT") || message.contains("not") || message.contains("Not"),
                "expected an honest 'not implemented'-style message, got: {message}"
            );

            let _ = std::fs::remove_file(&test_path);
        });
    }

    /// Without the `cloud` Cargo feature, remote URLs (previously silently
    /// mishandled: the S3 example in this function's own docstring could
    /// not work) must raise a clear, honest error -- never a confusing
    /// generic `FileNotFoundError`/`IOError` and never a silent fake read.
    #[cfg(not(feature = "cloud"))]
    #[test]
    fn test_open_raster_remote_url_without_cloud_feature_is_honest_error() {
        pyo3::Python::initialize();
        pyo3::Python::attach(|py| {
            let err = match open_raster(py, "s3://some-bucket/some-file.tif", "r", None, None) {
                Ok(_) => panic!("remote URL must not silently succeed without the cloud feature"),
                Err(e) => e,
            };
            let message = err.to_string();
            assert!(
                message.to_lowercase().contains("cloud"),
                "expected a message pointing at the 'cloud' feature, got: {message}"
            );
        });
    }

    // ========== CRS Parsing Tests ==========

    #[test]
    fn test_parse_crs_string_epsg() {
        // Test EPSG:XXXX format
        let result = parse_crs_string("EPSG:4326");
        assert!(result.is_ok());
        let crs = result.expect("should parse");
        assert_eq!(crs.epsg_code(), Some(4326));

        // Test numeric format
        let result = parse_crs_string("4326");
        assert!(result.is_ok());
        let crs = result.expect("should parse");
        assert_eq!(crs.epsg_code(), Some(4326));
    }

    #[test]
    fn test_parse_crs_string_invalid() {
        // Invalid EPSG code
        assert!(parse_crs_string("EPSG:invalid").is_err());

        // Unknown format
        assert!(parse_crs_string("unknown_crs").is_err());
    }
}
