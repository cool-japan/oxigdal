//! Huge Pages Support
//!
//! This module provides support for transparent huge pages (THP):
//! - Huge page allocation (2MB/1GB pages)
//! - Fallback to standard pages
//! - Platform-specific APIs (Linux madvise, Windows large pages)
//! - Huge page statistics

// Unsafe code is necessary for huge page allocations
#![allow(unsafe_code)]

use crate::error::{OxiGdalError, Result};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Standard page size (4KB)
pub const STANDARD_PAGE_SIZE: usize = 4096;

/// 2MB huge page size
pub const HUGE_PAGE_2MB: usize = 2 * 1024 * 1024;

/// 1GB huge page size
pub const HUGE_PAGE_1GB: usize = 1024 * 1024 * 1024;

/// Huge page size
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HugePageSize {
    /// 2MB pages
    Size2MB,
    /// 1GB pages
    Size1GB,
}

impl HugePageSize {
    /// Get size in bytes
    #[must_use]
    pub fn bytes(&self) -> usize {
        match self {
            Self::Size2MB => HUGE_PAGE_2MB,
            Self::Size1GB => HUGE_PAGE_1GB,
        }
    }
}

/// Huge page configuration
#[derive(Debug, Clone)]
pub struct HugePageConfig {
    /// Preferred huge page size
    pub page_size: HugePageSize,
    /// Fallback to standard pages if huge pages unavailable
    pub allow_fallback: bool,
    /// Enable transparent huge pages
    pub use_transparent: bool,
    /// Track statistics
    pub track_stats: bool,
}

impl Default for HugePageConfig {
    fn default() -> Self {
        Self {
            page_size: HugePageSize::Size2MB,
            allow_fallback: true,
            use_transparent: true,
            track_stats: true,
        }
    }
}

impl HugePageConfig {
    /// Create new configuration
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set page size
    #[must_use]
    pub fn with_page_size(mut self, size: HugePageSize) -> Self {
        self.page_size = size;
        self
    }

    /// Set fallback behavior
    #[must_use]
    pub fn with_fallback(mut self, allow: bool) -> Self {
        self.allow_fallback = allow;
        self
    }

    /// Set transparent huge pages
    #[must_use]
    pub fn with_transparent(mut self, enable: bool) -> Self {
        self.use_transparent = enable;
        self
    }

    /// Set statistics tracking
    #[must_use]
    pub fn with_stats(mut self, track: bool) -> Self {
        self.track_stats = track;
        self
    }
}

/// Huge page statistics
#[derive(Debug, Default)]
pub struct HugePageStats {
    /// Successful huge page allocations
    pub huge_page_allocations: AtomicU64,
    /// Fallback to standard pages
    pub fallback_allocations: AtomicU64,
    /// Total bytes in huge pages
    pub bytes_in_huge_pages: AtomicUsize,
    /// Total bytes in standard pages
    pub bytes_in_standard_pages: AtomicUsize,
    /// Number of promotions to huge pages
    pub promotions: AtomicU64,
}

impl HugePageStats {
    /// Create new statistics
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a huge page allocation
    pub fn record_huge_page(&self, size: usize) {
        self.huge_page_allocations.fetch_add(1, Ordering::Relaxed);
        self.bytes_in_huge_pages.fetch_add(size, Ordering::Relaxed);
    }

    /// Record a fallback allocation
    pub fn record_fallback(&self, size: usize) {
        self.fallback_allocations.fetch_add(1, Ordering::Relaxed);
        self.bytes_in_standard_pages
            .fetch_add(size, Ordering::Relaxed);
    }

    /// Record a promotion
    pub fn record_promotion(&self) {
        self.promotions.fetch_add(1, Ordering::Relaxed);
    }

    /// Get huge page success rate
    pub fn success_rate(&self) -> f64 {
        let huge = self.huge_page_allocations.load(Ordering::Relaxed);
        let fallback = self.fallback_allocations.load(Ordering::Relaxed);
        let total = huge + fallback;

        if total == 0 {
            0.0
        } else {
            (huge as f64 / total as f64) * 100.0
        }
    }

