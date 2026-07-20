//! Byte-range coalescing: column-chunk ranges → few HTTP fetches.
//!
//! Dedupes `(row_group, leaf_column)` ranges, sorts by start offset and
//! merges neighbors whose gap is at most 64 KiB into `FetchRange`s, then
//! maps each fetched buffer back to the per-chunk segments it serves
//! (one merged fetch may cover several column chunks).
//!
//! Implemented by WP C2 (GeoParquet Live lane); stub created by WP W0.

// Consumed by the wasm-only `session` bindings (WP C4); until that lands these
// helpers look unused to the non-test lib build.
#![allow(dead_code)]

use std::collections::HashSet;

use bytes::Bytes;

use crate::error::GpqLiveError;
use crate::sparse::Segment;

/// Neighbouring column chunks separated by no more than this many bytes are
/// merged into a single HTTP range request. 64 KiB trades a little wasted
/// download for markedly fewer round trips.
pub const MAX_GAP: u64 = 64 * 1024;

/// A single column chunk's byte range within the Parquet file, tagged with the
/// `(row_group, leaf_column)` it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkRange {
    /// Row-group index this chunk lives in.
    pub row_group: usize,
    /// Leaf-column index within the schema.
    pub leaf_column: usize,
    /// Absolute start offset (inclusive of the dictionary page).
    pub start: u64,
    /// Total compressed length of the chunk in bytes.
    pub len: u64,
}

/// A contiguous byte range to fetch over HTTP, possibly covering several
/// coalesced [`ChunkRange`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FetchRange {
    /// Absolute start offset of the fetch.
    pub start: u64,
    /// Length of the fetch in bytes.
    pub len: u64,
}

impl FetchRange {
    /// Absolute offset one past the last byte of this fetch.
    #[inline]
    #[must_use]
    pub fn end(&self) -> u64 {
        self.start + self.len
    }
}

/// The absolute byte span of one column chunk served by a merged fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChunkSpan {
    start: u64,
    len: u64,
}

/// The result of coalescing column-chunk ranges into a small set of fetches.
///
/// `fetches` are the HTTP range requests to issue (ascending, non-overlapping).
/// `mapping[i]` lists the column-chunk spans served by `fetches[i]`, letting the
/// caller carve a fetched buffer back into tight per-chunk [`Segment`]s.
#[derive(Debug, Clone, Default)]
pub struct CoalescedRanges {
    /// The HTTP range requests to issue, ascending by start.
    pub fetches: Vec<FetchRange>,
    /// Per-fetch list of the column-chunk spans it covers (parallel to `fetches`).
    mapping: Vec<Vec<ChunkSpan>>,
}

impl CoalescedRanges {
    /// Total bytes across all planned fetches (includes merged-over gap bytes).
    #[must_use]
    pub fn total_fetch_bytes(&self) -> u64 {
        self.fetches.iter().map(|f| f.len).sum()
    }

    /// Number of HTTP requests this plan will make.
    #[must_use]
    pub fn request_count(&self) -> usize {
        self.fetches.len()
    }

    /// Carves the fetched `buffers` (one per [`fetches`](Self::fetches), in order)
    /// back into tight per-chunk [`Segment`]s for a `SparseChunkReader`.
    ///
    /// Each buffer must have exactly the length of its corresponding fetch;
    /// a mismatch means the fetch layer returned the wrong bytes and yields a
    /// [`GpqLiveError::Parquet`]. The returned segments are sorted by start.
    pub fn segments(&self, buffers: &[Bytes]) -> Result<Vec<Segment>, GpqLiveError> {
        if buffers.len() != self.fetches.len() {
            return Err(GpqLiveError::Parquet(
                parquet::errors::ParquetError::General(format!(
                    "coalesce: expected {} fetched buffers, got {}",
                    self.fetches.len(),
                    buffers.len()
                )),
            ));
        }
        let mut segments = Vec::new();
        for ((fetch, spans), buf) in self.fetches.iter().zip(self.mapping.iter()).zip(buffers) {
            if buf.len() as u64 != fetch.len {
                return Err(GpqLiveError::Parquet(
                    parquet::errors::ParquetError::General(format!(
                        "coalesce: fetch at {} expected {} bytes, buffer has {}",
                        fetch.start,
                        fetch.len,
                        buf.len()
                    )),
                ));
            }
            for span in spans {
                let offset = (span.start - fetch.start) as usize;
                let end = offset + span.len as usize;
                segments.push(Segment {
                    start: span.start,
                    data: buf.slice(offset..end),
                });
            }
        }
        segments.sort_by_key(|s| s.start);
        Ok(segments)
    }
}

