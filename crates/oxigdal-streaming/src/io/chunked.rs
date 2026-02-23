//! Chunked I/O implementation for efficient data access.

use super::buffer::{ChunkDescriptor, ChunkedBuffer};
use crate::error::{Result, StreamingError};
use async_trait::async_trait;
use bytes::Bytes;
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};
use tracing::{debug, info};

/// Strategy for chunking data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkStrategy {
    /// Fixed-size chunks
    FixedSize(usize),

    /// Adaptive chunk size based on available memory
    Adaptive {
        min_size: usize,
        max_size: usize,
        target_memory: usize,
    },

    /// Line-based chunking (for text data)
    LineBased {
        max_lines: usize,
        max_bytes: usize,
    },
}

impl ChunkStrategy {
    /// Get the chunk size for a given index and available memory.
    pub fn chunk_size_for_index(&self, index: usize, available_memory: usize) -> usize {
        match self {
            ChunkStrategy::FixedSize(size) => *size,
            ChunkStrategy::Adaptive {
                min_size,
                max_size,
                target_memory,
            } => {
                // Calculate adaptive chunk size based on available memory
                let target_size = available_memory.min(*target_memory);
                target_size.max(*min_size).min(*max_size)
            }
            ChunkStrategy::LineBased { max_bytes, .. } => *max_bytes,
        }
    }
}

impl Default for ChunkStrategy {
    fn default() -> Self {
        ChunkStrategy::FixedSize(1024 * 1024) // 1MB default
    }
}

/// Trait for chunked I/O operations.
#[async_trait]
pub trait ChunkedIO: Send + Sync {
    /// Read a chunk at the specified offset.
    async fn read_chunk(&mut self, descriptor: &ChunkDescriptor) -> Result<Bytes>;

    /// Write a chunk at the specified offset.
    async fn write_chunk(&mut self, descriptor: &ChunkDescriptor, data: Bytes) -> Result<()>;

    /// Get the total size of the data.
    async fn total_size(&self) -> Result<u64>;

    /// Flush any pending writes.
    async fn flush(&mut self) -> Result<()>;
}

/// File-based chunked I/O implementation.
pub struct FileChunkedIO {
    /// Path to the file
    path: PathBuf,

    /// File handle for reading
    read_file: Option<File>,

    /// File handle for writing
    write_file: Option<File>,

    /// Chunk strategy
    strategy: ChunkStrategy,

    /// Whether to use direct I/O (if supported)
    direct_io: bool,
}

impl FileChunkedIO {
    /// Create a new file-based chunked I/O.
    pub async fn new<P: AsRef<Path>>(path: P, strategy: ChunkStrategy) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        Ok(Self {
            path,
            read_file: None,
            write_file: None,
            strategy,
            direct_io: false,
        })
    }

    /// Open the file for reading.
    pub async fn open_read(&mut self) -> Result<()> {
        if self.read_file.is_some() {
            return Ok(());
        }

        let file = File::open(&self.path)
            .await
            .map_err(|e| StreamingError::Io(e))?;

        info!("Opened file for reading: {:?}", self.path);
        self.read_file = Some(file);
        Ok(())
    }

    /// Open the file for writing.
    pub async fn open_write(&mut self) -> Result<()> {
        if self.write_file.is_some() {
            return Ok(());
        }

        let file = File::create(&self.path)
            .await
            .map_err(|e| StreamingError::Io(e))?;

        info!("Opened file for writing: {:?}", self.path);
        self.write_file = Some(file);
        Ok(())
    }

    /// Enable direct I/O (platform-dependent).
    pub fn with_direct_io(mut self, enable: bool) -> Self {
        self.direct_io = enable;
        self
    }
}

#[async_trait]
impl ChunkedIO for FileChunkedIO {
    async fn read_chunk(&mut self, descriptor: &ChunkDescriptor) -> Result<Bytes> {
        if self.read_file.is_none() {
            self.open_read().await?;
        }

        let file = self.read_file.as_mut()
            .ok_or_else(|| StreamingError::InvalidState("File not open".to_string()))?;

        // Seek to the chunk offset
        file.seek(SeekFrom::Start(descriptor.offset))
            .await
            .map_err(|e| StreamingError::Io(e))?;

        // Read the chunk data
        let mut buffer = vec![0u8; descriptor.length];
        let bytes_read = file.read_exact(&mut buffer)
            .await
            .map_err(|e| StreamingError::Io(e))?;

        debug!(
            "Read chunk {} at offset {} ({} bytes)",
            descriptor.index, descriptor.offset, bytes_read
        );

        Ok(Bytes::from(buffer))
    }

    async fn write_chunk(&mut self, descriptor: &ChunkDescriptor, data: Bytes) -> Result<()> {
        if self.write_file.is_none() {
            self.open_write().await?;
        }

        let file = self.write_file.as_mut()
            .ok_or_else(|| StreamingError::InvalidState("File not open".to_string()))?;

        // Seek to the chunk offset
        file.seek(SeekFrom::Start(descriptor.offset))
            .await
            .map_err(|e| StreamingError::Io(e))?;

        // Write the chunk data
        file.write_all(&data)
            .await
            .map_err(|e| StreamingError::Io(e))?;

        debug!(
            "Wrote chunk {} at offset {} ({} bytes)",
            descriptor.index, descriptor.offset, data.len()
        );

        Ok(())
    }

