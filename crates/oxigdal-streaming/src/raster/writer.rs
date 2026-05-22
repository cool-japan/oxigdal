//! Async raster stream writer for large datasets.
//!
//! Writes GeoTIFF files by accumulating chunk data and flushing the complete
//! image on [`RasterStreamWriter::finalize`].

use super::{ChunkStats, RasterChunk, RasterStreamConfig};
use crate::error::{Result, StreamingError};
use oxigdal_core::types::RasterMetadata;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::info;

/// Async raster stream writer.
///
/// Accepts chunks via [`write_chunk`](Self::write_chunk) and writes a complete
/// GeoTIFF when [`finalize`](Self::finalize) is called.
pub struct RasterStreamWriter {
    /// Path to the output raster file
    path: PathBuf,

    /// Stream configuration
    config: RasterStreamConfig,

    /// Raster metadata
    metadata: RasterMetadata,

    /// Statistics
    stats: Arc<RwLock<ChunkStats>>,

    /// Accumulated chunk data indexed by (row, col)
    chunk_store: Arc<RwLock<HashMap<(usize, usize), RasterChunk>>>,

    /// Total chunks
    total_chunks: (usize, usize),
}

impl RasterStreamWriter {
    /// Create a new raster stream writer.
    pub async fn new<P: AsRef<Path>>(
        path: P,
        metadata: RasterMetadata,
        config: RasterStreamConfig,
    ) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        // Calculate total chunks
        let total_chunks = Self::calculate_chunks(
            metadata.width as usize,
            metadata.height as usize,
            config.chunk_size.0,
            config.chunk_size.1,
            config.overlap,
        );

        info!(
            "Created raster stream writer for {}x{} raster ({} x {} chunks)",
            metadata.width, metadata.height, total_chunks.0, total_chunks.1
        );

        Ok(Self {
            path,
            config,
            metadata,
            stats: Arc::new(RwLock::new(ChunkStats::new())),
            chunk_store: Arc::new(RwLock::new(HashMap::new())),
            total_chunks,
        })
    }

    /// Calculate the number of chunks needed.
    pub fn calculate_chunks(
        width: usize,
        height: usize,
        chunk_width: usize,
        chunk_height: usize,
        overlap: usize,
    ) -> (usize, usize) {
        let effective_chunk_width = chunk_width - overlap;
        let effective_chunk_height = chunk_height - overlap;

        let num_cols = width.div_ceil(effective_chunk_width);
        let num_rows = height.div_ceil(effective_chunk_height);

        (num_rows, num_cols)
    }

    /// Write a chunk to the raster.
    ///
    /// Chunks are stored in memory and written to disk on [`finalize`](Self::finalize).
    pub async fn write_chunk(&self, chunk: RasterChunk) -> Result<()> {
        let start = Instant::now();
        let size = chunk.size_bytes();
        let indices = chunk.indices;

        let mut store = self.chunk_store.write().await;
        store.insert(indices, chunk);
        drop(store);

        let elapsed = start.elapsed().as_millis() as u64;
        let mut stats_guard = self.stats.write().await;
        stats_guard.record_chunk(size, elapsed);

        Ok(())
    }

    /// Write multiple chunks.
    pub async fn write_chunks(&self, chunks: Vec<RasterChunk>) -> Result<()> {
        for chunk in chunks {
            self.write_chunk(chunk).await?;
        }
        Ok(())
    }

    /// Get statistics for written chunks.
    pub async fn stats(&self) -> ChunkStats {
        self.stats.read().await.clone()
    }

    /// Flush — no-op for the accumulation model (all data is held in memory).
    pub async fn flush(&self) -> Result<()> {
        Ok(())
    }

    /// Finalize the raster file.
    ///
    /// Assembles all accumulated chunks into a contiguous pixel buffer and
    /// writes a GeoTIFF using the `oxigdal-geotiff` driver.
    pub async fn finalize(&self) -> Result<()> {
        let img_width = self.metadata.width as usize;
        let img_height = self.metadata.height as usize;
        let band_count = self.metadata.band_count as usize;
        let data_type = self.metadata.data_type;
        let bytes_per_sample = data_type.size_bytes();
        let bytes_per_pixel = bytes_per_sample * band_count;
        let total_bytes = img_width * img_height * bytes_per_pixel;

        // Assemble chunks into a contiguous image buffer
        let mut image_data = vec![0u8; total_bytes];
        let chunk_store = self.chunk_store.read().await;

        let chunk_w = self.config.chunk_size.0;
        let chunk_h = self.config.chunk_size.1;
        let overlap = self.config.overlap;
        let effective_w = chunk_w - overlap;
        let effective_h = chunk_h - overlap;

        for (&(row, col), chunk) in chunk_store.iter() {
            let x_start = col * effective_w;
            let y_start = row * effective_h;
            let src_width = chunk.buffer.width() as usize / band_count; // actual pixel width
            let src_height = chunk.buffer.height() as usize;
            let src_bytes = chunk.buffer.as_bytes();

            for cy in 0..src_height {
                let dst_y = y_start + cy;
                if dst_y >= img_height {
                    break;
                }
                for cx in 0..src_width {
                    let dst_x = x_start + cx;
                    if dst_x >= img_width {
                        break;
                    }
                    let src_offset = (cy * src_width + cx) * bytes_per_pixel;
                    let dst_offset = (dst_y * img_width + dst_x) * bytes_per_pixel;

                    if src_offset + bytes_per_pixel <= src_bytes.len()
                        && dst_offset + bytes_per_pixel <= image_data.len()
                    {
                        image_data[dst_offset..dst_offset + bytes_per_pixel]
                            .copy_from_slice(&src_bytes[src_offset..src_offset + bytes_per_pixel]);
                    }
                }
            }
        }
        drop(chunk_store);

        // Write the assembled image to disk
        let path = self.path.clone();
        let meta = self.metadata.clone();
        tokio::task::spawn_blocking(move || Self::write_geotiff_blocking(&path, &meta, &image_data))
            .await
            .map_err(|e| StreamingError::Other(format!("finalize task failed: {}", e)))?
    }

    /// Write a complete image to a GeoTIFF file.
    fn write_geotiff_blocking(path: &Path, metadata: &RasterMetadata, data: &[u8]) -> Result<()> {
        use oxigdal_geotiff::tiff::{Compression, PhotometricInterpretation, Predictor};
        use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};

        let photometric = if metadata.band_count >= 3 {
            PhotometricInterpretation::Rgb
        } else {
            PhotometricInterpretation::BlackIsZero
        };

        let mut config = WriterConfig::new(
            metadata.width,
            metadata.height,
            metadata.band_count as u16,
            metadata.data_type,
        )
        .with_compression(Compression::Lzw)
        .with_predictor(Predictor::HorizontalDifferencing)
        .with_tile_size(256, 256)
        .with_photometric(photometric)
        .with_nodata(metadata.nodata)
        .with_overviews(false, oxigdal_geotiff::OverviewResampling::Average);

        if let Some(gt) = metadata.geo_transform {
            config = config.with_geo_transform(gt);
        }

        let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())
            .map_err(|e| {
                StreamingError::Other(format!(
                    "Failed to create GeoTIFF writer for '{}': {}",
                    path.display(),
                    e
                ))
            })?;

        writer.write(data).map_err(|e| {
            StreamingError::Other(format!(
                "Failed to write GeoTIFF data to '{}': {}",
                path.display(),
                e
            ))
        })?;

        info!("Finalized GeoTIFF: {}", path.display());
        Ok(())
    }

    /// Get the total number of chunks.
    pub fn total_chunks(&self) -> (usize, usize) {
        self.total_chunks
    }

    /// Get the current write position (number of chunks written so far).
    pub async fn current_position(&self) -> (usize, usize) {
        let store = self.chunk_store.read().await;
        let count = store.len();
        drop(store);
        // Convert linear count to (row, col) for compatibility
        let cols = self.total_chunks.1.max(1);
        (count / cols, count % cols)
    }
}

