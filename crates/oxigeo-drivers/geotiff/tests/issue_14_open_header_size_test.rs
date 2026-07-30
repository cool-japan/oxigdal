//! cool-japan/oxigeo#14 — what `GeoTiffReader::open` does with a file too short
//! to hold a TIFF header.
//!
//! `TiffFile::parse` used to ask every source for exactly
//! `TiffHeader::BIGTIFF_HEADER_SIZE` (16) bytes before looking at any of them.
//! That is the *BigTIFF* header size; a classic TIFF header is
//! `TiffHeader::MIN_HEADER_SIZE` (8) bytes, and `TiffHeader::parse` is written
//! to handle every length from there up, with a specific error for each way a
//! header can fall short. Two things followed, neither of them intended:
//!
//! 1. **The verdict depended on the data source.** `FileDataSource` and
//!    `MmapDataSource` implement `read_range` as an exact read and fail when the
//!    range runs past the end; in-memory sources across the workspace clamp and
//!    return what they have. The same 8-byte file was therefore rejected before
//!    the header parser saw it through one source and handed to the header
//!    parser through another.
//! 2. **The error described the driver, not the file.** Every short file — 2
//!    bytes or 15 — came back as `Failed to read 16 bytes at offset 0: failed
//!    to fill whole buffer`, which names an internal fixed-size read. Nothing
//!    told the caller the file was truncated, and the three precise errors the
//!    header parser already had could never fire.
//!
//! `parse` now falls back to the source's actual length when — and only when —
//! the source is genuinely smaller than a BigTIFF header, so the header parser
//! renders the verdict. Truncated files still error, which is what this file
//! pins, along with the errors now being accurate and source-independent.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

use oxigeo_core::error::Result;
use oxigeo_core::io::{ByteRange, DataSource, FileDataSource};
use oxigeo_geotiff::GeoTiffReader;

/// A source that clamps out-of-range reads instead of failing them, like the
/// in-memory sources used throughout the workspace.
#[derive(Debug)]
struct ClampingSource(Vec<u8>);

impl DataSource for ClampingSource {
    fn size(&self) -> Result<u64> {
        Ok(self.0.len() as u64)
    }

    fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
        let start = (range.start as usize).min(self.0.len());
        let end = (range.end as usize).min(self.0.len()).max(start);
        Ok(self.0[start..end].to_vec())
    }
}

