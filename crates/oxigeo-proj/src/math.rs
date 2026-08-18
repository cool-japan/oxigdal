//! Portable floating-point math for `std` and `no_std` targets.
//!
//! `core` only exposes the floating-point operations that map to a hardware
//! instruction on every target (comparisons, `copysign`, …).  The
//! transcendental ones (`sin`, `cos`, `sqrt`, `atan2`, `powf`, …) live in
//! `std`, because on a hosted platform they are backed by the platform libm.
//! On a `no_std` build they simply do not exist as inherent `f64` methods.
//!
//! Until now `--no-default-features` builds of this crate compiled only by
//! accident: `oxiproj` was a *mandatory* dependency, which pulled `std` into
//! the compilation, and `rustc` collects the inherent impls of primitive types
//! from every crate loaded into it — including `std`'s
//! `impl f64 { fn sin(…) … }`.  Once `oxiproj` became optional (activated by
//! the `std` feature), a `no_std` + `alloc` build lost those methods and 73
//! call sites across [`crate::geodesic`], [`crate::datum_transform`],
//! [`crate::ups_projection`], [`crate::geoid`] and
//! [`crate::operation_selection`] stopped resolving.
//!
//! Only the *library* build of `--no-default-features` exercises this module:
//! `--all-targets` (clippy, `cargo test`) puts the dev-dependencies — and with
//! them `std` — back into the compilation, so there the inherent methods win
//! again.  `cargo check -p oxigeo-proj --no-default-features` is therefore the
//! one command that protects the `no_std` build; do not drop it from CI in
//! favour of the `--all-targets` variants.
//!
//! [`FloatExt`] restores them on `no_std` through the pure-Rust [`libm`]
//! crate, mirroring the inherent signatures exactly so that call sites are
//! byte-for-byte identical between the two configurations — the only
//! difference is a `use crate::math::FloatExt;` import that is compiled in
//! exclusively for `no_std`.  Under `std` the trait does not exist at all and
//! the inherent methods are used directly, so hosted builds are unchanged.

/// Floating-point operations that `core` does not provide, backed by the
/// pure-Rust `libm` crate on `no_std` targets.
///
/// Only the operations this crate actually calls are declared; the signatures
/// deliberately match the inherent `f64` methods of the same name so that
/// nothing at the call sites has to change.
///
/// `allow(dead_code)`: rustc collects the inherent impls of primitive types
/// from every crate loaded into the compilation, so as soon as something drags
/// `std` back in — an optional dependency (`proj4rs-compat` → `proj4rs`), or a
/// dev-dependency under `--all-targets` — the inherent `f64` methods reappear
/// and win over this trait, making it unused in those configurations.
#[cfg(not(feature = "std"))]
#[allow(dead_code)]
pub(crate) trait FloatExt {
    /// Sine of `self` (radians).
    fn sin(self) -> Self;
    /// Cosine of `self` (radians).
    fn cos(self) -> Self;
    /// Tangent of `self` (radians).
    fn tan(self) -> Self;
    /// Arcsine of `self`, in `[-π/2, π/2]`.
    fn asin(self) -> Self;
    /// Arctangent of `self`, in `[-π/2, π/2]`.
    fn atan(self) -> Self;
    /// Four-quadrant arctangent of `self` (`y`) and `other` (`x`).
    fn atan2(self, other: Self) -> Self;
    /// Square root.
    fn sqrt(self) -> Self;
    /// `self` raised to the floating-point power `n`.
    fn powf(self, n: Self) -> Self;
    /// `self` raised to the integer power `n`.
    fn powi(self, n: i32) -> Self;
    /// `ln(1 + self)`, accurate for `self` close to zero.
    fn ln_1p(self) -> Self;
    /// Largest integer less than or equal to `self`.
    fn floor(self) -> Self;
    /// Least non-negative remainder of `self (mod rhs)`.
    fn rem_euclid(self, rhs: Self) -> Self;
}

#[cfg(not(feature = "std"))]
impl FloatExt for f64 {
    #[inline]
    fn sin(self) -> Self {
        libm::sin(self)
    }

    #[inline]
    fn cos(self) -> Self {
        libm::cos(self)
    }

    #[inline]
    fn tan(self) -> Self {
        libm::tan(self)
    }

    #[inline]
    fn asin(self) -> Self {
        libm::asin(self)
    }

    #[inline]
    fn atan(self) -> Self {
        libm::atan(self)
    }

    #[inline]
    fn atan2(self, other: Self) -> Self {
        libm::atan2(self, other)
    }

    #[inline]
    fn sqrt(self) -> Self {
        libm::sqrt(self)
    }

    #[inline]
    fn powf(self, n: Self) -> Self {
        libm::pow(self, n)
    }

    #[inline]
    fn powi(self, n: i32) -> Self {
        libm::pow(self, f64::from(n))
    }

    #[inline]
    fn ln_1p(self) -> Self {
        libm::log1p(self)
    }

    #[inline]
    fn floor(self) -> Self {
        libm::floor(self)
    }

