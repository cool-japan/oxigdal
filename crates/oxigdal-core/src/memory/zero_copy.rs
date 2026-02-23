//! Zero-Copy Data Transfers
//!
//! This module provides zero-copy buffer sharing and transfers:
//! - Buffer sharing between operations
//! - GPU-CPU zero-copy transfers (pinned memory)
//! - Reference-counted buffers
//! - Copy-on-write semantics

// Unsafe code is necessary for zero-copy operations
#![allow(unsafe_code)]

use crate::error::{OxiGdalError, Result};
use std::ops::Deref;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Configuration for zero-copy buffers
#[derive(Debug, Clone)]
pub struct ZeroCopyConfig {
    /// Use pinned memory for GPU transfers
    pub use_pinned_memory: bool,
    /// Enable copy-on-write semantics
    pub enable_cow: bool,
    /// Alignment requirement
    pub alignment: usize,
    /// Enable statistics tracking
    pub track_stats: bool,
}

impl Default for ZeroCopyConfig {
    fn default() -> Self {
        Self {
            use_pinned_memory: false,
            enable_cow: true,
            alignment: 64,
            track_stats: true,
        }
    }
}

impl ZeroCopyConfig {
    /// Create new configuration
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable pinned memory
    #[must_use]
    pub fn with_pinned_memory(mut self, enable: bool) -> Self {
        self.use_pinned_memory = enable;
        self
    }

    /// Enable copy-on-write
    #[must_use]
    pub fn with_cow(mut self, enable: bool) -> Self {
        self.enable_cow = enable;
        self
    }

    /// Set alignment
    #[must_use]
    pub fn with_alignment(mut self, alignment: usize) -> Self {
        self.alignment = alignment;
        self
    }

    /// Enable statistics
    #[must_use]
    pub fn with_stats(mut self, enable: bool) -> Self {
        self.track_stats = enable;
        self
    }
}

/// Reference-counted buffer with zero-copy semantics
pub struct SharedBuffer {
    /// Pointer to data
    ptr: NonNull<u8>,
    /// Length in bytes
    len: usize,
    /// Capacity in bytes
    capacity: usize,
    /// Reference count
    ref_count: Arc<AtomicUsize>,
    /// Whether buffer is pinned
    is_pinned: bool,
    /// Configuration
    config: ZeroCopyConfig,
}

impl SharedBuffer {
    /// Create a new shared buffer
    pub fn new(size: usize) -> Result<Self> {
        Self::with_config(size, ZeroCopyConfig::default())
    }

    /// Create a new shared buffer with configuration
    pub fn with_config(size: usize, config: ZeroCopyConfig) -> Result<Self> {
        if size == 0 {
            return Err(OxiGdalError::invalid_parameter(
                "parameter",
                "Buffer size must be non-zero".to_string(),
            ));
        }

        let layout = std::alloc::Layout::from_size_align(size, config.alignment)
            .map_err(|e| OxiGdalError::allocation_error(e.to_string()))?;

        let ptr = if config.use_pinned_memory {
            Self::allocate_pinned(layout)?
        } else {
            unsafe {
                let raw_ptr = std::alloc::alloc(layout);
                if raw_ptr.is_null() {
                    return Err(OxiGdalError::allocation_error(
                        "Failed to allocate buffer".to_string(),
                    ));
                }
                NonNull::new_unchecked(raw_ptr)
            }
        };

        Ok(Self {
            ptr,
            len: size,
            capacity: size,
            ref_count: Arc::new(AtomicUsize::new(1)),
            is_pinned: config.use_pinned_memory,
            config,
        })
    }

    /// Allocate pinned memory for GPU transfers
    ///
    /// # Safety
    ///
    /// Uses unsafe allocation. Pinned memory is allocated differently on different platforms.
    #[allow(unsafe_code)]
    fn allocate_pinned(layout: std::alloc::Layout) -> Result<NonNull<u8>> {
        #[cfg(target_os = "linux")]
        {
            // On Linux, use mmap with MAP_LOCKED
            let ptr = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    layout.size(),
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_LOCKED,
                    -1,
                    0,
                )
            };

