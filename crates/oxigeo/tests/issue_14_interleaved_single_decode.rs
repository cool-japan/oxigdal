//! cool-japan/oxigeo#14 — the interleaved readers must decode each block once.
//!
//! [`Dataset::read_interleaved`] and its three siblings used to be built out of
//! one-band-at-a-time driver calls, walked strip by strip: an `n`-band read of a
//! **chunky** (`PlanarConfiguration = 1`) file decoded every block `n` times and
//! discarded `n − 1` bands each pass.  They now make a single multi-band driver
//! call — `read_bands_into_typed` for a full read, `read_window_bands_into_typed`
//! for a windowed one — which decodes each block once.
//!
//! # Why the counting source is on the driver and not on the [`Dataset`]
//!
//! A [`Dataset`] is built from a *path* and opens its own source internally; the
//! reader it uses is the concrete type `GeoTiffReader<FileDataSource>`, with no
//! constructor, builder or trait object anywhere on the public surface that
//! would take a caller-supplied [`DataSource`].  There is therefore no seam at
//! which a counting source can be handed to a `Dataset`, and no facade API that
//! reports blocks read.  Nothing in this file pretends otherwise.
//!
//! What it does instead is close the gap from both ends, with no timing anywhere:
//!
//! * the facade reads a real file through its real path, and the *same file's
//!   bytes* are then read through the very driver entry point the facade now
//!   calls, over a source that counts every block fetch — and the two results are
//!   asserted equal element for element, full-raster and windowed;
//! * the same counting source then performs the read in the shape the facade used
//!   to use — one single-band call per band — and the block-fetch counts of the
//!   two shapes are compared.
//!
//! An implementation that quietly went back to per-band reads would still pass
//! the equality assertions, so those are not the whole proof; the count
//! assertions are what pin the saving down, and the equality assertions are what
//! tie the counted read to the facade's own output.

#![cfg(feature = "geotiff")]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use oxigeo::core_types::error::{OxiGeoError, Result};
use oxigeo::core_types::io::{ByteRange, DataSource};
use oxigeo::geotiff::GeoTiffReader;
use oxigeo::{Dataset, GeoTransform, RasterDataType};

// ---------------------------------------------------------------------------
// Counting data source
// ---------------------------------------------------------------------------

/// In-memory source that tallies every read, so one block decode costs exactly
/// one counted call.
#[derive(Debug)]
struct CountingSource {
    /// The whole file.
    data: Vec<u8>,
    /// Shared tally of reads issued against it.
    reads: Arc<AtomicUsize>,
}

impl CountingSource {
    /// Wraps `data`, handing back the shared read counter.
    fn new(data: Vec<u8>) -> (Self, Arc<AtomicUsize>) {
        let reads = Arc::new(AtomicUsize::new(0));
        (
            Self {
                data,
                reads: Arc::clone(&reads),
            },
            reads,
        )
    }

    /// The bytes of `range`, clamped to the file.
    fn slice(&self, range: ByteRange) -> Result<&[u8]> {
        let start = (range.start as usize).min(self.data.len());
        let end = (range.end as usize).min(self.data.len());
        if start > end {
            return Err(OxiGeoError::OutOfBounds {
                message: format!("range {start}..{end} outside source"),
            });
        }
        Ok(&self.data[start..end])
    }
}

impl DataSource for CountingSource {
    fn size(&self) -> Result<u64> {
        Ok(self.data.len() as u64)
    }

    fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        Ok(self.slice(range)?.to_vec())
    }

    fn read_range_into(&self, range: ByteRange, dst: &mut [u8]) -> Result<usize> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        let src = self.slice(range)?;
        let out = dst
            .get_mut(..src.len())
            .ok_or_else(|| OxiGeoError::OutOfBounds {
                message: "destination too small".to_string(),
            })?;
        out.copy_from_slice(src);
        Ok(src.len())
    }
}

/// Runs `body` and returns how many source reads it made.
fn count_reads<R>(reads: &AtomicUsize, body: impl FnOnce() -> R) -> (R, usize) {
    let before = reads.load(Ordering::Relaxed);
    let value = body();
    (value, reads.load(Ordering::Relaxed) - before)
}

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

/// Per-test scratch fixture inside the system temp dir (house policy: no
/// hardcoded absolute paths).
///
/// The leaf name embeds the process id and a monotonic counter, so no two test
/// binaries — nor two concurrent runs of this one — can land on the same file.
/// Dropping the guard removes the fixture, so a panicking test leaks nothing.
struct TempPath(std::path::PathBuf);

