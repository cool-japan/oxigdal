//! Regression test for cool-japan/oxigeo#14 — QC scanners on big-endian (`MM`)
//! TIFFs.
//!
//! A TIFF stores its samples in the byte order its header declares, so on an
//! `MM` file every multi-byte sample is byte-reversed with respect to a
//! little-endian host *on disk*. Exactly one layer is allowed to undo that: the
//! GeoTIFF driver, which normalises decoded samples to host order on the way out
//! of block decode. Everything above it — including both raster QC scanners —
//! reads samples with `from_ne_bytes` and must not consult the file's byte order
//! at all.
//!
//! Both scanners have been wrong in both directions, and both failures were
//! silent, which for a QC tool is the worst possible outcome:
//!
//! * Originally they decoded with `from_le_bytes` while the driver returned
//!   *file*-order bytes, so an `MM` raster's sentinel matched nothing
//!   ("NoData metadata but no NoData pixels" on an intact footprint) and every
//!   min/max/mean/out-of-range verdict was fiction.
//! * Then they compensated by re-parsing the TIFF header and decoding in the
//!   file's order — correct until the driver started normalising, at which point
//!   the compensation became a *second* swap and re-broke `MM` files.
//!
//! This test pins the end state and nothing weaker: it builds the *same logical
//! raster* twice — once `II`, once `MM` — and asserts each report matches the
//! known pixel values **and** that the two reports are identical. That pair of
//! assertions fails under either mistake: a missing swap breaks `MM` alone, a
//! double swap breaks `MM` alone, and only "swapped exactly once, in the driver"
//! satisfies both. The fixtures are hand-built because the crate's writer only
//! ever emits `II`.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
#![allow(clippy::float_cmp)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use oxigeo_geotiff::tiff::TiffTag;
use oxigeo_qc::raster::nodata::NoDataValidator;
use oxigeo_qc::raster::radiometric::{BandRange, RadiometricValidator, SensorProfile};

const WIDTH: u32 = 8;
const HEIGHT: u32 = 8;
const TILE: u32 = 4;
const BANDS: u16 = 3;

/// NoData sentinel. Deliberately **not** byte-palindromic: `0x7FFF` reversed is
/// `0xFF7F` = 65_407, so a scanner that ignores byte order cannot match it by
/// accident (which `0xFFFF` would have let it do).
const NODATA: u16 = 0x7FFF;

/// The fixture's sample values: bands 0 and 1 carry NoData sentinels on every
/// eighth pixel, band 2 carries none. No value is byte-palindromic.
fn sample_value(band: usize, index: u32) -> u16 {
    if band < 2 && index.is_multiple_of(8) {
        return NODATA;
    }
    (band as u16 + 1) * 100 + index as u16
}

fn pixel_index(x: u32, y: u32) -> u32 {
    y * WIDTH + x
}

// ---------------------------------------------------------------------------
// Byte-order-parametric serialisation
// ---------------------------------------------------------------------------

/// Which byte order the fixture is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endian {
    Little,
    Big,
}

impl Endian {
    fn magic(self) -> &'static [u8; 2] {
        match self {
            Self::Little => b"II",
            Self::Big => b"MM",
        }
    }

    fn u16(self, v: u16) -> [u8; 2] {
        match self {
            Self::Little => v.to_le_bytes(),
            Self::Big => v.to_be_bytes(),
        }
    }

    fn u32(self, v: u32) -> [u8; 4] {
        match self {
            Self::Little => v.to_le_bytes(),
            Self::Big => v.to_be_bytes(),
        }
    }

    fn tag(self) -> &'static str {
        match self {
            Self::Little => "ii",
            Self::Big => "mm",
        }
    }
}

// ---------------------------------------------------------------------------
// Synthetic TIFF builder (uncompressed, tiled, UInt16, chunky)
// ---------------------------------------------------------------------------

type Entry = (TiffTag, u16, u32, Vec<u8>);

const ASCII: u16 = 2;
const SHORT: u16 = 3;
const LONG: u16 = 4;

