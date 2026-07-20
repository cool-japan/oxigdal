//! HDF5 Superblock Version 2 and 3 parsing.
//!
//! Per HDF5 File Format Specification §III.A.1 "Superblock Version 2/3":
//!
//! V2 introduced in HDF5 1.8, V3 in HDF5 1.10.  Both share the same binary
//! layout; V3 adds "file consistency flags" in the low bits of the `flags`
//! byte but the parse procedure is identical.
//!
//! Layout immediately after the 8-byte signature and 1-byte version field:
//!
//! ```text
//! size_of_offsets                   (1 byte, u8)
//! size_of_lengths                   (1 byte, u8)
//! file_consistency_flags            (1 byte, u8)
//! base_address                      (size_of_offsets bytes)
//! superblock_extension_address      (size_of_offsets bytes)
//! end_of_file_address               (size_of_offsets bytes)
//! root_group_object_header_address  (size_of_offsets bytes)
//! checksum                          (4 bytes, u32 LE)
//! ```
//!
//! The checksum covers every byte from the 8-byte signature up to (but not
//! including) the 4-byte checksum field, using the Jenkins lookup3
//! `hashlittle` algorithm with `initval = 0`.

use byteorder::ReadBytesExt;
use std::io::{Read, Seek};

use crate::error::{Hdf5Error, Result};

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// Parsed fields of a HDF5 Superblock Version 2 or Version 3.
///
/// Both versions share the same binary layout; V3 uses the low bits of
/// [`flags`][`SuperblockV2::flags`] for "file consistency flags" but is
/// parsed identically.
#[derive(Debug, Clone)]
pub struct SuperblockV2 {
    /// Number of bytes used to store file addresses (typically 4 or 8).
    pub size_of_offsets: u8,
    /// Number of bytes used to store dataset lengths (typically 4 or 8).
    pub size_of_lengths: u8,
    /// File consistency / superblock flags byte.
    ///
    /// V2: all bits reserved (should be 0).
    /// V3: bits 0–3 carry "file consistency flags"; upper bits reserved.
    pub flags: u8,
    /// Absolute file address of the base address block.
    pub base_address: u64,
    /// Absolute file address of the superblock extension object header, or
    /// the undefined-address sentinel (`u32::MAX` / `u64::MAX` depending on
    /// `size_of_offsets`) when absent.
    pub superblock_extension_address: u64,
    /// Absolute file address one byte past the end of the file.
    pub end_of_file_address: u64,
    /// Absolute file address of the root group object header.
    pub root_group_object_header_address: u64,
    /// Stored checksum (LE u32) covering the superblock bytes that precede it.
    pub checksum: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
// Private helper: variable-width offset reader
// ─────────────────────────────────────────────────────────────────────────────

/// Read a `size`-byte offset from `reader`, appending the raw bytes to
/// `accumulator` for later checksum validation.
fn read_offset_accumulate<R: Read>(
    reader: &mut R,
    size: u8,
    accumulator: &mut Vec<u8>,
) -> Result<u64> {
    match size {
        4 => {
            let mut buf = [0u8; 4];
            reader.read_exact(&mut buf)?;
            accumulator.extend_from_slice(&buf);
            Ok(u32::from_le_bytes(buf) as u64)
        }
        8 => {
            let mut buf = [0u8; 8];
            reader.read_exact(&mut buf)?;
            accumulator.extend_from_slice(&buf);
            Ok(u64::from_le_bytes(buf))
        }
        other => Err(Hdf5Error::invalid_format(format!(
            "Superblock V2/V3: unsupported size_of_offsets value {other}; expected 4 or 8"
        ))),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Public entry points
// ─────────────────────────────────────────────────────────────────────────────

/// Read a Superblock Version 2 or 3 from `reader`.
///
/// The caller is expected to have already consumed and validated the 8-byte
/// HDF5 signature and the 1-byte version field before calling this function.
/// Both of those bytes must be present in `header_so_far` so that the
/// checksum calculation covers the complete prefix.
///
/// On return, `header_so_far` contains every byte from the start of the
/// superblock (signature + version + all V2 fields) including the 4-byte
/// checksum.  Pass `header_so_far` to
/// [`validate_superblock_checksum`] to verify integrity.
pub fn read_superblock_v2<R: Read + Seek>(
    reader: &mut R,
    header_so_far: &mut Vec<u8>,
) -> Result<SuperblockV2> {
    // ── size_of_offsets (1 byte) ────────────────────────────────────────────
    let size_of_offsets = reader.read_u8()?;
    header_so_far.push(size_of_offsets);

    // ── size_of_lengths (1 byte) ────────────────────────────────────────────
    let size_of_lengths = reader.read_u8()?;
    header_so_far.push(size_of_lengths);

    // ── flags (1 byte) ──────────────────────────────────────────────────────
    let flags = reader.read_u8()?;
    header_so_far.push(flags);

    // Validate offset-size early so we produce a clean error before trying
    // to read any variable-width field.
    if size_of_offsets != 4 && size_of_offsets != 8 {
        return Err(Hdf5Error::invalid_format(format!(
            "Superblock V2/V3: unsupported size_of_offsets value {size_of_offsets}; expected 4 or 8"
        )));
    }

    // ── base_address ────────────────────────────────────────────────────────
    let base_address = read_offset_accumulate(reader, size_of_offsets, header_so_far)?;

    // ── superblock_extension_address ────────────────────────────────────────
    let superblock_extension_address =
        read_offset_accumulate(reader, size_of_offsets, header_so_far)?;

    // ── end_of_file_address ─────────────────────────────────────────────────
    let end_of_file_address = read_offset_accumulate(reader, size_of_offsets, header_so_far)?;

    // ── root_group_object_header_address ────────────────────────────────────
    let root_group_object_header_address =
        read_offset_accumulate(reader, size_of_offsets, header_so_far)?;

    // ── checksum (4 bytes, LE u32) ───────────────────────────────────────────
    let mut checksum_buf = [0u8; 4];
    reader.read_exact(&mut checksum_buf)?;
    let checksum = u32::from_le_bytes(checksum_buf);
    header_so_far.extend_from_slice(&checksum_buf);

    Ok(SuperblockV2 {
        size_of_offsets,
        size_of_lengths,
        flags,
        base_address,
        superblock_extension_address,
        end_of_file_address,
        root_group_object_header_address,
        checksum,
    })
}

/// Validate the Jenkins lookup3 checksum stored in a Superblock V2/V3
/// header buffer.
///
/// `header` must contain every byte from the start of the superblock
/// (8-byte signature) up to **and including** the 4-byte checksum at the
/// end.  The checksum covers `header[..header.len()-4]`; the last four
/// bytes are compared against the computed value.
///
/// Returns `Ok(())` on success, or
/// [`Hdf5Error::InvalidFormat`] when the checksum does not match.
pub fn validate_superblock_checksum(header: &[u8]) -> Result<()> {
    if header.len() < 4 {
        return Err(Hdf5Error::invalid_format(
            "Superblock V2/V3: header too short to contain a checksum",
        ));
    }
    let payload = &header[..header.len() - 4];
    let stored = u32::from_le_bytes([
        header[header.len() - 4],
        header[header.len() - 3],
        header[header.len() - 2],
        header[header.len() - 1],
    ]);
    let computed = jenkins_lookup3_hashlittle(payload, 0);
    if computed != stored {
        Err(Hdf5Error::invalid_format(format!(
            "Superblock V2/V3 checksum mismatch: computed {computed:#010x}, stored {stored:#010x}"
        )))
    } else {
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Jenkins lookup3 hashlittle
// ─────────────────────────────────────────────────────────────────────────────

/// Bob Jenkins' lookup3 `hashlittle` hash function (2006).
///
/// This is the little-endian variant used by HDF5 for superblock checksum
/// validation.  The HDF5 library always calls it with `initval = 0`.
///
/// # Reference
///
/// - <http://burtleburtle.net/bob/hash/doobs.html>
/// - `H5checksum.c` in the official HDF5 source
pub fn jenkins_lookup3_hashlittle(data: &[u8], initval: u32) -> u32 {
    let len = data.len();

    // Initialise a, b, c to the same value, incorporating the length and
    // initval exactly as Jenkins' reference C code does.
    let init = 0xdeadbeef_u32
        .wrapping_add(len as u32)
        .wrapping_add(initval);
    let mut a: u32 = init;
    let mut b: u32 = init;
    let mut c: u32 = init;

    // ── Process 12-byte blocks ────────────────────────────────────────────
    let mut i = 0usize;
    while i + 12 <= len {
        a = a.wrapping_add(u32::from_le_bytes([
            data[i],
            data[i + 1],
            data[i + 2],
            data[i + 3],
        ]));
        b = b.wrapping_add(u32::from_le_bytes([
            data[i + 4],
            data[i + 5],
            data[i + 6],
            data[i + 7],
        ]));
        c = c.wrapping_add(u32::from_le_bytes([
            data[i + 8],
            data[i + 9],
            data[i + 10],
            data[i + 11],
        ]));

        // mix
        a = a.wrapping_sub(c);
        a ^= c.rotate_left(4);
        c = c.wrapping_add(b);
        b = b.wrapping_sub(a);
        b ^= a.rotate_left(6);
        a = a.wrapping_add(c);
        c = c.wrapping_sub(b);
        c ^= b.rotate_left(8);
        b = b.wrapping_add(a);
        a = a.wrapping_sub(c);
        a ^= c.rotate_left(16);
        c = c.wrapping_add(b);
        b = b.wrapping_sub(a);
        b ^= a.rotate_left(19);
        a = a.wrapping_add(c);
        c = c.wrapping_sub(b);
        c ^= b.rotate_left(4);
        b = b.wrapping_add(a);

        i += 12;
    }

    // ── Handle the tail (0–11 remaining bytes) ───────────────────────────
    // Jenkins' reference C code uses a fall-through switch that adds bytes
    // to a/b/c from the highest position downward for each case.
    // We replicate the same byte assignments with explicit match arms.
    let tail = &data[i..];
    let remaining = tail.len();

    if remaining > 0 {
        // Each case adds the byte at position `k` shifted left by the
        // appropriate amount, mirroring Jenkins' little-endian byte order.
        match remaining {
            11 => {
                c = c.wrapping_add((tail[10] as u32) << 24);
                c = c.wrapping_add((tail[9] as u32) << 16);
                c = c.wrapping_add((tail[8] as u32) << 8);
                b = b.wrapping_add((tail[7] as u32) << 24);
                b = b.wrapping_add((tail[6] as u32) << 16);
                b = b.wrapping_add((tail[5] as u32) << 8);
                b = b.wrapping_add(tail[4] as u32);
                a = a.wrapping_add((tail[3] as u32) << 24);
                a = a.wrapping_add((tail[2] as u32) << 16);
                a = a.wrapping_add((tail[1] as u32) << 8);
                a = a.wrapping_add(tail[0] as u32);
            }
            10 => {
                c = c.wrapping_add((tail[9] as u32) << 16);
                c = c.wrapping_add((tail[8] as u32) << 8);
                b = b.wrapping_add((tail[7] as u32) << 24);
                b = b.wrapping_add((tail[6] as u32) << 16);
                b = b.wrapping_add((tail[5] as u32) << 8);
                b = b.wrapping_add(tail[4] as u32);
                a = a.wrapping_add((tail[3] as u32) << 24);
                a = a.wrapping_add((tail[2] as u32) << 16);
                a = a.wrapping_add((tail[1] as u32) << 8);
                a = a.wrapping_add(tail[0] as u32);
            }
            9 => {
                c = c.wrapping_add((tail[8] as u32) << 8);
                b = b.wrapping_add((tail[7] as u32) << 24);
                b = b.wrapping_add((tail[6] as u32) << 16);
                b = b.wrapping_add((tail[5] as u32) << 8);
                b = b.wrapping_add(tail[4] as u32);
                a = a.wrapping_add((tail[3] as u32) << 24);
                a = a.wrapping_add((tail[2] as u32) << 16);
                a = a.wrapping_add((tail[1] as u32) << 8);
                a = a.wrapping_add(tail[0] as u32);
            }
            8 => {
                b = b.wrapping_add((tail[7] as u32) << 24);
                b = b.wrapping_add((tail[6] as u32) << 16);
                b = b.wrapping_add((tail[5] as u32) << 8);
                b = b.wrapping_add(tail[4] as u32);
                a = a.wrapping_add((tail[3] as u32) << 24);
                a = a.wrapping_add((tail[2] as u32) << 16);
                a = a.wrapping_add((tail[1] as u32) << 8);
                a = a.wrapping_add(tail[0] as u32);
            }
            7 => {
                b = b.wrapping_add((tail[6] as u32) << 16);
                b = b.wrapping_add((tail[5] as u32) << 8);
                b = b.wrapping_add(tail[4] as u32);
                a = a.wrapping_add((tail[3] as u32) << 24);
                a = a.wrapping_add((tail[2] as u32) << 16);
                a = a.wrapping_add((tail[1] as u32) << 8);
                a = a.wrapping_add(tail[0] as u32);
            }
            6 => {
                b = b.wrapping_add((tail[5] as u32) << 8);
                b = b.wrapping_add(tail[4] as u32);
                a = a.wrapping_add((tail[3] as u32) << 24);
                a = a.wrapping_add((tail[2] as u32) << 16);
                a = a.wrapping_add((tail[1] as u32) << 8);
                a = a.wrapping_add(tail[0] as u32);
            }
            5 => {
                b = b.wrapping_add(tail[4] as u32);
                a = a.wrapping_add((tail[3] as u32) << 24);
                a = a.wrapping_add((tail[2] as u32) << 16);
                a = a.wrapping_add((tail[1] as u32) << 8);
                a = a.wrapping_add(tail[0] as u32);
            }
            4 => {
                a = a.wrapping_add((tail[3] as u32) << 24);
                a = a.wrapping_add((tail[2] as u32) << 16);
                a = a.wrapping_add((tail[1] as u32) << 8);
                a = a.wrapping_add(tail[0] as u32);
            }
            3 => {
                a = a.wrapping_add((tail[2] as u32) << 16);
                a = a.wrapping_add((tail[1] as u32) << 8);
                a = a.wrapping_add(tail[0] as u32);
            }
            2 => {
                a = a.wrapping_add((tail[1] as u32) << 8);
                a = a.wrapping_add(tail[0] as u32);
            }
            1 => {
                a = a.wrapping_add(tail[0] as u32);
            }
            _ => {} // remaining == 0, handled by the outer `if`
        }
    }

    // ── Final mix ────────────────────────────────────────────────────────
    c ^= b;
    c = c.wrapping_sub(b.rotate_left(14));
    a ^= c;
    a = a.wrapping_sub(c.rotate_left(11));
    b ^= a;
    b = b.wrapping_sub(a.rotate_left(25));
    c ^= b;
    c = c.wrapping_sub(b.rotate_left(16));
    a ^= c;
    a = a.wrapping_sub(c.rotate_left(4));
    b ^= a;
    b = b.wrapping_sub(a.rotate_left(14));
    c ^= b;
    c = c.wrapping_sub(b.rotate_left(24));

    c
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests (in-module, accessible regardless of visibility)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // ── Jenkins lookup3 tests ─────────────────────────────────────────────

    /// Empty input, initval = 0.
    ///
    /// With `len = 0` and `initval = 0` the three accumulators are all
    /// initialised to `0xdeadbeef`.  No 12-byte block iterations occur and
    /// no tail bytes are added, so the final mix is applied directly to
    /// `(0xdeadbeef, 0xdeadbeef, 0xdeadbeef)`.  The result must be
    /// deterministic and equal to the independently computed final-mix value.
    #[test]
    fn test_jenkins_empty_initval_0() {
        let h = jenkins_lookup3_hashlittle(&[], 0);
        // Idempotence: same input always yields the same output.
        assert_eq!(h, jenkins_lookup3_hashlittle(&[], 0));
        // Independently compute the expected value by applying the final-mix
        // directly to the initial state (0xdeadbeef, 0xdeadbeef, 0xdeadbeef).
        let expected = {
            let init = 0xdeadbeef_u32;
            let mut a = init;
            let mut b = init;
            let mut c = init;
            c ^= b;
            c = c.wrapping_sub(b.rotate_left(14));
            a ^= c;
            a = a.wrapping_sub(c.rotate_left(11));
            b ^= a;
            b = b.wrapping_sub(a.rotate_left(25));
            c ^= b;
            c = c.wrapping_sub(b.rotate_left(16));
            a ^= c;
            a = a.wrapping_sub(c.rotate_left(4));
            b ^= a;
            b = b.wrapping_sub(a.rotate_left(14));
            c ^= b;
            c = c.wrapping_sub(b.rotate_left(24));
            c
        };
        assert_eq!(h, expected);
    }

    #[test]
    fn test_jenkins_deterministic_single_byte() {
        let h1 = jenkins_lookup3_hashlittle(b"a", 0);
        let h2 = jenkins_lookup3_hashlittle(b"a", 0);
        assert_eq!(h1, h2);
        // Different initval must produce a different result
        let h3 = jenkins_lookup3_hashlittle(b"a", 1);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_jenkins_distinct_inputs_distinct_hashes() {
        let h1 = jenkins_lookup3_hashlittle(b"hello", 0);
        let h2 = jenkins_lookup3_hashlittle(b"world", 0);
        let h3 = jenkins_lookup3_hashlittle(b"hello world", 0);
        assert_ne!(h1, h2);
        assert_ne!(h1, h3);
        assert_ne!(h2, h3);
    }

    /// Verify that exactly 12 bytes (one full block, no tail) and 13 bytes
    /// (one full block + 1 tail byte) produce different, deterministic results.
    #[test]
    fn test_jenkins_12_byte_boundary() {
        let data12 = b"123456789012";
        let data13 = b"1234567890123";
        let h12 = jenkins_lookup3_hashlittle(data12, 0);
        let h13 = jenkins_lookup3_hashlittle(data13, 0);
        assert_eq!(h12, jenkins_lookup3_hashlittle(data12, 0));
        assert_ne!(h12, h13);
    }

    /// Test all tail lengths (1–11) to ensure no panic and determinism.
    #[test]
    fn test_jenkins_all_tail_lengths() {
        for tail_len in 1..=11usize {
            let data: Vec<u8> = (0..tail_len as u8).collect();
            let h1 = jenkins_lookup3_hashlittle(&data, 0);
            let h2 = jenkins_lookup3_hashlittle(&data, 0);
            assert_eq!(h1, h2, "tail_len={tail_len}");
        }
    }

    // ── read_superblock_v2 / validate_superblock_checksum tests ──────────

    /// Descriptor for a synthetic superblock used in tests.
    struct SbSpec {
        version: u8,
        soo: u8,
        sol: u8,
        flags: u8,
        base: u64,
        ext: u64,
        eof: u64,
        root: u64,
    }

    impl SbSpec {
        fn build(&self) -> Vec<u8> {
            let sig = b"\x89HDF\r\n\x1a\n";
            let mut bytes = Vec::new();
            bytes.extend_from_slice(sig);
            bytes.push(self.version);
            bytes.push(self.soo);
            bytes.push(self.sol);
            bytes.push(self.flags);
            // Only 4 and 8 byte offsets are valid; any other soo would be
            // rejected by read_superblock_v2 anyway, so this helper simply
            // falls through to the default which produces a truncated (but
            // still writable) byte sequence.
            if self.soo == 4 {
                bytes.extend_from_slice(&(self.base as u32).to_le_bytes());
                bytes.extend_from_slice(&(self.ext as u32).to_le_bytes());
                bytes.extend_from_slice(&(self.eof as u32).to_le_bytes());
                bytes.extend_from_slice(&(self.root as u32).to_le_bytes());
            } else {
                bytes.extend_from_slice(&self.base.to_le_bytes());
                bytes.extend_from_slice(&self.ext.to_le_bytes());
                bytes.extend_from_slice(&self.eof.to_le_bytes());
                bytes.extend_from_slice(&self.root.to_le_bytes());
            }
            let checksum = jenkins_lookup3_hashlittle(&bytes, 0);
            bytes.extend_from_slice(&checksum.to_le_bytes());
            bytes
        }
    }

    #[test]
    fn test_parse_v2_8byte_offsets() {
        let bytes = SbSpec {
            version: 2,
            soo: 8,
            sol: 8,
            flags: 0,
            base: 16,
            ext: u64::MAX,
            eof: 65536,
            root: 2048,
        }
        .build();
        let mut cursor = Cursor::new(&bytes[9..]);
        let mut hdr = bytes[..9].to_vec();
        let v2 = read_superblock_v2(&mut cursor, &mut hdr)
            .expect("v2 parse with 8-byte offsets must succeed");

        assert_eq!(v2.size_of_offsets, 8);
        assert_eq!(v2.size_of_lengths, 8);
        assert_eq!(v2.flags, 0);
        assert_eq!(v2.base_address, 16);
        assert_eq!(v2.superblock_extension_address, u64::MAX);
        assert_eq!(v2.end_of_file_address, 65536);
        assert_eq!(v2.root_group_object_header_address, 2048);

        validate_superblock_checksum(&hdr)
            .expect("checksum must be valid for a correctly built superblock");
    }

    #[test]
    fn test_parse_v2_4byte_offsets() {
        let bytes = SbSpec {
            version: 2,
            soo: 4,
            sol: 4,
            flags: 0,
            base: 16,
            ext: u32::MAX as u64,
            eof: 4096,
            root: 512,
        }
        .build();
        let mut cursor = Cursor::new(&bytes[9..]);
        let mut hdr = bytes[..9].to_vec();
        let v2 = read_superblock_v2(&mut cursor, &mut hdr)
            .expect("v2 parse with 4-byte offsets must succeed");

        assert_eq!(v2.size_of_offsets, 4);
        assert_eq!(v2.root_group_object_header_address, 512);

        validate_superblock_checksum(&hdr)
            .expect("checksum must be valid for a correctly built superblock");
    }

    #[test]
    fn test_parse_v3_same_layout() {
        // V3 uses the same binary layout; only the version byte differs.
        let bytes = SbSpec {
            version: 3,
            soo: 8,
            sol: 8,
            flags: 0b0011,
            base: 0,
            ext: u64::MAX,
            eof: 100_000,
            root: 1_000,
        }
        .build();
        let mut cursor = Cursor::new(&bytes[9..]);
        let mut hdr = bytes[..9].to_vec();
        let v3 = read_superblock_v2(&mut cursor, &mut hdr).expect("v3 parse must succeed");

        assert_eq!(v3.flags, 0b0011);
        assert_eq!(v3.root_group_object_header_address, 1_000);

        validate_superblock_checksum(&hdr).expect("v3 checksum must be valid");
    }

    #[test]
    fn test_checksum_corruption_detected() {
        let mut bytes = SbSpec {
            version: 2,
            soo: 8,
            sol: 8,
            flags: 0,
            base: 0,
            ext: u64::MAX,
            eof: 1024,
            root: 256,
        }
        .build();
        // Flip a byte inside the payload (before the checksum).
        bytes[12] ^= 0xFF;
        let result = validate_superblock_checksum(&bytes);
        assert!(result.is_err(), "corrupted header must fail checksum");
        // The error message must mention "checksum mismatch".
        if let Err(e) = result {
            let msg = format!("{e}");
            assert!(
                msg.contains("checksum mismatch"),
                "error should mention checksum mismatch: {msg}"
            );
        }
    }

    #[test]
    fn test_invalid_offset_size_rejected() {
        // Build a header with soo=6 (invalid — only 4 and 8 are supported).
        let sig = b"\x89HDF\r\n\x1a\n";
        let mut bytes = Vec::new();
        bytes.extend_from_slice(sig);
        bytes.push(2u8); // version
        bytes.push(6u8); // invalid soo
        bytes.push(8u8); // sol
        bytes.push(0u8); // flags
        // checksum placeholder (parse will fail before reaching validation)
        bytes.extend_from_slice(&[0u8; 4]);

        let mut cursor = Cursor::new(&bytes[9..]);
        let mut hdr = bytes[..9].to_vec();
        let result = read_superblock_v2(&mut cursor, &mut hdr);
        assert!(result.is_err());
    }

    // ── read_offset_accumulate (coverage) ────────────────────────────────

    #[test]
    fn test_read_offset_accumulate_4() {
        let data = 42u32.to_le_bytes();
        let mut cursor = Cursor::new(data);
        let mut acc = Vec::new();
        let val = read_offset_accumulate(&mut cursor, 4, &mut acc)
            .expect("4-byte accumulate must succeed");
        assert_eq!(val, 42u64);
        assert_eq!(acc, 42u32.to_le_bytes());
    }

    #[test]
    fn test_read_offset_accumulate_8() {
        let data = 0xDEAD_BEEF_CAFE_BABEu64.to_le_bytes();
        let mut cursor = Cursor::new(data);
        let mut acc = Vec::new();
        let val = read_offset_accumulate(&mut cursor, 8, &mut acc)
            .expect("8-byte accumulate must succeed");
        assert_eq!(val, 0xDEAD_BEEF_CAFE_BABE);
        assert_eq!(acc, 0xDEAD_BEEF_CAFE_BABEu64.to_le_bytes());
    }

    #[test]
    fn test_read_offset_accumulate_invalid() {
        let data = [0u8; 16];
        let mut cursor = Cursor::new(data);
        let mut acc = Vec::new();
        assert!(read_offset_accumulate(&mut cursor, 3, &mut acc).is_err());
        // Size 6 is also unsupported
        let mut cursor2 = Cursor::new(data);
        assert!(read_offset_accumulate(&mut cursor2, 6, &mut acc).is_err());
    }
}
