//! Core raster streaming types and implementations.

use crate::core::StreamElement;
use crate::error::{Result, StreamingError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use oxigeo_core::{
    buffer::RasterBuffer,
    types::{BoundingBox, GeoTransform, RasterMetadata},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tracing::{info, warn};

/// Configuration for raster streaming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RasterStreamConfig {
    /// Chunk size in pixels (width, height)
    pub chunk_size: (usize, usize),

    /// Overlap between chunks in pixels
    pub overlap: usize,

    /// Buffer size for the stream
    pub buffer_size: usize,

    /// Whether to enable compression
    pub compression: bool,

    /// Compression level (1-9)
    pub compression_level: u8,

    /// Maximum memory usage in bytes
    pub max_memory_bytes: usize,

    /// Number of prefetch chunks
    pub prefetch_count: usize,

    /// Enable parallel chunk processing
    pub parallel: bool,

    /// Number of parallel workers
    pub num_workers: usize,
}

impl Default for RasterStreamConfig {
    fn default() -> Self {
        Self {
            chunk_size: (512, 512),
            overlap: 0,
            buffer_size: 100,
            compression: false,
            compression_level: 6,
            max_memory_bytes: 1024 * 1024 * 1024, // 1GB
            prefetch_count: 2,
            parallel: true,
            num_workers: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4),
        }
    }
}

impl RasterStreamConfig {
    /// Create a new configuration with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the chunk size.
    pub fn with_chunk_size(mut self, width: usize, height: usize) -> Self {
        self.chunk_size = (width, height);
        self
    }

    /// Set the overlap size.
    pub fn with_overlap(mut self, overlap: usize) -> Self {
        self.overlap = overlap;
        self
    }

    /// Enable compression.
    pub fn with_compression(mut self, level: u8) -> Self {
        self.compression = true;
        self.compression_level = level;
        self
    }

    /// Set the maximum memory usage.
    pub fn with_max_memory(mut self, bytes: usize) -> Self {
        self.max_memory_bytes = bytes;
        self
    }

    /// Set the number of prefetch chunks.
    pub fn with_prefetch(mut self, count: usize) -> Self {
        self.prefetch_count = count;
        self
    }

    /// Enable or disable parallel processing.
    pub fn with_parallel(mut self, parallel: bool, num_workers: usize) -> Self {
        self.parallel = parallel;
        self.num_workers = num_workers;
        self
    }
}

/// A chunk of raster data with spatial metadata.
#[derive(Debug, Clone)]
pub struct RasterChunk {
    /// The raster data buffer
    pub buffer: RasterBuffer,

    /// Bounding box of this chunk
    pub bbox: BoundingBox,

    /// Geotransform for this chunk
    pub geotransform: GeoTransform,

    /// Chunk indices (row, col)
    pub indices: (usize, usize),

    /// Timestamp when chunk was created
    pub timestamp: DateTime<Utc>,

    /// Chunk metadata
    pub metadata: ChunkMetadata,
}

impl RasterChunk {
    /// Create a new raster chunk.
    pub fn new(
        buffer: RasterBuffer,
        bbox: BoundingBox,
        geotransform: GeoTransform,
        indices: (usize, usize),
    ) -> Self {
        Self {
            buffer,
            bbox,
            geotransform,
            indices,
            timestamp: Utc::now(),
            metadata: ChunkMetadata::default(),
        }
    }

    /// Get the size in bytes of this chunk.
    pub fn size_bytes(&self) -> usize {
        self.buffer.as_bytes().len()
    }

    /// Check if this chunk overlaps with another.
    pub fn overlaps_with(&self, other: &RasterChunk) -> bool {
        self.bbox.intersects(&other.bbox)
    }

    /// Get the overlap region with another chunk.
    pub fn overlap_region(&self, other: &RasterChunk) -> Option<BoundingBox> {
        self.bbox.intersection(&other.bbox)
    }

