//! SIMD (Single Instruction Multiple Data) optimizations for performance-critical operations
//!
//! This module provides SIMD-accelerated implementations of raster operations, statistics,
//! resampling, and mathematical functions. It uses architecture-specific intrinsics
//! (`std::arch`) with runtime CPU feature detection and graceful scalar fallback.
//!
//! # Architecture Support
//!
//! - **aarch64**: NEON (128-bit, baseline on all aarch64 CPUs)
//! - **x86-64**: SSE2 (baseline), AVX2 (runtime detected), AVX-512 (runtime detected)
//! - **Other**: Scalar fallback with auto-vectorization hints
//!
//! # Runtime Detection
//!
//! On x86-64, the module uses `is_x86_feature_detected!` to select the best available
//! instruction set at runtime. On aarch64, NEON is always available as part of the
//! base architecture. See [`detect`] module for details.
//!
//! # Performance
//!
//! Expected speedups over scalar implementations:
//!
//! - Raster operations: 2-8x (element-wise ops, min/max, thresholds)
//! - Statistics: 4-8x (sum, mean, histograms)
//! - Resampling: 2-4x (bilinear, bicubic interpolation)
//! - Math functions: 3-6x (sqrt, log, exp, trig)
//!
//! # Usage
//!
//! Enable SIMD optimizations with the `simd` feature (enabled by default):
//!
//! ```toml
//! [dependencies]
//! oxigdal-algorithms = { version = "0.1", features = ["simd"] }
//! ```
//!
//! # Example
//!
//! ```rust
//! use oxigdal_algorithms::simd::raster::add_f32;
//!
//! let a = vec![1.0_f32; 1000];
//! let b = vec![2.0_f32; 1000];
//! let mut result = vec![0.0_f32; 1000];
//!
//! // Automatically uses best available SIMD instruction set
//! add_f32(&a, &b, &mut result);
//!
//! assert_eq!(result[0], 3.0);
//! ```
//!
//! # Safety
//!
//! All public SIMD operations are safe. Unsafe intrinsics are encapsulated internally
//! with proper bounds checking, alignment handling, and error propagation.
//! Pure Rust implementation (no C/Fortran dependencies).

#![allow(clippy::module_name_repetitions)]
#![allow(unsafe_code)]

pub mod colorspace;
pub mod cost_distance_simd;
pub mod filters;
pub mod focal_simd;
pub mod histogram;
pub mod hydrology_simd;
pub mod math;
pub mod morphology;
pub mod projection;
pub mod raster;
pub mod resampling;
pub mod statistics;
pub mod terrain_simd;
pub mod texture_simd;
pub mod threshold;

/// Runtime SIMD capability detection
///
/// This module provides runtime detection of available SIMD instruction sets.
/// On x86-64, it uses `is_x86_feature_detected!` to probe CPU features.
/// On aarch64, NEON is always available.
pub mod detect {
    use std::sync::OnceLock;

    /// Available SIMD instruction set levels
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub enum SimdLevel {
        /// No SIMD - pure scalar operations
        Scalar,
        /// 128-bit SIMD (SSE2 on x86-64, NEON on aarch64)
        Simd128,
        /// 256-bit SIMD (AVX2 on x86-64)
        Simd256,
        /// 512-bit SIMD (AVX-512 on x86-64)
        Simd512,
    }

    impl SimdLevel {
        /// Get the lane width for f32 operations at this SIMD level
        #[must_use]
        pub const fn lanes_f32(self) -> usize {
            match self {
                Self::Scalar => 1,
                Self::Simd128 => 4,  // 128 / 32
                Self::Simd256 => 8,  // 256 / 32
                Self::Simd512 => 16, // 512 / 32
            }
        }

        /// Get the lane width for f64 operations at this SIMD level
        #[must_use]
        pub const fn lanes_f64(self) -> usize {
            match self {
                Self::Scalar => 1,
                Self::Simd128 => 2, // 128 / 64
                Self::Simd256 => 4, // 256 / 64
                Self::Simd512 => 8, // 512 / 64
            }
        }

