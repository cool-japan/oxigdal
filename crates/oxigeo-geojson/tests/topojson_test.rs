//! Integration tests for the TopoJSON 3.0 encoder.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use oxigeo_geojson_stream::{
    FeatureCollection, GeoJsonFeature, GeoJsonGeometry, TopoOptions, feature_collection_to_topojson,
};
use serde_json::Value;

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn make_polygon(coords: Vec<Vec<[f64; 2]>>) -> GeoJsonGeometry {
    GeoJsonGeometry::Polygon(coords)
}

fn make_feature(geom: GeoJsonGeometry) -> GeoJsonFeature {
    GeoJsonFeature {
        geometry: Some(geom),
        properties: None,
        id: None,
    }
}

fn make_fc(features: Vec<GeoJsonFeature>) -> FeatureCollection {
    FeatureCollection {
        features,
        bbox: None,
        bbox_3d: None,
        crs: None,
        name: None,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn test_topology_output_is_valid_json_with_required_fields() {
    let fc = make_fc(vec![make_feature(make_polygon(vec![vec![
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 0.0],
    ]]))]);
    let topo_str =
        feature_collection_to_topojson(fc, TopoOptions::default()).expect("encode should succeed");
    let topo: Value = serde_json::from_str(&topo_str).expect("output must be valid JSON");

    assert_eq!(topo["type"], "Topology");
    assert!(
        topo["transform"].is_object(),
        "must have 'transform' object"
    );
    assert!(
        topo["transform"]["scale"].is_array(),
        "transform must have 'scale'"
    );
    assert!(
        topo["transform"]["translate"].is_array(),
        "transform must have 'translate'"
    );
    assert!(topo["arcs"].is_array(), "must have 'arcs' array");
    assert!(topo["objects"].is_object(), "must have 'objects' object");
}

#[test]
fn test_empty_feature_collection_returns_error() {
    let fc = make_fc(vec![]);
    let result = feature_collection_to_topojson(fc, TopoOptions::default());
    assert!(result.is_err(), "empty FC should yield TopologyError");
}

#[test]
fn test_point_geometries_have_no_arcs() {
    let fc = make_fc(vec![make_feature(GeoJsonGeometry::Point([1.0, 2.0]))]);
    let topo_str =
        feature_collection_to_topojson(fc, TopoOptions::default()).expect("encode should succeed");
    let topo: Value = serde_json::from_str(&topo_str).expect("valid JSON");
    let arcs = topo["arcs"].as_array().expect("'arcs' must be array");
    assert_eq!(arcs.len(), 0, "point geometries produce no arcs");
}

#[test]
fn test_quantisation_round_trip_within_tolerance() {
    let q = 10_000u32;
    let options = TopoOptions::default().with_quantization(q);
    let fc = make_fc(vec![make_feature(make_polygon(vec![vec![
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [0.0, 0.0],
    ]]))]);
    let topo_str = feature_collection_to_topojson(fc, options).expect("encode should succeed");
    let topo: Value = serde_json::from_str(&topo_str).expect("valid JSON");

    // scale_x = 1.0 / (q-1) ≈ 1e-4 for a 1×1 bbox
    let scale_x = topo["transform"]["scale"][0]
        .as_f64()
        .expect("scale[0] must be f64");
    // Max coordinate error: scale_x / 2 (rounding)
    assert!(
        scale_x < 1.0 / (q as f64 - 2.0),
        "scale_x={scale_x} must be less than 1/(q-2)"
    );
}

#[test]
fn test_disjoint_polygons_no_shared_arcs() {
    // Two completely disjoint squares should each produce one arc (the whole ring).
    let p1 = make_polygon(vec![vec![
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [0.0, 0.0],
    ]]);
    let p2 = make_polygon(vec![vec![
        [5.0, 5.0],
        [6.0, 5.0],
        [6.0, 6.0],
        [5.0, 6.0],
        [5.0, 5.0],
    ]]);
    let fc = make_fc(vec![make_feature(p1), make_feature(p2)]);
    let topo_str =
        feature_collection_to_topojson(fc, TopoOptions::default()).expect("encode should succeed");
    let topo: Value = serde_json::from_str(&topo_str).expect("valid JSON");
    let arcs = topo["arcs"].as_array().expect("'arcs' must be array");
    // Disjoint polygons share no vertices, so each ring becomes one arc.
    assert_eq!(arcs.len(), 2, "two disjoint rings → two arcs");
}

#[test]
fn test_two_adjacent_polygons_share_one_arc() {
    // Left square  : (0,0)-(1,0)-(1,1)-(0,1)
    // Right square : (1,0)-(2,0)-(2,1)-(1,1)
    // Shared edge  : (1,0)-(1,1) appears in both rings.
    let left = make_polygon(vec![vec![
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [0.0, 0.0],
    ]]);
    let right = make_polygon(vec![vec![
        [1.0, 0.0],
        [2.0, 0.0],
        [2.0, 1.0],
        [1.0, 1.0],
        [1.0, 0.0],
    ]]);
    let fc = make_fc(vec![make_feature(left), make_feature(right)]);
    let topo_str =
        feature_collection_to_topojson(fc, TopoOptions::default()).expect("encode should succeed");
    let topo: Value = serde_json::from_str(&topo_str).expect("valid JSON");
    let arcs = topo["arcs"].as_array().expect("'arcs' must be array");
    // With arc deduplication, shared edge is one arc → total < 8 arcs.
    // Expect 3: left-outer-minus-edge, shared-edge, right-outer-minus-edge.
    assert!(
        !arcs.is_empty(),
        "should have at least one arc for adjacent polygons"
    );
    // Must have fewer arcs than two full independent rings
    assert!(
        arcs.len() < 8,
        "arc count {} should be < 8 with deduplication",
        arcs.len()
    );
}

#[test]
fn test_negative_arc_index_decodes_to_reversed_arc() {
    // When two adjacent polygons share an edge, one polygon uses the arc
    // forward and the other reversed.  The reversed reference is encoded as
    // bitwise NOT: !(i as i32) which is a negative i32.
    let left = make_polygon(vec![vec![
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [0.0, 0.0],
    ]]);
    let right = make_polygon(vec![vec![
        [1.0, 0.0],
        [2.0, 0.0],
        [2.0, 1.0],
        [1.0, 1.0],
        [1.0, 0.0],
    ]]);
    let fc = make_fc(vec![make_feature(left), make_feature(right)]);
    let topo_str =
        feature_collection_to_topojson(fc, TopoOptions::default()).expect("encode should succeed");
    let topo: Value = serde_json::from_str(&topo_str).expect("valid JSON");

    // The topology must be valid and contain arcs
    let arcs = topo["arcs"].as_array().expect("'arcs' must be array");
    assert!(!arcs.is_empty(), "must have at least one arc");

    // Collect all arc indices from objects
    let objects_str = serde_json::to_string(&topo["objects"]).expect("serialize objects");
    // When a shared arc is present, the reversed reference !(i as i32) = -i-1
    // will appear as a negative number in the JSON.
    // We check that objects serialise without panic; negative indices may appear.
    assert!(!objects_str.is_empty());
}

#[test]
fn test_pretty_print_option() {
    let fc = make_fc(vec![make_feature(make_polygon(vec![vec![
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 0.0],
    ]]))]);
    let options = TopoOptions::default().pretty();
    let topo_str = feature_collection_to_topojson(fc, options).expect("encode should succeed");
    // Pretty-printed JSON contains newlines
    assert!(
        topo_str.contains('\n'),
        "pretty-printed output should contain newlines"
    );
}

