//! Adaptive tile size selection for Cloud Optimized GeoTIFF (COG).
//!
//! Optimal tile size depends on:
//! - Data dimensions (avoids wasted padding on small images)
//! - Network conditions (larger tiles = fewer requests but more transferred data)
//! - Access patterns (random vs. sequential)
//! - Compression characteristics (DEFLATE/Zstd compress well at 512×512+)

// ── Tile size ─────────────────────────────────────────────────────────────────

/// A tile's width × height in pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileSize {
    /// Tile width in pixels.
    pub width: u32,
    /// Tile height in pixels.
    pub height: u32,
}

impl TileSize {
    /// Creates a new tile size.
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Creates a square tile with side `size`.
    #[must_use]
    pub const fn square(size: u32) -> Self {
        Self {
            width: size,
            height: size,
        }
    }

    /// Returns the number of pixels per tile.
    #[must_use]
    pub fn pixels_per_tile(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    /// Returns `(tiles_x, tiles_y)` — how many tiles are needed to cover an
    /// image of the given pixel dimensions.
    #[must_use]
    pub fn tiles_for_image(&self, img_width: u32, img_height: u32) -> (u32, u32) {
        let nx = img_width.div_ceil(self.width.max(1));
        let ny = img_height.div_ceil(self.height.max(1));
        (nx, ny)
    }

    /// Returns the total number of tiles required to cover the image.
    #[must_use]
    pub fn total_tiles(&self, img_width: u32, img_height: u32) -> u32 {
        let (nx, ny) = self.tiles_for_image(img_width, img_height);
        nx.saturating_mul(ny)
    }

    /// Returns the fraction of tiled area that is padding (`0.0` = perfect fit,
    /// approaching `1.0` = almost all padding).
    #[must_use]
    pub fn padding_fraction(&self, img_width: u32, img_height: u32) -> f64 {
        let (nx, ny) = self.tiles_for_image(img_width, img_height);
        let total_tiled =
            u64::from(nx) * u64::from(self.width) * u64::from(ny) * u64::from(self.height);
        let actual = u64::from(img_width) * u64::from(img_height);
        if total_tiled == 0 {
            0.0
        } else {
            1.0 - actual as f64 / total_tiled as f64
        }
    }
}

// ── Access pattern ────────────────────────────────────────────────────────────

/// A hint about how the raster will be accessed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessPattern {
    /// Mostly random pixel or region access (e.g., web map tile serving).
    Random,
    /// Sequential full-image processing (e.g., raster algebra).
    Sequential,
    /// Tile-pyramid / zoom-level access (e.g., slippy map).
    TilePyramid,
    /// Generating overviews (processes large contiguous strips).
    OverviewBuild,
}

// ── Storage conditions ────────────────────────────────────────────────────────

/// Describes the performance characteristics of the storage medium.
#[derive(Debug, Clone)]
pub struct StorageConditions {
    /// Round-trip latency in milliseconds.
    pub latency_ms: f64,
    /// Available bandwidth in MB/s.
    pub bandwidth_mbps: f64,
    /// `true` when storage is remote (cloud object store, HTTP, etc.).
    pub is_cloud: bool,
}

impl Default for StorageConditions {
    fn default() -> Self {
        Self {
            latency_ms: 100.0,
            bandwidth_mbps: 100.0,
            is_cloud: true,
        }
    }
}

impl StorageConditions {
    /// Preset for a fast local NVMe/SSD.
    #[must_use]
    pub fn local() -> Self {
        Self {
            latency_ms: 0.1,
            bandwidth_mbps: 1_000.0,
            is_cloud: false,
        }
    }

    /// Preset for a slow or congested cloud connection.
    #[must_use]
    pub fn slow_cloud() -> Self {
        Self {
            latency_ms: 200.0,
            bandwidth_mbps: 10.0,
            is_cloud: true,
        }
    }

    /// Preset for a fast cloud connection (e.g., co-located compute).
    #[must_use]
    pub fn fast_cloud() -> Self {
        Self {
            latency_ms: 20.0,
            bandwidth_mbps: 500.0,
            is_cloud: true,
        }
    }
}

// ── Adaptive selector ─────────────────────────────────────────────────────────

/// Chooses optimal tile parameters given image properties and storage conditions.
pub struct AdaptiveTileSelector;

impl AdaptiveTileSelector {
    /// Selects the best tile size for an image and storage environment.
    ///
    /// The algorithm:
    /// 1. For tiny images, returns the image dimensions directly (no tiling needed).
    /// 2. Picks a base tile size from the access pattern.
    /// 3. Adjusts for network latency / bandwidth.
    /// 4. Snaps to the nearest power of two.
    /// 5. Clamps so the tile is never larger than the image.
    #[must_use]
    pub fn select(
        img_width: u32,
        img_height: u32,
        access_pattern: &AccessPattern,
        conditions: &StorageConditions,
    ) -> TileSize {
        // Tiny images: no tiling benefit.
        if img_width <= 256 && img_height <= 256 {
            return TileSize::new(img_width, img_height);
        }

        // Base size driven by access pattern.
        let base: u32 = match access_pattern {
            AccessPattern::Random => 256,
            AccessPattern::Sequential => 512,
            AccessPattern::TilePyramid => 256,
            AccessPattern::OverviewBuild => 1024,
        };

        // Adjust for network conditions.
        let adjusted = if conditions.is_cloud {
            if conditions.latency_ms > 150.0 {
                // High latency → larger tiles to amortise round-trip cost.
                (base * 2).min(1024)
            } else if conditions.bandwidth_mbps < 20.0 {
                // Low bandwidth → smaller tiles to avoid wasted transfers.
                (base / 2).max(128)
            } else {
                base
            }
        } else {
            // Local storage: low latency allows smaller tiles.
            base.min(512)
        };

        let snapped = snap_to_power_of_2(adjusted);

        // Never exceed image dimensions.
        TileSize::new(snapped.min(img_width), snapped.min(img_height))
    }

    /// Suggests the overview level denominators (2, 4, 8, …) for the given
    /// image size and tile size.
    ///
    /// Adds a level as long as the downsampled image is still larger than
    /// two tiles in either dimension.
    #[must_use]
    pub fn overview_levels(img_width: u32, img_height: u32, tile_size: &TileSize) -> Vec<u32> {
        let mut levels = Vec::new();
        let mut w = img_width;
        let mut h = img_height;
        let mut level = 2u32;

        loop {
            let threshold_w = tile_size.width.saturating_mul(2);
            let threshold_h = tile_size.height.saturating_mul(2);
            if w <= threshold_w && h <= threshold_h {
                break;
            }
            levels.push(level);
            w = w.div_ceil(2);
            h = h.div_ceil(2);
            level = level.saturating_mul(2);
        }
        levels
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Rounds `v` to the nearest power of two (minimum 1).
///
/// When `v` is exactly halfway between two powers of two, the *lower* power
/// is preferred (less data transferred per tile).
#[must_use]
pub fn snap_to_power_of_2(v: u32) -> u32 {
    if v == 0 {
        return 256;
    }
    let mut p = 1u32;
    while p < v {
        p = p.saturating_mul(2);
    }
    // p is the first power ≥ v; p/2 is the one below.
    let lower = p / 2;
    if lower > 0 && (v - lower) < (p - v) {
        lower
    } else {
        p
    }
}
