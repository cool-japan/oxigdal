//! DBF record and field type definitions
//!
//! This module handles DBF (dBase) field types and record structures for
//! attribute data in Shapefiles.

use crate::error::{Result, ShapefileError};
use std::io::{Read, Write};

/// DBF field types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    /// Character string (ASCII)
    Character,
    /// Numeric (integer or float)
    Number,
    /// Logical (boolean)
    Logical,
    /// Date (YYYYMMDD)
    Date,
    /// Float (like Number but explicitly floating point)
    Float,
    /// Memo (reference to memo file - not commonly used in Shapefiles)
    Memo,
}

impl FieldType {
    /// Converts a field type code to a `FieldType`
    pub fn from_code(code: u8) -> Result<Self> {
        match code {
            b'C' => Ok(Self::Character),
            b'N' => Ok(Self::Number),
            b'L' => Ok(Self::Logical),
            b'D' => Ok(Self::Date),
            b'F' => Ok(Self::Float),
            b'M' => Ok(Self::Memo),
            _ => Err(ShapefileError::DbfError {
                message: format!("invalid field type code: {}", code as char),
                field: None,
                record: None,
            }),
        }
    }

    /// Converts a `FieldType` to its code
    pub fn to_code(self) -> u8 {
        match self {
            Self::Character => b'C',
            Self::Number => b'N',
            Self::Logical => b'L',
            Self::Date => b'D',
            Self::Float => b'F',
            Self::Memo => b'M',
        }
    }

    /// Returns the name of the field type
    pub fn name(self) -> &'static str {
        match self {
            Self::Character => "Character",
            Self::Number => "Number",
            Self::Logical => "Logical",
            Self::Date => "Date",
            Self::Float => "Float",
            Self::Memo => "Memo",
        }
    }
}

/// DBF field descriptor (32 bytes)
#[derive(Debug, Clone)]
pub struct FieldDescriptor {
    /// Field name (up to 10 characters, null-terminated)
    pub name: String,
    /// Field type
    pub field_type: FieldType,
    /// Field length in bytes
    pub length: u8,
    /// Decimal count (for numeric fields)
    pub decimal_count: u8,
}

impl FieldDescriptor {
    /// Creates a new field descriptor
    pub fn new(name: String, field_type: FieldType, length: u8, decimal_count: u8) -> Result<Self> {
        if name.len() > 10 {
            return Err(ShapefileError::InvalidFieldDescriptor {
                message: format!("field name too long: {} (max 10)", name.len()),
                field: Some(name),
            });
        }

        if name.is_empty() {
            return Err(ShapefileError::InvalidFieldDescriptor {
                message: "field name cannot be empty".to_string(),
                field: None,
            });
        }

        Ok(Self {
            name,
            field_type,
            length,
            decimal_count,
        })
    }

    /// Reads a field descriptor from a reader
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        // Read field name (11 bytes)
        let mut name_bytes = [0u8; 11];
        reader
            .read_exact(&mut name_bytes)
            .map_err(|_| ShapefileError::unexpected_eof("reading field name"))?;

        // Convert to string, stopping at first null byte
        let name_end = name_bytes.iter().position(|&b| b == 0).unwrap_or(11);
        let name = String::from_utf8_lossy(&name_bytes[..name_end]).to_string();

        // Read field type (1 byte)
        let mut type_byte = [0u8; 1];
        reader
            .read_exact(&mut type_byte)
            .map_err(|_| ShapefileError::unexpected_eof("reading field type"))?;
        let field_type = FieldType::from_code(type_byte[0])?;

        // Skip reserved bytes (4 bytes)
        let mut reserved = [0u8; 4];
        reader
            .read_exact(&mut reserved)
            .map_err(|_| ShapefileError::unexpected_eof("reading field reserved bytes"))?;

        // Read field length (1 byte)
        let mut length = [0u8; 1];
        reader
            .read_exact(&mut length)
            .map_err(|_| ShapefileError::unexpected_eof("reading field length"))?;

        // Read decimal count (1 byte)
        let mut decimal_count = [0u8; 1];
        reader
            .read_exact(&mut decimal_count)
            .map_err(|_| ShapefileError::unexpected_eof("reading decimal count"))?;

        // Skip remaining reserved bytes (14 bytes)
        let mut reserved2 = [0u8; 14];
        reader
            .read_exact(&mut reserved2)
            .map_err(|_| ShapefileError::unexpected_eof("reading field reserved bytes 2"))?;

        Ok(Self {
            name,
            field_type,
            length: length[0],
            decimal_count: decimal_count[0],
        })
    }

    /// Writes a field descriptor to a writer
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        // Write field name (11 bytes, null-padded)
        let mut name_bytes = [0u8; 11];
        let name_slice = self.name.as_bytes();
        let copy_len = name_slice.len().min(10);
        name_bytes[..copy_len].copy_from_slice(&name_slice[..copy_len]);
        writer.write_all(&name_bytes).map_err(ShapefileError::Io)?;

        // Write field type (1 byte)
        writer
            .write_all(&[self.field_type.to_code()])
            .map_err(ShapefileError::Io)?;

        // Write reserved bytes (4 bytes)
        writer.write_all(&[0u8; 4]).map_err(ShapefileError::Io)?;

        // Write field length (1 byte)
        writer
            .write_all(&[self.length])
            .map_err(ShapefileError::Io)?;

        // Write decimal count (1 byte)
        writer
            .write_all(&[self.decimal_count])
            .map_err(ShapefileError::Io)?;

        // Write remaining reserved bytes (14 bytes)
        writer.write_all(&[0u8; 14]).map_err(ShapefileError::Io)?;

        Ok(())
    }
}

