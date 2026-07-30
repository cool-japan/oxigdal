//! Regression tests for cool-japan/oxigeo#14 — per-band and windowed reads.
//!
//! Three defects are covered here:
//!
//! 1. **`read_band` ignored its `band` argument.** It returned the whole
//!    pixel-interleaved plane (`w·h·bytes·samples`) regardless of which band was
//!    asked for, so every multi-band GeoTIFF was unreadable through
//!    `Dataset::read_band` (the facade feeds the result to `RasterBuffer::new`,
//!    which wants `w·h·bytes`). Now each band is de-interleaved (chunky) or
//!    plane-selected (planar).
//! 2. **There was no read-into-caller-buffer API.** `read_band_into` /
//!    `read_band_into_typed` decode straight into memory the caller owns, fusing
//!    the element-type conversion into the same pass — the GDAL
//!    `RasterBand::read_into_slice` equivalent the issue asks for.
//! 3. **There was no real windowed read.** `read_window*` now touches only the
//!    tiles/strips that overlap the window instead of decoding the whole band.
//!
//! Layouts are checked against hand-built TIFFs as well as writer-produced ones,
//! because the crate's writer only ever emits `PlanarConfiguration = Chunky`.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::env;
#[cfg(any(feature = "deflate", feature = "lzw"))]
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use oxigeo_core::error::{OxiGeoError, Result};
use oxigeo_core::io::{ByteRange, DataSource};
use oxigeo_core::types::RasterDataType;
use oxigeo_geotiff::GeoTiffReader;
use oxigeo_geotiff::tiff::TiffTag;
// Only the codec-gated speed-evidence cases below go through the writer and a
// real file; the synthetic-TIFF cases need neither.
#[cfg(any(feature = "deflate", feature = "lzw"))]
use oxigeo_core::io::FileDataSource;
#[cfg(any(feature = "deflate", feature = "lzw"))]
use oxigeo_geotiff::writer::{
    GeoTiffWriter, GeoTiffWriterOptions, OverviewResampling, WriterConfig,
};

// ---------------------------------------------------------------------------
// Data sources
// ---------------------------------------------------------------------------

/// Plain in-memory data source.
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
                message: format!("invalid range {}..{}", range.start, range.end),
            });
        }
        Ok(self.0[start..end].to_vec())
    }
}

/// In-memory data source that counts the bytes it hands out, so a test can prove
/// a windowed read really does skip the blocks it does not need.
#[derive(Debug)]
struct CountingSource {
    data: Vec<u8>,
    bytes: Arc<AtomicUsize>,
}

impl DataSource for CountingSource {
    fn size(&self) -> Result<u64> {
        Ok(self.data.len() as u64)
    }

    fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
        let start = range.start as usize;
        let end = (range.end as usize).min(self.data.len());
        if start > end {
            return Err(OxiGeoError::OutOfBounds {
                message: format!("invalid range {}..{}", range.start, range.end),
            });
        }
        self.bytes.fetch_add(end - start, Ordering::Relaxed);
        Ok(self.data[start..end].to_vec())
    }
}

// ---------------------------------------------------------------------------
// Synthetic TIFF builder (uncompressed; chunky *and* planar)
// ---------------------------------------------------------------------------

/// One uncompressed raster layout to synthesise.
#[derive(Debug, Clone, Copy)]
struct Spec {
    label: &'static str,
    width: u32,
    height: u32,
    samples_per_pixel: u16,
    bits_per_sample: u16,
    /// TIFF `SampleFormat`: 1 = unsigned, 2 = signed, 3 = IEEE float.
    sample_format: u16,
    /// TIFF `PlanarConfiguration`: 1 = chunky (interleaved), 2 = planar.
    planar: u16,
    /// `Some((tile_w, tile_h))` for a tiled layout, `None` for strips.
    tile: Option<(u32, u32)>,
    /// `RowsPerStrip`, used only when `tile` is `None`.
    rows_per_strip: u32,
}

impl Spec {
    fn bytes_per_sample(&self) -> usize {
        (self.bits_per_sample / 8) as usize
    }

    fn pixel_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    fn data_type(&self) -> RasterDataType {
        match (self.sample_format, self.bits_per_sample) {
            (3, 32) => RasterDataType::Float32,
            (3, 64) => RasterDataType::Float64,
            (2, 16) => RasterDataType::Int16,
            (1, 16) => RasterDataType::UInt16,
            (1, 32) => RasterDataType::UInt32,
            _ => RasterDataType::UInt8,
        }
    }

    /// Interleaved (chunky) reference image: one deterministic value per sample.
    fn pattern(&self) -> Vec<u8> {
        let bps = self.bytes_per_sample();
        let mut out =
            Vec::with_capacity(self.pixel_count() * self.samples_per_pixel as usize * bps);
        for y in 0..self.height {
            for x in 0..self.width {
                for band in 0..self.samples_per_pixel {
                    let n = u64::from(x) * 7 + u64::from(y) * 131 + u64::from(band) * 1_000_003;
                    match (self.sample_format, self.bits_per_sample) {
                        (3, 32) => out.extend_from_slice(&(n as f32 * 0.25 - 500.0).to_le_bytes()),
                        (3, 64) => out.extend_from_slice(&(n as f64 * 0.125 - 900.0).to_le_bytes()),
                        (2, 16) => {
                            out.extend_from_slice(&((n as i64 % 30_000) as i16).to_le_bytes())
                        }
                        (1, 16) => out.extend_from_slice(&((n % 65_536) as u16).to_le_bytes()),
                        (1, 32) => {
                            out.extend_from_slice(&((n % 4_000_000_000) as u32).to_le_bytes())
                        }
                        _ => out.push((n % 256) as u8),
                    }
                }
            }
        }
        out
    }

