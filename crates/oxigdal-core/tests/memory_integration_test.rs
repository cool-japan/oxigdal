//! Comprehensive integration tests for memory module

#![allow(
    useless_ptr_null_checks,
    clippy::needless_range_loop,
    clippy::approx_constant
)]

use oxigdal_core::Result;
use oxigdal_core::memory::*;
use std::sync::atomic::Ordering;

#[test]
fn test_allocator_integration() -> Result<()> {
    // Test slab allocator
    let slab = SlabAllocator::new();
    let mut ptrs = Vec::new();

    for _ in 0..10 {
        let ptr = slab.allocate(1024)?;
        ptrs.push(ptr);
    }

    let stats = slab.stats();
    assert_eq!(stats.total_allocations.load(Ordering::Relaxed), 10);

    for ptr in ptrs {
        slab.deallocate(ptr, 1024)?;
    }

    assert_eq!(stats.total_deallocations.load(Ordering::Relaxed), 10);

    Ok(())
}

#[test]
fn test_buddy_allocator_integration() -> Result<()> {
    let buddy = BuddyAllocator::with_defaults()?;

    // Allocate various sizes
    let ptr1 = buddy.allocate(1000)?;
    let ptr2 = buddy.allocate(5000)?;
    let ptr3 = buddy.allocate(10000)?;

    buddy.deallocate(ptr1, 1000)?;
    buddy.deallocate(ptr2, 5000)?;
    buddy.deallocate(ptr3, 10000)?;

    Ok(())
}

#[test]
fn test_memory_map_integration() -> Result<()> {
    use std::io::Write;

    // Create temporary file
    let mut file = tempfile::NamedTempFile::new()
        .map_err(|e| oxigdal_core::error::OxiGdalError::io_error(e.to_string()))?;
    let data = vec![42u8; 8192];
    file.write_all(&data)
        .map_err(|e| oxigdal_core::error::OxiGdalError::io_error(e.to_string()))?;
    file.flush()
        .map_err(|e| oxigdal_core::error::OxiGdalError::io_error(e.to_string()))?;

    let path = file.path();

    // Test memory mapping
    let map = MemoryMap::new(path)?;
    assert_eq!(map.len(), 8192);

    let slice = map.as_slice();
    assert_eq!(slice[0], 42);
    assert_eq!(slice[100], 42);

    // Test prefetch
    map.prefetch(0, 4096)?;

    // Test typed slice
    let u32_slice: &[u32] = map.as_typed_slice()?;
    assert_eq!(u32_slice.len(), 2048);

    Ok(())
}

#[test]
fn test_shared_buffer_integration() -> Result<()> {
    // Create shared buffer
    let mut buffer = SharedBuffer::new(4096)?;
    assert_eq!(buffer.len(), 4096);
    assert_eq!(buffer.ref_count(), 1);

    // Share the buffer
    let shared1 = buffer.share();
    let shared2 = buffer.share();

    assert_eq!(buffer.ref_count(), 3);
    assert_eq!(shared1.ref_count(), 3);
    assert_eq!(shared2.ref_count(), 3);

    // Trigger copy-on-write
    {
        let slice = buffer.as_mut_slice()?;
        slice[0] = 123;
    }

    assert_eq!(buffer.ref_count(), 1);
    assert_eq!(shared1.ref_count(), 2);
    assert_eq!(buffer.as_slice()[0], 123);
    assert_eq!(shared1.as_slice()[0], 0);

    Ok(())
}

#[test]
fn test_zero_copy_buffer_integration() -> Result<()> {
    let mut buffer: ZeroCopyBuffer<u32> = ZeroCopyBuffer::new(1024)?;
    assert_eq!(buffer.len(), 1024);

    // Write data
    {
        let slice = buffer.as_mut_slice()?;
        for i in 0..1024 {
            slice[i] = i as u32;
        }
    }

    // Read data
    let slice = buffer.as_slice();
    assert_eq!(slice[0], 0);
    assert_eq!(slice[100], 100);
    assert_eq!(slice[1023], 1023);

    // Share buffer
    let shared = buffer.share();
    assert_eq!(buffer.ref_count(), 2);
    assert_eq!(shared.ref_count(), 2);

    Ok(())
}

#[test]
fn test_arena_integration() -> Result<()> {
    let arena = Arena::with_capacity(65536)?;

    // Allocate multiple blocks
    let ptr1 = arena.allocate(1000)?;
    let ptr2 = arena.allocate(2000)?;
    let ptr3 = arena.allocate(3000)?;

    assert!(!ptr1.as_ptr().is_null());
    assert!(!ptr2.as_ptr().is_null());
    assert!(!ptr3.as_ptr().is_null());

    let usage = arena.usage();
    assert!(usage >= 6000);

    // Reset arena
    arena.reset();
    assert_eq!(arena.usage(), 0);

    // Allocate again
    let ptr4 = arena.allocate(5000)?;
    assert!(!ptr4.as_ptr().is_null());

    Ok(())
}

