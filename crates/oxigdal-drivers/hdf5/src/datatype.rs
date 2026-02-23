//! HDF5 datatype definitions and conversions.
//!
//! This module provides type definitions for HDF5 data types, including
//! integer, floating-point, string, and compound types.

use crate::error::{Hdf5Error, Result};
use byteorder::{BigEndian, ByteOrder, LittleEndian};
use serde::{Deserialize, Serialize};
use std::fmt;

/// HDF5 datatype class
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum DatatypeClass {
    /// Fixed-point (integer) type
    FixedPoint = 0,
    /// Floating-point type
    FloatingPoint = 1,
    /// Time type
    Time = 2,
    /// String type
    String = 3,
    /// Bitfield type
    Bitfield = 4,
    /// Opaque type
    Opaque = 5,
    /// Compound type
    Compound = 6,
    /// Reference type
    Reference = 7,
    /// Enumeration type
    Enum = 8,
    /// Variable-length type
    VariableLength = 9,
    /// Array type
    Array = 10,
}

impl DatatypeClass {
    /// Create from u8 value
    pub fn from_u8(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::FixedPoint),
            1 => Ok(Self::FloatingPoint),
            2 => Ok(Self::Time),
            3 => Ok(Self::String),
            4 => Ok(Self::Bitfield),
            5 => Ok(Self::Opaque),
            6 => Ok(Self::Compound),
            7 => Ok(Self::Reference),
            8 => Ok(Self::Enum),
            9 => Ok(Self::VariableLength),
            10 => Ok(Self::Array),
            _ => Err(Hdf5Error::invalid_datatype(format!(
                "Unknown datatype class: {}",
                value
            ))),
        }
    }
}

/// Byte order (endianness) for HDF5 data
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Hdf5ByteOrder {
    /// Little-endian
    LittleEndian,
    /// Big-endian
    BigEndian,
}

/// String padding type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StringPadding {
    /// Null-terminated
    NullTerminated,
    /// Null-padded
    NullPadded,
    /// Space-padded
    SpacePadded,
}

/// HDF5 datatype
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Datatype {
    /// 8-bit signed integer
    Int8,
    /// 8-bit unsigned integer
    UInt8,
    /// 16-bit signed integer
    Int16,
    /// 16-bit unsigned integer
    UInt16,
    /// 32-bit signed integer
    Int32,
    /// 32-bit unsigned integer
    UInt32,
    /// 64-bit signed integer
    Int64,
    /// 64-bit unsigned integer
    UInt64,
    /// 32-bit floating-point
    Float32,
    /// 64-bit floating-point
    Float64,
    /// Fixed-length string
    FixedString {
        /// String length
        length: usize,
        /// Padding type
        padding: StringPadding,
    },
    /// Variable-length string
    VarString {
        /// Padding type
        padding: StringPadding,
    },
    /// Compound type (struct)
    Compound {
        /// Size in bytes
        size: usize,
        /// Member fields
        members: Vec<CompoundMember>,
    },
    /// Enumeration type
    Enum {
        /// Base integer type
        base_type: Box<Datatype>,
        /// Enum members
        members: Vec<EnumMember>,
    },
    /// Array type
    Array {
        /// Base type
        base_type: Box<Datatype>,
        /// Array dimensions
        dimensions: Vec<usize>,
    },
    /// Variable-length type
    VarLen {
        /// Base type
        base_type: Box<Datatype>,
    },
    /// Opaque type
    Opaque {
        /// Size in bytes
        size: usize,
        /// Tag
        tag: String,
    },
}

impl Datatype {
    /// Get the size in bytes of this datatype
    pub fn size(&self) -> usize {
        match self {
            Self::Int8 | Self::UInt8 => 1,
            Self::Int16 | Self::UInt16 => 2,
            Self::Int32 | Self::UInt32 | Self::Float32 => 4,
            Self::Int64 | Self::UInt64 | Self::Float64 => 8,
            Self::FixedString { length, .. } => *length,
            Self::VarString { .. } => 16, // Size of variable-length heap reference
            Self::Compound { size, .. } => *size,
            Self::Opaque { size, .. } => *size,
            Self::Array {
                base_type,
                dimensions,
            } => {
                let base_size = base_type.size();
                let total_elements: usize = dimensions.iter().product();
                base_size * total_elements
            }
            Self::VarLen { .. } => 16, // Size of variable-length heap reference
            Self::Enum { base_type, .. } => base_type.size(),
        }
    }

