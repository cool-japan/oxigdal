//! cool-japan/oxigeo#14 — the interleaved (multi-band) readers.
//!
//! `Dataset::read_band` used to return the whole pixel-interleaved image
//! whatever band index it was handed.  Fixing that to return one band left
//! callers who *wanted* the interleaved image with no supported path;
//! [`Dataset::read_interleaved`] and friends are it.
//!
//! The assertions here are deliberately of the shape that catches a lazy
//! implementation:
//!
//! * every interleaved read is compared **element for element** against the
//!   result of reading each selected band on its own and weaving the planes by
//!   hand — the very loop the API exists to spare users;
//! * and against the fixture's own known sample values, so a bug that happens to
//!   affect both sides equally still shows up;
//! * over **chunky (`PlanarConfiguration = 1`) and planar (`= 2`)** sources,
//!   tiled and striped, at `u8` / `u16` / `f32` sample types.  Planar is where a
//!   "just stride the interleaved plane" implementation silently returns
//!   garbage, and the crate's own writer cannot emit it, so the fixtures below
//!   are hand-built TIFFs.

#![cfg(feature = "geotiff")]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::sync::atomic::{AtomicU64, Ordering};

use oxigeo::{BoundingBox, Dataset, OxiGeoError, RasterElement};

/// Per-test scratch fixture inside the system temp dir (house policy: no
/// hardcoded absolute paths).
///
/// The leaf name embeds the process id and a monotonic counter, so no two test
/// binaries — nor two concurrent runs of this one — can ever land on the same
/// file.  Dropping the guard removes the fixture, so a panicking test leaks
/// nothing.
struct TempPath(std::path::PathBuf);

impl TempPath {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "oxigeo_issue14_{}_{seq}_{name}",
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

// ---------------------------------------------------------------------------
// Synthetic TIFF fixtures (uncompressed; chunky *and* planar)
// ---------------------------------------------------------------------------

const TAG_IMAGE_WIDTH: u16 = 256;
const TAG_IMAGE_LENGTH: u16 = 257;
const TAG_BITS_PER_SAMPLE: u16 = 258;
const TAG_COMPRESSION: u16 = 259;
const TAG_PHOTOMETRIC: u16 = 262;
const TAG_STRIP_OFFSETS: u16 = 273;
const TAG_SAMPLES_PER_PIXEL: u16 = 277;
const TAG_ROWS_PER_STRIP: u16 = 278;
const TAG_STRIP_BYTE_COUNTS: u16 = 279;
const TAG_PLANAR_CONFIGURATION: u16 = 284;
const TAG_TILE_WIDTH: u16 = 322;
const TAG_TILE_LENGTH: u16 = 323;
const TAG_TILE_OFFSETS: u16 = 324;
const TAG_TILE_BYTE_COUNTS: u16 = 325;
const TAG_SAMPLE_FORMAT: u16 = 339;

const TIFF_SHORT: u16 = 3;
const TIFF_LONG: u16 = 4;

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

    fn bands(&self) -> usize {
        self.samples_per_pixel as usize
    }

    /// The value stored at `(x, y)` of `band`, as an `f64` (every fixture type
    /// below is exactly representable, so comparisons stay exact).
    fn sample(&self, x: u32, y: u32, band: usize) -> f64 {
        let n = u64::from(x) * 7 + u64::from(y) * 131 + band as u64 * 1_000_003;
        match (self.sample_format, self.bits_per_sample) {
            (3, 32) => f64::from(n as f32 * 0.25 - 500.0),
            (1, 16) => (n % 65_536) as f64,
            _ => (n % 251) as f64,
        }
    }

