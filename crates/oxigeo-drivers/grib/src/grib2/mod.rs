//! GRIB Edition 2 format support.
//!
//! This module provides parsing and data extraction for GRIB2 format files.
//! GRIB2 uses a template-based system with sections for identification, grid definition,
//! product definition, data representation, and data.

pub mod decoder;
#[cfg(feature = "jpeg2000")]
pub mod jpeg2000;
pub mod section1;
pub mod section3;
pub mod section4;
pub mod section5;
pub mod section7;

use crate::error::{GribError, Result};
use crate::grid::GridDefinition;
use crate::message::SectionHeader;
use crate::parameter::{LevelType, Parameter, lookup_grib2_parameter};

/// Returns `&data[start..]`, validating that `start` does not exceed
/// `data.len()`.
///
/// GRIB2 section lengths are attacker/corruption-controlled input read
/// from a file or stream. Slicing `data[start..]` directly panics if
/// `start > data.len()`; this helper surfaces a typed error instead.
fn checked_tail(data: &[u8], start: usize) -> Result<&[u8]> {
    if start > data.len() {
        return Err(GribError::TruncatedMessage {
            expected: start,
            actual: data.len(),
        });
    }
    Ok(&data[start..])
}

/// Returns `&data[start..start + len]`, validating both bounds against
/// `data.len()` first (including the addition itself, to avoid a `usize`
/// overflow wrapping the range around to a small, spuriously "valid" one).
fn checked_slice(data: &[u8], start: usize, len: usize) -> Result<&[u8]> {
    let end = start.saturating_add(len);
    if end > data.len() {
        return Err(GribError::TruncatedMessage {
            expected: end,
            actual: data.len(),
        });
    }
    Ok(&data[start..end])
}

/// Advances `offset` by `delta`, validating the result stays within
/// `data.len()` (and does not overflow `usize`).
fn checked_advance(data: &[u8], offset: usize, delta: usize) -> Result<usize> {
    let new_offset = offset.saturating_add(delta);
    if new_offset > data.len() {
        return Err(GribError::TruncatedMessage {
            expected: new_offset,
            actual: data.len(),
        });
    }
    Ok(new_offset)
}

