//! SIMD-aligned buffer management for high-performance raster operations
//!
//! This module provides buffers with guaranteed alignment for efficient SIMD operations.
//! It ensures that data is properly aligned for AVX-512 (64 bytes), AVX2 (32 bytes),
//! or SSE2/NEON (16 bytes) instructions.
//!
//! # Features
//!
//! - **Configurable Alignment**: Support for 16, 32, and 64-byte alignment
//! - **Zero-Copy Views**: Create strided views without copying data
//! - **Tiled Access**: Cache-friendly tiled iteration for large rasters
//! - **Type-Safe**: Generic over element types with proper trait bounds

// Unsafe code is necessary for aligned memory allocation and SIMD operations
#![allow(unsafe_code)]

//! # Example
//!
//! ```rust
//! use oxigdal_core::simd_buffer::AlignedBuffer;
//! use oxigdal_core::error::Result;
//!
//! # fn main() -> Result<()> {
//! // Create a 64-byte aligned buffer for f32 data
//! let mut buffer = AlignedBuffer::<f32>::new(1000, 64)?;
//!
//! // Fill with data
//! for (i, val) in buffer.as_mut_slice().iter_mut().enumerate() {
//!     *val = i as f32;
//! }
//!
//! // Access as slice
//! let sum: f32 = buffer.as_slice().iter().sum();
//! assert_eq!(sum, 499500.0);
//! # Ok(())
//! # }
//! ```
//!
//! # Cache-Friendly Tiling
//!
//! For large rasters, tiled iteration improves cache locality:
//!
//! ```rust
//! use oxigdal_core::simd_buffer::TiledBuffer;
//! use oxigdal_core::error::Result;
//!
//! # fn main() -> Result<()> {
//! let buffer: TiledBuffer<f32> = TiledBuffer::new(1024, 1024, 64, 64)?;
//!
//! for tile in buffer.tiles() {
//!     // Process each 64x64 tile independently
//!     // Better cache locality and SIMD-friendly
//! }
//! # Ok(())
//! # }
//! ```

use std::alloc::{self, Layout};
use std::ptr::NonNull;
use std::slice;

use crate::error::{OxiGdalError, Result};

/// A buffer with guaranteed SIMD-friendly alignment
///
/// This buffer ensures that data is aligned to the specified boundary,
/// which is critical for efficient SIMD operations. It uses Rust's
/// global allocator with custom alignment.
pub struct AlignedBuffer<T> {
    /// Pointer to the aligned data
    ptr: NonNull<T>,
    /// Number of elements
    len: usize,
    /// Alignment in bytes
    align: usize,
    /// Layout for deallocation
    layout: Layout,
}

impl<T> AlignedBuffer<T> {
    /// Create a new aligned buffer with the specified capacity and alignment
    ///
    /// # Arguments
    ///
    /// * `capacity` - Number of elements to allocate
    /// * `align` - Alignment in bytes (must be a power of 2)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Alignment is not a power of 2
    /// - Alignment is less than the natural alignment of T
    /// - Memory allocation fails
    pub fn new(capacity: usize, align: usize) -> Result<Self> {
        if !align.is_power_of_two() {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "align",
                message: "Alignment must be a power of 2".to_string(),
            });
        }

        if align < std::mem::align_of::<T>() {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "align",
                message: format!(
                    "Alignment {} is less than natural alignment of {}",
                    align,
                    std::mem::align_of::<T>()
                ),
            });
        }

        if capacity == 0 {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "capacity",
                message: "Capacity must be greater than 0".to_string(),
            });
        }

        let size = capacity
            .checked_mul(std::mem::size_of::<T>())
            .ok_or_else(|| OxiGdalError::InvalidParameter {
                parameter: "capacity",
                message: "Capacity overflow".to_string(),
            })?;

        let layout = Layout::from_size_align(size, align).map_err(|e| OxiGdalError::Internal {
            message: format!("Invalid layout: {e}"),
        })?;

        // Safety: We've validated the layout above
        let ptr = unsafe { alloc::alloc(layout) };

        let ptr = NonNull::new(ptr)
            .ok_or_else(|| OxiGdalError::Internal {
                message: "Failed to allocate aligned buffer".to_string(),
            })?
            .cast::<T>();

        Ok(Self {
            ptr,
            len: capacity,
            align,
            layout,
        })
    }

    /// Create a new aligned buffer filled with zeros
    ///
    /// # Arguments
    ///
    /// * `capacity` - Number of elements to allocate
    /// * `align` - Alignment in bytes (must be a power of 2)
    ///
    /// # Errors
    ///
    /// Returns an error if allocation fails
    pub fn zeros(capacity: usize, align: usize) -> Result<Self>
    where
        T: Default + Copy,
    {
        let buffer = Self::new(capacity, align)?;

        // Safety: The buffer is properly allocated and we have exclusive access
        unsafe {
            std::ptr::write_bytes(buffer.ptr.as_ptr(), 0, capacity);
        }

        Ok(buffer)
    }

    /// Get the number of elements in the buffer
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Check if the buffer is empty
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Get the alignment of the buffer
    #[must_use]
    pub const fn alignment(&self) -> usize {
        self.align
    }

    /// Get a raw pointer to the buffer
    #[must_use]
    pub fn as_ptr(&self) -> *const T {
        self.ptr.as_ptr()
    }

    /// Get a mutable raw pointer to the buffer
    #[must_use]
    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.ptr.as_ptr()
    }

    /// Get the buffer as a slice
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        // Safety: The buffer is properly allocated with `len` elements
        unsafe { slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    /// Get the buffer as a mutable slice
    #[must_use]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        // Safety: The buffer is properly allocated with `len` elements
        unsafe { slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }

    /// Copy data from a slice into the buffer
    ///
    /// # Errors
    ///
    /// Returns an error if the slice length doesn't match the buffer capacity
    pub fn copy_from_slice(&mut self, src: &[T]) -> Result<()>
    where
        T: Copy,
    {
        if src.len() != self.len {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "src",
                message: format!(
                    "Source length {} doesn't match buffer capacity {}",
                    src.len(),
                    self.len
                ),
            });
        }

        self.as_mut_slice().copy_from_slice(src);
        Ok(())
    }

    /// Create a strided view of the buffer
    ///
    /// This is useful for accessing every nth element without copying data.
    ///
    /// # Arguments
    ///
    /// * `stride` - Step size between elements
    ///
    /// # Errors
    ///
    /// Returns an error if stride is 0
    pub fn strided_view(&self, stride: usize) -> Result<StridedView<'_, T>> {
        if stride == 0 {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "stride",
                message: "Stride must be greater than 0".to_string(),
            });
        }

        Ok(StridedView {
            buffer: self.as_slice(),
            stride,
        })
    }
}