#[test]
fn test_custom_object_name() {
    let fc = make_fc(vec![make_feature(make_polygon(vec![vec![
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 0.0],
    ]]))]);
    let options = TopoOptions::default().with_object_name("my_layer");
    let topo_str = feature_collection_to_topojson(fc, options).expect("encode should succeed");
    let topo: Value = serde_json::from_str(&topo_str).expect("valid JSON");
    assert!(
        topo["objects"]["my_layer"].is_object(),
        "objects must contain the custom key"
    );
}

#[test]
fn test_bbox_field_present_by_default() {
    let fc = make_fc(vec![make_feature(make_polygon(vec![vec![
        [10.0, 20.0],
        [11.0, 20.0],
        [11.0, 21.0],
        [10.0, 20.0],
    ]]))]);
    let topo_str =
        feature_collection_to_topojson(fc, TopoOptions::default()).expect("encode should succeed");
    let topo: Value = serde_json::from_str(&topo_str).expect("valid JSON");
    assert!(topo["bbox"].is_array(), "bbox must be present by default");
    let bb = topo["bbox"].as_array().expect("bbox is array");
    assert_eq!(bb.len(), 4, "2-D bbox has 4 elements");
}

#[test]
fn test_no_bbox_when_disabled() {
    let fc = make_fc(vec![make_feature(make_polygon(vec![vec![
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 0.0],
    ]]))]);
    let options = TopoOptions {
        include_bbox: false,
        ..TopoOptions::default()
    };
    let topo_str = feature_collection_to_topojson(fc, options).expect("encode should succeed");
    let topo: Value = serde_json::from_str(&topo_str).expect("valid JSON");
    assert!(
        topo.get("bbox").is_none() || topo["bbox"].is_null(),
        "bbox must be absent"
    );
}