        /// Get the lane width for u8 operations at this SIMD level
        #[must_use]
        pub const fn lanes_u8(self) -> usize {
            match self {
                Self::Scalar => 1,
                Self::Simd128 => 16, // 128 / 8
                Self::Simd256 => 32, // 256 / 8
                Self::Simd512 => 64, // 512 / 8
            }
        }

        /// Get the preferred memory alignment in bytes
        #[must_use]
        pub const fn preferred_alignment(self) -> usize {
            match self {
                Self::Scalar => 8,
                Self::Simd128 => 16,
                Self::Simd256 => 32,
                Self::Simd512 => 64,
            }
        }

        /// Get the register width in bits
        #[must_use]
        pub const fn width_bits(self) -> usize {
            match self {
                Self::Scalar => 64,
                Self::Simd128 => 128,
                Self::Simd256 => 256,
                Self::Simd512 => 512,
            }
        }
    }

    impl core::fmt::Display for SimdLevel {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            match self {
                Self::Scalar => write!(f, "Scalar"),
                Self::Simd128 => {
                    #[cfg(target_arch = "x86_64")]
                    {
                        write!(f, "SSE2 (128-bit)")
                    }
                    #[cfg(target_arch = "aarch64")]
                    {
                        write!(f, "NEON (128-bit)")
                    }
                    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
                    {
                        write!(f, "SIMD128")
                    }
                }
                Self::Simd256 => write!(f, "AVX2 (256-bit)"),
                Self::Simd512 => write!(f, "AVX-512 (512-bit)"),
            }
        }
    }

    /// Runtime SIMD capabilities for the current CPU
    #[derive(Debug, Clone)]
    pub struct SimdCapabilities {
        /// Highest available SIMD level
        pub max_level: SimdLevel,
        /// Whether SSE2 is available (x86-64 only, always true on x86-64)
        pub has_sse2: bool,
        /// Whether SSE4.1 is available (x86-64 only)
        pub has_sse41: bool,
        /// Whether AVX2 is available (x86-64 only)
        pub has_avx2: bool,
        /// Whether FMA is available (x86-64 only)
        pub has_fma: bool,
        /// Whether AVX-512F is available (x86-64 only)
        pub has_avx512f: bool,
        /// Whether NEON is available (aarch64 only, always true on aarch64)
        pub has_neon: bool,
    }

    impl SimdCapabilities {
        /// Detect SIMD capabilities of the current CPU at runtime
        #[must_use]
        pub fn detect() -> Self {
            #[cfg(target_arch = "x86_64")]
            {
                let has_sse2 = is_x86_feature_detected!("sse2");
                let has_sse41 = is_x86_feature_detected!("sse4.1");
                let has_avx2 = is_x86_feature_detected!("avx2");
                let has_fma = is_x86_feature_detected!("fma");
                let has_avx512f = is_x86_feature_detected!("avx512f");

                let max_level = if has_avx512f {
                    SimdLevel::Simd512
                } else if has_avx2 {
                    SimdLevel::Simd256
                } else if has_sse2 {
                    SimdLevel::Simd128
                } else {
                    SimdLevel::Scalar
                };

                Self {
                    max_level,
                    has_sse2,
                    has_sse41,
                    has_avx2,
                    has_fma,
                    has_avx512f,
                    has_neon: false,
                }
            }

            #[cfg(target_arch = "aarch64")]
            {
                // NEON is always available on aarch64
                Self {
                    max_level: SimdLevel::Simd128,
                    has_sse2: false,
                    has_sse41: false,
                    has_avx2: false,
                    has_fma: false,
                    has_avx512f: false,
                    has_neon: true,
                }
            }

            #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
            {
                Self {
                    max_level: SimdLevel::Scalar,
                    has_sse2: false,
                    has_sse41: false,
                    has_avx2: false,
                    has_fma: false,
                    has_avx512f: false,
                    has_neon: false,
                }
            }
        }
    }

    /// Get the cached SIMD capabilities for the current CPU
    ///
    /// This is initialized once on first call and cached for subsequent calls.
    #[must_use]
    pub fn capabilities() -> &'static SimdCapabilities {
        static CAPS: OnceLock<SimdCapabilities> = OnceLock::new();
        CAPS.get_or_init(SimdCapabilities::detect)
    }

    /// Get the best available SIMD level for the current CPU
    #[must_use]
    pub fn best_level() -> SimdLevel {
        capabilities().max_level
    }

    /// Check if a specific SIMD level is available
    #[must_use]
    pub fn is_available(level: SimdLevel) -> bool {
        best_level() >= level
    }
}

