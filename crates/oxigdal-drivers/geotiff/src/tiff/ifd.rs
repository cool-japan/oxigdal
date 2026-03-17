//! TIFF Image File Directory (IFD) parsing
//!
//! This module handles parsing of TIFF IFDs and their entries (tags).

use oxigdal_core::error::{OxiGdalError, Result};
use oxigdal_core::io::{ByteRange, DataSource};

use super::header::{ByteOrderType, TiffVariant};
use super::tags::TiffTag;

/// TIFF field types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum FieldType {
    /// 8-bit unsigned integer
    Byte = 1,
    /// 8-bit byte (7-bit ASCII + NUL)
    Ascii = 2,
    /// 16-bit unsigned integer
    Short = 3,
    /// 32-bit unsigned integer
    Long = 4,
    /// Two LONGs: numerator, denominator
    Rational = 5,
    /// 8-bit signed integer
    SByte = 6,
    /// 8-bit undefined
    Undefined = 7,
    /// 16-bit signed integer
    SShort = 8,
    /// 32-bit signed integer
    SLong = 9,
    /// Two SLONGs
    SRational = 10,
    /// 32-bit IEEE floating point
    Float = 11,
    /// 64-bit IEEE floating point
    Double = 12,
    /// BigTIFF: 64-bit unsigned integer
    Long8 = 16,
    /// BigTIFF: 64-bit signed integer
    SLong8 = 17,
    /// BigTIFF: 64-bit IFD offset
    Ifd8 = 18,
}

impl FieldType {
    /// Creates a FieldType from a u16 value
    #[must_use]
    pub const fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::Byte),
            2 => Some(Self::Ascii),
            3 => Some(Self::Short),
            4 => Some(Self::Long),
            5 => Some(Self::Rational),
            6 => Some(Self::SByte),
            7 => Some(Self::Undefined),
            8 => Some(Self::SShort),
            9 => Some(Self::SLong),
            10 => Some(Self::SRational),
            11 => Some(Self::Float),
            12 => Some(Self::Double),
            16 => Some(Self::Long8),
            17 => Some(Self::SLong8),
            18 => Some(Self::Ifd8),
            _ => None,
        }
    }

    /// Returns the size in bytes of a single element of this type
    #[must_use]
    pub const fn element_size(self) -> usize {
        match self {
            Self::Byte | Self::Ascii | Self::SByte | Self::Undefined => 1,
            Self::Short | Self::SShort => 2,
            Self::Long | Self::SLong | Self::Float => 4,
            Self::Rational
            | Self::SRational
            | Self::Double
            | Self::Long8
            | Self::SLong8
            | Self::Ifd8 => 8,
        }
    }

    /// Returns true if this type is signed
    #[must_use]
    pub const fn is_signed(self) -> bool {
        matches!(
            self,
            Self::SByte | Self::SShort | Self::SLong | Self::SRational | Self::SLong8
        )
    }

    /// Returns true if this is a floating-point type
    #[must_use]
    pub const fn is_floating_point(self) -> bool {
        matches!(self, Self::Float | Self::Double)
    }
}

/// A single IFD entry (tag)
#[derive(Debug, Clone)]
pub struct IfdEntry {
    /// Tag identifier
    pub tag: u16,
    /// Field type
    pub field_type: FieldType,
    /// Number of values
    pub count: u64,
    /// Offset to value data (or inline value if small enough)
    pub value_offset: u64,
    /// Raw inline value bytes (for small values that fit in offset field)
    pub inline_value: Option<Vec<u8>>,
}

impl IfdEntry {
    /// Returns the total size in bytes of this entry's value
    #[must_use]
    pub fn value_size(&self) -> u64 {
        self.count * self.field_type.element_size() as u64
    }

    /// Returns true if the value fits inline in the offset field
    pub fn is_inline(&self, variant: TiffVariant) -> bool {
        let inline_capacity = match variant {
            TiffVariant::Classic => 4,
            TiffVariant::BigTiff => 8,
        };
        self.value_size() <= inline_capacity
    }

