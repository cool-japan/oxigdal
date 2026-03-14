//! Fuzz target: WKB geometry decoder.
//!
//! Tests that WKB parsing functions never panic on arbitrary input.
//! Exercises both the bbox helper and the full geometry reader.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // wkb_bbox: returns Option, must never panic
    let _ = oxigdal_geoparquet::geometry::wkb_extended::wkb_bbox(data);

    // Full WKB geometry decode: returns Result, must never panic
    let mut reader = oxigdal_geoparquet::geometry::WkbReader::new(data);
    let _ = reader.read_geometry();
});
