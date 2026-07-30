//! Regression tests for cool-japan/oxigeo#14 — decoded samples must reach the
//! caller in the **host's** byte order, never the file's.
//!
//! The GeoTIFF driver used to hand back whatever byte order the file declared,
//! while `oxigeo_core::buffer::RasterBuffer` (no byte-order field; `get_pixel`,
//! `get_u16`…`get_f64`, `as_slice`) and `convert_raw_into` (`from_ne_bytes`) are
//! all native-endian. Every numeric value read from a big-endian (`MM`) GeoTIFF
//! anywhere in the workspace was therefore silently byte-reversed — no error, no
//! warning, plausible-looking garbage.
//!
//! # How these tests are built
//!
//! Each case builds a **pair** of synthetic TIFFs from one set of logical sample
//! values: an `MM` file and its `II` twin, identical in every respect except the
//! header magic, the byte order of the IFD scalars, and the byte order of the
//! samples (and, where a predictor is in play, the predictor encoding — which is
//! itself defined on file-order data). Two assertions then hold for every
//! normalising entry point:
//!
//! 1. `read(MM) == read(II)` — the file's byte order must be invisible; and
//! 2. `read(II) == expected` — where `expected` is the sample bit patterns laid
//!    out in *host* order, computed independently of the driver.
//!
//! Together these rule out both "forgot to swap" and "swapped everything twice".
//!
//! # Ordering versus the predictor
//!
//! [`test_issue_14_byte_order_predictor_2`] and
//! [`test_issue_14_byte_order_predictor_3`] are the load-bearing ones: both TIFF
//! predictors are defined on file-order samples (horizontal differencing carries
//! across a whole sample read with the file's byte order; the floating-point
//! predictor de-interleaves byte planes stored most-significant-first). A swap
//! placed *before* the predictor reversal decodes without error and produces
//! wrong pixels, so these two cases are what pins the swap to its only correct
//! position.

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
// Element types
// ---------------------------------------------------------------------------

/// The sample types this suite exercises.
///
/// `U8`/`I8` are the no-swap controls: their bytes must come back untouched
/// whatever the file's byte order says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Elem {
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    F32,
    F64,
}

impl Elem {
    const ALL: [Self; 10] = [
        Self::U8,
        Self::I8,
        Self::U16,
        Self::I16,
        Self::U32,
        Self::I32,
        Self::U64,
        Self::I64,
        Self::F32,
        Self::F64,
    ];

    const fn bytes(self) -> usize {
        match self {
            Self::U8 | Self::I8 => 1,
            Self::U16 | Self::I16 => 2,
            Self::U32 | Self::I32 | Self::F32 => 4,
            Self::U64 | Self::I64 | Self::F64 => 8,
        }
    }

    const fn bits(self) -> u16 {
        (self.bytes() * 8) as u16
    }

    /// TIFF `SampleFormat`: 1 = unsigned, 2 = signed, 3 = IEEE float.
    const fn sample_format(self) -> u16 {
        match self {
            Self::U8 | Self::U16 | Self::U32 | Self::U64 => 1,
            Self::I8 | Self::I16 | Self::I32 | Self::I64 => 2,
            Self::F32 | Self::F64 => 3,
        }
    }

    /// Only the DEFLATE case picks a predictor by sample class.
    #[cfg(feature = "deflate")]
    const fn is_float(self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }
}

