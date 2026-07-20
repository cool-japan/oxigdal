//! Feature sorting by property value or spatial key (Hilbert / geohash).
//!
//! # Overview
//!
//! Three sort key variants are provided:
//!
//! - [`FeatureSortKey::Property`] — lexicographic / natural ordering on a JSON
//!   property value with a total order across all JSON types.
//! - [`FeatureSortKey::Hilbert`] — maps each feature's centroid to a Hilbert
//!   curve index, yielding spatial locality.
//! - [`FeatureSortKey::Geohash`] — encodes each feature's centroid as a
//!   base-32 geohash string; lexicographic ordering of geohash strings
//!   approximates spatial proximity.
//!
//! Features with no centroid (null geometry, empty geometry collection) sort to
//! the end in ascending order regardless of key type.

use std::cmp::Ordering;

use serde_json::Value;

use crate::error::GeoJsonError;
use crate::parser::FeatureCollection;
use crate::types::GeoJsonFeature;

// ─── Public types ────────────────────────────────────────────────────────────

/// Discriminates the sort key used when ordering [`GeoJsonFeature`]s.
#[derive(Debug, Clone)]
pub enum FeatureSortKey {
    /// Sort by a named JSON property value.
    ///
    /// Total order across JSON types: `Null < Bool < Number < String < Array < Object`.
    /// Features whose properties object does not contain the named key are
    /// placed **after** all features that do contain it (in ascending order).
    Property(String),

    /// Sort by Hilbert curve index derived from the feature centroid.
    ///
    /// `precision` is the number of bits per axis used to quantise longitude
    /// and latitude into a 2^precision × 2^precision grid. Values outside the
    /// range `[1, 20]` are clamped. Higher precision distinguishes nearby
    /// points at the cost of a coarser guarantee for very distant points
    /// (the index is a `u64`; at `precision = 20` the grid is 1 048 576 ×
    /// 1 048 576 and the index occupies 40 bits).
    Hilbert {
        /// Bits per axis; clamped to `[1, 20]`.
        precision: u8,
    },

