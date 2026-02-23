//! VRT reader with lazy evaluation

use crate::band::PixelFunction;
use crate::dataset::VrtDataset;
use crate::error::{Result, VrtError};
use crate::mosaic::MosaicCompositor;
use crate::source::{PixelRect, VrtSource};
use crate::xml::VrtXmlParser;
use lru::LruCache;
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::io::FileDataSource;
use oxigdal_core::types::{GeoTransform, NoDataValue, RasterDataType, RasterMetadata};
use oxigdal_geotiff::GeoTiffReader;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// VRT reader with lazy source loading
pub struct VrtReader {
    /// VRT dataset definition
    dataset: VrtDataset,
    /// Cache of opened source datasets
    source_cache: Arc<Mutex<LruCache<PathBuf, Arc<SourceDataset>>>>,
    /// Mosaic compositor
    compositor: MosaicCompositor,
}

impl VrtReader {
    /// Opens a VRT file
    ///
    /// # Errors
    /// Returns an error if the file cannot be opened or parsed
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let dataset = VrtXmlParser::parse_file(&path)?;
        Self::from_dataset(dataset)
    }

    /// Creates a reader from a VRT dataset
    ///
    /// # Errors
    /// Returns an error if the dataset is invalid
    pub fn from_dataset(dataset: VrtDataset) -> Result<Self> {
        dataset.validate()?;

        // Create source cache (default 32 open files)
        let cache_size =
            NonZeroUsize::new(32).ok_or_else(|| VrtError::cache_error("Invalid cache size"))?;
        let source_cache = Arc::new(Mutex::new(LruCache::new(cache_size)));

        let compositor = MosaicCompositor::new();

        Ok(Self {
            dataset,
            source_cache,
            compositor,
        })
    }

    /// Gets the raster width
    #[must_use]
    pub fn width(&self) -> u64 {
        self.dataset.raster_x_size
    }

    /// Gets the raster height
    #[must_use]
    pub fn height(&self) -> u64 {
        self.dataset.raster_y_size
    }

    /// Gets the number of bands
    #[must_use]
    pub fn band_count(&self) -> usize {
        self.dataset.band_count()
    }

    /// Gets the GeoTransform
    #[must_use]
    pub fn geo_transform(&self) -> Option<&GeoTransform> {
        self.dataset.geo_transform.as_ref()
    }

    /// Gets the spatial reference system
    #[must_use]
    pub fn srs(&self) -> Option<&str> {
        self.dataset.srs.as_deref()
    }

    /// Gets the block size
    #[must_use]
    pub fn block_size(&self) -> (u32, u32) {
        self.dataset.effective_block_size()
    }

    /// Gets the metadata
    #[must_use]
    pub fn metadata(&self) -> RasterMetadata {
        let (tile_width, tile_height) = self.block_size();
        RasterMetadata {
            width: self.dataset.raster_x_size,
            height: self.dataset.raster_y_size,
            band_count: self.dataset.band_count() as u32,
            data_type: self
                .dataset
                .primary_data_type()
                .unwrap_or(RasterDataType::UInt8),
            geo_transform: self.dataset.geo_transform,
            crs_wkt: self.dataset.srs.clone(),
            nodata: NoDataValue::None,
            color_interpretation: Vec::new(),
            layout: oxigdal_core::types::PixelLayout::Tiled {
                tile_width,
                tile_height,
            },
            driver_metadata: Vec::new(),
        }
    }

    /// Reads a band's data for a specific window
    ///
    /// # Errors
    /// Returns an error if reading fails
    pub fn read_window(&self, band: usize, window: PixelRect) -> Result<RasterBuffer> {
        let band_idx = band - 1;
        let vrt_band = self
            .dataset
            .get_band(band_idx)
            .ok_or_else(|| VrtError::band_out_of_range(band, self.dataset.band_count()))?;

        // Get sources that intersect with the window
        let contributing_sources: Vec<&VrtSource> = vrt_band
            .sources
            .iter()
            .filter(|s| s.dst_rect().map(|r| r.intersects(&window)).unwrap_or(false))
            .collect();

        if contributing_sources.is_empty() {
            return Err(VrtError::invalid_window(
                "No sources contribute to this window",
            ));
        }

        // Create output buffer
        let data_size = (window.x_size * window.y_size) as usize * vrt_band.data_type.size_bytes();
        let mut data = vec![0u8; data_size];

        // If pixel function is present, read all sources separately and apply function
        if let Some(ref pixel_func) = vrt_band.pixel_function {
            self.apply_pixel_function(
                &contributing_sources,
                &window,
                vrt_band.data_type,
                vrt_band.nodata,
                pixel_func,
                &mut data,
            )?;
        } else {
            // Composite data from all contributing sources (no pixel function)
            for source in &contributing_sources {
                self.read_source_contribution(source, &window, vrt_band.data_type, &mut data)?;
            }
        }

        RasterBuffer::new(
            data,
            window.x_size,
            window.y_size,
            vrt_band.data_type,
            vrt_band.nodata,
        )
        .map_err(|e| e.into())
    }

    /// Reads a full band
    ///
    /// # Errors
    /// Returns an error if reading fails
    pub fn read_band(&self, band: usize) -> Result<RasterBuffer> {
        let window = PixelRect::new(0, 0, self.width(), self.height());
        self.read_window(band, window)
    }

    /// Reads a source's contribution to a window
    fn read_source_contribution(
        &self,
        source: &VrtSource,
        dst_window: &PixelRect,
        data_type: RasterDataType,
        output: &mut [u8],
    ) -> Result<()> {
        let source_dst_rect = source
            .dst_rect()
            .ok_or_else(|| VrtError::invalid_source("Source has no destination rectangle"))?;

        // Calculate intersection between source and requested window
        let intersection = source_dst_rect
            .intersect(dst_window)
            .ok_or_else(|| VrtError::invalid_window("Source does not intersect window"))?;

        // Open source dataset
        let dataset = self.open_source(source)?;

        // Calculate source rectangle
        let src_window = source
            .window
            .as_ref()
            .ok_or_else(|| VrtError::invalid_source("Source has no window configuration"))?;

        // Calculate offset within source
        let src_x_off = src_window.src_rect.x_off + (intersection.x_off - source_dst_rect.x_off);
        let src_y_off = src_window.src_rect.y_off + (intersection.y_off - source_dst_rect.y_off);

        let src_rect = PixelRect::new(
            src_x_off,
            src_y_off,
            intersection.x_size,
            intersection.y_size,
        );

        // Read from source
        let source_data = dataset.read_window(source.source_band, src_rect)?;

        // Copy to output buffer at correct position
        let dst_x_off = intersection.x_off - dst_window.x_off;
        let dst_y_off = intersection.y_off - dst_window.y_off;

        let params = crate::mosaic::CompositeParams::new(
            dst_x_off,
            dst_y_off,
            intersection.x_size,
            intersection.y_size,
            dst_window.x_size,
            data_type,
        );
        self.compositor
            .composite(source_data.as_bytes(), output, &params)?;

        Ok(())
    }

    /// Applies pixel function to source data
    fn apply_pixel_function(
        &self,
        sources: &[&VrtSource],
        window: &PixelRect,
        data_type: RasterDataType,
        nodata: NoDataValue,
        pixel_func: &PixelFunction,
        output: &mut [u8],
    ) -> Result<()> {
        let pixel_count = (window.x_size * window.y_size) as usize;
        let _bytes_per_pixel = data_type.size_bytes();

        // Read all source bands
        let mut source_buffers = Vec::new();
        for source in sources {
            let source_dst_rect = source
                .dst_rect()
                .ok_or_else(|| VrtError::invalid_source("Source has no destination rectangle"))?;

            let intersection = source_dst_rect
                .intersect(window)
                .ok_or_else(|| VrtError::invalid_window("Source does not intersect window"))?;

            let dataset = self.open_source(source)?;

            let src_window = source
                .window
                .as_ref()
                .ok_or_else(|| VrtError::invalid_source("Source has no window configuration"))?;

            let src_x_off =
                src_window.src_rect.x_off + (intersection.x_off - source_dst_rect.x_off);
            let src_y_off =
                src_window.src_rect.y_off + (intersection.y_off - source_dst_rect.y_off);

            let src_rect = PixelRect::new(
                src_x_off,
                src_y_off,
                intersection.x_size,
                intersection.y_size,
            );

            let source_data = dataset.read_window(source.source_band, src_rect)?;
            source_buffers.push((source_data, intersection));
        }

        // Apply pixel function to each pixel
        for pixel_idx in 0..pixel_count {
            let y = pixel_idx as u64 / window.x_size;
            let x = pixel_idx as u64 % window.x_size;
            let global_x = window.x_off + x;
            let global_y = window.y_off + y;

            // Collect values from all sources for this pixel
            let mut values = Vec::new();
            for (source_buffer, intersection) in &source_buffers {
                if global_x >= intersection.x_off
                    && global_x < intersection.x_off + intersection.x_size
                    && global_y >= intersection.y_off
                    && global_y < intersection.y_off + intersection.y_size
                {
                    let local_x = global_x - intersection.x_off;
                    let local_y = global_y - intersection.y_off;
                    let local_idx = (local_y * intersection.x_size + local_x) as usize;

                    // Read value from source buffer
                    let value = self.read_pixel_value(
                        source_buffer.as_bytes(),
                        local_idx,
                        data_type,
                        nodata,
                    )?;
                    values.push(value);
                } else {
                    values.push(None);
                }
            }

            // Apply pixel function
            let result = pixel_func.apply(&values)?;

            // Write result to output
            self.write_pixel_value(output, pixel_idx, result, data_type)?;
        }

        Ok(())
    }

    /// Reads a single pixel value from a buffer
    fn read_pixel_value(
        &self,
        buffer: &[u8],
        pixel_idx: usize,
        data_type: RasterDataType,
        nodata: NoDataValue,
    ) -> Result<Option<f64>> {
        let bytes_per_pixel = data_type.size_bytes();
        let offset = pixel_idx * bytes_per_pixel;

        if offset + bytes_per_pixel > buffer.len() {
            return Ok(None);
        }

        let value = match data_type {
            RasterDataType::UInt8 => buffer[offset] as f64,
            RasterDataType::Int8 => buffer[offset] as i8 as f64,
            RasterDataType::UInt16 => {
                let val = u16::from_le_bytes([buffer[offset], buffer[offset + 1]]);
                val as f64
            }
            RasterDataType::Int16 => {
                let val = i16::from_le_bytes([buffer[offset], buffer[offset + 1]]);
                val as f64
            }
            RasterDataType::UInt32 => {
                let val = u32::from_le_bytes([
                    buffer[offset],
                    buffer[offset + 1],
                    buffer[offset + 2],
                    buffer[offset + 3],
                ]);
                val as f64
            }
            RasterDataType::Int32 => {
                let val = i32::from_le_bytes([
                    buffer[offset],
                    buffer[offset + 1],
                    buffer[offset + 2],
                    buffer[offset + 3],
                ]);
                val as f64
            }
            RasterDataType::Float32 => {
                let val = f32::from_le_bytes([
                    buffer[offset],
                    buffer[offset + 1],
                    buffer[offset + 2],
                    buffer[offset + 3],
                ]);
                val as f64
            }
            RasterDataType::Float64 => f64::from_le_bytes([
                buffer[offset],
                buffer[offset + 1],
                buffer[offset + 2],
                buffer[offset + 3],
                buffer[offset + 4],
                buffer[offset + 5],
                buffer[offset + 6],
                buffer[offset + 7],
            ]),
            _ => return Err(VrtError::invalid_source("Unsupported data type")),
        };

        // Check for NoData
        let is_nodata = match nodata {
            NoDataValue::None => false,
            NoDataValue::Integer(nd) => (value - nd as f64).abs() < f64::EPSILON,
            NoDataValue::Float(nd) => (value - nd).abs() < f64::EPSILON,
        };

        if is_nodata { Ok(None) } else { Ok(Some(value)) }
    }

    /// Writes a single pixel value to a buffer
    fn write_pixel_value(
        &self,
        buffer: &mut [u8],
        pixel_idx: usize,
        value: Option<f64>,
        data_type: RasterDataType,
    ) -> Result<()> {
        let bytes_per_pixel = data_type.size_bytes();
        let offset = pixel_idx * bytes_per_pixel;

        if offset + bytes_per_pixel > buffer.len() {
            return Err(VrtError::invalid_window("Pixel offset out of bounds"));
        }

        let write_val = value.unwrap_or(0.0);

        match data_type {
            RasterDataType::UInt8 => {
                buffer[offset] = write_val.clamp(0.0, 255.0) as u8;
            }
            RasterDataType::Int8 => {
                buffer[offset] = write_val.clamp(-128.0, 127.0) as i8 as u8;
            }
            RasterDataType::UInt16 => {
                let val = write_val.clamp(0.0, 65535.0) as u16;
                buffer[offset..offset + 2].copy_from_slice(&val.to_le_bytes());
            }
            RasterDataType::Int16 => {
                let val = write_val.clamp(-32768.0, 32767.0) as i16;
                buffer[offset..offset + 2].copy_from_slice(&val.to_le_bytes());
            }
            RasterDataType::UInt32 => {
                let val = write_val.clamp(0.0, u32::MAX as f64) as u32;
                buffer[offset..offset + 4].copy_from_slice(&val.to_le_bytes());
            }
            RasterDataType::Int32 => {
                let val = write_val.clamp(i32::MIN as f64, i32::MAX as f64) as i32;
                buffer[offset..offset + 4].copy_from_slice(&val.to_le_bytes());
            }
            RasterDataType::Float32 => {
                let val = write_val as f32;
                buffer[offset..offset + 4].copy_from_slice(&val.to_le_bytes());
            }
            RasterDataType::Float64 => {
                buffer[offset..offset + 8].copy_from_slice(&write_val.to_le_bytes());
            }
            _ => return Err(VrtError::invalid_source("Unsupported data type")),
        }

        Ok(())
    }

    /// Opens a source dataset (with caching)
    fn open_source(&self, source: &VrtSource) -> Result<Arc<SourceDataset>> {
        let path = if let Some(ref vrt_path) = self.dataset.vrt_path {
            source.filename.resolve(vrt_path)?
        } else {
            source.filename.path.clone()
        };

        // Check cache first
        {
            let mut cache = self
                .source_cache
                .lock()
                .map_err(|_| VrtError::cache_error("Failed to lock source cache"))?;

            if let Some(dataset) = cache.get(&path) {
                return Ok(Arc::clone(dataset));
            }
        }

        // Open new dataset
        let dataset = SourceDataset::open(&path)?;
        let arc_dataset = Arc::new(dataset);

        // Add to cache
        {
            let mut cache = self
                .source_cache
                .lock()
                .map_err(|_| VrtError::cache_error("Failed to lock source cache"))?;
            cache.put(path, Arc::clone(&arc_dataset));
        }

        Ok(arc_dataset)
    }

    /// Clears the source cache
    pub fn clear_cache(&mut self) {
        if let Ok(mut cache) = self.source_cache.lock() {
            cache.clear();
        }
    }

    /// Gets the current cache size
    pub fn cache_size(&self) -> usize {
        self.source_cache
            .lock()
            .map(|cache| cache.len())
            .unwrap_or(0)
    }
}

