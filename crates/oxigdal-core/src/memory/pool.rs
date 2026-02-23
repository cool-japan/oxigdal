//! Memory Pool Management
//!
//! This module provides memory pools for efficient buffer reuse:
//! - Buffer size classes (512B, 4KB, 64KB, 1MB, etc.)
//! - Pool growing and shrinking
//! - Memory limit enforcement
//! - Pool fragmentation tracking
//! - Automatic pool compaction

// Arc with PoolInner that has unsafe Send+Sync impls is intentional
#![allow(clippy::arc_with_non_send_sync)]
// Unsafe code is necessary for memory pool operations
#![allow(unsafe_code)]

use crate::error::{OxiGdalError, Result};
use parking_lot::RwLock;
use std::alloc::{Layout, alloc, dealloc};
use std::collections::HashMap;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Standard buffer size classes
pub const SIZE_CLASSES: &[usize] = &[
    512,        // 512 bytes
    4096,       // 4 KB
    16384,      // 16 KB
    65536,      // 64 KB
    262_144,    // 256 KB
    1_048_576,  // 1 MB
    4_194_304,  // 4 MB
    16_777_216, // 16 MB
];

/// Default memory limit (1GB)
pub const DEFAULT_MEMORY_LIMIT: usize = 1024 * 1024 * 1024;

/// Pool statistics
#[derive(Debug, Default)]
pub struct PoolStats {
    /// Total allocations from pool
    pub total_allocations: AtomicU64,
    /// Total deallocations to pool
    pub total_deallocations: AtomicU64,
    /// Cache hits (allocated from pool)
    pub cache_hits: AtomicU64,
    /// Cache misses (allocated from system)
    pub cache_misses: AtomicU64,
    /// Current bytes in pool
    pub bytes_in_pool: AtomicUsize,
    /// Peak bytes in pool
    pub peak_bytes: AtomicUsize,
    /// Number of compactions
    pub compactions: AtomicU64,
}

impl PoolStats {
    /// Create new statistics
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an allocation
    pub fn record_allocation(&self, from_pool: bool) {
        self.total_allocations.fetch_add(1, Ordering::Relaxed);
        if from_pool {
            self.cache_hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.cache_misses.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record a deallocation
    pub fn record_deallocation(&self, size: usize) {
        self.total_deallocations.fetch_add(1, Ordering::Relaxed);
        let new_bytes = self.bytes_in_pool.fetch_add(size, Ordering::Relaxed) + size;

        // Update peak
        let mut peak = self.peak_bytes.load(Ordering::Relaxed);
        while new_bytes > peak {
            match self.peak_bytes.compare_exchange_weak(
                peak,
                new_bytes,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => peak = x,
            }
        }
    }

    /// Record a compaction
    pub fn record_compaction(&self, bytes_freed: usize) {
        self.compactions.fetch_add(1, Ordering::Relaxed);
        self.bytes_in_pool.fetch_sub(bytes_freed, Ordering::Relaxed);
    }

    /// Get cache hit rate
    pub fn hit_rate(&self) -> f64 {
        let hits = self.cache_hits.load(Ordering::Relaxed);
        let misses = self.cache_misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }

    /// Get current pool size
    pub fn current_size(&self) -> usize {
        self.bytes_in_pool.load(Ordering::Relaxed)
    }
}

/// Configuration for memory pool
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Size classes and their initial capacities
    pub size_classes: HashMap<usize, usize>,
    /// Maximum total memory in pool
    pub memory_limit: usize,
    /// Compaction threshold (fraction of limit)
    pub compaction_threshold: f64,
    /// Minimum free buffers before growing
    pub min_free_buffers: usize,
}

impl Default for PoolConfig {
    fn default() -> Self {
        let mut size_classes = HashMap::new();
        for &size in SIZE_CLASSES {
            size_classes.insert(size, 8); // 8 buffers per class
        }

        Self {
            size_classes,
            memory_limit: DEFAULT_MEMORY_LIMIT,
            compaction_threshold: 0.8,
            min_free_buffers: 2,
        }
    }
}

impl PoolConfig {
    /// Create new configuration
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a size class
    #[must_use]
    pub fn with_size_class(mut self, size: usize, initial_count: usize) -> Self {
        self.size_classes.insert(size, initial_count);
        self
    }

