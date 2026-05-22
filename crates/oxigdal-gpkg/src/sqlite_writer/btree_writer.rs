//! SQLite leaf-table B-tree page writer.
//!
//! Produces byte-perfect single-leaf-page B-trees that are readable by
//! [`crate::btree::parse_leaf_table_page`].
//!
//! # Page layout (type 0x0D — leaf table)
//!
//! ```text
//! [header_offset .. header_offset+8]  leaf page header
//!   +0  page type       = 0x0D
//!   +1  freeblock hi    = 0x00
//!   +2  freeblock lo    = 0x00
//!   +3  cell count hi
//!   +4  cell count lo
//!   +5  content-start hi
//!   +6  content-start lo
//!   +7  fragmented free bytes = 0x00
//! [header_offset+8 .. header_offset+8+cell_count*2]  cell pointer array
//!   each pointer is a big-endian u16 offset from the start of the page
//! [content start .. PAGE_SIZE]  cell content area (grows from bottom)
//!   each cell: varint(payload_len) varint(rowid) payload_bytes
//! ```

use crate::btree::encode_sqlite_varint;
use crate::error::GpkgError;

use super::{PAGE_SIZE, page_allocator::PageAllocator};

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

/// SQLite leaf table page type byte.
const PAGE_TYPE_LEAF: u8 = 0x0D;

/// Leaf page header is 8 bytes.
const LEAF_HEADER_BYTES: usize = 8;

/// Maximum inline payload size per SQLite spec: `usable_size - 35`.
/// With page_size = 4096 and reserved_bytes = 0: 4096 - 35 = 4061.
pub const MAX_PAYLOAD_INLINE: usize = PAGE_SIZE - 35;

// ─────────────────────────────────────────────────────────────────────────────
// LeafPageBuilder
// ─────────────────────────────────────────────────────────────────────────────

/// Accumulates `(rowid, encoded_record)` pairs and emits a single leaf page.
///
/// Records exceeding [`MAX_PAYLOAD_INLINE`] bytes will cause
/// [`write_table`] to return [`GpkgError::RowOverflowsPage`].
/// Interior page chains (for large tables) are not yet supported.
pub struct LeafPageBuilder {
    /// Accumulated rows: (rowid, encoded record payload).
    rows: Vec<(i64, Vec<u8>)>,
    /// Running total of bytes consumed in the page's cell content area.
    current_used: usize,
}

