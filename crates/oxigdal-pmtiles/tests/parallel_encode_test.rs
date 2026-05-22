//! Integration tests for the parallel tile encoding pipeline.
//!
//! All tests require both the `parallel` and `compression` features. They are
//! gated with `#[cfg(all(feature = "parallel", feature = "compression"))]` so
//! that the test binary compiles (but skips these items) when those features
//! are absent.

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::panic)]

#[cfg(all(feature = "parallel", feature = "compression"))]
mod tests {
    use oxigdal_pmtiles::{
        Compression, ParallelEncodeConfig, PmTilesBuilder, PmTilesReader, RawTile, TileType,
        build_pmtiles_parallel, compress_tile, compress_tiles_parallel, zxy_to_tile_id,
    };

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    /// Build a [`RawTile`] from a tile ID and a byte slice.
    fn make_raw_tile(tile_id: u64, content: &[u8]) -> RawTile {
        RawTile {
            tile_id,
            raw_data: content.to_vec(),
        }
    }

    /// Produce `n` bytes with a repeating pattern that compresses well.
    fn repetitive_data(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i % 7) as u8).collect()
    }

    /// Decompress gzip bytes using the OxiARC gzip codec (pure Rust).
    fn decompress_gzip(data: &[u8]) -> Vec<u8> {
        let mut cursor = std::io::Cursor::new(data);
        oxiarc_archive::gzip::decompress(&mut cursor)
            .expect("gzip decompression should succeed in tests")
    }

    // -----------------------------------------------------------------------
    // 1. compress_tile: gzip round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn test_compress_tile_gzip_round_trip() {
        let original = b"Hello, PMTiles! This is a round-trip test payload.";
        let compressed =
            compress_tile(original, Compression::Gzip).expect("gzip compression should succeed");
        assert_ne!(
            compressed.as_slice(),
            original,
            "compressed bytes should differ from the input"
        );
        let decompressed = decompress_gzip(&compressed);
        assert_eq!(
            decompressed.as_slice(),
            original,
            "decompressed bytes must match the original input"
        );
    }

    // -----------------------------------------------------------------------
    // 2. compress_tile: Compression::None is a pass-through
    // -----------------------------------------------------------------------

    #[test]
    fn test_compress_tile_none_returns_input() {
        let data = b"no compression here";
        let result =
            compress_tile(data, Compression::None).expect("None compression should not fail");
        assert_eq!(
            result.as_slice(),
            data,
            "None compression must return the input unchanged"
        );
    }

    // -----------------------------------------------------------------------
    // 3. compress_tiles_parallel: ordering after sort matches input tile IDs
    // -----------------------------------------------------------------------

    #[test]
    fn test_compress_tiles_parallel_preserves_order() {
        let tiles: Vec<RawTile> = (0u64..10)
            .map(|id| make_raw_tile(id, &[id as u8; 32]))
            .collect();
        let config = ParallelEncodeConfig::default();
        let mut result = compress_tiles_parallel(&tiles, Compression::Gzip, &config)
            .expect("parallel compression should succeed");

        // After sorting by tile_id the order must match the input ascending IDs.
        result.sort_unstable_by_key(|(id, _)| *id);

        assert_eq!(result.len(), 10, "all 10 tiles should be in the output");
        for (i, (id, _)) in result.iter().enumerate() {
            assert_eq!(*id, i as u64, "tile_id at index {i} should be {i}");
        }
    }

    // -----------------------------------------------------------------------
    // 4. compress_tiles_parallel: empty input → empty output
    // -----------------------------------------------------------------------

    #[test]
    fn test_compress_tiles_parallel_empty_input() {
        let config = ParallelEncodeConfig::default();
        let result = compress_tiles_parallel(&[], Compression::Gzip, &config)
            .expect("empty input should not fail");
        assert!(result.is_empty(), "empty input must yield empty output");
    }

    // -----------------------------------------------------------------------
    // 5. compress_tiles_parallel: single tile compresses correctly
    // -----------------------------------------------------------------------

    #[test]
    fn test_compress_tiles_parallel_single_tile() {
        let payload = repetitive_data(256);
        let tiles = vec![make_raw_tile(42, &payload)];
        let config = ParallelEncodeConfig::default();
        let result = compress_tiles_parallel(&tiles, Compression::Gzip, &config)
            .expect("single-tile compression should succeed");

        assert_eq!(result.len(), 1, "exactly one result expected");
        let (tile_id, compressed) = &result[0];
        assert_eq!(*tile_id, 42, "tile_id must be preserved");
        // Verify the compressed output decompresses back to the original.
        let decompressed = decompress_gzip(compressed);
        assert_eq!(
            decompressed, payload,
            "round-trip must match for single tile"
        );
    }

    // -----------------------------------------------------------------------
    // 6. compress_tiles_parallel: chunk_size=1, all tiles appear in output
    // -----------------------------------------------------------------------

    #[test]
    fn test_compress_tiles_parallel_chunk_size_one() {
        let tiles: Vec<RawTile> = (0u64..5)
            .map(|id| make_raw_tile(id, &[id as u8; 16]))
            .collect();
        let config = ParallelEncodeConfig {
            chunk_size: 1,
            threads: None,
        };
        let mut result = compress_tiles_parallel(&tiles, Compression::Gzip, &config)
            .expect("chunk_size=1 compression should succeed");
        result.sort_unstable_by_key(|(id, _)| *id);

        assert_eq!(result.len(), 5, "all 5 tiles must appear in output");
        for (i, (id, _)) in result.iter().enumerate() {
            assert_eq!(*id, i as u64);
        }
    }

    // -----------------------------------------------------------------------
    // 7. compress_tiles_parallel: chunk_size > input length
    // -----------------------------------------------------------------------

    #[test]
    fn test_compress_tiles_parallel_chunk_size_larger_than_input() {
        let tiles: Vec<RawTile> = (0u64..3)
            .map(|id| make_raw_tile(id, &[id as u8; 8]))
            .collect();
        let config = ParallelEncodeConfig {
            chunk_size: 100,
            threads: None,
        };
        let result = compress_tiles_parallel(&tiles, Compression::Gzip, &config)
            .expect("oversized chunk_size should succeed");
        assert_eq!(
            result.len(),
            3,
            "all 3 tiles must be present even with oversized chunk"
        );
    }

    // -----------------------------------------------------------------------
    // 8. build_pmtiles_parallel: three tiles produce a non-empty archive
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_pmtiles_parallel_three_tiles_produces_valid_archive() {
        let tiles = vec![
            make_raw_tile(0, b"tile-zero"),
            make_raw_tile(1, b"tile-one"),
            make_raw_tile(4, b"tile-four"),
        ];
        let builder = PmTilesBuilder::new(TileType::Mvt, 0, 2);
        let config = ParallelEncodeConfig::default();
        let (archive, _stats) = build_pmtiles_parallel(tiles, Compression::Gzip, builder, &config)
            .expect("build should succeed with 3 tiles");

        assert!(
            archive.len() > 127,
            "archive must be larger than the bare header"
        );
    }

    // -----------------------------------------------------------------------
    // 9. build_pmtiles_parallel: stats show compression_ratio < 1.0 for
    //    repetitive (highly compressible) data
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_pmtiles_parallel_stats_compression_ratio_below_1_for_repetitive_data() {
        // Use a payload that is large enough to compress meaningfully.
        let payload = repetitive_data(4096);
        let tiles: Vec<RawTile> = (0u64..8).map(|id| make_raw_tile(id, &payload)).collect();
        let builder = PmTilesBuilder::new(TileType::Mvt, 0, 3);
        let config = ParallelEncodeConfig::default();
        let (_archive, stats) = build_pmtiles_parallel(tiles, Compression::Gzip, builder, &config)
            .expect("build with repetitive data should succeed");

        assert!(
            stats.compression_ratio < 1.0,
            "highly repetitive input should compress to less than 100% of raw size; \
             got ratio={:.4}",
            stats.compression_ratio
        );
        assert_eq!(stats.tiles_encoded, 8, "all 8 tiles should be counted");
    }

    // -----------------------------------------------------------------------
    // 10. build_pmtiles_parallel: round-trip via PmTilesReader
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_pmtiles_parallel_round_trip_via_reader() {
        // Prepare tiles using known (z, x, y) coordinates → tile IDs via
        // `zxy_to_tile_id` so the Hilbert IDs are valid.
        let tile_specs: &[(u8, u32, u32, &[u8])] = &[
            (0, 0, 0, b"z0/x0/y0 tile content"),
            (1, 0, 0, b"z1/x0/y0 tile content"),
            (1, 1, 0, b"z1/x1/y0 tile content"),
        ];

        let raw_tiles: Vec<RawTile> = tile_specs
            .iter()
            .map(|(z, x, y, data)| {
                let tile_id =
                    zxy_to_tile_id(*z, *x, *y).expect("valid zxy should produce a tile_id");
                make_raw_tile(tile_id, data)
            })
            .collect();

        let builder = PmTilesBuilder::new(TileType::Mvt, 0, 1);
        let config = ParallelEncodeConfig::default();
        let (archive, stats) =
            build_pmtiles_parallel(raw_tiles, Compression::Gzip, builder, &config)
                .expect("round-trip build should succeed");

        assert!(
            stats.tiles_encoded > 0,
            "stats should reflect encoded tiles"
        );

        // Parse the archive back and verify we can fetch each tile.
        let reader = PmTilesReader::from_bytes(archive)
            .expect("PmTilesReader should parse the archive produced by parallel build");

        assert_eq!(
            reader.header.addressed_tiles,
            tile_specs.len() as u64,
            "archive should record the correct number of addressed tiles"
        );
        assert_eq!(reader.header.spec_version, 3, "spec version must be 3");

        // Fetch every tile by (z, x, y) with transparent decompression and
        // verify that the decompressed content matches the original raw payload.
        for (z, x, y, expected_data) in tile_specs {
            let tile_bytes = reader
                .get_tile_decompressed(*z, *x, *y)
                .expect("get_tile_decompressed should not error")
                .unwrap_or_else(|| panic!("tile z={z}/x={x}/y={y} should exist in the archive"));
            assert_eq!(
                tile_bytes.as_slice(),
                *expected_data,
                "tile z={z}/x={x}/y={y} payload mismatch after round-trip"
            );
        }
    }

    // -----------------------------------------------------------------------
    // 11. ParallelEncodeConfig: default chunk_size is 64
    // -----------------------------------------------------------------------

    #[test]
    fn test_parallel_encode_config_default_chunk_size_64() {
        let cfg = ParallelEncodeConfig::default();
        assert_eq!(cfg.chunk_size, 64, "default chunk_size must be 64");
        assert!(cfg.threads.is_none(), "default threads must be None");
    }

    // -----------------------------------------------------------------------
    // 12. build_pmtiles_parallel: explicit thread count runs without panic
    // -----------------------------------------------------------------------

    #[test]
    fn test_parallel_encode_config_with_explicit_threads() {
        let tiles: Vec<RawTile> = (0u64..20)
            .map(|id| make_raw_tile(id, &repetitive_data(512)))
            .collect();
        let builder = PmTilesBuilder::new(TileType::Mvt, 0, 4);
        let config = ParallelEncodeConfig {
            chunk_size: 4,
            threads: Some(2),
        };
        let (archive, stats) = build_pmtiles_parallel(tiles, Compression::Gzip, builder, &config)
            .expect("build with threads=Some(2) should succeed without panic");

        assert!(archive.len() > 127, "archive must be non-trivial");
        assert_eq!(stats.tiles_encoded, 20, "all 20 tiles should be encoded");
        // With chunk_size=4 and 20 tiles: 20/4 = 5 chunks.
        assert_eq!(stats.chunks_processed, 5, "chunks_processed should be 5");
    }
}
