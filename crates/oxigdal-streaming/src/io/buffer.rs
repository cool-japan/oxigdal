//! Buffer management for chunked I/O operations.

use crate::error::{Result, StreamingError};
use bytes::{Bytes, BytesMut};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Descriptor for a data chunk.
#[derive(Debug, Clone)]
pub struct ChunkDescriptor {
    /// Offset in bytes from the start of the data
    pub offset: u64,

    /// Length of the chunk in bytes
    pub length: usize,

    /// Chunk index
    pub index: usize,

    /// Total number of chunks
    pub total_chunks: usize,

    /// Whether this is the last chunk
    pub is_last: bool,
}

impl ChunkDescriptor {
    /// Create a new chunk descriptor.
    pub fn new(offset: u64, length: usize, index: usize, total_chunks: usize) -> Self {
        Self {
            offset,
            length,
            index,
            total_chunks,
            is_last: index + 1 == total_chunks,
        }
    }

    /// Get the end offset of this chunk.
    pub fn end_offset(&self) -> u64 {
        self.offset + self.length as u64
    }
}

/// A buffer that manages chunked data.
pub struct ChunkedBuffer {
    /// The underlying buffer
    inner: Arc<RwLock<ChunkedBufferInner>>,

    /// Chunk size in bytes
    chunk_size: usize,

    /// Maximum buffer size in bytes
    max_size: usize,
}

struct ChunkedBufferInner {
    /// Queue of buffered chunks
    chunks: VecDeque<BufferedChunk>,

    /// Current size in bytes
    current_size: usize,

    /// Next chunk index to read
    next_read_index: usize,

    /// Next chunk index to write
    next_write_index: usize,

    /// Total number of chunks
    total_chunks: Option<usize>,

    /// Whether writing is complete
    write_complete: bool,
}

struct BufferedChunk {
    descriptor: ChunkDescriptor,
    data: Bytes,
}

