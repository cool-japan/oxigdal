//! Comprehensive tests for the GeoTIFF color space transformation module.

use oxigdal_geotiff::color_space::{
    CieLabConverter, CmykConverter, ColorSpace, ColorSpaceConverter, ColorSpaceSig, DeviceClass,
    IccProfile, Pixel, YCbCrConverter,
};

// ============================================================================
// ColorSpace::from_photometric
// ============================================================================

#[test]
fn test_from_photometric_white_is_zero() {
    let cs = ColorSpace::from_photometric(0).expect("should parse 0");
    assert_eq!(cs, ColorSpace::WhiteIsZero);
}

#[test]
fn test_from_photometric_black_is_zero() {
    let cs = ColorSpace::from_photometric(1).expect("should parse 1");
    assert_eq!(cs, ColorSpace::BlackIsZero);
}

#[test]
fn test_from_photometric_rgb() {
    let cs = ColorSpace::from_photometric(2).expect("should parse 2");
    assert_eq!(cs, ColorSpace::Rgb);
}

#[test]
fn test_from_photometric_palette() {
    let cs = ColorSpace::from_photometric(3).expect("should parse 3");
    assert_eq!(cs, ColorSpace::Palette);
}

#[test]
fn test_from_photometric_cmyk() {
    let cs = ColorSpace::from_photometric(5).expect("should parse 5");
    assert_eq!(cs, ColorSpace::Cmyk);
}

#[test]
fn test_from_photometric_ycbcr() {
    let cs = ColorSpace::from_photometric(6).expect("should parse 6");
    assert_eq!(cs, ColorSpace::YCbCr);
}

#[test]
fn test_from_photometric_cie_lab() {
    let cs = ColorSpace::from_photometric(8).expect("should parse 8");
    assert_eq!(cs, ColorSpace::CieLab);
}

#[test]
fn test_from_photometric_icc_lab() {
    let cs = ColorSpace::from_photometric(9).expect("should parse 9");
    assert_eq!(cs, ColorSpace::IccLab);
}

#[test]
fn test_from_photometric_itu_lab() {
    let cs = ColorSpace::from_photometric(10).expect("should parse 10");
    assert_eq!(cs, ColorSpace::ItuLab);
}

#[test]
fn test_from_photometric_unknown_errors() {
    assert!(ColorSpace::from_photometric(4).is_err());
    assert!(ColorSpace::from_photometric(7).is_err());
    assert!(ColorSpace::from_photometric(11).is_err());
    assert!(ColorSpace::from_photometric(999).is_err());
}

// ============================================================================
// ColorSpace::to_photometric (round-trip)
// ============================================================================

#[test]
fn test_to_photometric_round_trip() {
    let values = [0u16, 1, 2, 3, 5, 6, 8, 9, 10];
    for v in values {
        let cs = ColorSpace::from_photometric(v).expect("valid");
        assert_eq!(cs.to_photometric(), v, "round-trip failed for tag {v}");
    }
}

// ============================================================================
// ColorSpace::channel_count
// ============================================================================

#[test]
fn test_channel_count_white_is_zero() {
    assert_eq!(ColorSpace::WhiteIsZero.channel_count(), 1);
}

#[test]
fn test_channel_count_black_is_zero() {
    assert_eq!(ColorSpace::BlackIsZero.channel_count(), 1);
}

#[test]
fn test_channel_count_rgb() {
    assert_eq!(ColorSpace::Rgb.channel_count(), 3);
}

#[test]
fn test_channel_count_palette() {
    assert_eq!(ColorSpace::Palette.channel_count(), 1);
}

#[test]
fn test_channel_count_cmyk() {
    assert_eq!(ColorSpace::Cmyk.channel_count(), 4);
}

#[test]
fn test_channel_count_ycbcr() {
    assert_eq!(ColorSpace::YCbCr.channel_count(), 3);
}

#[test]
fn test_channel_count_lab_variants() {
    assert_eq!(ColorSpace::CieLab.channel_count(), 3);
    assert_eq!(ColorSpace::IccLab.channel_count(), 3);
    assert_eq!(ColorSpace::ItuLab.channel_count(), 3);
}

// ============================================================================
// ColorSpace::is_grayscale
// ============================================================================

