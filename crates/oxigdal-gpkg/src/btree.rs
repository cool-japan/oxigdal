//! Pure Rust SQLite B-tree page traversal.
//!
//! Parses interior and leaf table B-tree pages from the SQLite binary format,
//! supporting full table scans including the `sqlite_master` bootstrap table.
//!
//! Reference: <https://www.sqlite.org/fileformat.html#b_tree_pages>

use crate::error::GpkgError;

// ─────────────────────────────────────────────────────────────────────────────
// SQLite varint decoding
// ─────────────────────────────────────────────────────────────────────────────

/// Decode a SQLite variable-length integer from `data`.
///
/// SQLite varints use **big-endian accumulation** (NOT LEB128):
/// - Bytes 1 through 8: high bit = continuation flag, low 7 bits = data
/// - Byte 9 (if reached): all 8 bits are data bits
/// - Maximum 9 bytes, maximum value 2^64 - 1
///
/// Returns `(value, bytes_consumed)` on success.
///
/// # Errors
/// Returns [`GpkgError::InsufficientData`] if `data` is empty.
pub fn decode_sqlite_varint(data: &[u8]) -> Result<(u64, usize), GpkgError> {
    if data.is_empty() {
        return Err(GpkgError::InsufficientData {
            needed: 1,
            available: 0,
        });
    }

    let mut result: u64 = 0;

    for i in 0..8 {
        if i >= data.len() {
            return Err(GpkgError::InsufficientData {
                needed: i + 1,
                available: data.len(),
            });
        }
        let byte = data[i];
        // Accumulate the low 7 bits (big-endian: shift existing bits left)
        result = (result << 7) | (byte & 0x7F) as u64;
        // If the high bit is not set, this is the last byte
        if byte & 0x80 == 0 {
            return Ok((result, i + 1));
        }
    }

    // If we reach the 9th byte, all 8 bits are data
    if data.len() < 9 {
        return Err(GpkgError::InsufficientData {
            needed: 9,
            available: data.len(),
        });
    }
    result = (result << 8) | data[8] as u64;
    Ok((result, 9))
}

/// Encode a `u64` value as a SQLite variable-length integer.
///
/// Returns a vector of 1-9 bytes in the SQLite varint format.
pub fn encode_sqlite_varint(mut value: u64) -> Vec<u8> {
    if value <= 0x7F {
        return vec![value as u8];
    }

    // Check if we need all 9 bytes: values > 2^56 - 1
    // The 9th byte carries all 8 bits (no continuation flag).
    if value > 0x00FF_FFFF_FFFF_FFFF {
        // Collect the 9 output bytes in a fixed buffer.
        let mut buf = [0u8; 9];
        buf[8] = (value & 0xFF) as u8;
        value >>= 8;
        for i in (0..8).rev() {
            buf[i] = (value & 0x7F) as u8 | 0x80;
            value >>= 7;
        }
        return buf.to_vec();
    }

    // For smaller multi-byte values: collect 7-bit groups from LSB into `temp`,
    // then reverse-output with continuation bits on all but the last byte.
    let mut temp = [0u8; 9];
    temp[0] = (value & 0x7F) as u8;
    value >>= 7;
    let mut len: usize = 1;

    while value > 0 {
        temp[len] = (value & 0x7F) as u8;
        value >>= 7;
        len += 1;
    }

    let mut result = Vec::with_capacity(len);
    for i in (1..len).rev() {
        result.push(temp[i] | 0x80);
    }
    result.push(temp[0]); // last byte: no continuation bit
    result
}

// ─────────────────────────────────────────────────────────────────────────────
// CellValue — typed SQLite record value
// ─────────────────────────────────────────────────────────────────────────────

/// A typed value decoded from a SQLite record (cell payload).
#[derive(Debug, Clone, PartialEq)]
pub enum CellValue {
    /// SQL NULL.
    Null,
    /// Signed 64-bit integer (covers serial types 1-6, 8, 9).
    Integer(i64),
    /// IEEE-754 64-bit float (serial type 7).
    Float(f64),
    /// UTF-8 text (serial type >= 13, odd).
    Text(String),
    /// Raw binary data (serial type >= 12, even).
    Blob(Vec<u8>),
}

// ─────────────────────────────────────────────────────────────────────────────
// MasterEntry — a row from sqlite_master
// ─────────────────────────────────────────────────────────────────────────────

