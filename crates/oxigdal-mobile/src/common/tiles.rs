//! Tile management for mobile map rendering.
//!
//! Provides XYZ tile coordinate system utilities and tile loading
//! optimized for mobile devices with offline support.

use crate::ffi::types::*;
use std::sync::atomic::{AtomicBool, Ordering};

/// Offline mode flag.
static OFFLINE_MODE: AtomicBool = AtomicBool::new(false);

/// Sets offline mode.
pub fn set_offline_mode(enabled: bool) {
    OFFLINE_MODE.store(enabled, Ordering::Relaxed);
}

/// Checks if offline mode is enabled.
pub fn is_offline_mode() -> bool {
    OFFLINE_MODE.load(Ordering::Relaxed)
}

/// Web Mercator EPSG code.
pub const WEB_MERCATOR_EPSG: i32 = 3857;

/// Tile size in pixels (standard is 256).
pub const TILE_SIZE: i32 = 256;

/// Maximum zoom level (typically 22 for most applications).
pub const MAX_ZOOM: i32 = 22;

/// Converts geographic coordinates to tile coordinates.
///
/// # Parameters
/// - `lon`: Longitude in degrees
/// - `lat`: Latitude in degrees
/// - `zoom`: Zoom level
///
/// # Returns
/// Tile coordinates (x, y) at given zoom level
pub fn lonlat_to_tile(lon: f64, lat: f64, zoom: i32) -> (i32, i32) {
    let n = 2_f64.powi(zoom);
    let x = ((lon + 180.0) / 360.0 * n).floor() as i32;
    let lat_rad = lat.to_radians();
    let y = ((1.0 - lat_rad.tan().asinh() / std::f64::consts::PI) / 2.0 * n).floor() as i32;
    (x, y)
}

/// Converts tile coordinates to geographic bounding box.
///
/// # Parameters
/// - `x`: Tile X coordinate
/// - `y`: Tile Y coordinate
/// - `zoom`: Zoom level
///
/// # Returns
/// Bounding box (min_lon, min_lat, max_lon, max_lat)
pub fn tile_to_bbox(x: i32, y: i32, zoom: i32) -> (f64, f64, f64, f64) {
    let n = 2_f64.powi(zoom);

    let min_lon = x as f64 / n * 360.0 - 180.0;
    let max_lon = (x + 1) as f64 / n * 360.0 - 180.0;

    let min_lat = mercator_y_to_lat((y + 1) as f64 / n);
    let max_lat = mercator_y_to_lat(y as f64 / n);

    (min_lon, min_lat, max_lon, max_lat)
}

/// Converts Web Mercator Y to latitude.
fn mercator_y_to_lat(y: f64) -> f64 {
    let n = std::f64::consts::PI - 2.0 * std::f64::consts::PI * y;
    (n.exp().atan() - std::f64::consts::PI / 4.0).to_degrees() * 2.0
}

/// Calculates tiles needed for a bounding box at given zoom level.
///
/// # Parameters
/// - `bbox`: Bounding box
/// - `zoom`: Zoom level
///
/// # Returns
/// Vector of (x, y, z) tile coordinates
pub fn tiles_for_bbox(bbox: &OxiGdalBbox, zoom: i32) -> Vec<(i32, i32, i32)> {
    let (min_x, min_y) = lonlat_to_tile(bbox.min_x, bbox.max_y, zoom);
    let (max_x, max_y) = lonlat_to_tile(bbox.max_x, bbox.min_y, zoom);

    let mut tiles = Vec::new();
    for x in min_x..=max_x {
        for y in min_y..=max_y {
            tiles.push((x, y, zoom));
        }
    }
    tiles
}

/// FFI: Converts lon/lat to tile coordinates.
///
/// # Safety
/// - out_x and out_y must be valid pointers
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_lonlat_to_tile(
    lon: std::os::raw::c_double,
    lat: std::os::raw::c_double,
    zoom: std::os::raw::c_int,
    out_x: *mut std::os::raw::c_int,
    out_y: *mut std::os::raw::c_int,
) -> OxiGdalErrorCode {
    if out_x.is_null() || out_y.is_null() {
        crate::ffi::error::set_last_error("Null output pointers".to_string());
        return OxiGdalErrorCode::NullPointer;
    }

    if !(0..=MAX_ZOOM).contains(&zoom) {
        crate::ffi::error::set_last_error(format!("Invalid zoom level: {}", zoom));
        return OxiGdalErrorCode::InvalidArgument;
    }

    let (x, y) = lonlat_to_tile(lon, lat, zoom);
    unsafe {
        *out_x = x;
        *out_y = y;
    }

    OxiGdalErrorCode::Success
}

/// FFI: Converts tile coordinates to bounding box.
///
/// # Safety
/// - out_bbox must be a valid pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_tile_to_bbox(
    x: std::os::raw::c_int,
    y: std::os::raw::c_int,
    zoom: std::os::raw::c_int,
    out_bbox: *mut OxiGdalBbox,
) -> OxiGdalErrorCode {
    if out_bbox.is_null() {
        crate::ffi::error::set_last_error("Null output bbox".to_string());
        return OxiGdalErrorCode::NullPointer;
    }

    if !(0..=MAX_ZOOM).contains(&zoom) {
        crate::ffi::error::set_last_error(format!("Invalid zoom level: {}", zoom));
        return OxiGdalErrorCode::InvalidArgument;
    }

    let (min_lon, min_lat, max_lon, max_lat) = tile_to_bbox(x, y, zoom);

    unsafe {
        *out_bbox = OxiGdalBbox {
            min_x: min_lon,
            min_y: min_lat,
            max_x: max_lon,
            max_y: max_lat,
        };
    }

    OxiGdalErrorCode::Success
}

