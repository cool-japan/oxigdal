//! Tier-2 integration tests for the JPEG2000 crate
//!
//! Covers: progression order iterators, packet decoding, tag trees, ROI maps,
//! RoiShift round-trips, JP2 box parsing, color space extraction, QualityLayer,
//! RateController, and edge cases.

use oxigdal_jpeg2000::codestream::ProgressionOrder;
use oxigdal_jpeg2000::jp2_boxes::{BoxType, ColorSpace, JP2_MAGIC, Jp2Parser};
use oxigdal_jpeg2000::tier2::packet::{BitReader, CodeBlockInclusion, PacketDecoder, TagTree};
use oxigdal_jpeg2000::tier2::progression::{CodeBlockAddress, ProgressionIterator};
use oxigdal_jpeg2000::tier2::rate_control::{QualityLayer, RateController, SlopeEntry};
use oxigdal_jpeg2000::tier2::roi::{RoiMap, RoiShift};

// ============================================================================
// Helpers
// ============================================================================

fn make_box(type_code: u32, payload: &[u8]) -> Vec<u8> {
    let total_len = 8u32 + payload.len() as u32;
    let mut v = Vec::new();
    v.extend_from_slice(&total_len.to_be_bytes());
    v.extend_from_slice(&type_code.to_be_bytes());
    v.extend_from_slice(payload);
    v
}

fn collect_iter(iter: ProgressionIterator) -> Vec<CodeBlockAddress> {
    iter.collect()
}

// ============================================================================
// 1. Progression order iterators
// ============================================================================

#[test]
fn test_prog_lrcp_total_count_simple() {
    // 3 layers × 2 resolutions × 2 components × 1 precinct = 12
    let iter = ProgressionIterator::new(ProgressionOrder::Lrcp, 3, 2, 2, &[1, 1]);
    assert_eq!(iter.total_packets(), 12);
    let items = collect_iter(iter);
    assert_eq!(items.len(), 12);
}

#[test]
fn test_prog_lrcp_outer_is_layer() {
    let iter = ProgressionIterator::new(ProgressionOrder::Lrcp, 3, 1, 1, &[1]);
    let items: Vec<_> = iter.collect();
    // First 1 item: layer=0; next 1: layer=1; final 1: layer=2
    assert_eq!(items[0].layer, 0);
    assert_eq!(items[1].layer, 1);
    assert_eq!(items[2].layer, 2);
}

#[test]
fn test_prog_rlcp_outer_is_resolution() {
    let iter = ProgressionIterator::new(ProgressionOrder::Rlcp, 2, 3, 1, &[1, 1, 1]);
    let items: Vec<_> = iter.collect();
    // First group: resolution=0, then resolution=1, then resolution=2
    assert_eq!(items[0].resolution, 0);
    assert_eq!(items[2].resolution, 1);
    assert_eq!(items[4].resolution, 2);
}

#[test]
fn test_prog_rpcl_outer_is_resolution() {
    let iter = ProgressionIterator::new(ProgressionOrder::Rpcl, 1, 2, 1, &[1, 1]);
    let items: Vec<_> = iter.collect();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].resolution, 0);
    assert_eq!(items[1].resolution, 1);
}

#[test]
fn test_prog_pcrl_outer_is_precinct() {
    let iter = ProgressionIterator::new(ProgressionOrder::Pcrl, 1, 1, 1, &[4]);
    let items: Vec<_> = iter.collect();
    assert_eq!(items.len(), 4);
    assert_eq!(items[0].precinct, 0);
    assert_eq!(items[1].precinct, 1);
    assert_eq!(items[2].precinct, 2);
    assert_eq!(items[3].precinct, 3);
}

#[test]
fn test_prog_cprl_outer_is_component() {
    let iter = ProgressionIterator::new(ProgressionOrder::Cprl, 1, 1, 3, &[1]);
    let items: Vec<_> = iter.collect();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].component, 0);
    assert_eq!(items[1].component, 1);
    assert_eq!(items[2].component, 2);
}

