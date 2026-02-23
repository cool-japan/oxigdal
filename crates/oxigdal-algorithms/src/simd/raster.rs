//! SIMD-accelerated raster operations
//!
//! This module provides high-performance element-wise operations on raster data
//! using architecture-specific SIMD intrinsics. On aarch64, it uses NEON. On
//! x86-64, it uses SSE2 with runtime AVX2 dispatch. All functions include a
//! scalar fallback for platforms without SIMD support.
//!
//! # Supported Operations
//!
//! - **Arithmetic**: add, subtract, multiply, divide, fused multiply-add
//! - **Comparison**: min, max, clamp
//! - **Logical**: threshold, mask
//! - **Type Conversion**: u8 <-> f32, scaling
//!
//! # Performance
//!
//! Expected speedup over scalar: 2-8x depending on operation and data type.
//! NEON (aarch64): 4x for f32 ops. AVX2 (x86-64): 8x for f32 ops.

#![allow(unsafe_code)]

use crate::error::{AlgorithmError, Result};

// ============================================================================
// Architecture-specific SIMD implementations
// ============================================================================

#[cfg(target_arch = "aarch64")]
mod neon_impl {
    use std::arch::aarch64::*;

    /// NEON f32x4 add
    /// SAFETY: Caller must ensure slices are valid and same length.
    #[target_feature(enable = "neon")]
    pub(crate) unsafe fn add_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        unsafe {
            let len = a.len();
            let chunks = len / 4;
            let a_ptr = a.as_ptr();
            let b_ptr = b.as_ptr();
            let o_ptr = out.as_mut_ptr();

            for i in 0..chunks {
                let off = i * 4;
                let va = vld1q_f32(a_ptr.add(off));
                let vb = vld1q_f32(b_ptr.add(off));
                let vr = vaddq_f32(va, vb);
                vst1q_f32(o_ptr.add(off), vr);
            }
            let rem = chunks * 4;
            for i in rem..len {
                *o_ptr.add(i) = *a_ptr.add(i) + *b_ptr.add(i);
            }
        }
    }

    /// NEON f32x4 subtract
    #[target_feature(enable = "neon")]
    pub(crate) unsafe fn sub_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        unsafe {
            let len = a.len();
            let chunks = len / 4;
            let a_ptr = a.as_ptr();
            let b_ptr = b.as_ptr();
            let o_ptr = out.as_mut_ptr();

            for i in 0..chunks {
                let off = i * 4;
                let va = vld1q_f32(a_ptr.add(off));
                let vb = vld1q_f32(b_ptr.add(off));
                let vr = vsubq_f32(va, vb);
                vst1q_f32(o_ptr.add(off), vr);
            }
            let rem = chunks * 4;
            for i in rem..len {
                *o_ptr.add(i) = *a_ptr.add(i) - *b_ptr.add(i);
            }
        }
    }

    /// NEON f32x4 multiply
    #[target_feature(enable = "neon")]
    pub(crate) unsafe fn mul_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        unsafe {
            let len = a.len();
            let chunks = len / 4;
            let a_ptr = a.as_ptr();
            let b_ptr = b.as_ptr();
            let o_ptr = out.as_mut_ptr();

            for i in 0..chunks {
                let off = i * 4;
                let va = vld1q_f32(a_ptr.add(off));
                let vb = vld1q_f32(b_ptr.add(off));
                let vr = vmulq_f32(va, vb);
                vst1q_f32(o_ptr.add(off), vr);
            }
            let rem = chunks * 4;
            for i in rem..len {
                *o_ptr.add(i) = *a_ptr.add(i) * *b_ptr.add(i);
            }
        }
    }

    /// NEON f32x4 divide
    #[target_feature(enable = "neon")]
    pub(crate) unsafe fn div_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        unsafe {
            let len = a.len();
            let chunks = len / 4;
            let a_ptr = a.as_ptr();
            let b_ptr = b.as_ptr();
            let o_ptr = out.as_mut_ptr();

            for i in 0..chunks {
                let off = i * 4;
                let va = vld1q_f32(a_ptr.add(off));
                let vb = vld1q_f32(b_ptr.add(off));
                let vr = vdivq_f32(va, vb);
                vst1q_f32(o_ptr.add(off), vr);
            }
            let rem = chunks * 4;
            for i in rem..len {
                *o_ptr.add(i) = *a_ptr.add(i) / *b_ptr.add(i);
            }
        }
    }

    /// NEON f32x4 fused multiply-add: out = a * b + c
    #[target_feature(enable = "neon")]
    pub(crate) unsafe fn fma_f32(a: &[f32], b: &[f32], c: &[f32], out: &mut [f32]) {
        unsafe {
            let len = a.len();
            let chunks = len / 4;
            let a_ptr = a.as_ptr();
            let b_ptr = b.as_ptr();
            let c_ptr = c.as_ptr();
            let o_ptr = out.as_mut_ptr();

            for i in 0..chunks {
                let off = i * 4;
                let va = vld1q_f32(a_ptr.add(off));
                let vb = vld1q_f32(b_ptr.add(off));
                let vc = vld1q_f32(c_ptr.add(off));
                // vfmaq_f32 computes vc + va * vb (fused multiply-add)
                let vr = vfmaq_f32(vc, va, vb);
                vst1q_f32(o_ptr.add(off), vr);
            }
            let rem = chunks * 4;
            for i in rem..len {
                *o_ptr.add(i) = (*a_ptr.add(i)).mul_add(*b_ptr.add(i), *c_ptr.add(i));
            }
        }
    }

    /// NEON f32x4 min
    #[target_feature(enable = "neon")]
    pub(crate) unsafe fn min_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        unsafe {
            let len = a.len();
            let chunks = len / 4;
            let a_ptr = a.as_ptr();
            let b_ptr = b.as_ptr();
            let o_ptr = out.as_mut_ptr();

            for i in 0..chunks {
                let off = i * 4;
                let va = vld1q_f32(a_ptr.add(off));
                let vb = vld1q_f32(b_ptr.add(off));
                let vr = vminq_f32(va, vb);
                vst1q_f32(o_ptr.add(off), vr);
            }
            let rem = chunks * 4;
            for i in rem..len {
                *o_ptr.add(i) = (*a_ptr.add(i)).min(*b_ptr.add(i));
            }
        }
    }

    /// NEON f32x4 max
    #[target_feature(enable = "neon")]
    pub(crate) unsafe fn max_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        unsafe {
            let len = a.len();
            let chunks = len / 4;
            let a_ptr = a.as_ptr();
            let b_ptr = b.as_ptr();
            let o_ptr = out.as_mut_ptr();

            for i in 0..chunks {
                let off = i * 4;
                let va = vld1q_f32(a_ptr.add(off));
                let vb = vld1q_f32(b_ptr.add(off));
                let vr = vmaxq_f32(va, vb);
                vst1q_f32(o_ptr.add(off), vr);
            }
            let rem = chunks * 4;
            for i in rem..len {
                *o_ptr.add(i) = (*a_ptr.add(i)).max(*b_ptr.add(i));
            }
        }
    }

    /// NEON f32x4 clamp
    #[target_feature(enable = "neon")]
    pub(crate) unsafe fn clamp_f32(data: &[f32], min_val: f32, max_val: f32, out: &mut [f32]) {
        unsafe {
            let len = data.len();
            let chunks = len / 4;
            let d_ptr = data.as_ptr();
            let o_ptr = out.as_mut_ptr();
            let vmin = vdupq_n_f32(min_val);
            let vmax = vdupq_n_f32(max_val);

            for i in 0..chunks {
                let off = i * 4;
                let vd = vld1q_f32(d_ptr.add(off));
                let vr = vminq_f32(vmaxq_f32(vd, vmin), vmax);
                vst1q_f32(o_ptr.add(off), vr);
            }
            let rem = chunks * 4;
            for i in rem..len {
                *o_ptr.add(i) = (*d_ptr.add(i)).clamp(min_val, max_val);
            }
        }
    }

    /// NEON f32x4 scale and offset: out = data * scale + offset
    #[target_feature(enable = "neon")]
    pub(crate) unsafe fn scale_offset_f32(data: &[f32], scale: f32, offset: f32, out: &mut [f32]) {
        unsafe {
            let len = data.len();
            let chunks = len / 4;
            let d_ptr = data.as_ptr();
            let o_ptr = out.as_mut_ptr();
            let vscale = vdupq_n_f32(scale);
            let voffset = vdupq_n_f32(offset);

            for i in 0..chunks {
                let off = i * 4;
                let vd = vld1q_f32(d_ptr.add(off));
                // FMA: offset + data * scale
                let vr = vfmaq_f32(voffset, vd, vscale);
                vst1q_f32(o_ptr.add(off), vr);
            }
            let rem = chunks * 4;
            for i in rem..len {
                *o_ptr.add(i) = (*d_ptr.add(i)).mul_add(scale, offset);
            }
        }
    }

    /// NEON f32x4 threshold: out = if data >= threshold { 1.0 } else { 0.0 }
    #[target_feature(enable = "neon")]
    pub(crate) unsafe fn threshold_f32(data: &[f32], threshold: f32, out: &mut [f32]) {
        unsafe {
            let len = data.len();
            let chunks = len / 4;
            let d_ptr = data.as_ptr();
            let o_ptr = out.as_mut_ptr();
            let vthresh = vdupq_n_f32(threshold);
            let vone = vdupq_n_f32(1.0);
            let vzero = vdupq_n_f32(0.0);

            for i in 0..chunks {
                let off = i * 4;
                let vd = vld1q_f32(d_ptr.add(off));
                // Compare: data >= threshold -> all bits set or zero
                let mask = vcgeq_f32(vd, vthresh);
                // Use bitwise select: mask ? one : zero
                let vr = vbslq_f32(mask, vone, vzero);
                vst1q_f32(o_ptr.add(off), vr);
            }
            let rem = chunks * 4;
            for i in rem..len {
                *o_ptr.add(i) = if *d_ptr.add(i) >= threshold { 1.0 } else { 0.0 };
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
mod sse_impl {
    use std::arch::x86_64::*;

    /// SSE2 f32x4 add
    #[target_feature(enable = "sse2")]
    pub(crate) unsafe fn add_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        unsafe {
            let len = a.len();
            let chunks = len / 4;
            let a_ptr = a.as_ptr();
            let b_ptr = b.as_ptr();
            let o_ptr = out.as_mut_ptr();

            for i in 0..chunks {
                let off = i * 4;
                let va = _mm_loadu_ps(a_ptr.add(off));
                let vb = _mm_loadu_ps(b_ptr.add(off));
                let vr = _mm_add_ps(va, vb);
                _mm_storeu_ps(o_ptr.add(off), vr);
            }
            let rem = chunks * 4;
            for i in rem..len {
                *o_ptr.add(i) = *a_ptr.add(i) + *b_ptr.add(i);
            }
        }
    }

    /// SSE2 f32x4 subtract
    #[target_feature(enable = "sse2")]
    pub(crate) unsafe fn sub_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        unsafe {
            let len = a.len();
            let chunks = len / 4;
            let a_ptr = a.as_ptr();
            let b_ptr = b.as_ptr();
            let o_ptr = out.as_mut_ptr();

            for i in 0..chunks {
                let off = i * 4;
                let va = _mm_loadu_ps(a_ptr.add(off));
                let vb = _mm_loadu_ps(b_ptr.add(off));
                let vr = _mm_sub_ps(va, vb);
                _mm_storeu_ps(o_ptr.add(off), vr);
            }
            let rem = chunks * 4;
            for i in rem..len {
                *o_ptr.add(i) = *a_ptr.add(i) - *b_ptr.add(i);
            }
        }
    }

    /// SSE2 f32x4 multiply
    #[target_feature(enable = "sse2")]
    pub(crate) unsafe fn mul_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        unsafe {
            let len = a.len();
            let chunks = len / 4;
            let a_ptr = a.as_ptr();
            let b_ptr = b.as_ptr();
            let o_ptr = out.as_mut_ptr();

            for i in 0..chunks {
                let off = i * 4;
                let va = _mm_loadu_ps(a_ptr.add(off));
                let vb = _mm_loadu_ps(b_ptr.add(off));
                let vr = _mm_mul_ps(va, vb);
                _mm_storeu_ps(o_ptr.add(off), vr);
            }
            let rem = chunks * 4;
            for i in rem..len {
                *o_ptr.add(i) = *a_ptr.add(i) * *b_ptr.add(i);
            }
        }
    }

    /// SSE2 f32x4 divide
    #[target_feature(enable = "sse2")]
    pub(crate) unsafe fn div_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        unsafe {
            let len = a.len();
            let chunks = len / 4;
            let a_ptr = a.as_ptr();
            let b_ptr = b.as_ptr();
            let o_ptr = out.as_mut_ptr();

            for i in 0..chunks {
                let off = i * 4;
                let va = _mm_loadu_ps(a_ptr.add(off));
                let vb = _mm_loadu_ps(b_ptr.add(off));
                let vr = _mm_div_ps(va, vb);
                _mm_storeu_ps(o_ptr.add(off), vr);
            }
            let rem = chunks * 4;
            for i in rem..len {
                *o_ptr.add(i) = *a_ptr.add(i) / *b_ptr.add(i);
            }
        }
    }

    /// SSE2 f32x4 min
    #[target_feature(enable = "sse2")]
    pub(crate) unsafe fn min_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        unsafe {
            let len = a.len();
            let chunks = len / 4;
            let a_ptr = a.as_ptr();
            let b_ptr = b.as_ptr();
            let o_ptr = out.as_mut_ptr();

            for i in 0..chunks {
                let off = i * 4;
                let va = _mm_loadu_ps(a_ptr.add(off));
                let vb = _mm_loadu_ps(b_ptr.add(off));
                let vr = _mm_min_ps(va, vb);
                _mm_storeu_ps(o_ptr.add(off), vr);
            }
            let rem = chunks * 4;
            for i in rem..len {
                *o_ptr.add(i) = (*a_ptr.add(i)).min(*b_ptr.add(i));
            }
        }
    }

    /// SSE2 f32x4 max
    #[target_feature(enable = "sse2")]
    pub(crate) unsafe fn max_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        unsafe {
            let len = a.len();
            let chunks = len / 4;
            let a_ptr = a.as_ptr();
            let b_ptr = b.as_ptr();
            let o_ptr = out.as_mut_ptr();

            for i in 0..chunks {
                let off = i * 4;
                let va = _mm_loadu_ps(a_ptr.add(off));
                let vb = _mm_loadu_ps(b_ptr.add(off));
                let vr = _mm_max_ps(va, vb);
                _mm_storeu_ps(o_ptr.add(off), vr);
            }
            let rem = chunks * 4;
            for i in rem..len {
                *o_ptr.add(i) = (*a_ptr.add(i)).max(*b_ptr.add(i));
            }
        }
    }

    /// SSE2 f32x4 clamp
    #[target_feature(enable = "sse2")]
    pub(crate) unsafe fn clamp_f32(data: &[f32], min_val: f32, max_val: f32, out: &mut [f32]) {
        unsafe {
            let len = data.len();
            let chunks = len / 4;
            let d_ptr = data.as_ptr();
            let o_ptr = out.as_mut_ptr();
            let vmin = _mm_set1_ps(min_val);
            let vmax = _mm_set1_ps(max_val);

            for i in 0..chunks {
                let off = i * 4;
                let vd = _mm_loadu_ps(d_ptr.add(off));
                let vr = _mm_min_ps(_mm_max_ps(vd, vmin), vmax);
                _mm_storeu_ps(o_ptr.add(off), vr);
            }
            let rem = chunks * 4;
            for i in rem..len {
                *o_ptr.add(i) = (*d_ptr.add(i)).clamp(min_val, max_val);
            }
        }
    }

    /// SSE2 f32x4 scale + offset
    #[target_feature(enable = "sse2")]
    pub(crate) unsafe fn scale_offset_f32(data: &[f32], scale: f32, offset: f32, out: &mut [f32]) {
        unsafe {
            let len = data.len();
            let chunks = len / 4;
            let d_ptr = data.as_ptr();
            let o_ptr = out.as_mut_ptr();
            let vscale = _mm_set1_ps(scale);
            let voffset = _mm_set1_ps(offset);

            for i in 0..chunks {
                let off = i * 4;
                let vd = _mm_loadu_ps(d_ptr.add(off));
                let vr = _mm_add_ps(_mm_mul_ps(vd, vscale), voffset);
                _mm_storeu_ps(o_ptr.add(off), vr);
            }
            let rem = chunks * 4;
            for i in rem..len {
                *o_ptr.add(i) = (*d_ptr.add(i)).mul_add(scale, offset);
            }
        }
    }

    /// SSE2 f32x4 threshold
    #[target_feature(enable = "sse2")]
    pub(crate) unsafe fn threshold_f32(data: &[f32], threshold: f32, out: &mut [f32]) {
        unsafe {
            let len = data.len();
            let chunks = len / 4;
            let d_ptr = data.as_ptr();
            let o_ptr = out.as_mut_ptr();
            let vthresh = _mm_set1_ps(threshold);
            let vone = _mm_set1_ps(1.0);

            for i in 0..chunks {
                let off = i * 4;
                let vd = _mm_loadu_ps(d_ptr.add(off));
                // Compare: data >= threshold (NLT = not less than)
                let mask = _mm_cmpge_ps(vd, vthresh);
                // AND with 1.0: mask ? 1.0 : 0.0
                let vr = _mm_and_ps(mask, vone);
                _mm_storeu_ps(o_ptr.add(off), vr);
            }
            let rem = chunks * 4;
            for i in rem..len {
                *o_ptr.add(i) = if *d_ptr.add(i) >= threshold { 1.0 } else { 0.0 };
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
mod avx2_impl {
    use std::arch::x86_64::*;

    /// AVX2 f32x8 add
    #[target_feature(enable = "avx2")]
    pub(crate) unsafe fn add_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        unsafe {
            let len = a.len();
            let chunks = len / 8;
            let a_ptr = a.as_ptr();
            let b_ptr = b.as_ptr();
            let o_ptr = out.as_mut_ptr();

            for i in 0..chunks {
                let off = i * 8;
                let va = _mm256_loadu_ps(a_ptr.add(off));
                let vb = _mm256_loadu_ps(b_ptr.add(off));
                let vr = _mm256_add_ps(va, vb);
                _mm256_storeu_ps(o_ptr.add(off), vr);
            }
            // Handle remainder
            let rem = chunks * 8;
            for i in rem..len {
                *o_ptr.add(i) = *a_ptr.add(i) + *b_ptr.add(i);
            }
        }
    }

    /// AVX2 f32x8 subtract
    #[target_feature(enable = "avx2")]
    pub(crate) unsafe fn sub_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        unsafe {
            let len = a.len();
            let chunks = len / 8;
            let a_ptr = a.as_ptr();
            let b_ptr = b.as_ptr();
            let o_ptr = out.as_mut_ptr();

            for i in 0..chunks {
                let off = i * 8;
                let va = _mm256_loadu_ps(a_ptr.add(off));
                let vb = _mm256_loadu_ps(b_ptr.add(off));
                let vr = _mm256_sub_ps(va, vb);
                _mm256_storeu_ps(o_ptr.add(off), vr);
            }
            let rem = chunks * 8;
            for i in rem..len {
                *o_ptr.add(i) = *a_ptr.add(i) - *b_ptr.add(i);
            }
        }
    }

    /// AVX2 f32x8 multiply
    #[target_feature(enable = "avx2")]
    pub(crate) unsafe fn mul_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        unsafe {
            let len = a.len();
            let chunks = len / 8;
            let a_ptr = a.as_ptr();
            let b_ptr = b.as_ptr();
            let o_ptr = out.as_mut_ptr();

            for i in 0..chunks {
                let off = i * 8;
                let va = _mm256_loadu_ps(a_ptr.add(off));
                let vb = _mm256_loadu_ps(b_ptr.add(off));
                let vr = _mm256_mul_ps(va, vb);
                _mm256_storeu_ps(o_ptr.add(off), vr);
            }
            let rem = chunks * 8;
            for i in rem..len {
                *o_ptr.add(i) = *a_ptr.add(i) * *b_ptr.add(i);
            }
        }
    }

    /// AVX2 f32x8 divide
    #[target_feature(enable = "avx2")]
    pub(crate) unsafe fn div_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        unsafe {
            let len = a.len();
            let chunks = len / 8;
            let a_ptr = a.as_ptr();
            let b_ptr = b.as_ptr();
            let o_ptr = out.as_mut_ptr();

            for i in 0..chunks {
                let off = i * 8;
                let va = _mm256_loadu_ps(a_ptr.add(off));
                let vb = _mm256_loadu_ps(b_ptr.add(off));
                let vr = _mm256_div_ps(va, vb);
                _mm256_storeu_ps(o_ptr.add(off), vr);
            }
            let rem = chunks * 8;
            for i in rem..len {
                *o_ptr.add(i) = *a_ptr.add(i) / *b_ptr.add(i);
            }
        }
    }

    /// AVX2 f32x8 min
    #[target_feature(enable = "avx2")]
    pub(crate) unsafe fn min_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        unsafe {
            let len = a.len();
            let chunks = len / 8;
            let a_ptr = a.as_ptr();
            let b_ptr = b.as_ptr();
            let o_ptr = out.as_mut_ptr();

            for i in 0..chunks {
                let off = i * 8;
                let va = _mm256_loadu_ps(a_ptr.add(off));
                let vb = _mm256_loadu_ps(b_ptr.add(off));
                let vr = _mm256_min_ps(va, vb);
                _mm256_storeu_ps(o_ptr.add(off), vr);
            }
            let rem = chunks * 8;
            for i in rem..len {
                *o_ptr.add(i) = (*a_ptr.add(i)).min(*b_ptr.add(i));
            }
        }
    }

    /// AVX2 f32x8 max
    #[target_feature(enable = "avx2")]
    pub(crate) unsafe fn max_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        unsafe {
            let len = a.len();
            let chunks = len / 8;
            let a_ptr = a.as_ptr();
            let b_ptr = b.as_ptr();
            let o_ptr = out.as_mut_ptr();

            for i in 0..chunks {
                let off = i * 8;
                let va = _mm256_loadu_ps(a_ptr.add(off));
                let vb = _mm256_loadu_ps(b_ptr.add(off));
                let vr = _mm256_max_ps(va, vb);
                _mm256_storeu_ps(o_ptr.add(off), vr);
            }
            let rem = chunks * 8;
            for i in rem..len {
                *o_ptr.add(i) = (*a_ptr.add(i)).max(*b_ptr.add(i));
            }
        }
    }

    /// AVX2 f32x8 clamp
    #[target_feature(enable = "avx2")]
    pub(crate) unsafe fn clamp_f32(data: &[f32], min_val: f32, max_val: f32, out: &mut [f32]) {
        unsafe {
            let len = data.len();
            let chunks = len / 8;
            let d_ptr = data.as_ptr();
            let o_ptr = out.as_mut_ptr();
            let vmin = _mm256_set1_ps(min_val);
            let vmax = _mm256_set1_ps(max_val);

            for i in 0..chunks {
                let off = i * 8;
                let vd = _mm256_loadu_ps(d_ptr.add(off));
                let vr = _mm256_min_ps(_mm256_max_ps(vd, vmin), vmax);
                _mm256_storeu_ps(o_ptr.add(off), vr);
            }
            let rem = chunks * 8;
            for i in rem..len {
                *o_ptr.add(i) = (*d_ptr.add(i)).clamp(min_val, max_val);
            }
        }
    }

    /// AVX2+FMA f32x8 scale + offset
    #[target_feature(enable = "avx2", enable = "fma")]
    pub(crate) unsafe fn scale_offset_f32_fma(
        data: &[f32],
        scale: f32,
        offset: f32,
        out: &mut [f32],
    ) {
        unsafe {
            let len = data.len();
            let chunks = len / 8;
            let d_ptr = data.as_ptr();
            let o_ptr = out.as_mut_ptr();
            let vscale = _mm256_set1_ps(scale);
            let voffset = _mm256_set1_ps(offset);

            for i in 0..chunks {
                let off = i * 8;
                let vd = _mm256_loadu_ps(d_ptr.add(off));
                // FMA: data * scale + offset
                let vr = _mm256_fmadd_ps(vd, vscale, voffset);
                _mm256_storeu_ps(o_ptr.add(off), vr);
            }
            let rem = chunks * 8;
            for i in rem..len {
                *o_ptr.add(i) = (*d_ptr.add(i)).mul_add(scale, offset);
            }
        }
    }

    /// AVX2 f32x8 scale + offset (without FMA)
    #[target_feature(enable = "avx2")]
    pub(crate) unsafe fn scale_offset_f32(data: &[f32], scale: f32, offset: f32, out: &mut [f32]) {
        unsafe {
            let len = data.len();
            let chunks = len / 8;
            let d_ptr = data.as_ptr();
            let o_ptr = out.as_mut_ptr();
            let vscale = _mm256_set1_ps(scale);
            let voffset = _mm256_set1_ps(offset);

            for i in 0..chunks {
                let off = i * 8;
                let vd = _mm256_loadu_ps(d_ptr.add(off));
                let vr = _mm256_add_ps(_mm256_mul_ps(vd, vscale), voffset);
                _mm256_storeu_ps(o_ptr.add(off), vr);
            }
            let rem = chunks * 8;
            for i in rem..len {
                *o_ptr.add(i) = (*d_ptr.add(i)).mul_add(scale, offset);
            }
        }
    }
}