impl ChunkedBuffer {
    /// Create a new chunked buffer.
    pub fn new(chunk_size: usize, max_size: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(ChunkedBufferInner {
                chunks: VecDeque::new(),
                current_size: 0,
                next_read_index: 0,
                next_write_index: 0,
                total_chunks: None,
                write_complete: false,
            })),
            chunk_size,
            max_size,
        }
    }

    /// Create a new chunked buffer with default settings.
    pub fn with_defaults() -> Self {
        Self::new(1024 * 1024, 100 * 1024 * 1024) // 1MB chunks, 100MB max
    }

    /// Calculate the number of chunks needed for a given size.
    pub fn calculate_chunks(&self, total_size: u64) -> usize {
        ((total_size + self.chunk_size as u64 - 1) / self.chunk_size as u64) as usize
    }

    /// Get a chunk descriptor for a given index.
    pub fn descriptor_for_index(&self, index: usize, total_size: u64) -> ChunkDescriptor {
        let total_chunks = self.calculate_chunks(total_size);
        let offset = (index as u64) * (self.chunk_size as u64);
        let remaining = total_size.saturating_sub(offset);
        let length = remaining.min(self.chunk_size as u64) as usize;

        ChunkDescriptor::new(offset, length, index, total_chunks)
    }

    /// Push a chunk into the buffer.
    pub async fn push(&self, descriptor: ChunkDescriptor, data: Bytes) -> Result<()> {
        let mut inner = self.inner.write().await;

        // Check if buffer is full
        if inner.current_size + data.len() > self.max_size {
            return Err(StreamingError::BufferFull);
        }

        // Verify chunk index
        if descriptor.index != inner.next_write_index {
            return Err(StreamingError::InvalidOperation(format!(
                "Expected chunk {}, got {}",
                inner.next_write_index, descriptor.index
            )));
        }

        inner.chunks.push_back(BufferedChunk {
            descriptor: descriptor.clone(),
            data,
        });

        inner.current_size += descriptor.length;
        inner.next_write_index += 1;

        if let Some(total) = inner.total_chunks {
            if descriptor.index + 1 == total {
                inner.write_complete = true;
            }
        } else if descriptor.is_last {
            inner.total_chunks = Some(descriptor.total_chunks);
            inner.write_complete = true;
        }

        debug!(
            "Pushed chunk {} ({} bytes), buffer size: {}",
            descriptor.index, descriptor.length, inner.current_size
        );

        Ok(())
    }

    /// Pop a chunk from the buffer.
    pub async fn pop(&self) -> Result<Option<(ChunkDescriptor, Bytes)>> {
        let mut inner = self.inner.write().await;

        if inner.chunks.is_empty() {
            if inner.write_complete {
                return Ok(None);
            } else {
                return Err(StreamingError::Other("No chunks available".to_string()));
            }
        }

        let chunk = inner.chunks.pop_front()
            .ok_or_else(|| StreamingError::Other("Failed to pop chunk".to_string()))?;

        inner.current_size = inner.current_size.saturating_sub(chunk.descriptor.length);
        inner.next_read_index += 1;

        debug!(
            "Popped chunk {} ({} bytes), buffer size: {}",
            chunk.descriptor.index, chunk.descriptor.length, inner.current_size
        );

        Ok(Some((chunk.descriptor, chunk.data)))
    }

    /// Peek at the next chunk without removing it.
    pub async fn peek(&self) -> Result<Option<ChunkDescriptor>> {
        let inner = self.inner.read().await;
        Ok(inner.chunks.front().map(|c| c.descriptor.clone()))
    }

    /// Get the number of chunks currently in the buffer.
    pub async fn len(&self) -> usize {
        let inner = self.inner.read().await;
        inner.chunks.len()
    }

    /// Check if the buffer is empty.
    pub async fn is_empty(&self) -> bool {
        let inner = self.inner.read().await;
        inner.chunks.is_empty()
    }

    /// Get the current buffer size in bytes.
    pub async fn size_bytes(&self) -> usize {
        let inner = self.inner.read().await;
        inner.current_size
    }

    /// Check if writing is complete.
    pub async fn is_complete(&self) -> bool {
        let inner = self.inner.read().await;
        inner.write_complete
    }

    /// Clear all chunks from the buffer.
    pub async fn clear(&self) {
        let mut inner = self.inner.write().await;
        inner.chunks.clear();
        inner.current_size = 0;
        debug!("Buffer cleared");
    }

    /// Get buffer statistics.
    pub async fn stats(&self) -> BufferStats {
        let inner = self.inner.read().await;
        BufferStats {
            chunks_buffered: inner.chunks.len(),
            bytes_buffered: inner.current_size,
            max_bytes: self.max_size,
            utilization: (inner.current_size as f64) / (self.max_size as f64),
            chunks_read: inner.next_read_index,
            chunks_written: inner.next_write_index,
            total_chunks: inner.total_chunks,
            complete: inner.write_complete,
        }
    }
}

/// Buffer statistics.
#[derive(Debug, Clone)]
pub struct BufferStats {
    /// Number of chunks currently buffered
    pub chunks_buffered: usize,

    /// Number of bytes currently buffered
    pub bytes_buffered: usize,

    /// Maximum buffer size in bytes
    pub max_bytes: usize,

    /// Buffer utilization (0.0 to 1.0)
    pub utilization: f64,

    /// Number of chunks read
    pub chunks_read: usize,

    /// Number of chunks written
    pub chunks_written: usize,

    /// Total number of chunks (if known)
    pub total_chunks: Option<usize>,

    /// Whether writing is complete
    pub complete: bool,
}

impl BufferStats {
    /// Calculate progress percentage.
    pub fn progress(&self) -> Option<f64> {
        self.total_chunks.map(|total| {
            if total == 0 {
                100.0
            } else {
                (self.chunks_read as f64 / total as f64) * 100.0
            }
        })
    }
}

/// A circular buffer for efficient chunk management.
pub struct CircularChunkBuffer {
    /// The underlying buffer
    buffer: Vec<u8>,

    /// Read position
    read_pos: usize,

    /// Write position
    write_pos: usize,

    /// Number of bytes available
    available: usize,

    /// Buffer capacity
    capacity: usize,
}