    /// Parses an IFD entry from bytes
    ///
    /// # Arguments
    /// * `data` - Entry bytes (12 for classic, 20 for BigTIFF)
    /// * `byte_order` - Byte order
    /// * `variant` - TIFF variant
    pub fn parse(data: &[u8], byte_order: ByteOrderType, variant: TiffVariant) -> Result<Self> {
        let expected_size = variant.ifd_entry_size();
        if data.len() < expected_size {
            return Err(OxiGdalError::io_error_builder("IFD entry too small")
                .with_operation("parse_ifd_entry")
                .with_parameter("actual_size", data.len().to_string())
                .with_parameter("expected_size", expected_size.to_string())
                .with_parameter("variant", format!("{:?}", variant))
                .with_suggestion("File may be truncated or corrupted. Verify TIFF file integrity")
                .build());
        }

        let tag = byte_order.read_u16(&data[0..2]);
        let field_type_raw = byte_order.read_u16(&data[2..4]);

        let field_type = FieldType::from_u16(field_type_raw).ok_or_else(|| {
            OxiGdalError::io_error_builder("Unknown TIFF field type")
                .with_operation("parse_ifd_entry")
                .with_parameter("tag", tag.to_string())
                .with_parameter("field_type_id", field_type_raw.to_string())
                .with_suggestion(
                    "File contains unsupported or invalid TIFF field type. Verify file format",
                )
                .build()
        })?;

        let (count, value_offset, inline_value) = match variant {
            TiffVariant::Classic => {
                let count = u64::from(byte_order.read_u32(&data[4..8]));
                let value_size = count * field_type.element_size() as u64;

                if value_size <= 4 {
                    // Value fits inline
                    let inline = data[8..12].to_vec();
                    (count, 0, Some(inline))
                } else {
                    let offset = u64::from(byte_order.read_u32(&data[8..12]));
                    (count, offset, None)
                }
            }
            TiffVariant::BigTiff => {
                let count = byte_order.read_u64(&data[4..12]);
                let value_size = count * field_type.element_size() as u64;

                if value_size <= 8 {
                    // Value fits inline
                    let inline = data[12..20].to_vec();
                    (count, 0, Some(inline))
                } else {
                    let offset = byte_order.read_u64(&data[12..20]);
                    (count, offset, None)
                }
            }
        };

        Ok(Self {
            tag,
            field_type,
            count,
            value_offset,
            inline_value,
        })
    }

    /// Gets the value bytes
    pub fn get_value_bytes<S: DataSource>(
        &self,
        source: &S,
        _variant: TiffVariant,
    ) -> Result<Vec<u8>> {
        if let Some(inline) = &self.inline_value {
            Ok(inline[..self.value_size() as usize].to_vec())
        } else {
            let range = ByteRange::from_offset_length(self.value_offset, self.value_size());
            source.read_range(range)
        }
    }

    /// Gets the value as a single u64
    pub fn get_u64(&self, byte_order: ByteOrderType) -> Result<u64> {
        let bytes = self.inline_value.as_ref().ok_or_else(|| {
            OxiGdalError::io_error_builder("Expected inline TIFF tag value")
                .with_operation("get_u64")
                .with_parameter("tag", self.tag.to_string())
                .with_parameter("value_offset", self.value_offset.to_string())
                .with_suggestion("Tag value is stored externally but inline value expected")
                .build()
        })?;

        let value = match self.field_type {
            FieldType::Byte | FieldType::Undefined => u64::from(bytes[0]),
            FieldType::Short => u64::from(byte_order.read_u16(bytes)),
            FieldType::Long => u64::from(byte_order.read_u32(bytes)),
            FieldType::Long8 => byte_order.read_u64(bytes),
            _ => {
                return Err(OxiGdalError::invalid_parameter_builder(
                    "field_type",
                    "Incompatible field type for u64 conversion",
                )
                .with_operation("get_u64")
                .with_parameter("tag", self.tag.to_string())
                .with_parameter("field_type", format!("{:?}", self.field_type))
                .with_suggestion("Use appropriate getter method for this field type")
                .build());
            }
        };

        Ok(value)
    }

