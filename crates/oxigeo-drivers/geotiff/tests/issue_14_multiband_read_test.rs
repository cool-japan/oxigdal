//! cool-japan/oxigeo#14 — the multi-band read path decodes every block once.
//!
//! `GeoTiffReader::read_bands_into_typed` and
//! `GeoTiffReader::read_window_bands_into_typed` exist because a **chunky**
//! (`PlanarConfiguration = 1`) block physically holds every band of every pixel
//! it covers. Asking the single-band engine for `n` bands therefore decompresses
//! each block `n` times and throws `n − 1` bands away each pass — an `n`× waste
//! on the layout almost every RGB, RGBA and multispectral GeoTIFF uses.
//!
//! Two things are proved here.
//!
//! # 1. Each block is decoded once
//!
//! Counted structurally, not timed. Every call to `band_read::decode_block`
//! fetches its block through exactly one [`DataSource`] entry point — a
//! `read_tile_into` is one `read_range_into`, `read_range` or `range_slice` —
//! and the block offsets themselves are pre-parsed at open time, so during a
//! read the source is touched once per block decode and never otherwise. The
//! source below refuses to lend its bytes (no `range_slice`), so every fetch
//! lands on a counter, and the counter is sampled around the read call alone.
//! *Block fetches during a read = block decodes.*
//!
//! The comparison is against the same read expressed as `n` single-band calls,
//! which is exactly what the facade's `Dataset::read_interleaved` did before
//! this API existed.
//!
//! # 2. It agrees with the single-band engine, everywhere
//!
//! The single-band path is used as the oracle: read each selected band with
//! `read_band_into_typed`/`read_window_into_typed` and weave the planes by hand,
//! then demand the multi-band call produce that buffer element for element, over
//! {chunky, planar} × {tiled, striped} × {`UInt16`, `Float32`} × both byte orders
//! × band selections that reorder, repeat and subset × several destination
//! element types. One configuration is additionally checked against ground truth
//! computed without the driver, so an oracle that was itself wrong could not hide
//! behind the comparison.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use oxigeo_core::buffer::RasterElement;
use oxigeo_core::error::{OxiGeoError, Result};
use oxigeo_core::io::{ByteRange, DataSource};
use oxigeo_geotiff::GeoTiffReader;
use oxigeo_geotiff::compression;
use oxigeo_geotiff::tiff::{ByteOrderType, Compression, Predictor, TiffTag};

// ---------------------------------------------------------------------------
// A data source that counts block fetches
// ---------------------------------------------------------------------------

/// In-memory source that tallies every read and never lends its bytes, so that
/// each block decode costs exactly one counted call.
#[derive(Debug)]
struct CountingSource {
    data: Vec<u8>,
    reads: Arc<AtomicUsize>,
}

impl CountingSource {
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
// Fixtures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Elem {
    U8,
    U16,
    F32,
}

impl Elem {
    const fn bytes(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::U16 => 2,
            Self::F32 => 4,
        }
    }

    const fn bits(self) -> u16 {
        (self.bytes() * 8) as u16
    }

    /// TIFF `SampleFormat`: 1 = unsigned integer, 3 = IEEE float.
    const fn sample_format(self) -> u16 {
        match self {
            Self::U8 | Self::U16 => 1,
            Self::F32 => 3,
        }
    }
}

/// Bit pattern of the sample at `(band, y, x)`. Neighbouring samples differ in
/// every byte, so a mis-strided read cannot coincidentally agree.
fn raw_bits(elem: Elem, band: usize, y: usize, x: usize) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for v in [band as u64 + 1, y as u64 + 1, x as u64 + 1] {
        h ^= v;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    match elem {
        Elem::U8 => h & 0xff,
        Elem::U16 => h & 0xffff,
        Elem::F32 => {
            let v = ((h % 200_003) as f32 - 100_001.0) / 3.0;
            u64::from(v.to_bits())
        }
    }
}

fn file_bytes(elem: Elem, bits: u64, byte_order: ByteOrderType) -> Vec<u8> {
    let mut out = bits.to_le_bytes()[..elem.bytes()].to_vec();
    if byte_order == ByteOrderType::BigEndian {
        out.reverse();
    }
    out
}

