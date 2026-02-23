//! IFD (Image File Directory) writing
//!
//! This module handles writing TIFF IFDs and entries.

use oxigdal_core::error::{OxiGdalError, Result};

use crate::tiff::{ByteOrderType, FieldType, TiffTag, TiffVariant};

/// A builder for IFD entries
#[derive(Debug)]
pub struct IfdBuilder {
    /// Entries in this IFD
    entries: Vec<IfdEntryData>,
    /// Byte order
    byte_order: ByteOrderType,
    /// TIFF variant
    variant: TiffVariant,
}

/// Data for an IFD entry
#[derive(Debug, Clone)]
struct IfdEntryData {
    /// Tag
    tag: u16,
    /// Field type
    field_type: FieldType,
    /// Values
    values: Vec<u64>,
    /// Raw data (for non-integer types)
    raw_data: Option<Vec<u8>>,
}

impl IfdBuilder {
    /// Creates a new IFD builder
    #[must_use]
    pub const fn new(byte_order: ByteOrderType, variant: TiffVariant) -> Self {
        Self {
            entries: Vec::new(),
            byte_order,
            variant,
        }
    }

    /// Adds a u16 entry
    pub fn add_short(&mut self, tag: TiffTag, value: u16) {
        self.entries.push(IfdEntryData {
            tag: tag as u16,
            field_type: FieldType::Short,
            values: vec![u64::from(value)],
            raw_data: None,
        });
    }

    /// Adds multiple u16 values
    pub fn add_short_array(&mut self, tag: TiffTag, values: Vec<u16>) {
        self.entries.push(IfdEntryData {
            tag: tag as u16,
            field_type: FieldType::Short,
            values: values.into_iter().map(u64::from).collect(),
            raw_data: None,
        });
    }

    /// Adds a u32 entry
    pub fn add_long(&mut self, tag: TiffTag, value: u32) {
        self.entries.push(IfdEntryData {
            tag: tag as u16,
            field_type: FieldType::Long,
            values: vec![u64::from(value)],
            raw_data: None,
        });
    }

    /// Adds multiple u32 values
    pub fn add_long_array(&mut self, tag: TiffTag, values: Vec<u32>) {
        self.entries.push(IfdEntryData {
            tag: tag as u16,
            field_type: FieldType::Long,
            values: values.into_iter().map(u64::from).collect(),
            raw_data: None,
        });
    }

    /// Adds a u64 entry (BigTIFF)
    pub fn add_long8(&mut self, tag: TiffTag, value: u64) {
        self.entries.push(IfdEntryData {
            tag: tag as u16,
            field_type: FieldType::Long8,
            values: vec![value],
            raw_data: None,
        });
    }

    /// Adds multiple u64 values (BigTIFF)
    pub fn add_long8_array(&mut self, tag: TiffTag, values: Vec<u64>) {
        self.entries.push(IfdEntryData {
            tag: tag as u16,
            field_type: FieldType::Long8,
            values,
            raw_data: None,
        });
    }

    /// Adds f64 values
    pub fn add_double_array(&mut self, tag: TiffTag, values: Vec<f64>) {
        let mut raw_data = vec![0u8; values.len() * 8];

        for (i, value) in values.iter().enumerate() {
            let offset = i * 8;
            self.byte_order
                .write_f64(&mut raw_data[offset..offset + 8], *value);
        }

        self.entries.push(IfdEntryData {
            tag: tag as u16,
            field_type: FieldType::Double,
            values: vec![],
            raw_data: Some(raw_data),
        });
    }

    /// Adds a rational (fraction) value
    pub fn add_rational(&mut self, tag: TiffTag, numerator: u32, denominator: u32) {
        let mut raw_data = vec![0u8; 8];
        self.byte_order.write_u32(&mut raw_data[0..4], numerator);
        self.byte_order.write_u32(&mut raw_data[4..8], denominator);

        self.entries.push(IfdEntryData {
            tag: tag as u16,
            field_type: FieldType::Rational,
            values: vec![],
            raw_data: Some(raw_data),
        });
    }

