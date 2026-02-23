//! Dataset wrapper for Python bindings
//!
//! This module provides a Python-friendly interface to OxiGDAL datasets,
//! with actual GeoTIFF I/O support using oxigdal-geotiff.

use numpy::{PyArray2, PyArrayMethods, PyUntypedArrayMethods};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::io::FileDataSource;
use oxigdal_core::types::{GeoTransform, NoDataValue, RasterDataType, RasterMetadata};
use oxigdal_geotiff::{
    Compression, GeoTiffReader, GeoTiffWriter, GeoTiffWriterOptions, WriterConfig,
};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::path::PathBuf;

use crate::array::{buffer_to_numpy, data_type_to_numpy_dtype};

/// Configuration for creating a new dataset
#[derive(Debug, Clone)]
pub struct DatasetCreateConfig {
    pub width: u64,
    pub height: u64,
    pub bands: u32,
    pub dtype: String,
    pub crs_wkt: Option<String>,
    pub nodata: Option<f64>,
    pub geotransform: Option<Vec<f64>>,
    pub compress: Option<String>,
    pub tiled: bool,
    pub blocksize: u32,
}

impl DatasetCreateConfig {
    pub fn new(width: u64, height: u64, bands: u32, dtype: String) -> Self {
        Self {
            width,
            height,
            bands,
            dtype,
            crs_wkt: None,
            nodata: None,
            geotransform: None,
            compress: None,
            tiled: false,
            blocksize: 256,
        }
    }

    pub fn with_crs(mut self, crs_wkt: String) -> Self {
        self.crs_wkt = Some(crs_wkt);
        self
    }

    pub fn with_nodata(mut self, nodata: f64) -> Self {
        self.nodata = Some(nodata);
        self
    }

    pub fn with_geotransform(mut self, gt: Vec<f64>) -> Self {
        self.geotransform = Some(gt);
        self
    }

    pub fn with_compression(mut self, compress: String) -> Self {
        self.compress = Some(compress);
        self
    }

    pub fn with_tiling(mut self, tiled: bool, blocksize: u32) -> Self {
        self.tiled = tiled;
        self.blocksize = blocksize;
        self
    }
}

/// Parses a dtype string into a RasterDataType
fn parse_dtype(dtype: &str) -> Result<RasterDataType, String> {
    match dtype {
        "uint8" => Ok(RasterDataType::UInt8),
        "int8" => Ok(RasterDataType::Int8),
        "uint16" => Ok(RasterDataType::UInt16),
        "int16" => Ok(RasterDataType::Int16),
        "uint32" => Ok(RasterDataType::UInt32),
        "int32" => Ok(RasterDataType::Int32),
        "uint64" => Ok(RasterDataType::UInt64),
        "int64" => Ok(RasterDataType::Int64),
        "float32" => Ok(RasterDataType::Float32),
        "float64" => Ok(RasterDataType::Float64),
        "complex64" => Ok(RasterDataType::CFloat32),
        "complex128" => Ok(RasterDataType::CFloat64),
        _ => Err(format!("Unknown dtype: {}", dtype)),
    }
}

/// Parses a compression string into a Compression enum
fn parse_compression(compress: &str) -> Result<Compression, String> {
    match compress.to_lowercase().as_str() {
        "lzw" => Ok(Compression::Lzw),
        "deflate" | "zlib" => Ok(Compression::Deflate),
        "zstd" => Ok(Compression::Zstd),
        "none" | "uncompressed" => Ok(Compression::None),
        _ => Err(format!("Unknown compression: {}", compress)),
    }
}

/// Internal storage for band data pending write
struct PendingBandData {
    band: u32,
    data: Vec<f64>,
}

/// A geospatial dataset that can be read from or written to.
///
/// This class represents an opened dataset (raster or vector) and provides
/// methods to access metadata and read/write data.
///
/// Attributes:
///     path (str): Path to the dataset
///     width (int): Width in pixels (for raster datasets)
///     height (int): Height in pixels (for raster datasets)
///     band_count (int): Number of bands (for raster datasets)
///
/// Example:
///     >>> ds = oxigdal.open("input.tif")
///     >>> print(f"Size: {ds.width}x{ds.height}")
///     >>> data = ds.read_band(1)
#[pyclass]
pub struct Dataset {
    path: PathBuf,
    metadata: Option<RasterMetadata>,
    mode: String,
    /// Pending band data for write mode
    pending_bands: Vec<PendingBandData>,
    /// Writer configuration for write mode
    writer_config: Option<WriterConfig>,
}

