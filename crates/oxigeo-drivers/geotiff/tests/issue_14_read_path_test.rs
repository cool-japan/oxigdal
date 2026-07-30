//! Regression tests for cool-japan/oxigeo#14 — GeoTIFF read-path performance.
//!
//! Issue #14 reports that reading a float32 GeoTIFF DEM is ~2.5x slower than the
//! C-GDAL wrapper. Profiling found three defects in the driver internals, each
//! covered here:
//!
//! 1. `CogReader::tile_byte_range` re-read *and* re-parsed the entire
//!    `TileOffsets`/`TileByteCounts` (or `Strip*`) arrays on every tile lookup —
//!    two `DataSource::read_range` calls plus two full `Vec<u64>` parses per
//!    block, i.e. O(n²) work for a whole-band read. It is now an O(1) lookup into
//!    an index parsed once at `open()`.
//! 2. `Compression::None` decompression allocated and copied a whole tile
//!    (`data.to_vec()`) that the caller immediately copied out of again;
//!    `read_tile_into` now decodes straight into a caller-owned buffer.
//! 3. The floating-point predictor allocated one scratch `Vec` per *scanline*.
//!    (Covered by the unit tests in `compression::predictor`.)
//!
//! The assertions here are all deterministic (I/O call counts and byte-for-byte
//! equality); timing is only ever *printed*, never asserted, so the tests cannot
//! become flaky on loaded CI machines.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use oxigeo_core::error::{OxiGeoError, Result};
use oxigeo_core::io::{ByteRange, DataSource, FileDataSource};
use oxigeo_core::types::RasterDataType;
use oxigeo_geotiff::cog::CogReader;
use oxigeo_geotiff::tiff::{Compression, Predictor, TiffTag};
use oxigeo_geotiff::writer::{
    GeoTiffWriter, GeoTiffWriterOptions, OverviewResampling, WriterConfig,
};

// ---------------------------------------------------------------------------
// Test data sources
// ---------------------------------------------------------------------------

/// Read counters shared with a [`CountingSource`] after it has been moved into a
/// `CogReader`.
#[derive(Debug, Default)]
struct ReadCounters {
    calls: AtomicUsize,
    bytes: AtomicUsize,
}

impl ReadCounters {
    fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }

    fn bytes(&self) -> usize {
        self.bytes.load(Ordering::Relaxed)
    }
}

/// In-memory data source that counts every `read_range` call, so a test can prove
/// that a lookup performs no I/O at all.
#[derive(Debug)]
struct CountingSource {
    data: Vec<u8>,
    counters: Arc<ReadCounters>,
}

impl DataSource for CountingSource {
    fn size(&self) -> Result<u64> {
        Ok(self.data.len() as u64)
    }

    fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
        let start = range.start as usize;
        let end = range.end as usize;
        if start > end || end > self.data.len() {
            return Err(OxiGeoError::OutOfBounds {
                message: format!(
                    "range {}..{} outside {}-byte source",
                    range.start,
                    range.end,
                    self.data.len()
                ),
            });
        }
        self.counters.calls.fetch_add(1, Ordering::Relaxed);
        self.counters
            .bytes
            .fetch_add(end - start, Ordering::Relaxed);
        Ok(self.data[start..end].to_vec())
    }
}

/// Plain in-memory data source (no counting), used to re-parse offset arrays the
/// slow way for cross-checking.
#[derive(Debug)]
struct MemorySource(Vec<u8>);

impl DataSource for MemorySource {
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
}

// ---------------------------------------------------------------------------
// Synthetic striped TIFF builder
// ---------------------------------------------------------------------------

const ENTRY_COUNT: u32 = 10;
const IFD_OFFSET: u32 = 8;
/// 2-byte entry count + 12 bytes per entry + 4-byte next-IFD pointer.
const IFD_SIZE: u32 = 2 + ENTRY_COUNT * 12 + 4;

