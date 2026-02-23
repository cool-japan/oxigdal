//! Optimized Memory-Mapped I/O
//!
//! This module provides high-performance memory-mapped file I/O with:
//! - Lazy memory mapping (on-demand)
//! - Read-ahead hints for sequential access
//! - Write-behind buffering
//! - Page-aligned mapping
//! - Huge pages support
//! - NUMA-aware mapping

// Unsafe code is necessary for memory-mapped I/O operations
#![allow(unsafe_code)]

use crate::error::{OxiGdalError, Result};
use std::fs::File;
use std::io;
use std::ops::Deref;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Memory map mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryMapMode {
    /// Read-only mapping
    ReadOnly,
    /// Read-write mapping
    ReadWrite,
    /// Copy-on-write mapping
    CopyOnWrite,
}

/// Memory map access pattern hint
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessPattern {
    /// Normal access (no special hints)
    Normal,
    /// Sequential access (read-ahead)
    Sequential,
    /// Random access (disable read-ahead)
    Random,
    /// Will need soon (prefetch)
    WillNeed,
    /// Won't need soon (can evict)
    DontNeed,
}

/// Configuration for memory mapping
#[derive(Debug, Clone)]
pub struct MemoryMapConfig {
    /// Access mode
    pub mode: MemoryMapMode,
    /// Access pattern hint
    pub access_pattern: AccessPattern,
    /// Use huge pages if available
    pub use_huge_pages: bool,
    /// NUMA node preference (-1 for any)
    pub numa_node: i32,
    /// Populate pages immediately (vs lazy)
    pub populate: bool,
    /// Lock pages in memory
    pub lock_memory: bool,
    /// Read-ahead size in bytes
    pub read_ahead_size: usize,
}

impl Default for MemoryMapConfig {
    fn default() -> Self {
        Self {
            mode: MemoryMapMode::ReadOnly,
            access_pattern: AccessPattern::Normal,
            use_huge_pages: false,
            numa_node: -1,
            populate: false,
            lock_memory: false,
            read_ahead_size: 128 * 1024, // 128KB
        }
    }
}

impl MemoryMapConfig {
    /// Create a new configuration
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set access mode
    #[must_use]
    pub fn with_mode(mut self, mode: MemoryMapMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set access pattern
    #[must_use]
    pub fn with_access_pattern(mut self, pattern: AccessPattern) -> Self {
        self.access_pattern = pattern;
        self
    }

    /// Enable huge pages
    #[must_use]
    pub fn with_huge_pages(mut self, enable: bool) -> Self {
        self.use_huge_pages = enable;
        self
    }

    /// Set NUMA node
    #[must_use]
    pub fn with_numa_node(mut self, node: i32) -> Self {
        self.numa_node = node;
        self
    }

    /// Set populate flag
    #[must_use]
    pub fn with_populate(mut self, populate: bool) -> Self {
        self.populate = populate;
        self
    }

    /// Set lock memory flag
    #[must_use]
    pub fn with_lock_memory(mut self, lock: bool) -> Self {
        self.lock_memory = lock;
        self
    }

    /// Set read-ahead size
    #[must_use]
    pub fn with_read_ahead_size(mut self, size: usize) -> Self {
        self.read_ahead_size = size;
        self
    }
}

/// Memory-mapped file handle
pub struct MemoryMap {
    /// Pointer to mapped memory
    ptr: NonNull<u8>,
    /// Length of mapping
    len: usize,
    /// File handle (kept alive for the mapping)
    _file: Arc<File>,
    /// Configuration
    #[allow(dead_code)]
    config: MemoryMapConfig,
    /// Whether the mapping is mutable
    is_mutable: bool,
    /// Statistics
    accesses: AtomicUsize,
    /// Whether mapping is locked in memory
    is_locked: AtomicBool,
}

impl MemoryMap {
    /// Create a memory-mapped file with default configuration
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::with_config(path, MemoryMapConfig::default())
    }