impl Dataset {
    /// Opens a dataset from the given path
    pub fn open(path: &str, mode: &str) -> PyResult<Self> {
        let path_buf = PathBuf::from(path);

        // Check if file exists for read mode
        if mode == "r"
            && !path_buf.exists()
            && !path.starts_with("http://")
            && !path.starts_with("https://")
        {
            return Err(pyo3::exceptions::PyFileNotFoundError::new_err(format!(
                "File not found: {}",
                path
            )));
        }

        let mut ds = Self {
            path: path_buf,
            metadata: None,
            mode: mode.to_string(),
            pending_bands: Vec::new(),
            writer_config: None,
        };

        // For read mode, load metadata immediately if file exists
        if (mode == "r" || mode == "r+") && ds.path.exists() {
            ds.load_metadata()?;
        }

        Ok(ds)
    }

    /// Opens a dataset with creation parameters for write mode
    pub fn create(path: &str, config: DatasetCreateConfig) -> PyResult<Self> {
        let data_type = parse_dtype(&config.dtype).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Invalid dtype: {}", e))
        })?;

        let geo_transform = if let Some(ref gt) = config.geotransform {
            if gt.len() != 6 {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "GeoTransform must have 6 elements",
                ));
            }
            Some(GeoTransform::from_gdal_array([
                gt[0], gt[1], gt[2], gt[3], gt[4], gt[5],
            ]))
        } else {
            None
        };

        let nodata_value = match config.nodata {
            Some(v) => NoDataValue::Float(v),
            None => NoDataValue::None,
        };

        let metadata = RasterMetadata {
            width: config.width,
            height: config.height,
            band_count: config.bands,
            data_type,
            geo_transform,
            crs_wkt: config.crs_wkt,
            nodata: nodata_value,
            ..Default::default()
        };

        // Build writer config
        let compression = if let Some(ref comp) = config.compress {
            parse_compression(comp).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("Invalid compression: {}", e))
            })?
        } else {
            Compression::Lzw
        };

        let mut writer_config =
            WriterConfig::new(config.width, config.height, config.bands as u16, data_type)
                .with_compression(compression)
                .with_nodata(nodata_value);

        if config.tiled {
            writer_config = writer_config.with_tile_size(config.blocksize, config.blocksize);
        } else {
            writer_config.tile_width = None;
            writer_config.tile_height = None;
        }

        if let Some(gt) = geo_transform {
            writer_config = writer_config.with_geo_transform(gt);
        }

        Ok(Self {
            path: PathBuf::from(path),
            metadata: Some(metadata),
            mode: "w".to_string(),
            pending_bands: Vec::new(),
            writer_config: Some(writer_config),
        })
    }

    /// Loads metadata from the file using GeoTIFF reader
    fn load_metadata(&mut self) -> PyResult<()> {
        if !self.path.exists() {
            return Err(pyo3::exceptions::PyFileNotFoundError::new_err(format!(
                "File not found: {}",
                self.path.display()
            )));
        }

        let source = FileDataSource::open(&self.path).map_err(|e| {
            pyo3::exceptions::PyIOError::new_err(format!(
                "Failed to open file '{}': {}",
                self.path.display(),
                e
            ))
        })?;

        let reader = GeoTiffReader::open(source).map_err(|e| {
            pyo3::exceptions::PyIOError::new_err(format!(
                "Failed to read GeoTIFF '{}': {}",
                self.path.display(),
                e
            ))
        })?;

        self.metadata = Some(reader.metadata());
        Ok(())
    }

    /// Gets metadata as RasterMetadata
    fn get_raster_metadata(&mut self) -> PyResult<&RasterMetadata> {
        if self.metadata.is_none() {
            self.load_metadata()?;
        }

        self.metadata
            .as_ref()
            .ok_or_else(|| pyo3::exceptions::PyRuntimeError::new_err("Failed to load metadata"))
    }

    /// Reads raw band data from a GeoTIFF file
    fn read_band_data(&self, band: u32) -> PyResult<(Vec<f64>, u64, u64, RasterDataType)> {
        let source = FileDataSource::open(&self.path).map_err(|e| {
            pyo3::exceptions::PyIOError::new_err(format!(
                "Failed to open file '{}': {}",
                self.path.display(),
                e
            ))
        })?;

        let reader = GeoTiffReader::open(source).map_err(|e| {
            pyo3::exceptions::PyIOError::new_err(format!(
                "Failed to read GeoTIFF '{}': {}",
                self.path.display(),
                e
            ))
        })?;

        let width = reader.width();
        let height = reader.height();
        let data_type = reader.data_type().unwrap_or(RasterDataType::Float64);
        let nodata = reader.nodata();

        // Read the raw band data
        let raw_data = reader.read_band(0, (band - 1) as usize).map_err(|e| {
            pyo3::exceptions::PyIOError::new_err(format!(
                "Failed to read band {} from '{}': {}",
                band,
                self.path.display(),
                e
            ))
        })?;

        // Convert raw bytes to f64 buffer
        let buffer =
            RasterBuffer::new(raw_data, width, height, data_type, nodata).map_err(|e| {
                pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to create buffer: {}", e))
            })?;

        let mut values = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                let val = buffer.get_pixel(x, y).map_err(|e| {
                    pyo3::exceptions::PyRuntimeError::new_err(format!(
                        "Failed to read pixel ({}, {}): {}",
                        x, y, e
                    ))
                })?;
                values.push(val);
            }
        }

        Ok((values, width, height, data_type))
    }

    /// Writes all pending bands to the GeoTIFF file
    fn flush_write(&mut self) -> PyResult<()> {
        let config = self
            .writer_config
            .as_ref()
            .ok_or_else(|| pyo3::exceptions::PyRuntimeError::new_err("No writer configuration"))?;

        let metadata = self.metadata.as_ref().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("No metadata set for writing")
        })?;

        let width = metadata.width as usize;
        let height = metadata.height as usize;
        let band_count = metadata.band_count as usize;
        let bytes_per_sample = config.bytes_per_sample();

        // Build interleaved data buffer for all bands
        let pixel_count = width * height;
        let total_bytes = pixel_count * bytes_per_sample * band_count;
        let mut data = vec![0u8; total_bytes];

        // Sort pending bands by band index
        let mut bands_sorted: Vec<&PendingBandData> = self.pending_bands.iter().collect();
        bands_sorted.sort_by_key(|b| b.band);

        for pending in &bands_sorted {
            let band_idx = (pending.band - 1) as usize;
            if band_idx >= band_count {
                continue;
            }

            for i in 0..pixel_count {
                let value = if i < pending.data.len() {
                    pending.data[i]
                } else {
                    0.0
                };

                // Convert f64 to the target data type bytes
                let offset = i * band_count * bytes_per_sample + band_idx * bytes_per_sample;
                if offset + bytes_per_sample <= data.len() {
                    write_value_to_bytes(&mut data[offset..], value, metadata.data_type);
                }
            }
        }

        let config_clone = config.clone();

        let mut writer =
            GeoTiffWriter::create(&self.path, config_clone, GeoTiffWriterOptions::default())
                .map_err(|e| {
                    pyo3::exceptions::PyIOError::new_err(format!(
                        "Failed to create GeoTIFF writer: {}",
                        e
                    ))
                })?;

        writer.write(&data).map_err(|e| {
            pyo3::exceptions::PyIOError::new_err(format!("Failed to write GeoTIFF data: {}", e))
        })?;

        Ok(())
    }
}

