//! Portable floating-point math for `std` and `no_std` targets.
//!
//! `core` only exposes the floating-point operations that map to hardware
//! instructions on every target (`abs`, `copysign`, comparisons, …). The
//! transcendental and fused operations (`sqrt`, `floor`, `atan2`, `mul_add`,
//! …) live in `std` because on a hosted platform they are backed by the C
//! runtime's `libm`. On a bare-metal `no_std` target there is no libstd, so
//! those methods are unavailable.
//!
//! To keep the always-compiled `types`/`buffer` code working on genuine
//! embedded targets, this module provides the [`FloatExt`] extension trait that
//! routes the missing operations through the pure-Rust [`libm`] crate. It is
//! only compiled for `no_std` builds (`#[cfg(not(feature = "std"))]`); under
//! `std` the inherent `f64` methods are used directly and `libm` is not linked
//! in, so the hosted build is byte-for-byte identical to before.

/// Floating-point operations that `core` does not provide, backed by the
/// pure-Rust `libm` crate on `no_std` targets.
///
/// This trait intentionally mirrors the signatures of the corresponding
/// inherent `f64` methods so that call sites are identical between `std` and
/// `no_std` builds — the only difference is a `use crate::math::FloatExt;`
/// import that is compiled in exclusively for `no_std`.
#[cfg(not(feature = "std"))]
pub(crate) trait FloatExt {
    /// Fused multiply-add: `self * a + b` computed with a single rounding.
    fn mul_add(self, a: Self, b: Self) -> Self;
    /// Square root.
    fn sqrt(self) -> Self;
    /// Largest integer less than or equal to `self`.
    fn floor(self) -> Self;
    /// Four-quadrant arctangent of `self` (`y`) and `other` (`x`).
    fn atan2(self, other: Self) -> Self;
}

#[cfg(not(feature = "std"))]
impl FloatExt for f64 {
    #[inline]
    fn mul_add(self, a: Self, b: Self) -> Self {
        libm::fma(self, a, b)
    }

    #[inline]
    fn sqrt(self) -> Self {
        libm::sqrt(self)
    }

    #[inline]
    fn floor(self) -> Self {
        libm::floor(self)
    }

    #[inline]
    fn atan2(self, other: Self) -> Self {
        libm::atan2(self, other)
    }
}

#[cfg(not(feature = "std"))]
impl FloatExt for f32 {
    #[inline]
    fn mul_add(self, a: Self, b: Self) -> Self {
        libm::fmaf(self, a, b)
    }

    #[inline]
    fn sqrt(self) -> Self {
        libm::sqrtf(self)
    }

    #[inline]
    fn floor(self) -> Self {
        libm::floorf(self)
    }

    #[inline]
    fn atan2(self, other: Self) -> Self {
        libm::atan2f(self, other)
    }
}