/// Serialises the fixture in the requested byte order.
///
/// Every multi-byte quantity — header, IFD entries, out-of-line payloads *and*
/// the pixel samples themselves — is written in `endian`, exactly as a real
/// `MM` writer would.
fn build_tiff(endian: Endian) -> Vec<u8> {
    let spp = BANDS as usize;
    let across = WIDTH.div_ceil(TILE);
    let down = HEIGHT.div_ceil(TILE);

    let mut blocks: Vec<Vec<u8>> = Vec::with_capacity((across * down) as usize);
    for by in 0..down {
        for bx in 0..across {
            let mut block = Vec::with_capacity((TILE * TILE) as usize * spp * 2);
            for row in 0..TILE {
                for col in 0..TILE {
                    let (x, y) = (bx * TILE + col, by * TILE + row);
                    for band in 0..spp {
                        if x >= WIDTH || y >= HEIGHT {
                            block.extend_from_slice(&endian.u16(0));
                            continue;
                        }
                        block.extend_from_slice(&endian.u16(sample_value(band, pixel_index(x, y))));
                    }
                }
            }
            blocks.push(block);
        }
    }

    let block_count = blocks.len() as u32;
    let byte_counts: Vec<u8> = blocks
        .iter()
        .flat_map(|b| endian.u32(b.len() as u32))
        .collect();
    let mut nodata_ascii = NODATA.to_string().into_bytes();
    nodata_ascii.push(0);

    let mut entries: Vec<Entry> = vec![
        (TiffTag::ImageWidth, LONG, 1, endian.u32(WIDTH).to_vec()),
        (TiffTag::ImageLength, LONG, 1, endian.u32(HEIGHT).to_vec()),
        (
            TiffTag::BitsPerSample,
            SHORT,
            u32::from(BANDS),
            (0..BANDS).flat_map(|_| endian.u16(16)).collect(),
        ),
        (TiffTag::Compression, SHORT, 1, endian.u16(1).to_vec()),
        (
            TiffTag::PhotometricInterpretation,
            SHORT,
            1,
            endian.u16(2).to_vec(),
        ),
        (
            TiffTag::SamplesPerPixel,
            SHORT,
            1,
            endian.u16(BANDS).to_vec(),
        ),
        (
            TiffTag::PlanarConfiguration,
            SHORT,
            1,
            endian.u16(1).to_vec(),
        ),
        (TiffTag::TileWidth, LONG, 1, endian.u32(TILE).to_vec()),
        (TiffTag::TileLength, LONG, 1, endian.u32(TILE).to_vec()),
        (
            TiffTag::TileOffsets,
            LONG,
            block_count,
            vec![0; block_count as usize * 4],
        ),
        (TiffTag::TileByteCounts, LONG, block_count, byte_counts),
        (TiffTag::SampleFormat, SHORT, 1, endian.u16(1).to_vec()),
        (
            TiffTag::GdalNodata,
            ASCII,
            nodata_ascii.len() as u32,
            nodata_ascii,
        ),
    ];
    entries.sort_by_key(|(tag, _, _, _)| *tag as u16);

    let ifd_offset = 8u32;
    let ifd_size = 2 + entries.len() as u32 * 12 + 4;
    let mut external_offsets: Vec<Option<u32>> = Vec::with_capacity(entries.len());
    let mut external_size = 0u32;
    for (_, _, _, payload) in &entries {
        if payload.len() <= 4 {
            external_offsets.push(None);
        } else {
            external_offsets.push(Some(ifd_offset + ifd_size + external_size));
            external_size += payload.len() as u32;
            external_size += external_size % 2;
        }
    }
    let data_start = ifd_offset + ifd_size + external_size;

    let mut cursor = data_start;
    let mut block_offsets = Vec::with_capacity(blocks.len());
    for block in &blocks {
        block_offsets.push(cursor);
        cursor += block.len() as u32;
    }
    let offsets_payload: Vec<u8> = block_offsets.iter().flat_map(|o| endian.u32(*o)).collect();
    for entry in entries.iter_mut() {
        if entry.0 == TiffTag::TileOffsets {
            entry.3 = offsets_payload.clone();
        }
    }

    let mut out = Vec::with_capacity(cursor as usize);
    out.extend_from_slice(endian.magic());
    out.extend_from_slice(&endian.u16(42));
    out.extend_from_slice(&endian.u32(ifd_offset));
    out.extend_from_slice(&endian.u16(entries.len() as u16));
    for (index, (tag, field_type, count, payload)) in entries.iter().enumerate() {
        out.extend_from_slice(&endian.u16(*tag as u16));
        out.extend_from_slice(&endian.u16(*field_type));
        out.extend_from_slice(&endian.u32(*count));
        match external_offsets[index] {
            Some(offset) => out.extend_from_slice(&endian.u32(offset)),
            None => {
                // A short value sits left-justified in the 4-byte value field,
                // in both byte orders.
                let mut inline = [0u8; 4];
                inline[..payload.len()].copy_from_slice(payload);
                out.extend_from_slice(&inline);
            }
        }
    }
    out.extend_from_slice(&endian.u32(0)); // no next IFD
    for (index, (_, _, _, payload)) in entries.iter().enumerate() {
        if external_offsets[index].is_some() {
            out.extend_from_slice(payload);
            if out.len() % 2 != 0 {
                out.push(0);
            }
        }
    }
    assert_eq!(out.len() as u32, data_start);
    for block in &blocks {
        out.extend_from_slice(block);
    }
    assert_eq!(out.len() as u32, cursor);
    out
}

/// Per-test scratch fixture inside the system temp dir (house policy: no
/// hardcoded absolute paths).
///
/// The leaf name embeds the process id and a monotonic counter, so no two test
/// binaries — nor two concurrent runs of this one — can ever land on the same
/// file.  Dropping the guard removes the fixture, so a panicking test leaks
/// nothing.
struct TempPath(PathBuf);

impl TempPath {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "oxigeo_issue14_qc_{}_{seq}_{name}",
            std::process::id()
        )))
    }
}