impl TempPath {
    /// A fresh, unique path ending in `name`.
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "oxigeo_issue14_decode_{}_{seq}_{name}",
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

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Writes a `width × height` three-band `u8` GeoTIFF and returns its path guard.
///
/// The crate's writer emits chunky (`PlanarConfiguration = 1`) data, which is
/// exactly the layout the single-decode change is about: one block there holds
/// all three bands of the pixels it covers.
fn write_rgb_fixture(name: &str, width: u32, height: u32) -> TempPath {
    use oxigeo::builder::{DatasetCreateBuilder, OutputFormat};

    let path = TempPath::new(name);
    let mut writer = DatasetCreateBuilder::new(&*path, OutputFormat::GeoTiff)
        .create()
        .expect("create writer");
    writer.set_dimensions(width, height, 3).expect("dimensions");
    writer.set_data_type(RasterDataType::UInt8);
    writer.set_geo_transform(GeoTransform::north_up(0.0, f64::from(height), 1.0, 1.0));

    let pixels = width as usize * height as usize;
    let mut planes = vec![0u8; pixels * 3];
    for (band, plane) in planes.chunks_exact_mut(pixels).enumerate() {
        for (index, sample) in plane.iter_mut().enumerate() {
            *sample = (index.wrapping_mul(7).wrapping_add(band * 41) % 251) as u8;
        }
    }
    writer.write_all_bands(&planes).expect("write bands");
    writer.finalize().expect("finalize");
    path
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// One multi-band call fetches each block once; the three single-band calls the
/// facade used to make fetch each block three times.
///
/// The fixture is the very file the facade reads — written by the crate's own
/// writer, read back off disk byte for byte — so the counted reads are the same
/// decode work the facade does, just reached through a source that can be
/// counted.  The facade's own result is then asserted equal to the counted
/// multi-band read, element for element.
#[test]
fn test_issue_14_interleaved_read_fetches_each_block_once() {
    const WIDTH: u32 = 512;
    const HEIGHT: u32 = 2048;
    let pixels = WIDTH as usize * HEIGHT as usize;

    let path = write_rgb_fixture("fullread.tif", WIDTH, HEIGHT);

    // What the facade produces, through its real path-based API.
    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("open");
    let facade: Vec<u8> = ds.read_interleaved(Some(&[0, 1, 2])).expect("facade read");
    assert_eq!(facade.len(), pixels * 3);

    // The same bytes, over a source that counts.
    let bytes = std::fs::read(&*path).expect("read fixture bytes");
    let (source, reads) = CountingSource::new(bytes);
    let reader = GeoTiffReader::open(source).expect("open counting reader");

    // The call the facade now makes.
    let mut multi = vec![0u8; pixels * 3];
    let ((), multi_reads) = count_reads(&reads, || {
        reader
            .read_bands_into_typed(0, &[0, 1, 2], &mut multi)
            .expect("multi-band read");
    });

    // The shape the facade used to use: one single-band call per band.
    let mut plane = vec![0u8; pixels];
    let ((), per_band_reads) = count_reads(&reads, || {
        for band in 0..3 {
            reader
                .read_band_into_typed(0, band, &mut plane)
                .expect("single-band read");
        }
    });

    println!(
        "issue#14 {WIDTH}x{HEIGHT} chunky RGB, full raster: \
         one multi-band call fetched {multi_reads} blocks, \
         three single-band calls fetched {per_band_reads}"
    );
    assert!(
        multi_reads > 0,
        "the multi-band read must actually have read something"
    );
    assert_eq!(
        per_band_reads,
        multi_reads * 3,
        "three single-band calls must fetch every block three times; if they no longer do, \
         this test has stopped measuring the saving it claims"
    );
    assert_eq!(
        facade, multi,
        "the facade's interleaved read must equal the counted multi-band read, or the \
         count above is not the count the facade pays"
    );
}

/// The windowed reader delegates to the windowed multi-band call, with the same
/// one-fetch-per-block saving.
///
/// A window is the case where a per-band implementation is most tempting and
/// most wasteful — a 256×256 RGB tile server would decode every tile three
/// times, forever.
#[test]
fn test_issue_14_window_interleaved_read_fetches_each_block_once() {
    const WIDTH: u32 = 512;
    const HEIGHT: u32 = 1024;
    const WIN: (u32, u32, u32, u32) = (37, 401, 256, 192);

    let path = write_rgb_fixture("windowread.tif", WIDTH, HEIGHT);
    let win_pixels = WIN.2 as usize * WIN.3 as usize;

    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("open");
    let facade: Vec<u8> = ds
        .read_window_interleaved(Some(&[2, 1, 0]), WIN.0, WIN.1, WIN.2, WIN.3)
        .expect("facade windowed read");
    assert_eq!(facade.len(), win_pixels * 3);

    let bytes = std::fs::read(&*path).expect("read fixture bytes");
    let (source, reads) = CountingSource::new(bytes);
    let reader = GeoTiffReader::open(source).expect("open counting reader");

    let mut multi = vec![0u8; win_pixels * 3];
    let ((), multi_reads) = count_reads(&reads, || {
        reader
            .read_window_bands_into_typed(
                0,
                &[2, 1, 0],
                u64::from(WIN.0),
                u64::from(WIN.1),
                u64::from(WIN.2),
                u64::from(WIN.3),
                &mut multi,
            )
            .expect("multi-band windowed read");
    });

    let mut plane = vec![0u8; win_pixels];
    let ((), per_band_reads) = count_reads(&reads, || {
        for band in [2usize, 1, 0] {
            reader
                .read_window_into_typed(
                    0,
                    band,
                    u64::from(WIN.0),
                    u64::from(WIN.1),
                    u64::from(WIN.2),
                    u64::from(WIN.3),
                    &mut plane,
                )
                .expect("single-band windowed read");
        }
    });

    println!(
        "issue#14 {WIDTH}x{HEIGHT} chunky RGB, window {}x{} at ({},{}), BGR order: \
         one multi-band call fetched {multi_reads} blocks, \
         three single-band calls fetched {per_band_reads}",
        WIN.2, WIN.3, WIN.0, WIN.1
    );
    assert!(
        multi_reads > 0,
        "the windowed multi-band read must actually have read something"
    );
    assert_eq!(
        per_band_reads,
        multi_reads * 3,
        "three single-band windowed calls must fetch every overlapped block three times"
    );
    assert_eq!(
        facade, multi,
        "the facade's windowed interleaved read must equal the counted multi-band read"
    );
}
