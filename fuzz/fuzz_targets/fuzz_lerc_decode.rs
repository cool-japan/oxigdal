//! Fuzz target: LERC codec decoder.
//!
//! Tests that LercCodec::decode never panics on arbitrary input.
//! Also exercises parse_header and is_lerc helpers.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Full decode attempt
    let _ = oxigdal_geotiff::lerc_codec::LercCodec::decode(data);
    // Header parse attempt
    let _ = oxigdal_geotiff::lerc_codec::LercCodec::parse_header(data);
    // Magic bytes check (must always return without panic)
    let _ = oxigdal_geotiff::lerc_codec::LercCodec::is_lerc(data);
});