/// DBF field value
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    /// String value
    String(String),
    /// Integer value
    Integer(i64),
    /// Float value
    Float(f64),
    /// Boolean value
    Boolean(bool),
    /// Date value (YYYYMMDD string)
    Date(String),
    /// Null value
    Null,
}

impl FieldValue {
    /// Parses a field value from raw bytes
    pub fn parse(bytes: &[u8], field_type: FieldType, decimal_count: u8) -> Result<Self> {
        // Trim whitespace
        let trimmed = String::from_utf8_lossy(bytes).trim().to_string();

        if trimmed.is_empty() {
            return Ok(Self::Null);
        }

        match field_type {
            FieldType::Character => Ok(Self::String(trimmed)),
            FieldType::Number | FieldType::Float => {
                if decimal_count > 0 {
                    // Parse as float
                    trimmed
                        .parse::<f64>()
                        .map(Self::Float)
                        .map_err(|_| ShapefileError::DbfError {
                            message: format!("failed to parse float: {}", trimmed),
                            field: None,
                            record: None,
                        })
                } else {
                    // Parse as integer
                    trimmed.parse::<i64>().map(Self::Integer).map_err(|_| {
                        ShapefileError::DbfError {
                            message: format!("failed to parse integer: {}", trimmed),
                            field: None,
                            record: None,
                        }
                    })
                }
            }
            FieldType::Logical => {
                let value = match trimmed.chars().next() {
                    Some('T') | Some('t') | Some('Y') | Some('y') => true,
                    Some('F') | Some('f') | Some('N') | Some('n') => false,
                    _ => {
                        return Err(ShapefileError::DbfError {
                            message: format!("invalid logical value: {}", trimmed),
                            field: None,
                            record: None,
                        });
                    }
                };
                Ok(Self::Boolean(value))
            }
            FieldType::Date => {
                // Validate date format (YYYYMMDD)
                if trimmed.len() != 8 {
                    return Err(ShapefileError::DbfError {
                        message: format!("invalid date format: {} (expected YYYYMMDD)", trimmed),
                        field: None,
                        record: None,
                    });
                }
                Ok(Self::Date(trimmed))
            }
            FieldType::Memo => Ok(Self::String(trimmed)),
        }
    }

    /// Formats a field value to bytes for writing
    pub fn format(&self, length: usize) -> Vec<u8> {
        let mut buffer = vec![b' '; length];

        let content = match self {
            Self::String(s) => s.clone(),
            Self::Integer(i) => i.to_string(),
            Self::Float(f) => f.to_string(),
            Self::Boolean(b) => {
                if *b {
                    "T".to_string()
                } else {
                    "F".to_string()
                }
            }
            Self::Date(d) => d.clone(),
            Self::Null => String::new(),
        };

        let bytes = content.as_bytes();
        let copy_len = bytes.len().min(length);
        buffer[..copy_len].copy_from_slice(&bytes[..copy_len]);

        buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_field_type_conversion() {
        assert_eq!(
            FieldType::from_code(b'C').expect("valid field type code"),
            FieldType::Character
        );
        assert_eq!(FieldType::Character.to_code(), b'C');
        assert_eq!(FieldType::Number.name(), "Number");
    }

    #[test]
    fn test_field_descriptor_round_trip() {
        let desc = FieldDescriptor::new("NAME".to_string(), FieldType::Character, 50, 0)
            .expect("valid field descriptor");

        let mut buffer = Vec::new();
        desc.write(&mut buffer).expect("write field descriptor");

        assert_eq!(buffer.len(), 32); // Field descriptor is 32 bytes

        let mut cursor = Cursor::new(buffer);
        let read_desc = FieldDescriptor::read(&mut cursor).expect("read field descriptor");

        assert_eq!(read_desc.name, "NAME");
        assert_eq!(read_desc.field_type, FieldType::Character);
        assert_eq!(read_desc.length, 50);
    }

    #[test]
    fn test_field_value_parsing() {
        // String
        let value =
            FieldValue::parse(b"  test  ", FieldType::Character, 0).expect("parse character field");
        assert_eq!(value, FieldValue::String("test".to_string()));

        // Integer
        let value =
            FieldValue::parse(b"  123  ", FieldType::Number, 0).expect("parse integer field");
        assert_eq!(value, FieldValue::Integer(123));

        // Float
        let value = FieldValue::parse(b" 12.34 ", FieldType::Number, 2).expect("parse float field");
        assert_eq!(value, FieldValue::Float(12.34));

        // Boolean
        let value =
            FieldValue::parse(b"T", FieldType::Logical, 0).expect("parse logical field true");
        assert_eq!(value, FieldValue::Boolean(true));

        let value =
            FieldValue::parse(b"F", FieldType::Logical, 0).expect("parse logical field false");
        assert_eq!(value, FieldValue::Boolean(false));

        // Date
        let value = FieldValue::parse(b"20240125", FieldType::Date, 0).expect("parse date field");
        assert_eq!(value, FieldValue::Date("20240125".to_string()));

        // Null (empty)
        let value = FieldValue::parse(b"   ", FieldType::Character, 0).expect("parse empty field");
        assert_eq!(value, FieldValue::Null);
    }

    #[test]
    fn test_field_value_formatting() {
        let value = FieldValue::String("test".to_string());
        let formatted = value.format(10);
        assert_eq!(formatted.len(), 10);
        assert_eq!(&formatted[..4], b"test");

        let value = FieldValue::Integer(123);
        let formatted = value.format(10);
        assert_eq!(&formatted[..3], b"123");

        let value = FieldValue::Boolean(true);
        let formatted = value.format(1);
        assert_eq!(formatted[0], b'T');
    }
}