    /// Interleaved (chunky) reference image in the file's own byte layout.
    fn pattern(&self) -> Vec<u8> {
        let mut out =
            Vec::with_capacity(self.pixel_count() * self.bands() * self.bytes_per_sample());
        for y in 0..self.height {
            for x in 0..self.width {
                for band in 0..self.bands() {
                    let value = self.sample(x, y, band);
                    match (self.sample_format, self.bits_per_sample) {
                        (3, 32) => out.extend_from_slice(&(value as f32).to_le_bytes()),
                        (1, 16) => out.extend_from_slice(&(value as u16).to_le_bytes()),
                        _ => out.push(value as u8),
                    }
                }
            }
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

/// Serialises `interleaved` into the on-disk block order `spec` describes and
/// wraps it in a little-endian classic TIFF.
fn build_tiff(spec: &Spec, interleaved: &[u8]) -> Vec<u8> {
    let bps = spec.bytes_per_sample();
    let spp = spec.bands();
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
    let mut entries: Vec<(u16, u16, u32, Vec<u8>)> = Vec::new();

    entries.push((
        TAG_IMAGE_WIDTH,
        TIFF_LONG,
        1,
        spec.width.to_le_bytes().to_vec(),
    ));
    entries.push((
        TAG_IMAGE_LENGTH,
        TIFF_LONG,
        1,
        spec.height.to_le_bytes().to_vec(),
    ));
    let bits: Vec<u8> = (0..spp)
        .flat_map(|_| spec.bits_per_sample.to_le_bytes())
        .collect();
    entries.push((TAG_BITS_PER_SAMPLE, TIFF_SHORT, spp as u32, bits));
    entries.push((TAG_COMPRESSION, TIFF_SHORT, 1, 1u16.to_le_bytes().to_vec()));
    let photometric: u16 = if spp >= 3 { 2 } else { 1 };
    entries.push((
        TAG_PHOTOMETRIC,
        TIFF_SHORT,
        1,
        photometric.to_le_bytes().to_vec(),
    ));
    if spec.tile.is_none() {
        entries.push((
            TAG_STRIP_OFFSETS,
            TIFF_LONG,
            block_count,
            vec![0; block_count as usize * 4],
        ));
    }
    entries.push((
        TAG_SAMPLES_PER_PIXEL,
        TIFF_SHORT,
        1,
        spec.samples_per_pixel.to_le_bytes().to_vec(),
    ));
    if spec.tile.is_none() {
        entries.push((
            TAG_ROWS_PER_STRIP,
            TIFF_LONG,
            1,
            spec.rows_per_strip.to_le_bytes().to_vec(),
        ));
        let counts: Vec<u8> = blocks
            .iter()
            .flat_map(|b| (b.len() as u32).to_le_bytes())
            .collect();
        entries.push((TAG_STRIP_BYTE_COUNTS, TIFF_LONG, block_count, counts));
    }
    entries.push((
        TAG_PLANAR_CONFIGURATION,
        TIFF_SHORT,
        1,
        spec.planar.to_le_bytes().to_vec(),
    ));
    if let Some((tw, th)) = spec.tile {
        entries.push((TAG_TILE_WIDTH, TIFF_LONG, 1, tw.to_le_bytes().to_vec()));
        entries.push((TAG_TILE_LENGTH, TIFF_LONG, 1, th.to_le_bytes().to_vec()));
        entries.push((
            TAG_TILE_OFFSETS,
            TIFF_LONG,
            block_count,
            vec![0; block_count as usize * 4],
        ));
        let counts: Vec<u8> = blocks
            .iter()
            .flat_map(|b| (b.len() as u32).to_le_bytes())
            .collect();
        entries.push((TAG_TILE_BYTE_COUNTS, TIFF_LONG, block_count, counts));
    }
    entries.push((
        TAG_SAMPLE_FORMAT,
        TIFF_SHORT,
        1,
        spec.sample_format.to_le_bytes().to_vec(),
    ));
    entries.sort_by_key(|(tag, _, _, _)| *tag);

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
        TAG_TILE_OFFSETS
    } else {
        TAG_STRIP_OFFSETS
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
        out.extend_from_slice(&tag.to_le_bytes());
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

/// Materialise `spec` as a file under the system temp dir and open it.
///
/// `scope` names the calling test; the returned [`TempPath`] guard keeps the
/// fixture alive for as long as the caller holds it and deletes it afterwards,
/// panic or not.  Callers must bind it (`let (ds, _fixture) = …`) rather than
/// drop it on the spot.
fn open_fixture(spec: &Spec, scope: &str, name: &str) -> (Dataset, TempPath) {
    let path = TempPath::new(&format!("interleaved_{scope}_{name}.tif"));
    let bytes = build_tiff(spec, &spec.pattern());
    std::fs::write(&path, &bytes).expect("write fixture");
    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("open fixture");
    (ds, path)
}

/// The layouts every assertion below runs over: chunky and planar, tiled and
/// striped, at three sample types.
fn layout_matrix() -> Vec<Spec> {
    let mut specs = Vec::new();
    for &planar in &[1u16, 2] {
        for &(kind, tile, rows) in &[
            ("tiled", Some((16u32, 16u32)), 0u32),
            ("striped", None, 7u32),
        ] {
            for &(type_label, bits, format) in &[("u8", 8u16, 1u16), ("u16", 16, 1), ("f32", 32, 3)]
            {
                // Deliberately not a multiple of the tile size or the strip
                // height, so edge blocks are partial in both directions.
                specs.push(Spec {
                    label: Box::leak(
                        format!("planar{planar}_{kind}_{type_label}").into_boxed_str(),
                    ),
                    width: 37,
                    height: 23,
                    samples_per_pixel: 3,
                    bits_per_sample: bits,
                    sample_format: format,
                    planar,
                    tile,
                    rows_per_strip: rows,
                });
            }
        }
    }
    specs
}

// ---------------------------------------------------------------------------
// The reference implementation: read each band, weave the planes by hand
// ---------------------------------------------------------------------------

/// Read each selected band on its own and interleave the planes manually.
///
/// This is exactly the loop `read_interleaved_into` exists to replace, written
/// the way a user would have to write it — so agreement between the two is the
/// property that matters.
fn manual_interleave<T: RasterElement>(ds: &Dataset, bands: &[u32]) -> Vec<T> {
    let pixels = ds.width() as usize * ds.height() as usize;
    let mut out = vec![T::default(); pixels * bands.len()];
    let mut plane = vec![T::default(); pixels];
    for (slot, &band) in bands.iter().enumerate() {
        ds.read_band_into(band, &mut plane).expect("read_band_into");
        for (pixel, sample) in plane.iter().enumerate() {
            out[pixel * bands.len() + slot] = *sample;
        }
    }
    out
}

/// The windowed twin of [`manual_interleave`].
fn manual_window_interleave<T: RasterElement>(
    ds: &Dataset,
    bands: &[u32],
    col: u32,
    row: u32,
    width: u32,
    height: u32,
) -> Vec<T> {
    let pixels = width as usize * height as usize;
    let mut out = vec![T::default(); pixels * bands.len()];
    let mut plane = vec![T::default(); pixels];
    for (slot, &band) in bands.iter().enumerate() {
        ds.read_window_into(band, col, row, width, height, &mut plane)
            .expect("read_window_into");
        for (pixel, sample) in plane.iter().enumerate() {
            out[pixel * bands.len() + slot] = *sample;
        }
    }
    out
}

/// Every band selection worth exercising on a 3-band fixture.
fn selections() -> Vec<Option<Vec<u32>>> {
    vec![
        None,                // all bands, in file order
        Some(vec![0]),       // degenerate: one band, must equal read_band_into
        Some(vec![2, 1, 0]), // reordered (RGB read as BGR)
        Some(vec![1, 2]),    // subset, not starting at band 0
        Some(vec![0, 0, 2]), // a repeated band, GDAL's grey-to-RGB trick
    ]
}

// ---------------------------------------------------------------------------
// T4 — the interleaved readers equal a hand-woven per-band read
// ---------------------------------------------------------------------------

#[test]
fn test_interleaved_equals_manual_band_interleave() {
    for spec in layout_matrix() {
        let (ds, _fixture) = open_fixture(&spec, "equal", spec.label);
        assert_eq!(ds.band_count(), 3, "{}: band count", spec.label);
        assert_eq!(ds.width(), spec.width, "{}: width", spec.label);

        for selection in selections() {
            let bands: Vec<u32> = selection
                .clone()
                .unwrap_or_else(|| (0..ds.band_count()).collect());
            let argument = selection.as_deref();
            let slots = bands.len();
            let pixels = spec.pixel_count();

            // f64 destination: exercises the fused element conversion on every
            // fixture type, and is exact for all three.
            let expected = manual_interleave::<f64>(&ds, &bands);

            let owned: Vec<f64> = ds
                .read_interleaved(argument)
                .unwrap_or_else(|e| panic!("{} {bands:?}: read_interleaved: {e}", spec.label));
            assert_eq!(owned.len(), pixels * slots, "{} {bands:?}", spec.label);
            assert_eq!(owned, expected, "{} {bands:?}: owned", spec.label);

            let mut into = vec![0.0f64; pixels * slots];
            ds.read_interleaved_into(argument, &mut into)
                .unwrap_or_else(|e| panic!("{} {bands:?}: read_interleaved_into: {e}", spec.label));
            assert_eq!(into, expected, "{} {bands:?}: into", spec.label);

            // …and against the fixture's own values, so a bug shared by both
            // sides cannot hide.
            for y in 0..spec.height {
                for x in 0..spec.width {
                    let base = (y as usize * spec.width as usize + x as usize) * slots;
                    for (slot, &band) in bands.iter().enumerate() {
                        assert_eq!(
                            into[base + slot],
                            spec.sample(x, y, band as usize),
                            "{} {bands:?}: pixel ({x},{y}) slot {slot}",
                            spec.label
                        );
                    }
                }
            }
        }
    }
}

/// The same equivalence for the windowed reader, over windows that are tile
/// aligned, straddle block boundaries, and hit the ragged right/bottom edge.
#[test]
fn test_window_interleaved_equals_manual_window_interleave() {
    let windows: &[(u32, u32, u32, u32)] = &[
        (0, 0, 16, 16),
        (15, 6, 5, 9),
        (1, 1, 35, 21),
        (36, 22, 1, 1),
        (0, 0, 37, 23),
    ];

    for spec in layout_matrix() {
        let (ds, _fixture) = open_fixture(&spec, "window", spec.label);
        for selection in selections() {
            let bands: Vec<u32> = selection
                .clone()
                .unwrap_or_else(|| (0..ds.band_count()).collect());
            let argument = selection.as_deref();

            for &(col, row, width, height) in windows {
                let expected =
                    manual_window_interleave::<f64>(&ds, &bands, col, row, width, height);

                let owned: Vec<f64> = ds
                    .read_window_interleaved(argument, col, row, width, height)
                    .expect("read_window_interleaved");
                assert_eq!(
                    owned, expected,
                    "{} {bands:?} window {col},{row} {width}×{height}",
                    spec.label
                );

                let mut into = vec![0.0f64; (width * height) as usize * bands.len()];
                ds.read_window_interleaved_into(argument, col, row, width, height, &mut into)
                    .expect("read_window_interleaved_into");
                assert_eq!(
                    into, expected,
                    "{} {bands:?} window {col},{row} {width}×{height} (into)",
                    spec.label
                );
            }
        }
    }
}

/// A single-band selection must be bit-identical to [`Dataset::read_band_into`]
/// — the new API must not have grown its own decode path.
#[test]
fn test_single_band_selection_equals_read_band_into() {
    for spec in layout_matrix() {
        let (ds, _fixture) = open_fixture(&spec, "single", spec.label);
        for band in 0..ds.band_count() {
            let mut direct = vec![0.0f64; spec.pixel_count()];
            ds.read_band_into(band, &mut direct)
                .expect("read_band_into");

            let mut interleaved = vec![0.0f64; spec.pixel_count()];
            ds.read_interleaved_into(Some(&[band]), &mut interleaved)
                .expect("read_interleaved_into");
            assert_eq!(direct, interleaved, "{} band {band}", spec.label);
        }
    }
}

// ---------------------------------------------------------------------------
// T4 — the strip loop
// ---------------------------------------------------------------------------

/// A raster big enough that the destination spans **many** interleave strips.
///
/// The strip height is chosen from a byte budget, so a small fixture is always a
/// single strip and would never exercise the loop, the per-strip source-window
/// arithmetic, or the scratch reuse.  This one is deliberately over the budget:
/// with `f64` output and 4 bands its destination is 8 MiB, several times the
/// 2 MiB strip target.
#[test]
fn test_multi_strip_read_equals_manual_interleave() {
    for (planar, tile, rows) in [(1u16, Some((256u32, 256u32)), 0u32), (2, None, 32)] {
        let spec = Spec {
            label: "multistrip",
            width: 512,
            height: 512,
            samples_per_pixel: 4,
            bits_per_sample: 16,
            sample_format: 1,
            planar,
            tile,
            rows_per_strip: rows,
        };
        let (ds, _fixture) = open_fixture(&spec, "multistrip", &format!("planar{planar}"));
        let bands: Vec<u32> = (0..4).collect();

        let expected = manual_interleave::<f64>(&ds, &bands);
        let mut into = vec![0.0f64; spec.pixel_count() * 4];
        ds.read_interleaved_into(None, &mut into)
            .expect("read_interleaved_into");
        assert_eq!(into.len(), 512 * 512 * 4);
        assert_eq!(into, expected, "planar={planar} multi-strip interleave");

        // Spot-check the last pixel explicitly: an off-by-one strip cursor
        // leaves the tail of `dst` untouched, which equality against a
        // similarly-broken reference could not catch — but this can.
        let last = (spec.pixel_count() - 1) * 4;
        for band in 0..4 {
            assert_eq!(
                into[last + band],
                spec.sample(spec.width - 1, spec.height - 1, band),
                "planar={planar} last pixel band {band}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// T4 — clip semantics and error handling
// ---------------------------------------------------------------------------

/// The interleaved readers work in the dataset's *current* (clipped) grid, like
/// every other reader in the crate.
///
/// The hand-built fixtures above carry no georeferencing and `clip` needs a
/// geo-transform, so this one goes through the crate's own writer — which emits
/// chunky only, hence the planar coverage living in the tests above.
#[test]
fn test_interleaved_honours_clip_window() {
    use oxigeo::geotiff::tiff::{Compression, PhotometricInterpretation, Predictor};
    use oxigeo::geotiff::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
    use oxigeo::{GeoTransform, RasterDataType};

    const W: usize = 37;
    const H: usize = 23;
    /// Band `b` of pixel `p` holds `p * 8 + b` — distinct across both axes.
    fn value(pixel: usize, band: usize) -> u16 {
        (pixel as u16).wrapping_mul(8).wrapping_add(band as u16)
    }

    let path = TempPath::new("interleaved_clip.tif");
    let mut bytes = Vec::with_capacity(W * H * 3 * 2);
    for pixel in 0..W * H {
        for band in 0..3 {
            bytes.extend_from_slice(&value(pixel, band).to_ne_bytes());
        }
    }
    let mut config = WriterConfig::new(W as u64, H as u64, 3, RasterDataType::UInt16)
        .with_compression(Compression::None)
        .with_predictor(Predictor::None)
        .with_photometric(PhotometricInterpretation::Rgb)
        .with_geo_transform(GeoTransform::north_up(0.0, H as f64, 1.0, 1.0));
    config.generate_overviews = false;
    config.tile_width = Some(16);
    config.tile_height = Some(16);
    let mut writer = GeoTiffWriter::create(&path, config, GeoTiffWriterOptions::default())
        .expect("create writer");
    writer.write(&bytes).expect("write fixture");
    drop(writer);

    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("open");
    let gt = ds
        .geotransform()
        .copied()
        .expect("fixture is georeferenced");
    let (x0, y0) = gt.pixel_to_world(4.0, 3.0);
    let (x1, y1) = gt.pixel_to_world(14.0, 11.0);
    let bbox = BoundingBox::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)).expect("bbox");
    let clipped = ds.clip(bbox).expect("clip");
    assert_eq!(clipped.width(), 10, "clip really narrowed the extent");
    assert_eq!(clipped.height(), 8);

    let (w, h) = (clipped.width() as usize, clipped.height() as usize);
    let expected = manual_interleave::<f64>(&clipped, &[0, 1, 2]);
    let mut into = vec![0.0f64; w * h * 3];
    clipped
        .read_interleaved_into(None, &mut into)
        .expect("clipped read_interleaved_into");
    assert_eq!(into, expected);

    // And it really is the clipped rectangle of the source, not the origin.
    for row in 0..h {
        for col in 0..w {
            let source_pixel = (row + 3) * W + (col + 4);
            for band in 0..3 {
                assert_eq!(
                    into[(row * w + col) * 3 + band],
                    f64::from(value(source_pixel, band)),
                    "clipped pixel ({col},{row}) band {band}"
                );
            }
        }
    }

    // Windows of a clipped dataset stay relative to the clipped grid.
    let mut window = vec![0.0f64; 2 * 2 * 3];
    clipped
        .read_window_interleaved_into(None, 1, 1, 2, 2, &mut window)
        .expect("clipped window");
    assert_eq!(
        window,
        manual_window_interleave::<f64>(&clipped, &[0, 1, 2], 1, 1, 2, 2)
    );

    // The unclipped length is now the wrong length.
    let mut full = vec![0.0f64; W * H * 3];
    assert!(
        clipped.read_interleaved_into(None, &mut full).is_err(),
        "a clipped dataset must reject a full-extent destination"
    );
}

/// Bad arguments are rejected with a typed error that names what is wrong —
/// never a truncated read and never a panic.
#[test]
fn test_interleaved_rejects_bad_arguments() {
    let spec = Spec {
        label: "errors",
        width: 37,
        height: 23,
        samples_per_pixel: 3,
        bits_per_sample: 8,
        sample_format: 1,
        planar: 1,
        tile: None,
        rows_per_strip: 7,
    };
    let (ds, _fixture) = open_fixture(&spec, "errors", "errors");
    let pixels = spec.pixel_count();

    // Empty selection.
    let mut dst = vec![0u8; pixels * 3];
    assert!(matches!(
        ds.read_interleaved_into(Some(&[]), &mut dst),
        Err(OxiGeoError::InvalidParameter { .. })
    ));
    assert!(ds.read_interleaved::<u8>(Some(&[])).is_err());

    // Out-of-range band index.
    assert!(matches!(
        ds.read_interleaved_into(Some(&[0, 3]), &mut dst),
        Err(OxiGeoError::InvalidParameter { .. })
    ));

    // Wrong destination length, both short and long, with the expected length
    // named in the message.
    let mut short = vec![0u8; pixels * 3 - 1];
    let err = ds
        .read_interleaved_into(None, &mut short)
        .expect_err("short destination must error");
    assert!(
        err.to_string().contains(&(pixels * 3).to_string()),
        "error must name the expected length: {err}"
    );
    assert!(
        short.iter().all(|v| *v == 0),
        "a rejected read must not have written anything"
    );

    let mut long = vec![0u8; pixels * 3 + 1];
    assert!(ds.read_interleaved_into(None, &mut long).is_err());

    // A per-band-sized destination is the classic mistake: it must be rejected,
    // not silently filled with the first band's worth of samples.
    let mut one_band = vec![0u8; pixels];
    assert!(ds.read_interleaved_into(None, &mut one_band).is_err());

    // Degenerate and out-of-range windows.
    let mut window = vec![0u8; 4 * 3];
    assert!(
        ds.read_window_interleaved_into(None, 0, 0, 0, 2, &mut window)
            .is_err()
    );
    assert!(
        ds.read_window_interleaved_into(None, 36, 0, 4, 1, &mut window)
            .is_err()
    );
    assert!(ds.read_window_interleaved::<u8>(None, 0, 0, 0, 2).is_err());
}

/// A vector dataset has no pixels: the interleaved readers must say so rather
/// than pretend.
#[test]
#[cfg(feature = "geojson")]
fn test_interleaved_on_vector_dataset_is_unsupported() {
    let path = TempPath::new("interleaved_vector.geojson");
    std::fs::write(
        &path,
        br#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"Point","coordinates":[0,0]},"properties":{}}
        ]}"#,
    )
    .expect("write geojson");
    let ds = Dataset::open(path.to_str().expect("utf-8 path")).expect("open");

    assert!(matches!(
        ds.read_interleaved::<u8>(None),
        Err(OxiGeoError::NotSupported { .. })
    ));
    let mut dst = vec![0u8; 4];
    assert!(matches!(
        ds.read_interleaved_into(None, &mut dst),
        Err(OxiGeoError::NotSupported { .. })
    ));
}
