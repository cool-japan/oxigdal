//! Fuzz target: HDF5 superblock (v0/v1/v2/v3) and object-header parsing.
//!
//! Exercises two independent parsers over the same attacker-controlled
//! bytes:
//!
//! 1. `oxih5::File::open_from_bytes` - the real Pure-Rust HDF5 engine that
//!    backs `oxigeo-hdf5`. Its docs explicitly call this constructor out as
//!    intended "for testing and fuzzing": it parses the superblock (all four
//!    versions), walks the root group's object header, and (for anything
//!    that looks like a dataset) resolves its datatype/dataspace/layout
//!    messages and chunk index. Any `Err` is acceptable; panics and
//!    out-of-bounds reads are not.
//! 2. `oxigeo_hdf5::superblock_v2::read_superblock_v2` /
//!    `validate_superblock_checksum` - the hand-rolled V2/V3 superblock
//!    reader used by `oxigeo-hdf5`'s own (legacy) reader path, run directly
//!    against a `Cursor` over the raw bytes (skipping the 9-byte
//!    signature+version prefix `read_superblock_v2` expects the caller to
//!    have already consumed).
#![no_main]
use libfuzzer_sys::fuzz_target;
use oxigeo_hdf5::superblock_v2::{read_superblock_v2, validate_superblock_checksum};
use std::io::Cursor;

fuzz_target!(|data: &[u8]| {
    // Full real-engine parse: superblock -> root group -> recursive walk ->
    // per-entry group/dataset resolution, bounded to a modest number of
    // entries so a pathological tree can't turn one input into unbounded
    // work.
    if let Ok(h5) = oxih5::File::open_from_bytes(data) {
        let mut visited = 0usize;
        let _ = h5.walk(&mut |path, is_group| {
            if visited >= 64 {
                return;
            }
            visited += 1;
            if is_group {
                let _ = h5.group(path);
            } else {
                let _ = h5.dataset(path);
            }
        });
    }

    // Direct V2/V3 superblock reader, run past a synthetic 9-byte
    // signature+version prefix (the real 8-byte HDF5 magic plus a version
    // byte of 2) so the fixed-offset field reads inside `read_superblock_v2`
    // land on attacker-controlled bytes from `data` itself.
    if data.len() > 9 {
        let mut header_so_far: Vec<u8> = Vec::with_capacity(16);
        header_so_far.extend_from_slice(b"\x89HDF\r\n\x1a\n");
        header_so_far.push(2);
        let mut cursor = Cursor::new(&data[9..]);
        if let Ok(_v2) = read_superblock_v2(&mut cursor, &mut header_so_far) {
            let _ = validate_superblock_checksum(&header_so_far);
        }
    }
});