/// Opens `bytes` through a real file and returns the error text, if any.
///
/// The fixture leaf name embeds the process id and a monotonic counter, so no
/// two test binaries — nor two concurrent runs of this one — can ever land on
/// the same file.
fn open_as_file(name: &str, bytes: &[u8]) -> std::result::Result<(), String> {
    /// Removes the fixture on scope exit, including on an `expect` panic below.
    struct TempPath(std::path::PathBuf);

    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = TempPath(std::env::temp_dir().join(format!(
        "oxigeo_geotiff_open_header_{}_{seq}_{name}",
        std::process::id()
    )));
    {
        let mut file = std::fs::File::create(&path.0).expect("create fixture");
        file.write_all(bytes).expect("write fixture");
    }
    let source = FileDataSource::open(&path.0).expect("open fixture");
    match GeoTiffReader::open(source) {
        Ok(_) => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}

/// Opens `bytes` through a clamping in-memory source.
fn open_in_memory(bytes: &[u8]) -> std::result::Result<(), String> {
    match GeoTiffReader::open(ClampingSource(bytes.to_vec())) {
        Ok(_) => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}

/// A complete, valid 1x1 8-bit single-band TIFF: 123 bytes, so it is well past
/// every threshold this file is about and must open cleanly.
fn minimal_tiff() -> Vec<u8> {
    // 9 inline entries: 8 (header) + 2 (count) + 9*12 + 4 (next) = 122.
    const DATA_OFFSET: u32 = 122;
    const SHORT: u16 = 3;
    const LONG: u16 = 4;
    let entries: [(u16, u16, u32); 9] = [
        (256, LONG, 1),           // ImageWidth
        (257, LONG, 1),           // ImageLength
        (258, SHORT, 8),          // BitsPerSample
        (259, SHORT, 1),          // Compression = none
        (262, SHORT, 1),          // PhotometricInterpretation = BlackIsZero
        (273, LONG, DATA_OFFSET), // StripOffsets
        (277, SHORT, 1),          // SamplesPerPixel
        (278, LONG, 1),           // RowsPerStrip
        (279, LONG, 1),           // StripByteCounts
    ];

    let mut out = Vec::with_capacity(DATA_OFFSET as usize + 1);
    out.extend_from_slice(b"II");
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, field_type, value) in entries {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&field_type.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        // SHORT values sit in the first two bytes of the value field.
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(out.len() as u32, DATA_OFFSET);
    out.push(0x7f);
    out
}

/// Every file too short to be a TIFF is still rejected — through either kind of
/// data source, and with an error that names the file's problem rather than the
/// size of an internal read.
#[test]
fn test_issue_14_open_rejects_files_shorter_than_a_header() {
    // Two bytes: not even a byte-order marker's worth of context.
    let two = [0x49u8, 0x49];
    let file_err =
        open_as_file("2b.tif", &two).expect_err("a 2-byte file must not open as a GeoTIFF");
    let memory_err = open_in_memory(&two).expect_err("a 2-byte source must not open as a GeoTIFF");

    for (kind, err) in [("file", &file_err), ("memory", &memory_err)] {
        assert!(
            err.contains("header too small"),
            "{kind} source: a short file must be reported as a short header, got {err:?}"
        );
        assert!(
            !err.contains("Failed to read 16 bytes"),
            "{kind} source: the error must describe the file, not the driver's \
             fixed-size header read, got {err:?}"
        );
    }
    assert_eq!(
        file_err, memory_err,
        "the same truncated file must get the same verdict whether the source \
         clamps out-of-range reads or refuses them"
    );
}

/// A header-only stub parses as a header and then fails for the honest reason:
/// there is no IFD behind it. What must never happen is that it *opens*.
#[test]
fn test_issue_14_open_rejects_header_only_stubs() {
    // 8 bytes: a complete classic TIFF header pointing at an IFD that is not
    // there. This is the stub shape a downstream test used to write.
    let stub = [0x49u8, 0x49, 0x2a, 0x00, 0x08, 0x00, 0x00, 0x00];
    assert!(
        open_as_file("8b.tif", &stub).is_err(),
        "a header-only stub has no IFD and must not open"
    );
    assert!(
        open_in_memory(&stub).is_err(),
        "a header-only stub has no IFD and must not open from memory either"
    );

    // 15 bytes: header plus a zero-entry IFD, one byte short of the old floor.
    // The IFD parses and is empty, so the failure is a missing mandatory tag.
    let mut empty_ifd = stub.to_vec();
    empty_ifd.extend_from_slice(&[0u8; 7]);
    assert_eq!(empty_ifd.len(), 15);
    let err = open_as_file("15b.tif", &empty_ifd).expect_err("an image with no tags must not open");
    assert!(
        err.contains("ImageWidth"),
        "a tagless IFD must be reported as a missing tag, got {err:?}"
    );

    // BigTIFF declares 8-byte offsets and therefore genuinely needs all 16
    // header bytes; a truncated one must say so rather than be misread as a
    // classic TIFF.
    let big = [0x49u8, 0x49, 0x2b, 0x00, 0x08, 0x00, 0x00, 0x00, 0x10, 0x00];
    let err =
        open_as_file("bigtiff.tif", &big).expect_err("a truncated BigTIFF header must not open");
    assert!(
        err.contains("BigTIFF header too small"),
        "a truncated BigTIFF header must be named as such, got {err:?}"
    );
}

/// A header whose first-IFD offset points outside the file must **error, not
/// panic**.
///
/// This is independent of the header-size question — the files below are a full
/// 16 bytes, so the header read has always succeeded — but it is the same
/// underlying defect: through a clamping source `Ifd::parse` received a short
/// (often empty) buffer and indexed it directly, aborting inside `byteorder`
/// rather than reporting a truncated file. Every read in the IFD parser is
/// length-checked now.
#[test]
fn test_issue_14_open_rejects_ifd_offsets_outside_the_file() {
    for offset in [16u32, 4096, u32::MAX] {
        let mut bytes = vec![0x49u8, 0x49, 0x2a, 0x00];
        bytes.extend_from_slice(&offset.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 8]);
        assert_eq!(bytes.len(), 16);
        assert!(
            open_in_memory(&bytes).is_err(),
            "an IFD offset of {offset} outside the file must error"
        );
        assert!(
            open_as_file("bad_ifd.tif", &bytes).is_err(),
            "an IFD offset of {offset} outside the file must error through a file too"
        );
    }

    // An IFD that claims five entries but carries only part of one.
    let mut bytes = vec![0x49u8, 0x49, 0x2a, 0x00];
    bytes.extend_from_slice(&8u32.to_le_bytes());
    bytes.extend_from_slice(&5u16.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 6]);
    assert!(
        open_in_memory(&bytes).is_err(),
        "an IFD whose entry array is cut off must error"
    );
    assert!(
        open_as_file("short_ifd.tif", &bytes).is_err(),
        "an IFD whose entry array is cut off must error through a file too"
    );
}

/// The control: nothing above has made a real file harder to open.
#[test]
fn test_issue_14_open_accepts_a_minimal_valid_tiff() {
    let bytes = minimal_tiff();
    open_as_file("minimal.tif", &bytes).expect("a valid 1x1 TIFF must open");
    open_in_memory(&bytes).expect("a valid 1x1 TIFF must open from memory");

    let reader = GeoTiffReader::open(ClampingSource(bytes)).expect("open");
    assert_eq!(reader.width(), 1);
    assert_eq!(reader.height(), 1);
    assert_eq!(reader.band_count(), 1);
    assert_eq!(reader.read_band(0, 0).expect("read_band"), vec![0x7f]);
}