/// Platform-specific SIMD configuration (compile-time constants)
pub mod platform {
    /// Check if AVX2 is available at compile time
    #[cfg(target_feature = "avx2")]
    pub const HAS_AVX2: bool = true;

    /// Check if AVX2 is available at compile time
    #[cfg(not(target_feature = "avx2"))]
    pub const HAS_AVX2: bool = false;

    /// Check if AVX-512 is available at compile time
    #[cfg(target_feature = "avx512f")]
    pub const HAS_AVX512: bool = true;

    /// Check if AVX-512 is available at compile time
    #[cfg(not(target_feature = "avx512f"))]
    pub const HAS_AVX512: bool = false;

    /// Check if NEON is available at compile time
    #[cfg(target_feature = "neon")]
    pub const HAS_NEON: bool = true;

    /// Check if NEON is available at compile time
    #[cfg(not(target_feature = "neon"))]
    pub const HAS_NEON: bool = false;

    /// Get the SIMD lane width for f32 operations
    #[must_use]
    pub const fn lane_width_f32() -> usize {
        #[cfg(target_feature = "avx512f")]
        {
            16
        }
        #[cfg(all(target_feature = "avx2", not(target_feature = "avx512f")))]
        {
            8
        }
        #[cfg(all(
            target_feature = "neon",
            not(target_feature = "avx2"),
            not(target_feature = "avx512f")
        ))]
        {
            4
        }
        #[cfg(not(any(
            target_feature = "avx512f",
            target_feature = "avx2",
            target_feature = "neon"
        )))]
        {
            4
        }
    }

    /// Get the SIMD lane width for f64 operations
    #[must_use]
    pub const fn lane_width_f64() -> usize {
        #[cfg(target_feature = "avx512f")]
        {
            8
        }
        #[cfg(all(target_feature = "avx2", not(target_feature = "avx512f")))]
        {
            4
        }
        #[cfg(not(any(target_feature = "avx512f", target_feature = "avx2")))]
        {
            2
        }
    }

    /// Get the SIMD lane width for u8 operations
    #[must_use]
    pub const fn lane_width_u8() -> usize {
        #[cfg(target_feature = "avx512f")]
        {
            64
        }
        #[cfg(all(target_feature = "avx2", not(target_feature = "avx512f")))]
        {
            32
        }
        #[cfg(not(any(target_feature = "avx512f", target_feature = "avx2")))]
        {
            16
        }
    }

    /// Get the preferred alignment for SIMD operations
    #[must_use]
    pub const fn preferred_alignment() -> usize {
        #[cfg(target_feature = "avx512f")]
        {
            64
        }
        #[cfg(all(target_feature = "avx2", not(target_feature = "avx512f")))]
        {
            32
        }
        #[cfg(not(any(target_feature = "avx512f", target_feature = "avx2")))]
        {
            16
        }
    }
}

/// Utilities for working with SIMD operations
pub mod util {
    use std::alloc::Layout;

    /// Calculate the number of SIMD chunks for a given length and lane width
    #[must_use]
    pub const fn chunks(len: usize, lane_width: usize) -> usize {
        len / lane_width
    }

    /// Calculate the remainder after SIMD chunks
    #[must_use]
    pub const fn remainder(len: usize, lane_width: usize) -> usize {
        len % lane_width
    }

    /// Check if a pointer is aligned to the given boundary
    #[must_use]
    pub fn is_aligned<T>(ptr: *const T, align: usize) -> bool {
        (ptr as usize) % align == 0
    }

    /// Round up to the nearest multiple of alignment
    #[must_use]
    pub const fn align_up(value: usize, align: usize) -> usize {
        (value + align - 1) & !(align - 1)
    }

    /// Round down to the nearest multiple of alignment
    #[must_use]
    pub const fn align_down(value: usize, align: usize) -> usize {
        value & !(align - 1)
    }

