//! Arena Allocators for Batch Operations
//!
//! This module provides arena allocators optimized for temporary batch operations:
//! - Bump allocator for fast sequential allocation
//! - Arena pooling and reuse
//! - Automatic arena reset
//! - Per-thread arenas
//! - Arena statistics

// Arena allocators intentionally return mutable refs from shared self
#![allow(clippy::mut_from_ref)]
// Default impl uses expect() for configuration errors - acceptable here
#![allow(clippy::expect_used)]
// Unsafe code is necessary for arena allocators
#![allow(unsafe_code)]

use crate::error::{OxiGdalError, Result};
use parking_lot::Mutex;
use std::alloc::{Layout, alloc, dealloc};
use std::cell::RefCell;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Default arena size (1MB)
pub const DEFAULT_ARENA_SIZE: usize = 1024 * 1024;

/// Arena statistics
#[derive(Debug, Default)]
pub struct ArenaStats {
    /// Total allocations
    pub total_allocations: AtomicU64,
    /// Total bytes allocated
    pub bytes_allocated: AtomicUsize,
    /// Number of arena resets
    pub resets: AtomicU64,
    /// Peak memory usage
    pub peak_usage: AtomicUsize,
}

impl ArenaStats {
    /// Create new statistics
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an allocation
    pub fn record_allocation(&self, size: usize) {
        self.total_allocations.fetch_add(1, Ordering::Relaxed);
        let prev = self.bytes_allocated.fetch_add(size, Ordering::Relaxed);
        let new_allocated = prev.saturating_add(size);

        // Update peak
        let mut peak = self.peak_usage.load(Ordering::Relaxed);
        while new_allocated > peak {
            match self.peak_usage.compare_exchange_weak(
                peak,
                new_allocated,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => peak = x,
            }
        }
    }

    /// Record a reset
    pub fn record_reset(&self, bytes_freed: usize) {
        self.resets.fetch_add(1, Ordering::Relaxed);
        self.bytes_allocated
            .fetch_sub(bytes_freed, Ordering::Relaxed);
    }

    /// Get allocation rate
    pub fn allocation_rate(&self) -> f64 {
        let total = self.total_allocations.load(Ordering::Relaxed);
        let peak = self.peak_usage.load(Ordering::Relaxed);
        if peak == 0 {
            0.0
        } else {
            total as f64 / peak as f64
        }
    }
}

/// Bump allocator arena
pub struct Arena {
    /// Base pointer
    base: NonNull<u8>,
    /// Current offset
    offset: AtomicUsize,
    /// Total capacity
    capacity: usize,
    /// Statistics
    stats: Arc<ArenaStats>,
    /// Alignment
    alignment: usize,
}

impl Arena {
    /// Create a new arena with default size
    pub fn new() -> Result<Self> {
        Self::with_capacity(DEFAULT_ARENA_SIZE)
    }

    /// Create a new arena with specified capacity
    pub fn with_capacity(capacity: usize) -> Result<Self> {
        Self::with_capacity_and_alignment(capacity, 16)
    }

    /// Create a new arena with specified capacity and alignment
    pub fn with_capacity_and_alignment(capacity: usize, alignment: usize) -> Result<Self> {
        if capacity == 0 {
            return Err(OxiGdalError::invalid_parameter(
                "parameter",
                "Arena capacity must be non-zero".to_string(),
            ));
        }

        if !alignment.is_power_of_two() {
            return Err(OxiGdalError::invalid_parameter(
                "parameter",
                "Alignment must be a power of 2".to_string(),
            ));
        }

        let layout = Layout::from_size_align(capacity, alignment)
            .map_err(|e| OxiGdalError::allocation_error(e.to_string()))?;

        // SAFETY: Layout is valid and we check for null before creating NonNull.
        // The pointer is properly aligned according to the layout.
        let base = unsafe {
            let ptr = alloc(layout);
            if ptr.is_null() {
                return Err(OxiGdalError::allocation_error(
                    "Failed to allocate arena".to_string(),
                ));
            }
            NonNull::new_unchecked(ptr)
        };

        Ok(Self {
            base,
            offset: AtomicUsize::new(0),
            capacity,
            stats: Arc::new(ArenaStats::new()),
            alignment,
        })
    }

    /// Allocate memory from the arena
    pub fn allocate(&self, size: usize) -> Result<NonNull<u8>> {
        self.allocate_aligned(size, self.alignment)
    }

