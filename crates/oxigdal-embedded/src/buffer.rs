//! Fixed-size buffer implementations for embedded systems

use crate::error::{EmbeddedError, Result};
use core::mem::MaybeUninit;

/// Fixed-size buffer with compile-time capacity
pub struct FixedBuffer<T, const N: usize> {
    data: [MaybeUninit<T>; N],
    len: usize,
}

impl<T, const N: usize> FixedBuffer<T, N> {
    /// Create a new empty buffer
    pub const fn new() -> Self {
        Self {
            // SAFETY: MaybeUninit array doesn't require initialization
            data: unsafe { MaybeUninit::uninit().assume_init() },
            len: 0,
        }
    }

    /// Get the capacity of the buffer
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Get the current length
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Check if buffer is empty
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Check if buffer is full
    pub const fn is_full(&self) -> bool {
        self.len >= N
    }

    /// Push an item to the buffer
    ///
    /// # Errors
    ///
    /// Returns error if buffer is full
    pub fn push(&mut self, item: T) -> Result<()> {
        if self.is_full() {
            return Err(EmbeddedError::BufferTooSmall {
                required: 1,
                available: 0,
            });
        }

        self.data[self.len].write(item);
        self.len += 1;
        Ok(())
    }

    /// Pop an item from the buffer
    ///
    /// # Errors
    ///
    /// Returns error if buffer is empty
    pub fn pop(&mut self) -> Result<T> {
        if self.is_empty() {
            return Err(EmbeddedError::InvalidParameter);
        }

        self.len -= 1;
        // SAFETY: We just verified len > 0
        let item = unsafe { self.data[self.len].assume_init_read() };
        Ok(item)
    }

    /// Get a reference to an item at index
    ///
    /// # Errors
    ///
    /// Returns error if index is out of bounds
    pub fn get(&self, index: usize) -> Result<&T> {
        if index >= self.len {
            return Err(EmbeddedError::OutOfBounds {
                index,
                max: self.len.saturating_sub(1),
            });
        }

        // SAFETY: We verified index is within bounds and initialized
        let item = unsafe { self.data[index].assume_init_ref() };
        Ok(item)
    }

    /// Get a mutable reference to an item at index
    ///
    /// # Errors
    ///
    /// Returns error if index is out of bounds
    pub fn get_mut(&mut self, index: usize) -> Result<&mut T> {
        if index >= self.len {
            return Err(EmbeddedError::OutOfBounds {
                index,
                max: self.len.saturating_sub(1),
            });
        }

        // SAFETY: We verified index is within bounds and initialized
        let item = unsafe { self.data[index].assume_init_mut() };
        Ok(item)
    }

    /// Clear the buffer
    pub fn clear(&mut self) {
        // Drop all initialized elements
        for i in 0..self.len {
            // SAFETY: Elements up to len are initialized
            unsafe {
                self.data[i].assume_init_drop();
            }
        }
        self.len = 0;
    }

    /// Get a slice of the buffer contents
    pub fn as_slice(&self) -> &[T] {
        // SAFETY: Elements up to len are initialized
        unsafe { core::slice::from_raw_parts(self.data.as_ptr().cast(), self.len) }
    }

    /// Get a mutable slice of the buffer contents
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: Elements up to len are initialized
        unsafe { core::slice::from_raw_parts_mut(self.data.as_mut_ptr().cast(), self.len) }
    }

    /// Extend buffer from slice
    ///
    /// # Errors
    ///
    /// Returns error if not enough space
    pub fn extend_from_slice(&mut self, items: &[T]) -> Result<()>
    where
        T: Copy,
    {
        if self.len + items.len() > N {
            return Err(EmbeddedError::BufferTooSmall {
                required: items.len(),
                available: N - self.len,
            });
        }

        for item in items {
            self.data[self.len].write(*item);
            self.len += 1;
        }

        Ok(())
    }
}

impl<T, const N: usize> Default for FixedBuffer<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> Drop for FixedBuffer<T, N> {
    fn drop(&mut self) {
        self.clear();
    }
}

/// Ring buffer for streaming data
pub struct RingBuffer<T: Copy, const N: usize> {
    data: [T; N],
    read_pos: usize,
    write_pos: usize,
    full: bool,
}

impl<T: Copy, const N: usize> RingBuffer<T, N> {
    /// Create a new ring buffer with default-initialized data
    pub fn new(default: T) -> Self {
        Self {
            data: [default; N],
            read_pos: 0,
            write_pos: 0,
            full: false,
        }
    }

