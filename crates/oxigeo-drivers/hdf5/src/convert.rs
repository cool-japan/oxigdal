//! Conversions between the real Pure-Rust [`oxih5`] HDF5 types and the
//! oxigeo-hdf5 driver types.
//!
//! The driver reads genuine `.h5` / NetCDF-4 files through `oxih5` and maps the
//! decoded datatypes, attribute values, and errors onto the driver's own public
//! surface so dependents keep the same API.

use crate::attribute::AttributeValue;
use crate::datatype::{CompoundMember, Datatype, EnumMember, StringPadding};
use crate::error::Hdf5Error;
use oxih5::{AttrView, ByteOrder, Dtype, OxiH5Error};

/// Map an `oxih5` [`Dtype`] onto the closest oxigeo [`Datatype`].
///
/// Datatypes with no direct oxigeo equivalent (references, bitfields) are
/// represented as [`Datatype::Opaque`] carrying the correct element size and a
/// descriptive tag so shape/size reporting stays meaningful.
pub(crate) fn map_dtype(dtype: &Dtype) -> Datatype {
    match dtype {
        Dtype::Int { size, signed, .. } => match (*size, *signed) {
            (1, true) => Datatype::Int8,
            (1, false) => Datatype::UInt8,
            (2, true) => Datatype::Int16,
            (2, false) => Datatype::UInt16,
            (4, true) => Datatype::Int32,
            (4, false) => Datatype::UInt32,
            (8, true) => Datatype::Int64,
            (8, false) => Datatype::UInt64,
            (other, _) => Datatype::Opaque {
                size: other,
                tag: "int".to_string(),
            },
        },
        Dtype::Float { size, .. } => match *size {
            4 => Datatype::Float32,
            8 => Datatype::Float64,
            other => Datatype::Opaque {
                size: other,
                tag: "float".to_string(),
            },
        },
        Dtype::String {
            fixed_len: Some(n), ..
        } => Datatype::FixedString {
            length: *n,
            padding: StringPadding::NullTerminated,
        },
        Dtype::String {
            fixed_len: None, ..
        } => Datatype::VarString {
            padding: StringPadding::NullTerminated,
        },
        Dtype::Array { base, dims } => Datatype::Array {
            base_type: Box::new(map_dtype(base)),
            dimensions: dims.clone(),
        },
        Dtype::Enum { base, members } => Datatype::Enum {
            base_type: Box::new(map_dtype(base)),
            members: members
                .iter()
                .map(|(name, value)| EnumMember::new(name.clone(), *value))
                .collect(),
        },
        Dtype::VarLen { base } => Datatype::VarLen {
            base_type: Box::new(map_dtype(base)),
        },
        Dtype::Opaque { size, tag } => Datatype::Opaque {
            size: *size,
            tag: tag.clone(),
        },
        Dtype::Compound { fields } => {
            let size = dtype.size().unwrap_or(0);
            let members = fields
                .iter()
                .map(|f| CompoundMember::new(f.name.clone(), map_dtype(&f.dtype), f.offset))
                .collect();
            Datatype::Compound { size, members }
        }
        Dtype::Reference { .. } => Datatype::Opaque {
            size: 8,
            tag: "reference".to_string(),
        },
        Dtype::Bitfield { size, .. } => Datatype::Opaque {
            size: *size,
            tag: "bitfield".to_string(),
        },
    }
}

/// `true` when a dataset of this datatype stores its elements as a plain,
/// directly-usable little-endian byte buffer (so the raw bytes can be handed
/// back verbatim by the reader).
pub(crate) fn is_storable(dtype: &Datatype) -> bool {
    dtype.is_integer() || dtype.is_float() || matches!(dtype, Datatype::FixedString { .. })
}