/// Builds a classic little-endian TIFF with `strips` single-row Float32 strips.
///
/// Returns the file bytes plus the expected `(offset, byte_count)` of every strip,
/// so tests can check the reader against ground truth rather than against itself.
fn build_striped_float_tiff(strips: u32, width: u32) -> (Vec<u8>, Vec<(u64, u64)>) {
    let strip_bytes = width * 4;
    let offsets_pos = IFD_OFFSET + IFD_SIZE;
    let counts_pos = offsets_pos + strips * 4;
    let data_pos = counts_pos + strips * 4;

    let mut expected = Vec::with_capacity(strips as usize);
    for i in 0..strips {
        expected.push((
            u64::from(data_pos + i * strip_bytes),
            u64::from(strip_bytes),
        ));
    }

    let mut out = Vec::with_capacity((data_pos + strips * strip_bytes) as usize);
    // Header: "II", 42, first IFD offset.
    out.extend_from_slice(b"II");
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&IFD_OFFSET.to_le_bytes());

    // IFD.
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
    entry(TiffTag::SampleFormat, SHORT, 1, 3); // IEEE float
    out.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    assert_eq!(out.len() as u32, IFD_OFFSET + IFD_SIZE);

    // StripOffsets array, StripByteCounts array, then the pixel data itself.
    for (offset, _) in &expected {
        out.extend_from_slice(&(*offset as u32).to_le_bytes());
    }
    for (_, count) in &expected {
        out.extend_from_slice(&(*count as u32).to_le_bytes());
    }
    for row in 0..strips {
        for col in 0..width {
            let value = row as f32 * 100.0 + col as f32;
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    assert_eq!(out.len() as u32, data_pos + strips * strip_bytes);

    (out, expected)
}

/// Re-parses the strip offset/byte-count arrays the way `tile_byte_range` used to
/// on *every* lookup, so tests can compare the cached values against a live
/// recomputation.
fn recompute_strip_arrays(bytes: &[u8]) -> (Vec<u64>, Vec<u64>) {
    let source = MemorySource(bytes.to_vec());
    let tiff = oxigeo_geotiff::TiffFile::parse(&source).expect("parse tiff");
    let byte_order = tiff.byte_order();
    let variant = tiff.header.variant;
    let ifd = &tiff.ifds[0];
    let offsets = ifd
        .get_entry(TiffTag::StripOffsets)
        .expect("StripOffsets")
        .get_u64_vec(&source, byte_order, variant)
        .expect("offsets");
    let counts = ifd
        .get_entry(TiffTag::StripByteCounts)
        .expect("StripByteCounts")
        .get_u64_vec(&source, byte_order, variant)
        .expect("counts");
    (offsets, counts)
}

// ---------------------------------------------------------------------------
// T1 — tile_byte_range is O(1)
// ---------------------------------------------------------------------------

/// The offset index must be parsed once at `open()`: walking every block of a
/// band must then perform **zero** additional `read_range` calls, instead of the
/// two-per-block (offsets + byte counts) the old code issued.
#[test]
fn test_issue_14_tile_byte_range_performs_no_io_per_lookup() {
    let strips = 4096u32;
    let (bytes, expected) = build_striped_float_tiff(strips, 64);

    let counters = Arc::new(ReadCounters::default());
    let source = CountingSource {
        data: bytes.clone(),
        counters: Arc::clone(&counters),
    };
    let reader = CogReader::open(source).expect("open synthetic COG");

    let after_open = counters.calls();
    let bytes_after_open = counters.bytes();

    for y in 0..strips {
        let range = reader.tile_byte_range(0, 0, y).expect("strip range");
        let (offset, count) = expected[y as usize];
        assert_eq!(range.start, offset, "strip {y} offset");
        assert_eq!(range.len(), count, "strip {y} byte count");
    }

    let lookup_calls = counters.calls() - after_open;
    let lookup_bytes = counters.bytes() - bytes_after_open;
    assert_eq!(
        lookup_calls,
        0,
        "tile_byte_range must not perform I/O per lookup (did {lookup_calls} reads for \
         {strips} strips; the pre-fix code did {} )",
        2 * strips
    );
    assert_eq!(lookup_bytes, 0, "no bytes may be read per lookup");

    // The index is parsed exactly once per level, so opening the file reads the
    // two arrays a single time (plus header/IFD reads).
    assert!(
        after_open < 32,
        "open() should read the index once, not repeatedly (did {after_open} reads)"
    );
}

/// The cached values must be identical to a live re-parse of the IFD arrays (the
/// pre-fix code path).
#[test]
fn test_issue_14_offset_cache_matches_recomputation() {
    let strips = 512u32;
    let (bytes, _) = build_striped_float_tiff(strips, 32);
    let (offsets, counts) = recompute_strip_arrays(&bytes);
    assert_eq!(offsets.len(), strips as usize);
    assert_eq!(counts.len(), strips as usize);

    let reader = CogReader::open(MemorySource(bytes)).expect("open synthetic COG");
    for y in 0..strips {
        let range = reader.tile_byte_range(0, 0, y).expect("strip range");
        assert_eq!(range.start, offsets[y as usize], "strip {y} offset");
        assert_eq!(range.len(), counts[y as usize], "strip {y} byte count");
    }
}

/// Out-of-range block coordinates must still produce the same out-of-bounds error
/// they always did — the cache must not turn a bad lookup into a silent success.
#[test]
fn test_issue_14_offset_cache_preserves_out_of_bounds_errors() {
    let (bytes, _) = build_striped_float_tiff(8, 16);
    let reader = CogReader::open(MemorySource(bytes)).expect("open synthetic COG");

    let err = reader
        .tile_byte_range(0, 0, 8)
        .expect_err("strip 8 of 8 must be out of bounds");
    assert!(
        format!("{err}").contains("Tile/strip (0, 8) out of bounds"),
        "unexpected error: {err}"
    );

    // A striped image reports `tiles_across() == 1`, so `tile_x` simply advances
    // the flat block index (long-standing behaviour, unchanged by the cache):
    // block 8 of 8 is therefore out of bounds.
    let err = reader
        .tile_byte_range(0, 8, 0)
        .expect_err("block 8 of 8 must be out of bounds");
    assert!(
        format!("{err}").contains("out of bounds"),
        "unexpected error: {err}"
    );

    let err = reader
        .tile_byte_range(3, 0, 0)
        .expect_err("level 3 does not exist");
    assert!(
        format!("{err}").contains("Overview level 3 out of bounds"),
        "unexpected error: {err}"
    );
}

/// A file whose offset arrays cannot be pre-parsed must fall back to the original
/// on-demand lookup and report the very same `MissingTag` error.
#[test]
fn test_issue_14_missing_offset_tag_still_errors() {
    let (mut bytes, _) = build_striped_float_tiff(4, 16);
    // Rewrite the StripOffsets tag id (273) to an unused private tag so the entry
    // disappears from the reader's point of view. The tag is the first 2 bytes of
    // the 6th IFD entry.
    let entry_index = 5usize;
    let tag_pos = (IFD_OFFSET as usize) + 2 + entry_index * 12;
    assert_eq!(
        u16::from_le_bytes([bytes[tag_pos], bytes[tag_pos + 1]]),
        TiffTag::StripOffsets as u16
    );
    bytes[tag_pos..tag_pos + 2].copy_from_slice(&65000u16.to_le_bytes());

    let reader = CogReader::open(MemorySource(bytes)).expect("open still succeeds");
    let err = reader
        .tile_byte_range(0, 0, 0)
        .expect_err("missing StripOffsets must error");
    let msg = format!("{err}");
    assert!(
        msg.contains("StripOffsets"),
        "expected a MissingTag error naming StripOffsets, got: {msg}"
    );
}

/// Evidence, not an assertion: prints the cost of walking every block through the
/// cached lookup versus the pre-fix re-parse-everything approach.
#[test]
fn test_issue_14_offset_lookup_scaling_evidence() {
    for &strips in &[512u32, 1024, 4096] {
        let (bytes, _) = build_striped_float_tiff(strips, 32);
        let source = MemorySource(bytes.clone());
        let reader = CogReader::open(source).expect("open synthetic COG");

        let start = Instant::now();
        let mut checksum = 0u64;
        for y in 0..strips {
            let range = reader.tile_byte_range(0, 0, y).expect("cached lookup");
            checksum = checksum.wrapping_add(range.start).wrapping_add(range.len());
        }
        let cached = start.elapsed();

        // The old behaviour: re-read and re-parse both arrays for every block.
        let slow_source = MemorySource(bytes.clone());
        let tiff = oxigeo_geotiff::TiffFile::parse(&slow_source).expect("parse");
        let byte_order = tiff.byte_order();
        let variant = tiff.header.variant;
        let offsets_entry = tiff.ifds[0]
            .get_entry(TiffTag::StripOffsets)
            .expect("StripOffsets");
        let counts_entry = tiff.ifds[0]
            .get_entry(TiffTag::StripByteCounts)
            .expect("StripByteCounts");
        let start = Instant::now();
        let mut slow_checksum = 0u64;
        for y in 0..strips {
            let offsets = offsets_entry
                .get_u64_vec(&slow_source, byte_order, variant)
                .expect("offsets");
            let counts = counts_entry
                .get_u64_vec(&slow_source, byte_order, variant)
                .expect("counts");
            slow_checksum = slow_checksum
                .wrapping_add(offsets[y as usize])
                .wrapping_add(counts[y as usize]);
        }
        let recomputed = start.elapsed();

        assert_eq!(checksum, slow_checksum, "the two paths must agree");
        eprintln!(
            "issue#14 offset lookup, {strips:>5} strips: cached {cached:>12?}  \
             re-parse-per-lookup {recomputed:>12?}  ({:.0}x)",
            recomputed.as_secs_f64() / cached.as_secs_f64().max(f64::EPSILON)
        );
    }
}

// ---------------------------------------------------------------------------
// T2/T4 — decode-path throughput evidence
// ---------------------------------------------------------------------------

/// Pre-fix floating-point predictor decode, transcribed verbatim from the
/// implementation this lane replaced: one `Vec` allocation per scanline plus a
/// `match byte_order` and a multiply per byte. Kept here purely to quantify the
/// fix (and to prove the new code is bit-identical to it).
fn reference_float_predictor_reverse(
    data: &mut [u8],
    bytes_per_sample: usize,
    samples_per_pixel: usize,
    width: usize,
    byte_order: oxigeo_geotiff::tiff::ByteOrderType,
) {
    use oxigeo_geotiff::tiff::ByteOrderType;

    let row_bytes = width * samples_per_pixel * bytes_per_sample;
    for row_start in (0..data.len()).step_by(row_bytes) {
        let row_end = (row_start + row_bytes).min(data.len());
        let row = &mut data[row_start..row_end];
        let cc = row.len();
        let sample_count = cc / bytes_per_sample;
        let stride = samples_per_pixel.max(1);
        for i in stride..cc {
            row[i] = row[i].wrapping_add(row[i - stride]);
        }
        let planes = row.to_vec(); // <- one allocation per scanline
        for sample in 0..sample_count {
            for byte in 0..bytes_per_sample {
                let plane = match byte_order {
                    ByteOrderType::BigEndian => byte,
                    ByteOrderType::LittleEndian => bytes_per_sample - byte - 1,
                };
                row[bytes_per_sample * sample + byte] = planes[plane * sample_count + sample];
            }
        }
    }
}

/// Evidence, not an assertion: prints throughput for the two decode-path fixes
/// (uncompressed copy, floating-point predictor) and checks they still agree with
/// the pre-fix implementation byte for byte.
#[test]
fn test_issue_14_decode_path_speed_evidence() {
    use oxigeo_geotiff::compression;
    use oxigeo_geotiff::tiff::ByteOrderType;

    const MIB: usize = 1024 * 1024;
    let bytes = 16 * MIB;

    // --- T2: Compression::None -------------------------------------------
    let src: Vec<u8> = (0..bytes).map(|i| (i % 251) as u8).collect();
    let mut dst = vec![0u8; bytes];

    let start = Instant::now();
    let owned = compression::decompress(&src, Compression::None, bytes).expect("decompress");
    let owned_elapsed = start.elapsed();

    let start = Instant::now();
    compression::decompress_into(&src, Compression::None, &mut dst).expect("decompress_into");
    let into_elapsed = start.elapsed();

    assert_eq!(owned, dst, "decompress_into must match decompress");
    eprintln!(
        "issue#14 uncompressed {} MiB: decompress (alloc+copy) {owned_elapsed:>12?}  \
         decompress_into (copy) {into_elapsed:>12?}",
        bytes / MIB
    );

    // --- T4: floating-point predictor ------------------------------------
    // 256-pixel Float32 scanlines (1 KiB rows), i.e. a 256x256 Float32 tile every
    // 256 rows — the layout a GDAL-produced float DEM COG actually uses.
    let width = 256usize;
    let encoded: Vec<u8> = (0..bytes)
        .map(|i| (i.wrapping_mul(31) % 253) as u8)
        .collect();
    let mib = bytes as f64 / MIB as f64;

    let mut reference_best = f64::MAX;
    let mut optimised_best = f64::MAX;
    for _ in 0..3 {
        let mut reference = encoded.clone();
        let start = Instant::now();
        reference_float_predictor_reverse(&mut reference, 4, 1, width, ByteOrderType::LittleEndian);
        reference_best = reference_best.min(start.elapsed().as_secs_f64());

        let mut optimised = encoded.clone();
        let start = Instant::now();
        compression::apply_predictor_reverse(
            &mut optimised,
            Predictor::FloatingPoint,
            4,
            1,
            width,
            ByteOrderType::LittleEndian,
        )
        .expect("reverse float predictor");
        optimised_best = optimised_best.min(start.elapsed().as_secs_f64());

        assert_eq!(
            reference, optimised,
            "optimised float predictor must be bit-identical to the pre-fix code"
        );
    }
    eprintln!(
        "issue#14 float predictor {mib} MiB (f32, {width}-px rows): \
         pre-fix {:.3} ms ({:.0} MiB/s)  optimised {:.3} ms ({:.0} MiB/s)",
        reference_best * 1e3,
        mib / reference_best,
        optimised_best * 1e3,
        mib / optimised_best
    );
}

// ---------------------------------------------------------------------------
// T5 — read_tile_into equivalence
// ---------------------------------------------------------------------------

/// An RAII fixture path inside [`std::env::temp_dir`].
///
/// The leaf name embeds the process id and a monotonic counter, so no two test
/// binaries — nor two concurrent runs of this one — can ever land on the same
/// file.  (The old `oxigeo_issue14_{name}` shape shared a namespace with the
/// facade crate's issue-14 tests, so the two binaries collided.)  Dropping the
/// guard removes the fixture, so a panicking test leaks nothing.
struct TempPath(PathBuf);

impl TempPath {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(env::temp_dir().join(format!(
            "oxigeo_geotiff_issue14_readpath_{}_{seq}_{name}",
            std::process::id()
        )))
    }
}