/// Scalar fallback implementations
mod scalar_impl {
    pub(crate) fn add_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        for i in 0..a.len() {
            out[i] = a[i] + b[i];
        }
    }
    pub(crate) fn sub_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        for i in 0..a.len() {
            out[i] = a[i] - b[i];
        }
    }
    pub(crate) fn mul_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        for i in 0..a.len() {
            out[i] = a[i] * b[i];
        }
    }
    pub(crate) fn div_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        for i in 0..a.len() {
            out[i] = a[i] / b[i];
        }
    }
    pub(crate) fn fma_f32(a: &[f32], b: &[f32], c: &[f32], out: &mut [f32]) {
        for i in 0..a.len() {
            out[i] = a[i].mul_add(b[i], c[i]);
        }
    }
    pub(crate) fn min_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        for i in 0..a.len() {
            out[i] = a[i].min(b[i]);
        }
    }
    pub(crate) fn max_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
        for i in 0..a.len() {
            out[i] = a[i].max(b[i]);
        }
    }
    pub(crate) fn clamp_f32(data: &[f32], min_val: f32, max_val: f32, out: &mut [f32]) {
        for i in 0..data.len() {
            out[i] = data[i].clamp(min_val, max_val);
        }
    }
    pub(crate) fn threshold_f32(data: &[f32], threshold: f32, out: &mut [f32]) {
        for i in 0..data.len() {
            out[i] = if data[i] >= threshold { 1.0 } else { 0.0 };
        }
    }
    pub(crate) fn scale_offset_f32(data: &[f32], scale: f32, offset: f32, out: &mut [f32]) {
        for i in 0..data.len() {
            out[i] = data[i].mul_add(scale, offset);
        }
    }
}

