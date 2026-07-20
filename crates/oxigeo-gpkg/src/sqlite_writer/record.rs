//! SQLite record encoder (inverse of the decoder in `btree.rs`).
//!
//! A SQLite record (cell payload) has the layout:
//!
//! ```text
//! header_length  : varint   — total byte-length of the header section
//!                             (includes the header_length varint itself)
//! serial_type_N  : varint   — one per column
//! data_N         : bytes    — concatenated column data
//! ```
//!
//! Reference: <https://www.sqlite.org/fileformat.html#record_format>

use crate::btree::encode_sqlite_varint;

// ─────────────────────────────────────────────────────────────────────────────
// CellValue — the writer-side counterpart of btree::CellValue
// ─────────────────────────────────────────────────────────────────────────────

/// A typed value to encode into a SQLite record.
///
/// This mirrors [`crate::btree::CellValue`] but uses borrowed slices for
/// `Blob` and `Text` to avoid unnecessary allocations during record building.
pub enum RecordValue<'a> {
    /// SQL NULL (serial type 0, zero body bytes).
    Null,
    /// Signed 64-bit integer — the encoder picks the smallest storage class.
    Int(i64),
    /// IEEE-754 64-bit float (serial type 7, 8 body bytes, big-endian).
    Float(f64),
    /// Raw binary blob.  Serial type = `N * 2 + 12` where `N` is the byte length.
    Blob(&'a [u8]),
    /// UTF-8 text.  Serial type = `N * 2 + 13` where `N` is the byte length.
    Text(&'a str),
}

// ─────────────────────────────────────────────────────────────────────────────
// Serial-type helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Compute the SQLite serial type for a given [`RecordValue`].
fn serial_type(val: &RecordValue<'_>) -> u64 {
    match val {
        RecordValue::Null => 0,
        RecordValue::Int(v) => smallest_int_serial(*v),
        RecordValue::Float(_) => 7,
        RecordValue::Blob(b) => (b.len() as u64) * 2 + 12,
        RecordValue::Text(s) => (s.len() as u64) * 2 + 13,
    }
}

/// Return the smallest integer serial type that can represent `v`.
///
/// Serial types for integers:
/// - 1: i8  (1 byte, range −128 … 127)
/// - 2: i16 (2 bytes, big-endian)
/// - 3: i24 (3 bytes, big-endian, −8 388 608 … 8 388 607)
/// - 4: i32 (4 bytes, big-endian)
/// - 5: i48 (6 bytes, big-endian)
/// - 6: i64 (8 bytes, big-endian)
fn smallest_int_serial(v: i64) -> u64 {
    if (i8::MIN as i64..=i8::MAX as i64).contains(&v) {
        1
    } else if (i16::MIN as i64..=i16::MAX as i64).contains(&v) {
        2
    } else if (-8_388_608..=8_388_607_i64).contains(&v) {
        3
    } else if (i32::MIN as i64..=i32::MAX as i64).contains(&v) {
        4
    } else if (-140_737_488_355_328..=140_737_488_355_327_i64).contains(&v) {
        5
    } else {
        6
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Data bytes for a single value
// ─────────────────────────────────────────────────────────────────────────────

/// Encode the body bytes for a single value (appended to `buf`).
fn encode_value_bytes(val: &RecordValue<'_>, buf: &mut Vec<u8>) {
    match val {
        RecordValue::Null => {}
        RecordValue::Int(v) => {
            let st = smallest_int_serial(*v);
            match st {
                1 => buf.push(*v as u8),
                2 => buf.extend_from_slice(&(*v as i16).to_be_bytes()),
                3 => {
                    let raw = (*v as i32).to_be_bytes();
                    buf.push(raw[1]);
                    buf.push(raw[2]);
                    buf.push(raw[3]);
                }
                4 => buf.extend_from_slice(&(*v as i32).to_be_bytes()),
                5 => {
                    let raw = v.to_be_bytes();
                    // 6 bytes: bytes 2..8 of the big-endian i64
                    buf.extend_from_slice(&raw[2..8]);
                }
                6 => buf.extend_from_slice(&v.to_be_bytes()),
                _ => unreachable!("smallest_int_serial returns 1-6"),
            }
        }
        RecordValue::Float(v) => {
            buf.extend_from_slice(&v.to_be_bytes());
        }
        RecordValue::Blob(b) => {
            buf.extend_from_slice(b);
        }
        RecordValue::Text(s) => {
            buf.extend_from_slice(s.as_bytes());
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Public encoder
// ─────────────────────────────────────────────────────────────────────────────

/// Encode a sequence of typed values into a SQLite record payload.
///
/// The returned `Vec<u8>` is the complete record body: the SQLite cell format
/// prepends `varint(payload_len) + varint(rowid)` in front of this — that is
/// done by [`super::btree_writer::LeafPageBuilder`].
pub fn encode_record(cols: &[RecordValue<'_>]) -> Vec<u8> {
    // 1. Compute serial types.
    let serial_types: Vec<u64> = cols.iter().map(serial_type).collect();

    // 2. Encode each serial type as a varint.
    let type_varints: Vec<Vec<u8>> = serial_types
        .iter()
        .map(|st| encode_sqlite_varint(*st))
        .collect();

    // 3. Compute total header size:
    //    header_length_varint_size + sum(type_varint_sizes)
    //    We may need two passes because the header_length varint itself is
    //    variable-length.  Use an iterative approach to converge.
    let type_varints_total: usize = type_varints.iter().map(|v| v.len()).sum();

    // Initial estimate: assume header_length fits in 1 varint byte.
    // The header_length value includes its own varint's bytes.
    let header_len_estimate = 1 + type_varints_total;
    let header_len_varint = encode_sqlite_varint(header_len_estimate as u64);

    // Recalculate with the actual varint size.
    let true_header_len = header_len_varint.len() + type_varints_total;
    let true_varint = encode_sqlite_varint(true_header_len as u64);

    // If the encoding size changed, recompute once more (rarely > 2 iterations
    // for realistic payloads; varint grows at 128, 16384, etc.).
    let final_header_len = if true_varint.len() != header_len_varint.len() {
        true_varint.len() + type_varints_total
    } else {
        true_header_len
    };
    let final_varint = encode_sqlite_varint(final_header_len as u64);

    // 4. Encode data bytes.
    let mut data_buf = Vec::new();
    for col in cols {
        encode_value_bytes(col, &mut data_buf);
    }

    // 5. Assemble: header_length_varint + type_varints + data_bytes.
    let mut out = Vec::with_capacity(final_varint.len() + type_varints_total + data_buf.len());
    out.extend_from_slice(&final_varint);
    for tv in &type_varints {
        out.extend_from_slice(tv);
    }
    out.extend_from_slice(&data_buf);
    out
}