/// `true` when the real on-disk numeric datatype message declares
/// big-endian byte order.
///
/// Oxigeo's own [`Datatype`] enum (unlike `oxih5`'s [`Dtype`]) carries no
/// byte-order flag — every consumer of a stored dataset's raw bytes
/// (`Hdf5Reader::read_f32`/`read_f64`/`read_i32`/`read_slice`) assumes
/// little-endian. This lets the reader normalize big-endian source bytes to
/// little-endian once, at read time, rather than silently misreading every
/// element of a big-endian-authored file.
pub(crate) fn is_big_endian(dtype: &Dtype) -> bool {
    matches!(
        dtype,
        Dtype::Int {
            order: ByteOrder::Big,
            ..
        } | Dtype::Float {
            order: ByteOrder::Big,
            ..
        }
    )
}

/// Decode an `oxih5` attribute view into an oxigeo [`AttributeValue`].
///
/// Returns `None` for attribute datatypes with no oxigeo representation
/// (compound, reference, opaque, bitfield, vlen sequences), so callers can skip
/// them without failing the whole read.
pub(crate) fn decode_attr(view: &AttrView<'_>) -> Option<AttributeValue> {
    let scalar = view.is_scalar();
    // Trust the attribute's DECLARED element count (its dataspace), not the raw
    // byte length. Some HDF5 writers pad scalar or small attributes with
    // trailing zero bytes up to an 8-byte floor; decoding the whole buffer by
    // dtype-sized chunks would then surface phantom extra elements. The
    // dataspace shape yields the true count (`Scalar` → `[]` → 1, `Null` →
    // `[0]` → 0, `Simple` → product of dims), so the real payload is
    // `count * dtype_size` bytes and any trailing padding is ignored.
    let count =
        usize::try_from(view.attr.shape().iter().copied().product::<u64>()).unwrap_or(usize::MAX);
    match view.dtype() {
        Dtype::String { .. } => {
            let strings = view.as_strings().ok()?;
            if scalar {
                Some(AttributeValue::String(
                    strings.into_iter().next().unwrap_or_default(),
                ))
            } else {
                Some(AttributeValue::StringArray(strings))
            }
        }
        Dtype::Float { size, order } => {
            decode_floats(&view.attr.data, *size, *order, scalar, count)
        }
        Dtype::Int {
            size,
            signed,
            order,
        } => decode_ints(&view.attr.data, *size, *signed, *order, scalar, count),
        Dtype::Enum { base, .. } => match base.as_ref() {
            Dtype::Int {
                size,
                signed,
                order,
            } => decode_ints(&view.attr.data, *size, *signed, *order, scalar, count),
            _ => None,
        },
        _ => None,
    }
}

/// Drop trailing writer padding from an attribute's raw byte buffer.
///
/// Returns the prefix of `data` holding exactly `count` elements of `size`
/// bytes each. When `data` is longer than that declared payload (the writer
/// appended zero padding), the excess is dropped. When `data` is already the
/// right length — or is *shorter* than expected — it is returned verbatim, so
/// genuine multi-element arrays are never truncated and short/degenerate
/// buffers keep their existing lenient decoding.
fn trim_padding(data: &[u8], count: usize, size: usize) -> &[u8] {
    let expected = count.saturating_mul(size);
    if expected > 0 && data.len() > expected {
        &data[..expected]
    } else {
        data
    }
}

fn decode_floats(
    data: &[u8],
    size: usize,
    order: ByteOrder,
    scalar: bool,
    count: usize,
) -> Option<AttributeValue> {
    let data = trim_padding(data, count, size);
    match size {
        4 => {
            let vals: Vec<f32> = data
                .chunks_exact(4)
                .map(|c| {
                    let a = [c[0], c[1], c[2], c[3]];
                    match order {
                        ByteOrder::Little => f32::from_le_bytes(a),
                        ByteOrder::Big => f32::from_be_bytes(a),
                    }
                })
                .collect();
            if scalar {
                Some(AttributeValue::Float32(*vals.first()?))
            } else {
                Some(AttributeValue::Float32Array(vals))
            }
        }
        8 => {
            let vals: Vec<f64> = data
                .chunks_exact(8)
                .map(|c| {
                    let mut a = [0u8; 8];
                    a.copy_from_slice(c);
                    match order {
                        ByteOrder::Little => f64::from_le_bytes(a),
                        ByteOrder::Big => f64::from_be_bytes(a),
                    }
                })
                .collect();
            if scalar {
                Some(AttributeValue::Float64(*vals.first()?))
            } else {
                Some(AttributeValue::Float64Array(vals))
            }
        }
        _ => None,
    }
}

