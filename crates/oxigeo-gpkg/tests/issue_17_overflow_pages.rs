//! Regression tests for cool-japan/oxigeo#17 — table B-tree cells whose payload
//! spills onto overflow pages.
//!
//! The reporter created a GeoPackage layer in QGIS with a ~5000-character table
//! name, which makes the matching `sqlite_master.sql` row wider than one
//! 4096-byte page. `GeoPackage::from_bytes(..)` + `load_contents()` then failed
//! with
//!
//! ```text
//! Invalid format: Cell 0: overflow cell needs 4061 bytes inline + 4-byte
//! pointer, but only 3209 available from cell start
//! ```
//!
//! Two defects combined to produce that. The local (on-page) payload size was
//! computed as `min(P, U - 35)`, but SQLite only stores `U - 35` bytes locally
//! when the payload fits entirely — an overflowing cell stores `K` or `M`
//! instead, which is 489 bytes at a 4096-byte page size, not 4061. And even
//! with the right size the overflow chain was never followed, so the row could
//! not have been reassembled anyway.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use oxigeo_gpkg::{CellValue, scan_sqlite_master, scan_table};

const PAGE_SIZE: usize = 4096;

/// Local payload size for a table-leaf cell, per the SQLite file format spec.
/// Duplicated here on purpose: the fixtures must be built to the *spec*, not to
/// whatever the parser currently believes, or the test would validate itself.
fn local_payload(payload_len: usize, usable: usize) -> usize {
    let max_local = usable - 35;
    if payload_len <= max_local {
        return payload_len;
    }
    let min_local = ((usable - 12) * 32 / 255) - 23;
    let k = min_local + ((payload_len - min_local) % (usable - 4));
    if k <= max_local { k } else { min_local }
}

fn encode_varint(mut value: u64) -> Vec<u8> {
    if value == 0 {
        return vec![0];
    }
    let mut groups = Vec::new();
    while value > 0 {
        groups.push((value & 0x7F) as u8);
        value >>= 7;
    }
    groups.reverse();
    let last = groups.len() - 1;
    for (i, byte) in groups.iter_mut().enumerate() {
        if i != last {
            *byte |= 0x80;
        }
    }
    groups
}

/// A column value in a SQLite record.
enum Col<'a> {
    Text(&'a str),
    Int(i64),
}

