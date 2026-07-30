//! Regression test for cool-japan/oxigeo#14 — QC scanners on
//! `PlanarConfiguration = 2` files.
//!
//! `CogReader::read_tile` is planar-blind: it hands back whatever block the flat
//! `tile_y * tiles_across + tile_x` index names, and in a planar file that grid
//! covers only the **first** plane while each block holds one band's samples
//! rather than `SamplesPerPixel` interleaved ones.
//!
//! Both raster scanners used to walk a file that way and de-interleave by hand,
//! so on a planar raster they
//!
//! * never looked at plane 1..n at all,
//! * ran past the end of every block after `1/SamplesPerPixel` of its pixels
//!   (their `bytes_per_pixel` was `spp` times the block's real pixel stride),
//!   and
//! * attributed the handful of samples they did read to the wrong bands.
//!
//! A NoData or radiometric report computed from ~`1/spp` of the wrong band is
//! worse than no report at all, so this test pins both scanners to the exact
//! per-band answer on a real planar file — and, as a control, checks that the
//! chunky answer is unchanged.
//!
//! The fixtures are hand-built: the crate's writer only ever emits
//! `PlanarConfiguration = Chunky`.

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
const NODATA: u16 = 65_535;

/// The fixture's sample values: band 0 and band 1 carry NoData sentinels on
/// every eighth pixel, band 2 carries none at all.
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
// Synthetic TIFF builder (uncompressed, tiled, UInt16, little-endian)
// ---------------------------------------------------------------------------

type Entry = (TiffTag, u16, u32, Vec<u8>);

const ASCII: u16 = 2;
const SHORT: u16 = 3;
const LONG: u16 = 4;

