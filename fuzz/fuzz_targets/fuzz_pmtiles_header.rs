//! Fuzz target: PMTiles v2/v3 binary header and directory parsers.
//!
//! Tests that `PmTilesHeader::parse` (v3 127-byte header), `parse_v2_header`
//! / `read_v2_entry` (legacy v2 header + fixed-size entries), and
//! `decode_directory` (v3 varint-encoded directory, root or leaf) never
//! panic on arbitrary input. Any `Err` is acceptable; panics and
//! out-of-bounds reads are not.
#![no_main]
use libfuzzer_sys::fuzz_target;
use oxigdal_pmtiles::PmTilesHeader;
use oxigdal_pmtiles::directory::decode_directory;
use oxigdal_pmtiles::header::PMTILES_HEADER_SIZE;
use oxigdal_pmtiles::header_v2::{parse_v2_header, read_v2_entry};

fuzz_target!(|data: &[u8]| {
    // v3 fixed 127-byte header.
    if let Ok(header) = PmTilesHeader::parse(data) {
        // Header parsed successfully; try decoding whatever follows it as
        // a v3 directory (root directory immediately follows the header
        // on disk in the common case).
        if data.len() > PMTILES_HEADER_SIZE {
            let _ = decode_directory(&data[PMTILES_HEADER_SIZE..]);
        }
        let _ = header.tile_type;
    }

    // v3 directory decoder, run directly on the raw input too (covers
    // leaf directories, which live at arbitrary offsets, not just right
    // after the header).
    let _ = decode_directory(data);

    // Legacy v2 fixed header + root directory entries.
    let _ = parse_v2_header(data);

    // Raw fixed-size (17-byte) v2 directory entry decode, tried at a few
    // offsets so short/misaligned inputs are exercised too.
    for offset in [0usize, 2, 17] {
        let _ = read_v2_entry(data, offset);
    }
});