    /// Get total allocations
    pub fn total_allocations(&self) -> u64 {
        self.huge_page_allocations.load(Ordering::Relaxed)
            + self.fallback_allocations.load(Ordering::Relaxed)
    }
}

/// Check if huge pages are available
#[must_use]
pub fn is_huge_pages_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::path::Path::new("/sys/kernel/mm/transparent_hugepage/enabled").exists()
    }

    #[cfg(target_os = "windows")]
    {
        // Check for large page privilege on Windows
        // This is a simplified check
        true
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        false
    }
}

/// Huge page allocator
pub struct HugePageAllocator {
    /// Configuration
    config: HugePageConfig,
    /// Statistics
    stats: Arc<HugePageStats>,
}

impl HugePageAllocator {
    /// Create a new huge page allocator
    pub fn new() -> Result<Self> {
        Self::with_config(HugePageConfig::default())
    }

    /// Create with custom configuration
    pub fn with_config(config: HugePageConfig) -> Result<Self> {
        if !is_huge_pages_available() && !config.allow_fallback {
            return Err(OxiGdalError::not_supported(
                "Huge pages not available and fallback disabled".to_string(),
            ));
        }

        Ok(Self {
            config,
            stats: Arc::new(HugePageStats::new()),
        })
    }

    /// Allocate memory with huge pages
    pub fn allocate(&self, size: usize) -> Result<*mut u8> {
        if size == 0 {
            return Err(OxiGdalError::invalid_parameter(
                "parameter",
                "Size must be non-zero".to_string(),
            ));
        }

        // Try huge page allocation first
        match self.allocate_huge_page(size) {
            Ok(ptr) => {
                if self.config.track_stats {
                    self.stats.record_huge_page(size);
                }
                Ok(ptr)
            }
            Err(_) if self.config.allow_fallback => {
                // Fall back to standard allocation
                let ptr = self.allocate_standard(size)?;
                if self.config.track_stats {
                    self.stats.record_fallback(size);
                }
                Ok(ptr)
            }
            Err(e) => Err(e),
        }
    }