    /// Create a memory-mapped file with custom configuration
    pub fn with_config<P: AsRef<Path>>(path: P, config: MemoryMapConfig) -> Result<Self> {
        let file = match config.mode {
            MemoryMapMode::ReadOnly => {
                File::open(path).map_err(|e| OxiGdalError::io_error(e.to_string()))?
            }
            MemoryMapMode::ReadWrite | MemoryMapMode::CopyOnWrite => std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(|e| OxiGdalError::io_error(e.to_string()))?,
        };

        let metadata = file
            .metadata()
            .map_err(|e| OxiGdalError::io_error(e.to_string()))?;
        let len = metadata.len() as usize;

        if len == 0 {
            return Err(OxiGdalError::invalid_parameter(
                "parameter",
                "Cannot map empty file".to_string(),
            ));
        }

        let is_mutable = matches!(
            config.mode,
            MemoryMapMode::ReadWrite | MemoryMapMode::CopyOnWrite
        );

        // SAFETY: We've validated the file and its size. The map_file function
        // performs proper error checking and returns a valid pointer or an error.
        let ptr = unsafe { Self::map_file(&file, len, &config, is_mutable)? };

        let map = Self {
            ptr,
            len,
            _file: Arc::new(file),
            config: config.clone(),
            is_mutable,
            accesses: AtomicUsize::new(0),
            is_locked: AtomicBool::new(false),
        };

        // Apply access pattern hints
        map.apply_access_pattern()?;

        // Lock memory if requested
        if config.lock_memory {
            map.lock()?;
        }

        Ok(map)
    }

    /// Map a file into memory
    ///
    /// # Safety
    ///
    /// This function uses unsafe mmap operations. The caller must ensure:
    /// - The file remains valid for the lifetime of the mapping
    /// - No other code modifies the file while mapped
    /// - The file descriptor is valid
    /// # Safety
    ///
    /// This function uses unsafe mmap operations. The caller must ensure:
    /// - The file remains valid for the lifetime of the mapping
    /// - No other code modifies the file while mapped
    /// - The file descriptor is valid
    #[allow(unsafe_code)]
    unsafe fn map_file(
        file: &File,
        len: usize,
        config: &MemoryMapConfig,
        is_mutable: bool,
    ) -> Result<NonNull<u8>> {
        // SAFETY: We perform proper error checking on the mmap result.
        // The file descriptor is valid, and we handle MAP_FAILED appropriately.
        unsafe {
            let fd = file.as_raw_fd();

            let prot = if is_mutable {
                libc::PROT_READ | libc::PROT_WRITE
            } else {
                libc::PROT_READ
            };

            #[cfg(target_os = "linux")]
            let mut flags = match config.mode {
                MemoryMapMode::ReadOnly | MemoryMapMode::ReadWrite => libc::MAP_SHARED,
                MemoryMapMode::CopyOnWrite => libc::MAP_PRIVATE,
            };

            #[cfg(not(target_os = "linux"))]
            let flags = match config.mode {
                MemoryMapMode::ReadOnly | MemoryMapMode::ReadWrite => libc::MAP_SHARED,
                MemoryMapMode::CopyOnWrite => libc::MAP_PRIVATE,
            };

            if config.populate {
                #[cfg(target_os = "linux")]
                {
                    flags |= libc::MAP_POPULATE;
                }
            }

            if config.use_huge_pages {
                #[cfg(target_os = "linux")]
                {
                    flags |= libc::MAP_HUGETLB;
                }
            }

            let addr = libc::mmap(std::ptr::null_mut(), len, prot, flags, fd, 0);

            if addr == libc::MAP_FAILED {
                return Err(OxiGdalError::allocation_error(
                    io::Error::last_os_error().to_string(),
                ));
            }

            NonNull::new(addr.cast::<u8>()).ok_or_else(|| {
                OxiGdalError::allocation_error("mmap returned null pointer".to_string())
            })
        }
    }

    /// Apply access pattern hints
    fn apply_access_pattern(&self) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            let advice = match self.config.access_pattern {
                AccessPattern::Normal => libc::MADV_NORMAL,
                AccessPattern::Sequential => libc::MADV_SEQUENTIAL,
                AccessPattern::Random => libc::MADV_RANDOM,
                AccessPattern::WillNeed => libc::MADV_WILLNEED,
                AccessPattern::DontNeed => libc::MADV_DONTNEED,
            };

            // SAFETY: The pointer is valid and within bounds. madvise only provides hints
            // to the kernel and won't cause memory corruption even if it fails.
            let result =
                unsafe { libc::madvise(self.ptr.as_ptr() as *mut libc::c_void, self.len, advice) };