/// The raw bit pattern of the sample at `(level, band, y, x)`.
///
/// An FNV-style hash, so consecutive samples differ in *every* byte: a missed or
/// doubled swap cannot coincidentally survive. Float patterns are built from a
/// finite value rather than from raw hash bits so that no fixture ever contains a
/// NaN (which would make the equality assertions meaningless).
fn raw_bits(elem: Elem, level: usize, band: usize, y: usize, x: usize) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for v in [
        level as u64 + 1,
        band as u64 + 1,
        y as u64 + 1,
        x as u64 + 1,
    ] {
        h ^= v;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    match elem {
        Elem::U8 | Elem::I8 => h & 0xff,
        Elem::U16 | Elem::I16 => h & 0xffff,
        Elem::U32 | Elem::I32 => h & 0xffff_ffff,
        Elem::U64 | Elem::I64 => h,
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

/// The low `elem.bytes()` bytes of `bits`, ordered as `byte_order` stores them.
fn file_bytes(elem: Elem, bits: u64, byte_order: ByteOrderType) -> Vec<u8> {
    let mut out = bits.to_le_bytes()[..elem.bytes()].to_vec();
    if byte_order == ByteOrderType::BigEndian {
        out.reverse();
    }
    out
}

/// The same sample as the host stores it — the ground truth every read is
/// checked against.
fn native_bytes(elem: Elem, bits: u64) -> Vec<u8> {
    let mut out = bits.to_le_bytes()[..elem.bytes()].to_vec();
    if cfg!(target_endian = "big") {
        out.reverse();
    }
    out
}

// ---------------------------------------------------------------------------
// Fixture specification
// ---------------------------------------------------------------------------

/// One synthetic raster, described independently of its byte order so that the
/// `MM` and `II` twins are provably the same image.
#[derive(Debug, Clone, Copy)]
struct Spec {
    width: u32,
    height: u32,
    elem: Elem,
    bands: u16,
    /// TIFF `PlanarConfiguration`: 1 = chunky (interleaved), 2 = planar.
    planar: u16,
    /// `Some((tile_w, tile_h))` for a tiled raster, `None` for a striped one.
    tile: Option<(u32, u32)>,
    /// `RowsPerStrip`, used only when `tile` is `None`.
    rows_per_strip: u32,
    compression: Compression,
    predictor: Predictor,
    /// Whether to append a half-size overview IFD.
    overview: bool,
}

impl Spec {
    const fn base(elem: Elem) -> Self {
        Self {
            width: 37,
            height: 29,
            elem,
            bands: 1,
            planar: 1,
            tile: Some((16, 16)),
            rows_per_strip: 8,
            compression: Compression::None,
            predictor: Predictor::None,
            overview: false,
        }
    }

    const fn striped(mut self) -> Self {
        self.tile = None;
        self
    }

    const fn bands(mut self, bands: u16, planar: u16) -> Self {
        self.bands = bands;
        self.planar = planar;
        self
    }

    const fn compression(mut self, compression: Compression) -> Self {
        self.compression = compression;
        self
    }

    const fn predictor(mut self, predictor: Predictor) -> Self {
        self.predictor = predictor;
        self
    }

    const fn overview(mut self) -> Self {
        self.overview = true;
        self
    }

    /// Dimensions of resolution level `level` (level 1 is the half-size overview).
    fn level_size(&self, level: usize) -> (u32, u32) {
        if level == 0 {
            (self.width, self.height)
        } else {
            (
                self.width.div_ceil(2).max(1),
                self.height.div_ceil(2).max(1),
            )
        }
    }

    /// Block grid of `level`: `(block_w, block_h, across, down)`.
    fn block_grid(&self, level: usize) -> (u32, u32, u32, u32) {
        let (w, h) = self.level_size(level);
        match self.tile {
            Some((tw, th)) => (tw, th, w.div_ceil(tw), h.div_ceil(th)),
            None => (w, self.rows_per_strip, 1, h.div_ceil(self.rows_per_strip)),
        }
    }

    /// Samples stored per pixel inside one block (all bands, or exactly one).
    const fn samples_in_block(&self) -> usize {
        if self.planar == 2 {
            1
        } else {
            self.bands as usize
        }
    }

    /// Rows of pixels held by block row `ty` of `level`.
    fn block_rows(&self, level: usize, ty: u32) -> u32 {
        let (_, bh, _, _) = self.block_grid(level);
        if self.tile.is_some() {
            // Tiles are always full height; the overhang is padding.
            bh
        } else {
            let (_, h) = self.level_size(level);
            h.saturating_sub(ty * bh).min(bh)
        }
    }
}

// ---------------------------------------------------------------------------
// TIFF construction
// ---------------------------------------------------------------------------

/// A tag value, in the two field types these fixtures need.
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

    /// The packed value array in `byte_order`.
    fn encode(&self, byte_order: ByteOrderType) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.byte_len());
        match self {
            Self::Short(v) => {
                for &x in v {
                    match byte_order {
                        ByteOrderType::LittleEndian => out.extend_from_slice(&x.to_le_bytes()),
                        ByteOrderType::BigEndian => out.extend_from_slice(&x.to_be_bytes()),
                    }
                }
            }
            Self::Long(v) => {
                for &x in v {
                    match byte_order {
                        ByteOrderType::LittleEndian => out.extend_from_slice(&x.to_le_bytes()),
                        ByteOrderType::BigEndian => out.extend_from_slice(&x.to_be_bytes()),
                    }
                }
            }
        }
        out
    }
}

/// One IFD entry, with the file offset assigned to it if its value is external.
#[derive(Debug, Clone)]
struct Entry {
    tag: u16,
    val: FieldVal,
    external_offset: Option<u32>,
}

