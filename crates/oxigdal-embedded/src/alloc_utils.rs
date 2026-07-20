//! Custom allocator implementations for no_std environments
//!
//! Provides allocator wrappers compatible with Rust's allocator API

#[cfg(feature = "alloc")]
use alloc::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;

use crate::error::{EmbeddedError, Result};
use crate::memory_pool::MemoryPool;

/// Bump allocator for sequential allocations
///
/// Simple and fast allocator that only supports allocation, not deallocation
/// of individual items. Perfect for temporary buffers and stack-like usage.
pub struct BumpAllocator<P: MemoryPool> {
    pool: P,
}

impl<P: MemoryPool> BumpAllocator<P> {
    /// Create a new bump allocator with the given pool
    pub const fn new(pool: P) -> Self {
        Self { pool }
    }

    /// Allocate memory from the bump allocator
    ///
    /// # Errors
    ///
    /// Returns error if the pool is exhausted or alignment requirements cannot be met
    pub fn allocate(&self, size: usize, align: usize) -> Result<NonNull<u8>> {
        self.pool.allocate(size, align)
    }

    /// Get the total capacity
    pub fn capacity(&self) -> usize {
        self.pool.capacity()
    }

    /// Get currently used bytes
    pub fn used(&self) -> usize {
        self.pool.used()
    }

    /// Get available bytes
    pub fn available(&self) -> usize {
        self.pool.available()
    }

    /// Reset the allocator (reclaim all memory)
    ///
    /// # Safety
    ///
    /// All pointers allocated from this allocator must not be used after reset
    pub unsafe fn reset(&self) -> Result<()> {
        // SAFETY: Caller guarantees all allocated pointers will not be used after reset
        unsafe { self.pool.reset() }
    }
}

#[cfg(feature = "alloc")]
unsafe impl<P: MemoryPool + Sync> GlobalAlloc for BumpAllocator<P> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match self.pool.allocate(layout.size(), layout.align()) {
            Ok(ptr) => ptr.as_ptr(),
            Err(_) => core::ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // Bump allocator doesn't support individual deallocation
        let _ = ptr;
        let _ = layout;
    }
}

/// Stack-based allocator for fixed-size allocations
///
/// Maintains a stack of allocations and only allows deallocation in LIFO order
pub struct StackAllocator<const N: usize> {
    buffer: [u8; N],
    offset: core::cell::Cell<usize>,
}

impl<const N: usize> StackAllocator<N> {
    /// Create a new stack allocator
    pub const fn new() -> Self {
        Self {
            buffer: [0u8; N],
            offset: core::cell::Cell::new(0),
        }
    }

    /// Allocate from the stack
    ///
    /// # Errors
    ///
    /// Returns error if insufficient space or invalid alignment
    pub fn allocate(&self, size: usize, align: usize) -> Result<NonNull<u8>> {
        if size == 0 {
            return Err(EmbeddedError::InvalidParameter);
        }

        if !align.is_power_of_two() {
            return Err(EmbeddedError::InvalidAlignment {
                required: align,
                actual: 0,
            });
        }

        let current_offset = self.offset.get();
        let base_addr = self.buffer.as_ptr() as usize;

        // Align the real base address, not the bare offset. The returned pointer
        // is only a multiple of `align` when `base_addr % align == 0`, which is
        // not guaranteed for `[u8; N]` (alignment 1). Compute the aligned address
        // from `base_addr + current_offset`, then derive the offset back out.
        let current_addr = base_addr.wrapping_add(current_offset);
        let aligned_addr = current_addr.wrapping_add(align - 1) & !(align - 1);
        let aligned_offset = aligned_addr.wrapping_sub(base_addr);

        let new_offset = match aligned_offset.checked_add(size) {
            Some(offset) if offset <= N => offset,
            _ => {
                return Err(EmbeddedError::BufferTooSmall {
                    required: size,
                    available: N.saturating_sub(current_offset),
                });
            }
        };

        self.offset.set(new_offset);

        // SAFETY: `aligned_addr` is within `[base_addr, base_addr + N)` (verified
        // by the bounds check above) and is a non-null, properly aligned pointer.
        let ptr = unsafe { NonNull::new_unchecked(aligned_addr as *mut u8) };
        Ok(ptr)
    }

    /// Pop the last allocation
    ///
    /// # Safety
    ///
    /// Must be called in LIFO order matching allocations
    pub unsafe fn pop(&self, size: usize) -> Result<()> {
        let current_offset = self.offset.get();
        if size > current_offset {
            return Err(EmbeddedError::InvalidParameter);
        }

        self.offset.set(current_offset - size);
        Ok(())
    }

    /// Get current offset
    pub fn used(&self) -> usize {
        self.offset.get()
    }

    /// Get remaining capacity
    pub fn available(&self) -> usize {
        N.saturating_sub(self.offset.get())
    }

    /// Reset the allocator
    pub fn reset(&self) {
        self.offset.set(0);
    }
}

impl<const N: usize> Default for StackAllocator<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Arena allocator for temporary allocations
///
/// Fast allocator for temporary objects that will all be freed together
pub struct Arena<const N: usize> {
    buffer: core::cell::UnsafeCell<[u8; N]>,
    offset: core::cell::Cell<usize>,
}

impl<const N: usize> Arena<N> {
    /// Create a new arena
    pub const fn new() -> Self {
        Self {
            buffer: core::cell::UnsafeCell::new([0u8; N]),
            offset: core::cell::Cell::new(0),
        }
    }

