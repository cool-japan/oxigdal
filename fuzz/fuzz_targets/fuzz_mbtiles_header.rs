//! Fuzz target: `.mbtiles` archive ingestion boundary.
//!
//! MBTiles has no hand-rolled binary header of its own - the container
//! format *is* SQLite, and `oxigdal-mbtiles` delegates all page/record
//! parsing to the Pure-Rust `oxisql-sqlite-compat` engine (see
//! `reader.rs`). What this crate DOES own is the untrusted-bytes ingestion
//! path: `MBTilesReader::open_in_memory` spills attacker-controlled bytes
//! to a temp file and opens them as a full SQLite database image, then
//! walks the `metadata`/`tiles` tables and decodes tile blobs. Fuzzing this
//! boundary exercises that whole chain without hand-writing SQL.
//!
//! Any `Err` is acceptable; panics are not.
#![no_main]
use libfuzzer_sys::fuzz_target;
use oxigdal_mbtiles::MBTilesReader;

fuzz_target!(|data: &[u8]| {
    if let Ok(reader) = MBTilesReader::open_in_memory(data) {
        let _ = reader.metadata();
        if let Ok(tiles) = reader.list_tiles() {
            for coord in tiles.into_iter().take(16) {
                let _ = reader.get_tile(&coord);
            }
        }
    }
});