    /// The bytes `read_band(_, band)` must return for this layout.
    fn expected_band(&self, interleaved: &[u8], band: usize) -> Vec<u8> {
        let bps = self.bytes_per_sample();
        let spp = self.samples_per_pixel as usize;
        let mut out = Vec::with_capacity(self.pixel_count() * bps);
        for pixel in 0..self.pixel_count() {
            let start = (pixel * spp + band) * bps;
            out.extend_from_slice(&interleaved[start..start + bps]);
        }
        out
    }

    fn blocks_across(&self) -> u32 {
        match self.tile {
            Some((tw, _)) => self.width.div_ceil(tw),
            None => 1,
        }
    }

    fn blocks_down(&self) -> u32 {
        match self.tile {
            Some((_, th)) => self.height.div_ceil(th),
            None => self.height.div_ceil(self.rows_per_strip),
        }
    }
}

/// Serialises `interleaved` into the on-disk block order this spec describes and
/// wraps it in a little-endian classic TIFF.
fn build_tiff(spec: &Spec, interleaved: &[u8]) -> Vec<u8> {
    let bps = spec.bytes_per_sample();
    let spp = spec.samples_per_pixel as usize;
    let planes = if spec.planar == 2 { spp } else { 1 };
    let samples_in_block = if spec.planar == 2 { 1 } else { spp };
    let (block_w, block_h) = match spec.tile {
        Some((tw, th)) => (tw, th),
        None => (spec.width, spec.rows_per_strip),
    };
    let across = spec.blocks_across();
    let down = spec.blocks_down();

    // Block payloads, in the plane-major order the TIFF spec mandates.
    let mut blocks: Vec<Vec<u8>> = Vec::with_capacity(planes * across as usize * down as usize);
    for plane in 0..planes {
        for by in 0..down {
            let rows = if spec.tile.is_some() {
                block_h
            } else {
                (spec.height - by * block_h).min(block_h)
            };
            for bx in 0..across {
                let mut block =
                    Vec::with_capacity(block_w as usize * rows as usize * samples_in_block * bps);
                for row in 0..rows {
                    let y = by * block_h + row;
                    for col in 0..block_w {
                        let x = bx * block_w + col;
                        for s in 0..samples_in_block {
                            if x >= spec.width || y >= spec.height {
                                // Tile padding outside the image.
                                block.extend(std::iter::repeat_n(0u8, bps));
                                continue;
                            }
                            let band = plane * samples_in_block + s;
                            let start = ((y as usize * spec.width as usize + x as usize) * spp
                                + band)
                                * bps;
                            block.extend_from_slice(&interleaved[start..start + bps]);
                        }
                    }
                }
                blocks.push(block);
            }
        }
    }

    // --- IFD layout -------------------------------------------------------
    // Entries must be ordered by ascending tag.
    let block_count = blocks.len() as u32;
    let mut entries: Vec<(TiffTag, u16, u32, Vec<u8>)> = Vec::new();
    const SHORT: u16 = 3;
    const LONG: u16 = 4;

    entries.push((
        TiffTag::ImageWidth,
        LONG,
        1,
        spec.width.to_le_bytes().to_vec(),
    ));
    entries.push((
        TiffTag::ImageLength,
        LONG,
        1,
        spec.height.to_le_bytes().to_vec(),
    ));
    let bits: Vec<u8> = (0..spp)
        .flat_map(|_| spec.bits_per_sample.to_le_bytes())
        .collect();
    entries.push((TiffTag::BitsPerSample, SHORT, spp as u32, bits));
    entries.push((TiffTag::Compression, SHORT, 1, 1u16.to_le_bytes().to_vec()));
    let photometric: u16 = if spp >= 3 { 2 } else { 1 };
    entries.push((
        TiffTag::PhotometricInterpretation,
        SHORT,
        1,
        photometric.to_le_bytes().to_vec(),
    ));
    if spec.tile.is_none() {
        entries.push((
            TiffTag::StripOffsets,
            LONG,
            block_count,
            vec![0; block_count as usize * 4],
        ));
    }
    entries.push((
        TiffTag::SamplesPerPixel,
        SHORT,
        1,
        spec.samples_per_pixel.to_le_bytes().to_vec(),
    ));
    if spec.tile.is_none() {
        entries.push((
            TiffTag::RowsPerStrip,
            LONG,
            1,
            spec.rows_per_strip.to_le_bytes().to_vec(),
        ));
        let counts: Vec<u8> = blocks
            .iter()
            .flat_map(|b| (b.len() as u32).to_le_bytes())
            .collect();
        entries.push((TiffTag::StripByteCounts, LONG, block_count, counts));
    }
    entries.push((
        TiffTag::PlanarConfiguration,
        SHORT,
        1,
        spec.planar.to_le_bytes().to_vec(),
    ));
    if let Some((tw, th)) = spec.tile {
        entries.push((TiffTag::TileWidth, LONG, 1, tw.to_le_bytes().to_vec()));
        entries.push((TiffTag::TileLength, LONG, 1, th.to_le_bytes().to_vec()));
        entries.push((
            TiffTag::TileOffsets,
            LONG,
            block_count,
            vec![0; block_count as usize * 4],
        ));
        let counts: Vec<u8> = blocks
            .iter()
            .flat_map(|b| (b.len() as u32).to_le_bytes())
            .collect();
        entries.push((TiffTag::TileByteCounts, LONG, block_count, counts));
    }
    entries.push((
        TiffTag::SampleFormat,
        SHORT,
        1,
        spec.sample_format.to_le_bytes().to_vec(),
    ));
    entries.sort_by_key(|(tag, _, _, _)| *tag as u16);

    let ifd_offset = 8u32;
    let ifd_size = 2 + entries.len() as u32 * 12 + 4;
    // Out-of-line payloads, word-aligned, laid out right after the IFD.
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

    // Now the block offsets are known; patch the offsets array in place.
    let mut block_offsets = Vec::with_capacity(blocks.len());
    let mut cursor = data_start;
    for block in &blocks {
        block_offsets.push(cursor);
        cursor += block.len() as u32;
    }
    let offsets_payload: Vec<u8> = block_offsets.iter().flat_map(|o| o.to_le_bytes()).collect();
    let offsets_tag = if spec.tile.is_some() {
        TiffTag::TileOffsets
    } else {
        TiffTag::StripOffsets
    };
    for entry in entries.iter_mut() {
        if entry.0 == offsets_tag {
            entry.3 = offsets_payload.clone();
        }
    }

    // --- Emit -------------------------------------------------------------
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
    assert_eq!(out.len() as u32, ifd_offset + ifd_size);

    for (index, (_, _, _, payload)) in entries.iter().enumerate() {
        if external_offsets[index].is_some() {
            assert_eq!(
                out.len() as u32,
                external_offsets[index].unwrap_or_default()
            );
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

/// The matrix of layouts every band-extraction assertion runs over.
fn layout_matrix() -> Vec<Spec> {
    let mut specs = Vec::new();
    for &planar in &[1u16, 2] {
        for &(label_kind, tile, rows) in &[
            ("tiled", Some((16u32, 16u32)), 0u32),
            ("striped", None, 7u32),
        ] {
            for &(sample_format, bits) in &[(1u16, 8u16), (1, 16), (2, 16), (3, 32), (3, 64)] {
                let label: &'static str = Box::leak(
                    format!("planar{planar}_{label_kind}_fmt{sample_format}_bits{bits}")
                        .into_boxed_str(),
                );
                specs.push(Spec {
                    label,
                    // Deliberately not a multiple of the tile size, so the right
                    // and bottom edges are partial tiles.
                    width: 37,
                    height: 23,
                    samples_per_pixel: 3,
                    bits_per_sample: bits,
                    sample_format,
                    planar,
                    tile,
                    rows_per_strip: if tile.is_none() { rows } else { 0 },
                });
            }
        }
    }
    specs
}

/// An RAII fixture path inside [`std::env::temp_dir`].
///
/// The leaf name embeds the process id and a monotonic counter, so no two test
/// binaries — nor two concurrent runs of this one — can ever land on the same
/// file.  Dropping the guard removes the fixture, so a panicking test leaks
/// nothing.
#[cfg(any(feature = "deflate", feature = "lzw"))]
struct TempPath(PathBuf);

#[cfg(any(feature = "deflate", feature = "lzw"))]
impl TempPath {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(env::temp_dir().join(format!(
            "oxigeo_geotiff_issue14_band_{}_{seq}_{name}",
            std::process::id()
        )))
    }
}

#[cfg(any(feature = "deflate", feature = "lzw"))]
impl std::ops::Deref for TempPath {
    type Target = std::path::Path;

    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

#[cfg(any(feature = "deflate", feature = "lzw"))]
impl AsRef<std::path::Path> for TempPath {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

#[cfg(any(feature = "deflate", feature = "lzw"))]
impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(any(feature = "deflate", feature = "lzw"))]
fn temp_test_file(name: &str) -> TempPath {
    TempPath::new(name)
}

// ---------------------------------------------------------------------------
// T1 — read_band honours the band index
// ---------------------------------------------------------------------------

/// The headline bug: `read_band(level, band)` used to discard `band` and return
/// the whole interleaved plane, so every multi-band file was broken. Each band
/// must now come back de-interleaved (chunky) or plane-selected (planar), at the
/// single-band length `w·h·bytes_per_sample`.
#[test]
fn test_issue_14_read_band_extracts_each_band() {
    for spec in layout_matrix() {
        let interleaved = spec.pattern();
        let reader = GeoTiffReader::open(MemorySource(build_tiff(&spec, &interleaved)))
            .unwrap_or_else(|e| panic!("{}: open failed: {e}", spec.label));

        assert_eq!(reader.width(), u64::from(spec.width), "{}", spec.label);
        assert_eq!(reader.height(), u64::from(spec.height), "{}", spec.label);
        assert_eq!(
            reader.band_count(),
            u32::from(spec.samples_per_pixel),
            "{}",
            spec.label
        );

        let single_band_len = spec.pixel_count() * spec.bytes_per_sample();
        assert_eq!(
            reader.band_byte_len(0).expect("band_byte_len"),
            single_band_len,
            "{}: band_byte_len",
            spec.label
        );

        for band in 0..spec.samples_per_pixel as usize {
            let actual = reader
                .read_band(0, band)
                .unwrap_or_else(|e| panic!("{}: read_band({band}) failed: {e}", spec.label));
            assert_eq!(
                actual.len(),
                single_band_len,
                "{}: band {band} must be a single band, not the interleaved plane",
                spec.label
            );
            assert_eq!(
                actual,
                spec.expected_band(&interleaved, band),
                "{}: band {band} samples",
                spec.label
            );
        }

        // Bands past the end are rejected instead of silently aliasing band 0.
        let err = reader
            .read_band(0, spec.samples_per_pixel as usize)
            .expect_err("out-of-range band must error");
        assert!(
            format!("{err}").contains("band"),
            "{}: unexpected error: {err}",
            spec.label
        );
    }
}

/// The exact failure reported on the issue: a writer-produced RGB file read
/// through the driver used to hand back 3x too many bytes, which is why
/// `RasterBuffer::new` rejected it with "Data size mismatch".
#[test]
#[cfg(feature = "lzw")]
fn test_issue_14_writer_rgb_band_is_single_band() {
    use oxigeo_core::buffer::RasterBuffer;
    use oxigeo_core::types::NoDataValue;

    let path = temp_test_file("writer_rgb.tif");
    let (width, height) = (64u64, 64u64);
    let mut interleaved = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            interleaved.push((x % 256) as u8);
            interleaved.push((y % 256) as u8);
            interleaved.push(((x + y) % 256) as u8);
        }
    }
    {
        let config = WriterConfig::new(width, height, 3, RasterDataType::UInt8)
            .with_compression(oxigeo_geotiff::Compression::Lzw)
            .with_tile_size(16, 16)
            .with_overviews(false, OverviewResampling::Nearest);
        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("create writer");
        writer.write(&interleaved).expect("write RGB");
    }

    let reader = GeoTiffReader::open(FileDataSource::open(&path).expect("open")).expect("reader");
    for band in 0..3usize {
        let plane = reader.read_band(0, band).expect("read_band");
        assert_eq!(plane.len(), (width * height) as usize);
        let expected: Vec<u8> = (0..(width * height) as usize)
            .map(|i| interleaved[i * 3 + band])
            .collect();
        assert_eq!(plane, expected, "band {band}");
        // The exact thing that used to fail: the facade wraps this in a
        // RasterBuffer sized w*h*size_bytes.
        RasterBuffer::new(
            plane,
            width,
            height,
            RasterDataType::UInt8,
            NoDataValue::None,
        )
        .expect("RasterBuffer must accept a single-band buffer");
    }

    let _ = std::fs::remove_file(path);
}

