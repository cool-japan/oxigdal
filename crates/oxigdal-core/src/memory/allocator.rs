//! Custom Memory Allocators for Geospatial Data
//!
//! This module provides specialized allocators optimized for geospatial workloads:
//! - Slab allocator for fixed-size blocks (tiles)
//! - Buddy allocator for variable-size blocks
//! - Thread-local allocation pools
//! - Allocation tracking and statistics
//! - Memory leak detection in debug mode

// Unsafe code is necessary for custom allocators
#![allow(unsafe_code)]
// Default impls use expect() for configuration errors - acceptable here
#![allow(clippy::expect_used)]
// Arc with custom allocator types that have unsafe Send+Sync impls
#![allow(clippy::arc_with_non_send_sync)]

use crate::error::{OxiGdalError, Result};
use parking_lot::{Mutex, RwLock};
use std::alloc::{Layout, alloc, dealloc};
use std::collections::{BTreeMap, HashMap};
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Minimum alignment for all allocations
pub const MIN_ALIGNMENT: usize = 16;

/// Maximum block size for buddy allocator (16MB)
pub const MAX_BLOCK_SIZE: usize = 16 * 1024 * 1024;

/// Slab sizes for common geospatial tile dimensions
pub const SLAB_SIZES: &[usize] = &[
    256,       // 16x16 bytes
    1024,      // 32x32 bytes
    4096,      // 64x64 bytes or 4KB page
    16384,     // 128x128 bytes
    65536,     // 256x256 bytes or 64KB
    262_144,   // 512x512 bytes or 256KB
    1_048_576, // 1024x1024 bytes or 1MB
    4_194_304, // 2048x2048 bytes or 4MB
];

/// Statistics for allocator performance tracking
#[derive(Debug, Default)]
pub struct AllocatorStats {
    /// Total number of allocations
    pub total_allocations: AtomicU64,
    /// Total number of deallocations
    pub total_deallocations: AtomicU64,
    /// Current bytes allocated
    pub bytes_allocated: AtomicUsize,
    /// Peak bytes allocated
    pub peak_bytes_allocated: AtomicUsize,
    /// Number of allocation failures
    pub allocation_failures: AtomicU64,
    /// Number of slab hits
    pub slab_hits: AtomicU64,
    /// Number of slab misses
    pub slab_misses: AtomicU64,
}