    /// Allocate from the arena
    ///
    /// # Errors
    ///
    /// Returns error if insufficient space
    pub fn allocate(&self, size: usize, align: usize) -> Result<NonNull<u8>> {
        if size == 0 {
            return Err(EmbeddedError::InvalidParameter);
        }

        if !align.is_power_of_two() {
            return Err(EmbeddedError::InvalidAlignment {
                required: align,
                actual: 0,
            });
        }

        let current_offset = self.offset.get();
        let base_ptr = self.buffer.get() as *mut u8;
        let base_addr = base_ptr as usize;

        // Align the real base address, not the bare offset (see StackAllocator).
        let current_addr = base_addr.wrapping_add(current_offset);
        let aligned_addr = current_addr.wrapping_add(align - 1) & !(align - 1);
        let aligned_offset = aligned_addr.wrapping_sub(base_addr);

        let new_offset = match aligned_offset.checked_add(size) {
            Some(offset) if offset <= N => offset,
            _ => {
                return Err(EmbeddedError::BufferTooSmall {
                    required: size,
                    available: N.saturating_sub(current_offset),
                });
            }
        };

        self.offset.set(new_offset);

        // SAFETY: We own the buffer and `aligned_offset` is within bounds, so
        // `base_ptr + aligned_offset == aligned_addr` points into the buffer.
        let ptr = unsafe { base_ptr.add(aligned_offset) };
        let nonnull = NonNull::new(ptr).ok_or(EmbeddedError::AllocationFailed)?;
        Ok(nonnull)
    }

    /// Allocate a typed value
    ///
    /// Returns a `NonNull<T>` pointer to properly aligned, uninitialized memory.
    /// The caller is responsible for initializing the memory before use.
    ///
    /// # Errors
    ///
    /// Returns error if insufficient space
    pub fn allocate_typed<T>(&self) -> Result<NonNull<T>> {
        let ptr = self.allocate(core::mem::size_of::<T>(), core::mem::align_of::<T>())?;
        Ok(ptr.cast::<T>())
    }

    /// Clear the arena (reclaim all memory)
    pub fn clear(&self) {
        self.offset.set(0);
    }

    /// Get used bytes
    pub fn used(&self) -> usize {
        self.offset.get()
    }

    /// Get available bytes
    pub fn available(&self) -> usize {
        N.saturating_sub(self.offset.get())
    }
}

impl<const N: usize> Default for Arena<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_pool::StaticPool;

    #[test]
    fn test_bump_allocator() {
        let pool = StaticPool::<1024>::new();
        let allocator = BumpAllocator::new(pool);

        let ptr1 = allocator.allocate(64, 8).expect("allocation failed");
        let ptr2 = allocator.allocate(128, 16).expect("allocation failed");

        assert_ne!(ptr1, ptr2);
        assert!(allocator.used() > 0);
    }

    #[test]
    fn test_stack_allocator() {
        // Use an alignment (8) that is <= the allocator's struct alignment so no
        // padding is inserted regardless of the buffer's base address; this
        // keeps `pop(size)` exact for the LIFO-mechanics check. Over-alignment
        // correctness (which may introduce base-dependent padding) is covered by
        // `test_stack_allocator_over_alignment`.
        let allocator = StackAllocator::<1024>::new();

        let _ptr1 = allocator.allocate(64, 8).expect("allocation failed");
        assert_eq!(allocator.used(), 64);

        let _ptr2 = allocator.allocate(128, 8).expect("allocation failed");
        assert_eq!(allocator.used(), 64 + 128);

        // SAFETY: We're popping in LIFO order and no padding was inserted.
        unsafe {
            allocator.pop(128).expect("pop failed");
        }
        assert_eq!(allocator.used(), 64);
    }

    #[test]
    fn test_arena_allocator() {
        let arena = Arena::<1024>::new();

        let _ptr1 = arena.allocate(64, 8).expect("allocation failed");
        let _ptr2 = arena.allocate(128, 16).expect("allocation failed");

        assert!(arena.used() > 0);

        arena.clear();
        assert_eq!(arena.used(), 0);
    }

    #[test]
    fn test_stack_allocator_over_alignment() {
        // Regression: aligning the bare offset (not the real base address) yields
        // pointers that are only aligned when the buffer base happens to be
        // aligned. Exercise alignments larger than the struct's incidental
        // alignment to catch the misalignment bug.
        let allocator = StackAllocator::<4096>::new();
        for &align in &[16usize, 32, 64, 128] {
            let ptr = allocator
                .allocate(8, align)
                .expect("allocation should succeed");
            assert_eq!(
                ptr.as_ptr() as usize % align,
                0,
                "StackAllocator returned pointer misaligned for align={align}"
            );
        }
    }

    #[test]
    fn test_arena_over_alignment() {
        let arena = Arena::<4096>::new();
        for &align in &[16usize, 32, 64, 128] {
            let ptr = arena.allocate(8, align).expect("allocation should succeed");
            assert_eq!(
                ptr.as_ptr() as usize % align,
                0,
                "Arena returned pointer misaligned for align={align}"
            );
        }
    }

    #[test]
    fn test_arena_typed_allocation() {
        let arena = Arena::<1024>::new();

        let mut ptr: NonNull<u64> = arena.allocate_typed().expect("allocation failed");
        // SAFETY: We just allocated this memory and have exclusive access
        let value = unsafe { ptr.as_mut() };
        *value = 42;
        assert_eq!(*value, 42);
    }
}
