//! Static memory pool implementations for no_std environments
//!
//! Provides memory pool abstractions that allow predictable allocation behavior
//! in embedded systems without heap allocation.

use core::cell::UnsafeCell;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::error::{EmbeddedError, Result};

/// Memory pool trait for unified pool interface
pub trait MemoryPool {
    /// Allocate memory from the pool
    ///
    /// # Errors
    ///
    /// Returns `PoolExhausted` if no memory is available
    /// Returns `InvalidAlignment` if alignment requirements cannot be met
    fn allocate(&self, size: usize, align: usize) -> Result<NonNull<u8>>;

    /// Deallocate memory back to the pool
    ///
    /// # Safety
    ///
    /// The pointer must have been allocated from this pool
    unsafe fn deallocate(&self, ptr: NonNull<u8>, size: usize, align: usize) -> Result<()>;

    /// Get the total capacity of the pool
    fn capacity(&self) -> usize;

    /// Get the currently used bytes
    fn used(&self) -> usize;

    /// Get the available bytes
    fn available(&self) -> usize {
        self.capacity().saturating_sub(self.used())
    }

    /// Reset the pool (deallocate all)
    ///
    /// # Safety
    ///
    /// All pointers allocated from this pool must not be used after reset
    unsafe fn reset(&self) -> Result<()>;
}

/// Static memory pool with compile-time size
///
/// Uses a simple bump allocator strategy suitable for embedded systems
pub struct StaticPool<const N: usize> {
    buffer: UnsafeCell<[u8; N]>,
    offset: AtomicUsize,
    locked: AtomicBool,
}

impl<const N: usize> StaticPool<N> {
    /// Create a new static pool
    pub const fn new() -> Self {
        Self {
            buffer: UnsafeCell::new([0u8; N]),
            offset: AtomicUsize::new(0),
            locked: AtomicBool::new(false),
        }
    }

    /// Get the base address of the pool
    fn base_addr(&self) -> usize {
        self.buffer.get() as usize
    }

    /// Try to acquire lock for allocation
    fn try_lock(&self) -> Result<()> {
        match self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        {
            Ok(_) => Ok(()),
            Err(_) => Err(EmbeddedError::ResourceBusy),
        }
    }

    /// Release the lock
    fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

impl<const N: usize> Default for StaticPool<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> MemoryPool for StaticPool<N> {
    fn allocate(&self, size: usize, align: usize) -> Result<NonNull<u8>> {
        if size == 0 {
            return Err(EmbeddedError::InvalidParameter);
        }

        if !align.is_power_of_two() {
            return Err(EmbeddedError::InvalidAlignment {
                required: align,
                actual: 0,
            });
        }

        self.try_lock()?;

        let current_offset = self.offset.load(Ordering::Relaxed);
        let base = self.base_addr();

        // Calculate aligned offset considering the base address
        // We need (base + aligned_offset) % align == 0
        // So aligned_offset = align_up(base + current_offset, align) - base
        let current_addr = base.wrapping_add(current_offset);
        let aligned_addr =
            (current_addr.wrapping_add(align.wrapping_sub(1))) & !align.wrapping_sub(1);
        let aligned_offset = aligned_addr.wrapping_sub(base);

        let new_offset = match aligned_offset.checked_add(size) {
            Some(offset) if offset <= N => offset,
            _ => {
                self.unlock();
                return Err(EmbeddedError::PoolExhausted);
            }
        };

        self.offset.store(new_offset, Ordering::Release);
        self.unlock();

        // SAFETY: We've verified the pointer is within our buffer and properly aligned
        let ptr = unsafe { NonNull::new_unchecked(aligned_addr as *mut u8) };
        Ok(ptr)
    }

    unsafe fn deallocate(&self, _ptr: NonNull<u8>, _size: usize, _align: usize) -> Result<()> {
        // Bump allocator doesn't support individual deallocation
        // Use reset() to reclaim all memory
        Ok(())
    }

    fn capacity(&self) -> usize {
        N
    }

    fn used(&self) -> usize {
        self.offset.load(Ordering::Relaxed)
    }

