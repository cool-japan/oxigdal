//! cool-japan/oxigeo#14 — hard evidence that a whole-band GeoTIFF read no longer
//! allocates per block.
//!
//! Profiling issue #14 counted three full-band-sized allocations between the file
//! bytes and the user's array. The last one lived in the I/O layer:
//! `DataSource::read_range` returns an owned `Vec`, so `CogReader::read_tile_raw`
//! allocated one buffer per tile/strip. With
//! [`DataSource::read_range_into`](oxigeo_core::io::DataSource::read_range_into)
//! the reader writes into buffers that already exist, and the count reaches zero.
//!
//! # Why this file holds exactly one test
//!
//! The counter is process-wide. `cargo nextest` gives each test its own process,
//! but the built-in `cargo test` harness runs a file's tests on parallel threads,
//! where a second test allocating concurrently would corrupt the measurement.
//! One test per file makes it exact under both runners.
//!
//! # Unsafe
//!
//! `GlobalAlloc` is an unsafe trait, so a counting allocator cannot avoid
//! `unsafe`. Every method forwards its arguments unchanged to
//! [`std::alloc::System`] — the allocator that would have served the request
//! anyway — and adds nothing but a relaxed atomic increment. No pointer or layout
//! is synthesised, adjusted or interpreted here.

#![allow(unsafe_code)]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use oxigeo_core::error::{OxiGeoError, Result};
use oxigeo_core::io::{ByteRange, DataSource};
use oxigeo_geotiff::cog::CogReader;
use oxigeo_geotiff::compression;
use oxigeo_geotiff::tiff::{Compression, TiffTag};

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

/// Forwarding allocator that counts every allocating call.
struct CountingAllocator;

// SAFETY: every method delegates verbatim to `System`, which already upholds the
// `GlobalAlloc` contract for these exact arguments; the wrapper only bumps a
// relaxed counter, which neither allocates nor panics.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator;

// ---------------------------------------------------------------------------
// Plain in-memory source: no `range_slice`, so the reader must go through
// `read_range_into` exactly as it would for a real file.
// ---------------------------------------------------------------------------

/// In-memory source that deliberately declines the zero-copy path, so the
/// measurement below reflects the file-backed behaviour rather than borrowing.
#[derive(Debug)]
struct PlainSource(Vec<u8>);

impl DataSource for PlainSource {
    fn size(&self) -> Result<u64> {
        Ok(self.0.len() as u64)
    }

    fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
        Ok(self.slice(range)?.to_vec())
    }

    fn read_range_into(&self, range: ByteRange, dst: &mut [u8]) -> Result<usize> {
        let src = self.slice(range)?;
        if dst.len() < src.len() {
            return Err(OxiGeoError::invalid_parameter(
                "dst",
                "destination too small",
            ));
        }
        dst[..src.len()].copy_from_slice(src);
        Ok(src.len())
    }
}

impl PlainSource {
    fn slice(&self, range: ByteRange) -> Result<&[u8]> {
        let start = range.start as usize;
        let end = range.end as usize;
        if start > end || end > self.0.len() {
            return Err(OxiGeoError::OutOfBounds {
                message: format!("range {start}..{end} outside {}-byte source", self.0.len()),
            });
        }
        Ok(&self.0[start..end])
    }
}

// ---------------------------------------------------------------------------
// Synthetic striped Float32 TIFF
// ---------------------------------------------------------------------------

const ENTRY_COUNT: u32 = 10;
const IFD_OFFSET: u32 = 8;
const IFD_SIZE: u32 = 2 + ENTRY_COUNT * 12 + 4;

