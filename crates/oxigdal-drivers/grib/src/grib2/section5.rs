//! GRIB2 Section 5: Data Representation Section

use crate::error::{GribError, Result};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;

/// GRIB2 Section 5: Data Representation Section
///
/// Describes how the data values are packed and scaled.
#[derive(Debug, Clone)]
pub struct DataRepresentationSection {
    /// Number of data points
    pub num_data_points: u32,
    /// Data representation template number
    pub template_number: u16,
    /// Reference value (R) used in packing
    pub reference_value: f32,
    /// Binary scale factor (E)
    pub binary_scale_factor: i16,
    /// Decimal scale factor (D)
    pub decimal_scale_factor: i16,
    /// Number of bits used for each packed value
    pub bits_per_value: u8,
}

impl DataRepresentationSection {
    /// Parses the data representation section from raw bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut cursor = Cursor::new(data);

        let num_data_points = cursor.read_u32::<BigEndian>()?;
        let template_number = cursor.read_u16::<BigEndian>()?;

        match template_number {
            0 | 40 => {
                // Template 0: Simple packing
                let reference_value = cursor.read_f32::<BigEndian>()?;
                let binary_scale_factor = cursor.read_i16::<BigEndian>()?;
                let decimal_scale_factor = cursor.read_i16::<BigEndian>()?;
                let bits_per_value = cursor.read_u8()?;

                Ok(Self {
                    num_data_points,
                    template_number,
                    reference_value,
                    binary_scale_factor,
                    decimal_scale_factor,
                    bits_per_value,
                })
            }
            _ => Err(GribError::UnsupportedDataTemplate(template_number)),
        }
    }

    /// Calculates the binary scale multiplier (2^E).
    pub fn scale_multiplier(&self) -> f32 {
        2.0f32.powi(self.binary_scale_factor as i32)
    }

    /// Calculates the decimal divisor (10^D).
    pub fn decimal_divisor(&self) -> f32 {
        10.0f32.powi(self.decimal_scale_factor as i32)
    }
}