#[test]
fn test_is_grayscale() {
    assert!(ColorSpace::WhiteIsZero.is_grayscale());
    assert!(ColorSpace::BlackIsZero.is_grayscale());
    assert!(!ColorSpace::Rgb.is_grayscale());
    assert!(!ColorSpace::Cmyk.is_grayscale());
    assert!(!ColorSpace::YCbCr.is_grayscale());
    assert!(!ColorSpace::CieLab.is_grayscale());
}

// ============================================================================
// YCbCrConverter
// ============================================================================

#[test]
fn test_ycbcr_rec601_neutral_gray() {
    // Y=128, Cb=128, Cr=128 should produce near-gray (128,128,128).
    let conv = YCbCrConverter::rec601();
    // Normalise: 128/255 ≈ 0.502
    let n = 128.0_f32 / 255.0;
    let [r, g, b] = conv.to_rgb(n, n, n);
    let ri = (r * 255.0).round() as i32;
    let gi = (g * 255.0).round() as i32;
    let bi = (b * 255.0).round() as i32;
    // All channels should be within ±2 of 128.
    assert!((ri - 128).abs() <= 2, "R={ri} not near 128");
    assert!((gi - 128).abs() <= 2, "G={gi} not near 128");
    assert!((bi - 128).abs() <= 2, "B={bi} not near 128");
}

#[test]
fn test_ycbcr_rec601_red_pixel() {
    // Y=76, Cb=85, Cr=255 is approximately a red pixel.
    let conv = YCbCrConverter::rec601();
    let yn = 76.0_f32 / 255.0;
    let cbn = 85.0_f32 / 255.0;
    let crn = 255.0_f32 / 255.0;
    let [r, g, b] = conv.to_rgb(yn, cbn, crn);
    // Red should be dominant.
    assert!(r > g, "expected R > G, got r={r} g={g}");
    assert!(r > b, "expected R > B, got r={r} b={b}");
}

#[test]
fn test_ycbcr_from_rgb_to_rgb_round_trip() {
    let conv = YCbCrConverter::rec601();
    let original = (0.8_f32, 0.4_f32, 0.2_f32);
    let ycbcr = conv.from_rgb(original.0, original.1, original.2);
    let [r, g, b] = conv.to_rgb(ycbcr[0], ycbcr[1], ycbcr[2]);
    let tol = 0.01_f32;
    assert!(
        (r - original.0).abs() < tol,
        "R round-trip: {r} vs {}",
        original.0
    );
    assert!(
        (g - original.1).abs() < tol,
        "G round-trip: {g} vs {}",
        original.1
    );
    assert!(
        (b - original.2).abs() < tol,
        "B round-trip: {b} vs {}",
        original.2
    );
}

#[test]
fn test_ycbcr_buffer_to_rgb_length_preserved() {
    let conv = YCbCrConverter::rec601();
    // 4 pixels × 3 bytes = 12 bytes in, 12 bytes out.
    let input: Vec<u8> = vec![128, 128, 128, 76, 85, 255, 29, 255, 107, 149, 43, 21];
    let out = conv.buffer_to_rgb(&input);
    assert_eq!(
        out.len(),
        input.len(),
        "output length must equal input length"
    );
}

#[test]
fn test_ycbcr_buffer_to_rgb_partial_pixels_dropped() {
    let conv = YCbCrConverter::rec601();
    // 7 bytes — only 2 complete pixels (6 bytes) are processed.
    let input = vec![128u8, 128, 128, 76, 85, 255, 99];
    let out = conv.buffer_to_rgb(&input);
    assert_eq!(out.len(), 6, "7-byte input should produce 6-byte output");
}

#[test]
fn test_ycbcr_rec709_has_different_luma_coefficients() {
    let rec601 = YCbCrConverter::rec601();
    let rec709 = YCbCrConverter::rec709();
    assert!(
        (rec601.luma_coefficients[0] - rec709.luma_coefficients[0]).abs() > 0.01,
        "Rec. 601 and Rec. 709 luma coefficients should differ"
    );
}

#[test]
fn test_ycbcr_from_tiff_tags_defaults_to_rec601() {
    let conv = YCbCrConverter::from_tiff_tags(None, None);
    let rec601 = YCbCrConverter::rec601();
    assert_eq!(conv.luma_coefficients, rec601.luma_coefficients);
    assert_eq!(conv.reference_black_white, rec601.reference_black_white);
}

