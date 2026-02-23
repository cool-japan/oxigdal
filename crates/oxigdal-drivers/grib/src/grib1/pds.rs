//! GRIB1 Product Definition Section (PDS).
//!
//! The PDS contains metadata about the data parameter, level, time, and generating process.

use crate::error::Result;
use byteorder::{BigEndian, ReadBytesExt};
use chrono::{NaiveDate, NaiveDateTime};
use std::io::Read;

/// GRIB1 Product Definition Section
#[derive(Debug, Clone)]
pub struct ProductDefinitionSection {
    /// Parameter table version number
    pub table_version: u8,
    /// Identification of originating center
    pub center_id: u8,
    /// Generating process ID
    pub process_id: u8,
    /// Grid identification
    pub grid_id: u8,
    /// Flag indicating presence of GDS
    pub has_gds: bool,
    /// Flag indicating presence of BMS (bitmap section)
    pub has_bms: bool,
    /// Parameter and unit indicator
    pub parameter_number: u8,
    /// Type of level or layer
    pub level_type: u8,
    /// Level value
    pub level_value: f64,
    /// Year of century
    pub year: u16,
    /// Month
    pub month: u8,
    /// Day
    pub day: u8,
    /// Hour
    pub hour: u8,
    /// Minute
    pub minute: u8,
    /// Forecast time unit indicator
    pub time_range_indicator: u8,
    /// Period of time (P1)
    pub time_range_p1: u8,
    /// Period of time (P2)
    pub time_range_p2: u8,
    /// Number of products in average
    pub time_range_n: u16,
    /// Number of missing products
    pub time_range_nmissing: u8,
    /// Century (20 for 1900s, 21 for 2000s)
    pub century: u8,
    /// Sub-center identification
    pub subcenter_id: u8,
    /// Decimal scale factor
    pub decimal_scale: i16,
}

impl ProductDefinitionSection {
    /// Parse PDS from reader
    pub fn from_reader<R: Read>(reader: &mut R) -> Result<Self> {
        // Read section length (3 bytes)
        let length_bytes = [reader.read_u8()?, reader.read_u8()?, reader.read_u8()?];
        let _length = ((length_bytes[0] as usize) << 16)
            | ((length_bytes[1] as usize) << 8)
            | (length_bytes[2] as usize);

        // Parameter table version number
        let table_version = reader.read_u8()?;

        // Center ID
        let center_id = reader.read_u8()?;

        // Process ID
        let process_id = reader.read_u8()?;

        // Grid identification
        let grid_id = reader.read_u8()?;

        // GDS/BMS flag
        let gds_bms_flag = reader.read_u8()?;
        let has_gds = (gds_bms_flag & 0b1000_0000) != 0;
        let has_bms = (gds_bms_flag & 0b0100_0000) != 0;

        // Parameter number
        let parameter_number = reader.read_u8()?;

        // Level type
        let level_type = reader.read_u8()?;

        // Level values (2 bytes, interpretation depends on level type)
        let level1 = reader.read_u8()?;
        let level2 = reader.read_u8()?;
        let level_value = Self::decode_level_value(level_type, level1, level2);

        // Reference time
        let year = reader.read_u8()? as u16;
        let month = reader.read_u8()?;
        let day = reader.read_u8()?;
        let hour = reader.read_u8()?;
        let minute = reader.read_u8()?;

        // Time range indicator
        let time_range_indicator = reader.read_u8()?;

        // Time range P1 and P2
        let time_range_p1 = reader.read_u8()?;
        let time_range_p2 = reader.read_u8()?;

        // Time range N and number missing
        let time_range_n = reader.read_u16::<BigEndian>()?;
        let time_range_nmissing = reader.read_u8()?;

        // Century
        let century = reader.read_u8()?;

        // Sub-center
        let subcenter_id = reader.read_u8()?;

        // Decimal scale factor (signed)
        let decimal_scale = reader.read_i16::<BigEndian>()?;

        Ok(Self {
            table_version,
            center_id,
            process_id,
            grid_id,
            has_gds,
            has_bms,
            parameter_number,
            level_type,
            level_value,
            year,
            month,
            day,
            hour,
            minute,
            time_range_indicator,
            time_range_p1,
            time_range_p2,
            time_range_n,
            time_range_nmissing,
            century,
            subcenter_id,
            decimal_scale,
        })
    }

    /// Decode level value from level type and bytes
    fn decode_level_value(level_type: u8, level1: u8, level2: u8) -> f64 {
        match level_type {
            100 => {
                // Isobaric level in hPa, convert to Pa
                let hpa = ((level1 as u16) << 8) | (level2 as u16);
                (hpa as f64) * 100.0
            }
            103 => {
                // Height above ground in meters
                ((level1 as u16) << 8 | level2 as u16) as f64
            }
            105 => {
                // Sigma level
                ((level1 as u16) << 8 | level2 as u16) as f64 / 10000.0
            }
            107 => {
                // Isentropic level in K
                ((level1 as u16) << 8 | level2 as u16) as f64
            }
            111 => {
                // Depth below land surface in cm
                ((level1 as u16) << 8 | level2 as u16) as f64
            }
            _ => {
                // Default: treat as unsigned 16-bit value
                ((level1 as u16) << 8 | level2 as u16) as f64
            }
        }
    }

    /// Get reference time as NaiveDateTime
    pub fn reference_time(&self) -> Option<NaiveDateTime> {
        // Full year = (century - 1) * 100 + year
        let full_year = (self.century as i32 - 1) * 100 + self.year as i32;

        let date = NaiveDate::from_ymd_opt(full_year, self.month as u32, self.day as u32)?;
        date.and_hms_opt(self.hour as u32, self.minute as u32, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};

    #[test]
    fn test_level_decoding() {
        // Isobaric level: 500 hPa = 50000 Pa
        let value = ProductDefinitionSection::decode_level_value(100, 0x01, 0xF4);
        assert_eq!(value, 50000.0);

        // Height above ground: 10 meters
        let value = ProductDefinitionSection::decode_level_value(103, 0x00, 0x0A);
        assert_eq!(value, 10.0);

        // Sigma level: 0.9934 (0x26CE / 10000)
        let value = ProductDefinitionSection::decode_level_value(105, 0x26, 0xCE);
        assert!((value - 0.9934).abs() < 0.0001);
    }

    #[test]
    fn test_reference_time() {
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
            year: 24,
            month: 1,
            day: 15,
            hour: 12,
            minute: 0,
            time_range_indicator: 0,
            time_range_p1: 6,
            time_range_p2: 0,
            time_range_n: 0,
            time_range_nmissing: 0,
            century: 21,
            subcenter_id: 0,
            decimal_scale: 0,
        };

        let ref_time = pds
            .reference_time()
            .expect("Failed to parse PDS reference time");
        assert_eq!(ref_time.year(), 2024);
        assert_eq!(ref_time.month(), 1);
        assert_eq!(ref_time.day(), 15);
        assert_eq!(ref_time.hour(), 12);
    }
}
