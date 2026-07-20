//! `FlatGeobuf` `Feature` (de)serialization
//!
//! Each feature is a size-prefixed `FlatBuffers` `Feature` table containing an
//! optional `Geometry` table and a `properties` byte buffer. The property
//! buffer is a packed sequence of `(column_index: u16, value)` entries whose
//! value encoding is determined by the column type declared in the header. Null
//! properties are omitted from the buffer.
//!
//! These helpers are shared by the synchronous, asynchronous, and HTTP readers
//! and by the writer so that the exact wire encoding lives in a single place.

use crate::error::{FlatGeobufError, Result};
use crate::fbs::{self, FbTable};
use crate::geometry::GeometryCodec;
use crate::header::{Column, ColumnType, Header};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use flatbuffers::FlatBufferBuilder;
use oxigdal_core::vector::{Feature, FieldValue};
use std::io::{Cursor, Read};

/// Encodes a feature into a bare `FlatBuffers` `Feature` message (no size
/// prefix). Callers prepend a `u32` length to obtain the on-disk record.
pub fn encode_feature(
    header: &Header,
    codec: &GeometryCodec,
    feature: &Feature,
) -> Result<Vec<u8>> {
    let mut fbb = FlatBufferBuilder::new();

    // Child offsets must be created before the Feature table is started.
    let geom_off = match feature.geometry {
        Some(ref g) => Some(codec.build(&mut fbb, g)?),
        None => None,
    };

    let props = encode_properties(header, feature)?;
    let props_off = if props.is_empty() {
        None
    } else {
        Some(fbb.create_vector::<u8>(&props))
    };

    let wip = fbb.start_table();
    if let Some(o) = geom_off {
        fbb.push_slot_always(fbs::FEATURE_VT_GEOMETRY, o);
    }
    if let Some(o) = props_off {
        fbb.push_slot_always(fbs::FEATURE_VT_PROPERTIES, o);
    }
    let feat = fbb.end_table(wip);
    fbb.finish(feat, None);
    Ok(fbb.finished_data().to_vec())
}

/// Decodes a bare `FlatBuffers` `Feature` message into an `OxiGDAL` [`Feature`].
pub fn decode_feature(header: &Header, codec: &GeometryCodec, data: &[u8]) -> Result<Feature> {
    let table = FbTable::root(data)?;

    let geometry = match table.get_table(fbs::FEATURE_VT_GEOMETRY)? {
        Some(geom_table) => Some(codec.read(&geom_table, header.geometry_type)?),
        None => None,
    };

    let mut feature = match geometry {
        Some(g) => Feature::new(g),
        None => Feature::new_attribute_only(),
    };

    // Columns absent from the property buffer are null.
    for column in &header.columns {
        feature.set_property(column.name.clone(), FieldValue::Null);
    }

    if let Some(props) = table.get_u8_vector(fbs::FEATURE_VT_PROPERTIES)? {
        decode_properties(header, props, &mut feature)?;
    }

    Ok(feature)
}

/// Encodes the non-null properties of `feature` into the `FlatGeobuf` property
/// byte buffer.
fn encode_properties(header: &Header, feature: &Feature) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    for (idx, column) in header.columns.iter().enumerate() {
        let value = match feature.get_property(&column.name) {
            Some(v) if !v.is_null() => v,
            _ => continue,
        };
        buf.write_u16::<LittleEndian>(idx as u16)?;
        write_property_value(&mut buf, value, column)?;
    }
    Ok(buf)
}

/// Decodes the `FlatGeobuf` property byte buffer into `feature`.
fn decode_properties(header: &Header, props: &[u8], feature: &mut Feature) -> Result<()> {
    let mut cursor = Cursor::new(props);
    let end = props.len() as u64;
    while cursor.position() < end {
        let idx = cursor.read_u16::<LittleEndian>()? as usize;
        let column = header.columns.get(idx).ok_or_else(|| {
            FlatGeobufError::InvalidFeature(format!("property column index {idx} out of range"))
        })?;
        let value = read_property_value(&mut cursor, column)?;
        feature.set_property(column.name.clone(), value);
    }
    Ok(())
}