// ============================================================================
// Dispatch helpers - route to best available SIMD implementation
// ============================================================================

/// Dispatch a binary f32 operation to the best available SIMD implementation
#[inline]
fn dispatch_binary_f32(
    a: &[f32],
    b: &[f32],
    out: &mut [f32],
    #[cfg(target_arch = "aarch64")] neon_fn: unsafe fn(&[f32], &[f32], &mut [f32]),
    #[cfg(target_arch = "x86_64")] sse_fn: unsafe fn(&[f32], &[f32], &mut [f32]),
    #[cfg(target_arch = "x86_64")] avx2_fn: unsafe fn(&[f32], &[f32], &mut [f32]),
    scalar_fn: fn(&[f32], &[f32], &mut [f32]),
) {
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON is always available on aarch64, slices are validated by caller
        unsafe {
            neon_fn(a, b, out);
        }
    }

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            // SAFETY: AVX2 runtime detected, slices validated by caller
            unsafe {
                avx2_fn(a, b, out);
            }
        } else {
            // SAFETY: SSE2 is baseline on x86-64, slices validated by caller
            unsafe {
                sse_fn(a, b, out);
            }
        }
    }

    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        scalar_fn(a, b, out);
    }
}

// ============================================================================
// Public API - safe wrappers with validation
// ============================================================================

