//! cool-japan/oxigeo#14 — the timing harness behind the claim that
//! `read_band_into_typed` is the *fastest* way to get a band into a
//! caller-owned buffer, not merely the leanest.
//!
//! The test is `#[ignore]`d: it needs a 4000 × 4000 `Float32` fixture (64 MiB on
//! disk, 128 MiB per `f64` destination) and it measures wall-clock time, so it is
//! run deliberately, on a quiet machine:
//!
//! ```text
//! cargo test -p oxigeo-geotiff --release --test issue_14_scatter_bench \
//!     -- --ignored --nocapture
//! ```
//!
//! # Method
//!
//! Every variant is timed in the *same process*, interleaved: one round runs all
//! variants once, and each round rotates the starting variant so that no variant
//! permanently owns the cold-cache or the warm-cache slot. Both the median and
//! the minimum over the rounds are reported; the minimum is the one to trust when
//! the machine is not idle, the median when it is.
//!
//! The source is an in-memory `Vec<u8>` so that the numbers measure the decode
//! and scatter rather than the page cache.
//!
//! # Unsafe
//!
//! The "old workaround" variants reinterpret the raw band bytes as `f32` with
//! `slice::align_to`, which is what `bytemuck::cast_slice` does and what the
//! issue reporter actually wrote. Doing it any other way would make the baseline
//! artificially slow and the comparison dishonest.

#![allow(unsafe_code)]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use oxigeo_core::error::{OxiGeoError, Result};
use oxigeo_core::io::{ByteRange, DataSource};
use oxigeo_core::types::RasterDataType;
use oxigeo_geotiff::GeoTiffReader;
use oxigeo_geotiff::tiff::{Compression, Predictor};
use oxigeo_geotiff::writer::{
    GeoTiffWriter, GeoTiffWriterOptions, OverviewResampling, WriterConfig,
};

const WIDTH: u64 = 4000;
const HEIGHT: u64 = 4000;
const TILE: u32 = 256;
const ROUNDS: usize = 15;

/// In-memory data source, so the timings measure decode + scatter and not I/O.
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

/// Writes the fixture once and returns its bytes.
///
/// At 4000x4000 f32 this fixture is ~64 MiB and takes appreciable time to
/// author, so it is deliberately *cached* across runs under a stable,
/// content-describing name (`{WIDTH}x{HEIGHT}_f32_t{TILE}`) rather than being
/// regenerated every time.  A stable name alone would race: two concurrent
/// runs would interleave writes into one half-written file and each read back
/// garbage.  The cache is therefore populated **atomically** — the fixture is
/// built under a private, pid+counter-unique scratch name and then `rename`d
/// into place.  `rename` within one directory is atomic on every platform this
/// crate targets, so a reader only ever observes the complete previous file or
/// the complete new one, never a partial write.  Losing the rename race is
/// harmless: the winner's bytes are byte-identical, since the fixture is a pure
/// function of the three constants in its own name.
fn fixture() -> Vec<u8> {
    /// Removes the scratch file if the rename never happened (e.g. a panic).
    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    let expected = (WIDTH * HEIGHT * 4) as usize;
    let cached = std::env::temp_dir().join(format!(
        "oxigeo_geotiff_issue14_bench_{WIDTH}x{HEIGHT}_f32_t{TILE}.tif"
    ));
    if let Ok(bytes) = std::fs::read(&cached)
        && bytes.len() >= expected
    {
        return bytes;
    }

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let scratch = Scratch(std::env::temp_dir().join(format!(
        "oxigeo_geotiff_issue14_bench_{}_{seq}.tif.partial",
        std::process::id()
    )));
    build_fixture(&scratch.0);
    let bytes = std::fs::read(&scratch.0).expect("read fixture");
    // Publish atomically. If this fails (e.g. a cross-device temp dir) we still
    // have the bytes; the cache simply stays unpopulated for the next run.
    let _ = std::fs::rename(&scratch.0, &cached);
    bytes
}

