//! Raster I/O bindings for Node.js
//!
//! This module provides comprehensive raster dataset operations including
//! reading, writing, metadata management, and band operations.

use napi::bindgen_prelude::*;
use napi_derive::napi;
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::io::FileDataSource;
use oxigdal_core::types::{
    ColorInterpretation, GeoTransform, NoDataValue, PixelLayout, RasterDataType, RasterMetadata,
};
use oxigdal_geotiff::tiff::Predictor;
use oxigdal_geotiff::writer::{GeoTiffWriterOptions, OverviewResampling, WriterConfig};
use oxigdal_geotiff::{Compression, PhotometricInterpretation};
use std::path::Path;

use crate::buffer::BufferWrapper;
use crate::error::{NodeError, ToNapiResult};

/// Raster dataset for reading and writing geospatial raster data
#[napi]
pub struct Dataset {
    metadata: RasterMetadata,
    bands: Vec<RasterBuffer>,
    file_path: Option<String>,
}

#[napi]
impl Dataset {
    /// Opens a raster dataset from a file
    #[napi(factory)]
    pub fn open(path: String) -> Result<Self> {
        // Determine format from file extension
        let path_obj = Path::new(&path);
        let ext = path_obj
            .extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| NodeError {
                code: "INVALID_FILE".to_string(),
                message: "File has no extension".to_string(),
            })?;