/// Helper for validating binary operation inputs
fn validate_binary(a: &[f32], b: &[f32], out: &[f32]) -> Result<()> {
    if a.len() != b.len() || a.len() != out.len() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "input",
            message: format!(
                "Slice length mismatch: a={}, b={}, out={}",
                a.len(),
                b.len(),
                out.len()
            ),
        });
    }
    Ok(())
}

/// Helper for validating unary operation inputs
fn validate_unary(data: &[f32], out: &[f32]) -> Result<()> {
    if data.len() != out.len() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "input",
            message: format!(
                "Slice length mismatch: data={}, out={}",
                data.len(),
                out.len()
            ),
        });
    }
    Ok(())
}

/// Add two f32 slices element-wise using SIMD
///
/// Computes `out[i] = a[i] + b[i]` for all elements.
/// Automatically dispatches to NEON (aarch64) or SSE2/AVX2 (x86-64).
///
/// # Errors
///
/// Returns an error if slice lengths don't match.
pub fn add_f32(a: &[f32], b: &[f32], out: &mut [f32]) -> Result<()> {
    validate_binary(a, b, out)?;
    dispatch_binary_f32(
        a,
        b,
        out,
        #[cfg(target_arch = "aarch64")]
        neon_impl::add_f32,
        #[cfg(target_arch = "x86_64")]
        sse_impl::add_f32,
        #[cfg(target_arch = "x86_64")]
        avx2_impl::add_f32,
        scalar_impl::add_f32,
    );
    Ok(())
}