fn decode_ints(
    data: &[u8],
    size: usize,
    signed: bool,
    order: ByteOrder,
    scalar: bool,
    count: usize,
) -> Option<AttributeValue> {
    let data = trim_padding(data, count, size);
    macro_rules! decode {
        ($ty:ty, $n:expr, $scalar_variant:ident, $array_variant:ident) => {{
            let vals: Vec<$ty> = data
                .chunks_exact($n)
                .map(|c| {
                    let mut a = [0u8; $n];
                    a.copy_from_slice(c);
                    match order {
                        ByteOrder::Little => <$ty>::from_le_bytes(a),
                        ByteOrder::Big => <$ty>::from_be_bytes(a),
                    }
                })
                .collect();
            if scalar {
                Some(AttributeValue::$scalar_variant(*vals.first()?))
            } else {
                Some(AttributeValue::$array_variant(vals))
            }
        }};
    }

    match (size, signed) {
        (1, true) => decode!(i8, 1, Int8, Int8Array),
        (1, false) => decode!(u8, 1, UInt8, UInt8Array),
        (2, true) => decode!(i16, 2, Int16, Int16Array),
        (2, false) => decode!(u16, 2, UInt16, UInt16Array),
        (4, true) => decode!(i32, 4, Int32, Int32Array),
        (4, false) => decode!(u32, 4, UInt32, UInt32Array),
        (8, true) => decode!(i64, 8, Int64, Int64Array),
        (8, false) => decode!(u64, 8, UInt64, UInt64Array),
        _ => None,
    }
}

/// Map an oxigeo [`Datatype`] onto the `oxih5` [`Dtype`] used when creating a
/// zero-filled dataset through the real writer.
///
/// `oxih5`'s zero-fill `create_dataset` only accepts the element types
/// `i32`, `i64`, `f32`, `f64`, and `u8`; every other datatype returns a typed
/// error so the writer fails loud rather than silently mis-encoding.
pub(crate) fn to_oxih5_dtype(dt: &Datatype) -> Result<Dtype, Hdf5Error> {
    let d = match dt {
        Datatype::UInt8 => Dtype::Int {
            size: 1,
            signed: false,
            order: ByteOrder::Little,
        },
        Datatype::Int32 => Dtype::Int {
            size: 4,
            signed: true,
            order: ByteOrder::Little,
        },
        Datatype::Int64 => Dtype::Int {
            size: 8,
            signed: true,
            order: ByteOrder::Little,
        },
        Datatype::Float32 => Dtype::Float {
            size: 4,
            order: ByteOrder::Little,
        },
        Datatype::Float64 => Dtype::Float {
            size: 8,
            order: ByteOrder::Little,
        },
        other => {
            return Err(Hdf5Error::feature_not_available(format!(
                "creating a real HDF5 dataset of type {} (the real writer supports zero-filled i32, i64, f32, f64, u8)",
                other.name()
            )));
        }
    };
    Ok(d)
}