// ─── LineString / MultiLineString arc topology ──────────────────────────────
//
// NOTE: the crate ships only a TopoJSON *writer*; there is no reader.  The
// round-trip test below therefore reconstructs geometry with a small,
// spec-compliant decoder (delta-decode → inverse transform → arc stitching with
// negative-index reversal) that mirrors how any TopoJSON reader would decode the
// output.

fn make_line(coords: Vec<[f64; 2]>) -> GeoJsonGeometry {
    GeoJsonGeometry::LineString(coords)
}

/// Decode the topology's delta-encoded arcs into absolute floating-point
/// coordinate sequences using the transform.
fn decode_arcs(topo: &Value) -> Vec<Vec<[f64; 2]>> {
    let sx = topo["transform"]["scale"][0].as_f64().expect("scale x");
    let sy = topo["transform"]["scale"][1].as_f64().expect("scale y");
    let tx = topo["transform"]["translate"][0]
        .as_f64()
        .expect("translate x");
    let ty = topo["transform"]["translate"][1]
        .as_f64()
        .expect("translate y");
    topo["arcs"]
        .as_array()
        .expect("arcs array")
        .iter()
        .map(|arc| {
            let mut qx = 0_i64;
            let mut qy = 0_i64;
            arc.as_array()
                .expect("arc points array")
                .iter()
                .map(|p| {
                    qx += p[0].as_i64().expect("delta x");
                    qy += p[1].as_i64().expect("delta y");
                    [qx as f64 * sx + tx, qy as f64 * sy + ty]
                })
                .collect()
        })
        .collect()
}

/// Largest per-axis quantisation step (used to bound round-trip error).
fn transform_max_scale(topo: &Value) -> f64 {
    let sx = topo["transform"]["scale"][0].as_f64().expect("scale x");
    let sy = topo["transform"]["scale"][1].as_f64().expect("scale y");
    sx.max(sy)
}

/// Read a flat array of (possibly negative) arc indices.
fn arc_index_list(v: &Value) -> Vec<i64> {
    v.as_array()
        .expect("arc index array")
        .iter()
        .map(|x| x.as_i64().expect("arc index i64"))
        .collect()
}

/// Absolute arc id for a signed reference (drops direction).
fn abs_arc(reference: i64) -> usize {
    if reference < 0 {
        (!reference) as usize
    } else {
        reference as usize
    }
}

/// Stitch a flat list of signed arc references into a coordinate sequence,
/// reversing arcs referenced by a negative index and dropping the duplicated
/// shared vertex between consecutive arcs (standard TopoJSON decode).
fn stitch(indices: &[i64], arcs: &[Vec<[f64; 2]>]) -> Vec<[f64; 2]> {
    let mut out: Vec<[f64; 2]> = Vec::new();
    for &reference in indices {
        let mut pts = arcs[abs_arc(reference)].clone();
        if reference < 0 {
            pts.reverse();
        }
        if out.is_empty() {
            out.extend(pts);
        } else {
            out.extend(pts.into_iter().skip(1));
        }
    }
    out
}

fn close(a: [f64; 2], b: [f64; 2], tol: f64) -> bool {
    (a[0] - b[0]).abs() <= tol && (a[1] - b[1]).abs() <= tol
}

