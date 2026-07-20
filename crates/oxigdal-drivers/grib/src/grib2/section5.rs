//! GRIB2 Section 5: Data Representation Section

use crate::error::{GribError, Result};
use crate::grib2::decoder::{ComplexPackingParams, SpatialDiffParams};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;

/// GRIB2 Section 5: Data Representation Section
///
/// Describes how the data values are packed and scaled.
///
/// The flat fields (`reference_value`, `binary_scale_factor`,
/// `decimal_scale_factor`, `bits_per_value`) describe simple packing
/// (DRT 5.0 / 5.40) and are populated for every supported template so the
/// simple-packing API stays stable. For complex packing (DRT 5.2) the
/// `complex_packing` field carries the full parameter block; DRT 5.3
/// additionally populates `spatial_diff`.
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
    /// Complex-packing parameters (present for DRT 5.2 and 5.3).
    pub complex_packing: Option<ComplexPackingParams>,
    /// Spatial-differencing parameters (present for DRT 5.3 only).
    pub spatial_diff: Option<SpatialDiffParams>,
}

impl DataRepresentationSection {
    /// Parses the data representation section from raw bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut cursor = Cursor::new(data);

        let num_data_points = cursor.read_u32::<BigEndian>()?;
        let template_number = cursor.read_u16::<BigEndian>()?;