#[test]
fn test_prog_lrcp_all_unique() {
    let iter = ProgressionIterator::new(ProgressionOrder::Lrcp, 2, 2, 2, &[2, 2]);
    let items: Vec<_> = iter.collect();
    // Total = (2+2) * 2 * 2 = 16
    assert_eq!(items.len(), 16);
    // All (layer, resolution, component, precinct) combos must be unique
    let mut seen = std::collections::HashSet::new();
    for item in &items {
        let key = (item.layer, item.resolution, item.component, item.precinct);
        assert!(seen.insert(key), "duplicate: {:?}", key);
    }
}

#[test]
fn test_prog_rlcp_all_unique() {
    let iter = ProgressionIterator::new(ProgressionOrder::Rlcp, 2, 2, 2, &[1, 1]);
    let items: Vec<_> = iter.collect();
    assert_eq!(items.len(), 8);
    let mut seen = std::collections::HashSet::new();
    for item in &items {
        let key = (item.layer, item.resolution, item.component, item.precinct);
        assert!(seen.insert(key));
    }
}

#[test]
fn test_prog_all_five_same_count() {
    let num_packets = |order: ProgressionOrder| -> usize {
        ProgressionIterator::new(order, 2, 3, 2, &[1, 1, 1])
            .collect::<Vec<_>>()
            .len()
    };
    let expected = 2 * 3 * 2; // layers × resolutions × components × 1 precinct
    assert_eq!(num_packets(ProgressionOrder::Lrcp), expected);
    assert_eq!(num_packets(ProgressionOrder::Rlcp), expected);
    assert_eq!(num_packets(ProgressionOrder::Rpcl), expected);
    assert_eq!(num_packets(ProgressionOrder::Pcrl), expected);
    assert_eq!(num_packets(ProgressionOrder::Cprl), expected);
}

#[test]
fn test_prog_zero_components_empty() {
    let iter = ProgressionIterator::new(ProgressionOrder::Lrcp, 3, 3, 0, &[1, 1, 1]);
    assert_eq!(iter.collect::<Vec<_>>().len(), 0);
}

#[test]
fn test_prog_zero_resolutions_empty() {
    let iter = ProgressionIterator::new(ProgressionOrder::Lrcp, 3, 0, 3, &[]);
    assert_eq!(iter.collect::<Vec<_>>().len(), 0);
}

#[test]
fn test_prog_one_of_each() {
    let iter = ProgressionIterator::new(ProgressionOrder::Lrcp, 1, 1, 1, &[1]);
    let items: Vec<_> = iter.collect();
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item.layer, 0);
    assert_eq!(item.resolution, 0);
    assert_eq!(item.component, 0);
    assert_eq!(item.precinct, 0);
}

#[test]
fn test_prog_multi_precinct_per_resolution() {
    // 1 layer, 2 resolutions, 1 component; res0 has 3 precincts, res1 has 5
    let iter = ProgressionIterator::new(ProgressionOrder::Lrcp, 1, 2, 1, &[3, 5]);
    let items: Vec<_> = iter.collect();
    // 3 + 5 = 8
    assert_eq!(items.len(), 8);
    // First 3 from resolution 0
    assert!(items[..3].iter().all(|i| i.resolution == 0));
    // Next 5 from resolution 1
    assert!(items[3..].iter().all(|i| i.resolution == 1));
}

// ============================================================================
// 2. Packet header parsing and BitReader
// ============================================================================

#[test]
fn test_bit_reader_single_byte_msb_first() {
    // 0b10110011 read MSB-first: 1,0,1,1,0,0,1,1
    let data = [0b1011_0011u8];
    let mut br = BitReader::new(&data);
    let bits: Vec<u8> = (0..8).map(|_| br.read_bit().unwrap()).collect();
    assert_eq!(bits, vec![1, 0, 1, 1, 0, 0, 1, 1]);
}

#[test]
fn test_bit_reader_two_bytes() {
    let data = [0xAB, 0xCDu8];
    let mut br = BitReader::new(&data);
    let high = br.read_bits(8).unwrap();
    let low = br.read_bits(8).unwrap();
    assert_eq!(high, 0xAB);
    assert_eq!(low, 0xCD);
}

#[test]
fn test_bit_reader_partial_read() {
    let data = [0b1100_1010u8]; // 0xCA
    let mut br = BitReader::new(&data);
    assert_eq!(br.read_bits(4).unwrap(), 0b1100);
    assert_eq!(br.read_bits(4).unwrap(), 0b1010);
}