    /// Get the datatype class
    pub fn class(&self) -> DatatypeClass {
        match self {
            Self::Int8
            | Self::UInt8
            | Self::Int16
            | Self::UInt16
            | Self::Int32
            | Self::UInt32
            | Self::Int64
            | Self::UInt64 => DatatypeClass::FixedPoint,
            Self::Float32 | Self::Float64 => DatatypeClass::FloatingPoint,
            Self::FixedString { .. } | Self::VarString { .. } => DatatypeClass::String,
            Self::Compound { .. } => DatatypeClass::Compound,
            Self::Enum { .. } => DatatypeClass::Enum,
            Self::Array { .. } => DatatypeClass::Array,
            Self::VarLen { .. } => DatatypeClass::VariableLength,
            Self::Opaque { .. } => DatatypeClass::Opaque,
        }
    }

    /// Get a human-readable name for this datatype
    pub fn name(&self) -> String {
        match self {
            Self::Int8 => "int8".to_string(),
            Self::UInt8 => "uint8".to_string(),
            Self::Int16 => "int16".to_string(),
            Self::UInt16 => "uint16".to_string(),
            Self::Int32 => "int32".to_string(),
            Self::UInt32 => "uint32".to_string(),
            Self::Int64 => "int64".to_string(),
            Self::UInt64 => "uint64".to_string(),
            Self::Float32 => "float32".to_string(),
            Self::Float64 => "float64".to_string(),
            Self::FixedString { length, .. } => format!("string[{}]", length),
            Self::VarString { .. } => "varstring".to_string(),
            Self::Compound { members, .. } => {
                let member_names: Vec<_> = members.iter().map(|m| m.name.as_str()).collect();
                format!("compound{{{}}}", member_names.join(", "))
            }
            Self::Enum { .. } => "enum".to_string(),
            Self::Array {
                base_type,
                dimensions,
            } => {
                let dims: Vec<_> = dimensions.iter().map(|d| d.to_string()).collect();
                format!("{}[{}]", base_type.name(), dims.join(","))
            }
            Self::VarLen { base_type } => format!("varlen<{}>", base_type.name()),
            Self::Opaque { tag, .. } => format!("opaque:{}", tag),
        }
    }

    /// Check if this is an integer type
    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            Self::Int8
                | Self::UInt8
                | Self::Int16
                | Self::UInt16
                | Self::Int32
                | Self::UInt32
                | Self::Int64
                | Self::UInt64
        )
    }

    /// Check if this is a floating-point type
    pub fn is_float(&self) -> bool {
        matches!(self, Self::Float32 | Self::Float64)
    }

    /// Check if this is a string type
    pub fn is_string(&self) -> bool {
        matches!(self, Self::FixedString { .. } | Self::VarString { .. })
    }

    /// Check if this is a compound type
    pub fn is_compound(&self) -> bool {
        matches!(self, Self::Compound { .. })
    }
}

impl fmt::Display for Datatype {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Compound type member
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompoundMember {
    /// Member name
    pub name: String,
    /// Member datatype
    pub datatype: Datatype,
    /// Byte offset within compound type
    pub offset: usize,
}

impl CompoundMember {
    /// Create a new compound member
    pub fn new(name: String, datatype: Datatype, offset: usize) -> Self {
        Self {
            name,
            datatype,
            offset,
        }
    }
}

/// Enumeration type member
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumMember {
    /// Member name
    pub name: String,
    /// Member value
    pub value: i64,
}

impl EnumMember {
    /// Create a new enum member
    pub fn new(name: String, value: i64) -> Self {
        Self { name, value }
    }
}

/// Type conversion utilities
pub struct TypeConverter;

impl TypeConverter {
    /// Read i8 from bytes
    pub fn read_i8(data: &[u8]) -> Result<i8> {
        if data.is_empty() {
            return Err(Hdf5Error::invalid_datatype("Empty data for i8"));
        }
        Ok(data[0] as i8)
    }

    /// Read u8 from bytes
    pub fn read_u8(data: &[u8]) -> Result<u8> {
        if data.is_empty() {
            return Err(Hdf5Error::invalid_datatype("Empty data for u8"));
        }
        Ok(data[0])
    }

