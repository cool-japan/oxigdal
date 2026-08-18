//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

#[cfg(test)]
pub(super) mod tests {
    use super::super::types_2::Jpeg2000Reader;
    use super::super::types_3::ProgressiveDecodingState;
    use crate::codestream::{CodingStyle, ImageSize};
    use crate::error::{Jpeg2000Error, ResilienceMode};
    use std::io::Cursor;

    /// Build a minimal valid J2K codestream: SOC + SIZ + COD + QCD + SOT + SOD + EOC
    /// 4×4 grayscale, 1 decomposition level, 1 code-block, 0 packet data.
    fn build_minimal_j2k_4x4_grayscale() -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();

        // SOC: 0xFF4F
        out.extend_from_slice(&[0xFF, 0x4F]);

        // SIZ marker: 0xFF51
        // Lsiz = 2 + 2 + 4 + 4 + 4 + 4 + 4 + 4 + 4 + 4 + 2 + (1×3) = 41 bytes
        // But the length field includes itself (2 bytes), so Lsiz = 41
        // Fields: Rsiz(2) + Xsiz(4) + Ysiz(4) + XOsiz(4) + YOsiz(4)
        //       + XTsiz(4) + YTsiz(4) + XTOsiz(4) + YTOsiz(4) + Csiz(2)
        //       + Ssiz(1) + XRsiz(1) + YRsiz(1)  → 2+8×4+2+3 = 39 data bytes
        //       Lsiz = 39 + 2 = 41
        out.extend_from_slice(&[0xFF, 0x51]);
        out.extend_from_slice(&[0x00, 0x29]); // Lsiz = 41
        out.extend_from_slice(&[0x00, 0x00]); // Rsiz
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x04]); // Xsiz = 4
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x04]); // Ysiz = 4
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // XOsiz = 0
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // YOsiz = 0
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x04]); // XTsiz = 4
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x04]); // YTsiz = 4
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // XTOsiz = 0
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // YTOsiz = 0
        out.extend_from_slice(&[0x00, 0x01]); // Csiz = 1
        out.push(0x07); // Ssiz: signed=0, precision-1=7 → 8-bit unsigned
        out.push(0x01); // XRsiz = 1
        out.push(0x01); // YRsiz = 1

        // COD marker: 0xFF52
        // Lcod = 2 + 1+1+2+1+1+1+1+1+1 = 12 → Lcod = 12
        out.extend_from_slice(&[0xFF, 0x52]);
        out.extend_from_slice(&[0x00, 0x0C]); // Lcod = 12
        out.push(0x00); // Scod: no precincts, no SOT markers, no EPH
        out.push(0x00); // progression order = LRCP
        out.extend_from_slice(&[0x00, 0x01]); // num_layers = 1
        out.push(0x00); // mct = 0
        out.push(0x00); // num_levels = 0 (no decomposition)
        out.push(0x02); // xcb = 2 → code-block width = 1<<(2+2) = 16, but tile is 4x4
        out.push(0x02); // ycb = 2 → code-block height = 16, clamped to 4
        out.push(0x00); // code-block style
        out.push(0x01); // wavelet = 1 → 5/3 reversible

        // QCD marker: 0xFF5C — reversible quantization, no quantization (style=0)
        // Lqcd = 2 + 1 + num_steps
        // With style=0 (no quantization), 1 step size byte needed for 1 subband
        out.extend_from_slice(&[0xFF, 0x5C]);
        out.extend_from_slice(&[0x00, 0x04]); // Lqcd = 4
        out.push(0x00); // Sqcd = 0 (no quantization)
        out.push(0x00); // step size for LL subband

        // SOT marker: 0xFF90
        // Lsot = 10 (fixed), Isot = 0, Psot = 0 (unknown), TPsot = 0, TNsot = 1
        out.extend_from_slice(&[0xFF, 0x90]);
        out.extend_from_slice(&[0x00, 0x0A]); // Lsot = 10
        out.extend_from_slice(&[0x00, 0x00]); // Isot = 0
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Psot = 0 (unknown)
        out.push(0x00); // TPsot = 0
        out.push(0x01); // TNsot = 1

        // SOD: 0xFF93 — no packet data follows
        out.extend_from_slice(&[0xFF, 0x93]);

        // EOC: 0xFFD9
        out.extend_from_slice(&[0xFF, 0xD9]);

        out
    }

    #[test]
    fn test_decode_rgb_minimal_j2k_empty_sod() {
        let data = build_minimal_j2k_4x4_grayscale();
        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        // parse_headers will parse the J2K markers
        reader.parse_headers().expect("parse_headers failed");

        // Width/height should be 4×4 from SIZ
        assert_eq!(reader.width().expect("width"), 4);
        assert_eq!(reader.height().expect("height"), 4);
        assert_eq!(reader.num_components().expect("num_components"), 1);

        // raw_codestream should now be stored
        assert!(reader.raw_codestream.is_some());

        // decode_rgb: with zero coefficients and no wavelet levels,
        // level_shift(0, 8, false) = 0, so all output pixels should be 0
        // (unsigned, shift=0 → 0 clamped to [0, 255] → 0)
        let rgb = reader.decode_rgb().expect("decode_rgb failed");
        assert_eq!(rgb.len(), 4 * 4 * 3);
        // All pixels are gray-equivalent (all channel equal)
        for i in 0..(4 * 4) {
            assert_eq!(rgb[i * 3], rgb[i * 3 + 1], "R != G at pixel {}", i);
            assert_eq!(rgb[i * 3 + 1], rgb[i * 3 + 2], "G != B at pixel {}", i);
        }
    }

    #[test]
    fn test_decode_quality_layers_returns_real_pixels_not_flat_gray() {
        // Regression: decode_quality_layers() used to ignore the codestream and
        // fill the buffer with a flat gray value = 128 * (max_layer+1)/num_layers.
        // It must now return real decoded pixel data.
        let data = build_minimal_j2k_4x4_grayscale();
        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");
        reader.parse_headers().expect("parse_headers failed");

        let layered = reader
            .decode_quality_layers(0)
            .expect("decode_quality_layers failed");
        assert_eq!(layered.len(), 4 * 4 * 3);

        // The old stub would have produced all-128 pixels; the real decode of an
        // all-zero-coefficient stream yields all-zero pixels.
        assert!(
            layered.iter().all(|&p| p == 0),
            "decode_quality_layers must return real decoded pixels, not the flat-gray stub"
        );

        // Progressive output must agree with the non-progressive decode path.
        let rgb = reader.decode_rgb().expect("decode_rgb failed");
        assert_eq!(layered, rgb, "progressive and full decode must agree");

        // Progressive state must be recorded for the requested layer, and the
        // stored buffer must match what was returned.
        assert_eq!(reader.progressive_layer(), Some(0));
        assert_eq!(reader.progressive_data(), Some(layered.as_slice()));
    }

    #[test]
    fn test_decode_quality_layers_rejects_out_of_range_layer() {
        let data = build_minimal_j2k_4x4_grayscale();
        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");
        reader.parse_headers().expect("parse_headers failed");

        // Only one quality layer exists; requesting layer 1 must error, not decode.
        assert!(reader.decode_quality_layers(1).is_err());
    }

    #[test]
    fn test_decode_rgb_with_nonempty_packet_data_is_resilient() {
        // Regression for the naive even-division demux + error-resilient tier-1
        // fallback: a tile that actually carries packet bytes must decode without
        // panicking and yield a correctly sized buffer.
        let mut data = build_minimal_j2k_4x4_grayscale();
        // Insert non-empty "packet" bytes just before the trailing EOC (0xFF 0xD9).
        let eoc_pos = data.len() - 2;
        let packet_bytes = [0x80u8, 0x40, 0x55, 0x00, 0x12, 0x34];
        data.splice(eoc_pos..eoc_pos, packet_bytes.iter().copied());

        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");
        reader.parse_headers().expect("parse_headers failed");

        let rgb = reader.decode_rgb().expect("decode_rgb failed");
        assert_eq!(rgb.len(), 4 * 4 * 3);
    }

    /// Build a two-tile (2×1) raw J2K codestream. Image is 8×4 with 4×4 tiles,
    /// so tile 0 covers x∈[0,4) and tile 1 covers x∈[4,8). Each tile-part carries
    /// a distinctive, correctly `Psot`-bounded SOD payload so bleed across tile
    /// boundaries is detectable.
    fn build_two_tile_j2k(tile0_data: &[u8], tile1_data: &[u8]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&[0xFF, 0x4F]); // SOC

        // SIZ (Lsiz = 41), 8×4 image, 4×4 tiles, 1 component.
        out.extend_from_slice(&[0xFF, 0x51]);
        out.extend_from_slice(&[0x00, 0x29]);
        out.extend_from_slice(&[0x00, 0x00]); // Rsiz
        out.extend_from_slice(&8u32.to_be_bytes()); // Xsiz
        out.extend_from_slice(&4u32.to_be_bytes()); // Ysiz
        out.extend_from_slice(&0u32.to_be_bytes()); // XOsiz
        out.extend_from_slice(&0u32.to_be_bytes()); // YOsiz
        out.extend_from_slice(&4u32.to_be_bytes()); // XTsiz
        out.extend_from_slice(&4u32.to_be_bytes()); // YTsiz
        out.extend_from_slice(&0u32.to_be_bytes()); // XTOsiz
        out.extend_from_slice(&0u32.to_be_bytes()); // YTOsiz
        out.extend_from_slice(&1u16.to_be_bytes()); // Csiz
        out.push(0x07); // Ssiz: 8-bit unsigned
        out.push(0x01); // XRsiz
        out.push(0x01); // YRsiz

        // COD (Lcod = 12), LRCP, 1 layer, no levels, 5/3.
        out.extend_from_slice(&[0xFF, 0x52]);
        out.extend_from_slice(&[0x00, 0x0C]);
        out.push(0x00);
        out.push(0x00);
        out.extend_from_slice(&1u16.to_be_bytes());
        out.push(0x00);
        out.push(0x00);
        out.push(0x00);
        out.push(0x00);
        out.push(0x00);
        out.push(0x01);

        // QCD (Lqcd = 4), no quantization.
        out.extend_from_slice(&[0xFF, 0x5C]);
        out.extend_from_slice(&[0x00, 0x04]);
        out.push(0x00);
        out.push(0x00);

        // Tile-part 0: SOT(12) + SOD(2) + data. Psot = 14 + data.len().
        out.extend_from_slice(&[0xFF, 0x90]);
        out.extend_from_slice(&10u16.to_be_bytes()); // Lsot
        out.extend_from_slice(&0u16.to_be_bytes()); // Isot = 0
        out.extend_from_slice(&((14 + tile0_data.len()) as u32).to_be_bytes()); // Psot
        out.push(0x00); // TPsot
        out.push(0x01); // TNsot
        out.extend_from_slice(&[0xFF, 0x93]); // SOD
        out.extend_from_slice(tile0_data);

        // Tile-part 1.
        out.extend_from_slice(&[0xFF, 0x90]);
        out.extend_from_slice(&10u16.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes()); // Isot = 1
        out.extend_from_slice(&((14 + tile1_data.len()) as u32).to_be_bytes()); // Psot
        out.push(0x00);
        out.push(0x01);
        out.extend_from_slice(&[0xFF, 0x93]); // SOD
        out.extend_from_slice(tile1_data);

        out.extend_from_slice(&[0xFF, 0xD9]); // EOC
        out
    }

    #[test]
    fn test_find_tile_bitstream_bounds_per_tile() {
        // Regression: each tile's returned bitstream must be bounded by its own
        // Psot, never bleed to the end of the whole codestream.
        let tile0 = [0xAAu8, 0xBB, 0xCC];
        let tile1 = [0x11u8, 0x22];
        let data = build_two_tile_j2k(&tile0, &tile1);
        let mut reader = Jpeg2000Reader::new(Cursor::new(data)).expect("reader creation failed");
        reader.parse_headers().expect("parse_headers failed");

        assert_eq!(reader.image_size.as_ref().map(|s| s.num_tiles()), Some(2));

        let b0 = reader.find_tile_bitstream(0).expect("tile 0 bitstream");
        let b1 = reader.find_tile_bitstream(1).expect("tile 1 bitstream");
        assert_eq!(b0, tile0, "tile 0 must not include tile 1's bytes");
        assert_eq!(b1, tile1, "tile 1 must return exactly its own bytes");
    }

    #[test]
    fn test_decode_rgb_two_tiles_full_size() {
        // Multi-tile decode must compose the full 8×4 raster (not a single tile).
        let data = build_two_tile_j2k(&[], &[]);
        let mut reader = Jpeg2000Reader::new(Cursor::new(data)).expect("reader creation failed");
        reader.parse_headers().expect("parse_headers failed");
        let rgb = reader.decode_rgb().expect("decode_rgb failed");
        assert_eq!(rgb.len(), 8 * 4 * 3);
    }

    #[test]
    fn test_decode_rgb_without_codestream_errors_not_gray() {
        // Regression: decode must never fabricate a flat-gray Ok when the
        // codestream was never parsed.
        let mut reader = Jpeg2000Reader::new(Cursor::new(vec![
            0xFF, 0x4F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ]))
        .expect("reader creation failed");
        reader.image_size = Some(ImageSize {
            width: 8,
            height: 8,
            x_offset: 0,
            y_offset: 0,
            tile_width: 8,
            tile_height: 8,
            tile_x_offset: 0,
            tile_y_offset: 0,
            num_components: 1,
            components: vec![],
        });
        assert!(matches!(
            reader.decode_rgb(),
            Err(Jpeg2000Error::CodestreamError(_))
        ));
    }

    /// Find the byte offset of a two-byte marker in a codestream.
    fn find_marker(data: &[u8], marker: u16) -> Option<usize> {
        let hi = (marker >> 8) as u8;
        let lo = marker as u8;
        data.windows(2).position(|w| w[0] == hi && w[1] == lo)
    }

    #[test]
    fn test_decode_tile_real_packet_matches_tier1() {
        // The core Tier-2 wiring assertion: a hand-built codestream carrying one
        // real packet (single included code block) must have its body bytes
        // sliced exactly by the packet parser and fed to Tier-1 — the decoded
        // coefficients must match a *direct* Tier-1 decode of those same bytes.
        use crate::tier1::{CodeBlockDecoder, SubbandType};
        use crate::tier2::layout::code_block_bitplanes;

        let body: [u8; 5] = [0x95, 0x40, 0x22, 0x0C, 0x71];
        // Packet header for a single included code block (1x1 grid):
        //   present=1, inclusion=1, zbp-terminator=1, num_passes(=1)=0,
        //   Lblock comma=0, length(=5)=0b101  =>  0b1110_0101 = 0xE5.
        let mut packet = vec![0xE5u8];
        packet.extend_from_slice(&body);

        let mut data = build_minimal_j2k_4x4_grayscale();
        let eoc = data.len() - 2; // splice packet data in just before the EOC
        data.splice(eoc..eoc, packet.iter().copied());

        let mut reader = Jpeg2000Reader::new(Cursor::new(data)).expect("reader creation failed");
        reader.parse_headers().expect("parse_headers failed");

        let comps = reader
            .decode_tile_to_components(0, 0)
            .expect("tile decode failed");
        assert_eq!(comps.len(), 1);

        // Parameters the reader derives from this stream: guard=0, exponent=0,
        // precision=8, zbp=0 => 9 bit-planes; 4x4 LL code block.
        let num_bitplanes = code_block_bitplanes(0, 0, 8, 0);
        let reference = CodeBlockDecoder::with_subband(4, 4, num_bitplanes, SubbandType::Ll)
            .decode(&body)
            .expect("reference tier-1 decode failed");

        assert_eq!(
            comps[0], reference,
            "Tier-2 must slice exactly the packet body bytes and feed them to Tier-1"
        );
        // The routed bytes must actually influence the output (not a no-op).
        assert_ne!(
            reference,
            vec![0i32; 16],
            "chosen body should decode to some non-zero coefficients"
        );

        // The full RGB path must also succeed and be correctly sized.
        let rgb = reader.decode_rgb().expect("decode_rgb failed");
        assert_eq!(rgb.len(), 4 * 4 * 3);
    }

    #[test]
    fn test_decode_multi_layer_codestream_rejected() {
        // Multi-layer streams must fail loud (typed error), never mis-slice.
        let mut data = build_minimal_j2k_4x4_grayscale();
        let cod = find_marker(&data, 0xFF52).expect("COD marker present");
        // num_layers is 2 bytes after COD marker + Lcod + Scod + progression.
        data[cod + 6] = 0x00;
        data[cod + 7] = 0x02;

        let mut reader = Jpeg2000Reader::new(Cursor::new(data)).expect("reader creation failed");
        reader.parse_headers().expect("parse_headers failed");
        assert_eq!(reader.num_quality_layers(), 2);

        let result = reader.decode_rgb();
        assert!(
            matches!(result, Err(Jpeg2000Error::UnsupportedFeature(_))),
            "multi-layer decode must return UnsupportedFeature, got {:?}",
            result.map(|v| v.len())
        );
    }

    #[test]
    fn test_decode_all_progression_orders_single_layer() {
        // Every progression order must decode for a single-layer stream (they
        // share the ProgressionIterator); none should spuriously reject.
        for order in 0u8..=4 {
            let mut data = build_minimal_j2k_4x4_grayscale();
            let cod = find_marker(&data, 0xFF52).expect("COD marker present");
            // Progression order is 1 byte after COD marker + Lcod + Scod.
            data[cod + 5] = order;

            let mut reader =
                Jpeg2000Reader::new(Cursor::new(data)).expect("reader creation failed");
            reader.parse_headers().expect("parse_headers failed");

            let rgb = reader
                .decode_rgb()
                .unwrap_or_else(|e| panic!("progression {order} must decode, got {e:?}"));
            assert_eq!(rgb.len(), 4 * 4 * 3);
        }
    }

    #[test]
    fn test_reader_creation() {
        // Create minimal JP2 signature
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, // Box length
            0x6A, 0x50, 0x20, 0x20, // 'jP  '
            0x0D, 0x0A, 0x87, 0x0A, // Signature
        ];

        let cursor = Cursor::new(data);
        let result = Jpeg2000Reader::new(cursor);
        assert!(result.is_ok());

        let reader = result.expect("reader failed");
        assert!(reader.is_jp2);
    }

    #[test]
    fn test_j2k_detection() {
        // Create minimal J2K codestream (SOC marker + padding to 12 bytes for detection)
        let data = vec![
            0xFF, 0x4F, // SOC marker
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // padding
        ];

        let cursor = Cursor::new(data);
        let result = Jpeg2000Reader::new(cursor);
        assert!(result.is_ok());

        let reader = result.expect("reader failed");
        assert!(!reader.is_jp2);
    }

    #[test]
    fn test_resilience_mode_default() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];
        let cursor = Cursor::new(data);
        let reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        assert_eq!(reader.resilience_mode(), ResilienceMode::None);
        assert!(!reader.resilience_mode().is_enabled());
    }

    #[test]
    fn test_resilience_mode_configuration() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];
        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        // Test basic resilience
        reader.enable_error_resilience();
        assert_eq!(reader.resilience_mode(), ResilienceMode::Basic);
        assert!(reader.resilience_mode().is_enabled());

        // Test full resilience
        reader.enable_full_error_resilience();
        assert_eq!(reader.resilience_mode(), ResilienceMode::Full);
        assert!(reader.resilience_mode().is_full());

        // Test disable
        reader.disable_error_resilience();
        assert_eq!(reader.resilience_mode(), ResilienceMode::None);
    }

    #[test]
    fn test_progressive_state_initialization() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];
        let cursor = Cursor::new(data);
        let reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        assert!(!reader.is_progressive_active());
        assert!(reader.progressive_layer().is_none());
    }

    #[test]
    fn test_progressive_state_reset() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];
        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        // Initialize state by setting it manually
        reader.progressive_state = Some(ProgressiveDecodingState {
            current_layer: 2,
            max_layers: 5,
            intermediate_data: vec![],
            width: 256,
            height: 256,
        });

        assert!(reader.is_progressive_active());
        assert_eq!(reader.progressive_layer(), Some(2));

        // Reset state
        reader.reset_progressive_state();
        assert!(!reader.is_progressive_active());
        assert!(reader.progressive_layer().is_none());
    }

    #[test]
    fn test_region_bounds_validation() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];
        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        // Set up minimal image size
        reader.image_size = Some(ImageSize {
            width: 256,
            height: 256,
            x_offset: 0,
            y_offset: 0,
            tile_width: 256,
            tile_height: 256,
            tile_x_offset: 0,
            tile_y_offset: 0,
            num_components: 3,
            components: vec![],
        });

        // An in-bounds region no longer fabricates placeholder pixels: without a
        // parsed codestream, decoding must return a typed error, never fake data.
        let result = reader.decode_region(0, 0, 128, 128);
        assert!(matches!(result, Err(Jpeg2000Error::CodestreamError(_))));

        // Region exceeding width should fail at bounds validation
        let result = reader.decode_region(200, 0, 100, 128);
        assert!(result.is_err());

        // Region exceeding height should fail at bounds validation
        let result = reader.decode_region(0, 200, 128, 100);
        assert!(result.is_err());
    }

    #[test]
    fn test_compute_intersecting_tiles() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];
        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        // Set up image with multiple tiles
        reader.image_size = Some(ImageSize {
            width: 512,
            height: 512,
            x_offset: 0,
            y_offset: 0,
            tile_width: 128,
            tile_height: 128,
            tile_x_offset: 0,
            tile_y_offset: 0,
            num_components: 3,
            components: vec![],
        });

        // Region in first tile only
        let tiles = reader.compute_intersecting_tiles(0, 0, 64, 64);
        assert!(tiles.is_ok());
        let tiles = tiles.expect("tiles");
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0], (0, 0));

        // Region spanning multiple tiles
        let tiles = reader.compute_intersecting_tiles(64, 64, 128, 128);
        assert!(tiles.is_ok());
        let tiles = tiles.expect("tiles");
        assert!(!tiles.is_empty());

        // Region covering entire image
        let tiles = reader.compute_intersecting_tiles(0, 0, 512, 512);
        assert!(tiles.is_ok());
        let tiles = tiles.expect("tiles");
        assert_eq!(tiles.len(), 16); // 4x4 tiles
    }

    #[test]
    fn test_resolution_level_scaling() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];
        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        // Set up image
        reader.image_size = Some(ImageSize {
            width: 256,
            height: 256,
            x_offset: 0,
            y_offset: 0,
            tile_width: 256,
            tile_height: 256,
            tile_x_offset: 0,
            tile_y_offset: 0,
            num_components: 3,
            components: vec![],
        });

        // Set coding style with decomposition levels
        reader.coding_style = Some(CodingStyle {
            progression_order: crate::codestream::ProgressionOrder::Lrcp,
            num_layers: 5,
            use_mct: true,
            num_levels: 3,
            code_block_width: 64,
            code_block_height: 64,
            code_block_style: 0,
            wavelet: crate::codestream::WaveletTransform::Reversible53,
            has_sop: false,
            has_eph: false,
        });

        // Full/half resolution decodes without a parsed codestream must error
        // (no fabricated placeholder), while the scale-factor bounds math still
        // runs first.
        assert!(
            reader
                .decode_region_at_resolution(0, 0, 128, 128, 0)
                .is_err()
        );
        assert!(reader.decode_region_at_resolution(0, 0, 64, 64, 1).is_err());

        // Invalid resolution level should fail at validation
        let result = reader.decode_region_at_resolution(0, 0, 64, 64, 10);
        assert!(result.is_err());
    }

    #[test]
    fn test_metadata_accessors() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];
        let cursor = Cursor::new(data);
        let reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        // Initially, all metadata should be None
        assert!(reader.file_type().is_none());
        assert!(reader.image_header().is_none());
        assert!(reader.color_specification().is_none());
        assert!(reader.capture_resolution().is_none());
        assert!(reader.display_resolution().is_none());
        assert!(reader.xml_metadata().is_empty());
        assert!(reader.uuid_boxes().is_empty());
    }

    #[test]
    fn test_quality_layer_accessors() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];
        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        // Default should be 1 layer
        assert_eq!(reader.num_quality_layers(), 1);

        // Set coding style with multiple layers
        reader.coding_style = Some(CodingStyle {
            progression_order: crate::codestream::ProgressionOrder::Lrcp,
            num_layers: 10,
            use_mct: false,
            num_levels: 5,
            code_block_width: 64,
            code_block_height: 64,
            code_block_style: 0,
            wavelet: crate::codestream::WaveletTransform::Reversible53,
            has_sop: false,
            has_eph: false,
        });

        assert_eq!(reader.num_quality_layers(), 10);
        assert_eq!(reader.num_decomposition_levels(), 5);
        assert!(!reader.uses_mct());
    }

    #[test]
    fn test_progressive_decoder_iterator() {
        let data = vec![
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];
        let cursor = Cursor::new(data);
        let mut reader = Jpeg2000Reader::new(cursor).expect("reader creation failed");

        // Set up minimal configuration
        reader.image_size = Some(ImageSize {
            width: 64,
            height: 64,
            x_offset: 0,
            y_offset: 0,
            tile_width: 64,
            tile_height: 64,
            tile_x_offset: 0,
            tile_y_offset: 0,
            num_components: 3,
            components: vec![],
        });

        reader.coding_style = Some(CodingStyle {
            progression_order: crate::codestream::ProgressionOrder::Lrcp,
            num_layers: 3,
            use_mct: false,
            num_levels: 2,
            code_block_width: 32,
            code_block_height: 32,
            code_block_style: 0,
            wavelet: crate::codestream::WaveletTransform::Reversible53,
            has_sop: false,
            has_eph: false,
        });

        let decoder = reader.decode_progressive().expect("decoder");

        assert_eq!(decoder.total_layers(), 3);
        assert_eq!(decoder.current_layer(), 0);
        assert!(!decoder.is_complete());
        assert_eq!(decoder.progress(), 0.0);
    }
}