#[test]
fn test_bit_reader_overflow_returns_err() {
    let data = [0xFFu8];
    let mut br = BitReader::new(&data);
    // consume all 8 bits
    br.read_bits(8).unwrap();
    // next read must fail
    assert!(br.read_bit().is_err());
}

#[test]
fn test_empty_packet_via_decoder() {
    // First bit = 0 → empty packet
    let data = [0x00u8];
    let mut prev = vec![];
    let (pkt, consumed) = PacketDecoder::decode(&data, 2, 2, &mut prev).unwrap();
    assert!(pkt.header.is_empty);
    assert!(pkt.code_block_data.is_empty());
    assert_eq!(consumed, 1);
    // Inclusions should be created for all 4 code blocks, all marked not included
    assert_eq!(pkt.header.inclusions.len(), 4);
    assert!(pkt.header.inclusions.iter().all(|i| !i.included));
}

#[test]
fn test_code_block_inclusion_fields() {
    let incl = CodeBlockInclusion {
        included: true,
        new_passes: 5,
        data_length: 256,
    };
    assert!(incl.included);
    assert_eq!(incl.new_passes, 5);
    assert_eq!(incl.data_length, 256);
}

#[test]
fn test_packet_decoder_updates_prev_included() {
    // Empty packet → prev_included should remain false
    let data = [0x00u8];
    let mut prev = vec![false; 4];
    PacketDecoder::decode(&data, 2, 2, &mut prev).unwrap();
    assert!(prev.iter().all(|&b| !b));
}

// ============================================================================
// 3. Tag tree
// ============================================================================

#[test]
fn test_tag_tree_levels_1x1() {
    let tt = TagTree::new(1, 1);
    assert_eq!(tt.num_levels(), 1);
}

#[test]
fn test_tag_tree_levels_2x2() {
    let tt = TagTree::new(2, 2);
    // 2x2 → 1x1 root: 2 levels
    assert_eq!(tt.num_levels(), 2);
}

#[test]
fn test_tag_tree_levels_4x4() {
    let tt = TagTree::new(4, 4);
    // 4×4 → 2×2 → 1×1: 3 levels
    assert_eq!(tt.num_levels(), 3);
}

#[test]
fn test_tag_tree_levels_non_square() {
    let tt = TagTree::new(3, 5);
    // 3×5 → 2×3 → 1×2 → 1×1: 4 levels
    assert_eq!(tt.num_levels(), 4);
}

#[test]
fn test_tag_tree_decode_root_threshold_exceeded() {
    // If we feed bits that keep incrementing lower > threshold, should return false
    let mut tt = TagTree::new(1, 1);
    // Feed bit stream that always outputs 0 (never sets value)
    let data = [0x00u8; 16];
    let mut br = BitReader::new(&data);
    // threshold=0: if we read 0 immediately, value > 0 → should return false
    let result = tt.decode_value(0, 0, 0, &mut br);
    // With all-zero bits, the first 0 means we haven't reached threshold=0,
    // then lower increments to 1, so 1 > 0 → false
    assert!(result.is_ok());
}

#[test]
fn test_tag_tree_decode_immediate_inclusion() {
    // Feed a bit stream starting with 1 → value <= threshold (=0) → true
    let data = [0b1000_0000u8]; // first bit is 1
    let mut br = BitReader::new(&data);
    let mut tt = TagTree::new(1, 1);
    let included = tt.decode_value(0, 0, 0, &mut br).unwrap();
    assert!(included);
}

// ============================================================================
// 4. ROI map
// ============================================================================

#[test]
fn test_roi_map_new_empty() {
    let map = RoiMap::new(32, 32, 4);
    assert!(map.is_empty());
    assert_eq!(map.roi_pixel_count(), 0);
    assert_eq!(map.width(), 32);
    assert_eq!(map.height(), 32);
    assert_eq!(map.shift(), 4);
}

#[test]
fn test_roi_map_add_full_rect() {
    let mut map = RoiMap::new(4, 4, 1);
    map.add_rect(0, 0, 4, 4);
    assert_eq!(map.roi_pixel_count(), 16);
    assert!(!map.is_empty());
}

