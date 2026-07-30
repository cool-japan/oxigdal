// The same-type fast path reinterprets a caller-owned `&mut [T]` as raw bytes so
// the copy lowers to a single `memcpy`; that needs a small amount of `unsafe`.
// Every `unsafe` block in this file is justified inline.
#![allow(unsafe_code)]

//! Typed raster elements and allocation-free bulk conversions.
//!
//! This module is the answer to "how do I read a band straight into my own
//! `Vec<f64>` / `ndarray::Array2<f64>` without an extra full-size allocation?".
//!
//! * [`RasterElement`] associates a Rust primitive with its [`RasterDataType`]
//!   and defines exact conversion semantics.
//! * [`convert_raw_into`] converts raw driver bytes into a **caller-supplied**
//!   `&mut [T]` in a single pass, with no intermediate buffer.
//! * [`convert_raw_bytes`] is the byte-to-byte variant used by
//!   [`RasterBuffer::convert_to`](crate::buffer::RasterBuffer::convert_to).
//!
//! # Conversion semantics
//!
//! | source → destination | behaviour |
//! |---|---|
//! | same type | `memcpy`, bit-for-bit |
//! | integer → integer | **saturating**, exact (bridged through `i128`, never through `f64`) |
//! | integer → float | `as` cast, round-to-nearest-even (IEEE 754 default) |
//! | float → float | `as` cast, round-to-nearest-even |
//! | float → integer | **saturating**, [`FloatToIntRounding::Nearest`] (half away from zero) by default |
//!
//! Special float inputs converting to an integer destination:
//!
//! | input | result |
//! |---|---|
//! | `NaN` | `0` |
//! | `+∞` / value above `T::MAX` | `T::MAX` |
//! | `-∞` / value below `T::MIN` | `T::MIN` |
//!
//! Nothing ever wraps, traps or produces undefined behaviour.
//!
//! # Precision
//!
//! Integer → integer conversions go through `i128`, so `u64`/`i64` values beyond
//! 2<sup>53</sup> survive **exactly** — unlike the historical
//! `get_pixel`/`set_pixel` path, which bridged everything through `f64`.
//!
//! Conversions whose *destination* is a float are still bounded by the
//! destination's mantissa: a `u64`/`i64` magnitude above 2<sup>53</sup> cannot be
//! represented exactly in `f64` (above 2<sup>24</sup> for `f32`) and is rounded
//! to the nearest representable value. This is inherent to the destination type,
//! not an artefact of the conversion.
//!
//! # Complex data types
//!
//! [`RasterDataType::CFloat32`] and [`RasterDataType::CFloat64`] have no
//! `RasterElement` impl (they are not scalars). They are still accepted by
//! [`convert_raw_into`] and [`convert_raw_bytes`] as *source* types, where the
//! **real component** is used — matching
//! [`RasterBuffer::get_pixel`](crate::buffer::RasterBuffer::get_pixel). As a
//! *destination* (only reachable through [`convert_raw_bytes`]) the real
//! component is written and the imaginary component is zeroed.
//!
//! # Examples
//!
//! ```
//! use oxigeo_core::buffer::convert_raw_into;
//! use oxigeo_core::types::RasterDataType;
//!
//! // Raw bytes exactly as a driver hands them over (native-endian UInt16).
//! let raw: Vec<u8> = [1u16, 2, 3, 60_000]
//!     .iter()
//!     .flat_map(|v| v.to_ne_bytes())
//!     .collect();
//!
//! // Destination owned by the caller — one allocation, and it is *yours*.
//! let mut pixels = vec![0.0f64; 4];
//! convert_raw_into(&raw, RasterDataType::UInt16, &mut pixels)?;
//!
//! assert_eq!(pixels, vec![1.0, 2.0, 3.0, 60_000.0]);
//! # Ok::<(), oxigeo_core::error::OxiGeoError>(())
//! ```

#[cfg(not(feature = "std"))]
use crate::compat::*;
use crate::error::{OxiGeoError, Result};
#[cfg(not(feature = "std"))]
use crate::math::FloatExt;
use crate::types::RasterDataType;

mod private {
    /// Prevents out-of-crate implementations of [`super::RasterElement`].
    pub trait Sealed {}
}

/// Broad category of a [`RasterElement`], used to pick an exact conversion path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RasterElementKind {
    /// A signed or unsigned integer (`u8` … `i64`).
    Integer,
    /// An IEEE 754 binary floating-point number (`f32`, `f64`).
    Float,
}

/// Rounding applied when a floating-point sample is stored into an integer type.
///
/// Saturation at the destination bounds always happens, independent of this
/// setting; only the treatment of the fractional part differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FloatToIntRounding {
    /// Round to the nearest integer, halves away from zero
    /// (`2.5 → 3`, `-2.5 → -3`). This matches GDAL's `RasterIO` behaviour and is
    /// the default for every public entry point that does not take an explicit
    /// rounding mode.
    #[default]
    Nearest,
    /// Truncate towards zero (`2.9 → 2`, `-2.9 → -2`), i.e. plain C-style
    /// `(int)x` semantics.
    ///
    /// Used by [`RasterBuffer::convert_to`](crate::buffer::RasterBuffer::convert_to)
    /// so that its results stay bit-for-bit identical to the historical
    /// per-pixel implementation.
    Truncate,
}

/// A Rust primitive that can appear as a raster sample.
///
/// Implemented for exactly the ten scalar raster types — `u8`, `i8`, `u16`,
/// `i16`, `u32`, `i32`, `u64`, `i64`, `f32` and `f64` — and sealed, so the set
/// cannot grow outside this crate.
///
/// # Thread safety
///
/// Every implementor is a primitive scalar, so `Send + Sync` is part of the
/// contract. Generic code bounded on `RasterElement` may therefore split a
/// `&mut [T]` across worker threads without restating the bound — which is what
/// lets the GeoTIFF driver run its *typed* band reads on rayon workers, not just
/// the raw-byte ones. Because the trait is sealed the guarantee costs nothing:
/// no out-of-crate type can ever weaken it.
///
/// Use it as a bound to write code that is generic over the pixel type:
///
/// ```
/// use oxigeo_core::buffer::{RasterElement, convert_raw_into};
/// use oxigeo_core::error::Result;
/// use oxigeo_core::types::RasterDataType;
///
/// fn decode<T: RasterElement>(raw: &[u8], src: RasterDataType, dst: &mut [T]) -> Result<()> {
///     convert_raw_into(raw, src, dst)
/// }
///
/// let mut out = [0i32; 2];
/// decode(&[1u8, 2], RasterDataType::UInt8, &mut out)?;
/// assert_eq!(out, [1, 2]);
/// assert_eq!(i32::DATA_TYPE, RasterDataType::Int32);
/// # Ok::<(), oxigeo_core::error::OxiGeoError>(())
/// ```
pub trait RasterElement: Copy + Default + Send + Sync + 'static + private::Sealed {
    /// The [`RasterDataType`] this Rust type stores.
    const DATA_TYPE: RasterDataType;

    /// Whether this type is an integer or a float.
    const KIND: RasterElementKind;

    /// Size of one sample in bytes (always `size_of::<Self>()`).
    const SIZE: usize = core::mem::size_of::<Self>();

    /// The fixed-size native-endian byte array for this type
    /// (`[u8; size_of::<Self>()]`).
    type Bytes: AsRef<[u8]> + AsMut<[u8]> + Copy + Default + for<'a> TryFrom<&'a [u8]>;

    /// Decodes a sample from its native-endian byte array.
    fn from_ne_bytes(bytes: Self::Bytes) -> Self;

    /// Encodes a sample into its native-endian byte array.
    fn to_ne_bytes(self) -> Self::Bytes;

    /// Widens the sample to `f64`.
    ///
    /// Exact for every type except `u64`/`i64` magnitudes beyond
    /// 2<sup>53</sup>, which are rounded to the nearest representable `f64`.
    fn to_raster_f64(self) -> f64;

    /// Widens the sample to `i128`.
    ///
    /// Exact for every integer type. For floats the value is rounded half away
    /// from zero and saturated at the `i128` bounds; `NaN` becomes `0`.
    fn to_raster_i128(self) -> i128;

    /// Narrows an `f64` to this type, saturating at the type bounds and
    /// rounding halves away from zero.
    ///
    /// `NaN` becomes `0` for integer destinations; float destinations keep
    /// `NaN`/`±∞` as-is.
    fn from_raster_f64(value: f64) -> Self;

    /// Narrows an `f64` to this type, saturating at the type bounds and
    /// truncating towards zero (C-style `(int)x`).
    ///
    /// Identical to [`RasterElement::from_raster_f64`] for float destinations.
    fn from_raster_f64_truncating(value: f64) -> Self;

    /// Narrows an `i128` to this type, saturating at the type bounds.
    ///
    /// Exact for integer destinations. Float destinations round to nearest.
    fn from_raster_i128(value: i128) -> Self;

    /// Decodes a sample from the first [`SIZE`](RasterElement::SIZE) bytes of
    /// `bytes` (native endian).
    ///
    /// Longer slices are truncated — this is what makes complex sources work,
    /// where each stride holds `[real, imaginary]` and only the real component
    /// is read. Shorter slices yield [`Default::default`] instead of panicking.
    #[inline]
    fn from_ne_slice(bytes: &[u8]) -> Self {
        match bytes
            .get(..Self::SIZE)
            .and_then(|head| <Self::Bytes as TryFrom<&[u8]>>::try_from(head).ok())
        {
            Some(raw) => Self::from_ne_bytes(raw),
            None => Self::default(),
        }
    }
}