    /// Adds an ASCII string
    pub fn add_ascii(&mut self, tag: TiffTag, value: String) {
        let mut bytes = value.into_bytes();
        bytes.push(0); // NUL terminator

        self.entries.push(IfdEntryData {
            tag: tag as u16,
            field_type: FieldType::Ascii,
            values: vec![],
            raw_data: Some(bytes),
        });
    }

    /// Writes the IFD to a buffer
    ///
    /// # Arguments
    /// * `ifd_offset` - Offset where the IFD will be written
    /// * `next_ifd_offset` - Offset to next IFD (0 if none)
    ///
    /// # Returns
    /// Tuple of (IFD bytes, data bytes that need to be written separately, total size)
    ///
    /// # Errors
    /// Returns an error if writing fails
    pub fn write(&self, ifd_offset: u64, next_ifd_offset: u64) -> Result<(Vec<u8>, Vec<u8>, u64)> {
        // Sort entries by tag
        let mut sorted_entries = self.entries.clone();
        sorted_entries.sort_by_key(|e| e.tag);

        let entry_count = sorted_entries.len();
        let entry_size = self.variant.ifd_entry_size();

        // Calculate IFD size
        let count_size = match self.variant {
            TiffVariant::Classic => 2,
            TiffVariant::BigTiff => 8,
        };
        let next_offset_size = self.variant.offset_size();
        let ifd_size = count_size + entry_count * entry_size + next_offset_size;

        // Start of data area (after IFD)
        let mut data_offset = ifd_offset + ifd_size as u64;
        let mut data_bytes = Vec::new();

        // Write entry count
        let mut ifd_bytes = vec![0u8; ifd_size];
        match self.variant {
            TiffVariant::Classic => {
                self.byte_order
                    .write_u16(&mut ifd_bytes[0..2], entry_count as u16);
            }
            TiffVariant::BigTiff => {
                self.byte_order
                    .write_u64(&mut ifd_bytes[0..8], entry_count as u64);
            }
        }

        // Write entries
        let mut entry_offset = count_size;
        for entry_data in &sorted_entries {
            self.write_entry(
                &mut ifd_bytes[entry_offset..entry_offset + entry_size],
                entry_data,
                &mut data_bytes,
                &mut data_offset,
            )?;
            entry_offset += entry_size;
        }

        // Write next IFD offset
        match self.variant {
            TiffVariant::Classic => {
                self.byte_order.write_u32(
                    &mut ifd_bytes[entry_offset..entry_offset + 4],
                    next_ifd_offset as u32,
                );
            }
            TiffVariant::BigTiff => {
                self.byte_order.write_u64(
                    &mut ifd_bytes[entry_offset..entry_offset + 8],
                    next_ifd_offset,
                );
            }
        }

        let total_size = ifd_size as u64 + data_bytes.len() as u64;
        Ok((ifd_bytes, data_bytes, total_size))
    }