pub use decoder::Grib2Decoder;
pub use decoder::{
    BitReader, ComplexPackingParams, SpatialDiffParams, decode_complex_packing,
    decode_complex_with_spatial_diff,
};
pub use section1::IdentificationSection;
pub use section3::GridDefinitionSection;
pub use section4::{EnsembleInfo, ProductDefinitionSection, StatisticalProcessInfo, TimeRangeSpec};
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
        let section1_header = SectionHeader::from_bytes(checked_tail(data, offset)?)?;
        offset = checked_advance(data, offset, 5)?;
        let section1_data = checked_slice(data, offset, section1_header.payload_len(5)?)?;
        let identification = IdentificationSection::from_bytes(section1_data)?;
        offset = checked_advance(data, offset, section1_data.len())?;

        // Section 2: Local Use (optional, skip for now)
        if offset < data.len() {
            let peek_header = SectionHeader::from_bytes(checked_tail(data, offset)?)?;
            if peek_header.number as u8 == 2 {
                offset = checked_advance(data, offset, peek_header.length as usize)?;
            }
        }

        // Section 3: Grid Definition
        let section3_header = SectionHeader::from_bytes(checked_tail(data, offset)?)?;
        offset = checked_advance(data, offset, 5)?;
        let section3_data = checked_slice(data, offset, section3_header.payload_len(5)?)?;
        let grid_definition = GridDefinitionSection::from_bytes(section3_data)?;
        offset = checked_advance(data, offset, section3_data.len())?;

        // Section 4: Product Definition
        let section4_header = SectionHeader::from_bytes(checked_tail(data, offset)?)?;
        offset = checked_advance(data, offset, 5)?;
        let section4_data = checked_slice(data, offset, section4_header.payload_len(5)?)?;
        let product_definition = ProductDefinitionSection::from_bytes(section4_data)?;
        offset = checked_advance(data, offset, section4_data.len())?;

        // Section 5: Data Representation
        let section5_header = SectionHeader::from_bytes(checked_tail(data, offset)?)?;
        offset = checked_advance(data, offset, 5)?;
        let section5_data = checked_slice(data, offset, section5_header.payload_len(5)?)?;
        let data_representation = DataRepresentationSection::from_bytes(section5_data)?;
        offset = checked_advance(data, offset, section5_data.len())?;

        // Section 6: Bitmap (optional)
        let bitmap = if offset < data.len() {
            let section6_header = SectionHeader::from_bytes(checked_tail(data, offset)?)?;
            if section6_header.number as u8 == 6 {
                offset = checked_advance(data, offset, 5)?;
                let bitmap_indicator =
                    *data
                        .get(offset)
                        .ok_or_else(|| GribError::TruncatedMessage {
                            expected: offset + 1,
                            actual: data.len(),
                        })?;
                offset = checked_advance(data, offset, 1)?;

                if bitmap_indicator == 0 {
                    // Bitmap present
                    let bitmap_bytes = section6_header.payload_len(6)?;
                    let bitmap_data = checked_slice(data, offset, bitmap_bytes)?;
                    let mut bm = Vec::with_capacity(bitmap_bytes.saturating_mul(8));
                    for &byte in bitmap_data {
                        for bit in 0..8 {
                            bm.push((byte & (1 << (7 - bit))) != 0);
                        }
                    }
                    offset = checked_advance(data, offset, bitmap_bytes)?;
                    Some(bm)
                } else {
                    let skip_len = section6_header.payload_len(6)?;
                    offset = checked_advance(data, offset, skip_len)?;
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // Section 7: Data
        let section7_header = SectionHeader::from_bytes(checked_tail(data, offset)?)?;
        offset = checked_advance(data, offset, 5)?;
        let section7_data = checked_slice(data, offset, section7_header.payload_len(5)?)?;
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

    /// Returns `true` if this message carries a statistically-processed field
    /// accumulated/averaged/etc. over a time interval (PDT 4.8) rather than an
    /// instantaneous value (PDT 4.0/4.1).
    pub fn is_time_interval(&self) -> bool {
        self.product_definition.is_time_interval()
    }

    /// End of the overall statistical time interval, for time-interval
    /// products (PDT 4.8). `None` for instantaneous fields.
    pub fn interval_end_time(&self) -> Option<chrono::NaiveDateTime> {
        self.product_definition
            .statistical_process
            .as_ref()
            .and_then(|s| s.interval_end_time())
    }

    /// Get valid time.
    ///
    /// For an instantaneous field (PDT 4.0/4.1) this is the reference time
    /// advanced by the forecast offset. For a statistically-processed field
    /// over a time interval (PDT 4.8) the valid time is the *end* of the
    /// overall time interval, so accumulated/averaged quantities are not
    /// mis-reported at the start of their accumulation window.
    pub fn valid_time(&self) -> Option<chrono::NaiveDateTime> {
        if let Some(end) = self.interval_end_time() {
            return Some(end);
        }
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
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    /// Wraps a section payload with its 5-octet GRIB2 header (length + number).
    fn framed_section(number: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(payload.len() + 5);
        out.extend_from_slice(&((payload.len() + 5) as u32).to_be_bytes());
        out.push(number);
        out.extend_from_slice(payload);
        out
    }

    /// Builds a full GRIB2 message body (sections 1, 3, 4, given section 5, and
    /// a section 7 payload) with `num_points` grid points, so the decoder can
    /// be exercised end-to-end on a chosen Data Representation Template.
    fn message_with_drt(
        num_points: u32,
        section5_payload: &[u8],
        section7_payload: &[u8],
    ) -> Vec<u8> {
        let mut data = Vec::new();

        // Section 1: Identification.
        let mut s1 = Vec::new();
        s1.extend_from_slice(&0u16.to_be_bytes()); // center
        s1.extend_from_slice(&0u16.to_be_bytes()); // subcenter
        s1.push(2); // master table version
        s1.push(0); // local table version
        s1.push(1); // significance of reference time
        s1.extend_from_slice(&2024u16.to_be_bytes()); // year
        s1.push(1); // month
        s1.push(1); // day
        s1.push(0); // hour
        s1.push(0); // minute
        s1.push(0); // second
        s1.push(0); // production status
        s1.push(0); // type of data
        data.extend_from_slice(&framed_section(1, &s1));

        // Section 3: Grid Definition (GDT 3.0, all-zero body is fine; the grid
        // point count comes from the section's num_points field).
        let mut s3 = Vec::new();
        s3.push(0); // source
        s3.extend_from_slice(&num_points.to_be_bytes());
        s3.push(0);
        s3.push(0);
        s3.extend_from_slice(&0u16.to_be_bytes()); // template 3.0
        s3.extend_from_slice(&[0u8; 58]); // regular lat/lon body
        data.extend_from_slice(&framed_section(3, &s3));

        // Section 4: Product Definition (PDT 4.0).
        let mut s4 = Vec::new();
        s4.extend_from_slice(&0u16.to_be_bytes()); // num_coordinates
        s4.extend_from_slice(&0u16.to_be_bytes()); // template 4.0
        s4.extend_from_slice(&[0u8; 25]); // PDT 4.0 body (first + second surface)
        data.extend_from_slice(&framed_section(4, &s4));

        // Section 5: caller-supplied.
        data.extend_from_slice(&framed_section(5, section5_payload));

        // Section 7: caller-supplied packed/encoded data.
        data.extend_from_slice(&framed_section(7, section7_payload));

        data
    }

    /// DRT 5.4 IEEE 32-bit floating-point data must round-trip the raw
    /// big-endian f32 values in Section 7 unchanged (no R/E/D scaling).
    #[test]
    fn test_decode_ieee_float32_roundtrip() {
        let mut s5 = Vec::new();
        s5.extend_from_slice(&3u32.to_be_bytes()); // num_data_points
        s5.extend_from_slice(&4u16.to_be_bytes()); // template 5.4
        s5.push(1); // precision code: IEEE 32-bit

        let values = [1.5f32, -273.15, 42.0];
        let mut s7 = Vec::new();
        for v in values {
            s7.extend_from_slice(&v.to_be_bytes());
        }

        let msg = message_with_drt(3, &s5, &s7);
        let parsed = Grib2Message::from_bytes(&msg, 0).expect("parse IEEE message");
        let decoded = parsed.decode_data().expect("decode IEEE data");
        assert_eq!(decoded.len(), 3);
        for (got, want) in decoded.iter().zip(values.iter()) {
            assert!((got - want).abs() < 1e-6, "got {got}, want {want}");
        }
    }

    /// DRT 5.40 (JPEG2000): a codestream that is not a valid J2K stream must
    /// return a typed decoding error — never a silent simple-packing
    /// mis-unpack producing plausible-but-wrong values.
    #[cfg(feature = "jpeg2000")]
    #[test]
    fn test_decode_jpeg2000_invalid_codestream_errors() {
        let mut s5 = Vec::new();
        s5.extend_from_slice(&4u32.to_be_bytes()); // num_data_points
        s5.extend_from_slice(&40u16.to_be_bytes()); // template 5.40
        s5.extend_from_slice(&0.0f32.to_be_bytes()); // reference value
        s5.extend_from_slice(&0u16.to_be_bytes()); // binary scale
        s5.extend_from_slice(&0u16.to_be_bytes()); // decimal scale
        s5.push(16); // bits per value

        // Section 7: bytes that are NOT a JPEG2000 codestream.
        let s7 = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x11, 0x22, 0x33];

        let msg = message_with_drt(4, &s5, &s7);
        let parsed = Grib2Message::from_bytes(&msg, 0).expect("parse 5.40 message");
        let result = parsed.decode_data();
        assert!(
            result.is_err(),
            "invalid JPEG2000 codestream must error, not silently mis-unpack"
        );
    }

    /// DRT 5.41 (PNG) is not implemented and must fail with a typed error
    /// rather than mis-unpacking the compressed payload as simple packing.
    #[test]
    fn test_decode_png_template_errors() {
        let mut s5 = Vec::new();
        s5.extend_from_slice(&4u32.to_be_bytes()); // num_data_points
        s5.extend_from_slice(&41u16.to_be_bytes()); // template 5.41
        s5.extend_from_slice(&0.0f32.to_be_bytes()); // reference value
        s5.extend_from_slice(&0u16.to_be_bytes()); // binary scale
        s5.extend_from_slice(&0u16.to_be_bytes()); // decimal scale
        s5.push(8); // bits per value

        let s7 = vec![0x89, b'P', b'N', b'G', 0, 0, 0, 0];
        let msg = message_with_drt(4, &s5, &s7);
        let parsed = Grib2Message::from_bytes(&msg, 0).expect("parse 5.41 message");
        assert!(matches!(
            parsed.decode_data(),
            Err(GribError::UnsupportedPacking(_))
        ));
    }

    #[test]
    fn test_level_type() {
        assert_eq!(LevelType::from_grib2_code(1), LevelType::Surface);
        assert_eq!(LevelType::from_grib2_code(100), LevelType::Isobaric);
        assert_eq!(
            LevelType::from_grib2_code(103),
            LevelType::HeightAboveGround
        );
    }

    /// Regression test: a crafted Section 1 header whose `length` field is
    /// smaller than the 5-octet header itself must return a typed error
    /// instead of underflowing the `usize` subtraction and panicking on the
    /// subsequent slice operation.
    #[test]
    fn test_from_bytes_section1_length_underflow_returns_error() {
        for length in 0u32..5 {
            let mut data = Vec::new();
            data.extend_from_slice(&length.to_be_bytes());
            data.push(1); // section number 1
            let result = Grib2Message::from_bytes(&data, 0);
            assert!(
                result.is_err(),
                "section length {length} < 5 must error, not panic"
            );
        }
    }

    /// Regression test: a section header claiming a length that exceeds the
    /// remaining buffer must return a typed error instead of panicking on
    /// an out-of-bounds slice.
    #[test]
    fn test_from_bytes_section1_length_exceeds_buffer_returns_error() {
        let mut data = Vec::new();
        data.extend_from_slice(&1_000_000u32.to_be_bytes()); // implausibly large length
        data.push(1); // section number 1
        let result = Grib2Message::from_bytes(&data, 0);
        assert!(result.is_err());
    }

    /// Builds a minimal, structurally valid GRIB2 message body (sections
    /// 1, 3, 4, 5) followed by a caller-supplied Section 6 header/payload
    /// and a trailing Section 7, so the Section 6 bitmap path can be
    /// exercised end-to-end.
    fn minimal_message_through_section5() -> Vec<u8> {
        let mut data = Vec::new();

        // Section 1: Identification (16-octet payload + 5-octet header).
        let mut section1 = Vec::new();
        section1.extend_from_slice(&0u16.to_be_bytes()); // center_id
        section1.extend_from_slice(&0u16.to_be_bytes()); // subcenter_id
        section1.push(2); // master_table_version
        section1.push(0); // local_table_version
        section1.push(1); // significance_of_reference_time
        section1.extend_from_slice(&2024u16.to_be_bytes()); // year
        section1.push(1); // month
        section1.push(1); // day
        section1.push(0); // hour
        section1.push(0); // minute
        section1.push(0); // second
        section1.push(0); // production_status
        section1.push(0); // type_of_data
        data.extend_from_slice(&((section1.len() + 5) as u32).to_be_bytes());
        data.push(1); // section number
        data.extend_from_slice(&section1);

        // Section 3: Grid Definition (unsupported template is fine here --
        // this helper is only used by tests that fail before reaching it).
        let mut section3 = Vec::new();
        section3.push(0); // source
        section3.extend_from_slice(&1u32.to_be_bytes()); // num_points
        section3.push(0);
        section3.push(0);
        section3.extend_from_slice(&0u16.to_be_bytes()); // template 3.0
        // Regular lat/lon body (minimal, all zero): shape(1) +
        // scale/scaled radius/major/minor(3*(1+4)) + ni/nj(8) +
        // basic_angle/subdivisions(8) + la1/lo1(8) + resolution(1) +
        // la2/lo2(8) + di/dj(8) + scan_flags(1) = 58 octets.
        section3.extend_from_slice(&[0u8; 58]);
        data.extend_from_slice(&((section3.len() + 5) as u32).to_be_bytes());
        data.push(3);
        data.extend_from_slice(&section3);

        // Section 4: Product Definition (PDT 4.0, minimal).
        let mut section4 = Vec::new();
        section4.extend_from_slice(&0u16.to_be_bytes()); // num_coordinates
        section4.extend_from_slice(&0u16.to_be_bytes()); // template 4.0
        // Fixed-size PDT 4.0 body: category/number/process/background/
        // analysis(5) + hours_cutoff(2) + minutes_cutoff(1) + time_unit(1)
        // + forecast_time(4) + first surface [type(1) + scale(1) + value(4)]
        // + second surface [type(1) + scale(1) + value(4)] = 25 octets.
        section4.extend_from_slice(&[0u8; 25]);
        data.extend_from_slice(&((section4.len() + 5) as u32).to_be_bytes());
        data.push(4);
        data.extend_from_slice(&section4);

        // Section 5: Data Representation (DRT 5.0, minimal).
        let mut section5 = Vec::new();
        section5.extend_from_slice(&1u32.to_be_bytes()); // num_data_points
        section5.extend_from_slice(&0u16.to_be_bytes()); // template 5.0
        section5.extend_from_slice(&0.0f32.to_be_bytes()); // reference value
        section5.extend_from_slice(&0u16.to_be_bytes()); // binary scale
        section5.extend_from_slice(&0u16.to_be_bytes()); // decimal scale
        section5.push(8); // bits per value
        data.extend_from_slice(&((section5.len() + 5) as u32).to_be_bytes());
        data.push(5);
        data.extend_from_slice(&section5);

        data
    }

    /// Regression test: a Section 6 header whose `length` field is smaller
    /// than the 6-octet bitmap-section header must return a typed error
    /// instead of underflowing the `usize` subtraction (which previously
    /// risked both a slice panic and an oversized `Vec::with_capacity`
    /// allocation).
    #[test]
    fn test_from_bytes_section6_length_underflow_returns_error() {
        for length in 0u32..6 {
            let mut data = minimal_message_through_section5();
            data.extend_from_slice(&length.to_be_bytes());
            data.push(6); // section number 6
            data.push(0); // bitmap indicator: bitmap present
            let result = Grib2Message::from_bytes(&data, 0);
            assert!(
                result.is_err(),
                "section 6 length {length} < 6 must error, not panic"
            );
        }
    }

    /// Regression test: a Section 6 bitmap that claims more payload bytes
    /// than actually remain in the buffer must return a typed error
    /// instead of panicking on an out-of-bounds slice/index.
    #[test]
    fn test_from_bytes_section6_bitmap_exceeds_buffer_returns_error() {
        let mut data = minimal_message_through_section5();
        data.extend_from_slice(&1_000_000u32.to_be_bytes()); // implausibly large length
        data.push(6); // section number 6
        data.push(0); // bitmap indicator: bitmap present
        let result = Grib2Message::from_bytes(&data, 0);
        assert!(result.is_err());
    }
}