/// Encodes a SQLite record: header (its own length, then one serial type per
/// column) followed by the concatenated column bodies.
fn encode_record(cols: &[Col<'_>]) -> Vec<u8> {
    let mut serial_types = Vec::new();
    let mut body = Vec::new();

    for col in cols {
        match col {
            Col::Text(s) => {
                serial_types.push(encode_varint(2 * s.len() as u64 + 13));
                body.extend_from_slice(s.as_bytes());
            }
            Col::Int(v) => {
                // Serial type 6 is an 8-byte big-endian signed integer, valid
                // for every i64 so the fixture needs no width analysis.
                serial_types.push(encode_varint(6));
                body.extend_from_slice(&v.to_be_bytes());
            }
        }
    }

    let types_len: usize = serial_types.iter().map(Vec::len).sum();
    // The header length varint counts itself; 1 byte suffices well past any
    // column count these fixtures use.
    let header_len = types_len + 1;
    let mut record = encode_varint(header_len as u64);
    assert_eq!(record.len(), 1, "fixture assumes a 1-byte header length");
    for st in &serial_types {
        record.extend_from_slice(st);
    }
    record.extend_from_slice(&body);
    record
}

/// Builds a complete single-table SQLite image: page 1 holds `sqlite_master`
/// with one row, whose payload spills onto overflow pages 2..N.
///
/// `reserved_bytes` exercises the usable-size path (byte 20 of the file header).
fn build_file_with_overflow_row(cols: &[Col<'_>], reserved_bytes: u8) -> Vec<u8> {
    let usable = PAGE_SIZE - reserved_bytes as usize;
    let payload = encode_record(cols);
    let inline_len = local_payload(payload.len(), usable);
    let spilled = &payload[inline_len..];

    let per_page = usable - 4;
    let overflow_page_count = spilled.len().div_ceil(per_page);

    // Page 1 (sqlite_master) + the overflow chain.
    let total_pages = 1 + overflow_page_count;
    let mut file = vec![0u8; PAGE_SIZE * total_pages];

    // ── SQLite file header ────────────────────────────────────────────────
    file[..16].copy_from_slice(b"SQLite format 3\x00");
    file[16..18].copy_from_slice(&(PAGE_SIZE as u16).to_be_bytes());
    file[18] = 1; // write version
    file[19] = 1; // read version
    file[20] = reserved_bytes;
    file[21] = 64; // max embedded payload fraction
    file[22] = 32; // min embedded payload fraction
    file[23] = 32; // leaf payload fraction
    file[28..32].copy_from_slice(&(total_pages as u32).to_be_bytes());
    file[44..48].copy_from_slice(&4u32.to_be_bytes()); // schema format
    file[56..60].copy_from_slice(&1u32.to_be_bytes()); // UTF-8
    file[68..72].copy_from_slice(&0x4750_4B47u32.to_be_bytes()); // "GPKG"

    // ── Cell: payload_len varint, rowid varint, inline payload, chain head ──
    let mut cell = encode_varint(payload.len() as u64);
    cell.extend_from_slice(&encode_varint(1));
    cell.extend_from_slice(&payload[..inline_len]);
    let first_overflow: u32 = if spilled.is_empty() { 0 } else { 2 };
    cell.extend_from_slice(&first_overflow.to_be_bytes());

    // ── Page 1 leaf header (after the 100-byte file header) ────────────────
    let hdr = 100;
    let cell_offset = usable - cell.len();
    file[cell_offset..cell_offset + cell.len()].copy_from_slice(&cell);

    file[hdr] = 13; // leaf table page
    file[hdr + 1..hdr + 3].copy_from_slice(&0u16.to_be_bytes()); // first freeblock
    file[hdr + 3..hdr + 5].copy_from_slice(&1u16.to_be_bytes()); // cell count
    file[hdr + 5..hdr + 7].copy_from_slice(&(cell_offset as u16).to_be_bytes());
    file[hdr + 7] = 0; // fragmented free bytes
    file[hdr + 8..hdr + 10].copy_from_slice(&(cell_offset as u16).to_be_bytes());

    // ── Overflow chain ─────────────────────────────────────────────────────
    for (i, chunk) in spilled.chunks(per_page).enumerate() {
        let page_start = (i + 1) * PAGE_SIZE;
        let next: u32 = if i + 1 < overflow_page_count {
            (i + 3) as u32
        } else {
            0
        };
        file[page_start..page_start + 4].copy_from_slice(&next.to_be_bytes());
        file[page_start + 4..page_start + 4 + chunk.len()].copy_from_slice(chunk);
    }

    file
}

/// The reporter's exact shape: a `sqlite_master` row whose `sql` column pushes
/// the payload past one page.
#[test]
fn test_issue_17_sqlite_master_row_spanning_overflow_pages() {
    let long_name = "t".repeat(5000);
    let sql = format!("CREATE TABLE \"{long_name}\" (fid INTEGER PRIMARY KEY, geom BLOB)");

    let file = build_file_with_overflow_row(
        &[
            Col::Text("table"),
            Col::Text(&long_name),
            Col::Text(&long_name),
            Col::Int(2),
            Col::Text(&sql),
        ],
        0,
    );

    let entries = scan_sqlite_master(&file, PAGE_SIZE)
        .expect("a sqlite_master row wider than a page must parse (cool-japan/oxigeo#17)");

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].entry_type, "table");
    assert_eq!(entries[0].name, long_name, "the name must not be truncated");
    assert_eq!(entries[0].sql, sql, "the SQL must be reassembled in full");
}