#[test]
fn test_ycbcr_rgb_to_buffer_length_preserved() {
    let conv = YCbCrConverter::rec601();
    let rgb = vec![255u8, 0, 0, 0, 255, 0, 0, 0, 255];
    let out = conv.rgb_to_buffer(&rgb);
    assert_eq!(out.len(), rgb.len());
}

// ============================================================================
// CmykConverter
// ============================================================================

#[test]
fn test_cmyk_white_is_zero_ink() {
    // (0,0,0,0) → white (1.0,1.0,1.0)
    let conv = CmykConverter::new();
    let [r, g, b] = conv.to_rgb(0.0, 0.0, 0.0, 0.0);
    assert!((r - 1.0).abs() < 0.001, "R should be ~1.0, got {r}");
    assert!((g - 1.0).abs() < 0.001, "G should be ~1.0, got {g}");
    assert!((b - 1.0).abs() < 0.001, "B should be ~1.0, got {b}");
}

#[test]
fn test_cmyk_full_black_key() {
    // (0,0,0,1.0) → black (0,0,0)
    let conv = CmykConverter::new();
    let [r, g, b] = conv.to_rgb(0.0, 0.0, 0.0, 1.0);
    assert!((r).abs() < 0.001, "R should be ~0, got {r}");
    assert!((g).abs() < 0.001, "G should be ~0, got {g}");
    assert!((b).abs() < 0.001, "B should be ~0, got {b}");
}

#[test]
fn test_cmyk_full_cyan_channel() {
    // (1,0,0,0) → cyan → RGB (0,1,1)
    let conv = CmykConverter::new();
    let [r, g, b] = conv.to_rgb(1.0, 0.0, 0.0, 0.0);
    assert!(r < 0.01, "R should be ~0, got {r}");
    assert!((g - 1.0).abs() < 0.001, "G should be ~1, got {g}");
    assert!((b - 1.0).abs() < 0.001, "B should be ~1, got {b}");
}

#[test]
fn test_cmyk_from_rgb_round_trip() {
    let conv = CmykConverter::new();
    let original = (0.6_f32, 0.3_f32, 0.9_f32);
    let cmyk = conv.from_rgb(original.0, original.1, original.2);
    let [r, g, b] = conv.to_rgb(cmyk[0], cmyk[1], cmyk[2], cmyk[3]);
    let tol = 0.001_f32;
    assert!((r - original.0).abs() < tol, "R: {r} vs {}", original.0);
    assert!((g - original.1).abs() < tol, "G: {g} vs {}", original.1);
    assert!((b - original.2).abs() < tol, "B: {b} vs {}", original.2);
}

#[test]
fn test_cmyk_pure_black_from_rgb() {
    let conv = CmykConverter::new();
    let cmyk = conv.from_rgb(0.0, 0.0, 0.0);
    assert!((cmyk[3] - 1.0).abs() < 0.001, "K should be 1.0 for black");
    assert!(cmyk[0].abs() < 0.001);
    assert!(cmyk[1].abs() < 0.001);
    assert!(cmyk[2].abs() < 0.001);
}

#[test]
fn test_cmyk_buffer_to_rgb_length() {
    let conv = CmykConverter::new();
    // 2 pixels, 4 bytes each = 8 bytes in; 2 pixels, 3 bytes each = 6 bytes out.
    let input = vec![0u8, 0, 0, 0, 255, 0, 0, 0];
    let out = conv.buffer_to_rgb(&input);
    assert_eq!(out.len(), 6, "2 CMYK pixels → 6 RGB bytes");
}

#[test]
fn test_cmyk_buffer_to_rgb_white_pixel() {
    let conv = CmykConverter::new();
    let input = vec![0u8, 0, 0, 0]; // C=0,M=0,Y=0,K=0 → white
    let out = conv.buffer_to_rgb(&input);
    assert_eq!(out[0], 255);
    assert_eq!(out[1], 255);
    assert_eq!(out[2], 255);
}

// ============================================================================
// CieLabConverter
// ============================================================================

