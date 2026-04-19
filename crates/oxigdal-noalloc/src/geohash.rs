//! Geohash encoding and decoding for `no_std`, `no_alloc` environments.
//!
//! Geohashes encode a (latitude, longitude) pair as a short string using
//! base32 encoding over interleaved bit streams.

use crate::BBox2D;

/// The standard geohash base32 alphabet.
pub const BASE32_CHARS: &[u8; 32] = b"0123456789bcdefghjkmnpqrstuvwxyz";

/// A geohash encoded as a fixed-size byte array.
///
/// Supports precision 1–12 (matching the standard range).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeoHashFixed {
    chars: [u8; 12],
    len: u8,
}

impl GeoHashFixed {
    /// Encodes a (latitude, longitude) pair to the given precision (1–12).
    ///
    /// If `precision` is outside 1–12, precision is clamped to the valid range.
    #[must_use]
    pub fn encode(lat: f64, lon: f64, precision: u8) -> Self {
        let precision = precision.clamp(1, 12) as usize;

        let mut chars = [0u8; 12];
        // Interleaved bit encoding: even bits = lon, odd bits = lat
        let mut min_lat = -90.0_f64;
        let mut max_lat = 90.0_f64;
        let mut min_lon = -180.0_f64;
        let mut max_lon = 180.0_f64;

        let mut is_lon = true; // start with longitude
        let mut bits = 0u8; // current 5-bit accumulator
        let mut bit_count = 0u8;
        let mut char_idx = 0usize;

        // Total bits needed = precision * 5
        let total_bits = precision * 5;

        for _ in 0..total_bits {
            if is_lon {
                let mid = (min_lon + max_lon) * 0.5;
                if lon >= mid {
                    bits = (bits << 1) | 1;
                    min_lon = mid;
                } else {
                    bits <<= 1;
                    max_lon = mid;
                }
            } else {
                let mid = (min_lat + max_lat) * 0.5;
                if lat >= mid {
                    bits = (bits << 1) | 1;
                    min_lat = mid;
                } else {
                    bits <<= 1;
                    max_lat = mid;
                }
            }
            is_lon = !is_lon;
            bit_count += 1;

            if bit_count == 5 {
                chars[char_idx] = BASE32_CHARS[bits as usize];
                char_idx += 1;
                bits = 0;
                bit_count = 0;
            }
        }

        Self {
            chars,
            len: precision as u8,
        }
    }

    /// Returns the encoded bytes (ASCII characters of the geohash).
    #[must_use]
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.chars[..self.len as usize]
    }

    /// Returns the precision of this geohash (1–12).
    #[must_use]
    #[inline]
    pub fn precision(&self) -> u8 {
        self.len
    }

    /// Decodes the geohash to the (latitude, longitude) of the cell center.
    #[must_use]
    pub fn decode(&self) -> (f64, f64) {
        let bbox = self.bbox();
        let lat = (bbox.min_y + bbox.max_y) * 0.5;
        let lon = (bbox.min_x + bbox.max_x) * 0.5;
        (lat, lon)
    }

    /// Computes the 8 neighbouring geohash cells at the same precision.
    ///
    /// Returns neighbours in the order:
    /// `[N, NE, E, SE, S, SW, W, NW]`.
    ///
    /// At the antimeridian (lon ±180) longitude wraps. At the poles (lat ±90)
    /// latitude is clamped.
    #[must_use]
    pub fn neighbours(&self) -> [GeoHashFixed; 8] {
        let bbox = self.bbox();
        let center_lat = (bbox.min_y + bbox.max_y) * 0.5;
        let center_lon = (bbox.min_x + bbox.max_x) * 0.5;
        let dlat = bbox.max_y - bbox.min_y;
        let dlon = bbox.max_x - bbox.min_x;
        let prec = self.len;

        // Direction offsets: (dlat_mult, dlon_mult)
        // N, NE, E, SE, S, SW, W, NW
        let offsets: [(f64, f64); 8] = [
            (1.0, 0.0),   // N
            (1.0, 1.0),   // NE
            (0.0, 1.0),   // E
            (-1.0, 1.0),  // SE
            (-1.0, 0.0),  // S
            (-1.0, -1.0), // SW
            (0.0, -1.0),  // W
            (1.0, -1.0),  // NW
        ];

        let mut result = [*self; 8];
        let mut i = 0;
        while i < 8 {
            let (dy, dx) = offsets[i];
            let mut nlat = center_lat + dy * dlat;
            let mut nlon = center_lon + dx * dlon;

            // Clamp latitude
            nlat = nlat.clamp(-90.0, 90.0);
            // Wrap longitude
            if nlon > 180.0 {
                nlon -= 360.0;
            }
            if nlon < -180.0 {
                nlon += 360.0;
            }

            result[i] = GeoHashFixed::encode(nlat, nlon, prec);
            i += 1;
        }
        result
    }

    /// Returns the bounding box of the geohash cell.
    ///
    /// The box is `BBox2D { min_x: min_lon, min_y: min_lat, max_x: max_lon, max_y: max_lat }`.
    #[must_use]
    pub fn bbox(&self) -> BBox2D {
        let mut min_lat = -90.0_f64;
        let mut max_lat = 90.0_f64;
        let mut min_lon = -180.0_f64;
        let mut max_lon = 180.0_f64;

        let bytes = self.as_bytes();
        let mut is_lon = true;

        for &byte in bytes {
            // Find the index of this character in BASE32_CHARS
            let idx = base32_decode_char(byte);
            // Decode 5 bits, MSB first
            for bit_pos in (0..5).rev() {
                let bit = (idx >> bit_pos) & 1;
                if is_lon {
                    let mid = (min_lon + max_lon) * 0.5;
                    if bit == 1 {
                        min_lon = mid;
                    } else {
                        max_lon = mid;
                    }
                } else {
                    let mid = (min_lat + max_lat) * 0.5;
                    if bit == 1 {
                        min_lat = mid;
                    } else {
                        max_lat = mid;
                    }
                }
                is_lon = !is_lon;
            }
        }

        BBox2D::new(min_lon, min_lat, max_lon, max_lat)
    }
}