/// Subtract two f32 slices element-wise using SIMD
///
/// Computes `out[i] = a[i] - b[i]` for all elements.
pub fn sub_f32(a: &[f32], b: &[f32], out: &mut [f32]) -> Result<()> {
    validate_binary(a, b, out)?;
    dispatch_binary_f32(
        a,
        b,
        out,
        #[cfg(target_arch = "aarch64")]
        neon_impl::sub_f32,
        #[cfg(target_arch = "x86_64")]
        sse_impl::sub_f32,
        #[cfg(target_arch = "x86_64")]
        avx2_impl::sub_f32,
        scalar_impl::sub_f32,
    );
    Ok(())
}

/// Multiply two f32 slices element-wise using SIMD
///
/// Computes `out[i] = a[i] * b[i]` for all elements.
pub fn mul_f32(a: &[f32], b: &[f32], out: &mut [f32]) -> Result<()> {
    validate_binary(a, b, out)?;
    dispatch_binary_f32(
        a,
        b,
        out,
        #[cfg(target_arch = "aarch64")]
        neon_impl::mul_f32,
        #[cfg(target_arch = "x86_64")]
        sse_impl::mul_f32,
        #[cfg(target_arch = "x86_64")]
        avx2_impl::mul_f32,
        scalar_impl::mul_f32,
    );
    Ok(())
}