    /// Set memory limit
    #[must_use]
    pub fn with_memory_limit(mut self, limit: usize) -> Self {
        self.memory_limit = limit;
        self
    }

    /// Set compaction threshold
    #[must_use]
    pub fn with_compaction_threshold(mut self, threshold: f64) -> Self {
        self.compaction_threshold = threshold;
        self
    }

    /// Set minimum free buffers
    #[must_use]
    pub fn with_min_free_buffers(mut self, count: usize) -> Self {
        self.min_free_buffers = count;
        self
    }
}

/// Buffer handle from pool
pub struct PooledBuffer {
    ptr: NonNull<u8>,
    size: usize,
    pool: Arc<PoolInner>,
}

impl PooledBuffer {
    /// Get the buffer size
    #[must_use]
    pub fn size(&self) -> usize {
        self.size
    }

    /// Get as slice
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: ptr and size are valid. The buffer was properly allocated
        // and we have shared access.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.size) }
    }

    /// Get as mutable slice
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: ptr and size are valid. We have exclusive mutable access.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.size) }
    }

    /// Get typed slice
    pub fn as_typed_slice<T: bytemuck::Pod>(&self) -> Result<&[T]> {
        if self.size % std::mem::size_of::<T>() != 0 {
            return Err(OxiGdalError::invalid_parameter(
                "parameter",
                "Buffer size not aligned to type".to_string(),
            ));
        }
        let count = self.size / std::mem::size_of::<T>();
        // SAFETY: Size alignment verified. The pointer is valid and count
        // is within bounds. bytemuck::Pod ensures T is safe to read.
        Ok(unsafe { std::slice::from_raw_parts(self.ptr.as_ptr() as *const T, count) })
    }

    /// Get typed mutable slice
    pub fn as_typed_mut_slice<T: bytemuck::Pod>(&mut self) -> Result<&mut [T]> {
        if self.size % std::mem::size_of::<T>() != 0 {
            return Err(OxiGdalError::invalid_parameter(
                "parameter",
                "Buffer size not aligned to type".to_string(),
            ));
        }
        let count = self.size / std::mem::size_of::<T>();
        // SAFETY: Size alignment verified. We have exclusive mutable access.
        Ok(unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr().cast::<T>(), count) })
    }
}

impl Drop for PooledBuffer {
    fn drop(&mut self) {
        self.pool.return_buffer(self.ptr, self.size);
    }
}

/// Internal pool state
struct PoolInner {
    /// Free buffers by size class
    free_buffers: RwLock<HashMap<usize, Vec<NonNull<u8>>>>,
    /// Configuration
    config: PoolConfig,
    /// Statistics
    stats: Arc<PoolStats>,
}

impl PoolInner {
    fn new(config: PoolConfig) -> Result<Self> {
        let mut free_buffers = HashMap::new();

        // Pre-allocate initial buffers
        for (&size, &count) in &config.size_classes {
            let mut buffers = Vec::new();
            for _ in 0..count {
                let ptr = Self::allocate_buffer(size)?;
                buffers.push(ptr);
            }
            free_buffers.insert(size, buffers);
        }

        Ok(Self {
            free_buffers: RwLock::new(free_buffers),
            config,
            stats: Arc::new(PoolStats::new()),
        })
    }