/// Rounds half away from zero without relying on `f64::round`, which is
/// unavailable in `no_std` builds.
///
/// Unlike the `(x + 0.5).floor()` shortcut this is exact for every input:
/// values at or above 2<sup>52</sup> are already integral and pass through
/// unchanged, and `NaN`/`±∞` fall through to `floor`'s own result.
#[inline]
fn round_half_away_from_zero(value: f64) -> f64 {
    let floor = value.floor();
    // `value - floor(value)` is exact (both live in the same binade), so no
    // rounding error can flip the comparison below.
    let fract = value - floor;
    if value.is_sign_negative() {
        // Halves must move away from zero, i.e. stay on `floor`: -2.5 -> -3.
        if fract > 0.5 { floor + 1.0 } else { floor }
    } else if fract >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

macro_rules! impl_raster_element_int {
    ($($ty:ty => $variant:ident, $len:literal;)*) => {
        $(
            impl private::Sealed for $ty {}

            impl RasterElement for $ty {
                const DATA_TYPE: RasterDataType = RasterDataType::$variant;
                const KIND: RasterElementKind = RasterElementKind::Integer;
                type Bytes = [u8; $len];

                #[inline]
                fn from_ne_bytes(bytes: Self::Bytes) -> Self {
                    <$ty>::from_ne_bytes(bytes)
                }

                #[inline]
                fn to_ne_bytes(self) -> Self::Bytes {
                    <$ty>::to_ne_bytes(self)
                }

                #[inline]
                fn to_raster_f64(self) -> f64 {
                    self as f64
                }

                #[inline]
                fn to_raster_i128(self) -> i128 {
                    self as i128
                }

                #[inline]
                fn from_raster_f64(value: f64) -> Self {
                    // `as` from float to int already saturates and maps NaN to 0.
                    round_half_away_from_zero(value) as $ty
                }

                #[inline]
                fn from_raster_f64_truncating(value: f64) -> Self {
                    value as $ty
                }

                #[inline]
                fn from_raster_i128(value: i128) -> Self {
                    // Both bounds are exactly representable in i128 for every
                    // integer type, so the clamp is lossless.
                    value.clamp(<$ty>::MIN as i128, <$ty>::MAX as i128) as $ty
                }
            }
        )*
    };
}

macro_rules! impl_raster_element_float {
    ($($ty:ty => $variant:ident, $len:literal;)*) => {
        $(
            impl private::Sealed for $ty {}

            impl RasterElement for $ty {
                const DATA_TYPE: RasterDataType = RasterDataType::$variant;
                const KIND: RasterElementKind = RasterElementKind::Float;
                type Bytes = [u8; $len];

                #[inline]
                fn from_ne_bytes(bytes: Self::Bytes) -> Self {
                    <$ty>::from_ne_bytes(bytes)
                }

                #[inline]
                fn to_ne_bytes(self) -> Self::Bytes {
                    <$ty>::to_ne_bytes(self)
                }

                #[inline]
                fn to_raster_f64(self) -> f64 {
                    self as f64
                }

                #[inline]
                fn to_raster_i128(self) -> i128 {
                    round_half_away_from_zero(self as f64) as i128
                }

                #[inline]
                fn from_raster_f64(value: f64) -> Self {
                    value as $ty
                }

                #[inline]
                fn from_raster_f64_truncating(value: f64) -> Self {
                    value as $ty
                }

                #[inline]
                fn from_raster_i128(value: i128) -> Self {
                    value as $ty
                }
            }
        )*
    };
}

impl_raster_element_int! {
    u8 => UInt8, 1;
    i8 => Int8, 1;
    u16 => UInt16, 2;
    i16 => Int16, 2;
    u32 => UInt32, 4;
    i32 => Int32, 4;
    u64 => UInt64, 8;
    i64 => Int64, 8;
}

impl_raster_element_float! {
    f32 => Float32, 4;
    f64 => Float64, 8;
}

/// Expands `$body!(rust_type, stride)` once per [`RasterDataType`].
///
/// The stride is the *pixel* size, which differs from the Rust type size for the
/// complex types: their real component is the leading `f32`/`f64`.
macro_rules! dispatch_data_type {
    ($data_type:expr, $body:ident) => {
        match $data_type {
            RasterDataType::UInt8 => $body!(u8, 1),
            RasterDataType::Int8 => $body!(i8, 1),
            RasterDataType::UInt16 => $body!(u16, 2),
            RasterDataType::Int16 => $body!(i16, 2),
            RasterDataType::UInt32 => $body!(u32, 4),
            RasterDataType::Int32 => $body!(i32, 4),
            RasterDataType::UInt64 => $body!(u64, 8),
            RasterDataType::Int64 => $body!(i64, 8),
            RasterDataType::Float32 => $body!(f32, 4),
            RasterDataType::Float64 => $body!(f64, 8),
            RasterDataType::CFloat32 => $body!(f32, 8),
            RasterDataType::CFloat64 => $body!(f64, 16),
        }
    };
}

/// Reinterprets raw driver bytes as a real `&[S]`, or `None` when that would be
/// misaligned.
///
/// With a genuine `&[S]` in hand the conversion loop is `dst[i] = f(src[i])`
/// over two primitive slices — the shape every autovectoriser recognises. An
/// `f32 → f64` band then lowers to `fcvtl` on aarch64 and `cvtps2pd` on x86-64.
///
/// Assembling each sample from a byte chunk instead *can* vectorise just as well
/// — with rustc 1.95 / LLVM 22 it demonstrably does, on both of those targets —
/// but that depends on the optimiser seeing through `chunks_exact` plus a
/// `TryFrom<&[u8]>` plus an `Option`. Going through the typed slice does not
/// depend on anything: it is the same loop the "decode, then run a separate
/// vectorised map" workaround runs, so no backend can make it slower than that.
///
/// Allocator-backed buffers are aligned in practice, but nothing *guarantees*
/// it — a `Vec<u8>` only promises alignment 1, and sub-slices such as
/// `&bytes[1..]` are routinely misaligned — so the caller must handle `None`.
#[inline]
fn samples_from_bytes<S: RasterElement>(src: &[u8]) -> Option<&[S]> {
    if S::SIZE == 0 || !src.len().is_multiple_of(S::SIZE) {
        return None;
    }
    // `align_offset` is permitted to answer `usize::MAX` ("cannot tell"), which
    // this comparison reads as "not aligned". A conservative answer only costs
    // the (equally fast) unaligned path, which is always correct.
    if src.as_ptr().align_offset(core::mem::align_of::<S>()) != 0 {
        return None;
    }
    // SAFETY: `S` is one of the ten primitive numeric types (the trait is
    // sealed), so it has no padding and every bit pattern is a valid value —
    // any run of `S::SIZE` initialised bytes is a valid `S`. The pointer is
    // checked to be aligned for `S` just above, the length is an exact multiple
    // of `S::SIZE`, so `src.len() / S::SIZE` elements cover exactly the same
    // bytes as `src` (no overflow: the byte range already exists). The returned
    // slice borrows `src` for the same lifetime and is shared, like `src`.
    Some(unsafe { core::slice::from_raw_parts(src.as_ptr().cast::<S>(), src.len() / S::SIZE) })
}

/// [`samples_from_bytes`] for a destination buffer.
#[inline]
fn samples_from_bytes_mut<D: RasterElement>(dst: &mut [u8]) -> Option<&mut [D]> {
    if D::SIZE == 0 || !dst.len().is_multiple_of(D::SIZE) {
        return None;
    }
    if dst.as_ptr().align_offset(core::mem::align_of::<D>()) != 0 {
        return None;
    }
    let count = dst.len() / D::SIZE;
    // SAFETY: as `samples_from_bytes`, plus: the borrow is unique (`&mut [u8]`)
    // and is consumed by this call, so the returned `&mut [D]` is the only way
    // to reach those bytes for its lifetime. Every bit pattern of `D` is valid,
    // so writes through it leave the buffer in a state that is still a valid
    // `[u8]` afterwards.
    Some(unsafe { core::slice::from_raw_parts_mut(dst.as_mut_ptr().cast::<D>(), count) })
}

/// How many whole samples of `SIZE` bytes a byte slice holds.
#[inline]
const fn sample_capacity(bytes: usize, size: usize) -> usize {
    match bytes.checked_div(size) {
        Some(count) => count,
        None => 0,
    }
}

/// The vectorisable kernel: convert `src[i]` into `dst[i]` for as many elements
/// as both slices hold.
///
/// `S::KIND`/`D::KIND` are associated consts, so the dispatch below collapses at
/// monomorphisation and the loop body is a single straight-line expression over
/// two primitive slices.
#[inline]
fn convert_samples<S: RasterElement, D: RasterElement>(
    src: &[S],
    dst: &mut [D],
    rounding: FloatToIntRounding,
) {
    macro_rules! run {
        ($convert:expr) => {{
            for (value, out) in src.iter().zip(dst.iter_mut()) {
                *out = $convert(*value);
            }
        }};
    }

    match (S::KIND, D::KIND, rounding) {
        (RasterElementKind::Integer, RasterElementKind::Integer, _) => {
            run!(|value: S| D::from_raster_i128(value.to_raster_i128()));
        }
        (_, _, FloatToIntRounding::Nearest) => {
            run!(|value: S| D::from_raster_f64(value.to_raster_f64()));
        }
        (_, _, FloatToIntRounding::Truncate) => {
            run!(|value: S| D::from_raster_f64_truncating(value.to_raster_f64()));
        }
    }
}

/// [`convert_samples`] for a source that could not be reinterpreted in place.
///
/// Each sample is loaded with an *unaligned typed* read rather than assembled
/// from a byte chunk. That keeps the loop a plain strided load + convert +
/// store, which vectorises exactly like the aligned path (unaligned vector loads
/// are full speed on every target this crate builds for), while staging through
/// an intermediate buffer would add a whole extra pass over the data — measured
/// at 1.7× slower on a 16 Mpx `f32 → f64` band.
#[inline]
fn convert_unaligned_samples<S: RasterElement, D: RasterElement>(
    src: &[u8],
    dst: &mut [D],
    rounding: FloatToIntRounding,
) {
    let count = dst.len().min(sample_capacity(src.len(), S::SIZE));
    let base = src.as_ptr();
    let Some(dst) = dst.get_mut(..count) else {
        return;
    };

    macro_rules! run {
        ($convert:expr) => {{
            for (index, out) in dst.iter_mut().enumerate() {
                // SAFETY: `index < count` and `count * S::SIZE <= src.len()`, so
                // the read stays inside `src`'s allocation. `read_unaligned`
                // imposes no alignment requirement, and every bit pattern of `S`
                // is a valid value (sealed trait: ten primitive numerics), so any
                // `S::SIZE` initialised bytes decode to a valid `S` — the same
                // value `S::from_ne_bytes` would produce, by the definition of
                // the native-endian representation.
                let value: S = unsafe { base.add(index * S::SIZE).cast::<S>().read_unaligned() };
                *out = $convert(value);
            }
        }};
    }

    match (S::KIND, D::KIND, rounding) {
        (RasterElementKind::Integer, RasterElementKind::Integer, _) => {
            run!(|value: S| D::from_raster_i128(value.to_raster_i128()));
        }
        (_, _, FloatToIntRounding::Nearest) => {
            run!(|value: S| D::from_raster_f64(value.to_raster_f64()));
        }
        (_, _, FloatToIntRounding::Truncate) => {
            run!(|value: S| D::from_raster_f64_truncating(value.to_raster_f64()));
        }
    }
}

/// Converts a *contiguous* run of `S` samples (one sample every `S::SIZE` bytes)
/// into `dst`.
///
/// Reinterprets the source in place when it is aligned and falls back to
/// unaligned typed loads when it is not. Both paths run the same conversion
/// expression on the same decoded value, so they are bit-identical by
/// construction — and the test suite proves it on NaN payloads, signed zeroes,
/// subnormals and every integer bound.
#[inline]
fn convert_contiguous<S: RasterElement, D: RasterElement>(
    src: &[u8],
    dst: &mut [D],
    rounding: FloatToIntRounding,
) {
    match samples_from_bytes::<S>(src) {
        Some(values) => convert_samples(values, dst, rounding),
        None => convert_unaligned_samples::<S, D>(src, dst, rounding),
    }
}

/// Byte-to-byte counterpart of [`convert_contiguous`] for a destination that
/// could not be reinterpreted as `&mut [D]`.
#[inline]
fn convert_unaligned_bytes<S: RasterElement, D: RasterElement>(
    src: &[u8],
    dst: &mut [u8],
    rounding: FloatToIntRounding,
) {
    let count = sample_capacity(src.len(), S::SIZE).min(sample_capacity(dst.len(), D::SIZE));
    let base = src.as_ptr();
    let out = dst.as_mut_ptr();

    macro_rules! run {
        ($convert:expr) => {{
            for index in 0..count {
                // SAFETY: `count` is clamped so that both `index * S::SIZE +
                // S::SIZE <= src.len()` and `index * D::SIZE + D::SIZE <=
                // dst.len()`, so both accesses stay inside their allocation.
                // Neither read nor write requires alignment. Every bit pattern of
                // `S` is a valid `S` and every bit pattern of `D` is a valid `u8`
                // sequence, so both directions are well defined. `src` and `dst`
                // are a shared and a unique borrow of distinct objects, so the
                // write cannot invalidate the read.
                let value: S = unsafe { base.add(index * S::SIZE).cast::<S>().read_unaligned() };
                let converted: D = $convert(value);
                unsafe {
                    out.add(index * D::SIZE)
                        .cast::<D>()
                        .write_unaligned(converted);
                }
            }
        }};
    }

    match (S::KIND, D::KIND, rounding) {
        (RasterElementKind::Integer, RasterElementKind::Integer, _) => {
            run!(|value: S| D::from_raster_i128(value.to_raster_i128()));
        }
        (_, _, FloatToIntRounding::Nearest) => {
            run!(|value: S| D::from_raster_f64(value.to_raster_f64()));
        }
        (_, _, FloatToIntRounding::Truncate) => {
            run!(|value: S| D::from_raster_f64_truncating(value.to_raster_f64()));
        }
    }
}

/// Core loop: decode `S` samples out of `src` (one every `src_stride` bytes) and
/// write converted `D` values into `dst`.
///
/// `src` must hold at least `dst.len()` strides; callers validate that.
#[inline]
fn convert_into_typed<S: RasterElement, D: RasterElement, const SRC_STRIDE: usize>(
    src: &[u8],
    dst: &mut [D],
    rounding: FloatToIntRounding,
) {
    // Scalar source: samples are back to back, so the whole run can go through
    // the bulk path. `SRC_STRIDE` is a const generic, so this is resolved at
    // monomorphisation and the other branch is not even compiled in.
    if SRC_STRIDE == S::SIZE {
        convert_contiguous::<S, D>(src, dst, rounding);
        return;
    }

    // Strided source — only the complex data types reach here, where each stride
    // holds `[real, imaginary]` and just the real component is read.
    macro_rules! run {
        ($convert:expr) => {{
            for (chunk, out) in src.chunks_exact(SRC_STRIDE).zip(dst.iter_mut()) {
                *out = $convert(S::from_ne_slice(chunk));
            }
        }};
    }

    match (S::KIND, D::KIND, rounding) {
        (RasterElementKind::Integer, RasterElementKind::Integer, _) => {
            run!(|value: S| D::from_raster_i128(value.to_raster_i128()));
        }
        (_, _, FloatToIntRounding::Nearest) => {
            run!(|value: S| D::from_raster_f64(value.to_raster_f64()));
        }
        (_, _, FloatToIntRounding::Truncate) => {
            run!(|value: S| D::from_raster_f64_truncating(value.to_raster_f64()));
        }
    }
}

/// Same as [`convert_into_typed`] but writing native-endian bytes, so that
/// complex destination types (real component + zeroed imaginary component) are
/// expressible.
///
/// Both strides are const generics: with `DST_STRIDE` and `D::SIZE` known at
/// compile time the store becomes a fixed-size write and the imaginary-component
/// fill disappears entirely for scalar destinations. Passing them as ordinary
/// arguments made every element pay for an out-of-line `memcpy`/`memset` call
/// (measured on 4.2M UInt16 → Float32 samples: 8.5 ms vs 0.59 ms).
#[inline]
fn convert_into_bytes<
    S: RasterElement,
    D: RasterElement,
    const SRC_STRIDE: usize,
    const DST_STRIDE: usize,
>(
    src: &[u8],
    dst: &mut [u8],
    rounding: FloatToIntRounding,
) {
    // Both sides contiguous scalars (i.e. neither is complex): route through the
    // same bulk kernel the typed entry point uses. Both strides are const
    // generics, so this test disappears at monomorphisation.
    if SRC_STRIDE == S::SIZE && DST_STRIDE == D::SIZE {
        match samples_from_bytes_mut::<D>(dst) {
            Some(out) => convert_contiguous::<S, D>(src, out, rounding),
            None => convert_unaligned_bytes::<S, D>(src, dst, rounding),
        }
        return;
    }

    macro_rules! run {
        ($convert:expr) => {{
            for (chunk, out) in src
                .chunks_exact(SRC_STRIDE)
                .zip(dst.chunks_exact_mut(DST_STRIDE))
            {
                let value: D = $convert(S::from_ne_slice(chunk));
                let encoded = value.to_ne_bytes();
                if let Some(slot) = out.get_mut(..D::SIZE) {
                    if let Some(bytes) = encoded.as_ref().get(..D::SIZE) {
                        slot.copy_from_slice(bytes);
                    }
                }
                // Zero the imaginary component of complex destinations. Both
                // sides are compile-time constants, so this vanishes for every
                // scalar destination.
                if DST_STRIDE > D::SIZE {
                    if let Some(padding) = out.get_mut(D::SIZE..) {
                        padding.fill(0);
                    }
                }
            }
        }};
    }

    match (S::KIND, D::KIND, rounding) {
        (RasterElementKind::Integer, RasterElementKind::Integer, _) => {
            run!(|value: S| D::from_raster_i128(value.to_raster_i128()));
        }
        (_, _, FloatToIntRounding::Nearest) => {
            run!(|value: S| D::from_raster_f64(value.to_raster_f64()));
        }
        (_, _, FloatToIntRounding::Truncate) => {
            run!(|value: S| D::from_raster_f64_truncating(value.to_raster_f64()));
        }
    }
}

/// "A raster data type claims to occupy zero bytes", built out of line.
#[cold]
#[inline(never)]
fn zero_stride_error() -> OxiGeoError {
    OxiGeoError::Internal {
        message: "Raster data type reported a zero-byte sample size".to_string(),
    }
}

/// "`src` is not a whole number of samples", built out of line.
///
/// `#[cold]`/`#[inline(never)]`: the `format!` machinery would otherwise be
/// inlined into the validation, which is on the hottest path in the crate — the
/// GeoTIFF driver runs it once per tile row.
#[cold]
#[inline(never)]
fn partial_sample_error(src_len: usize, stride: usize) -> OxiGeoError {
    OxiGeoError::InvalidParameter {
        parameter: "src",
        message: format!(
            "Source length {} is not a whole number of {}-byte samples",
            src_len, stride
        ),
    }
}

/// "`dst` has the wrong length", built out of line.
#[cold]
#[inline(never)]
fn length_mismatch_error(
    count: usize,
    src_len: usize,
    stride: usize,
    dst_len: usize,
) -> OxiGeoError {
    OxiGeoError::InvalidParameter {
        parameter: "dst",
        message: format!(
            "Destination length mismatch: source holds {} samples ({} bytes / {} bytes each), destination has {}",
            count, src_len, stride, dst_len
        ),
    }
}

/// Validates that `src` holds exactly `dst_len` whole samples of `stride` bytes.
fn sample_count(src_len: usize, stride: usize, dst_len: usize) -> Result<usize> {
    if stride == 0 {
        return Err(zero_stride_error());
    }
    if !src_len.is_multiple_of(stride) {
        return Err(partial_sample_error(src_len, stride));
    }
    let count = src_len / stride;
    if count != dst_len {
        return Err(length_mismatch_error(count, src_len, stride, dst_len));
    }
    Ok(count)
}

/// [`sample_count`] with the stride known at compile time.
///
/// Same checks, same errors — but both divisions collapse to shifts. With a
/// runtime stride they are two `udiv`s, and that is not free where it matters:
/// the GeoTIFF driver converts one tile row per call, i.e. 62 500 calls for a
/// 4000 × 4000 raster of 256 × 256 tiles. Measured on that call pattern, a
/// runtime stride costs **25 ns per call — 1.6 ms per band read** — while a
/// constant one costs nothing at all.
#[inline]
fn check_sample_count<const STRIDE: usize>(src_len: usize, dst_len: usize) -> Result<()> {
    // Every instantiation passes a non-zero literal, so this folds away; it is
    // here so that a zero can never reach the `%` below and panic.
    if STRIDE == 0 {
        return Err(zero_stride_error());
    }
    if !src_len.is_multiple_of(STRIDE) {
        return Err(partial_sample_error(src_len, STRIDE));
    }
    let count = src_len / STRIDE;
    if count != dst_len {
        return Err(length_mismatch_error(count, src_len, STRIDE, dst_len));
    }
    Ok(())
}

/// The `src_type == T::DATA_TYPE` fast path: identical layout, so the whole
/// conversion is one `memcpy`, bit for bit.
#[inline]
fn copy_same_type<T: RasterElement>(src: &[u8], dst: &mut [T]) {
    // SAFETY: `T` is one of the ten primitive numeric types (sealed trait): it
    // has no padding and every bit pattern is a valid value, so raw bytes may be
    // written into it. The pointer comes from a live `&mut [T]`, and `u8` needs
    // no alignment. The slice covers exactly `size_of_val(dst)` bytes of that
    // same allocation.
    let dst_bytes = unsafe {
        core::slice::from_raw_parts_mut(dst.as_mut_ptr().cast::<u8>(), core::mem::size_of_val(dst))
    };
    // Callers validate the lengths; the guard makes the copy unable to panic
    // even if that ever stopped being true.
    if dst_bytes.len() == src.len() {
        dst_bytes.copy_from_slice(src);
    }
}

/// Reinterprets typed samples as their native-endian bytes.
///
/// Zero-copy: the returned slice borrows `values`. Useful when handing a typed
/// array to a writer that speaks raw bytes.
///
/// # Examples
///
/// ```
/// use oxigeo_core::buffer::elements_as_bytes;
///
/// let values = [1u16, 2u16];
/// assert_eq!(elements_as_bytes(&values).len(), 4);
/// ```
#[must_use]
pub fn elements_as_bytes<T: RasterElement>(values: &[T]) -> &[u8] {
    // SAFETY: `T` is one of the ten primitive numeric types (the trait is
    // sealed). Those have no padding bytes and no niches, so every byte of the
    // slice is initialised and readable as `u8`. The resulting slice borrows
    // `values` for the same lifetime and covers exactly the same bytes, and `u8`
    // has an alignment of 1 so no alignment requirement can be violated.
    unsafe {
        core::slice::from_raw_parts(values.as_ptr().cast::<u8>(), core::mem::size_of_val(values))
    }
}

/// Converts raw raster bytes of `src_type` into `dst` in a single pass, without
/// allocating.
///
/// This is the primitive behind
/// [`RasterBuffer::copy_to_slice`](crate::buffer::RasterBuffer::copy_to_slice):
/// feed it the bytes a driver decoded and a destination the caller already owns
/// (a `Vec<f64>`, an `ndarray::Array2<f64>`'s backing slice, a memory-mapped
/// region…) and no intermediate buffer is ever materialised.
///
/// `src` does **not** need to be aligned for `T`: samples are decoded with
/// native-endian byte reads.
///
/// Float samples are rounded with [`FloatToIntRounding::Nearest`]; use
/// [`convert_raw_into_with`] to choose. See the [module documentation](self) for
/// the full semantics.
///
/// # Errors
///
/// Returns [`OxiGeoError::InvalidParameter`] if `src` is not a whole number of
/// `src_type` samples, or if `dst.len()` differs from that sample count.
///
/// # Examples
///
/// ```
/// use oxigeo_core::buffer::convert_raw_into;
/// use oxigeo_core::types::RasterDataType;
///
/// let raw: Vec<u8> = [-1i16, 0, 1].iter().flat_map(|v| v.to_ne_bytes()).collect();
///
/// let mut pixels = vec![0.0f64; 3];
/// convert_raw_into(&raw, RasterDataType::Int16, &mut pixels)?;
/// assert_eq!(pixels, vec![-1.0, 0.0, 1.0]);
///
/// // Wrong-sized destinations are rejected instead of truncating silently.
/// let mut too_small = vec![0.0f64; 2];
/// assert!(convert_raw_into(&raw, RasterDataType::Int16, &mut too_small).is_err());
/// # Ok::<(), oxigeo_core::error::OxiGeoError>(())
/// ```
pub fn convert_raw_into<T: RasterElement>(
    src: &[u8],
    src_type: RasterDataType,
    dst: &mut [T],
) -> Result<()> {
    convert_raw_into_with(src, src_type, dst, FloatToIntRounding::Nearest)
}

/// [`convert_raw_into`] with an explicit float→integer rounding mode.
///
/// # Errors
///
/// Same as [`convert_raw_into`].
///
/// # Examples
///
/// ```
/// use oxigeo_core::buffer::{FloatToIntRounding, convert_raw_into_with};
/// use oxigeo_core::types::RasterDataType;
///
/// let raw: Vec<u8> = [2.5f32, -2.5].iter().flat_map(|v| v.to_ne_bytes()).collect();
///
/// let mut nearest = [0i16; 2];
/// convert_raw_into_with(&raw, RasterDataType::Float32, &mut nearest, FloatToIntRounding::Nearest)?;
/// assert_eq!(nearest, [3, -3]);
///
/// let mut truncated = [0i16; 2];
/// convert_raw_into_with(&raw, RasterDataType::Float32, &mut truncated, FloatToIntRounding::Truncate)?;
/// assert_eq!(truncated, [2, -2]);
/// # Ok::<(), oxigeo_core::error::OxiGeoError>(())
/// ```
pub fn convert_raw_into_with<T: RasterElement>(
    src: &[u8],
    src_type: RasterDataType,
    dst: &mut [T],
    rounding: FloatToIntRounding,
) -> Result<()> {
    // Dispatch *first*, then validate with the stride of the arm we landed in:
    // `$stride` is a literal, so the length checks cost two shifts instead of
    // two integer divisions. See [`check_sample_count`] for why that is worth
    // doing.
    macro_rules! body {
        ($source:ty, $stride:literal) => {{
            check_sample_count::<$stride>(src.len(), dst.len())?;
            if src_type == T::DATA_TYPE {
                // Same on-disk and in-memory layout: one memcpy. The comparison
                // is between two constants inside each arm, so only the arm that
                // can match keeps this branch at all.
                copy_same_type(src, dst);
            } else {
                convert_into_typed::<$source, T, $stride>(src, dst, rounding);
            }
        }};
    }
    dispatch_data_type!(src_type, body);

    Ok(())
}

/// Converts raw raster bytes of `src_type` into raw bytes of `dst_type`, in a
/// single pass and without allocating.
///
/// The byte-level counterpart of [`convert_raw_into`]. Prefer the typed version
/// whenever the destination type is known at compile time; this one exists for
/// dynamically typed destinations and for the complex data types, which have no
/// [`RasterElement`] impl.
///
/// Neither slice needs any particular alignment.
///
/// # Errors
///
/// Returns [`OxiGeoError::InvalidParameter`] if either slice is not a whole
/// number of samples, or if the two sample counts differ.
///
/// # Examples
///
/// ```
/// use oxigeo_core::buffer::{FloatToIntRounding, convert_raw_bytes};
/// use oxigeo_core::types::RasterDataType;
///
/// let src: Vec<u8> = [200u16, 1000, 65_535]
///     .iter()
///     .flat_map(|v| v.to_ne_bytes())
///     .collect();
/// let mut dst = vec![0u8; 3]; // three UInt8 samples
///
/// convert_raw_bytes(
///     &src,
///     RasterDataType::UInt16,
///     &mut dst,
///     RasterDataType::UInt8,
///     FloatToIntRounding::Nearest,
/// )?;
///
/// // 1000 saturates to 255 instead of wrapping to 232.
/// assert_eq!(dst, vec![200, 255, 255]);
/// # Ok::<(), oxigeo_core::error::OxiGeoError>(())
/// ```
pub fn convert_raw_bytes(
    src: &[u8],
    src_type: RasterDataType,
    dst: &mut [u8],
    dst_type: RasterDataType,
    rounding: FloatToIntRounding,
) -> Result<()> {
    let src_stride = src_type.size_bytes();
    let dst_stride = dst_type.size_bytes();

    if dst_stride == 0 {
        return Err(OxiGeoError::Internal {
            message: "Raster data type reported a zero-byte sample size".to_string(),
        });
    }
    if !dst.len().is_multiple_of(dst_stride) {
        return Err(OxiGeoError::InvalidParameter {
            parameter: "dst",
            message: format!(
                "Destination length {} is not a whole number of {}-byte samples",
                dst.len(),
                dst_stride
            ),
        });
    }
    sample_count(src.len(), src_stride, dst.len() / dst_stride)?;

    if src_type == dst_type {
        // Identical layout: straight memcpy, bit-for-bit (including the
        // imaginary components of complex types).
        dst.copy_from_slice(src);
        return Ok(());
    }

    macro_rules! destination {
        ($dest:ty, $dst_stride:literal) => {{
            macro_rules! source {
                ($source:ty, $src_stride:literal) => {
                    convert_into_bytes::<$source, $dest, $src_stride, $dst_stride>(
                        src, dst, rounding,
                    )
                };
            }
            dispatch_data_type!(src_type, source)
        }};
    }
    dispatch_data_type!(dst_type, destination);

    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn test_data_type_mapping() {
        assert_eq!(u8::DATA_TYPE, RasterDataType::UInt8);
        assert_eq!(i8::DATA_TYPE, RasterDataType::Int8);
        assert_eq!(u16::DATA_TYPE, RasterDataType::UInt16);
        assert_eq!(i16::DATA_TYPE, RasterDataType::Int16);
        assert_eq!(u32::DATA_TYPE, RasterDataType::UInt32);
        assert_eq!(i32::DATA_TYPE, RasterDataType::Int32);
        assert_eq!(u64::DATA_TYPE, RasterDataType::UInt64);
        assert_eq!(i64::DATA_TYPE, RasterDataType::Int64);
        assert_eq!(f32::DATA_TYPE, RasterDataType::Float32);
        assert_eq!(f64::DATA_TYPE, RasterDataType::Float64);
    }

    #[test]
    fn test_elements_are_send_and_sync() {
        // Proves the bound is reachable *through* the trait: generic code that
        // only knows `T: RasterElement` may hand `T` to another thread. This is
        // what allows the GeoTIFF driver's typed band reads to split a
        // `&mut [T]` across rayon workers.
        const fn assert_send_sync<T: Send + Sync>() {}
        const fn assert_element<T: RasterElement>() {
            assert_send_sync::<T>();
        }
        assert_element::<u8>();
        assert_element::<i8>();
        assert_element::<u16>();
        assert_element::<i16>();
        assert_element::<u32>();
        assert_element::<i32>();
        assert_element::<u64>();
        assert_element::<i64>();
        assert_element::<f32>();
        assert_element::<f64>();
    }

    #[test]
    fn test_size_matches_data_type() {
        assert_eq!(u8::SIZE, RasterDataType::UInt8.size_bytes());
        assert_eq!(i16::SIZE, RasterDataType::Int16.size_bytes());
        assert_eq!(f32::SIZE, RasterDataType::Float32.size_bytes());
        assert_eq!(f64::SIZE, RasterDataType::Float64.size_bytes());
        assert_eq!(u64::SIZE, RasterDataType::UInt64.size_bytes());
    }

    #[test]
    fn test_round_half_away_from_zero() {
        assert_eq!(round_half_away_from_zero(2.5), 3.0);
        assert_eq!(round_half_away_from_zero(-2.5), -3.0);
        assert_eq!(round_half_away_from_zero(2.4999), 2.0);
        assert_eq!(round_half_away_from_zero(-2.4999), -2.0);
        assert_eq!(round_half_away_from_zero(0.0), 0.0);
        assert_eq!(round_half_away_from_zero(-0.4), -0.0);
        // Largest f64 below 0.5 must not round up.
        assert_eq!(round_half_away_from_zero(0.499_999_999_999_999_94), 0.0);
        // Values at or above 2^52 are already integral and pass through.
        let big = 9_007_199_254_740_993f64; // 2^53 + 1 is not representable; nearest is used
        assert_eq!(round_half_away_from_zero(big), big);
        assert_eq!(
            round_half_away_from_zero(4_503_599_627_370_497.0),
            4_503_599_627_370_497.0
        );
        assert!(round_half_away_from_zero(f64::NAN).is_nan());
        assert_eq!(round_half_away_from_zero(f64::INFINITY), f64::INFINITY);
        assert_eq!(
            round_half_away_from_zero(f64::NEG_INFINITY),
            f64::NEG_INFINITY
        );
    }

    #[test]
    fn test_float_to_int_saturates() {
        assert_eq!(u8::from_raster_f64(300.0), 255);
        assert_eq!(u8::from_raster_f64(-5.0), 0);
        assert_eq!(i8::from_raster_f64(1e9), 127);
        assert_eq!(i8::from_raster_f64(-1e9), -128);
        assert_eq!(u8::from_raster_f64(f64::NAN), 0);
        assert_eq!(i32::from_raster_f64(f64::INFINITY), i32::MAX);
        assert_eq!(i32::from_raster_f64(f64::NEG_INFINITY), i32::MIN);
    }

    #[test]
    fn test_int_bridge_is_exact_beyond_f64_mantissa() {
        let value = (1i128 << 53) + 1;
        assert_eq!(i64::from_raster_i128(value), 9_007_199_254_740_993);
        assert_eq!(u64::from_raster_i128(u64::MAX as i128), u64::MAX);
        assert_eq!(u64::from_raster_i128(-1), 0);
        assert_eq!(i32::from_raster_i128(i128::MAX), i32::MAX);
        assert_eq!(i32::from_raster_i128(i128::MIN), i32::MIN);
    }

    #[test]
    fn test_from_ne_slice_truncates_and_pads() {
        let bytes = 0x0102_0304u32.to_ne_bytes();
        assert_eq!(u32::from_ne_slice(&bytes), 0x0102_0304);
        // Extra bytes (complex stride) are ignored.
        let mut padded = bytes.to_vec();
        padded.extend_from_slice(&[0xFF; 4]);
        assert_eq!(u32::from_ne_slice(&padded), 0x0102_0304);
        // Short slices yield the default instead of panicking.
        assert_eq!(u32::from_ne_slice(&bytes[..2]), 0);
    }

    #[test]
    fn test_elements_as_bytes() {
        let values = [0x0102u16, 0x0304u16];
        let bytes = elements_as_bytes(&values);
        assert_eq!(bytes.len(), 4);
        assert_eq!(
            bytes,
            &[0x0102u16.to_ne_bytes(), 0x0304u16.to_ne_bytes()].concat()[..]
        );
    }

    #[test]
    fn test_complex_source_reads_real_component() {
        // CFloat32 pixel = [real f32, imaginary f32]
        let mut raw = Vec::new();
        raw.extend_from_slice(&1.5f32.to_ne_bytes());
        raw.extend_from_slice(&9.0f32.to_ne_bytes());
        raw.extend_from_slice(&(-2.5f32).to_ne_bytes());
        raw.extend_from_slice(&7.0f32.to_ne_bytes());

        let mut dst = [0.0f64; 2];
        convert_raw_into(&raw, RasterDataType::CFloat32, &mut dst).expect("convert");
        assert_eq!(dst, [1.5, -2.5]);
    }

    #[test]
    fn test_complex_destination_zeroes_imaginary() {
        let src: Vec<u8> = [1.5f32, -2.5]
            .iter()
            .flat_map(|v| v.to_ne_bytes())
            .collect();
        let mut dst = vec![0xAAu8; 16];
        convert_raw_bytes(
            &src,
            RasterDataType::Float32,
            &mut dst,
            RasterDataType::CFloat32,
            FloatToIntRounding::Nearest,
        )
        .expect("convert");

        assert_eq!(&dst[0..4], &1.5f32.to_ne_bytes());
        assert_eq!(&dst[4..8], &0f32.to_ne_bytes());
        assert_eq!(&dst[8..12], &(-2.5f32).to_ne_bytes());
        assert_eq!(&dst[12..16], &0f32.to_ne_bytes());
    }

    #[test]
    fn test_length_validation() {
        let src = [0u8; 6];
        let mut dst = [0u16; 3];
        // 6 bytes / 4-byte samples is not a whole number of samples.
        assert!(convert_raw_into(&src, RasterDataType::Float32, &mut dst).is_err());
        // 6 bytes / 2-byte samples = 3 samples, matching dst.
        assert!(convert_raw_into(&src, RasterDataType::UInt16, &mut dst).is_ok());
        // Mismatched destination length.
        let mut short = [0u16; 2];
        assert!(convert_raw_into(&src, RasterDataType::UInt16, &mut short).is_err());
    }

    #[test]
    fn test_unaligned_source_slice() {
        // Deliberately start the sample data at an odd offset so the source can
        // never be aligned for f64.
        let mut raw = vec![0u8; 1];
        for value in [1.0f64, -2.0, 3.5] {
            raw.extend_from_slice(&value.to_ne_bytes());
        }
        let mut dst = [0.0f32; 3];
        convert_raw_into(&raw[1..], RasterDataType::Float64, &mut dst).expect("convert");
        assert_eq!(dst, [1.0f32, -2.0, 3.5]);
    }

    #[test]
    fn test_memcpy_fast_path_is_bit_exact() {
        let values = [f32::NAN, f32::INFINITY, -0.0, 1.25];
        let raw: Vec<u8> = values.iter().flat_map(|v| v.to_ne_bytes()).collect();
        let mut dst = [0.0f32; 4];
        convert_raw_into(&raw, RasterDataType::Float32, &mut dst).expect("convert");
        assert!(dst[0].is_nan());
        assert_eq!(dst[1], f32::INFINITY);
        assert_eq!(dst[2].to_bits(), (-0.0f32).to_bits());
        assert_eq!(dst[3], 1.25);
    }

    #[test]
    fn test_empty_input() {
        let mut dst: [f64; 0] = [];
        convert_raw_into(&[], RasterDataType::UInt16, &mut dst).expect("convert");
        convert_raw_into(&[], RasterDataType::Float64, &mut dst).expect("memcpy path");
    }

    // -- bulk fast paths vs the per-sample reference ------------------------

    /// The conversion kernel exactly as it was before the bulk paths existed:
    /// one sample assembled from its byte chunk, no slice reinterpretation and
    /// no typed loads.
    ///
    /// Every fast path must agree with this **bit for bit**, which is what
    /// [`assert_paths_agree`] checks — on NaN payloads, `-0.0`, denormals and
    /// every integer bound, with the source and/or the destination deliberately
    /// misaligned.
    fn reference_convert<S: RasterElement, D: RasterElement>(
        src: &[u8],
        dst: &mut [D],
        rounding: FloatToIntRounding,
    ) {
        for (chunk, out) in src.chunks_exact(S::SIZE).zip(dst.iter_mut()) {
            let value = S::from_ne_slice(chunk);
            *out = match (S::KIND, D::KIND, rounding) {
                (RasterElementKind::Integer, RasterElementKind::Integer, _) => {
                    D::from_raster_i128(value.to_raster_i128())
                }
                (_, _, FloatToIntRounding::Nearest) => D::from_raster_f64(value.to_raster_f64()),
                (_, _, FloatToIntRounding::Truncate) => {
                    D::from_raster_f64_truncating(value.to_raster_f64())
                }
            };
        }
    }

    /// Asserts two conversion results are the same, bit for bit.
    ///
    /// The one admitted exception is a `NaN` **payload** under Miri. Rust does
    /// not specify which payload a float cast produces, and Miri deliberately
    /// varies it between otherwise identical operations precisely to catch code
    /// that relies on one. On a real target both paths run the same instruction
    /// on the same input and the payloads do match, which is what the strict
    /// comparison below pins down everywhere except under the interpreter. The
    /// exception is narrow on purpose: it applies only when *both* sides are
    /// `NaN`, and never to an integer destination (where `NaN` maps to `0`
    /// deterministically).
    fn assert_same_bits<D: RasterElement>(actual: &[D], expected: &[D], label: &str) {
        if cfg!(miri)
            && D::KIND == RasterElementKind::Float
            && actual.len() == expected.len()
            && actual.iter().zip(expected.iter()).all(|(a, b)| {
                let (a, b) = (a.to_raster_f64(), b.to_raster_f64());
                a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
            })
        {
            return;
        }
        assert_eq!(
            elements_as_bytes(actual),
            elements_as_bytes(expected),
            "{label}"
        );
    }

    /// Runs every code path for `S → D` over `values` and asserts they produce
    /// byte-identical output.
    ///
    /// Paths exercised: the reference loop, the typed entry point with an
    /// aligned source, the same with a source deliberately offset by one byte
    /// (unaligned path), and the byte entry point with an aligned and with a
    /// misaligned destination.
    fn assert_paths_agree<S: RasterElement, D: RasterElement>(
        values: &[S],
        rounding: FloatToIntRounding,
    ) {
        let count = values.len();
        let src = elements_as_bytes(values);

        let mut expected = vec![D::default(); count];
        reference_convert::<S, D>(src, &mut expected, rounding);

        let label = |path: &str| {
            format!(
                "{:?} -> {:?} ({:?}) mismatch on the {path} path",
                S::DATA_TYPE,
                D::DATA_TYPE,
                rounding
            )
        };

        // Typed destination, aligned source.
        let mut typed = vec![D::default(); count];
        convert_raw_into_with(src, S::DATA_TYPE, &mut typed, rounding).expect("typed convert");
        assert_same_bits(&typed, &expected, &label("typed"));

        // Typed destination, source shifted one byte out of alignment.
        let mut shifted = Vec::with_capacity(src.len() + 1);
        shifted.push(0xA5u8);
        shifted.extend_from_slice(src);
        let mut unaligned = vec![D::default(); count];
        convert_raw_into_with(&shifted[1..], S::DATA_TYPE, &mut unaligned, rounding)
            .expect("unaligned convert");
        assert_same_bits(&unaligned, &expected, &label("unaligned-source"));

        // Byte destination, both sides aligned.
        let mut bytes_out = vec![D::default(); count];
        convert_raw_bytes(
            src,
            S::DATA_TYPE,
            byte_view(&mut bytes_out),
            D::DATA_TYPE,
            rounding,
        )
        .expect("byte convert");
        assert_same_bits(&bytes_out, &expected, &label("bytes"));

        // Byte destination, destination shifted one byte out of alignment.
        let mut shifted_out = vec![0u8; count * D::SIZE + 1];
        convert_raw_bytes(
            &shifted[1..],
            S::DATA_TYPE,
            &mut shifted_out[1..],
            D::DATA_TYPE,
            rounding,
        )
        .expect("byte convert (misaligned)");
        // Decode the written bytes back with `from_ne_bytes` — a pure reinterpret
        // with no arithmetic in it, so this compares exactly what was stored.
        let mut recovered = vec![D::default(); count];
        for (slot, chunk) in recovered
            .iter_mut()
            .zip(shifted_out[1..].chunks_exact(D::SIZE))
        {
            *slot = D::from_ne_slice(chunk);
        }
        assert_same_bits(&recovered, &expected, &label("unaligned-destination"));
    }

    /// The native-endian byte image of a typed buffer, as a unique borrow.
    fn byte_view<D: RasterElement>(values: &mut [D]) -> &mut [u8] {
        // SAFETY: identical to `elements_as_bytes`, for a unique borrow: `D` is
        // a primitive numeric (sealed trait) with no padding and no invalid bit
        // patterns, so every byte is initialised and any byte value written back
        // is a valid `D`. `u8` needs no alignment and the slice covers exactly
        // the same allocation.
        unsafe {
            core::slice::from_raw_parts_mut(
                values.as_mut_ptr().cast::<u8>(),
                core::mem::size_of_val(values),
            )
        }
    }

    /// Repeats `values` to a length that is not a multiple of any plausible
    /// vector width, so the vectorised body *and* its scalar tail are covered.
    fn padded<S: RasterElement>(values: &[S]) -> Vec<S> {
        // Miri interprets every one of these conversions, and the matrix covers
        // 100 type pairs under two rounding modes; a shorter run still crosses
        // the vector body and its tail, which is all the UB checks need.
        const LENGTH: usize = if cfg!(miri) { 37 } else { 293 };
        let mut out = Vec::with_capacity(LENGTH);
        while out.len() < LENGTH {
            out.extend_from_slice(values);
        }
        out.truncate(LENGTH);
        out
    }

    /// Cross-checks one source type against all ten destination types, under
    /// both rounding modes.
    fn check_all_destinations<S: RasterElement>(values: &[S]) {
        let values = padded(values);
        for rounding in [FloatToIntRounding::Nearest, FloatToIntRounding::Truncate] {
            assert_paths_agree::<S, u8>(&values, rounding);
            assert_paths_agree::<S, i8>(&values, rounding);
            assert_paths_agree::<S, u16>(&values, rounding);
            assert_paths_agree::<S, i16>(&values, rounding);
            assert_paths_agree::<S, u32>(&values, rounding);
            assert_paths_agree::<S, i32>(&values, rounding);
            assert_paths_agree::<S, u64>(&values, rounding);
            assert_paths_agree::<S, i64>(&values, rounding);
            assert_paths_agree::<S, f32>(&values, rounding);
            assert_paths_agree::<S, f64>(&values, rounding);
        }
    }

    #[test]
    fn test_bulk_paths_match_reference_for_integer_sources() {
        check_all_destinations::<u8>(&[0, 1, 127, 128, 254, 255]);
        check_all_destinations::<i8>(&[i8::MIN, -128 + 1, -1, 0, 1, 126, i8::MAX]);
        check_all_destinations::<u16>(&[0, 1, 255, 256, 32_767, 32_768, 65_534, u16::MAX]);
        check_all_destinations::<i16>(&[i16::MIN, -32_767, -1, 0, 1, 255, 256, i16::MAX]);
        check_all_destinations::<u32>(&[0, 1, 65_535, 65_536, 2_147_483_647, u32::MAX]);
        check_all_destinations::<i32>(&[i32::MIN, -1, 0, 1, 16_777_217, i32::MAX]);
        check_all_destinations::<u64>(&[0, 1, (1 << 53) + 1, u64::MAX - 1, u64::MAX]);
        check_all_destinations::<i64>(&[i64::MIN, -(1 << 53) - 1, -1, 0, (1 << 53) + 1, i64::MAX]);
    }

    #[test]
    fn test_bulk_paths_match_reference_for_float_sources() {
        // NaN with a non-default payload, both signs of zero, subnormals, the
        // integer-bound neighbourhood and the halfway cases the rounding modes
        // disagree on.
        let f32_values = [
            f32::from_bits(0x7FC0_1234),
            f32::from_bits(0xFFC0_0001),
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            0.0,
            -0.0,
            f32::MIN_POSITIVE,
            f32::from_bits(1),  // smallest subnormal
            -f32::from_bits(1), // negative subnormal
            2.5,
            -2.5,
            0.499_999_97,
            255.5,
            -0.5,
            65_535.5,
            f32::MAX,
            f32::MIN,
            16_777_217.0,
            -16_777_217.0,
        ];
        check_all_destinations::<f32>(&f32_values);

        let f64_values = [
            f64::from_bits(0x7FF8_0000_1234_5678),
            f64::from_bits(0xFFF8_0000_0000_0001),
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            0.0,
            -0.0,
            f64::MIN_POSITIVE,
            f64::from_bits(1),
            -f64::from_bits(1),
            2.5,
            -2.5,
            0.499_999_999_999_999_94,
            255.5,
            -0.5,
            4_294_967_295.5,
            9_007_199_254_740_993.0,
            f64::MAX,
            f64::MIN,
            1e300,
        ];
        check_all_destinations::<f64>(&f64_values);
    }

    #[test]
    fn test_misaligned_source_takes_the_unaligned_path() {
        // Guards the assumption the bit-identity test relies on: a `Vec<u8>`
        // offset by one byte really is rejected by the aligned reinterpretation
        // (for every multi-byte type), so the unaligned path is genuinely
        // covered rather than silently skipped.
        // Heap-allocated on purpose: the allocator hands back a well-aligned
        // block, so `&raw[1..]` is reliably *mis*aligned for every wider type.
        let raw: Vec<u8> = (0u8..64).collect();
        assert!(
            samples_from_bytes::<u8>(&raw[1..]).is_some(),
            "u8 is align 1"
        );
        assert!(samples_from_bytes::<u16>(&raw[1..]).is_none());
        assert!(samples_from_bytes::<f32>(&raw[1..]).is_none());
        assert!(samples_from_bytes::<f64>(&raw[1..]).is_none());
        // A length that is not a whole number of samples is rejected too.
        assert!(samples_from_bytes::<f64>(&raw[..12]).is_none());
    }

    #[test]
    fn test_unaligned_load_decodes_exactly_like_from_ne_bytes() {
        // The unaligned path swaps `S::from_ne_bytes` for a typed unaligned
        // read. This pins the claim that the two are the same decode, bit for
        // bit — the byte patterns below are a NaN with a payload, a negative
        // zero, a subnormal and a negative infinity.
        let values = [
            f32::from_bits(0x7FC0_1234),
            -0.0,
            f32::from_bits(1),
            1.25,
            f32::NEG_INFINITY,
        ];
        let mut shifted = vec![0xA5u8];
        shifted.extend_from_slice(elements_as_bytes(&values));

        // Run the very same bytes through the *integer* instantiation: `u32` is
        // exact end to end (no float cast is involved anywhere), so this asserts
        // the load itself is bit-exact — including under Miri, which is free to
        // give a float cast a different NaN payload each time.
        let expected_bits: Vec<u32> = values.iter().map(|v| v.to_bits()).collect();
        let mut as_bits = [0u32; 5];
        convert_unaligned_samples::<u32, u32>(
            &shifted[1..],
            &mut as_bits,
            FloatToIntRounding::Nearest,
        );
        assert_eq!(as_bits.as_slice(), expected_bits.as_slice());

        let mut bytes_out = vec![0u8; values.len() * 4 + 1];
        convert_unaligned_bytes::<u32, u32>(
            &shifted[1..],
            &mut bytes_out[1..],
            FloatToIntRounding::Nearest,
        );
        assert_eq!(&bytes_out[1..], elements_as_bytes(&values));

        // And once as `f32`, where the value must survive the `f64` round trip
        // the conversion performs (payloads excepted under Miri, see
        // `assert_same_bits`).
        let mut decoded = [0.0f32; 5];
        convert_unaligned_samples::<f32, f32>(
            &shifted[1..],
            &mut decoded,
            FloatToIntRounding::Nearest,
        );
        assert_same_bits(&decoded, &values, "unaligned f32 decode");
    }

    #[test]
    fn test_sample_capacity_never_divides_by_zero() {
        assert_eq!(sample_capacity(9, 4), 2);
        assert_eq!(sample_capacity(0, 4), 0);
        assert_eq!(sample_capacity(9, 0), 0);
    }
}