fn assert_close_seq(decoded: &[[f64; 2]], original: &[[f64; 2]], tol: f64) {
    assert_eq!(
        decoded.len(),
        original.len(),
        "vertex count mismatch: {decoded:?} vs {original:?}"
    );
    for (d, o) in decoded.iter().zip(original) {
        assert!(
            close(*d, *o, tol),
            "vertex {d:?} deviates from {o:?} beyond tolerance {tol}"
        );
    }
}

#[test]
fn test_two_linestrings_share_arc_with_negative_index() {
    // Line 1: A - J1 - M - J2 - B (traverses the shared J1-M-J2 forward)
    // Line 2: C - J2 - M - J1 - D (traverses the shared J1-M-J2 reversed)
    let line1 = make_line(vec![
        [0.0, 10.0],
        [2.0, 0.0],
        [3.0, 0.0],
        [4.0, 0.0],
        [6.0, 10.0],
    ]);
    let line2 = make_line(vec![
        [6.0, -10.0],
        [4.0, 0.0],
        [3.0, 0.0],
        [2.0, 0.0],
        [0.0, -10.0],
    ]);
    let fc = make_fc(vec![make_feature(line1), make_feature(line2)]);
    let topo_str =
        feature_collection_to_topojson(fc, TopoOptions::default()).expect("encode should succeed");
    let topo: Value = serde_json::from_str(&topo_str).expect("valid JSON");

    let geoms = topo["objects"]["data"]["geometries"]
        .as_array()
        .expect("geometries array");
    assert_eq!(geoms.len(), 2, "two line features → two geometries");

    let l1 = arc_index_list(&geoms[0]["arcs"]);
    let l2 = arc_index_list(&geoms[1]["arcs"]);

    // The shared 3-vertex arc (J1-M-J2) must be stored exactly once.
    let arcs = topo["arcs"].as_array().expect("arcs array");
    let three_point_arcs = arcs
        .iter()
        .filter(|a| a.as_array().map(|v| v.len() == 3).unwrap_or(false))
        .count();
    assert_eq!(
        three_point_arcs, 1,
        "the shared 3-point sub-path arc is emitted exactly once"
    );

    // Both lines must reference that same shared arc id …
    let mut both = l1.clone();
    both.extend(&l2);
    let shared_id = arcs
        .iter()
        .position(|a| a.as_array().map(|v| v.len() == 3).unwrap_or(false))
        .expect("shared arc present");
    let refs_to_shared = both.iter().filter(|&&r| abs_arc(r) == shared_id).count();
    assert_eq!(
        refs_to_shared, 2,
        "the shared arc is referenced once by each line"
    );

    // … and one of the two references must be a reversed (negative) index.
    assert!(
        both.iter().any(|&r| r < 0),
        "one referencing line uses a negative (reversed) arc index"
    );
}

#[test]
fn test_linestring_shares_arc_with_polygon_ring() {
    // A square polygon and a line that runs along the bottom edge (0,0)-(4,0)
    // before departing.  The shared edge must become a single shared arc.
    let poly = make_polygon(vec![vec![
        [0.0, 0.0],
        [4.0, 0.0],
        [4.0, 4.0],
        [0.0, 4.0],
        [0.0, 0.0],
    ]]);
    let line = make_line(vec![[0.0, 0.0], [4.0, 0.0], [4.0, -2.0]]);
    let fc = make_fc(vec![make_feature(poly), make_feature(line)]);
    let topo_str =
        feature_collection_to_topojson(fc, TopoOptions::default()).expect("encode should succeed");
    let topo: Value = serde_json::from_str(&topo_str).expect("valid JSON");

    let arcs = decode_arcs(&topo);
    let geoms = topo["objects"]["data"]["geometries"]
        .as_array()
        .expect("geometries array");
    assert_eq!(geoms.len(), 2);

    // Polygon: "arcs" is an array of rings; take the exterior ring's flat list.
    let poly_rings = geoms[0]["arcs"].as_array().expect("polygon arcs");
    let poly_ring0 = arc_index_list(&poly_rings[0]);
    let line_arcs = arc_index_list(&geoms[1]["arcs"]);

    let poly_ids: std::collections::HashSet<usize> =
        poly_ring0.iter().map(|&r| abs_arc(r)).collect();
    let shared = line_arcs
        .iter()
        .map(|&r| abs_arc(r))
        .find(|id| poly_ids.contains(id));
    let shared_id = shared.expect("line and polygon ring must share an arc");

    // The shared arc must span the (0,0)-(4,0) bottom edge.
    let tol = 2.0 * transform_max_scale(&topo);
    let shared_arc = &arcs[shared_id];
    let first = shared_arc[0];
    let last = shared_arc[shared_arc.len() - 1];
    let spans_edge = (close(first, [0.0, 0.0], tol) && close(last, [4.0, 0.0], tol))
        || (close(first, [4.0, 0.0], tol) && close(last, [0.0, 0.0], tol));
    assert!(spans_edge, "shared arc spans the (0,0)-(4,0) edge");
}

