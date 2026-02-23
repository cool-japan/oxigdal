//! I/O traits for data sources
//!
//! This module provides abstract traits for reading and writing geospatial data
//! from various sources (local files, HTTP, cloud storage, etc.).

use crate::error::Result;

/// Byte range for partial reads
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    /// Start offset (inclusive)
    pub start: u64,
    /// End offset (exclusive)
    pub end: u64,
}

impl ByteRange {
    /// Creates a new byte range
    #[must_use]
    pub const fn new(start: u64, end: u64) -> Self {
        Self { start, end }
    }

    /// Creates a byte range from an offset and length
    #[must_use]
    pub const fn from_offset_length(offset: u64, length: u64) -> Self {
        Self {
            start: offset,
            end: offset + length,
        }
    }

    /// Returns the length of this range
    #[must_use]
    pub const fn len(&self) -> u64 {
        self.end - self.start
    }

    /// Returns true if the range is empty
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// Returns true if this range overlaps with another
    #[must_use]
    pub const fn overlaps(&self, other: &Self) -> bool {
        self.start < other.end && self.end > other.start
    }

    /// Returns true if this range is adjacent to another
    #[must_use]
    pub const fn is_adjacent(&self, other: &Self) -> bool {
        self.end == other.start || self.start == other.end
    }

    /// Merges two overlapping or adjacent ranges
    #[must_use]
    pub fn merge(&self, other: &Self) -> Option<Self> {
        if self.overlaps(other) || self.is_adjacent(other) {
            Some(Self {
                start: self.start.min(other.start),
                end: self.end.max(other.end),
            })
        } else {
            None
        }
    }
}

/// Trait for synchronous data sources
pub trait DataSource: Send + Sync {
    /// Returns the total size of the data source in bytes
    fn size(&self) -> Result<u64>;

    /// Reads bytes from the specified range
    fn read_range(&self, range: ByteRange) -> Result<Vec<u8>>;

    /// Reads bytes from multiple ranges (for optimization)
    fn read_ranges(&self, ranges: &[ByteRange]) -> Result<Vec<Vec<u8>>> {
        ranges.iter().map(|r| self.read_range(*r)).collect()
    }

    /// Returns true if this data source supports range requests
    fn supports_range_requests(&self) -> bool {
        true
    }
}

/// Trait for async data sources
#[cfg(feature = "async")]
#[async_trait::async_trait]
pub trait AsyncDataSource: Send + Sync {
    /// Returns the total size of the data source in bytes
    async fn size(&self) -> Result<u64>;

    /// Reads bytes from the specified range
    async fn read_range(&self, range: ByteRange) -> Result<Vec<u8>>;

    /// Reads bytes from multiple ranges concurrently
    async fn read_ranges(&self, ranges: &[ByteRange]) -> Result<Vec<Vec<u8>>> {
        let mut results = Vec::with_capacity(ranges.len());
        for range in ranges {
            results.push(self.read_range(*range).await?);
        }
        Ok(results)
    }

    /// Returns true if this data source supports range requests
    fn supports_range_requests(&self) -> bool {
        true
    }
}

/// Trait for seekable byte-level writes
pub trait DataSink: Send + Sync {
    /// Writes bytes at the specified offset
    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()>;

    /// Appends bytes to the end
    fn append(&mut self, data: &[u8]) -> Result<u64>;

    /// Flushes any buffered data
    fn flush(&mut self) -> Result<()>;

    /// Truncates the data to the specified size
    fn truncate(&mut self, size: u64) -> Result<()>;

    /// Returns the current size
    fn size(&self) -> Result<u64>;
}

/// Read capability for raster datasets
pub trait RasterRead {
    /// The buffer type returned by read operations
    type Buffer;

    /// Reads a region of the raster
    fn read_region(
        &self,
        band: u32,
        x_offset: u64,
        y_offset: u64,
        width: u64,
        height: u64,
    ) -> Result<Self::Buffer>;

    /// Reads a single tile (for tiled datasets)
    fn read_tile(&self, band: u32, tile_col: u32, tile_row: u32) -> Result<Self::Buffer>;
}

/// Write capability for raster datasets
pub trait RasterWrite {
    /// The buffer type for write operations
    type Buffer;

    /// Writes a region to the raster
    fn write_region(
        &mut self,
        band: u32,
        x_offset: u64,
        y_offset: u64,
        data: &Self::Buffer,
    ) -> Result<()>;

    /// Writes a single tile
    fn write_tile(
        &mut self,
        band: u32,
        tile_col: u32,
        tile_row: u32,
        data: &Self::Buffer,
    ) -> Result<()>;
}

/// Async read capability for raster datasets
#[cfg(feature = "async")]
#[async_trait::async_trait]
pub trait AsyncRasterRead: Send + Sync {
    /// The buffer type returned by read operations
    type Buffer: Send;

    /// Reads a region of the raster asynchronously
    async fn read_region(
        &self,
        band: u32,
        x_offset: u64,
        y_offset: u64,
        width: u64,
        height: u64,
    ) -> Result<Self::Buffer>;

    /// Reads a single tile asynchronously
    async fn read_tile(&self, band: u32, tile_col: u32, tile_row: u32) -> Result<Self::Buffer>;
}

/// Overview (pyramid) level support
pub trait OverviewSupport {
    /// Returns the number of overview levels
    fn overview_count(&self) -> u32;

    /// Returns the dimensions of an overview level
    fn overview_dimensions(&self, level: u32) -> Option<(u64, u64)>;
}

/// COG-specific operations
pub trait CogSupport: OverviewSupport {
    /// Returns the tile size
    fn tile_size(&self) -> (u32, u32);

    /// Returns the number of tiles in X and Y
    fn tile_count(&self) -> (u32, u32);

    /// Returns the byte range for a specific tile
    fn tile_byte_range(&self, level: u32, tile_col: u32, tile_row: u32) -> Option<ByteRange>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byte_range() {
        let range = ByteRange::new(100, 200);
        assert_eq!(range.len(), 100);
        assert!(!range.is_empty());

        let empty = ByteRange::new(100, 100);
        assert!(empty.is_empty());
    }

    #[test]
    fn test_byte_range_overlap() {
        let a = ByteRange::new(0, 100);
        let b = ByteRange::new(50, 150);
        let c = ByteRange::new(200, 300);

        assert!(a.overlaps(&b));
        assert!(b.overlaps(&a));
        assert!(!a.overlaps(&c));
    }

    #[test]
    fn test_byte_range_merge() {
        let a = ByteRange::new(0, 100);
        let b = ByteRange::new(100, 200);
        let c = ByteRange::new(50, 150);

        // Adjacent merge
        let merged_adj = a.merge(&b);
        assert!(merged_adj.is_some());
        let merged = merged_adj.expect("merge should work");
        assert_eq!(merged.start, 0);
        assert_eq!(merged.end, 200);

        // Overlapping merge
        let merged_overlap = a.merge(&c);
        assert!(merged_overlap.is_some());
        let merged2 = merged_overlap.expect("merge should work");
        assert_eq!(merged2.start, 0);
        assert_eq!(merged2.end, 150);

        // Non-overlapping - no merge
        let d = ByteRange::new(300, 400);
        assert!(a.merge(&d).is_none());
    }

    #[test]
    fn test_from_offset_length() {
        let range = ByteRange::from_offset_length(100, 50);
        assert_eq!(range.start, 100);
        assert_eq!(range.end, 150);
        assert_eq!(range.len(), 50);
    }
}
