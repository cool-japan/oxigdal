//! GRIB2 data decoder

use crate::error::{GribError, Result};
use crate::grib2::Grib2Message;

/// GRIB2 data decoder for unpacking binary meteorological data.
pub struct Grib2Decoder<'a> {
    /// Reference to the GRIB2 message to decode
    message: &'a Grib2Message,
}

impl<'a> Grib2Decoder<'a> {
    /// Creates a new decoder for the given GRIB2 message.
    pub fn new(message: &'a Grib2Message) -> Result<Self> {
        Ok(Self { message })
    }

    /// Decodes the packed binary data into f32 values.
    pub fn decode(&self) -> Result<Vec<f32>> {
        let dr = &self.message.data_representation;
        let num_points = self.message.num_points();

        if dr.bits_per_value == 0 {
            return Ok(vec![dr.reference_value; num_points]);
        }

        let packed_values = self.unpack_bits(
            &self.message.data_section.packed_data,
            dr.bits_per_value,
            dr.num_data_points as usize,
        )?;

        let scale = dr.scale_multiplier();
        let decimal = dr.decimal_divisor();

        let mut values = Vec::with_capacity(num_points);

        if let Some(bitmap) = &self.message.bitmap {
            let mut packed_idx = 0;
            for &present in bitmap.iter().take(num_points) {
                if present {
                    if packed_idx >= packed_values.len() {
                        return Err(GribError::DecodingError(
                            "Packed data too short for bitmap".to_string(),
                        ));
                    }
                    let value =
                        (dr.reference_value + packed_values[packed_idx] as f32 * scale) / decimal;
                    values.push(value);
                    packed_idx += 1;
                } else {
                    values.push(f32::NAN);
                }
            }
        } else {
            for &raw in packed_values.iter().take(num_points) {
                let value = (dr.reference_value + raw as f32 * scale) / decimal;
                values.push(value);
            }
        }

        Ok(values)
    }

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

    fn read_bits(&self, data: &[u8], bit_offset: usize, num_bits: usize) -> Result<u32> {
        if num_bits == 0 {
            return Ok(0);
        }

        let byte_offset = bit_offset / 8;
        let bit_in_byte = bit_offset % 8;
        let bytes_needed = (bit_in_byte + num_bits).div_ceil(8).max(1);

        if byte_offset + bytes_needed > data.len() {
            return Err(GribError::TruncatedMessage {
                expected: byte_offset + bytes_needed,
                actual: data.len(),
            });
        }

        let mut accumulator = 0u64;
        for i in 0..bytes_needed.min(8) {
            if byte_offset + i < data.len() {
                accumulator = (accumulator << 8) | (data[byte_offset + i] as u64);
            }
        }

        let total_bits = bytes_needed * 8;
        let shift_amount = total_bits - bit_in_byte - num_bits;
        let value = (accumulator >> shift_amount) & ((1u64 << num_bits) - 1);

        Ok(value as u32)
    }
}
