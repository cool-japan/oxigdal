//! SQLite 100-byte file header encoder.
//!
//! Reproduces every field at the exact offsets specified in
//! <https://www.sqlite.org/fileformat.html#the_database_header> and verified
//! against the offsets read by [`crate::sqlite_reader`].

use super::{APP_ID_GPKG, PAGE_SIZE, SQLITE_MAGIC, USER_VERSION_130};

/// Write the 100-byte SQLite file header into `out`.
///
/// # Arguments
/// * `out` — target 100-byte buffer (mutated in place)
/// * `page_size` — database page size in bytes (must be a power of two
///   between 512 and 32768, or exactly 65536)
/// * `page_count` — total number of pages in the database
/// * `app_id` — application ID written at offset 68 (use [`APP_ID_GPKG`])
/// * `user_version` — user version at offset 60 (use [`USER_VERSION_130`])
pub fn write_file_header(
    out: &mut [u8; 100],
    page_size: u32,
    page_count: u32,
    app_id: u32,
    user_version: u32,
) {
    // ── Magic string: bytes 0-15 ─────────────────────────────────────────────
    out[0..16].copy_from_slice(SQLITE_MAGIC);

    // ── Page size: bytes 16-17, big-endian u16 ───────────────────────────────
    // Special rule: the value 1 represents 65536 bytes.
    let page_size_raw: u16 = if page_size == 65536 {
        1
    } else {
        page_size as u16
    };
    let ps_bytes = page_size_raw.to_be_bytes();
    out[16] = ps_bytes[0];
    out[17] = ps_bytes[1];

    // ── File format write version: byte 18 (1 = legacy, 2 = WAL) ────────────
    out[18] = 1;
    // ── File format read version: byte 19 ───────────────────────────────────
    out[19] = 1;

    // ── Reserved space per page: byte 20 (0 = none) ─────────────────────────
    out[20] = 0;

    // ── Embedded payload fraction limits: bytes 21-23 ────────────────────────
    // Per spec: max=64, min=32, leaf=32.
    out[21] = 64; // max embedded payload fraction
    out[22] = 32; // min embedded payload fraction
    out[23] = 32; // leaf payload fraction

    // ── File change counter: bytes 24-27, big-endian u32 ────────────────────
    out[24..28].copy_from_slice(&1u32.to_be_bytes());

    // ── Database size in pages: bytes 28-31, big-endian u32 ─────────────────
    out[28..32].copy_from_slice(&page_count.to_be_bytes());

    // ── First trunk freelist page: bytes 32-35 (0 = empty freelist) ─────────
    out[32..36].copy_from_slice(&0u32.to_be_bytes());

    // ── Total freelist pages: bytes 36-39 ───────────────────────────────────
    out[36..40].copy_from_slice(&0u32.to_be_bytes());

    // ── Schema cookie: bytes 40-43 (1 = one schema change) ──────────────────
    out[40..44].copy_from_slice(&1u32.to_be_bytes());

    // ── Schema format number: bytes 44-47 (4 = most capable) ────────────────
    out[44..48].copy_from_slice(&4u32.to_be_bytes());

    // ── Default page cache size: bytes 48-51 (0 = use SQLite default) ────────
    out[48..52].copy_from_slice(&0i32.to_be_bytes());

    // ── Largest B-tree root page in auto-vacuum: bytes 52-55 (0 = disabled) ──
    out[52..56].copy_from_slice(&0u32.to_be_bytes());

    // ── Text encoding: bytes 56-59 (1 = UTF-8) ───────────────────────────────
    out[56..60].copy_from_slice(&1u32.to_be_bytes());

    // ── User version: bytes 60-63 ────────────────────────────────────────────
    out[60..64].copy_from_slice(&user_version.to_be_bytes());

    // ── Incremental vacuum mode: bytes 64-67 (0 = disabled) ─────────────────
    out[64..68].copy_from_slice(&0u32.to_be_bytes());

    // ── Application ID: bytes 68-71 ──────────────────────────────────────────
    out[68..72].copy_from_slice(&app_id.to_be_bytes());

    // ── Reserved for expansion: bytes 72-91 (zeroed) ─────────────────────────
    for b in &mut out[72..92] {
        *b = 0;
    }

    // ── Version-valid-for number: bytes 92-95 ────────────────────────────────
    // Set to the file change counter value (1).
    out[92..96].copy_from_slice(&1u32.to_be_bytes());

    // ── SQLite version number: bytes 96-99 ───────────────────────────────────
    // 3.40.1 = 3 * 1_000_000 + 40 * 1_000 + 1 = 3_040_001
    out[96..100].copy_from_slice(&3_040_001u32.to_be_bytes());
}

/// Create a freshly zeroed 100-byte header buffer and populate it with the
/// GeoPackage-specific values.
///
/// Convenience wrapper around [`write_file_header`] using [`APP_ID_GPKG`] and
/// [`USER_VERSION_130`].
pub fn make_gpkg_header(page_count: u32) -> [u8; 100] {
    let mut buf = [0u8; 100];
    write_file_header(
        &mut buf,
        PAGE_SIZE as u32,
        page_count,
        APP_ID_GPKG,
        USER_VERSION_130,
    );
    buf
}