        match template_number {
            0 | 40 => {
                // Template 5.0: Simple packing (5.40 is JPEG2000 but its
                // common header is laid out identically; the actual JPEG2000
                // payload decode remains out of scope).
                let reference_value = cursor.read_f32::<BigEndian>()?;
                let binary_scale_factor = read_i16_sign_magnitude(&mut cursor)?;
                let decimal_scale_factor = read_i16_sign_magnitude(&mut cursor)?;
                let bits_per_value = cursor.read_u8()?;

                Ok(Self {
                    num_data_points,
                    template_number,
                    reference_value,
                    binary_scale_factor,
                    decimal_scale_factor,
                    bits_per_value,
                    complex_packing: None,
                    spatial_diff: None,
                })
            }
            2 => {
                // Template 5.2: Complex packing.
                let complex = Self::parse_complex_packing(&mut cursor)?;
                Ok(Self {
                    num_data_points,
                    template_number,
                    reference_value: complex.reference_value,
                    binary_scale_factor: complex.binary_scale_factor,
                    decimal_scale_factor: complex.decimal_scale_factor,
                    bits_per_value: complex.bits_per_value,
                    complex_packing: Some(complex),
                    spatial_diff: None,
                })
            }
            3 => {
                // Template 5.3: Complex packing with spatial differencing.
                let complex = Self::parse_complex_packing(&mut cursor)?;
                let spatial = Self::parse_spatial_diff(&mut cursor)?;
                Ok(Self {
                    num_data_points,
                    template_number,
                    reference_value: complex.reference_value,
                    binary_scale_factor: complex.binary_scale_factor,
                    decimal_scale_factor: complex.decimal_scale_factor,
                    bits_per_value: complex.bits_per_value,
                    complex_packing: Some(complex),
                    spatial_diff: Some(spatial),
                })
            }
            _ => Err(GribError::UnsupportedDataTemplate(template_number)),
        }
    }

    /// Parses the DRT 5.2 complex-packing parameter block.
    ///
    /// The cursor must be positioned immediately after the template number
    /// (i.e. at octet 12 of the section). Octet numbers in the comments
    /// follow the WMO Manual on Codes Vol. I.2, Template 5.2.
    fn parse_complex_packing(cursor: &mut Cursor<&[u8]>) -> Result<ComplexPackingParams> {
        // Octets 12-15: reference value R (IEEE-754 f32).
        let reference_value = cursor.read_f32::<BigEndian>()?;
        // Octets 16-17: binary scale factor E (sign-and-magnitude, per WMO
        // Regulation 92.1.5 -- see `read_i16_sign_magnitude`).
        let binary_scale_factor = read_i16_sign_magnitude(cursor)?;
        // Octets 18-19: decimal scale factor D (sign-and-magnitude).
        let decimal_scale_factor = read_i16_sign_magnitude(cursor)?;
        // Octet 20: number of bits for each group reference value.
        let bits_per_value = cursor.read_u8()?;
        // Octet 21: type of original field values (0 = float, 1 = int).
        let _type_of_original = cursor.read_u8()?;
        // Octet 22: group splitting method used.
        let _group_splitting_method = cursor.read_u8()?;
        // Octet 23: missing value management used.
        let missing_value_management = cursor.read_u8()?;
        // Octets 24-27: primary missing value substitute.
        let primary_missing_substitute = cursor.read_u32::<BigEndian>()?;
        // Octets 28-31: secondary missing value substitute.
        let secondary_missing_substitute = cursor.read_u32::<BigEndian>()?;
        // Octets 32-35: NG, number of groups of data values.
        let num_groups = cursor.read_u32::<BigEndian>()?;
        // Octet 36: reference for group widths.
        let group_widths_reference = cursor.read_u8()?;
        // Octet 37: number of bits used for the group widths.
        let group_widths_bits = cursor.read_u8()?;
        // Octets 38-41: reference for group lengths.
        let group_lengths_reference = cursor.read_u32::<BigEndian>()?;
        // Octet 42: length increment for the group lengths.
        let group_length_increment = cursor.read_u8()?;
        // Octets 43-46: true length of the last group.
        let group_last_length = cursor.read_u32::<BigEndian>()?;
        // Octet 47: number of bits used for the scaled group lengths.
        let group_lengths_bits = cursor.read_u8()?;

        Ok(ComplexPackingParams {
            reference_value,
            binary_scale_factor,
            decimal_scale_factor,
            bits_per_value,
            num_groups,
            group_widths_reference,
            group_widths_bits,
            group_lengths_reference,
            group_length_increment,
            group_last_length,
            group_lengths_bits,
            missing_value_management,
            primary_missing_substitute,
            secondary_missing_substitute,
        })
    }

    /// Parses the DRT 5.3 spatial-differencing extension.
    ///
    /// The cursor must be positioned immediately after the complex-packing
    /// block (octet 48 of the section).
    fn parse_spatial_diff(cursor: &mut Cursor<&[u8]>) -> Result<SpatialDiffParams> {
        // Octet 48: order of spatial differencing (1 or 2).
        let order = cursor.read_u8()?;
        // Octet 49: number of octets required in the data section to specify
        // the extra spatial-differencing descriptor values.
        let extra_octets = cursor.read_u8()?;

        if order != 1 && order != 2 {
            return Err(GribError::InvalidDataRepresentation(format!(
                "DRT 5.3: unsupported spatial differencing order {order} (only 1 or 2)"
            )));
        }
        if extra_octets == 0 {
            return Err(GribError::InvalidDataRepresentation(
                "DRT 5.3: extra octet count for spatial differencing is zero".to_string(),
            ));
        }

        Ok(SpatialDiffParams {
            order,
            extra_octets,
        })
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

/// Reads a big-endian 16-bit WMO sign-and-magnitude integer: the most
/// significant bit is a sign flag, and the remaining 15 bits hold the
/// magnitude. This is the encoding used by GRIB2 `signed[2]` fields --
/// binary/decimal scale factor E/D in Data Representation Templates 5.0,
/// 5.2 and 5.3 (confirmed against eccodes'
/// definitions/grib2/templates/template.5.packing.def) -- per WMO Manual on
/// Codes Regulation 92.1.5. This mirrors `BitReader::read_sign_magnitude`
/// in `grib2::decoder`, which implements the same convention at the bit
/// level for DRT 5.3 spatial-differencing descriptors.
fn read_i16_sign_magnitude(cursor: &mut Cursor<&[u8]>) -> Result<i16> {
    let raw = cursor.read_u16::<BigEndian>()?;
    let magnitude = (raw & 0x7FFF) as i16;
    Ok(if raw & 0x8000 != 0 {
        -magnitude
    } else {
        magnitude
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal DRT 5.0 (simple packing) section buffer.
    fn simple_packing_bytes(binary_scale_raw: u16, decimal_scale_raw: u16) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&100u32.to_be_bytes()); // num_data_points
        data.extend_from_slice(&0u16.to_be_bytes()); // template 5.0
        data.extend_from_slice(&0.0f32.to_be_bytes()); // reference value
        data.extend_from_slice(&binary_scale_raw.to_be_bytes());
        data.extend_from_slice(&decimal_scale_raw.to_be_bytes());
        data.push(12); // bits per value
        data
    }

    #[test]
    fn test_simple_packing_negative_scale_factors() {
        // E = -3 (0x8003), D = -2 (0x8002).
        let data = simple_packing_bytes(0x8003, 0x8002);
        let drs = DataRepresentationSection::from_bytes(&data)
            .expect("failed to parse DRT 5.0 with negative scale factors");

        assert_eq!(drs.binary_scale_factor, -3);
        assert_eq!(drs.decimal_scale_factor, -2);
        assert!((drs.scale_multiplier() - 0.125).abs() < 1e-6);
        assert!((drs.decimal_divisor() - 0.01).abs() < 1e-6);
    }

    #[test]
    fn test_simple_packing_positive_scale_factors() {
        let data = simple_packing_bytes(0x0003, 0x0002);
        let drs = DataRepresentationSection::from_bytes(&data)
            .expect("failed to parse DRT 5.0 with positive scale factors");

        assert_eq!(drs.binary_scale_factor, 3);
        assert_eq!(drs.decimal_scale_factor, 2);
        assert!((drs.scale_multiplier() - 8.0).abs() < 1e-6);
        assert!((drs.decimal_divisor() - 100.0).abs() < 1e-6);
    }

    /// Builds a minimal DRT 5.2 (complex packing) section buffer.
    fn complex_packing_bytes(binary_scale_raw: u16, decimal_scale_raw: u16) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&100u32.to_be_bytes()); // num_data_points
        data.extend_from_slice(&2u16.to_be_bytes()); // template 5.2
        data.extend_from_slice(&0.0f32.to_be_bytes()); // reference value
        data.extend_from_slice(&binary_scale_raw.to_be_bytes());
        data.extend_from_slice(&decimal_scale_raw.to_be_bytes());
        data.push(12); // bits per value
        data.push(0); // type of original values
        data.push(1); // group splitting method
        data.push(0); // missing value management
        data.extend_from_slice(&0u32.to_be_bytes()); // primary missing substitute
        data.extend_from_slice(&0u32.to_be_bytes()); // secondary missing substitute
        data.extend_from_slice(&1u32.to_be_bytes()); // num groups
        data.push(0); // group widths reference
        data.push(4); // group widths bits
        data.extend_from_slice(&0u32.to_be_bytes()); // group lengths reference
        data.push(1); // group length increment
        data.extend_from_slice(&100u32.to_be_bytes()); // group last length
        data.push(4); // group lengths bits
        data
    }

    #[test]
    fn test_complex_packing_negative_scale_factors() {
        let data = complex_packing_bytes(0x8003, 0x8002);
        let drs = DataRepresentationSection::from_bytes(&data)
            .expect("failed to parse DRT 5.2 with negative scale factors");

        assert_eq!(drs.binary_scale_factor, -3);
        assert_eq!(drs.decimal_scale_factor, -2);
        let complex = drs
            .complex_packing
            .as_ref()
            .expect("complex packing params missing");
        assert_eq!(complex.binary_scale_factor, -3);
        assert_eq!(complex.decimal_scale_factor, -2);
    }

    #[test]
    fn test_read_i16_sign_magnitude() {
        let data = 0x8003u16.to_be_bytes();
        let mut cursor = Cursor::new(&data[..]);
        assert_eq!(
            read_i16_sign_magnitude(&mut cursor).expect("read failed"),
            -3
        );

        let data = 0x0003u16.to_be_bytes();
        let mut cursor = Cursor::new(&data[..]);
        assert_eq!(
            read_i16_sign_magnitude(&mut cursor).expect("read failed"),
            3
        );
    }
}
