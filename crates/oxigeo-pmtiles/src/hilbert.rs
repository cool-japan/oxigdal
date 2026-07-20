//! Hilbert curve encoding for PMTiles v3 tile IDs.
//!
//! The PMTiles specification assigns each tile a unique 64-bit ID by encoding
//! its `(z, x, y)` coordinates onto a Hilbert curve of order `2^z`.
//!
//! Reference: <https://github.com/protomaps/PMTiles/blob/main/spec/v3/spec.md>

use crate::error::PmTilesError;

/// Convert an `(x, y)` coordinate to a Hilbert curve index for a grid of
/// size `2^order x 2^order`.
///
/// The algorithm is the standard iterative approach described by Wikipedia
/// (Hilbert curve § Applications and mapping algorithms).
pub fn xy_to_hilbert(x: u32, y: u32, order: u32) -> u64 {
    let mut rx: u32;
    let mut ry: u32;
    let mut d: u64 = 0;
    let mut mx = x;
    let mut my = y;

    if order == 0 {
        return 0;
    }
    let mut s = 1u32 << (order - 1);

    while s > 0 {
        rx = if (mx & s) > 0 { 1 } else { 0 };
        ry = if (my & s) > 0 { 1 } else { 0 };
        d += u64::from(s) * u64::from(s) * u64::from((3 * rx) ^ ry);
        // Rotate
        rotate(s, &mut mx, &mut my, rx, ry);
        s >>= 1;
    }
    d
}

/// Convert a Hilbert curve index `d` back to `(x, y)` coordinates for a grid
/// of size `2^order x 2^order`.
pub fn hilbert_to_xy(d: u64, order: u32) -> (u32, u32) {
    let mut rx: u32;
    let mut ry: u32;
    let mut x: u32 = 0;
    let mut y: u32 = 0;
    let mut t = d;
    let mut s: u32 = 1;

    while s < (1u32 << order) {
        rx = (1 & (t / 2)) as u32;
        ry = (1 & (t ^ u64::from(rx))) as u32;
        rotate(s, &mut x, &mut y, rx, ry);
        x += s * rx;
        y += s * ry;
        t /= 4;
        s <<= 1;
    }
    (x, y)
}

/// Rotate/flip a quadrant.
fn rotate(n: u32, x: &mut u32, y: &mut u32, rx: u32, ry: u32) {
    if ry == 0 {
        if rx == 1 {
            *x = n.wrapping_sub(1).wrapping_sub(*x);
            *y = n.wrapping_sub(1).wrapping_sub(*y);
        }
        std::mem::swap(x, y);
    }
}

/// Compute the cumulative number of tiles from zoom level 0 up to (but not
/// including) zoom level `z`.
///
/// Each zoom level `k` has `4^k` tiles, so the sum is `(4^z - 1) / 3`.
fn zoom_offset(z: u8) -> u64 {
    if z == 0 {
        return 0;
    }
    // (4^z - 1) / 3
    // Use bit shift: 4^z = 1 << (2*z)
    let four_z = 1u64 << (2 * u64::from(z));
    (four_z - 1) / 3
}

/// Convert `(z, x, y)` tile coordinates to a PMTiles v3 tile ID.
///
/// The tile ID is computed as: `zoom_offset(z) + hilbert(x, y, z)`.
///
/// # Errors
/// Returns [`PmTilesError::InvalidFormat`] if `x` or `y` exceeds the valid
/// range for zoom level `z` (i.e. `>= 2^z`), or if `z > 26`.
pub fn zxy_to_tile_id(z: u8, x: u32, y: u32) -> Result<u64, PmTilesError> {
    if z > 26 {
        return Err(PmTilesError::InvalidFormat(format!(
            "Zoom level {z} exceeds maximum supported (26)"
        )));
    }
    if z > 0 {
        let max_coord = 1u32 << z;
        if x >= max_coord || y >= max_coord {
            return Err(PmTilesError::InvalidFormat(format!(
                "Tile ({z}/{x}/{y}) out of range (max coord at z={z} is {})",
                max_coord - 1
            )));
        }
    } else if x != 0 || y != 0 {
        return Err(PmTilesError::InvalidFormat(
            "At zoom 0, x and y must be 0".into(),
        ));
    }
    let hilbert_index = xy_to_hilbert(x, y, u32::from(z));
    Ok(zoom_offset(z) + hilbert_index)
}

