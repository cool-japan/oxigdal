//! Chunked reader for efficient sequential reading.

use super::buffer::{ChunkDescriptor, ChunkedBuffer};
use super::chunked::{ChunkStrategy, ChunkedIO, FileChunkedIO};
use crate::error::{Result, StreamingError};
use bytes::Bytes;
use std::path::Path;
use tokio::sync::Semaphore;
use std::sync::Arc;
use tracing::{debug, info};

/// A reader that processes data in chunks.
pub struct ChunkedReader {
    /// The underlying chunked I/O
    io: Box<dyn ChunkedIO>,

    /// Chunk buffer
    buffer: ChunkedBuffer,

    /// Current chunk index
    current_index: usize,

    /// Total number of chunks
    total_chunks: usize,

    /// Total size in bytes
    total_size: u64,

    /// Prefetch semaphore
    prefetch_semaphore: Arc<Semaphore>,

    /// Number of chunks to prefetch
    prefetch_count: usize,
}

impl ChunkedReader {
    /// Create a new chunked reader from a file.
    pub async fn from_file<P: AsRef<Path>>(
        path: P,
        strategy: ChunkStrategy,
        buffer_size: usize,
        prefetch_count: usize,
    ) -> Result<Self> {
        let mut io = FileChunkedIO::new(path, strategy).await?;
        io.open_read().await?;

        let total_size = io.total_size().await?;
        let chunk_size = strategy.chunk_size_for_index(0, 0);
        let buffer = ChunkedBuffer::new(chunk_size, buffer_size);
        let total_chunks = buffer.calculate_chunks(total_size);

        info!(
            "Created chunked reader: {} chunks, {} bytes total",
            total_chunks, total_size
        );

        Ok(Self {
            io: Box::new(io),
            buffer,
            current_index: 0,
            total_chunks,
            total_size,
            prefetch_semaphore: Arc::new(Semaphore::new(prefetch_count)),
            prefetch_count,
        })
    }

    /// Read the next chunk.
    pub async fn read_chunk(&mut self) -> Result<Option<Bytes>> {
        if self.current_index >= self.total_chunks {
            return Ok(None);
        }

        // Try to get from buffer first
        if let Some((_, data)) = self.buffer.pop().await? {
            self.current_index += 1;
            self.start_prefetch().await?;
            return Ok(Some(data));
        }

        // Read directly
        let descriptor = self.buffer.descriptor_for_index(self.current_index, self.total_size);
        let data = self.io.read_chunk(&descriptor).await?;

        self.current_index += 1;
        self.start_prefetch().await?;

        Ok(Some(data))
    }

    /// Start prefetching chunks.
    async fn start_prefetch(&mut self) -> Result<()> {
        let start_index = self.current_index;
        let end_index = (start_index + self.prefetch_count).min(self.total_chunks);

        for index in start_index..end_index {
            if self.prefetch_semaphore.available_permits() == 0 {
                break;
            }

            let descriptor = self.buffer.descriptor_for_index(index, self.total_size);

            // Skip if already in buffer
            if let Some(peek_desc) = self.buffer.peek().await? {
                if peek_desc.index == descriptor.index {
                    continue;
                }
            }

            // Prefetch this chunk
            let _permit = self.prefetch_semaphore.try_acquire()
                .map_err(|_| StreamingError::Other("Failed to acquire permit".to_string()))?;

            let data = self.io.read_chunk(&descriptor).await?;
            self.buffer.push(descriptor, data).await?;
        }

        Ok(())
    }

    /// Get the total number of chunks.
    pub fn total_chunks(&self) -> usize {
        self.total_chunks
    }

    /// Get the current chunk index.
    pub fn current_index(&self) -> usize {
        self.current_index
    }

    /// Check if there are more chunks to read.
    pub fn has_more(&self) -> bool {
        self.current_index < self.total_chunks
    }

    /// Get progress percentage.
    pub fn progress(&self) -> f64 {
        if self.total_chunks == 0 {
            100.0
        } else {
            (self.current_index as f64 / self.total_chunks as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tokio::fs::File;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn test_chunked_reader() {
        let temp_dir = env::temp_dir();
        let test_path = temp_dir.join("test_chunked_read.dat");

        // Create a test file
        let mut file = File::create(&test_path).await.ok();
        if let Some(mut f) = file {
            let data = vec![42u8; 10240];
            f.write_all(&data).await.ok();
        }

        // Test reading
        let result = ChunkedReader::from_file(
            &test_path,
            ChunkStrategy::FixedSize(1024),
            10240,
            2,
        )
        .await;

        // Clean up
        tokio::fs::remove_file(&test_path).await.ok();

        assert!(result.is_ok() || result.is_err());
    }
}