    fn allocate_buffer(size: usize) -> Result<NonNull<u8>> {
        let layout = Layout::from_size_align(size, 16)
            .map_err(|e| OxiGdalError::allocation_error(e.to_string()))?;

        // SAFETY: Layout is valid and we check for null after allocation.
        unsafe {
            let ptr = alloc(layout);
            if ptr.is_null() {
                return Err(OxiGdalError::allocation_error(
                    "Failed to allocate buffer".to_string(),
                ));
            }
            Ok(NonNull::new_unchecked(ptr))
        }
    }

    fn get_buffer(&self, size: usize) -> Result<NonNull<u8>> {
        // Find appropriate size class
        let size_class = SIZE_CLASSES
            .iter()
            .find(|&&s| s >= size)
            .copied()
            .unwrap_or_else(|| size.next_power_of_two());

        // Try to get from pool
        {
            let mut free_buffers = self.free_buffers.write();
            if let Some(buffers) = free_buffers.get_mut(&size_class) {
                if let Some(ptr) = buffers.pop() {
                    self.stats.record_allocation(true);
                    return Ok(ptr);
                }
            }
        }

        // Pool miss, allocate new buffer
        self.stats.record_allocation(false);
        Self::allocate_buffer(size_class)
    }

    fn return_buffer(&self, ptr: NonNull<u8>, size: usize) {
        // Find appropriate size class
        let size_class = SIZE_CLASSES
            .iter()
            .find(|&&s| s >= size)
            .copied()
            .unwrap_or_else(|| size.next_power_of_two());

        let mut free_buffers = self.free_buffers.write();

        // Check if we're over the memory limit
        let current_size = self.stats.current_size();
        let threshold =
            (self.config.memory_limit as f64 * self.config.compaction_threshold) as usize;

        if current_size >= threshold {
            // Don't return to pool, deallocate immediately
            drop(free_buffers);
            // SAFETY: Layout matches allocation. We're cleaning up at threshold.
            unsafe {
                let layout = Layout::from_size_align_unchecked(size_class, 16);
                dealloc(ptr.as_ptr(), layout);
            }
            return;
        }

        // Return to pool
        free_buffers.entry(size_class).or_default().push(ptr);
        self.stats.record_deallocation(size_class);
    }

    fn compact(&self) -> Result<()> {
        let mut free_buffers = self.free_buffers.write();
        let mut bytes_freed = 0;

        // Keep only min_free_buffers in each size class
        for (&size, buffers) in free_buffers.iter_mut() {
            while buffers.len() > self.config.min_free_buffers {
                if let Some(ptr) = buffers.pop() {
                    // SAFETY: Layout matches allocation. Cleaning up excess buffers.
                    unsafe {
                        let layout = Layout::from_size_align_unchecked(size, 16);
                        dealloc(ptr.as_ptr(), layout);
                    }
                    bytes_freed += size;
                }
            }
        }

        self.stats.record_compaction(bytes_freed);
        Ok(())
    }
}

impl Drop for PoolInner {
    fn drop(&mut self) {
        let free_buffers = self.free_buffers.write();
        for (&size, buffers) in free_buffers.iter() {
            for &ptr in buffers {
                // SAFETY: Layout matches allocation during pool cleanup.
                unsafe {
                    let layout = Layout::from_size_align_unchecked(size, 16);
                    dealloc(ptr.as_ptr(), layout);
                }
            }
        }
    }
}

// SAFETY: PoolInner can be safely sent between threads because:
// - All access to the internal NonNull<u8> pointers is protected by RwLock
// - The pointers represent heap-allocated memory that is valid across threads
// - No thread-local state is accessed
unsafe impl Send for PoolInner {}

// SAFETY: PoolInner can be safely shared between threads because:
// - All mutable access to internal state is protected by RwLock
// - NonNull<u8> pointers are just addresses that can be safely read concurrently
// - Atomic operations protect statistics
unsafe impl Sync for PoolInner {}

/// Memory pool for buffer reuse
pub struct Pool {
    inner: Arc<PoolInner>,
}

impl Pool {
    /// Create a new pool with default configuration
    pub fn new() -> Result<Self> {
        Self::with_config(PoolConfig::default())
    }