    unsafe fn reset(&self) -> Result<()> {
        self.try_lock()?;
        self.offset.store(0, Ordering::Release);
        self.unlock();
        Ok(())
    }
}

// SAFETY: StaticPool uses atomic operations for thread-safe access
unsafe impl<const N: usize> Sync for StaticPool<N> {}
unsafe impl<const N: usize> Send for StaticPool<N> {}

/// Block-based memory pool with fixed-size blocks
///
/// More efficient for frequent allocations/deallocations of similar sizes
pub struct BlockPool<const BLOCK_SIZE: usize, const NUM_BLOCKS: usize, const BITMAP_SIZE: usize> {
    blocks: UnsafeCell<[[u8; BLOCK_SIZE]; NUM_BLOCKS]>,
    free_bitmap: UnsafeCell<[u8; BITMAP_SIZE]>,
    free_count: AtomicUsize,
    locked: AtomicBool,
}

impl<const BLOCK_SIZE: usize, const NUM_BLOCKS: usize, const BITMAP_SIZE: usize>
    BlockPool<BLOCK_SIZE, NUM_BLOCKS, BITMAP_SIZE>
{
    /// Create a new block pool
    ///
    /// # Panics
    ///
    /// Panics if BITMAP_SIZE is not sufficient for NUM_BLOCKS
    pub const fn new() -> Self {
        // We need at least (NUM_BLOCKS + 7) / 8 bytes for the bitmap
        // This check will be evaluated at compile time
        assert!(
            BITMAP_SIZE * 8 >= NUM_BLOCKS,
            "BITMAP_SIZE is too small for NUM_BLOCKS"
        );

        Self {
            blocks: UnsafeCell::new([[0u8; BLOCK_SIZE]; NUM_BLOCKS]),
            free_bitmap: UnsafeCell::new([0xFF; BITMAP_SIZE]),
            free_count: AtomicUsize::new(NUM_BLOCKS),
            locked: AtomicBool::new(false),
        }
    }

    /// Try to acquire lock
    fn try_lock(&self) -> Result<()> {
        match self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        {
            Ok(_) => Ok(()),
            Err(_) => Err(EmbeddedError::ResourceBusy),
        }
    }

    /// Release the lock
    fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }

    /// Find and allocate a free block
    fn find_free_block(&self) -> Option<usize> {
        // SAFETY: We hold the lock
        let bitmap = unsafe { &mut *self.free_bitmap.get() };

        for (byte_idx, byte) in bitmap.iter_mut().enumerate() {
            if *byte == 0 {
                continue;
            }

            // Find first set bit
            for bit_idx in 0..8 {
                let block_idx = byte_idx * 8 + bit_idx;
                if block_idx >= NUM_BLOCKS {
                    return None;
                }

                if (*byte >> bit_idx) & 1 != 0 {
                    // Mark as allocated
                    *byte &= !(1 << bit_idx);
                    return Some(block_idx);
                }
            }
        }

        None
    }

    /// Mark a block as free
    ///
    /// # Safety
    ///
    /// block_idx must be valid and currently allocated
    unsafe fn free_block(&self, block_idx: usize) {
        // SAFETY: Caller guarantees block_idx is valid and we hold the lock
        let bitmap = unsafe { &mut *self.free_bitmap.get() };
        let byte_idx = block_idx / 8;
        let bit_idx = block_idx % 8;

        bitmap[byte_idx] |= 1 << bit_idx;
    }
}

impl<const BLOCK_SIZE: usize, const NUM_BLOCKS: usize, const BITMAP_SIZE: usize> Default
    for BlockPool<BLOCK_SIZE, NUM_BLOCKS, BITMAP_SIZE>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<const BLOCK_SIZE: usize, const NUM_BLOCKS: usize, const BITMAP_SIZE: usize> MemoryPool
    for BlockPool<BLOCK_SIZE, NUM_BLOCKS, BITMAP_SIZE>
{
    fn allocate(&self, size: usize, align: usize) -> Result<NonNull<u8>> {
        if size > BLOCK_SIZE {
            return Err(EmbeddedError::BufferTooSmall {
                required: size,
                available: BLOCK_SIZE,
            });
        }

        if !align.is_power_of_two() {
            return Err(EmbeddedError::InvalidAlignment {
                required: align,
                actual: 0,
            });
        }

        self.try_lock()?;

        let block_idx = match self.find_free_block() {
            Some(idx) => idx,
            None => {
                self.unlock();
                return Err(EmbeddedError::PoolExhausted);
            }
        };

        self.free_count.fetch_sub(1, Ordering::Relaxed);

        // SAFETY: We hold the lock and block_idx is valid
        let blocks = unsafe { &mut *self.blocks.get() };
        let ptr = blocks[block_idx].as_mut_ptr();

        // Verify alignment
        let ptr_addr = ptr as usize;
        if ptr_addr % align != 0 {
            // SAFETY: We just allocated this block
            unsafe { self.free_block(block_idx) };
            self.free_count.fetch_add(1, Ordering::Relaxed);
            self.unlock();
            return Err(EmbeddedError::InvalidAlignment {
                required: align,
                actual: ptr_addr % align.max(1),
            });
        }

        self.unlock();

        // SAFETY: ptr is non-null and valid
        let nonnull = unsafe { NonNull::new_unchecked(ptr) };
        Ok(nonnull)
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, _size: usize, _align: usize) -> Result<()> {
        self.try_lock()?;

        // Calculate block index from pointer
        // SAFETY: We hold the lock and the blocks array is valid
        let blocks = unsafe { &*self.blocks.get() };
        let base_addr = blocks.as_ptr() as usize;
        let ptr_addr = ptr.as_ptr() as usize;

        if ptr_addr < base_addr {
            self.unlock();
            return Err(EmbeddedError::InvalidParameter);
        }

        let offset = ptr_addr.wrapping_sub(base_addr);
        let block_idx = offset / BLOCK_SIZE;

        if block_idx >= NUM_BLOCKS {
            self.unlock();
            return Err(EmbeddedError::InvalidParameter);
        }

        // SAFETY: We've verified block_idx is valid and ptr was allocated from this pool
        unsafe { self.free_block(block_idx) };
        self.free_count.fetch_add(1, Ordering::Relaxed);

        self.unlock();
        Ok(())
    }

    fn capacity(&self) -> usize {
        BLOCK_SIZE * NUM_BLOCKS
    }

    fn used(&self) -> usize {
        let free = self.free_count.load(Ordering::Relaxed);
        (NUM_BLOCKS.saturating_sub(free)) * BLOCK_SIZE
    }

    unsafe fn reset(&self) -> Result<()> {
        self.try_lock()?;

        // SAFETY: We hold the lock and the bitmap is valid
        let bitmap = unsafe { &mut *self.free_bitmap.get() };
        bitmap.fill(0xFF);

        self.free_count.store(NUM_BLOCKS, Ordering::Release);
        self.unlock();
        Ok(())
    }
}

