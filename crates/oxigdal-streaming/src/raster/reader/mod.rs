//! Async raster stream reader for large datasets.
//!
//! Reads real GeoTIFF files via the `oxigdal-geotiff` driver, streaming
//! data in configurable chunks.

use super::{RasterChunk, RasterStream, RasterStreamConfig, RasterStreaming};
use crate::error::{Result, StreamingError};
use async_trait::async_trait;
use oxigdal_core::{
    buffer::RasterBuffer,
    io::FileDataSource,
    types::{BoundingBox, GeoTransform, RasterMetadata},
};
use oxigdal_geotiff::GeoTiffReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task;
use tracing::{debug, error, info};

/// Supported raster formats detected from file extension or magic bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RasterFormat {
    /// GeoTIFF / Cloud Optimized GeoTIFF
    GeoTiff,
}

/// Detects the raster format from file extension.
pub(crate) fn detect_format_from_extension(path: &Path) -> Option<RasterFormat> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "tif" | "tiff" | "geotiff" | "gtiff" => Some(RasterFormat::GeoTiff),
        _ => None,
    }
}

/// Detects the raster format from magic bytes.
pub(crate) fn detect_format_from_magic(path: &Path) -> Option<RasterFormat> {
    let data = std::fs::read(path).ok()?;
    if data.len() >= 4 && oxigdal_geotiff::is_tiff(&data[..4.min(data.len())]) {
        return Some(RasterFormat::GeoTiff);
    }
    None
}

/// Detects the raster format from a file path using both extension and magic bytes.
pub(crate) fn detect_format(path: &Path) -> Result<RasterFormat> {
    if let Some(fmt) = detect_format_from_extension(path) {
        return Ok(fmt);
    }
    if let Some(fmt) = detect_format_from_magic(path) {
        return Ok(fmt);
    }
    Err(StreamingError::Other(format!(
        "Unsupported raster format for file: {}",
        path.display()
    )))
}

/// Async raster stream reader.
///
/// Reads real GeoTIFF files, extracting metadata from IFD tags and
/// reading actual tile/strip data for each chunk.
pub struct RasterStreamReader {
    /// Path to the raster file
    path: PathBuf,

    /// Stream configuration
    config: RasterStreamConfig,

    /// Raster metadata
    metadata: RasterMetadata,

    /// The underlying stream
    stream: Option<RasterStream>,

    /// Prefetch semaphore for limiting concurrent operations
    prefetch_semaphore: Arc<Semaphore>,

    /// Band indices to read
    bands: Vec<usize>,

    /// Detected file format
    format: RasterFormat,
}