/// Convert a PMTiles v3 tile ID back to `(z, x, y)` coordinates.
///
/// # Errors
/// Returns [`PmTilesError::InvalidFormat`] if the tile ID is out of valid
/// range.
pub fn tile_id_to_zxy(tile_id: u64) -> Result<(u8, u32, u32), PmTilesError> {
    // Find the zoom level by scanning cumulative offsets.
    // Max zoom we support is 26.
    let mut z: u8 = 0;
    loop {
        if z > 26 {
            return Err(PmTilesError::InvalidFormat(format!(
                "Tile ID {tile_id} exceeds maximum supported range"
            )));
        }
        let next_offset = zoom_offset(z + 1);
        if tile_id < next_offset {
            break;
        }
        z += 1;
    }

    let hilbert_index = tile_id - zoom_offset(z);
    let (x, y) = hilbert_to_xy(hilbert_index, u32::from(z));
    Ok((z, x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xy_to_hilbert_order0() {
        assert_eq!(xy_to_hilbert(0, 0, 0), 0);
    }

    #[test]
    fn test_hilbert_round_trip_order1() {
        for d in 0..4u64 {
            let (x, y) = hilbert_to_xy(d, 1);
            let d2 = xy_to_hilbert(x, y, 1);
            assert_eq!(d, d2, "round trip failed for d={d}");
        }
    }

    #[test]
    fn test_hilbert_round_trip_order2() {
        for d in 0..16u64 {
            let (x, y) = hilbert_to_xy(d, 2);
            let d2 = xy_to_hilbert(x, y, 2);
            assert_eq!(d, d2, "round trip failed for d={d}");
        }
    }

    #[test]
    fn test_hilbert_round_trip_order3() {
        for d in 0..64u64 {
            let (x, y) = hilbert_to_xy(d, 3);
            let d2 = xy_to_hilbert(x, y, 3);
            assert_eq!(d, d2, "round trip failed for d={d}");
        }
    }

    #[test]
    fn test_hilbert_order1_known_values() {
        // Order 1 (2x2 grid): Hilbert curve visits (0,0)->(0,1)->(1,1)->(1,0)
        assert_eq!(hilbert_to_xy(0, 1), (0, 0));
        assert_eq!(hilbert_to_xy(1, 1), (0, 1));
        assert_eq!(hilbert_to_xy(2, 1), (1, 1));
        assert_eq!(hilbert_to_xy(3, 1), (1, 0));
    }

    #[test]
    fn test_zoom_offset_values() {
        assert_eq!(zoom_offset(0), 0);
        assert_eq!(zoom_offset(1), 1); // 4^0 = 1 tile at z=0
        assert_eq!(zoom_offset(2), 5); // 1 + 4 = 5
        assert_eq!(zoom_offset(3), 21); // 1 + 4 + 16 = 21
    }

    #[test]
    fn test_zxy_to_tile_id_z0() {
        let id = zxy_to_tile_id(0, 0, 0).expect("valid");
        assert_eq!(id, 0);
    }

    #[test]
    fn test_tile_id_to_zxy_z0() {
        let (z, x, y) = tile_id_to_zxy(0).expect("valid");
        assert_eq!((z, x, y), (0, 0, 0));
    }

    #[test]
    fn test_zxy_tile_id_round_trip_z1() {
        for x in 0..2u32 {
            for y in 0..2u32 {
                let id = zxy_to_tile_id(1, x, y).expect("valid");
                let (z2, x2, y2) = tile_id_to_zxy(id).expect("valid");
                assert_eq!((1, x, y), (z2, x2, y2));
            }
        }
    }

    #[test]
    fn test_zxy_tile_id_round_trip_z2() {
        for x in 0..4u32 {
            for y in 0..4u32 {
                let id = zxy_to_tile_id(2, x, y).expect("valid");
                let (z2, x2, y2) = tile_id_to_zxy(id).expect("valid");
                assert_eq!((2, x, y), (z2, x2, y2));
            }
        }
    }

    #[test]
    fn test_zxy_tile_id_round_trip_z5() {
        let dim = 1u32 << 5;
        for x in 0..dim {
            for y in 0..dim {
                let id = zxy_to_tile_id(5, x, y).expect("valid");
                let (z2, x2, y2) = tile_id_to_zxy(id).expect("valid");
                assert_eq!((5, x, y), (z2, x2, y2));
            }
        }
    }

    #[test]
    fn test_zxy_out_of_range_error() {
        assert!(zxy_to_tile_id(1, 2, 0).is_err());
        assert!(zxy_to_tile_id(1, 0, 2).is_err());
        assert!(zxy_to_tile_id(0, 1, 0).is_err());
    }

    #[test]
    fn test_zxy_zoom_too_large() {
        assert!(zxy_to_tile_id(27, 0, 0).is_err());
    }

    #[test]
    fn test_tile_id_first_of_each_zoom() {
        // First tile at each zoom is at the zoom offset
        for z in 0..6u8 {
            let id = zxy_to_tile_id(z, 0, 0).expect("valid");
            assert_eq!(id, zoom_offset(z));
        }
    }
}