#[derive(Debug, Clone, Copy)]
struct Spec {
    width: u32,
    height: u32,
    elem: Elem,
    bands: u16,
    /// TIFF `PlanarConfiguration`: 1 = chunky, 2 = planar.
    planar: u16,
    /// `Some((tile_w, tile_h))` for a tiled raster, `None` for a striped one.
    tile: Option<(u32, u32)>,
    rows_per_strip: u32,
    predictor: Predictor,
    compression: Compression,
}

impl Spec {
    const fn tiled(elem: Elem, planar: u16) -> Self {
        Self {
            width: 20,
            height: 12,
            elem,
            bands: 3,
            planar,
            tile: Some((8, 8)),
            rows_per_strip: 5,
            predictor: Predictor::None,
            compression: Compression::None,
        }
    }

    const fn striped(mut self) -> Self {
        self.tile = None;
        self
    }

    const fn with_predictor(mut self, predictor: Predictor) -> Self {
        self.predictor = predictor;
        self
    }

    const fn with_compression(mut self, compression: Compression) -> Self {
        self.compression = compression;
        self
    }

    /// `(block_w, block_h, across, down)`, `down` counting block rows of one
    /// plane.
    const fn block_grid(&self) -> (u32, u32, u32, u32) {
        match self.tile {
            Some((tw, th)) => (tw, th, self.width.div_ceil(tw), self.height.div_ceil(th)),
            None => (
                self.width,
                self.rows_per_strip,
                1,
                self.height.div_ceil(self.rows_per_strip),
            ),
        }
    }

    /// Blocks one plane occupies — the number of decodes a whole-raster read of
    /// a single band must perform.
    const fn blocks_per_plane(&self) -> usize {
        let (_, _, across, down) = self.block_grid();
        (across * down) as usize
    }

    const fn samples_in_block(&self) -> usize {
        if self.planar == 2 {
            1
        } else {
            self.bands as usize
        }
    }

    const fn planes(&self) -> usize {
        if self.planar == 2 {
            self.bands as usize
        } else {
            1
        }
    }

    const fn block_rows(&self, ty: u32) -> u32 {
        let (_, bh, _, _) = self.block_grid();
        if self.tile.is_some() {
            bh
        } else {
            let remaining = self.height.saturating_sub(ty * bh);
            if remaining < bh { remaining } else { bh }
        }
    }

    fn label(&self) -> String {
        format!(
            "{:?} {} {} pred={:?} comp={:?}",
            self.elem,
            if self.planar == 2 { "planar" } else { "chunky" },
            if self.tile.is_some() {
                "tiled"
            } else {
                "striped"
            },
            self.predictor,
            self.compression,
        )
    }
}

/// The decoded bytes of block `(plane, tx, ty)` in `bo`, padding included.
fn block_samples(spec: &Spec, plane: usize, tx: u32, ty: u32, bo: ByteOrderType) -> Vec<u8> {
    let (bw, bh, _, _) = spec.block_grid();
    let rows = spec.block_rows(ty);
    let per_pixel = spec.samples_in_block();

    let mut out = Vec::new();
    for row in 0..rows {
        let y = ty * bh + row;
        for col in 0..bw {
            let x = tx * bw + col;
            for s in 0..per_pixel {
                let band = if spec.planar == 2 { plane } else { s };
                if x < spec.width && y < spec.height {
                    out.extend_from_slice(&file_bytes(
                        spec.elem,
                        raw_bits(spec.elem, band, y as usize, x as usize),
                        bo,
                    ));
                } else {
                    out.extend(std::iter::repeat_n(0u8, spec.elem.bytes()));
                }
            }
        }
    }
    out
}

#[derive(Debug, Clone)]
enum FieldVal {
    Short(Vec<u16>),
    Long(Vec<u32>),
}

impl FieldVal {
    const fn field_type(&self) -> u16 {
        match self {
            Self::Short(_) => 3,
            Self::Long(_) => 4,
        }
    }