    /// Get the capacity
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Get the number of items in the buffer
    pub fn len(&self) -> usize {
        if self.full {
            N
        } else if self.write_pos >= self.read_pos {
            self.write_pos - self.read_pos
        } else {
            N - self.read_pos + self.write_pos
        }
    }

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        !self.full && self.read_pos == self.write_pos
    }

    /// Check if buffer is full
    pub const fn is_full(&self) -> bool {
        self.full
    }

    /// Write an item to the buffer
    ///
    /// # Errors
    ///
    /// Returns error if buffer is full
    pub fn write(&mut self, item: T) -> Result<()> {
        if self.full {
            return Err(EmbeddedError::BufferTooSmall {
                required: 1,
                available: 0,
            });
        }

        self.data[self.write_pos] = item;
        self.write_pos = (self.write_pos + 1) % N;

        if self.write_pos == self.read_pos {
            self.full = true;
        }

        Ok(())
    }

    /// Read an item from the buffer
    ///
    /// # Errors
    ///
    /// Returns error if buffer is empty
    pub fn read(&mut self) -> Result<T> {
        if self.is_empty() {
            return Err(EmbeddedError::InvalidParameter);
        }

        let item = self.data[self.read_pos];
        self.read_pos = (self.read_pos + 1) % N;
        self.full = false;

        Ok(item)
    }

    /// Peek at the next item without removing it
    pub fn peek(&self) -> Result<T> {
        if self.is_empty() {
            return Err(EmbeddedError::InvalidParameter);
        }

        Ok(self.data[self.read_pos])
    }

    /// Clear the buffer
    pub fn clear(&mut self) {
        self.read_pos = 0;
        self.write_pos = 0;
        self.full = false;
    }
}

/// Aligned buffer for DMA or hardware access
#[repr(C, align(64))]
pub struct AlignedBuffer<const N: usize> {
    data: [u8; N],
}

impl<const N: usize> AlignedBuffer<N> {
    /// Create a new aligned buffer
    pub const fn new() -> Self {
        Self { data: [0u8; N] }
    }

    /// Get a slice of the buffer
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    /// Get a mutable slice of the buffer
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// Get the buffer pointer
    pub fn as_ptr(&self) -> *const u8 {
        self.data.as_ptr()
    }

    /// Get the mutable buffer pointer
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.data.as_mut_ptr()
    }

    /// Get the alignment of the buffer
    pub const fn alignment(&self) -> usize {
        64
    }

    /// Verify the buffer is properly aligned
    pub fn verify_alignment(&self) -> bool {
        self.as_ptr() as usize % 64 == 0
    }
}

impl<const N: usize> Default for AlignedBuffer<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_buffer() {
        let mut buffer = FixedBuffer::<u32, 8>::new();
        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());

        buffer.push(1).expect("push failed");
        buffer.push(2).expect("push failed");
        assert_eq!(buffer.len(), 2);

        assert_eq!(*buffer.get(0).expect("get failed"), 1);
        assert_eq!(*buffer.get(1).expect("get failed"), 2);

        let item = buffer.pop().expect("pop failed");
        assert_eq!(item, 2);
        assert_eq!(buffer.len(), 1);
    }

    #[test]
    fn test_fixed_buffer_overflow() {
        let mut buffer = FixedBuffer::<u32, 2>::new();
        buffer.push(1).expect("push failed");
        buffer.push(2).expect("push failed");

        let result = buffer.push(3);
        assert!(matches!(result, Err(EmbeddedError::BufferTooSmall { .. })));
    }

    #[test]
    fn test_ring_buffer() {
        let mut buffer = RingBuffer::<u32, 4>::new(0);
        assert!(buffer.is_empty());

        buffer.write(1).expect("write failed");
        buffer.write(2).expect("write failed");
        assert_eq!(buffer.len(), 2);

        let item = buffer.read().expect("read failed");
        assert_eq!(item, 1);

        buffer.write(3).expect("write failed");
        buffer.write(4).expect("write failed");
        buffer.write(5).expect("write failed");

        assert!(buffer.is_full());
        assert!(buffer.write(6).is_err());
    }

    #[test]
    fn test_aligned_buffer() {
        let buffer = AlignedBuffer::<256>::new();
        assert_eq!(buffer.alignment(), 64);
        assert!(buffer.verify_alignment());
    }
}