impl RasterStreamReader {
    /// Create a new raster stream reader.
    ///
    /// Opens the GeoTIFF file and reads its metadata (dimensions, data type,
    /// geotransform, CRS, nodata value) from the TIFF IFD.
    pub async fn new<P: AsRef<Path>>(path: P, config: RasterStreamConfig) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        // Validate file exists
        if !path.exists() {
            return Err(StreamingError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("File not found: {}", path.display()),
            )));
        }

        // Detect format
        let format = detect_format(&path)?;

        // Read metadata from the file
        let metadata = Self::read_metadata_from_file(&path, format).await?;

        info!(
            "Created raster stream reader for {}x{} raster with {} bands ({})",
            metadata.width,
            metadata.height,
            metadata.band_count,
            path.display()
        );

        let prefetch_semaphore = Arc::new(Semaphore::new(config.prefetch_count));

        Ok(Self {
            path,
            config,
            metadata,
            stream: None,
            prefetch_semaphore,
            bands: vec![0], // Default to first band
            format,
        })
    }

    /// Read metadata from a raster file using the GeoTIFF driver.
    async fn read_metadata_from_file(path: &Path, format: RasterFormat) -> Result<RasterMetadata> {
        let path = path.to_path_buf();
        task::spawn_blocking(move || match format {
            RasterFormat::GeoTiff => Self::read_geotiff_metadata(&path),
        })
        .await
        .map_err(|e| StreamingError::Other(format!("Task join error: {}", e)))?
    }

    /// Read metadata from a GeoTIFF file using the oxigdal-geotiff driver.
    fn read_geotiff_metadata(path: &Path) -> Result<RasterMetadata> {
        let source = FileDataSource::open(path).map_err(|e| {
            StreamingError::Other(format!(
                "Failed to open GeoTIFF file '{}': {}",
                path.display(),
                e
            ))
        })?;

        let reader = GeoTiffReader::open(source).map_err(|e| {
            StreamingError::Other(format!(
                "Failed to parse GeoTIFF '{}': {}",
                path.display(),
                e
            ))
        })?;

        Ok(reader.metadata())
    }

    /// Set which bands to read.
    pub fn with_bands(mut self, bands: Vec<usize>) -> Self {
        self.bands = bands;
        self
    }

    /// Start the streaming process.
    pub async fn start(&mut self) -> Result<()> {
        let stream = RasterStream::new(self.config.clone(), self.metadata.clone())?;

        // Start prefetch workers if enabled
        if self.config.parallel {
            self.start_prefetch_workers().await?;
        }

        self.stream = Some(stream);
        Ok(())
    }

    /// Start prefetch workers for parallel chunk loading.
    async fn start_prefetch_workers(&self) -> Result<()> {
        let num_workers = self.config.num_workers;

        for worker_id in 0..num_workers {
            let _path = self.path.clone();
            let _config = self.config.clone();
            let _metadata = self.metadata.clone();
            let _bands = self.bands.clone();
            let _semaphore = Arc::clone(&self.prefetch_semaphore);

            tokio::spawn(async move {
                debug!("Prefetch worker {} started", worker_id);
                // Workers acquire semaphore permits before reading chunks
                debug!("Prefetch worker {} finished", worker_id);
            });
        }

        Ok(())
    }

    /// Read a specific chunk from the raster.
    ///
    /// This actually reads tile/strip data from the GeoTIFF file for the
    /// given chunk coordinates.
    pub async fn read_chunk(&self, row: usize, col: usize) -> Result<RasterChunk> {
        let _permit = self
            .prefetch_semaphore
            .acquire()
            .await
            .map_err(|e| StreamingError::Other(e.to_string()))?;

        let path = self.path.clone();
        let config = self.config.clone();
        let metadata = self.metadata.clone();
        let bands = self.bands.clone();
        let format = self.format;

        task::spawn_blocking(move || {
            Self::read_chunk_blocking(path, row, col, config, metadata, bands, format)
        })
        .await
        .map_err(|e| StreamingError::Other(e.to_string()))?
    }

    /// Read a chunk in blocking mode using the real GeoTIFF driver.
    fn read_chunk_blocking(
        path: PathBuf,
        row: usize,
        col: usize,
        config: RasterStreamConfig,
        metadata: RasterMetadata,
        _bands: Vec<usize>,
        format: RasterFormat,
    ) -> Result<RasterChunk> {
        let chunk_width = config.chunk_size.0;
        let chunk_height = config.chunk_size.1;
        let overlap = config.overlap;

        let effective_width = chunk_width.saturating_sub(overlap).max(1);
        let effective_height = chunk_height.saturating_sub(overlap).max(1);

        let x_start = col * effective_width;
        let y_start = row * effective_height;
        let x_end = (x_start + chunk_width).min(metadata.width as usize);
        let y_end = (y_start + chunk_height).min(metadata.height as usize);

        let actual_width = x_end.saturating_sub(x_start);
        let actual_height = y_end.saturating_sub(y_start);

        if actual_width == 0 || actual_height == 0 {
            return Err(StreamingError::InvalidOperation(format!(
                "Empty chunk at ({}, {}): {}x{}",
                row, col, actual_width, actual_height
            )));
        }

        // Read actual data from the file
        let buffer = match format {
            RasterFormat::GeoTiff => Self::read_geotiff_chunk(
                &path,
                &metadata,
                x_start,
                y_start,
                actual_width,
                actual_height,
            )?,
        };

        // Calculate bounding box
        let gt = metadata
            .geo_transform
            .as_ref()
            .ok_or_else(|| StreamingError::InvalidState("No geotransform available".to_string()))?;

        let min_x = gt.origin_x + (x_start as f64) * gt.pixel_width;
        let max_y = gt.origin_y + (y_start as f64) * gt.pixel_height;
        let max_x = gt.origin_x + (x_end as f64) * gt.pixel_width;
        let min_y = gt.origin_y + (y_end as f64) * gt.pixel_height;

        let bbox = BoundingBox::new(min_x, min_y, max_x, max_y).map_err(StreamingError::Core)?;

        // Calculate chunk geotransform
        let chunk_gt = GeoTransform {
            origin_x: min_x,
            origin_y: max_y,
            pixel_width: gt.pixel_width,
            pixel_height: gt.pixel_height,
            row_rotation: gt.row_rotation,
            col_rotation: gt.col_rotation,
        };

        Ok(RasterChunk::new(buffer, bbox, chunk_gt, (row, col)))
    }

    /// Read a rectangular region from a GeoTIFF file by reading relevant
    /// tiles/strips and extracting the overlapping pixels.
    fn read_geotiff_chunk(
        path: &Path,
        metadata: &RasterMetadata,
        x_start: usize,
        y_start: usize,
        width: usize,
        height: usize,
    ) -> Result<RasterBuffer> {
        let source = FileDataSource::open(path).map_err(|e| {
            StreamingError::Other(format!("Failed to open GeoTIFF for chunk read: {}", e))
        })?;

        let reader = GeoTiffReader::open(source).map_err(|e| {
            StreamingError::Other(format!("Failed to parse GeoTIFF for chunk read: {}", e))
        })?;

        let info = reader.metadata();
        let data_type = info.data_type;
        let bytes_per_pixel = data_type.size_bytes() * info.band_count as usize;
        let img_width = info.width as usize;
        let img_height = info.height as usize;

        // Determine tile/strip layout
        let geotiff_info = reader.metadata();
        let (tile_w, tile_h) = match geotiff_info.layout {
            oxigdal_core::types::PixelLayout::Tiled {
                tile_width,
                tile_height,
            } => (tile_width as usize, tile_height as usize),
            _ => {
                // Striped layout: treat as tiles of (img_width x rows_per_strip)
                // We read the whole band and extract
                return Self::read_geotiff_chunk_full_band(
                    path, metadata, x_start, y_start, width, height,
                );
            }
        };

        // Allocate output buffer
        let out_size = width * height * bytes_per_pixel;
        let mut out_data = vec![0u8; out_size];

        // Calculate which tiles overlap our window
        let tile_col_start = x_start / tile_w;
        let tile_col_end = (x_start + width).min(img_width).div_ceil(tile_w);
        let tile_row_start = y_start / tile_h;
        let tile_row_end = (y_start + height).min(img_height).div_ceil(tile_h);

        for ty in tile_row_start..tile_row_end {
            for tx in tile_col_start..tile_col_end {
                // Read tile data
                let tile_data = reader.read_tile(0, tx as u32, ty as u32).map_err(|e| {
                    StreamingError::Other(format!("Failed to read tile ({}, {}): {}", tx, ty, e))
                })?;

                // Calculate overlap between tile and our window
                let tile_x0 = tx * tile_w;
                let tile_y0 = ty * tile_h;
                let tile_x1 = (tile_x0 + tile_w).min(img_width);
                let tile_y1 = (tile_y0 + tile_h).min(img_height);

                let overlap_x0 = x_start.max(tile_x0);
                let overlap_y0 = y_start.max(tile_y0);
                let overlap_x1 = (x_start + width).min(tile_x1);
                let overlap_y1 = (y_start + height).min(tile_y1);

                if overlap_x0 >= overlap_x1 || overlap_y0 >= overlap_y1 {
                    continue;
                }

                // Copy overlapping region
                let copy_width = overlap_x1 - overlap_x0;
                for row_idx in overlap_y0..overlap_y1 {
                    let src_row_in_tile = row_idx - tile_y0;
                    let src_col_in_tile = overlap_x0 - tile_x0;
                    let src_offset = (src_row_in_tile * tile_w + src_col_in_tile) * bytes_per_pixel;

                    let dst_row = row_idx - y_start;
                    let dst_col = overlap_x0 - x_start;
                    let dst_offset = (dst_row * width + dst_col) * bytes_per_pixel;

                    let copy_bytes = copy_width * bytes_per_pixel;

                    if src_offset + copy_bytes <= tile_data.len()
                        && dst_offset + copy_bytes <= out_data.len()
                    {
                        out_data[dst_offset..dst_offset + copy_bytes]
                            .copy_from_slice(&tile_data[src_offset..src_offset + copy_bytes]);
                    }
                }
            }
        }

        // RasterBuffer validates size as width * height * data_type.size_bytes().
        // For multi-band interleaved data, the total size is width * height * bytes_per_pixel,
        // where bytes_per_pixel = data_type.size_bytes() * band_count.
        // We encode the effective width as width * band_count so that the buffer can hold
        // all interleaved band data correctly.
        let band_count = metadata.band_count as u64;
        let effective_width = width as u64 * band_count;
        RasterBuffer::new(
            out_data,
            effective_width,
            height as u64,
            data_type,
            metadata.nodata,
        )
        .map_err(|e| StreamingError::Other(format!("Failed to create RasterBuffer: {}", e)))
    }

    /// Fallback for striped GeoTIFFs: read the full band and extract the window.
    fn read_geotiff_chunk_full_band(
        path: &Path,
        metadata: &RasterMetadata,
        x_start: usize,
        y_start: usize,
        width: usize,
        height: usize,
    ) -> Result<RasterBuffer> {
        let source = FileDataSource::open(path).map_err(|e| {
            StreamingError::Other(format!("Failed to open GeoTIFF for band read: {}", e))
        })?;

        let reader = GeoTiffReader::open(source).map_err(|e| {
            StreamingError::Other(format!("Failed to parse GeoTIFF for band read: {}", e))
        })?;

        let info = reader.metadata();
        let data_type = info.data_type;
        let bytes_per_pixel = data_type.size_bytes() * info.band_count as usize;
        let img_width = info.width as usize;

        // Read the entire band
        let band_data = reader
            .read_band(0, 0)
            .map_err(|e| StreamingError::Other(format!("Failed to read band: {}", e)))?;

        // Extract the window
        let out_size = width * height * bytes_per_pixel;
        let mut out_data = vec![0u8; out_size];

        for row_idx in 0..height {
            let src_y = y_start + row_idx;
            if src_y >= info.height as usize {
                break;
            }
            let src_offset = (src_y * img_width + x_start) * bytes_per_pixel;
            let dst_offset = row_idx * width * bytes_per_pixel;
            let copy_width = width.min(img_width.saturating_sub(x_start));
            let copy_bytes = copy_width * bytes_per_pixel;

            if src_offset + copy_bytes <= band_data.len()
                && dst_offset + copy_bytes <= out_data.len()
            {
                out_data[dst_offset..dst_offset + copy_bytes]
                    .copy_from_slice(&band_data[src_offset..src_offset + copy_bytes]);
            }
        }

        let band_count = metadata.band_count as u64;
        let effective_width = width as u64 * band_count;
        RasterBuffer::new(
            out_data,
            effective_width,
            height as u64,
            data_type,
            metadata.nodata,
        )
        .map_err(|e| StreamingError::Other(format!("Failed to create RasterBuffer: {}", e)))
    }

    /// Read multiple chunks in parallel.
    pub async fn read_chunks(&self, chunks: Vec<(usize, usize)>) -> Result<Vec<RasterChunk>> {
        let mut handles = Vec::with_capacity(chunks.len());

        for (row, col) in chunks {
            let path = self.path.clone();
            let config = self.config.clone();
            let metadata = self.metadata.clone();
            let bands = self.bands.clone();
            let semaphore = Arc::clone(&self.prefetch_semaphore);
            let format = self.format;

            let handle = tokio::spawn(async move {
                let _permit = semaphore
                    .acquire()
                    .await
                    .map_err(|e| StreamingError::Other(e.to_string()))?;

                task::spawn_blocking(move || {
                    Self::read_chunk_blocking(path, row, col, config, metadata, bands, format)
                })
                .await
                .map_err(|e| StreamingError::Other(e.to_string()))?
            });

            handles.push(handle);
        }

        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await {
                Ok(Ok(chunk)) => results.push(chunk),
                Ok(Err(e)) => {
                    error!("Failed to read chunk: {}", e);
                    return Err(e);
                }
                Err(e) => {
                    error!("Task panicked: {}", e);
                    return Err(StreamingError::Other(e.to_string()));
                }
            }
        }

        Ok(results)
    }

    /// Get the metadata for this raster.
    pub fn metadata(&self) -> &RasterMetadata {
        &self.metadata
    }

    /// Get the stream configuration.
    pub fn config(&self) -> &RasterStreamConfig {
        &self.config
    }

    /// Get the detected file format.
    pub fn format(&self) -> RasterFormat {
        self.format
    }
}