impl<T> Drop for AlignedBuffer<T> {
    fn drop(&mut self) {
        // Safety: The pointer was allocated with this layout
        unsafe {
            alloc::dealloc(self.ptr.as_ptr().cast::<u8>(), self.layout);
        }
    }
}

// Safety: AlignedBuffer can be sent to another thread if T can be sent
unsafe impl<T: Send> Send for AlignedBuffer<T> {}

// Safety: AlignedBuffer can be shared between threads if T can be shared
unsafe impl<T: Sync> Sync for AlignedBuffer<T> {}

/// A strided view into a buffer for accessing every nth element
pub struct StridedView<'a, T> {
    buffer: &'a [T],
    stride: usize,
}

impl<T> StridedView<'_, T> {
    /// Get the number of elements in the strided view
    #[must_use]
    pub fn len(&self) -> usize {
        self.buffer.len().div_ceil(self.stride)
    }

    /// Check if the view is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Get an element at the specified index
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&T> {
        let offset = index * self.stride;
        self.buffer.get(offset)
    }

    /// Create an iterator over the strided elements
    #[must_use]
    pub fn iter(&self) -> StridedIterator<'_, T> {
        StridedIterator {
            buffer: self.buffer,
            stride: self.stride,
            index: 0,
        }
    }
}

/// Iterator for strided buffer access
pub struct StridedIterator<'a, T> {
    buffer: &'a [T],
    stride: usize,
    index: usize,
}

impl<'a, T> Iterator for StridedIterator<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        let offset = self.index * self.stride;
        if offset < self.buffer.len() {
            self.index += 1;
            Some(&self.buffer[offset])
        } else {
            None
        }
    }
}

/// A tiled buffer for cache-friendly access patterns
///
/// Large rasters can be divided into tiles for better cache locality.
/// This is especially important for SIMD operations on multi-megabyte datasets.
pub struct TiledBuffer<T> {
    buffer: AlignedBuffer<T>,
    width: usize,
    height: usize,
    tile_width: usize,
    tile_height: usize,
}

impl<T: Default + Copy> TiledBuffer<T> {
    /// Create a new tiled buffer
    ///
    /// # Arguments
    ///
    /// * `width` - Total width in elements
    /// * `height` - Total height in elements
    /// * `tile_width` - Tile width
    /// * `tile_height` - Tile height
    ///
    /// # Errors
    ///
    /// Returns an error if allocation fails or dimensions are invalid
    pub fn new(width: usize, height: usize, tile_width: usize, tile_height: usize) -> Result<Self> {
        if tile_width == 0 || tile_height == 0 {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "tile_size",
                message: "Tile dimensions must be greater than 0".to_string(),
            });
        }

        let capacity = width
            .checked_mul(height)
            .ok_or_else(|| OxiGdalError::Internal {
                message: "Buffer size overflow".to_string(),
            })?;

        let buffer = AlignedBuffer::zeros(capacity, 64)?;

        Ok(Self {
            buffer,
            width,
            height,
            tile_width,
            tile_height,
        })
    }

    /// Get the total width
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Get the total height
    #[must_use]
    pub const fn height(&self) -> usize {
        self.height
    }

    /// Get an iterator over tiles
    #[must_use]
    pub fn tiles(&self) -> TileIterator<'_, T> {
        TileIterator {
            buffer: &self.buffer,
            width: self.width,
            height: self.height,
            tile_width: self.tile_width,
            tile_height: self.tile_height,
            current_x: 0,
            current_y: 0,
        }
    }

    /// Get the underlying buffer
    #[must_use]
    pub const fn buffer(&self) -> &AlignedBuffer<T> {
        &self.buffer
    }
}

