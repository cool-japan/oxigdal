//! cool-japan/oxigeo#14 — the multi-band read allocates a fixed number of
//! buffers, independent of the raster size and of the block count.
//!
//! `read_bands_into_typed` decodes each block once and fans it out into every
//! requested slot, and it does that through three reusable scratch buffers — the
//! staged block group, one de-interleave row, and one lane buffer holding
//! `bands.len()` runs of converted samples. None of them is per block, none of
//! them is band-sized, and none of them grows with the raster's height. This
//! test pins that: the same read over a raster with three times as many strips
//! must allocate exactly as many times.
//!
//! It is the multi-band sibling of `issue_14_band_read_alloc_test.rs`, and it
//! lives in its own file for the same reason: the counter is process-wide, so a
//! second test allocating concurrently would corrupt the measurement.
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

/// In-memory source implementing the allocation-free read entry point, so the
/// block loop is measured without a `read_range` `Vec` masking the result.
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

const WIDTH: u32 = 128;
const ROWS_PER_STRIP: u32 = 4;
const BANDS: u16 = 3;

/// Builds an uncompressed, chunky, 3-band `UInt16` striped TIFF of `strips`
/// strips.
fn build_striped_rgb_tiff(strips: u32) -> Vec<u8> {
    let height = ROWS_PER_STRIP * strips;
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
            strips,
            vec![0; strips as usize * 4],
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
            strips,
            (0..strips)
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

    let offsets: Vec<u8> = (0..strips)
        .flat_map(|i| (data_start + i * strip_bytes).to_le_bytes())
        .collect();
    for entry in entries.iter_mut() {
        if entry.0 == TiffTag::StripOffsets {
            entry.3 = offsets.clone();
        }
    }

    let mut out = Vec::with_capacity((data_start + strips * strip_bytes) as usize);
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
            if !out.len().is_multiple_of(2) {
                out.push(0);
            }
        }
    }
    assert_eq!(out.len() as u32, data_start);
    for i in 0..(strips * strip_bytes) / 2 {
        out.extend_from_slice(&((i % 65_536) as u16).to_le_bytes());
    }
    out
}

/// Reads all three bands interleaved and returns `(pixels, allocations)`.
fn measure(strips: u32) -> (Vec<f64>, usize) {
    let bytes = build_striped_rgb_tiff(strips);
    let pixels = WIDTH as usize * (ROWS_PER_STRIP * strips) as usize;
    // Below the parallel threshold, so the serial path is measured under every
    // feature combination.
    assert!(pixels * 2 < 1 << 20, "keep the fixture on the serial path");

    let reader = GeoTiffReader::open(SliceSource(bytes)).expect("open");
    let mut dst = vec![0f64; pixels * 3];
    // Warm-up: force any lazily-built internal state before measuring.
    reader
        .read_bands_into_typed(0, &[2, 1, 0], &mut dst)
        .expect("warm-up");

    let before = ALLOCATIONS.load(Ordering::Relaxed);
    reader
        .read_bands_into_typed(0, &[2, 1, 0], &mut dst)
        .expect("read_bands_into_typed");
    let allocations = ALLOCATIONS.load(Ordering::Relaxed) - before;
    (dst, allocations)
}

/// `read_bands_into_typed` must allocate a constant number of buffers — never
/// one per block, never one per band, never a band-sized one.
#[test]
fn test_issue_14_read_bands_into_typed_allocates_a_constant_number_of_buffers() {
    let (small, small_allocs) = measure(8);
    let (large, large_allocs) = measure(24);

    eprintln!(
        "issue#14 interleaved 3-band read: 8 strips -> {small_allocs} allocations, \
         24 strips -> {large_allocs}"
    );
    assert_eq!(
        small_allocs, large_allocs,
        "tripling the block count must not change the allocation count \
         ({small_allocs} vs {large_allocs})"
    );
    // Staged block group + de-interleave row + lane buffer. Nothing else.
    assert!(
        large_allocs <= 3,
        "read_bands_into_typed must allocate at most its three scratch buffers, \
         but it made {large_allocs} allocations"
    );

    // A cheap sanity check that the reads actually produced something: the two
    // rasters share their first 8 strips' worth of pixels.
    let shared = small.len();
    assert_eq!(
        &small[..],
        &large[..shared],
        "the two fixtures must agree over their common region"
    );
    assert!(
        small.iter().any(|&v| v != 0.0),
        "the read produced only zeros"
    );
}
