//! Fuzz target: GeoJSON document parser (RFC 7946).
//!
//! `GeoJsonParser::parse` runs `serde_json` deserialization followed by
//! oxigeo's own structural/coordinate validation (ring closure, winding,
//! bbox computation, CRS extension parsing, geometry-type dispatch). Any
//! `Err` is acceptable; panics and out-of-bounds reads are not.
#![no_main]
use libfuzzer_sys::fuzz_target;
use oxigeo_geojson_stream::GeoJsonParser;

fuzz_target!(|data: &[u8]| {
    let lenient = GeoJsonParser::new();
    if let Ok(doc) = lenient.parse(data) {
        let _ = doc.document_type();
        if let Some(collection) = doc.as_feature_collection() {
            let _ = collection.len();
            let _ = collection.compute_bbox();
            let _ = collection.compute_bbox_3d();
            let _ = collection.geometry_types();
        }
    }

    // Strict mode exercises additional validation branches (coordinate
    // precision clamping, stricter ring/CRS checks) over the same bytes.
    let strict = GeoJsonParser::new().strict();
    let _ = strict.parse(data);

    // Header-only parse (feature-collection framing without decoding every
    // feature) is a distinct, cheaper code path worth covering directly.
    let _ = lenient.parse_header(data);
});
