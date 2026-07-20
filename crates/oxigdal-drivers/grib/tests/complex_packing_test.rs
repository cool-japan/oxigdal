//! Hand-built synthetic fixtures for GRIB2 complex packing (DRT 5.2 / 5.3).
//!
//! Every fixture is an in-memory byte array assembled bit by bit, with the
//! expected `f32` output computed by hand in the accompanying comment. This
//! is the only reliable way to catch bit-packing off-by-one errors, so the
//! tests double as executable documentation of the WMO Template 5.2 / 5.3
//! layout (WMO Manual on Codes Vol. I.2).

use oxigdal_grib::grib2::decoder::{
    BitReader, ComplexPackingParams, SpatialDiffParams, decode_complex_packing,
    decode_complex_with_spatial_diff,
};
use oxigdal_grib::grib2::section5::DataRepresentationSection;

// ---------------------------------------------------------------------------
// Bit-packing helper for building fixtures.
// ---------------------------------------------------------------------------

/// Minimal MSB-first bit writer mirroring the GRIB convention. Used only to
/// construct test fixtures; it is the inverse of the production `BitReader`.
struct BitWriter {
    bytes: Vec<u8>,
    bit_pos: usize,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            bit_pos: 0,
        }
    }

    /// Appends the low `n` bits of `value`, most-significant bit first.
    fn write_bits(&mut self, value: u64, n: u32) {
        for i in (0..n).rev() {
            let bit = ((value >> i) & 1) as u8;
            let byte_idx = self.bit_pos / 8;
            if byte_idx >= self.bytes.len() {
                self.bytes.push(0);
            }
            let bit_in_byte = 7 - (self.bit_pos % 8);
            self.bytes[byte_idx] |= bit << bit_in_byte;
            self.bit_pos += 1;
        }
    }

    /// Pads with zero bits up to the next byte boundary.
    fn align(&mut self) {
        while !self.bit_pos.is_multiple_of(8) {
            self.write_bits(0, 1);
        }
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

/// Builds default complex-packing parameters; tests override what they need.
fn base_params() -> ComplexPackingParams {
    ComplexPackingParams {
        reference_value: 0.0,
        binary_scale_factor: 0,
        decimal_scale_factor: 0,
        bits_per_value: 8,
        num_groups: 1,
        group_widths_reference: 0,
        group_widths_bits: 8,
        group_lengths_reference: 0,
        group_length_increment: 1,
        group_last_length: 1,
        group_lengths_bits: 8,
        missing_value_management: 0,
        primary_missing_substitute: 0,
        secondary_missing_substitute: 0,
    }
}

// ===========================================================================
// 1. BitReader: MSB-first ordering.
// ===========================================================================

#[test]
fn test_bit_reader_msb_first() {
    // Byte 0b1010_0110 = 0xA6. Reading 8 bits MSB-first must reproduce 0xA6.
    // Reading it as 1 + 3 + 4 bits: 1 -> 0b1, 3 -> 0b010, 4 -> 0b0110.
    let bytes = [0xA6u8];
    let mut reader = BitReader::new(&bytes);
    assert_eq!(reader.read_bits(8).expect("8-bit read in bounds"), 0xA6);

    let mut reader = BitReader::new(&bytes);
    assert_eq!(reader.read_bits(1).expect("1-bit read in bounds"), 0b1); // 1
    assert_eq!(reader.read_bits(3).expect("3-bit read in bounds"), 0b010); // 2
    assert_eq!(reader.read_bits(4).expect("4-bit read in bounds"), 0b0110); // 6

    // Reading 0 bits is always Ok(0) and does not advance.
    let mut reader = BitReader::new(&bytes);
    assert_eq!(reader.read_bits(0).expect("0-bit read is always Ok"), 0);
    assert_eq!(reader.bit_position(), 0);
}

// ===========================================================================
// 2. BitReader: overrun returns Err, never panics.
// ===========================================================================

#[test]
fn test_bit_reader_overrun_errors() {
    let bytes = [0xFFu8]; // exactly 8 bits available
    let mut reader = BitReader::new(&bytes);
    assert_eq!(reader.read_bits(8).expect("8-bit read in bounds"), 0xFF);
    // The 9th bit does not exist -> Err, not a panic.
    assert!(reader.read_bits(1).is_err());

    // A single oversized request also errors cleanly.
    let mut reader = BitReader::new(&bytes);
    assert!(reader.read_bits(9).is_err());

    // Requesting more than 64 bits is rejected.
    let wide = [0u8; 16];
    let mut reader = BitReader::new(&wide);
    assert!(reader.read_bits(65).is_err());
}

// ===========================================================================
// 3. BitReader: a value straddling a byte boundary.
// ===========================================================================

#[test]
fn test_bit_reader_spanning_byte_boundary() {
    // Two bytes: 0b0000_1111, 0b0000_0000 = 0x0F, 0x00.
    // Skip 4 bits, then read 8 bits. The window covers the low nibble of
    // byte 0 (0b1111) and the high nibble of byte 1 (0b0000):
    //   value = 0b1111_0000 = 0xF0 = 240.
    let bytes = [0x0Fu8, 0x00u8];
    let mut reader = BitReader::new(&bytes);
    assert_eq!(reader.read_bits(4).expect("4-bit read in bounds"), 0b0000);
    assert_eq!(
        reader
            .read_bits(8)
            .expect("8-bit boundary-spanning read in bounds"),
        0xF0
    );

    // Cross-check: 0b1010_1100, 0b1100_0011 = 0xAC, 0xC3.
    // Skip 6 bits -> at low 2 bits of byte 0 (0b00); read 6 bits ->
    //   0b00 ++ top 4 bits of byte 1 (0b1100) = 0b00_1100 = 12.
    let bytes = [0xACu8, 0xC3u8];
    let mut reader = BitReader::new(&bytes);
    let _ = reader.read_bits(6).expect("first 6-bit read in bounds");
    assert_eq!(
        reader
            .read_bits(6)
            .expect("second 6-bit boundary-spanning read in bounds"),
        0b001100
    );
}

// ===========================================================================
// 4. DRT 5.2: parse a synthetic Section 5 octet block.
// ===========================================================================

#[test]
fn test_drt52_parse_complex_params() {
    // Build a Section-5 body (the bytes AFTER the 5-octet section header,
    // i.e. starting at octet 6: num_data_points). Layout per Template 5.2:
    //   oct  6- 9: num_data_points     = 10
    //   oct 10-11: template_number     = 2
    //   oct 12-15: reference value R   = 1.5f32
    //   oct 16-17: binary scale E      = 3
    //   oct 18-19: decimal scale D     = 1
    //   oct 20   : bits_per_value      = 9
    //   oct 21   : type of original    = 0
    //   oct 22   : group splitting     = 1
    //   oct 23   : missing value mgmt  = 0
    //   oct 24-27: primary missing     = 0
    //   oct 28-31: secondary missing   = 0
    //   oct 32-35: NG number of groups = 4
    //   oct 36   : group widths ref    = 2
    //   oct 37   : group widths bits   = 5
    //   oct 38-41: group lengths ref   = 7
    //   oct 42   : length increment    = 1
    //   oct 43-46: last group length   = 3
    //   oct 47   : group lengths bits  = 6
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(&10u32.to_be_bytes()); // num_data_points
    body.extend_from_slice(&2u16.to_be_bytes()); // template
    body.extend_from_slice(&1.5f32.to_be_bytes()); // R
    body.extend_from_slice(&3i16.to_be_bytes()); // E
    body.extend_from_slice(&1i16.to_be_bytes()); // D
    body.push(9); // bits_per_value
    body.push(0); // type of original
    body.push(1); // group splitting method
    body.push(0); // missing value management
    body.extend_from_slice(&0u32.to_be_bytes()); // primary missing
    body.extend_from_slice(&0u32.to_be_bytes()); // secondary missing
    body.extend_from_slice(&4u32.to_be_bytes()); // NG
    body.push(2); // group widths reference
    body.push(5); // group widths bits
    body.extend_from_slice(&7u32.to_be_bytes()); // group lengths reference
    body.push(1); // length increment
    body.extend_from_slice(&3u32.to_be_bytes()); // last group length
    body.push(6); // group lengths bits

    let section = DataRepresentationSection::from_bytes(&body).expect("synthetic Section 5 parses");
    assert_eq!(section.num_data_points, 10);
    assert_eq!(section.template_number, 2);
    // Flat simple-packing fields must be mirrored from the complex block.
    assert_eq!(section.reference_value, 1.5);
    assert_eq!(section.binary_scale_factor, 3);
    assert_eq!(section.decimal_scale_factor, 1);
    assert_eq!(section.bits_per_value, 9);
    assert!(section.spatial_diff.is_none());

    let cp = section.complex_packing.expect("complex params present");
    assert_eq!(cp.reference_value, 1.5);
    assert_eq!(cp.binary_scale_factor, 3);
    assert_eq!(cp.decimal_scale_factor, 1);
    assert_eq!(cp.bits_per_value, 9);
    assert_eq!(cp.num_groups, 4);
    assert_eq!(cp.group_widths_reference, 2);
    assert_eq!(cp.group_widths_bits, 5);
    assert_eq!(cp.group_lengths_reference, 7);
    assert_eq!(cp.group_length_increment, 1);
    assert_eq!(cp.group_last_length, 3);
    assert_eq!(cp.group_lengths_bits, 6);
    assert_eq!(cp.missing_value_management, 0);
}

// ===========================================================================
// 5. DRT 5.2: uniform group widths.
// ===========================================================================

#[test]
fn test_drt52_uniform_group_widths() {
    // Two groups, each 3 values wide, every member packed in 4 bits.
    //   group 0: reference = 10, residuals 1, 2, 3 -> X = 11, 12, 13
    //   group 1: reference = 20, residuals 0, 5, 7 -> X = 20, 25, 27
    // R = 0, E = 0, D = 0  -> value == X.
    //
    // Section-7 layout:
    //   references @ bits_per_value(8): [10, 20]            -> align
    //   widths     @ group_widths_bits(8): true width 4 each,
    //              group_widths_reference = 0, so scaled = 4 -> [4, 4] -> align
    //   lengths    @ group_lengths_bits(8): non-last from
    //              ref(0) + inc(1)*scaled; group 0 length 3 -> scaled 3.
    //              last group length is group_last_length = 3, scaled ignored
    //              (write 3 anyway).                         -> [3, 3] -> align
    //   residuals  @ width 4: 1,2,3 then 0,5,7.
    let mut params = base_params();
    params.bits_per_value = 8;
    params.num_groups = 2;
    params.group_widths_reference = 0;
    params.group_widths_bits = 8;
    params.group_lengths_reference = 0;
    params.group_length_increment = 1;
    params.group_last_length = 3;
    params.group_lengths_bits = 8;

    let mut w = BitWriter::new();
    // references
    w.write_bits(10, 8);
    w.write_bits(20, 8);
    w.align();
    // widths (scaled, reference 0)
    w.write_bits(4, 8);
    w.write_bits(4, 8);
    w.align();
    // lengths (scaled)
    w.write_bits(3, 8);
    w.write_bits(3, 8);
    w.align();
    // residuals group 0
    w.write_bits(1, 4);
    w.write_bits(2, 4);
    w.write_bits(3, 4);
    // residuals group 1
    w.write_bits(0, 4);
    w.write_bits(5, 4);
    w.write_bits(7, 4);
    let data = w.finish();

    let out = decode_complex_packing(&data, &params, 6).expect("synthetic DRT 5.2 data decodes");
    assert_eq!(out, vec![11.0, 12.0, 13.0, 20.0, 25.0, 27.0]);
}

// ===========================================================================
// 6. DRT 5.2: variable group widths.
// ===========================================================================

#[test]
fn test_drt52_variable_group_widths() {
    // Three groups of differing widths.
    //   group 0: ref 100, width 5, len 2, residuals 1, 30 -> X = 101, 130
    //   group 1: ref 200, width 3, len 2, residuals 7, 0  -> X = 207, 200
    //   group 2: ref 50,  width 10, len 1, residual 1000  -> X = 1050
    // R = 0, E = 0, D = 0 -> value == X.
    //
    //   references @ 8 bits: [100, 200, 50]                 -> align
    //   widths     @ 8 bits (reference 0): [5, 3, 10]       -> align
    //   lengths    @ 8 bits: non-last = ref(0)+inc(1)*scaled,
    //              group 0 -> scaled 2, group 1 -> scaled 2,
    //              last group length = group_last_length = 1 -> [2, 2, 1] -> align
    //   residuals: g0 @5: 1, 30 ; g1 @3: 7, 0 ; g2 @10: 1000.
    let mut params = base_params();
    params.bits_per_value = 8;
    params.num_groups = 3;
    params.group_widths_reference = 0;
    params.group_widths_bits = 8;
    params.group_lengths_reference = 0;
    params.group_length_increment = 1;
    params.group_last_length = 1;
    params.group_lengths_bits = 8;

    let mut w = BitWriter::new();
    w.write_bits(100, 8);
    w.write_bits(200, 8);
    w.write_bits(50, 8);
    w.align();
    w.write_bits(5, 8);
    w.write_bits(3, 8);
    w.write_bits(10, 8);
    w.align();
    w.write_bits(2, 8);
    w.write_bits(2, 8);
    w.write_bits(1, 8);
    w.align();
    // residuals
    w.write_bits(1, 5);
    w.write_bits(30, 5);
    w.write_bits(7, 3);
    w.write_bits(0, 3);
    w.write_bits(1000, 10);
    let data = w.finish();

    let out = decode_complex_packing(&data, &params, 5).expect("synthetic DRT 5.2 data decodes");
    assert_eq!(out, vec![101.0, 130.0, 207.0, 200.0, 1050.0]);
}

// ===========================================================================
// 7. DRT 5.2: a width-0 group yields all values == group reference.
// ===========================================================================

#[test]
fn test_drt52_zero_width_group_all_equal_reference() {
    // Two groups.
    //   group 0: ref 42, width 0, len 4 -> all members == 42 (no residuals)
    //   group 1: ref 7,  width 4, len 2, residuals 1, 2 -> X = 8, 9
    // R = 0, E = 0, D = 0 -> value == X.
    //
    //   references @ 8 bits: [42, 7]                        -> align
    //   widths     @ 8 bits (reference 0): [0, 4]           -> align
    //   lengths    @ 8 bits: group 0 scaled 4, last group
    //              length = group_last_length = 2           -> [4, 2] -> align
    //   residuals: group 0 has width 0 -> NONE written;
    //              group 1 @4: 1, 2.
    let mut params = base_params();
    params.bits_per_value = 8;
    params.num_groups = 2;
    params.group_widths_reference = 0;
    params.group_widths_bits = 8;
    params.group_lengths_reference = 0;
    params.group_length_increment = 1;
    params.group_last_length = 2;
    params.group_lengths_bits = 8;

    let mut w = BitWriter::new();
    w.write_bits(42, 8);
    w.write_bits(7, 8);
    w.align();
    w.write_bits(0, 8); // width 0
    w.write_bits(4, 8);
    w.align();
    w.write_bits(4, 8);
    w.write_bits(2, 8);
    w.align();
    // only group 1 residuals
    w.write_bits(1, 4);
    w.write_bits(2, 4);
    let data = w.finish();

    let out = decode_complex_packing(&data, &params, 6).expect("synthetic DRT 5.2 data decodes");
    assert_eq!(out, vec![42.0, 42.0, 42.0, 42.0, 8.0, 9.0]);
}

// ===========================================================================
// 8. DRT 5.2: E and D scaling applied.
// ===========================================================================

#[test]
fn test_drt52_e_d_scaling_applied() {
    // One group, 3 values @ 6 bits, reference 4.
    //   residuals 0, 8, 20 -> X = 4, 12, 24.
    // R = 2.0, E = 2 (2^2 = 4), D = 1 (10^1 = 10).
    //   value = (R + X * 2^E) / 10^D = (2 + X*4) / 10
    //   X=4  -> (2 + 16)/10 = 18/10 = 1.8
    //   X=12 -> (2 + 48)/10 = 50/10 = 5.0
    //   X=24 -> (2 + 96)/10 = 98/10 = 9.8
    //
    //   references @ 8 bits: [4]                            -> align
    //   widths     @ 8 bits (reference 0): [6]              -> align
    //   lengths    @ 8 bits: single group is the last group,
    //              length = group_last_length = 3           -> [3] -> align
    //   residuals @6: 0, 8, 20.
    let mut params = base_params();
    params.reference_value = 2.0;
    params.binary_scale_factor = 2;
    params.decimal_scale_factor = 1;
    params.bits_per_value = 8;
    params.num_groups = 1;
    params.group_widths_reference = 0;
    params.group_widths_bits = 8;
    params.group_lengths_bits = 8;
    params.group_last_length = 3;

    let mut w = BitWriter::new();
    w.write_bits(4, 8);
    w.align();
    w.write_bits(6, 8);
    w.align();
    w.write_bits(3, 8);
    w.align();
    w.write_bits(0, 6);
    w.write_bits(8, 6);
    w.write_bits(20, 6);
    let data = w.finish();

    let out = decode_complex_packing(&data, &params, 3).expect("synthetic DRT 5.2 data decodes");
    assert_eq!(out.len(), 3);
    assert!((out[0] - 1.8).abs() < 1e-5, "out[0] = {}", out[0]);
    assert!((out[1] - 5.0).abs() < 1e-5, "out[1] = {}", out[1]);
    assert!((out[2] - 9.8).abs() < 1e-5, "out[2] = {}", out[2]);
}

// ===========================================================================
// 9. DRT 5.2: simplest single-group round trip.
// ===========================================================================

#[test]
fn test_drt52_single_group_round_trip() {
    // One group, 5 values @ 12 bits, reference 1000.
    //   residuals 0, 1, 100, 2000, 4095 -> X = 1000, 1001, 1100, 3000, 5095.
    // R = 0, E = 0, D = 0 -> value == X.
    let mut params = base_params();
    params.bits_per_value = 16;
    params.num_groups = 1;
    params.group_widths_reference = 0;
    params.group_widths_bits = 8;
    params.group_lengths_bits = 8;
    params.group_last_length = 5;

    let residuals: [u64; 5] = [0, 1, 100, 2000, 4095];
    let mut w = BitWriter::new();
    w.write_bits(1000, 16); // reference
    w.align();
    w.write_bits(12, 8); // width
    w.align();
    w.write_bits(5, 8); // length
    w.align();
    for &r in &residuals {
        w.write_bits(r, 12);
    }
    let data = w.finish();

    let out = decode_complex_packing(&data, &params, 5).expect("synthetic DRT 5.2 data decodes");
    assert_eq!(out, vec![1000.0, 1001.0, 1100.0, 3000.0, 5095.0]);
}

// ===========================================================================
// 10. DRT 5.3: parse order + extra_octets from a synthetic Section 5.
// ===========================================================================

#[test]
fn test_drt53_parse_spatial_diff_params() {
    // Template 5.3 body = the 5.2 block followed by:
    //   oct 48: order of spatial differencing = 2
    //   oct 49: number of extra octets        = 2
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(&20u32.to_be_bytes()); // num_data_points
    body.extend_from_slice(&3u16.to_be_bytes()); // template = 3
    body.extend_from_slice(&0.0f32.to_be_bytes()); // R
    body.extend_from_slice(&0i16.to_be_bytes()); // E
    body.extend_from_slice(&0i16.to_be_bytes()); // D
    body.push(8); // bits_per_value
    body.push(0); // type of original
    body.push(1); // group splitting
    body.push(0); // missing value mgmt
    body.extend_from_slice(&0u32.to_be_bytes()); // primary missing
    body.extend_from_slice(&0u32.to_be_bytes()); // secondary missing
    body.extend_from_slice(&2u32.to_be_bytes()); // NG
    body.push(0); // group widths ref
    body.push(8); // group widths bits
    body.extend_from_slice(&0u32.to_be_bytes()); // group lengths ref
    body.push(1); // length increment
    body.extend_from_slice(&5u32.to_be_bytes()); // last group length
    body.push(8); // group lengths bits
    body.push(2); // spatial diff order
    body.push(2); // extra octets

    let section = DataRepresentationSection::from_bytes(&body).expect("synthetic Section 5 parses");
    assert_eq!(section.template_number, 3);
    assert!(section.complex_packing.is_some());
    let sd = section.spatial_diff.expect("spatial diff params present");
    assert_eq!(sd.order, 2);
    assert_eq!(sd.extra_octets, 2);
}

// ===========================================================================
// 11. DRT 5.3: first-order inverse spatial differencing.
// ===========================================================================

#[test]
fn test_drt53_first_order_spatial_diff() {
    // Order-1 reconstruction: v[0] = initial; v[i] = d[i] + v[i-1].
    //
    // Target series we want to reconstruct: v = [5, 8, 6, 11, 11].
    //   raw differences  delta[i] = v[i] - v[i-1] (i>=1):
    //     delta[1] = 3, delta[2] = -2, delta[3] = 5, delta[4] = 0.
    //   overall minimum of the differences = -2.
    //   stored (non-negative) differences  d_stored[i] = delta[i] - min:
    //     position 0 is the initial slot; we store 0 there.
    //     d_stored = [0, 5, 0, 7, 2].
    //   Adding the minimum back: d = d_stored + (-2) = [-2, 3, -2, 5, 0].
    //   Inverse: v[0]=initial(5); v[1]=3+5=8; v[2]=-2+8=6;
    //            v[3]=5+6=11; v[4]=0+11=11.  -> [5, 8, 6, 11, 11]. OK.
    // R = 0, E = 0, D = 0 -> value == v.
    //
    // extra_octets = 2 -> each descriptor is 16 bits sign-magnitude.
    //   descriptor 0 (initial value)   = 5   -> 0x0005
    //   descriptor 1 (overall minimum) = -2  -> sign bit + magnitude 2
    //                                          = 0x8002
    // After the descriptors: align, then the 5.2 group block.
    //   One group, 5 values @ width 4 (max stored d is 7 -> fits in 4 bits),
    //   group reference 0.
    //   references @ 8 bits: [0]                 -> align
    //   widths     @ 8 bits (reference 0): [4]   -> align
    //   lengths    @ 8 bits: [5] (last group)    -> align
    //   residuals  @ 4 bits: 0, 5, 0, 7, 2.
    let mut params = base_params();
    params.bits_per_value = 8;
    params.num_groups = 1;
    params.group_widths_reference = 0;
    params.group_widths_bits = 8;
    params.group_lengths_bits = 8;
    params.group_last_length = 5;

    let sd = SpatialDiffParams {
        order: 1,
        extra_octets: 2,
    };

    let mut w = BitWriter::new();
    // extra descriptors: initial = 5, overall minimum = -2 (sign-magnitude).
    w.write_bits(5, 16); // 0x0005
    w.write_bits((1u64 << 15) | 2, 16); // 0x8002 = -2
    w.align();
    // group block
    w.write_bits(0, 8); // reference
    w.align();
    w.write_bits(4, 8); // width
    w.align();
    w.write_bits(5, 8); // length
    w.align();
    // residuals = stored differences
    for d in [0u64, 5, 0, 7, 2] {
        w.write_bits(d, 4);
    }
    let data = w.finish();

    let out = decode_complex_with_spatial_diff(&data, &params, &sd, 5)
        .expect("synthetic DRT 5.3 data decodes");
    assert_eq!(out, vec![5.0, 8.0, 6.0, 11.0, 11.0]);
}

// ===========================================================================
// 12. DRT 5.3: second-order inverse spatial differencing.
// ===========================================================================

#[test]
fn test_drt53_second_order_spatial_diff() {
    // Order-2 reconstruction:
    //   v[0], v[1] = the two initial values;
    //   v[i] = d[i] + 2*v[i-1] - v[i-2].
    //
    // Target series: v = [3, 5, 10, 20, 33].
    //   second differences dd[i] = v[i] - 2*v[i-1] + v[i-2] (i>=2):
    //     dd[2] = 10 - 2*5 + 3   = 3
    //     dd[3] = 20 - 2*10 + 5  = 5
    //     dd[4] = 33 - 2*20 + 10 = 3
    //   overall minimum of the second differences = 3.
    //   stored differences  d_stored[i] = dd[i] - min:
    //     positions 0 and 1 are the initial slots; store 0 there.
    //     d_stored = [0, 0, 0, 2, 0].
    //   Adding the minimum back: d = d_stored + 3 = [3, 3, 3, 5, 3].
    //   Inverse:
    //     v[0] = initial0 = 3
    //     v[1] = initial1 = 5
    //     v[2] = d[2] + 2*v[1] - v[0] = 3 + 10 - 3 = 10
    //     v[3] = d[3] + 2*v[2] - v[1] = 5 + 20 - 5 = 20
    //     v[4] = d[4] + 2*v[3] - v[2] = 3 + 40 - 10 = 33
    //   -> [3, 5, 10, 20, 33]. OK.
    // R = 0, E = 0, D = 0 -> value == v.
    //
    // extra_octets = 1 -> each descriptor is 8 bits sign-magnitude.
    //   descriptor 0 (initial v[0]) = 3 -> 0x03
    //   descriptor 1 (initial v[1]) = 5 -> 0x05
    //   descriptor 2 (overall min)  = 3 -> 0x03
    // Then align, then the 5.2 group block:
    //   one group, 5 values @ width 2 (max stored d is 2 -> needs 2 bits),
    //   group reference 0.
    //   references @ 8 bits: [0]                 -> align
    //   widths     @ 8 bits (reference 0): [2]   -> align
    //   lengths    @ 8 bits: [5] (last group)    -> align
    //   residuals  @ 2 bits: 0, 0, 0, 2, 0.
    let mut params = base_params();
    params.bits_per_value = 8;
    params.num_groups = 1;
    params.group_widths_reference = 0;
    params.group_widths_bits = 8;
    params.group_lengths_bits = 8;
    params.group_last_length = 5;

    let sd = SpatialDiffParams {
        order: 2,
        extra_octets: 1,
    };

    let mut w = BitWriter::new();
    // extra descriptors: v0 = 3, v1 = 5, overall minimum = 3.
    w.write_bits(3, 8);
    w.write_bits(5, 8);
    w.write_bits(3, 8);
    w.align();
    // group block
    w.write_bits(0, 8); // reference
    w.align();
    w.write_bits(2, 8); // width
    w.align();
    w.write_bits(5, 8); // length
    w.align();
    // residuals = stored differences
    for d in [0u64, 0, 0, 2, 0] {
        w.write_bits(d, 2);
    }
    let data = w.finish();

    let out = decode_complex_with_spatial_diff(&data, &params, &sd, 5)
        .expect("synthetic DRT 5.3 data decodes");
    assert_eq!(out, vec![3.0, 5.0, 10.0, 20.0, 33.0]);
}
