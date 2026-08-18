//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
pub(super) mod tests {
    use futures::stream::{self, StreamExt};
    use oxigeo_core::error::{OxiGeoError, Result};
    use oxigeo_core::io::ByteRange;
    use std::future::Future;

    use super::super::constants::{
        MAX_CONCURRENT_TILE_FETCHES, PHOTOMETRIC_TRANSPARENCY_MASK, TAG_IMAGE_LENGTH,
        TAG_IMAGE_WIDTH, TAG_NEW_SUBFILE_TYPE, TAG_TILE_LENGTH, TAG_TILE_WIDTH,
    };
    use super::super::functions::{
        RangeSource, apply_horizontal_predictor, assemble_window, assemble_window_rgb8,
        bytes_to_u16, decompress_tile, expected_tile_byte_size, finish_tile_decode, is_mask_ifd,
        normalize_pixel_scale_y, normalize_samples_to_native, tile_byte_range,
        undo_horizontal_predictor_u8, undo_horizontal_predictor_u16,
    };
    use super::super::types::IfdMetadata;
    use super::super::types_2::WasmCogReader;
    use super::super::types_3::{ByteOrder, CogMetadata};

    /// Forward horizontal differencing for a single-band 16-bit row: the
    /// inverse of [`undo_horizontal_predictor_u16`], used to build test inputs.
    fn forward_predictor_u16(row: &mut [u16]) {
        for i in (1..row.len()).rev() {
            row[i] = row[i].wrapping_sub(row[i - 1]);
        }
    }

    /// Forward horizontal differencing for an 8-bit `spp`-interleaved row.
    fn forward_predictor_u8(row: &mut [u8], spp: usize) {
        for i in (spp..row.len()).rev() {
            row[i] = row[i].wrapping_sub(row[i - spp]);
        }
    }

    #[test]
    fn predictor_u16_round_trip() {
        let original: Vec<u16> = vec![100, 105, 103, 250, 60000, 60005, 1, 0, 65535];
        let mut diffed = original.clone();
        forward_predictor_u16(&mut diffed);
        // Sanity: differencing actually changed the buffer.
        assert_ne!(diffed, original);
        undo_horizontal_predictor_u16(&mut diffed);
        assert_eq!(diffed, original);
    }

    #[test]
    fn predictor_u16_wraps() {
        // Deltas that overflow u16 must reconstruct via wrapping arithmetic.
        let original: Vec<u16> = vec![65530, 5, 65535, 10];
        let mut diffed = original.clone();
        forward_predictor_u16(&mut diffed);
        undo_horizontal_predictor_u16(&mut diffed);
        assert_eq!(diffed, original);
    }

    #[test]
    fn predictor_u8_rgb_round_trip() {
        // 4 pixels, 3 channels interleaved: R R R R / G G G G / B B B B pattern.
        let original: Vec<u8> = vec![
            10, 200, 30, // px0 RGB
            12, 205, 28, // px1
            15, 255, 25, // px2
            18, 2, 20, // px3
        ];
        let mut diffed = original.clone();
        forward_predictor_u8(&mut diffed, 3);
        assert_ne!(diffed, original);
        undo_horizontal_predictor_u8(&mut diffed, 3);
        assert_eq!(diffed, original);
    }