impl std::ops::Deref for TempPath {
    type Target = std::path::Path;

    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

impl AsRef<std::path::Path> for TempPath {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn temp_test_file(name: &str) -> TempPath {
    TempPath::new(name)
}

/// One single-band raster layout to write and read back.
struct RasterSpec {
    /// Temporary file name suffix / case label.
    name: &'static str,
    /// Raster width in pixels.
    width: u64,
    /// Raster height in pixels.
    height: u64,
    /// Sample type.
    data_type: RasterDataType,
    /// TIFF compression scheme.
    compression: Compression,
    /// TIFF predictor.
    predictor: Predictor,
    /// `Some((w, h))` for a tiled layout, `None` for strips.
    tile_size: Option<(u32, u32)>,
}

impl RasterSpec {
    /// Builds the test pattern matching this spec's sample type.
    fn pattern(&self) -> Vec<u8> {
        match self.data_type {
            RasterDataType::Float32 => float32_pattern(self.width, self.height),
            _ => uint16_pattern(self.width, self.height),
        }
    }
}

/// Writes a single-band raster and returns the path.
fn write_raster(spec: &RasterSpec, data: &[u8]) -> TempPath {
    let path = temp_test_file(spec.name);
    let mut config = WriterConfig::new(spec.width, spec.height, 1, spec.data_type)
        .with_compression(spec.compression)
        .with_predictor(spec.predictor)
        .with_overviews(false, OverviewResampling::Nearest);
    match spec.tile_size {
        Some((tw, th)) => {
            config = config.with_tile_size(tw, th);
        }
        None => {
            config.tile_width = None;
            config.tile_height = None;
        }
    }
    let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
        .expect("create writer");
    writer.write(data).expect("write raster");
    path
}

/// Float32 test pattern with a smooth ramp (good for the float predictor).
fn float32_pattern(width: u64, height: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let value = (y as f32) * 12.5 + (x as f32) * 0.25 - 3000.0;
            data.extend_from_slice(&value.to_le_bytes());
        }
    }
    data
}

