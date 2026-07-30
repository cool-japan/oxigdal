//! cool-japan/oxigeo#14 — the block I/O path of `CogReader`.
//!
//! The last full-band-sized allocation the profiling run found sat *below* the
//! driver: `DataSource::read_range` returns an owned `Vec`, so `read_tile_raw`
//! allocated one buffer per tile/strip before `read_tile_into` had decoded a
//! single byte. The reader now goes through [`DataSource::read_range_into`] (and
//! [`DataSource::range_slice`] where the source can lend its bytes), so no block
//! buffer is allocated at all.
//!
//! These tests assert *which* entry point the reader uses, and that the bytes it
//! produces are unchanged. Allocation counts live in
//! `issue_14_zero_alloc_blocks.rs`, which needs a global allocator to itself;
//! timings here are only ever printed, never asserted.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use oxigeo_core::error::{OxiGeoError, Result};
use oxigeo_core::io::{ByteRange, DataSource};
use oxigeo_geotiff::cog::CogReader;
use oxigeo_geotiff::compression;
use oxigeo_geotiff::tiff::{Compression, Predictor, TiffTag};

// ---------------------------------------------------------------------------
// A data source that records which entry point the reader used
// ---------------------------------------------------------------------------

/// One counter per [`DataSource`] entry point.
#[derive(Debug, Default)]
struct Dispatch {
    read_range: AtomicUsize,
    read_range_into: AtomicUsize,
    range_slice: AtomicUsize,
}

impl Dispatch {
    fn snapshot(&self) -> (usize, usize, usize) {
        (
            self.read_range.load(Ordering::Relaxed),
            self.read_range_into.load(Ordering::Relaxed),
            self.range_slice.load(Ordering::Relaxed),
        )
    }
}

/// In-memory source that tallies every call, and optionally lends its bytes out
/// through [`DataSource::range_slice`] the way a memory-mapped file does.
#[derive(Debug)]
struct DispatchSource {
    data: Vec<u8>,
    lends: bool,
    dispatch: Arc<Dispatch>,
}

impl DispatchSource {
    fn new(data: Vec<u8>, lends: bool) -> (Self, Arc<Dispatch>) {
        let dispatch = Arc::new(Dispatch::default());
        (
            Self {
                data,
                lends,
                dispatch: Arc::clone(&dispatch),
            },
            dispatch,
        )
    }

    fn slice(&self, range: ByteRange) -> Result<&[u8]> {
        let start = range.start as usize;
        let end = range.end as usize;
        if start > end || end > self.data.len() {
            return Err(OxiGeoError::OutOfBounds {
                message: format!(
                    "range {start}..{end} outside {}-byte source",
                    self.data.len()
                ),
            });
        }
        Ok(&self.data[start..end])
    }
}

impl DataSource for DispatchSource {
    fn size(&self) -> Result<u64> {
        Ok(self.data.len() as u64)
    }

    fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
        self.dispatch.read_range.fetch_add(1, Ordering::Relaxed);
        Ok(self.slice(range)?.to_vec())
    }

    fn read_range_into(&self, range: ByteRange, dst: &mut [u8]) -> Result<usize> {
        self.dispatch
            .read_range_into
            .fetch_add(1, Ordering::Relaxed);
        let needed = (range.end - range.start) as usize;
        if dst.len() < needed {
            return Err(OxiGeoError::invalid_parameter(
                "dst",
                format!("destination buffer is {} bytes, need {needed}", dst.len()),
            ));
        }
        let src = self.slice(range)?;
        dst[..src.len()].copy_from_slice(src);
        Ok(src.len())
    }

    fn range_slice(&self, range: ByteRange) -> Option<&[u8]> {
        if !self.lends {
            return None;
        }
        self.dispatch.range_slice.fetch_add(1, Ordering::Relaxed);
        self.slice(range).ok()
    }
}

// ---------------------------------------------------------------------------
// Synthetic striped Float32 TIFF
// ---------------------------------------------------------------------------

const ENTRY_COUNT: u32 = 10;
const IFD_OFFSET: u32 = 8;
/// 2-byte entry count + 12 bytes per entry + 4-byte next-IFD pointer.
const IFD_SIZE: u32 = 2 + ENTRY_COUNT * 12 + 4;