    #[test]
    fn apply_predictor_over_tile_u16() {
        // 2x2 tile, single band, little-endian. Undoing per-row must not leak
        // across the row boundary (row 1 is independent of row 0).
        let rows: [[u16; 2]; 2] = [[100, 150], [4000, 4020]];
        let mut expected: Vec<u16> = Vec::new();
        let mut bytes: Vec<u8> = Vec::new();
        for row in &rows {
            let mut diffed = row.to_vec();
            forward_predictor_u16(&mut diffed);
            for v in &diffed {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            expected.extend_from_slice(row);
        }
        apply_horizontal_predictor(&mut bytes, 2, 2, 16, 1, true);
        // The predictor leaves samples in *file* order; normalisation is the
        // separate step `read_tile_level` applies next.
        normalize_samples_to_native(&mut bytes, 16, true);
        assert_eq!(bytes_to_u16(&bytes), expected);
    }

    /// `normalize_samples_to_native` is the pivot the whole wasm byte-order
    /// contract turns on, so pin both directions explicitly.
    #[test]
    fn normalize_samples_to_native_swaps_only_foreign_order() {
        let host_is_le = cfg!(target_endian = "little");

        // A file in the host's own order is already native: byte-for-byte
        // identical after normalisation, at every supported width.
        for (bits, width) in [(16u16, 2usize), (32, 4), (64, 8)] {
            let original: Vec<u8> = (0..(width as u8 * 3)).collect();
            let mut same = original.clone();
            normalize_samples_to_native(&mut same, bits, host_is_le);
            assert_eq!(same, original, "{bits}-bit host-order file must not move");

            // A file in the opposite order has each sample reversed in place,
            // and never across sample boundaries.
            let mut foreign = original.clone();
            normalize_samples_to_native(&mut foreign, bits, !host_is_le);
            let expected: Vec<u8> = original
                .chunks_exact(width)
                .flat_map(|c| c.iter().rev().copied())
                .collect();
            assert_eq!(foreign, expected, "{bits}-bit foreign file must swap");
        }

        // 8-bit and exotic widths have no sample order to normalise.
        for bits in [1u16, 8, 24] {
            let original: Vec<u8> = (0..12u8).collect();
            let mut data = original.clone();
            normalize_samples_to_native(&mut data, bits, !host_is_le);
            assert_eq!(data, original, "{bits}-bit samples must pass through");
        }
    }

    /// A tile whose sample count is not a whole multiple of the sample width
    /// (a truncated block) must normalise its complete samples and drop the
    /// trailing partial one rather than panicking or shifting the buffer.
    #[test]
    fn normalize_samples_to_native_ignores_trailing_partial_sample() {
        let host_is_le = cfg!(target_endian = "little");
        let mut data = vec![1u8, 2, 3, 4, 5];
        normalize_samples_to_native(&mut data, 16, !host_is_le);
        assert_eq!(data, vec![2, 1, 4, 3, 5]);
    }

    /// Minimal level record for the tile-decode tests.
    fn level_16bit(tile_width: u32, tile_height: u32, predictor: u16) -> IfdMetadata {
        IfdMetadata {
            width: u64::from(tile_width),
            height: u64::from(tile_height),
            tile_width,
            tile_height,
            bits_per_sample: 16,
            samples_per_pixel: 1,
            sample_format: 1,
            compression: 1,
            photometric_interpretation: 1,
            predictor,
            tile_offsets: Vec::new(),
            tile_byte_counts: Vec::new(),
            pixel_scale_x: None,
            pixel_scale_y: None,
            tiepoint_pixel_x: None,
            tiepoint_pixel_y: None,
            tiepoint_geo_x: None,
            tiepoint_geo_y: None,
            epsg_code: None,
        }
    }

    /// Serialise one 2x2 16-bit tile of `values`, in `II` or `MM`, optionally
    /// horizontally predicted — i.e. exactly the bytes a real COG would store.
    fn encode_tile_16bit(values: &[[u16; 2]; 2], little_endian: bool, predictor: u16) -> Vec<u8> {
        let mut out = Vec::new();
        for row in values {
            let mut row = row.to_vec();
            if predictor == 2 {
                // Forward horizontal differencing, the encoder's transform.
                for i in (1..row.len()).rev() {
                    row[i] = row[i].wrapping_sub(row[i - 1]);
                }
            }
            for v in row {
                out.extend_from_slice(&if little_endian {
                    v.to_le_bytes()
                } else {
                    v.to_be_bytes()
                });
            }
        }
        out
    }

    /// The revert-proof test for the URL reader's half of the byte-order
    /// contract (cool-japan/oxigeo#14).
    ///
    /// `read_tile_level` itself needs a browser `fetch`, so this drives the
    /// post-decompression pipeline it delegates to, `finish_tile_decode`, over
    /// an `MM` tile and its byte-identical `II` twin. The same logical raster
    /// must decode to the same samples from both, with and without a predictor.
    ///
    /// Delete the `normalize_samples_to_native` call from `finish_tile_decode`
    /// and the `MM` case decodes to byte-swapped garbage while `II` stays
    /// correct, so the equality assertion fails. Re-introduce a *second* swap
    /// anywhere above it and it fails the same way.
    #[test]
    fn finish_tile_decode_is_byte_order_agnostic() {
        // No value is byte-palindromic, so a missed or doubled swap cannot
        // coincidentally produce the right number.
        let values: [[u16; 2]; 2] = [[0x7FFF, 100], [4000, 0x0102]];
        let expected: Vec<u16> = values.iter().flatten().copied().collect();

        for predictor in [1u16, 2] {
            let lvl = level_16bit(2, 2, predictor);

            let mut le = encode_tile_16bit(&values, true, predictor);
            let mut be = encode_tile_16bit(&values, false, predictor);
            assert_ne!(le, be, "an MM fixture identical to II proves nothing");

            finish_tile_decode(&mut le, &lvl, true);
            finish_tile_decode(&mut be, &lvl, false);

            let from_le = bytes_to_u16(&le);
            let from_be = bytes_to_u16(&be);

            assert_eq!(
                from_le, expected,
                "predictor {predictor}: II tile must decode to the written values"
            );
            assert_eq!(
                from_be, expected,
                "predictor {predictor}: MM tile must decode to the written values — \
                 read_tile_level normalises to host order, so the file's byte \
                 order must not survive into the samples"
            );
            assert_eq!(
                from_le, from_be,
                "predictor {predictor}: II and MM encodings of one tile must \
                 yield one result"
            );
        }
    }

    /// Build a synthetic tile whose every pixel encodes its global coordinate
    /// as `gy * 1000 + gx`, so assembly can be checked positionally.
    fn synthetic_tile_u16(tx: u32, ty: u32, tw: u32, th: u32) -> (u32, u32, Vec<u16>) {
        let mut data = Vec::with_capacity((tw * th) as usize);
        for row in 0..th {
            for col in 0..tw {
                let gx = tx * tw + col;
                let gy = ty * th + row;
                data.push((gy * 1000 + gx) as u16);
            }
        }
        (tx, ty, data)
    }

    #[test]
    fn assemble_window_crosses_tiles() {
        // 3x3 grid of 2x2 tiles => 6x6 image. Window [1,1) size 3x3 spans four
        // tiles (0,0),(1,0),(0,1),(1,1).
        let tw = 2u32;
        let th = 2u32;
        let mut tiles = Vec::new();
        for ty in 0..3 {
            for tx in 0..3 {
                tiles.push(synthetic_tile_u16(tx, ty, tw, th));
            }
        }
        let (x0, y0, w, h) = (1u64, 1u64, 3u32, 3u32);
        let out = assemble_window(&tiles, tw, th, x0, y0, w, h);
        assert_eq!(out.len(), (w * h) as usize);
        for oy in 0..h as u64 {
            for ox in 0..w as u64 {
                let gx = x0 + ox;
                let gy = y0 + oy;
                let expect = (gy * 1000 + gx) as u16;
                assert_eq!(
                    out[(oy * w as u64 + ox) as usize],
                    expect,
                    "mismatch at window ({ox},{oy}) global ({gx},{gy})"
                );
            }
        }
    }

    #[test]
    fn assemble_window_off_grid_crop_zero_fills() {
        // Only tile (0,0) of a 2x2 tile grid is supplied; a 3x3 window at the
        // origin overhangs the tile. Covered pixels carry data; the rest are 0.
        let tw = 2u32;
        let th = 2u32;
        let tiles = vec![synthetic_tile_u16(0, 0, tw, th)];
        let out = assemble_window(&tiles, tw, th, 0, 0, 3, 3);
        assert_eq!(out.len(), 9);
        // In-tile region (x<2, y<2) matches gy*1000+gx.
        assert_eq!(out[0], 0); // (0,0)
        assert_eq!(out[1], 1); // (1,0)
        assert_eq!(out[3], 1000); // (0,1)
        assert_eq!(out[4], 1001); // (1,1)
        // Overhang columns/rows are zero.
        assert_eq!(out[2], 0); // (2,0) no tile
        assert_eq!(out[5], 0); // (2,1)
        assert_eq!(out[6], 0); // (0,2)
        assert_eq!(out[7], 0); // (1,2)
        assert_eq!(out[8], 0); // (2,2)
    }

    #[test]
    fn assemble_window_rgb8_crops() {
        // Single 2x2 RGB tile; window offset by (1,1) with size 2x2 overhangs
        // to the right and bottom, leaving one covered pixel.
        let tw = 2u32;
        let th = 2u32;
        let data: Vec<u8> = vec![
            1, 2, 3, // (0,0)
            4, 5, 6, // (1,0)
            7, 8, 9, // (0,1)
            10, 11, 12, // (1,1)
        ];
        let tiles = vec![(0u32, 0u32, data)];
        let out = assemble_window_rgb8(&tiles, tw, th, 1, 1, 2, 2);
        assert_eq!(out.len(), 2 * 2 * 3);
        // Output pixel (0,0) maps to global (1,1) => tile pixel (1,1) = 10,11,12.
        assert_eq!(&out[0..3], &[10, 11, 12]);
        // The other three output pixels overhang the tile => zero.
        assert_eq!(&out[3..12], &[0u8; 9]);
    }

    #[test]
    fn assemble_window_is_order_independent() {
        // Regression test for concurrent tile fetching: `read_window_u16` now
        // gathers tiles via `buffer_unordered`, so they can arrive in any
        // order. `assemble_window` places each tile by its own (tx, ty), so
        // shuffling the input vector must not change the output.
        let tw = 2u32;
        let th = 2u32;
        let mut tiles = Vec::new();
        for ty in 0..3 {
            for tx in 0..3 {
                tiles.push(synthetic_tile_u16(tx, ty, tw, th));
            }
        }
        let (x0, y0, w, h) = (1u64, 1u64, 3u32, 3u32);
        let sequential = assemble_window(&tiles, tw, th, x0, y0, w, h);

        // Reverse order (as if the last tile requested finished first).
        let mut reversed = tiles.clone();
        reversed.reverse();
        assert_eq!(assemble_window(&reversed, tw, th, x0, y0, w, h), sequential);

        // An interleaved/shuffled order (odd indices first, then even).
        let mut shuffled: Vec<_> = tiles.iter().skip(1).step_by(2).cloned().collect();
        shuffled.extend(tiles.iter().step_by(2).cloned());
        assert_eq!(assemble_window(&shuffled, tw, th, x0, y0, w, h), sequential);
    }

    #[test]
    fn assemble_window_rgb8_is_order_independent() {
        let tw = 2u32;
        let th = 2u32;
        let tiles: Vec<(u32, u32, Vec<u8>)> = vec![
            (0, 0, vec![1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 4]),
            (1, 0, vec![5, 5, 5, 6, 6, 6, 7, 7, 7, 8, 8, 8]),
        ];
        let sequential = assemble_window_rgb8(&tiles, tw, th, 0, 0, 4, 2);

        let mut reversed = tiles.clone();
        reversed.reverse();
        assert_eq!(
            assemble_window_rgb8(&reversed, tw, th, 0, 0, 4, 2),
            sequential
        );
    }

    /// Drives an iterator of coordinates through the exact same
    /// `stream::iter(...).buffer_unordered(N)` shape used by
    /// `read_window_u16`/`read_window_rgb8`, with a fake per-tile fetch
    /// whose futures are ready in reverse submission order (later
    /// coordinates resolve first via `future::ready` reordering below), to
    /// prove the concurrency change can't scramble which bytes end up at
    /// which (tx, ty) — this ordering-insensitivity is what makes concurrent
    /// (rather than strictly sequential) fetching safe here.
    #[test]
    fn concurrent_tile_gather_preserves_coordinate_mapping() {
        futures::executor::block_on(async {
            // Deliberately built in reverse (ty, tx) order to stand in for
            // fetches completing out of their original request order.
            let coords: Vec<(u32, u32)> = (0..3)
                .rev()
                .flat_map(|ty| (0..3).rev().map(move |tx| (tx, ty)))
                .collect();

            let fetches = stream::iter(coords.iter().copied().map(|(tx, ty)| async move {
                Ok::<_, OxiGeoError>((tx, ty, vec![(ty * 1000 + tx) as u16]))
            }))
            .buffer_unordered(MAX_CONCURRENT_TILE_FETCHES)
            .collect::<Vec<_>>()
            .await;

            let mut tiles: Vec<(u32, u32, Vec<u16>)> = Vec::with_capacity(fetches.len());
            for result in fetches {
                tiles.push(result.expect("fake fetch never errors"));
            }

            // Every requested coordinate is present exactly once, each still
            // carrying its own correct payload, regardless of completion order.
            assert_eq!(tiles.len(), coords.len());
            for (tx, ty) in coords {
                let found = tiles
                    .iter()
                    .find(|(ttx, tty, _)| *ttx == tx && *tty == ty)
                    .unwrap_or_else(|| panic!("missing tile ({tx},{ty})"));
                assert_eq!(found.2, vec![(ty * 1000 + tx) as u16]);
            }
        });
    }

    /// `bytes_to_u16` is host-native by construction — it has no byte-order
    /// parameter left to get wrong. Feeding it native bytes must round-trip.
    #[test]
    fn bytes_to_u16_is_host_native() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0x0201u16.to_ne_bytes());
        bytes.extend_from_slice(&0x00FFu16.to_ne_bytes());
        assert_eq!(bytes_to_u16(&bytes), vec![0x0201, 0x00FF]);
        // A trailing odd byte is dropped, not misread.
        bytes.push(0xAB);
        assert_eq!(bytes_to_u16(&bytes), vec![0x0201, 0x00FF]);
    }

    #[test]
    fn decompress_tile_passthrough_and_errors() {
        let raw = vec![1u8, 2, 3, 4];
        assert_eq!(decompress_tile(raw.clone(), 1, raw.len()).unwrap(), raw);
        assert!(decompress_tile(raw, 99, 4).is_err()); // unknown codec
    }

    /// Compression code `8` (DEFLATE) and the older-code alias `32946` wrap
    /// the exact same zlib stream layout — both must decode identically.
    #[test]
    fn decompress_tile_zlib_and_old_style_deflate_code_agree() {
        let original: Vec<u8> = (0..64u8).cycle().take(500).collect();
        let compressed = oxiarc_deflate::zlib_compress(&original, 6).unwrap();

        assert_eq!(
            decompress_tile(compressed.clone(), 8, original.len()).unwrap(),
            original
        );
        assert_eq!(
            decompress_tile(compressed, 32946, original.len()).unwrap(),
            original
        );
    }

    /// Compression code `5` (LZW) round-trips through `oxiarc_lzw`'s own
    /// TIFF-flavoured compressor.
    #[test]
    fn decompress_tile_lzw_round_trip() {
        let original: Vec<u8> = b"This is a test of LZW tile decompression! ".repeat(4);
        let compressed = oxiarc_lzw::compress_tiff(&original).unwrap();

        assert_eq!(
            decompress_tile(compressed, 5, original.len()).unwrap(),
            original
        );
    }

    /// Compression code `32773` (PackBits): a hand-built stream mixing a
    /// literal run and a repeat run, matching TIFF 6.0 §9.
    #[test]
    fn decompress_tile_packbits_round_trip() {
        // n=2 -> 3 literal bytes (10, 20, 30); n=-4 -> repeat 0xAA 5 times.
        let compressed = vec![2u8, 10, 20, 30, (-4i8) as u8, 0xAA];
        let expected = vec![10u8, 20, 30, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA];

        assert_eq!(
            decompress_tile(compressed, 32773, expected.len()).unwrap(),
            expected
        );
    }

    /// The PackBits no-op control byte (`0x80`, i.e. `i8::MIN`) consumes no
    /// following byte and contributes nothing to the output.
    #[test]
    fn decompress_tile_packbits_noop_byte_is_skipped() {
        // 0x80 -> no-op; n=1 -> 2 literal bytes (9, 5).
        let compressed = vec![0x80u8, 1u8, 9, 5];
        let expected = vec![9u8, 5];

        assert_eq!(
            decompress_tile(compressed, 32773, expected.len()).unwrap(),
            expected
        );
    }

    /// A PackBits literal run whose declared count reaches past the end of
    /// the buffer is truncated/hostile input and must error, not panic.
    #[test]
    fn decompress_tile_packbits_truncated_literal_errors_not_panics() {
        // n=4 declares a 5-byte literal run but only 2 bytes follow.
        let compressed = vec![4u8, 1, 2];
        assert!(decompress_tile(compressed, 32773, 10).is_err());
    }

    /// A PackBits repeat-run control byte with no following byte to repeat
    /// must error, not panic.
    #[test]
    fn decompress_tile_packbits_truncated_repeat_errors_not_panics() {
        let compressed = vec![(-1i8) as u8];
        assert!(decompress_tile(compressed, 32773, 10).is_err());
    }

    /// A PackBits stream that would decode past `expected_size` is truncated
    /// to it rather than over-allocating on a hostile/mismatched size hint.
    #[test]
    fn decompress_tile_packbits_caps_output_at_expected_size() {
        // n=-127 -> repeat 128 times, but expected_size caps it at 5.
        let compressed = vec![(-127i8) as u8, 0x42];
        let out = decompress_tile(compressed, 32773, 5).unwrap();
        assert_eq!(out, vec![0x42u8; 5]);
    }

    /// `expected_size` is a geometry-derived hint, not a guarantee — a
    /// strip-based TIFF's last strip is legitimately shorter than
    /// `rows_per_strip` implies (the same `tile_height` is reused for every
    /// strip in a level, so it necessarily overestimates the final one). A
    /// properly EOI-terminated LZW stream that produces fewer bytes than that
    /// inflated hint must still succeed instead of erroring.
    #[test]
    fn decompress_tile_lzw_short_stream_under_inflated_expected_size_succeeds() {
        let original = b"short strip".to_vec();
        let compressed = oxiarc_lzw::compress_tiff(&original).unwrap();

        // Ask for far more than the stream actually encodes; EOI must still
        // end decoding cleanly rather than surfacing UnexpectedEof.
        let inflated_expected_size = original.len() * 10;
        let out = decompress_tile(compressed, 5, inflated_expected_size).unwrap();
        assert_eq!(out, original);
    }

    /// A tile whose decompressed buffer is *shorter* than its nominal tile
    /// geometry implies (a legitimately short last strip, or hostile
    /// truncated input that a codec accepted without erroring — see the
    /// PackBits/LZW round-trip tests above) must not panic when it flows
    /// through the rest of the decode pipeline with a horizontal predictor.
    /// `apply_horizontal_predictor` and `normalize_samples_to_native` must
    /// both stop cleanly at the buffer's actual length rather than indexing
    /// past it.
    #[test]
    fn decompress_tile_short_output_survives_finish_tile_decode_with_predictor() {
        let lvl = level_16bit(4, 4, 2); // nominal 4x4 16-bit tile, predictor=2
        let nominal_size = expected_tile_byte_size(&lvl);
        assert_eq!(nominal_size, 32); // 4*4*1*16 bits = 32 bytes

        // A PackBits stream that decodes to only 10 bytes: far short of the
        // 32-byte nominal tile size.
        let compressed = vec![9u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]; // n=9 -> 10 literal bytes
        let mut decompressed = decompress_tile(compressed, 32773, nominal_size).unwrap();
        assert_eq!(decompressed.len(), 10);

        // Must not panic despite being shorter than the tile's nominal size.
        finish_tile_decode(&mut decompressed, &lvl, true);
    }

    /// The GeoTIFF `ModelPixelScaleTag` is spec'd as strictly positive; a
    /// fixture mimicking a nonconforming writer that stored a negative
    /// (northing-signed) ScaleY must come out positive, matching the
    /// `openBytes` path's `.abs()`-normalised convention.
    #[test]
    fn normalize_pixel_scale_y_rejects_negative_sign() {
        assert_eq!(normalize_pixel_scale_y(-0.000_089_831_5), 0.000_089_831_5);
        assert_eq!(normalize_pixel_scale_y(0.000_089_831_5), 0.000_089_831_5);
        assert_eq!(normalize_pixel_scale_y(0.0), 0.0);
    }

    /// `expected_tile_byte_size` is the size hint threaded into LZW/PackBits
    /// decode; pin it against a hand-computed byte count for a representative
    /// multi-band tile.
    #[test]
    fn expected_tile_byte_size_matches_hand_computed() {
        let mut lvl = level_16bit(256, 256, 1);
        // 256*256*1*16 bits = 1_048_576 bits = 131_072 bytes.
        assert_eq!(expected_tile_byte_size(&lvl), 131_072);

        lvl.samples_per_pixel = 3;
        lvl.bits_per_sample = 8;
        // 256*256*3*8 bits = 1_572_864 bits = 196_608 bytes.
        assert_eq!(expected_tile_byte_size(&lvl), 196_608);
    }

    // ---------------------------------------------------------------------
    // IFD chain walk: GDAL internal masks are not pyramid levels
    // ---------------------------------------------------------------------

    /// In-memory [`RangeSource`] standing in for `FetchBackend` so the IFD
    /// chain walk can run natively (`web_sys::fetch` is browser-only).
    ///
    /// Reads past the end return the available prefix, exactly as an HTTP
    /// server does for an over-long `Range` — the parser always asks for a
    /// fixed 4 KiB window, which overruns these small fixtures.
    struct BytesSource(Vec<u8>);

    impl RangeSource for BytesSource {
        fn read_range(&self, range: ByteRange) -> impl Future<Output = Result<Vec<u8>>> {
            let len = self.0.len();
            let start = usize::try_from(range.start).unwrap_or(usize::MAX).min(len);
            let end = usize::try_from(range.end).unwrap_or(usize::MAX).min(len);
            let bytes = self.0[start..end.max(start)].to_vec();
            async move { Ok(bytes) }
        }
    }

    /// One synthetic IFD. Every image is exactly one tile, so a `count == 1`
    /// TileOffsets/TileByteCounts array is the truthful encoding.
    #[derive(Clone, Copy)]
    struct IfdSpec {
        size: u32,
        subfile_type: u32,
        photometric: u16,
        tile_offset: u32,
        tile_byte_count: u32,
    }

    impl IfdSpec {
        /// A full-resolution or reduced-resolution image IFD.
        fn image(size: u32, subfile_type: u32) -> Self {
            Self {
                size,
                subfile_type,
                photometric: 1, // BlackIsZero
                tile_offset: 100_000 + size,
                tile_byte_count: 4_096 + size,
            }
        }

        /// A GDAL internal mask marked the way GDAL marks it: NewSubfileType
        /// bit 2 set, PhotometricInterpretation left at BlackIsZero.
        fn mask_by_subfile_type(size: u32) -> Self {
            Self {
                subfile_type: 0x4,
                ..Self::image(size, 0)
            }
        }

        /// A mask marked *only* by PhotometricInterpretation 4 — some writers
        /// omit the NewSubfileType bit, so either marker must be conclusive.
        fn mask_by_photometric(size: u32) -> Self {
            Self {
                photometric: PHOTOMETRIC_TRANSPARENCY_MASK,
                ..Self::image(size, 0)
            }
        }
    }

    /// Little-endian IFD entry: tag, field type, count, inline value.
    ///
    /// TIFF left-justifies a value shorter than 4 bytes in the value field,
    /// which is what `read_value` expects for SHORT.
    fn ifd_entry(tag: u16, field_type: u16, count: u32, value: u32) -> Vec<u8> {
        let mut entry = Vec::with_capacity(12);
        entry.extend_from_slice(&tag.to_le_bytes());
        entry.extend_from_slice(&field_type.to_le_bytes());
        entry.extend_from_slice(&count.to_le_bytes());
        match field_type {
            3 => {
                // SHORT: two value bytes, then two bytes of padding.
                entry.extend_from_slice(&(value as u16).to_le_bytes());
                entry.extend_from_slice(&[0, 0]);
            }
            _ => entry.extend_from_slice(&value.to_le_bytes()),
        }
        entry
    }

    const TIFF_SHORT: u16 = 3;
    const TIFF_LONG: u16 = 4;

    /// Serialise one IFD (entries in ascending tag order, then the link).
    fn build_ifd(spec: &IfdSpec, next_ifd_offset: u32) -> Vec<u8> {
        let entries = [
            ifd_entry(TAG_NEW_SUBFILE_TYPE, TIFF_LONG, 1, spec.subfile_type),
            ifd_entry(TAG_IMAGE_WIDTH, TIFF_LONG, 1, spec.size),
            ifd_entry(TAG_IMAGE_LENGTH, TIFF_LONG, 1, spec.size),
            ifd_entry(259, TIFF_SHORT, 1, 1), // Compression: none
            ifd_entry(262, TIFF_SHORT, 1, u32::from(spec.photometric)),
            ifd_entry(TAG_TILE_WIDTH, TIFF_SHORT, 1, spec.size),
            ifd_entry(TAG_TILE_LENGTH, TIFF_SHORT, 1, spec.size),
            ifd_entry(324, TIFF_LONG, 1, spec.tile_offset),
            ifd_entry(325, TIFF_LONG, 1, spec.tile_byte_count),
        ];

        let mut out = Vec::new();
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for entry in &entries {
            out.extend_from_slice(entry);
        }
        out.extend_from_slice(&next_ifd_offset.to_le_bytes());
        out
    }

    /// Bytes reserved per IFD in the fixture; comfortably above the 114 bytes
    /// a nine-entry IFD actually occupies.
    const IFD_STRIDE: u32 = 256;
    /// Offset of the first IFD (right after the 8-byte header, padded).
    const FIRST_IFD: u32 = 16;

    /// Lay out a classic little-endian TIFF whose IFDs form `specs` in order.
    ///
    /// The `next` link of each IFD holds an **absolute** file offset, matching
    /// what the parser expects and how `open` uses the returned value.
    fn build_tiff_chain(specs: &[IfdSpec]) -> Vec<u8> {
        let total = FIRST_IFD + IFD_STRIDE * specs.len() as u32;
        let mut buf = vec![0u8; total as usize];
        buf[0..2].copy_from_slice(b"II");
        buf[2..4].copy_from_slice(&42u16.to_le_bytes());
        buf[4..8].copy_from_slice(&FIRST_IFD.to_le_bytes());

        for (i, spec) in specs.iter().enumerate() {
            let at = (FIRST_IFD + IFD_STRIDE * i as u32) as usize;
            let next = if i + 1 == specs.len() {
                0
            } else {
                FIRST_IFD + IFD_STRIDE * (i as u32 + 1)
            };
            let ifd = build_ifd(spec, next);
            assert!(ifd.len() <= IFD_STRIDE as usize, "IFD overflows its slot");
            buf[at..at + ifd.len()].copy_from_slice(&ifd);
        }

        buf
    }

    fn walk_chain(specs: &[IfdSpec]) -> CogMetadata {
        let source = BytesSource(build_tiff_chain(specs));
        futures::executor::block_on(WasmCogReader::parse_ifd_chain(
            &source,
            ByteOrder::LittleEndian,
            u64::from(FIRST_IFD),
        ))
        .expect("synthetic chain parses")
    }

    /// Control: with no mask in the chain, all three reduced-resolution IFDs
    /// are overviews. Pins the fixture itself, so the skip tests below can
    /// only fail for the reason they claim.
    #[test]
    fn ifd_chain_without_masks_counts_every_reduced_image() {
        let meta = walk_chain(&[
            IfdSpec::image(512, 0),
            IfdSpec::image(256, 1),
            IfdSpec::image(128, 1),
            IfdSpec::image(64, 1),
        ]);

        assert_eq!(meta.width, 512);
        assert_eq!(meta.overview_count, 3);
        assert_eq!(meta.levels.len(), 4);
        assert_eq!(
            meta.levels.iter().map(|l| l.width).collect::<Vec<_>>(),
            vec![512, 256, 128, 64]
        );
    }

    /// A GDAL internal mask (NewSubfileType bit 2) sits in the same IFD chain
    /// as the overviews. Counting it inflates `overview_count` *and* shifts
    /// every later level's index, so `levels[2]` would hand back the mask's
    /// tile directory instead of the next real resolution.
    #[test]
    fn ifd_chain_skips_mask_marked_by_new_subfile_type() {
        let meta = walk_chain(&[
            IfdSpec::image(512, 0),
            IfdSpec::image(256, 1),
            IfdSpec::mask_by_subfile_type(512),
            IfdSpec::image(128, 1),
        ]);

        assert_eq!(meta.overview_count, 2, "mask counted as an overview level");
        assert_eq!(meta.levels.len(), 3);
        // Indices are unshifted: level 2 is the 128px overview, not the mask.
        assert_eq!(
            meta.levels.iter().map(|l| l.width).collect::<Vec<_>>(),
            vec![512, 256, 128]
        );
        assert_eq!(
            meta.overviews.iter().map(|o| o.width).collect::<Vec<_>>(),
            vec![256, 128]
        );
    }

    /// The same skip must happen when the only marker is
    /// `PhotometricInterpretation == 4`, and the chain must keep being walked
    /// *through* the mask — the 128px overview after it is still found.
    #[test]
    fn ifd_chain_skips_mask_marked_only_by_photometric() {
        let meta = walk_chain(&[
            IfdSpec::image(512, 0),
            IfdSpec::image(256, 1),
            IfdSpec::mask_by_photometric(512),
            IfdSpec::image(128, 1),
        ]);

        assert_eq!(meta.overview_count, 2);
        assert_eq!(
            meta.levels.iter().map(|l| l.width).collect::<Vec<_>>(),
            vec![512, 256, 128]
        );
    }

    /// Both markers together (what GDAL actually writes) still skips exactly
    /// once, and a mask in the final position doesn't truncate the chain.
    #[test]
    fn ifd_chain_skips_trailing_mask_with_both_markers() {
        let mut mask = IfdSpec::mask_by_subfile_type(512);
        mask.photometric = PHOTOMETRIC_TRANSPARENCY_MASK;

        let meta = walk_chain(&[IfdSpec::image(512, 0), IfdSpec::image(256, 1), mask]);

        assert_eq!(meta.overview_count, 1);
        assert_eq!(meta.levels.len(), 2);
    }

    /// The canonical `CreateMaskBand` layout: the full-resolution mask sits
    /// immediately after the primary IFD, ahead of every overview. The first
    /// iteration of the walk must skip it, or `levels[1]` is the mask and each
    /// overview index is off by one.
    #[test]
    fn ifd_chain_skips_mask_directly_after_the_primary_ifd() {
        let meta = walk_chain(&[
            IfdSpec::image(512, 0),
            IfdSpec::mask_by_subfile_type(512),
            IfdSpec::image(256, 1),
            IfdSpec::image(128, 1),
        ]);

        assert_eq!(meta.overview_count, 2);
        assert_eq!(
            meta.levels.iter().map(|l| l.width).collect::<Vec<_>>(),
            vec![512, 256, 128]
        );
    }

    /// The pure classifier behind the walk: either marker alone is
    /// conclusive, and an ordinary reduced-resolution IFD (`subfile_type == 1`)
    /// is never a mask.
    #[test]
    fn is_mask_ifd_accepts_either_marker() {
        assert!(!is_mask_ifd(0, 1));
        assert!(!is_mask_ifd(1, 1)); // plain overview
        assert!(is_mask_ifd(0x4, 1)); // NewSubfileType bit 2 only
        assert!(is_mask_ifd(0x5, 1)); // reduced-resolution + mask
        assert!(is_mask_ifd(0, PHOTOMETRIC_TRANSPARENCY_MASK)); // photometric only
    }

    // ---------------------------------------------------------------------
    // Per-level tile addressing
    // ---------------------------------------------------------------------

    /// `read_tile_level` must address the requested level's *own* tile grid
    /// and tile directory. Before the fix, `WasmCogViewer::read_tile` dropped
    /// its `level` argument and called the level-0 shortcut, so a level-1
    /// request fetched level-0 bytes — a silent wrong-resolution read.
    ///
    /// The viewer's URL path itself needs `web_sys::fetch` and cannot run
    /// natively; this pins the geometry/directory resolution it depends on.
    #[test]
    fn tile_byte_range_resolves_against_the_requested_level() {
        let meta = walk_chain(&[IfdSpec::image(512, 0), IfdSpec::image(256, 1)]);
        assert_eq!(meta.levels.len(), 2);

        let level0 = tile_byte_range(&meta.levels[0], 0, 0, 0).expect("level 0 tile (0,0)");
        let level1 = tile_byte_range(&meta.levels[1], 1, 0, 0).expect("level 1 tile (0,0)");

        // Each level's range comes from its own tile directory, so the two are
        // disjoint — a level-1 read can never return level-0 bytes.
        assert_eq!(level0.start, u64::from(IfdSpec::image(512, 0).tile_offset));
        assert_eq!(level1.start, u64::from(IfdSpec::image(256, 1).tile_offset));
        assert_ne!(level0.start, level1.start);
        assert_ne!(level0.len(), level1.len());
    }

    /// Tile indexing uses the level's own grid width: on a 4x4-tile level,
    /// tile (1, 1) is index 5, and the level-1 grid (2x2 tiles) puts the same
    /// coordinate at index 3. A level mix-up therefore reads the wrong tile
    /// even when both levels are in range.
    #[test]
    fn tile_byte_range_indexes_the_levels_own_grid() {
        let mut level0 = level_16bit(256, 256, 1);
        level0.width = 1024;
        level0.height = 1024; // 4x4 tiles
        level0.tile_offsets = (0..16).map(|i| 1_000 + i * 10).collect();
        level0.tile_byte_counts = vec![10; 16];

        let mut level1 = level_16bit(256, 256, 1);
        level1.width = 512;
        level1.height = 512; // 2x2 tiles
        level1.tile_offsets = (0..4).map(|i| 9_000 + i * 10).collect();
        level1.tile_byte_counts = vec![10; 4];

        // (1,1) -> index 1*4 + 1 = 5 at level 0, and 1*2 + 1 = 3 at level 1.
        assert_eq!(
            tile_byte_range(&level0, 0, 1, 1)
                .expect("level 0 (1,1)")
                .start,
            1_050
        );
        assert_eq!(
            tile_byte_range(&level1, 1, 1, 1)
                .expect("level 1 (1,1)")
                .start,
            9_030
        );

        // Off-grid coordinates error rather than silently reading a neighbour.
        assert!(tile_byte_range(&level1, 1, 3, 3).is_err());
    }
}