/// Builds a classic little-endian TIFF with `strips` single-row Float32 strips.
fn build_striped_float_tiff(strips: u32, width: u32) -> Vec<u8> {
    let strip_bytes = width * 4;
    let offsets_pos = IFD_OFFSET + IFD_SIZE;
    let counts_pos = offsets_pos + strips * 4;
    let data_pos = counts_pos + strips * 4;

    let mut out = Vec::with_capacity((data_pos + strips * strip_bytes) as usize);
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
    entry(TiffTag::Compression, SHORT, 1, Compression::None as u32);
    entry(TiffTag::PhotometricInterpretation, SHORT, 1, 1);
    entry(TiffTag::StripOffsets, LONG, strips, offsets_pos);
    entry(TiffTag::SamplesPerPixel, SHORT, 1, 1);
    entry(TiffTag::RowsPerStrip, LONG, 1, 1);
    entry(TiffTag::StripByteCounts, LONG, strips, counts_pos);
    entry(TiffTag::SampleFormat, SHORT, 1, 3);
    out.extend_from_slice(&0u32.to_le_bytes());

    for i in 0..strips {
        out.extend_from_slice(&(data_pos + i * strip_bytes).to_le_bytes());
    }
    for _ in 0..strips {
        out.extend_from_slice(&strip_bytes.to_le_bytes());
    }
    for row in 0..strips {
        for col in 0..width {
            out.extend_from_slice(&(row as f32 * 0.5 - col as f32 * 0.25).to_le_bytes());
        }
    }
    out
}

const STRIPS: u32 = 1024;
const WIDTH: u32 = 256;

#[test]
fn test_issue_14_whole_band_read_allocates_nothing_per_block() {
    // ---- setup, outside every measured region ----------------------------
    let bytes = build_striped_float_tiff(STRIPS, WIDTH);
    let reader = CogReader::open(PlainSource(bytes.clone())).expect("open synthetic COG");
    let block = reader.tile_decoded_size(0, 0).expect("decoded size");
    let mut band = vec![0u8; block * STRIPS as usize];

    // Warm-up walk: the reader's scratch buffer grows to the largest block here,
    // once, so the measured walk sees the steady state a real band read is in
    // after its first block.
    for y in 0..STRIPS {
        let offset = y as usize * block;
        reader
            .read_tile_into(0, 0, y, &mut band[offset..offset + block])
            .expect("warm-up read_tile_into");
    }
    let warm_band = band.clone();

    // ---- measured region 1: the whole band through read_tile_into --------
    band.fill(0);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    for y in 0..STRIPS {
        let offset = y as usize * block;
        reader
            .read_tile_into(0, 0, y, &mut band[offset..offset + block])
            .expect("read_tile_into");
    }
    let post_fix_allocs = ALLOCATIONS.load(Ordering::Relaxed) - before;
    let post_fix_band_ok = band == warm_band;

    // ---- measured region 2: the pre-fix path, transcribed -----------------
    // `read_tile_raw` (an owned `Vec` per block) followed by a decode into the
    // band buffer — exactly what the driver did before this change.
    let reference = CogReader::open(PlainSource(bytes)).expect("open synthetic COG");
    band.fill(0);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    for y in 0..STRIPS {
        let offset = y as usize * block;
        let raw = reference.read_tile_raw(0, 0, y).expect("read_tile_raw");
        compression::decompress_into_partial(
            &raw,
            Compression::None,
            &mut band[offset..offset + block],
        )
        .expect("decompress_into_partial");
    }
    let pre_fix_allocs = ALLOCATIONS.load(Ordering::Relaxed) - before;
    let pre_fix_band_ok = band == warm_band;

    // ---- assertions (allocating freely again) ----------------------------
    assert!(
        post_fix_band_ok,
        "the measured walk must reconstruct the band"
    );
    assert!(pre_fix_band_ok, "the pre-fix emulation must agree with it");

    assert_eq!(
        post_fix_allocs, 0,
        "a {STRIPS}-strip band read through read_tile_into must allocate nothing, \
         but it made {post_fix_allocs} allocations"
    );
    assert!(
        pre_fix_allocs >= STRIPS as usize,
        "the pre-fix path is expected to allocate at least once per block \
         ({STRIPS} strips made {pre_fix_allocs} allocations) — if this ever reaches \
         0 the assertion above has stopped proving anything"
    );

    eprintln!(
        "issue#14 allocations for a {STRIPS}-strip band read: \
         pre-fix {pre_fix_allocs} -> post-fix {post_fix_allocs}"
    );
}
