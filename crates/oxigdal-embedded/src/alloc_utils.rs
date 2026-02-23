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
        let aligned_offset = (current_offset + align - 1) & !(align - 1);

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

        let ptr_addr = base_addr.wrapping_add(aligned_offset);
        // SAFETY: We've verified the pointer is within bounds
        let ptr = unsafe { NonNull::new_unchecked(ptr_addr as *mut u8) };
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
        let aligned_offset = (current_offset + align - 1) & !(align - 1);

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

        // SAFETY: We own the buffer and offset is within bounds
        let base_ptr = self.buffer.get() as *mut u8;
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
        let allocator = StackAllocator::<1024>::new();

        let _ptr1 = allocator.allocate(64, 8).expect("allocation failed");
        assert_eq!(allocator.used(), 64);

        let _ptr2 = allocator.allocate(128, 16).expect("allocation failed");
        assert!(allocator.used() >= 64 + 128);

        // SAFETY: We're popping in LIFO order
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
    fn test_arena_typed_allocation() {
        let arena = Arena::<1024>::new();

        let mut ptr: NonNull<u64> = arena.allocate_typed().expect("allocation failed");
        // SAFETY: We just allocated this memory and have exclusive access
        let value = unsafe { ptr.as_mut() };
        *value = 42;
        assert_eq!(*value, 42);
    }
}