impl LeafPageBuilder {
    /// Create a new, empty builder.
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            current_used: 0,
        }
    }

    /// Attempt to add a row to this page.
    ///
    /// Returns `true` when the row fits, `false` when it would overflow the
    /// page (caller should write a new page).
    ///
    /// Space consumed per row:
    /// - 2 bytes: cell-pointer entry in the pointer array
    /// - `varint(payload_len)` bytes (1-9 bytes)
    /// - `varint(rowid)` bytes (1-9 bytes)
    /// - `payload.len()` bytes
    pub fn try_add(&mut self, rowid: i64, payload: Vec<u8>) -> bool {
        let pl_varint = encode_sqlite_varint(payload.len() as u64);
        let rid_varint = encode_sqlite_varint(rowid as u64);
        let cell_bytes = pl_varint.len() + rid_varint.len() + payload.len();

        // Total overhead for this row: 2 (pointer entry) + cell_bytes.
        let needed = 2 + cell_bytes;

        // Available = PAGE_SIZE
        //           - header_offset (handled by emit)
        //           - LEAF_HEADER_BYTES
        //           - already-used pointer array (rows.len() * 2)
        //           - already-used content area (current_used)
        //           - this row's pointer (2)
        // We compute conservatively with header_offset = 100 (worst case = page 1).
        let worst_header_offset = 100;
        let used_so_far = worst_header_offset
            + LEAF_HEADER_BYTES
            + self.rows.len() * 2  // existing pointer entries
            + self.current_used;

        if used_so_far + needed > PAGE_SIZE {
            return false;
        }

        self.current_used += cell_bytes;
        self.rows.push((rowid, payload));
        true
    }

    /// Emit the complete leaf page bytes (exactly [`PAGE_SIZE`] bytes).
    ///
    /// # Arguments
    /// * `header_offset` — byte offset within the page where the 8-byte leaf
    ///   header starts.  Always `0` for pages 2..=N; always `100` for page 1
    ///   (the SQLite file header occupies the first 100 bytes).
    pub fn emit(&self, header_offset: usize) -> Vec<u8> {
        let mut page = vec![0u8; PAGE_SIZE];
        let cell_count = self.rows.len();

        // Build cell content from the bottom of the page upward.
        let mut content_end = PAGE_SIZE;
        let mut cell_offsets: Vec<usize> = Vec::with_capacity(cell_count);

        for (rowid, payload) in &self.rows {
            let pl_varint = encode_sqlite_varint(payload.len() as u64);
            let rid_varint = encode_sqlite_varint(*rowid as u64);
            let cell_size = pl_varint.len() + rid_varint.len() + payload.len();

            content_end -= cell_size;
            let start = content_end;
            cell_offsets.push(start);

            let mut pos = start;
            page[pos..pos + pl_varint.len()].copy_from_slice(&pl_varint);
            pos += pl_varint.len();
            page[pos..pos + rid_varint.len()].copy_from_slice(&rid_varint);
            pos += rid_varint.len();
            page[pos..pos + payload.len()].copy_from_slice(payload);
        }

        // Write leaf page header at `header_offset`.
        let hdr = header_offset;
        page[hdr] = PAGE_TYPE_LEAF;
        page[hdr + 1] = 0; // first freeblock offset (hi)
        page[hdr + 2] = 0; // first freeblock offset (lo)
        let cc = cell_count as u16;
        page[hdr + 3] = (cc >> 8) as u8;
        page[hdr + 4] = (cc & 0xFF) as u8;
        let content_start_val = content_end as u16;
        page[hdr + 5] = (content_start_val >> 8) as u8;
        page[hdr + 6] = (content_start_val & 0xFF) as u8;
        page[hdr + 7] = 0; // fragmented free bytes

        // Write cell pointer array immediately after the header.
        let ptr_start = hdr + LEAF_HEADER_BYTES;
        for (i, &offset) in cell_offsets.iter().enumerate() {
            let o = offset as u16;
            page[ptr_start + i * 2] = (o >> 8) as u8;
            page[ptr_start + i * 2 + 1] = (o & 0xFF) as u8;
        }

        page
    }
}

impl Default for LeafPageBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// write_table — allocate + fill a single leaf page
// ─────────────────────────────────────────────────────────────────────────────

/// Allocate one page from `allocator`, write all `rows` to a single leaf page,
/// and return the root page number.
///
/// Returns [`GpkgError::RowOverflowsPage`] if any individual record exceeds
/// [`MAX_PAYLOAD_INLINE`] bytes or if the total rows do not fit on a single
/// leaf page.
///
/// # Arguments
/// * `allocator`     — page pool to allocate from
/// * `rows`          — `(rowid, record_payload)` pairs in rowid order
/// * `header_offset` — pass `100` for page 1, `0` for all other pages
pub fn write_table(
    allocator: &mut PageAllocator,
    rows: &[(i64, Vec<u8>)],
    header_offset: usize,
) -> Result<u32, GpkgError> {
    let page_num = allocator.alloc();

    let mut builder = LeafPageBuilder::new();
    for (rowid, payload) in rows {
        if payload.len() > MAX_PAYLOAD_INLINE {
            return Err(GpkgError::RowOverflowsPage {
                size: payload.len(),
                max: MAX_PAYLOAD_INLINE,
            });
        }
        if !builder.try_add(*rowid, payload.clone()) {
            return Err(GpkgError::RowOverflowsPage {
                size: payload.len(),
                max: MAX_PAYLOAD_INLINE,
            });
        }
    }

    let page_bytes = builder.emit(header_offset);
    allocator.write(page_num, page_bytes);
    Ok(page_num)
}
