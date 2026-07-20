//! Sequential page allocator for the SQLite writer.
//!
//! Pages are assigned monotonically-increasing 1-indexed numbers, matching the
//! SQLite file format (page 1 is always the first 4096 bytes of the file).

use super::PAGE_SIZE;

/// Manages a pool of allocated pages and assembles them into a flat byte buffer.
///
/// All pages must be exactly [`PAGE_SIZE`] bytes when written via
/// [`PageAllocator::write`].  A page number is reserved with
/// [`PageAllocator::alloc`] and its content is committed later with
/// [`PageAllocator::write`].
pub struct PageAllocator {
    /// Next page number to hand out (1-indexed).
    next_page: u32,
    /// Committed pages: `(page_number, page_bytes)`.  Stored in insertion order;
    /// [`finalize`] sorts by page number before concatenating.
    pages: Vec<(u32, Vec<u8>)>,
}

impl PageAllocator {
    /// Create a new allocator.  Page numbering starts at 1.
    pub fn new() -> Self {
        Self {
            next_page: 1,
            pages: Vec::new(),
        }
    }

    /// Allocate the next page number and return it.
    ///
    /// This does **not** write any bytes — call [`write`] separately with the
    /// same page number once the content is ready.
    pub fn alloc(&mut self) -> u32 {
        let n = self.next_page;
        self.next_page += 1;
        n
    }

    /// Commit `bytes` as the content of `page_num`.
    ///
    /// # Panics
    /// Panics in debug mode when `bytes.len() != PAGE_SIZE`.
    pub fn write(&mut self, page_num: u32, bytes: Vec<u8>) {
        debug_assert_eq!(
            bytes.len(),
            PAGE_SIZE,
            "page {page_num}: expected {PAGE_SIZE} bytes, got {}",
            bytes.len()
        );
        self.pages.push((page_num, bytes));
    }

    /// Return the total number of pages allocated so far.
    pub fn page_count(&self) -> u32 {
        self.next_page - 1
    }

    /// Sort all committed pages by page number, assert contiguous coverage
    /// starting from page 1, and concatenate them into a single flat buffer.
    ///
    /// Pages whose content was never committed (via [`write`]) will be
    /// represented as zero-filled pages.
    pub fn finalize(mut self) -> Vec<u8> {
        let total = self.page_count() as usize;
        self.pages.sort_by_key(|(n, _)| *n);

        let mut out = Vec::with_capacity(total * PAGE_SIZE);

        let mut committed_iter = self.pages.into_iter().peekable();
        for page_idx in 1..=(total as u32) {
            let has_this_page = committed_iter
                .peek()
                .map(|(n, _)| *n == page_idx)
                .unwrap_or(false);

            if has_this_page {
                if let Some((_, bytes)) = committed_iter.next() {
                    // Pad or truncate to exact PAGE_SIZE
                    if bytes.len() == PAGE_SIZE {
                        out.extend_from_slice(&bytes);
                    } else {
                        let mut padded = bytes;
                        padded.resize(PAGE_SIZE, 0);
                        out.extend_from_slice(&padded);
                    }
                }
            } else {
                // No bytes were written for this page — emit zeros.
                out.extend_from_slice(&[0u8; PAGE_SIZE]);
            }
        }

        out
    }
}

impl Default for PageAllocator {
    fn default() -> Self {
        Self::new()
    }
}