    /// Allocate aligned memory from the arena
    pub fn allocate_aligned(&self, size: usize, alignment: usize) -> Result<NonNull<u8>> {
        if size == 0 {
            return Err(OxiGdalError::invalid_parameter(
                "parameter",
                "Allocation size must be non-zero".to_string(),
            ));
        }

        // Align the current offset
        let mut current = self.offset.load(Ordering::Relaxed);
        loop {
            let aligned = (current + alignment - 1) & !(alignment - 1);
            let new_offset = aligned + size;

            if new_offset > self.capacity {
                return Err(OxiGdalError::allocation_error(format!(
                    "Arena exhausted: requested {}, available {}",
                    size,
                    self.capacity - current
                )));
            }

            match self.offset.compare_exchange_weak(
                current,
                new_offset,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.stats.record_allocation(size);
                    // SAFETY: Pointer arithmetic is within allocated bounds.
                    // aligned is computed to be within capacity.
                    let ptr = unsafe { NonNull::new_unchecked(self.base.as_ptr().add(aligned)) };
                    return Ok(ptr);
                }
                Err(x) => current = x,
            }
        }
    }

    /// Reset the arena (invalidates all previous allocations)
    pub fn reset(&self) {
        let freed = self.offset.swap(0, Ordering::Relaxed);
        self.stats.record_reset(freed);
    }

    /// Get current usage
    pub fn usage(&self) -> usize {
        self.offset.load(Ordering::Relaxed)
    }

    /// Get capacity
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Get available space
    pub fn available(&self) -> usize {
        self.capacity.saturating_sub(self.usage())
    }

    /// Check if arena is exhausted
    pub fn is_exhausted(&self) -> bool {
        self.available() == 0
    }

    /// Get statistics
    pub fn stats(&self) -> Arc<ArenaStats> {
        Arc::clone(&self.stats)
    }

    /// Allocate a slice of values
    pub fn allocate_slice<T>(&self, count: usize) -> Result<&mut [T]> {
        let size = count * std::mem::size_of::<T>();
        let alignment = std::mem::align_of::<T>();
        let ptr = self.allocate_aligned(size, alignment)?;

        // SAFETY: ptr is valid and properly aligned. count has been validated
        // to fit within the allocated arena space.
        Ok(unsafe { std::slice::from_raw_parts_mut(ptr.as_ptr().cast::<T>(), count) })
    }

    /// Allocate and initialize a value
    pub fn allocate_value<T>(&self, value: T) -> Result<&mut T> {
        let size = std::mem::size_of::<T>();
        let alignment = std::mem::align_of::<T>();
        let ptr = self.allocate_aligned(size, alignment)?;

        // SAFETY: ptr is valid, aligned, and we have exclusive access.
        // T's Drop implementation will be called automatically.
        unsafe {
            let typed_ptr = ptr.as_ptr().cast::<T>();
            std::ptr::write(typed_ptr, value);
            Ok(&mut *typed_ptr)
        }
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        // SAFETY: Layout matches the one used during allocation.
        // We have exclusive ownership and this is the last use of the pointer.
        unsafe {
            let layout = Layout::from_size_align_unchecked(self.capacity, self.alignment);
            dealloc(self.base.as_ptr(), layout);
        }
    }
}

// SAFETY: Arena can be sent between threads because:
// - base pointer is owned and uniquely accessed
// - AtomicUsize provides thread-safe offset management
unsafe impl Send for Arena {}
// SAFETY: Arena can be shared between threads because:
// - All operations use atomic operations for synchronization
// - Memory allocation is internally synchronized
unsafe impl Sync for Arena {}

/// Arena pool for reusing arenas
pub struct ArenaPool {
    /// Available arenas
    available: Mutex<Vec<Arena>>,
    /// Arena capacity
    capacity: usize,
    /// Maximum pool size
    max_pool_size: usize,
    /// Statistics
    stats: Arc<ArenaStats>,
}

impl ArenaPool {
    /// Create a new arena pool
    #[must_use]
    pub fn new(capacity: usize, max_pool_size: usize) -> Self {
        Self {
            available: Mutex::new(Vec::new()),
            capacity,
            max_pool_size,
            stats: Arc::new(ArenaStats::new()),
        }
    }

    /// Create with default settings
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(DEFAULT_ARENA_SIZE, 16)
    }

    /// Acquire an arena from the pool
    pub fn acquire(&self) -> Result<Arena> {
        let mut available = self.available.lock();

        if let Some(arena) = available.pop() {
            arena.reset();
            Ok(arena)
        } else {
            drop(available);
            Arena::with_capacity(self.capacity)
        }
    }

    /// Return an arena to the pool
    pub fn release(&self, arena: Arena) {
        let mut available = self.available.lock();

        if available.len() < self.max_pool_size {
            available.push(arena);
        }
        // Otherwise, arena is dropped
    }

    /// Get pool statistics
    pub fn stats(&self) -> Arc<ArenaStats> {
        Arc::clone(&self.stats)
    }

    /// Get current pool size
    pub fn pool_size(&self) -> usize {
        self.available.lock().len()
    }

    /// Clear the pool
    pub fn clear(&self) {
        self.available.lock().clear();
    }
}

impl Default for ArenaPool {
    fn default() -> Self {
        Self::with_defaults()
    }
}

