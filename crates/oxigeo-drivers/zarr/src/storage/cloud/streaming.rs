//! Streaming chunk reader for memory-efficient processing

use crate::error::{Result, StorageError, ZarrError};

// ============================================================================
// Streaming Reader
// ============================================================================

/// Streaming chunk reader for memory-efficient processing
pub struct StreamingChunkReader {
    /// Current buffer
    buffer: Vec<u8>,
    /// Buffer capacity
    capacity: usize,
    /// Current read position
    position: usize,
    /// Total bytes read
    total_read: u64,
    /// Whether the stream is complete
    complete: bool,
}

impl StreamingChunkReader {
    /// Creates a new streaming chunk reader
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
            capacity,
            position: 0,
            total_read: 0,
            complete: false,
        }
    }

    /// Writes data to the buffer
    ///
    /// # Errors
    /// Returns error if buffer is full and data exceeds capacity
    pub fn write(&mut self, data: &[u8]) -> Result<usize> {
        let available = self.capacity.saturating_sub(self.buffer.len());
        let to_write = data.len().min(available);

        if to_write == 0 && !data.is_empty() {
            return Err(ZarrError::Storage(StorageError::Cache {
                message: "Streaming buffer full".to_string(),
            }));
        }

        self.buffer.extend_from_slice(&data[..to_write]);
        self.total_read += to_write as u64;

        Ok(to_write)
    }

    /// Reads data from the buffer
    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        let available = self.buffer.len() - self.position;
        let to_read = buf.len().min(available);

        if to_read > 0 {
            buf[..to_read].copy_from_slice(&self.buffer[self.position..self.position + to_read]);
            self.position += to_read;
        }

        to_read
    }

    /// Returns the number of bytes available to read
    #[must_use]
    pub fn available(&self) -> usize {
        self.buffer.len() - self.position
    }

    /// Returns true if all data has been read
    #[must_use]
    pub fn is_exhausted(&self) -> bool {
        self.complete && self.position >= self.buffer.len()
    }

    /// Marks the stream as complete (no more data will be written)
    pub fn mark_complete(&mut self) {
        self.complete = true;
    }

    /// Resets the reader position to the beginning
    pub fn rewind(&mut self) {
        self.position = 0;
    }

    /// Clears the buffer and resets state
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.position = 0;
        self.complete = false;
    }

    /// Compacts the buffer by removing already-read data
    pub fn compact(&mut self) {
        if self.position > 0 {
            self.buffer.drain(..self.position);
            self.position = 0;
        }
    }

    /// Returns the total bytes read
    #[must_use]
    pub fn total_read(&self) -> u64 {
        self.total_read
    }

    /// Takes all buffered data
    pub fn take_all(&mut self) -> Vec<u8> {
        self.position = 0;
        self.complete = false;
        std::mem::take(&mut self.buffer)
    }
}