// ---------------------------------------------------------------------------
// T2 — read into a caller-supplied buffer
// ---------------------------------------------------------------------------

/// `read_band_into` must produce exactly what `read_band` produces, for every
/// layout, without allocating a band-sized intermediate.
#[test]
fn test_issue_14_read_band_into_matches_read_band() {
    for spec in layout_matrix() {
        let interleaved = spec.pattern();
        let reader = GeoTiffReader::open(MemorySource(build_tiff(&spec, &interleaved)))
            .unwrap_or_else(|e| panic!("{}: open failed: {e}", spec.label));
        let len = reader.band_byte_len(0).expect("band_byte_len");

        for band in 0..spec.samples_per_pixel as usize {
            let owned = reader.read_band(0, band).expect("read_band");
            // Poison the destination so a partial write would be visible.
            let mut into = vec![0xA5u8; len];
            reader
                .read_band_into(0, band, &mut into)
                .expect("read_band_into");
            assert_eq!(into, owned, "{}: band {band}", spec.label);
        }
    }
}

/// `read_band_into_typed::<f64>` on a Float32 file must equal the manual
/// `cast_slice` + `as f64` the issue's reporter is hand-rolling today — but with
/// no full-size intermediates.
#[test]
fn test_issue_14_read_band_into_typed_matches_manual_cast() {
    let spec = Spec {
        label: "typed_f32",
        width: 37,
        height: 23,
        samples_per_pixel: 3,
        bits_per_sample: 32,
        sample_format: 3,
        planar: 1,
        tile: Some((16, 16)),
        rows_per_strip: 0,
    };
    let interleaved = spec.pattern();
    let reader = GeoTiffReader::open(MemorySource(build_tiff(&spec, &interleaved))).expect("open");
    let pixels = reader.band_pixel_count(0).expect("band_pixel_count");
    assert_eq!(pixels, spec.pixel_count());

    for band in 0..spec.samples_per_pixel as usize {
        let raw = reader.read_band(0, band).expect("read_band");
        let manual: Vec<f64> = raw
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f64)
            .collect();

        let mut fused = vec![0.0f64; pixels];
        reader
            .read_band_into_typed(0, band, &mut fused)
            .expect("read_band_into_typed");
        assert_eq!(fused, manual, "band {band} as f64");

        // Same file into f32 (the memcpy fast path) and into i32 (saturating,
        // round-half-away-from-zero).
        let mut same: Vec<f32> = vec![0.0; pixels];
        reader
            .read_band_into_typed(0, band, &mut same)
            .expect("read_band_into_typed f32");
        let manual_f32: Vec<f32> = raw
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        assert_eq!(same, manual_f32, "band {band} as f32");
    }
}