/// UInt16 test pattern.
fn uint16_pattern(width: u64, height: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity((width * height * 2) as usize);
    for y in 0..height {
        for x in 0..width {
            let value = ((x * 37 + y * 11) % 65536) as u16;
            data.extend_from_slice(&value.to_le_bytes());
        }
    }
    data
}

/// `read_tile_into` must produce exactly what `read_tile` produces, for every
/// combination of layout, codec and predictor the writer can emit.
#[test]
fn test_issue_14_read_tile_into_matches_read_tile() {
    let width = 48u64;
    let height = 48u64;

    // Extended in place by the codec-gated blocks below; with no codec feature
    // enabled nothing pushes, and the binding is then genuinely immutable.
    #[cfg_attr(not(any(feature = "deflate", feature = "lzw")), allow(unused_mut))]
    let mut cases = vec![
        RasterSpec {
            name: "none_tiled_f32",
            width,
            height,
            data_type: RasterDataType::Float32,
            compression: Compression::None,
            predictor: Predictor::None,
            tile_size: Some((16, 16)),
        },
        RasterSpec {
            name: "none_striped_f32",
            width,
            height,
            data_type: RasterDataType::Float32,
            compression: Compression::None,
            predictor: Predictor::None,
            tile_size: None,
        },
        RasterSpec {
            name: "packbits_tiled_u16",
            width,
            height,
            data_type: RasterDataType::UInt16,
            compression: Compression::Packbits,
            predictor: Predictor::None,
            tile_size: Some((16, 16)),
        },
        RasterSpec {
            name: "none_tiled_f32_floatpred",
            width,
            height,
            data_type: RasterDataType::Float32,
            compression: Compression::None,
            predictor: Predictor::FloatingPoint,
            tile_size: Some((16, 16)),
        },
        RasterSpec {
            name: "none_striped_f32_floatpred",
            width,
            height,
            data_type: RasterDataType::Float32,
            compression: Compression::None,
            predictor: Predictor::FloatingPoint,
            tile_size: None,
        },
    ];
    #[cfg(feature = "deflate")]
    {
        cases.push(RasterSpec {
            name: "deflate_tiled_f32_floatpred",
            width,
            height,
            data_type: RasterDataType::Float32,
            compression: Compression::Deflate,
            predictor: Predictor::FloatingPoint,
            tile_size: Some((16, 16)),
        });
        cases.push(RasterSpec {
            name: "deflate_striped_f32_floatpred",
            width,
            height,
            data_type: RasterDataType::Float32,
            compression: Compression::Deflate,
            predictor: Predictor::FloatingPoint,
            tile_size: None,
        });
    }
    #[cfg(feature = "lzw")]
    cases.push(RasterSpec {
        name: "lzw_tiled_u16_horizontal",
        width,
        height,
        data_type: RasterDataType::UInt16,
        compression: Compression::Lzw,
        predictor: Predictor::HorizontalDifferencing,
        tile_size: Some((16, 16)),
    });
    #[cfg(feature = "zstd")]
    cases.push(RasterSpec {
        name: "zstd_tiled_f32",
        width,
        height,
        data_type: RasterDataType::Float32,
        compression: Compression::Zstd,
        predictor: Predictor::None,
        tile_size: Some((16, 16)),
    });

    for case in cases {
        let path = write_raster(&case, &case.pattern());

        let source = FileDataSource::open(&path).expect("open written file");
        let reader = CogReader::open(source).expect("open CogReader");
        let (tiles_x, tiles_y) = reader.tile_count();
        assert!(tiles_x >= 1 && tiles_y >= 1, "{}: no blocks", case.name);

        // One scratch buffer for the whole band, exactly as a band reader would.
        let max_size = (0..tiles_y)
            .map(|ty| reader.tile_decoded_size(0, ty).expect("decoded size"))
            .max()
            .unwrap_or(0);
        let mut scratch = vec![0xCDu8; max_size];

        for ty in 0..tiles_y {
            let decoded_size = reader.tile_decoded_size(0, ty).expect("decoded size");
            for tx in 0..tiles_x {
                let expected = reader.read_tile(0, tx, ty).expect("read_tile");
                // Poison the buffer so a stale-data leak would be caught.
                scratch.fill(0xCD);
                reader
                    .read_tile_into(0, tx, ty, &mut scratch[..decoded_size])
                    .expect("read_tile_into");
                assert_eq!(
                    expected.len(),
                    decoded_size,
                    "{}: read_tile length vs decoded size at ({tx},{ty})",
                    case.name
                );
                assert_eq!(
                    &scratch[..decoded_size],
                    expected.as_slice(),
                    "{}: read_tile_into mismatch at ({tx},{ty})",
                    case.name
                );
            }
        }

        let _ = std::fs::remove_file(path);
    }
}

