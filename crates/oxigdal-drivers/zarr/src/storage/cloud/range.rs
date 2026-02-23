//! Byte range for partial object reads

// ============================================================================
// Range Request
// ============================================================================

/// Byte range for partial object reads
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    /// Start offset (inclusive)
    pub start: u64,
    /// End offset (exclusive)
    pub end: u64,
}

impl ByteRange {
    /// Creates a new byte range
    ///
    /// # Arguments
    /// * `start` - Start offset (inclusive)
    /// * `end` - End offset (exclusive)
    ///
    /// # Returns
    /// `Some(ByteRange)` if valid (start < end), `None` otherwise
    #[must_use]
    pub fn new(start: u64, end: u64) -> Option<Self> {
        if start < end {
            Some(Self { start, end })
        } else {
            None
        }
    }

    /// Creates a range from start to the end of the object
    #[must_use]
    pub fn from_start(start: u64) -> Self {
        Self {
            start,
            end: u64::MAX,
        }
    }

    /// Creates a range for the last N bytes
    #[must_use]
    pub fn last_n_bytes(n: u64) -> Self {
        Self {
            start: u64::MAX - n,
            end: u64::MAX,
        }
    }

    /// Returns the length of this range
    #[must_use]
    pub fn len(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    /// Returns true if the range is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// Formats the range as an HTTP Range header value
    #[must_use]
    pub fn to_http_header(&self) -> String {
        if self.end == u64::MAX {
            if self.start == u64::MAX.saturating_sub(self.len()) {
                format!("bytes=-{}", self.len())
            } else {
                format!("bytes={}-", self.start)
            }
        } else {
            format!("bytes={}-{}", self.start, self.end - 1)
        }
    }

    /// Checks if this range overlaps with another
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        self.start < other.end && other.start < self.end
    }

    /// Checks if this range is contiguous with another
    #[must_use]
    pub fn is_contiguous(&self, other: &Self) -> bool {
        self.end == other.start || other.end == self.start
    }

    /// Merges this range with another if they overlap or are contiguous
    #[must_use]
    pub fn merge(&self, other: &Self) -> Option<Self> {
        if self.overlaps(other) || self.is_contiguous(other) {
            Some(Self {
                start: self.start.min(other.start),
                end: self.end.max(other.end),
            })
        } else {
            None
        }
    }
}
