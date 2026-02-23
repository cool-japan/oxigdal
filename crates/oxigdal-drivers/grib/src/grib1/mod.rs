//! GRIB Edition 1 format support.
//!
//! This module provides parsing and data extraction for GRIB1 format files.
//! GRIB1 consists of several sections: Indicator, Product Definition, Grid Definition,
//! Bitmap (optional), Binary Data, and End sections.

pub mod bds;
pub mod decoder;
pub mod gds;
pub mod pds;

use crate::error::{GribError, Result};
use crate::grid::GridDefinition;
use crate::parameter::{LevelType, Parameter, lookup_grib1_parameter};
use byteorder::{BigEndian, ReadBytesExt};
use chrono::NaiveDateTime;
use std::io::Cursor;

pub use bds::BinaryDataSection;
pub use decoder::Grib1Decoder;
pub use gds::GridDefinitionSection;
pub use pds::ProductDefinitionSection;

/// GRIB1 message structure
#[derive(Debug, Clone)]
pub struct Grib1Message {
    /// Product Definition Section
    pub pds: ProductDefinitionSection,
    /// Grid Definition Section (optional in spec, but usually present)
    pub gds: Option<GridDefinitionSection>,
    /// Binary Data Section
    pub bds: BinaryDataSection,
    /// Bitmap (optional)
    pub bitmap: Option<Vec<bool>>,
}

impl Grib1Message {
    /// Parse GRIB1 message from data bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut cursor = Cursor::new(data);

        // Parse PDS (Product Definition Section)
        let pds = ProductDefinitionSection::from_reader(&mut cursor)?;

        // Parse GDS (Grid Definition Section) if present
        let gds = if pds.has_gds {
            Some(GridDefinitionSection::from_reader(&mut cursor)?)
        } else {
            None
        };

        // Parse Bitmap Section if present
        let bitmap = if pds.has_bms {
            Some(Self::parse_bitmap(&mut cursor)?)
        } else {
            None
        };

        // Parse BDS (Binary Data Section)
        let bds = BinaryDataSection::from_reader(&mut cursor)?;

        Ok(Self {
            pds,
            gds,
            bds,
            bitmap,
        })
    }

    /// Parse bitmap section
    fn parse_bitmap<R: std::io::Read>(reader: &mut R) -> Result<Vec<bool>> {
        // Section length (3 bytes)
        let length = Self::read_u24(reader)?;

        // Unused bits at end of section
        let _unused_bits = reader.read_u8()?;

        // Bitmap indicator (0 = bitmap follows, 254 = predefined bitmap)
        let indicator = reader.read_u16::<BigEndian>()?;

        if indicator != 0 {
            return Err(GribError::InvalidBitmap(format!(
                "Unsupported bitmap indicator: {}",
                indicator
            )));
        }

        // Read bitmap bytes
        let bitmap_bytes = length.saturating_sub(6);
        let mut bitmap = Vec::with_capacity(bitmap_bytes * 8);

        for _ in 0..bitmap_bytes {
            let byte = reader.read_u8()?;
            for bit in 0..8 {
                bitmap.push((byte & (1 << (7 - bit))) != 0);
            }
        }

        Ok(bitmap)
    }

    /// Read 3-byte unsigned integer (big-endian)
    fn read_u24<R: std::io::Read>(reader: &mut R) -> Result<usize> {
        let b1 = reader.read_u8()? as usize;
        let b2 = reader.read_u8()? as usize;
        let b3 = reader.read_u8()? as usize;
        Ok((b1 << 16) | (b2 << 8) | b3)
    }

    /// Get parameter information
    pub fn parameter(&self) -> Result<Parameter> {
        lookup_grib1_parameter(self.pds.table_version, self.pds.parameter_number)
    }

    /// Get grid definition
    pub fn grid(&self) -> Option<&GridDefinition> {
        self.gds.as_ref().map(|gds| &gds.grid)
    }

    /// Get level type
    pub fn level_type(&self) -> LevelType {
        LevelType::from_grib1_code(self.pds.level_type)
    }

    /// Get level value
    pub fn level_value(&self) -> f64 {
        self.pds.level_value
    }

    /// Get reference time (forecast base time)
    pub fn reference_time(&self) -> Option<NaiveDateTime> {
        self.pds.reference_time()
    }

    /// Get forecast time offset (hours from reference time)
    pub fn forecast_offset_hours(&self) -> u16 {
        self.pds.time_range_p1 as u16
    }

    /// Get valid time (reference time + forecast offset)
    pub fn valid_time(&self) -> Option<NaiveDateTime> {
        let ref_time = self.reference_time()?;
        Some(ref_time + chrono::Duration::hours(self.forecast_offset_hours() as i64))
    }

    /// Decode data values
    pub fn decode_data(&self) -> Result<Vec<f32>> {
        let decoder = Grib1Decoder::new(self)?;
        decoder.decode()
    }

    /// Get number of grid points
    pub fn num_points(&self) -> usize {
        self.gds.as_ref().map(|g| g.grid.num_points()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_u24() {
        let data = [0x01, 0x02, 0x03];
        let mut cursor = Cursor::new(&data);
        let value =
            Grib1Message::read_u24(&mut cursor).expect("Failed to read 3-byte unsigned integer");
        assert_eq!(value, 0x010203);
    }

    #[test]
    fn test_level_type() {
        // Create a minimal PDS for testing
        let pds = ProductDefinitionSection {
            table_version: 3,
            center_id: 7,
            process_id: 0,
            grid_id: 255,
            has_gds: true,
            has_bms: false,
            parameter_number: 11,
            level_type: 100,
            level_value: 50000.0,
            year: 2024,
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

        let msg = Grib1Message {
            pds,
            gds: None,
            bds: BinaryDataSection {
                reference_value: 0.0,
                binary_scale: 0,
                num_bits: 16,
                packed_data: vec![],
            },
            bitmap: None,
        };

        assert_eq!(msg.level_type(), LevelType::Isobaric);
        assert_eq!(msg.level_value(), 50000.0);
    }
}