/// The spilled bytes must be stitched back in the right order across *several*
/// overflow pages, not just one.
#[test]
fn test_issue_17_multi_page_overflow_chain_round_trips() {
    // ~3 pages of payload: forces a chain of more than one overflow page.
    let blob_text: String = (0..12_000)
        .map(|i| char::from(b'a' + (i % 26) as u8))
        .collect();

    let file = build_file_with_overflow_row(
        &[
            Col::Text("table"),
            Col::Text("wide"),
            Col::Text("wide"),
            Col::Int(2),
            Col::Text(&blob_text),
        ],
        0,
    );

    let rows = scan_table(&file, 1, PAGE_SIZE).expect("scan");
    assert_eq!(rows.len(), 1);
    match &rows[0].1[4] {
        CellValue::Text(s) => {
            assert_eq!(s.len(), blob_text.len(), "no bytes lost across the chain");
            assert_eq!(s, &blob_text, "chunks must be stitched in chain order");
        }
        other => panic!("expected text column, got {other:?}"),
    }
}

/// A payload that fits entirely on its page must be unaffected by the change.
#[test]
fn test_issue_17_non_overflowing_row_unchanged() {
    let file = build_file_with_overflow_row(
        &[
            Col::Text("table"),
            Col::Text("small"),
            Col::Text("small"),
            Col::Int(2),
            Col::Text("CREATE TABLE small (fid INTEGER PRIMARY KEY)"),
        ],
        0,
    );

    let entries = scan_sqlite_master(&file, PAGE_SIZE).expect("scan");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "small");
    assert_eq!(entries[0].rootpage, 2);
}

/// Files that reserve trailing bytes per page shift every spill boundary, since
/// the SQLite spill arithmetic is defined on the *usable* size. Reading byte 20
/// of the file header is what keeps `M`/`K`/`X` correct for those files.
#[test]
fn test_issue_17_overflow_respects_reserved_bytes() {
    let long_name = "r".repeat(5000);
    let sql = format!("CREATE TABLE \"{long_name}\" (fid INTEGER PRIMARY KEY)");

    let file = build_file_with_overflow_row(
        &[
            Col::Text("table"),
            Col::Text(&long_name),
            Col::Text(&long_name),
            Col::Int(2),
            Col::Text(&sql),
        ],
        32,
    );

    let entries = scan_sqlite_master(&file, PAGE_SIZE).expect("reserved-bytes file must parse");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, long_name);
    assert_eq!(entries[0].sql, sql);
}

/// `GeoPackage::from_bytes` accepts caller-supplied data, so a corrupt or
/// hostile overflow chain must terminate rather than spin forever.
#[test]
fn test_issue_17_cyclic_overflow_chain_is_rejected() {
    let long_name = "c".repeat(5000);
    let mut file = build_file_with_overflow_row(
        &[
            Col::Text("table"),
            Col::Text(&long_name),
            Col::Text(&long_name),
            Col::Int(2),
            Col::Text("CREATE TABLE c (fid INTEGER PRIMARY KEY)"),
        ],
        0,
    );

    // Point the first overflow page back at itself.
    file[PAGE_SIZE..PAGE_SIZE + 4].copy_from_slice(&2u32.to_be_bytes());

    let err = scan_sqlite_master(&file, PAGE_SIZE)
        .expect_err("a cyclic overflow chain must be an error, not a hang");
    assert!(
        err.to_string().contains("cycle"),
        "error should name the cycle, got: {err}"
    );
}

/// A chain that ends before delivering the promised bytes must be an error, not
/// a silently truncated string.
#[test]
fn test_issue_17_truncated_overflow_chain_is_rejected() {
    let long_text = "d".repeat(12_000);
    let mut file = build_file_with_overflow_row(
        &[
            Col::Text("table"),
            Col::Text("t"),
            Col::Text("t"),
            Col::Int(2),
            Col::Text(&long_text),
        ],
        0,
    );

    // Terminate the chain at the first overflow page.
    file[PAGE_SIZE..PAGE_SIZE + 4].copy_from_slice(&0u32.to_be_bytes());

    assert!(
        scan_sqlite_master(&file, PAGE_SIZE).is_err(),
        "a short overflow chain must not yield a truncated value"
    );
}
