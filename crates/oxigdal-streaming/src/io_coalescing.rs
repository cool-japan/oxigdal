//! Parallel I/O coalescing for cloud object storage reads.
//!
//! When reading multiple byte ranges from the same object (e.g., COG tiles),
//! it is more efficient to:
//! 1. Merge nearby ranges into a single larger request (coalescing)
//! 2. Issue remaining ranges in parallel
//!
//! This avoids the overhead of many small HTTP range requests.

/// A byte range `[start, end)` (exclusive end).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ByteRange {
    /// Start offset (inclusive).
    pub start: u64,
    /// End offset (exclusive).
    pub end: u64,
}

impl ByteRange {
    /// Creates a new `ByteRange`.
    ///
    /// # Panics
    /// Panics in debug mode if `end < start`.
    pub fn new(start: u64, end: u64) -> Self {
        debug_assert!(end >= start, "end must be >= start");
        Self { start, end }
    }

    /// Returns the length of this range in bytes.
    #[must_use]
    pub fn len(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    /// Returns `true` if this range covers zero bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// Returns `true` if this range overlaps with or directly adjoins `other`.
    #[must_use]
    pub fn overlaps_or_adjoins(&self, other: &ByteRange) -> bool {
        self.start <= other.end && other.start <= self.end
    }

    /// Returns the smallest range that contains both `self` and `other`.
    #[must_use]
    pub fn merge(&self, other: &ByteRange) -> ByteRange {
        ByteRange {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    /// Returns the gap in bytes between `self.end` and `other.start`.
    /// Returns `0` if the ranges overlap or adjoin.
    #[must_use]
    pub fn gap_to(&self, other: &ByteRange) -> u64 {
        other.start.saturating_sub(self.end)
    }
}

/// Configuration for the I/O coalescing algorithm.
#[derive(Debug, Clone)]
pub struct CoalescingConfig {
    /// Merge two adjacent ranges when the gap between them is smaller than this threshold.
    pub max_gap_bytes: u64,
    /// Do not create a merged range larger than this limit.
    pub max_merged_size: u64,
    /// Maximum number of parallel fetch requests to issue.
    pub max_parallel_requests: usize,
}

impl Default for CoalescingConfig {
    fn default() -> Self {
        Self {
            max_gap_bytes: 8 * 1024,
            max_merged_size: 16 * 1024 * 1024,
            max_parallel_requests: 8,
        }
    }
}

/// A single coalesced (merged) fetch request with the original sub-ranges it covers.
#[derive(Debug, Clone)]
pub struct CoalescedRequest {
    /// The merged byte range that should actually be fetched.
    pub fetch_range: ByteRange,
    /// The original ranges that are covered by `fetch_range`.
    pub sub_ranges: Vec<ByteRange>,
}

impl CoalescedRequest {
    /// Extracts the slice corresponding to `sub_range` from a buffer that contains
    /// the full contents of `fetch_range`.
    ///
    /// Returns `None` if `sub_range` is not fully contained within `fetch_range`
    /// or if the buffer is too short.
    #[must_use]
    pub fn extract<'a>(&self, merged_data: &'a [u8], sub_range: &ByteRange) -> Option<&'a [u8]> {
        if sub_range.start < self.fetch_range.start || sub_range.end > self.fetch_range.end {
            return None;
        }
        let offset = (sub_range.start - self.fetch_range.start) as usize;
        let len = sub_range.len() as usize;
        let end = offset + len;
        if end <= merged_data.len() {
            Some(&merged_data[offset..end])
        } else {
            None
        }
    }
}

/// Coalesces a list of byte ranges according to `config`.
///
/// Ranges whose gap is smaller than `config.max_gap_bytes` are merged into a
/// single `CoalescedRequest`, provided the merged size does not exceed
/// `config.max_merged_size`.
pub fn coalesce_ranges(
    mut ranges: Vec<ByteRange>,
    config: &CoalescingConfig,
) -> Vec<CoalescedRequest> {
    if ranges.is_empty() {
        return Vec::new();
    }

    ranges.sort();
    ranges.dedup();

    let mut result: Vec<CoalescedRequest> = Vec::new();
    let mut current = CoalescedRequest {
        fetch_range: ranges[0].clone(),
        sub_ranges: vec![ranges[0].clone()],
    };

    for range in ranges.into_iter().skip(1) {
        let gap = current.fetch_range.gap_to(&range);
        let merged_size = range.end.saturating_sub(current.fetch_range.start);

        if gap <= config.max_gap_bytes && merged_size <= config.max_merged_size {
            current.fetch_range = current.fetch_range.merge(&range);
            current.sub_ranges.push(range);
        } else {
            result.push(current);
            current = CoalescedRequest {
                fetch_range: range.clone(),
                sub_ranges: vec![range],
            };
        }
    }
    result.push(current);
    result
}

/// Statistics describing the efficiency of a coalescing operation.
#[derive(Debug, Clone, Default)]
pub struct CoalescingStats {
    /// Number of original (pre-coalescing) range requests.
    pub original_requests: usize,
    /// Number of coalesced (post-coalescing) fetch requests.
    pub coalesced_requests: usize,
    /// Total bytes that will be fetched (including gap fill-in).
    pub bytes_fetched: u64,
    /// Total bytes actually needed (sum of original range lengths).
    pub bytes_needed: u64,
    /// Bytes fetched that are not part of any original range.
    pub overhead_bytes: u64,
}

impl CoalescingStats {
    /// Fraction of fetched bytes that are overhead (`0.0` = no overhead).
    #[must_use]
    pub fn overhead_ratio(&self) -> f64 {
        if self.bytes_needed == 0 {
            0.0
        } else {
            self.overhead_bytes as f64 / self.bytes_needed as f64
        }
    }

    /// Fraction by which the request count was reduced (`0.0` = no reduction,
    /// `1.0` = all merged into one).
    #[must_use]
    pub fn request_reduction(&self) -> f64 {
        if self.original_requests == 0 {
            0.0
        } else {
            1.0 - self.coalesced_requests as f64 / self.original_requests as f64
        }
    }
}

/// Computes [`CoalescingStats`] for the given original ranges and coalesced output.
#[must_use]
pub fn compute_stats(original: &[ByteRange], coalesced: &[CoalescedRequest]) -> CoalescingStats {
    let bytes_needed: u64 = original.iter().map(ByteRange::len).sum();
    let bytes_fetched: u64 = coalesced.iter().map(|c| c.fetch_range.len()).sum();
    CoalescingStats {
        original_requests: original.len(),
        coalesced_requests: coalesced.len(),
        bytes_fetched,
        bytes_needed,
        overhead_bytes: bytes_fetched.saturating_sub(bytes_needed),
    }
}