    /// Allocate a SIMD-aligned `Vec<f32>`
    ///
    /// Returns a Vec whose data pointer is aligned to the specified boundary.
    /// This ensures optimal SIMD load/store performance.
    ///
    /// # Errors
    ///
    /// Returns None if allocation fails.
    pub fn aligned_vec_f32(len: usize, align: usize) -> Option<Vec<f32>> {
        if len == 0 {
            return Some(Vec::new());
        }
        let layout = Layout::from_size_align(len * std::mem::size_of::<f32>(), align).ok()?;
        // SAFETY: layout is valid (checked by from_size_align), and we properly construct a Vec
        unsafe {
            let ptr = std::alloc::alloc_zeroed(layout);
            if ptr.is_null() {
                return None;
            }
            let slice = std::slice::from_raw_parts_mut(ptr.cast::<f32>(), len);
            Some(Vec::from_raw_parts(slice.as_mut_ptr(), len, len))
        }
    }

    /// Allocate a SIMD-aligned `Vec<f64>`
    ///
    /// Returns a Vec whose data pointer is aligned to the specified boundary.
    ///
    /// # Errors
    ///
    /// Returns None if allocation fails.
    pub fn aligned_vec_f64(len: usize, align: usize) -> Option<Vec<f64>> {
        if len == 0 {
            return Some(Vec::new());
        }
        let layout = Layout::from_size_align(len * std::mem::size_of::<f64>(), align).ok()?;
        // SAFETY: layout is valid (checked by from_size_align), and we properly construct a Vec
        unsafe {
            let ptr = std::alloc::alloc_zeroed(layout);
            if ptr.is_null() {
                return None;
            }
            let slice = std::slice::from_raw_parts_mut(ptr.cast::<f64>(), len);
            Some(Vec::from_raw_parts(slice.as_mut_ptr(), len, len))
        }
    }

    /// Process a slice in SIMD-width chunks, calling a function on each chunk
    /// and handling the scalar remainder.
    ///
    /// Returns the index where the remainder begins.
    #[inline]
    pub fn process_chunks(len: usize, lanes: usize) -> (usize, usize) {
        let n_chunks = len / lanes;
        let remainder_start = n_chunks * lanes;
        (n_chunks, remainder_start)
    }
}

/// Internal macros for reducing SIMD boilerplate
///
/// These macros generate architecture-dispatched function bodies for common patterns.
#[macro_export]
#[doc(hidden)]
macro_rules! simd_validate_binary {
    ($a:expr, $b:expr, $out:expr) => {
        if $a.len() != $b.len() || $a.len() != $out.len() {
            return Err($crate::error::AlgorithmError::InvalidParameter {
                parameter: "input",
                message: format!(
                    "Slice length mismatch: a={}, b={}, out={}",
                    $a.len(),
                    $b.len(),
                    $out.len()
                ),
            });
        }
    };
}