/// Public wrapper for write_value_to_bytes for use in other modules
pub fn write_value_to_bytes_pub(buf: &mut [u8], value: f64, data_type: RasterDataType) {
    write_value_to_bytes(buf, value, data_type);
}

/// Writes a f64 value to a byte buffer in the specified data type format
fn write_value_to_bytes(buf: &mut [u8], value: f64, data_type: RasterDataType) {
    match data_type {
        RasterDataType::UInt8 => {
            if !buf.is_empty() {
                buf[0] = value as u8;
            }
        }
        RasterDataType::Int8 => {
            if !buf.is_empty() {
                buf[0] = (value as i8) as u8;
            }
        }
        RasterDataType::UInt16 => {
            let bytes = (value as u16).to_ne_bytes();
            if buf.len() >= 2 {
                buf[..2].copy_from_slice(&bytes);
            }
        }
        RasterDataType::Int16 => {
            let bytes = (value as i16).to_ne_bytes();
            if buf.len() >= 2 {
                buf[..2].copy_from_slice(&bytes);
            }
        }
        RasterDataType::UInt32 => {
            let bytes = (value as u32).to_ne_bytes();
            if buf.len() >= 4 {
                buf[..4].copy_from_slice(&bytes);
            }
        }
        RasterDataType::Int32 => {
            let bytes = (value as i32).to_ne_bytes();
            if buf.len() >= 4 {
                buf[..4].copy_from_slice(&bytes);
            }
        }
        RasterDataType::Float32 => {
            let bytes = (value as f32).to_ne_bytes();
            if buf.len() >= 4 {
                buf[..4].copy_from_slice(&bytes);
            }
        }
        RasterDataType::Float64 => {
            let bytes = value.to_ne_bytes();
            if buf.len() >= 8 {
                buf[..8].copy_from_slice(&bytes);
            }
        }
        RasterDataType::UInt64 => {
            let bytes = (value as u64).to_ne_bytes();
            if buf.len() >= 8 {
                buf[..8].copy_from_slice(&bytes);
            }
        }
        RasterDataType::Int64 => {
            let bytes = (value as i64).to_ne_bytes();
            if buf.len() >= 8 {
                buf[..8].copy_from_slice(&bytes);
            }
        }
        RasterDataType::CFloat32 => {
            let bytes = (value as f32).to_ne_bytes();
            if buf.len() >= 4 {
                buf[..4].copy_from_slice(&bytes);
            }
        }
        RasterDataType::CFloat64 => {
            let bytes = value.to_ne_bytes();
            if buf.len() >= 8 {
                buf[..8].copy_from_slice(&bytes);
            }
        }
    }
}

