//! cool-japan/oxigeo#14 — hard evidence that `read_band_into` allocates a fixed
//! number of buffers, independent of the raster size and of the block count.
//!
//! The pre-fix `read_band` paid, for a band of `B` blocks:
//!
//! * one **full-band-sized** zero-filled `Vec` (and it held the whole
//!   *interleaved* plane, i.e. `SamplesPerPixel ×` bigger than the band asked
//!   for), plus
//! * two `Vec`s per block (`read_range` for the compressed bytes, `decompress`
//!   for the decoded ones),
//!
//! and every caller then paid a second full-band `Vec` to de-interleave the band
//! out and a third to convert it to `f64`. The engine in `band_read` decodes into
//! the caller's own buffer through one reusable scratch tile, so the count drops
//! to a constant.
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
//! [`std::alloc::System`] and adds nothing but a relaxed atomic increment.

#![allow(unsafe_code)]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use oxigeo_core::error::{OxiGeoError, Result};
use oxigeo_core::io::{ByteRange, DataSource};
use oxigeo_geotiff::GeoTiffReader;
use oxigeo_geotiff::cog::CogReader;
use oxigeo_geotiff::tiff::TiffTag;

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
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// In-memory source that implements the allocation-free read entry point, so the
/// block loop can be measured without the `read_range` `Vec` masking the result.
struct SliceSource(Vec<u8>);

impl DataSource for SliceSource {
    fn size(&self) -> Result<u64> {
        Ok(self.0.len() as u64)
    }

    fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
        let start = range.start as usize;
        let end = (range.end as usize).min(self.0.len());
        if start > end {
            return Err(OxiGeoError::OutOfBounds {
                message: "invalid range".to_string(),
            });
        }
        Ok(self.0[start..end].to_vec())
    }

    fn read_range_into(&self, range: ByteRange, dst: &mut [u8]) -> Result<usize> {
        let start = range.start as usize;
        let end = (range.end as usize).min(self.0.len());
        if start > end {
            return Err(OxiGeoError::OutOfBounds {
                message: "invalid range".to_string(),
            });
        }
        let src = &self.0[start..end];
        let dst = dst
            .get_mut(..src.len())
            .ok_or_else(|| OxiGeoError::OutOfBounds {
                message: "destination too small".to_string(),
            })?;
        dst.copy_from_slice(src);
        Ok(src.len())
    }
}

const WIDTH: u32 = 256;
const ROWS_PER_STRIP: u32 = 4;
const STRIPS: u32 = 24;
const BANDS: u16 = 3;