    fn count(&self) -> u32 {
        match self {
            Self::Short(v) => v.len() as u32,
            Self::Long(v) => v.len() as u32,
        }
    }

    fn byte_len(&self) -> usize {
        match self {
            Self::Short(v) => v.len() * 2,
            Self::Long(v) => v.len() * 4,
        }
    }

    fn encode(&self, bo: ByteOrderType) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.byte_len());
        match self {
            Self::Short(v) => {
                for &x in v {
                    match bo {
                        ByteOrderType::LittleEndian => out.extend_from_slice(&x.to_le_bytes()),
                        ByteOrderType::BigEndian => out.extend_from_slice(&x.to_be_bytes()),
                    }
                }
            }
            Self::Long(v) => {
                for &x in v {
                    match bo {
                        ByteOrderType::LittleEndian => out.extend_from_slice(&x.to_le_bytes()),
                        ByteOrderType::BigEndian => out.extend_from_slice(&x.to_be_bytes()),
                    }
                }
            }
        }
        out
    }
}

#[derive(Debug, Clone)]
struct Entry {
    tag: u16,
    val: FieldVal,
    external_offset: Option<u32>,
}

/// Every block, predicted, compressed and in on-disk (plane-major) order.
fn encode_blocks(spec: &Spec, bo: ByteOrderType) -> Vec<Vec<u8>> {
    let (bw, _, across, down) = spec.block_grid();
    let predictor_spp = spec.samples_in_block();

    let mut blocks = Vec::with_capacity(spec.planes() * across as usize * down as usize);
    for plane in 0..spec.planes() {
        for ty in 0..down {
            for tx in 0..across {
                let mut raw = block_samples(spec, plane, tx, ty, bo);
                compression::apply_predictor_forward(
                    &mut raw,
                    spec.predictor,
                    spec.elem.bytes(),
                    predictor_spp,
                    bw as usize,
                    bo,
                )
                .expect("predictor encode");
                blocks.push(compression::compress(&raw, spec.compression).expect("compress"));
            }
        }
    }
    blocks
}

fn ifd_entries(spec: &Spec, block_count: usize) -> Vec<Entry> {
    let (bw, bh, _, _) = spec.block_grid();
    let bands = spec.bands as usize;

    let mut entries: Vec<(u16, FieldVal)> = vec![
        (TiffTag::ImageWidth as u16, FieldVal::Long(vec![spec.width])),
        (
            TiffTag::ImageLength as u16,
            FieldVal::Long(vec![spec.height]),
        ),
        (
            TiffTag::BitsPerSample as u16,
            FieldVal::Short(vec![spec.elem.bits(); bands]),
        ),
        (
            TiffTag::Compression as u16,
            FieldVal::Short(vec![spec.compression as u16]),
        ),
        (
            TiffTag::PhotometricInterpretation as u16,
            FieldVal::Short(vec![1]),
        ),
        (
            TiffTag::SamplesPerPixel as u16,
            FieldVal::Short(vec![spec.bands]),
        ),
        (
            TiffTag::PlanarConfiguration as u16,
            FieldVal::Short(vec![spec.planar]),
        ),
        (
            TiffTag::Predictor as u16,
            FieldVal::Short(vec![spec.predictor as u16]),
        ),
        (
            TiffTag::SampleFormat as u16,
            FieldVal::Short(vec![spec.elem.sample_format(); bands]),
        ),
    ];

    let placeholder = FieldVal::Long(vec![0u32; block_count]);
    if spec.tile.is_some() {
        entries.push((TiffTag::TileWidth as u16, FieldVal::Long(vec![bw])));
        entries.push((TiffTag::TileLength as u16, FieldVal::Long(vec![bh])));
        entries.push((TiffTag::TileOffsets as u16, placeholder.clone()));
        entries.push((TiffTag::TileByteCounts as u16, placeholder));
    } else {
        entries.push((TiffTag::RowsPerStrip as u16, FieldVal::Long(vec![bh])));
        entries.push((TiffTag::StripOffsets as u16, placeholder.clone()));
        entries.push((TiffTag::StripByteCounts as u16, placeholder));
    }

    entries.sort_by_key(|(tag, _)| *tag);
    entries
        .into_iter()
        .map(|(tag, val)| Entry {
            tag,
            val,
            external_offset: None,
        })
        .collect()
}