#[test]
fn test_cie_lab_d65_white() {
    // L*=100, a*=0, b*=0 → white (1,1,1).
    let conv = CieLabConverter::d65();
    let [r, g, b] = conv.lab_to_rgb(100.0, 0.0, 0.0);
    assert!(
        (r - 1.0).abs() < 0.01,
        "R should be ~1.0 for L*=100, got {r}"
    );
    assert!(
        (g - 1.0).abs() < 0.01,
        "G should be ~1.0 for L*=100, got {g}"
    );
    assert!(
        (b - 1.0).abs() < 0.01,
        "B should be ~1.0 for L*=100, got {b}"
    );
}

#[test]
fn test_cie_lab_d65_black() {
    // L*=0, a*=0, b*=0 → black (0,0,0).
    let conv = CieLabConverter::d65();
    let [r, g, b] = conv.lab_to_rgb(0.0, 0.0, 0.0);
    assert!(r < 0.01, "R should be ~0 for black, got {r}");
    assert!(g < 0.01, "G should be ~0 for black, got {g}");
    assert!(b < 0.01, "B should be ~0 for black, got {b}");
}

#[test]
fn test_cie_lab_rgb_to_lab_white() {
    let conv = CieLabConverter::d65();
    let [l, a, b] = conv.rgb_to_lab(1.0, 1.0, 1.0);
    assert!(
        (l - 100.0).abs() < 0.5,
        "L* should be ~100 for white, got {l}"
    );
    assert!(a.abs() < 1.0, "a* should be ~0 for white, got {a}");
    assert!(b.abs() < 1.0, "b* should be ~0 for white, got {b}");
}

#[test]
fn test_cie_lab_rgb_to_lab_black() {
    let conv = CieLabConverter::d65();
    let [l, a, b] = conv.rgb_to_lab(0.0, 0.0, 0.0);
    assert!(l.abs() < 0.5, "L* should be ~0 for black, got {l}");
    assert!(a.abs() < 1.0, "a* should be ~0 for black, got {a}");
    assert!(b.abs() < 1.0, "b* should be ~0 for black, got {b}");
}

#[test]
fn test_cie_lab_round_trip_midtone() {
    let conv = CieLabConverter::d65();
    let original = (0.5_f32, 0.3_f32, 0.7_f32);
    let [l, a, b_val] = conv.rgb_to_lab(original.0, original.1, original.2);
    let [r, g, b] = conv.lab_to_rgb(l, a, b_val);
    let tol = 0.02_f32;
    assert!(
        (r - original.0).abs() < tol,
        "R round-trip: {r} vs {}",
        original.0
    );
    assert!(
        (g - original.1).abs() < tol,
        "G round-trip: {g} vs {}",
        original.1
    );
    assert!(
        (b - original.2).abs() < tol,
        "B round-trip: {b} vs {}",
        original.2
    );
}

#[test]
fn test_cie_lab_d50_different_white_point_than_d65() {
    let d65 = CieLabConverter::d65();
    let d50 = CieLabConverter::d50();
    // White points must differ between illuminants.
    assert!(
        (d65.white_x - d50.white_x).abs() > 0.001,
        "D65 and D50 X white points should differ"
    );
    assert!(
        (d65.white_z - d50.white_z).abs() > 0.001,
        "D65 and D50 Z white points should differ"
    );
}

#[test]
fn test_cie_lab_buffer_to_rgb_length() {
    let conv = CieLabConverter::d65();
    // 3 pixels × 3 bytes = 9 bytes in, 9 bytes out.
    let input = vec![128u8, 128, 128, 200, 100, 150, 50, 200, 80];
    let out = conv.buffer_to_rgb(&input);
    assert_eq!(
        out.len(),
        input.len(),
        "output must be same length as input"
    );
}

#[test]
fn test_cie_lab_xyz_intermediate() {
    // For L*=50, a*=0, b*=0 we expect a neutral gray in XYZ.
    let conv = CieLabConverter::d65();
    let [x, y, z] = conv.lab_to_xyz(50.0, 0.0, 0.0);
    // Neutral: x/wx ≈ y/wy ≈ z/wz
    let rx = x / conv.white_x;
    let ry = y / conv.white_y;
    let rz = z / conv.white_z;
    assert!(
        (rx - ry).abs() < 0.01,
        "x/wx and y/wy should be equal for neutral gray"
    );
    assert!(
        (ry - rz).abs() < 0.01,
        "y/wy and z/wz should be equal for neutral gray"
    );
}

// ============================================================================
// Pixel helpers
// ============================================================================

