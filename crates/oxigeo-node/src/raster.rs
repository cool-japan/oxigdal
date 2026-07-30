//! Raster I/O bindings for Node.js
//!
//! This module provides comprehensive raster dataset operations including
//! reading, writing, metadata management, and band operations.

use napi::bindgen_prelude::*;
use napi_derive::napi;
use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::io::FileDataSource;
use oxigeo_core::types::{
    ColorInterpretation, GeoTransform, NoDataValue, PixelLayout, RasterDataType, RasterMetadata,
};
use oxigeo_geotiff::tiff::Predictor;
use oxigeo_geotiff::writer::{GeoTiffWriterOptions, OverviewResampling, WriterConfig};
use oxigeo_geotiff::{Compression, PhotometricInterpretation};
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
                let reader = oxigeo_geotiff::GeoTiffReader::open(data_source).to_napi()?;
                let metadata = reader.metadata().clone();
                let band_count = metadata.band_count as usize;

                // `read_band(level, band)` returns one de-interleaved band plane
                // (`width * height * bytes_per_sample` bytes) for the 0-based
                // `band`, handling both chunky and planar on-disk layouts. Read
                // each band in turn; `interleave_bands` still re-weaves them on
                // save, which is what makes multi-band GeoTIFFs round-trip.
                // See <https://github.com/cool-japan/oxigeo/issues/14>.
                let mut bands = Vec::with_capacity(band_count);
                for band_index in 0..band_count {
                    let plane = reader.read_band(0, band_index).to_napi()?;
                    bands.push(Self::band_plane_to_buffer(
                        plane,
                        metadata.width,
                        metadata.height,
                        band_index,
                        metadata.data_type,
                        metadata.nodata,
                    )?);
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
            statistics: None,
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
                    oxigeo_geotiff::writer::GeoTiffWriter::create(&path, config, options)
                        .to_napi()?;

                // The GeoTIFF writer expects a single, fully band-interleaved
                // (BIP: band-interleaved-by-pixel) buffer covering every band,
                // and validates the total length against
                // width * height * bytes_per_sample * band_count. Calling
                // `write` once per band (with only a single band's bytes) fails
                // that check for multi-band rasters and rewrites the TIFF header
                // on every call. Instead, interleave all bands up front and
                // write exactly once.
                let interleaved = self.interleave_bands()?;
                writer.write(&interleaved).to_napi()?;

                Ok(())
            }
            _ => Err(NodeError {
                code: "UNSUPPORTED_FORMAT".to_string(),
                message: format!("Unsupported output format: .{}", ext),
            }
            .into()),
        }
    }

    /// Builds a single band-interleaved-by-pixel (BIP) byte buffer spanning
    /// every band, in the layout the GeoTIFF writer expects.
    ///
    /// For each pixel (in row-major order) the `bytes_per_sample` bytes of
    /// every band are emitted consecutively, i.e. `[b0_p0, b1_p0, ..., b0_p1,
    /// b1_p1, ...]`. This mirrors the interleaving logic used by the Python
    /// bindings and is required for `band_count > 1`; for a single band it is a
    /// straight copy of that band's bytes.
    fn interleave_bands(&self) -> Result<Vec<u8>> {
        let bytes_per_sample = self.metadata.data_type.size_bytes();
        let pixel_count = (self.metadata.width as usize) * (self.metadata.height as usize);
        let expected_band_len = pixel_count * bytes_per_sample;

        // Every band buffer must match the dataset dimensions/type exactly, or
        // the interleaved slicing below would read past the end of a band.
        for (index, band) in self.bands.iter().enumerate() {
            let band_len = band.as_bytes().len();
            if band_len != expected_band_len {
                return Err(NodeError {
                    code: "BAND_SIZE_MISMATCH".to_string(),
                    message: format!(
                        "Band {} has {} bytes, expected {} ({}x{} x {} bytes/sample)",
                        index,
                        band_len,
                        expected_band_len,
                        self.metadata.width,
                        self.metadata.height,
                        bytes_per_sample
                    ),
                }
                .into());
            }
        }

        // Fast path: a single band is already in the required layout.
        if self.bands.len() == 1 {
            return Ok(self.bands[0].as_bytes().to_vec());
        }

        let band_count = self.bands.len();
        let mut interleaved = vec![0u8; pixel_count * band_count * bytes_per_sample];
        for (band_index, band) in self.bands.iter().enumerate() {
            let band_bytes = band.as_bytes();
            for pixel in 0..pixel_count {
                let src = pixel * bytes_per_sample;
                let dst = (pixel * band_count + band_index) * bytes_per_sample;
                interleaved[dst..dst + bytes_per_sample]
                    .copy_from_slice(&band_bytes[src..src + bytes_per_sample]);
            }
        }

        Ok(interleaved)
    }

    /// Wraps one de-interleaved band plane, as returned by
    /// `GeoTiffReader::read_band`, in a [`RasterBuffer`].
    ///
    /// The driver already performs the de-interleave that
    /// [`Self::interleave_bands`] undoes on save, so this only checks that the
    /// plane is `width * height * bytes_per_sample` bytes -- the size
    /// `RasterBuffer::new` demands -- and reports a clear `FORMAT_ERROR` if it
    /// is not. See <https://github.com/cool-japan/oxigeo/issues/14>.
    fn band_plane_to_buffer(
        plane: Vec<u8>,
        width: u64,
        height: u64,
        band_index: usize,
        data_type: RasterDataType,
        nodata: NoDataValue,
    ) -> Result<RasterBuffer> {
        let bytes_per_sample = data_type.size_bytes();
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|px| px.checked_mul(bytes_per_sample))
            .ok_or_else(|| NodeError {
                code: "FORMAT_ERROR".to_string(),
                message: format!(
                    "Raster dimensions {}x{} x {} bytes/sample overflow the address space",
                    width, height, bytes_per_sample
                ),
            })?;

        if plane.len() != expected {
            return Err(NodeError {
                code: "FORMAT_ERROR".to_string(),
                message: format!(
                    "Band {} plane has {} bytes, expected {} ({}x{} x {} bytes/sample)",
                    band_index,
                    plane.len(),
                    expected,
                    width,
                    height,
                    bytes_per_sample
                ),
            }
            .into());
        }

        RasterBuffer::new(plane, width, height, data_type, nodata).to_napi()
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

    /// Borrows the underlying per-band pixel buffers (crate-internal).
    ///
    /// Used by the parallel-processing helpers in `async_ops` which need direct
    /// read access to each band's pixels.
    pub(crate) fn bands(&self) -> &[RasterBuffer] {
        &self.bands
    }

    /// Builds a new dataset that shares this dataset's metadata/geo-referencing
    /// but carries a freshly computed set of band buffers (crate-internal).
    ///
    /// The number of supplied bands must match the existing band count so that
    /// the metadata stays consistent with the pixel data.
    pub(crate) fn with_bands(&self, bands: Vec<RasterBuffer>) -> Self {
        let mut metadata = self.metadata.clone();
        metadata.band_count = bands.len() as u32;
        Self {
            metadata,
            bands,
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicU64, Ordering};

    /// Per-test scratch fixture inside the system temp dir (house policy: no
    /// hardcoded absolute paths).
    ///
    /// The leaf name embeds the process id and a monotonic counter, so no two
    /// test binaries — nor two concurrent runs of this one — can ever land on
    /// the same file.  Dropping the guard removes the fixture, so a panicking
    /// test leaks nothing.
    struct TempPath(std::path::PathBuf);

    impl TempPath {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            Self(
                std::env::temp_dir()
                    .join(format!("oxigeo_node_{}_{seq}_{name}", std::process::id())),
            )
        }

        /// The napi surface takes owned `String` paths, so hand out a copy.
        fn as_string(&self) -> String {
            self.0.to_string_lossy().into_owned()
        }
    }

    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn interleave_single_band_is_a_straight_copy() {
        let mut ds = Dataset::create(3, 2, 1, "uint8".to_string()).expect("create");
        let mut buf = BufferWrapper::new(3, 2, "uint8".to_string()).expect("buffer");
        for y in 0..2 {
            for x in 0..3 {
                buf.set_pixel(x, y, (x + y * 3) as f64).expect("set");
            }
        }
        ds.write_band(0, &buf).expect("write_band");
        let interleaved = ds.interleave_bands().expect("interleave");
        assert_eq!(interleaved, ds.bands[0].as_bytes());
    }

    #[test]
    fn interleave_two_bands_is_band_interleaved_by_pixel() {
        let mut ds = Dataset::create(2, 1, 2, "uint8".to_string()).expect("create");
        let mut b0 = BufferWrapper::new(2, 1, "uint8".to_string()).expect("b0");
        let mut b1 = BufferWrapper::new(2, 1, "uint8".to_string()).expect("b1");
        b0.set_pixel(0, 0, 10.0).expect("set");
        b0.set_pixel(1, 0, 11.0).expect("set");
        b1.set_pixel(0, 0, 20.0).expect("set");
        b1.set_pixel(1, 0, 21.0).expect("set");
        ds.write_band(0, &b0).expect("write b0");
        ds.write_band(1, &b1).expect("write b1");

        let interleaved = ds.interleave_bands().expect("interleave");
        // BIP layout for 2 pixels x 2 bands (uint8): b0p0, b1p0, b0p1, b1p1.
        assert_eq!(interleaved, vec![10u8, 20u8, 11u8, 21u8]);
    }

    #[test]
    fn save_and_reopen_multiband_geotiff_roundtrips() {
        let width = 8u32;
        let height = 6u32;
        let mut ds = Dataset::create(width, height, 3, "uint8".to_string()).expect("create");

        // Distinct, per-band gradients so a band/offset mixup would be caught.
        for band in 0..3u32 {
            let mut buf = BufferWrapper::new(width, height, "uint8".to_string()).expect("buffer");
            for y in 0..height {
                for x in 0..width {
                    let value = (band * 40 + x + y) % 256;
                    buf.set_pixel(x, y, value as f64).expect("set");
                }
            }
            ds.write_band(band, &buf).expect("write_band");
        }

        let path = TempPath::new("multiband.tif");
        ds.save(path.as_string()).expect("save multi-band geotiff");

        let reopened = Dataset::open(path.as_string()).expect("reopen");
        assert_eq!(reopened.band_count(), 3);
        assert_eq!(reopened.width(), width);
        assert_eq!(reopened.height(), height);

        for band in 0..3u32 {
            let read = reopened.read_band(band).expect("read_band");
            for y in 0..height {
                for x in 0..width {
                    let expected = ((band * 40 + x + y) % 256) as f64;
                    let actual = read.get_pixel(x, y).expect("get_pixel");
                    assert!(
                        (actual - expected).abs() < f64::EPSILON,
                        "band {band} pixel ({x},{y}): expected {expected}, got {actual}"
                    );
                }
            }
        }
    }

    /// Regression test for <https://github.com/cool-japan/oxigeo/issues/14>.
    ///
    /// `Dataset::open` used to issue a single `read_band(0, 0)` and split the
    /// result with `deinterleave_bands`, which hard-errored unless the buffer
    /// was `width * height * bytes_per_sample * band_count` bytes. Once
    /// `read_band` started returning a single de-interleaved band plane,
    /// opening *any* multi-band GeoTIFF from Node failed outright with
    /// `FORMAT_ERROR`. The pre-existing failure mode of the hand-rolled split
    /// was worse than an error, though -- wrong pixels -- so this asserts the
    /// per-band values, not merely that `open` succeeded.
    #[test]
    fn test_issue_14_open_multiband_geotiff_bands_are_distinct() {
        let width = 7u32;
        let height = 5u32;
        let band_count = 3u32;

        // Disjoint value range per band (band 0 -> 0..=12, band 1 -> 70..=82,
        // band 2 -> 140..=152) so "band 0 repeated", "bands swapped" and
        // "interleaved garbage" are each individually detectable.
        let expected =
            |band: u32, x: u32, y: u32| -> f64 { f64::from(band * 70 + ((y * width + x) % 13)) };

        let mut ds = Dataset::create(width, height, band_count, "uint8".to_string())
            .expect("create 3-band dataset");
        for band in 0..band_count {
            let mut buf = BufferWrapper::new(width, height, "uint8".to_string()).expect("buffer");
            for y in 0..height {
                for x in 0..width {
                    buf.set_pixel(x, y, expected(band, x, y)).expect("set");
                }
            }
            ds.write_band(band, &buf).expect("write_band");
        }

        let path = TempPath::new("issue14_multiband.tif");
        ds.save(path.as_string()).expect("save multi-band geotiff");

        let reopened = Dataset::open(path.as_string())
            .expect("Dataset::open must succeed for a multi-band GeoTIFF (issue #14)");
        assert_eq!(
            reopened.band_count(),
            band_count,
            "band_count: expected {}, got {}",
            band_count,
            reopened.band_count()
        );

        for band in 0..band_count {
            let read = reopened.read_band(band).expect("read_band");
            for y in 0..height {
                for x in 0..width {
                    let want = expected(band, x, y);
                    let got = read.get_pixel(x, y).expect("get_pixel");
                    assert!(
                        (got - want).abs() < f64::EPSILON,
                        "band {band} pixel ({x},{y}): expected {want}, got {got}"
                    );
                }
            }
        }

        // Every band must differ from band 0 at the same pixel: a plane that
        // was silently re-sliced (or duplicated) would collapse these.
        let band0 = reopened.read_band(0).expect("read_band 0");
        for band in 1..band_count {
            let other = reopened.read_band(band).expect("read_band");
            for y in 0..height {
                for x in 0..width {
                    let base = band0.get_pixel(x, y).expect("get_pixel");
                    let got = other.get_pixel(x, y).expect("get_pixel");
                    assert!(
                        (got - base).abs() > f64::EPSILON,
                        "band {band} pixel ({x},{y}): expected a value distinct from band 0's \
                         {base}, got {got}"
                    );
                }
            }
        }
    }

    #[test]
    fn interleave_rejects_wrong_size_band() {
        // Manufacture an inconsistent dataset: metadata says 2x2 but a band is
        // the wrong length. `with_bands` keeps metadata, so build via fields.
        let mut ds = Dataset::create(2, 2, 1, "uint8".to_string()).expect("create");
        ds.bands[0] = RasterBuffer::zeros(3, 3, RasterDataType::UInt8);
        let err = ds.interleave_bands();
        assert!(err.is_err(), "mismatched band length must be rejected");
    }
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