    /// Convert to a stream element using JSON serialization of spatial metadata
    /// and a raw bytes payload for the raster data.
    pub fn to_stream_element(&self) -> Result<StreamElement> {
        // Encode spatial metadata as JSON, then append raw pixel bytes.
        // Layout: [8 bytes: json_len][json_len bytes: JSON][remaining: raw pixels]
        let meta = serde_json::json!({
            "bbox": {
                "min_x": self.bbox.min_x(),
                "min_y": self.bbox.min_y(),
                "max_x": self.bbox.max_x(),
                "max_y": self.bbox.max_y(),
            },
            "gt": {
                "origin_x": self.geotransform.origin_x,
                "origin_y": self.geotransform.origin_y,
                "pixel_width": self.geotransform.pixel_width,
                "pixel_height": self.geotransform.pixel_height,
                "row_rotation": self.geotransform.row_rotation,
                "col_rotation": self.geotransform.col_rotation,
            },
            "row": self.indices.0,
            "col": self.indices.1,
            "width": self.buffer.width(),
            "height": self.buffer.height(),
            "data_type": format!("{:?}", self.buffer.data_type()),
        });
        let json_bytes = serde_json::to_vec(&meta)
            .map_err(|e| StreamingError::SerializationError(e.to_string()))?;
        let json_len = json_bytes.len() as u64;
        let pixel_bytes = self.buffer.as_bytes();

        let mut data = Vec::with_capacity(8 + json_bytes.len() + pixel_bytes.len());
        data.extend_from_slice(&json_len.to_le_bytes());
        data.extend_from_slice(&json_bytes);
        data.extend_from_slice(pixel_bytes);

        Ok(StreamElement::new(data, self.timestamp))
    }

    /// Reconstruct from a stream element.  Only the raw pixel buffer and
    /// spatial metadata are restored; band-level metadata is left at defaults.
    pub fn from_stream_element(element: &StreamElement) -> Result<Self> {
        use oxigeo_core::types::{BoundingBox, NoDataValue, RasterDataType};

        let data = &element.data;
        if data.len() < 8 {
            return Err(StreamingError::DeserializationError(
                "stream element too short to contain header".to_string(),
            ));
        }
        let json_len = u64::from_le_bytes(
            data[..8]
                .try_into()
                .map_err(|_| StreamingError::DeserializationError("bad header".to_string()))?,
        ) as usize;
        if data.len() < 8 + json_len {
            return Err(StreamingError::DeserializationError(
                "stream element truncated: JSON metadata missing".to_string(),
            ));
        }
        let meta: serde_json::Value = serde_json::from_slice(&data[8..8 + json_len])
            .map_err(|e| StreamingError::DeserializationError(e.to_string()))?;

        let pixel_bytes = data[8 + json_len..].to_vec();

        let width = meta["width"]
            .as_u64()
            .ok_or_else(|| StreamingError::DeserializationError("missing width".to_string()))?;
        let height = meta["height"]
            .as_u64()
            .ok_or_else(|| StreamingError::DeserializationError("missing height".to_string()))?;
        let row = meta["row"]
            .as_u64()
            .ok_or_else(|| StreamingError::DeserializationError("missing row index".to_string()))?
            as usize;
        let col = meta["col"]
            .as_u64()
            .ok_or_else(|| StreamingError::DeserializationError("missing col index".to_string()))?
            as usize;

        let gt = GeoTransform {
            origin_x: meta["gt"]["origin_x"].as_f64().unwrap_or(0.0),
            origin_y: meta["gt"]["origin_y"].as_f64().unwrap_or(0.0),
            pixel_width: meta["gt"]["pixel_width"].as_f64().unwrap_or(1.0),
            pixel_height: meta["gt"]["pixel_height"].as_f64().unwrap_or(-1.0),
            row_rotation: meta["gt"]["row_rotation"].as_f64().unwrap_or(0.0),
            col_rotation: meta["gt"]["col_rotation"].as_f64().unwrap_or(0.0),
        };

        let min_x = meta["bbox"]["min_x"].as_f64().unwrap_or(0.0);
        let min_y = meta["bbox"]["min_y"].as_f64().unwrap_or(0.0);
        let max_x = meta["bbox"]["max_x"].as_f64().unwrap_or(0.0);
        let max_y = meta["bbox"]["max_y"].as_f64().unwrap_or(0.0);
        let bbox = BoundingBox::new(min_x, min_y, max_x, max_y).map_err(StreamingError::Core)?;

        let buffer = RasterBuffer::new(
            pixel_bytes,
            width,
            height,
            RasterDataType::UInt8, // fallback; full round-trip requires type registry
            NoDataValue::None,
        )
        .map_err(|e| StreamingError::DeserializationError(e.to_string()))?;

        Ok(Self {
            buffer,
            bbox,
            geotransform: gt,
            indices: (row, col),
            timestamp: element.event_time,
            metadata: ChunkMetadata::default(),
        })
    }
}