/// Builds a classic little-endian TIFF with `strips` single-row Float32 strips
/// encoded with `compression`, plus the decoded pixel bytes for cross-checking.
///
/// `Compression::None` exercises the direct-to-destination fast path;
/// `Compression::Packbits` (always available, no cargo feature needed) exercises
/// the borrowed / scratch tiers, where the block bytes and the decoded bytes are
/// genuinely different buffers.
fn build_striped_float_tiff(
    strips: u32,
    width: u32,
    compression: Compression,
) -> (Vec<u8>, Vec<u8>) {
    let strip_bytes = width * 4;

    // Decoded pixels first: the block payloads are derived from them.
    let mut pixels = Vec::with_capacity((strips * strip_bytes) as usize);
    for row in 0..strips {
        for col in 0..width {
            let value = row as f32 * 0.5 - col as f32 * 0.25;
            pixels.extend_from_slice(&value.to_le_bytes());
        }
    }

    let blocks: Vec<Vec<u8>> = (0..strips)
        .map(|row| {
            let start = (row * strip_bytes) as usize;
            let end = start + strip_bytes as usize;
            compression::compress(&pixels[start..end], compression).expect("encode strip")
        })
        .collect();

    let offsets_pos = IFD_OFFSET + IFD_SIZE;
    let counts_pos = offsets_pos + strips * 4;
    let data_pos = counts_pos + strips * 4;
    let mut offsets = Vec::with_capacity(strips as usize);
    let mut cursor = data_pos;
    for block in &blocks {
        offsets.push(cursor);
        cursor += block.len() as u32;
    }

    let mut out = Vec::with_capacity(cursor as usize);
    out.extend_from_slice(b"II");
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&IFD_OFFSET.to_le_bytes());

    out.extend_from_slice(&(ENTRY_COUNT as u16).to_le_bytes());
    let mut entry = |tag: TiffTag, field_type: u16, count: u32, value: u32| {
        out.extend_from_slice(&(tag as u16).to_le_bytes());
        out.extend_from_slice(&field_type.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&value.to_le_bytes());
    };
    const SHORT: u16 = 3;
    const LONG: u16 = 4;
    entry(TiffTag::ImageWidth, LONG, 1, width);
    entry(TiffTag::ImageLength, LONG, 1, strips);
    entry(TiffTag::BitsPerSample, SHORT, 1, 32);
    entry(TiffTag::Compression, SHORT, 1, compression as u32);
    entry(TiffTag::PhotometricInterpretation, SHORT, 1, 1);
    entry(TiffTag::StripOffsets, LONG, strips, offsets_pos);
    entry(TiffTag::SamplesPerPixel, SHORT, 1, 1);
    entry(TiffTag::RowsPerStrip, LONG, 1, 1);
    entry(TiffTag::StripByteCounts, LONG, strips, counts_pos);
    entry(TiffTag::SampleFormat, SHORT, 1, 3); // IEEE float
    out.extend_from_slice(&0u32.to_le_bytes()); // no next IFD

    for offset in &offsets {
        out.extend_from_slice(&offset.to_le_bytes());
    }
    for block in &blocks {
        out.extend_from_slice(&(block.len() as u32).to_le_bytes());
    }
    for block in &blocks {
        out.extend_from_slice(block);
    }

    (out, pixels)
}

// ---------------------------------------------------------------------------
// Dispatch assertions
// ---------------------------------------------------------------------------

/// A whole-band walk must issue exactly one `read_range_into` per block and
/// **zero** `read_range` calls: the owning entry point, and therefore the
/// per-block allocation it implies, is off the read path — for uncompressed
/// blocks (read straight into the caller's buffer) and for compressed ones
/// (staged through the reader's reusable scratch) alike.
#[test]
fn test_issue_14_read_tile_into_uses_read_range_into() {
    let strips = 256u32;
    let width = 64u32;

    for compression in [Compression::None, Compression::Packbits] {
        let (bytes, pixels) = build_striped_float_tiff(strips, width, compression);
        let (source, dispatch) = DispatchSource::new(bytes, false);

        let reader = CogReader::open(source).expect("open synthetic COG");
        let (open_range, open_into, _) = dispatch.snapshot();

        let mut scratch = vec![0u8; reader.tile_decoded_size(0, 0).expect("decoded size")];
        let mut band = Vec::with_capacity(pixels.len());
        for y in 0..strips {
            reader
                .read_tile_into(0, 0, y, &mut scratch)
                .expect("read_tile_into");
            band.extend_from_slice(&scratch);
        }

        let (range_calls, into_calls, slice_calls) = dispatch.snapshot();
        let range_calls = range_calls - open_range;
        let into_calls = into_calls - open_into;

        assert_eq!(
            range_calls, 0,
            "{compression:?}: read_tile_into must not use the allocating read_range \
             (did {range_calls} calls for {strips} strips)"
        );
        assert_eq!(
            into_calls, strips as usize,
            "{compression:?}: read_tile_into must issue exactly one read_range_into per block"
        );
        assert_eq!(
            slice_calls, 0,
            "{compression:?}: a source that does not lend its bytes must never be asked to"
        );
        assert_eq!(
            band, pixels,
            "{compression:?}: the band must be reconstructed byte for byte"
        );
    }
}