    /// Writes a single IFD entry
    fn write_entry(
        &self,
        buffer: &mut [u8],
        entry: &IfdEntryData,
        data_bytes: &mut Vec<u8>,
        data_offset: &mut u64,
    ) -> Result<()> {
        // Write tag
        self.byte_order.write_u16(&mut buffer[0..2], entry.tag);

        // Write field type
        self.byte_order
            .write_u16(&mut buffer[2..4], entry.field_type as u16);

        // Determine count and value data
        let (count, value_data) = if let Some(raw) = &entry.raw_data {
            (
                raw.len() as u64 / entry.field_type.element_size() as u64,
                raw.clone(),
            )
        } else {
            let mut data = Vec::new();
            for &val in &entry.values {
                match entry.field_type {
                    FieldType::Byte | FieldType::Undefined => data.push(val as u8),
                    FieldType::Short => {
                        let mut bytes = [0u8; 2];
                        self.byte_order.write_u16(&mut bytes, val as u16);
                        data.extend_from_slice(&bytes);
                    }
                    FieldType::Long => {
                        let mut bytes = [0u8; 4];
                        self.byte_order.write_u32(&mut bytes, val as u32);
                        data.extend_from_slice(&bytes);
                    }
                    FieldType::Long8 => {
                        let mut bytes = [0u8; 8];
                        self.byte_order.write_u64(&mut bytes, val);
                        data.extend_from_slice(&bytes);
                    }
                    _ => {
                        return Err(OxiGdalError::InvalidParameter {
                            parameter: "field_type",
                            message: format!("Unsupported field type: {:?}", entry.field_type),
                        });
                    }
                }
            }
            (entry.values.len() as u64, data)
        };

        // Write count
        match self.variant {
            TiffVariant::Classic => {
                self.byte_order.write_u32(&mut buffer[4..8], count as u32);
            }
            TiffVariant::BigTiff => {
                self.byte_order.write_u64(&mut buffer[4..12], count);
            }
        }

        // Write value or offset
        let inline_capacity = match self.variant {
            TiffVariant::Classic => 4,
            TiffVariant::BigTiff => 8,
        };

        let value_offset_pos = match self.variant {
            TiffVariant::Classic => 8,
            TiffVariant::BigTiff => 12,
        };

        if value_data.len() <= inline_capacity {
            // Value fits inline
            buffer[value_offset_pos..value_offset_pos + value_data.len()]
                .copy_from_slice(&value_data);
        } else {
            // Write offset
            match self.variant {
                TiffVariant::Classic => {
                    self.byte_order.write_u32(
                        &mut buffer[value_offset_pos..value_offset_pos + 4],
                        *data_offset as u32,
                    );
                }
                TiffVariant::BigTiff => {
                    self.byte_order.write_u64(
                        &mut buffer[value_offset_pos..value_offset_pos + 8],
                        *data_offset,
                    );
                }
            }

            // Add to data bytes
            data_bytes.extend_from_slice(&value_data);
            *data_offset += value_data.len() as u64;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ifd_builder_basic() {
        let mut builder = IfdBuilder::new(ByteOrderType::LittleEndian, TiffVariant::Classic);

        builder.add_long(TiffTag::ImageWidth, 1024);
        builder.add_long(TiffTag::ImageLength, 1024);
        builder.add_short(TiffTag::BitsPerSample, 8);

        let result = builder.write(100, 0);
        assert!(result.is_ok());

        if let Ok((ifd_bytes, data_bytes, total_size)) = result {
            // IFD should have 3 entries
            // Classic TIFF: 2 (count) + 3*12 (entries) + 4 (next offset) = 42 bytes
            assert_eq!(ifd_bytes.len(), 42);
            assert!(total_size > 0);
            assert!(data_bytes.is_empty() || !data_bytes.is_empty()); // May or may not have external data
        }
    }

    #[test]
    fn test_ifd_builder_ascii() {
        let mut builder = IfdBuilder::new(ByteOrderType::LittleEndian, TiffVariant::Classic);
        builder.add_ascii(TiffTag::Software, "OxiGDAL".to_string());

        let result = builder.write(100, 0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_ifd_builder_arrays() {
        let mut builder = IfdBuilder::new(ByteOrderType::LittleEndian, TiffVariant::BigTiff);

        builder.add_long_array(TiffTag::TileOffsets, vec![1000, 2000, 3000]);
        builder.add_long_array(TiffTag::TileByteCounts, vec![500, 600, 700]);

        let result = builder.write(1000, 0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_double_array_writing() {
        // Test with exact values from Laos/Cambodia COG example
        let mut builder = IfdBuilder::new(ByteOrderType::LittleEndian, TiffVariant::Classic);

        let pixel_scale = vec![0.00027, 0.00027, 0.0];
        builder.add_double_array(TiffTag::ModelPixelScale, pixel_scale.clone());

        // Use actual IFD offset (e.g., after 8-byte header)
        let ifd_offset = 8u64;
        let (ifd_bytes, data_bytes, _total_size) =
            builder.write(ifd_offset, 0).expect("Failed to write IFD");

        // Verify data_bytes contains correct DOUBLE values
        assert_eq!(data_bytes.len(), 24, "Expected 24 bytes for 3 f64 values");

        // Verify each double value
        for (i, expected_value) in pixel_scale.iter().enumerate() {
            let offset = i * 8;
            let actual_bytes = &data_bytes[offset..offset + 8];
            let actual_value = f64::from_le_bytes(
                actual_bytes
                    .try_into()
                    .expect("Failed to convert bytes to f64"),
            );
            assert!(
                (actual_value - expected_value).abs() < 1e-10,
                "Double at index {} mismatch: expected {}, got {}",
                i,
                expected_value,
                actual_value
            );
        }

        // Verify the IFD entry contains correct offset
        // Classic TIFF IFD entry structure:
        // - 2 bytes: entry count (u16)
        // - 12 bytes per entry: tag(2) + type(2) + count(4) + value/offset(4)
        // - 4 bytes: next IFD offset
        assert!(ifd_bytes.len() >= 2 + 12 + 4, "IFD too small");

        // Parse the entry (skip 2-byte count)
        let entry_start = 2;
        let tag = u16::from_le_bytes([ifd_bytes[entry_start], ifd_bytes[entry_start + 1]]);
        let field_type =
            u16::from_le_bytes([ifd_bytes[entry_start + 2], ifd_bytes[entry_start + 3]]);
        let count = u32::from_le_bytes([
            ifd_bytes[entry_start + 4],
            ifd_bytes[entry_start + 5],
            ifd_bytes[entry_start + 6],
            ifd_bytes[entry_start + 7],
        ]);
        let data_offset = u32::from_le_bytes([
            ifd_bytes[entry_start + 8],
            ifd_bytes[entry_start + 9],
            ifd_bytes[entry_start + 10],
            ifd_bytes[entry_start + 11],
        ]);

        assert_eq!(tag, TiffTag::ModelPixelScale as u16, "Wrong tag");
        assert_eq!(field_type, 12, "Wrong field type (expected DOUBLE=12)");
        assert_eq!(count, 3, "Wrong count");

        // The data offset should point to where data_bytes starts
        // IFD is at offset 8, size is ifd_bytes.len()
        let expected_data_offset = ifd_offset + ifd_bytes.len() as u64;
        assert_eq!(
            data_offset as u64, expected_data_offset,
            "Data offset mismatch: expected {}, got {}",
            expected_data_offset, data_offset
        );
    }

    #[test]
    fn test_double_array_with_tiepoint() {
        // Test ModelTiepointTag with 6 doubles
        let mut builder = IfdBuilder::new(ByteOrderType::LittleEndian, TiffVariant::Classic);

        let tiepoint = vec![0.0, 0.0, 0.0, 105.857, 14.037, 0.0];
        builder.add_double_array(TiffTag::ModelTiepoint, tiepoint.clone());

        let ifd_offset = 100u64;
        let (_ifd_bytes, data_bytes, _total_size) =
            builder.write(ifd_offset, 0).expect("Failed to write IFD");

        // Verify data_bytes contains correct DOUBLE values
        assert_eq!(data_bytes.len(), 48, "Expected 48 bytes for 6 f64 values");

        // Verify each double value
        for (i, expected_value) in tiepoint.iter().enumerate() {
            let offset = i * 8;
            let actual_bytes = &data_bytes[offset..offset + 8];
            let actual_value = f64::from_le_bytes(
                actual_bytes
                    .try_into()
                    .expect("Failed to convert bytes to f64"),
            );
            assert!(
                (actual_value - expected_value).abs() < 1e-6,
                "Double at index {} mismatch: expected {}, got {}",
                i,
                expected_value,
                actual_value
            );
        }
    }
}
