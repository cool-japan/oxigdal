//! GRIB1 Binary Data Section (BDS).
//!
//! The BDS contains the actual meteorological data in packed binary format.

use crate::error::{GribError, Result};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::Read;

/// GRIB1 Binary Data Section
#[derive(Debug, Clone)]
pub struct BinaryDataSection {
    /// Reference value (minimum value)
    pub reference_value: f32,
    /// Binary scale factor
    pub binary_scale: i16,
    /// Number of bits per data value
    pub num_bits: u8,
    /// Packed data bytes
    pub packed_data: Vec<u8>,
}

impl BinaryDataSection {
    /// Parse BDS from reader
    pub fn from_reader<R: Read>(reader: &mut R) -> Result<Self> {
        // Read section length (3 bytes)
        let length_bytes = [reader.read_u8()?, reader.read_u8()?, reader.read_u8()?];
        let length = ((length_bytes[0] as usize) << 16)
            | ((length_bytes[1] as usize) << 8)
            | (length_bytes[2] as usize);

        if length < 11 {
            return Err(GribError::InvalidSectionLength {
                expected: 11,
                actual: length,
            });
        }

        // Flag and unused bits
        let flag = reader.read_u8()?;
        let num_bits = flag & 0x0F; // Lower 4 bits

        // Check for unsupported features
        if (flag & 0x80) != 0 {
            return Err(GribError::UnsupportedPacking(
                "Spherical harmonics not supported".to_string(),
            ));
        }
        if (flag & 0x40) != 0 {
            return Err(GribError::UnsupportedPacking(
                "Complex packing not supported".to_string(),
            ));
        }

        // Binary scale factor (signed 16-bit)
        let binary_scale = reader.read_i16::<BigEndian>()?;

        // Reference value (IEEE 754 32-bit float, big endian)
        let reference_value = reader.read_f32::<BigEndian>()?;

        // Read packed data
        let packed_data_length = length.saturating_sub(11);
        let mut packed_data = vec![0u8; packed_data_length];
        reader.read_exact(&mut packed_data)?;

        Ok(Self {
            reference_value,
            binary_scale,
            num_bits,
            packed_data,
        })
    }

    /// Get the scale factor as a multiplier
    pub fn scale_multiplier(&self) -> f32 {
        2.0f32.powi(self.binary_scale as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scale_multiplier() {
        let bds = BinaryDataSection {
            reference_value: 0.0,
            binary_scale: 0,
            num_bits: 12,
            packed_data: vec![],
        };
        assert_eq!(bds.scale_multiplier(), 1.0);

        let bds = BinaryDataSection {
            reference_value: 0.0,
            binary_scale: -3,
            num_bits: 12,
            packed_data: vec![],
        };
        assert_eq!(bds.scale_multiplier(), 0.125);

        let bds = BinaryDataSection {
            reference_value: 0.0,
            binary_scale: 2,
            num_bits: 12,
            packed_data: vec![],
        };
        assert_eq!(bds.scale_multiplier(), 4.0);
    }
}
