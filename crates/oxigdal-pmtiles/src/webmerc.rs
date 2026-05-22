//! Web Mercator tile ↔ WGS-84 lon/lat conversion helpers.
//!
//! Implements the standard slippy-map tile numbering scheme used by
//! OpenStreetMap, Mapbox, and the PMTiles specification.  All angles are in
//! decimal degrees unless noted otherwise.
//!
//! # Coordinate conventions
//! - **lon** (longitude): −180 to +180°, positive East.
//! - **lat** (latitude): −85.051 129 to +85.051 129°, positive North.
//!   Values outside this range are clamped; the poles cannot be represented
//!   in Web Mercator and would produce ±∞.
//! - **z** (zoom level): 0–26 (matching the PMTiles v3 limit).
//! - **x**: column index, 0 at the International Date Line West.
//! - **y**: row index, 0 at the top (North).
//!
//! # References
//! - <https://wiki.openstreetmap.org/wiki/Slippy_map_tilenames>

use std::f64::consts::PI;

/// Maximum Web Mercator latitude (degrees).  Beyond ±85.051 129° the
/// Mercator projection becomes undefined (tan saturates).
pub const WEB_MERC_LAT_MAX: f64 = 85.051_129;

// -------------------------------------------------------------------------
// Core tile ↔ coordinate functions
// -------------------------------------------------------------------------

/// Return the **top-left** corner (lon, lat) of tile `(z, x, y)` in degrees.
///
/// The formula follows the OSM slippy-map convention:
/// ```text
/// lon = x / 2^z × 360 − 180
/// lat = atan(sinh(π × (1 − 2y / 2^z))) in degrees
/// ```
/// Latitude is clamped to [`WEB_MERC_LAT_MAX`] to avoid ±∞ at the poles.
///
/// # Examples
/// ```
/// use oxigdal_pmtiles::webmerc::tile_to_lonlat;
/// let (lon, lat) = tile_to_lonlat(0, 0, 0);
/// assert!((lon - (-180.0)).abs() < 1e-6);
/// assert!(lat > 85.0);
/// ```
pub fn tile_to_lonlat(z: u8, x: u32, y: u32) -> (f64, f64) {
    let n = (1u64 << z as u64) as f64; // 2^z
    let lon = (x as f64) / n * 360.0 - 180.0;
    let lat_rad = (PI * (1.0 - 2.0 * (y as f64) / n)).sinh().atan();
    let lat = lat_rad
        .to_degrees()
        .clamp(-WEB_MERC_LAT_MAX, WEB_MERC_LAT_MAX);
    (lon, lat)
}

/// Return the **bounding box** `(min_lon, min_lat, max_lon, max_lat)` of
/// tile `(z, x, y)` in WGS-84 decimal degrees.
///
/// The bottom-right corner is computed as the top-left of the *next* tile
/// `(z, x+1, y+1)`.  At the edges of the tile grid (x or y at maximum
/// for the zoom level) the coordinates are clamped so that the result
/// remains within `[−180, +180] × [−85.05, +85.05]`.
///
/// # Examples
/// ```
/// use oxigdal_pmtiles::webmerc::tile_bounds_lonlat;
/// // z=0 single tile covers the whole world.
/// let (min_lon, min_lat, max_lon, max_lat) = tile_bounds_lonlat(0, 0, 0);
/// assert!((min_lon - (-180.0)).abs() < 1e-6);
/// assert!((max_lon - 180.0).abs() < 1e-6);
/// assert!(min_lat < -85.0 && max_lat > 85.0);
/// ```
pub fn tile_bounds_lonlat(z: u8, x: u32, y: u32) -> (f64, f64, f64, f64) {
    // Top-left corner of this tile.
    let (min_lon, max_lat) = tile_to_lonlat(z, x, y);

    // Bottom-right corner = top-left of the *next* tile in both dimensions.
    // At zoom 0 the next tile wraps to 1, which tile_to_lonlat handles
    // correctly because 1/1 * 360 − 180 = +180.
    let x_next = x.saturating_add(1);
    let y_next = y.saturating_add(1);
    let (max_lon, min_lat) = tile_to_lonlat(z, x_next, y_next);

    (min_lon, min_lat, max_lon, max_lat)
}