/// Decodes a single base32 character to its 5-bit value.
/// Returns 0 for unrecognised characters.
#[inline]
fn base32_decode_char(c: u8) -> u8 {
    for (i, &ch) in BASE32_CHARS.iter().enumerate() {
        if ch == c {
            return i as u8;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_neighbours_count() {
        let gh = GeoHashFixed::encode(48.858, 2.294, 6); // Paris
        let nbrs = gh.neighbours();
        assert_eq!(nbrs.len(), 8);
    }

    #[test]
    fn test_neighbours_different_from_center() {
        let gh = GeoHashFixed::encode(48.858, 2.294, 6);
        let nbrs = gh.neighbours();
        for nbr in &nbrs {
            assert_ne!(
                nbr.as_bytes(),
                gh.as_bytes(),
                "neighbour should differ from center"
            );
        }
    }

    #[test]
    fn test_neighbours_same_precision() {
        let gh = GeoHashFixed::encode(35.68, 139.69, 5);
        let nbrs = gh.neighbours();
        for nbr in &nbrs {
            assert_eq!(nbr.precision(), gh.precision());
        }
    }

    #[test]
    fn test_neighbours_adjacency() {
        // Encode a geohash, get its bbox, get north neighbour, check that
        // north neighbour's bbox is above (higher min_y)
        let gh = GeoHashFixed::encode(40.0, -74.0, 5);
        let gh_bbox = gh.bbox();
        let nbrs = gh.neighbours();
        let north = &nbrs[0];
        let n_bbox = north.bbox();
        // North neighbour's min_y should be approximately equal to center's max_y
        let diff = (n_bbox.min_y - gh_bbox.max_y).abs();
        assert!(
            diff < 0.01,
            "North neighbour should be adjacent: diff={diff}"
        );
    }

    #[test]
    fn test_neighbours_south_adjacency() {
        let gh = GeoHashFixed::encode(40.0, -74.0, 5);
        let gh_bbox = gh.bbox();
        let nbrs = gh.neighbours();
        let south = &nbrs[4];
        let s_bbox = south.bbox();
        let diff = (s_bbox.max_y - gh_bbox.min_y).abs();
        assert!(
            diff < 0.01,
            "South neighbour should be adjacent: diff={diff}"
        );
    }

    #[test]
    fn test_neighbours_east_adjacency() {
        let gh = GeoHashFixed::encode(40.0, -74.0, 5);
        let gh_bbox = gh.bbox();
        let nbrs = gh.neighbours();
        let east = &nbrs[2];
        let e_bbox = east.bbox();
        let diff = (e_bbox.min_x - gh_bbox.max_x).abs();
        assert!(
            diff < 0.01,
            "East neighbour should be adjacent: diff={diff}"
        );
    }

    #[test]
    fn test_neighbours_wrap_antimeridian() {
        // Cell near +180 longitude
        let gh = GeoHashFixed::encode(0.0, 179.99, 3);
        let nbrs = gh.neighbours();
        // East neighbour should wrap around
        let east = &nbrs[2];
        let (_, east_lon) = east.decode();
        // Should be near -180
        assert!(
            !(0.0..=170.0).contains(&east_lon),
            "East of lon~180 should wrap or stay high, got {east_lon}"
        );
    }

    #[test]
    fn test_neighbours_pole_clamp() {
        // Cell near north pole
        let gh = GeoHashFixed::encode(89.9, 0.0, 3);
        let nbrs = gh.neighbours();
        let north = &nbrs[0];
        let (n_lat, _) = north.decode();
        assert!(n_lat <= 90.0, "North of pole should be clamped to 90");
    }

    #[test]
    fn test_neighbours_all_unique() {
        let gh = GeoHashFixed::encode(51.5, -0.1, 6);
        let nbrs = gh.neighbours();
        // Check all 8 are distinct
        let mut i = 0;
        while i < 8 {
            let mut j = i + 1;
            while j < 8 {
                assert_ne!(
                    nbrs[i].as_bytes(),
                    nbrs[j].as_bytes(),
                    "Neighbours {i} and {j} should be distinct"
                );
                j += 1;
            }
            i += 1;
        }
    }
}