impl std::ops::Deref for TempPath {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TempPath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Writes the fixture into the platform temp dir and returns its guard.
fn fixture(name: &str, endian: Endian) -> TempPath {
    let path = TempPath::new(&format!("{name}_{}_{WIDTH}x{HEIGHT}.tif", endian.tag()));
    std::fs::write(&path, build_tiff(endian)).expect("write fixture");
    path
}

/// Every sample of `band`, in the row-major order the scanners walk.
fn expected_band(band: usize) -> Vec<f64> {
    (0..WIDTH * HEIGHT)
        .map(|index| f64::from(sample_value(band, index)))
        .collect()
}

// ---------------------------------------------------------------------------
// Sanity: the two fixtures really are the same raster, byte-swapped
// ---------------------------------------------------------------------------

#[test]
fn test_issue_14_big_endian_fixture_is_a_real_mm_file() {
    let le = build_tiff(Endian::Little);
    let be = build_tiff(Endian::Big);
    assert_eq!(&le[0..2], b"II");
    assert_eq!(&be[0..2], b"MM");
    assert_eq!(
        le.len(),
        be.len(),
        "the two fixtures must differ only in byte order"
    );
    assert_ne!(le, be, "an MM fixture identical to II proves nothing");
}

// ---------------------------------------------------------------------------
// NoData
// ---------------------------------------------------------------------------

#[test]
fn test_issue_14_nodata_scanner_honours_file_byte_order() {
    let mut reports = Vec::new();
    for endian in [Endian::Little, Endian::Big] {
        let path = fixture("nodata_endian", endian);
        let result = NoDataValidator::new()
            .check_file(&path)
            .unwrap_or_else(|e| panic!("{endian:?}: {e}"));

        assert_eq!(result.per_band.len(), BANDS as usize, "{endian:?}");
        for (band, stats) in result.per_band.iter().enumerate() {
            let expected = expected_band(band)
                .iter()
                .filter(|v| **v == f64::from(NODATA))
                .count() as u64;
            assert_eq!(
                stats.actual_nodata_count,
                expected,
                "{endian:?} band {band}: the sentinel 0x{NODATA:04X} must be \
                 matched against host-native samples. Any byte swap in the QC \
                 crate — whether by decoding an MM file little-endian, or by \
                 re-applying the file's byte order on top of the driver's \
                 normalisation — turns every sentinel into 0x{:04X} and \
                 collapses this count to 0",
                NODATA.swap_bytes()
            );
        }

        assert_eq!(result.per_band[0].actual_nodata_count, 8, "{endian:?}");
        assert_eq!(result.per_band[1].actual_nodata_count, 8, "{endian:?}");
        assert_eq!(result.per_band[2].actual_nodata_count, 0, "{endian:?}");
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.rule_id.as_deref() == Some("NODATA-COMMON-FOOTPRINT-OUTLIER")),
            "{endian:?}: expected the outlier warning, got {:#?}",
            result.issues
        );

        reports.push(
            result
                .per_band
                .iter()
                .map(|s| (s.actual_nodata_count, s.declared_nodata))
                .collect::<Vec<_>>(),
        );
    }

    assert_eq!(
        reports[0], reports[1],
        "II and MM encodings of one raster must yield one report"
    );
}

// ---------------------------------------------------------------------------
// Radiometric
// ---------------------------------------------------------------------------

#[test]
fn test_issue_14_radiometric_scanner_honours_file_byte_order() {
    // Wide enough that nothing is out of range: the point is which *numbers*
    // the samples decode to, not what the thresholds make of them.
    let profile = SensorProfile::Custom {
        ranges: (0..BANDS)
            .map(|_| BandRange {
                min: 0.0,
                max: 70_000.0,
                expected_mean: None,
                expected_std: None,
            })
            .collect(),
    };

    let mut reports = Vec::new();
    for endian in [Endian::Little, Endian::Big] {
        let path = fixture("radiometric_endian", endian);
        let result = RadiometricValidator::new(profile.clone())
            .check_file(&path)
            .unwrap_or_else(|e| panic!("{endian:?}: {e}"));

        assert_eq!(result.per_band.len(), BANDS as usize, "{endian:?}");
        for (band, stats) in result.per_band.iter().enumerate() {
            // 64 pixels ⇒ stride 1 ⇒ every pixel of every band is sampled.
            let expected = expected_band(band);
            let min = expected.iter().copied().fold(f64::INFINITY, f64::min);
            let max = expected.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let mean = expected.iter().sum::<f64>() / expected.len() as f64;

            assert_eq!(stats.band_idx, band, "{endian:?}");
            assert_eq!(
                stats.min_sampled, min,
                "{endian:?} band {band} min: samples reach the scanner in the \
                 host's byte order, already normalised by the driver — the QC \
                 crate must not swap them again"
            );
            assert_eq!(stats.max_sampled, max, "{endian:?} band {band} max");
            assert_eq!(stats.mean_sampled, mean, "{endian:?} band {band} mean");
        }

        reports.push(
            result
                .per_band
                .iter()
                .map(|s| (s.min_sampled, s.max_sampled, s.mean_sampled))
                .collect::<Vec<_>>(),
        );
    }

    assert_eq!(
        reports[0], reports[1],
        "II and MM encodings of one raster must yield one report"
    );
}
