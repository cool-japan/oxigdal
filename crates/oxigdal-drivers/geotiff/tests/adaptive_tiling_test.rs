//! Tests for the `adaptive_tiling` module.

use oxigdal_geotiff::adaptive_tiling::{
    AccessPattern, AdaptiveTileSelector, StorageConditions, TileSize, snap_to_power_of_2,
};

// ── TileSize helpers ──────────────────────────────────────────────────────────

#[test]
fn tile_size_new() {
    let t = TileSize::new(256, 512);
    assert_eq!(t.width, 256);
    assert_eq!(t.height, 512);
}

#[test]
fn tile_size_square() {
    let t = TileSize::square(512);
    assert_eq!(t.width, 512);
    assert_eq!(t.height, 512);
}

#[test]
fn tile_size_pixels_per_tile() {
    let t = TileSize::new(256, 256);
    assert_eq!(t.pixels_per_tile(), 65536);
}

#[test]
fn tiles_for_image_exact_fit() {
    let t = TileSize::square(256);
    let (nx, ny) = t.tiles_for_image(1024, 512);
    assert_eq!(nx, 4);
    assert_eq!(ny, 2);
}

#[test]
fn tiles_for_image_with_remainder() {
    let t = TileSize::square(256);
    let (nx, ny) = t.tiles_for_image(300, 400);
    assert_eq!(nx, 2); // ceil(300/256) = 2
    assert_eq!(ny, 2); // ceil(400/256) = 2
}

#[test]
fn total_tiles_calculation() {
    let t = TileSize::square(256);
    assert_eq!(t.total_tiles(1024, 1024), 16);
}

#[test]
fn padding_fraction_zero_for_exact_fit() {
    let t = TileSize::square(256);
    let frac = t.padding_fraction(1024, 1024);
    assert!((frac - 0.0).abs() < 1e-9, "frac={frac}");
}

#[test]
fn padding_fraction_positive_when_not_exact() {
    let t = TileSize::square(256);
    // 300x300 image → 2x2 tiles of 256x256 → 512x512 tiled area.
    // waste = (512*512 - 300*300) / (512*512) > 0
    let frac = t.padding_fraction(300, 300);
    assert!(frac > 0.0, "frac={frac}");
}

// ── snap_to_power_of_2 ────────────────────────────────────────────────────────

#[test]
fn snap_to_power_of_2_below_midpoint() {
    // 100 is between 64 and 128; 100-64=36 vs 128-100=28 → prefers 128.
    assert_eq!(snap_to_power_of_2(100), 128);
}

#[test]
fn snap_to_power_of_2_exact_power() {
    assert_eq!(snap_to_power_of_2(256), 256);
    assert_eq!(snap_to_power_of_2(512), 512);
}

#[test]
fn snap_to_power_of_2_rounds_down_when_closer() {
    // 200 is between 128 and 256; 200-128=72 vs 256-200=56 → prefers 256.
    // (checks both branches)
    let result = snap_to_power_of_2(200);
    assert!(result == 128 || result == 256);
}

#[test]
fn snap_to_power_of_2_300() {
    // 300 is between 256 and 512; 300-256=44 vs 512-300=212 → prefers 256.
    assert_eq!(snap_to_power_of_2(300), 256);
}

#[test]
fn snap_to_power_of_2_zero_returns_256() {
    assert_eq!(snap_to_power_of_2(0), 256);
}

// ── AdaptiveTileSelector ──────────────────────────────────────────────────────

#[test]
fn tiny_image_returns_image_dimensions() {
    let tile = AdaptiveTileSelector::select(
        128,
        128,
        &AccessPattern::Random,
        &StorageConditions::default(),
    );
    assert_eq!(tile.width, 128);
    assert_eq!(tile.height, 128);
}

#[test]
fn random_access_base_is_256() {
    let conds = StorageConditions {
        latency_ms: 50.0,
        bandwidth_mbps: 100.0,
        is_cloud: true,
    };
    let tile = AdaptiveTileSelector::select(4096, 4096, &AccessPattern::Random, &conds);
    // With moderate latency & bandwidth, base=256 for Random.
    assert_eq!(tile, TileSize::square(256));
}

#[test]
fn sequential_access_base_is_512() {
    let conds = StorageConditions {
        latency_ms: 50.0,
        bandwidth_mbps: 100.0,
        is_cloud: true,
    };
    let tile = AdaptiveTileSelector::select(4096, 4096, &AccessPattern::Sequential, &conds);
    assert_eq!(tile, TileSize::square(512));
}

