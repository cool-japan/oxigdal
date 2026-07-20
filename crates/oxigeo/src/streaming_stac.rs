//! STAC feature streaming implementation.
//!
//! Reads a STAC JSON file (ItemCollection, Feature/Item, Catalog, or
//! Collection) from the local filesystem and yields each STAC Item as a
//! [`StreamingFeature`].
//!
//! # File-type dispatch
//!
//! | JSON `"type"` field      | Behaviour                                   |
//! |--------------------------|---------------------------------------------|
//! | `"FeatureCollection"`    | Iterate the `features` array                |
//! | `"Feature"`              | Single-element stream                       |
//! | `"Catalog"` / `"Collection"` | Empty stream (remote links not followed) |
//! | anything else            | Empty stream                                |
//!
//! Geometry is encoded as ISO WKB little-endian bytes from the GeoJSON
//! geometry value using `oxigeo_geojson::Geometry::to_wkb`.  Properties are
//! built from `item.properties` merged with flattened asset hrefs
//! (`assets.<key>` → `{"href": "…", "type": "…", …}`).
//!
//! # Remote catalogs
//!
//! Following `links[rel=item]` that point to HTTP/HTTPS URIs would require
//! either async I/O or a blocking HTTP client, neither of which is available
//! here without the `reqwest` feature.  Catalog-type files therefore return an
//! empty stream; callers that need remote catalog traversal should use the
//! `oxigeo_stac::StacClient` directly.

use std::collections::HashMap;

use oxigeo_core::error::{IoError, OxiGeoError};
use serde_json::Value as JsonValue;

use crate::streaming::{FeatureStream, StreamingFeature};
use crate::{DatasetInfo, Result};

