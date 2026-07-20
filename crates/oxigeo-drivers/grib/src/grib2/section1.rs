//! GRIB2 Section 1: Identification Section

use crate::error::Result;
use byteorder::{BigEndian, ReadBytesExt};
use chrono::{NaiveDate, NaiveDateTime};
use std::io::Cursor;

/// GRIB2 Section 1: Identification Section
///
/// Contains metadata about the originating center and reference time.
#[derive(Debug, Clone)]
pub struct IdentificationSection {
    /// WMO originating center ID
    pub center_id: u16,
    /// Originating sub-center ID
    pub subcenter_id: u16,
    /// Master tables version number
    pub master_table_version: u8,
    /// Local tables version number
    pub local_table_version: u8,
    /// Significance of reference time
    pub significance_of_reference_time: u8,
    /// Reference year
    pub year: u16,
    /// Reference month
    pub month: u8,
    /// Reference day
    pub day: u8,
    /// Reference hour
    pub hour: u8,
    /// Reference minute
    pub minute: u8,
    /// Reference second
    pub second: u8,
    /// Production status of data
    pub production_status: u8,
    /// Type of processed data
    pub type_of_data: u8,
}

impl IdentificationSection {
    /// Parses the identification section from raw bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut cursor = Cursor::new(data);
        Ok(Self {
            center_id: cursor.read_u16::<BigEndian>()?,
            subcenter_id: cursor.read_u16::<BigEndian>()?,
            master_table_version: cursor.read_u8()?,
            local_table_version: cursor.read_u8()?,
            significance_of_reference_time: cursor.read_u8()?,
            year: cursor.read_u16::<BigEndian>()?,
            month: cursor.read_u8()?,
            day: cursor.read_u8()?,
            hour: cursor.read_u8()?,
            minute: cursor.read_u8()?,
            second: cursor.read_u8()?,
            production_status: cursor.read_u8()?,
            type_of_data: cursor.read_u8()?,
        })
    }

    /// Returns the reference time as a NaiveDateTime.
    pub fn reference_time(&self) -> Option<NaiveDateTime> {
        let date = NaiveDate::from_ymd_opt(self.year as i32, self.month as u32, self.day as u32)?;
        date.and_hms_opt(self.hour as u32, self.minute as u32, self.second as u32)
    }
}
