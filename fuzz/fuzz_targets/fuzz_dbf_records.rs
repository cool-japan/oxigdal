//! Fuzz target: DBF (`.dbf`) full record decoding.
//!
//! `fuzz_shapefile_header` already covers `DbfHeader::read` (the fixed
//! 32-byte header) in isolation. This target goes one layer deeper: it
//! drives `DbfReader::new` (header + field-descriptor-array parsing, with
//! its own `header_size`/field-count sanity checks) and then walks
//! `read_record` across the field descriptors it recovered, decoding each
//! field value (numeric/date/logical/character/memo) via
//! `FieldValue::parse_with_encoding`. Any `Err` is acceptable; panics,
//! out-of-bounds reads, and unbounded allocations driven by attacker fields
//! (record length, field count, decimal counts) are not.
#![no_main]
use libfuzzer_sys::fuzz_target;
use oxigeo_shapefile::dbf::DbfReader;
use std::io::Cursor;

fuzz_target!(|data: &[u8]| {
    let cursor = Cursor::new(data);
    if let Ok(mut reader) = DbfReader::new(cursor) {
        // Bounded: a crafted `record_count` in the header must not turn one
        // fuzz input into an unbounded read loop.
        for _ in 0..256 {
            match reader.read_record() {
                Ok(Some(_record)) => continue,
                Ok(None) | Err(_) => break,
            }
        }
    }
});