            if ptr == libc::MAP_FAILED {
                return Err(OxiGdalError::allocation_error(
                    "Failed to allocate pinned memory".to_string(),
                ));
            }

            NonNull::new(ptr as *mut u8)
                .ok_or_else(|| OxiGdalError::allocation_error("mmap returned null".to_string()))
        }

        #[cfg(not(target_os = "linux"))]
        {
            // Fallback to regular allocation
            unsafe {
                let raw_ptr = std::alloc::alloc(layout);
                if raw_ptr.is_null() {
                    return Err(OxiGdalError::allocation_error(
                        "Failed to allocate buffer".to_string(),
                    ));
                }
                Ok(NonNull::new_unchecked(raw_ptr))
            }
        }
    }

    /// Create a shared reference to this buffer
    #[must_use]
    pub fn share(&self) -> Self {
        self.ref_count.fetch_add(1, Ordering::Relaxed);
        Self {
            ptr: self.ptr,
            len: self.len,
            capacity: self.capacity,
            ref_count: Arc::clone(&self.ref_count),
            is_pinned: self.is_pinned,
            config: self.config.clone(),
        }
    }

    /// Get current reference count
    #[must_use]
    pub fn ref_count(&self) -> usize {
        self.ref_count.load(Ordering::Relaxed)
    }

    /// Check if this is the only reference
    #[must_use]
    pub fn is_unique(&self) -> bool {
        self.ref_count() == 1
    }

    /// Get length
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Check if empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Get capacity
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Check if buffer is pinned
    #[must_use]
    pub fn is_pinned(&self) -> bool {
        self.is_pinned
    }

    /// Get a slice of the buffer
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    /// Get a mutable slice (requires unique ownership or COW)
    pub fn as_mut_slice(&mut self) -> Result<&mut [u8]> {
        if !self.is_unique() {
            if self.config.enable_cow {
                self.make_unique()?;
            } else {
                return Err(OxiGdalError::invalid_operation(
                    "Cannot mutate shared buffer without COW".to_string(),
                ));
            }
        }

        Ok(unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) })
    }

    /// Make this buffer unique by copying if necessary (COW)
    fn make_unique(&mut self) -> Result<()> {
        if self.is_unique() {
            return Ok(());
        }

        // Allocate new buffer
        let layout = std::alloc::Layout::from_size_align(self.capacity, self.config.alignment)
            .map_err(|e| OxiGdalError::allocation_error(e.to_string()))?;

        let new_ptr = if self.is_pinned {
            Self::allocate_pinned(layout)?
        } else {
            unsafe {
                let raw_ptr = std::alloc::alloc(layout);
                if raw_ptr.is_null() {
                    return Err(OxiGdalError::allocation_error(
                        "Failed to allocate buffer for COW".to_string(),
                    ));
                }
                NonNull::new_unchecked(raw_ptr)
            }
        };

        // Copy data
        unsafe {
            std::ptr::copy_nonoverlapping(self.ptr.as_ptr(), new_ptr.as_ptr(), self.len);
        }

        // Update reference count
        self.ref_count.fetch_sub(1, Ordering::Relaxed);

        // Update self to use new buffer
        self.ptr = new_ptr;
        self.ref_count = Arc::new(AtomicUsize::new(1));

        Ok(())
    }

    /// Clone the buffer data (explicit copy)
    pub fn clone_data(&self) -> Result<Self> {
        let new_buffer = Self::with_config(self.len, self.config.clone())?;
        unsafe {
            std::ptr::copy_nonoverlapping(self.ptr.as_ptr(), new_buffer.ptr.as_ptr(), self.len);
        }
        Ok(new_buffer)
    }

    /// Get a typed slice view
    pub fn as_typed_slice<T: bytemuck::Pod>(&self) -> Result<&[T]> {
        if self.len % std::mem::size_of::<T>() != 0 {
            return Err(OxiGdalError::invalid_parameter(
                "parameter",
                "Buffer size not aligned to type size".to_string(),
            ));
        }

        let count = self.len / std::mem::size_of::<T>();
        Ok(unsafe { std::slice::from_raw_parts(self.ptr.as_ptr() as *const T, count) })
    }

    /// Get a typed mutable slice view
    pub fn as_typed_mut_slice<T: bytemuck::Pod>(&mut self) -> Result<&mut [T]> {
        if !self.is_unique() {
            if self.config.enable_cow {
                self.make_unique()?;
            } else {
                return Err(OxiGdalError::invalid_operation(
                    "Cannot mutate shared buffer without COW".to_string(),
                ));
            }
        }

        if self.len % std::mem::size_of::<T>() != 0 {
            return Err(OxiGdalError::invalid_parameter(
                "parameter",
                "Buffer size not aligned to type size".to_string(),
            ));
        }

        let count = self.len / std::mem::size_of::<T>();
        Ok(unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr().cast::<T>(), count) })
    }
}

