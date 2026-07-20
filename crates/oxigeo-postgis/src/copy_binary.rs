//! PostgreSQL binary COPY payload encoder (PostgreSQL §53.2 — "Binary Format").
//!
//! Produces the byte stream consumed by `COPY ... FROM STDIN WITH (FORMAT binary)`.
//! All integers are big-endian. Fully unit-testable without a database.
//!
//! # Binary COPY stream layout
//!
//! A binary-COPY stream is structured as:
//!
//! ```text
//! +------------------------------------------------------------------+
//! | File header                                                      |
//! |   signature      : 11 bytes  = "PGCOPY\n\xff\r\n\0"               |
//! |   flags          : i32 BE    = 0  (no OID column)                 |
//! |   header ext len : i32 BE    = 0  (no header extension)           |
//! +------------------------------------------------------------------+
//! | Tuple 0                                                          |
//! |   field count    : i16 BE    = number of fields in the row       |
//! |   field 0        : i32 BE length prefix + that many raw bytes,   |
//! |                    or i32 BE -1 to signal SQL NULL               |
//! |   field 1        : ...                                           |
//! +------------------------------------------------------------------+
//! | Tuple 1 ...                                                      |
//! +------------------------------------------------------------------+
//! | File trailer                                                     |
//! |   field count    : i16 BE    = -1  (end-of-data marker)          |
//! +------------------------------------------------------------------+
//! ```
//!
//! For a non-null field the i32 length prefix counts only the field body, not
//! the prefix itself. A length prefix of `-1` denotes SQL `NULL` and is *not*
//! followed by any bytes.

use crate::error::{Result, WkbError};

/// 11-byte fixed signature that opens every binary-COPY stream.
pub const COPY_BINARY_SIGNATURE: [u8; 11] = *b"PGCOPY\n\xff\r\n\0";

/// EWKB SRID flag bit set on the geometry-type word when an SRID is embedded.
///
/// PostGIS Extended WKB sets this bit on the 32-bit geometry-type word to mark
/// that an `i32` SRID immediately follows the type word.
const EWKB_SRID_FLAG: u32 = 0x2000_0000;

/// Incremental encoder for a binary-COPY payload.
///
/// The encoder owns a growing byte buffer. Construction emits the file header;
/// [`begin_row`](CopyBinaryEncoder::begin_row), the `write_*` methods and
/// [`finish`](CopyBinaryEncoder::finish) append tuples and the trailer.
pub struct CopyBinaryEncoder {
    buf: Vec<u8>,
}

impl CopyBinaryEncoder {
    /// New encoder — emits signature + 4-byte flags(0) + 4-byte header extension length(0).
    pub fn new() -> Self {
        let mut buf = Vec::new();
        buf.extend_from_slice(&COPY_BINARY_SIGNATURE);
        buf.extend_from_slice(&0i32.to_be_bytes()); // flags
        buf.extend_from_slice(&0i32.to_be_bytes()); // header extension length
        Self { buf }
    }

    /// Start a row with `field_count` fields (i16 BE).
    pub fn begin_row(&mut self, field_count: i16) {
        self.buf.extend_from_slice(&field_count.to_be_bytes());
    }

    /// Write a non-null field: i32 BE length prefix + raw bytes.
    pub fn write_field_bytes(&mut self, bytes: &[u8]) {
        self.buf
            .extend_from_slice(&(bytes.len() as i32).to_be_bytes());
        self.buf.extend_from_slice(bytes);
    }

    /// Write a NULL field: i32 BE -1.
    pub fn write_null(&mut self) {
        self.buf.extend_from_slice(&(-1i32).to_be_bytes());
    }

    /// Finish: append i16 BE -1 trailer, return the payload.
    pub fn finish(mut self) -> Vec<u8> {
        self.buf.extend_from_slice(&(-1i16).to_be_bytes());
        self.buf
    }