    /// Gets the value as a single u64, reading from source if not inline
    ///
    /// Unlike [`get_u64`](Self::get_u64), this method can handle values stored
    /// at an external file offset by reading from the data source. It also
    /// supports Float and Double field types (converting via `as u64`).
    pub fn get_u64_from_source<S: DataSource>(
        &self,
        source: &S,
        byte_order: ByteOrderType,
        variant: TiffVariant,
    ) -> Result<u64> {
        let bytes = if let Some(ref inline) = self.inline_value {
            inline.clone()
        } else {
            self.get_value_bytes(source, variant)?
        };

        let value = match self.field_type {
            FieldType::Byte | FieldType::Undefined => {
                if bytes.is_empty() {
                    return Err(OxiGdalError::io_error_builder(
                        "Empty value bytes for Byte/Undefined tag",
                    )
                    .with_operation("get_u64_from_source")
                    .with_parameter("tag", self.tag.to_string())
                    .with_suggestion("Tag value data is missing or zero-length")
                    .build());
                }
                u64::from(bytes[0])
            }
            FieldType::Short => {
                if bytes.len() < 2 {
                    return Err(OxiGdalError::io_error_builder(
                        "Insufficient bytes for Short tag value",
                    )
                    .with_operation("get_u64_from_source")
                    .with_parameter("tag", self.tag.to_string())
                    .with_parameter("bytes_available", bytes.len().to_string())
                    .with_parameter("bytes_needed", "2")
                    .with_suggestion("Tag value data is truncated or corrupted")
                    .build());
                }
                u64::from(byte_order.read_u16(&bytes))
            }
            FieldType::Long => {
                if bytes.len() < 4 {
                    return Err(OxiGdalError::io_error_builder(
                        "Insufficient bytes for Long tag value",
                    )
                    .with_operation("get_u64_from_source")
                    .with_parameter("tag", self.tag.to_string())
                    .with_parameter("bytes_available", bytes.len().to_string())
                    .with_parameter("bytes_needed", "4")
                    .with_suggestion("Tag value data is truncated or corrupted")
                    .build());
                }
                u64::from(byte_order.read_u32(&bytes))
            }
            FieldType::Long8 => {
                if bytes.len() < 8 {
                    return Err(OxiGdalError::io_error_builder(
                        "Insufficient bytes for Long8 tag value",
                    )
                    .with_operation("get_u64_from_source")
                    .with_parameter("tag", self.tag.to_string())
                    .with_parameter("bytes_available", bytes.len().to_string())
                    .with_parameter("bytes_needed", "8")
                    .with_suggestion("Tag value data is truncated or corrupted")
                    .build());
                }
                byte_order.read_u64(&bytes)
            }
            FieldType::Float => {
                if bytes.len() < 4 {
                    return Err(OxiGdalError::io_error_builder(
                        "Insufficient bytes for Float tag value",
                    )
                    .with_operation("get_u64_from_source")
                    .with_parameter("tag", self.tag.to_string())
                    .with_parameter("bytes_available", bytes.len().to_string())
                    .with_parameter("bytes_needed", "4")
                    .with_suggestion("Tag value data is truncated or corrupted")
                    .build());
                }
                byte_order.read_f32(&bytes) as u64
            }
            FieldType::Double => {
                if bytes.len() < 8 {
                    return Err(OxiGdalError::io_error_builder(
                        "Insufficient bytes for Double tag value",
                    )
                    .with_operation("get_u64_from_source")
                    .with_parameter("tag", self.tag.to_string())
                    .with_parameter("bytes_available", bytes.len().to_string())
                    .with_parameter("bytes_needed", "8")
                    .with_suggestion("Tag value data is truncated or corrupted")
                    .build());
                }
                byte_order.read_f64(&bytes) as u64
            }
            _ => {
                return Err(OxiGdalError::invalid_parameter_builder(
                    "field_type",
                    "Incompatible field type for u64 conversion",
                )
                .with_operation("get_u64_from_source")
                .with_parameter("tag", self.tag.to_string())
                .with_parameter("field_type", format!("{:?}", self.field_type))
                .with_suggestion("Use appropriate getter method for this field type")
                .build());
            }
        };

        Ok(value)
    }