/// A source that can lend its bytes (memory-mapped, in-memory) must be taken up
/// on it whenever the block has to be decoded from a separate buffer: the reader
/// then neither allocates nor copies the compressed block.
#[test]
fn test_issue_14_lending_source_takes_the_zero_copy_path() {
    let strips = 128u32;
    let width = 48u32;
    // PackBits keeps the block bytes distinct from the decoded bytes, so both
    // entry points must go through the borrowing tier. (For `Compression::None`
    // they take the even shorter direct-to-destination path instead, which is
    // covered by the test above.)
    let (bytes, pixels) = build_striped_float_tiff(strips, width, Compression::Packbits);
    let (source, dispatch) = DispatchSource::new(bytes, true);

    let reader = CogReader::open(source).expect("open synthetic COG");
    let (open_range, open_into, open_slice) = dispatch.snapshot();

    let mut scratch = vec![0u8; reader.tile_decoded_size(0, 0).expect("decoded size")];
    for y in 0..strips {
        let owned = reader
            .read_tile(0, 0, y)
            .expect("read_tile must work on a lending source");
        reader
            .read_tile_into(0, 0, y, &mut scratch)
            .expect("read_tile_into");
        let expected = &pixels[(y * width * 4) as usize..((y + 1) * width * 4) as usize];
        assert_eq!(owned, expected, "read_tile decoded strip {y} incorrectly");
        assert_eq!(
            &scratch[..],
            expected,
            "read_tile_into decoded strip {y} incorrectly through the zero-copy path"
        );
    }

    let (range_calls, into_calls, slice_calls) = dispatch.snapshot();
    assert_eq!(
        range_calls - open_range,
        0,
        "a lending source must never fall back to the allocating read_range"
    );
    assert_eq!(
        into_calls - open_into,
        0,
        "a lending source must not be asked to copy either — borrowing is cheaper"
    );
    assert_eq!(
        slice_calls - open_slice,
        2 * strips as usize,
        "both read_tile and read_tile_into must borrow, once per block"
    );
}

/// `read_tile` and `read_tile_into` must agree with a byte-for-byte emulation of
/// the pre-fix path (`read_range` + `decompress` + predictor), for a lending and
/// a non-lending source alike.
#[test]
fn test_issue_14_block_io_is_bit_identical_to_the_pre_fix_path() {
    let strips = 64u32;
    let width = 32u32;

    for compression in [Compression::None, Compression::Packbits] {
        let (bytes, _) = build_striped_float_tiff(strips, width, compression);
        for lends in [false, true] {
            let (source, _) = DispatchSource::new(bytes.clone(), lends);
            let reader = CogReader::open(source).expect("open synthetic COG");
            let mut scratch = vec![0xABu8; reader.tile_decoded_size(0, 0).expect("decoded size")];

            // The pre-fix reader, transcribed: own the block, then decode it.
            let (reference_source, _) = DispatchSource::new(bytes.clone(), lends);

            for y in 0..strips {
                let range = reader.tile_byte_range(0, 0, y).expect("byte range");
                let raw = reference_source.read_range(range).expect("read_range");
                let expected_size = reader.tile_decoded_size(0, y).expect("decoded size");
                let mut reference =
                    compression::decompress(&raw, compression, expected_size).expect("decompress");
                compression::apply_predictor_reverse(
                    &mut reference,
                    Predictor::None,
                    4,
                    1,
                    width as usize,
                    oxigeo_geotiff::tiff::ByteOrderType::LittleEndian,
                )
                .expect("predictor");

                let via_read_tile = reader.read_tile(0, 0, y).expect("read_tile");
                scratch.fill(0xAB);
                reader
                    .read_tile_into(0, 0, y, &mut scratch)
                    .expect("read_tile_into");

                assert_eq!(
                    via_read_tile, reference,
                    "{compression:?}/lends={lends}: read_tile differs at strip {y}"
                );
                assert_eq!(
                    scratch, reference,
                    "{compression:?}/lends={lends}: read_tile_into differs at strip {y}"
                );
            }
        }
    }
}

