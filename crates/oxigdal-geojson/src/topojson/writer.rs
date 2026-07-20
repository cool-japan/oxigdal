//! TopoJSON 3.0 writer — converts a `FeatureCollection` to a Topology string.
//!
//! The implementation follows the [TopoJSON specification](https://github.com/topojson/topojson-specification).
//!
//! ## Usage
//!
//! ```rust,ignore
//! use oxigdal_geojson_stream::{feature_collection_to_topojson, TopoOptions, FeatureCollection};
//!
//! let topo = feature_collection_to_topojson(fc, TopoOptions::default())?;
//! ```

use serde::Serialize;
use serde_json::{Map, Value};

use crate::error::GeoJsonError;
use crate::parser::FeatureCollection;
use crate::types::GeoJsonGeometry;

use super::arcs::{delta_encode, detect_junctions, extract_arcs, normalize_ring};
use super::quantize::{QuantTransform, RingSource, collect_all_rings, compute_fc_bbox};

// ─── Public API ───────────────────────────────────────────────────────────────

/// Configuration for the TopoJSON encoder.
#[derive(Debug, Clone)]
pub struct TopoOptions {
    /// Integer grid resolution (default `10 000`).  Higher values give more
    /// faithful coordinate representation at the cost of larger output.
    pub quantization: u32,
    /// Name of the geometry object within the `"objects"` map.
    pub object_name: String,
    /// Emit pretty-printed JSON (default `false`).
    pub pretty: bool,
    /// Number of decimal places for the `transform.scale` and
    /// `transform.translate` values (default `6`).
    pub coordinate_precision: usize,
    /// Include a top-level `"bbox"` field (default `true`).
    pub include_bbox: bool,
}

impl Default for TopoOptions {
    fn default() -> Self {
        Self {
            quantization: 10_000,
            object_name: "data".to_string(),
            pretty: false,
            coordinate_precision: 6,
            include_bbox: true,
        }
    }
}

impl TopoOptions {
    /// Set the quantisation grid size.
    #[must_use]
    pub fn with_quantization(mut self, q: u32) -> Self {
        self.quantization = q;
        self
    }

    /// Set the object name in the `"objects"` map.
    #[must_use]
    pub fn with_object_name(mut self, name: impl Into<String>) -> Self {
        self.object_name = name.into();
        self
    }

    /// Enable pretty-printed JSON output.
    #[must_use]
    pub fn pretty(mut self) -> Self {
        self.pretty = true;
        self
    }
}

// ─── Serde-serialisable output structs ──────────────────────────────────────

#[derive(Serialize)]
struct Topology {
    #[serde(rename = "type")]
    topology_type: &'static str,
    transform: TransformJson,
    #[serde(skip_serializing_if = "Option::is_none")]
    bbox: Option<[f64; 4]>,
    objects: Map<String, Value>,
    arcs: Vec<Vec<[i32; 2]>>,
}

#[derive(Serialize)]
struct TransformJson {
    scale: [f64; 2],
    translate: [f64; 2],
}

// ─── Main entry point ────────────────────────────────────────────────────────