/// A row from the `sqlite_master` system table.
///
/// The five columns are: `type`, `name`, `tbl_name`, `rootpage`, `sql`.
#[derive(Debug, Clone)]
pub struct MasterEntry {
    /// Object type: `"table"`, `"index"`, `"view"`, or `"trigger"`.
    pub entry_type: String,
    /// Object name.
    pub name: String,
    /// Associated table name (same as `name` for tables).
    pub tbl_name: String,
    /// Root B-tree page number (0 for views/triggers without storage).
    pub rootpage: u32,
    /// The `CREATE` SQL statement that created this object.
    pub sql: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Record parsing
// ─────────────────────────────────────────────────────────────────────────────

/// Parse the serial-type–encoded values from a SQLite record payload.
///
/// A record is laid out as:
/// - `header_length` (varint): total header size including this varint
/// - serial types (varints): one per column
/// - values: concatenated, sizes determined by serial types
///
/// Returns a vector of [`CellValue`]s.
fn parse_record(data: &[u8]) -> Result<Vec<CellValue>, GpkgError> {
    if data.is_empty() {
        return Err(GpkgError::InvalidFormat("Empty record payload".into()));
    }

    // Read the header length varint
    let (header_len, hdr_varint_size) = decode_sqlite_varint(data)?;
    let header_len = header_len as usize;

    if header_len > data.len() {
        return Err(GpkgError::InsufficientData {
            needed: header_len,
            available: data.len(),
        });
    }

    // Read all serial types from the header
    let mut serial_types = Vec::new();
    let mut hdr_pos = hdr_varint_size;
    while hdr_pos < header_len {
        if hdr_pos >= data.len() {
            return Err(GpkgError::InsufficientData {
                needed: hdr_pos + 1,
                available: data.len(),
            });
        }
        let (st, consumed) = decode_sqlite_varint(&data[hdr_pos..])?;
        hdr_pos += consumed;
        serial_types.push(st);
    }

    // Read values starting after the header
    let mut val_pos = header_len;
    let mut values = Vec::with_capacity(serial_types.len());

    for st in &serial_types {
        let (value, size) = decode_serial_value(data, val_pos, *st)?;
        values.push(value);
        val_pos += size;
    }

    Ok(values)
}

/// Decode a single value from a record body given its serial type.
///
/// Returns `(value, bytes_consumed)`.
fn decode_serial_value(
    data: &[u8],
    pos: usize,
    serial_type: u64,
) -> Result<(CellValue, usize), GpkgError> {
    match serial_type {
        // NULL
        0 => Ok((CellValue::Null, 0)),
        // 8-bit signed integer
        1 => {
            check_bounds(data, pos, 1)?;
            let v = data[pos] as i8 as i64;
            Ok((CellValue::Integer(v), 1))
        }
        // 16-bit signed integer, big-endian
        2 => {
            check_bounds(data, pos, 2)?;
            let v = i16::from_be_bytes([data[pos], data[pos + 1]]) as i64;
            Ok((CellValue::Integer(v), 2))
        }
        // 24-bit signed integer, big-endian
        3 => {
            check_bounds(data, pos, 3)?;
            // Sign-extend from 24 bits
            let raw =
                ((data[pos] as u32) << 16) | ((data[pos + 1] as u32) << 8) | data[pos + 2] as u32;
            let v = if raw & 0x80_0000 != 0 {
                // negative: sign-extend
                (raw | 0xFF00_0000) as i32 as i64
            } else {
                raw as i64
            };
            Ok((CellValue::Integer(v), 3))
        }
        // 32-bit signed integer, big-endian
        4 => {
            check_bounds(data, pos, 4)?;
            let v =
                i32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as i64;
            Ok((CellValue::Integer(v), 4))
        }
        // 48-bit signed integer, big-endian
        5 => {
            check_bounds(data, pos, 6)?;
            let raw = ((data[pos] as u64) << 40)
                | ((data[pos + 1] as u64) << 32)
                | ((data[pos + 2] as u64) << 24)
                | ((data[pos + 3] as u64) << 16)
                | ((data[pos + 4] as u64) << 8)
                | data[pos + 5] as u64;
            let v = if raw & 0x0000_8000_0000_0000 != 0 {
                // Sign-extend from 48 bits
                (raw | 0xFFFF_0000_0000_0000) as i64
            } else {
                raw as i64
            };
            Ok((CellValue::Integer(v), 6))
        }
        // 64-bit signed integer, big-endian
        6 => {
            check_bounds(data, pos, 8)?;
            let v = i64::from_be_bytes([
                data[pos],
                data[pos + 1],
                data[pos + 2],
                data[pos + 3],
                data[pos + 4],
                data[pos + 5],
                data[pos + 6],
                data[pos + 7],
            ]);
            Ok((CellValue::Integer(v), 8))
        }
        // 64-bit IEEE 754 float, big-endian
        7 => {
            check_bounds(data, pos, 8)?;
            let v = f64::from_be_bytes([
                data[pos],
                data[pos + 1],
                data[pos + 2],
                data[pos + 3],
                data[pos + 4],
                data[pos + 5],
                data[pos + 6],
                data[pos + 7],
            ]);
            Ok((CellValue::Float(v), 8))
        }
        // Literal integer 0
        8 => Ok((CellValue::Integer(0), 0)),
        // Literal integer 1
        9 => Ok((CellValue::Integer(1), 0)),
        // Reserved (10, 11) — treat as NULL
        10 | 11 => Ok((CellValue::Null, 0)),
        // BLOB: serial_type >= 12 and even → length = (N-12)/2
        n if n >= 12 && n % 2 == 0 => {
            let len = ((n - 12) / 2) as usize;
            check_bounds(data, pos, len)?;
            let blob = data[pos..pos + len].to_vec();
            Ok((CellValue::Blob(blob), len))
        }
        // TEXT: serial_type >= 13 and odd → length = (N-13)/2
        n if n >= 13 => {
            let len = ((n - 13) / 2) as usize;
            check_bounds(data, pos, len)?;
            let text = String::from_utf8_lossy(&data[pos..pos + len]).into_owned();
            Ok((CellValue::Text(text), len))
        }
        other => Err(GpkgError::InvalidFormat(format!(
            "Unknown serial type {other}"
        ))),
    }
}

/// Check that `data[pos..pos+size]` is within bounds.
fn check_bounds(data: &[u8], pos: usize, size: usize) -> Result<(), GpkgError> {
    let end = pos
        .checked_add(size)
        .ok_or_else(|| GpkgError::InvalidFormat("Overflow computing data bounds".into()))?;
    if end > data.len() {
        return Err(GpkgError::InsufficientData {
            needed: end,
            available: data.len(),
        });
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Leaf table B-tree page (type 13)
// ─────────────────────────────────────────────────────────────────────────────

/// Parse a leaf table B-tree page (type 13).
///
/// `page_data` is the raw page bytes (exactly `page_size` bytes).
/// `header_offset` is 100 for page 1 (after the SQLite file header), 0 for all
/// other pages.
///
/// Returns a vector of `(rowid, column_values)` tuples, one per cell.
pub fn parse_leaf_table_page(
    page_data: &[u8],
    page_size: usize,
    header_offset: usize,
) -> Result<Vec<(i64, Vec<CellValue>)>, GpkgError> {
    if page_data.len() < page_size {
        return Err(GpkgError::InsufficientData {
            needed: page_size,
            available: page_data.len(),
        });
    }

    let hdr = header_offset;

    // Verify page type
    if hdr >= page_data.len() {
        return Err(GpkgError::InvalidFormat(
            "Header offset beyond page boundary".into(),
        ));
    }
    let page_type = page_data[hdr];
    if page_type != 13 {
        return Err(GpkgError::InvalidFormat(format!(
            "Expected leaf table page (type 13), got type {page_type}"
        )));
    }

    // Leaf page header: 8 bytes
    // [0] type (1 byte)
    // [1..3] first freeblock offset (2 bytes BE)
    // [3..5] cell count (2 bytes BE)
    // [5..7] content area start (2 bytes BE, 0 means 65536)
    // [7] fragmented free bytes (1 byte)
    if hdr + 8 > page_data.len() {
        return Err(GpkgError::InsufficientData {
            needed: hdr + 8,
            available: page_data.len(),
        });
    }

    let cell_count = u16::from_be_bytes([page_data[hdr + 3], page_data[hdr + 4]]) as usize;

    // Cell pointer array: starts right after the 8-byte header
    let ptr_array_start = hdr + 8;
    let ptr_array_end = ptr_array_start + cell_count * 2;
    if ptr_array_end > page_data.len() {
        return Err(GpkgError::InsufficientData {
            needed: ptr_array_end,
            available: page_data.len(),
        });
    }

    let mut rows = Vec::with_capacity(cell_count);

    for i in 0..cell_count {
        let ptr_offset = ptr_array_start + i * 2;
        let cell_offset =
            u16::from_be_bytes([page_data[ptr_offset], page_data[ptr_offset + 1]]) as usize;

        if cell_offset >= page_size || cell_offset >= page_data.len() {
            return Err(GpkgError::InvalidFormat(format!(
                "Cell pointer {i} offset {cell_offset} out of page bounds"
            )));
        }

        let cell_data = &page_data[cell_offset..];

        // Cell format: payload_length(varint) + rowid(varint) + payload
        let (payload_len, pl_size) = decode_sqlite_varint(cell_data)?;
        let payload_len = payload_len as usize;
        let (rowid_raw, rid_size) = decode_sqlite_varint(&cell_data[pl_size..])?;
        let rowid = rowid_raw as i64;

        let payload_start = pl_size + rid_size;

        // Check for overflow: payload that exceeds available inline space
        // Overflow threshold for leaf table cells: usable_size - 35
        let usable_size = page_size; // assumes 0 reserved bytes (typical)
        let overflow_threshold = usable_size.saturating_sub(35);
        let inline_len = if payload_len > overflow_threshold {
            // Compute M = min(P, U-35) for the inline portion
            // The remaining data spills to overflow pages — we don't follow them
            let m = overflow_threshold.min(payload_len);
            // Check if the inline data + 4-byte overflow pointer fits
            if payload_start + m + 4 > cell_data.len() {
                return Err(GpkgError::InvalidFormat(format!(
                    "Cell {i}: overflow cell needs {} bytes inline + 4-byte pointer, \
                     but only {} available from cell start",
                    m,
                    cell_data.len().saturating_sub(payload_start)
                )));
            }
            m
        } else {
            payload_len
        };

        if payload_start + inline_len > cell_data.len() {
            return Err(GpkgError::InsufficientData {
                needed: payload_start + inline_len,
                available: cell_data.len(),
            });
        }

        let payload = &cell_data[payload_start..payload_start + inline_len];

        // If the payload was truncated due to overflow, try to parse what we have.
        // If the record header is fully inline, we might get partial results.
        let values = if inline_len < payload_len {
            // Overflow: attempt to parse what is available; report error if header
            // itself is incomplete.
            parse_record(payload).map_err(|_| {
                GpkgError::InvalidFormat(format!(
                    "Cell {i}: payload overflows to another page \
                     (total {payload_len} bytes, inline {inline_len} bytes); \
                     overflow pages are not yet supported"
                ))
            })?
        } else {
            parse_record(payload)?
        };

        rows.push((rowid, values));
    }

    Ok(rows)
}

// ─────────────────────────────────────────────────────────────────────────────
// Interior table B-tree page (type 5)
// ─────────────────────────────────────────────────────────────────────────────

/// Parse an interior table B-tree page (type 5).
///
/// Returns `(children, rightmost_child)` where `children` is a vector of
/// `(left_child_page, key_rowid)` pairs and `rightmost_child` is the
/// right-most child page pointer.
pub fn parse_interior_table_page(
    page_data: &[u8],
    page_size: usize,
    header_offset: usize,
) -> Result<(Vec<(u32, i64)>, u32), GpkgError> {
    if page_data.len() < page_size {
        return Err(GpkgError::InsufficientData {
            needed: page_size,
            available: page_data.len(),
        });
    }

    let hdr = header_offset;

    if hdr >= page_data.len() {
        return Err(GpkgError::InvalidFormat(
            "Header offset beyond page boundary".into(),
        ));
    }
    let page_type = page_data[hdr];
    if page_type != 5 {
        return Err(GpkgError::InvalidFormat(format!(
            "Expected interior table page (type 5), got type {page_type}"
        )));
    }

    // Interior page header: 12 bytes
    // [0] type (1 byte)
    // [1..3] first freeblock offset (2 bytes BE)
    // [3..5] cell count (2 bytes BE)
    // [5..7] content area start (2 bytes BE)
    // [7] fragmented free bytes (1 byte)
    // [8..12] right-most child page pointer (4 bytes BE)
    if hdr + 12 > page_data.len() {
        return Err(GpkgError::InsufficientData {
            needed: hdr + 12,
            available: page_data.len(),
        });
    }

    let cell_count = u16::from_be_bytes([page_data[hdr + 3], page_data[hdr + 4]]) as usize;
    let rightmost_child = u32::from_be_bytes([
        page_data[hdr + 8],
        page_data[hdr + 9],
        page_data[hdr + 10],
        page_data[hdr + 11],
    ]);

    // Cell pointer array: starts right after the 12-byte header
    let ptr_array_start = hdr + 12;
    let ptr_array_end = ptr_array_start + cell_count * 2;
    if ptr_array_end > page_data.len() {
        return Err(GpkgError::InsufficientData {
            needed: ptr_array_end,
            available: page_data.len(),
        });
    }

    let mut children = Vec::with_capacity(cell_count);

    for i in 0..cell_count {
        let ptr_offset = ptr_array_start + i * 2;
        let cell_offset =
            u16::from_be_bytes([page_data[ptr_offset], page_data[ptr_offset + 1]]) as usize;

        if cell_offset >= page_size || cell_offset + 4 >= page_data.len() {
            return Err(GpkgError::InvalidFormat(format!(
                "Interior cell pointer {i} offset {cell_offset} out of bounds"
            )));
        }

        let cell_data = &page_data[cell_offset..];

        // Interior cell: left_child_page(4 bytes BE) + key_rowid(varint)
        if cell_data.len() < 4 {
            return Err(GpkgError::InsufficientData {
                needed: cell_offset + 4,
                available: page_data.len(),
            });
        }

        let left_child =
            u32::from_be_bytes([cell_data[0], cell_data[1], cell_data[2], cell_data[3]]);
        let (key_rowid, _) = decode_sqlite_varint(&cell_data[4..])?;

        children.push((left_child, key_rowid as i64));
    }

    Ok((children, rightmost_child))
}

// ─────────────────────────────────────────────────────────────────────────────
// Full table scan
// ─────────────────────────────────────────────────────────────────────────────

/// Scan an entire table B-tree, returning all rows.
///
/// `file_data` is the complete SQLite file bytes.
/// `root_page` is the 1-indexed root page of the table.
/// `page_size` is the page size in bytes.
///
/// Returns `(rowid, column_values)` for every row in the table.
pub fn scan_table(
    file_data: &[u8],
    root_page: u32,
    page_size: usize,
) -> Result<Vec<(i64, Vec<CellValue>)>, GpkgError> {
    let mut results = Vec::new();
    let mut stack: Vec<u32> = vec![root_page];

    while let Some(page_num) = stack.pop() {
        let page_data = get_page_slice(file_data, page_num, page_size)?;
        let header_offset = if page_num == 1 { 100 } else { 0 };

        if header_offset >= page_data.len() {
            return Err(GpkgError::InvalidFormat(format!(
                "Page {page_num}: header offset {header_offset} beyond page"
            )));
        }

        let page_type = page_data[header_offset];

        match page_type {
            13 => {
                // Leaf table page
                let rows = parse_leaf_table_page(page_data, page_size, header_offset)?;
                results.extend(rows);
            }
            5 => {
                // Interior table page
                let (children, rightmost) =
                    parse_interior_table_page(page_data, page_size, header_offset)?;

                // Push rightmost child first (it will be processed last due to stack)
                stack.push(rightmost);
                // Push left children in reverse order so they are visited in order
                for (child_page, _key) in children.iter().rev() {
                    stack.push(*child_page);
                }
            }
            other => {
                return Err(GpkgError::InvalidFormat(format!(
                    "Page {page_num}: unexpected B-tree page type {other} \
                     (expected 5 for interior or 13 for leaf)"
                )));
            }
        }
    }

    Ok(results)
}

/// Get the byte slice for a 1-indexed page from the full file data.
fn get_page_slice(file_data: &[u8], page_num: u32, page_size: usize) -> Result<&[u8], GpkgError> {
    if page_num == 0 {
        return Err(GpkgError::InvalidFormat(
            "Page numbers are 1-indexed; 0 is invalid".into(),
        ));
    }
    let offset = (page_num as usize - 1) * page_size;
    let end = offset + page_size;
    if end > file_data.len() {
        return Err(GpkgError::InvalidFormat(format!(
            "Page {page_num} out of range (file has {} bytes, need {end})",
            file_data.len()
        )));
    }
    Ok(&file_data[offset..end])
}

// ─────────────────────────────────────────────────────────────────────────────
// sqlite_master scan
// ─────────────────────────────────────────────────────────────────────────────

/// Scan the `sqlite_master` table (always on root page 1) and return all entries.
///
/// The `sqlite_master` table has columns:
/// `type TEXT, name TEXT, tbl_name TEXT, rootpage INTEGER, sql TEXT`
pub fn scan_sqlite_master(
    file_data: &[u8],
    page_size: usize,
) -> Result<Vec<MasterEntry>, GpkgError> {
    let rows = scan_table(file_data, 1, page_size)?;
    let mut entries = Vec::with_capacity(rows.len());

    for (_, values) in rows {
        if values.len() < 5 {
            continue; // skip malformed rows
        }

        let entry_type = cell_value_to_string(&values[0]);
        let name = cell_value_to_string(&values[1]);
        let tbl_name = cell_value_to_string(&values[2]);
        let rootpage = cell_value_to_u32(&values[3]);
        let sql = cell_value_to_string(&values[4]);

        entries.push(MasterEntry {
            entry_type,
            name,
            tbl_name,
            rootpage,
            sql,
        });
    }

    Ok(entries)
}

/// Extract a string from a CellValue (returns empty string for non-text types).
fn cell_value_to_string(val: &CellValue) -> String {
    match val {
        CellValue::Text(s) => s.clone(),
        CellValue::Integer(i) => i.to_string(),
        CellValue::Float(f) => f.to_string(),
        CellValue::Blob(b) => String::from_utf8_lossy(b).into_owned(),
        CellValue::Null => String::new(),
    }
}

/// Extract a u32 from a CellValue (returns 0 for non-integer types).
fn cell_value_to_u32(val: &CellValue) -> u32 {
    match val {
        CellValue::Integer(i) if *i >= 0 && *i <= u32::MAX as i64 => *i as u32,
        _ => 0,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::panic, clippy::approx_constant, clippy::vec_init_then_push)]
mod tests {
    use super::*;

    // ── SQLite varint tests ──────────────────────────────────────────────────

    #[test]
    fn varint_single_byte_zero() {
        let data = [0x00];
        let (val, len) = decode_sqlite_varint(&data).expect("decode");
        assert_eq!(val, 0);
        assert_eq!(len, 1);
    }

    #[test]
    fn varint_single_byte_max() {
        let data = [0x7F]; // 127
        let (val, len) = decode_sqlite_varint(&data).expect("decode");
        assert_eq!(val, 127);
        assert_eq!(len, 1);
    }

    #[test]
    fn varint_two_bytes_128() {
        // 128 = 0b10000000 → needs 2 bytes in SQLite varint
        // First byte: continuation=1, value bits = 0x01 → 0x81
        // Second byte: continuation=0, value bits = 0x00 → 0x00
        let data = [0x81, 0x00];
        let (val, len) = decode_sqlite_varint(&data).expect("decode");
        assert_eq!(val, 128);
        assert_eq!(len, 2);
    }

    #[test]
    fn varint_two_bytes_16383() {
        // 16383 = 0x3FFF = 0b0011_1111_1111_1111
        // Split into 7-bit groups from MSB: 0x7F, 0x7F
        // First byte: 0xFF (continuation + 0x7F)
        // Second byte: 0x7F (no continuation)
        let data = [0xFF, 0x7F];
        let (val, len) = decode_sqlite_varint(&data).expect("decode");
        assert_eq!(val, 16383);
        assert_eq!(len, 2);
    }

    #[test]
    fn varint_nine_bytes_max_u64() {
        // All 9 bytes: bytes 1-8 = 0xFF (continuation + 7 data bits = 0x7F),
        // byte 9 = 0xFF (all 8 data bits)
        // Total data bits: 8 * 7 + 8 = 64 bits
        let data = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        let (val, len) = decode_sqlite_varint(&data).expect("decode");
        assert_eq!(val, u64::MAX);
        assert_eq!(len, 9);
    }

    #[test]
    fn varint_empty_data_error() {
        let data: [u8; 0] = [];
        assert!(decode_sqlite_varint(&data).is_err());
    }

    #[test]
    fn varint_encode_decode_roundtrip() {
        let test_values = [
            0u64,
            1,
            127,
            128,
            255,
            256,
            16383,
            16384,
            1_000_000,
            u32::MAX as u64,
            u64::MAX / 2,
            u64::MAX,
        ];
        for &v in &test_values {
            let encoded = encode_sqlite_varint(v);
            let (decoded, len) = decode_sqlite_varint(&encoded)
                .unwrap_or_else(|e| panic!("Failed to decode varint for value {v}: {e}"));
            assert_eq!(
                decoded, v,
                "Round-trip failed for value {v}: encoded={encoded:?}, decoded={decoded}"
            );
            assert_eq!(len, encoded.len(), "Consumed length mismatch for value {v}");
        }
    }

    #[test]
    fn varint_encode_single_byte_values() {
        for v in 0..=127u64 {
            let encoded = encode_sqlite_varint(v);
            assert_eq!(encoded.len(), 1, "Values 0-127 should encode to 1 byte");
            assert_eq!(encoded[0], v as u8);
        }
    }

    // ── Record parsing tests ─────────────────────────────────────────────────

    #[test]
    fn parse_record_null_column() {
        // Header: length=2, serial_type=0 (NULL)
        let data = [0x02, 0x00];
        let values = parse_record(&data).expect("parse");
        assert_eq!(values.len(), 1);
        assert_eq!(values[0], CellValue::Null);
    }

    #[test]
    fn parse_record_integer_i8() {
        // Header: length=2, serial_type=1 (i8). Body: 0x2A = 42
        let data = [0x02, 0x01, 0x2A];
        let values = parse_record(&data).expect("parse");
        assert_eq!(values.len(), 1);
        assert_eq!(values[0], CellValue::Integer(42));
    }

    #[test]
    fn parse_record_integer_i16() {
        // Header: length=2, serial_type=2 (i16). Body: 0x01 0x00 = 256
        let data = [0x02, 0x02, 0x01, 0x00];
        let values = parse_record(&data).expect("parse");
        assert_eq!(values[0], CellValue::Integer(256));
    }

    #[test]
    fn parse_record_float_f64() {
        // Header: length=2, serial_type=7 (f64). Body: 3.14 in BE
        let mut data = vec![0x02, 0x07];
        data.extend_from_slice(&3.14f64.to_be_bytes());
        let values = parse_record(&data).expect("parse");
        if let CellValue::Float(f) = values[0] {
            assert!((f - 3.14).abs() < 1e-10);
        } else {
            panic!("Expected Float, got {:?}", values[0]);
        }
    }

    #[test]
    fn parse_record_literal_zero() {
        // serial_type=8 → integer 0 (no body bytes)
        let data = [0x02, 0x08];
        let values = parse_record(&data).expect("parse");
        assert_eq!(values[0], CellValue::Integer(0));
    }

    #[test]
    fn parse_record_literal_one() {
        // serial_type=9 → integer 1 (no body bytes)
        let data = [0x02, 0x09];
        let values = parse_record(&data).expect("parse");
        assert_eq!(values[0], CellValue::Integer(1));
    }

    #[test]
    fn parse_record_text() {
        // serial_type=15 → TEXT, len=(15-13)/2 = 1 byte
        let data = [0x02, 0x0F, b'A'];
        let values = parse_record(&data).expect("parse");
        assert_eq!(values[0], CellValue::Text("A".to_string()));
    }

    #[test]
    fn parse_record_text_multi_byte() {
        // "Hello" = 5 bytes → serial_type = 5*2 + 13 = 23 = 0x17
        let mut data = vec![0x02, 0x17];
        data.extend_from_slice(b"Hello");
        let values = parse_record(&data).expect("parse");
        assert_eq!(values[0], CellValue::Text("Hello".to_string()));
    }

    #[test]
    fn parse_record_blob() {
        // 3 bytes blob → serial_type = 3*2 + 12 = 18 = 0x12
        let data = [0x02, 0x12, 0xDE, 0xAD, 0xBE];
        let values = parse_record(&data).expect("parse");
        assert_eq!(values[0], CellValue::Blob(vec![0xDE, 0xAD, 0xBE]));
    }

    #[test]
    fn parse_record_multiple_columns() {
        // Two columns: integer(42) + text("Hi")
        // Header: length=3, serial_types=[1, 17] (17 = 2*2+13 for "Hi")
        // Body: 0x2A (42 as i8), "Hi"
        let data = [0x03, 0x01, 0x11, 0x2A, b'H', b'i'];
        let values = parse_record(&data).expect("parse");
        assert_eq!(values.len(), 2);
        assert_eq!(values[0], CellValue::Integer(42));
        assert_eq!(values[1], CellValue::Text("Hi".to_string()));
    }

    // ── Leaf page parsing tests ──────────────────────────────────────────────

    /// Build a synthetic leaf table page with given cells.
    ///
    /// Each cell is `(rowid, record_payload_bytes)`.
    fn build_leaf_page(page_size: usize, cells: &[(i64, &[u8])], header_offset: usize) -> Vec<u8> {
        let mut page = vec![0u8; page_size];
        let cell_count = cells.len();

        // Build cells from the end of the page upward
        let mut content_end = page_size;
        let mut cell_offsets = Vec::with_capacity(cell_count);

        for (rowid, payload) in cells {
            // Encode payload_length varint
            let pl_varint = encode_sqlite_varint(payload.len() as u64);
            // Encode rowid varint
            let rid_varint = encode_sqlite_varint(*rowid as u64);
            let cell_size = pl_varint.len() + rid_varint.len() + payload.len();

            content_end -= cell_size;
            let start = content_end;
            cell_offsets.push(start);

            let mut pos = start;
            page[pos..pos + pl_varint.len()].copy_from_slice(&pl_varint);
            pos += pl_varint.len();
            page[pos..pos + rid_varint.len()].copy_from_slice(&rid_varint);
            pos += rid_varint.len();
            page[pos..pos + payload.len()].copy_from_slice(payload);
        }

        // Write the leaf page header
        let hdr = header_offset;
        page[hdr] = 13; // page type: leaf table
        page[hdr + 1] = 0; // first freeblock (high byte)
        page[hdr + 2] = 0; // first freeblock (low byte)
        page[hdr + 3] = ((cell_count >> 8) & 0xFF) as u8;
        page[hdr + 4] = (cell_count & 0xFF) as u8;
        let content_start = content_end as u16;
        page[hdr + 5] = ((content_start >> 8) & 0xFF) as u8;
        page[hdr + 6] = (content_start & 0xFF) as u8;
        page[hdr + 7] = 0; // fragmented free bytes

        // Write cell pointer array
        let ptr_start = hdr + 8;
        for (i, offset) in cell_offsets.iter().enumerate() {
            let o = *offset as u16;
            page[ptr_start + i * 2] = ((o >> 8) & 0xFF) as u8;
            page[ptr_start + i * 2 + 1] = (o & 0xFF) as u8;
        }

        page
    }

    #[test]
    fn parse_leaf_page_single_cell() {
        let page_size = 4096;
        // Record: header_len=2, serial_type=8 (literal 0) → single NULL-ish column
        let payload = [0x02u8, 0x08]; // header=2 bytes, serial_type=8 (integer 0)
        let page = build_leaf_page(page_size, &[(1, &payload)], 0);

        let rows = parse_leaf_table_page(&page, page_size, 0).expect("parse");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, 1); // rowid
        assert_eq!(rows[0].1.len(), 1);
        assert_eq!(rows[0].1[0], CellValue::Integer(0));
    }

    #[test]
    fn parse_leaf_page_multiple_cells() {
        let page_size = 4096;

        // Cell 1: rowid=1, record with integer 42
        let payload1 = [0x02u8, 0x01, 0x2A]; // header_len=2, serial_type=1 (i8), value=42

        // Cell 2: rowid=2, record with text "AB"
        // serial_type for "AB" (2 bytes): 2*2+13=17=0x11
        let payload2 = [0x02u8, 0x11, b'A', b'B'];

        let page = build_leaf_page(page_size, &[(1, &payload1), (2, &payload2)], 0);
        let rows = parse_leaf_table_page(&page, page_size, 0).expect("parse");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, 1);
        assert_eq!(rows[0].1[0], CellValue::Integer(42));
        assert_eq!(rows[1].0, 2);
        assert_eq!(rows[1].1[0], CellValue::Text("AB".to_string()));
    }

    #[test]
    fn parse_leaf_page_with_header_offset_100() {
        let page_size = 4096;
        let payload = [0x02u8, 0x09]; // integer 1
        let page = build_leaf_page(page_size, &[(10, &payload)], 100);

        let rows = parse_leaf_table_page(&page, page_size, 100).expect("parse page 1");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, 10);
        assert_eq!(rows[0].1[0], CellValue::Integer(1));
    }

