//! I/O traits for data sources
//!
//! This module provides abstract traits for reading and writing geospatial data
//! from various sources (local files, HTTP, cloud storage, etc.).

#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use crate::compat::*;
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

/// Converts `range.len()` to a `usize`, the length of the buffer a read of that
/// range needs.
///
/// Shared by every in-tree [`DataSource`] so they agree on what an
/// unrepresentable length means: on a 64-bit target the conversion is infallible,
/// on a 32-bit one (`wasm32`, `thumbv7em`, …) a range wider than `usize::MAX`
/// is reported rather than silently truncated to a shorter read.
///
/// # Errors
/// Returns [`OxiGeoError::OutOfBounds`] if the range is wider than `usize::MAX`.
pub(crate) fn range_len_usize(range: ByteRange) -> Result<usize> {
    usize::try_from(range.len()).map_err(|_| crate::error::OxiGeoError::OutOfBounds {
        message: format!(
            "byte range {}..{} is {} bytes long, which exceeds usize::MAX ({})",
            range.start,
            range.end,
            range.len(),
            usize::MAX
        ),
    })
}

/// Converts a range to the `(offset, length)` pair an in-memory source indexes
/// with, rejecting values that do not fit a `usize`.
///
/// # Errors
/// Returns [`OxiGeoError::OutOfBounds`] if either value exceeds `usize::MAX`.
// Only the memory-mapped sources index by `(offset, len)`, and those are std-only.
#[cfg(feature = "std")]
pub(crate) fn range_bounds_usize(range: ByteRange) -> Result<(usize, usize)> {
    let start =
        usize::try_from(range.start).map_err(|_| crate::error::OxiGeoError::OutOfBounds {
            message: format!(
                "byte range start {} exceeds usize::MAX ({})",
                range.start,
                usize::MAX
            ),
        })?;
    Ok((start, range_len_usize(range)?))
}

/// Builds the error every [`DataSource::read_range_into`] implementation returns
/// when the caller's destination buffer cannot hold the whole range.
pub(crate) fn dst_too_small(needed: usize, available: usize) -> crate::error::OxiGeoError {
    crate::error::OxiGeoError::invalid_parameter(
        "dst",
        format!(
            "destination buffer is {available} bytes but the requested range needs {needed}; \
             size it with ByteRange::len()"
        ),
    )
}

/// Trait for synchronous data sources
pub trait DataSource: Send + Sync {
    /// Returns the total size of the data source in bytes
    fn size(&self) -> Result<u64>;

    /// Reads bytes from the specified range
    fn read_range(&self, range: ByteRange) -> Result<Vec<u8>>;

    /// Reads `range` directly into `dst`, avoiding the intermediate allocation of
    /// [`DataSource::read_range`]. Returns the number of bytes written.
    ///
    /// This is the allocation-free entry point for block-oriented readers: a
    /// caller walking thousands of tiles or strips can size one scratch buffer
    /// up front and reuse it, instead of paying a heap allocation per block
    /// (cool-japan/oxigeo#14).
    ///
    /// # Buffer sizing
    ///
    /// * `dst` **longer** than the range is fine — only `dst[..n]` is written and
    ///   the tail keeps whatever the caller left there.
    /// * `dst` **shorter** than `range.len()` is rejected with
    ///   [`OxiGeoError::InvalidParameter`](crate::error::OxiGeoError::InvalidParameter)
    ///   *before* any I/O happens, leaving `dst` untouched. Truncating silently
    ///   would hand back a partial block that looks like a complete read.
    /// * An **empty** range writes nothing and returns `Ok(0)`; `dst` may itself
    ///   be empty in that case.
    ///
    /// The returned count is what the source actually produced. It equals
    /// `range.len()` for every source that reports a short read as an error
    /// (all of the built-in ones); a source whose `read_range` clamps to its own
    /// end returns the same clamped length here.
    ///
    /// On error the contents of `dst` are unspecified — an implementation is free
    /// to write directly into it and fail part-way through.
    ///
    /// # Errors
    /// Returns exactly what [`DataSource::read_range`] returns for `range`, plus
    /// the `dst`-too-short error described above.
    fn read_range_into(&self, range: ByteRange, dst: &mut [u8]) -> Result<usize> {
        let needed = range_len_usize(range)?;
        if dst.len() < needed {
            return Err(dst_too_small(needed, dst.len()));
        }
        let data = self.read_range(range)?;
        // A source whose `read_range` clamps may return fewer bytes than the
        // range asked for; never more than `dst` can hold, because `needed` is an
        // upper bound on a well-behaved source's output and `dst` holds `needed`.
        let written = data.len().min(dst.len());
        dst[..written].copy_from_slice(&data[..written]);
        Ok(written)
    }

    /// Returns a borrowed view of `range` when this source can serve it without
    /// copying — a memory-mapped file or a fully in-memory buffer, for instance.
    ///
    /// The default implementation returns `None`, meaning "copy me instead", so
    /// callers must always keep a copying fallback ([`DataSource::read_range`] or
    /// [`DataSource::read_range_into`]). An implementation that returns `Some`
    /// must return exactly the bytes [`DataSource::read_range`] would have
    /// returned; any range it cannot serve in full (out of bounds, inverted, or
    /// wider than `usize`) must yield `None` so the fallback reports the error.
    fn range_slice(&self, range: ByteRange) -> Option<&[u8]> {
        let _ = range;
        None
    }

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

    /// Reads `range` directly into `dst`, avoiding the intermediate allocation of
    /// [`AsyncDataSource::read_range`]. Returns the number of bytes written.
    ///
    /// The buffer-sizing and error contract is identical to the synchronous
    /// [`DataSource::read_range_into`]: a longer `dst` is fine, a `dst` shorter
    /// than `range.len()` is rejected before any I/O, and an empty range returns
    /// `Ok(0)`.
    ///
    /// # Errors
    /// Returns exactly what [`AsyncDataSource::read_range`] returns for `range`,
    /// plus the `dst`-too-short error.
    async fn read_range_into(&self, range: ByteRange, dst: &mut [u8]) -> Result<usize> {
        let needed = range_len_usize(range)?;
        if dst.len() < needed {
            return Err(dst_too_small(needed, dst.len()));
        }
        let data = self.read_range(range).await?;
        let written = data.len().min(dst.len());
        dst[..written].copy_from_slice(&data[..written]);
        Ok(written)
    }

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
