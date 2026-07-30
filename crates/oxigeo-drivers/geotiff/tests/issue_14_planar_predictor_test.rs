//! Regression tests for the tile-level read APIs on **planar** rasters
//! (`PlanarConfiguration = 2`), cool-japan/oxigeo#14.
//!
//! A planar TIFF stores `SamplesPerPixel × TilesPerImage` blocks in plane-major
//! order and each of those blocks holds exactly **one** band. Two things follow,
//! and [`CogReader`] used to get both wrong:
//!
//! 1. A block's decoded size is `block_w · block_h · bytes_per_sample` — *not*
//!    that times `SamplesPerPixel`, which is what a chunky block holds.
//! 2. Both TIFF predictors are parameterised by the number of samples per pixel
//!    **in the block being decoded**, which is 1 for a planar block whatever
//!    `SamplesPerPixel` says. Reversing horizontal differencing with a stride of
//!    3 on a single-band plane subtracts the wrong neighbour from every sample
//!    and also mis-computes the scanline length (`width · 3 · bps`), so rows
//!    bleed into each other. Nothing errors; the pixels are simply wrong.
//!
//! The band-level API (`GeoTiffReader::read_band`/`read_window`, and therefore
//! `read_tile_band_buffer`) always had its own plane-aware branch in
//! `band_read::decode_block`, so it is used here as an independent oracle: the
//! tile APIs must agree with it, and both must agree with the ground truth this
//! file computes without the driver.
//!
//! Fixtures are built here rather than by the crate's writer because the writer
//! only ever emits `PlanarConfiguration = 1`.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use oxigeo_core::error::Result;
use oxigeo_core::io::{ByteRange, DataSource};
use oxigeo_geotiff::compression;
use oxigeo_geotiff::tiff::{ByteOrderType, Compression, Predictor, TiffTag};
use oxigeo_geotiff::{CogReader, GeoTiffReader};

// ---------------------------------------------------------------------------
// In-memory data source
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct MemorySource(Vec<u8>);

impl DataSource for MemorySource {
    fn size(&self) -> Result<u64> {
        Ok(self.0.len() as u64)
    }

    fn read_range(&self, range: ByteRange) -> Result<Vec<u8>> {
        let start = (range.start as usize).min(self.0.len());
        let end = (range.end as usize).min(self.0.len());
        Ok(self.0[start..end.max(start)].to_vec())
    }
}

// ---------------------------------------------------------------------------
// Sample types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Elem {
    U16,
    U32,
    F32,
    F64,
}

impl Elem {
    const fn bytes(self) -> usize {
        match self {
            Self::U16 => 2,
            Self::U32 | Self::F32 => 4,
            Self::F64 => 8,
        }
    }

    const fn bits(self) -> u16 {
        (self.bytes() * 8) as u16
    }

    /// TIFF `SampleFormat`: 1 = unsigned, 3 = IEEE float.
    const fn sample_format(self) -> u16 {
        match self {
            Self::U16 | Self::U32 => 1,
            Self::F32 | Self::F64 => 3,
        }
    }
}

