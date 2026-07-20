//! Fuzz target: Shapefile (.shp/.dbf) header and shape record parsers.
//!
//! Tests that `ShapefileHeader::read`, `DbfHeader::read`, and `Shape::read`
//! never panic on arbitrary input. Any `Err` is acceptable; panics,
//! out-of-bounds reads, and other UB are not.
#![no_main]
use libfuzzer_sys::fuzz_target;
use oxigdal_shapefile::dbf::DbfHeader;
use oxigdal_shapefile::shp::ShapefileHeader;
use oxigdal_shapefile::shp::header::HEADER_SIZE;
use std::io::Cursor;

fuzz_target!(|data: &[u8]| {
    // Main .shp 100-byte header parser.
    let mut cursor = Cursor::new(data);
    let _ = ShapefileHeader::read(&mut cursor);

    // .dbf header parser (different binary layout, same input bytes).
    let mut cursor = Cursor::new(data);
    let _ = DbfHeader::read(&mut cursor);

    // Once a valid-looking .shp header is parsed, keep decoding shape
    // records from whatever bytes follow it - exercises the per-record
    // geometry decoder on attacker-controlled trailing bytes.
    if data.len() > HEADER_SIZE {
        let mut cursor = Cursor::new(&data[HEADER_SIZE..]);
        for _ in 0..64 {
            match oxigdal_shapefile::shp::Shape::read(&mut cursor) {
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    }
});