/// Stream features from a STAC JSON file specified by `info.path`.
///
/// When the `stac` feature is enabled and `info.path` points to a readable
/// STAC file, this returns a [`FeatureStream`] over STAC Items.
///
/// Falls back to an empty stream when:
/// - `info.path` is `None` (programmatic dataset)
/// - the file cannot be read
/// - the JSON `"type"` is `"Catalog"` or `"Collection"` (remote links not followed)
pub(crate) fn stream_stac_features(info: &DatasetInfo) -> Result<FeatureStream> {
    let path = match &info.path {
        Some(p) => p.clone(),
        None => return Ok(FeatureStream::empty()),
    };

    let file = std::fs::File::open(&path).map_err(|e| {
        OxiGeoError::Io(IoError::Read {
            message: format!("cannot open STAC file for streaming '{path}': {e}"),
        })
    })?;

    let root: JsonValue = serde_json::from_reader(std::io::BufReader::new(file)).map_err(|e| {
        OxiGeoError::Internal {
            message: format!("cannot parse STAC JSON from '{path}': {e}"),
        }
    })?;

    let type_str = root
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_uppercase();

    let feature_values: Vec<&JsonValue> = match type_str.as_str() {
        "FEATURECOLLECTION" => {
            // Standard GeoJSON FeatureCollection / STAC ItemCollection.
            root.get("features")
                .and_then(|f| f.as_array())
                .map(|arr| arr.iter().collect())
                .unwrap_or_default()
        }
        "FEATURE" => {
            // Single STAC Item exposed directly.
            vec![&root]
        }
        _ => {
            // Catalog, Collection, or unknown — empty stream.
            Vec::new()
        }
    };

    let features = feature_values
        .into_iter()
        .map(stac_feature_to_streaming)
        .collect::<Vec<_>>();

    Ok(FeatureStream::from_vec(features))
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Convert a single STAC Feature JSON value to a [`StreamingFeature`].
///
/// - `geometry` → WKB bytes (None if absent or not encodable)
/// - `properties` → copied as-is
/// - `assets` → each key flattened as `"assets.<key>"` with its JSON object
/// - `id` → set as the feature identifier
fn stac_feature_to_streaming(feature: &JsonValue) -> StreamingFeature {
    // ── Geometry ─────────────────────────────────────────────────────────────
    let geometry = feature
        .get("geometry")
        .filter(|g| !g.is_null())
        .and_then(geojson_value_to_wkb);

    // ── Properties ───────────────────────────────────────────────────────────
    let mut properties: HashMap<String, JsonValue> = feature
        .get("properties")
        .and_then(|p| p.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();

    // ── Assets (flattened) ────────────────────────────────────────────────────
    // Each asset is stored as `"assets.<key>"` → its JSON object value.
    if let Some(assets_obj) = feature.get("assets").and_then(|a| a.as_object()) {
        for (asset_key, asset_val) in assets_obj {
            properties.insert(format!("assets.{asset_key}"), asset_val.clone());
        }
    }

    // ── Identifier ───────────────────────────────────────────────────────────
    let mut sf = StreamingFeature::new(geometry, properties);
    if let Some(id) = feature.get("id").and_then(|v| v.as_str()) {
        sf = sf.with_id(id);
    }

    sf
}

/// Encode a GeoJSON geometry [`JsonValue`] as little-endian ISO WKB bytes.
///
/// Returns `None` when the geometry type is unsupported or the value is
/// malformed.  Only 2D coordinate types are handled (Z/M are ignored).
fn geojson_value_to_wkb(geom: &JsonValue) -> Option<Vec<u8>> {
    let type_str = geom.get("type")?.as_str()?;
    let coords = geom.get("coordinates");

    match type_str {
        "Point" => {
            let c = coords?.as_array()?;
            let x = c.first()?.as_f64()?;
            let y = c.get(1)?.as_f64()?;
            Some(wkb_point(x, y))
        }
        "LineString" => {
            let pts = coords?
                .as_array()?
                .iter()
                .filter_map(coord2d)
                .collect::<Vec<_>>();
            if pts.is_empty() {
                return None;
            }
            Some(wkb_linestring(&pts))
        }
        "Polygon" => {
            let rings_raw = coords?.as_array()?;
            let rings: Vec<Vec<(f64, f64)>> = rings_raw
                .iter()
                .map(|ring| {
                    ring.as_array()
                        .map(|pts| pts.iter().filter_map(coord2d).collect())
                        .unwrap_or_default()
                })
                .collect();
            if rings.is_empty() || rings[0].is_empty() {
                return None;
            }
            Some(wkb_polygon(&rings))
        }
        "MultiPoint" => {
            let pts = coords?
                .as_array()?
                .iter()
                .filter_map(coord2d)
                .collect::<Vec<_>>();
            Some(wkb_multi_point(&pts))
        }
        "MultiLineString" => {
            let lines: Vec<Vec<(f64, f64)>> = coords?
                .as_array()?
                .iter()
                .map(|line| {
                    line.as_array()
                        .map(|pts| pts.iter().filter_map(coord2d).collect())
                        .unwrap_or_default()
                })
                .collect();
            Some(wkb_multi_linestring(&lines))
        }
        "MultiPolygon" => {
            let polys: Vec<Vec<Vec<(f64, f64)>>> = coords?
                .as_array()?
                .iter()
                .map(|poly| {
                    poly.as_array()
                        .map(|rings| {
                            rings
                                .iter()
                                .map(|ring| {
                                    ring.as_array()
                                        .map(|pts| pts.iter().filter_map(coord2d).collect())
                                        .unwrap_or_default()
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .collect();
            Some(wkb_multi_polygon(&polys))
        }
        _ => None,
    }
}

/// Extract a 2D (x, y) coordinate from a GeoJSON coordinate array value.
fn coord2d(v: &JsonValue) -> Option<(f64, f64)> {
    let arr = v.as_array()?;
    let x = arr.first()?.as_f64()?;
    let y = arr.get(1)?.as_f64()?;
    Some((x, y))
}

// ── Minimal WKB encoders (ISO WKB little-endian) ─────────────────────────────

/// Write an IEEE-754 double-precision float as 8 LE bytes.
#[inline]
fn write_f64_le(buf: &mut Vec<u8>, v: f64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

/// Write a u32 as 4 LE bytes.
#[inline]
fn write_u32_le(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

/// Encode a 2D WKB Point (type code 1).
fn wkb_point(x: f64, y: f64) -> Vec<u8> {
    let mut buf = Vec::with_capacity(21);
    buf.push(1); // LE byte order
    write_u32_le(&mut buf, 1); // WKB Point
    write_f64_le(&mut buf, x);
    write_f64_le(&mut buf, y);
    buf
}

/// Encode a 2D WKB LineString (type code 2).
fn wkb_linestring(pts: &[(f64, f64)]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(9 + pts.len() * 16);
    buf.push(1);
    write_u32_le(&mut buf, 2); // WKB LineString
    write_u32_le(&mut buf, pts.len() as u32);
    for &(x, y) in pts {
        write_f64_le(&mut buf, x);
        write_f64_le(&mut buf, y);
    }
    buf
}

/// Encode a 2D WKB Polygon (type code 3).
///
/// `rings[0]` is the exterior ring; subsequent rings are interior rings.
fn wkb_polygon(rings: &[Vec<(f64, f64)>]) -> Vec<u8> {
    let total_pts: usize = rings.iter().map(|r| r.len()).sum();
    let mut buf = Vec::with_capacity(9 + rings.len() * 4 + total_pts * 16);
    buf.push(1);
    write_u32_le(&mut buf, 3); // WKB Polygon
    write_u32_le(&mut buf, rings.len() as u32);
    for ring in rings {
        write_u32_le(&mut buf, ring.len() as u32);
        for &(x, y) in ring {
            write_f64_le(&mut buf, x);
            write_f64_le(&mut buf, y);
        }
    }
    buf
}

/// Encode a 2D WKB MultiPoint (type code 4).
fn wkb_multi_point(pts: &[(f64, f64)]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(9 + pts.len() * 21);
    buf.push(1);
    write_u32_le(&mut buf, 4); // WKB MultiPoint
    write_u32_le(&mut buf, pts.len() as u32);
    for &(x, y) in pts {
        buf.extend_from_slice(&wkb_point(x, y));
    }
    buf
}

/// Encode a 2D WKB MultiLineString (type code 5).
fn wkb_multi_linestring(lines: &[Vec<(f64, f64)>]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(1);
    write_u32_le(&mut buf, 5); // WKB MultiLineString
    write_u32_le(&mut buf, lines.len() as u32);
    for line in lines {
        buf.extend_from_slice(&wkb_linestring(line));
    }
    buf
}

/// Encode a 2D WKB MultiPolygon (type code 6).
fn wkb_multi_polygon(polys: &[Vec<Vec<(f64, f64)>>]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(1);
    write_u32_le(&mut buf, 6); // WKB MultiPolygon
    write_u32_le(&mut buf, polys.len() as u32);
    for poly in polys {
        buf.extend_from_slice(&wkb_polygon(poly));
    }
    buf
}