impl CircularChunkBuffer {
    /// Create a new circular buffer.
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![0; capacity],
            read_pos: 0,
            write_pos: 0,
            available: 0,
            capacity,
        }
    }

    /// Write data to the buffer.
    pub fn write(&mut self, data: &[u8]) -> Result<usize> {
        let space_available = self.capacity - self.available;
        let to_write = data.len().min(space_available);

        if to_write == 0 {
            return Ok(0);
        }

        let end_pos = self.write_pos + to_write;
        if end_pos <= self.capacity {
            // Simple case: write doesn't wrap
            self.buffer[self.write_pos..end_pos].copy_from_slice(&data[..to_write]);
            self.write_pos = end_pos % self.capacity;
        } else {
            // Write wraps around
            let first_part = self.capacity - self.write_pos;
            self.buffer[self.write_pos..].copy_from_slice(&data[..first_part]);
            self.buffer[..to_write - first_part].copy_from_slice(&data[first_part..to_write]);
            self.write_pos = to_write - first_part;
        }

        self.available += to_write;
        Ok(to_write)
    }

    /// Read data from the buffer.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let to_read = buf.len().min(self.available);

        if to_read == 0 {
            return Ok(0);
        }

        let end_pos = self.read_pos + to_read;
        if end_pos <= self.capacity {
            // Simple case: read doesn't wrap
            buf[..to_read].copy_from_slice(&self.buffer[self.read_pos..end_pos]);
            self.read_pos = end_pos % self.capacity;
        } else {
            // Read wraps around
            let first_part = self.capacity - self.read_pos;
            buf[..first_part].copy_from_slice(&self.buffer[self.read_pos..]);
            buf[first_part..to_read].copy_from_slice(&self.buffer[..to_read - first_part]);
            self.read_pos = to_read - first_part;
        }

        self.available -= to_read;
        Ok(to_read)
    }

    /// Get the number of bytes available to read.
    pub fn available(&self) -> usize {
        self.available
    }

    /// Get the amount of free space in the buffer.
    pub fn space_available(&self) -> usize {
        self.capacity - self.available
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.available == 0
    }

    /// Check if the buffer is full.
    pub fn is_full(&self) -> bool {
        self.available == self.capacity
    }

    /// Clear the buffer.
    pub fn clear(&mut self) {
        self.read_pos = 0;
        self.write_pos = 0;
        self.available = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_chunked_buffer() {
        let buffer = ChunkedBuffer::new(1024, 10240);

        let desc = ChunkDescriptor::new(0, 1024, 0, 10);
        let data = Bytes::from(vec![0u8; 1024]);

        buffer.push(desc.clone(), data.clone()).await.ok();

        assert_eq!(buffer.len().await, 1);
        assert_eq!(buffer.size_bytes().await, 1024);

        let popped = buffer.pop().await.ok().flatten();
        assert!(popped.is_some());

        assert_eq!(buffer.len().await, 0);
    }

    #[test]
    fn test_circular_buffer() {
        let mut buffer = CircularChunkBuffer::new(10);

        // Write some data
        let written = buffer.write(&[1, 2, 3, 4, 5]).ok();
        assert_eq!(written, Some(5));
        assert_eq!(buffer.available(), 5);

        // Read some data
        let mut read_buf = [0u8; 3];
        let read = buffer.read(&mut read_buf).ok();
        assert_eq!(read, Some(3));
        assert_eq!(read_buf, [1, 2, 3]);
        assert_eq!(buffer.available(), 2);

        // Write more data (should wrap)
        let written = buffer.write(&[6, 7, 8, 9, 10]).ok();
        assert_eq!(written, Some(5));
        assert_eq!(buffer.available(), 7);
    }

    #[test]
    fn test_chunk_descriptor() {
        let desc = ChunkDescriptor::new(0, 1024, 0, 10);
        assert_eq!(desc.offset, 0);
        assert_eq!(desc.length, 1024);
        assert_eq!(desc.end_offset(), 1024);
        assert_eq!(desc.is_last, false);

        let last_desc = ChunkDescriptor::new(9216, 1024, 9, 10);
        assert_eq!(last_desc.is_last, true);
    }
}