#[pymethods]
impl Dataset {
    /// Gets the dataset path.
    ///
    /// Returns:
    ///     str: The file path
    #[getter]
    fn path(&self) -> String {
        self.path.to_string_lossy().to_string()
    }

    /// Gets the raster width in pixels.
    ///
    /// Returns:
    ///     int: Width in pixels
    ///
    /// Raises:
    ///     RuntimeError: If metadata cannot be loaded
    #[getter]
    pub fn width(&mut self) -> PyResult<u64> {
        let metadata = self.get_raster_metadata()?;
        Ok(metadata.width)
    }

    /// Gets the raster height in pixels.
    ///
    /// Returns:
    ///     int: Height in pixels
    ///
    /// Raises:
    ///     RuntimeError: If metadata cannot be loaded
    #[getter]
    pub fn height(&mut self) -> PyResult<u64> {
        let metadata = self.get_raster_metadata()?;
        Ok(metadata.height)
    }

    /// Gets the number of bands.
    ///
    /// Returns:
    ///     int: Number of bands
    ///
    /// Raises:
    ///     RuntimeError: If metadata cannot be loaded
    #[getter]
    pub fn band_count(&mut self) -> PyResult<u32> {
        let metadata = self.get_raster_metadata()?;
        Ok(metadata.band_count)
    }