impl AllocatorStats {
    /// Create new statistics
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an allocation
    pub fn record_allocation(&self, size: usize) {
        self.total_allocations.fetch_add(1, Ordering::Relaxed);
        let new_allocated = self.bytes_allocated.fetch_add(size, Ordering::Relaxed) + size;

        // Update peak if necessary
        let mut peak = self.peak_bytes_allocated.load(Ordering::Relaxed);
        while new_allocated > peak {
            match self.peak_bytes_allocated.compare_exchange_weak(
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

    /// Record a deallocation
    pub fn record_deallocation(&self, size: usize) {
        self.total_deallocations.fetch_add(1, Ordering::Relaxed);
        self.bytes_allocated.fetch_sub(size, Ordering::Relaxed);
    }

    /// Record an allocation failure
    pub fn record_failure(&self) {
        self.allocation_failures.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a slab hit
    pub fn record_slab_hit(&self) {
        self.slab_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a slab miss
    pub fn record_slab_miss(&self) {
        self.slab_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Get current allocation count
    pub fn current_allocations(&self) -> u64 {
        self.total_allocations.load(Ordering::Relaxed)
            - self.total_deallocations.load(Ordering::Relaxed)
    }

    /// Get slab hit rate
    pub fn slab_hit_rate(&self) -> f64 {
        let hits = self.slab_hits.load(Ordering::Relaxed);
        let misses = self.slab_misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }
}

/// A block in the slab allocator
#[allow(dead_code)]
struct SlabBlock {
    ptr: NonNull<u8>,
    size: usize,
}

impl Drop for SlabBlock {
    fn drop(&mut self) {
        // SAFETY: The layout matches the one used during allocation.
        // The pointer is valid and aligned, and we have exclusive ownership.
        unsafe {
            let layout = Layout::from_size_align_unchecked(self.size, MIN_ALIGNMENT);
            dealloc(self.ptr.as_ptr(), layout);
        }
    }
}

/// Slab allocator for fixed-size blocks
pub struct SlabAllocator {
    /// Available blocks for each size class
    free_lists: Arc<RwLock<HashMap<usize, Vec<NonNull<u8>>>>>,
    /// Statistics
    stats: Arc<AllocatorStats>,
    /// Block tracking for leak detection in debug mode
    #[cfg(debug_assertions)]
    allocated_blocks: Arc<Mutex<HashMap<usize, Vec<NonNull<u8>>>>>,
}

// SAFETY: SlabAllocator is Send because all NonNull pointers are protected
// by Arc<RwLock> or Arc<Mutex>, providing proper synchronization
unsafe impl Send for SlabAllocator {}

// SAFETY: SlabAllocator is Sync because all NonNull pointers are protected
// by Arc<RwLock> or Arc<Mutex>, providing proper synchronization
unsafe impl Sync for SlabAllocator {}

impl SlabAllocator {
    /// Create a new slab allocator
    #[must_use]
    pub fn new() -> Self {
        let mut free_lists = HashMap::new();
        for &size in SLAB_SIZES {
            free_lists.insert(size, Vec::new());
        }

        Self {
            free_lists: Arc::new(RwLock::new(free_lists)),
            stats: Arc::new(AllocatorStats::new()),
            #[cfg(debug_assertions)]
            allocated_blocks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Allocate a block of the given size
    pub fn allocate(&self, size: usize) -> Result<NonNull<u8>> {
        // Find the appropriate slab size
        let slab_size = SLAB_SIZES
            .iter()
            .find(|&&s| s >= size)
            .copied()
            .ok_or_else(|| {
                self.stats.record_slab_miss();
                OxiGdalError::invalid_parameter(
                    "parameter",
                    format!(
                        "Size {} exceeds maximum slab size {}",
                        size,
                        SLAB_SIZES.last().copied().unwrap_or(0)
                    ),
                )
            })?;

        // Try to get a block from the free list
        {
            let mut free_lists = self.free_lists.write();
            if let Some(blocks) = free_lists.get_mut(&slab_size) {
                if let Some(ptr) = blocks.pop() {
                    self.stats.record_slab_hit();
                    self.stats.record_allocation(slab_size);

                    #[cfg(debug_assertions)]
                    {
                        let mut allocated = self.allocated_blocks.lock();
                        allocated.entry(slab_size).or_default().push(ptr);
                    }

                    return Ok(ptr);
                }
            }
        }

        // No free block available, allocate a new one
        self.stats.record_slab_miss();
        let layout = Layout::from_size_align(slab_size, MIN_ALIGNMENT)
            .map_err(|e| OxiGdalError::allocation_error(e.to_string()))?;

        // SAFETY: We've validated the layout and check for null after allocation.
        // NonNull::new_unchecked is safe because we've verified the pointer is non-null.
        let ptr = unsafe {
            let raw_ptr = alloc(layout);
            if raw_ptr.is_null() {
                self.stats.record_failure();
                return Err(OxiGdalError::allocation_error(
                    "Failed to allocate memory".to_string(),
                ));
            }
            NonNull::new_unchecked(raw_ptr)
        };

        self.stats.record_allocation(slab_size);

        #[cfg(debug_assertions)]
        {
            let mut allocated = self.allocated_blocks.lock();
            allocated.entry(slab_size).or_default().push(ptr);
        }

        Ok(ptr)
    }

    /// Deallocate a block
    pub fn deallocate(&self, ptr: NonNull<u8>, size: usize) -> Result<()> {
        // Find the appropriate slab size
        let slab_size = SLAB_SIZES
            .iter()
            .find(|&&s| s >= size)
            .copied()
            .ok_or_else(|| {
                OxiGdalError::invalid_parameter(
                    "parameter",
                    format!("Size {size} exceeds maximum slab size"),
                )
            })?;

        #[cfg(debug_assertions)]
        {
            let mut allocated = self.allocated_blocks.lock();
            if let Some(blocks) = allocated.get_mut(&slab_size) {
                if let Some(pos) = blocks.iter().position(|&p| p == ptr) {
                    blocks.swap_remove(pos);
                } else {
                    return Err(OxiGdalError::invalid_parameter(
                        "parameter",
                        "Block not found in allocated list".to_string(),
                    ));
                }
            } else {
                return Err(OxiGdalError::invalid_parameter(
                    "parameter",
                    "Invalid slab size for deallocation".to_string(),
                ));
            }
        }

        // Return the block to the free list
        let mut free_lists = self.free_lists.write();
        free_lists.entry(slab_size).or_default().push(ptr);

        self.stats.record_deallocation(slab_size);
        Ok(())
    }

    /// Get statistics
    #[must_use]
    pub fn stats(&self) -> Arc<AllocatorStats> {
        Arc::clone(&self.stats)
    }

    /// Check for memory leaks (debug mode only)
    #[cfg(debug_assertions)]
    pub fn check_leaks(&self) -> Result<()> {
        let allocated = self.allocated_blocks.lock();
        let mut total_leaks = 0;
        for (size, blocks) in allocated.iter() {
            if !blocks.is_empty() {
                eprintln!(
                    "Memory leak detected: {} blocks of size {} bytes",
                    blocks.len(),
                    size
                );
                total_leaks += blocks.len();
            }
        }

        if total_leaks > 0 {
            Err(OxiGdalError::invalid_state(format!(
                "Detected {total_leaks} memory leaks"
            )))
        } else {
            Ok(())
        }
    }
}

impl Default for SlabAllocator {
    fn default() -> Self {
        Self::new()
    }
}

/// Buddy allocator for variable-size blocks
pub struct BuddyAllocator {
    /// Free lists for each order (power of 2)
    free_lists: Arc<Mutex<BTreeMap<usize, Vec<NonNull<u8>>>>>,
    /// Minimum block size (power of 2)
    min_block_size: usize,
    /// Maximum block size (power of 2)
    max_block_size: usize,
    /// Statistics
    stats: Arc<AllocatorStats>,
}

// SAFETY: BuddyAllocator is Send because all NonNull pointers are protected
// by Arc<Mutex>, providing proper synchronization
unsafe impl Send for BuddyAllocator {}

// SAFETY: BuddyAllocator is Sync because all NonNull pointers are protected
// by Arc<Mutex>, providing proper synchronization
unsafe impl Sync for BuddyAllocator {}

impl BuddyAllocator {
    /// Create a new buddy allocator
    pub fn new(min_block_size: usize, max_block_size: usize) -> Result<Self> {
        if !min_block_size.is_power_of_two() || !max_block_size.is_power_of_two() {
            return Err(OxiGdalError::invalid_parameter(
                "parameter",
                "Block sizes must be powers of 2".to_string(),
            ));
        }

        if min_block_size > max_block_size {
            return Err(OxiGdalError::invalid_parameter(
                "parameter",
                "Min block size must be <= max block size".to_string(),
            ));
        }

        Ok(Self {
            free_lists: Arc::new(Mutex::new(BTreeMap::new())),
            min_block_size,
            max_block_size,
            stats: Arc::new(AllocatorStats::new()),
        })
    }

    /// Create with default sizes
    ///
    /// # Errors
    ///
    /// Returns an error if allocation fails
    pub fn with_defaults() -> Result<Self> {
        Self::new(MIN_ALIGNMENT, MAX_BLOCK_SIZE)
    }

    /// Round up to next power of 2
    fn next_power_of_two(&self, size: usize) -> usize {
        if size <= self.min_block_size {
            self.min_block_size
        } else {
            size.next_power_of_two().min(self.max_block_size)
        }
    }

    /// Allocate a block
    pub fn allocate(&self, size: usize) -> Result<NonNull<u8>> {
        let block_size = self.next_power_of_two(size);

        if block_size > self.max_block_size {
            self.stats.record_failure();
            return Err(OxiGdalError::allocation_error(format!(
                "Requested size {} exceeds maximum block size {}",
                size, self.max_block_size
            )));
        }

        // Try to find a free block
        let mut free_lists = self.free_lists.lock();

        // Look for a block of the exact size or larger
        let mut found_size = None;
        for (&list_size, blocks) in free_lists.range(block_size..) {
            if !blocks.is_empty() {
                found_size = Some(list_size);
                break;
            }
        }

        if let Some(found_size) = found_size {
            // Take a block from the free list
            let blocks = free_lists
                .get_mut(&found_size)
                .ok_or_else(|| OxiGdalError::invalid_state("Free list disappeared".to_string()))?;
            let ptr = blocks
                .pop()
                .ok_or_else(|| OxiGdalError::invalid_state("Block disappeared".to_string()))?;

            // Split the block if it's larger than needed
            let mut current_size = found_size;
            while current_size > block_size {
                current_size /= 2;
                if current_size >= block_size {
                    // Create buddy block
                    // SAFETY: The pointer arithmetic is within the allocated block bounds.
                    // current_size is guaranteed to be less than the original allocation size.
                    let buddy_ptr =
                        unsafe { NonNull::new_unchecked(ptr.as_ptr().add(current_size)) };
                    free_lists.entry(current_size).or_default().push(buddy_ptr);
                }
            }

            self.stats.record_allocation(block_size);
            Ok(ptr)
        } else {
            // No free block available, allocate a new one
            drop(free_lists);

            let layout = Layout::from_size_align(block_size, block_size)
                .map_err(|e| OxiGdalError::allocation_error(e.to_string()))?;

            // SAFETY: Layout is valid and we check for null before creating NonNull.
            // The pointer will be properly aligned according to the layout.
            let ptr = unsafe {
                let raw_ptr = alloc(layout);
                if raw_ptr.is_null() {
                    self.stats.record_failure();
                    return Err(OxiGdalError::allocation_error(
                        "Failed to allocate memory".to_string(),
                    ));
                }
                NonNull::new_unchecked(raw_ptr)
            };

            self.stats.record_allocation(block_size);
            Ok(ptr)
        }
    }

    /// Deallocate a block
    pub fn deallocate(&self, ptr: NonNull<u8>, size: usize) -> Result<()> {
        let block_size = self.next_power_of_two(size);

        let mut free_lists = self.free_lists.lock();
        free_lists.entry(block_size).or_default().push(ptr);

        self.stats.record_deallocation(block_size);
        Ok(())
    }

    /// Get statistics
    #[must_use]
    pub fn stats(&self) -> Arc<AllocatorStats> {
        Arc::clone(&self.stats)
    }
}

/// Thread-local allocator pool
pub struct ThreadLocalAllocator {
    /// Slab allocator for small fixed-size allocations
    slab: SlabAllocator,
    /// Buddy allocator for larger variable-size allocations
    buddy: BuddyAllocator,
    /// Threshold for using slab vs buddy
    slab_threshold: usize,
}

// SAFETY: ThreadLocalAllocator is Send because it contains SlabAllocator and
// BuddyAllocator which are both Send
unsafe impl Send for ThreadLocalAllocator {}

// SAFETY: ThreadLocalAllocator is Sync because it contains SlabAllocator and
// BuddyAllocator which are both Sync
unsafe impl Sync for ThreadLocalAllocator {}

impl ThreadLocalAllocator {
    /// Create a new thread-local allocator
    ///
    /// # Errors
    ///
    /// Returns an error if allocator initialization fails
    pub fn new() -> Result<Self> {
        Ok(Self {
            slab: SlabAllocator::new(),
            buddy: BuddyAllocator::with_defaults()?,
            slab_threshold: *SLAB_SIZES.last().ok_or_else(|| OxiGdalError::Internal {
                message: "SLAB_SIZES is empty".to_string(),
            })?,
        })
    }

    /// Allocate memory
    pub fn allocate(&self, size: usize, alignment: usize) -> Result<NonNull<u8>> {
        if alignment > MIN_ALIGNMENT && alignment > size {
            return Err(OxiGdalError::invalid_parameter(
                "parameter",
                "Alignment requirements not supported".to_string(),
            ));
        }

        if size <= self.slab_threshold {
            self.slab.allocate(size)
        } else {
            self.buddy.allocate(size)
        }
    }

    /// Deallocate memory
    pub fn deallocate(&self, ptr: NonNull<u8>, size: usize) -> Result<()> {
        if size <= self.slab_threshold {
            self.slab.deallocate(ptr, size)
        } else {
            self.buddy.deallocate(ptr, size)
        }
    }

    /// Get combined statistics
    #[must_use]
    pub fn stats(&self) -> (Arc<AllocatorStats>, Arc<AllocatorStats>) {
        (self.slab.stats(), self.buddy.stats())
    }
}

/// Generic allocator trait
pub trait Allocator: Send + Sync {
    /// Allocate memory
    fn allocate(&self, size: usize, alignment: usize) -> Result<NonNull<u8>>;

    /// Deallocate memory
    fn deallocate(&self, ptr: NonNull<u8>, size: usize) -> Result<()>;

    /// Get statistics
    fn stats(&self) -> Arc<AllocatorStats>;
}

impl Allocator for SlabAllocator {
    fn allocate(&self, size: usize, _alignment: usize) -> Result<NonNull<u8>> {
        self.allocate(size)
    }

    fn deallocate(&self, ptr: NonNull<u8>, size: usize) -> Result<()> {
        self.deallocate(ptr, size)
    }

    fn stats(&self) -> Arc<AllocatorStats> {
        self.stats()
    }
}

impl Allocator for BuddyAllocator {
    fn allocate(&self, size: usize, _alignment: usize) -> Result<NonNull<u8>> {
        self.allocate(size)
    }

    fn deallocate(&self, ptr: NonNull<u8>, size: usize) -> Result<()> {
        self.deallocate(ptr, size)
    }

    fn stats(&self) -> Arc<AllocatorStats> {
        self.stats()
    }
}

impl Allocator for ThreadLocalAllocator {
    fn allocate(&self, size: usize, alignment: usize) -> Result<NonNull<u8>> {
        self.allocate(size, alignment)
    }

    fn deallocate(&self, ptr: NonNull<u8>, size: usize) -> Result<()> {
        self.deallocate(ptr, size)
    }

    fn stats(&self) -> Arc<AllocatorStats> {
        // Return slab stats for now
        self.slab.stats()
    }
}

#[cfg(test)]
#[allow(useless_ptr_null_checks)]
mod tests {
    use super::*;

    #[test]
    fn test_slab_allocator() {
        let allocator = SlabAllocator::new();

        // Allocate a small block
        let ptr1 = allocator
            .allocate(100)
            .expect("Slab allocator should allocate 100 bytes");
        assert!(!ptr1.as_ptr().is_null());

        // Allocate another block
        let ptr2 = allocator
            .allocate(200)
            .expect("Slab allocator should allocate 200 bytes");
        assert!(!ptr2.as_ptr().is_null());
        assert_ne!(ptr1, ptr2);

        // Deallocate
        allocator
            .deallocate(ptr1, 100)
            .expect("Deallocation should succeed");
        allocator
            .deallocate(ptr2, 200)
            .expect("Deallocation should succeed");

        // Check stats
        let stats = allocator.stats();
        assert_eq!(stats.total_allocations.load(Ordering::Relaxed), 2);
        assert_eq!(stats.total_deallocations.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_buddy_allocator() {
        let allocator =
            BuddyAllocator::with_defaults().expect("Default buddy allocator should be created");

        // Allocate blocks
        let ptr1 = allocator
            .allocate(1000)
            .expect("Buddy allocator should allocate 1000 bytes");
        let ptr2 = allocator
            .allocate(2000)
            .expect("Buddy allocator should allocate 2000 bytes");

        assert!(!ptr1.as_ptr().is_null());
        assert!(!ptr2.as_ptr().is_null());

        // Deallocate
        allocator
            .deallocate(ptr1, 1000)
            .expect("Buddy deallocation should succeed");
        allocator
            .deallocate(ptr2, 2000)
            .expect("Buddy deallocation should succeed");
    }

    #[test]
    fn test_thread_local_allocator() {
        let allocator =
            ThreadLocalAllocator::new().expect("Thread-local allocator should be created");

        // Test small allocation (slab)
        let ptr1 = allocator
            .allocate(256, MIN_ALIGNMENT)
            .expect("Thread-local allocator should allocate small block");
        assert!(!ptr1.as_ptr().is_null());

        // Test large allocation (buddy)
        let ptr2 = allocator
            .allocate(1024 * 1024, MIN_ALIGNMENT)
            .expect("Thread-local allocator should allocate large block");
        assert!(!ptr2.as_ptr().is_null());

        // Deallocate
        allocator
            .deallocate(ptr1, 256)
            .expect("Thread-local deallocation should succeed");
        allocator
            .deallocate(ptr2, 1024 * 1024)
            .expect("Thread-local deallocation should succeed");
    }

    #[test]
    fn test_allocator_stats() {
        let stats = AllocatorStats::new();

        stats.record_allocation(1024);
        stats.record_allocation(2048);

        assert_eq!(stats.total_allocations.load(Ordering::Relaxed), 2);
        assert_eq!(stats.bytes_allocated.load(Ordering::Relaxed), 3072);
        assert_eq!(stats.peak_bytes_allocated.load(Ordering::Relaxed), 3072);

        stats.record_deallocation(1024);
        assert_eq!(stats.bytes_allocated.load(Ordering::Relaxed), 2048);
        assert_eq!(stats.current_allocations(), 1);
    }
}