#[test]
fn test_pixel_from_u8_slice_rgb() {
    let data = [200u8, 100, 50];
    let p = Pixel::from_u8_slice(&data, &ColorSpace::Rgb).expect("valid RGB slice");
    assert!((p.r() - 200.0 / 255.0).abs() < 0.001);
    assert!((p.g() - 100.0 / 255.0).abs() < 0.001);
    assert!((p.b() - 50.0 / 255.0).abs() < 0.001);
    assert_eq!(p.channel_count, 3);
}

#[test]
fn test_pixel_from_u8_slice_cmyk() {
    let data = [255u8, 128, 64, 0];
    let p = Pixel::from_u8_slice(&data, &ColorSpace::Cmyk).expect("valid CMYK slice");
    assert_eq!(p.channel_count, 4);
    assert!((p.r() - 1.0).abs() < 0.001, "C channel should be 1.0");
    assert!((p.a() - 0.0).abs() < 0.001, "K channel should be 0.0");
}

#[test]
fn test_pixel_from_u8_slice_ycbcr() {
    let data = [76u8, 85, 255];
    let p = Pixel::from_u8_slice(&data, &ColorSpace::YCbCr).expect("valid YCbCr slice");
    assert_eq!(p.channel_count, 3);
}

#[test]
fn test_pixel_from_u8_slice_insufficient_channels() {
    let data = [200u8, 100]; // only 2 bytes for RGB
    let result = Pixel::from_u8_slice(&data, &ColorSpace::Rgb);
    assert!(result.is_err(), "should fail with too few channels");
}

#[test]
fn test_pixel_to_u8_rgb_clamps() {
    // channels slightly outside [0,1]
    let p = Pixel {
        channels: [1.5, -0.1, 0.5, 0.0],
        channel_count: 3,
    };
    let rgb = p.to_u8_rgb();
    assert_eq!(rgb[0], 255, "channel > 1.0 should clamp to 255");
    assert_eq!(rgb[1], 0, "channel < 0.0 should clamp to 0");
    assert_eq!(rgb[2], 128, "channel 0.5 should be ~128");
}

#[test]
fn test_pixel_to_u8_rgba_clamps() {
    let p = Pixel {
        channels: [1.0, 0.0, 0.5, 1.2],
        channel_count: 4,
    };
    let rgba = p.to_u8_rgba();
    assert_eq!(rgba[3], 255, "alpha > 1.0 should clamp to 255");
}

#[test]
fn test_pixel_gray() {
    let p = Pixel::gray(0.75);
    assert_eq!(p.channel_count, 1);
    assert!((p.r() - 0.75).abs() < 0.001);
}

#[test]
fn test_pixel_rgba() {
    let p = Pixel::rgba(1.0, 0.5, 0.0, 0.8);
    assert_eq!(p.channel_count, 4);
    assert!((p.a() - 0.8).abs() < 0.001);
}

#[test]
fn test_pixel_from_u16_slice_rgb() {
    let data = [65535u16, 32768, 0];
    let p = Pixel::from_u16_slice(&data, &ColorSpace::Rgb).expect("valid u16 RGB");
    assert!((p.r() - 1.0).abs() < 0.001);
    assert!((p.g() - 32768.0 / 65535.0).abs() < 0.001);
    assert!(p.b().abs() < 0.001);
}

#[test]
fn test_pixel_from_u16_slice_insufficient() {
    let data = [65535u16];
    let result = Pixel::from_u16_slice(&data, &ColorSpace::Rgb);
    assert!(result.is_err());
}

// ============================================================================
// IccProfile header parsing
// ============================================================================

/// Build a minimal valid 132-byte ICC header.
fn make_icc_header(
    size: u32,
    device_class: &[u8; 4],
    cs_sig: &[u8; 4],
    pcs_sig: &[u8; 4],
    intent: u32,
    tag_count: u32,
) -> Vec<u8> {
    let mut buf = vec![0u8; 132];
    // bytes 0-3: profile size (big-endian)
    buf[0..4].copy_from_slice(&size.to_be_bytes());
    // bytes 16-19: device class
    buf[16..20].copy_from_slice(device_class);
    // bytes 20-23: color space sig
    buf[20..24].copy_from_slice(cs_sig);
    // bytes 24-27: PCS
    buf[24..28].copy_from_slice(pcs_sig);
    // bytes 64-67: rendering intent
    buf[64..68].copy_from_slice(&intent.to_be_bytes());
    // bytes 128-131: tag count
    buf[128..132].copy_from_slice(&tag_count.to_be_bytes());
    buf
}

