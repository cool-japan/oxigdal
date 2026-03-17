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