/// Validate that two slices have the same length (unary operation)
#[macro_export]
#[doc(hidden)]
macro_rules! simd_validate_unary {
    ($data:expr, $out:expr) => {
        if $data.len() != $out.len() {
            return Err($crate::error::AlgorithmError::InvalidParameter {
                parameter: "input",
                message: format!(
                    "Slice length mismatch: data={}, out={}",
                    $data.len(),
                    $out.len()
                ),
            });
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_detection() {
        let _avx2 = platform::HAS_AVX2;
        let _avx512 = platform::HAS_AVX512;
        let _neon = platform::HAS_NEON;

        assert!(platform::lane_width_f32() >= 2);
        assert!(platform::lane_width_f64() >= 2);
        assert!(platform::lane_width_u8() >= 8);

        let align = platform::preferred_alignment();
        assert!(align.is_power_of_two());
        assert!(align >= 16);
    }

    #[test]
    fn test_runtime_detection() {
        let caps = detect::SimdCapabilities::detect();

        // On aarch64, NEON should be available
        #[cfg(target_arch = "aarch64")]
        {
            assert!(caps.has_neon);
            assert_eq!(caps.max_level, detect::SimdLevel::Simd128);
        }

        // On x86-64, at least SSE2 should be available
        #[cfg(target_arch = "x86_64")]
        {
            assert!(caps.has_sse2);
            assert!(caps.max_level >= detect::SimdLevel::Simd128);
        }

        // max_level should be at least Scalar on any platform
        assert!(caps.max_level >= detect::SimdLevel::Scalar);
    }

    #[test]
    fn test_cached_capabilities() {
        let caps1 = detect::capabilities();
        let caps2 = detect::capabilities();

        // Should return the same cached instance
        assert_eq!(caps1.max_level, caps2.max_level);
        assert_eq!(caps1.has_neon, caps2.has_neon);
    }

    #[test]
    fn test_simd_level_properties() {
        assert_eq!(detect::SimdLevel::Scalar.lanes_f32(), 1);
        assert_eq!(detect::SimdLevel::Simd128.lanes_f32(), 4);
        assert_eq!(detect::SimdLevel::Simd256.lanes_f32(), 8);
        assert_eq!(detect::SimdLevel::Simd512.lanes_f32(), 16);

        assert_eq!(detect::SimdLevel::Simd128.lanes_f64(), 2);
        assert_eq!(detect::SimdLevel::Simd256.lanes_f64(), 4);

        assert_eq!(detect::SimdLevel::Simd128.lanes_u8(), 16);
        assert_eq!(detect::SimdLevel::Simd256.lanes_u8(), 32);

        assert_eq!(detect::SimdLevel::Simd128.preferred_alignment(), 16);
        assert_eq!(detect::SimdLevel::Simd256.preferred_alignment(), 32);
        assert_eq!(detect::SimdLevel::Simd512.preferred_alignment(), 64);
    }

    #[test]
    fn test_simd_level_display() {
        let level = detect::best_level();
        let display = format!("{level}");
        assert!(!display.is_empty());
    }

    #[test]
    fn test_simd_level_ordering() {
        assert!(detect::SimdLevel::Scalar < detect::SimdLevel::Simd128);
        assert!(detect::SimdLevel::Simd128 < detect::SimdLevel::Simd256);
        assert!(detect::SimdLevel::Simd256 < detect::SimdLevel::Simd512);
    }

    #[test]
    fn test_is_available() {
        // Scalar should always be available
        assert!(detect::is_available(detect::SimdLevel::Scalar));

        // The best level should be available
        let best = detect::best_level();
        assert!(detect::is_available(best));
    }

    #[test]
    fn test_util_functions() {
        assert_eq!(util::chunks(100, 8), 12);
        assert_eq!(util::remainder(100, 8), 4);

        assert_eq!(util::align_up(15, 16), 16);
        assert_eq!(util::align_up(16, 16), 16);
        assert_eq!(util::align_up(17, 16), 32);

        assert_eq!(util::align_down(15, 16), 0);
        assert_eq!(util::align_down(16, 16), 16);
        assert_eq!(util::align_down(31, 16), 16);
    }

    #[test]
    fn test_process_chunks() {
        let (n_chunks, remainder_start) = util::process_chunks(100, 8);
        assert_eq!(n_chunks, 12);
        assert_eq!(remainder_start, 96);

        let (n_chunks, remainder_start) = util::process_chunks(7, 4);
        assert_eq!(n_chunks, 1);
        assert_eq!(remainder_start, 4);
    }

    #[test]
    fn test_pointer_alignment() {
        let data = vec![1.0_f32; 100];
        let ptr = data.as_ptr();
        assert!(util::is_aligned(ptr, std::mem::align_of::<f32>()));
    }

    #[test]
    fn test_aligned_vec_f32() {
        if let Some(v) = util::aligned_vec_f32(100, 64) {
            assert_eq!(v.len(), 100);
            assert!(util::is_aligned(v.as_ptr(), 64));
            for &val in &v {
                assert_eq!(val, 0.0);
            }
        }
    }

    #[test]
    fn test_aligned_vec_f64() {
        if let Some(v) = util::aligned_vec_f64(100, 64) {
            assert_eq!(v.len(), 100);
            assert!(util::is_aligned(v.as_ptr(), 64));
            for &val in &v {
                assert_eq!(val, 0.0);
            }
        }
    }

    #[test]
    fn test_aligned_vec_empty() {
        let v = util::aligned_vec_f32(0, 64);
        assert!(v.is_some());
        if let Some(v) = v {
            assert!(v.is_empty());
        }
    }
}