    /// Read i16 from bytes (little-endian)
    pub fn read_i16_le(data: &[u8]) -> Result<i16> {
        if data.len() < 2 {
            return Err(Hdf5Error::invalid_datatype("Insufficient data for i16"));
        }
        Ok(LittleEndian::read_i16(data))
    }

    /// Read i16 from bytes (big-endian)
    pub fn read_i16_be(data: &[u8]) -> Result<i16> {
        if data.len() < 2 {
            return Err(Hdf5Error::invalid_datatype("Insufficient data for i16"));
        }
        Ok(BigEndian::read_i16(data))
    }

    /// Read u16 from bytes (little-endian)
    pub fn read_u16_le(data: &[u8]) -> Result<u16> {
        if data.len() < 2 {
            return Err(Hdf5Error::invalid_datatype("Insufficient data for u16"));
        }
        Ok(LittleEndian::read_u16(data))
    }

    /// Read u16 from bytes (big-endian)
    pub fn read_u16_be(data: &[u8]) -> Result<u16> {
        if data.len() < 2 {
            return Err(Hdf5Error::invalid_datatype("Insufficient data for u16"));
        }
        Ok(BigEndian::read_u16(data))
    }

    /// Read i32 from bytes (little-endian)
    pub fn read_i32_le(data: &[u8]) -> Result<i32> {
        if data.len() < 4 {
            return Err(Hdf5Error::invalid_datatype("Insufficient data for i32"));
        }
        Ok(LittleEndian::read_i32(data))
    }

    /// Read i32 from bytes (big-endian)
    pub fn read_i32_be(data: &[u8]) -> Result<i32> {
        if data.len() < 4 {
            return Err(Hdf5Error::invalid_datatype("Insufficient data for i32"));
        }
        Ok(BigEndian::read_i32(data))
    }

    /// Read u32 from bytes (little-endian)
    pub fn read_u32_le(data: &[u8]) -> Result<u32> {
        if data.len() < 4 {
            return Err(Hdf5Error::invalid_datatype("Insufficient data for u32"));
        }
        Ok(LittleEndian::read_u32(data))
    }

    /// Read u32 from bytes (big-endian)
    pub fn read_u32_be(data: &[u8]) -> Result<u32> {
        if data.len() < 4 {
            return Err(Hdf5Error::invalid_datatype("Insufficient data for u32"));
        }
        Ok(BigEndian::read_u32(data))
    }

    /// Read i64 from bytes (little-endian)
    pub fn read_i64_le(data: &[u8]) -> Result<i64> {
        if data.len() < 8 {
            return Err(Hdf5Error::invalid_datatype("Insufficient data for i64"));
        }
        Ok(LittleEndian::read_i64(data))
    }

    /// Read i64 from bytes (big-endian)
    pub fn read_i64_be(data: &[u8]) -> Result<i64> {
        if data.len() < 8 {
            return Err(Hdf5Error::invalid_datatype("Insufficient data for i64"));
        }
        Ok(BigEndian::read_i64(data))
    }

    /// Read u64 from bytes (little-endian)
    pub fn read_u64_le(data: &[u8]) -> Result<u64> {
        if data.len() < 8 {
            return Err(Hdf5Error::invalid_datatype("Insufficient data for u64"));
        }
        Ok(LittleEndian::read_u64(data))
    }

    /// Read u64 from bytes (big-endian)
    pub fn read_u64_be(data: &[u8]) -> Result<u64> {
        if data.len() < 8 {
            return Err(Hdf5Error::invalid_datatype("Insufficient data for u64"));
        }
        Ok(BigEndian::read_u64(data))
    }

    /// Read f32 from bytes (little-endian)
    pub fn read_f32_le(data: &[u8]) -> Result<f32> {
        if data.len() < 4 {
            return Err(Hdf5Error::invalid_datatype("Insufficient data for f32"));
        }
        Ok(LittleEndian::read_f32(data))
    }

    /// Read f32 from bytes (big-endian)
    pub fn read_f32_be(data: &[u8]) -> Result<f32> {
        if data.len() < 4 {
            return Err(Hdf5Error::invalid_datatype("Insufficient data for f32"));
        }
        Ok(BigEndian::read_f32(data))
    }

    /// Read f64 from bytes (little-endian)
    pub fn read_f64_le(data: &[u8]) -> Result<f64> {
        if data.len() < 8 {
            return Err(Hdf5Error::invalid_datatype("Insufficient data for f64"));
        }
        Ok(LittleEndian::read_f64(data))
    }