/// Serialises a complete single-IFD classic TIFF for `spec`.
fn build_tiff(spec: &Spec, bo: ByteOrderType) -> Vec<u8> {
    let blocks = encode_blocks(spec, bo);
    let mut ifd = ifd_entries(spec, blocks.len());

    let mut cursor: u32 = 8;
    let ifd_offset = cursor;
    cursor += 2 + 12 * ifd.len() as u32 + 4;
    for entry in ifd.iter_mut() {
        if entry.val.byte_len() > 4 {
            entry.external_offset = Some(cursor);
            cursor += entry.val.byte_len() as u32;
            cursor += cursor % 2;
        }
    }
    let mut block_offsets = Vec::with_capacity(blocks.len());
    for block in &blocks {
        block_offsets.push(cursor);
        cursor += block.len() as u32;
    }

    for entry in ifd.iter_mut() {
        if entry.tag == TiffTag::TileOffsets as u16 || entry.tag == TiffTag::StripOffsets as u16 {
            entry.val = FieldVal::Long(block_offsets.clone());
        } else if entry.tag == TiffTag::TileByteCounts as u16
            || entry.tag == TiffTag::StripByteCounts as u16
        {
            entry.val = FieldVal::Long(blocks.iter().map(|b| b.len() as u32).collect());
        }
    }

    let mut out: Vec<u8> = Vec::with_capacity(cursor as usize);
    let put16 = |out: &mut Vec<u8>, v: u16| match bo {
        ByteOrderType::LittleEndian => out.extend_from_slice(&v.to_le_bytes()),
        ByteOrderType::BigEndian => out.extend_from_slice(&v.to_be_bytes()),
    };
    let put32 = |out: &mut Vec<u8>, v: u32| match bo {
        ByteOrderType::LittleEndian => out.extend_from_slice(&v.to_le_bytes()),
        ByteOrderType::BigEndian => out.extend_from_slice(&v.to_be_bytes()),
    };

    match bo {
        ByteOrderType::LittleEndian => out.extend_from_slice(b"II"),
        ByteOrderType::BigEndian => out.extend_from_slice(b"MM"),
    }
    put16(&mut out, 42);
    put32(&mut out, ifd_offset);

    put16(&mut out, ifd.len() as u16);
    for entry in &ifd {
        put16(&mut out, entry.tag);
        put16(&mut out, entry.val.field_type());
        put32(&mut out, entry.val.count());
        match entry.external_offset {
            Some(offset) => put32(&mut out, offset),
            None => {
                let mut bytes = entry.val.encode(bo);
                bytes.resize(4, 0);
                out.extend_from_slice(&bytes);
            }
        }
    }
    put32(&mut out, 0);

    for entry in &ifd {
        if let Some(offset) = entry.external_offset {
            assert_eq!(out.len() as u32, offset, "tag {} misplaced", entry.tag);
            out.extend_from_slice(&entry.val.encode(bo));
            if !out.len().is_multiple_of(2) {
                out.push(0);
            }
        }
    }
    for (index, block) in blocks.iter().enumerate() {
        assert_eq!(
            out.len() as u32,
            block_offsets[index],
            "block {index} misplaced"
        );
        out.extend_from_slice(block);
    }
    assert_eq!(out.len() as u32, cursor, "final layout disagrees");
    out
}

// ---------------------------------------------------------------------------
// The oracle: the single-band engine, woven by hand
// ---------------------------------------------------------------------------

/// Window of the raster to read: `None` = the whole thing.
type Window = Option<(u64, u64, u64, u64)>;

fn window_dims(spec: &Spec, window: Window) -> (usize, usize) {
    match window {
        Some((_, _, w, h)) => (w as usize, h as usize),
        None => (spec.width as usize, spec.height as usize),
    }
}