    async fn total_size(&self) -> Result<u64> {
        let metadata = tokio::fs::metadata(&self.path)
            .await
            .map_err(|e| StreamingError::Io(e))?;

        Ok(metadata.len())
    }

    async fn flush(&mut self) -> Result<()> {
        if let Some(file) = &mut self.write_file {
            file.flush()
                .await
                .map_err(|e| StreamingError::Io(e))?;
            file.sync_all()
                .await
                .map_err(|e| StreamingError::Io(e))?;
        }
        Ok(())
    }
}

/// Memory-based chunked I/O implementation for testing.
pub struct MemoryChunkedIO {
    /// The in-memory buffer
    buffer: Vec<u8>,

    /// Chunk strategy
    strategy: ChunkStrategy,
}

impl MemoryChunkedIO {
    /// Create a new memory-based chunked I/O.
    pub fn new(size: usize, strategy: ChunkStrategy) -> Self {
        Self {
            buffer: vec![0u8; size],
            strategy,
        }
    }

    /// Get a reference to the buffer.
    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }
}

#[async_trait]
impl ChunkedIO for MemoryChunkedIO {
    async fn read_chunk(&mut self, descriptor: &ChunkDescriptor) -> Result<Bytes> {
        let start = descriptor.offset as usize;
        let end = start + descriptor.length;

        if end > self.buffer.len() {
            return Err(StreamingError::InvalidOperation(
                "Chunk exceeds buffer size".to_string()
            ));
        }

        let data = self.buffer[start..end].to_vec();
        Ok(Bytes::from(data))
    }

    async fn write_chunk(&mut self, descriptor: &ChunkDescriptor, data: Bytes) -> Result<()> {
        let start = descriptor.offset as usize;
        let end = start + descriptor.length;

        if end > self.buffer.len() {
            return Err(StreamingError::InvalidOperation(
                "Chunk exceeds buffer size".to_string()
            ));
        }

        self.buffer[start..end].copy_from_slice(&data);
        Ok(())
    }

    async fn total_size(&self) -> Result<u64> {
        Ok(self.buffer.len() as u64)
    }

    async fn flush(&mut self) -> Result<()> {
        // No-op for memory-based I/O
        Ok(())
    }
}

/// Chunked I/O with prefetching and caching.
pub struct CachedChunkedIO<T: ChunkedIO> {
    /// The underlying chunked I/O
    inner: T,

    /// Chunk buffer for caching
    cache: ChunkedBuffer,

    /// Number of chunks to prefetch
    prefetch_count: usize,
}

impl<T: ChunkedIO> CachedChunkedIO<T> {
    /// Create a new cached chunked I/O.
    pub fn new(inner: T, cache_size: usize, prefetch_count: usize) -> Self {
        Self {
            inner,
            cache: ChunkedBuffer::new(1024 * 1024, cache_size),
            prefetch_count,
        }
    }

    /// Prefetch chunks starting from the given index.
    pub async fn prefetch(&mut self, start_index: usize, total_size: u64) -> Result<()> {
        let total_chunks = self.cache.calculate_chunks(total_size);

        for i in 0..self.prefetch_count {
            let index = start_index + i;
            if index >= total_chunks {
                break;
            }

            let descriptor = self.cache.descriptor_for_index(index, total_size);
            let data = self.inner.read_chunk(&descriptor).await?;
            self.cache.push(descriptor, data).await?;
        }

        Ok(())
    }
}

#[async_trait]
impl<T: ChunkedIO> ChunkedIO for CachedChunkedIO<T> {
    async fn read_chunk(&mut self, descriptor: &ChunkDescriptor) -> Result<Bytes> {
        // Check if chunk is in cache
        if let Some((_, data)) = self.cache.pop().await? {
            return Ok(data);
        }

        // Not in cache, read from underlying I/O
        self.inner.read_chunk(descriptor).await
    }

    async fn write_chunk(&mut self, descriptor: &ChunkDescriptor, data: Bytes) -> Result<()> {
        self.inner.write_chunk(descriptor, data).await
    }

    async fn total_size(&self) -> Result<u64> {
        self.inner.total_size().await
    }

    async fn flush(&mut self) -> Result<()> {
        self.inner.flush().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[tokio::test]
    async fn test_memory_chunked_io() {
        let mut io = MemoryChunkedIO::new(10240, ChunkStrategy::FixedSize(1024));

        let descriptor = ChunkDescriptor::new(0, 1024, 0, 10);
        let data = Bytes::from(vec![42u8; 1024]);

        io.write_chunk(&descriptor, data.clone()).await.ok();

        let read_data = io.read_chunk(&descriptor).await.ok();
        assert!(read_data.is_some());
        assert_eq!(read_data.expect("chunk read should succeed").len(), 1024);
    }

    #[test]
    fn test_chunk_strategy() {
        let strategy = ChunkStrategy::FixedSize(1024);
        assert_eq!(strategy.chunk_size_for_index(0, 2048), 1024);

        let adaptive = ChunkStrategy::Adaptive {
            min_size: 512,
            max_size: 2048,
            target_memory: 1024,
        };
        assert_eq!(adaptive.chunk_size_for_index(0, 1500), 1024);
        assert_eq!(adaptive.chunk_size_for_index(0, 500), 512);
        assert_eq!(adaptive.chunk_size_for_index(0, 3000), 2048);
    }
}