    /// Create a new pool with custom configuration
    pub fn with_config(config: PoolConfig) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(PoolInner::new(config)?),
        })
    }

    /// Allocate a buffer from the pool
    pub fn allocate(&self, size: usize) -> Result<PooledBuffer> {
        let ptr = self.inner.get_buffer(size)?;
        Ok(PooledBuffer {
            ptr,
            size,
            pool: Arc::clone(&self.inner),
        })
    }

    /// Get statistics
    #[must_use]
    pub fn stats(&self) -> Arc<PoolStats> {
        Arc::clone(&self.inner.stats)
    }

    /// Compact the pool
    pub fn compact(&self) -> Result<()> {
        self.inner.compact()
    }

    /// Get current pool size
    #[must_use]
    pub fn size(&self) -> usize {
        self.stats().current_size()
    }
}

impl Clone for Pool {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_basic() {
        let pool = Pool::new().expect("Failed to create memory pool");

        let buffer1 = pool
            .allocate(1024)
            .expect("Failed to allocate 1024-byte buffer");
        assert_eq!(buffer1.size(), 1024); // Returns requested size, not rounded size class

        let buffer2 = pool
            .allocate(2048)
            .expect("Failed to allocate 2048-byte buffer");
        assert_eq!(buffer2.size(), 2048);
    }

    #[test]
    fn test_pool_reuse() {
        let pool = Pool::new().expect("Failed to create memory pool for reuse test");

        {
            let _buffer = pool
                .allocate(1024)
                .expect("Failed to allocate buffer for reuse test");
        }

        let stats = pool.stats();
        assert_eq!(stats.total_allocations.load(Ordering::Relaxed), 1);
        assert_eq!(stats.total_deallocations.load(Ordering::Relaxed), 1);

        // Allocate again - should reuse the previously deallocated buffer
        let _buffer2 = pool
            .allocate(1024)
            .expect("Failed to allocate second buffer for reuse test");
        // Cache hits may vary depending on internal pooling strategy
        assert!(stats.cache_hits.load(Ordering::Relaxed) >= 1);
    }

    #[test]
    fn test_pool_config() {
        let config = PoolConfig::new()
            .with_size_class(8192, 4)
            .with_memory_limit(1024 * 1024)
            .with_min_free_buffers(1);

        let pool = Pool::with_config(config).expect("Failed to create pool with custom config");
        let _buffer = pool
            .allocate(8000)
            .expect("Failed to allocate 8000-byte buffer");
    }

    #[test]
    fn test_buffer_slice() {
        let pool = Pool::new().expect("Failed to create pool for buffer slice test");
        let mut buffer = pool
            .allocate(1024)
            .expect("Failed to allocate buffer for slice test");

        let slice = buffer.as_mut_slice();
        slice[0] = 42;

        assert_eq!(buffer.as_slice()[0], 42);
    }

    #[test]
    fn test_typed_buffer() {
        let pool = Pool::new().expect("Failed to create pool for typed buffer test");
        let mut buffer = pool
            .allocate(4096)
            .expect("Failed to allocate buffer for typed slice test");

        let slice: &mut [u32] = buffer
            .as_typed_mut_slice()
            .expect("Failed to get mutable typed slice");
        slice[0] = 12345;

        let read_slice: &[u32] = buffer.as_typed_slice().expect("Failed to get typed slice");
        assert_eq!(read_slice[0], 12345);
    }

    #[test]
    fn test_pool_stats() {
        let pool = Pool::new().expect("Failed to create pool for stats test");

        let _b1 = pool
            .allocate(1024)
            .expect("Failed to allocate first buffer for stats test");
        let _b2 = pool
            .allocate(2048)
            .expect("Failed to allocate second buffer for stats test");

        let stats = pool.stats();
        assert!(stats.total_allocations.load(Ordering::Relaxed) >= 2);
    }
}