/// Metadata for a raster chunk.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChunkMetadata {
    /// Band indices included in this chunk
    pub bands: Vec<usize>,

    /// Compression codec used
    pub compression: Option<String>,

    /// Original size in bytes (before compression)
    pub original_size: Option<usize>,

    /// Compressed size in bytes
    pub compressed_size: Option<usize>,

    /// Checksum of the data
    pub checksum: Option<String>,

    /// Custom attributes
    pub attributes: std::collections::HashMap<String, String>,
}

/// Async trait for raster streaming.
#[async_trait]
pub trait RasterStreaming: Send + Sync {
    /// Get the next chunk from the stream.
    async fn next_chunk(&mut self) -> Result<Option<RasterChunk>>;

    /// Get multiple chunks in parallel.
    async fn next_chunks(&mut self, count: usize) -> Result<Vec<RasterChunk>>;

    /// Skip to a specific chunk by index.
    async fn seek_to_chunk(&mut self, row: usize, col: usize) -> Result<()>;

    /// Get the total number of chunks.
    fn total_chunks(&self) -> (usize, usize);

    /// Get the current position in the stream.
    fn current_position(&self) -> (usize, usize);

    /// Check if the stream has more chunks.
    fn has_more_chunks(&self) -> bool;
}

/// A stream of raster chunks.
pub struct RasterStream {
    /// Stream configuration
    config: RasterStreamConfig,

    /// Raster metadata
    metadata: RasterMetadata,

    /// Total number of chunks (rows, cols)
    total_chunks: (usize, usize),

    /// Current chunk position
    current_position: Arc<RwLock<(usize, usize)>>,

    /// Channel for receiving chunks
    receiver: mpsc::Receiver<RasterChunk>,

    /// Sender for prefetch requests
    prefetch_sender: Option<mpsc::Sender<(usize, usize)>>,

    /// Current memory usage in bytes
    memory_usage: Arc<RwLock<usize>>,
}

