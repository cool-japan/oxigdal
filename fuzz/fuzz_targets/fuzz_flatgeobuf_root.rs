//! Fuzz target: FlatGeobuf magic bytes, FlatBuffers header, and spatial
//! index root parsing.
//!
//! Tests that `FlatGeobufReader::new` (magic check -> header ->
//! optional packed R-tree index) and the standalone `Header::read` never
//! panic on arbitrary input. Any `Err` is acceptable; panics and
//! out-of-bounds reads are not.
#![no_main]
use libfuzzer_sys::fuzz_target;
use oxigdal_flatgeobuf::Header;
use oxigdal_flatgeobuf::reader::FlatGeobufReader;
use std::io::Cursor;

fuzz_target!(|data: &[u8]| {
    // Standalone header decoder (operates past the 8-byte magic + 4-byte
    // size prefix that FlatGeobufReader::new strips off).
    let mut cursor = Cursor::new(data);
    let _ = Header::read(&mut cursor);

    // Full reader construction: magic bytes -> header size -> header ->
    // optional packed R-tree spatial index.
    let cursor = Cursor::new(data);
    let _ = FlatGeobufReader::new(cursor);
});