        match ext.to_lowercase().as_str() {
            "tif" | "tiff" => {
                // Open GeoTIFF using FileDataSource
                let data_source = FileDataSource::open(&path).to_napi()?;
                let reader = oxigdal_geotiff::GeoTiffReader::open(data_source).to_napi()?;
                let metadata = reader.metadata().clone();
                let band_count = metadata.band_count as usize;

                let mut bands = Vec::with_capacity(band_count);
                for i in 0..band_count {
                    let band_data = reader.read_band(0, i).to_napi()?;
                    let band_buffer = RasterBuffer::new(
                        band_data,
                        metadata.width,
                        metadata.height,
                        metadata.data_type,
                        metadata.nodata,
                    )
                    .to_napi()?;
                    bands.push(band_buffer);
                }

                Ok(Self {
                    metadata,
                    bands,
                    file_path: Some(path),
                })
            }
            "json" | "geojson" => Err(NodeError {
                code: "INVALID_FORMAT".to_string(),
                message: "GeoJSON is a vector format, use vector API".to_string(),
            }
            .into()),
            _ => Err(NodeError {
                code: "UNSUPPORTED_FORMAT".to_string(),
                message: format!("Unsupported file format: .{}", ext),
            }
            .into()),
        }
    }

    /// Creates a new raster dataset in memory
    #[napi(factory)]
    pub fn create(width: u32, height: u32, band_count: u32, data_type: String) -> Result<Self> {
        let dtype = parse_data_type(&data_type)?;

        let metadata = RasterMetadata {
            width: width as u64,
            height: height as u64,
            band_count,
            data_type: dtype,
            geo_transform: None,
            crs_wkt: None,
            nodata: NoDataValue::None,
            color_interpretation: vec![ColorInterpretation::Undefined; band_count as usize],
            layout: PixelLayout::BandSequential,
            driver_metadata: Vec::new(),
        };

        let mut bands = Vec::with_capacity(band_count as usize);
        for _ in 0..band_count {
            bands.push(RasterBuffer::zeros(width as u64, height as u64, dtype));
        }

        Ok(Self {
            metadata,
            bands,
            file_path: None,
        })
    }

    /// Gets the width of the dataset
    #[napi(getter)]
    pub fn width(&self) -> u32 {
        self.metadata.width as u32
    }

    /// Gets the height of the dataset
    #[napi(getter)]
    pub fn height(&self) -> u32 {
        self.metadata.height as u32
    }

    /// Gets the number of bands
    #[napi(getter)]
    pub fn band_count(&self) -> u32 {
        self.metadata.band_count
    }

    /// Gets the data type as a string
    #[napi(getter)]
    pub fn data_type(&self) -> String {
        format_data_type(self.metadata.data_type)
    }

    /// Gets the file path if opened from file
    #[napi(getter)]
    pub fn file_path(&self) -> Option<String> {
        self.file_path.clone()
    }

    /// Gets the CRS as WKT string
    #[napi(getter)]
    pub fn crs(&self) -> Option<String> {
        self.metadata.crs_wkt.clone()
    }

    /// Sets the CRS
    #[napi(setter)]
    pub fn set_crs(&mut self, crs: Option<String>) {
        self.metadata.crs_wkt = crs;
    }

    /// Gets the NoData value
    #[napi(getter)]
    pub fn nodata(&self) -> Option<f64> {
        self.metadata.nodata.as_f64()
    }

    /// Sets the NoData value
    #[napi(setter)]
    pub fn set_nodata(&mut self, value: Option<f64>) {
        self.metadata.nodata = match value {
            Some(v) => NoDataValue::Float(v),
            None => NoDataValue::None,
        };
    }

    /// Gets the geo transform as an array of 6 values
    #[napi]
    pub fn get_geo_transform(&self) -> Option<Vec<f64>> {
        self.metadata.geo_transform.as_ref().map(|gt| {
            vec![
                gt.origin_x,
                gt.pixel_width,
                gt.row_rotation,
                gt.origin_y,
                gt.col_rotation,
                gt.pixel_height,
            ]
        })
    }

    /// Sets the geo transform from an array of 6 values
    #[napi]
    pub fn set_geo_transform(&mut self, values: Vec<f64>) -> Result<()> {
        if values.len() != 6 {
            return Err(NodeError {
                code: "INVALID_PARAMETER".to_string(),
                message: "Geo transform must have exactly 6 values".to_string(),
            }
            .into());
        }

        self.metadata.geo_transform = Some(GeoTransform {
            origin_x: values[0],
            pixel_width: values[1],
            row_rotation: values[2],
            origin_y: values[3],
            col_rotation: values[4],
            pixel_height: values[5],
        });

        Ok(())
    }

    /// Gets the bounding box in geographic coordinates
    #[napi]
    pub fn get_bounds(&self) -> Option<Bounds> {
        self.metadata.geo_transform.as_ref().map(|gt| {
            let min_x = gt.origin_x;
            let max_y = gt.origin_y;
            let max_x = min_x + gt.pixel_width * self.metadata.width as f64;
            let min_y = max_y + gt.pixel_height * self.metadata.height as f64;

            Bounds {
                min_x,
                min_y,
                max_x,
                max_y,
            }
        })
    }

    /// Reads a band as a BufferWrapper
    #[napi]
    pub fn read_band(&self, band_index: u32) -> Result<BufferWrapper> {
        if band_index >= self.metadata.band_count {
            return Err(NodeError {
                code: "OUT_OF_BOUNDS".to_string(),
                message: format!(
                    "Band index {} out of range (0-{})",
                    band_index,
                    self.metadata.band_count - 1
                ),
            }
            .into());
        }

        let buffer = self.bands[band_index as usize].clone();
        Ok(BufferWrapper::from_raster_buffer(buffer))
    }

    /// Reads a band into a provided Node.js Buffer
    #[napi]
    pub fn read_band_into(&self, band_index: u32, mut buffer: Buffer) -> Result<()> {
        if band_index >= self.metadata.band_count {
            return Err(NodeError {
                code: "OUT_OF_BOUNDS".to_string(),
                message: format!(
                    "Band index {} out of range (0-{})",
                    band_index,
                    self.metadata.band_count - 1
                ),
            }
            .into());
        }

        let band = &self.bands[band_index as usize];
        let data = band.as_bytes();

        if buffer.len() != data.len() {
            return Err(NodeError {
                code: "BUFFER_SIZE_MISMATCH".to_string(),
                message: format!(
                    "Buffer size mismatch: expected {} bytes, got {}",
                    data.len(),
                    buffer.len()
                ),
            }
            .into());
        }

        // SAFETY: We've checked the buffer size matches
        buffer.copy_from_slice(data);

        Ok(())
    }

    /// Writes a band from a BufferWrapper
    #[napi]
    pub fn write_band(&mut self, band_index: u32, buffer: &BufferWrapper) -> Result<()> {
        if band_index >= self.metadata.band_count {
            return Err(NodeError {
                code: "OUT_OF_BOUNDS".to_string(),
                message: format!(
                    "Band index {} out of range (0-{})",
                    band_index,
                    self.metadata.band_count - 1
                ),
            }
            .into());
        }

        if buffer.width() != self.width() || buffer.height() != self.height() {
            return Err(NodeError {
                code: "DIMENSION_MISMATCH".to_string(),
                message: format!(
                    "Buffer dimensions ({}x{}) don't match dataset ({}x{})",
                    buffer.width(),
                    buffer.height(),
                    self.width(),
                    self.height()
                ),
            }
            .into());
        }

        self.bands[band_index as usize] = buffer.inner().clone();
        Ok(())
    }

    /// Reads a window (subset) of a band
    #[napi]
    pub fn read_window(
        &self,
        band_index: u32,
        x_off: u32,
        y_off: u32,
        width: u32,
        height: u32,
    ) -> Result<BufferWrapper> {
        if band_index >= self.metadata.band_count {
            return Err(NodeError {
                code: "OUT_OF_BOUNDS".to_string(),
                message: format!(
                    "Band index {} out of range (0-{})",
                    band_index,
                    self.metadata.band_count - 1
                ),
            }
            .into());
        }

        if x_off + width > self.width() || y_off + height > self.height() {
            return Err(NodeError {
                code: "OUT_OF_BOUNDS".to_string(),
                message: format!(
                    "Window ({}+{}, {}+{}) exceeds dataset bounds ({}x{})",
                    x_off,
                    width,
                    y_off,
                    height,
                    self.width(),
                    self.height()
                ),
            }
            .into());
        }

        let band = &self.bands[band_index as usize];
        let dtype = band.data_type();
        let mut window_buffer = RasterBuffer::zeros(width as u64, height as u64, dtype);

        // Copy window data
        for y in 0..height {
            for x in 0..width {
                let src_x = (x_off + x) as u64;
                let src_y = (y_off + y) as u64;
                let dst_x = x as u64;
                let dst_y = y as u64;

                // Copy pixel using get_pixel/set_pixel
                let value = band.get_pixel(src_x, src_y).to_napi()?;
                window_buffer.set_pixel(dst_x, dst_y, value).to_napi()?;
            }
        }

        Ok(BufferWrapper::from_raster_buffer(window_buffer))
    }

    /// Saves the dataset to a file
    #[napi]
    pub fn save(&self, path: String) -> Result<()> {
        let path_obj = Path::new(&path);
        let ext = path_obj
            .extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| NodeError {
                code: "INVALID_FILE".to_string(),
                message: "File has no extension".to_string(),
            })?;

        match ext.to_lowercase().as_str() {
            "tif" | "tiff" => {
                // Create WriterConfig from metadata
                let config = WriterConfig {
                    width: self.metadata.width,
                    height: self.metadata.height,
                    band_count: self.metadata.band_count as u16,
                    data_type: self.metadata.data_type,
                    compression: Compression::Lzw,
                    predictor: Predictor::HorizontalDifferencing,
                    tile_width: Some(256),
                    tile_height: Some(256),
                    photometric: PhotometricInterpretation::BlackIsZero,
                    geo_transform: self.metadata.geo_transform,
                    epsg_code: None,
                    nodata: self.metadata.nodata,
                    use_bigtiff: false,
                    generate_overviews: false,
                    overview_resampling: OverviewResampling::Average,
                    overview_levels: Vec::new(),
                };

                let options = GeoTiffWriterOptions::default();
                let mut writer =
                    oxigdal_geotiff::writer::GeoTiffWriter::create(&path, config, options)
                        .to_napi()?;

                // Write all bands as a single interleaved buffer
                // For now, write each band separately
                for band in &self.bands {
                    writer.write(band.as_bytes()).to_napi()?;
                }

                Ok(())
            }
            _ => Err(NodeError {
                code: "UNSUPPORTED_FORMAT".to_string(),
                message: format!("Unsupported output format: .{}", ext),
            }
            .into()),
        }
    }

    /// Gets metadata as a JavaScript object
    #[napi]
    pub fn get_metadata(&self) -> Metadata {
        Metadata {
            width: self.width(),
            height: self.height(),
            band_count: self.band_count(),
            data_type: self.data_type(),
            crs: self.crs(),
            nodata: self.nodata(),
            geo_transform: self.get_geo_transform(),
            bounds: self.get_bounds(),
        }
    }

    /// Creates a copy of the dataset
    #[napi]
    pub fn clone(&self) -> Self {
        Self {
            metadata: self.metadata.clone(),
            bands: self.bands.clone(),
            file_path: self.file_path.clone(),
        }
    }

    /// Converts pixel coordinates to geographic coordinates
    #[napi]
    pub fn pixel_to_geo(&self, x: f64, y: f64) -> Result<Coordinate> {
        let gt = self
            .metadata
            .geo_transform
            .as_ref()
            .ok_or_else(|| NodeError {
                code: "NO_GEO_TRANSFORM".to_string(),
                message: "Dataset has no geo transform".to_string(),
            })?;

        let geo_x = gt.origin_x + x * gt.pixel_width + y * gt.row_rotation;
        let geo_y = gt.origin_y + x * gt.col_rotation + y * gt.pixel_height;

        Ok(Coordinate { x: geo_x, y: geo_y })
    }

    /// Converts geographic coordinates to pixel coordinates
    #[napi]
    pub fn geo_to_pixel(&self, x: f64, y: f64) -> Result<Coordinate> {
        let gt = self
            .metadata
            .geo_transform
            .as_ref()
            .ok_or_else(|| NodeError {
                code: "NO_GEO_TRANSFORM".to_string(),
                message: "Dataset has no geo transform".to_string(),
            })?;

        // Inverse transform
        let det = gt.pixel_width * gt.pixel_height - gt.row_rotation * gt.col_rotation;
        if det.abs() < 1e-10 {
            return Err(NodeError {
                code: "INVALID_TRANSFORM".to_string(),
                message: "Geo transform is not invertible".to_string(),
            }
            .into());
        }

        let dx = x - gt.origin_x;
        let dy = y - gt.origin_y;

        let pixel_x = (gt.pixel_height * dx - gt.row_rotation * dy) / det;
        let pixel_y = (-gt.col_rotation * dx + gt.pixel_width * dy) / det;

        Ok(Coordinate {
            x: pixel_x,
            y: pixel_y,
        })
    }
}

