//! Common target utilities and helpers

use crate::error::{EmbeddedError, Result};
use core::ptr;

/// Memory copy optimized for target
///
/// # Safety
///
/// dst and src must be valid pointers with at least len bytes
pub unsafe fn memcpy(dst: *mut u8, src: *const u8, len: usize) -> Result<()> {
    if dst.is_null() || src.is_null() {
        return Err(EmbeddedError::InvalidParameter);
    }

    #[cfg(all(feature = "arm", target_feature = "neon"))]
    {
        // SAFETY: Caller guarantees dst and src are valid pointers
        unsafe {
            super::arm::simd::memcpy_neon(dst, src, len);
        }
        Ok(())
    }

    #[cfg(not(all(feature = "arm", target_feature = "neon")))]
    {
        // Fallback to standard copy
        // SAFETY: Caller guarantees dst and src are valid pointers with at least len bytes
        unsafe {
            ptr::copy_nonoverlapping(src, dst, len);
        }
        Ok(())
    }
}

/// Memory set optimized for target
///
/// # Safety
///
/// dst must be a valid pointer with at least len bytes
pub unsafe fn memset(dst: *mut u8, value: u8, len: usize) -> Result<()> {
    if dst.is_null() {
        return Err(EmbeddedError::InvalidParameter);
    }

    // SAFETY: Caller guarantees dst is valid with at least len bytes
    unsafe {
        ptr::write_bytes(dst, value, len);
    }
    Ok(())
}

/// Memory compare
///
/// # Safety
///
/// a and b must be valid pointers with at least len bytes
pub unsafe fn memcmp(a: *const u8, b: *const u8, len: usize) -> Result<i32> {
    if a.is_null() || b.is_null() {
        return Err(EmbeddedError::InvalidParameter);
    }

    for i in 0..len {
        // SAFETY: Caller guarantees a and b are valid pointers with at least len bytes
        let byte_a = unsafe { *a.add(i) };
        let byte_b = unsafe { *b.add(i) };

        if byte_a != byte_b {
            return Ok(byte_a as i32 - byte_b as i32);
        }
    }

    Ok(0)
}

/// Calculate CRC32 (software implementation)
pub fn crc32(data: &[u8]) -> u32 {
    const CRC32_POLYNOMIAL: u32 = 0xEDB8_8320;

    let mut crc = 0xFFFF_FFFF;

    for &byte in data {
        crc ^= u32::from(byte);

        for _ in 0..8 {
            let mask = u32::wrapping_sub(0, crc & 1);
            crc = (crc >> 1) ^ (CRC32_POLYNOMIAL & mask);
        }
    }

    !crc
}

/// Calculate checksum (simple sum)
pub fn checksum(data: &[u8]) -> u32 {
    data.iter()
        .fold(0u32, |acc, &byte| acc.wrapping_add(u32::from(byte)))
}

/// Align value up to alignment
pub fn align_up(value: usize, align: usize) -> Option<usize> {
    if !align.is_power_of_two() {
        return None;
    }

    let mask = align.wrapping_sub(1);
    value.checked_add(mask).map(|v| v & !mask)
}

/// Align value down to alignment
pub const fn align_down(value: usize, align: usize) -> Option<usize> {
    if !align.is_power_of_two() {
        return None;
    }

    let mask = align.wrapping_sub(1);
    Some(value & !mask)
}

/// Check if value is aligned
pub const fn is_aligned(value: usize, align: usize) -> bool {
    if !align.is_power_of_two() {
        return false;
    }

    value & align.wrapping_sub(1) == 0
}

/// Byte swap for endianness conversion
pub const fn bswap16(value: u16) -> u16 {
    value.swap_bytes()
}

/// Byte swap for endianness conversion
pub const fn bswap32(value: u32) -> u32 {
    value.swap_bytes()
}

/// Byte swap for endianness conversion
pub const fn bswap64(value: u64) -> u64 {
    value.swap_bytes()
}

/// Count leading zeros
pub const fn clz32(value: u32) -> u32 {
    value.leading_zeros()
}

/// Count trailing zeros
pub const fn ctz32(value: u32) -> u32 {
    value.trailing_zeros()
}

/// Population count (number of 1 bits)
pub const fn popcount32(value: u32) -> u32 {
    value.count_ones()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memcpy() {
        let src = [1u8, 2, 3, 4, 5];
        let mut dst = [0u8; 5];

        unsafe {
            memcpy(dst.as_mut_ptr(), src.as_ptr(), 5).expect("memcpy failed");
        }

        assert_eq!(src, dst);
    }

    #[test]
    fn test_memset() {
        let mut buffer = [0u8; 10];

        unsafe {
            memset(buffer.as_mut_ptr(), 0xFF, 10).expect("memset failed");
        }

        assert_eq!(buffer, [0xFF; 10]);
    }

    #[test]
    fn test_memcmp() {
        let a = [1u8, 2, 3, 4, 5];
        let b = [1u8, 2, 3, 4, 5];
        let c = [1u8, 2, 3, 4, 6];

        unsafe {
            assert_eq!(memcmp(a.as_ptr(), b.as_ptr(), 5).expect("memcmp failed"), 0);
            assert!(memcmp(a.as_ptr(), c.as_ptr(), 5).expect("memcmp failed") < 0);
        }
    }

    #[test]
    fn test_crc32() {
        let data = b"Hello, World!";
        let crc = crc32(data);
        assert_ne!(crc, 0);

        // CRC should be deterministic
        assert_eq!(crc, crc32(data));
    }

    #[test]
    fn test_alignment() {
        assert_eq!(align_up(10, 8), Some(16));
        assert_eq!(align_down(10, 8), Some(8));
        assert!(is_aligned(16, 8));
        assert!(!is_aligned(10, 8));
    }

    #[test]
    fn test_byte_swap() {
        assert_eq!(bswap16(0x1234), 0x3412);
        assert_eq!(bswap32(0x1234_5678), 0x7856_3412);
        assert_eq!(bswap64(0x0123_4567_89AB_CDEF), 0xEFCD_AB89_6745_2301);
    }

    #[test]
    fn test_bit_operations() {
        assert_eq!(clz32(0x0000_1000), 19);
        assert_eq!(ctz32(0x0000_1000), 12);
        assert_eq!(popcount32(0b1010_1010), 4);
    }
}
