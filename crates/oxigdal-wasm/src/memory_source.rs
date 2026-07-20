//! In-memory [`DataSource`] for locally-provided byte buffers.
//!
//! This backs the `openBytes` drag-and-drop path where a whole GeoTIFF is
//! handed to WASM by the browser rather than fetched over HTTP. Because the
//! entire file is resident in memory, the synchronous `oxigdal_geotiff::CogReader`
//! can be used directly, unlocking the full codec set (None/Deflate/LZW/Zstd/
//! PackBits/JPEG/WebP + predictor) instead of the DEFLATE-only URL fast path.
//!
//! The pattern mirrors [`crate::fetch::PrefetchedFetchBackend`], but keeps the
//! buffer behind a shared, cheaply-cloneable handle.

use std::sync::Arc;

use oxigdal_core::error::{IoError, OxiGdalError, Result};
use oxigdal_core::io::{ByteRange, DataSource};

/// A [`DataSource`] backed by an in-memory byte buffer.
///
/// The buffer is shared via [`Arc`] so cloning is O(1). `Arc` (rather than
/// `Rc`) is required because [`DataSource`] carries a `Send + Sync` bound.
#[derive(Debug, Clone)]
pub struct MemorySource {
    /// Shared, immutable file contents.
    data: Arc<Vec<u8>>,
}

impl MemorySource {
    /// Creates a new in-memory data source from an owned byte buffer.
    #[must_use]
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            data: Arc::new(data),
        }
    }

    /// Returns the number of bytes held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns `true` if the buffer is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl DataSource for MemorySource {
    fn size(&self) -> Result<u64> {
        Ok(self.data.len() as u64)
    }

    fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
        if range.end as usize > self.data.len() || range.start > range.end {
            return Err(OxiGdalError::Io(IoError::UnexpectedEof {
                offset: range.start,
            }));
        }
        Ok(self.data[range.start as usize..range.end as usize].to_vec())
    }

    fn supports_range_requests(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_reports_buffer_length() {
        let src = MemorySource::new(vec![0u8; 42]);
        assert_eq!(src.size().expect("size"), 42);
        assert_eq!(src.len(), 42);
        assert!(!src.is_empty());
    }

    #[test]
    fn read_range_returns_slice() {
        let src = MemorySource::new(vec![10, 20, 30, 40, 50]);
        let out = src
            .read_range(ByteRange::from_offset_length(1, 3))
            .expect("read");
        assert_eq!(out, vec![20, 30, 40]);
    }

    #[test]
    fn read_past_eof_errors() {
        let src = MemorySource::new(vec![1, 2, 3]);
        assert!(
            src.read_range(ByteRange::from_offset_length(2, 10))
                .is_err()
        );
    }

    #[test]
    fn clone_shares_buffer() {
        let src = MemorySource::new(vec![7u8; 8]);
        let clone = src.clone();
        assert_eq!(clone.size().expect("size"), 8);
        assert!(src.supports_range_requests());
    }
}
