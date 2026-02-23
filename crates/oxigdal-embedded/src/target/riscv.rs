//! RISC-V specific optimizations and support
//!
//! Provides implementations for RISC-V processors (RV32/RV64)

use super::{TargetArch, TargetCapabilities};
use core::sync::atomic::{Ordering, fence};

/// RISC-V target implementation
pub struct RiscVTarget;

impl TargetArch for RiscVTarget {
    fn name(&self) -> &'static str {
        if cfg!(target_arch = "riscv64") {
            "RISC-V 64"
        } else {
            "RISC-V 32"
        }
    }

    fn pointer_size(&self) -> usize {
        core::mem::size_of::<usize>()
    }

    fn native_alignment(&self) -> usize {
        core::mem::size_of::<usize>() // Natural alignment
    }

    fn supports_unaligned_access(&self) -> bool {
        // RISC-V generally does not support unaligned access in base ISA
        // Some implementations may support it via emulation
        false
    }

    fn memory_barrier(&self) {
        memory_barrier();
    }

    fn cycle_count(&self) -> Option<u64> {
        cycle_count()
    }
}

/// Get RISC-V target capabilities
pub fn get_capabilities() -> TargetCapabilities {
    TargetCapabilities {
        has_fpu: cfg!(target_feature = "f") || cfg!(target_feature = "d"),
        has_simd: cfg!(target_feature = "v"), // Vector extension
        has_aes: cfg!(target_feature = "zkne") || cfg!(target_feature = "zknd"), // Crypto extension
        has_crc: cfg!(target_feature = "zbkc"), // Bit manipulation crypto
        cache_line_size: 64,                  // Common cache line size
        num_cores: 1,
    }
}

/// RISC-V memory barrier
#[inline]
pub fn memory_barrier() {
    fence(Ordering::SeqCst);

    #[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
    {
        unsafe {
            // Full memory fence
            core::arch::asm!("fence rw, rw", options(nostack, nomem));
        }
    }
}

/// Get cycle count from RISC-V time CSR
#[inline]
pub fn cycle_count() -> Option<u64> {
    #[cfg(target_arch = "riscv32")]
    {
        let low: u32;
        let high1: u32;
        let high2: u32;

        unsafe {
            // Read time CSR (handles 64-bit value on 32-bit platform)
            loop {
                core::arch::asm!(
                    "rdtimeh {high}",
                    "rdtime {low}",
                    "rdtimeh {high2}",
                    high = out(reg) high1,
                    low = out(reg) low,
                    high2 = out(reg) high2,
                    options(nostack, nomem, preserves_flags)
                );

                // Ensure high word didn't change during read
                if high1 == high2 {
                    break;
                }
            }
        }

        Some(((high1 as u64) << 32) | (low as u64))
    }

    #[cfg(target_arch = "riscv64")]
    {
        let count: u64;
        unsafe {
            core::arch::asm!(
                "rdtime {}",
                out(reg) count,
                options(nostack, nomem, preserves_flags)
            );
        }
        Some(count)
    }

    #[cfg(not(any(target_arch = "riscv32", target_arch = "riscv64")))]
    {
        None
    }
}

/// RISC-V atomic operations
pub mod atomic {
    use crate::error::{EmbeddedError, Result};

    /// Atomic compare-and-swap
    ///
    /// # Safety
    ///
    /// ptr must be valid and properly aligned
    #[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
    pub unsafe fn compare_and_swap(ptr: *mut usize, old: usize, new: usize) -> Result<usize> {
        let result: usize;

        #[cfg(target_arch = "riscv32")]
        core::arch::asm!(
            "lr.w {tmp}, ({ptr})",
            "bne {tmp}, {old}, 1f",
            "sc.w {result}, {new}, ({ptr})",
            "j 2f",
            "1:",
            "li {result}, 1",
            "2:",
            ptr = in(reg) ptr,
            old = in(reg) old,
            new = in(reg) new,
            tmp = out(reg) _,
            result = out(reg) result,
            options(nostack)
        );

        #[cfg(target_arch = "riscv64")]
        core::arch::asm!(
            "lr.d {tmp}, ({ptr})",
            "bne {tmp}, {old}, 1f",
            "sc.d {result}, {new}, ({ptr})",
            "j 2f",
            "1:",
            "li {result}, 1",
            "2:",
            ptr = in(reg) ptr,
            old = in(reg) old,
            new = in(reg) new,
            tmp = out(reg) _,
            result = out(reg) result,
            options(nostack)
        );

        if result == 0 {
            Ok(new)
        } else {
            Err(EmbeddedError::ResourceBusy)
        }
    }

    /// Compare and swap (stub for non-RISC-V targets)
    ///
    /// # Safety
    ///
    /// The pointer must be valid and properly aligned
    #[cfg(not(any(target_arch = "riscv32", target_arch = "riscv64")))]
    pub unsafe fn compare_and_swap(_ptr: *mut usize, _old: usize, _new: usize) -> Result<usize> {
        Err(EmbeddedError::UnsupportedOperation)
    }
}

/// RISC-V cache operations
pub mod cache {
    use crate::error::Result;

    /// Flush instruction cache
    ///
    /// # Safety
    ///
    /// Must be called after modifying executable code
    #[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
    pub unsafe fn flush_icache() -> Result<()> {
        // FENCE.I instruction
        core::arch::asm!("fence.i", options(nostack, nomem));
        Ok(())
    }

    /// Flush instruction cache (stub for non-RISC-V targets)
    ///
    /// # Safety
    ///
    /// Must be called after modifying executable code
    #[cfg(not(any(target_arch = "riscv32", target_arch = "riscv64")))]
    pub unsafe fn flush_icache() -> Result<()> {
        Ok(())
    }

    /// Data cache fence
    #[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
    pub fn dcache_fence() {
        unsafe {
            core::arch::asm!("fence rw, rw", options(nostack, nomem));
        }
    }

    /// Data cache fence (stub for non-RISC-V targets)
    #[cfg(not(any(target_arch = "riscv32", target_arch = "riscv64")))]
    pub fn dcache_fence() {}
}

/// RISC-V power management
pub mod power {
    use crate::error::Result;

    /// Wait for interrupt (low power mode)
    #[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
    pub fn wait_for_interrupt() -> Result<()> {
        unsafe {
            // WFI instruction
            core::arch::asm!("wfi", options(nostack, nomem));
        }
        Ok(())
    }

    /// Wait for interrupt (stub for non-RISC-V targets)
    #[cfg(not(any(target_arch = "riscv32", target_arch = "riscv64")))]
    pub fn wait_for_interrupt() -> Result<()> {
        Ok(())
    }
}

/// RISC-V Vector extension support
#[cfg(target_feature = "v")]
pub mod vector {
    /// Check if vector extension is available
    pub fn is_available() -> bool {
        // Check if vector length is non-zero
        true
    }

    /// Get vector length in bytes
    #[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
    pub fn vector_length() -> usize {
        // This would need to query VLENB CSR
        // For now, return common value
        128 / 8 // 128-bit vectors = 16 bytes
    }

    #[cfg(not(any(target_arch = "riscv32", target_arch = "riscv64")))]
    pub fn vector_length() -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_riscv_target() {
        let target = RiscVTarget;
        assert!(target.name().contains("RISC-V"));
        assert!(target.pointer_size() > 0);
        assert!(target.native_alignment() > 0);
    }

    #[test]
    fn test_capabilities() {
        let caps = get_capabilities();
        assert!(caps.cache_line_size > 0);
    }

    #[test]
    fn test_memory_barrier() {
        // Should not panic
        memory_barrier();
    }
}
