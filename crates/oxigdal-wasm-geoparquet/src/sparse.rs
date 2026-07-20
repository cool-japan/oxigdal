//! `SparseChunkReader` — a [`parquet`] `ChunkReader` over prefetched segments.
//!
//! Holds the file's total length plus a sorted, non-overlapping list of
//! `(start, Bytes)` segments that were fetched via HTTP range requests.
//! `get_bytes` resolves reads with a binary search and zero-copy
//! `Bytes::slice`; any read outside a prefetched segment is a planning
//! bug and surfaces as a descriptive `ParquetError`.
//!
//! Implemented by WP C2 (GeoParquet Live lane); stub created by WP W0.

// This reader is consumed by the wasm-only `session` bindings (WP C4); until
// that lands the type looks unused to the non-test lib build.
#![allow(dead_code)]

use bytes::{Buf, Bytes};
use parquet::errors::{ParquetError, Result as ParquetResult};
use parquet::file::reader::{ChunkReader, Length};

/// A contiguous run of prefetched bytes located at an absolute file offset.
///
/// Segments are the unit the [`SparseChunkReader`] serves reads from: a
/// segment's `data` covers the byte range `[start, start + data.len())` of the
/// original Parquet file. Within a reader the segments are kept sorted by
/// `start` and are guaranteed non-overlapping.
#[derive(Debug, Clone)]
pub struct Segment {
    /// Absolute offset of this segment's first byte within the source file.
    pub start: u64,
    /// The prefetched bytes (zero-copy shareable).
    pub data: Bytes,
}

impl Segment {
    /// Absolute offset one past this segment's last byte.
    #[inline]
    #[must_use]
    pub fn end(&self) -> u64 {
        self.start + self.data.len() as u64
    }
}

/// A [`parquet`] [`ChunkReader`] backed by a sparse set of prefetched segments.
///
/// The predicate-pushdown planner determines exactly which column-chunk byte
/// ranges a query needs before any data is downloaded. Those ranges are fetched
/// (and merged) into [`Segment`]s and handed to this reader, which then satisfies
/// every `get_bytes` / `get_read` the Parquet decoder issues from memory.
///
/// A read that falls outside all prefetched segments means the planner and the
/// decoder disagree about what is needed; that is a bug, so it surfaces as a
/// descriptive [`ParquetError::General`] rather than triggering I/O.
#[derive(Debug, Clone)]
pub struct SparseChunkReader {
    total_len: u64,
    segments: Vec<Segment>,
}

impl SparseChunkReader {
    /// Builds a reader over `segments` for a source file of `total_len` bytes.
    ///
    /// The segments are sorted by start offset defensively; callers are expected
    /// to supply non-overlapping ranges (the coalescing pass guarantees this).
    #[must_use]
    pub fn new(total_len: u64, mut segments: Vec<Segment>) -> Self {
        segments.sort_by_key(|s| s.start);
        Self {
            total_len,
            segments,
        }
    }

    /// Number of prefetched segments.
    #[must_use]
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Total bytes held across all prefetched segments.
    #[must_use]
    pub fn prefetched_bytes(&self) -> u64 {
        self.segments.iter().map(|s| s.data.len() as u64).sum()
    }

    /// Returns the segment that contains absolute offset `start`, if any.
    ///
    /// Uses `partition_point` for an `O(log n)` search: the first segment whose
    /// start is greater than `start` is one past the candidate, so the candidate
    /// is at `idx - 1` — provided it actually spans `start`.
    fn containing_segment(&self, start: u64) -> Option<&Segment> {
        let idx = self.segments.partition_point(|s| s.start <= start);
        if idx == 0 {
            return None;
        }
        let seg = &self.segments[idx - 1];
        if start < seg.end() { Some(seg) } else { None }
    }
}

impl Length for SparseChunkReader {
    fn len(&self) -> u64 {
        self.total_len
    }
}

impl ChunkReader for SparseChunkReader {
    type T = bytes::buf::Reader<Bytes>;

    fn get_read(&self, start: u64) -> ParquetResult<Self::T> {
        match self.containing_segment(start) {
            Some(seg) => {
                let offset = (start - seg.start) as usize;
                Ok(seg.data.slice(offset..).reader())
            }
            None => Err(ParquetError::General(format!(
                "SparseChunkReader: read at {start} not prefetched"
            ))),
        }
    }