/// Bit pattern of the sample at `(band, y, x)`; consecutive samples differ in
/// every byte so a mis-strided predictor cannot coincidentally decode right.
fn raw_bits(elem: Elem, band: usize, y: usize, x: usize) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for v in [band as u64 + 1, y as u64 + 1, x as u64 + 1] {
        h ^= v;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    match elem {
        Elem::U16 => h & 0xffff,
        Elem::U32 => h & 0xffff_ffff,
        Elem::F32 => {
            let v = ((h % 2_000_003) as f32 - 1_000_001.0) / 3.0;
            u64::from(v.to_bits())
        }
        Elem::F64 => {
            let v = ((h % 2_000_003) as f64 - 1_000_001.0) / 3.0;
            v.to_bits()
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

/// The byte order the host stores samples in — what every decoded read produces.
const fn host_order() -> ByteOrderType {
    if cfg!(target_endian = "big") {
        ByteOrderType::BigEndian
    } else {
        ByteOrderType::LittleEndian
    }
}

// ---------------------------------------------------------------------------
// Fixture specification
// ---------------------------------------------------------------------------

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
}

impl Spec {
    const fn tiled(elem: Elem, planar: u16, predictor: Predictor) -> Self {
        Self {
            width: 20,
            height: 12,
            elem,
            bands: 3,
            planar,
            tile: Some((8, 8)),
            rows_per_strip: 5,
            predictor,
        }
    }

    const fn striped(mut self) -> Self {
        self.tile = None;
        self
    }

    /// `(block_w, block_h, across, down)` — `down` counts block rows of **one**
    /// plane, which is what `ImageInfo::tiles_down` reports too.
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

    /// Samples stored per pixel inside one block.
    const fn samples_in_block(&self) -> usize {
        if self.planar == 2 {
            1
        } else {
            self.bands as usize
        }
    }

    /// Planes stored on disk (one for chunky, `bands` for planar).
    const fn planes(&self) -> usize {
        if self.planar == 2 {
            self.bands as usize
        } else {
            1
        }
    }

    /// Pixel rows held by block row `ty`.
    const fn block_rows(&self, ty: u32) -> u32 {
        let (_, bh, _, _) = self.block_grid();
        if self.tile.is_some() {
            bh
        } else {
            let remaining = self.height.saturating_sub(ty * bh);
            if remaining < bh { remaining } else { bh }
        }
    }

    /// The flat block index the tile APIs address block `(band, tx, ty)` with.
    ///
    /// TIFF stores planar blocks plane-major, and `CogReader` indexes blocks as
    /// `row × tiles_across + tx`, so the plane is selected by offsetting the
    /// block row — exactly what `band_read::decode_block` does.
    const fn block_row(&self, band: usize, ty: u32) -> u32 {
        let (_, _, _, down) = self.block_grid();
        if self.planar == 2 {
            band as u32 * down + ty
        } else {
            ty
        }
    }
}

// ---------------------------------------------------------------------------
// Ground truth
// ---------------------------------------------------------------------------

/// The decoded bytes of block `(band, tx, ty)` in `bo`, padding included.
fn block_samples(spec: &Spec, band: usize, tx: u32, ty: u32, bo: ByteOrderType) -> Vec<u8> {
    let (bw, bh, _, _) = spec.block_grid();
    let rows = spec.block_rows(ty);
    let per_pixel = spec.samples_in_block();
    let elem = spec.elem;

    let mut out = Vec::with_capacity(bw as usize * rows as usize * per_pixel * elem.bytes());
    for row in 0..rows {
        let y = ty * bh + row;
        for col in 0..bw {
            let x = tx * bw + col;
            for s in 0..per_pixel {
                let b = if spec.planar == 2 { band } else { s };
                if x < spec.width && y < spec.height {
                    out.extend_from_slice(&file_bytes(
                        elem,
                        raw_bits(elem, b, y as usize, x as usize),
                        bo,
                    ));
                } else {
                    out.extend(std::iter::repeat_n(0u8, elem.bytes()));
                }
            }
        }
    }
    out
}

/// One whole band in host order — the oracle for `read_band`.
fn expected_band_native(spec: &Spec, band: usize) -> Vec<u8> {
    let mut out =
        Vec::with_capacity(spec.width as usize * spec.height as usize * spec.elem.bytes());
    for y in 0..spec.height as usize {
        for x in 0..spec.width as usize {
            out.extend_from_slice(&file_bytes(
                spec.elem,
                raw_bits(spec.elem, band, y, x),
                host_order(),
            ));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// TIFF construction
// ---------------------------------------------------------------------------

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

/// Every block, predicted and in on-disk (plane-major) order.
fn encode_blocks(spec: &Spec, bo: ByteOrderType) -> Vec<Vec<u8>> {
    let (bw, _, across, down) = spec.block_grid();
    // The encoder is the spec: a planar block holds one sample per pixel, so the
    // predictor stride is 1 whatever `SamplesPerPixel` says.
    let predictor_spp = spec.samples_in_block();

    let mut blocks = Vec::with_capacity(spec.planes() * across as usize * down as usize);
    for band in 0..spec.planes() {
        for ty in 0..down {
            for tx in 0..across {
                let mut raw = block_samples(spec, band, tx, ty, bo);
                compression::apply_predictor_forward(
                    &mut raw,
                    spec.predictor,
                    spec.elem.bytes(),
                    predictor_spp,
                    bw as usize,
                    bo,
                )
                .expect("predictor encode");
                blocks.push(raw);
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
            FieldVal::Short(vec![Compression::None as u16]),
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
// Shared assertion
// ---------------------------------------------------------------------------

/// Reads every block of `spec` through `CogReader::read_tile`,
/// `CogReader::read_tile_into` and `GeoTiffReader::read_band`, in both byte
/// orders, and checks all of them against the ground truth.
fn assert_tiles_match(spec: &Spec, label: &str) {
    for bo in [ByteOrderType::LittleEndian, ByteOrderType::BigEndian] {
        let bytes = build_tiff(spec, bo);
        let cog = CogReader::open(MemorySource(bytes.clone())).expect("open CogReader");
        let reader = GeoTiffReader::open(MemorySource(bytes)).expect("open GeoTiffReader");

        let (bw, _, across, down) = spec.block_grid();
        let bps = spec.elem.bytes();

        for band in 0..spec.planes() {
            for ty in 0..down {
                let block_row = spec.block_row(band, ty);

                // 1. `read_tile` decodes exactly the bytes the codec produced, so
                //    its length is right even when the geometry is not: this
                //    assertion isolates the *predictor stride*.
                for tx in 0..across {
                    let expected = block_samples(spec, band, tx, ty, host_order());
                    let tile = cog.read_tile(0, tx, block_row).expect("read_tile");
                    assert_eq!(
                        tile.len(),
                        expected.len(),
                        "{label} [{bo:?}]: read_tile({tx},{block_row}) length"
                    );
                    assert_eq!(
                        tile, expected,
                        "{label} [{bo:?}]: read_tile({tx},{block_row}) decoded band {band} wrong"
                    );
                }

                // 2. A planar block holds one band, so its decoded size must not
                //    be multiplied by SamplesPerPixel.
                let expected_size =
                    bw as usize * spec.block_rows(ty) as usize * spec.samples_in_block() * bps;
                let reported = cog
                    .tile_decoded_size(0, block_row)
                    .expect("tile_decoded_size");
                assert_eq!(
                    reported, expected_size,
                    "{label} [{bo:?}]: tile_decoded_size(band {band}, row {ty}) must describe \
                     one block, not one whole pixel-interleaved block"
                );

                // 3. ... and `read_tile_into`, whose buffer that size describes.
                for tx in 0..across {
                    let expected = block_samples(spec, band, tx, ty, host_order());
                    let mut into = vec![0u8; reported];
                    cog.read_tile_into(0, tx, block_row, &mut into)
                        .expect("read_tile_into");
                    assert_eq!(
                        into, expected,
                        "{label} [{bo:?}]: read_tile_into({tx},{block_row}) disagrees with \
                         read_tile"
                    );
                }
            }
        }

        // The band-level engine is plane-aware already; the tile APIs must agree
        // with it, so a fixture that fooled one would still be caught.
        for band in 0..spec.bands as usize {
            assert_eq!(
                reader.read_band(0, band).expect("read_band"),
                expected_band_native(spec, band),
                "{label} [{bo:?}]: read_band({band}) disagrees with ground truth"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The defect: planar + predictor through the tile APIs
// ---------------------------------------------------------------------------

/// `PlanarConfiguration=2` + `PREDICTOR=2`: the corrupting combination.
#[test]
fn test_issue_14_planar_predictor_2_tile_apis() {
    for elem in [Elem::U16, Elem::U32] {
        for spec in [
            Spec::tiled(elem, 2, Predictor::HorizontalDifferencing),
            Spec::tiled(elem, 2, Predictor::HorizontalDifferencing).striped(),
        ] {
            assert_tiles_match(&spec, &format!("planar predictor 2 {elem:?}"));
        }
    }
}

/// `PlanarConfiguration=2` + `PREDICTOR=3` (floating point).
#[test]
fn test_issue_14_planar_predictor_3_tile_apis() {
    for elem in [Elem::F32, Elem::F64] {
        for spec in [
            Spec::tiled(elem, 2, Predictor::FloatingPoint),
            Spec::tiled(elem, 2, Predictor::FloatingPoint).striped(),
        ] {
            assert_tiles_match(&spec, &format!("planar predictor 3 {elem:?}"));
        }
    }
}

/// Planar without any predictor: only the block *size* is at stake here, which
/// is what makes `read_tile_into` usable at all on a planar file.
#[test]
fn test_issue_14_planar_no_predictor_tile_apis() {
    for elem in [Elem::U16, Elem::F32] {
        for spec in [
            Spec::tiled(elem, 2, Predictor::None),
            Spec::tiled(elem, 2, Predictor::None).striped(),
        ] {
            assert_tiles_match(&spec, &format!("planar no predictor {elem:?}"));
        }
    }
}

/// The chunky control: identical rasters stored interleaved must keep decoding
/// exactly as before, predictor stride `SamplesPerPixel` and all. A fix that
/// simply forced the stride to 1 everywhere would fail here.
#[test]
fn test_issue_14_chunky_tile_apis_unchanged() {
    for (elem, predictor) in [
        (Elem::U16, Predictor::HorizontalDifferencing),
        (Elem::U32, Predictor::HorizontalDifferencing),
        (Elem::F32, Predictor::FloatingPoint),
        (Elem::F64, Predictor::FloatingPoint),
        (Elem::U16, Predictor::None),
    ] {
        for spec in [
            Spec::tiled(elem, 1, predictor),
            Spec::tiled(elem, 1, predictor).striped(),
        ] {
            assert_tiles_match(&spec, &format!("chunky {elem:?} {predictor:?}"));
        }
    }
}