/// Convert a WGS-84 point `(lon, lat)` (degrees) to the tile `(x, y)` that
/// contains it at zoom level `z`.
///
/// Uses floor so the returned tile is the one that contains the point.
/// Both x and y are clamped to `[0, 2^z − 1]` for safety.
///
/// # Panics
/// Does not panic; invalid inputs are clamped silently.
///
/// # Examples
/// ```
/// use oxigdal_pmtiles::webmerc::{lonlat_to_tile, tile_bounds_lonlat};
/// let z = 10u8;
/// let (lon, lat) = (13.405, 52.52); // Berlin
/// let (x, y) = lonlat_to_tile(z, lon, lat);
/// let (min_lon, min_lat, max_lon, max_lat) = tile_bounds_lonlat(z, x, y);
/// assert!(lon >= min_lon && lon <= max_lon);
/// assert!(lat >= min_lat && lat <= max_lat);
/// ```
pub fn lonlat_to_tile(z: u8, lon: f64, lat: f64) -> (u32, u32) {
    let n = 1u32 << z;
    let nf = n as f64;

    // Clamp lat to valid Mercator range before computing.
    let lat_clamped = lat.clamp(-WEB_MERC_LAT_MAX, WEB_MERC_LAT_MAX);

    let x_f = (lon + 180.0) / 360.0 * nf;
    let lat_rad = lat_clamped.to_radians();
    let y_f = (1.0 - lat_rad.tan().asinh() / PI) / 2.0 * nf;

    // Floor and clamp to [0, n-1].
    let x = (x_f.floor() as i64).clamp(0, (n - 1) as i64) as u32;
    let y = (y_f.floor() as i64).clamp(0, (n - 1) as i64) as u32;
    (x, y)
}

// -------------------------------------------------------------------------
// Unit tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_to_lonlat_z0_origin() {
        let (lon, lat) = tile_to_lonlat(0, 0, 0);
        assert!((lon - (-180.0)).abs() < 1e-6, "lon={lon}");
        assert!(lat > 85.0, "lat={lat}");
    }

    #[test]
    fn test_tile_to_lonlat_z1_nw() {
        // z=1, x=0, y=0 → top-left is (-180, ~85)
        let (lon, lat) = tile_to_lonlat(1, 0, 0);
        assert!((lon - (-180.0)).abs() < 1e-6);
        assert!(lat > 85.0);
    }

    #[test]
    fn test_tile_to_lonlat_z1_center() {
        // z=1, x=1, y=1 → top-left is (0, 0)
        let (lon, lat) = tile_to_lonlat(1, 1, 1);
        assert!((lon - 0.0).abs() < 1e-6, "lon={lon}");
        assert!(lat.abs() < 1e-6, "lat={lat}");
    }

    #[test]
    fn test_tile_bounds_z0_is_world() {
        let (min_lon, min_lat, max_lon, max_lat) = tile_bounds_lonlat(0, 0, 0);
        assert!((min_lon - (-180.0)).abs() < 1e-6);
        assert!((max_lon - 180.0).abs() < 1e-6);
        assert!(min_lat < -85.0);
        assert!(max_lat > 85.0);
    }

    #[test]
    fn test_lonlat_to_tile_round_trip_z10() {
        let z = 10u8;
        let (lon, lat) = (13.405_0, 52.520_0); // Berlin
        let (x, y) = lonlat_to_tile(z, lon, lat);
        let (min_lon, min_lat, max_lon, max_lat) = tile_bounds_lonlat(z, x, y);
        assert!(
            lon >= min_lon && lon <= max_lon,
            "lon={lon} not in [{min_lon},{max_lon}]"
        );
        assert!(
            lat >= min_lat && lat <= max_lat,
            "lat={lat} not in [{min_lat},{max_lat}]"
        );
    }

    #[test]
    fn test_lat_clamped_at_poles() {
        // y=0 at high zoom should clamp to WEB_MERC_LAT_MAX, not infinity.
        let (_, lat) = tile_to_lonlat(20, 0, 0);
        assert!(lat.is_finite());
        assert!(lat <= WEB_MERC_LAT_MAX + 1e-6);
    }
}