    /// Sort by standard base-32 geohash derived from the feature centroid.
    ///
    /// `precision` is the number of geohash characters (each character encodes
    /// 5 bits). Values outside `[1, 12]` are clamped. Lexicographic ordering
    /// of geohash strings approximates spatial clustering.
    Geohash {
        /// Character count; clamped to `[1, 12]`.
        precision: u8,
    },
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    /// Smallest first.
    Ascending,
    /// Largest first.
    Descending,
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Sort a mutable slice of features in place.
///
/// # Errors
///
/// Currently infallible; returns `Ok(())` always.  The `Result` return type is
/// present for forward compatibility (future key variants may require I/O).
pub fn sort_features(
    features: &mut [GeoJsonFeature],
    key: &FeatureSortKey,
    order: SortOrder,
) -> Result<(), GeoJsonError> {
    features.sort_by(|a, b| {
        let ord = compare_features_by_key(a, b, key);
        if order == SortOrder::Descending {
            ord.reverse()
        } else {
            ord
        }
    });
    Ok(())
}

/// Sort an owned `Vec<GeoJsonFeature>`, returning the sorted vec.
///
/// # Errors
///
/// Currently infallible.
pub fn sort_features_owned(
    mut features: Vec<GeoJsonFeature>,
    key: &FeatureSortKey,
    order: SortOrder,
) -> Result<Vec<GeoJsonFeature>, GeoJsonError> {
    sort_features(&mut features, key, order)?;
    Ok(features)
}

/// Sort the features inside a [`FeatureCollection`] in place.
///
/// The collection's metadata (`bbox`, `crs`, `name`) is **not** modified.
///
/// # Errors
///
/// Currently infallible.
pub fn sort_feature_collection(
    fc: &mut FeatureCollection,
    key: &FeatureSortKey,
    order: SortOrder,
) -> Result<(), GeoJsonError> {
    sort_features(&mut fc.features, key, order)
}

// ─── Spatial key functions ───────────────────────────────────────────────────

/// Extract the 2-D centroid `(lon, lat)` of a feature's geometry.
///
/// Returns `None` if the feature has no geometry or the geometry is empty /
/// null.
#[must_use]
pub fn feature_centroid(feature: &GeoJsonFeature) -> Option<(f64, f64)> {
    feature.geometry.as_ref()?.centroid().map(|[x, y]| (x, y))
}

/// Map `(lon, lat)` to a Hilbert curve index over the global bounding box
/// `[-180, 180] × [-90, 90]`.
///
/// `precision` = bits per axis (clamped to `[1, 20]`).  The grid size is
/// `2^precision × 2^precision`.  The returned index fits in a `u64` for all
/// valid precisions.
#[must_use]
pub fn hilbert_key(lon: f64, lat: f64, precision: u8) -> u64 {
    let precision = precision.clamp(1, 20);
    let n = 1u32 << precision;
    let n_f = n as f64;

    // Normalise to [0, n-1] grid — clamp to handle exact boundary values.
    let gx = ((lon + 180.0) / 360.0 * n_f).min(n_f - 1.0).max(0.0) as u32;
    let gy = ((lat + 90.0) / 180.0 * n_f).min(n_f - 1.0).max(0.0) as u32;

    xy_to_hilbert_d(n, gx, gy)
}

/// Map `(lon, lat)` to a standard base-32 geohash string of `precision`
/// characters.
///
/// The returned string uses the standard Niemeyer/Gustavo Niemeyer alphabet
/// `0-9bcdefghjkmnpqrstuvwxyz`.  `precision` is clamped to `[1, 12]`.
#[must_use]
pub fn geohash_key(lon: f64, lat: f64, precision: u8) -> String {
    // Standard geohash base-32 alphabet
    const BASE32: &[u8] = b"0123456789bcdefghjkmnpqrstuvwxyz";
    let precision = precision.clamp(1, 12) as usize;

    let mut lat_min = -90.0_f64;
    let mut lat_max = 90.0_f64;
    let mut lon_min = -180.0_f64;
    let mut lon_max = 180.0_f64;

    let mut hash = String::with_capacity(precision);
    let mut bits = 0u8;
    let mut bit_count = 0u8;
    // geohash alternates: first bit is longitude (even), then latitude (odd)
    let mut is_lon = true;

    while hash.len() < precision {
        if is_lon {
            let mid = (lon_min + lon_max) / 2.0;
            if lon >= mid {
                bits = (bits << 1) | 1;
                lon_min = mid;
            } else {
                bits <<= 1;
                lon_max = mid;
            }
        } else {
            let mid = (lat_min + lat_max) / 2.0;
            if lat >= mid {
                bits = (bits << 1) | 1;
                lat_min = mid;
            } else {
                bits <<= 1;
                lat_max = mid;
            }
        }
        is_lon = !is_lon;
        bit_count += 1;

        if bit_count == 5 {
            hash.push(BASE32[bits as usize] as char);
            bits = 0;
            bit_count = 0;
        }
    }

    hash
}

// ─── Internal comparison ─────────────────────────────────────────────────────

fn compare_features_by_key(
    a: &GeoJsonFeature,
    b: &GeoJsonFeature,
    key: &FeatureSortKey,
) -> Ordering {
    match key {
        FeatureSortKey::Property(name) => {
            // properties is Option<serde_json::Value>; the Value is typically
            // an Object, so .get(name) works directly on serde_json::Value.
            let va = a.properties.as_ref().and_then(|p| p.get(name.as_str()));
            let vb = b.properties.as_ref().and_then(|p| p.get(name.as_str()));
            match (va, vb) {
                (None, None) => Ordering::Equal,
                // Missing property sorts last in ascending order.
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(av), Some(bv)) => compare_json_values(av, bv),
            }
        }
        FeatureSortKey::Hilbert { precision } => {
            let ka = feature_centroid(a)
                .map(|(x, y)| hilbert_key(x, y, *precision))
                .unwrap_or(u64::MAX);
            let kb = feature_centroid(b)
                .map(|(x, y)| hilbert_key(x, y, *precision))
                .unwrap_or(u64::MAX);
            ka.cmp(&kb)
        }
        FeatureSortKey::Geohash { precision } => {
            let sentinel = "\u{ffff}".repeat(*precision as usize);
            let ka = feature_centroid(a)
                .map(|(x, y)| geohash_key(x, y, *precision))
                .unwrap_or_else(|| sentinel.clone());
            let kb = feature_centroid(b)
                .map(|(x, y)| geohash_key(x, y, *precision))
                .unwrap_or_else(|| sentinel.clone());
            ka.cmp(&kb)
        }
    }
}