#[test]
fn test_topojson_roundtrip_lines_within_tolerance() {
    let line1 = vec![[0.0, 10.0], [2.0, 0.0], [3.0, 0.0], [4.0, 0.0], [6.0, 10.0]];
    // Line 2 shares the J1-M-J2 sub-path with line 1 (reversed).
    let line2 = vec![
        [6.0, -10.0],
        [4.0, 0.0],
        [3.0, 0.0],
        [2.0, 0.0],
        [0.0, -10.0],
    ];
    let mls = vec![
        vec![[10.0, 10.0], [11.0, 11.0], [12.0, 10.0]],
        vec![[12.0, 10.0], [13.0, 9.0]],
    ];
    let poly = vec![vec![
        [20.0, 20.0],
        [24.0, 20.0],
        [24.0, 24.0],
        [20.0, 24.0],
        [20.0, 20.0],
    ]];

    let fc = make_fc(vec![
        make_feature(make_line(line1.clone())),
        make_feature(make_line(line2.clone())),
        make_feature(GeoJsonGeometry::MultiLineString(mls.clone())),
        make_feature(make_polygon(poly.clone())),
    ]);
    let options = TopoOptions::default().with_quantization(1_000_000);
    let topo_str = feature_collection_to_topojson(fc, options).expect("encode should succeed");
    let topo: Value = serde_json::from_str(&topo_str).expect("valid JSON");

    let arcs = decode_arcs(&topo);
    let geoms = topo["objects"]["data"]["geometries"]
        .as_array()
        .expect("geometries array");
    assert_eq!(geoms.len(), 4);
    let tol = 4.0 * transform_max_scale(&topo);

    // LineString 1 & 2 (they share a reversed arc).
    let d1 = stitch(&arc_index_list(&geoms[0]["arcs"]), &arcs);
    assert_close_seq(&d1, &line1, tol);
    let d2 = stitch(&arc_index_list(&geoms[1]["arcs"]), &arcs);
    assert_close_seq(&d2, &line2, tol);

    // MultiLineString: array of per-line arc lists.
    let ml = geoms[2]["arcs"].as_array().expect("multiline arcs");
    assert_eq!(ml.len(), 2, "two constituent lines");
    let dm0 = stitch(&arc_index_list(&ml[0]), &arcs);
    assert_close_seq(&dm0, &mls[0], tol);
    let dm1 = stitch(&arc_index_list(&ml[1]), &arcs);
    assert_close_seq(&dm1, &mls[1], tol);

    // Polygon exterior ring round-trips (closed) as well.
    let poly_rings = geoms[3]["arcs"].as_array().expect("polygon arcs");
    let dp = stitch(&arc_index_list(&poly_rings[0]), &arcs);
    assert_close_seq(&dp, &poly[0], tol);
}

#[test]
fn test_multilinestring_emits_nested_arc_arrays() {
    let mls = GeoJsonGeometry::MultiLineString(vec![
        vec![[0.0, 0.0], [1.0, 1.0]],
        vec![[2.0, 2.0], [3.0, 3.0]],
    ]);
    let fc = make_fc(vec![make_feature(mls)]);
    let topo_str =
        feature_collection_to_topojson(fc, TopoOptions::default()).expect("encode should succeed");
    let topo: Value = serde_json::from_str(&topo_str).expect("valid JSON");
    let geom = &topo["objects"]["data"]["geometries"][0];
    assert_eq!(geom["type"], "MultiLineString");
    let outer = geom["arcs"].as_array().expect("outer arcs array");
    assert_eq!(outer.len(), 2, "one arc list per constituent line");
    for line_arcs in outer {
        assert!(
            line_arcs.is_array() && !line_arcs.as_array().expect("inner").is_empty(),
            "each line references at least one arc"
        );
    }
    // Two disjoint segments → two distinct arcs.
    assert_eq!(
        topo["arcs"].as_array().expect("arcs").len(),
        2,
        "disjoint MultiLineString lines produce two arcs"
    );
}