/// Iterator over tiles in a tiled buffer
pub struct TileIterator<'a, T> {
    #[allow(dead_code)]
    buffer: &'a AlignedBuffer<T>,
    width: usize,
    height: usize,
    tile_width: usize,
    tile_height: usize,
    current_x: usize,
    current_y: usize,
}

/// A tile from a tiled buffer
pub struct Tile {
    /// X offset in the parent buffer
    pub x: usize,
    /// Y offset in the parent buffer
    pub y: usize,
    /// Tile width
    pub width: usize,
    /// Tile height
    pub height: usize,
}

impl<T> Iterator for TileIterator<'_, T> {
    type Item = Tile;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_y >= self.height {
            return None;
        }

        let tile = Tile {
            x: self.current_x,
            y: self.current_y,
            width: self.tile_width.min(self.width - self.current_x),
            height: self.tile_height.min(self.height - self.current_y),
        };

        // Move to next tile
        self.current_x += self.tile_width;
        if self.current_x >= self.width {
            self.current_x = 0;
            self.current_y += self.tile_height;
        }

        Some(tile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aligned_buffer_creation() {
        let buffer = AlignedBuffer::<f32>::new(100, 64)
            .expect("Failed to create aligned buffer with valid parameters");
        assert_eq!(buffer.len(), 100);
        assert_eq!(buffer.alignment(), 64);
        assert!(!buffer.is_empty());

        // Check alignment
        let ptr = buffer.as_ptr();
        assert_eq!((ptr as usize) % 64, 0);
    }

    #[test]
    fn test_aligned_buffer_zeros() {
        let buffer = AlignedBuffer::<f32>::zeros(100, 64)
            .expect("Failed to create zero-initialized aligned buffer");
        for val in buffer.as_slice() {
            assert_eq!(*val, 0.0);
        }
    }

    #[test]
    fn test_aligned_buffer_copy() {
        let mut buffer =
            AlignedBuffer::<f32>::new(10, 64).expect("Failed to create aligned buffer");
        let data: Vec<f32> = (0..10).map(|i| i as f32).collect();

        buffer
            .copy_from_slice(&data)
            .expect("Failed to copy data to aligned buffer");

        for (i, val) in buffer.as_slice().iter().enumerate() {
            assert_eq!(*val, i as f32);
        }
    }

    #[test]
    fn test_strided_view() {
        let mut buffer =
            AlignedBuffer::<f32>::new(10, 64).expect("Failed to create aligned buffer");
        let data: Vec<f32> = (0..10).map(|i| i as f32).collect();
        buffer
            .copy_from_slice(&data)
            .expect("Failed to copy data to buffer");

        let view = buffer
            .strided_view(2)
            .expect("Failed to create strided view");
        assert_eq!(view.len(), 5);

        let values: Vec<f32> = view.iter().copied().collect();
        assert_eq!(values, vec![0.0, 2.0, 4.0, 6.0, 8.0]);
    }

    #[test]
    fn test_tiled_buffer() {
        let buffer =
            TiledBuffer::<f32>::new(100, 100, 32, 32).expect("Failed to create tiled buffer");
        assert_eq!(buffer.width(), 100);
        assert_eq!(buffer.height(), 100);

        let tile_count = buffer.tiles().count();
        // 100x100 with 32x32 tiles = 4x4 = 16 tiles
        assert_eq!(tile_count, 16);
    }

    #[test]
    fn test_tile_dimensions() {
        let buffer =
            TiledBuffer::<f32>::new(100, 100, 32, 32).expect("Failed to create tiled buffer");
        let tiles: Vec<Tile> = buffer.tiles().collect();

        // Check first tile
        assert_eq!(tiles[0].x, 0);
        assert_eq!(tiles[0].y, 0);
        assert_eq!(tiles[0].width, 32);
        assert_eq!(tiles[0].height, 32);

        // Check last tile (partial)
        let last = &tiles[15];
        assert_eq!(last.x, 96);
        assert_eq!(last.y, 96);
        assert_eq!(last.width, 4); // 100 - 96 = 4
        assert_eq!(last.height, 4);
    }

    #[test]
    fn test_invalid_alignment() {
        // Non-power-of-2
        let result = AlignedBuffer::<f32>::new(100, 63);
        assert!(result.is_err());

        // Too small
        let result = AlignedBuffer::<f32>::new(100, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_zero_capacity() {
        let result = AlignedBuffer::<f32>::new(0, 64);
        assert!(result.is_err());
    }
}
