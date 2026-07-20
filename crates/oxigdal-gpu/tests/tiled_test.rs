//! CPU-only tests for tiled raster processing.
//!
//! No GPU device or wgpu adapter is required.  All tile_fn callbacks use a
//! simple passthrough or arithmetic transform so the tests run everywhere.

#![allow(clippy::unwrap_used, clippy::expect_used)]
#![allow(clippy::panic)]

use oxigdal_gpu::{
    RasterTile, TiledConfig, auto_tile_size, execute_tiled, split_into_tiles, stitch_tiles,
    vram_per_tile,
};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Create a synthetic raster of size `w × h` where pixel `(x, y)` has value
/// `(y * w + x) as f32`.  This makes every pixel unique and easy to identify.
fn make_raster(w: usize, h: usize) -> Vec<f32> {
    (0..w * h).map(|i| i as f32).collect()
}

/// Passthrough tile_fn: return tile data unchanged.
fn passthrough(tile: &RasterTile) -> oxigdal_gpu::GpuResult<Vec<f32>> {
    Ok(tile.data.clone())
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. test_split_into_tiles_exact_fit
// ─────────────────────────────────────────────────────────────────────────────

/// 1024 × 1024 raster with 512 × 512 tiles → exactly 4 tiles (2 × 2 grid).
#[test]
fn test_split_into_tiles_exact_fit() {
    let raster = make_raster(1024, 1024);
    let config = TiledConfig::default(); // tile_width=512, tile_height=512

    let tiles = split_into_tiles(&raster, 1024, 1024, &config);

    assert_eq!(
        tiles.len(),
        4,
        "Expected exactly 4 tiles for 1024×1024 / 512×512"
    );

    // Each tile should be exactly 512 × 512 with no overlap.
    for tile in &tiles {
        assert_eq!(tile.width, 512);
        assert_eq!(tile.height, 512);
        assert_eq!(tile.padded_width(), 512); // overlap = 0
        assert_eq!(tile.padded_height(), 512);
    }

    // Verify tile origins: (0,0), (512,0), (0,512), (512,512).
    let origins: Vec<(usize, usize)> = tiles.iter().map(|t| (t.origin_x, t.origin_y)).collect();
    assert!(origins.contains(&(0, 0)));
    assert!(origins.contains(&(512, 0)));
    assert!(origins.contains(&(0, 512)));
    assert!(origins.contains(&(512, 512)));
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. test_split_into_tiles_non_exact_fit
// ─────────────────────────────────────────────────────────────────────────────

/// 100 × 100 raster with 64 × 64 tiles → 4 tiles (2 × 2 grid).
/// Last column tiles have width 36; last row tiles have height 36.
#[test]
fn test_split_into_tiles_non_exact_fit() {
    let raster = make_raster(100, 100);
    let config = TiledConfig::default().with_tile_size(64, 64);

    let tiles = split_into_tiles(&raster, 100, 100, &config);

    // ceil(100/64) = 2 along each axis → 4 tiles.
    assert_eq!(tiles.len(), 4);

    // Collect dimensions keyed by origin.
    for tile in &tiles {
        match (tile.origin_x, tile.origin_y) {
            (0, 0) => {
                assert_eq!(tile.width, 64);
                assert_eq!(tile.height, 64);
            }
            (64, 0) => {
                assert_eq!(tile.width, 36); // 100 - 64
                assert_eq!(tile.height, 64);
            }
            (0, 64) => {
                assert_eq!(tile.width, 64);
                assert_eq!(tile.height, 36);
            }
            (64, 64) => {
                assert_eq!(tile.width, 36);
                assert_eq!(tile.height, 36);
            }
            other => panic!("Unexpected tile origin {:?}", other),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. test_split_overlap_adds_halo_pixels
// ─────────────────────────────────────────────────────────────────────────────

/// Tiles produced with overlap=4 have padded_width = core_width + 8 and
/// padded_height = core_height + 8 (4 pixels on each side).
#[test]
fn test_split_overlap_adds_halo_pixels() {
    let raster = make_raster(256, 256);
    let config = TiledConfig::default()
        .with_tile_size(128, 128)
        .with_overlap(4);

    let tiles = split_into_tiles(&raster, 256, 256, &config);

    // 2 × 2 grid → 4 tiles, each core 128 × 128.
    assert_eq!(tiles.len(), 4);
    for tile in &tiles {
        assert_eq!(tile.overlap_top, 4);
        assert_eq!(tile.overlap_bottom, 4);
        assert_eq!(tile.overlap_left, 4);
        assert_eq!(tile.overlap_right, 4);
        assert_eq!(tile.padded_width(), tile.width + 8);
        assert_eq!(tile.padded_height(), tile.height + 8);
        assert_eq!(tile.data.len(), tile.padded_len());
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. test_split_overlap_edge_replication_at_corners
// ─────────────────────────────────────────────────────────────────────────────

/// The top-left corner tile's top-left halo must contain the value of raster
/// pixel (0, 0) (edge-replication / clamp-to-edge).
#[test]
fn test_split_overlap_edge_replication_at_corners() {
    // 64 × 64 raster, single tile with overlap = 8.
    let raster = make_raster(64, 64);
    let config = TiledConfig::default()
        .with_tile_size(64, 64)
        .with_overlap(8);

    let tiles = split_into_tiles(&raster, 64, 64, &config);
    assert_eq!(tiles.len(), 1);

    let tile = &tiles[0];

    // The top-left corner of padded data (0,0 in padded coords) corresponds
    // to raster (-8, -8) → clamped to (0, 0) → raster value 0.
    assert_eq!(
        tile.data[0], 0.0,
        "Top-left halo corner must replicate raster pixel (0,0)"
    );

    // The top-right corner in padded coords is (padded_width-1, 0), which
    // maps to raster (63+8, -8) clamped to (63, 0) → raster value 63.
    let top_right_idx = tile.padded_width() - 1;
    assert_eq!(
        tile.data[top_right_idx], 63.0,
        "Top-right halo corner must replicate raster pixel (63, 0)"
    );

    // Bottom-left corner: (0, padded_height-1) → raster (-8, 63+8) clamped
    // to (0, 63) → value 63 * 64 = 4032.
    let bottom_left_idx = (tile.padded_height() - 1) * tile.padded_width();
    assert_eq!(
        tile.data[bottom_left_idx],
        (63 * 64) as f32,
        "Bottom-left halo corner must replicate raster pixel (0, 63)"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. test_stitch_reconstructs_identity
// ─────────────────────────────────────────────────────────────────────────────

/// split then stitch with a passthrough tile_fn == original raster.
#[test]
fn test_stitch_reconstructs_identity() {
    let width = 300;
    let height = 200;
    let raster = make_raster(width, height);
    let config = TiledConfig::default().with_tile_size(128, 128);

    let tiles = split_into_tiles(&raster, width, height, &config);
    // Passthrough: return tile.data unchanged.
    let stitched = stitch_tiles(&tiles, width, height);

    assert_eq!(stitched.len(), raster.len());
    for (i, (&orig, &got)) in raster.iter().zip(stitched.iter()).enumerate() {
        assert_eq!(orig, got, "Pixel {i} mismatch: expected {orig}, got {got}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. test_stitch_non_overlapping_tiles
// ─────────────────────────────────────────────────────────────────────────────

/// 200 × 100 raster split into 100 × 100 tiles → 2 tiles, stitch recovers all.
#[test]
fn test_stitch_non_overlapping_tiles() {
    let width = 200;
    let height = 100;
    let raster = make_raster(width, height);
    let config = TiledConfig::default().with_tile_size(100, 100);

    let tiles = split_into_tiles(&raster, width, height, &config);
    assert_eq!(tiles.len(), 2);

    let stitched = stitch_tiles(&tiles, width, height);

    assert_eq!(stitched.len(), raster.len());
    for (i, (&orig, &got)) in raster.iter().zip(stitched.iter()).enumerate() {
        assert_eq!(orig, got, "Pixel {i}: expected {orig}, got {got}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 7. test_vram_per_tile_formula
// ─────────────────────────────────────────────────────────────────────────────

/// `vram_per_tile` == 2 * padded_len * 4 + 256.
#[test]
fn test_vram_per_tile_formula() {
    // Build a tile with known dimensions.
    let tile = RasterTile {
        data: vec![0.0; (64 + 4 + 4) * (64 + 4 + 4)], // padded 72 × 72
        width: 64,
        height: 64,
        overlap_top: 4,
        overlap_right: 4,
        overlap_bottom: 4,
        overlap_left: 4,
        origin_x: 0,
        origin_y: 0,
        raster_width: 64,
        raster_height: 64,
        tile_index: 0,
    };

    let padded_len = tile.padded_len();
    assert_eq!(padded_len, 72 * 72);

    let expected = padded_len * 4 * 2 + 256;
    assert_eq!(vram_per_tile(&tile), expected);
}

// ─────────────────────────────────────────────────────────────────────────────
// 8. test_auto_tile_size_halves_to_fit_budget
// ─────────────────────────────────────────────────────────────────────────────

/// Providing a very small VRAM budget forces `auto_tile_size` to reduce the
/// tile until it fits, falling back to the minimum (16, 16) if necessary.
#[test]
fn test_auto_tile_size_halves_to_fit_budget() {
    // Budget of only 512 bytes — too small for even a 16×16 tile without
    // overlap (16*16*2*4+256 = 2304 bytes).  Expect fallback to (16, 16).
    let (w, h) = auto_tile_size(512, 512, 0, 512, 0.0);
    assert_eq!((w, h), (16, 16));
}

// ─────────────────────────────────────────────────────────────────────────────
// 9. test_auto_tile_size_preferred_fits_in_large_budget
// ─────────────────────────────────────────────────────────────────────────────

/// With a generous VRAM budget the preferred size is kept unchanged.
#[test]
fn test_auto_tile_size_preferred_fits_in_large_budget() {
    // 256 MB budget, safety margin 10 %, preferred 512 × 512, overlap 0.
    // vram_per_tile for 512×512 with no overlap = 512*512*8+256 = 2_097_408 bytes (~2 MB).
    let budget = 256 * 1024 * 1024; // 256 MB
    let (w, h) = auto_tile_size(512, 512, 0, budget, 0.1);
    assert_eq!((w, h), (512, 512));
}

// ─────────────────────────────────────────────────────────────────────────────
// 10. test_execute_tiled_passthrough_matches_original
// ─────────────────────────────────────────────────────────────────────────────

/// `execute_tiled` with identity tile_fn returns the same raster.
#[test]
fn test_execute_tiled_passthrough_matches_original() {
    let width = 256;
    let height = 256;
    let raster = make_raster(width, height);
    let config = TiledConfig::default().with_tile_size(128, 128);

    let result = execute_tiled(&raster, width, height, &config, passthrough).unwrap();

    assert_eq!(result.len(), raster.len());
    for (i, (&orig, &got)) in raster.iter().zip(result.iter()).enumerate() {
        assert_eq!(orig, got, "Pixel {i}: expected {orig}, got {got}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 11. test_execute_tiled_scale_fn_applies_per_tile
// ─────────────────────────────────────────────────────────────────────────────

/// Scaling tile data by 2.0 inside tile_fn → full output is also × 2.
#[test]
fn test_execute_tiled_scale_fn_applies_per_tile() {
    let width = 200;
    let height = 100;
    let raster = make_raster(width, height);
    let config = TiledConfig::default().with_tile_size(100, 100);

    let scale_fn = |tile: &RasterTile| -> oxigdal_gpu::GpuResult<Vec<f32>> {
        let scaled: Vec<f32> = tile.data.iter().map(|&v| v * 2.0).collect();
        Ok(scaled)
    };

    let result = execute_tiled(&raster, width, height, &config, scale_fn).unwrap();

    assert_eq!(result.len(), raster.len());
    for (i, (&orig, &got)) in raster.iter().zip(result.iter()).enumerate() {
        let expected = orig * 2.0;
        assert_eq!(got, expected, "Pixel {i}: expected {expected}, got {got}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 12. test_split_single_tile_covers_all
// ─────────────────────────────────────────────────────────────────────────────

/// When the tile is larger than the raster, a single tile is produced that
/// covers the entire raster.
#[test]
fn test_split_single_tile_covers_all() {
    let width = 50;
    let height = 30;
    let raster = make_raster(width, height);
    let config = TiledConfig::default().with_tile_size(200, 200);

    let tiles = split_into_tiles(&raster, width, height, &config);

    assert_eq!(tiles.len(), 1, "One tile should cover the entire raster");
    let tile = &tiles[0];
    assert_eq!(tile.width, width);
    assert_eq!(tile.height, height);
    assert_eq!(tile.origin_x, 0);
    assert_eq!(tile.origin_y, 0);
    // No overlap by default.
    assert_eq!(tile.data.len(), width * height);

    // All pixel values should match the original raster.
    for (i, (&orig, &got)) in raster.iter().zip(tile.data.iter()).enumerate() {
        assert_eq!(orig, got, "Tile pixel {i}: expected {orig}, got {got}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Additional robustness checks
// ─────────────────────────────────────────────────────────────────────────────

/// Empty raster → zero tiles.
#[test]
fn test_split_empty_raster_returns_no_tiles() {
    let config = TiledConfig::default();
    let tiles = split_into_tiles(&[], 0, 0, &config);
    assert!(tiles.is_empty());
}

/// Tile indices are in order for a non-trivial grid.
#[test]
fn test_tile_indices_are_row_major() {
    let raster = make_raster(200, 200);
    let config = TiledConfig::default().with_tile_size(100, 100);
    let tiles = split_into_tiles(&raster, 200, 200, &config);
    assert_eq!(tiles.len(), 4);

    let indices: Vec<usize> = tiles.iter().map(|t| t.tile_index).collect();
    assert_eq!(indices, vec![0, 1, 2, 3]);
}

/// Stitch of zero tiles → zero-filled output.
#[test]
fn test_stitch_empty_tiles_gives_zeros() {
    let out = stitch_tiles(&[], 4, 4);
    assert_eq!(out, vec![0.0_f32; 16]);
}

/// `execute_tiled` propagates tile_fn errors.
#[test]
fn test_execute_tiled_propagates_error() {
    let raster = make_raster(64, 64);
    let config = TiledConfig::default().with_tile_size(64, 64);

    let error_fn = |_tile: &RasterTile| -> oxigdal_gpu::GpuResult<Vec<f32>> {
        Err(oxigdal_gpu::GpuError::execution_failed(
            "deliberate test error",
        ))
    };

    let result = execute_tiled(&raster, 64, 64, &config, error_fn);
    assert!(result.is_err());
}

/// Stitch with overlap: split (overlap=4) then stitch recovers original raster.
#[test]
fn test_stitch_with_overlap_recovers_original() {
    let width = 128;
    let height = 128;
    let raster = make_raster(width, height);
    let config = TiledConfig::default()
        .with_tile_size(64, 64)
        .with_overlap(4);

    let tiles = split_into_tiles(&raster, width, height, &config);
    // Passthrough — keep padded data as-is.
    let stitched = stitch_tiles(&tiles, width, height);

    assert_eq!(stitched.len(), raster.len());
    for (i, (&orig, &got)) in raster.iter().zip(stitched.iter()).enumerate() {
        assert_eq!(orig, got, "Pixel {i}: expected {orig}, got {got}");
    }
}

/// `auto_tile_size` never returns a size larger than preferred.
#[test]
fn test_auto_tile_size_never_exceeds_preferred() {
    let budget = 1024 * 1024 * 1024; // 1 GB — very generous
    let (w, h) = auto_tile_size(512, 256, 0, budget, 0.05);
    assert!(w <= 512);
    assert!(h <= 256);
}