// SAFETY: BlockPool uses atomic operations and locking for thread-safe access
unsafe impl<const BLOCK_SIZE: usize, const NUM_BLOCKS: usize, const BITMAP_SIZE: usize> Sync
    for BlockPool<BLOCK_SIZE, NUM_BLOCKS, BITMAP_SIZE>
{
}
unsafe impl<const BLOCK_SIZE: usize, const NUM_BLOCKS: usize, const BITMAP_SIZE: usize> Send
    for BlockPool<BLOCK_SIZE, NUM_BLOCKS, BITMAP_SIZE>
{
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_static_pool_allocation() {
        let pool = StaticPool::<1024>::new();

        let ptr1 = pool.allocate(64, 8).expect("allocation failed");
        assert_eq!(pool.used(), 64);

        let ptr2 = pool.allocate(128, 16).expect("allocation failed");
        assert!(pool.used() >= 64 + 128);

        assert_ne!(ptr1, ptr2);
    }

    #[test]
    fn test_static_pool_exhaustion() {
        let pool = StaticPool::<128>::new();

        let _ptr1 = pool.allocate(64, 8).expect("allocation failed");
        let _ptr2 = pool.allocate(64, 8).expect("allocation failed");

        // Pool should be exhausted now
        let result = pool.allocate(64, 8);
        assert!(matches!(result, Err(EmbeddedError::PoolExhausted)));
    }

    #[test]
    fn test_static_pool_reset() {
        let pool = StaticPool::<1024>::new();

        let _ptr = pool.allocate(512, 8).expect("allocation failed");
        assert!(pool.used() > 0);

        // SAFETY: We won't use the pointer after reset
        unsafe { pool.reset().expect("reset failed") };
        assert_eq!(pool.used(), 0);
    }

    #[test]
    fn test_block_pool_allocation() {
        // BITMAP_SIZE = (NUM_BLOCKS + 7) / 8 = (16 + 7) / 8 = 2
        let pool = BlockPool::<64, 16, 2>::new();

        let ptr1 = pool.allocate(32, 4).expect("allocation failed");
        let ptr2 = pool.allocate(32, 4).expect("allocation failed");

        assert_ne!(ptr1, ptr2);
        assert_eq!(pool.used(), 128); // 2 blocks * 64 bytes
    }

    #[test]
    fn test_block_pool_deallocation() {
        // BITMAP_SIZE = (NUM_BLOCKS + 7) / 8 = (16 + 7) / 8 = 2
        let pool = BlockPool::<64, 16, 2>::new();

        let ptr = pool.allocate(32, 4).expect("allocation failed");
        assert_eq!(pool.used(), 64);

        // SAFETY: ptr was allocated from this pool
        unsafe { pool.deallocate(ptr, 32, 4).expect("deallocation failed") };
        assert_eq!(pool.used(), 0);
    }

    #[test]
    fn test_block_pool_exhaustion() {
        // BITMAP_SIZE = (NUM_BLOCKS + 7) / 8 = (4 + 7) / 8 = 1
        let pool = BlockPool::<64, 4, 1>::new();

        for _ in 0..4 {
            pool.allocate(32, 4).expect("allocation failed");
        }

        let result = pool.allocate(32, 4);
        assert!(matches!(result, Err(EmbeddedError::PoolExhausted)));
    }
}