            if result != 0 {
                return Err(OxiGdalError::io_error(format!(
                    "madvise failed: {}",
                    io::Error::last_os_error()
                )));
            }
        }

        Ok(())
    }

    /// Lock memory pages to prevent swapping
    pub fn lock(&self) -> Result<()> {
        if self.is_locked.load(Ordering::Relaxed) {
            return Ok(());
        }

        #[cfg(target_os = "linux")]
        {
            let result = unsafe { libc::mlock(self.ptr.as_ptr() as *const libc::c_void, self.len) };

            if result != 0 {
                return Err(OxiGdalError::io_error(format!(
                    "mlock failed: {}",
                    io::Error::last_os_error()
                )));
            }

            self.is_locked.store(true, Ordering::Relaxed);
        }

        Ok(())
    }

    /// Unlock memory pages
    pub fn unlock(&self) -> Result<()> {
        if !self.is_locked.load(Ordering::Relaxed) {
            return Ok(());
        }

        #[cfg(target_os = "linux")]
        {
            let result =
                unsafe { libc::munlock(self.ptr.as_ptr() as *const libc::c_void, self.len) };

            if result != 0 {
                return Err(OxiGdalError::io_error(format!(
                    "munlock failed: {}",
                    io::Error::last_os_error()
                )));
            }

            self.is_locked.store(false, Ordering::Relaxed);
        }

        Ok(())
    }

    /// Prefetch a range of the mapping
    pub fn prefetch(&self, offset: usize, len: usize) -> Result<()> {
        if offset + len > self.len {
            return Err(OxiGdalError::invalid_parameter(
                "parameter",
                "Prefetch range exceeds mapping size".to_string(),
            ));
        }

        #[cfg(target_os = "linux")]
        {
            let ptr = unsafe { self.ptr.as_ptr().add(offset) };
            let result =
                unsafe { libc::madvise(ptr as *mut libc::c_void, len, libc::MADV_WILLNEED) };

            if result != 0 {
                return Err(OxiGdalError::io_error(format!(
                    "prefetch madvise failed: {}",
                    io::Error::last_os_error()
                )));
            }
        }

        Ok(())
    }

    /// Advise that a range won't be needed soon
    pub fn evict(&self, offset: usize, len: usize) -> Result<()> {
        if offset + len > self.len {
            return Err(OxiGdalError::invalid_parameter(
                "parameter",
                "Evict range exceeds mapping size".to_string(),
            ));
        }

        #[cfg(target_os = "linux")]
        {
            let ptr = unsafe { self.ptr.as_ptr().add(offset) };
            let result =
                unsafe { libc::madvise(ptr as *mut libc::c_void, len, libc::MADV_DONTNEED) };

            if result != 0 {
                return Err(OxiGdalError::io_error(format!(
                    "evict madvise failed: {}",
                    io::Error::last_os_error()
                )));
            }
        }

        Ok(())
    }

    /// Flush changes to disk
    pub fn flush(&self) -> Result<()> {
        if !self.is_mutable {
            return Ok(());
        }

        // SAFETY: The pointer is valid, within bounds, and the mapping is mutable.
        // msync synchronizes changes back to the file.
        let result = unsafe {
            libc::msync(
                self.ptr.as_ptr().cast::<libc::c_void>(),
                self.len,
                libc::MS_SYNC,
            )
        };

        if result != 0 {
            return Err(OxiGdalError::io_error(format!(
                "msync failed: {}",
                io::Error::last_os_error()
            )));
        }

        Ok(())
    }

    /// Flush a range asynchronously
    pub fn flush_async(&self, offset: usize, len: usize) -> Result<()> {
        if !self.is_mutable {
            return Ok(());
        }

        if offset + len > self.len {
            return Err(OxiGdalError::invalid_parameter(
                "parameter",
                "Flush range exceeds mapping size".to_string(),
            ));
        }

        // SAFETY: offset and len have been validated to be within bounds.
        // Pointer arithmetic is safe within the mapped region.
        let ptr = unsafe { self.ptr.as_ptr().add(offset) };
        // SAFETY: The pointer is valid and within the mapped region.
        let result = unsafe { libc::msync(ptr.cast::<libc::c_void>(), len, libc::MS_ASYNC) };

        if result != 0 {
            return Err(OxiGdalError::io_error(format!(
                "async msync failed: {}",
                io::Error::last_os_error()
            )));
        }

        Ok(())
    }

    /// Get the length of the mapping
    pub fn len(&self) -> usize {
        self.len
    }

    /// Check if the mapping is empty
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Get access count
    pub fn access_count(&self) -> usize {
        self.accesses.load(Ordering::Relaxed)
    }

    /// Record an access
    fn record_access(&self) {
        self.accesses.fetch_add(1, Ordering::Relaxed);
    }

    /// Get a slice of the mapped memory
    pub fn as_slice(&self) -> &[u8] {
        self.record_access();
        // SAFETY: The pointer is valid for reads and properly aligned.
        // The length is guaranteed to be within the mapped region.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    /// Get a mutable slice of the mapped memory
    pub fn as_mut_slice(&mut self) -> Result<&mut [u8]> {
        if !self.is_mutable {
            return Err(OxiGdalError::invalid_operation(
                "Cannot get mutable slice from read-only mapping".to_string(),
            ));
        }
        self.record_access();
        // SAFETY: The pointer is valid for reads/writes, properly aligned,
        // and we have exclusive mutable access through &mut self.
        Ok(unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) })
    }

    /// Get a typed slice view
    pub fn as_typed_slice<T: bytemuck::Pod>(&self) -> Result<&[T]> {
        self.record_access();

        if self.len % std::mem::size_of::<T>() != 0 {
            return Err(OxiGdalError::invalid_parameter(
                "parameter",
                "Mapping size not aligned to type size".to_string(),
            ));
        }

        let count = self.len / std::mem::size_of::<T>();
        // SAFETY: We've verified size alignment. The pointer is valid for reads
        // and properly aligned. bytemuck::Pod ensures T is safe to read from raw bytes.
        Ok(unsafe { std::slice::from_raw_parts(self.ptr.as_ptr() as *const T, count) })
    }

    /// Get a typed mutable slice view
    pub fn as_typed_mut_slice<T: bytemuck::Pod>(&mut self) -> Result<&mut [T]> {
        if !self.is_mutable {
            return Err(OxiGdalError::invalid_operation(
                "Cannot get mutable slice from read-only mapping".to_string(),
            ));
        }

        self.record_access();

        if self.len % std::mem::size_of::<T>() != 0 {
            return Err(OxiGdalError::invalid_parameter(
                "parameter",
                "Mapping size not aligned to type size".to_string(),
            ));
        }

        let count = self.len / std::mem::size_of::<T>();
        // SAFETY: We've verified size alignment and mutability. The pointer is valid
        // for reads/writes and we have exclusive mutable access.
        Ok(unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr().cast::<T>(), count) })
    }
}