#[test]
fn test_icc_profile_parse_rgb_display() {
    let buf = make_icc_header(1024, b"mntr", b"RGB ", b"XYZ ", 0, 5);
    let profile = IccProfile::parse_header(&buf).expect("should parse valid header");
    assert_eq!(profile.profile_size, 1024);
    assert_eq!(profile.color_space_sig, ColorSpaceSig::Rgb);
    assert_eq!(profile.pcs, ColorSpaceSig::Xyz);
    assert_eq!(profile.device_class, DeviceClass::Display);
    assert_eq!(profile.rendering_intent, 0);
    assert_eq!(profile.tag_count, 5);
}

#[test]
fn test_icc_profile_parse_cmyk_printer() {
    let buf = make_icc_header(2048, b"prtr", b"CMYK", b"Lab ", 1, 12);
    let profile = IccProfile::parse_header(&buf).expect("should parse CMYK printer header");
    assert_eq!(profile.color_space_sig, ColorSpaceSig::Cmyk);
    assert_eq!(profile.pcs, ColorSpaceSig::Lab);
    assert_eq!(profile.device_class, DeviceClass::Output);
    assert_eq!(profile.tag_count, 12);
}

#[test]
fn test_icc_profile_parse_gray_scanner() {
    let buf = make_icc_header(512, b"scnr", b"GRAY", b"XYZ ", 0, 3);
    let profile = IccProfile::parse_header(&buf).expect("should parse gray scanner header");
    assert_eq!(profile.color_space_sig, ColorSpaceSig::Gray);
    assert_eq!(profile.device_class, DeviceClass::Input);
}

#[test]
fn test_icc_profile_is_valid_true() {
    let buf = make_icc_header(256, b"mntr", b"RGB ", b"XYZ ", 0, 4);
    let profile = IccProfile::parse_header(&buf).expect("parse");
    assert!(
        profile.is_valid(),
        "profile with known signatures should be valid"
    );
}

#[test]
fn test_icc_profile_is_valid_false_unknown_cs() {
    let buf = make_icc_header(256, b"mntr", b"JUNK", b"XYZ ", 0, 0);
    let profile = IccProfile::parse_header(&buf).expect("parse");
    assert!(
        !profile.is_valid(),
        "profile with unknown CS should be invalid"
    );
}

#[test]
fn test_icc_profile_is_valid_false_size_zero() {
    let buf = make_icc_header(0, b"mntr", b"RGB ", b"XYZ ", 0, 0);
    let profile = IccProfile::parse_header(&buf).expect("parse");
    assert!(
        !profile.is_valid(),
        "profile with size < 128 should be invalid"
    );
}

#[test]
fn test_icc_profile_parse_truncated_errors() {
    let buf = vec![0u8; 64]; // only 64 bytes, need at least 128
    let result = IccProfile::parse_header(&buf);
    assert!(result.is_err(), "truncated header should fail");
}

#[test]
fn test_icc_profile_tag_count_zero_without_extended() {
    // Provide exactly 128 bytes — tag count should default to 0.
    let mut buf = vec![0u8; 128];
    buf[0..4].copy_from_slice(&512u32.to_be_bytes());
    buf[16..20].copy_from_slice(b"mntr");
    buf[20..24].copy_from_slice(b"RGB ");
    buf[24..28].copy_from_slice(b"XYZ ");
    let profile = IccProfile::parse_header(&buf).expect("parse 128-byte header");
    assert_eq!(profile.tag_count, 0);
}

// ============================================================================
// ColorSpaceConverter — unified façade
// ============================================================================

#[test]
fn test_converter_white_is_zero_inverts() {
    // Input: all-white (255) → output should be all-black (0).
    let data = vec![255u8, 0, 128];
    let out = ColorSpaceConverter::to_rgb(&data, &ColorSpace::WhiteIsZero, 8)
        .expect("WhiteIsZero conversion");
    // each pixel: 255 → 0, 0 → 255, 128 → 127
    assert_eq!(out[0], 0, "255 inverted should be 0");
    assert_eq!(out[3], 255, "0 inverted should be 255");
    assert_eq!(out[6], 127, "128 inverted should be 127");
}