#[test]
fn test_roi_map_add_partial_rect() {
    let mut map = RoiMap::new(8, 8, 2);
    map.add_rect(2, 2, 3, 3);
    assert_eq!(map.roi_pixel_count(), 9);
}

#[test]
fn test_roi_map_rect_clamp_to_bounds() {
    let mut map = RoiMap::new(4, 4, 1);
    map.add_rect(3, 3, 10, 10); // extends far beyond image
    // Only the single pixel (3,3) is valid
    assert_eq!(map.roi_pixel_count(), 1);
}

#[test]
fn test_roi_map_circle_centre() {
    let mut map = RoiMap::new(11, 11, 3);
    map.add_circle(5, 5, 0); // radius 0 = just centre
    assert_eq!(map.roi_pixel_count(), 1);
    assert!(map.mask()[5 * 11 + 5]);
}

#[test]
fn test_roi_map_circle_radius2() {
    let mut map = RoiMap::new(20, 20, 3);
    map.add_circle(10, 10, 2);
    // Count expected: pixels with dx²+dy²<=4
    // (0,0),(±1,0),(0,±1),(±2,0),(0,±2),(±1,±1) = 1+4+4+4 = 13
    assert_eq!(map.roi_pixel_count(), 13);
}

#[test]
fn test_roi_map_block_in_roi_true() {
    let mut map = RoiMap::new(16, 16, 2);
    map.add_rect(4, 4, 8, 8);
    assert!(map.block_in_roi(4, 4, 4, 4)); // fully inside
    assert!(map.block_in_roi(0, 0, 8, 8)); // overlaps
    assert!(map.block_in_roi(10, 10, 4, 4)); // inside
}

#[test]
fn test_roi_map_block_in_roi_false() {
    let mut map = RoiMap::new(16, 16, 2);
    map.add_rect(8, 8, 4, 4);
    assert!(!map.block_in_roi(0, 0, 4, 4));
    assert!(!map.block_in_roi(4, 4, 4, 4));
}

#[test]
fn test_roi_map_block_shift_inside() {
    let mut map = RoiMap::new(8, 8, 6);
    map.add_rect(0, 0, 4, 4);
    assert_eq!(map.block_shift(0, 0, 4, 4), 6);
}

#[test]
fn test_roi_map_block_shift_outside() {
    let mut map = RoiMap::new(8, 8, 6);
    map.add_rect(0, 0, 4, 4);
    assert_eq!(map.block_shift(4, 4, 4, 4), 0);
}

#[test]
fn test_roi_map_overlapping_rects() {
    let mut map = RoiMap::new(8, 8, 1);
    map.add_rect(0, 0, 4, 4);
    map.add_rect(2, 2, 4, 4); // overlaps
    // Overlap area is 2×2=4, each rect covers 4×4=16, union = 16+16-4 = 28
    assert_eq!(map.roi_pixel_count(), 28);
}

#[test]
fn test_roi_map_zero_size_rect() {
    let mut map = RoiMap::new(8, 8, 1);
    map.add_rect(2, 2, 0, 4); // zero width
    assert_eq!(map.roi_pixel_count(), 0);
}

// ============================================================================
// 5. RoiShift upshift / downshift
// ============================================================================

#[test]
fn test_roi_shift_construction() {
    assert!(RoiShift::new(0).is_ok());
    assert!(RoiShift::new(31).is_ok());
    assert!(RoiShift::new(32).is_err());
}

#[test]
fn test_roi_shift_apply_only_roi_pixels() {
    let rs = RoiShift::new(2).unwrap(); // ×4
    let mut coeffs = vec![1i32, 2, 3, 4];
    let mask = vec![true, false, true, false];
    rs.apply_upshift(&mut coeffs, &mask).unwrap();
    assert_eq!(coeffs, vec![4, 2, 12, 4]);
}

#[test]
fn test_roi_shift_remove_only_roi_pixels() {
    let rs = RoiShift::new(2).unwrap();
    let mut coeffs = vec![4i32, 2, 12, 4];
    let mask = vec![true, false, true, false];
    rs.remove_upshift(&mut coeffs, &mask).unwrap();
    assert_eq!(coeffs, vec![1, 2, 3, 4]);
}