    fn get_bytes(&self, start: u64, length: usize) -> ParquetResult<Bytes> {
        if length == 0 {
            return Ok(Bytes::new());
        }
        // Binary-search for the single segment that must fully contain the read.
        let idx = self.segments.partition_point(|s| s.start <= start);
        if idx != 0 {
            let seg = &self.segments[idx - 1];
            let end = start + length as u64;
            if start >= seg.start && end <= seg.end() {
                let offset = (start - seg.start) as usize;
                return Ok(seg.data.slice(offset..offset + length));
            }
        }
        Err(ParquetError::General(format!(
            "SparseChunkReader: range {start}+{length} not prefetched"
        )))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::io::Read;

    fn seg(start: u64, bytes: &[u8]) -> Segment {
        Segment {
            start,
            data: Bytes::copy_from_slice(bytes),
        }
    }

    fn reader() -> SparseChunkReader {
        // Two disjoint segments: [100,110) and [200,205).
        SparseChunkReader::new(1_000, vec![seg(100, b"0123456789"), seg(200, b"ABCDE")])
    }

    #[test]
    fn length_reports_total_len_not_prefetched_len() {
        let r = reader();
        assert_eq!(r.len(), 1_000);
        assert_eq!(r.prefetched_bytes(), 15);
        assert_eq!(r.segment_count(), 2);
    }

    #[test]
    fn get_bytes_exact_segment_hit() {
        let r = reader();
        let out = r.get_bytes(100, 10).unwrap();
        assert_eq!(&out[..], b"0123456789");
        let out2 = r.get_bytes(200, 5).unwrap();
        assert_eq!(&out2[..], b"ABCDE");
    }

    #[test]
    fn get_bytes_sub_range_within_segment() {
        let r = reader();
        let out = r.get_bytes(103, 4).unwrap();
        assert_eq!(&out[..], b"3456");
    }

    #[test]
    fn get_bytes_zero_length_is_empty_ok() {
        let r = reader();
        let out = r.get_bytes(500, 0).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn get_bytes_miss_before_first_segment() {
        let r = reader();
        let err = r.get_bytes(0, 4).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("range 0+4 not prefetched"), "{msg}");
    }

    #[test]
    fn get_bytes_miss_in_gap_between_segments() {
        let r = reader();
        let err = r.get_bytes(150, 4).unwrap_err();
        assert!(err.to_string().contains("range 150+4 not prefetched"));
    }

    #[test]
    fn get_bytes_miss_spanning_two_segments_is_rejected() {
        // A read starting in the first segment but running past its end must
        // NOT be stitched across segments.
        let r = reader();
        let err = r.get_bytes(105, 100).unwrap_err();
        assert!(err.to_string().contains("range 105+100 not prefetched"));
    }

    #[test]
    fn get_bytes_boundary_end_exactly_at_segment_end_is_hit() {
        let r = reader();
        // last byte of first segment: [109,110)
        let out = r.get_bytes(109, 1).unwrap();
        assert_eq!(&out[..], b"9");
        // whole second segment right to its end
        let out2 = r.get_bytes(200, 5).unwrap();
        assert_eq!(&out2[..], b"ABCDE");
    }

    #[test]
    fn get_bytes_boundary_one_past_segment_end_is_miss() {
        let r = reader();
        // reading [110,111) is one past the first segment
        let err = r.get_bytes(110, 1).unwrap_err();
        assert!(err.to_string().contains("range 110+1 not prefetched"));
        // reading the last byte plus one over the end of a segment
        let err2 = r.get_bytes(109, 2).unwrap_err();
        assert!(err2.to_string().contains("range 109+2 not prefetched"));
    }

    #[test]
    fn get_bytes_start_exactly_at_next_segment_start() {
        // Offset equal to a segment's start selects that segment, not the prior.
        let r = reader();
        let out = r.get_bytes(200, 1).unwrap();
        assert_eq!(&out[..], b"A");
    }

    #[test]
    fn get_read_returns_reader_from_offset_to_segment_end() {
        let r = reader();
        let mut rdr = r.get_read(103).unwrap();
        let mut buf = Vec::new();
        rdr.read_to_end(&mut buf).unwrap();
        assert_eq!(&buf[..], b"3456789");
    }

    #[test]
    fn get_read_miss_is_descriptive_error() {
        let r = reader();
        let err = r.get_read(150).unwrap_err();
        assert!(err.to_string().contains("read at 150 not prefetched"));
    }

    #[test]
    fn new_sorts_out_of_order_segments() {
        let r = SparseChunkReader::new(1_000, vec![seg(200, b"ABCDE"), seg(100, b"0123456789")]);
        assert_eq!(&r.get_bytes(100, 3).unwrap()[..], b"012");
        assert_eq!(&r.get_bytes(200, 3).unwrap()[..], b"ABC");
    }

    #[test]
    fn slice_is_zero_copy_shares_backing() {
        // Bytes::slice shares the same allocation; verify the returned bytes are
        // a view (identical content, independent handle).
        let data = Bytes::from_static(b"zero-copy-check!!");
        let r = SparseChunkReader::new(64, vec![Segment { start: 0, data }]);
        let a = r.get_bytes(0, 4).unwrap();
        let b = r.get_bytes(0, 4).unwrap();
        assert_eq!(a, b);
        assert_eq!(&a[..], b"zero");
    }
}