    /// Gets the value as a `Vec<u64>`
    pub fn get_u64_vec<S: DataSource>(
        &self,
        source: &S,
        byte_order: ByteOrderType,
        variant: TiffVariant,
    ) -> Result<Vec<u64>> {
        let bytes = self.get_value_bytes(source, variant)?;
        let elem_size = self.field_type.element_size();
        let mut values = Vec::with_capacity(self.count as usize);

        for chunk in bytes.chunks_exact(elem_size) {
            let value = match self.field_type {
                FieldType::Byte | FieldType::Undefined => u64::from(chunk[0]),
                FieldType::Short => u64::from(byte_order.read_u16(chunk)),
                FieldType::Long => u64::from(byte_order.read_u32(chunk)),
                FieldType::Long8 => byte_order.read_u64(chunk),
                _ => {
                    return Err(OxiGdalError::invalid_parameter_builder(
                        "field_type",
                        "Incompatible field type for u64 vector conversion",
                    )
                    .with_operation("get_u64_vec")
                    .with_parameter("tag", self.tag.to_string())
                    .with_parameter("field_type", format!("{:?}", self.field_type))
                    .with_parameter("count", self.count.to_string())
                    .with_suggestion("Use appropriate getter method for this field type")
                    .build());
                }
            };
            values.push(value);
        }

        Ok(values)
    }

    /// Gets the value as a `Vec<f64>`
    pub fn get_f64_vec<S: DataSource>(
        &self,
        source: &S,
        byte_order: ByteOrderType,
        variant: TiffVariant,
    ) -> Result<Vec<f64>> {
        let bytes = self.get_value_bytes(source, variant)?;
        let elem_size = self.field_type.element_size();
        let mut values = Vec::with_capacity(self.count as usize);

        for chunk in bytes.chunks_exact(elem_size) {
            let value = match self.field_type {
                FieldType::Byte => f64::from(chunk[0]),
                FieldType::SByte => f64::from(chunk[0] as i8),
                FieldType::Short => f64::from(byte_order.read_u16(chunk)),
                FieldType::SShort => f64::from(byte_order.read_i16(chunk)),
                FieldType::Long => f64::from(byte_order.read_u32(chunk)),
                FieldType::SLong => f64::from(byte_order.read_i32(chunk)),
                FieldType::Float => f64::from(byte_order.read_f32(chunk)),
                FieldType::Double => byte_order.read_f64(chunk),
                FieldType::Rational => {
                    let num = byte_order.read_u32(&chunk[0..4]);
                    let den = byte_order.read_u32(&chunk[4..8]);
                    if den == 0 {
                        f64::NAN
                    } else {
                        f64::from(num) / f64::from(den)
                    }
                }
                FieldType::SRational => {
                    let num = byte_order.read_i32(&chunk[0..4]);
                    let den = byte_order.read_i32(&chunk[4..8]);
                    if den == 0 {
                        f64::NAN
                    } else {
                        f64::from(num) / f64::from(den)
                    }
                }
                _ => {
                    return Err(OxiGdalError::invalid_parameter_builder(
                        "field_type",
                        "Incompatible field type for f64 vector conversion",
                    )
                    .with_operation("get_f64_vec")
                    .with_parameter("tag", self.tag.to_string())
                    .with_parameter("field_type", format!("{:?}", self.field_type))
                    .with_parameter("count", self.count.to_string())
                    .with_suggestion("Use appropriate getter method for this field type")
                    .build());
                }
            };
            values.push(value);
        }

        Ok(values)
    }

    /// Gets the value as an ASCII string
    pub fn get_ascii<S: DataSource>(&self, source: &S, variant: TiffVariant) -> Result<String> {
        if self.field_type != FieldType::Ascii {
            return Err(OxiGdalError::invalid_parameter_builder(
                "field_type",
                "Expected ASCII field type",
            )
            .with_operation("get_ascii")
            .with_parameter("tag", self.tag.to_string())
            .with_parameter("actual_type", format!("{:?}", self.field_type))
            .with_parameter("expected_type", "Ascii")
            .with_suggestion("Use correct getter method for the field type")
            .build());
        }

        let bytes = self.get_value_bytes(source, variant)?;

        // Remove trailing NUL bytes
        let trimmed = bytes
            .iter()
            .position(|&b| b == 0)
            .map_or(&bytes[..], |pos| &bytes[..pos]);

        String::from_utf8(trimmed.to_vec()).map_err(|e| {
            OxiGdalError::io_error_builder("Invalid ASCII string in TIFF tag")
                .with_operation("get_ascii")
                .with_parameter("tag", self.tag.to_string())
                .with_parameter("error", e.to_string())
                .with_suggestion("Tag contains invalid UTF-8 data. File may be corrupted")
                .build()
        })
    }
}

/// An Image File Directory
#[derive(Debug, Clone)]
pub struct Ifd {
    /// Entries in this IFD
    pub entries: Vec<IfdEntry>,
    /// Offset to next IFD (0 if none)
    pub next_ifd_offset: u64,
}