    /// Read f64 from bytes (big-endian)
    pub fn read_f64_be(data: &[u8]) -> Result<f64> {
        if data.len() < 8 {
            return Err(Hdf5Error::invalid_datatype("Insufficient data for f64"));
        }
        Ok(BigEndian::read_f64(data))
    }

    /// Write i8 to bytes
    pub fn write_i8(data: &mut [u8], value: i8) -> Result<()> {
        if data.is_empty() {
            return Err(Hdf5Error::invalid_datatype("Empty buffer for i8"));
        }
        data[0] = value as u8;
        Ok(())
    }

    /// Write u8 to bytes
    pub fn write_u8(data: &mut [u8], value: u8) -> Result<()> {
        if data.is_empty() {
            return Err(Hdf5Error::invalid_datatype("Empty buffer for u8"));
        }
        data[0] = value;
        Ok(())
    }

    /// Write i16 to bytes (little-endian)
    pub fn write_i16_le(data: &mut [u8], value: i16) -> Result<()> {
        if data.len() < 2 {
            return Err(Hdf5Error::invalid_datatype("Insufficient buffer for i16"));
        }
        LittleEndian::write_i16(data, value);
        Ok(())
    }

    /// Write i16 to bytes (big-endian)
    pub fn write_i16_be(data: &mut [u8], value: i16) -> Result<()> {
        if data.len() < 2 {
            return Err(Hdf5Error::invalid_datatype("Insufficient buffer for i16"));
        }
        BigEndian::write_i16(data, value);
        Ok(())
    }

    /// Write u16 to bytes (little-endian)
    pub fn write_u16_le(data: &mut [u8], value: u16) -> Result<()> {
        if data.len() < 2 {
            return Err(Hdf5Error::invalid_datatype("Insufficient buffer for u16"));
        }
        LittleEndian::write_u16(data, value);
        Ok(())
    }

    /// Write u16 to bytes (big-endian)
    pub fn write_u16_be(data: &mut [u8], value: u16) -> Result<()> {
        if data.len() < 2 {
            return Err(Hdf5Error::invalid_datatype("Insufficient buffer for u16"));
        }
        BigEndian::write_u16(data, value);
        Ok(())
    }

    /// Write i32 to bytes (little-endian)
    pub fn write_i32_le(data: &mut [u8], value: i32) -> Result<()> {
        if data.len() < 4 {
            return Err(Hdf5Error::invalid_datatype("Insufficient buffer for i32"));
        }
        LittleEndian::write_i32(data, value);
        Ok(())
    }

    /// Write i32 to bytes (big-endian)
    pub fn write_i32_be(data: &mut [u8], value: i32) -> Result<()> {
        if data.len() < 4 {
            return Err(Hdf5Error::invalid_datatype("Insufficient buffer for i32"));
        }
        BigEndian::write_i32(data, value);
        Ok(())
    }

    /// Write u32 to bytes (little-endian)
    pub fn write_u32_le(data: &mut [u8], value: u32) -> Result<()> {
        if data.len() < 4 {
            return Err(Hdf5Error::invalid_datatype("Insufficient buffer for u32"));
        }
        LittleEndian::write_u32(data, value);
        Ok(())
    }

    /// Write u32 to bytes (big-endian)
    pub fn write_u32_be(data: &mut [u8], value: u32) -> Result<()> {
        if data.len() < 4 {
            return Err(Hdf5Error::invalid_datatype("Insufficient buffer for u32"));
        }
        BigEndian::write_u32(data, value);
        Ok(())
    }

    /// Write i64 to bytes (little-endian)
    pub fn write_i64_le(data: &mut [u8], value: i64) -> Result<()> {
        if data.len() < 8 {
            return Err(Hdf5Error::invalid_datatype("Insufficient buffer for i64"));
        }
        LittleEndian::write_i64(data, value);
        Ok(())
    }

    /// Write i64 to bytes (big-endian)
    pub fn write_i64_be(data: &mut [u8], value: i64) -> Result<()> {
        if data.len() < 8 {
            return Err(Hdf5Error::invalid_datatype("Insufficient buffer for i64"));
        }
        BigEndian::write_i64(data, value);
        Ok(())
    }

    /// Write u64 to bytes (little-endian)
    pub fn write_u64_le(data: &mut [u8], value: u64) -> Result<()> {
        if data.len() < 8 {
            return Err(Hdf5Error::invalid_datatype("Insufficient buffer for u64"));
        }
        LittleEndian::write_u64(data, value);
        Ok(())
    }

