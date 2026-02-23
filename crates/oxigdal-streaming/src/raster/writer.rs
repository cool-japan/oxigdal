//! Async raster stream writer for large datasets.

use super::{RasterChunk, RasterStreamConfig, ChunkStats};
use crate::error::{Result, StreamingError};
use oxigdal_core::{
    buffer::RasterBuffer,
    types::{RasterDataType, RasterMetadata},
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, RwLock, Semaphore};
use tokio::task;
use tracing::{debug, error, info, warn};

/// Async raster stream writer.
pub struct RasterStreamWriter {
    /// Path to the output raster file
    path: PathBuf,

    /// Stream configuration
    config: RasterStreamConfig,

    /// Raster metadata
    metadata: RasterMetadata,

    /// Channel for receiving chunks to write
    sender: mpsc::Sender<WriteRequest>,

    /// Write semaphore for limiting concurrent operations
    write_semaphore: Arc<Semaphore>,

    /// Statistics
    stats: Arc<RwLock<ChunkStats>>,

    /// Chunk cache for reordering
    chunk_cache: Arc<RwLock<HashMap<(usize, usize), RasterChunk>>>,

    /// Expected chunk order
    next_chunk: Arc<RwLock<(usize, usize)>>,

    /// Total chunks
    total_chunks: (usize, usize),
}

