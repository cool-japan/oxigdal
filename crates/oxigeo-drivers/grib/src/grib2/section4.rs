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
                // Scale factor of first fixed surface: `signed[1]` per WMO
                // PDT 4.0 (confirmed against eccodes'
                // definitions/grib2/templates/template.4.horizontal.def),
                // i.e. sign-and-magnitude (MSB = sign, low 7 bits =
                // magnitude), NOT two's complement.
                let scale_factor_raw = cursor.read_u8()?;
                // WMO Regulation 92.1.12: all bits set to 1 marks a missing
                // numeric value within a template; a missing scale factor
                // means "no scaling applied" (confirmed against eccodes'
                // G2Level::unpack_double, which skips the scaling loop
                // entirely when the scale factor is missing).
                let scale_factor_missing = scale_factor_raw == 0xFF;
                let scale_factor = if scale_factor_raw & 0x80 != 0 {
                    -((scale_factor_raw & 0x7F) as i32)
                } else {
                    scale_factor_raw as i32
                };
                // Scaled value of first fixed surface: `unsigned[4]` per the
                // same template -- plain unsigned, unlike the scale factor
                // above.
                let scaled_value = cursor.read_u32::<BigEndian>()?;
                // True value = scaled_value / 10^scale_factor (WMO PDT 4.0).
                // A missing scaled value (all bits set) has no defined
                // physical surface value; per the WMO convention (and
                // eccodes' G2Level::unpack_double) it decodes to 0 rather
                // than propagating a bogus magnitude.
                let first_surface_value = if scaled_value == u32::MAX {
                    0.0
                } else if scale_factor_missing {
                    scaled_value as f64
                } else {
                    scaled_value as f64 / 10f64.powi(scale_factor)
                };

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal PDT 4.0 byte buffer with the given raw scale-factor
    /// and scaled-value octets for the first fixed surface.
    fn pdt0_bytes(surface_type: u8, scale_factor_raw: u8, scaled_value: u32) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&0u16.to_be_bytes()); // num_coordinates
        data.extend_from_slice(&0u16.to_be_bytes()); // template 4.0
        data.push(0); // parameter_category
        data.push(0); // parameter_number
        data.push(0); // generating_process
        data.push(0); // background
        data.push(0); // analysis
        data.extend_from_slice(&0u16.to_be_bytes()); // hours cutoff
        data.push(0); // minutes cutoff
        data.push(1); // time unit
        data.extend_from_slice(&0u32.to_be_bytes()); // forecast_time
        data.push(surface_type); // first_surface_type
        data.push(scale_factor_raw); // scale factor of first fixed surface
        data.extend_from_slice(&scaled_value.to_be_bytes()); // scaled value
        data
    }

    #[test]
    fn test_first_surface_value_applies_positive_scale_factor() {
        // Isobaric surface, scale factor 2 (sign bit clear), scaled value
        // 850 -> true value 850 / 10^2 = 8.5.
        let data = pdt0_bytes(100, 0x02, 850);
        let pds = ProductDefinitionSection::from_bytes(&data)
            .expect("failed to parse PDT 4.0 with positive scale factor");
        assert!((pds.first_surface_value - 8.5).abs() < 1e-9);
    }

    #[test]
    fn test_first_surface_value_applies_negative_scale_factor() {
        // Scale factor -2 (sign bit set, magnitude 2): true value =
        // scaled_value / 10^-2 = scaled_value * 100.
        let data = pdt0_bytes(100, 0x82, 5);
        let pds = ProductDefinitionSection::from_bytes(&data)
            .expect("failed to parse PDT 4.0 with negative scale factor");
        assert!((pds.first_surface_value - 500.0).abs() < 1e-9);
    }

    #[test]
    fn test_first_surface_value_zero_scale_factor_is_identity() {
        let data = pdt0_bytes(1, 0x00, 42);
        let pds = ProductDefinitionSection::from_bytes(&data)
            .expect("failed to parse PDT 4.0 with zero scale factor");
        assert!((pds.first_surface_value - 42.0).abs() < 1e-9);
    }

    #[test]
    fn test_first_surface_value_missing_scaled_value_decodes_to_zero() {
        let data = pdt0_bytes(100, 0x00, u32::MAX);
        let pds = ProductDefinitionSection::from_bytes(&data)
            .expect("failed to parse PDT 4.0 with missing scaled value");
        assert_eq!(pds.first_surface_value, 0.0);
    }

    #[test]
    fn test_first_surface_value_missing_scale_factor_is_unscaled() {
        let data = pdt0_bytes(100, 0xFF, 12345);
        let pds = ProductDefinitionSection::from_bytes(&data)
            .expect("failed to parse PDT 4.0 with missing scale factor");
        assert!((pds.first_surface_value - 12345.0).abs() < 1e-9);
    }
}