// ─── Hilbert curve implementation ────────────────────────────────────────────

/// Convert (x, y) grid coordinates to a Hilbert curve distance `d`.
///
/// Uses the standard recursive rotation algorithm (Skilling 2004 / Wikipedia).
/// `n` must be a power of two.
fn xy_to_hilbert_d(n: u32, mut x: u32, mut y: u32) -> u64 {
    let mut d = 0u64;
    let mut s = n / 2;
    while s > 0 {
        let rx = u32::from((x & s) > 0);
        let ry = u32::from((y & s) > 0);
        // Each quadrant contributes s² * quadrant_index to d.
        d += (s as u64) * (s as u64) * ((3 * rx) ^ ry) as u64;
        // Rotate the sub-grid so that (rx, ry) = (0, 0) is always bottom-left.
        rotate_hilbert(s, &mut x, &mut y, rx, ry);
        s /= 2;
    }
    d
}

/// Apply the Hilbert rotation to bring (x, y) into the canonical orientation
/// for the next recursion level.
#[inline]
fn rotate_hilbert(n: u32, x: &mut u32, y: &mut u32, rx: u32, ry: u32) {
    if ry == 0 {
        if rx == 1 {
            *x = n.wrapping_sub(1).wrapping_sub(*x);
            *y = n.wrapping_sub(1).wrapping_sub(*y);
        }
        std::mem::swap(x, y);
    }
}

// ─── JSON value total order ───────────────────────────────────────────────────

/// Total order over `serde_json::Value`.
///
/// Type ranks: `Null(0) < Bool(1) < Number(2) < String(3) < Array(4) < Object(5)`.
/// Within the same type, values are compared naturally (arrays recursively).
fn compare_json_values(a: &Value, b: &Value) -> Ordering {
    let rank = |v: &Value| -> u8 {
        match v {
            Value::Null => 0,
            Value::Bool(_) => 1,
            Value::Number(_) => 2,
            Value::String(_) => 3,
            Value::Array(_) => 4,
            Value::Object(_) => 5,
        }
    };

    let ra = rank(a);
    let rb = rank(b);
    if ra != rb {
        return ra.cmp(&rb);
    }

    match (a, b) {
        (Value::Bool(av), Value::Bool(bv)) => av.cmp(bv),
        (Value::Number(av), Value::Number(bv)) => {
            let fa = av.as_f64().unwrap_or(f64::NAN);
            let fb = bv.as_f64().unwrap_or(f64::NAN);
            fa.partial_cmp(&fb).unwrap_or(Ordering::Equal)
        }
        (Value::String(av), Value::String(bv)) => av.cmp(bv),
        (Value::Array(av), Value::Array(bv)) => {
            for (ea, eb) in av.iter().zip(bv.iter()) {
                let ord = compare_json_values(ea, eb);
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            av.len().cmp(&bv.len())
        }
        // Objects: equal (we do not define a stable order for arbitrary objects)
        _ => Ordering::Equal,
    }
}
