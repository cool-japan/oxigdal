//! Fuzz target: JP2 box structure parser.
//!
//! Tests that Jp2Parser::parse never panics on arbitrary input.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = oxigdal_jpeg2000::Jp2Parser::parse(data);
});