/// Reading the same reader from several threads at once must stay correct even
/// though they contend for the one scratch buffer (the loser allocates privately).
#[test]
fn test_issue_14_concurrent_read_tile_into_is_correct() {
    let strips = 128u32;
    let width = 64u32;
    let (bytes, pixels) = build_striped_float_tiff(strips, width, Compression::Packbits);
    let (source, _) = DispatchSource::new(bytes, false);
    let reader = Arc::new(CogReader::open(source).expect("open synthetic COG"));
    let pixels = Arc::new(pixels);

    let mut handles = Vec::new();
    for t in 0..4u32 {
        let reader = Arc::clone(&reader);
        let pixels = Arc::clone(&pixels);
        handles.push(std::thread::spawn(move || {
            let mut scratch = vec![0u8; reader.tile_decoded_size(0, 0).expect("decoded size")];
            for round in 0..strips {
                let y = (round + t * 17) % strips;
                reader
                    .read_tile_into(0, 0, y, &mut scratch)
                    .expect("read_tile_into");
                let start = (y * width * 4) as usize;
                assert_eq!(
                    &scratch[..],
                    &pixels[start..start + (width * 4) as usize],
                    "thread {t} decoded strip {y} incorrectly"
                );
            }
        }));
    }
    for handle in handles {
        handle.join().expect("worker thread panicked");
    }
}

/// Evidence, not an assertion: wall-clock for a whole band of a many-block file,
/// old path versus new.
#[test]
fn test_issue_14_block_io_speed_evidence() {
    let strips = 4096u32;
    let width = 1024u32;
    let (bytes, pixels) = build_striped_float_tiff(strips, width, Compression::None);
    let mib = pixels.len() as f64 / (1024.0 * 1024.0);

    let (source, _) = DispatchSource::new(bytes.clone(), false);
    let reader = CogReader::open(source).expect("open synthetic COG");
    let block = reader.tile_decoded_size(0, 0).expect("decoded size");

    // Pre-fix: one owned `Vec` for the raw block, one for the decoded block.
    let (reference_source, _) = DispatchSource::new(bytes.clone(), false);
    let mut band = vec![0u8; pixels.len()];
    let mut pre_fix = f64::MAX;
    for _ in 0..3 {
        let start = Instant::now();
        for y in 0..strips {
            let range = reader.tile_byte_range(0, 0, y).expect("byte range");
            let raw = reference_source.read_range(range).expect("read_range");
            let decoded = compression::decompress(&raw, Compression::None, block).expect("decode");
            let offset = y as usize * block;
            band[offset..offset + block].copy_from_slice(&decoded);
        }
        pre_fix = pre_fix.min(start.elapsed().as_secs_f64());
    }
    assert_eq!(
        band, pixels,
        "the pre-fix emulation must reconstruct the band"
    );

    // Post-fix: read straight into the band buffer, no per-block buffer at all.
    let mut post_fix = f64::MAX;
    for _ in 0..3 {
        band.fill(0);
        let start = Instant::now();
        for y in 0..strips {
            let offset = y as usize * block;
            reader
                .read_tile_into(0, 0, y, &mut band[offset..offset + block])
                .expect("read_tile_into");
        }
        post_fix = post_fix.min(start.elapsed().as_secs_f64());
    }
    assert_eq!(band, pixels, "the post-fix path must reconstruct the band");

    eprintln!(
        "issue#14 band read, {strips} strips / {mib:.1} MiB: \
         pre-fix {:.2} ms ({:.0} MiB/s)  post-fix {:.2} ms ({:.0} MiB/s)  ({:.2}x)",
        pre_fix * 1e3,
        mib / pre_fix,
        post_fix * 1e3,
        mib / post_fix,
        pre_fix / post_fix.max(f64::EPSILON)
    );
}