/// Produces the decoded, file-order bytes of one block of `level`.
///
/// Padding beyond the raster edge is zero, matching what a writer emits and what
/// the reader expects to be able to ignore.
fn block_samples(
    spec: &Spec,
    level: usize,
    band: usize,
    tx: u32,
    ty: u32,
    bo: ByteOrderType,
) -> Vec<u8> {
    let (w, h) = spec.level_size(level);
    let (bw, bh, _, _) = spec.block_grid(level);
    let rows = spec.block_rows(level, ty);
    let per_pixel = spec.samples_in_block();
    let elem = spec.elem;

    let mut out = Vec::with_capacity(bw as usize * rows as usize * per_pixel * elem.bytes());
    for row in 0..rows {
        let y = ty * bh + row;
        for col in 0..bw {
            let x = tx * bw + col;
            for s in 0..per_pixel {
                let b = if spec.planar == 2 { band } else { s };
                if x < w && y < h {
                    let bits = raw_bits(elem, level, b, y as usize, x as usize);
                    out.extend_from_slice(&file_bytes(elem, bits, bo));
                } else {
                    out.extend(std::iter::repeat_n(0u8, elem.bytes()));
                }
            }
        }
    }
    out
}

/// Every block of `level`, already predicted and compressed, in on-disk block
/// order (plane-major for a planar file, exactly as TIFF specifies).
fn encode_blocks(spec: &Spec, level: usize, bo: ByteOrderType) -> Vec<Vec<u8>> {
    let (bw, _, across, down) = spec.block_grid(level);
    let planes = if spec.planar == 2 {
        spec.bands as usize
    } else {
        1
    };
    let predictor_spp = spec.samples_in_block();

    let mut blocks = Vec::with_capacity(planes * across as usize * down as usize);
    for band in 0..planes {
        for ty in 0..down {
            for tx in 0..across {
                let mut raw = block_samples(spec, level, band, tx, ty, bo);
                // Encode with exactly the parameters the reader will decode with:
                // the predictor operates on file-order samples, so this runs
                // *before* anything byte-order-normalising ever could.
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

/// Builds the entry list of one IFD, with placeholder block offsets.
fn ifd_entries(spec: &Spec, level: usize, block_count: usize) -> Vec<Entry> {
    let (w, h) = spec.level_size(level);
    let (bw, bh, _, _) = spec.block_grid(level);
    let bands = spec.bands as usize;

    let mut entries: Vec<(u16, FieldVal)> = Vec::new();
    if level > 0 {
        // NewSubfileType = 1 (reduced-resolution image). The crate's `TiffTag`
        // enum does not name tag 254, so it is written numerically.
        entries.push((254u16, FieldVal::Long(vec![1])));
    }
    entries.push((TiffTag::ImageWidth as u16, FieldVal::Long(vec![w])));
    entries.push((TiffTag::ImageLength as u16, FieldVal::Long(vec![h])));
    entries.push((
        TiffTag::BitsPerSample as u16,
        FieldVal::Short(vec![spec.elem.bits(); bands]),
    ));
    entries.push((
        TiffTag::Compression as u16,
        FieldVal::Short(vec![spec.compression as u16]),
    ));
    entries.push((
        TiffTag::PhotometricInterpretation as u16,
        FieldVal::Short(vec![1]),
    ));
    entries.push((
        TiffTag::SamplesPerPixel as u16,
        FieldVal::Short(vec![spec.bands]),
    ));
    entries.push((
        TiffTag::PlanarConfiguration as u16,
        FieldVal::Short(vec![spec.planar]),
    ));
    entries.push((
        TiffTag::Predictor as u16,
        FieldVal::Short(vec![spec.predictor as u16]),
    ));
    entries.push((
        TiffTag::SampleFormat as u16,
        FieldVal::Short(vec![spec.elem.sample_format(); bands]),
    ));

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

    // TIFF requires ascending tag order.
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

/// Serialises a complete classic TIFF for `spec` in `byte_order`.
///
/// The `MM` and `II` outputs of one `spec` differ only in the header magic, the
/// encoding of every IFD scalar, and the byte order of the samples — the image
/// they describe is identical.
fn build_tiff(spec: &Spec, byte_order: ByteOrderType) -> Vec<u8> {
    let levels = if spec.overview { 2 } else { 1 };

    // 1. Encode the pixel data first: block sizes drive the whole layout.
    let blocks: Vec<Vec<Vec<u8>>> = (0..levels)
        .map(|level| encode_blocks(spec, level, byte_order))
        .collect();

    // 2. Build each IFD's entries (sizes are final; only offset *values* change).
    let mut ifds: Vec<Vec<Entry>> = (0..levels)
        .map(|level| ifd_entries(spec, level, blocks[level].len()))
        .collect();

    // 3. Lay the file out: header, IFD chain, external tag values, pixel data.
    let mut ifd_offsets = Vec::with_capacity(levels);
    let mut cursor: u32 = 8;
    for ifd in &ifds {
        ifd_offsets.push(cursor);
        cursor += 2 + 12 * ifd.len() as u32 + 4;
    }
    for ifd in &mut ifds {
        for entry in ifd.iter_mut() {
            if entry.val.byte_len() > 4 {
                entry.external_offset = Some(cursor);
                cursor += entry.val.byte_len() as u32;
                cursor += cursor % 2; // keep values word-aligned
            }
        }
    }
    let mut block_offsets: Vec<Vec<u32>> = Vec::with_capacity(levels);
    for level_blocks in &blocks {
        let mut offsets = Vec::with_capacity(level_blocks.len());
        for block in level_blocks {
            offsets.push(cursor);
            cursor += block.len() as u32;
        }
        block_offsets.push(offsets);
    }

    // 4. Patch the real offsets/byte counts in.
    for (level, ifd) in ifds.iter_mut().enumerate() {
        for entry in ifd.iter_mut() {
            let offsets_tag = entry.tag == TiffTag::TileOffsets as u16
                || entry.tag == TiffTag::StripOffsets as u16;
            let counts_tag = entry.tag == TiffTag::TileByteCounts as u16
                || entry.tag == TiffTag::StripByteCounts as u16;
            if offsets_tag {
                entry.val = FieldVal::Long(block_offsets[level].clone());
            } else if counts_tag {
                entry.val = FieldVal::Long(blocks[level].iter().map(|b| b.len() as u32).collect());
            }
        }
    }

    // 5. Emit.
    let mut out: Vec<u8> = Vec::with_capacity(cursor as usize);
    let put16 = |out: &mut Vec<u8>, v: u16| match byte_order {
        ByteOrderType::LittleEndian => out.extend_from_slice(&v.to_le_bytes()),
        ByteOrderType::BigEndian => out.extend_from_slice(&v.to_be_bytes()),
    };
    let put32 = |out: &mut Vec<u8>, v: u32| match byte_order {
        ByteOrderType::LittleEndian => out.extend_from_slice(&v.to_le_bytes()),
        ByteOrderType::BigEndian => out.extend_from_slice(&v.to_be_bytes()),
    };

    match byte_order {
        ByteOrderType::LittleEndian => out.extend_from_slice(b"II"),
        ByteOrderType::BigEndian => out.extend_from_slice(b"MM"),
    }
    put16(&mut out, 42);
    put32(&mut out, ifd_offsets[0]);

    for (level, ifd) in ifds.iter().enumerate() {
        assert_eq!(
            out.len() as u32,
            ifd_offsets[level],
            "IFD {level} misplaced"
        );
        put16(&mut out, ifd.len() as u16);
        for entry in ifd {
            put16(&mut out, entry.tag);
            put16(&mut out, entry.val.field_type());
            put32(&mut out, entry.val.count());
            match entry.external_offset {
                Some(offset) => put32(&mut out, offset),
                None => {
                    // Inline: the value is left-justified in the 4-byte field,
                    // whatever the byte order (TIFF 6.0 p.15).
                    let mut bytes = entry.val.encode(byte_order);
                    bytes.resize(4, 0);
                    out.extend_from_slice(&bytes);
                }
            }
        }
        let next = ifd_offsets.get(level + 1).copied().unwrap_or(0);
        put32(&mut out, next);
    }

    for ifd in &ifds {
        for entry in ifd {
            if let Some(offset) = entry.external_offset {
                assert_eq!(out.len() as u32, offset, "tag {} misplaced", entry.tag);
                out.extend_from_slice(&entry.val.encode(byte_order));
                if !out.len().is_multiple_of(2) {
                    out.push(0);
                }
            }
        }
    }

    for (level, level_blocks) in blocks.iter().enumerate() {
        for (index, block) in level_blocks.iter().enumerate() {
            assert_eq!(
                out.len() as u32,
                block_offsets[level][index],
                "block {index} of level {level} misplaced"
            );
            out.extend_from_slice(block);
        }
    }
    assert_eq!(out.len() as u32, cursor, "final layout disagrees");

    out
}

/// The whole of one band of `level`, in host byte order — the ground truth every
/// read is compared against.
fn expected_band_native(spec: &Spec, level: usize, band: usize) -> Vec<u8> {
    let (w, h) = spec.level_size(level);
    let mut out = Vec::with_capacity(w as usize * h as usize * spec.elem.bytes());
    for y in 0..h as usize {
        for x in 0..w as usize {
            out.extend_from_slice(&native_bytes(
                spec.elem,
                raw_bits(spec.elem, level, band, y, x),
            ));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Shared assertions
// ---------------------------------------------------------------------------

/// Opens both twins of `spec`.
fn open_pair(spec: &Spec) -> (GeoTiffReader<MemorySource>, GeoTiffReader<MemorySource>) {
    let be = GeoTiffReader::open(MemorySource(build_tiff(spec, ByteOrderType::BigEndian)))
        .expect("open MM fixture");
    let le = GeoTiffReader::open(MemorySource(build_tiff(spec, ByteOrderType::LittleEndian)))
        .expect("open II fixture");
    assert_eq!(be.byte_order(), ByteOrderType::BigEndian);
    assert_eq!(le.byte_order(), ByteOrderType::LittleEndian);
    (be, le)
}

/// Runs every normalising whole-band entry point over both twins of `spec` and
/// checks all of them against the host-order ground truth.
fn assert_band_matches(spec: &Spec, label: &str) {
    let (be, le) = open_pair(spec);
    let levels = if spec.overview { 2 } else { 1 };

    for level in 0..levels {
        for band in 0..spec.bands as usize {
            let expected = expected_band_native(spec, level, band);

            let be_band = be.read_band(level, band).expect("MM read_band");
            let le_band = le.read_band(level, band).expect("II read_band");
            assert_eq!(
                le_band, expected,
                "{label}: II read_band(level {level}, band {band}) disagrees with ground truth"
            );
            assert_eq!(
                be_band, expected,
                "{label}: MM read_band(level {level}, band {band}) is not normalised to host \
                 byte order"
            );

            let mut into = vec![0u8; be.band_byte_len(level).expect("band_byte_len")];
            be.read_band_into(level, band, &mut into)
                .expect("MM read_band_into");
            assert_eq!(into, expected, "{label}: MM read_band_into level {level}");

            // A window that deliberately straddles block boundaries.
            let (w, h) = spec.level_size(level);
            let (bw, bh, _, _) = spec.block_grid(level);
            let wx = (bw / 2).min(w.saturating_sub(1));
            let wy = (bh / 2).min(h.saturating_sub(1));
            let ww = (w - wx).min(bw + 3).max(1);
            let wh = (h - wy).min(bh + 3).max(1);
            let be_win = be
                .read_window(
                    level,
                    band,
                    u64::from(wx),
                    u64::from(wy),
                    u64::from(ww),
                    u64::from(wh),
                )
                .expect("MM read_window");
            let le_win = le
                .read_window(
                    level,
                    band,
                    u64::from(wx),
                    u64::from(wy),
                    u64::from(ww),
                    u64::from(wh),
                )
                .expect("II read_window");
            let bps = spec.elem.bytes();
            let mut expected_win = Vec::with_capacity(ww as usize * wh as usize * bps);
            for row in 0..wh as usize {
                let src = ((wy as usize + row) * w as usize + wx as usize) * bps;
                expected_win.extend_from_slice(&expected[src..src + ww as usize * bps]);
            }
            assert_eq!(
                le_win, expected_win,
                "{label}: II read_window(level {level}, band {band}) disagrees with ground truth"
            );
            assert_eq!(
                be_win, expected_win,
                "{label}: MM read_window(level {level}, band {band}) is not normalised"
            );

            let mut win_into = vec![0u8; expected_win.len()];
            be.read_window_into(
                level,
                band,
                u64::from(wx),
                u64::from(wy),
                u64::from(ww),
                u64::from(wh),
                &mut win_into,
            )
            .expect("MM read_window_into");
            assert_eq!(
                win_into, expected_win,
                "{label}: MM read_window_into level {level} band {band}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// T1 — every element width, tiled and striped
// ---------------------------------------------------------------------------

#[test]
fn test_issue_14_byte_order_every_element_width_tiled() {
    for elem in Elem::ALL {
        assert_band_matches(&Spec::base(elem), &format!("tiled {elem:?}"));
    }
}

#[test]
fn test_issue_14_byte_order_every_element_width_striped() {
    for elem in Elem::ALL {
        assert_band_matches(&Spec::base(elem).striped(), &format!("striped {elem:?}"));
    }
}

/// `u8`/`i8` are the control: a byte-order swap that fired on single-byte
/// samples would reverse nothing but would still prove the branch is wrong, so
/// this pins that the two twins are byte-identical *and* untouched.
#[test]
fn test_issue_14_byte_order_single_byte_samples_are_untouched() {
    for elem in [Elem::U8, Elem::I8] {
        for spec in [Spec::base(elem), Spec::base(elem).striped()] {
            let (be, le) = open_pair(&spec);
            let expected = expected_band_native(&spec, 0, 0);
            assert_eq!(be.read_band(0, 0).expect("MM"), expected, "{elem:?}");
            assert_eq!(le.read_band(0, 0).expect("II"), expected, "{elem:?}");
        }
    }
}

// ---------------------------------------------------------------------------
// T2 — multi-band, chunky and planar
// ---------------------------------------------------------------------------

#[test]
fn test_issue_14_byte_order_multiband_chunky() {
    for elem in [Elem::U16, Elem::I32, Elem::F32, Elem::F64] {
        assert_band_matches(
            &Spec::base(elem).bands(3, 1),
            &format!("chunky x3 {elem:?}"),
        );
        assert_band_matches(
            &Spec::base(elem).striped().bands(3, 1),
            &format!("striped chunky x3 {elem:?}"),
        );
    }
}

#[test]
fn test_issue_14_byte_order_multiband_planar() {
    for elem in [Elem::U16, Elem::I32, Elem::F32, Elem::F64] {
        assert_band_matches(
            &Spec::base(elem).bands(3, 2),
            &format!("planar x3 {elem:?}"),
        );
        assert_band_matches(
            &Spec::base(elem).striped().bands(3, 2),
            &format!("striped planar x3 {elem:?}"),
        );
    }
}

// ---------------------------------------------------------------------------
// T3 — predictors (the ordering trap)
// ---------------------------------------------------------------------------

/// `PREDICTOR=2` on an `MM` file. Horizontal differencing reconstructs each
/// sample with carry-propagating addition performed *in the file's byte order*,
/// so normalising before the predictor runs gives a clean decode of wrong values.
#[test]
fn test_issue_14_byte_order_predictor_2() {
    for elem in [
        Elem::U16,
        Elem::I16,
        Elem::U32,
        Elem::I32,
        Elem::U64,
        Elem::I64,
    ] {
        for spec in [
            Spec::base(elem).predictor(Predictor::HorizontalDifferencing),
            Spec::base(elem)
                .striped()
                .predictor(Predictor::HorizontalDifferencing),
            Spec::base(elem)
                .bands(3, 1)
                .predictor(Predictor::HorizontalDifferencing),
            Spec::base(elem)
                .bands(3, 2)
                .predictor(Predictor::HorizontalDifferencing),
        ] {
            assert_band_matches(
                &spec,
                &format!("predictor 2 {elem:?} planar={}", spec.planar),
            );
        }
    }
}

/// `PREDICTOR=3` on an `MM` file. The floating-point predictor byte-plane
/// transposes each scanline with the most-significant plane first, so between
/// decompression and predictor reversal the block is not a sample array at all;
/// a swap placed there scrambles it irrecoverably.
#[test]
fn test_issue_14_byte_order_predictor_3() {
    for elem in [Elem::F32, Elem::F64] {
        for spec in [
            Spec::base(elem).predictor(Predictor::FloatingPoint),
            Spec::base(elem)
                .striped()
                .predictor(Predictor::FloatingPoint),
            Spec::base(elem)
                .bands(3, 1)
                .predictor(Predictor::FloatingPoint),
            Spec::base(elem)
                .bands(3, 2)
                .predictor(Predictor::FloatingPoint),
        ] {
            assert_band_matches(
                &spec,
                &format!("predictor 3 {elem:?} planar={}", spec.planar),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// T4 — compressed blocks
// ---------------------------------------------------------------------------

#[cfg(feature = "deflate")]
#[test]
fn test_issue_14_byte_order_deflate() {
    for elem in [Elem::U16, Elem::I32, Elem::F32, Elem::F64] {
        assert_band_matches(
            &Spec::base(elem).compression(Compression::Deflate),
            &format!("deflate {elem:?}"),
        );
        assert_band_matches(
            &Spec::base(elem)
                .striped()
                .compression(Compression::Deflate)
                .predictor(if elem.is_float() {
                    Predictor::FloatingPoint
                } else {
                    Predictor::HorizontalDifferencing
                }),
            &format!("deflate + predictor {elem:?}"),
        );
    }
}

#[test]
fn test_issue_14_byte_order_uncompressed_matches_deflate_shape() {
    // The uncompressed control for the DEFLATE cases above, so a failure can be
    // attributed to the codec or to the byte order but never to both.
    for elem in [Elem::U16, Elem::I32, Elem::F32, Elem::F64] {
        assert_band_matches(
            &Spec::base(elem).compression(Compression::None),
            &format!("uncompressed {elem:?}"),
        );
    }
}

// ---------------------------------------------------------------------------
// T5 — overviews
// ---------------------------------------------------------------------------

#[test]
fn test_issue_14_byte_order_overview_levels() {
    for elem in [Elem::U16, Elem::F32, Elem::F64] {
        assert_band_matches(
            &Spec::base(elem).overview(),
            &format!("overview tiled {elem:?}"),
        );
        assert_band_matches(
            &Spec::base(elem).striped().overview(),
            &format!("overview striped {elem:?}"),
        );
    }
}

// ---------------------------------------------------------------------------
// T6 — the typed path issue #14 is actually about
// ---------------------------------------------------------------------------

/// `read_band_into_typed::<f64>` over an `MM` `Float32` raster: the exact call a
/// user migrating from the C-GDAL wrapper makes, and the one that returned
/// garbage for every big-endian DEM.
#[test]
fn test_issue_14_byte_order_read_band_into_typed_f64_from_mm_f32() {
    for spec in [
        Spec::base(Elem::F32),
        Spec::base(Elem::F32).striped(),
        Spec::base(Elem::F32).predictor(Predictor::FloatingPoint),
        Spec::base(Elem::F32).bands(3, 1),
        Spec::base(Elem::F32).bands(3, 2),
    ] {
        let (be, le) = open_pair(&spec);
        for band in 0..spec.bands as usize {
            let count = be.band_pixel_count(0).expect("band_pixel_count");
            let mut be_out = vec![0.0f64; count];
            let mut le_out = vec![0.0f64; count];
            be.read_band_into_typed(0, band, &mut be_out)
                .expect("MM typed read");
            le.read_band_into_typed(0, band, &mut le_out)
                .expect("II typed read");

            let (w, h) = spec.level_size(0);
            let expected: Vec<f64> = (0..h as usize)
                .flat_map(|y| {
                    (0..w as usize).map(move |x| {
                        f32::from_bits(raw_bits(Elem::F32, 0, band, y, x) as u32) as f64
                    })
                })
                .collect();

            assert_eq!(le_out, expected, "II typed f32->f64, band {band}");
            assert_eq!(
                be_out, expected,
                "MM typed f32->f64, band {band}: samples were not normalised to host byte \
                 order before conversion"
            );
        }
    }
}

/// The typed window path, over the same `MM` fixture.
#[test]
fn test_issue_14_byte_order_read_window_into_typed_crosses_blocks() {
    let spec = Spec::base(Elem::F32);
    let (be, le) = open_pair(&spec);
    let (x, y, w, h) = (8u64, 8u64, 20u64, 20u64);
    let mut be_out = vec![0.0f64; (w * h) as usize];
    let mut le_out = vec![0.0f64; (w * h) as usize];
    be.read_window_into_typed(0, 0, x, y, w, h, &mut be_out)
        .expect("MM typed window");
    le.read_window_into_typed(0, 0, x, y, w, h, &mut le_out)
        .expect("II typed window");

    let expected: Vec<f64> = (0..h as usize)
        .flat_map(|row| {
            (0..w as usize).map(move |col| {
                f32::from_bits(raw_bits(Elem::F32, 0, 0, y as usize + row, x as usize + col) as u32)
                    as f64
            })
        })
        .collect();
    assert_eq!(le_out, expected, "II typed window");
    assert_eq!(be_out, expected, "MM typed window is not normalised");
}

// ---------------------------------------------------------------------------
// T7 — the tile-level APIs
// ---------------------------------------------------------------------------

/// `CogReader::read_tile` / `read_tile_into` and the `RasterBuffer` wrappers are
/// part of the normalising contract too; `read_tile_raw` deliberately is not.
#[test]
fn test_issue_14_byte_order_tile_apis() {
    for elem in [Elem::U16, Elem::I32, Elem::F32, Elem::F64] {
        for spec in [Spec::base(elem), Spec::base(elem).striped()] {
            let be_bytes = build_tiff(&spec, ByteOrderType::BigEndian);
            let le_bytes = build_tiff(&spec, ByteOrderType::LittleEndian);
            let be = CogReader::open(MemorySource(be_bytes.clone())).expect("open MM");
            let le = CogReader::open(MemorySource(le_bytes)).expect("open II");

            let (_, _, across, down) = spec.block_grid(0);
            for ty in 0..down {
                for tx in 0..across {
                    let be_tile = be.read_tile(0, tx, ty).expect("MM read_tile");
                    let le_tile = le.read_tile(0, tx, ty).expect("II read_tile");
                    assert_eq!(
                        be_tile, le_tile,
                        "{elem:?}: read_tile({tx},{ty}) differs between MM and II"
                    );
                    assert_eq!(
                        be_tile,
                        block_samples(&spec, 0, 0, tx, ty, host_order()),
                        "{elem:?}: MM read_tile({tx},{ty}) is not in host byte order"
                    );

                    let size = be.tile_decoded_size(0, ty).expect("tile_decoded_size");
                    let mut into = vec![0u8; size];
                    be.read_tile_into(0, tx, ty, &mut into)
                        .expect("MM read_tile_into");
                    assert_eq!(
                        into, be_tile,
                        "{elem:?}: read_tile_into disagrees with read_tile"
                    );
                }
            }

            // `read_tile_raw` must stay verbatim: the compressed block exactly as
            // stored, in the file's byte order.
            let raw = be.read_tile_raw(0, 0, 0).expect("read_tile_raw");
            let range = be.tile_byte_range(0, 0, 0).expect("tile_byte_range");
            assert_eq!(
                raw,
                be_bytes[range.start as usize..range.end as usize],
                "{elem:?}: read_tile_raw must not normalise anything"
            );
        }
    }
}

/// The `RasterBuffer` wrappers, which are what most downstream crates call.
#[test]
fn test_issue_14_byte_order_tile_buffer_apis() {
    for elem in [Elem::U16, Elem::F32, Elem::F64] {
        for spec in [
            Spec::base(elem),
            Spec::base(elem).striped(),
            Spec::base(elem).bands(3, 1),
            Spec::base(elem).bands(3, 2),
        ] {
            let (be, le) = open_pair(&spec);
            let (_, _, across, down) = spec.block_grid(0);
            for band in 0..spec.bands as usize {
                for ty in 0..down {
                    for tx in 0..across {
                        let be_buf = be
                            .read_tile_band_buffer(0, band, tx, ty)
                            .expect("MM read_tile_band_buffer");
                        let le_buf = le
                            .read_tile_band_buffer(0, band, tx, ty)
                            .expect("II read_tile_band_buffer");
                        assert_eq!(
                            be_buf.as_bytes(),
                            le_buf.as_bytes(),
                            "{elem:?} band {band}: tile buffer ({tx},{ty}) differs between MM \
                             and II"
                        );
                    }
                }
            }
            let be_zero = be.read_tile_buffer(0, 0, 0).expect("MM read_tile_buffer");
            let le_zero = le.read_tile_buffer(0, 0, 0).expect("II read_tile_buffer");
            assert_eq!(be_zero.as_bytes(), le_zero.as_bytes());
        }
    }
}

/// The byte order the host stores samples in — the order every read must produce.
const fn host_order() -> ByteOrderType {
    if cfg!(target_endian = "big") {
        ByteOrderType::BigEndian
    } else {
        ByteOrderType::LittleEndian
    }
}

// ---------------------------------------------------------------------------
// T8 — the accessor, and the fixtures themselves
// ---------------------------------------------------------------------------

/// The new accessor must report the file's own header, not the host's, so that a
/// caller can still reason about the file even though it no longer has to.
#[test]
fn test_issue_14_byte_order_accessor_reports_the_file() {
    let spec = Spec::base(Elem::F32);
    let (be, le) = open_pair(&spec);
    assert_eq!(be.byte_order(), ByteOrderType::BigEndian);
    assert_eq!(le.byte_order(), ByteOrderType::LittleEndian);
    // And it agrees with the low-level parse it saves callers from doing.
    let raw = build_tiff(&spec, ByteOrderType::BigEndian);
    assert_eq!(&raw[0..2], b"MM");
}

/// Guards the test harness itself: the two fixtures must genuinely differ on
/// disk, otherwise every assertion above would pass vacuously.
#[test]
fn test_issue_14_byte_order_fixtures_really_differ() {
    for elem in [Elem::U16, Elem::F32, Elem::F64] {
        let spec = Spec::base(elem);
        let be = build_tiff(&spec, ByteOrderType::BigEndian);
        let le = build_tiff(&spec, ByteOrderType::LittleEndian);
        assert_eq!(
            be.len(),
            le.len(),
            "{elem:?}: twins must be the same length"
        );
        assert_ne!(be, le, "{elem:?}: twins must differ on disk");
        assert_eq!(&be[0..2], b"MM");
        assert_eq!(&le[0..2], b"II");
    }
    // ... and for a single-byte type only the header/IFD scalars differ, never
    // the pixel payload.
    let spec = Spec::base(Elem::U8);
    let be = build_tiff(&spec, ByteOrderType::BigEndian);
    let le = build_tiff(&spec, ByteOrderType::LittleEndian);
    let reader = CogReader::open(MemorySource(be.clone())).expect("open MM");
    let range = reader.tile_byte_range(0, 0, 0).expect("range");
    assert_eq!(
        be[range.start as usize..range.end as usize],
        le[range.start as usize..range.end as usize],
        "single-byte samples must be stored identically in both twins"
    );
}