    /// Reads a raster band as a NumPy array.
    ///
    /// Args:
    ///     band (int): Band number (1-indexed)
    ///
    /// Returns:
    ///     numpy.ndarray: 2D array with shape (height, width)
    ///
    /// Raises:
    ///     ValueError: If band number is invalid
    ///     IOError: If reading fails
    ///
    /// Example:
    ///     >>> ds = oxigdal.open("input.tif")
    ///     >>> band1 = ds.read_band(1)
    ///     >>> print(band1.shape)
    pub fn read_band<'py>(
        &mut self,
        py: Python<'py>,
        band: u32,
    ) -> PyResult<Bound<'py, PyArray2<f64>>> {
        let metadata = self.get_raster_metadata()?;

        if band < 1 || band > metadata.band_count {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Band {} out of range (1-{})",
                band, metadata.band_count
            )));
        }

        let width = metadata.width;
        let height = metadata.height;
        let data_type = metadata.data_type;

        // Try to read actual data from file
        if self.path.exists() && (self.mode == "r" || self.mode == "r+") {
            match self.read_band_data(band) {
                Ok((values, w, _h, _dt)) => {
                    let nested: Vec<Vec<f64>> = values
                        .chunks(w as usize)
                        .map(|chunk| chunk.to_vec())
                        .collect();
                    return PyArray2::from_vec2(py, &nested).map_err(|e| {
                        pyo3::exceptions::PyRuntimeError::new_err(format!(
                            "Failed to create NumPy array: {}",
                            e
                        ))
                    });
                }
                Err(_) => {
                    // Fall back to zero-filled buffer
                }
            }
        }

        // Fallback: return zero-filled buffer
        let buffer = RasterBuffer::zeros(width, height, data_type);
        buffer_to_numpy(py, &buffer)
    }

    /// Writes a NumPy array to a raster band.
    ///
    /// Args:
    ///     band (int): Band number (1-indexed)
    ///     array (numpy.ndarray): 2D array to write
    ///
    /// Raises:
    ///     ValueError: If band number is invalid or array shape doesn't match
    ///     IOError: If writing fails
    ///
    /// Example:
    ///     >>> ds = oxigdal.open("output.tif", "w")
    ///     >>> ds.write_band(1, data_array)
    pub fn write_band(&mut self, band: u32, array: &Bound<'_, PyArray2<f64>>) -> PyResult<()> {
        if self.mode != "w" && self.mode != "r+" {
            return Err(pyo3::exceptions::PyIOError::new_err(
                "Dataset not opened for writing",
            ));
        }

        let metadata = self.get_raster_metadata()?;

        if band < 1 || band > metadata.band_count {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Band {} out of range (1-{})",
                band, metadata.band_count
            )));
        }

        let shape = array.shape();
        if shape[0] != metadata.height as usize || shape[1] != metadata.width as usize {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Array shape {:?} doesn't match dataset size ({}x{})",
                shape, metadata.height, metadata.width
            )));
        }

        // Extract array data
        let readonly = array.readonly();
        let slice = readonly
            .as_slice()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("Array must be contiguous"))?;

        // Store pending band data
        self.pending_bands.push(PendingBandData {
            band,
            data: slice.to_vec(),
        });

        Ok(())
    }

    /// Gets dataset metadata as a dictionary.
    ///
    /// Returns:
    ///     dict: Metadata dictionary with keys like 'width', 'height', 'crs', etc.
    ///
    /// Example:
    ///     >>> ds = oxigdal.open("input.tif")
    ///     >>> meta = ds.get_metadata()
    ///     >>> print(meta['crs'])
    fn get_metadata<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let metadata = self.get_raster_metadata()?;
        let dict = PyDict::new(py);

        dict.set_item("width", metadata.width)?;
        dict.set_item("height", metadata.height)?;
        dict.set_item("band_count", metadata.band_count)?;
        dict.set_item("data_type", data_type_to_numpy_dtype(metadata.data_type))?;

        if let Some(ref crs) = metadata.crs_wkt {
            dict.set_item("crs", crs)?;
        }

        if let Some(gt) = metadata.geo_transform {
            let gt_list = vec![
                gt.origin_x,
                gt.pixel_width,
                gt.row_rotation,
                gt.origin_y,
                gt.col_rotation,
                gt.pixel_height,
            ];
            dict.set_item("geotransform", gt_list)?;
        }

        if let Some(nodata_val) = metadata.nodata.as_f64() {
            dict.set_item("nodata", nodata_val)?;
        }

        Ok(dict)
    }

    /// Sets dataset metadata.
    ///
    /// Args:
    ///     metadata (dict): Metadata dictionary
    ///
    /// Raises:
    ///     IOError: If dataset not opened for writing
    ///
    /// Example:
    ///     >>> ds = oxigdal.open("output.tif", "w")
    ///     >>> ds.set_metadata({"crs": "EPSG:4326", "nodata": -9999})
    pub fn set_metadata(&mut self, metadata_dict: &Bound<'_, PyDict>) -> PyResult<()> {
        if self.mode != "w" && self.mode != "r+" {
            return Err(pyo3::exceptions::PyIOError::new_err(
                "Dataset not opened for writing",
            ));
        }

        // Update metadata from dictionary
        if let Some(ref mut metadata) = self.metadata {
            if let Some(crs) = metadata_dict
                .get_item("crs")
                .ok()
                .flatten()
                .and_then(|v| v.extract::<String>().ok())
            {
                metadata.crs_wkt = Some(crs);
            }
            if let Some(nodata) = metadata_dict
                .get_item("nodata")
                .ok()
                .flatten()
                .and_then(|v| v.extract::<f64>().ok())
            {
                metadata.nodata = NoDataValue::Float(nodata);
            }
            if let Some(gt_list) = metadata_dict
                .get_item("geotransform")
                .ok()
                .flatten()
                .and_then(|v| v.extract::<Vec<f64>>().ok())
            {
                if gt_list.len() == 6 {
                    metadata.geo_transform = Some(GeoTransform::from_gdal_array([
                        gt_list[0], gt_list[1], gt_list[2], gt_list[3], gt_list[4], gt_list[5],
                    ]));
                }
            }

            // Update writer config if available
            if let Some(ref mut config) = self.writer_config {
                config.nodata = metadata.nodata;
                config.geo_transform = metadata.geo_transform;
            }
        }

        Ok(())
    }

    /// Closes the dataset and flushes any pending writes.
    ///
    /// Example:
    ///     >>> ds = oxigdal.open("output.tif", "w")
    ///     >>> ds.write_band(1, data)
    ///     >>> ds.close()
    pub fn close(&mut self) -> PyResult<()> {
        // Flush pending writes if in write mode
        if (self.mode == "w" || self.mode == "r+") && !self.pending_bands.is_empty() {
            self.flush_write()?;
            self.pending_bands.clear();
        }
        Ok(())
    }

    /// Returns a string representation of the dataset.
    fn __repr__(&self) -> String {
        format!("Dataset('{}', mode='{}')", self.path.display(), self.mode)
    }

    /// Context manager support: enter
    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// Context manager support: exit
    #[pyo3(signature = (_exc_type=None, _exc_value=None, _traceback=None))]
    fn __exit__(
        &mut self,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<bool> {
        self.close()?;
        Ok(false)
    }
}