/// FFI: Calculates number of tiles for a bounding box.
///
/// # Safety
/// - bbox must be a valid pointer
///
/// # Returns
/// Number of tiles, or -1 on error
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_count_tiles_for_bbox(
    bbox: *const OxiGdalBbox,
    zoom: std::os::raw::c_int,
) -> std::os::raw::c_int {
    if bbox.is_null() {
        crate::ffi::error::set_last_error("Null bbox pointer".to_string());
        return -1;
    }

    if !(0..=MAX_ZOOM).contains(&zoom) {
        crate::ffi::error::set_last_error(format!("Invalid zoom level: {}", zoom));
        return -1;
    }

    unsafe {
        let bbox_ref = &*bbox;
        let tiles = tiles_for_bbox(bbox_ref, zoom);
        tiles.len() as i32
    }
}

/// FFI: Gets tile coordinates for a bounding box.
///
/// # Safety
/// - bbox must be valid
/// - out_tiles must be pre-allocated with sufficient size
/// - Call oxigdal_count_tiles_for_bbox first to get required size
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_get_tiles_for_bbox(
    bbox: *const OxiGdalBbox,
    zoom: std::os::raw::c_int,
    out_tiles: *mut OxiGdalTileCoord,
    max_tiles: std::os::raw::c_int,
) -> std::os::raw::c_int {
    if bbox.is_null() || out_tiles.is_null() {
        crate::ffi::error::set_last_error("Null pointer".to_string());
        return -1;
    }

    if !(0..=MAX_ZOOM).contains(&zoom) {
        crate::ffi::error::set_last_error(format!("Invalid zoom level: {}", zoom));
        return -1;
    }

    unsafe {
        let bbox_ref = &*bbox;
        let tiles = tiles_for_bbox(bbox_ref, zoom);

        let count = tiles.len().min(max_tiles as usize);

        for (i, (x, y, z)) in tiles.iter().take(count).enumerate() {
            *out_tiles.add(i) = OxiGdalTileCoord {
                x: *x,
                y: *y,
                z: *z,
            };
        }

        count as i32
    }
}

/// Calculates the resolution (meters per pixel) at given latitude and zoom.
///
/// # Parameters
/// - `lat`: Latitude in degrees
/// - `zoom`: Zoom level
///
/// # Returns
/// Resolution in meters per pixel
pub fn resolution_at_zoom(lat: f64, zoom: i32) -> f64 {
    // Earth circumference at equator in meters
    const EARTH_CIRCUMFERENCE: f64 = 40_075_016.686;

    let lat_rad = lat.to_radians();
    EARTH_CIRCUMFERENCE * lat_rad.cos() / (TILE_SIZE as f64 * 2_f64.powi(zoom))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lonlat_to_tile() {
        // Test known coordinate (0, 0 at zoom 0 should be tile 0, 0)
        let (x, y) = lonlat_to_tile(0.0, 0.0, 0);
        assert_eq!(x, 0);
        assert_eq!(y, 0);

        // Test at higher zoom
        let (x, y) = lonlat_to_tile(-122.4194, 37.7749, 10); // San Francisco
        assert!((0..1024).contains(&x)); // 2^10 = 1024
        assert!((0..1024).contains(&y));
    }

    #[test]
    fn test_tile_to_bbox() {
        let (min_lon, min_lat, max_lon, max_lat) = tile_to_bbox(0, 0, 0);

        // At zoom 0, should cover whole world
        assert!((min_lon + 180.0).abs() < 0.1);
        assert!((max_lon - 180.0).abs() < 0.1);
        assert!(min_lat < 0.0);
        assert!(max_lat > 0.0);
    }

    #[test]
    fn test_tiles_for_bbox() {
        let bbox = OxiGdalBbox {
            min_x: -1.0,
            min_y: -1.0,
            max_x: 1.0,
            max_y: 1.0,
        };

        let tiles = tiles_for_bbox(&bbox, 2);
        assert!(!tiles.is_empty());
        assert!(tiles.len() <= 16); // Max 4x4 at zoom 2
    }

    #[test]
    fn test_ffi_lonlat_to_tile() {
        let mut x = 0;
        let mut y = 0;

        let result = unsafe { oxigdal_lonlat_to_tile(0.0, 0.0, 5, &mut x, &mut y) };

        assert_eq!(result, OxiGdalErrorCode::Success);
        assert!(x >= 0);
        assert!(y >= 0);
    }

    #[test]
    fn test_ffi_tile_to_bbox() {
        let mut bbox = OxiGdalBbox {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 0.0,
            max_y: 0.0,
        };

        let result = unsafe { oxigdal_tile_to_bbox(0, 0, 5, &mut bbox) };

        assert_eq!(result, OxiGdalErrorCode::Success);
        assert!(bbox.min_x < bbox.max_x);
        assert!(bbox.min_y < bbox.max_y);
    }

    #[test]
    fn test_count_tiles() {
        let bbox = OxiGdalBbox {
            min_x: -10.0,
            min_y: -10.0,
            max_x: 10.0,
            max_y: 10.0,
        };

        let count = unsafe { oxigdal_count_tiles_for_bbox(&bbox, 3) };

        assert!(count > 0);
        assert!(count < 64); // Should be reasonable at zoom 3
    }

    #[test]
    fn test_resolution() {
        let res_z0 = resolution_at_zoom(0.0, 0);
        let res_z10 = resolution_at_zoom(0.0, 10);

        // Resolution should decrease with zoom
        assert!(res_z0 > res_z10);
        assert!(res_z10 > 0.0);
    }

    #[test]
    fn test_offline_mode() {
        set_offline_mode(true);
        assert!(is_offline_mode());

        set_offline_mode(false);
        assert!(!is_offline_mode());
    }
}
