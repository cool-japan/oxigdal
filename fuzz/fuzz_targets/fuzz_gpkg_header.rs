//! Fuzz target: hand-rolled Pure-Rust SQLite file-format reader that backs
//! `GeoPackage::from_bytes` (the `.gpkg` container format).
//!
//! GeoPackage does not use `oxisql`/SQL parsing for this path - the crate
//! implements its own 100-byte SQLite header parser and B-tree page walker
//! (see `sqlite_reader.rs` / `btree.rs`), which is exactly the kind of
//! from-scratch binary parser that benefits from fuzzing. Any `Err` is
//! acceptable; panics and out-of-bounds reads are not.
#![no_main]
use libfuzzer_sys::fuzz_target;
use oxigeo_gpkg::GeoPackage;
use oxigeo_gpkg::scan_sqlite_master;

fuzz_target!(|data: &[u8]| {
    if let Ok(gpkg) = GeoPackage::from_bytes(data.to_vec()) {
        // Walk the sqlite_master B-tree (page 1) - this is where a
        // malformed page-size/page-count/cell-pointer combination could
        // drive an out-of-bounds slice index in the hand-rolled reader.
        if let Ok(entries) = scan_sqlite_master(gpkg.reader.raw_data(), gpkg.page_size() as usize)
        {
            // Follow each table's root page too, bounding the work so a
            // pathological page-count doesn't turn one fuzz input into an
            // unbounded scan.
            for entry in entries.into_iter().take(16) {
                let _ = gpkg.scan_table(entry.rootpage);
            }
        }
    }
});