/// Metadata object for JavaScript
#[napi(object)]
pub struct Metadata {
    pub width: u32,
    pub height: u32,
    pub band_count: u32,
    pub data_type: String,
    pub crs: Option<String>,
    pub nodata: Option<f64>,
    pub geo_transform: Option<Vec<f64>>,
    pub bounds: Option<Bounds>,
}

/// Bounding box
#[napi(object)]
#[derive(Clone)]
pub struct Bounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

/// Coordinate pair
#[napi(object)]
pub struct Coordinate {
    pub x: f64,
    pub y: f64,
}

/// Parse data type string to RasterDataType
fn parse_data_type(dtype: &str) -> Result<RasterDataType> {
    match dtype.to_lowercase().as_str() {
        "uint8" | "u8" => Ok(RasterDataType::UInt8),
        "int16" | "i16" => Ok(RasterDataType::Int16),
        "uint16" | "u16" => Ok(RasterDataType::UInt16),
        "int32" | "i32" => Ok(RasterDataType::Int32),
        "uint32" | "u32" => Ok(RasterDataType::UInt32),
        "float32" | "f32" => Ok(RasterDataType::Float32),
        "float64" | "f64" => Ok(RasterDataType::Float64),
        _ => Err(NodeError {
            code: "INVALID_DATA_TYPE".to_string(),
            message: format!("Unknown data type: {}", dtype),
        }
        .into()),
    }
}

/// Format RasterDataType to string
fn format_data_type(dtype: RasterDataType) -> String {
    match dtype {
        RasterDataType::UInt8 => "uint8".to_string(),
        RasterDataType::Int16 => "int16".to_string(),
        RasterDataType::UInt16 => "uint16".to_string(),
        RasterDataType::Int32 => "int32".to_string(),
        RasterDataType::UInt32 => "uint32".to_string(),
        RasterDataType::Float32 => "float32".to_string(),
        RasterDataType::Float64 => "float64".to_string(),
        _ => "unknown".to_string(),
    }
}

/// Opens a raster dataset (convenience function)
#[allow(dead_code)]
#[napi]
pub fn open_raster(path: String) -> Result<Dataset> {
    Dataset::open(path)
}

/// Creates a new raster dataset (convenience function)
#[allow(dead_code)]
#[napi]
pub fn create_raster(
    width: u32,
    height: u32,
    band_count: u32,
    data_type: String,
) -> Result<Dataset> {
    Dataset::create(width, height, band_count, data_type)
}