/// `bands` read one at a time through the single-band API and interleaved here.
fn oracle<T: RasterElement>(
    reader: &GeoTiffReader<CountingSource>,
    spec: &Spec,
    bands: &[usize],
    window: Window,
) -> Vec<T> {
    let (w, h) = window_dims(spec, window);
    let mut out = vec![T::default(); w * h * bands.len()];
    let mut plane = vec![T::default(); w * h];
    for (slot, &band) in bands.iter().enumerate() {
        match window {
            None => reader
                .read_band_into_typed(0, band, &mut plane)
                .expect("read_band_into_typed"),
            Some((x, y, width, height)) => reader
                .read_window_into_typed(0, band, x, y, width, height, &mut plane)
                .expect("read_window_into_typed"),
        }
        for (pixel, sample) in out.chunks_exact_mut(bands.len()).zip(plane.iter()) {
            pixel[slot] = *sample;
        }
    }
    out
}

fn multi<T: RasterElement>(
    reader: &GeoTiffReader<CountingSource>,
    spec: &Spec,
    bands: &[usize],
    window: Window,
) -> Vec<T> {
    let (w, h) = window_dims(spec, window);
    let mut out = vec![T::default(); w * h * bands.len()];
    match window {
        None => reader
            .read_bands_into_typed(0, bands, &mut out)
            .expect("read_bands_into_typed"),
        Some((x, y, width, height)) => reader
            .read_window_bands_into_typed(0, bands, x, y, width, height, &mut out)
            .expect("read_window_bands_into_typed"),
    }
    out
}

/// Every band selection exercised: file order, reversed, repeated, subset and
/// single.
const SELECTIONS: [&[usize]; 6] = [
    &[0, 1, 2],
    &[2, 1, 0],
    &[0, 0, 0],
    &[1, 2],
    &[2, 0, 2, 1],
    &[1],
];

const WINDOWS: [Window; 4] = [
    None,
    // Crosses a tile boundary in both axes.
    Some((3, 2, 14, 9)),
    // Wholly inside one block.
    Some((1, 1, 5, 3)),
    // Flush against the right/bottom edges.
    Some((12, 4, 8, 8)),
];

