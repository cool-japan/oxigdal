//! Fuzz target: TIFF header parser.
//!
//! Tests that TiffHeader::parse never panics on arbitrary input.
//! Any error is acceptable; panics are not.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // TiffHeader::parse is re-exported via oxigeo_geotiff::TiffHeader
    let _ = oxigeo_geotiff::TiffHeader::parse(data);
});