/// Divide two f32 slices element-wise using SIMD
///
/// Computes `out[i] = a[i] / b[i]` for all elements.
pub fn div_f32(a: &[f32], b: &[f32], out: &mut [f32]) -> Result<()> {
    validate_binary(a, b, out)?;
    dispatch_binary_f32(
        a,
        b,
        out,
        #[cfg(target_arch = "aarch64")]
        neon_impl::div_f32,
        #[cfg(target_arch = "x86_64")]
        sse_impl::div_f32,
        #[cfg(target_arch = "x86_64")]
        avx2_impl::div_f32,
        scalar_impl::div_f32,
    );
    Ok(())
}

/// Fused multiply-add operation: out\[i\] = a\[i\] * b\[i\] + c\[i\]
///
/// This is more efficient than separate multiply and add on CPUs with FMA support.
/// Uses hardware FMA on aarch64 (NEON) and x86-64 (when FMA extension is available).
pub fn fma_f32(a: &[f32], b: &[f32], c: &[f32], out: &mut [f32]) -> Result<()> {
    if a.len() != b.len() || a.len() != c.len() || a.len() != out.len() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "input",
            message: "Slice length mismatch".to_string(),
        });
    }

    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON always available on aarch64, lengths validated above
        unsafe {
            neon_impl::fma_f32(a, b, c, out);
        }
    }

    #[cfg(not(target_arch = "aarch64"))]
    {
        // On x86-64, scalar mul_add may emit hardware FMA if available
        scalar_impl::fma_f32(a, b, c, out);
    }

    Ok(())
}

/// Compute element-wise minimum of two f32 slices
pub fn min_f32(a: &[f32], b: &[f32], out: &mut [f32]) -> Result<()> {
    validate_binary(a, b, out)?;
    dispatch_binary_f32(
        a,
        b,
        out,
        #[cfg(target_arch = "aarch64")]
        neon_impl::min_f32,
        #[cfg(target_arch = "x86_64")]
        sse_impl::min_f32,
        #[cfg(target_arch = "x86_64")]
        avx2_impl::min_f32,
        scalar_impl::min_f32,
    );
    Ok(())
}

/// Compute element-wise maximum of two f32 slices
pub fn max_f32(a: &[f32], b: &[f32], out: &mut [f32]) -> Result<()> {
    validate_binary(a, b, out)?;
    dispatch_binary_f32(
        a,
        b,
        out,
        #[cfg(target_arch = "aarch64")]
        neon_impl::max_f32,
        #[cfg(target_arch = "x86_64")]
        sse_impl::max_f32,
        #[cfg(target_arch = "x86_64")]
        avx2_impl::max_f32,
        scalar_impl::max_f32,
    );
    Ok(())
}

/// Clamp values to a range: out\[i\] = clamp(data\[i\], min, max)
pub fn clamp_f32(data: &[f32], min: f32, max: f32, out: &mut [f32]) -> Result<()> {
    validate_unary(data, out)?;

    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON always available, lengths validated
        unsafe {
            neon_impl::clamp_f32(data, min, max, out);
        }
        Ok(())
    }

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            // SAFETY: AVX2 detected, lengths validated
            unsafe {
                avx2_impl::clamp_f32(data, min, max, out);
            }
            Ok(())
        } else {
            // SAFETY: SSE2 baseline, lengths validated
            unsafe {
                sse_impl::clamp_f32(data, min, max, out);
            }
            Ok(())
        }
    }

    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        scalar_impl::clamp_f32(data, min, max, out);
        Ok(())
    }
}

/// Apply threshold: out\[i\] = if data\[i\] >= threshold { 1.0 } else { 0.0 }
pub fn threshold_f32(data: &[f32], threshold: f32, out: &mut [f32]) -> Result<()> {
    validate_unary(data, out)?;

    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON always available, lengths validated
        unsafe {
            neon_impl::threshold_f32(data, threshold, out);
        }
        Ok(())
    }

    #[cfg(target_arch = "x86_64")]
    {
        // Use SSE2 for threshold (AVX2 version similar but not worth the dispatch overhead)
        // SAFETY: SSE2 baseline, lengths validated
        unsafe {
            sse_impl::threshold_f32(data, threshold, out);
        }
        Ok(())
    }

    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        scalar_impl::threshold_f32(data, threshold, out);
        Ok(())
    }
}