// Thread-local arena
thread_local! {
    static THREAD_ARENA: RefCell<Option<Arena>> = const { RefCell::new(None) };
}

/// Get or create a thread-local arena
pub fn get_thread_arena() -> Result<Arena> {
    THREAD_ARENA.with(|arena| {
        let mut arena_ref = arena.borrow_mut();
        if arena_ref.is_none() {
            *arena_ref = Some(Arena::new()?);
        }
        Arena::new()
    })
}

/// Reset thread-local arena
pub fn reset_thread_arena() -> Result<()> {
    THREAD_ARENA.with(|arena| {
        if let Some(arena) = arena.borrow().as_ref() {
            arena.reset();
        }
        Ok(())
    })
}

/// RAII guard for automatic arena reset
pub struct ArenaGuard<'a> {
    arena: &'a Arena,
    saved_offset: usize,
}

impl<'a> ArenaGuard<'a> {
    /// Create a new arena guard
    pub fn new(arena: &'a Arena) -> Self {
        let saved_offset = arena.offset.load(Ordering::Relaxed);
        Self {
            arena,
            saved_offset,
        }
    }

    /// Get the arena
    #[must_use]
    pub fn arena(&self) -> &Arena {
        self.arena
    }
}

impl Drop for ArenaGuard<'_> {
    fn drop(&mut self) {
        // Restore the arena to the state it was in when the guard was created
        self.arena
            .offset
            .store(self.saved_offset, Ordering::Relaxed);
    }
}

#[cfg(test)]
#[allow(useless_ptr_null_checks)]
mod tests {
    use super::*;

    #[test]
    fn test_arena_basic() {
        // Use alignment of 1 to avoid padding in this test
        let arena = Arena::with_capacity_and_alignment(1024, 1)
            .expect("Test setup failed: arena creation with custom alignment");

        let ptr1 = arena
            .allocate(100)
            .expect("Arena allocation should succeed in test");
        let ptr2 = arena
            .allocate(200)
            .expect("Arena allocation should succeed in test");

        assert!(!ptr1.as_ptr().is_null());
        assert!(!ptr2.as_ptr().is_null());
        assert_ne!(ptr1, ptr2);

        assert_eq!(arena.usage(), 300);
        assert_eq!(arena.available(), 724);
    }

    #[test]
    fn test_arena_reset() {
        let arena = Arena::with_capacity(1024).expect("Test setup failed: arena creation");

        arena
            .allocate(500)
            .expect("Arena allocation should succeed in test");
        assert_eq!(arena.usage(), 500);

        arena.reset();
        assert_eq!(arena.usage(), 0);

        arena
            .allocate(300)
            .expect("Arena allocation should succeed after reset");
        assert_eq!(arena.usage(), 300);
    }

    #[test]
    fn test_arena_exhaustion() {
        let arena = Arena::with_capacity(100).expect("Test setup failed: arena creation");

        arena.allocate(50).expect("First allocation should succeed");
        arena
            .allocate(30)
            .expect("Second allocation should succeed");

        let result = arena.allocate(50);
        assert!(result.is_err());
    }

    #[test]
    fn test_arena_pool() {
        let pool = ArenaPool::new(1024, 4);

        let arena1 = pool.acquire().expect("Pool should acquire first arena");
        let arena2 = pool.acquire().expect("Pool should acquire second arena");

        assert_eq!(pool.pool_size(), 0);

        pool.release(arena1);
        pool.release(arena2);

        assert_eq!(pool.pool_size(), 2);
    }

    #[test]
    fn test_arena_slice() {
        let arena = Arena::with_capacity(1024).expect("Test setup failed: arena creation");

        let slice: &mut [u32] = arena
            .allocate_slice(10)
            .expect("Arena slice allocation should succeed");
        assert_eq!(slice.len(), 10);

        slice[0] = 42;
        assert_eq!(slice[0], 42);
    }

    #[test]
    fn test_arena_value() {
        let arena = Arena::with_capacity(1024).expect("Test setup failed: arena creation");

        let value = arena
            .allocate_value(42u32)
            .expect("Arena value allocation should succeed");
        assert_eq!(*value, 42);

        *value = 100;
        assert_eq!(*value, 100);
    }

    #[test]
    fn test_arena_guard() {
        // Use alignment of 1 to avoid padding in this test
        let arena = Arena::with_capacity_and_alignment(1024, 1)
            .expect("Test setup failed: arena creation with custom alignment");
        arena
            .allocate(100)
            .expect("Initial allocation should succeed");
        assert_eq!(arena.usage(), 100);

        {
            let _guard = ArenaGuard::new(&arena);
            arena
                .allocate(200)
                .expect("Allocation within guard should succeed");
            assert_eq!(arena.usage(), 300);
        }

        // Guard dropped, arena reset to state before guard
        assert_eq!(arena.usage(), 100);
    }
}