/// Builds an uncompressed, chunky, 3-band UInt16 striped TIFF.
fn build_striped_rgb_tiff() -> Vec<u8> {
    let height = ROWS_PER_STRIP * STRIPS;
    let strip_bytes = WIDTH * ROWS_PER_STRIP * u32::from(BANDS) * 2;

    const SHORT: u16 = 3;
    const LONG: u16 = 4;
    let mut entries: Vec<(TiffTag, u16, u32, Vec<u8>)> = vec![
        (TiffTag::ImageWidth, LONG, 1, WIDTH.to_le_bytes().to_vec()),
        (TiffTag::ImageLength, LONG, 1, height.to_le_bytes().to_vec()),
        (
            TiffTag::BitsPerSample,
            SHORT,
            u32::from(BANDS),
            (0..BANDS).flat_map(|_| 16u16.to_le_bytes()).collect(),
        ),
        (TiffTag::Compression, SHORT, 1, 1u16.to_le_bytes().to_vec()),
        (
            TiffTag::PhotometricInterpretation,
            SHORT,
            1,
            2u16.to_le_bytes().to_vec(),
        ),
        (
            TiffTag::StripOffsets,
            LONG,
            STRIPS,
            vec![0; STRIPS as usize * 4],
        ),
        (
            TiffTag::SamplesPerPixel,
            SHORT,
            1,
            BANDS.to_le_bytes().to_vec(),
        ),
        (
            TiffTag::RowsPerStrip,
            LONG,
            1,
            ROWS_PER_STRIP.to_le_bytes().to_vec(),
        ),
        (
            TiffTag::StripByteCounts,
            LONG,
            STRIPS,
            (0..STRIPS)
                .flat_map(|_| strip_bytes.to_le_bytes())
                .collect(),
        ),
        (
            TiffTag::PlanarConfiguration,
            SHORT,
            1,
            1u16.to_le_bytes().to_vec(),
        ),
        (TiffTag::SampleFormat, SHORT, 1, 1u16.to_le_bytes().to_vec()),
    ];
    entries.sort_by_key(|(tag, _, _, _)| *tag as u16);

    let ifd_offset = 8u32;
    let ifd_size = 2 + entries.len() as u32 * 12 + 4;
    let mut external: Vec<Option<u32>> = Vec::with_capacity(entries.len());
    let mut external_size = 0u32;
    for (_, _, _, payload) in &entries {
        if payload.len() <= 4 {
            external.push(None);
        } else {
            external.push(Some(ifd_offset + ifd_size + external_size));
            external_size += payload.len() as u32;
            external_size += external_size % 2;
        }
    }
    let data_start = ifd_offset + ifd_size + external_size;

    let offsets: Vec<u8> = (0..STRIPS)
        .flat_map(|i| (data_start + i * strip_bytes).to_le_bytes())
        .collect();
    for entry in entries.iter_mut() {
        if entry.0 == TiffTag::StripOffsets {
            entry.3 = offsets.clone();
        }
    }

    let mut out = Vec::with_capacity((data_start + STRIPS * strip_bytes) as usize);
    out.extend_from_slice(b"II");
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&ifd_offset.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (index, (tag, field_type, count, payload)) in entries.iter().enumerate() {
        out.extend_from_slice(&(*tag as u16).to_le_bytes());
        out.extend_from_slice(&field_type.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        match external[index] {
            Some(offset) => out.extend_from_slice(&offset.to_le_bytes()),
            None => {
                let mut inline = [0u8; 4];
                inline[..payload.len()].copy_from_slice(payload);
                out.extend_from_slice(&inline);
            }
        }
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    for (index, (_, _, _, payload)) in entries.iter().enumerate() {
        if external[index].is_some() {
            out.extend_from_slice(payload);
            if out.len() % 2 != 0 {
                out.push(0);
            }
        }
    }
    assert_eq!(out.len() as u32, data_start);
    for i in 0..(STRIPS * strip_bytes) / 2 {
        out.extend_from_slice(&((i % 65_536) as u16).to_le_bytes());
    }
    out
}

/// `read_band_into` must allocate a constant number of buffers — never one per
/// block and never a band-sized one.
#[test]
fn test_issue_14_read_band_into_allocates_a_constant_number_of_buffers() {
    let bytes = build_striped_rgb_tiff();
    let height = (ROWS_PER_STRIP * STRIPS) as usize;
    let band_bytes = WIDTH as usize * height * 2;
    // Below the parallel threshold, so the serial path is measured under every
    // feature combination.
    assert!(band_bytes < 1 << 20, "keep the fixture on the serial path");

    let reader = GeoTiffReader::open(SliceSource(bytes.clone())).expect("open");
    let cog = CogReader::open(SliceSource(bytes)).expect("open CogReader");

    let mut dst = vec![0u8; band_bytes];
    // Warm-up: force any lazily-built internal state before measuring.
    reader.read_band_into(0, 1, &mut dst).expect("warm-up");
    let reference = dst.clone();

    // ---- measured region 1: the post-fix path ----------------------------
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    reader
        .read_band_into(0, 1, &mut dst)
        .expect("read_band_into");
    let post_fix = ALLOCATIONS.load(Ordering::Relaxed) - before;

    // ---- measured region 2: what the pre-fix code did --------------------
    // One interleaved band-sized Vec, then an owned `read_tile` Vec per block,
    // then the de-interleave buffer every caller had to add on top. (The
    // original code allocated *two* Vecs per block; `read_tile` now reuses an
    // internal compressed-bytes buffer, so this emulation measures the already
    // improved version and is a lower bound on the real pre-fix cost.)
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let interleaved_bytes = band_bytes * BANDS as usize;
    let mut interleaved = vec![0u8; interleaved_bytes];
    let row_bytes = WIDTH as usize * BANDS as usize * 2;
    for strip in 0..STRIPS {
        let block = cog.read_tile(0, 0, strip).expect("read_tile");
        let offset = strip as usize * ROWS_PER_STRIP as usize * row_bytes;
        interleaved[offset..offset + block.len()].copy_from_slice(&block);
    }
    let mut deinterleaved = vec![0u8; band_bytes];
    for pixel in 0..(WIDTH as usize * height) {
        let from = (pixel * BANDS as usize + 1) * 2;
        deinterleaved[pixel * 2..pixel * 2 + 2].copy_from_slice(&interleaved[from..from + 2]);
    }
    let pre_fix = ALLOCATIONS.load(Ordering::Relaxed) - before;

    // ---- assertions (allocating freely again) ----------------------------
    assert_eq!(
        deinterleaved, reference,
        "the pre-fix emulation must agree with read_band_into"
    );
    eprintln!(
        "issue#14 whole-band read of a {WIDTH}x{height} 3-band UInt16 raster \
         ({STRIPS} strips): pre-fix {pre_fix} allocations, post-fix {post_fix}"
    );
    // scratch tile + de-interleave gather row; nothing per block, nothing
    // band-sized.
    assert!(
        post_fix <= 2,
        "read_band_into must allocate at most the scratch tile and the gather \
         row, but it made {post_fix} allocations"
    );
    assert!(
        pre_fix >= 2 + STRIPS as usize,
        "sanity: the pre-fix emulation should allocate two band-sized buffers \
         plus one per block, but it made only {pre_fix} allocations"
    );
}