/// Convert u8 to f32 with scaling: out\[i\] = data\[i\] as f32 / 255.0
pub fn u8_to_f32_normalized(data: &[u8], out: &mut [f32]) -> Result<()> {
    if data.len() != out.len() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "input",
            message: "Slice length mismatch".to_string(),
        });
    }

    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON always available on aarch64
        unsafe {
            use std::arch::aarch64::*;
            let len = data.len();
            let inv255 = vdupq_n_f32(1.0 / 255.0);
            let chunks = len / 16; // Process 16 u8 -> 16 f32

            for i in 0..chunks {
                let off = i * 16;
                // Load 16 u8 values
                let v_u8 = vld1q_u8(data.as_ptr().add(off));

                // Widen u8x16 -> two u16x8
                let lo_u16 = vmovl_u8(vget_low_u8(v_u8));
                let hi_u16 = vmovl_u8(vget_high_u8(v_u8));

                // Widen u16x8 -> four u32x4
                let a_u32 = vmovl_u16(vget_low_u16(lo_u16));
                let b_u32 = vmovl_u16(vget_high_u16(lo_u16));
                let c_u32 = vmovl_u16(vget_low_u16(hi_u16));
                let d_u32 = vmovl_u16(vget_high_u16(hi_u16));

                // Convert u32x4 -> f32x4 and multiply by 1/255
                let a_f32 = vmulq_f32(vcvtq_f32_u32(a_u32), inv255);
                let b_f32 = vmulq_f32(vcvtq_f32_u32(b_u32), inv255);
                let c_f32 = vmulq_f32(vcvtq_f32_u32(c_u32), inv255);
                let d_f32 = vmulq_f32(vcvtq_f32_u32(d_u32), inv255);

                vst1q_f32(out.as_mut_ptr().add(off), a_f32);
                vst1q_f32(out.as_mut_ptr().add(off + 4), b_f32);
                vst1q_f32(out.as_mut_ptr().add(off + 8), c_f32);
                vst1q_f32(out.as_mut_ptr().add(off + 12), d_f32);
            }

            let rem = chunks * 16;
            for i in rem..len {
                out[i] = f32::from(data[i]) / 255.0;
            }
        }
        Ok(())
    }

    #[cfg(not(target_arch = "aarch64"))]
    {
        for i in 0..data.len() {
            out[i] = f32::from(data[i]) / 255.0;
        }
        Ok(())
    }
}

/// Convert f32 to u8 with scaling: out\[i\] = (data\[i\] * 255.0).clamp(0, 255) as u8
pub fn f32_to_u8_normalized(data: &[f32], out: &mut [u8]) -> Result<()> {
    if data.len() != out.len() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "input",
            message: "Slice length mismatch".to_string(),
        });
    }

    // Scalar implementation with auto-vectorization hints
    const LANES: usize = 8;
    let chunks = data.len() / LANES;

    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;
        for j in start..end {
            let scaled = (data[j] * 255.0).clamp(0.0, 255.0);
            out[j] = scaled as u8;
        }
    }

    let remainder_start = chunks * LANES;
    for i in remainder_start..data.len() {
        let scaled = (data[i] * 255.0).clamp(0.0, 255.0);
        out[i] = scaled as u8;
    }

    Ok(())
}

/// Apply mask: out\[i\] = if mask\[i\] != 0 { data\[i\] } else { fill }
pub fn apply_mask_f32(data: &[f32], mask: &[u8], fill: f32, out: &mut [f32]) -> Result<()> {
    if data.len() != mask.len() || data.len() != out.len() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "input",
            message: "Slice length mismatch".to_string(),
        });
    }

    // SIMD-friendly loop structure for auto-vectorization
    const LANES: usize = 8;
    let chunks = data.len() / LANES;

    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;
        for j in start..end {
            out[j] = if mask[j] != 0 { data[j] } else { fill };
        }
    }

    let remainder_start = chunks * LANES;
    for i in remainder_start..data.len() {
        out[i] = if mask[i] != 0 { data[i] } else { fill };
    }

    Ok(())
}