fn check_all_selections(spec: &Spec, bo: ByteOrderType) {
    let bytes = build_tiff(spec, bo);
    let (source, _reads) = CountingSource::new(bytes);
    let reader = GeoTiffReader::open(source).expect("open");
    let label = spec.label();

    for window in WINDOWS {
        for bands in SELECTIONS {
            let context = format!("{label} [{bo:?}] bands={bands:?} window={window:?}");
            assert_eq!(
                multi::<f64>(&reader, spec, bands, window),
                oracle::<f64>(&reader, spec, bands, window),
                "{context}: f64 destination"
            );
            assert_eq!(
                multi::<u16>(&reader, spec, bands, window),
                oracle::<u16>(&reader, spec, bands, window),
                "{context}: u16 destination"
            );
            assert_eq!(
                multi::<i32>(&reader, spec, bands, window),
                oracle::<i32>(&reader, spec, bands, window),
                "{context}: i32 destination"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 1. Structural proof: one decode per block
// ---------------------------------------------------------------------------

/// A chunky three-band read must fetch — and therefore decode — each block
/// once, where three single-band reads fetch each block three times.
#[test]
fn test_issue_14_chunky_multiband_decodes_each_block_once() {
    for spec in [
        Spec::tiled(Elem::U16, 1),
        Spec::tiled(Elem::U16, 1).striped(),
        Spec::tiled(Elem::F32, 1).with_predictor(Predictor::FloatingPoint),
    ] {
        let blocks = spec.blocks_per_plane();
        let bytes = build_tiff(&spec, ByteOrderType::LittleEndian);
        let (source, reads) = CountingSource::new(bytes);
        let reader = GeoTiffReader::open(source).expect("open");
        let pixels = spec.width as usize * spec.height as usize;
        let label = spec.label();

        let mut out = vec![0f64; pixels * 3];
        let ((), multi_reads) = count_reads(&reads, || {
            reader
                .read_bands_into_typed(0, &[0, 1, 2], &mut out)
                .expect("read_bands_into_typed");
        });

        let mut plane = vec![0f64; pixels];
        let ((), single_reads) = count_reads(&reads, || {
            for band in 0..3 {
                reader
                    .read_band_into_typed(0, band, &mut plane)
                    .expect("read_band_into_typed");
            }
        });

        eprintln!(
            "issue#14 {label}: {blocks} blocks — multi-band {multi_reads} decodes, \
             three single-band calls {single_reads}"
        );
        assert_eq!(
            multi_reads, blocks,
            "{label}: a 3-band read must decode each of the {blocks} blocks exactly once"
        );
        assert_eq!(
            single_reads,
            blocks * 3,
            "{label}: sanity — the single-band path decodes every block once per band"
        );
    }
}

/// The same for a compressed file, where a "decode" is unambiguously a
/// decompression.
#[cfg(feature = "deflate")]
#[test]
fn test_issue_14_chunky_multiband_decompresses_each_block_once() {
    let spec = Spec::tiled(Elem::U16, 1)
        .with_predictor(Predictor::HorizontalDifferencing)
        .with_compression(Compression::Deflate);
    let blocks = spec.blocks_per_plane();
    let bytes = build_tiff(&spec, ByteOrderType::LittleEndian);
    let (source, reads) = CountingSource::new(bytes);
    let reader = GeoTiffReader::open(source).expect("open");
    let pixels = spec.width as usize * spec.height as usize;

    let mut out = vec![0u16; pixels * 3];
    let ((), multi_reads) = count_reads(&reads, || {
        reader
            .read_bands_into_typed(0, &[0, 1, 2], &mut out)
            .expect("read_bands_into_typed");
    });
    assert_eq!(
        multi_reads, blocks,
        "a DEFLATE 3-band read must inflate each of the {blocks} blocks exactly once"
    );

    // ... and the pixels are still right.
    assert_eq!(
        out,
        oracle::<u16>(&reader, &spec, &[0, 1, 2], None),
        "DEFLATE multi-band read disagrees with the single-band engine"
    );
}

/// A repeated band never costs a second decode, whatever the layout: the block
/// is already in hand on the chunky path, and the plane is copied out of the
/// slot that already holds it on the planar one.
#[test]
fn test_issue_14_repeated_band_decodes_once() {
    for planar in [1u16, 2] {
        let spec = Spec::tiled(Elem::U16, planar);
        let blocks = spec.blocks_per_plane();
        let bytes = build_tiff(&spec, ByteOrderType::LittleEndian);
        let (source, reads) = CountingSource::new(bytes);
        let reader = GeoTiffReader::open(source).expect("open");
        let pixels = spec.width as usize * spec.height as usize;

        let mut out = vec![0u16; pixels * 3];
        let ((), grey_reads) = count_reads(&reads, || {
            reader
                .read_bands_into_typed(0, &[1, 1, 1], &mut out)
                .expect("read_bands_into_typed");
        });
        assert_eq!(
            grey_reads, blocks,
            "planar={planar}: band 1 fanned out to three slots must decode {blocks} blocks once"
        );
        assert_eq!(
            out,
            oracle::<u16>(&reader, &spec, &[1, 1, 1], None),
            "planar={planar}: fanned-out band disagrees with the single-band engine"
        );
    }
}

/// Planar is the case with nothing to win: one block holds one band, so an
/// `n`-band read decodes `n` blocks per position no matter how it is written.
/// The multi-band path must not decode *more* than that.
#[test]
fn test_issue_14_planar_multiband_decodes_each_plane_block_once() {
    for spec in [
        Spec::tiled(Elem::U16, 2),
        Spec::tiled(Elem::U16, 2).striped(),
    ] {
        let blocks = spec.blocks_per_plane();
        let bytes = build_tiff(&spec, ByteOrderType::LittleEndian);
        let (source, reads) = CountingSource::new(bytes);
        let reader = GeoTiffReader::open(source).expect("open");
        let pixels = spec.width as usize * spec.height as usize;
        let label = spec.label();

        let mut out = vec![0f64; pixels * 3];
        let ((), multi_reads) = count_reads(&reads, || {
            reader
                .read_bands_into_typed(0, &[0, 1, 2], &mut out)
                .expect("read_bands_into_typed");
        });
        assert_eq!(
            multi_reads,
            blocks * 3,
            "{label}: three planes of {blocks} blocks, each decoded once"
        );
    }
}

/// The rayon path has its own block-row split and its own per-worker scratch,
/// so it needs a raster large enough to cross `PARALLEL_MIN_BYTES` before it is
/// taken at all. Above that threshold the answer must be identical to the
/// serial one — and each block must still be decoded exactly once.
///
/// Without the `parallel` feature this simply exercises the serial path on a
/// raster far larger than the rest of the file uses, which is worth having
/// anyway.
#[test]
fn test_issue_14_multiband_large_read_matches_single_band_engine() {
    let spec = Spec {
        width: 1024,
        height: 600,
        rows_per_strip: 16,
        ..Spec::tiled(Elem::U16, 1).striped()
    };
    // Comfortably past the 1 MiB the parallel path demands of a single band.
    assert!(spec.width as usize * spec.height as usize * 2 > 1 << 20);

    let blocks = spec.blocks_per_plane();
    let bytes = build_tiff(&spec, ByteOrderType::LittleEndian);
    let (source, reads) = CountingSource::new(bytes);
    let reader = GeoTiffReader::open(source).expect("open");
    let pixels = spec.width as usize * spec.height as usize;

    let bands = [2usize, 1, 0];
    let mut out = vec![0u16; pixels * bands.len()];
    let ((), multi_reads) = count_reads(&reads, || {
        reader
            .read_bands_into_typed(0, &bands, &mut out)
            .expect("read_bands_into_typed");
    });
    assert_eq!(
        multi_reads, blocks,
        "a large 3-band read must still decode each of the {blocks} strips once"
    );
    assert_eq!(
        out,
        oracle::<u16>(&reader, &spec, &bands, None),
        "large multi-band read disagrees with the single-band engine"
    );

    // ... and windowed, so the parallel split is exercised off a block boundary.
    let window = Some((7u64, 5, 1000, 560));
    assert_eq!(
        multi::<f64>(&reader, &spec, &bands, window),
        oracle::<f64>(&reader, &spec, &bands, window),
        "large multi-band window read disagrees with the single-band engine"
    );
}

/// The planar counterpart of the above: large enough for the rayon split, with
/// each plane still decoded exactly once.
#[test]
fn test_issue_14_planar_large_read_matches_single_band_engine() {
    let spec = Spec {
        width: 1024,
        height: 600,
        rows_per_strip: 16,
        ..Spec::tiled(Elem::U16, 2).striped()
    };
    let blocks = spec.blocks_per_plane();
    let bytes = build_tiff(&spec, ByteOrderType::LittleEndian);
    let (source, reads) = CountingSource::new(bytes);
    let reader = GeoTiffReader::open(source).expect("open");
    let pixels = spec.width as usize * spec.height as usize;

    let bands = [1usize, 0, 1];
    let mut out = vec![0f64; pixels * bands.len()];
    let ((), multi_reads) = count_reads(&reads, || {
        reader
            .read_bands_into_typed(0, &bands, &mut out)
            .expect("read_bands_into_typed");
    });
    assert_eq!(
        multi_reads,
        blocks * 2,
        "two distinct planes of {blocks} blocks, each decoded once; the repeated \
         band must be copied, not decoded again"
    );
    assert_eq!(
        out,
        oracle::<f64>(&reader, &spec, &bands, None),
        "large planar multi-band read disagrees with the single-band engine"
    );
}

// ---------------------------------------------------------------------------
// 2. Correctness matrix against the single-band oracle
// ---------------------------------------------------------------------------

#[test]
fn test_issue_14_multiband_matches_single_band_engine() {
    for bo in [ByteOrderType::LittleEndian, ByteOrderType::BigEndian] {
        for planar in [1u16, 2] {
            for elem in [Elem::U8, Elem::U16, Elem::F32] {
                let predictor = match elem {
                    Elem::F32 => Predictor::FloatingPoint,
                    _ => Predictor::HorizontalDifferencing,
                };
                for spec in [
                    Spec::tiled(elem, planar),
                    Spec::tiled(elem, planar).striped(),
                    Spec::tiled(elem, planar).with_predictor(predictor),
                    Spec::tiled(elem, planar)
                        .striped()
                        .with_predictor(predictor),
                ] {
                    check_all_selections(&spec, bo);
                }
            }
        }
    }
}

#[cfg(feature = "deflate")]
#[test]
fn test_issue_14_multiband_matches_single_band_engine_compressed() {
    for planar in [1u16, 2] {
        for spec in [
            Spec::tiled(Elem::U16, planar).with_compression(Compression::Deflate),
            Spec::tiled(Elem::U16, planar)
                .striped()
                .with_compression(Compression::Deflate)
                .with_predictor(Predictor::HorizontalDifferencing),
        ] {
            check_all_selections(&spec, ByteOrderType::LittleEndian);
        }
    }
}

/// One configuration checked against ground truth computed without the driver,
/// so a wrong oracle could not hide behind the comparisons above.
#[test]
fn test_issue_14_multiband_matches_ground_truth() {
    let spec = Spec::tiled(Elem::U16, 1);
    let bytes = build_tiff(&spec, ByteOrderType::BigEndian);
    let (source, _reads) = CountingSource::new(bytes);
    let reader = GeoTiffReader::open(source).expect("open");

    let bands = [2usize, 0, 1];
    let mut out = vec![0u16; spec.width as usize * spec.height as usize * bands.len()];
    reader
        .read_bands_into_typed(0, &bands, &mut out)
        .expect("read_bands_into_typed");

    let mut expected = Vec::with_capacity(out.len());
    for y in 0..spec.height as usize {
        for x in 0..spec.width as usize {
            for &band in &bands {
                expected.push(raw_bits(Elem::U16, band, y, x) as u16);
            }
        }
    }
    assert_eq!(out, expected, "interleaved BGR read disagrees with truth");
}

// ---------------------------------------------------------------------------
// 3. Argument validation
// ---------------------------------------------------------------------------

#[test]
fn test_issue_14_multiband_rejects_bad_arguments() {
    let spec = Spec::tiled(Elem::U16, 1);
    let bytes = build_tiff(&spec, ByteOrderType::LittleEndian);
    let (source, _reads) = CountingSource::new(bytes);
    let reader = GeoTiffReader::open(source).expect("open");
    let pixels = spec.width as usize * spec.height as usize;

    let mut buf = vec![0u16; pixels * 3];
    assert!(
        reader.read_bands_into_typed(0, &[], &mut buf).is_err(),
        "an empty band selection must be rejected, not silently read as nothing"
    );
    assert!(
        reader
            .read_bands_into_typed(0, &[0, 3, 1], &mut buf)
            .is_err(),
        "a band index past SamplesPerPixel must be rejected"
    );
    assert!(
        reader.read_bands_into_typed(0, &[0, 1], &mut buf).is_err(),
        "dst must be exactly pixels x bands.len()"
    );
    assert!(
        reader
            .read_bands_into_typed(9, &[0, 1, 2], &mut buf)
            .is_err(),
        "a level that names no overview must be rejected"
    );
    assert!(
        reader
            .read_window_bands_into_typed(0, &[0, 1, 2], 0, 0, 0, 4, &mut buf)
            .is_err(),
        "a zero-sized window must be rejected"
    );
    let mut small = vec![0u16; 3 * 4 * 3];
    assert!(
        reader
            .read_window_bands_into_typed(0, &[0, 1, 2], 18, 0, 3, 4, &mut small)
            .is_err(),
        "a window running past the right edge must be rejected"
    );
    assert!(
        reader
            .read_window_bands_into_typed(0, &[0, 1, 2], 2, 2, 3, 4, &mut small)
            .is_ok(),
        "the same window inside the extent must succeed"
    );
}