/// Encode a `FeatureCollection` as a TopoJSON 3.0 string.
///
/// # Errors
///
/// Returns [`GeoJsonError::TopologyError`] when:
/// - The feature collection is empty or contains no coordinate data.
/// - JSON serialisation fails (should not happen with well-formed data).
pub fn feature_collection_to_topojson(
    fc: FeatureCollection,
    options: TopoOptions,
) -> Result<String, GeoJsonError> {
    // ── 1. Compute bbox & build quantisation transform ─────────────────────
    let bbox = compute_fc_bbox(&fc).ok_or_else(|| {
        GeoJsonError::TopologyError("empty FeatureCollection — no coordinate data".into())
    })?;

    let transform = QuantTransform::from_bbox(&bbox, options.quantization);

    // ── 2. Collect all paths — polygon rings *and* line chains (with source
    //       metadata).  Rings are normalised (closing duplicate vertex removed);
    //       open chains are kept verbatim.  Degenerate paths are dropped.
    let (norm_rings, norm_sources) = collect_all_rings_normalised(&fc, &transform);
    let is_ring: Vec<bool> = norm_sources.iter().map(|s| s.is_ring).collect();

    // ── 3. Detect junctions & extract arcs ────────────────────────────────
    let junctions = detect_junctions(&norm_rings, &is_ring);
    let (raw_arcs, ring_arc_indices) = extract_arcs(&norm_rings, &is_ring, &junctions);

    // ── 4. Delta-encode arcs ───────────────────────────────────────────────
    let encoded_arcs: Vec<Vec<[i32; 2]>> = raw_arcs.iter().map(|a| delta_encode(a)).collect();

    // ── 5. Build the "objects" map with per-feature geometries ─────────────
    let objects = build_objects(&fc, &norm_sources, &ring_arc_indices, &transform, &options)?;

    // ── 6. Assemble and serialise ──────────────────────────────────────────
    let topology = Topology {
        topology_type: "Topology",
        transform: TransformJson {
            scale: [transform.scale_x, transform.scale_y],
            translate: [transform.translate_x, transform.translate_y],
        },
        bbox: if options.include_bbox {
            Some([bbox.min_x, bbox.min_y, bbox.max_x, bbox.max_y])
        } else {
            None
        },
        objects,
        arcs: encoded_arcs,
    };

    if options.pretty {
        serde_json::to_string_pretty(&topology)
            .map_err(|e| GeoJsonError::TopologyError(e.to_string()))
    } else {
        serde_json::to_string(&topology).map_err(|e| GeoJsonError::TopologyError(e.to_string()))
    }
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Like `collect_all_rings` but prepares each path for arc extraction:
/// closed polygon rings are normalised (closing duplicate vertex removed) while
/// open line chains are kept verbatim (a chain's last vertex is meaningful even
/// when it coincides with the first — a closed LineString is a loop, not a
/// ring).  Paths with fewer than 2 vertices are dropped, keeping sources
/// aligned.
fn collect_all_rings_normalised(
    fc: &FeatureCollection,
    transform: &QuantTransform,
) -> (Vec<Vec<super::quantize::QuantPoint>>, Vec<RingSource>) {
    let (raw_rings, raw_sources) = collect_all_rings(fc, transform);
    let mut rings = Vec::with_capacity(raw_rings.len());
    let mut sources = Vec::with_capacity(raw_sources.len());
    for (path, src) in raw_rings.into_iter().zip(raw_sources) {
        let prepared = if src.is_ring {
            normalize_ring(path)
        } else {
            path
        };
        if prepared.len() >= 2 {
            rings.push(prepared);
            sources.push(src);
        }
    }
    (rings, sources)
}

/// Build the `objects` map: a single `GeometryCollection` keyed by
/// `options.object_name`, where each feature becomes one geometry entry.
fn build_objects(
    fc: &FeatureCollection,
    sources: &[RingSource],
    ring_arc_indices: &[Vec<i32>],
    transform: &QuantTransform,
    options: &TopoOptions,
) -> Result<Map<String, Value>, GeoJsonError> {
    // Map: feature_idx → Vec of (poly_idx, ring_idx, arc_refs)
    // We need to reconstruct the per-feature arc structure from the flat ring list.
    let geom_values: Vec<Value> = fc
        .features
        .iter()
        .enumerate()
        .map(|(feat_idx, feat)| {
            let geom = match &feat.geometry {
                Some(g) if !matches!(g, GeoJsonGeometry::Null) => g,
                _ => return Value::Null,
            };

            build_geometry_value(geom, feat_idx, &[], sources, ring_arc_indices, transform)
        })
        .filter(|v| !v.is_null())
        .collect();

    let geometry_collection = serde_json::json!({
        "type": "GeometryCollection",
        "geometries": geom_values
    });

    let mut objects = Map::new();
    objects.insert(options.object_name.clone(), geometry_collection);
    Ok(objects)
}

/// Build the TopoJSON geometry `Value` for a single GeoJSON geometry,
/// referencing arc indices for polygon/line geometries and quantised
/// coordinates for point geometries.
fn build_geometry_value(
    geom: &GeoJsonGeometry,
    feat_idx: usize,
    member_path: &[usize],
    sources: &[RingSource],
    ring_arc_indices: &[Vec<i32>],
    transform: &QuantTransform,
) -> Value {
    match geom {
        GeoJsonGeometry::Point([x, y]) => {
            let (qx, qy) = transform.quantise(*x, *y);
            serde_json::json!({ "type": "Point", "coordinates": [qx, qy] })
        }
        GeoJsonGeometry::PointZ([x, y, _]) => {
            let (qx, qy) = transform.quantise(*x, *y);
            serde_json::json!({ "type": "Point", "coordinates": [qx, qy] })
        }
        GeoJsonGeometry::MultiPoint(pts) => {
            let coords: Vec<Value> = pts
                .iter()
                .map(|[x, y]| {
                    let (qx, qy) = transform.quantise(*x, *y);
                    serde_json::json!([qx, qy])
                })
                .collect();
            serde_json::json!({ "type": "MultiPoint", "coordinates": coords })
        }
        GeoJsonGeometry::MultiPointZ(pts) => {
            let coords: Vec<Value> = pts
                .iter()
                .map(|[x, y, _]| {
                    let (qx, qy) = transform.quantise(*x, *y);
                    serde_json::json!([qx, qy])
                })
                .collect();
            serde_json::json!({ "type": "MultiPoint", "coordinates": coords })
        }
        GeoJsonGeometry::Polygon(rings) => build_polygon_value(
            feat_idx,
            member_path,
            0,
            rings.len(),
            sources,
            ring_arc_indices,
        ),
        GeoJsonGeometry::PolygonZ(rings) => build_polygon_value(
            feat_idx,
            member_path,
            0,
            rings.len(),
            sources,
            ring_arc_indices,
        ),
        GeoJsonGeometry::MultiPolygon(polys) => {
            build_multipolygon_value(feat_idx, member_path, polys, sources, ring_arc_indices)
        }
        GeoJsonGeometry::MultiPolygonZ(polys) => {
            // Use ring counts from PolygonZ slices
            let ring_counts: Vec<usize> = polys.iter().map(|p| p.len()).collect();
            build_multipolygon_value_counts(
                feat_idx,
                member_path,
                &ring_counts,
                sources,
                ring_arc_indices,
            )
        }
        GeoJsonGeometry::LineString(_) | GeoJsonGeometry::LineStringZ(_) => {
            build_line_value(feat_idx, member_path, sources, ring_arc_indices)
        }
        GeoJsonGeometry::MultiLineString(lines) => build_multiline_value(
            feat_idx,
            member_path,
            lines.len(),
            sources,
            ring_arc_indices,
        ),
        GeoJsonGeometry::MultiLineStringZ(lines) => build_multiline_value(
            feat_idx,
            member_path,
            lines.len(),
            sources,
            ring_arc_indices,
        ),
        GeoJsonGeometry::GeometryCollection(geoms) => {
            let geom_values: Vec<Value> = geoms
                .iter()
                .enumerate()
                .map(|(member_idx, g)| {
                    let mut child_path = member_path.to_vec();
                    child_path.push(member_idx);
                    build_geometry_value(
                        g,
                        feat_idx,
                        &child_path,
                        sources,
                        ring_arc_indices,
                        transform,
                    )
                })
                .collect();
            serde_json::json!({ "type": "GeometryCollection", "geometries": geom_values })
        }
        GeoJsonGeometry::Null => Value::Null,
    }
}

/// Build a TopoJSON `Polygon` geometry value from arc index lookup.
///
/// Scans `sources` for rings that belong to `(feat_idx, poly_idx=0)`, in
/// ring order, and collects their arc index sequences.
fn build_polygon_value(
    feat_idx: usize,
    member_path: &[usize],
    poly_idx: usize,
    num_rings: usize,
    sources: &[RingSource],
    ring_arc_indices: &[Vec<i32>],
) -> Value {
    let mut arcs_per_ring: Vec<Vec<Value>> = Vec::with_capacity(num_rings);

    // Collect all rings for this (feat_idx, member_path, poly_idx), sorted by ring_idx
    let mut ring_entries: Vec<(usize, &Vec<i32>)> = sources
        .iter()
        .enumerate()
        .filter(|(_, src)| {
            src.is_ring
                && src.feature_idx == feat_idx
                && src.member_path == member_path
                && src.poly_idx == poly_idx
        })
        .map(|(ring_slot, src)| (src.ring_idx, &ring_arc_indices[ring_slot]))
        .collect();

    ring_entries.sort_unstable_by_key(|(ri, _)| *ri);

    for (_, arc_refs) in ring_entries {
        let arc_values: Vec<Value> = arc_refs.iter().map(|&r| Value::from(r)).collect();
        arcs_per_ring.push(arc_values);
    }

    serde_json::json!({ "type": "Polygon", "arcs": arcs_per_ring })
}

/// Build a TopoJSON `MultiPolygon` geometry value.
fn build_multipolygon_value(
    feat_idx: usize,
    member_path: &[usize],
    polys: &[Vec<Vec<[f64; 2]>>],
    sources: &[RingSource],
    ring_arc_indices: &[Vec<i32>],
) -> Value {
    let ring_counts: Vec<usize> = polys.iter().map(|p| p.len()).collect();
    build_multipolygon_value_counts(
        feat_idx,
        member_path,
        &ring_counts,
        sources,
        ring_arc_indices,
    )
}

/// Collect the ordered, signed arc references for a single line chain
/// identified by `(feat_idx, line_idx)`.
///
/// Line chains are stored with `is_ring == false`; a plain LineString uses
/// `line_idx == 0`, while a MultiLineString stores its `k`-th line at
/// `line_idx == k`.  Each matching source contributes its arc-reference list
/// (a chain is a single path, so at most one source matches for a well-formed
/// top-level geometry).
fn collect_chain_arc_refs(
    feat_idx: usize,
    member_path: &[usize],
    line_idx: usize,
    sources: &[RingSource],
    ring_arc_indices: &[Vec<i32>],
) -> Vec<Value> {
    sources
        .iter()
        .enumerate()
        .filter(|(_, src)| {
            !src.is_ring
                && src.feature_idx == feat_idx
                && src.member_path == member_path
                && src.poly_idx == line_idx
        })
        .flat_map(|(slot, _)| ring_arc_indices[slot].iter().map(|&r| Value::from(r)))
        .collect()
}

/// Build a TopoJSON `LineString` geometry value from arc index lookup.
///
/// A LineString's `"arcs"` is a *flat* array of signed arc references (unlike a
/// Polygon, whose `"arcs"` is an array of per-ring arrays).
fn build_line_value(
    feat_idx: usize,
    member_path: &[usize],
    sources: &[RingSource],
    ring_arc_indices: &[Vec<i32>],
) -> Value {
    let arc_values = collect_chain_arc_refs(feat_idx, member_path, 0, sources, ring_arc_indices);
    serde_json::json!({ "type": "LineString", "arcs": arc_values })
}

/// Build a TopoJSON `MultiLineString` geometry value.
///
/// A MultiLineString's `"arcs"` is an array of arrays: one flat arc-reference
/// list per constituent line, in line order.
fn build_multiline_value(
    feat_idx: usize,
    member_path: &[usize],
    num_lines: usize,
    sources: &[RingSource],
    ring_arc_indices: &[Vec<i32>],
) -> Value {
    let lines_value: Vec<Value> = (0..num_lines)
        .map(|line_idx| {
            Value::Array(collect_chain_arc_refs(
                feat_idx,
                member_path,
                line_idx,
                sources,
                ring_arc_indices,
            ))
        })
        .collect();
    serde_json::json!({ "type": "MultiLineString", "arcs": lines_value })
}

/// Build a TopoJSON `MultiPolygon` value given per-polygon ring counts.
fn build_multipolygon_value_counts(
    feat_idx: usize,
    member_path: &[usize],
    ring_counts: &[usize],
    sources: &[RingSource],
    ring_arc_indices: &[Vec<i32>],
) -> Value {
    let mut polys_value: Vec<Value> = Vec::with_capacity(ring_counts.len());

    for (poly_idx, &num_rings) in ring_counts.iter().enumerate() {
        let poly_v = build_polygon_value(
            feat_idx,
            member_path,
            poly_idx,
            num_rings,
            sources,
            ring_arc_indices,
        );
        // Extract the inner "arcs" array from the polygon value
        let inner_arcs = poly_v.get("arcs").cloned().unwrap_or(Value::Array(vec![]));
        polys_value.push(inner_arcs);
    }

    serde_json::json!({ "type": "MultiPolygon", "arcs": polys_value })
}