/// Typed reads of integer rasters go through the same fused path.
#[test]
fn test_issue_14_read_band_into_typed_integer_sources() {
    for spec in layout_matrix() {
        if spec.sample_format == 3 {
            continue;
        }
        let interleaved = spec.pattern();
        let reader = GeoTiffReader::open(MemorySource(build_tiff(&spec, &interleaved)))
            .unwrap_or_else(|e| panic!("{}: open failed: {e}", spec.label));
        let pixels = reader.band_pixel_count(0).expect("pixels");
        let bps = spec.bytes_per_sample();
        let data_type = spec.data_type();

        for band in 0..spec.samples_per_pixel as usize {
            let raw = reader.read_band(0, band).expect("read_band");
            let manual: Vec<f64> = raw
                .chunks_exact(bps)
                .map(|c| match data_type {
                    RasterDataType::UInt8 => f64::from(c[0]),
                    RasterDataType::UInt16 => f64::from(u16::from_le_bytes([c[0], c[1]])),
                    RasterDataType::Int16 => f64::from(i16::from_le_bytes([c[0], c[1]])),
                    RasterDataType::UInt32 => {
                        f64::from(u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    }
                    other => panic!("unexpected data type {other:?}"),
                })
                .collect();
            let mut fused = vec![0.0f64; pixels];
            reader
                .read_band_into_typed(0, band, &mut fused)
                .expect("read_band_into_typed");
            assert_eq!(fused, manual, "{}: band {band}", spec.label);
        }
    }
}

/// Wrong-sized destinations must be rejected, never truncated or over-written.
#[test]
fn test_issue_14_destination_length_is_validated() {
    let spec = Spec {
        label: "len_check",
        width: 20,
        height: 12,
        samples_per_pixel: 2,
        bits_per_sample: 16,
        sample_format: 1,
        planar: 1,
        tile: Some((8, 8)),
        rows_per_strip: 0,
    };
    let reader =
        GeoTiffReader::open(MemorySource(build_tiff(&spec, &spec.pattern()))).expect("open");
    let len = reader.band_byte_len(0).expect("len");
    let pixels = reader.band_pixel_count(0).expect("pixels");
    assert_eq!(len, pixels * 2);

    for wrong in [len - 1, len + 1] {
        let mut dst = vec![0u8; wrong];
        let err = reader
            .read_band_into(0, 0, &mut dst)
            .expect_err("wrong dst length must be rejected");
        assert!(
            format!("{err}").contains("destination length"),
            "unexpected error: {err}"
        );
    }
    for wrong in [pixels - 1, pixels + 1] {
        let mut dst = vec![0.0f64; wrong];
        let err = reader
            .read_band_into_typed(0, 0, &mut dst)
            .expect_err("wrong typed dst length must be rejected");
        assert!(
            format!("{err}").contains("destination length"),
            "unexpected error: {err}"
        );
    }

    let mut window_dst = vec![0u8; 4 * 4 * 2 + 1];
    let err = reader
        .read_window_into(0, 0, 2, 3, 4, 4, &mut window_dst)
        .expect_err("wrong window dst length must be rejected");
    assert!(
        format!("{err}").contains("destination length"),
        "unexpected error: {err}"
    );
}

// ---------------------------------------------------------------------------
// T3 — real windowed reads
// ---------------------------------------------------------------------------

/// Crops a full band the naive way, to check the tile-clipped reader against.
fn crop(band: &[u8], width: usize, bps: usize, x: usize, y: usize, w: usize, h: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(w * h * bps);
    for row in 0..h {
        let start = ((y + row) * width + x) * bps;
        out.extend_from_slice(&band[start..start + w * bps]);
    }
    out
}

/// A windowed read must equal the equivalent crop of a full read — for tiled and
/// striped layouts, for chunky and planar storage, for windows that cross block
/// boundaries and for windows that land on the partial edge blocks.
#[test]
fn test_issue_14_read_window_matches_full_read_crop() {
    // (x, y, w, h) against a 37x23 raster with 16x16 tiles / 7-row strips.
    let windows: &[(u64, u64, u64, u64)] = &[
        (0, 0, 37, 23), // the whole raster
        (0, 0, 1, 1),   // a single pixel
        (36, 22, 1, 1), // the last pixel (bottom-right partial block)
        (14, 5, 8, 9),  // crosses a tile boundary in both axes
        (32, 16, 5, 7), // entirely inside the partial edge blocks
        (0, 6, 37, 3),  // full-width band crossing a strip boundary
        (17, 0, 3, 23), // full-height column inside one tile column
    ];

    for spec in layout_matrix() {
        let interleaved = spec.pattern();
        let reader = GeoTiffReader::open(MemorySource(build_tiff(&spec, &interleaved)))
            .unwrap_or_else(|e| panic!("{}: open failed: {e}", spec.label));
        let bps = spec.bytes_per_sample();

        for band in 0..spec.samples_per_pixel as usize {
            let full = reader.read_band(0, band).expect("read_band");
            for &(x, y, w, h) in windows {
                let expected = crop(
                    &full,
                    spec.width as usize,
                    bps,
                    x as usize,
                    y as usize,
                    w as usize,
                    h as usize,
                );
                let actual = reader
                    .read_window(0, band, x, y, w, h)
                    .unwrap_or_else(|e| panic!("{}: read_window failed: {e}", spec.label));
                assert_eq!(
                    actual, expected,
                    "{}: band {band} window [{x},{y} {w}x{h}]",
                    spec.label
                );

                let mut into = vec![0x5Au8; (w * h) as usize * bps];
                reader
                    .read_window_into(0, band, x, y, w, h, &mut into)
                    .expect("read_window_into");
                assert_eq!(into, expected, "{}: read_window_into", spec.label);
            }
        }
    }
}

/// The typed windowed read fuses the conversion, exactly like the full-band one.
#[test]
fn test_issue_14_read_window_into_typed_matches_manual_cast() {
    let spec = Spec {
        label: "typed_window_f32",
        width: 37,
        height: 23,
        samples_per_pixel: 2,
        bits_per_sample: 32,
        sample_format: 3,
        planar: 2,
        tile: Some((16, 16)),
        rows_per_strip: 0,
    };
    let reader =
        GeoTiffReader::open(MemorySource(build_tiff(&spec, &spec.pattern()))).expect("open");

    for band in 0..spec.samples_per_pixel as usize {
        let raw = reader
            .read_window(0, band, 14, 5, 8, 9)
            .expect("read_window");
        let manual: Vec<f64> = raw
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f64)
            .collect();
        let mut fused = vec![0.0f64; 8 * 9];
        reader
            .read_window_into_typed(0, band, 14, 5, 8, 9, &mut fused)
            .expect("read_window_into_typed");
        assert_eq!(fused, manual, "band {band}");
    }
}

