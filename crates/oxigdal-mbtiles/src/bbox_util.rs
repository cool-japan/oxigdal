//! Geographic coordinate ↔ tile coordinate conversion utilities.
//!
//! All functions use the **XYZ** (Web Mercator / Slippy Map) tile scheme where
//! y = 0 is at the north pole, and the standard 256-pixel tile size.

use std::f64::consts::PI;

/// Earth radius used in the Web Mercator projection (metres).
const EARTH_RADIUS_M: f64 = 6_378_137.0;

/// Nominal tile size in pixels.
const TILE_PIXELS: f64 = 256.0;

// ── Internal helpers ─────────────────────────────────────────────────────────

/// Clamp `lat` to the Web Mercator valid range (±~85.051°).
#[inline]
fn clamp_lat(lat: f64) -> f64 {
    lat.clamp(-85.051_129_f64, 85.051_129_f64)
}

/// Convert longitude/latitude to fractional tile coordinates at `zoom`.
///
/// Returns `(fx, fy)` where each value may not be integers.
#[inline]
fn lonlat_to_tile_fractional(lon: f64, lat: f64, zoom: u8) -> (f64, f64) {
    let n = (1u64 << zoom) as f64; // 2^zoom
    let lat_rad = clamp_lat(lat).to_radians();
    let fx = (lon + 180.0) / 360.0 * n;
    let fy = (1.0 - (lat_rad.tan() + 1.0 / lat_rad.cos()).ln() / PI) / 2.0 * n;
    (fx, fy)
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Convert tile `(z, x, y)` in XYZ scheme to a WGS84 bounding box
/// `[west, south, east, north]` in decimal degrees.
///
/// The returned bounding box is the **tile's** geographic extent, not the
/// center point.
pub fn tile_to_bbox(z: u8, x: u32, y: u32) -> [f64; 4] {
    let (lon_west, lat_north) = tile_to_lonlat(x, y, z);
    let (lon_east, lat_south) = tile_to_lonlat(x + 1, y + 1, z);
    [lon_west, lat_south, lon_east, lat_north]
}

/// Convert a WGS84 bounding box to the tile extents that cover it at `zoom`.
///
/// Returns `(min_x, min_y, max_x, max_y)` in XYZ coordinates.  All values are
/// clamped to the valid range for the zoom level.
pub fn bbox_to_tiles(
    west: f64,
    south: f64,
    east: f64,
    north: f64,
    zoom: u8,
) -> (u32, u32, u32, u32) {
    let n = (1u64 << zoom) as u32;
    let max_idx = n.saturating_sub(1);

    let (min_x, min_y) = lonlat_to_tile(west, north, zoom);
    let (max_x, max_y) = lonlat_to_tile(east, south, zoom);

    (
        min_x.min(max_idx),
        min_y.min(max_idx),
        max_x.min(max_idx),
        max_y.min(max_idx),
    )
}

/// Convert longitude/latitude to the tile `(x, y)` at `zoom` in XYZ scheme.
///
/// Fractional parts are truncated (floor).  The result is clamped to the valid
/// tile range for the zoom level.
pub fn lonlat_to_tile(lon: f64, lat: f64, zoom: u8) -> (u32, u32) {
    let n = (1u64 << zoom) as u32;
    let max_idx = n.saturating_sub(1);
    let (fx, fy) = lonlat_to_tile_fractional(lon, lat, zoom);
    let x = (fx.floor() as i64).clamp(0, max_idx as i64) as u32;
    let y = (fy.floor() as i64).clamp(0, max_idx as i64) as u32;
    (x, y)
}

/// Convert tile upper-left corner `(x, y)` at `zoom` to longitude/latitude.
///
/// Returns `(longitude, latitude)` in decimal degrees.
pub fn tile_to_lonlat(x: u32, y: u32, zoom: u8) -> (f64, f64) {
    let n = (1u64 << zoom) as f64;
    let lon = x as f64 / n * 360.0 - 180.0;
    let lat_rad = ((PI * (1.0 - 2.0 * y as f64 / n)).sinh()).atan();
    let lat = lat_rad.to_degrees();
    (lon, lat)
}

/// Total number of tiles at a given zoom level: 4^zoom = (2^zoom)².
pub fn tile_count_at_zoom(zoom: u8) -> u64 {
    1u64 << (2 * zoom as u32)
}

/// Tile resolution in decimal degrees per pixel (at 256-pixel tiles).
///
/// This is the latitude/longitude span of one pixel at the given zoom level,
/// approximated using the total longitude span divided by the number of pixels.
pub fn tile_resolution_degrees(zoom: u8) -> f64 {
    let total_tiles = (1u64 << zoom) as f64;
    // Full longitude span (360°) divided by (tiles * pixels_per_tile)
    360.0 / (total_tiles * TILE_PIXELS)
}

/// Tile resolution in metres per pixel at the equator (at 256-pixel tiles).
///
/// Uses the circumference of the Earth at the equator divided by the total
/// pixel width at the given zoom level.
pub fn tile_resolution_metres(zoom: u8) -> f64 {
    let total_tiles = (1u64 << zoom) as f64;
    let circumference = 2.0 * PI * EARTH_RADIUS_M;
    circumference / (total_tiles * TILE_PIXELS)
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn tile_resolution_degrees_zoom0() {
        let res = tile_resolution_degrees(0);
        // At zoom 0: 360 / (1 * 256) ≈ 1.40625
        let expected = 360.0 / 256.0;
        assert!((res - expected).abs() < 1e-10);
    }

    #[test]
    fn tile_resolution_metres_zoom0() {
        let res = tile_resolution_metres(0);
        assert!(res > 100_000.0); // ~156,543 m/px at zoom 0
        assert!(res < 200_000.0);
    }
}