/// Writes a single property value according to its column type.
fn write_property_value(buf: &mut Vec<u8>, value: &FieldValue, column: &Column) -> Result<()> {
    match column.column_type {
        ColumnType::Byte => buf.write_i8(value.as_i64().unwrap_or(0) as i8)?,
        ColumnType::UByte => buf.write_u8(value.as_u64().unwrap_or(0) as u8)?,
        ColumnType::Bool => buf.write_u8(u8::from(value.as_bool().unwrap_or(false)))?,
        ColumnType::Short => buf.write_i16::<LittleEndian>(value.as_i64().unwrap_or(0) as i16)?,
        ColumnType::UShort => buf.write_u16::<LittleEndian>(value.as_u64().unwrap_or(0) as u16)?,
        ColumnType::Int => buf.write_i32::<LittleEndian>(value.as_i64().unwrap_or(0) as i32)?,
        ColumnType::UInt => buf.write_u32::<LittleEndian>(value.as_u64().unwrap_or(0) as u32)?,
        ColumnType::Long => buf.write_i64::<LittleEndian>(value.as_i64().unwrap_or(0))?,
        ColumnType::ULong => buf.write_u64::<LittleEndian>(value.as_u64().unwrap_or(0))?,
        ColumnType::Float => buf.write_f32::<LittleEndian>(value.as_f64().unwrap_or(0.0) as f32)?,
        ColumnType::Double => buf.write_f64::<LittleEndian>(value.as_f64().unwrap_or(0.0))?,
        ColumnType::String | ColumnType::Json | ColumnType::DateTime => {
            let s = value.as_string().unwrap_or("");
            let bytes = s.as_bytes();
            buf.write_u32::<LittleEndian>(bytes.len() as u32)?;
            buf.extend_from_slice(bytes);
        }
        ColumnType::Binary => {
            let bytes = value.as_blob().unwrap_or(&[]);
            buf.write_u32::<LittleEndian>(bytes.len() as u32)?;
            buf.extend_from_slice(bytes);
        }
    }
    Ok(())
}

/// Reads a single property value according to its column type.
fn read_property_value<R: Read>(reader: &mut R, column: &Column) -> Result<FieldValue> {
    match column.column_type {
        ColumnType::Byte => Ok(FieldValue::Integer(i64::from(reader.read_i8()?))),
        ColumnType::UByte => Ok(FieldValue::UInteger(u64::from(reader.read_u8()?))),
        ColumnType::Bool => Ok(FieldValue::Bool(reader.read_u8()? != 0)),
        ColumnType::Short => Ok(FieldValue::Integer(i64::from(
            reader.read_i16::<LittleEndian>()?,
        ))),
        ColumnType::UShort => Ok(FieldValue::UInteger(u64::from(
            reader.read_u16::<LittleEndian>()?,
        ))),
        ColumnType::Int => Ok(FieldValue::Integer(i64::from(
            reader.read_i32::<LittleEndian>()?,
        ))),
        ColumnType::UInt => Ok(FieldValue::UInteger(u64::from(
            reader.read_u32::<LittleEndian>()?,
        ))),
        ColumnType::Long => Ok(FieldValue::Integer(reader.read_i64::<LittleEndian>()?)),
        ColumnType::ULong => Ok(FieldValue::UInteger(reader.read_u64::<LittleEndian>()?)),
        ColumnType::Float => Ok(FieldValue::Float(f64::from(
            reader.read_f32::<LittleEndian>()?,
        ))),
        ColumnType::Double => Ok(FieldValue::Float(reader.read_f64::<LittleEndian>()?)),
        ColumnType::String | ColumnType::Json | ColumnType::DateTime => {
            let len = reader.read_u32::<LittleEndian>()? as usize;
            let mut bytes = vec![0u8; len];
            reader.read_exact(&mut bytes)?;
            Ok(FieldValue::String(String::from_utf8(bytes)?))
        }
        ColumnType::Binary => {
            let len = reader.read_u32::<LittleEndian>()? as usize;
            let mut bytes = vec![0u8; len];
            reader.read_exact(&mut bytes)?;
            Ok(FieldValue::Blob(bytes))
        }
    }
}