#[test]
fn test_roi_shift_full_round_trip() {
    let rs = RoiShift::new(5).unwrap();
    let original = vec![7i32, -3, 0, 100, -50];
    let mask = vec![true, true, false, true, false];
    let mut coeffs = original.clone();
    rs.apply_upshift(&mut coeffs, &mask).unwrap();
    rs.remove_upshift(&mut coeffs, &mask).unwrap();
    assert_eq!(coeffs, original);
}

#[test]
fn test_roi_shift_zero_shift_noop() {
    let rs = RoiShift::new(0).unwrap();
    let mut coeffs = vec![100i32, -50, 0];
    let original = coeffs.clone();
    let mask = vec![true, true, true];
    rs.apply_upshift(&mut coeffs, &mask).unwrap();
    assert_eq!(coeffs, original);
    rs.remove_upshift(&mut coeffs, &mask).unwrap();
    assert_eq!(coeffs, original);
}

#[test]
fn test_roi_shift_negative_value_round_trip() {
    let rs = RoiShift::new(3).unwrap();
    let mut coeffs = vec![-8i32, 8];
    let mask = vec![true, true];
    rs.apply_upshift(&mut coeffs, &mask).unwrap();
    assert_eq!(coeffs, vec![-64, 64]);
    rs.remove_upshift(&mut coeffs, &mask).unwrap();
    assert_eq!(coeffs, vec![-8, 8]);
}

#[test]
fn test_roi_shift_length_mismatch_error() {
    let rs = RoiShift::new(1).unwrap();
    let mut coeffs = vec![1i32, 2, 3];
    let mask = vec![true]; // wrong length
    assert!(rs.apply_upshift(&mut coeffs, &mask).is_err());
    assert!(rs.remove_upshift(&mut coeffs, &mask).is_err());
}

// ============================================================================
// 6. JP2 box parsing
// ============================================================================

#[test]
fn test_jp2_validate_signature() {
    assert!(Jp2Parser::validate_signature(&JP2_MAGIC));
    assert!(!Jp2Parser::validate_signature(&[0u8; 12]));
    assert!(!Jp2Parser::validate_signature(&[0xFFu8])); // too short
}

#[test]
fn test_jp2_parse_empty() {
    let boxes = Jp2Parser::parse(&[]).unwrap();
    assert!(boxes.is_empty());
}

#[test]
fn test_jp2_parse_ftyp_box() {
    let payload = b"jp2 \x00\x00\x00\x00jp2 ";
    let data = make_box(0x66747970, payload);
    let boxes = Jp2Parser::parse(&data).unwrap();
    assert_eq!(boxes.len(), 1);
    assert_eq!(boxes[0].box_type, BoxType::FileType);
    assert_eq!(&boxes[0].data, payload.as_ref());
}

#[test]
fn test_jp2_parse_xml_box() {
    let xml = b"<metadata><item>hello</item></metadata>";
    let data = make_box(0x786D6C20, xml);
    let boxes = Jp2Parser::parse(&data).unwrap();
    assert_eq!(boxes.len(), 1);
    assert_eq!(boxes[0].box_type, BoxType::Xml);
    assert_eq!(&boxes[0].data, xml.as_ref());
}

#[test]
fn test_jp2_find_codestream() {
    let codestream = vec![0xFF, 0x4F, 0xFF, 0xD9u8];
    let data = make_box(0x6A703263, &codestream);
    let boxes = Jp2Parser::parse(&data).unwrap();
    let cs_box = Jp2Parser::find_codestream(&boxes).unwrap();
    assert_eq!(cs_box.box_type, BoxType::ContiguousCodestream);
    assert_eq!(cs_box.data, codestream);
}

#[test]
fn test_jp2_find_codestream_absent() {
    let data = make_box(0x66747970, b"jp2 ");
    let boxes = Jp2Parser::parse(&data).unwrap();
    assert!(Jp2Parser::find_codestream(&boxes).is_none());
}