#[test]
fn test_converter_black_is_zero_passthrough() {
    let data = vec![100u8, 200];
    let out = ColorSpaceConverter::to_rgb(&data, &ColorSpace::BlackIsZero, 8)
        .expect("BlackIsZero conversion");
    // Each gray byte is broadcast to RGB.
    assert_eq!(out[0], 100);
    assert_eq!(out[1], 100);
    assert_eq!(out[2], 100);
    assert_eq!(out[3], 200);
}

#[test]
fn test_converter_rgb_passthrough() {
    let data = vec![10u8, 20, 30, 40, 50, 60];
    let out = ColorSpaceConverter::to_rgb(&data, &ColorSpace::Rgb, 8).expect("RGB passthrough");
    assert_eq!(out, data);
}

#[test]
fn test_converter_cmyk_to_rgb_produces_correct_length() {
    // 3 CMYK pixels (12 bytes) → 9 RGB bytes
    let data = vec![0u8; 12];
    let out = ColorSpaceConverter::to_rgb(&data, &ColorSpace::Cmyk, 8).expect("CMYK to RGB");
    assert_eq!(out.len(), 9);
}

#[test]
fn test_converter_ycbcr_to_rgb() {
    let data = vec![128u8, 128, 128]; // 1 YCbCr pixel
    let out = ColorSpaceConverter::to_rgb(&data, &ColorSpace::YCbCr, 8).expect("YCbCr to RGB");
    assert_eq!(out.len(), 3, "1 YCbCr pixel → 1 RGB pixel (3 bytes)");
}

#[test]
fn test_converter_cie_lab_to_rgb() {
    let data = vec![0u8, 128, 128]; // L*=0, a*=0, b*=0 → black
    let out = ColorSpaceConverter::to_rgb(&data, &ColorSpace::CieLab, 8).expect("CIE Lab to RGB");
    assert_eq!(out.len(), 3);
    // Near-black output expected.
    assert!(
        out[0] < 10,
        "R should be near 0 for black Lab, got {}",
        out[0]
    );
    assert!(
        out[1] < 10,
        "G should be near 0 for black Lab, got {}",
        out[1]
    );
    assert!(
        out[2] < 10,
        "B should be near 0 for black Lab, got {}",
        out[2]
    );
}

#[test]
fn test_converter_icc_lab_to_rgb() {
    let data = vec![255u8, 128, 128]; // L*≈100, a*=0, b*=0 → near white
    let out = ColorSpaceConverter::to_rgb(&data, &ColorSpace::IccLab, 8).expect("ICC Lab to RGB");
    assert_eq!(out.len(), 3);
    assert!(
        out[0] > 200,
        "R should be near 255 for near-white Lab, got {}",
        out[0]
    );
}

#[test]
fn test_converter_unsupported_bit_depth() {
    let data = vec![0u8; 4];
    let result = ColorSpaceConverter::to_rgb(&data, &ColorSpace::Rgb, 16);
    assert!(result.is_err(), "16-bit should return unsupported error");
}

#[test]
fn test_pixel_to_rgb_white_is_zero() {
    // channels[0] = 1.0 → inverted to 0.0 (black)
    let result = ColorSpaceConverter::pixel_to_rgb(&[1.0_f32], &ColorSpace::WhiteIsZero)
        .expect("pixel_to_rgb WhiteIsZero");
    assert!(
        result[0].abs() < 0.001,
        "inverted 1.0 should be 0.0, got {}",
        result[0]
    );
}

#[test]
fn test_pixel_to_rgb_insufficient_channels_error() {
    // RGB needs 3 channels.
    let result = ColorSpaceConverter::pixel_to_rgb(&[0.5_f32, 0.3], &ColorSpace::Rgb);
    assert!(result.is_err(), "should fail with insufficient channels");
}

#[test]
fn test_pixel_to_rgb_cmyk() {
    // CMYK (0,0,0,0) → white
    let result = ColorSpaceConverter::pixel_to_rgb(&[0.0_f32, 0.0, 0.0, 0.0], &ColorSpace::Cmyk)
        .expect("CMYK pixel_to_rgb");
    assert!((result[0] - 1.0).abs() < 0.001);
    assert!((result[1] - 1.0).abs() < 0.001);
    assert!((result[2] - 1.0).abs() < 0.001);
}