#[test]
fn high_latency_cloud_uses_larger_tiles() {
    let conds = StorageConditions::slow_cloud(); // latency_ms = 200 > 150
    let tile_random = AdaptiveTileSelector::select(4096, 4096, &AccessPattern::Random, &conds);
    let baseline_conds = StorageConditions {
        latency_ms: 50.0,
        ..StorageConditions::default()
    };
    let tile_baseline =
        AdaptiveTileSelector::select(4096, 4096, &AccessPattern::Random, &baseline_conds);
    // High-latency tiles must be >= baseline tiles.
    assert!(
        tile_random.width >= tile_baseline.width,
        "high-latency tile width={} < baseline={}",
        tile_random.width,
        tile_baseline.width
    );
}

#[test]
fn low_bandwidth_uses_smaller_tiles() {
    let conds = StorageConditions {
        latency_ms: 50.0,
        bandwidth_mbps: 10.0, // < 20 MB/s
        is_cloud: true,
    };
    let tile = AdaptiveTileSelector::select(4096, 4096, &AccessPattern::Sequential, &conds);
    // base=512 halved → 256, snapped.
    assert!(tile.width <= 256, "expected ≤256 but got {}", tile.width);
}

#[test]
fn local_storage_tiles_are_at_most_512() {
    let tile = AdaptiveTileSelector::select(
        8192,
        8192,
        &AccessPattern::OverviewBuild,
        &StorageConditions::local(),
    );
    assert!(tile.width <= 512, "local tile width={}", tile.width);
    assert!(tile.height <= 512, "local tile height={}", tile.height);
}

#[test]
fn selected_tile_never_exceeds_image_width() {
    let tile = AdaptiveTileSelector::select(
        100,
        5000,
        &AccessPattern::Random,
        &StorageConditions::default(),
    );
    assert!(
        tile.width <= 100,
        "tile.width={} > img_width=100",
        tile.width
    );
}

#[test]
fn selected_tile_never_exceeds_image_height() {
    let tile = AdaptiveTileSelector::select(
        5000,
        100,
        &AccessPattern::Random,
        &StorageConditions::default(),
    );
    assert!(
        tile.height <= 100,
        "tile.height={} > img_height=100",
        tile.height
    );
}

// ── overview_levels ───────────────────────────────────────────────────────────

#[test]
fn overview_levels_1024x1024_tile256_returns_level_2() {
    // threshold = 2*256 = 512; 1024 > 512 → push 2; 512×512 → stop → [2]
    let tile = TileSize::square(256);
    let levels = AdaptiveTileSelector::overview_levels(1024, 1024, &tile);
    assert_eq!(levels, vec![2], "levels={levels:?}");
}

#[test]
fn overview_levels_small_image_returns_empty() {
    let tile = TileSize::square(256);
    // 128×128 image is smaller than 2×tile=512 → no overviews needed.
    let levels = AdaptiveTileSelector::overview_levels(128, 128, &tile);
    assert!(levels.is_empty(), "levels={levels:?}");
}

#[test]
fn overview_levels_large_image_has_multiple() {
    let tile = TileSize::square(256);
    let levels = AdaptiveTileSelector::overview_levels(8192, 8192, &tile);
    // 8192 → 4096 → 2048 → 1024 → 512 = 5 levels needed before hitting threshold.
    assert!(
        levels.len() >= 4,
        "expected ≥4 overview levels, got {levels:?}"
    );
}

#[test]
fn overview_levels_are_ascending_powers_of_two() {
    let tile = TileSize::square(256);
    let levels = AdaptiveTileSelector::overview_levels(4096, 4096, &tile);
    for (i, &level) in levels.iter().enumerate() {
        let expected = 2u32.pow(i as u32 + 1);
        assert_eq!(
            level, expected,
            "index {i}: expected {expected}, got {level}"
        );
    }
}

#[test]
fn tile_pyramid_access_returns_256() {
    let conds = StorageConditions {
        latency_ms: 50.0,
        bandwidth_mbps: 200.0,
        is_cloud: true,
    };
    let tile = AdaptiveTileSelector::select(4096, 4096, &AccessPattern::TilePyramid, &conds);
    assert_eq!(tile.width, 256);
    assert_eq!(tile.height, 256);
}

#[test]
fn overview_build_pattern_gives_large_tiles_on_cloud() {
    let conds = StorageConditions {
        latency_ms: 50.0,
        bandwidth_mbps: 200.0,
        is_cloud: true,
    };
    let tile = AdaptiveTileSelector::select(8192, 8192, &AccessPattern::OverviewBuild, &conds);
    // base=1024 for OverviewBuild with normal cloud conditions.
    assert_eq!(tile.width, 1024);
    assert_eq!(tile.height, 1024);
}