#[test]
fn test_jp2_color_space_srgb() {
    // colr: method=1, precedence=0, approx=0, enumCS=16
    let colr = vec![0x01u8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10];
    let data = make_box(0x636F6C72, &colr);
    let boxes = Jp2Parser::parse(&data).unwrap();
    assert_eq!(
        Jp2Parser::extract_color_space(&boxes),
        Some(ColorSpace::SRgb)
    );
}

#[test]
fn test_jp2_color_space_grayscale() {
    let colr = vec![0x01u8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x11];
    let data = make_box(0x636F6C72, &colr);
    let boxes = Jp2Parser::parse(&data).unwrap();
    assert_eq!(
        Jp2Parser::extract_color_space(&boxes),
        Some(ColorSpace::Grayscale)
    );
}

#[test]
fn test_jp2_color_space_ycbcr() {
    let colr = vec![0x01u8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x12];
    let data = make_box(0x636F6C72, &colr);
    let boxes = Jp2Parser::parse(&data).unwrap();
    assert_eq!(
        Jp2Parser::extract_color_space(&boxes),
        Some(ColorSpace::YCbCr)
    );
}

#[test]
fn test_jp2_color_space_icc() {
    let icc_data = vec![0xDE, 0xAD, 0xBE, 0xEF];
    let mut colr = vec![0x02u8, 0x00, 0x00]; // method=2
    colr.extend_from_slice(&icc_data);
    let data = make_box(0x636F6C72, &colr);
    let boxes = Jp2Parser::parse(&data).unwrap();
    match Jp2Parser::extract_color_space(&boxes) {
        Some(ColorSpace::Icc(profile)) => assert_eq!(profile, icc_data),
        other => panic!("expected ICC, got {:?}", other),
    }
}

#[test]
fn test_jp2_box_offset_and_length() {
    let payload = b"xyz";
    let data = make_box(0x786D6C20, payload);
    let boxes = Jp2Parser::parse(&data).unwrap();
    assert_eq!(boxes[0].offset, 0);
    assert_eq!(boxes[0].length, 8 + 3); // header + 3 bytes
    assert_eq!(boxes[0].payload_len(), 3);
}

#[test]
fn test_jp2_multiple_boxes_sequential() {
    let mut data = Vec::new();
    data.extend(make_box(0x66747970, b"jp2 ")); // ftyp
    data.extend(make_box(0x786D6C20, b"<x/>")); // xml
    data.extend(make_box(0x6A703263, &[0xFF, 0x4F, 0xFF, 0xD9])); // jp2c

    let boxes = Jp2Parser::parse(&data).unwrap();
    assert_eq!(boxes.len(), 3);
    assert_eq!(boxes[0].box_type, BoxType::FileType);
    assert_eq!(boxes[1].box_type, BoxType::Xml);
    assert_eq!(boxes[2].box_type, BoxType::ContiguousCodestream);
}

#[test]
fn test_jp2_unknown_box_preserved() {
    let data = make_box(0x12345678, b"custom");
    let boxes = Jp2Parser::parse(&data).unwrap();
    assert_eq!(boxes.len(), 1);
    assert_eq!(boxes[0].box_type, BoxType::Unknown(0x12345678));
    assert_eq!(&boxes[0].data, b"custom".as_ref());
}

// ============================================================================
// 7. QualityLayer and RateController
// ============================================================================

#[test]
fn test_quality_layer_with_rate() {
    let layer = QualityLayer::with_rate(0, 2.5);
    assert_eq!(layer.layer_index, 0);
    assert_eq!(layer.target_rate, Some(2.5));
    assert!(layer.target_psnr.is_none());
    assert!(!layer.is_lossless());
}

#[test]
fn test_quality_layer_with_psnr() {
    let layer = QualityLayer::with_psnr(2, 45.0);
    assert_eq!(layer.layer_index, 2);
    assert_eq!(layer.target_psnr, Some(45.0));
    assert!(layer.target_rate.is_none());
    assert!(!layer.is_lossless());
}

#[test]
fn test_quality_layer_lossless() {
    let layer = QualityLayer::lossless(1);
    assert!(layer.is_lossless());
}

#[test]
fn test_rate_controller_new() {
    let rc = RateController::new(128, 128, 3);
    assert_eq!(rc.width, 128);
    assert_eq!(rc.height, 128);
    assert_eq!(rc.num_components, 3);
    assert_eq!(rc.num_layers(), 0);
}

