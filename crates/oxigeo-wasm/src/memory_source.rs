//! In-memory [`DataSource`] for locally-provided byte buffers.
//!
//! This backs the `openBytes` drag-and-drop path where a whole GeoTIFF is
//! handed to WASM by the browser rather than fetched over HTTP. Because the
//! entire file is resident in memory, the synchronous `oxigeo_geotiff::CogReader`
//! can be used directly, unlocking the full codec set (None/Deflate/LZW/Zstd/
//! PackBits/JPEG/WebP + predictor) instead of the DEFLATE-only URL fast path.
//!
//! The pattern mirrors [`crate::fetch::PrefetchedFetchBackend`], but keeps the
//! buffer behind a shared, cheaply-cloneable handle.

use std::sync::Arc;

use oxigeo_core::error::{IoError, OxiGeoError, Result};
use oxigeo_core::io::{ByteRange, DataSource};

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

    /// Borrows `range` out of the backing buffer, or reports the same
    /// end-of-file error [`DataSource::read_range`] reports for it.
    ///
    /// `usize::try_from` rather than `as usize`: on `wasm32` (a 32-bit target)
    /// a cast would silently truncate an offset past 4 GiB and hand back the
    /// wrong bytes instead of an error.
    fn slice_for(&self, range: ByteRange) -> Result<&[u8]> {
        let eof = || {
            OxiGeoError::Io(IoError::UnexpectedEof {
                offset: range.start,
            })
        };
        let start = usize::try_from(range.start).map_err(|_| eof())?;
        let end = usize::try_from(range.end).map_err(|_| eof())?;
        // `get` rejects both an inverted range (`start > end`) and one that runs
        // past the buffer, exactly like the explicit checks it replaces.
        self.data.get(start..end).ok_or_else(eof)
    }
}

/// Builds the error a `read_range_into` implementation returns when the
/// caller's destination buffer cannot hold the whole range.
///
/// Mirrors the message `oxigeo_core::io`'s built-in sources produce (their
/// helper is crate-private) so the diagnostic is identical whichever source a
/// caller is holding.
pub(crate) fn dst_too_small(needed: usize, available: usize) -> OxiGeoError {
    OxiGeoError::invalid_parameter(
        "dst",
        format!(
            "destination buffer is {available} bytes but the requested range needs {needed}; \
             size it with ByteRange::len()"
        ),
    )
}

/// Computes the destination length `range` requires, or `None` when the range
/// is itself malformed (inverted, or wider than `usize`).
///
/// A `None` result means "let the source's own range check report it", which
/// keeps `read_range_into` erroring exactly like `read_range` instead of
/// underflowing on `ByteRange::len`.
pub(crate) fn needed_len(range: ByteRange) -> Option<usize> {
    usize::try_from(range.end.checked_sub(range.start)?).ok()
}

impl DataSource for MemorySource {
    fn size(&self) -> Result<u64> {
        Ok(self.data.len() as u64)
    }

    fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
        Ok(self.slice_for(range)?.to_vec())
    }

    /// Copies straight out of the shared buffer, skipping the intermediate
    /// `Vec` the trait's default implementation would allocate per block
    /// (cool-japan/oxigeo#14).
    fn read_range_into(&self, range: ByteRange, dst: &mut [u8]) -> Result<usize> {
        if let Some(needed) = needed_len(range)
            && dst.len() < needed
        {
            return Err(dst_too_small(needed, dst.len()));
        }
        let src = self.slice_for(range)?;
        let available = dst.len();
        let out = dst
            .get_mut(..src.len())
            .ok_or_else(|| dst_too_small(src.len(), available))?;
        out.copy_from_slice(src);
        Ok(src.len())
    }

    /// Lends the requested bytes straight out of the resident buffer: a block
    /// read through this costs neither an allocation nor a copy.
    fn range_slice(&self, range: ByteRange) -> Option<&[u8]> {
        self.slice_for(range).ok()
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

    /// cool-japan/oxigeo#14: the zero-copy entry points must agree with
    /// `read_range` byte for byte, and error for error.
    #[test]
    fn test_issue_14_read_range_into_matches_read_range() {
        let src = MemorySource::new((0u8..32).collect());
        let cases = [
            ByteRange::new(0, 32),  // whole buffer
            ByteRange::new(8, 20),  // interior
            ByteRange::new(0, 1),   // leading boundary
            ByteRange::new(31, 32), // trailing boundary
            ByteRange::new(5, 5),   // empty
            ByteRange::new(32, 32), // empty at EOF
        ];
        for range in cases {
            let expected = src.read_range(range).expect("read_range");
            let mut dst = vec![0xAAu8; expected.len()];
            let written = src.read_range_into(range, &mut dst).expect("read_into");
            assert_eq!(written, expected.len(), "count mismatch for {range:?}");
            assert_eq!(dst, expected, "bytes mismatch for {range:?}");
        }

        // Past EOF and inverted ranges must fail the same way in both paths --
        // and `read_range_into` must not panic on the underflowing length.
        for range in [
            ByteRange::new(28, 40),
            ByteRange::new(32, 33),
            ByteRange::new(20, 8),
        ] {
            assert!(src.read_range(range).is_err(), "read_range {range:?}");
            let mut dst = vec![0u8; 64];
            let err = src
                .read_range_into(range, &mut dst)
                .expect_err("read_range_into should reject");
            assert!(
                matches!(err, OxiGeoError::Io(IoError::UnexpectedEof { .. })),
                "expected EOF for {range:?}, got {err}"
            );
        }
    }

    #[test]
    fn test_issue_14_read_range_into_buffer_sizing() {
        let src = MemorySource::new((0u8..16).collect());
        let range = ByteRange::new(4, 12);

        // Too long: only the first 8 bytes are written, the tail is preserved.
        let mut dst = vec![0xEEu8; 12];
        let written = src.read_range_into(range, &mut dst).expect("read_into");
        assert_eq!(written, 8);
        assert_eq!(&dst[..8], &(4u8..12).collect::<Vec<u8>>()[..]);
        assert_eq!(&dst[8..], &[0xEE; 4]);

        // Too short: rejected before anything is written.
        let mut dst = vec![0xEEu8; 7];
        let err = src
            .read_range_into(range, &mut dst)
            .expect_err("short dst must be rejected");
        assert!(
            matches!(err, OxiGeoError::InvalidParameter { parameter, .. } if parameter == "dst"),
            "expected an InvalidParameter(dst) error, got {err}"
        );
        assert_eq!(dst, vec![0xEE; 7], "dst must be untouched");

        // An empty range writes nothing, even into an empty destination.
        assert_eq!(
            src.read_range_into(ByteRange::new(3, 3), &mut [])
                .expect("empty range"),
            0
        );
    }

    #[test]
    fn test_issue_14_range_slice_borrows_backing_buffer() {
        let src = MemorySource::new((0u8..64).collect());
        let borrowed = src.range_slice(ByteRange::new(16, 48)).expect("borrow");
        assert_eq!(borrowed, &src.data[16..48]);
        assert!(
            std::ptr::eq(borrowed.as_ptr(), src.data[16..48].as_ptr()),
            "range_slice must borrow the backing buffer, not copy it"
        );

        // Whole-buffer and empty borrows are servable too.
        assert_eq!(
            src.range_slice(ByteRange::new(0, 64)).expect("whole").len(),
            64
        );
        assert!(
            src.range_slice(ByteRange::new(9, 9))
                .expect("empty")
                .is_empty()
        );

        // Anything it cannot serve in full falls back to the copying path.
        assert!(
            src.range_slice(ByteRange::new(60, 65)).is_none(),
            "past EOF"
        );
        assert!(src.range_slice(ByteRange::new(40, 8)).is_none(), "inverted");
        assert!(
            src.range_slice(ByteRange::new(u64::MAX - 1, u64::MAX))
                .is_none(),
            "unrepresentable offset"
        );
    }
}