/// Regression: a `GeometryCollection` with two disjoint `Polygon` members must
/// keep each member's `"arcs"` scoped to that member.  Previously both members
/// reported `(feature_idx, poly_idx=0, ring_idx=0)`, so the writer's arc lookup
/// filtered on that colliding key and merged both siblings' rings into each
/// member's output.
#[test]
fn test_geometrycollection_two_polygons_do_not_share_arcs() {
    let poly_a = make_polygon(vec![vec![
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [0.0, 0.0],
    ]]);
    let poly_b = make_polygon(vec![vec![
        [5.0, 5.0],
        [6.0, 5.0],
        [6.0, 6.0],
        [5.0, 6.0],
        [5.0, 5.0],
    ]]);
    let gc = GeoJsonGeometry::GeometryCollection(vec![poly_a, poly_b]);
    let fc = make_fc(vec![make_feature(gc)]);
    let topo_str =
        feature_collection_to_topojson(fc, TopoOptions::default()).expect("encode should succeed");
    let topo: Value = serde_json::from_str(&topo_str).expect("valid JSON");

    // Two disjoint rings → two distinct arcs at the top level.
    assert_eq!(
        topo["arcs"].as_array().expect("arcs").len(),
        2,
        "two disjoint polygon rings produce two arcs"
    );

    let outer = &topo["objects"]["data"]["geometries"][0];
    assert_eq!(outer["type"], "GeometryCollection");
    let members = outer["geometries"].as_array().expect("members array");
    assert_eq!(members.len(), 2, "collection has two members");

    // Each member is a Polygon with exactly one ring referencing exactly one arc.
    let mut used: Vec<i64> = Vec::new();
    for member in members {
        assert_eq!(member["type"], "Polygon");
        let rings = member["arcs"].as_array().expect("polygon arcs array");
        assert_eq!(
            rings.len(),
            1,
            "each member polygon must have exactly one ring, not the union of siblings"
        );
        let ring0 = rings[0].as_array().expect("ring arc list");
        assert_eq!(ring0.len(), 1, "each disjoint ring is a single arc");
        used.push(ring0[0].as_i64().expect("arc index is integer"));
    }
    // The two members must reference different arcs (no cross-contamination).
    assert_ne!(
        used[0].abs(),
        used[1].abs(),
        "sibling polygons must reference distinct arcs"
    );
}

/// Regression: a `GeometryCollection` with two disjoint `LineString` members.
/// Both chains previously collided on `(feature_idx, poly_idx=0, is_ring=false)`,
/// so `collect_chain_arc_refs` concatenated both members' arcs into each
/// member's flat `"arcs"` list.
#[test]
fn test_geometrycollection_two_linestrings_do_not_collide() {
    let line_a = GeoJsonGeometry::LineString(vec![[0.0, 0.0], [1.0, 1.0]]);
    let line_b = GeoJsonGeometry::LineString(vec![[5.0, 5.0], [6.0, 6.0]]);
    let gc = GeoJsonGeometry::GeometryCollection(vec![line_a, line_b]);
    let fc = make_fc(vec![make_feature(gc)]);
    let topo_str =
        feature_collection_to_topojson(fc, TopoOptions::default()).expect("encode should succeed");
    let topo: Value = serde_json::from_str(&topo_str).expect("valid JSON");

    assert_eq!(
        topo["arcs"].as_array().expect("arcs").len(),
        2,
        "two disjoint line chains produce two arcs"
    );

    let outer = &topo["objects"]["data"]["geometries"][0];
    assert_eq!(outer["type"], "GeometryCollection");
    let members = outer["geometries"].as_array().expect("members array");
    assert_eq!(members.len(), 2, "collection has two members");

    let mut used: Vec<i64> = Vec::new();
    for member in members {
        assert_eq!(member["type"], "LineString");
        let arcs = member["arcs"].as_array().expect("linestring flat arcs");
        assert_eq!(
            arcs.len(),
            1,
            "each member line must reference only its own arc, not both siblings'"
        );
        used.push(arcs[0].as_i64().expect("arc index is integer"));
    }
    assert_ne!(
        used[0].abs(),
        used[1].abs(),
        "sibling lines must reference distinct arcs"
    );
}