impl Ifd {
    /// Parses an IFD from a data source
    ///
    /// # Arguments
    /// * `source` - Data source
    /// * `offset` - Offset to IFD
    /// * `byte_order` - Byte order
    /// * `variant` - TIFF variant
    pub fn parse<S: DataSource>(
        source: &S,
        offset: u64,
        byte_order: ByteOrderType,
        variant: TiffVariant,
    ) -> Result<Self> {
        // Read entry count
        let count_size = match variant {
            TiffVariant::Classic => 2,
            TiffVariant::BigTiff => 8,
        };

        let count_bytes =
            source.read_range(ByteRange::from_offset_length(offset, count_size as u64))?;

        let entry_count = match variant {
            TiffVariant::Classic => u64::from(byte_order.read_u16(&count_bytes)),
            TiffVariant::BigTiff => byte_order.read_u64(&count_bytes),
        };

        if entry_count > 65535 {
            return Err(OxiGdalError::io_error_builder("Too many IFD entries")
                .with_operation("parse_ifd")
                .with_parameter("entry_count", entry_count.to_string())
                .with_parameter("max_entries", "65535")
                .with_parameter("ifd_offset", offset.to_string())
                .with_suggestion(
                    "File may be corrupted or maliciously crafted. Verify TIFF file integrity",
                )
                .build());
        }

        let entry_size = variant.ifd_entry_size();
        let entries_offset = offset + count_size as u64;
        let entries_size = entry_count * entry_size as u64;

        // Read all entries
        let entries_bytes =
            source.read_range(ByteRange::from_offset_length(entries_offset, entries_size))?;

        let mut entries = Vec::with_capacity(entry_count as usize);
        for i in 0..entry_count as usize {
            let start = i * entry_size;
            let end = start + entry_size;
            let entry = IfdEntry::parse(&entries_bytes[start..end], byte_order, variant)?;
            entries.push(entry);
        }

        // Read next IFD offset
        let next_offset_pos = entries_offset + entries_size;
        let next_offset_size = variant.offset_size();
        let next_offset_bytes = source.read_range(ByteRange::from_offset_length(
            next_offset_pos,
            next_offset_size as u64,
        ))?;

        let next_ifd_offset = match variant {
            TiffVariant::Classic => u64::from(byte_order.read_u32(&next_offset_bytes)),
            TiffVariant::BigTiff => byte_order.read_u64(&next_offset_bytes),
        };

        Ok(Self {
            entries,
            next_ifd_offset,
        })
    }

    /// Finds an entry by tag
    #[must_use]
    pub fn get_entry(&self, tag: TiffTag) -> Option<&IfdEntry> {
        self.entries.iter().find(|e| e.tag == tag as u16)
    }

    /// Finds an entry by raw tag value
    #[must_use]
    pub fn get_entry_raw(&self, tag: u16) -> Option<&IfdEntry> {
        self.entries.iter().find(|e| e.tag == tag)
    }

    /// Returns true if this IFD has another IFD following it
    #[must_use]
    pub const fn has_next(&self) -> bool {
        self.next_ifd_offset != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A simple in-memory DataSource for testing
    struct TestSource {
        data: Vec<u8>,
    }

    impl TestSource {
        fn new(data: Vec<u8>) -> Self {
            Self { data }
        }
    }

    impl DataSource for TestSource {
        fn size(&self) -> Result<u64> {
            Ok(self.data.len() as u64)
        }

        fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
            let start = range.start as usize;
            let end = range.end as usize;
            if end > self.data.len() {
                return Err(
                    OxiGdalError::io_error_builder("Read past end of TestSource")
                        .with_operation("read_range")
                        .with_parameter("range_end", end.to_string())
                        .with_parameter("data_len", self.data.len().to_string())
                        .build(),
                );
            }
            Ok(self.data[start..end].to_vec())
        }
    }

    #[test]
    fn test_field_type_sizes() {
        assert_eq!(FieldType::Byte.element_size(), 1);
        assert_eq!(FieldType::Short.element_size(), 2);
        assert_eq!(FieldType::Long.element_size(), 4);
        assert_eq!(FieldType::Double.element_size(), 8);
        assert_eq!(FieldType::Rational.element_size(), 8);
    }

