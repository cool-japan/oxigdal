//! WASM linear-memory management utilities.
//!
//! When passing data across the WASM boundary the host must be able to
//! allocate and free buffers inside the WASM linear memory.  The exported
//! `oxigdal_alloc` / `oxigdal_dealloc` pair provides that contract.
//!
//! Additionally, [`WasmBumpAllocator`] gives a cheap arena allocator for
//! short-lived temporary data inside WASM, avoiding the overhead of the
//! global allocator for bulk processing pipelines.

// The exported allocator/deallocator functions are intentionally unsafe extern
// "C" with `no_mangle`, required for the WASM Component Model ABI.  All unsafe
// invariants are documented on each function.
#![allow(unsafe_code)]

/// WASM linear-memory page size (64 KiB).
pub const WASM_PAGE_SIZE: usize = 65_536;

/// Allocate `size` bytes with a standard alignment guarantee (8-byte aligned).
///
/// The host calls this function to obtain a write buffer inside WASM linear
/// memory.  The returned pointer must be freed with [`oxigdal_dealloc`] using
/// the same `size`.
///
/// Returns a null pointer when `size` is zero.
#[unsafe(no_mangle)]
pub extern "C" fn oxigdal_alloc(size: usize) -> *mut u8 {
    if size == 0 {
        return std::ptr::null_mut();
    }
    // Use an aligned Vec<u64> so that the allocation is always 8-byte aligned,
    // which satisfies the alignment requirements of all ComponentDataType values.
    let align_units = size.div_ceil(8);
    let mut buf: Vec<u64> = Vec::with_capacity(align_units);
    let ptr = buf.as_mut_ptr().cast::<u8>();
    std::mem::forget(buf);
    ptr
}

/// Deallocate memory that was previously allocated by [`oxigdal_alloc`].
///
/// # Safety
///
/// - `ptr` **must** have been returned by [`oxigdal_alloc`] with the given
///   `size`.
/// - This function must be called exactly once for each allocation.
/// - Passing a null pointer is safe and is a no-op.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_dealloc(ptr: *mut u8, size: usize) {
    if ptr.is_null() || size == 0 {
        return;
    }
    let align_units = size.div_ceil(8);
    // Reconstruct the Vec<u64> that was created in `oxigdal_alloc` and let it
    // drop, freeing the memory back to the global allocator.
    // SAFETY: ptr was allocated by `oxigdal_alloc` with capacity `align_units`
    // Vec<u64> units, the length is 0 (no initialised elements), and this
    // function is called exactly once per allocation.
    unsafe {
        drop(Vec::from_raw_parts(ptr.cast::<u64>(), 0, align_units));
    }
}

/// Snapshot of WASM memory utilisation (for diagnostics / profiling).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WasmMemoryStats {
    /// Total bytes currently tracked as allocated.
    pub allocated_bytes: usize,
    /// Cumulative number of successful allocation calls.
    pub allocation_count: u64,
    /// Cumulative number of deallocation calls.
    pub deallocation_count: u64,
}

/// A simple linear (bump) arena allocator backed by a single contiguous buffer.
///
/// Ideal for short-lived, predictably-sized allocations inside WASM where the
/// global allocator's overhead is undesirable.  All allocations are freed at
/// once by calling [`reset`](WasmBumpAllocator::reset).
pub struct WasmBumpAllocator {
    buf: Vec<u8>,
    offset: usize,
}

impl WasmBumpAllocator {
    /// Create a new bump allocator with the given byte `capacity`.
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: vec![0u8; capacity],
            offset: 0,
        }
    }

    /// Allocate `size` bytes with the given power-of-two `align` (in bytes).
    ///
    /// Returns a mutable byte slice on success, or `None` when the arena is
    /// exhausted or `align` is zero.
    pub fn alloc(&mut self, size: usize, align: usize) -> Option<&mut [u8]> {
        if align == 0 || size == 0 {
            return None;
        }
        // Align the current offset up to the requested alignment.
        let aligned_offset = (self.offset + align - 1) & !(align - 1);
        let end = aligned_offset.checked_add(size)?;
        if end > self.buf.len() {
            return None;
        }
        self.offset = end;
        Some(&mut self.buf[aligned_offset..end])
    }

    /// Reset the bump pointer to zero, making the entire buffer available again.
    ///
    /// This does **not** zero the memory; existing bytes may be visible to
    /// subsequent allocations.
    pub fn reset(&mut self) {
        self.offset = 0;
    }

    /// Number of bytes consumed since the last [`reset`](Self::reset).
    pub fn used(&self) -> usize {
        self.offset
    }

    /// Number of bytes still available (before alignment padding).
    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.offset)
    }

    /// Total capacity of the arena in bytes.
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// Returns `true` when no bytes have been allocated since the last reset.
    pub fn is_empty(&self) -> bool {
        self.offset == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_new_capacity() {
        let a = WasmBumpAllocator::new(1024);
        assert_eq!(a.capacity(), 1024);
        assert_eq!(a.used(), 0);
        assert_eq!(a.remaining(), 1024);
        assert!(a.is_empty());
    }

    #[test]
    fn bump_alloc_basic() {
        let mut a = WasmBumpAllocator::new(256);
        let slice = a.alloc(64, 8).expect("allocation should succeed");
        assert_eq!(slice.len(), 64);
        assert_eq!(a.used(), 64);
        assert_eq!(a.remaining(), 192);
    }

    #[test]
    fn bump_alloc_exhausted() {
        let mut a = WasmBumpAllocator::new(8);
        assert!(a.alloc(16, 1).is_none());
    }

    #[test]
    fn bump_reset_reuse() {
        let mut a = WasmBumpAllocator::new(64);
        a.alloc(32, 4).expect("first alloc");
        assert_eq!(a.used(), 32);
        a.reset();
        assert_eq!(a.used(), 0);
        assert!(a.is_empty());
        a.alloc(64, 1).expect("second alloc after reset");
        assert_eq!(a.used(), 64);
    }

    #[test]
    fn bump_used_remaining_track() {
        let mut a = WasmBumpAllocator::new(100);
        a.alloc(10, 1).expect("alloc 10");
        assert_eq!(a.used(), 10);
        assert_eq!(a.remaining(), 90);
        a.alloc(40, 1).expect("alloc 40");
        assert_eq!(a.used(), 50);
        assert_eq!(a.remaining(), 50);
    }

    #[test]
    fn alloc_dealloc_roundtrip() {
        let ptr = oxigdal_alloc(128);
        assert!(!ptr.is_null());
        // Safety: ptr was allocated by oxigdal_alloc with size 128.
        unsafe { oxigdal_dealloc(ptr, 128) };
    }

    #[test]
    fn alloc_zero_returns_null() {
        let ptr = oxigdal_alloc(0);
        assert!(ptr.is_null());
    }
}