/// Wrapper for source datasets
pub struct SourceDataset {
    /// GeoTIFF reader (for now, only GeoTIFF sources are supported)
    geotiff: Option<GeoTiffReader<FileDataSource>>,
}

impl SourceDataset {
    /// Opens a source dataset
    ///
    /// # Errors
    /// Returns an error if the file cannot be opened
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        // Try to open as GeoTIFF
        match FileDataSource::open(path.as_ref()) {
            Ok(source) => match GeoTiffReader::open(source) {
                Ok(reader) => Ok(Self {
                    geotiff: Some(reader),
                }),
                Err(e) => Err(VrtError::source_error(
                    path.as_ref().display().to_string(),
                    format!("Failed to open as GeoTIFF: {}", e),
                )),
            },
            Err(e) => Err(VrtError::source_error(
                path.as_ref().display().to_string(),
                format!("Failed to open file: {}", e),
            )),
        }
    }

    /// Reads a window from the source dataset
    ///
    /// # Errors
    /// Returns an error if reading fails
    pub fn read_window(&self, band: usize, window: PixelRect) -> Result<RasterBuffer> {
        if let Some(ref geotiff) = self.geotiff {
            // For now, we read the full band and extract the window
            // A more efficient implementation would read only the necessary tiles
            let full_band = geotiff.read_band(0, band - 1).map_err(|e| {
                VrtError::source_error("source", format!("Failed to read band: {}", e))
            })?;

            // Extract window
            let width = geotiff.width() as usize;
            let height = geotiff.height() as usize;
            let data_type = geotiff.data_type().unwrap_or(RasterDataType::UInt8);
            let bytes_per_pixel = data_type.size_bytes();

            let mut window_data = Vec::new();

            for y in 0..window.y_size {
                let src_y = (window.y_off + y) as usize;
                if src_y >= height {
                    break;
                }

                let src_offset = (src_y * width + window.x_off as usize) * bytes_per_pixel;
                let copy_width = window.x_size.min((width as u64) - window.x_off) as usize;
                let copy_bytes = copy_width * bytes_per_pixel;

                if src_offset + copy_bytes <= full_band.len() {
                    window_data.extend_from_slice(&full_band[src_offset..src_offset + copy_bytes]);
                }
            }

            RasterBuffer::new(
                window_data,
                window.x_size,
                window.y_size,
                data_type,
                geotiff.nodata(),
            )
            .map_err(|e| e.into())
        } else {
            Err(VrtError::source_error(
                "unknown",
                "Unsupported source format",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::band::VrtBand;
    use crate::source::VrtSource;

    #[test]
    fn test_vrt_reader_creation() {
        let mut dataset = VrtDataset::new(512, 512);
        let source = VrtSource::simple("/test.tif", 1);
        let band = VrtBand::simple(1, RasterDataType::UInt8, source);
        dataset.add_band(band);

        let reader = VrtReader::from_dataset(dataset);
        assert!(reader.is_ok());
        let r = reader.expect("Should create reader");
        assert_eq!(r.width(), 512);
        assert_eq!(r.height(), 512);
        assert_eq!(r.band_count(), 1);
    }

    #[test]
    fn test_cache() {
        let mut dataset = VrtDataset::new(512, 512);
        let source = VrtSource::simple("/test.tif", 1);
        let band = VrtBand::simple(1, RasterDataType::UInt8, source);
        dataset.add_band(band);

        let mut reader = VrtReader::from_dataset(dataset).expect("Should create reader");
        assert_eq!(reader.cache_size(), 0);

        reader.clear_cache();
        assert_eq!(reader.cache_size(), 0);
    }
}