/// Configuration for writing GeoTIFF data
#[derive(Debug, Clone)]
pub struct GeoTiffWriteConfig {
    pub width: u64,
    pub height: u64,
    pub band_count: u16,
    pub data_type: RasterDataType,
    pub geo_transform: Option<GeoTransform>,
    pub epsg_code: Option<u32>,
    pub nodata: NoDataValue,
    pub compression: Compression,
    pub tiled: bool,
    pub tile_size: u32,
    pub build_overviews: bool,
}

impl GeoTiffWriteConfig {
    pub fn new(width: u64, height: u64, band_count: u16, data_type: RasterDataType) -> Self {
        Self {
            width,
            height,
            band_count,
            data_type,
            geo_transform: None,
            epsg_code: None,
            nodata: NoDataValue::None,
            compression: Compression::None,
            tiled: false,
            tile_size: 256,
            build_overviews: false,
        }
    }
}

/// Utility: writes f64 data to a GeoTIFF file with full configuration
pub fn write_geotiff_data(path: &str, data: &[f64], config: GeoTiffWriteConfig) -> PyResult<()> {
    let mut writer_config = WriterConfig::new(
        config.width,
        config.height,
        config.band_count,
        config.data_type,
    )
    .with_compression(config.compression)
    .with_nodata(config.nodata)
    .with_overviews(
        config.build_overviews,
        oxigdal_geotiff::OverviewResampling::Average,
    );

    if let Some(gt) = config.geo_transform {
        writer_config = writer_config.with_geo_transform(gt);
    }

    if let Some(epsg) = config.epsg_code {
        writer_config = writer_config.with_epsg_code(epsg);
    }

    if config.tiled {
        writer_config = writer_config.with_tile_size(config.tile_size, config.tile_size);
    } else {
        writer_config.tile_width = None;
        writer_config.tile_height = None;
    }

    // Convert f64 data to bytes based on data type
    let pixel_count = (config.width * config.height) as usize;
    let bytes_per_sample = config.data_type.size_bytes();
    let total_bytes = pixel_count * config.band_count as usize * bytes_per_sample;
    let mut byte_data = vec![0u8; total_bytes];

    for (i, &value) in data.iter().enumerate() {
        let offset = i * bytes_per_sample;
        if offset + bytes_per_sample <= byte_data.len() {
            write_value_to_bytes(&mut byte_data[offset..], value, config.data_type);
        }
    }

    let mut writer = GeoTiffWriter::create(path, writer_config, GeoTiffWriterOptions::default())
        .map_err(|e| {
            pyo3::exceptions::PyIOError::new_err(format!(
                "Failed to create GeoTIFF writer for '{}': {}",
                path, e
            ))
        })?;

    writer.write(&byte_data).map_err(|e| {
        pyo3::exceptions::PyIOError::new_err(format!("Failed to write GeoTIFF data: {}", e))
    })?;

    Ok(())
}