    /// Allocate with huge pages
    fn allocate_huge_page(&self, size: usize) -> Result<*mut u8> {
        #[cfg(target_os = "linux")]
        {
            use std::ptr::null_mut;

            let page_size = self.config.page_size.bytes();
            let aligned_size = ((size + page_size - 1) / page_size) * page_size;

            let mut flags = libc::MAP_PRIVATE | libc::MAP_ANONYMOUS;

            // Use MAP_HUGETLB for explicit huge pages
            if !self.config.use_transparent {
                flags |= libc::MAP_HUGETLB;

                // Set huge page size
                match self.config.page_size {
                    HugePageSize::Size2MB => {
                        #[cfg(target_arch = "x86_64")]
                        {
                            flags |= 21 << libc::MAP_HUGE_SHIFT; // 2MB = 2^21
                        }
                    }
                    HugePageSize::Size1GB => {
                        #[cfg(target_arch = "x86_64")]
                        {
                            flags |= 30 << libc::MAP_HUGE_SHIFT; // 1GB = 2^30
                        }
                    }
                }
            }

            let ptr = unsafe {
                libc::mmap(
                    null_mut(),
                    aligned_size,
                    libc::PROT_READ | libc::PROT_WRITE,
                    flags,
                    -1,
                    0,
                )
            };

            if ptr == libc::MAP_FAILED {
                return Err(OxiGdalError::allocation_error(
                    "Huge page allocation failed".to_string(),
                ));
            }

            // Apply transparent huge page hints
            if self.config.use_transparent {
                unsafe {
                    libc::madvise(ptr, aligned_size, libc::MADV_HUGEPAGE);
                }
            }

            Ok(ptr as *mut u8)
        }

        #[cfg(target_os = "windows")]
        {
            // Windows large page allocation would go here
            // Requires SeLockMemoryPrivilege
            Err(OxiGdalError::not_supported(
                "Windows large pages not implemented".to_string(),
            ))
        }

        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            let _ = size; // used on Linux (mmap aligned_size calculation)
            Err(OxiGdalError::not_supported(
                "Huge pages not supported on this platform".to_string(),
            ))
        }
    }

    /// Allocate with standard pages
    fn allocate_standard(&self, size: usize) -> Result<*mut u8> {
        let layout = std::alloc::Layout::from_size_align(size, STANDARD_PAGE_SIZE)
            .map_err(|e| OxiGdalError::allocation_error(e.to_string()))?;

        unsafe {
            let ptr = std::alloc::alloc(layout);
            if ptr.is_null() {
                return Err(OxiGdalError::allocation_error(
                    "Standard allocation failed".to_string(),
                ));
            }
            Ok(ptr)
        }
    }

    /// Deallocate memory
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - `ptr` was allocated by this allocator
    /// - `size` matches the original allocation size
    /// - `ptr` has not been deallocated previously
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    pub fn deallocate(&self, ptr: *mut u8, size: usize) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            let page_size = self.config.page_size.bytes();
            let aligned_size = ((size + page_size - 1) / page_size) * page_size;

            unsafe {
                libc::munmap(ptr as *mut libc::c_void, aligned_size);
            }

            return Ok(());
        }

        #[cfg(not(target_os = "linux"))]
        {
            unsafe {
                let layout =
                    std::alloc::Layout::from_size_align_unchecked(size, STANDARD_PAGE_SIZE);
                std::alloc::dealloc(ptr, layout);
            }
            Ok(())
        }
    }

    /// Promote existing allocation to huge pages
    ///
    /// # Safety
    ///
    /// The caller must ensure `ptr` points to a valid allocation of at least `size` bytes.
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    pub fn promote(&self, ptr: *mut u8, size: usize) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            if self.config.use_transparent {
                let result =
                    unsafe { libc::madvise(ptr as *mut libc::c_void, size, libc::MADV_HUGEPAGE) };

                if result == 0 {
                    self.stats.record_promotion();
                    Ok(())
                } else {
                    Err(OxiGdalError::io_error(
                        "Failed to promote to huge pages".to_string(),
                    ))
                }
            } else {
                Err(OxiGdalError::not_supported(
                    "Promotion requires transparent huge pages".to_string(),
                ))
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            let _ = (ptr, size);
            Err(OxiGdalError::not_supported(
                "Promotion not supported on this platform".to_string(),
            ))
        }
    }

    /// Get statistics
    #[must_use]
    pub fn stats(&self) -> Arc<HugePageStats> {
        Arc::clone(&self.stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_huge_pages_detection() {
        let available = is_huge_pages_available();
        println!("Huge pages available: {}", available);
    }

    #[test]
    fn test_huge_page_allocator() {
        let allocator =
            HugePageAllocator::new().expect("Failed to create HugePageAllocator in test");
        let size = HUGE_PAGE_2MB;

        let ptr = allocator
            .allocate(size)
            .expect("Failed to allocate 2MB huge page in test");
        assert!(!ptr.is_null());

        allocator
            .deallocate(ptr, size)
            .expect("Failed to deallocate huge page in test");
    }

    #[test]
    fn test_fallback() {
        let config = HugePageConfig::new()
            .with_fallback(true)
            .with_transparent(true);

        let allocator = HugePageAllocator::with_config(config)
            .expect("Failed to create HugePageAllocator with config in test");

        // Small allocation should work even if huge pages fail
        let ptr = allocator
            .allocate(4096)
            .expect("Failed to allocate 4096 bytes in fallback test");
        assert!(!ptr.is_null());

        allocator
            .deallocate(ptr, 4096)
            .expect("Failed to deallocate memory in fallback test");
    }

    #[test]
    fn test_huge_page_stats() {
        let stats = HugePageStats::new();

        stats.record_huge_page(HUGE_PAGE_2MB);
        stats.record_huge_page(HUGE_PAGE_2MB);
        stats.record_fallback(4096);

        assert_eq!(stats.huge_page_allocations.load(Ordering::Relaxed), 2);
        assert_eq!(stats.fallback_allocations.load(Ordering::Relaxed), 1);
        assert_eq!(stats.total_allocations(), 3);
        assert!((stats.success_rate() - 66.66).abs() < 0.1);
    }
}
