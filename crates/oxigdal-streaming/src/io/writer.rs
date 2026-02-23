//! Chunked writer for efficient sequential writing.

use super::buffer::{ChunkDescriptor, ChunkedBuffer};
use super::chunked::{ChunkStrategy, ChunkedIO, FileChunkedIO};
use crate::error::{Result, StreamingError};
use bytes::Bytes;
use std::path::Path;
use tokio::sync::Semaphore;
use std::sync::Arc;
use tracing::{debug, info};

/// A writer that processes data in chunks.
pub struct ChunkedWriter {
    /// The underlying chunked I/O
    io: Box<dyn ChunkedIO>,

    /// Chunk buffer
    buffer: ChunkedBuffer,

    /// Current chunk index
    current_index: usize,

    /// Total size written
    bytes_written: u64,

    /// Write semaphore
    write_semaphore: Arc<Semaphore>,

    /// Chunk strategy
    strategy: ChunkStrategy,
}

impl ChunkedWriter {
    /// Create a new chunked writer to a file.
    pub async fn from_file<P: AsRef<Path>>(
        path: P,
        strategy: ChunkStrategy,
        buffer_size: usize,
        max_concurrent_writes: usize,
    ) -> Result<Self> {
        let mut io = FileChunkedIO::new(path, strategy).await?;
        io.open_write().await?;

        let chunk_size = strategy.chunk_size_for_index(0, 0);
        let buffer = ChunkedBuffer::new(chunk_size, buffer_size);

        info!("Created chunked writer with chunk size {}", chunk_size);

        Ok(Self {
            io: Box::new(io),
            buffer,
            current_index: 0,
            bytes_written: 0,
            write_semaphore: Arc::new(Semaphore::new(max_concurrent_writes)),
            strategy,
        })
    }

    /// Write a chunk of data.
    pub async fn write_chunk(&mut self, data: Bytes) -> Result<()> {
        let _permit = self.write_semaphore.acquire().await
            .map_err(|e| StreamingError::Other(e.to_string()))?;

        let offset = self.bytes_written;
        let length = data.len();

        // For now, we don't know the total size, so we use 0 for total_chunks
        let descriptor = ChunkDescriptor::new(
            offset,
            length,
            self.current_index,
            0, // Will be updated when finalized
        );

        self.io.write_chunk(&descriptor, data).await?;

        self.current_index += 1;
        self.bytes_written += length as u64;

        debug!(
            "Wrote chunk {} ({} bytes), total: {} bytes",
            descriptor.index, length, self.bytes_written
        );

        Ok(())
    }

    /// Write multiple chunks in parallel.
    pub async fn write_chunks(&mut self, chunks: Vec<Bytes>) -> Result<()> {
        for chunk in chunks {
            self.write_chunk(chunk).await?;
        }
        Ok(())
    }

    /// Flush all pending writes.
    pub async fn flush(&mut self) -> Result<()> {
        self.io.flush().await?;
        info!("Flushed {} bytes in {} chunks", self.bytes_written, self.current_index);
        Ok(())
    }

    /// Finalize the writer and close the file.
    pub async fn finalize(mut self) -> Result<()> {
        self.flush().await?;
        info!("Finalized chunked writer");
        Ok(())
    }

    /// Get the number of chunks written.
    pub fn chunks_written(&self) -> usize {
        self.current_index
    }

    /// Get the total bytes written.
    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[tokio::test]
    async fn test_chunked_writer() {
        let temp_dir = env::temp_dir();
        let test_path = temp_dir.join("test_chunked_write.dat");

        let result = ChunkedWriter::from_file(
            &test_path,
            ChunkStrategy::FixedSize(1024),
            10240,
            4,
        )
        .await;

        if let Ok(mut writer) = result {
            let data = Bytes::from(vec![42u8; 1024]);
            writer.write_chunk(data).await.ok();
            writer.finalize().await.ok();
        }

        // Clean up
        tokio::fs::remove_file(&test_path).await.ok();
    }
}
