//! GRIB1 data decoder.
//!
//! This module provides decoding of packed GRIB1 binary data into floating-point values.

use crate::error::{GribError, Result};
use crate::grib1::Grib1Message;

/// GRIB1 data decoder
pub struct Grib1Decoder<'a> {
    message: &'a Grib1Message,
}

impl<'a> Grib1Decoder<'a> {
    /// Create a new decoder for a GRIB1 message
    pub fn new(message: &'a Grib1Message) -> Result<Self> {
        Ok(Self { message })
    }

    /// Decode the packed data into floating-point values
    pub fn decode(&self) -> Result<Vec<f32>> {
        let bds = &self.message.bds;
        let num_points = self.message.num_points();

        if num_points == 0 {
            return Ok(Vec::new());
        }

        let num_bits = bds.num_bits;
        if num_bits == 0 {
            // All values are the reference value
            return Ok(vec![bds.reference_value; num_points]);
        }

        // Decode packed values
        let packed_values = self.unpack_bits(&bds.packed_data, num_bits, num_points)?;

        // Apply scaling and reference value
        let scale = bds.scale_multiplier();
        let decimal_scale = 10.0f32.powi(self.message.pds.decimal_scale as i32);

        let mut values = Vec::with_capacity(num_points);

        if let Some(bitmap) = &self.message.bitmap {
            // Apply bitmap
            let mut packed_idx = 0;
            for &present in bitmap.iter().take(num_points) {
                if present {
                    if packed_idx >= packed_values.len() {
                        return Err(GribError::DecodingError(
                            "Packed data too short for bitmap".to_string(),
                        ));
                    }
                    let raw_value = packed_values[packed_idx];
                    let value = (bds.reference_value + raw_value as f32 * scale) / decimal_scale;
                    values.push(value);
                    packed_idx += 1;
                } else {
                    values.push(f32::NAN);
                }
            }
        } else {
            // No bitmap, all values present
            for &raw_value in packed_values.iter().take(num_points) {
                let value = (bds.reference_value + raw_value as f32 * scale) / decimal_scale;
                values.push(value);
            }
        }

        Ok(values)
    }

    /// Unpack bit-packed values from byte array
    fn unpack_bits(&self, data: &[u8], num_bits: u8, num_values: usize) -> Result<Vec<u32>> {
        if num_bits > 32 {
            return Err(GribError::InvalidBitOperation(format!(
                "Number of bits {} exceeds 32",
                num_bits
            )));
        }

        let mut values = Vec::with_capacity(num_values);
        let mut bit_offset = 0usize;

        for _ in 0..num_values {
            let value = self.read_bits(data, bit_offset, num_bits as usize)?;
            values.push(value);
            bit_offset += num_bits as usize;
        }

        Ok(values)
    }

    /// Read a specific number of bits from a byte array at a given bit offset
    fn read_bits(&self, data: &[u8], bit_offset: usize, num_bits: usize) -> Result<u32> {
        if num_bits == 0 {
            return Ok(0);
        }

        let byte_offset = bit_offset / 8;
        let bit_in_byte = bit_offset % 8;

        // Calculate how many bytes we need to read
        let bytes_needed = (bit_in_byte + num_bits).div_ceil(8).max(1);

        if byte_offset + bytes_needed > data.len() {
            return Err(GribError::TruncatedMessage {
                expected: byte_offset + bytes_needed,
                actual: data.len(),
            });
        }

        // Read up to 8 bytes into a u64 for extraction
        let mut accumulator = 0u64;
        for i in 0..bytes_needed.min(8) {
            if byte_offset + i < data.len() {
                accumulator = (accumulator << 8) | (data[byte_offset + i] as u64);
            }
        }

        // Shift to align the bits we want to the right
        let total_bits = bytes_needed * 8;
        let shift_amount = total_bits - bit_in_byte - num_bits;
        let value = (accumulator >> shift_amount) & ((1u64 << num_bits) - 1);

        Ok(value as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grib1::{BinaryDataSection, Grib1Message, ProductDefinitionSection};

    #[test]
    fn test_unpack_bits() {
        // Create a minimal message for testing
        let pds = ProductDefinitionSection {
            table_version: 3,
            center_id: 7,
            process_id: 0,
            grid_id: 255,
            has_gds: false,
            has_bms: false,
            parameter_number: 11,
            level_type: 100,
            level_value: 50000.0,
            year: 24,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            time_range_indicator: 0,
            time_range_p1: 0,
            time_range_p2: 0,
            time_range_n: 0,
            time_range_nmissing: 0,
            century: 21,
            subcenter_id: 0,
            decimal_scale: 0,
        };

        // Test data: [0b1010_1010, 0b1100_1100] = [170, 204]
        // With 8 bits per value: [170, 204]
        // With 4 bits per value: [10, 10, 12, 12]
        let data = vec![0b1010_1010, 0b1100_1100];

        let bds = BinaryDataSection {
            reference_value: 0.0,
            binary_scale: 0,
            num_bits: 8,
            packed_data: data.clone(),
        };

        let msg = Grib1Message {
            pds,
            gds: None,
            bds,
            bitmap: None,
        };

        let decoder = Grib1Decoder::new(&msg).expect("Failed to create GRIB1 decoder");

        // Test 8-bit unpacking
        let values = decoder
            .unpack_bits(&data, 8, 2)
            .expect("Failed to unpack 8-bit values");
        assert_eq!(values, vec![170, 204]);

        // Test 4-bit unpacking
        let values = decoder
            .unpack_bits(&data, 4, 4)
            .expect("Failed to unpack 4-bit values");
        assert_eq!(values, vec![10, 10, 12, 12]);
    }

    #[test]
    fn test_decode_constant_field() {
        let pds = ProductDefinitionSection {
            table_version: 3,
            center_id: 7,
            process_id: 0,
            grid_id: 255,
            has_gds: false,
            has_bms: false,
            parameter_number: 11,
            level_type: 100,
            level_value: 50000.0,
            year: 24,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            time_range_indicator: 0,
            time_range_p1: 0,
            time_range_p2: 0,
            time_range_n: 0,
            time_range_nmissing: 0,
            century: 21,
            subcenter_id: 0,
            decimal_scale: 0,
        };

        let bds = BinaryDataSection {
            reference_value: 273.15,
            binary_scale: 0,
            num_bits: 0, // Constant field
            packed_data: vec![],
        };

        let msg = Grib1Message {
            pds,
            gds: None,
            bds,
            bitmap: None,
        };

        let decoder =
            Grib1Decoder::new(&msg).expect("Failed to create GRIB1 decoder for constant field");
        let values = decoder.decode().expect("Failed to decode constant field");
        assert_eq!(values, vec![]);
    }
}