/// A windowed read must actually skip the blocks it does not overlap. The
/// pre-fix facade decoded the entire band and cropped, so this counted the whole
/// file either way.
#[test]
fn test_issue_14_read_window_touches_only_overlapping_blocks() {
    let spec = Spec {
        label: "window_io",
        width: 256,
        height: 256,
        samples_per_pixel: 1,
        bits_per_sample: 32,
        sample_format: 3,
        planar: 1,
        tile: Some((32, 32)),
        rows_per_strip: 0,
    };
    let bytes = build_tiff(&spec, &spec.pattern());
    let counters = Arc::new(AtomicUsize::new(0));
    let reader = GeoTiffReader::open(CountingSource {
        data: bytes,
        bytes: Arc::clone(&counters),
    })
    .expect("open");

    // One 32x32 tile's worth of pixels, aligned to a single tile.
    counters.store(0, Ordering::Relaxed);
    let _ = reader.read_window(0, 0, 64, 64, 32, 32).expect("window");
    let window_bytes = counters.load(Ordering::Relaxed);

    counters.store(0, Ordering::Relaxed);
    let _ = reader.read_band(0, 0).expect("full band");
    let full_bytes = counters.load(Ordering::Relaxed);

    let tile_bytes = 32 * 32 * 4;
    assert_eq!(
        window_bytes, tile_bytes,
        "a tile-aligned window must read exactly one tile ({tile_bytes} bytes)"
    );
    assert_eq!(full_bytes, 256 * 256 * 4, "the full band reads every tile");
    assert!(
        window_bytes * 60 < full_bytes,
        "windowed read ({window_bytes} B) must be far cheaper than the full band ({full_bytes} B)"
    );
}