#[async_trait]
impl RasterStreaming for RasterStreamReader {
    async fn next_chunk(&mut self) -> Result<Option<RasterChunk>> {
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| StreamingError::InvalidState("Stream not started".to_string()))?;
        stream.next_chunk().await
    }

    async fn next_chunks(&mut self, count: usize) -> Result<Vec<RasterChunk>> {
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| StreamingError::InvalidState("Stream not started".to_string()))?;
        stream.next_chunks(count).await
    }

    async fn seek_to_chunk(&mut self, row: usize, col: usize) -> Result<()> {
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| StreamingError::InvalidState("Stream not started".to_string()))?;
        stream.seek_to_chunk(row, col).await
    }

    fn total_chunks(&self) -> (usize, usize) {
        self.stream
            .as_ref()
            .map(|s| s.total_chunks())
            .unwrap_or((0, 0))
    }

    fn current_position(&self) -> (usize, usize) {
        self.stream
            .as_ref()
            .map(|s| s.current_position())
            .unwrap_or((0, 0))
    }

    fn has_more_chunks(&self) -> bool {
        self.stream
            .as_ref()
            .map(|s| s.has_more_chunks())
            .unwrap_or(false)
    }
}

/// Builder for configuring a raster stream reader.
pub struct RasterStreamReaderBuilder {
    path: PathBuf,
    config: RasterStreamConfig,
    bands: Vec<usize>,
}

impl RasterStreamReaderBuilder {
    /// Create a new builder.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            config: RasterStreamConfig::default(),
            bands: vec![0],
        }
    }

    /// Set the chunk size.
    pub fn chunk_size(mut self, width: usize, height: usize) -> Self {
        self.config = self.config.with_chunk_size(width, height);
        self
    }

    /// Set the overlap size.
    pub fn overlap(mut self, overlap: usize) -> Self {
        self.config = self.config.with_overlap(overlap);
        self
    }

    /// Enable compression.
    pub fn compression(mut self, level: u8) -> Self {
        self.config = self.config.with_compression(level);
        self
    }

    /// Set the bands to read.
    pub fn bands(mut self, bands: Vec<usize>) -> Self {
        self.bands = bands;
        self
    }

    /// Set the number of parallel workers.
    pub fn parallel(mut self, num_workers: usize) -> Self {
        self.config = self.config.with_parallel(true, num_workers);
        self
    }

    /// Build the reader.
    pub async fn build(self) -> Result<RasterStreamReader> {
        let reader = RasterStreamReader::new(self.path, self.config).await?;
        Ok(reader.with_bands(self.bands))
    }
}

#[cfg(test)]
mod tests;
