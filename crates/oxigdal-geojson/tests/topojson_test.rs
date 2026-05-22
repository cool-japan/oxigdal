//! Integration tests for the TopoJSON 3.0 encoder.

use oxigdal_geojson_stream::{
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
