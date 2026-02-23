//! ARM-specific optimizations and support
//!
//! Provides implementations for ARM Cortex-M and Cortex-A processors

use super::{TargetArch, TargetCapabilities};
use core::sync::atomic::{Ordering, fence};

/// ARM target implementation
pub struct ArmTarget;

impl TargetArch for ArmTarget {
    fn name(&self) -> &'static str {
        "ARM"
    }

    fn pointer_size(&self) -> usize {
        core::mem::size_of::<usize>()
    }

    fn native_alignment(&self) -> usize {
        4 // ARM typically prefers 4-byte alignment
    }

    fn supports_unaligned_access(&self) -> bool {
        // Cortex-M3/M4/M7 support unaligned access
        // Cortex-M0/M0+ do not
        cfg!(any(
            target_feature = "v7",
            target_feature = "v8",
            target_arch = "aarch64"
        ))
    }

    fn memory_barrier(&self) {
        memory_barrier();
    }

    fn cycle_count(&self) -> Option<u64> {
        cycle_count()
    }
}

/// Get ARM target capabilities
pub fn get_capabilities() -> TargetCapabilities {
    TargetCapabilities {
        has_fpu: cfg!(target_feature = "vfp2")
            || cfg!(target_feature = "vfp3")
            || cfg!(target_feature = "vfp4"),
        has_simd: cfg!(target_feature = "neon"),
        has_aes: cfg!(target_feature = "aes"),
        has_crc: cfg!(target_feature = "crc"),
        cache_line_size: 64, // Common ARM cache line size
        num_cores: 1,        // Embedded systems typically have 1 core
    }
}