/// Coalesces column-chunk ranges into a minimal set of HTTP range requests.
///
/// Duplicate `(row_group, leaf_column)` entries are dropped, the survivors are
/// sorted by start offset, and neighbours whose gap is at most [`MAX_GAP`] are
/// merged. The returned [`CoalescedRanges`] retains the reverse mapping so a
/// fetched buffer can be sliced back into per-chunk segments.
#[must_use]
pub fn coalesce(ranges: &[ChunkRange]) -> CoalescedRanges {
    // Deduplicate by (row_group, leaf_column): the same chunk may be requested
    // once per role (bbox / filter / output).
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    let mut unique: Vec<ChunkRange> = Vec::with_capacity(ranges.len());
    for r in ranges {
        if seen.insert((r.row_group, r.leaf_column)) {
            unique.push(*r);
        }
    }
    // Sort by start offset so the merge scan is a single linear pass.
    unique.sort_by_key(|r| r.start);

    let mut fetches: Vec<FetchRange> = Vec::new();
    let mut mapping: Vec<Vec<ChunkSpan>> = Vec::new();

    for r in unique {
        let span = ChunkSpan {
            start: r.start,
            len: r.len,
        };
        let merge = match fetches.last() {
            // `r.start >= last.start` holds because `unique` is sorted, so this
            // covers both overlap (`r.start <= last.end`) and a small gap.
            Some(last) => r.start <= last.end() + MAX_GAP,
            None => false,
        };
        if merge {
            let idx = fetches.len() - 1;
            let new_end = (r.start + r.len).max(fetches[idx].end());
            fetches[idx].len = new_end - fetches[idx].start;
            mapping[idx].push(span);
        } else {
            fetches.push(FetchRange {
                start: r.start,
                len: r.len,
            });
            mapping.push(vec![span]);
        }
    }

    CoalescedRanges { fetches, mapping }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn chunk(rg: usize, leaf: usize, start: u64, len: u64) -> ChunkRange {
        ChunkRange {
            row_group: rg,
            leaf_column: leaf,
            start,
            len,
        }
    }

    #[test]
    fn empty_input_yields_no_fetches() {
        let c = coalesce(&[]);
        assert!(c.fetches.is_empty());
        assert_eq!(c.request_count(), 0);
        assert_eq!(c.total_fetch_bytes(), 0);
    }

    #[test]
    fn single_range_becomes_single_fetch() {
        let c = coalesce(&[chunk(0, 0, 100, 50)]);
        assert_eq!(
            c.fetches,
            vec![FetchRange {
                start: 100,
                len: 50
            }]
        );
    }

    #[test]
    fn adjacent_ranges_merge() {
        // gap of 0 (touching) -> merged
        let c = coalesce(&[chunk(0, 0, 100, 50), chunk(0, 1, 150, 30)]);
        assert_eq!(
            c.fetches,
            vec![FetchRange {
                start: 100,
                len: 80
            }]
        );
    }

    #[test]
    fn ranges_within_gap_threshold_merge() {
        // gap exactly MAX_GAP -> merge; end of first = 200, next start = 200+MAX_GAP
        let c = coalesce(&[chunk(0, 0, 100, 100), chunk(0, 1, 200 + MAX_GAP, 40)]);
        assert_eq!(c.request_count(), 1);
        let f = c.fetches[0];
        assert_eq!(f.start, 100);
        assert_eq!(f.end(), 200 + MAX_GAP + 40);
    }

    #[test]
    fn ranges_beyond_gap_threshold_stay_separate() {
        // gap = MAX_GAP + 1 -> not merged
        let c = coalesce(&[chunk(0, 0, 100, 100), chunk(0, 1, 200 + MAX_GAP + 1, 40)]);
        assert_eq!(c.request_count(), 2);
    }

    #[test]
    fn overlapping_ranges_merge_using_max_end() {
        // second range is fully contained inside the first
        let c = coalesce(&[chunk(0, 0, 100, 200), chunk(0, 1, 150, 10)]);
        assert_eq!(
            c.fetches,
            vec![FetchRange {
                start: 100,
                len: 200
            }]
        );
    }

    #[test]
    fn duplicate_rg_leaf_is_deduped() {
        let c = coalesce(&[
            chunk(3, 2, 500, 40),
            chunk(3, 2, 500, 40),
            chunk(3, 2, 500, 40),
        ]);
        assert_eq!(
            c.fetches,
            vec![FetchRange {
                start: 500,
                len: 40
            }]
        );
    }

    #[test]
    fn unsorted_input_is_sorted_before_merging() {
        let c = coalesce(&[
            chunk(2, 0, 2_000_000, 20), // far away -> beyond MAX_GAP
            chunk(0, 0, 100, 20),
            chunk(1, 0, 130, 20), // gap 10 from previous -> merges with 100-block
        ]);
        // 100..120 and 130..150 merge (gap 10); 2_000_000 stays separate
        assert_eq!(
            c.fetches,
            vec![
                FetchRange {
                    start: 100,
                    len: 50
                },
                FetchRange {
                    start: 2_000_000,
                    len: 20
                },
            ]
        );
    }

    #[test]
    fn segments_maps_merged_buffer_back_to_per_chunk() {
        // Two chunks merged into one fetch [100,180): chunk A [100,150) with a
        // 20-byte gap, chunk B [170,180).
        let c = coalesce(&[chunk(0, 0, 100, 50), chunk(0, 1, 170, 10)]);
        assert_eq!(c.request_count(), 1);
        let f = c.fetches[0];
        assert_eq!((f.start, f.len), (100, 80));

        // Build a fake fetched buffer of exactly 80 bytes.
        let mut data = vec![0u8; 80];
        for (i, b) in data.iter_mut().enumerate() {
            *b = i as u8;
        }
        let segs = c.segments(&[Bytes::from(data)]).unwrap();
        assert_eq!(segs.len(), 2);
        // segment for chunk A: absolute [100,150), buffer offset 0..50
        assert_eq!(segs[0].start, 100);
        assert_eq!(segs[0].data.len(), 50);
        assert_eq!(segs[0].data[0], 0);
        assert_eq!(segs[0].data[49], 49);
        // segment for chunk B: absolute [170,180), buffer offset 70..80
        assert_eq!(segs[1].start, 170);
        assert_eq!(segs[1].data.len(), 10);
        assert_eq!(segs[1].data[0], 70);
        assert_eq!(segs[1].data[9], 79);
    }

    #[test]
    fn segments_across_multiple_fetches_are_sorted() {
        let c = coalesce(&[chunk(0, 0, 100, 10), chunk(1, 0, 2_000_000, 10)]);
        assert_eq!(c.request_count(), 2);
        let b0 = Bytes::from(vec![1u8; 10]);
        let b1 = Bytes::from(vec![2u8; 10]);
        let segs = c.segments(&[b0, b1]).unwrap();
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].start, 100);
        assert_eq!(segs[1].start, 2_000_000);
    }

    #[test]
    fn segments_rejects_wrong_buffer_count() {
        let c = coalesce(&[chunk(0, 0, 100, 10), chunk(1, 0, 2_000_000, 10)]);
        let err = c.segments(&[Bytes::from(vec![0u8; 10])]).unwrap_err();
        assert!(err.to_string().contains("expected 2 fetched buffers"));
    }

    #[test]
    fn segments_rejects_wrong_buffer_length() {
        let c = coalesce(&[chunk(0, 0, 100, 50)]);
        let err = c.segments(&[Bytes::from(vec![0u8; 49])]).unwrap_err();
        assert!(err.to_string().contains("expected 50 bytes"));
    }
}