#[test]
fn test_rate_controller_add_layers_ascending() {
    let mut rc = RateController::new(64, 64, 1);
    rc.add_layer(QualityLayer::with_rate(0, 0.5)).unwrap();
    rc.add_layer(QualityLayer::with_rate(1, 1.0)).unwrap();
    rc.add_layer(QualityLayer::lossless(2)).unwrap();
    assert_eq!(rc.num_layers(), 3);
}

#[test]
fn test_rate_controller_add_layers_same_index_fails() {
    let mut rc = RateController::new(64, 64, 1);
    rc.add_layer(QualityLayer::with_rate(0, 1.0)).unwrap();
    assert!(rc.add_layer(QualityLayer::with_rate(0, 2.0)).is_err());
}

#[test]
fn test_rate_controller_layer_byte_budget_1bpp() {
    let mut rc = RateController::new(256, 256, 1);
    rc.add_layer(QualityLayer::with_rate(0, 1.0)).unwrap();
    // 256*256*1 * 1 bpp / 8 = 8192 bytes
    assert_eq!(rc.layer_byte_budget(0), Some(8192));
}

#[test]
fn test_rate_controller_layer_byte_budget_rgb() {
    let mut rc = RateController::new(64, 64, 3);
    rc.add_layer(QualityLayer::with_rate(0, 8.0)).unwrap(); // 8 bpp
    // 64*64*3 * 8 / 8 = 12288
    assert_eq!(rc.layer_byte_budget(0), Some(12288));
}

#[test]
fn test_rate_controller_layer_budget_lossless_none() {
    let mut rc = RateController::new(64, 64, 1);
    rc.add_layer(QualityLayer::lossless(0)).unwrap();
    assert_eq!(rc.layer_byte_budget(0), None);
}

#[test]
fn test_rate_controller_layer_budget_out_of_range() {
    let rc = RateController::new(64, 64, 1);
    assert_eq!(rc.layer_byte_budget(99), None);
}

#[test]
fn test_slope_entry_slope_calculation() {
    let se = SlopeEntry {
        distortion_reduction: 200.0,
        byte_cost: 50,
    };
    assert!((se.slope() - 4.0).abs() < 1e-9);
}

#[test]
fn test_slope_entry_zero_cost_infinity() {
    let se = SlopeEntry {
        distortion_reduction: 10.0,
        byte_cost: 0,
    };
    assert_eq!(se.slope(), f64::INFINITY);
}

#[test]
fn test_rate_controller_allocate_passes_empty_slopes() {
    let mut rc = RateController::new(64, 64, 1);
    rc.add_layer(QualityLayer::lossless(0)).unwrap();
    let result = rc.allocate_passes(&[]).unwrap();
    assert!(result.is_empty());
}

#[test]
fn test_rate_controller_allocate_passes_no_layers_error() {
    let rc = RateController::new(64, 64, 1);
    let slopes = vec![SlopeEntry {
        distortion_reduction: 1.0,
        byte_cost: 10,
    }];
    assert!(rc.allocate_passes(&slopes).is_err());
}

#[test]
fn test_rate_controller_allocate_passes_returns_correct_count() {
    let mut rc = RateController::new(64, 64, 1);
    rc.add_layer(QualityLayer::with_rate(0, 1.0)).unwrap();
    rc.add_layer(QualityLayer::lossless(1)).unwrap();
    let slopes = vec![
        SlopeEntry {
            distortion_reduction: 10.0,
            byte_cost: 5,
        },
        SlopeEntry {
            distortion_reduction: 20.0,
            byte_cost: 5,
        },
        SlopeEntry {
            distortion_reduction: 30.0,
            byte_cost: 5,
        },
    ];
    let assignments = rc.allocate_passes(&slopes).unwrap();
    assert_eq!(assignments.len(), 3);
    // All assignments must be valid layer indices (0 or 1)
    assert!(assignments.iter().all(|&a| a <= 1));
}

#[test]
fn test_rate_controller_total_pixels() {
    let rc = RateController::new(50, 80, 4);
    assert_eq!(rc.total_pixels(), 50 * 80 * 4);
}
