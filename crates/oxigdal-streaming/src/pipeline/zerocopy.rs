//! Zero-copy buffer management for efficient streaming.

use crate::error::{Result, StreamingError};
use bytes::{Bytes, BytesMut};
use std::sync::Arc;
use tokio::sync::RwLock;

/// A zero-copy buffer that can be shared between pipeline stages.
#[derive(Debug, Clone)]
pub struct ZeroCopyBuffer {
    /// The underlying data
    data: Bytes,

    /// Offset in the original data
    offset: usize,

    /// Length of this view
    length: usize,
}

impl ZeroCopyBuffer {
    /// Create a new zero-copy buffer.
    pub fn new(data: Bytes) -> Self {
        let length = data.len();
        Self {
            data,
            offset: 0,
            length,
        }
    }

    /// Create a view of a subset of the data.
    pub fn slice(&self, start: usize, end: usize) -> Result<Self> {
        if end > self.length {
            return Err(StreamingError::InvalidOperation(
                "Slice end exceeds buffer length".to_string()
            ));
        }

        Ok(Self {
            data: self.data.clone(),
            offset: self.offset + start,
            length: end - start,
        })
    }

    /// Get a reference to the data.
    pub fn as_ref(&self) -> &[u8] {
        &self.data[self.offset..self.offset + self.length]
    }

    /// Get the underlying Bytes.
    pub fn bytes(&self) -> Bytes {
        self.data.slice(self.offset..self.offset + self.length)
    }

    /// Get the length of this buffer.
    pub fn len(&self) -> usize {
        self.length
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// Clone the data into a new owned buffer.
    pub fn to_owned(&self) -> Vec<u8> {
        self.as_ref().to_vec()
    }
}

/// A shared buffer that can be safely accessed from multiple threads.
pub struct SharedBuffer {
    /// The underlying buffer
    inner: Arc<RwLock<BytesMut>>,

    /// Current read position
    read_pos: Arc<RwLock<usize>>,

    /// Current write position
    write_pos: Arc<RwLock<usize>>,
}

impl SharedBuffer {
    /// Create a new shared buffer with the given capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(BytesMut::with_capacity(capacity))),
            read_pos: Arc::new(RwLock::new(0)),
            write_pos: Arc::new(RwLock::new(0)),
        }
    }

    /// Write data to the buffer.
    pub async fn write(&self, data: &[u8]) -> Result<usize> {
        let mut buffer = self.inner.write().await;
        let mut write_pos = self.write_pos.write().await;

        buffer.extend_from_slice(data);
        *write_pos += data.len();

        Ok(data.len())
    }

    /// Read data from the buffer.
    pub async fn read(&self, len: usize) -> Result<ZeroCopyBuffer> {
        let buffer = self.inner.read().await;
        let mut read_pos = self.read_pos.write().await;

        let available = buffer.len() - *read_pos;
        if available < len {
            return Err(StreamingError::Other(
                "Not enough data available".to_string()
            ));
        }

        let data = buffer.slice(*read_pos..*read_pos + len);
        *read_pos += len;

        Ok(ZeroCopyBuffer::new(data.freeze()))
    }

    /// Get the number of bytes available to read.
    pub async fn available(&self) -> usize {
        let buffer = self.inner.read().await;
        let read_pos = self.read_pos.read().await;
        buffer.len() - *read_pos
    }

    /// Clear the buffer and reset positions.
    pub async fn clear(&self) {
        let mut buffer = self.inner.write().await;
        let mut read_pos = self.read_pos.write().await;
        let mut write_pos = self.write_pos.write().await;

        buffer.clear();
        *read_pos = 0;
        *write_pos = 0;
    }
}

impl Clone for SharedBuffer {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            read_pos: Arc::clone(&self.read_pos),
            write_pos: Arc::clone(&self.write_pos),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zerocopy_buffer() {
        let data = Bytes::from(vec![1, 2, 3, 4, 5]);
        let buffer = ZeroCopyBuffer::new(data);

        assert_eq!(buffer.len(), 5);
        assert_eq!(buffer.as_ref(), &[1, 2, 3, 4, 5]);

        let slice = buffer.slice(1, 4).ok();
        assert!(slice.is_some());
        if let Some(slice) = slice {
            assert_eq!(slice.as_ref(), &[2, 3, 4]);
        }
    }

    #[tokio::test]
    async fn test_shared_buffer() {
        let buffer = SharedBuffer::with_capacity(1024);

        let data = vec![1, 2, 3, 4, 5];
        let written = buffer.write(&data).await.ok();
        assert_eq!(written, Some(5));

        let available = buffer.available().await;
        assert_eq!(available, 5);

        let read = buffer.read(3).await.ok();
        assert!(read.is_some());
        if let Some(read) = read {
            assert_eq!(read.as_ref(), &[1, 2, 3]);
        }
    }
}