/// Invalid windows are rejected with typed errors rather than clamped or
/// silently returning short buffers.
#[test]
fn test_issue_14_window_bounds_are_validated() {
    let spec = Spec {
        label: "window_bounds",
        width: 20,
        height: 12,
        samples_per_pixel: 1,
        bits_per_sample: 8,
        sample_format: 1,
        planar: 1,
        tile: Some((8, 8)),
        rows_per_strip: 0,
    };
    let reader =
        GeoTiffReader::open(MemorySource(build_tiff(&spec, &spec.pattern()))).expect("open");

    for &(x, y, w, h, what) in &[
        (0u64, 0u64, 0u64, 4u64, "zero width"),
        (0, 0, 4, 0, "zero height"),
        (0, 0, 21, 12, "past the right edge"),
        (0, 0, 20, 13, "past the bottom edge"),
        (20, 0, 1, 1, "origin past the right edge"),
        (0, 12, 1, 1, "origin past the bottom edge"),
        (u64::MAX, 0, 1, 1, "overflowing origin"),
    ] {
        let err = reader
            .read_window(0, 0, x, y, w, h)
            .unwrap_err_or_else(what);
        assert!(
            format!("{err}").contains("window"),
            "{what}: unexpected error: {err}"
        );
    }

    let err = reader
        .read_window(0, 1, 0, 0, 4, 4)
        .expect_err("band 1 of a 1-band raster must be rejected");
    assert!(format!("{err}").contains("band"), "unexpected error: {err}");
}

/// Small helper so the loop above reads cleanly.
trait UnwrapErrOrElse<T> {
    fn unwrap_err_or_else(self, what: &str) -> OxiGeoError;
}

impl<T: std::fmt::Debug> UnwrapErrOrElse<T> for Result<T> {
    fn unwrap_err_or_else(self, what: &str) -> OxiGeoError {
        match self {
            Ok(value) => panic!("{what} must be rejected, got Ok({value:?})"),
            Err(err) => err,
        }
    }
}