    #[test]
    fn test_field_type_properties() {
        assert!(!FieldType::Byte.is_signed());
        assert!(FieldType::SByte.is_signed());
        assert!(!FieldType::Float.is_signed());
        assert!(FieldType::Float.is_floating_point());
        assert!(FieldType::Double.is_floating_point());
    }

    #[test]
    fn test_parse_ifd_entry_classic() {
        // ImageWidth tag (256), LONG (4), count=1, value=1024
        let data = [
            0x00, 0x01, // Tag: 256 (ImageWidth)
            0x04, 0x00, // Type: LONG (4)
            0x01, 0x00, 0x00, 0x00, // Count: 1
            0x00, 0x04, 0x00, 0x00, // Value: 1024 (inline)
        ];

        let entry = IfdEntry::parse(&data, ByteOrderType::LittleEndian, TiffVariant::Classic)
            .expect("should parse");

        assert_eq!(entry.tag, 256);
        assert_eq!(entry.field_type, FieldType::Long);
        assert_eq!(entry.count, 1);
        assert!(entry.inline_value.is_some());

        let value = entry
            .get_u64(ByteOrderType::LittleEndian)
            .expect("should get value");
        assert_eq!(value, 1024);
    }

    #[test]
    fn test_get_u64_from_source_inline() {
        // When inline_value is Some, get_u64_from_source should behave like get_u64
        let bo = ByteOrderType::LittleEndian;

        // Build a Long inline entry with value 42
        let mut inline_bytes = vec![0u8; 4];
        bo.write_u32(&mut inline_bytes, 42);

        let entry = IfdEntry {
            tag: 256,
            field_type: FieldType::Long,
            count: 1,
            value_offset: 0,
            inline_value: Some(inline_bytes),
        };

        // Source data is irrelevant for inline values, but we still need one
        let source = TestSource::new(vec![0u8; 16]);

        let from_source = entry
            .get_u64_from_source(&source, bo, TiffVariant::Classic)
            .expect("inline Long should convert to u64 via get_u64_from_source");

        let from_inline = entry
            .get_u64(bo)
            .expect("inline Long should convert to u64 via get_u64");

        assert_eq!(from_source, 42);
        assert_eq!(from_source, from_inline);
    }

    #[test]
    fn test_get_u64_from_source_external_long() {
        let bo = ByteOrderType::LittleEndian;

        // Build a 64-byte source buffer; store a Long value 0x0000_BEEF at offset 32
        let mut data = vec![0u8; 64];
        bo.write_u32(&mut data[32..36], 0x0000_BEEF);

        let source = TestSource::new(data);

        let entry = IfdEntry {
            tag: 256,
            field_type: FieldType::Long,
            count: 1,
            value_offset: 32,
            inline_value: None, // external
        };

        let value = entry
            .get_u64_from_source(&source, bo, TiffVariant::Classic)
            .expect("external Long should be read from source");

        assert_eq!(value, 0x0000_BEEF);
    }

    #[test]
    fn test_get_u64_from_source_external_double() {
        let bo = ByteOrderType::LittleEndian;

        // Store a Double (f64) value 12345.0 at offset 16
        let mut data = vec![0u8; 32];
        bo.write_f64(&mut data[16..24], 12345.0);

        let source = TestSource::new(data);

        let entry = IfdEntry {
            tag: 300,
            field_type: FieldType::Double,
            count: 1,
            value_offset: 16,
            inline_value: None, // external
        };

        let value = entry
            .get_u64_from_source(&source, bo, TiffVariant::Classic)
            .expect("external Double should be read from source and cast to u64");

        // f64 12345.0 as u64 truncates to 12345
        assert_eq!(value, 12345);
    }

    #[test]
    fn test_get_u64_from_source_external_short() {
        let bo = ByteOrderType::LittleEndian;

        // Store a Short (u16) value 999 at offset 8
        let mut data = vec![0u8; 16];
        bo.write_u16(&mut data[8..10], 999);

        let source = TestSource::new(data);

        let entry = IfdEntry {
            tag: 258,
            field_type: FieldType::Short,
            count: 1,
            value_offset: 8,
            inline_value: None, // external
        };

        let value = entry
            .get_u64_from_source(&source, bo, TiffVariant::Classic)
            .expect("external Short should be read from source");

        assert_eq!(value, 999);
    }
}