/// Builder for configuring a raster stream writer.
pub struct RasterStreamWriterBuilder {
    path: PathBuf,
    metadata: RasterMetadata,
    config: RasterStreamConfig,
}

impl RasterStreamWriterBuilder {
    /// Create a new builder.
    pub fn new<P: AsRef<Path>>(path: P, metadata: RasterMetadata) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            metadata,
            config: RasterStreamConfig::default(),
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

    /// Set the number of parallel workers.
    pub fn parallel(mut self, num_workers: usize) -> Self {
        self.config = self.config.with_parallel(true, num_workers);
        self
    }

    /// Build the writer.
    pub async fn build(self) -> Result<RasterStreamWriter> {
        RasterStreamWriter::new(self.path, self.metadata, self.config).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::types::{GeoTransform, NoDataValue, PixelLayout, RasterDataType};
    use std::env;

    #[tokio::test]
    async fn test_writer_creation() {
        let temp_dir = env::temp_dir();
        let test_path = temp_dir.join("test_output.tif");

        let metadata = RasterMetadata {
            width: 1024,
            height: 1024,
            band_count: 1,
            data_type: RasterDataType::Float32,
            geo_transform: Some(GeoTransform {
                origin_x: 0.0,
                origin_y: 0.0,
                pixel_width: 1.0,
                pixel_height: -1.0,
                row_rotation: 0.0,
                col_rotation: 0.0,
            }),
            crs_wkt: None,
            nodata: NoDataValue::None,
            color_interpretation: Vec::new(),
            layout: PixelLayout::default(),
            driver_metadata: Vec::new(),
            statistics: None,
        };

        let result = RasterStreamWriterBuilder::new(&test_path, metadata)
            .chunk_size(256, 256)
            .compression(6)
            .parallel(4)
            .build()
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_chunk_calculation() {
        let chunks = RasterStreamWriter::calculate_chunks(1024, 1024, 256, 256, 0);
        assert_eq!(chunks, (4, 4));

        let chunks = RasterStreamWriter::calculate_chunks(1000, 1000, 256, 256, 0);
        assert_eq!(chunks, (4, 4));
    }
}