    /// Current length of the in-progress payload in bytes.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Returns `true` when the underlying buffer is empty.
    ///
    /// After [`new`](CopyBinaryEncoder::new) this is always `false` because the
    /// constructor has already emitted the 19-byte file header.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

impl Default for CopyBinaryEncoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert plain WKB to EWKB by setting the SRID flag bit on the geometry-type
/// word and inserting the i32 SRID. Returns EWKB bytes suitable for PostGIS COPY.
///
/// `wkb` must be standard WKB:
///
/// ```text
/// byte 0       : byte order  (0x00 = big-endian, 0x01 = little-endian)
/// bytes 1..5   : geometry type, u32 in the declared byte order
/// bytes 5..    : geometry body
/// ```
///
/// The produced EWKB sets bit `0x20000000` on the geometry-type word and splices
/// a 4-byte SRID (encoded in the same byte order) immediately after the type
/// word:
///
/// ```text
/// byte 0       : byte order  (unchanged)
/// bytes 1..5   : geometry type | 0x20000000, u32 in the declared byte order
/// bytes 5..9   : SRID, i32 in the declared byte order
/// bytes 9..    : geometry body (unchanged)
/// ```
///
/// If the input type word already has `0x20000000` set the WKB is assumed to be
/// EWKB already and is returned verbatim.
///
/// # Errors
///
/// Returns [`WkbError::BufferTooShort`] when `wkb` has fewer than 5 bytes (it
/// cannot contain a byte-order byte plus a 4-byte type word), and
/// [`WkbError::InvalidByteOrder`] when byte 0 is neither `0x00` nor `0x01`.
pub fn ewkb_from_wkb(wkb: &[u8], srid: i32) -> Result<Vec<u8>> {
    // A valid WKB header is 1 byte (byte order) + 4 bytes (geometry type).
    if wkb.len() < 5 {
        return Err(WkbError::BufferTooShort {
            expected: 5,
            actual: wkb.len(),
        }
        .into());
    }

    let order_byte = wkb[0];
    let little_endian = match order_byte {
        0x00 => false,
        0x01 => true,
        other => return Err(WkbError::InvalidByteOrder { byte: other }.into()),
    };

    // Read the 4-byte geometry-type word in the declared byte order.
    let type_bytes: [u8; 4] = [wkb[1], wkb[2], wkb[3], wkb[4]];
    let type_word = if little_endian {
        u32::from_le_bytes(type_bytes)
    } else {
        u32::from_be_bytes(type_bytes)
    };

    // Already EWKB: the SRID flag is set, so return the input untouched.
    if type_word & EWKB_SRID_FLAG != 0 {
        return Ok(wkb.to_vec());
    }

    // Set the SRID flag and re-encode the type word in the same byte order.
    let ewkb_type = type_word | EWKB_SRID_FLAG;
    let ewkb_type_bytes = if little_endian {
        ewkb_type.to_le_bytes()
    } else {
        ewkb_type.to_be_bytes()
    };
    let srid_bytes = if little_endian {
        srid.to_le_bytes()
    } else {
        srid.to_be_bytes()
    };

    // Assemble: order byte + flagged type word + SRID + original body.
    let mut out = Vec::with_capacity(wkb.len() + 4);
    out.push(order_byte);
    out.extend_from_slice(&ewkb_type_bytes);
    out.extend_from_slice(&srid_bytes);
    out.extend_from_slice(&wkb[5..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_emits_header() {
        let encoder = CopyBinaryEncoder::new();
        // 11-byte signature + 4-byte flags + 4-byte header extension length.
        assert_eq!(encoder.len(), 19);
        assert!(!encoder.is_empty());
    }

    #[test]
    fn test_finish_minimal_stream() {
        let encoder = CopyBinaryEncoder::new();
        let payload = encoder.finish();
        // 19-byte header + 2-byte trailer.
        assert_eq!(payload.len(), 21);
        assert_eq!(&payload[payload.len() - 2..], &[0xFF, 0xFF]);
    }

    #[test]
    fn test_ewkb_rejects_short_buffer() {
        let result = ewkb_from_wkb(&[0x01, 0x01, 0x00], 4326);
        assert!(result.is_err());
    }

    #[test]
    fn test_ewkb_rejects_bad_byte_order() {
        // Byte 0 = 0x02 is neither big- nor little-endian.
        let result = ewkb_from_wkb(&[0x02, 0x01, 0x00, 0x00, 0x00], 4326);
        assert!(result.is_err());
    }

    #[test]
    fn test_ewkb_idempotent_when_already_ewkb() {
        // Little-endian Point type word already carrying the SRID flag
        // (0x20000001) followed by an existing SRID.
        let ewkb = [
            0x01, // little-endian
            0x01, 0x00, 0x00, 0x20, // type = Point | 0x20000000
            0xE6, 0x10, 0x00, 0x00, // SRID 4326
        ];
        let out = ewkb_from_wkb(&ewkb, 3857).expect("ewkb conversion failed");
        assert_eq!(out, ewkb);
    }
}
