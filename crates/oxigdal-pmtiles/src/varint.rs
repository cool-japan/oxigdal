//! LEB-128 varint encoding and decoding utilities.
//!
//! PMTiles v3 directories use unsigned LEB-128 varints for all integer fields.

use crate::error::PmTilesError;

/// Re-export of [`crate::directory::decode_varint`] for convenience.
pub use crate::directory::decode_varint;

/// Encode an unsigned 64-bit integer as an LEB-128 varint.
///
/// Returns a `Vec<u8>` containing 1 to 10 bytes.
pub fn encode_varint(value: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(10);
    let mut val = value;
    loop {
        let byte = (val & 0x7F) as u8;
        val >>= 7;
        if val == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
    out
}

/// Encode an unsigned 64-bit integer as an LEB-128 varint, appending to `buf`.
pub fn encode_varint_into(value: u64, buf: &mut Vec<u8>) {
    let mut val = value;
    loop {
        let byte = (val & 0x7F) as u8;
        val >>= 7;
        if val == 0 {
            buf.push(byte);
            break;
        }
        buf.push(byte | 0x80);
    }
}

/// Decode a varint and validate it fits in `u32` range.
///
/// Returns `(value, bytes_consumed)`.
///
/// # Errors
/// Returns [`PmTilesError::InvalidFormat`] if the varint exceeds `u32::MAX` or
/// is malformed.
pub fn decode_varint_u32(data: &[u8]) -> Result<(u32, usize), PmTilesError> {
    let (val, consumed) = decode_varint(data)?;
    let val32 = u32::try_from(val)
        .map_err(|_| PmTilesError::InvalidFormat(format!("Varint value {val} exceeds u32::MAX")))?;
    Ok((val32, consumed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_varint_zero() {
        assert_eq!(encode_varint(0), vec![0x00]);
    }

    #[test]
    fn test_encode_varint_one() {
        assert_eq!(encode_varint(1), vec![0x01]);
    }

    #[test]
    fn test_encode_varint_127() {
        assert_eq!(encode_varint(127), vec![0x7F]);
    }

    #[test]
    fn test_encode_varint_128() {
        assert_eq!(encode_varint(128), vec![0x80, 0x01]);
    }

    #[test]
    fn test_encode_varint_16384() {
        assert_eq!(encode_varint(16384), vec![0x80, 0x80, 0x01]);
    }

    #[test]
    fn test_encode_decode_round_trip() {
        let values = [
            0,
            1,
            127,
            128,
            255,
            256,
            16383,
            16384,
            624_485,
            u32::MAX as u64,
            u64::MAX,
        ];
        for &v in &values {
            let encoded = encode_varint(v);
            let (decoded, consumed) = decode_varint(&encoded).expect("decode ok");
            assert_eq!(decoded, v, "round trip failed for {v}");
            assert_eq!(consumed, encoded.len());
        }
    }

    #[test]
    fn test_encode_varint_into() {
        let mut buf = Vec::new();
        encode_varint_into(128, &mut buf);
        assert_eq!(buf, vec![0x80, 0x01]);
        encode_varint_into(5, &mut buf);
        assert_eq!(buf, vec![0x80, 0x01, 0x05]);
    }

    #[test]
    fn test_decode_varint_u32_ok() {
        let encoded = encode_varint(12345);
        let (val, consumed) = decode_varint_u32(&encoded).expect("ok");
        assert_eq!(val, 12345);
        assert_eq!(consumed, encoded.len());
    }

    #[test]
    fn test_decode_varint_u32_overflow() {
        let encoded = encode_varint(u64::from(u32::MAX) + 1);
        assert!(decode_varint_u32(&encoded).is_err());
    }
}