/// Serialises the fixture with the requested `PlanarConfiguration`.
fn build_tiff(planar: u16) -> Vec<u8> {
    let spp = BANDS as usize;
    let planes = if planar == 2 { spp } else { 1 };
    let samples_in_block = if planar == 2 { 1 } else { spp };
    let across = WIDTH.div_ceil(TILE);
    let down = HEIGHT.div_ceil(TILE);

    // Blocks in the plane-major order the TIFF spec mandates.
    let mut blocks: Vec<Vec<u8>> = Vec::with_capacity(planes * (across * down) as usize);
    for plane in 0..planes {
        for by in 0..down {
            for bx in 0..across {
                let mut block = Vec::with_capacity((TILE * TILE) as usize * samples_in_block * 2);
                for row in 0..TILE {
                    for col in 0..TILE {
                        let (x, y) = (bx * TILE + col, by * TILE + row);
                        for s in 0..samples_in_block {
                            if x >= WIDTH || y >= HEIGHT {
                                block.extend_from_slice(&0u16.to_le_bytes());
                                continue;
                            }
                            let band = plane * samples_in_block + s;
                            block.extend_from_slice(
                                &sample_value(band, pixel_index(x, y)).to_le_bytes(),
                            );
                        }
                    }
                }
                blocks.push(block);
            }
        }
    }

    let block_count = blocks.len() as u32;
    let byte_counts: Vec<u8> = blocks
        .iter()
        .flat_map(|b| (b.len() as u32).to_le_bytes())
        .collect();
    let mut nodata_ascii = NODATA.to_string().into_bytes();
    nodata_ascii.push(0);

    let mut entries: Vec<Entry> = vec![
        (TiffTag::ImageWidth, LONG, 1, WIDTH.to_le_bytes().to_vec()),
        (TiffTag::ImageLength, LONG, 1, HEIGHT.to_le_bytes().to_vec()),
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
            TiffTag::SamplesPerPixel,
            SHORT,
            1,
            BANDS.to_le_bytes().to_vec(),
        ),
        (
            TiffTag::PlanarConfiguration,
            SHORT,
            1,
            planar.to_le_bytes().to_vec(),
        ),
        (TiffTag::TileWidth, LONG, 1, TILE.to_le_bytes().to_vec()),
        (TiffTag::TileLength, LONG, 1, TILE.to_le_bytes().to_vec()),
        (
            TiffTag::TileOffsets,
            LONG,
            block_count,
            vec![0; block_count as usize * 4],
        ),
        (TiffTag::TileByteCounts, LONG, block_count, byte_counts),
        (TiffTag::SampleFormat, SHORT, 1, 1u16.to_le_bytes().to_vec()),
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
    let offsets_payload: Vec<u8> = block_offsets.iter().flat_map(|o| o.to_le_bytes()).collect();
    for entry in entries.iter_mut() {
        if entry.0 == TiffTag::TileOffsets {
            entry.3 = offsets_payload.clone();
        }
    }

    let mut out = Vec::with_capacity(cursor as usize);
    out.extend_from_slice(b"II");
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&ifd_offset.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (index, (tag, field_type, count, payload)) in entries.iter().enumerate() {
        out.extend_from_slice(&(*tag as u16).to_le_bytes());
        out.extend_from_slice(&field_type.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        match external_offsets[index] {
            Some(offset) => out.extend_from_slice(&offset.to_le_bytes()),
            None => {
                let mut inline = [0u8; 4];
                inline[..payload.len()].copy_from_slice(payload);
                out.extend_from_slice(&inline);
            }
        }
    }
    out.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
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
fn fixture(tag: &str, planar: u16) -> TempPath {
    let path = TempPath::new(&format!("{tag}_planar{planar}_{WIDTH}x{HEIGHT}.tif"));
    std::fs::write(&path, build_tiff(planar)).expect("write fixture");
    path
}

/// Every sample of `band`, in the row-major order the scanners walk.
fn expected_band(band: usize) -> Vec<f64> {
    (0..WIDTH * HEIGHT)
        .map(|index| f64::from(sample_value(band, index)))
        .collect()
}

// ---------------------------------------------------------------------------
// NoData
// ---------------------------------------------------------------------------

#[test]
fn test_issue_14_nodata_scanner_sees_every_planar_band() {
    for planar in [1u16, 2] {
        let path = fixture("nodata", planar);
        let result = NoDataValidator::new()
            .check_file(&path)
            .unwrap_or_else(|e| panic!("planar={planar}: {e}"));

        assert_eq!(result.per_band.len(), BANDS as usize, "planar={planar}");
        for (band, stats) in result.per_band.iter().enumerate() {
            let expected = expected_band(band)
                .iter()
                .filter(|v| **v == f64::from(NODATA))
                .count() as u64;
            assert_eq!(
                stats.actual_nodata_count, expected,
                "planar={planar} band {band}: the scanner must count every NoData \
                 pixel of every band, not ~1/spp of plane 0"
            );
            assert_eq!(stats.declared_nodata, Some(f64::from(NODATA)));
        }

        // Bands 1 and 2 (1-based) share a footprint; band 3 has none, which is
        // exactly the fill-value-pollution shape the outlier rule exists for.
        assert_eq!(result.per_band[0].actual_nodata_count, 8);
        assert_eq!(result.per_band[1].actual_nodata_count, 8);
        assert_eq!(result.per_band[2].actual_nodata_count, 0);
        assert_eq!(
            result.common_footprint_count, 0,
            "planar={planar}: band 3 has no NoData, so no cell is NoData everywhere"
        );
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.rule_id.as_deref() == Some("NODATA-COMMON-FOOTPRINT-OUTLIER")),
            "planar={planar}: expected the outlier warning, got {:#?}",
            result.issues
        );
    }
}

// ---------------------------------------------------------------------------
// Radiometric
// ---------------------------------------------------------------------------

#[test]
fn test_issue_14_radiometric_scanner_samples_every_planar_band() {
    // A custom profile wide enough that nothing is out of range: the point here
    // is *which* samples are read, not what the thresholds make of them.
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

    for planar in [1u16, 2] {
        let path = fixture("radiometric", planar);
        let result = RadiometricValidator::new(profile.clone())
            .check_file(&path)
            .unwrap_or_else(|e| panic!("planar={planar}: {e}"));

        assert_eq!(result.per_band.len(), BANDS as usize, "planar={planar}");
        for (band, stats) in result.per_band.iter().enumerate() {
            // 64 pixels ⇒ stride 1 ⇒ every pixel of every band is sampled.
            let expected = expected_band(band);
            let min = expected.iter().copied().fold(f64::INFINITY, f64::min);
            let max = expected.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let mean = expected.iter().sum::<f64>() / expected.len() as f64;

            assert_eq!(stats.band_idx, band);
            assert_eq!(
                stats.min_sampled, min,
                "planar={planar} band {band} min: the sample set must be band \
                 {band}'s pixels, not plane 0's first few"
            );
            assert_eq!(stats.max_sampled, max, "planar={planar} band {band} max");
            assert_eq!(stats.mean_sampled, mean, "planar={planar} band {band} mean");
            assert_eq!(stats.oor_fraction, 0.0, "planar={planar} band {band} oor");
        }
    }
}