impl Clone for SharedBuffer {
    fn clone(&self) -> Self {
        self.share()
    }
}

impl Deref for SharedBuffer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl AsRef<[u8]> for SharedBuffer {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl Drop for SharedBuffer {
    fn drop(&mut self) {
        let count = self.ref_count.fetch_sub(1, Ordering::Relaxed);
        if count == 1 {
            // Last reference, deallocate
            unsafe {
                if self.is_pinned {
                    #[cfg(target_os = "linux")]
                    {
                        libc::munmap(self.ptr.as_ptr() as *mut libc::c_void, self.capacity);
                    }
                    #[cfg(not(target_os = "linux"))]
                    {
                        let layout = std::alloc::Layout::from_size_align_unchecked(
                            self.capacity,
                            self.config.alignment,
                        );
                        std::alloc::dealloc(self.ptr.as_ptr(), layout);
                    }
                } else {
                    let layout = std::alloc::Layout::from_size_align_unchecked(
                        self.capacity,
                        self.config.alignment,
                    );
                    std::alloc::dealloc(self.ptr.as_ptr(), layout);
                }
            }
        }
    }
}

// Safety: SharedBuffer can be sent between threads
unsafe impl Send for SharedBuffer {}
unsafe impl Sync for SharedBuffer {}

/// Zero-copy buffer wrapper
pub struct ZeroCopyBuffer<T: bytemuck::Pod> {
    /// Underlying shared buffer
    buffer: SharedBuffer,
    /// Phantom data for type
    _phantom: std::marker::PhantomData<T>,
}

impl<T: bytemuck::Pod> ZeroCopyBuffer<T> {
    /// Create a new zero-copy buffer
    pub fn new(count: usize) -> Result<Self> {
        let size = count * std::mem::size_of::<T>();
        let buffer = SharedBuffer::new(size)?;
        Ok(Self {
            buffer,
            _phantom: std::marker::PhantomData,
        })
    }

    /// Create with configuration
    pub fn with_config(count: usize, config: ZeroCopyConfig) -> Result<Self> {
        let size = count * std::mem::size_of::<T>();
        let buffer = SharedBuffer::with_config(size, config)?;
        Ok(Self {
            buffer,
            _phantom: std::marker::PhantomData,
        })
    }

    /// Create from existing buffer
    pub fn from_buffer(buffer: SharedBuffer) -> Result<Self> {
        if buffer.len() % std::mem::size_of::<T>() != 0 {
            return Err(OxiGdalError::invalid_parameter(
                "parameter",
                "Buffer size not aligned to type size".to_string(),
            ));
        }

        Ok(Self {
            buffer,
            _phantom: std::marker::PhantomData,
        })
    }

    /// Get length in elements
    #[must_use]
    pub fn len(&self) -> usize {
        self.buffer.len() / std::mem::size_of::<T>()
    }