impl RasterStream {
    /// Create a new raster stream.
    pub fn new(config: RasterStreamConfig, metadata: RasterMetadata) -> Result<Self> {
        // Calculate total number of chunks
        let total_chunks = Self::calculate_chunks(
            metadata.width as usize,
            metadata.height as usize,
            config.chunk_size.0,
            config.chunk_size.1,
            config.overlap,
        );

        let (_sender, receiver) = mpsc::channel(config.buffer_size);

        info!(
            "Created raster stream with {} x {} chunks",
            total_chunks.0, total_chunks.1
        );

        Ok(Self {
            config,
            metadata,
            total_chunks,
            current_position: Arc::new(RwLock::new((0, 0))),
            receiver,
            prefetch_sender: None,
            memory_usage: Arc::new(RwLock::new(0)),
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

    /// Get the bounding box for a specific chunk.
    pub fn chunk_bbox(&self, row: usize, col: usize) -> Result<BoundingBox> {
        if row >= self.total_chunks.0 || col >= self.total_chunks.1 {
            return Err(StreamingError::InvalidOperation(format!(
                "Chunk ({}, {}) out of bounds",
                row, col
            )));
        }

        let chunk_width = self.config.chunk_size.0;
        let chunk_height = self.config.chunk_size.1;
        let overlap = self.config.overlap;

        let effective_width = chunk_width - overlap;
        let effective_height = chunk_height - overlap;

        let x_start = col * effective_width;
        let y_start = row * effective_height;
        let x_end = (x_start + chunk_width).min(self.metadata.width as usize);
        let y_end = (y_start + chunk_height).min(self.metadata.height as usize);

        // Convert pixel coordinates to geographic coordinates
        let gt =
            self.metadata.geo_transform.as_ref().ok_or_else(|| {
                StreamingError::InvalidState("No geotransform available".to_string())
            })?;

        let min_x = gt.origin_x + (x_start as f64) * gt.pixel_width;
        let max_y = gt.origin_y + (y_start as f64) * gt.pixel_height;
        let max_x = gt.origin_x + (x_end as f64) * gt.pixel_width;
        let min_y = gt.origin_y + (y_end as f64) * gt.pixel_height;

        BoundingBox::new(min_x, min_y, max_x, max_y).map_err(StreamingError::Core)
    }

    /// Get the geotransform for a specific chunk.
    pub fn chunk_geotransform(&self, row: usize, col: usize) -> Result<GeoTransform> {
        let gt =
            self.metadata.geo_transform.as_ref().ok_or_else(|| {
                StreamingError::InvalidState("No geotransform available".to_string())
            })?;

        let chunk_width = self.config.chunk_size.0;
        let chunk_height = self.config.chunk_size.1;
        let overlap = self.config.overlap;

        let effective_width = chunk_width - overlap;
        let effective_height = chunk_height - overlap;

        let x_start = col * effective_width;
        let y_start = row * effective_height;

        let origin_x = gt.origin_x + (x_start as f64) * gt.pixel_width;
        let origin_y = gt.origin_y + (y_start as f64) * gt.pixel_height;

        Ok(GeoTransform {
            origin_x,
            origin_y,
            pixel_width: gt.pixel_width,
            pixel_height: gt.pixel_height,
            row_rotation: gt.row_rotation,
            col_rotation: gt.col_rotation,
        })
    }

    /// Get memory usage statistics.
    pub async fn memory_stats(&self) -> MemoryStats {
        let current = *self.memory_usage.read().await;
        MemoryStats {
            current_bytes: current,
            max_bytes: self.config.max_memory_bytes,
            utilization: (current as f64) / (self.config.max_memory_bytes as f64),
        }
    }

    /// Update memory usage.
    #[allow(dead_code)]
    async fn update_memory(&self, delta: isize) -> Result<()> {
        let mut usage = self.memory_usage.write().await;
        if delta > 0 {
            let new_usage = *usage + delta as usize;
            if new_usage > self.config.max_memory_bytes {
                return Err(StreamingError::Other("Memory limit exceeded".to_string()));
            }
            *usage = new_usage;
        } else {
            *usage = usage.saturating_sub((-delta) as usize);
        }
        Ok(())
    }
}

#[async_trait]
impl RasterStreaming for RasterStream {
    async fn next_chunk(&mut self) -> Result<Option<RasterChunk>> {
        match self.receiver.recv().await {
            Some(chunk) => {
                // Update position
                let mut pos = self.current_position.write().await;
                pos.1 += 1;
                if pos.1 >= self.total_chunks.1 {
                    pos.1 = 0;
                    pos.0 += 1;
                }

                Ok(Some(chunk))
            }
            None => Ok(None),
        }
    }

    async fn next_chunks(&mut self, count: usize) -> Result<Vec<RasterChunk>> {
        let mut chunks = Vec::with_capacity(count);
        for _ in 0..count {
            match self.next_chunk().await? {
                Some(chunk) => chunks.push(chunk),
                None => break,
            }
        }
        Ok(chunks)
    }

    async fn seek_to_chunk(&mut self, row: usize, col: usize) -> Result<()> {
        if row >= self.total_chunks.0 || col >= self.total_chunks.1 {
            return Err(StreamingError::InvalidOperation(format!(
                "Chunk ({}, {}) out of bounds",
                row, col
            )));
        }

        let mut pos = self.current_position.write().await;
        *pos = (row, col);

        // Send prefetch request if enabled
        if let Some(sender) = &self.prefetch_sender
            && sender.try_send((row, col)).is_err()
        {
            warn!("Failed to send prefetch request");
        }

        Ok(())
    }

    fn total_chunks(&self) -> (usize, usize) {
        self.total_chunks
    }

    fn current_position(&self) -> (usize, usize) {
        // Note: This is a blocking call on async lock, but it's okay for a quick read
        // In production, you might want to use a different synchronization primitive
        match self.current_position.try_read() {
            Ok(pos) => *pos,
            Err(_) => (0, 0),
        }
    }

    fn has_more_chunks(&self) -> bool {
        let pos = self.current_position();
        pos.0 < self.total_chunks.0
    }
}

/// Memory usage statistics.
#[derive(Debug, Clone)]
pub struct MemoryStats {
    /// Current memory usage in bytes
    pub current_bytes: usize,

    /// Maximum allowed memory in bytes
    pub max_bytes: usize,

    /// Memory utilization (0.0 to 1.0)
    pub utilization: f64,
}

/// Chunk processing statistics.
#[derive(Debug, Clone, Default)]
pub struct ChunkStats {
    /// Number of chunks processed
    pub chunks_processed: usize,

    /// Number of chunks failed
    pub chunks_failed: usize,

    /// Total bytes processed
    pub bytes_processed: usize,

    /// Total processing time in milliseconds
    pub processing_time_ms: u64,

    /// Average chunk processing time in milliseconds
    pub avg_chunk_time_ms: f64,
}

impl ChunkStats {
    /// Create new statistics.
    pub fn new() -> Self {
        Self::default()
    }

    /// Update statistics with a processed chunk.
    pub fn record_chunk(&mut self, size_bytes: usize, time_ms: u64) {
        self.chunks_processed += 1;
        self.bytes_processed += size_bytes;
        self.processing_time_ms += time_ms;
        self.avg_chunk_time_ms = (self.processing_time_ms as f64) / (self.chunks_processed as f64);
    }

    /// Record a failed chunk.
    pub fn record_failure(&mut self) {
        self.chunks_failed += 1;
    }

    /// Get the throughput in MB/s.
    pub fn throughput_mbps(&self) -> f64 {
        if self.processing_time_ms == 0 {
            return 0.0;
        }
        let mb = (self.bytes_processed as f64) / (1024.0 * 1024.0);
        let seconds = (self.processing_time_ms as f64) / 1000.0;
        mb / seconds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_calculation() {
        let chunks = RasterStream::calculate_chunks(1024, 1024, 256, 256, 0);
        assert_eq!(chunks, (4, 4));

        let chunks = RasterStream::calculate_chunks(1000, 1000, 256, 256, 0);
        assert_eq!(chunks, (4, 4));

        let chunks = RasterStream::calculate_chunks(1024, 1024, 256, 256, 16);
        assert_eq!(chunks, (5, 5));
    }

    #[test]
    fn test_config_builder() {
        let config = RasterStreamConfig::new()
            .with_chunk_size(1024, 1024)
            .with_overlap(32)
            .with_compression(9)
            .with_prefetch(4);

        assert_eq!(config.chunk_size, (1024, 1024));
        assert_eq!(config.overlap, 32);
        assert!(config.compression);
        assert_eq!(config.compression_level, 9);
        assert_eq!(config.prefetch_count, 4);
    }

    #[test]
    fn test_chunk_stats() {
        let mut stats = ChunkStats::new();
        stats.record_chunk(1024 * 1024, 100);
        stats.record_chunk(1024 * 1024, 150);

        assert_eq!(stats.chunks_processed, 2);
        assert_eq!(stats.bytes_processed, 2 * 1024 * 1024);
        assert_eq!(stats.avg_chunk_time_ms, 125.0);
        assert!(stats.throughput_mbps() > 0.0);
    }
}