// ---------------------------------------------------------------------------
// Overview levels
// ---------------------------------------------------------------------------

/// `read_band`'s `level` argument used to be forwarded to `read_tile` while the
/// surrounding geometry came from the *full-resolution* image, so any overview
/// read produced garbage. The level's own geometry is now resolved from its IFD.
#[test]
#[cfg(feature = "lzw")]
fn test_issue_14_read_band_uses_the_requested_level_geometry() {
    use oxigeo_geotiff::writer::{CogWriter, CogWriterOptions};

    let path = temp_test_file("levels.tif");
    let (width, height) = (128u64, 128u64);
    let data: Vec<u8> = (0..(width * height) as usize)
        .map(|i| (i % 251) as u8)
        .collect();
    {
        let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
            .with_compression(oxigeo_geotiff::Compression::Lzw)
            .with_tile_size(32, 32)
            .with_overviews(true, OverviewResampling::Nearest)
            .with_overview_levels(vec![2, 4]);
        let mut writer =
            CogWriter::create(&path, config, CogWriterOptions::default()).expect("create COG");
        writer.write(&data).expect("write COG");
    }

    let reader = GeoTiffReader::open(FileDataSource::open(&path).expect("open")).expect("reader");
    assert_eq!(reader.overview_count(), 2, "two overviews were requested");

    // Level 0 is the full-resolution image; levels 1 and 2 are /2 and /4.
    for (level, expected_side) in [(0usize, 128usize), (1, 64), (2, 32)] {
        let len = reader
            .band_byte_len(level)
            .unwrap_or_else(|e| panic!("level {level}: band_byte_len failed: {e}"));
        assert_eq!(
            len,
            expected_side * expected_side,
            "level {level} band byte length"
        );

        let full = reader
            .read_band(level, 0)
            .unwrap_or_else(|e| panic!("level {level}: read_band failed: {e}"));
        assert_eq!(full.len(), len, "level {level} read_band length");

        let mut into = vec![0xEEu8; len];
        reader
            .read_band_into(level, 0, &mut into)
            .expect("read_band_into");
        assert_eq!(into, full, "level {level}: read_band_into");

        // A window inside the level must match the crop of that level's band.
        let side = expected_side as u64;
        let (wx, wy, ww, wh) = (side / 4, side / 4, side / 2, side / 3);
        let window = reader
            .read_window(level, 0, wx, wy, ww, wh)
            .expect("read_window");
        assert_eq!(
            window,
            crop(
                &full,
                expected_side,
                1,
                wx as usize,
                wy as usize,
                ww as usize,
                wh as usize
            ),
            "level {level} window"
        );
    }

    let err = reader.read_band(3, 0).expect_err("level 3 does not exist");
    assert!(
        format!("{err}").contains("Overview level 3 out of bounds"),
        "unexpected error: {err}"
    );

    let _ = std::fs::remove_file(path);
}

// ---------------------------------------------------------------------------
// T4 — parallel decode
// ---------------------------------------------------------------------------

/// Builds a raster big enough that `read_band_into` takes the parallel path.
#[cfg(feature = "parallel")]
fn large_spec() -> Spec {
    Spec {
        label: "parallel_large",
        width: 1024,
        height: 1024,
        samples_per_pixel: 1,
        bits_per_sample: 32,
        sample_format: 3,
        planar: 1,
        tile: Some((128, 128)),
        rows_per_strip: 0,
    }
}

/// The parallel path must be bit-identical to the serial one.
///
/// The full-band read is 4 MiB, comfortably over the parallel threshold, while
/// each single-block-row window is 512 KiB and therefore stays serial — so this
/// really does compare the two implementations against each other, not just
/// against the source pattern.
#[test]
#[cfg(feature = "parallel")]
fn test_issue_14_parallel_matches_serial() {
    let spec = large_spec();
    let interleaved = spec.pattern();
    let reader = GeoTiffReader::open(MemorySource(build_tiff(&spec, &interleaved))).expect("open");

    let len = reader.band_byte_len(0).expect("len");
    let mut parallel = vec![0u8; len];
    reader
        .read_band_into(0, 0, &mut parallel)
        .expect("read_band_into");

    // Reassemble the same band from block-row windows, each below the parallel
    // threshold and therefore decoded serially.
    let mut serial = Vec::with_capacity(len);
    for by in 0..spec.blocks_down() {
        let y = u64::from(by * 128);
        let rows = u64::from(spec.height).saturating_sub(y).min(128);
        serial.extend_from_slice(
            &reader
                .read_window(0, 0, 0, y, u64::from(spec.width), rows)
                .expect("read_window"),
        );
    }
    assert_eq!(serial.len(), len, "serial reassembly length");
    assert_eq!(parallel, serial, "parallel output must equal serial output");
    assert_eq!(
        parallel,
        spec.expected_band(&interleaved, 0),
        "parallel output must equal the source pattern"
    );
}