/// Scale and offset: out\[i\] = data\[i\] * scale + offset
///
/// Uses hardware FMA (fused multiply-add) on supported platforms.
pub fn scale_offset_f32(data: &[f32], scale: f32, offset: f32, out: &mut [f32]) -> Result<()> {
    validate_unary(data, out)?;

    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: NEON always available, lengths validated
        unsafe {
            neon_impl::scale_offset_f32(data, scale, offset, out);
        }
        Ok(())
    }

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") {
            // SAFETY: AVX2+FMA detected, lengths validated
            unsafe {
                avx2_impl::scale_offset_f32_fma(data, scale, offset, out);
            }
            Ok(())
        } else if is_x86_feature_detected!("avx2") {
            // SAFETY: AVX2 detected (no FMA), lengths validated
            unsafe {
                avx2_impl::scale_offset_f32(data, scale, offset, out);
            }
            Ok(())
        } else {
            // SAFETY: SSE2 baseline, lengths validated
            unsafe {
                sse_impl::scale_offset_f32(data, scale, offset, out);
            }
            Ok(())
        }
    }

    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        scalar_impl::scale_offset_f32(data, scale, offset, out);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_add_f32() {
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let b = vec![5.0, 4.0, 3.0, 2.0, 1.0];
        let mut out = vec![0.0; 5];

        add_f32(&a, &b, &mut out).expect("add_f32 failed");

        for &val in &out {
            assert_relative_eq!(val, 6.0);
        }
    }

    #[test]
    fn test_add_f32_large() {
        let n = 1000;
        let a = vec![1.0; n];
        let b = vec![2.0; n];
        let mut out = vec![0.0; n];

        add_f32(&a, &b, &mut out).expect("add_f32 large failed");

        for &val in &out {
            assert_relative_eq!(val, 3.0);
        }
    }

    #[test]
    fn test_sub_f32() {
        let a = vec![5.0, 4.0, 3.0, 2.0, 1.0];
        let b = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let mut out = vec![0.0; 5];

        sub_f32(&a, &b, &mut out).expect("sub_f32 failed");

        assert_relative_eq!(out[0], 4.0);
        assert_relative_eq!(out[2], 0.0);
        assert_relative_eq!(out[4], -4.0);
    }

    #[test]
    fn test_mul_f32() {
        let a = vec![2.0, 3.0, 4.0, 5.0];
        let b = vec![3.0, 4.0, 5.0, 6.0];
        let mut out = vec![0.0; 4];

        mul_f32(&a, &b, &mut out).expect("mul_f32 failed");

        assert_relative_eq!(out[0], 6.0);
        assert_relative_eq!(out[1], 12.0);
        assert_relative_eq!(out[2], 20.0);
        assert_relative_eq!(out[3], 30.0);
    }

    #[test]
    fn test_div_f32() {
        let a = vec![6.0, 12.0, 20.0, 30.0];
        let b = vec![2.0, 3.0, 4.0, 5.0];
        let mut out = vec![0.0; 4];

        div_f32(&a, &b, &mut out).expect("div_f32 failed");

        assert_relative_eq!(out[0], 3.0);
        assert_relative_eq!(out[1], 4.0);
        assert_relative_eq!(out[2], 5.0);
        assert_relative_eq!(out[3], 6.0);
    }

    #[test]
    fn test_fma_f32() {
        let a = vec![2.0; 10];
        let b = vec![3.0; 10];
        let c = vec![4.0; 10];
        let mut out = vec![0.0; 10];

        fma_f32(&a, &b, &c, &mut out).expect("fma_f32 failed");

        for &val in &out {
            assert_relative_eq!(val, 10.0); // 2*3 + 4 = 10
        }
    }

    #[test]
    fn test_min_max_f32() {
        let a = vec![1.0, 5.0, 3.0, 7.0];
        let b = vec![3.0, 2.0, 6.0, 4.0];
        let mut out_min = vec![0.0; 4];
        let mut out_max = vec![0.0; 4];

        min_f32(&a, &b, &mut out_min).expect("min_f32 failed");
        max_f32(&a, &b, &mut out_max).expect("max_f32 failed");

        assert_relative_eq!(out_min[0], 1.0);
        assert_relative_eq!(out_min[1], 2.0);
        assert_relative_eq!(out_max[2], 6.0);
        assert_relative_eq!(out_max[3], 7.0);
    }

    #[test]
    fn test_clamp_f32() {
        let data = vec![-1.0, 0.5, 2.0, 5.0, 10.0];
        let mut out = vec![0.0; 5];

        clamp_f32(&data, 0.0, 5.0, &mut out).expect("clamp_f32 failed");

        assert_relative_eq!(out[0], 0.0);
        assert_relative_eq!(out[1], 0.5);
        assert_relative_eq!(out[2], 2.0);
        assert_relative_eq!(out[3], 5.0);
        assert_relative_eq!(out[4], 5.0);
    }

    #[test]
    fn test_threshold_f32() {
        let data = vec![0.5, 1.5, 2.5, 3.5];
        let mut out = vec![0.0; 4];

        threshold_f32(&data, 2.0, &mut out).expect("threshold_f32 failed");

        assert_relative_eq!(out[0], 0.0);
        assert_relative_eq!(out[1], 0.0);
        assert_relative_eq!(out[2], 1.0);
        assert_relative_eq!(out[3], 1.0);
    }

    #[test]
    fn test_u8_to_f32_conversion() {
        let data = vec![0, 128, 255];
        let mut out = vec![0.0; 3];

        u8_to_f32_normalized(&data, &mut out).expect("u8_to_f32 failed");

        assert_relative_eq!(out[0], 0.0);
        assert_relative_eq!(out[1], 128.0 / 255.0, epsilon = 1e-6);
        assert_relative_eq!(out[2], 1.0);
    }

    #[test]
    fn test_u8_to_f32_large() {
        // Test with >16 elements to exercise SIMD path
        let data: Vec<u8> = (0..=255).cycle().take(1024).collect();
        let mut out = vec![0.0; 1024];

        u8_to_f32_normalized(&data, &mut out).expect("u8_to_f32 large failed");

        for i in 0..1024 {
            assert_relative_eq!(out[i], f32::from(data[i]) / 255.0, epsilon = 1e-5);
        }
    }

    #[test]
    fn test_f32_to_u8_conversion() {
        let data = vec![0.0, 0.5, 1.0, 1.5];
        let mut out = vec![0; 4];

        f32_to_u8_normalized(&data, &mut out).expect("f32_to_u8 failed");

        assert_eq!(out[0], 0);
        assert_eq!(out[1], 127); // 0.5 * 255 = 127.5 -> 127
        assert_eq!(out[2], 255);
        assert_eq!(out[3], 255); // Clamped
    }

    #[test]
    fn test_apply_mask() {
        let data = vec![1.0, 2.0, 3.0, 4.0];
        let mask = vec![1, 0, 1, 0];
        let mut out = vec![0.0; 4];

        apply_mask_f32(&data, &mask, -999.0, &mut out).expect("apply_mask failed");

        assert_relative_eq!(out[0], 1.0);
        assert_relative_eq!(out[1], -999.0);
        assert_relative_eq!(out[2], 3.0);
        assert_relative_eq!(out[3], -999.0);
    }

    #[test]
    fn test_scale_offset() {
        let data = vec![1.0, 2.0, 3.0, 4.0];
        let mut out = vec![0.0; 4];

        scale_offset_f32(&data, 2.0, 10.0, &mut out).expect("scale_offset failed");

        assert_relative_eq!(out[0], 12.0); // 1*2 + 10
        assert_relative_eq!(out[1], 14.0);
        assert_relative_eq!(out[2], 16.0);
        assert_relative_eq!(out[3], 18.0);
    }

    #[test]
    fn test_scale_offset_large() {
        let n = 10000;
        let data: Vec<f32> = (0..n).map(|i| i as f32).collect();
        let mut out = vec![0.0; n];

        scale_offset_f32(&data, 2.0, 1.0, &mut out).expect("scale_offset large failed");

        for i in 0..n {
            assert_relative_eq!(out[i], (i as f32) * 2.0 + 1.0, epsilon = 1e-4);
        }
    }

    #[test]
    fn test_length_mismatch() {
        let a = vec![1.0; 10];
        let b = vec![2.0; 5];
        let mut out = vec![0.0; 10];

        assert!(add_f32(&a, &b, &mut out).is_err());
    }

    #[test]
    fn test_simd_dispatch_info() {
        // Verify we can query what SIMD level is being used
        let caps = crate::simd::detect::capabilities();
        #[cfg(target_arch = "aarch64")]
        assert!(caps.has_neon);

        #[cfg(target_arch = "x86_64")]
        assert!(caps.has_sse2);

        let _ = caps; // suppress unused warning on other arches
    }
}