    /// Write u64 to bytes (big-endian)
    pub fn write_u64_be(data: &mut [u8], value: u64) -> Result<()> {
        if data.len() < 8 {
            return Err(Hdf5Error::invalid_datatype("Insufficient buffer for u64"));
        }
        BigEndian::write_u64(data, value);
        Ok(())
    }

    /// Write f32 to bytes (little-endian)
    pub fn write_f32_le(data: &mut [u8], value: f32) -> Result<()> {
        if data.len() < 4 {
            return Err(Hdf5Error::invalid_datatype("Insufficient buffer for f32"));
        }
        LittleEndian::write_f32(data, value);
        Ok(())
    }

    /// Write f32 to bytes (big-endian)
    pub fn write_f32_be(data: &mut [u8], value: f32) -> Result<()> {
        if data.len() < 4 {
            return Err(Hdf5Error::invalid_datatype("Insufficient buffer for f32"));
        }
        BigEndian::write_f32(data, value);
        Ok(())
    }

    /// Write f64 to bytes (little-endian)
    pub fn write_f64_le(data: &mut [u8], value: f64) -> Result<()> {
        if data.len() < 8 {
            return Err(Hdf5Error::invalid_datatype("Insufficient buffer for f64"));
        }
        LittleEndian::write_f64(data, value);
        Ok(())
    }

    /// Write f64 to bytes (big-endian)
    pub fn write_f64_be(data: &mut [u8], value: f64) -> Result<()> {
        if data.len() < 8 {
            return Err(Hdf5Error::invalid_datatype("Insufficient buffer for f64"));
        }
        BigEndian::write_f64(data, value);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_datatype_size() {
        assert_eq!(Datatype::Int8.size(), 1);
        assert_eq!(Datatype::UInt8.size(), 1);
        assert_eq!(Datatype::Int16.size(), 2);
        assert_eq!(Datatype::UInt16.size(), 2);
        assert_eq!(Datatype::Int32.size(), 4);
        assert_eq!(Datatype::UInt32.size(), 4);
        assert_eq!(Datatype::Int64.size(), 8);
        assert_eq!(Datatype::UInt64.size(), 8);
        assert_eq!(Datatype::Float32.size(), 4);
        assert_eq!(Datatype::Float64.size(), 8);
        assert_eq!(
            Datatype::FixedString {
                length: 10,
                padding: StringPadding::NullTerminated
            }
            .size(),
            10
        );
    }

    #[test]
    fn test_datatype_class() {
        assert_eq!(Datatype::Int32.class(), DatatypeClass::FixedPoint);
        assert_eq!(Datatype::Float64.class(), DatatypeClass::FloatingPoint);
        assert_eq!(
            Datatype::FixedString {
                length: 10,
                padding: StringPadding::NullTerminated
            }
            .class(),
            DatatypeClass::String
        );
    }

    #[test]
    fn test_datatype_name() {
        assert_eq!(Datatype::Int32.name(), "int32");
        assert_eq!(Datatype::Float64.name(), "float64");
        assert_eq!(
            Datatype::FixedString {
                length: 10,
                padding: StringPadding::NullTerminated
            }
            .name(),
            "string[10]"
        );
    }

    #[test]
    fn test_type_predicates() {
        assert!(Datatype::Int32.is_integer());
        assert!(!Datatype::Float64.is_integer());
        assert!(Datatype::Float64.is_float());
        assert!(!Datatype::Int32.is_float());
        assert!(
            Datatype::FixedString {
                length: 10,
                padding: StringPadding::NullTerminated
            }
            .is_string()
        );
    }

    #[test]
    fn test_type_converter_i32() {
        let mut data = vec![0u8; 4];
        TypeConverter::write_i32_le(&mut data, 42).expect("write failed");
        let value = TypeConverter::read_i32_le(&data).expect("read failed");
        assert_eq!(value, 42);
    }

    #[test]
    fn test_type_converter_f64() {
        let mut data = vec![0u8; 8];
        TypeConverter::write_f64_le(&mut data, std::f64::consts::PI).expect("write failed");
        let value = TypeConverter::read_f64_le(&data).expect("read failed");
        assert!((value - std::f64::consts::PI).abs() < 1e-10);
    }
}