/// Evidence, not an assertion: prints whole-band read throughput. Run with
/// `--features parallel` to see the multi-threaded number, and set
/// `OXIGEO_ISSUE14_BAND_MIB` to size the raster (default 16 MiB).
#[test]
fn test_issue_14_band_read_speed_evidence() {
    use std::time::Instant;

    let target_mib: usize = env::var("OXIGEO_ISSUE14_BAND_MIB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16);
    // Float32, square, rounded to a multiple of the 256-pixel tile size.
    let side = (((target_mib * 1024 * 1024 / 4) as f64).sqrt() as usize).next_multiple_of(256);
    let spec = Spec {
        label: "speed",
        width: side as u32,
        height: side as u32,
        samples_per_pixel: 1,
        bits_per_sample: 32,
        sample_format: 3,
        planar: 1,
        tile: Some((256, 256)),
        rows_per_strip: 0,
    };
    let mib = (side * side * 4) as f64 / (1024.0 * 1024.0);
    let reader =
        GeoTiffReader::open(MemorySource(build_tiff(&spec, &spec.pattern()))).expect("open");
    let len = reader.band_byte_len(0).expect("len");

    let mut raw = vec![0u8; len];
    let mut best_raw = f64::MAX;
    for _ in 0..3 {
        let start = Instant::now();
        reader
            .read_band_into(0, 0, &mut raw)
            .expect("read_band_into");
        best_raw = best_raw.min(start.elapsed().as_secs_f64());
    }

    let mut typed = vec![0.0f64; reader.band_pixel_count(0).expect("pixels")];
    let mut best_typed = f64::MAX;
    for _ in 0..3 {
        let start = Instant::now();
        reader
            .read_band_into_typed(0, 0, &mut typed)
            .expect("read_band_into_typed");
        best_typed = best_typed.min(start.elapsed().as_secs_f64());
    }

    let mut best_owned = f64::MAX;
    for _ in 0..3 {
        let start = Instant::now();
        let owned = reader.read_band(0, 0).expect("read_band");
        best_owned = best_owned.min(start.elapsed().as_secs_f64());
        assert_eq!(owned.len(), len);
    }

    eprintln!(
        "issue#14 band read {mib:.0} MiB f32 ({side}x{side}, 256px tiles, uncompressed, \
         parallel={}): read_band_into {:.2} ms ({:.0} MiB/s)  read_band_into_typed::<f64> \
         {:.2} ms ({:.0} MiB/s)  read_band (owned Vec) {:.2} ms ({:.0} MiB/s)",
        cfg!(feature = "parallel"),
        best_raw * 1e3,
        mib / best_raw,
        best_typed * 1e3,
        mib / best_typed,
        best_owned * 1e3,
        mib / best_owned,
    );
}

/// Evidence, not an assertion: the same measurement on a DEFLATE + floating-point
/// predictor COG, i.e. what a GDAL-produced float DEM actually looks like — the
/// case issue #14 reports.
#[test]
#[cfg(feature = "deflate")]
fn test_issue_14_compressed_band_read_speed_evidence() {
    use std::time::Instant;

    let target_mib: usize = env::var("OXIGEO_ISSUE14_BAND_MIB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16);
    let side = (((target_mib * 1024 * 1024 / 4) as f64).sqrt() as usize).next_multiple_of(256);
    let mib = (side * side * 4) as f64 / (1024.0 * 1024.0);
    let path = temp_test_file("deflate_speed.tif");

    let mut pattern = Vec::with_capacity(side * side * 4);
    for y in 0..side {
        for x in 0..side {
            // A smooth DEM-like surface: what the float predictor is designed for.
            let value = (y as f32) * 0.5 + (x as f32) * 0.125 - 1000.0;
            pattern.extend_from_slice(&value.to_le_bytes());
        }
    }
    {
        let config = WriterConfig::new(side as u64, side as u64, 1, RasterDataType::Float32)
            .with_compression(oxigeo_geotiff::Compression::Deflate)
            .with_predictor(oxigeo_geotiff::tiff::Predictor::FloatingPoint)
            .with_tile_size(256, 256)
            .with_overviews(false, OverviewResampling::Nearest);
        let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
            .expect("create writer");
        writer.write(&pattern).expect("write raster");
    }

    let reader = GeoTiffReader::open(FileDataSource::open(&path).expect("open")).expect("reader");
    let len = reader.band_byte_len(0).expect("len");
    let mut raw = vec![0u8; len];
    let mut best_raw = f64::MAX;
    for _ in 0..3 {
        let start = Instant::now();
        reader
            .read_band_into(0, 0, &mut raw)
            .expect("read_band_into");
        best_raw = best_raw.min(start.elapsed().as_secs_f64());
    }
    assert_eq!(raw, pattern, "DEFLATE + float predictor round trip");

    let mut typed = vec![0.0f64; reader.band_pixel_count(0).expect("pixels")];
    let mut best_typed = f64::MAX;
    for _ in 0..3 {
        let start = Instant::now();
        reader
            .read_band_into_typed(0, 0, &mut typed)
            .expect("read_band_into_typed");
        best_typed = best_typed.min(start.elapsed().as_secs_f64());
    }

    eprintln!(
        "issue#14 band read {mib:.0} MiB f32 ({side}x{side}, 256px tiles, DEFLATE+floatpred, \
         parallel={}): read_band_into {:.2} ms ({:.0} MiB/s)  read_band_into_typed::<f64> \
         {:.2} ms ({:.0} MiB/s)",
        cfg!(feature = "parallel"),
        best_raw * 1e3,
        mib / best_raw,
        best_typed * 1e3,
        mib / best_typed,
    );

    let _ = std::fs::remove_file(path);
}
