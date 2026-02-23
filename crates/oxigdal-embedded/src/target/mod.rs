//! Target-specific optimizations and support
//!
//! Provides architecture-specific implementations for embedded platforms

pub mod arm;
pub mod common;
pub mod esp32;
pub mod riscv;

/// Target architecture trait
pub trait TargetArch {
    /// Get the target name
    fn name(&self) -> &'static str;

    /// Get the native pointer size in bytes
    fn pointer_size(&self) -> usize;

    /// Get the native alignment
    fn native_alignment(&self) -> usize;

    /// Check if target supports unaligned access
    fn supports_unaligned_access(&self) -> bool;

    /// Perform a memory barrier
    fn memory_barrier(&self);

    /// Get cycle counter if available
    fn cycle_count(&self) -> Option<u64>;
}

/// Memory ordering for atomic operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryOrder {
    /// Relaxed ordering
    Relaxed,
    /// Acquire ordering
    Acquire,
    /// Release ordering
    Release,
    /// Acquire-Release ordering
    AcquireRelease,
    /// Sequentially consistent ordering
    SeqCst,
}

/// Cache operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheOp {
    /// Clean cache (write back dirty lines)
    Clean,
    /// Invalidate cache (discard lines)
    Invalidate,
    /// Clean and invalidate
    CleanInvalidate,
}

/// Target capabilities
#[derive(Debug, Clone, Copy)]
pub struct TargetCapabilities {
    /// Has hardware floating point
    pub has_fpu: bool,
    /// Has SIMD instructions
    pub has_simd: bool,
    /// Has hardware AES
    pub has_aes: bool,
    /// Has hardware CRC
    pub has_crc: bool,
    /// Cache line size in bytes
    pub cache_line_size: usize,
    /// Number of cores
    pub num_cores: usize,
}

impl Default for TargetCapabilities {
    fn default() -> Self {
        Self {
            has_fpu: false,
            has_simd: false,
            has_aes: false,
            has_crc: false,
            cache_line_size: 64,
            num_cores: 1,
        }
    }
}

/// Get current target capabilities
pub fn get_capabilities() -> TargetCapabilities {
    #[cfg(feature = "arm")]
    {
        arm::get_capabilities()
    }

    #[cfg(all(feature = "riscv", not(feature = "arm")))]
    {
        riscv::get_capabilities()
    }

    #[cfg(all(feature = "esp32", not(any(feature = "arm", feature = "riscv"))))]
    {
        esp32::get_capabilities()
    }

    #[cfg(not(any(feature = "arm", feature = "riscv", feature = "esp32")))]
    {
        TargetCapabilities::default()
    }
}

/// Perform a memory barrier appropriate for the target
#[inline]
pub fn memory_barrier() {
    #[cfg(feature = "arm")]
    arm::memory_barrier();

    #[cfg(all(feature = "riscv", not(feature = "arm")))]
    riscv::memory_barrier();

    #[cfg(all(feature = "esp32", not(any(feature = "arm", feature = "riscv"))))]
    esp32::memory_barrier();

    #[cfg(not(any(feature = "arm", feature = "riscv", feature = "esp32")))]
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}

/// Get cycle count if available
#[inline]
pub fn cycle_count() -> Option<u64> {
    #[cfg(feature = "arm")]
    {
        arm::cycle_count()
    }

    #[cfg(all(feature = "riscv", not(feature = "arm")))]
    {
        riscv::cycle_count()
    }

    #[cfg(all(feature = "esp32", not(any(feature = "arm", feature = "riscv"))))]
    {
        esp32::cycle_count()
    }

    #[cfg(not(any(feature = "arm", feature = "riscv", feature = "esp32")))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capabilities() {
        let caps = get_capabilities();
        assert!(caps.cache_line_size > 0);
        assert!(caps.num_cores > 0);
    }

    #[test]
    fn test_memory_barrier() {
        // Should not panic
        memory_barrier();
    }
}