#[test]
fn test_arena_pool_integration() -> Result<()> {
    let pool = ArenaPool::new(32768, 4);

    // Acquire multiple arenas
    let arena1 = pool.acquire()?;
    let arena2 = pool.acquire()?;

    arena1.allocate(1000)?;
    arena2.allocate(2000)?;

    // Return to pool
    pool.release(arena1);
    pool.release(arena2);

    assert_eq!(pool.pool_size(), 2);

    // Reacquire
    let arena3 = pool.acquire()?;
    assert_eq!(arena3.usage(), 0); // Should be reset

    Ok(())
}

#[test]
fn test_memory_pool_integration() -> Result<()> {
    let config = PoolConfig::new()
        .with_size_class(4096, 4)
        .with_size_class(16384, 2)
        .with_memory_limit(10 * 1024 * 1024);

    let pool = Pool::with_config(config)?;

    // Allocate buffers
    let mut buffers = Vec::new();
    for _ in 0..10 {
        let buffer = pool.allocate(4096)?;
        buffers.push(buffer);
    }

    let stats = pool.stats();
    assert!(stats.total_allocations.load(Ordering::Relaxed) >= 10);

    // Drop buffers (return to pool)
    drop(buffers);

    assert!(stats.total_deallocations.load(Ordering::Relaxed) >= 10);

    // Allocate again (should hit pool)
    let _buffer = pool.allocate(4096)?;
    assert!(stats.cache_hits.load(Ordering::Relaxed) > 0);

    Ok(())
}

#[test]
fn test_numa_integration() -> Result<()> {
    let node_count = numa::get_numa_node_count()?;
    assert!(node_count >= 1);

    let current_node = numa::get_current_node()?;
    assert!(current_node.id() >= 0);

    let allocator = NumaAllocator::new()?;
    let ptr = allocator.allocate(4096)?;
    assert!(!ptr.is_null());

    allocator.deallocate(ptr, 4096)?;

    Ok(())
}

#[test]
fn test_huge_pages_integration() -> Result<()> {
    let config = HugePageConfig::new()
        .with_page_size(HugePageSize::Size2MB)
        .with_fallback(true);

    let allocator = HugePageAllocator::with_config(config)?;

    // Try to allocate 2MB
    let size = 2 * 1024 * 1024;
    let ptr = allocator.allocate(size)?;
    assert!(!ptr.is_null());

    allocator.deallocate(ptr, size)?;

    let stats = allocator.stats();
    let total = stats.total_allocations();
    assert_eq!(total, 1);

    Ok(())
}

#[test]
fn test_mixed_allocators() -> Result<()> {
    // Test using multiple allocators together
    let slab = SlabAllocator::new();
    let buddy = BuddyAllocator::with_defaults()?;
    let pool = Pool::new()?;
    let arena = Arena::new()?;

    // Allocate from each
    let _slab_ptr = slab.allocate(256)?;
    let _buddy_ptr = buddy.allocate(1024)?;
    let _pool_buffer = pool.allocate(4096)?;
    let _arena_ptr = arena.allocate(512)?;

    Ok(())
}

#[test]
fn test_concurrent_allocations() -> Result<()> {
    use std::sync::Arc;
    use std::thread;

    let pool = Arc::new(Pool::new()?);
    let mut handles = Vec::new();

    // Spawn multiple threads
    for _ in 0..4 {
        let pool_clone = Arc::clone(&pool);
        let handle = thread::spawn(move || {
            for _ in 0..100 {
                let buffer = pool_clone
                    .allocate(4096)
                    .expect("pool allocation should succeed in concurrent test");
                // Use buffer
                let _slice = buffer.as_slice();
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().expect("thread should complete successfully");
    }

    let stats = pool.stats();
    assert_eq!(stats.total_allocations.load(Ordering::Relaxed), 400);

    Ok(())
}

#[test]
fn test_memory_limit_enforcement() -> Result<()> {
    let config = PoolConfig::new()
        .with_size_class(4096, 2)
        .with_memory_limit(16384); // Only 16KB

    let pool = Pool::with_config(config)?;

    // Allocate up to limit
    let _b1 = pool.allocate(4096)?;
    let _b2 = pool.allocate(4096)?;
    let _b3 = pool.allocate(4096)?;
    let _b4 = pool.allocate(4096)?;

    // Pool should handle this by not caching returns

    Ok(())
}

#[test]
fn test_alignment() -> Result<()> {
    let arena = Arena::with_capacity_and_alignment(8192, 64)?;

    let ptr = arena.allocate_aligned(1000, 64)?;
    let addr = ptr.as_ptr() as usize;
    assert_eq!(addr % 64, 0); // Check 64-byte alignment

    Ok(())
}

#[test]
fn test_typed_allocations() -> Result<()> {
    let arena = Arena::with_capacity(8192)?;

    // Allocate a slice
    let slice: &mut [f64] = arena.allocate_slice(100)?;
    slice[0] = 3.14;
    slice[99] = 2.71;

    assert_eq!(slice[0], 3.14);
    assert_eq!(slice[99], 2.71);

    // Allocate a value
    let value = arena.allocate_value(42i32)?;
    assert_eq!(*value, 42);

    Ok(())
}