fn build_fixture(path: &PathBuf) {
    let mut data = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let value = (y as f32) * 12.5 + (x as f32) * 0.25 - 3000.0;
            data.extend_from_slice(&value.to_le_bytes());
        }
    }
    let config = WriterConfig::new(WIDTH, HEIGHT, 1, RasterDataType::Float32)
        .with_compression(Compression::None)
        .with_predictor(Predictor::None)
        .with_tile_size(TILE, TILE)
        .with_overviews(false, OverviewResampling::Nearest);
    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())
        .expect("create writer");
    writer.write(&data).expect("write fixture");
}

/// Reinterprets a little-endian `Float32` band as `&[f32]`, the way the issue
/// reporter's `bytemuck::cast_slice` did.
fn as_f32(raw: &[u8]) -> &[f32] {
    // SAFETY: `f32` has no invalid bit patterns and no `Drop`; `align_to` itself
    // guarantees the returned middle slice is correctly aligned and in bounds.
    // The fixture is little-endian and the test only runs on little-endian
    // targets (asserted by the caller).
    let (head, mid, _) = unsafe { raw.align_to::<f32>() };
    assert!(head.is_empty(), "band buffer was not f32-aligned");
    mid
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Variant {
    /// A — `read_band` only: the decode floor, no conversion, fresh `Vec<u8>`.
    ReadBand,
    /// B — old workaround into a fresh `Vec<f64>`.
    OldFreshF64,
    /// C — `read_band_into_typed::<f64>` into a fresh `Vec<f64>`.
    NewFreshF64,
    /// E — old workaround into a reused `Vec<f64>`.
    OldReuseF64,
    /// F — `read_band_into_typed::<f64>` into a reused `Vec<f64>`.
    NewReuseF64,
    /// old workaround into a fresh `Vec<f32>` — the no-conversion floor.
    OldFreshF32,
    /// `read_band_into_typed::<f32>` into a fresh `Vec<f32>` — the floor.
    NewFreshF32,
    /// old workaround into a reused `Vec<f32>`.
    OldReuseF32,
    /// `read_band_into_typed::<f32>` into a reused `Vec<f32>`.
    NewReuseF32,
}

const VARIANTS: [(Variant, &str); 9] = [
    (Variant::ReadBand, "A  read_band (decode only)      "),
    (Variant::OldFreshF64, "B  old workaround -> fresh f64  "),
    (Variant::NewFreshF64, "C  read_band_into_typed f64 new "),
    (Variant::OldReuseF64, "E  old workaround -> reused f64 "),
    (Variant::NewReuseF64, "F  read_band_into_typed f64 use "),
    (Variant::OldFreshF32, "B32 old workaround -> fresh f32 "),
    (Variant::NewFreshF32, "C32 read_band_into_typed f32 new"),
    (Variant::OldReuseF32, "E32 old workaround -> reused f32"),
    (Variant::NewReuseF32, "F32 read_band_into_typed f32 use"),
];

struct Harness {
    reader: GeoTiffReader<SliceSource>,
    pixels: usize,
    reuse64: Vec<f64>,
    reuse32: Vec<f32>,
}

impl Harness {
    /// Runs one variant and returns a checksum, so nothing can be optimised out.
    fn run(&mut self, variant: Variant) -> f64 {
        match variant {
            Variant::ReadBand => {
                let raw = self.reader.read_band(0, 0).expect("read_band");
                checksum_u8(&raw)
            }
            Variant::OldFreshF64 => {
                let raw = self.reader.read_band(0, 0).expect("read_band");
                let out: Vec<f64> = as_f32(&raw).iter().map(|&v| v as f64).collect();
                checksum_f64(&out)
            }
            Variant::NewFreshF64 => {
                let mut out = vec![0.0f64; self.pixels];
                self.reader
                    .read_band_into_typed(0, 0, &mut out)
                    .expect("typed");
                checksum_f64(&out)
            }
            Variant::OldReuseF64 => {
                let raw = self.reader.read_band(0, 0).expect("read_band");
                let src = as_f32(&raw);
                for (dst, &v) in self.reuse64.iter_mut().zip(src) {
                    *dst = v as f64;
                }
                checksum_f64(&self.reuse64)
            }
            Variant::NewReuseF64 => {
                let mut out = core::mem::take(&mut self.reuse64);
                self.reader
                    .read_band_into_typed(0, 0, &mut out)
                    .expect("typed");
                self.reuse64 = out;
                checksum_f64(&self.reuse64)
            }
            Variant::OldFreshF32 => {
                let raw = self.reader.read_band(0, 0).expect("read_band");
                let out: Vec<f32> = as_f32(&raw).to_vec();
                checksum_f32(&out)
            }
            Variant::NewFreshF32 => {
                let mut out = vec![0.0f32; self.pixels];
                self.reader
                    .read_band_into_typed(0, 0, &mut out)
                    .expect("typed");
                checksum_f32(&out)
            }
            Variant::OldReuseF32 => {
                let raw = self.reader.read_band(0, 0).expect("read_band");
                let src = as_f32(&raw);
                self.reuse32.copy_from_slice(src);
                checksum_f32(&self.reuse32)
            }
            Variant::NewReuseF32 => {
                let mut out = core::mem::take(&mut self.reuse32);
                self.reader
                    .read_band_into_typed(0, 0, &mut out)
                    .expect("typed");
                self.reuse32 = out;
                checksum_f32(&self.reuse32)
            }
        }
    }
}

fn checksum_u8(data: &[u8]) -> f64 {
    let step = (data.len() / 64).max(1);
    data.iter().step_by(step).map(|&v| v as f64).sum()
}

fn checksum_f32(data: &[f32]) -> f64 {
    let step = (data.len() / 64).max(1);
    data.iter().step_by(step).map(|&v| v as f64).sum()
}

fn checksum_f64(data: &[f64]) -> f64 {
    let step = (data.len() / 64).max(1);
    data.iter().step_by(step).sum()
}

fn median(values: &mut [Duration]) -> Duration {
    values.sort_unstable();
    values[values.len() / 2]
}

#[test]
#[ignore = "wall-clock benchmark over a 64 MiB fixture; run explicitly on a quiet machine"]
fn issue_14_scatter_bench() {
    // The fixture and the `cast_slice` baseline are little-endian only.
    #[cfg(target_endian = "big")]
    {
        println!("skipped: this benchmark is little-endian only");
        return;
    }

    let bytes = fixture();
    let reader = GeoTiffReader::open(SliceSource(bytes)).expect("open");
    let pixels = reader.band_pixel_count(0).expect("pixel count");
    assert_eq!(pixels, (WIDTH * HEIGHT) as usize);

    let mut harness = Harness {
        reader,
        pixels,
        reuse64: vec![0.0f64; pixels],
        reuse32: vec![0.0f32; pixels],
    };

    // Warm-up: fault in the reused destinations and any lazy reader state.
    for (variant, _) in VARIANTS {
        std::hint::black_box(harness.run(variant));
    }

    let mut samples: Vec<Vec<Duration>> = vec![Vec::with_capacity(ROUNDS); VARIANTS.len()];
    for round in 0..ROUNDS {
        for step in 0..VARIANTS.len() {
            // Rotate the order every round so no variant permanently owns the
            // first (cold) or last (warm) slot.
            let index = (round + step) % VARIANTS.len();
            let (variant, _) = VARIANTS[index];
            let start = Instant::now();
            let sum = harness.run(variant);
            let elapsed = start.elapsed();
            std::hint::black_box(sum);
            samples[index].push(elapsed);
        }
    }

    println!(
        "\nissue#14 scatter benchmark — {WIDTH}x{HEIGHT} Float32, {TILE}x{TILE} tiles, \
         uncompressed, in-memory source, {ROUNDS} interleaved rounds\n"
    );
    println!("{:<34} {:>12} {:>12}", "variant", "median", "min");
    for (index, (_, label)) in VARIANTS.iter().enumerate() {
        let mut values = samples[index].clone();
        let med = median(&mut values);
        let min = values.iter().copied().min().unwrap_or_default();
        println!("{:<34} {:>10.3?} {:>10.3?}", label.trim_end(), med, min);
    }

    // Machine-readable line, so a before/after run can be diffed mechanically.
    for (index, (_, label)) in VARIANTS.iter().enumerate() {
        let mut values = samples[index].clone();
        let med = median(&mut values);
        let min = values.iter().copied().min().unwrap_or_default();
        println!(
            "CSV,{},{:.4},{:.4}",
            label.trim_end(),
            med.as_secs_f64() * 1e3,
            min.as_secs_f64() * 1e3
        );
    }
}
