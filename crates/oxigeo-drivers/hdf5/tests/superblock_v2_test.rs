//! Integration tests for HDF5 Superblock V2/V3 parsing.
//!
//! These tests exercise the public API exposed from `oxigeo_hdf5::superblock_v2`
//! and verify end-to-end dispatch via `Hdf5Reader::open`.

use oxigeo_hdf5::superblock_v2::{
    jenkins_lookup3_hashlittle, read_superblock_v2, validate_superblock_checksum,
};
use std::io::Cursor;

// ─────────────────────────────────────────────────────────────────────────────
// Jenkins lookup3 hashlittle tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_jenkins_empty_string_initval_0() {
    // Empty input with initval=0 must always return the same value.
    let result = jenkins_lookup3_hashlittle(&[], 0);
    assert_eq!(result, jenkins_lookup3_hashlittle(&[], 0));
}

#[test]
fn test_jenkins_single_byte() {
    let h1 = jenkins_lookup3_hashlittle(b"a", 0);
    let h2 = jenkins_lookup3_hashlittle(b"a", 0);
    // Same input → same output (deterministic)
    assert_eq!(h1, h2);
    // Different initval → different output
    let h3 = jenkins_lookup3_hashlittle(b"a", 1);
    assert_ne!(h1, h3);
}

#[test]
fn test_jenkins_different_lengths_different_hashes() {
    let h1 = jenkins_lookup3_hashlittle(b"hello", 0);
    let h2 = jenkins_lookup3_hashlittle(b"world", 0);
    let h3 = jenkins_lookup3_hashlittle(b"hello world", 0);
    assert_ne!(h1, h2);
    assert_ne!(h1, h3);
    assert_ne!(h2, h3);
}