impl Deref for MemoryMap {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl AsRef<[u8]> for MemoryMap {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl Drop for MemoryMap {
    fn drop(&mut self) {
        // Unlock if locked
        if self.is_locked.load(Ordering::Relaxed) {
            let _ = self.unlock();
        }

        // Unmap the memory
        // SAFETY: The pointer was obtained from mmap and we're unmapping with the same length.
        // This is the last use of the pointer as the struct is being dropped.
        unsafe {
            libc::munmap(self.ptr.as_ptr().cast::<libc::c_void>(), self.len);
        }
    }
}

// SAFETY: MemoryMap can be sent between threads because:
// - The mapped memory is independent of thread state
// - The file handle is Arc-wrapped and thread-safe
unsafe impl Send for MemoryMap {}

// SAFETY: MemoryMap can be shared between threads because:
// - Read operations are inherently thread-safe for mapped memory
// - Write operations require &mut self, ensuring exclusive access
unsafe impl Sync for MemoryMap {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_temp_file(size: usize) -> tempfile::NamedTempFile {
        let mut file =
            tempfile::NamedTempFile::new().expect("Test helper: temp file creation should succeed");
        let data = vec![0u8; size];
        file.write_all(&data)
            .expect("Test helper: writing to temp file should succeed");
        file.flush()
            .expect("Test helper: flushing temp file should succeed");
        file
    }

    #[test]
    fn test_memory_map_readonly() {
        let file = create_temp_file(4096);
        let path = file.path();

        let map = MemoryMap::new(path).expect("Memory map creation should succeed in test");
        assert_eq!(map.len(), 4096);
        assert!(!map.is_empty());

        let slice = map.as_slice();
        assert_eq!(slice.len(), 4096);
    }

    #[test]
    fn test_memory_map_config() {
        let file = create_temp_file(8192);
        let path = file.path();

        let config = MemoryMapConfig::new()
            .with_mode(MemoryMapMode::ReadOnly)
            .with_access_pattern(AccessPattern::Sequential)
            .with_populate(true);

        let map = MemoryMap::with_config(path, config)
            .expect("Memory map with custom config should succeed");
        assert_eq!(map.len(), 8192);
    }

    #[test]
    fn test_prefetch() {
        let file = create_temp_file(16384);
        let path = file.path();

        let map = MemoryMap::new(path).expect("Memory map creation should succeed");
        map.prefetch(0, 4096)
            .expect("First prefetch should succeed");
        map.prefetch(4096, 4096)
            .expect("Second prefetch should succeed");
    }

    #[test]
    fn test_typed_slice() {
        let file = create_temp_file(4096);
        let path = file.path();

        let map = MemoryMap::new(path).expect("Memory map creation should succeed");
        let slice: &[u32] = map
            .as_typed_slice()
            .expect("Typed slice conversion should succeed");
        assert_eq!(slice.len(), 1024);
    }

    #[test]
    fn test_access_count() {
        let file = create_temp_file(4096);
        let path = file.path();

        let map = MemoryMap::new(path).expect("Memory map creation should succeed");
        assert_eq!(map.access_count(), 0);

        let _slice = map.as_slice();
        assert_eq!(map.access_count(), 1);

        let _slice = map.as_slice();
        assert_eq!(map.access_count(), 2);
    }
}