/// `read_tile_into` must reject a destination buffer of the wrong size rather than
/// silently writing a partial tile.
#[test]
fn test_issue_14_read_tile_into_rejects_wrong_buffer_size() {
    let spec = RasterSpec {
        name: "wrong_size.tif",
        width: 32,
        height: 32,
        data_type: RasterDataType::Float32,
        compression: Compression::None,
        predictor: Predictor::None,
        tile_size: Some((16, 16)),
    };
    let path = write_raster(&spec, &spec.pattern());

    let source = FileDataSource::open(&path).expect("open written file");
    let reader = CogReader::open(source).expect("open CogReader");
    let decoded_size = reader.tile_decoded_size(0, 0).expect("decoded size");
    assert_eq!(decoded_size, 16 * 16 * 4);

    let mut too_small = vec![0u8; decoded_size - 1];
    let err = reader
        .read_tile_into(0, 0, 0, &mut too_small)
        .expect_err("undersized buffer must be rejected");
    assert!(
        format!("{err}").contains("decoded size"),
        "unexpected error: {err}"
    );

    let mut too_large = vec![0u8; decoded_size + 1];
    let err = reader
        .read_tile_into(0, 0, 0, &mut too_large)
        .expect_err("oversized buffer must be rejected");
    assert!(
        format!("{err}").contains("decoded size"),
        "unexpected error: {err}"
    );

    let _ = std::fs::remove_file(path);
}