    #[test]
    fn parse_leaf_page_wrong_type_error() {
        let mut page = vec![0u8; 4096];
        page[0] = 5; // interior, not leaf
        let result = parse_leaf_table_page(&page, 4096, 0);
        assert!(result.is_err());
    }

    #[test]
    fn parse_leaf_page_empty() {
        let page_size = 4096;
        let page = build_leaf_page(page_size, &[], 0);
        let rows = parse_leaf_table_page(&page, page_size, 0).expect("parse");
        assert!(rows.is_empty());
    }

    // ── Interior page parsing tests ──────────────────────────────────────────

    #[test]
    fn parse_interior_page_basic() {
        let page_size = 4096;
        let mut page = vec![0u8; page_size];

        let hdr = 0;
        page[hdr] = 5; // interior table
        page[hdr + 1] = 0; // freeblock high
        page[hdr + 2] = 0; // freeblock low
        page[hdr + 3] = 0; // cell count high
        page[hdr + 4] = 1; // cell count = 1
        page[hdr + 5] = 0; // content start high
        page[hdr + 6] = 0; // content start low
        page[hdr + 7] = 0; // fragmented

        // Rightmost child pointer: page 3
        page[hdr + 8] = 0;
        page[hdr + 9] = 0;
        page[hdr + 10] = 0;
        page[hdr + 11] = 3;

        // One cell at offset 4080
        let cell_offset: u16 = 4080;
        page[hdr + 12] = (cell_offset >> 8) as u8;
        page[hdr + 13] = (cell_offset & 0xFF) as u8;

        // Cell: left_child=2 (4 bytes BE), key_rowid=50 (varint: 0x32)
        let co = cell_offset as usize;
        page[co] = 0;
        page[co + 1] = 0;
        page[co + 2] = 0;
        page[co + 3] = 2;
        page[co + 4] = 50; // rowid 50 as 1-byte varint

        let (children, rightmost) = parse_interior_table_page(&page, page_size, 0).expect("parse");
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].0, 2); // left child
        assert_eq!(children[0].1, 50); // key rowid
        assert_eq!(rightmost, 3);
    }

    #[test]
    fn parse_interior_page_wrong_type_error() {
        let mut page = vec![0u8; 4096];
        page[0] = 13; // leaf, not interior
        let result = parse_interior_table_page(&page, 4096, 0);
        assert!(result.is_err());
    }

    // ── Full table scan tests ────────────────────────────────────────────────

    #[test]
    fn scan_table_single_leaf_page() {
        let page_size = 4096;

        // Build a single-page SQLite file with a leaf table on page 1
        let payload = [0x02u8, 0x01, 0x07]; // integer 7

        // Build the leaf page content, but we need to account for the
        // 100-byte SQLite header on page 1
        let page = build_leaf_page(page_size, &[(1, &payload)], 100);

        // The full file is just this one page with a valid SQLite header prefix
        let mut file_data = page;
        // Write SQLite magic
        file_data[..16].copy_from_slice(b"SQLite format 3\x00");
        // Page size (offset 16, 2 bytes BE)
        file_data[16..18].copy_from_slice(&(page_size as u16).to_be_bytes());
        // db_size_pages (offset 28)
        file_data[28..32].copy_from_slice(&1u32.to_be_bytes());

        let rows = scan_table(&file_data, 1, page_size).expect("scan");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, 1);
        assert_eq!(rows[0].1[0], CellValue::Integer(7));
    }

    #[test]
    fn scan_table_interior_plus_leaf_with_ten_rows() {
        // A 3-page B-tree: page 1 is interior, points to page 2 (leaf, 5 rows)
        // and page 3 (rightmost leaf, 5 rows). Total: 10 rows across 2 leaves.
        let page_size = 4096;
        let mut file_data = vec![0u8; page_size * 3];

        // SQLite file header
        file_data[..16].copy_from_slice(b"SQLite format 3\x00");
        file_data[16..18].copy_from_slice(&(page_size as u16).to_be_bytes());
        file_data[28..32].copy_from_slice(&3u32.to_be_bytes());

        // Page 1: interior table page at offset 100
        let hdr = 100;
        file_data[hdr] = 5; // interior table
        file_data[hdr + 3] = 0;
        file_data[hdr + 4] = 1; // cell_count = 1
        file_data[hdr + 5] = ((4080u16) >> 8) as u8;
        file_data[hdr + 6] = (4080u16 & 0xFF) as u8;
        file_data[hdr + 8] = 0;
        file_data[hdr + 9] = 0;
        file_data[hdr + 10] = 0;
        file_data[hdr + 11] = 3; // rightmost child = page 3
        // cell pointer at (hdr + 12, +13) → cell at offset 4080
        file_data[hdr + 12] = (4080u16 >> 8) as u8;
        file_data[hdr + 13] = (4080u16 & 0xFF) as u8;
        // cell at offset 4080: left_child=2 (4 BE), key_rowid=5 (1-byte varint)
        file_data[4080] = 0;
        file_data[4081] = 0;
        file_data[4082] = 0;
        file_data[4083] = 2;
        file_data[4084] = 5;

        // Build 5 records for page 2: rowids 1..=5, integer column = rowid × 10.
        let page2_cells: Vec<(i64, Vec<u8>)> = (1..=5i64)
            .map(|rowid| {
                // Record: header_len=2, serial_type=1 (i8), value=rowid*10
                let val = (rowid * 10) as u8;
                (rowid, vec![0x02u8, 0x01, val])
            })
            .collect();
        let page2_refs: Vec<(i64, &[u8])> = page2_cells
            .iter()
            .map(|(r, p)| (*r, p.as_slice()))
            .collect();
        let leaf2 = build_leaf_page(page_size, &page2_refs, 0);
        file_data[page_size..page_size * 2].copy_from_slice(&leaf2);

        // Build 5 records for page 3: rowids 6..=10, integer column = rowid × 10.
        let page3_cells: Vec<(i64, Vec<u8>)> = (6..=10i64)
            .map(|rowid| {
                let val = (rowid * 10) as u8;
                (rowid, vec![0x02u8, 0x01, val])
            })
            .collect();
        let page3_refs: Vec<(i64, &[u8])> = page3_cells
            .iter()
            .map(|(r, p)| (*r, p.as_slice()))
            .collect();
        let leaf3 = build_leaf_page(page_size, &page3_refs, 0);
        file_data[page_size * 2..page_size * 3].copy_from_slice(&leaf3);

        // Full-table scan should yield all 10 rows.
        let rows = scan_table(&file_data, 1, page_size).expect("scan");
        assert_eq!(rows.len(), 10, "expected 10 rows across both leaves");

        // Verify every rowid 1..=10 is present with the correct value.
        for expected_rowid in 1..=10i64 {
            let row = rows
                .iter()
                .find(|(rid, _)| *rid == expected_rowid)
                .unwrap_or_else(|| panic!("rowid {expected_rowid} missing"));
            assert_eq!(
                row.1[0],
                CellValue::Integer(expected_rowid * 10),
                "rowid {expected_rowid}: wrong value"
            );
        }
    }

    #[test]
    fn scan_table_with_interior_node() {
        let page_size = 4096;

        // Build a 3-page file:
        // Page 1: interior table node (page 1 has 100-byte file header offset)
        // Page 2: leaf table page
        // Page 3: leaf table page

        let mut file_data = vec![0u8; page_size * 3];

        // SQLite header
        file_data[..16].copy_from_slice(b"SQLite format 3\x00");
        file_data[16..18].copy_from_slice(&(page_size as u16).to_be_bytes());
        file_data[28..32].copy_from_slice(&3u32.to_be_bytes());

        // Page 1: interior table page at offset 100
        let hdr = 100;
        file_data[hdr] = 5; // interior table
        // cell count = 1
        file_data[hdr + 3] = 0;
        file_data[hdr + 4] = 1;
        // content start
        file_data[hdr + 5] = ((4080u16) >> 8) as u8;
        file_data[hdr + 6] = (4080u16 & 0xFF) as u8;
        // rightmost child = page 3
        file_data[hdr + 8] = 0;
        file_data[hdr + 9] = 0;
        file_data[hdr + 10] = 0;
        file_data[hdr + 11] = 3;

        // Cell pointer: points to cell at offset 4080
        file_data[hdr + 12] = (4080u16 >> 8) as u8;
        file_data[hdr + 13] = (4080u16 & 0xFF) as u8;

        // Cell at offset 4080: left_child=2, key_rowid=5
        file_data[4080] = 0;
        file_data[4081] = 0;
        file_data[4082] = 0;
        file_data[4083] = 2; // left child = page 2
        file_data[4084] = 5; // key rowid = 5

        // Page 2 (offset 4096): leaf table page with rowid=1, value=100
        let page2_base = page_size;
        let payload1 = [0x02u8, 0x01, 0x64]; // integer 100
        let leaf2 = build_leaf_page(page_size, &[(1, &payload1)], 0);
        file_data[page2_base..page2_base + page_size].copy_from_slice(&leaf2);

        // Page 3 (offset 8192): leaf table page with rowid=10, value=200
        let page3_base = page_size * 2;
        // Record: header_len=2 (1 for self + 1 for serial_type varint),
        //         serial_type=2 (i16), value=200 (0x00C8 BE)
        let payload2 = [0x02u8, 0x02, 0x00, 0xC8];
        let leaf3 = build_leaf_page(page_size, &[(10, &payload2)], 0);
        file_data[page3_base..page3_base + page_size].copy_from_slice(&leaf3);

        let rows = scan_table(&file_data, 1, page_size).expect("scan");
        assert_eq!(rows.len(), 2);

        // Rows should contain both entries (order depends on traversal)
        let rowids: Vec<i64> = rows.iter().map(|r| r.0).collect();
        assert!(rowids.contains(&1), "Should contain rowid 1");
        assert!(rowids.contains(&10), "Should contain rowid 10");

        // Verify values
        for (rowid, values) in &rows {
            match *rowid {
                1 => assert_eq!(values[0], CellValue::Integer(100)),
                10 => assert_eq!(values[0], CellValue::Integer(200)),
                other => panic!("Unexpected rowid {other}"),
            }
        }
    }

    // ── sqlite_master scan test ──────────────────────────────────────────────

    #[test]
    fn scan_sqlite_master_basic() {
        let page_size = 4096;

        // Build a sqlite_master row for a table:
        // Columns: type="table", name="my_table", tbl_name="my_table",
        //          rootpage=2, sql="CREATE TABLE my_table(id INTEGER)"
        let entry_type = b"table";
        let name = b"my_table";
        let tbl_name = b"my_table";
        let rootpage: i32 = 2;
        let sql = b"CREATE TABLE my_table(id INTEGER)";

        // Build record payload:
        // serial_type for "table" (5 bytes): 5*2+13=23=0x17
        // serial_type for "my_table" (8 bytes): 8*2+13=29=0x1D
        // serial_type for "my_table" (8 bytes): 29=0x1D
        // serial_type for rootpage (i32): 4
        // serial_type for sql (33 bytes): 33*2+13=79=0x4F
        // Header length = varint-size(1) + 5 × 1-byte serial-types = 6.
        let header_len = 6u8;
        let mut payload = Vec::new();
        payload.push(header_len);
        payload.push(0x17); // "table" serial type
        payload.push(0x1D); // "my_table" serial type
        payload.push(0x1D); // "my_table" serial type
        payload.push(0x04); // i32 serial type
        payload.push(0x4F); // sql serial type

        // Values:
        payload.extend_from_slice(entry_type);
        payload.extend_from_slice(name);
        payload.extend_from_slice(tbl_name);
        payload.extend_from_slice(&rootpage.to_be_bytes());
        payload.extend_from_slice(sql);

        // Build the leaf page at page 1 (header_offset = 100)
        let page = build_leaf_page(page_size, &[(1, &payload)], 100);

        let mut file_data = page;
        file_data[..16].copy_from_slice(b"SQLite format 3\x00");
        file_data[16..18].copy_from_slice(&(page_size as u16).to_be_bytes());
        file_data[28..32].copy_from_slice(&1u32.to_be_bytes());

        let entries = scan_sqlite_master(&file_data, page_size).expect("scan");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].entry_type, "table");
        assert_eq!(entries[0].name, "my_table");
        assert_eq!(entries[0].tbl_name, "my_table");
        assert_eq!(entries[0].rootpage, 2);
        assert_eq!(entries[0].sql, "CREATE TABLE my_table(id INTEGER)");
    }

    // ── Integer serial type edge cases ───────────────────────────────────────

    #[test]
    fn parse_record_negative_i8() {
        // serial_type=1, value=-1 (0xFF as i8)
        let data = [0x02, 0x01, 0xFF];
        let values = parse_record(&data).expect("parse");
        assert_eq!(values[0], CellValue::Integer(-1));
    }

    #[test]
    fn parse_record_i24_negative() {
        // serial_type=3 (i24), value=-1 = 0xFFFFFF
        let data = [0x02, 0x03, 0xFF, 0xFF, 0xFF];
        let values = parse_record(&data).expect("parse");
        assert_eq!(values[0], CellValue::Integer(-1));
    }

    #[test]
    fn parse_record_i32() {
        // serial_type=4 (i32), value=100000 = 0x000186A0
        let data = [0x02, 0x04, 0x00, 0x01, 0x86, 0xA0];
        let values = parse_record(&data).expect("parse");
        assert_eq!(values[0], CellValue::Integer(100_000));
    }

    #[test]
    fn parse_record_i48() {
        // serial_type=5 (i48), value=1 = 0x000000000001
        let data = [0x02, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01];
        let values = parse_record(&data).expect("parse");
        assert_eq!(values[0], CellValue::Integer(1));
    }

    #[test]
    fn parse_record_i48_negative() {
        // serial_type=5 (i48), value=-1 = 0xFFFFFFFFFFFF
        let data = [0x02, 0x05, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        let values = parse_record(&data).expect("parse");
        assert_eq!(values[0], CellValue::Integer(-1));
    }

    #[test]
    fn parse_record_i64() {
        // serial_type=6 (i64), value=i64::MAX
        let mut data = vec![0x02, 0x06];
        data.extend_from_slice(&i64::MAX.to_be_bytes());
        let values = parse_record(&data).expect("parse");
        assert_eq!(values[0], CellValue::Integer(i64::MAX));
    }

    #[test]
    fn parse_record_i64_negative() {
        // serial_type=6 (i64), value=i64::MIN
        let mut data = vec![0x02, 0x06];
        data.extend_from_slice(&i64::MIN.to_be_bytes());
        let values = parse_record(&data).expect("parse");
        assert_eq!(values[0], CellValue::Integer(i64::MIN));
    }
}