    /// Mirrors the semantics of the inherent `f64::rem_euclid`: the result has
    /// the sign of `rhs.abs()` and lies in `[0, rhs.abs())`, with the same
    /// `NaN`/infinity behaviour as `fmod`.
    #[inline]
    fn rem_euclid(self, rhs: Self) -> Self {
        let r = libm::fmod(self, rhs);
        if r < 0.0 { r + libm::fabs(rhs) } else { r }
    }
}

#[cfg(all(test, not(feature = "std")))]
mod tests {
    use super::FloatExt;

    /// `f64::abs` is itself unavailable on `no_std`, so comparisons go through
    /// `libm` directly rather than through the trait under test.
    fn close(a: f64, b: f64, tol: f64) -> bool {
        libm::fabs(a - b) < tol
    }

    /// Relative variant of [`close`], for arguments whose magnitude is not
    /// bounded by 1 (`tan` near a pole, `powf`/`powi` of a large base).
    fn close_rel(a: f64, b: f64, rel: f64) -> bool {
        let scale = if libm::fabs(b) > 1.0 {
            libm::fabs(b)
        } else {
            1.0
        };
        libm::fabs(a - b) <= rel * scale
    }

    /// The `libm` implementations must agree with the textbook values of the
    /// inherent `std` methods they stand in for.
    #[test]
    fn float_ext_matches_std() {
        let angle = core::f64::consts::FRAC_PI_6;
        assert!(close(FloatExt::sin(angle), 0.5, 1e-15));
        assert!(close(FloatExt::cos(angle), 0.866_025_403_784_438_6, 1e-15));
        assert!(close(FloatExt::tan(angle), 0.577_350_269_189_625_8, 1e-15));
        assert!(close(
            FloatExt::asin(1.0),
            core::f64::consts::FRAC_PI_2,
            1e-15
        ));
        assert!(close(
            FloatExt::atan(1.0),
            core::f64::consts::FRAC_PI_4,
            1e-15
        ));
        assert!(close(
            FloatExt::atan2(1.0, 1.0),
            core::f64::consts::FRAC_PI_4,
            1e-15
        ));
        assert!(close(FloatExt::sqrt(2.0), core::f64::consts::SQRT_2, 1e-15));
        assert!(close(FloatExt::powf(2.0, 10.0), 1024.0, 1e-9));
        assert!(close(FloatExt::powi(2.0, 10), 1024.0, 1e-9));
        assert!(close(FloatExt::ln_1p(0.0), 0.0, 1e-15));
        assert!(close(FloatExt::floor(-1.5), -2.0, 1e-15));
    }

    #[test]
    fn rem_euclid_is_non_negative() {
        assert!(close(FloatExt::rem_euclid(-30.0_f64, 360.0), 330.0, 1e-12));
        assert!(close(FloatExt::rem_euclid(370.0_f64, 360.0), 10.0, 1e-12));
        assert!(close(FloatExt::rem_euclid(0.0_f64, 360.0), 0.0, 1e-12));
    }

    /// Cross-check the shim against the inherent `f64` methods over a sweep of
    /// geodetically interesting arguments.
    ///
    /// The unqualified `value.sin()` calls below resolve to the *inherent*
    /// `std` methods whenever `std` is present in the crate graph — which it is
    /// here, because the `test` harness itself links it — so this compares
    /// `libm` against the platform libm the 800 `std` tests exercise. On a host
    /// where `std` really is absent they resolve to [`FloatExt`] instead and the
    /// assertions degenerate to identities rather than failing to build.
    #[test]
    fn float_ext_agrees_with_inherent_methods() {
        for &value in &[
            0.0,
            0.1,
            -0.75,
            core::f64::consts::FRAC_PI_6,
            1.0,
            -1.234_567_89,
            2.5,
            core::f64::consts::PI,
            -core::f64::consts::FRAC_PI_2,
            1.569_050_992_542_303, // 89.9° in radians
        ] {
            assert!(close(FloatExt::sin(value), value.sin(), 1e-15));
            assert!(close(FloatExt::cos(value), value.cos(), 1e-15));
            assert!(close_rel(FloatExt::tan(value), value.tan(), 1e-12));
            assert!(close(FloatExt::atan(value), value.atan(), 1e-15));
            assert!(close(FloatExt::atan2(value, 0.5), value.atan2(0.5), 1e-15));
            assert!(close(FloatExt::floor(value), value.floor(), 1e-15));
            assert!(close(
                FloatExt::rem_euclid(value, 360.0),
                value.rem_euclid(360.0),
                1e-12
            ));

            // `sqrt`, `powf` and `ln_1p` are only real-valued for arguments
            // above 0 / -1 respectively, and that is the whole domain this
            // crate feeds them (`operation_selection` calls `ln_1p` on a
            // non-negative coverage ratio).
            let positive = libm::fabs(value) + 0.25;
            assert!(close(FloatExt::sqrt(positive), positive.sqrt(), 1e-15));
            assert!(close(FloatExt::ln_1p(positive), positive.ln_1p(), 1e-15));
            assert!(close_rel(
                FloatExt::powf(positive, 1.5),
                positive.powf(1.5),
                1e-13
            ));
            assert!(close_rel(
                FloatExt::powi(positive, 3),
                positive.powi(3),
                1e-13
            ));

            let sine = FloatExt::sin(value);
            assert!(close(FloatExt::asin(sine), sine.asin(), 1e-15));
        }
    }
}