    /// Check if empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get as slice
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        // SAFETY: Type alignment is verified in constructor (new, with_config, from_buffer).
        // The buffer size is always a multiple of std::mem::size_of::<T>(), so this
        // conversion is safe. bytemuck::Pod ensures T is safe to read from raw bytes.
        let count = self.buffer.len() / std::mem::size_of::<T>();
        unsafe { std::slice::from_raw_parts(self.buffer.ptr.as_ptr() as *const T, count) }
    }

    /// Get as mutable slice
    pub fn as_mut_slice(&mut self) -> Result<&mut [T]> {
        self.buffer.as_typed_mut_slice()
    }

    /// Share the buffer
    #[must_use]
    pub fn share(&self) -> Self {
        Self {
            buffer: self.buffer.share(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Check if unique
    #[must_use]
    pub fn is_unique(&self) -> bool {
        self.buffer.is_unique()
    }

    /// Get reference count
    #[must_use]
    pub fn ref_count(&self) -> usize {
        self.buffer.ref_count()
    }

    /// Clone the buffer data
    pub fn clone_data(&self) -> Result<Self> {
        Ok(Self {
            buffer: self.buffer.clone_data()?,
            _phantom: std::marker::PhantomData,
        })
    }
}

impl<T: bytemuck::Pod> Clone for ZeroCopyBuffer<T> {
    fn clone(&self) -> Self {
        self.share()
    }
}

impl<T: bytemuck::Pod> Deref for ZeroCopyBuffer<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T: bytemuck::Pod> AsRef<[T]> for ZeroCopyBuffer<T> {
    fn as_ref(&self) -> &[T] {
        self.as_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shared_buffer() {
        let buffer = SharedBuffer::new(1024).expect("Failed to create shared buffer");
        assert_eq!(buffer.len(), 1024);
        assert_eq!(buffer.ref_count(), 1);
        assert!(buffer.is_unique());

        let shared = buffer.share();
        assert_eq!(buffer.ref_count(), 2);
        assert_eq!(shared.ref_count(), 2);
        assert!(!buffer.is_unique());
        assert!(!shared.is_unique());
    }

    #[test]
    fn test_copy_on_write() {
        let mut buffer = SharedBuffer::new(1024).expect("Failed to create shared buffer");
        let shared = buffer.share();

        assert_eq!(buffer.ref_count(), 2);

        // This should trigger COW
        let slice = buffer
            .as_mut_slice()
            .expect("Failed to get mutable slice (COW should trigger)");
        slice[0] = 42;

        assert_eq!(buffer.ref_count(), 1);
        assert_eq!(shared.ref_count(), 1);
        assert_eq!(buffer.as_slice()[0], 42);
        assert_eq!(shared.as_slice()[0], 0);
    }

    #[test]
    fn test_zero_copy_buffer() {
        let buffer: ZeroCopyBuffer<u32> =
            ZeroCopyBuffer::new(256).expect("Failed to create zero-copy buffer");
        assert_eq!(buffer.len(), 256);
        assert_eq!(buffer.ref_count(), 1);

        let shared = buffer.share();
        assert_eq!(buffer.ref_count(), 2);
        assert_eq!(shared.ref_count(), 2);
    }

    #[test]
    fn test_typed_slice() {
        let buffer = SharedBuffer::new(1024).expect("Failed to create shared buffer");
        let slice: &[u32] = buffer
            .as_typed_slice()
            .expect("Failed to create typed slice from buffer");
        assert_eq!(slice.len(), 256);
    }

    #[test]
    fn test_clone_data() {
        let buffer = SharedBuffer::new(1024).expect("Failed to create shared buffer");
        let cloned = buffer.clone_data().expect("Failed to clone buffer data");

        assert_eq!(buffer.len(), cloned.len());
        assert_eq!(buffer.ref_count(), 1);
        assert_eq!(cloned.ref_count(), 1);
    }
}
