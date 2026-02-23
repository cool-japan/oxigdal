//! GRIB2 Section 4: Product Definition Section

use crate::error::{GribError, Result};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;

/// GRIB2 Section 4: Product Definition Section
///
/// Contains information about the meteorological parameter,
/// forecast time, and vertical level.
#[derive(Debug, Clone)]
pub struct ProductDefinitionSection {
    /// Parameter category (e.g., temperature, moisture)
    pub parameter_category: u8,
    /// Parameter number within category
    pub parameter_number: u8,
    /// Type of generating process
    pub generating_process: u8,
    /// Forecast time in units indicated by time range
    pub forecast_time: u32,
    /// Type of first fixed surface
    pub first_surface_type: u8,
    /// Value of first fixed surface
    pub first_surface_value: f64,
}

impl ProductDefinitionSection {
    /// Parses the product definition section from raw bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut cursor = Cursor::new(data);

        let _num_coordinates = cursor.read_u16::<BigEndian>()?;
        let template_number = cursor.read_u16::<BigEndian>()?;

        match template_number {
            0 | 1 | 8 => {
                // Template 0: Analysis or forecast at horizontal level/layer
                let parameter_category = cursor.read_u8()?;
                let parameter_number = cursor.read_u8()?;
                let generating_process = cursor.read_u8()?;
                let _background = cursor.read_u8()?;
                let _analysis = cursor.read_u8()?;
                let _hours_cutoff = cursor.read_u16::<BigEndian>()?;
                let _minutes_cutoff = cursor.read_u8()?;
                let _time_unit = cursor.read_u8()?;
                let forecast_time = cursor.read_u32::<BigEndian>()?;
                let first_surface_type = cursor.read_u8()?;
                let _scale_factor = cursor.read_u8()?;
                let first_surface_value = cursor.read_u32::<BigEndian>()? as f64;

                Ok(Self {
                    parameter_category,
                    parameter_number,
                    generating_process,
                    forecast_time,
                    first_surface_type,
                    first_surface_value,
                })
            }
            _ => Err(GribError::UnsupportedProductTemplate(template_number)),
        }
    }
}