/// ARM memory barrier
#[inline]
pub fn memory_barrier() {
    fence(Ordering::SeqCst);

    #[cfg(target_arch = "arm")]
    {
        // Data Memory Barrier
        unsafe {
            core::arch::asm!("dmb", options(nostack, nomem));
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        // Data Memory Barrier
        unsafe {
            core::arch::asm!("dmb sy", options(nostack, nomem));
        }
    }
}

/// Get cycle count from ARM performance counter
#[inline]
pub fn cycle_count() -> Option<u64> {
    #[cfg(target_arch = "arm")]
    {
        // Read PMCCNTR (Performance Monitors Cycle Count Register)
        // Note: This requires appropriate permissions and setup
        let count: u32;
        unsafe {
            core::arch::asm!(
                "mrc p15, 0, {}, c9, c13, 0",
                out(reg) count,
                options(nostack, nomem, preserves_flags)
            );
        }
        Some(count as u64)
    }

    #[cfg(target_arch = "aarch64")]
    {
        // Read PMCCNTR_EL0 (Performance Monitors Cycle Count Register)
        let count: u64;
        unsafe {
            core::arch::asm!(
                "mrs {}, pmccntr_el0",
                out(reg) count,
                options(nostack, nomem, preserves_flags)
            );
        }
        Some(count)
    }

    #[cfg(not(any(target_arch = "arm", target_arch = "aarch64")))]
    {
        None
    }
}

/// ARM cache operations
pub mod cache {
    use crate::error::Result;

    /// Clean data cache by address range
    ///
    /// # Safety
    ///
    /// The address range must be valid
    #[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
    pub unsafe fn clean_dcache(addr: usize, size: usize) -> Result<()> {
        let cache_line_size = 64; // Common ARM cache line size
        let start = addr & !(cache_line_size - 1);
        let end = (addr + size + cache_line_size - 1) & !(cache_line_size - 1);

        let mut current = start;
        while current < end {
            #[cfg(target_arch = "arm")]
            {
                // SAFETY: Inline assembly for cache cleaning
                unsafe {
                    core::arch::asm!(
                        "mcr p15, 0, {}, c7, c10, 1",
                        in(reg) current,
                        options(nostack, preserves_flags)
                    );
                }
            }

            #[cfg(target_arch = "aarch64")]
            {
                // SAFETY: Inline assembly for cache cleaning
                unsafe {
                    core::arch::asm!(
                        "dc cvac, {}",
                        in(reg) current,
                        options(nostack, preserves_flags)
                    );
                }
            }

            current = current.wrapping_add(cache_line_size);
        }

        super::memory_barrier();
        Ok(())
    }

    /// Invalidate data cache by address range
    ///
    /// # Safety
    ///
    /// The address range must be valid
    #[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
    pub unsafe fn invalidate_dcache(addr: usize, size: usize) -> Result<()> {
        let cache_line_size = 64;
        let start = addr & !(cache_line_size - 1);
        let end = (addr + size + cache_line_size - 1) & !(cache_line_size - 1);

        let mut current = start;
        while current < end {
            #[cfg(target_arch = "arm")]
            {
                // SAFETY: Inline assembly for cache invalidation
                unsafe {
                    core::arch::asm!(
                        "mcr p15, 0, {}, c7, c6, 1",
                        in(reg) current,
                        options(nostack, preserves_flags)
                    );
                }
            }

            #[cfg(target_arch = "aarch64")]
            {
                // SAFETY: Inline assembly for cache invalidation
                unsafe {
                    core::arch::asm!(
                        "dc ivac, {}",
                        in(reg) current,
                        options(nostack, preserves_flags)
                    );
                }
            }

            current = current.wrapping_add(cache_line_size);
        }

        super::memory_barrier();
        Ok(())
    }

    /// Clean data cache for the specified address range (no-op on non-ARM)
    #[cfg(not(any(target_arch = "arm", target_arch = "aarch64")))]
    pub unsafe fn clean_dcache(_addr: usize, _size: usize) -> Result<()> {
        Ok(())
    }

    /// Invalidate data cache for the specified address range (no-op on non-ARM)
    #[cfg(not(any(target_arch = "arm", target_arch = "aarch64")))]
    pub unsafe fn invalidate_dcache(_addr: usize, _size: usize) -> Result<()> {
        Ok(())
    }
}

/// ARM SIMD operations (NEON)
#[cfg(target_feature = "neon")]
pub mod simd {
    #[cfg(target_arch = "arm")]
    use core::arch::arm::*;

    /// Copy memory using NEON instructions
    ///
    /// # Safety
    ///
    /// src and dst must be valid and properly aligned
    #[cfg(target_arch = "arm")]
    pub unsafe fn memcpy_neon(dst: *mut u8, src: *const u8, len: usize) {
        let mut offset = 0;
        let chunks = len / 16;

        for _ in 0..chunks {
            // SAFETY: Caller guarantees dst and src are valid and aligned
            unsafe {
                let data = vld1q_u8(src.add(offset));
                vst1q_u8(dst.add(offset), data);
            }
            offset += 16;
        }

        // Handle remaining bytes
        for i in offset..len {
            // SAFETY: Caller guarantees dst and src are valid
            unsafe {
                *dst.add(i) = *src.add(i);
            }
        }
    }

    /// Copy memory using NEON instructions (AArch64)
    ///
    /// # Safety
    ///
    /// src and dst must be valid and properly aligned
    #[cfg(target_arch = "aarch64")]
    pub unsafe fn memcpy_neon(dst: *mut u8, src: *const u8, len: usize) {
        use core::arch::aarch64::*;

        let mut offset = 0;
        let chunks = len / 16;

        for _ in 0..chunks {
            // SAFETY: Caller guarantees dst and src are valid and aligned
            unsafe {
                let data = vld1q_u8(src.add(offset));
                vst1q_u8(dst.add(offset), data);
            }
            offset += 16;
        }

        // Handle remaining bytes
        for i in offset..len {
            // SAFETY: Caller guarantees dst and src are valid
            unsafe {
                *dst.add(i) = *src.add(i);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arm_target() {
        let target = ArmTarget;
        assert_eq!(target.name(), "ARM");
        assert!(target.pointer_size() > 0);
        assert!(target.native_alignment() > 0);
    }

    #[test]
    fn test_capabilities() {
        let caps = get_capabilities();
        assert!(caps.cache_line_size > 0);
    }
}
