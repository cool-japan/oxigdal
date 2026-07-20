//! Fuzz target: FlatGeobuf magic bytes, FlatBuffers header, and spatial
//! index root parsing.
//!
//! Tests that `FlatGeobufReader::new` (magic check -> header ->
//! optional packed R-tree index) and the standalone `Header::from_bytes`
//! never panic on arbitrary input. Any `Err` is acceptable; panics and
//! out-of-bounds reads are not.
#![no_main]
use libfuzzer_sys::fuzz_target;
use oxigeo_flatgeobuf::Header;
use oxigeo_flatgeobuf::reader::FlatGeobufReader;
use std::io::Cursor;

fuzz_target!(|data: &[u8]| {
    // Standalone header decoder (operates on the bare FlatBuffers message
    // past the 8-byte magic + 4-byte size prefix that FlatGeobufReader::new
    // strips off).
    let _ = Header::from_bytes(data);

    // Full reader construction: magic bytes -> header size -> header ->
    // optional packed R-tree spatial index.
    let cursor = Cursor::new(data);
    let _ = FlatGeobufReader::new(cursor);
});