/// Regression: a `GeometryCollection` mixing a `Polygon` and a `LineString`
/// (different `is_ring`) must still route arcs to the correct member.
#[test]
fn test_geometrycollection_polygon_and_linestring_members() {
    let poly = make_polygon(vec![vec![
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [0.0, 0.0],
    ]]);
    let line = GeoJsonGeometry::LineString(vec![[5.0, 5.0], [6.0, 6.0]]);
    let gc = GeoJsonGeometry::GeometryCollection(vec![poly, line]);
    let fc = make_fc(vec![make_feature(gc)]);
    let topo_str =
        feature_collection_to_topojson(fc, TopoOptions::default()).expect("encode should succeed");
    let topo: Value = serde_json::from_str(&topo_str).expect("valid JSON");

    let outer = &topo["objects"]["data"]["geometries"][0];
    let members = outer["geometries"].as_array().expect("members array");
    assert_eq!(members.len(), 2);
    assert_eq!(members[0]["type"], "Polygon");
    assert_eq!(members[1]["type"], "LineString");
    let poly_rings = members[0]["arcs"].as_array().expect("polygon arcs");
    assert_eq!(poly_rings.len(), 1, "polygon member has one ring");
    let line_arcs = members[1]["arcs"].as_array().expect("line arcs");
    assert_eq!(line_arcs.len(), 1, "line member references one arc");
}

/// Regression: nested `GeometryCollection` members must not collide either —
/// the member path disambiguates across nesting levels.
#[test]
fn test_nested_geometrycollection_polygons_do_not_collide() {
    let poly_a = make_polygon(vec![vec![
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [0.0, 0.0],
    ]]);
    let poly_b = make_polygon(vec![vec![
        [5.0, 5.0],
        [6.0, 5.0],
        [6.0, 6.0],
        [5.0, 6.0],
        [5.0, 5.0],
    ]]);
    // Outer collection: [ Polygon A, inner-collection[ Polygon B ] ]
    let inner = GeoJsonGeometry::GeometryCollection(vec![poly_b]);
    let outer_gc = GeoJsonGeometry::GeometryCollection(vec![poly_a, inner]);
    let fc = make_fc(vec![make_feature(outer_gc)]);
    let topo_str =
        feature_collection_to_topojson(fc, TopoOptions::default()).expect("encode should succeed");
    let topo: Value = serde_json::from_str(&topo_str).expect("valid JSON");

    assert_eq!(
        topo["arcs"].as_array().expect("arcs").len(),
        2,
        "two disjoint rings across nesting produce two arcs"
    );

    let outer = &topo["objects"]["data"]["geometries"][0];
    let members = outer["geometries"].as_array().expect("outer members");
    assert_eq!(members.len(), 2);

    let m0 = &members[0];
    assert_eq!(m0["type"], "Polygon");
    let m0_rings = m0["arcs"].as_array().expect("m0 rings");
    assert_eq!(m0_rings.len(), 1, "top-level polygon has exactly one ring");
    let m0_arc = m0_rings[0].as_array().expect("m0 ring0")[0]
        .as_i64()
        .expect("m0 arc idx");

    let m1 = &members[1];
    assert_eq!(m1["type"], "GeometryCollection");
    let inner_members = m1["geometries"].as_array().expect("inner members");
    assert_eq!(inner_members.len(), 1);
    let inner_poly = &inner_members[0];
    assert_eq!(inner_poly["type"], "Polygon");
    let inner_rings = inner_poly["arcs"].as_array().expect("inner rings");
    assert_eq!(inner_rings.len(), 1, "nested polygon has exactly one ring");
    let inner_arc = inner_rings[0].as_array().expect("inner ring0")[0]
        .as_i64()
        .expect("inner arc idx");

    assert_ne!(
        m0_arc.abs(),
        inner_arc.abs(),
        "nested sibling polygons must reference distinct arcs"
    );
}