/// A write request for a raster chunk.
#[derive(Debug)]
struct WriteRequest {
    chunk: RasterChunk,
    response: tokio::sync::oneshot::Sender<Result<()>>,
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
            metadata.width,
            metadata.height,
            config.chunk_size.0,
            config.chunk_size.1,
            config.overlap,
        );

        let (sender, mut receiver) = mpsc::channel::<WriteRequest>(config.buffer_size);
        let write_semaphore = Arc::new(Semaphore::new(config.num_workers));
        let stats = Arc::new(RwLock::new(ChunkStats::new()));
        let chunk_cache = Arc::new(RwLock::new(HashMap::new()));
        let next_chunk = Arc::new(RwLock::new((0, 0)));

        info!(
            "Created raster stream writer for {}x{} raster with {} x {} chunks",
            metadata.width, metadata.height, total_chunks.0, total_chunks.1
        );

        // Start the write worker
        let write_path = path.clone();
        let write_stats = Arc::clone(&stats);
        let write_cache = Arc::clone(&chunk_cache);
        let write_next = Arc::clone(&next_chunk);
        let write_semaphore_clone = Arc::clone(&write_semaphore);

        tokio::spawn(async move {
            while let Some(request) = receiver.recv().await {
                let _permit = write_semaphore_clone.acquire().await;

                let result = Self::write_chunk_async(
                    &write_path,
                    request.chunk,
                    &write_stats,
                    &write_cache,
                    &write_next,
                    total_chunks,
                )
                .await;

                let _ = request.response.send(result);
            }
        });

        Ok(Self {
            path,
            config,
            metadata,
            sender,
            write_semaphore,
            stats,
            chunk_cache,
            next_chunk,
            total_chunks,
        })
    }

    /// Calculate the number of chunks needed.
    fn calculate_chunks(
        width: usize,
        height: usize,
        chunk_width: usize,
        chunk_height: usize,
        overlap: usize,
    ) -> (usize, usize) {
        let effective_chunk_width = chunk_width - overlap;
        let effective_chunk_height = chunk_height - overlap;

        let num_cols = (width + effective_chunk_width - 1) / effective_chunk_width;
        let num_rows = (height + effective_chunk_height - 1) / effective_chunk_height;

        (num_rows, num_cols)
    }

    /// Write a chunk to the raster.
    pub async fn write_chunk(&self, chunk: RasterChunk) -> Result<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let request = WriteRequest {
            chunk,
            response: tx,
        };

        self.sender
            .send(request)
            .await
            .map_err(|_| StreamingError::SendError)?;

        rx.await
            .map_err(|_| StreamingError::RecvError)?
    }

    /// Write multiple chunks in parallel.
    pub async fn write_chunks(&self, chunks: Vec<RasterChunk>) -> Result<()> {
        let mut handles = Vec::with_capacity(chunks.len());

        for chunk in chunks {
            let sender = self.sender.clone();

            let handle = tokio::spawn(async move {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let request = WriteRequest {
                    chunk,
                    response: tx,
                };

                sender
                    .send(request)
                    .await
                    .map_err(|_| StreamingError::SendError)?;

                rx.await
                    .map_err(|_| StreamingError::RecvError)?
            });

            handles.push(handle);
        }

        for handle in handles {
            handle
                .await
                .map_err(|e| StreamingError::Other(e.to_string()))??;
        }

        Ok(())
    }

    /// Write a chunk asynchronously with ordering.
    async fn write_chunk_async(
        path: &Path,
        chunk: RasterChunk,
        stats: &Arc<RwLock<ChunkStats>>,
        cache: &Arc<RwLock<HashMap<(usize, usize), RasterChunk>>>,
        next_chunk: &Arc<RwLock<(usize, usize)>>,
        total_chunks: (usize, usize),
    ) -> Result<()> {
        let start = Instant::now();

        // Check if this is the next expected chunk
        let expected = *next_chunk.read().await;
        if chunk.indices != expected {
            // Cache this chunk for later
            debug!("Caching chunk {:?}, expected {:?}", chunk.indices, expected);
            let mut cache_guard = cache.write().await;
            cache_guard.insert(chunk.indices, chunk);
            return Ok(());
        }

        // Write this chunk
        Self::write_chunk_blocking(path, &chunk).await?;

        let elapsed = start.elapsed().as_millis() as u64;
        let mut stats_guard = stats.write().await;
        stats_guard.record_chunk(chunk.size_bytes(), elapsed);
        drop(stats_guard);

        // Update next chunk
        let mut next_guard = next_chunk.write().await;
        next_guard.1 += 1;
        if next_guard.1 >= total_chunks.1 {
            next_guard.1 = 0;
            next_guard.0 += 1;
        }
        let next_expected = *next_guard;
        drop(next_guard);

        // Check if the next chunk is in cache
        let mut cache_guard = cache.write().await;
        if let Some(cached_chunk) = cache_guard.remove(&next_expected) {
            drop(cache_guard);
            // Recursively write cached chunks
            Self::write_chunk_async(path, cached_chunk, stats, cache, next_chunk, total_chunks).await?;
        }

        Ok(())
    }

    /// Write a chunk in blocking mode.
    async fn write_chunk_blocking(path: &Path, chunk: &RasterChunk) -> Result<()> {
        let path = path.to_path_buf();
        let indices = chunk.indices;
        let size = chunk.size_bytes();

        task::spawn_blocking(move || {
            // Placeholder for actual write implementation
            debug!("Writing chunk {:?} ({} bytes)", indices, size);
            Ok(())
        })
        .await
        .map_err(|e| StreamingError::Other(e.to_string()))?
    }

    /// Get statistics for written chunks.
    pub async fn stats(&self) -> ChunkStats {
        self.stats.read().await.clone()
    }

    /// Flush all pending writes.
    pub async fn flush(&self) -> Result<()> {
        // Wait for all cached chunks to be written
        loop {
            let cache_size = self.chunk_cache.read().await.len();
            if cache_size == 0 {
                break;
            }
            debug!("Waiting for {} cached chunks to be written", cache_size);
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        info!("All chunks flushed");
        Ok(())
    }

    /// Finalize the raster file.
    pub async fn finalize(&self) -> Result<()> {
        self.flush().await?;

        // Write final metadata, overviews, etc.
        let path = self.path.clone();
        task::spawn_blocking(move || {
            debug!("Finalizing raster file: {:?}", path);
            // Placeholder for finalization logic
            Ok(())
        })
        .await
        .map_err(|e| StreamingError::Other(e.to_string()))?
    }

    /// Get the total number of chunks.
    pub fn total_chunks(&self) -> (usize, usize) {
        self.total_chunks
    }

    /// Get the current write position.
    pub async fn current_position(&self) -> (usize, usize) {
        *self.next_chunk.read().await
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
    use oxigdal_core::types::GeoTransform;
    use std::env;

    #[tokio::test]
    async fn test_writer_creation() {
        let temp_dir = env::temp_dir();
        let test_path = temp_dir.join("test_output.tif");

        let metadata = RasterMetadata {
            width: 1024,
            height: 1024,
            bands: 1,
            data_type: RasterDataType::Float32,
            geotransform: Some(GeoTransform {
                origin_x: 0.0,
                origin_y: 0.0,
                pixel_width: 1.0,
                pixel_height: -1.0,
                rotation_x: 0.0,
                rotation_y: 0.0,
            }),
            crs: None,
            no_data_value: None,
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