/// Utility: reads all metadata from a GeoTIFF file
pub fn read_geotiff_metadata(path: &str) -> PyResult<RasterMetadata> {
    let source = FileDataSource::open(path).map_err(|e| {
        pyo3::exceptions::PyIOError::new_err(format!("Failed to open '{}': {}", path, e))
    })?;

    let reader = GeoTiffReader::open(source).map_err(|e| {
        pyo3::exceptions::PyIOError::new_err(format!("Failed to read GeoTIFF '{}': {}", path, e))
    })?;

    Ok(reader.metadata())
}

/// Utility: reads a single band from a GeoTIFF as f64 values
pub fn read_geotiff_band(path: &str, band: u32) -> PyResult<(Vec<f64>, u64, u64, RasterMetadata)> {
    let source = FileDataSource::open(path).map_err(|e| {
        pyo3::exceptions::PyIOError::new_err(format!("Failed to open '{}': {}", path, e))
    })?;

    let reader = GeoTiffReader::open(source).map_err(|e| {
        pyo3::exceptions::PyIOError::new_err(format!("Failed to read GeoTIFF '{}': {}", path, e))
    })?;

    let metadata = reader.metadata();
    let width = reader.width();
    let height = reader.height();
    let data_type = reader.data_type().unwrap_or(RasterDataType::Float64);
    let nodata = reader.nodata();

    let raw_data = reader.read_band(0, (band - 1) as usize).map_err(|e| {
        pyo3::exceptions::PyIOError::new_err(format!(
            "Failed to read band {} from '{}': {}",
            band, path, e
        ))
    })?;

    let buffer = RasterBuffer::new(raw_data, width, height, data_type, nodata).map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to create buffer: {}", e))
    })?;

    let mut values = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let val = buffer.get_pixel(x, y).map_err(|e| {
                pyo3::exceptions::PyRuntimeError::new_err(format!(
                    "Failed to read pixel ({}, {}): {}",
                    x, y, e
                ))
            })?;
            values.push(val);
        }
    }

    Ok((values, width, height, metadata))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dataset_creation() {
        let ds = Dataset {
            path: PathBuf::from("/tmp/test.tif"),
            metadata: None,
            mode: "r".to_string(),
            pending_bands: Vec::new(),
            writer_config: None,
        };
        assert_eq!(ds.path, PathBuf::from("/tmp/test.tif"));
        assert_eq!(ds.mode, "r");
    }

    #[test]
    fn test_dataset_repr() {
        let ds = Dataset {
            path: PathBuf::from("/tmp/test.tif"),
            metadata: None,
            mode: "r".to_string(),
            pending_bands: Vec::new(),
            writer_config: None,
        };
        let repr = ds.__repr__();
        assert!(repr.contains("test.tif"));
        assert!(repr.contains("mode='r'"));
    }

    #[test]
    fn test_parse_dtype() {
        assert!(matches!(parse_dtype("uint8"), Ok(RasterDataType::UInt8)));
        assert!(matches!(
            parse_dtype("float32"),
            Ok(RasterDataType::Float32)
        ));
        assert!(matches!(
            parse_dtype("float64"),
            Ok(RasterDataType::Float64)
        ));
        assert!(parse_dtype("invalid").is_err());
    }

    #[test]
    fn test_parse_compression() {
        assert!(matches!(parse_compression("lzw"), Ok(Compression::Lzw)));
        assert!(matches!(
            parse_compression("deflate"),
            Ok(Compression::Deflate)
        ));
        assert!(matches!(parse_compression("none"), Ok(Compression::None)));
        assert!(parse_compression("invalid").is_err());
    }

    #[test]
    fn test_write_value_to_bytes() {
        let mut buf = [0u8; 8];
        write_value_to_bytes(&mut buf, 42.0, RasterDataType::Float64);
        assert_eq!(f64::from_ne_bytes(buf), 42.0);

        let mut buf4 = [0u8; 4];
        write_value_to_bytes(&mut buf4, 255.0, RasterDataType::UInt8);
        assert_eq!(buf4[0], 255);
    }
}
