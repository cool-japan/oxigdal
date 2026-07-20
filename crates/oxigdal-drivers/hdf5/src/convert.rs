//! Conversions between the real Pure-Rust [`oxih5`] HDF5 types and the
//! oxigdal-hdf5 driver types.
//!
//! The driver reads genuine `.h5` / NetCDF-4 files through `oxih5` and maps the
//! decoded datatypes, attribute values, and errors onto the driver's own public
//! surface so dependents keep the same API.

use crate::attribute::AttributeValue;
use crate::datatype::{CompoundMember, Datatype, EnumMember, StringPadding};
use crate::error::Hdf5Error;
use oxih5::{AttrView, ByteOrder, Dtype, OxiH5Error};

/// Map an `oxih5` [`Dtype`] onto the closest oxigdal [`Datatype`].
///
/// Datatypes with no direct oxigdal equivalent (references, bitfields) are
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

/// Decode an `oxih5` attribute view into an oxigdal [`AttributeValue`].
///
/// Returns `None` for attribute datatypes with no oxigdal representation
/// (compound, reference, opaque, bitfield, vlen sequences), so callers can skip
/// them without failing the whole read.
pub(crate) fn decode_attr(view: &AttrView<'_>) -> Option<AttributeValue> {
    let scalar = view.is_scalar();
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
        Dtype::Float { size, order } => decode_floats(&view.attr.data, *size, *order, scalar),
        Dtype::Int {
            size,
            signed,
            order,
        } => decode_ints(&view.attr.data, *size, *signed, *order, scalar),
        Dtype::Enum { base, .. } => match base.as_ref() {
            Dtype::Int {
                size,
                signed,
                order,
            } => decode_ints(&view.attr.data, *size, *signed, *order, scalar),
            _ => None,
        },
        _ => None,
    }
}

fn decode_floats(
    data: &[u8],
    size: usize,
    order: ByteOrder,
    scalar: bool,
) -> Option<AttributeValue> {
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
) -> Option<AttributeValue> {
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

/// Map an oxigdal [`Datatype`] onto the `oxih5` [`Dtype`] used when creating a
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
        OxiH5Error::TypeMismatch => Hdf5Error::type_conversion("oxih5 value", "oxigdal value"),
        OxiH5Error::DataTruncated => Hdf5Error::InvalidSize("data buffer truncated".to_string()),
        OxiH5Error::NotImplemented(s) => Hdf5Error::feature_not_available(s),
        OxiH5Error::Format(s) => Hdf5Error::InvalidFormat(s),
        OxiH5Error::UnsupportedFilter(s) => Hdf5Error::UnsupportedCompressionFilter(s),
        OxiH5Error::Corrupted(s) => Hdf5Error::InvalidFormat(format!("corrupted: {s}")),
    }
}