/// Reading a whole band through `read_tile_into` into one reused buffer must
/// reconstruct the original raster exactly — the end-to-end property issue #14 is
/// really about.
#[test]
fn test_issue_14_band_reconstruction_via_read_tile_into() {
    let spec = RasterSpec {
        name: "band_reconstruction.tif",
        width: 40,
        height: 40,
        data_type: RasterDataType::Float32,
        compression: Compression::None,
        predictor: Predictor::FloatingPoint,
        tile_size: Some((16, 16)),
    };
    let (width, height) = (spec.width, spec.height);
    let original = spec.pattern();
    let path = write_raster(&spec, &original);

    let source = FileDataSource::open(&path).expect("open written file");
    let reader = CogReader::open(source).expect("open CogReader");
    let (tiles_x, tiles_y) = reader.tile_count();
    let (tile_w, tile_h) = reader.tile_size().expect("tiled");

    let mut band = vec![0u8; (width * height * 4) as usize];
    let mut scratch = vec![0u8; reader.tile_decoded_size(0, 0).expect("decoded size")];

    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            reader
                .read_tile_into(0, tx, ty, &mut scratch)
                .expect("read_tile_into");
            let x_start = (tx * tile_w) as usize;
            let y_start = (ty * tile_h) as usize;
            for row in 0..tile_h as usize {
                let dst_y = y_start + row;
                if dst_y >= height as usize {
                    break;
                }
                let copy_width = (tile_w as usize).min(width as usize - x_start);
                let src = row * tile_w as usize * 4;
                let dst = (dst_y * width as usize + x_start) * 4;
                band[dst..dst + copy_width * 4]
                    .copy_from_slice(&scratch[src..src + copy_width * 4]);
            }
        }
    }

    assert_eq!(band, original, "band reconstruction must be bit-exact");
    let _ = std::fs::remove_file(path);
}