/// Map an `oxih5` error onto the driver's [`Hdf5Error`] taxonomy.
pub(crate) fn map_oxih5_err(e: OxiH5Error) -> Hdf5Error {
    match e {
        OxiH5Error::Io(io) => Hdf5Error::Io(io),
        OxiH5Error::BadSignature => Hdf5Error::InvalidSignature(Vec::new()),
        OxiH5Error::UnsupportedSuperblock(v) => Hdf5Error::UnsupportedSuperblockVersion(v),
        OxiH5Error::UnsupportedHeader(v) => {
            Hdf5Error::InvalidObjectHeader(format!("unsupported object header version {v}"))
        }
        OxiH5Error::UnsupportedDatatype(c) => {
            Hdf5Error::UnsupportedDatatype(format!("datatype class {c}"))
        }
        OxiH5Error::UnsupportedLayout(c) => {
            Hdf5Error::UnsupportedLayout(format!("data layout class {c}"))
        }
        OxiH5Error::NotFound(s) => Hdf5Error::DatasetNotFound(s),
        OxiH5Error::TypeMismatch => Hdf5Error::type_conversion("oxih5 value", "oxigeo value"),
        OxiH5Error::DataTruncated => Hdf5Error::InvalidSize("data buffer truncated".to_string()),
        OxiH5Error::NotImplemented(s) => Hdf5Error::feature_not_available(s),
        OxiH5Error::Format(s) => Hdf5Error::InvalidFormat(s),
        OxiH5Error::UnsupportedFilter(s) => Hdf5Error::UnsupportedCompressionFilter(s),
        OxiH5Error::Corrupted(s) => Hdf5Error::InvalidFormat(format!("corrupted: {s}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_big_endian_true_for_big_endian_int_and_float() {
        assert!(is_big_endian(&Dtype::Int {
            size: 4,
            signed: true,
            order: ByteOrder::Big,
        }));
        assert!(is_big_endian(&Dtype::Float {
            size: 8,
            order: ByteOrder::Big,
        }));
    }

    #[test]
    fn test_is_big_endian_false_for_little_endian_and_non_numeric() {
        assert!(!is_big_endian(&Dtype::Int {
            size: 4,
            signed: true,
            order: ByteOrder::Little,
        }));
        assert!(!is_big_endian(&Dtype::Float {
            size: 8,
            order: ByteOrder::Little,
        }));
        // Non-numeric datatypes carry no byte order at all — never
        // classified as big-endian.
        assert!(!is_big_endian(&Dtype::Opaque {
            size: 4,
            tag: "test".to_string(),
        }));
    }

    /// Regression for the oxih5 0.2.1 FileWriter padding quirk: a scalar i32
    /// attribute whose 4-byte payload is written with trailing zero padding up
    /// to an 8-byte floor must decode to exactly one value (the declared count
    /// is 1, so the phantom trailing zero is dropped before decoding).
    #[test]
    fn test_decode_ints_scalar_i32_ignores_padding() {
        let padded = [0xF1, 0xD8, 0xFF, 0xFF, 0, 0, 0, 0];
        assert_eq!(
            decode_ints(&padded, 4, true, ByteOrder::Little, true, 1),
            Some(AttributeValue::Int32(-9999))
        );
    }

    /// A genuine multi-element i32 array (declared count 3, 12 bytes, no
    /// padding) must decode to all three values — the guard must never
    /// truncate legitimate arrays.
    #[test]
    fn test_decode_ints_multi_element_not_truncated() {
        let mut data = Vec::new();
        for v in [1i32, 2, 3] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(
            decode_ints(&data, 4, true, ByteOrder::Little, false, 3),
            Some(AttributeValue::Int32Array(vec![1, 2, 3]))
        );
    }

    /// A small non-scalar array whose payload (< 8 bytes) was padded up to the
    /// 8-byte floor must still decode to its *declared* element count, not the
    /// phantom count implied by the padded buffer. Here a 2-element i16 array
    /// (4 real bytes + 4 padding) must yield exactly two values.
    #[test]
    fn test_decode_ints_small_padded_array_uses_declared_count() {
        // 7i16 = [07 00], -3i16 = [FD FF], then 4 bytes of writer padding.
        let padded = [0x07, 0x00, 0xFD, 0xFF, 0, 0, 0, 0];
        assert_eq!(
            decode_ints(&padded, 2, true, ByteOrder::Little, false, 2),
            Some(AttributeValue::Int16Array(vec![7, -3]))
        );
    }

    /// The same padding guard applies to floating-point scalars: a scalar f32
    /// attribute padded to 8 bytes must decode to exactly one value.
    #[test]
    fn test_decode_floats_scalar_f32_ignores_padding() {
        let mut data = 1.5f32.to_le_bytes().to_vec();
        data.extend_from_slice(&[0, 0, 0, 0]);
        assert_eq!(
            decode_floats(&data, 4, ByteOrder::Little, true, 1),
            Some(AttributeValue::Float32(1.5))
        );
    }
}
