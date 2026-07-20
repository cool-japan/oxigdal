//! Memory-mapped file support for local reads.
//!
//! For local files, this module provides an in-memory buffer backed view that
//! simulates mmap semantics using pure Rust `std::fs` I/O.  On platforms where
//! OS-level mmap is available a future feature flag could swap in a proper
//! implementation without changing the public API.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::error::StreamingError;

/// A memory-buffered view of a file, providing mmap-like random-access semantics.
pub struct MappedFile {
    path: PathBuf,
    data: Vec<u8>,
    file_size: u64,
}

impl MappedFile {
    /// Opens `path` and loads its entire contents into memory.
    ///
    /// # Errors
    /// Returns [`StreamingError`] on any I/O failure.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StreamingError> {
        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path)?;
        let file_size = file.seek(SeekFrom::End(0))?;
        file.seek(SeekFrom::Start(0))?;

        let mut data = Vec::with_capacity(file_size as usize);
        file.read_to_end(&mut data)?;

        Ok(Self {
            path,
            data,
            file_size,
        })
    }

    /// Returns a slice covering bytes `[start, start + len)`.
    ///
    /// # Errors
    /// Returns an error if the requested range falls outside the file.
    pub fn read_range(&self, start: u64, len: usize) -> Result<&[u8], StreamingError> {
        let start_usize = start as usize;
        let end = start_usize + len;
        if end > self.data.len() {
            return Err(StreamingError::Other(format!(
                "Range [{start_usize}, {end}) out of bounds (file size {})",
                self.file_size
            )));
        }
        Ok(&self.data[start_usize..end])
    }

    /// Returns a slice of all file bytes.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    /// Returns the file size in bytes.
    #[must_use]
    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    /// Returns the path used to open this file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns `true` if the file contains no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Reads multiple `(start, len)` ranges simultaneously.
    ///
    /// The returned `Vec` preserves the order of the input slice.
    pub fn read_ranges(&self, ranges: &[(u64, usize)]) -> Vec<Result<&[u8], StreamingError>> {
        ranges
            .iter()
            .map(|(start, len)| self.read_range(*start, *len))
            .collect()
    }
}

// ── Prefetch support ─────────────────────────────────────────────────────────

/// Priority level for a prefetch hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefetchPriority {
    /// Background, best-effort prefetch.
    Low,
    /// Default prefetch priority.
    Normal,
    /// Urgent prefetch — schedule first.
    High,
}

impl PartialOrd for PrefetchPriority {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrefetchPriority {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let rank = |p: &PrefetchPriority| match p {
            PrefetchPriority::Low => 0u8,
            PrefetchPriority::Normal => 1,
            PrefetchPriority::High => 2,
        };
        rank(self).cmp(&rank(other))
    }
}

/// A hint advising the scheduler to prefetch a region of a file.
#[derive(Debug, Clone)]
pub struct PrefetchHint {
    /// Byte offset where the region starts.
    pub offset: u64,
    /// Number of bytes to prefetch.
    pub length: usize,
    /// Priority of this prefetch hint.
    pub priority: PrefetchPriority,
}

/// Collects prefetch hints and can return them in priority order.
pub struct PrefetchScheduler {
    hints: Vec<PrefetchHint>,
    max_prefetch_bytes: usize,
}

impl PrefetchScheduler {
    /// Creates a new `PrefetchScheduler` with the given byte cap.
    #[must_use]
    pub fn new(max_prefetch_bytes: usize) -> Self {
        Self {
            hints: Vec::new(),
            max_prefetch_bytes,
        }
    }

    /// Adds a prefetch hint.
    pub fn add_hint(&mut self, hint: PrefetchHint) {
        self.hints.push(hint);
    }

    /// Returns hints sorted by descending priority, then ascending offset.
    #[must_use]
    pub fn sorted_hints(&self) -> Vec<&PrefetchHint> {
        let mut sorted: Vec<&PrefetchHint> = self.hints.iter().collect();
        sorted.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.offset.cmp(&b.offset)));
        sorted
    }

    /// Returns the number of hints currently held.
    #[must_use]
    pub fn hint_count(&self) -> usize {
        self.hints.len()
    }

    /// Returns the sum of all hinted lengths.
    #[must_use]
    pub fn total_bytes_hinted(&self) -> usize {
        self.hints.iter().map(|h| h.length).sum()
    }

    /// Returns the configured maximum prefetch byte limit.
    #[must_use]
    pub fn max_prefetch_bytes(&self) -> usize {
        self.max_prefetch_bytes
    }

    /// Clears all stored hints.
    pub fn clear(&mut self) {
        self.hints.clear();
    }
}
