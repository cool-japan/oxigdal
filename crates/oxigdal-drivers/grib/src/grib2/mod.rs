//! GRIB Edition 2 format support.
//!
//! This module provides parsing and data extraction for GRIB2 format files.
//! GRIB2 uses a template-based system with sections for identification, grid definition,
//! product definition, data representation, and data.

pub mod decoder;
pub mod section1;
pub mod section3;
pub mod section4;
pub mod section5;
pub mod section7;

use crate::error::Result;
use crate::grid::GridDefinition;
use crate::message::SectionHeader;
use crate::parameter::{LevelType, Parameter, lookup_grib2_parameter};

pub use decoder::Grib2Decoder;
pub use section1::IdentificationSection;
pub use section3::GridDefinitionSection;
pub use section4::ProductDefinitionSection;
pub use section5::DataRepresentationSection;
pub use section7::DataSection;

/// GRIB2 message structure
#[derive(Debug, Clone)]
pub struct Grib2Message {
    /// Discipline (from Section 0)
    pub discipline: u8,
    /// Section 1: Identification
    pub identification: IdentificationSection,
    /// Section 3: Grid Definition
    pub grid_definition: GridDefinitionSection,
    /// Section 4: Product Definition
    pub product_definition: ProductDefinitionSection,
    /// Section 5: Data Representation
    pub data_representation: DataRepresentationSection,
    /// Section 6: Bitmap (optional)
    pub bitmap: Option<Vec<bool>>,
    /// Section 7: Data
    pub data_section: DataSection,
}

impl Grib2Message {
    /// Parse GRIB2 message from data bytes
    pub fn from_bytes(data: &[u8], discipline: u8) -> Result<Self> {
        let mut offset = 0usize;

        // Section 1: Identification
        let section1_header = SectionHeader::from_bytes(&data[offset..])?;
        offset += 5;
        let section1_data = &data[offset..offset + section1_header.length as usize - 5];
        let identification = IdentificationSection::from_bytes(section1_data)?;
        offset += section1_data.len();

        // Section 2: Local Use (optional, skip for now)
        if offset < data.len() {
            let peek_header = SectionHeader::from_bytes(&data[offset..])?;
            if peek_header.number as u8 == 2 {
                offset += peek_header.length as usize;
            }
        }

        // Section 3: Grid Definition
        let section3_header = SectionHeader::from_bytes(&data[offset..])?;
        offset += 5;
        let section3_data = &data[offset..offset + section3_header.length as usize - 5];
        let grid_definition = GridDefinitionSection::from_bytes(section3_data)?;
        offset += section3_data.len();

        // Section 4: Product Definition
        let section4_header = SectionHeader::from_bytes(&data[offset..])?;
        offset += 5;
        let section4_data = &data[offset..offset + section4_header.length as usize - 5];
        let product_definition = ProductDefinitionSection::from_bytes(section4_data)?;
        offset += section4_data.len();

        // Section 5: Data Representation
        let section5_header = SectionHeader::from_bytes(&data[offset..])?;
        offset += 5;
        let section5_data = &data[offset..offset + section5_header.length as usize - 5];
        let data_representation = DataRepresentationSection::from_bytes(section5_data)?;
        offset += section5_data.len();

        // Section 6: Bitmap (optional)
        let bitmap = if offset < data.len() {
            let section6_header = SectionHeader::from_bytes(&data[offset..])?;
            if section6_header.number as u8 == 6 {
                offset += 5;
                let bitmap_indicator = data[offset];
                offset += 1;

                if bitmap_indicator == 0 {
                    // Bitmap present
                    let bitmap_bytes = section6_header.length as usize - 6;
                    let mut bm = Vec::with_capacity(bitmap_bytes * 8);
                    for i in 0..bitmap_bytes {
                        let byte = data[offset + i];
                        for bit in 0..8 {
                            bm.push((byte & (1 << (7 - bit))) != 0);
                        }
                    }
                    offset += bitmap_bytes;
                    Some(bm)
                } else {
                    offset += section6_header.length as usize - 6;
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // Section 7: Data
        let section7_header = SectionHeader::from_bytes(&data[offset..])?;
        offset += 5;
        let section7_data = &data[offset..offset + section7_header.length as usize - 5];
        let data_section = DataSection::from_bytes(section7_data)?;

        Ok(Self {
            discipline,
            identification,
            grid_definition,
            product_definition,
            data_representation,
            bitmap,
            data_section,
        })
    }

    /// Get parameter information
    pub fn parameter(&self) -> Result<Parameter> {
        lookup_grib2_parameter(
            self.discipline,
            self.product_definition.parameter_category,
            self.product_definition.parameter_number,
        )
    }

    /// Get grid definition
    pub fn grid(&self) -> &GridDefinition {
        &self.grid_definition.grid
    }

    /// Get level type
    pub fn level_type(&self) -> LevelType {
        LevelType::from_grib2_code(self.product_definition.first_surface_type)
    }

    /// Get level value
    pub fn level_value(&self) -> f64 {
        self.product_definition.first_surface_value
    }

    /// Get reference time
    pub fn reference_time(&self) -> Option<chrono::NaiveDateTime> {
        self.identification.reference_time()
    }

    /// Get forecast offset in hours
    pub fn forecast_offset_hours(&self) -> u32 {
        self.product_definition.forecast_time
    }

    /// Get valid time
    pub fn valid_time(&self) -> Option<chrono::NaiveDateTime> {
        let ref_time = self.reference_time()?;
        Some(ref_time + chrono::Duration::hours(self.forecast_offset_hours() as i64))
    }

    /// Decode data values
    pub fn decode_data(&self) -> Result<Vec<f32>> {
        let decoder = Grib2Decoder::new(self)?;
        decoder.decode()
    }

    /// Get number of grid points
    pub fn num_points(&self) -> usize {
        self.grid_definition.num_points
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level_type() {
        assert_eq!(LevelType::from_grib2_code(1), LevelType::Surface);
        assert_eq!(LevelType::from_grib2_code(100), LevelType::Isobaric);
        assert_eq!(
            LevelType::from_grib2_code(103),
            LevelType::HeightAboveGround
        );
    }
}
