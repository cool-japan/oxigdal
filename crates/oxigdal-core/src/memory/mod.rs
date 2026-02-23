//! Advanced Memory Optimization for Geospatial Data
//!
//! This module provides comprehensive memory management features optimized for
//! geospatial workloads, including:
//!
//! - Custom allocators (slab, buddy) for efficient tile allocation
//! - Memory-mapped I/O with read-ahead and write-behind
//! - Zero-copy data transfers between operations
//! - Arena allocators for batch processing
//! - Memory pools for buffer reuse
//! - NUMA-aware allocation for multi-socket systems
//! - Huge pages support for large datasets
//!
//! # Performance Targets
//!
//! - 50% reduction in memory usage for large datasets
//! - 2-3x faster allocation for small objects
//! - Zero-copy transfers where possible
//! - NUMA local access > 90%
//!
//! # Example
//!
//! ```
//! use oxigdal_core::memory::{Pool, PoolConfig};
//! use oxigdal_core::error::Result;
//!
//! # fn main() -> Result<()> {
//! // Create a memory pool for 4KB buffers
//! let config = PoolConfig::new()
//!     .with_size_class(4096, 100);
//! let pool = Pool::with_config(config)?;
//!
//! // Allocate a buffer from the pool
//! let buffer = pool.allocate(4096)?;
//! # Ok(())
//! # }
//! ```

pub mod allocator;
pub mod arena;
#[cfg(unix)]
pub mod hugepages;
#[cfg(unix)]
pub mod mmap;
#[cfg(unix)]
pub mod numa;
pub mod pool;
pub mod zero_copy;

// Re-export commonly used types
pub use allocator::{
    Allocator, AllocatorStats, BuddyAllocator, SlabAllocator, ThreadLocalAllocator,
};
pub use arena::{Arena, ArenaPool, ArenaStats};
#[cfg(unix)]
pub use hugepages::{HugePageAllocator, HugePageConfig, HugePageSize, HugePageStats};
#[cfg(unix)]
pub use mmap::{MemoryMap, MemoryMapConfig, MemoryMapMode};
#[cfg(unix)]
pub use numa::{NumaAllocator, NumaConfig, NumaNode, NumaStats};
pub use pool::{Pool, PoolConfig, PoolStats};
pub use zero_copy::{SharedBuffer, ZeroCopyBuffer, ZeroCopyConfig};

/// Memory alignment for SIMD operations (64 bytes for AVX-512)
pub const SIMD_ALIGNMENT: usize = 64;

/// Memory alignment for GPU transfers (256 bytes)
pub const GPU_ALIGNMENT: usize = 256;

/// Default page size (4KB)
pub const PAGE_SIZE: usize = 4096;

/// Huge page size (2MB on most systems)
pub const HUGE_PAGE_SIZE: usize = 2 * 1024 * 1024;

/// Maximum pool size before compaction (1GB)
pub const MAX_POOL_SIZE: usize = 1024 * 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(SIMD_ALIGNMENT, 64);
        assert_eq!(GPU_ALIGNMENT, 256);
        assert_eq!(PAGE_SIZE, 4096);
        assert_eq!(HUGE_PAGE_SIZE, 2 * 1024 * 1024);
    }
}