#[test]
fn test_jenkins_12_byte_boundary() {
    let data12 = b"123456789012";
    let data13 = b"1234567890123";
    let h12 = jenkins_lookup3_hashlittle(data12, 0);
    let h13 = jenkins_lookup3_hashlittle(data13, 0);
    // Deterministic
    assert_eq!(h12, jenkins_lookup3_hashlittle(data12, 0));
    // Different inputs → different hashes
    assert_ne!(h12, h13);
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper builder for synthetic superblock byte sequences
// ─────────────────────────────────────────────────────────────────────────────

/// Builder for a minimal synthetic HDF5 Superblock V2/V3 byte sequence.
///
/// Avoids the clippy `too_many_arguments` lint by using method chaining.
struct SuperblockBuilder {
    version: u8,
    soo: u8,
    sol: u8,
    flags: u8,
    base: u64,
    ext: u64,
    eof: u64,
    root: u64,
}

impl SuperblockBuilder {
    fn new() -> Self {
        Self {
            version: 2,
            soo: 8,
            sol: 8,
            flags: 0,
            base: 0,
            ext: u64::MAX,
            eof: 0,
            root: 0,
        }
    }

    fn version(mut self, v: u8) -> Self {
        self.version = v;
        self
    }
    fn soo(mut self, v: u8) -> Self {
        self.soo = v;
        self
    }
    fn sol(mut self, v: u8) -> Self {
        self.sol = v;
        self
    }
    fn flags(mut self, v: u8) -> Self {
        self.flags = v;
        self
    }
    fn base(mut self, v: u64) -> Self {
        self.base = v;
        self
    }
    fn ext(mut self, v: u64) -> Self {
        self.ext = v;
        self
    }
    fn eof(mut self, v: u64) -> Self {
        self.eof = v;
        self
    }
    fn root(mut self, v: u64) -> Self {
        self.root = v;
        self
    }

    /// Serialise to bytes, appending a valid Jenkins checksum at the end.
    fn build(self) -> Vec<u8> {
        let sig = b"\x89HDF\r\n\x1a\n";
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(sig);
        bytes.push(self.version);
        bytes.push(self.soo);
        bytes.push(self.sol);
        bytes.push(self.flags);
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

// ─────────────────────────────────────────────────────────────────────────────
// Superblock V2 parsing tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_superblock_v2_parse_synthetic_8byte_offsets() {
    let bytes = SuperblockBuilder::new()
        .version(2)
        .soo(8)
        .sol(8)
        .flags(0)
        .base(16)
        .ext(u64::MAX)
        .eof(65536)
        .root(2048)
        .build();

    let mut cursor = Cursor::new(&bytes[9..]);
    let mut header_so_far = bytes[..9].to_vec();

    let v2 = read_superblock_v2(&mut cursor, &mut header_so_far)
        .expect("V2 superblock with 8-byte offsets must parse successfully");

    assert_eq!(v2.size_of_offsets, 8);
    assert_eq!(v2.size_of_lengths, 8);
    assert_eq!(v2.flags, 0);
    assert_eq!(v2.base_address, 16);
    assert_eq!(v2.superblock_extension_address, u64::MAX);
    assert_eq!(v2.end_of_file_address, 65536);
    assert_eq!(v2.root_group_object_header_address, 2048);

    validate_superblock_checksum(&header_so_far)
        .expect("checksum must be valid for a correctly built superblock");
}

#[test]
fn test_superblock_v2_checksum_corruption_detected() {
    let mut bytes = SuperblockBuilder::new()
        .version(2)
        .soo(8)
        .sol(8)
        .flags(0)
        .base(0)
        .ext(u64::MAX)
        .eof(1024)
        .root(256)
        .build();
    // Corrupt a byte in the payload (before the checksum).
    bytes[12] ^= 0xFF;
    let result = validate_superblock_checksum(&bytes);
    assert!(result.is_err(), "corrupted superblock must fail checksum");
}

#[test]
fn test_superblock_v2_4byte_offsets() {
    let bytes = SuperblockBuilder::new()
        .version(2)
        .soo(4)
        .sol(4)
        .flags(0)
        .base(16)
        .ext(u32::MAX as u64)
        .eof(4096)
        .root(512)
        .build();

    let mut cursor = Cursor::new(&bytes[9..]);
    let mut header_so_far = bytes[..9].to_vec();
    let v2 = read_superblock_v2(&mut cursor, &mut header_so_far)
        .expect("V2 superblock with 4-byte offsets must parse successfully");

    assert_eq!(v2.size_of_offsets, 4);
    assert_eq!(v2.root_group_object_header_address, 512);
}

#[test]
fn test_superblock_v3_same_layout_as_v2() {
    // V3 has an identical binary layout to V2; only the version byte differs.
    let bytes = SuperblockBuilder::new()
        .version(3)
        .soo(8)
        .sol(8)
        .flags(0b0011)
        .base(0)
        .ext(u64::MAX)
        .eof(100_000)
        .root(1_000)
        .build();

    let mut cursor = Cursor::new(&bytes[9..]);
    let mut header_so_far = bytes[..9].to_vec();
    let v3 = read_superblock_v2(&mut cursor, &mut header_so_far)
        .expect("V3 superblock must parse with same logic as V2");

    assert_eq!(v3.flags, 0b0011);
    assert_eq!(v3.root_group_object_header_address, 1_000);

    validate_superblock_checksum(&header_so_far).expect("V3 checksum must be valid");
}

// ─────────────────────────────────────────────────────────────────────────────
// End-to-end reader dispatch test
// ─────────────────────────────────────────────────────────────────────────────

/// Verify that `Hdf5Reader::open` no longer returns the old
/// "requires hdf5_sys feature" stub error for V2 superblocks.
///
/// The reader may still fail at a later stage (e.g. when following the root
/// group object header address), but the specific stub error must be gone.
#[test]
fn test_superblock_v2_reader_dispatch_no_longer_errors_with_hdf5_sys_stub() {
    use oxigeo_hdf5::Hdf5Reader;

    let sig = b"\x89HDF\r\n\x1a\n";
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(sig);
    bytes.push(2u8); // version 2
    bytes.push(8u8); // size_of_offsets = 8
    bytes.push(8u8); // size_of_lengths = 8
    bytes.push(0u8); // flags

    // base_address = 0
    bytes.extend_from_slice(&0u64.to_le_bytes());
    // superblock_extension_address = undefined (all-ones)
    bytes.extend_from_slice(&u64::MAX.to_le_bytes());
    // end_of_file_address placeholder — we patch it after computing total size
    let eof_offset = bytes.len();
    bytes.extend_from_slice(&0u64.to_le_bytes());
    // root_group_object_header_address = 0 (invalid but we won't reach it)
    bytes.extend_from_slice(&0u64.to_le_bytes());

    // Patch eof to total size (current length + 4 bytes for checksum)
    let total_size = (bytes.len() + 4) as u64;
    bytes[eof_offset..eof_offset + 8].copy_from_slice(&total_size.to_le_bytes());

    // Append correct checksum
    let checksum = jenkins_lookup3_hashlittle(&bytes, 0);
    bytes.extend_from_slice(&checksum.to_le_bytes());

    let tmp = std::env::temp_dir().join("oxigeo_test_hdf5_v2_superblock_dispatch.h5");
    std::fs::write(&tmp, &bytes).expect("should be able to write temp file");

    match Hdf5Reader::open(&tmp) {
        Ok(_) => { /* success is also acceptable */ }
        Err(e) => {
            let msg = format!("{e:?}");
            assert!(
                !msg.contains("requires hdf5_sys feature"),
                "reader must not return the old hdf5_sys stub error; got: {msg}"
            );
        }
    }

    let _ = std::fs::remove_file(&tmp);
}
