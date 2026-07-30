//! `Compression::Lerc` combined with a TIFF `Predictor` — an undefined
//! combination that must fail loudly rather than decode to wrong pixels
//! (cool-japan/oxigeo#14).
//!
//! # Why the combination is undefined
//!
//! * libtiff's LERC codec (`tif_lerc.c`) never calls `TIFFPredictorInit`, so it
//!   neither applies tag 317 when writing nor reverses it when reading: to
//!   libtiff the tag simply does not exist for LERC.
//! * GDAL gates the `PREDICTOR` creation option on `GTIFFSupportsPredictor()`,
//!   which covers LZW/DEFLATE/ZSTD and excludes LERC, so `-co COMPRESS=LERC -co
//!   PREDICTOR=2` never writes a predictor tag.
//! * This crate cannot write LERC at all (`compression::compress` returns a
//!   typed "LERC encoding … not implemented" error), and neither
//!   `analyze_for_cog` nor `CogConverter` can select it.
//!
//! So no encoder in the ecosystem produces a predicted LERC block, and a file
//! carrying both tags gives a decoder no way to know whether a predictor was
//! ever applied.
//!
//! # What the driver used to do
//!
//! It reversed the predictor anyway, and did so twice wrong:
//!
//! * *at all* — the LERC payload was never differenced, so running the inverse
//!   difference over it corrupts every sample after the first **in either byte
//!   order**. On this `II` fixture the four `7.0f32` pixels came back with every
//!   second one replaced by `0x40E00000 + 0x40E00000 = 0x81C00000`, i.e. about
//!   `-7e-38` — a clean `Ok` holding garbage, no error, no warning; and
//! * *in the file's declared byte order*, while `lerc_codec` hands samples back
//!   in the **host's** order — a second, independent error that only shows up on
//!   an `MM` file.
//!
//! These tests pin the explicit rejection that replaced it, and pin that plain
//! (unpredicted) LERC still decodes.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use oxigeo_core::error::Result;
use oxigeo_core::io::{ByteRange, DataSource};
use oxigeo_geotiff::tiff::{ByteOrderType, Compression, Predictor, TiffTag};
use oxigeo_geotiff::{CogReader, GeoTiffReader};

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

/// The constant value every pixel of the fixture's LERC block holds.
const LERC_VALUE: f32 = 7.0;

/// A minimal checksum-free LERC2 v2 constant-image blob: 2x2 `Float`, all
/// pixels valid, `zMin == zMax == 7.0`.
///
/// The LERC wire format is little-endian by definition, independently of the
/// enclosing TIFF's `II`/`MM` header — which is precisely why reversing a TIFF
/// predictor over LERC output in the *file's* byte order is meaningless.
fn lerc_constant_blob() -> Vec<u8> {
    let mut blob = Vec::new();
    blob.extend_from_slice(b"Lerc2 ");
    blob.extend_from_slice(&2i32.to_le_bytes()); // version 2 (no checksum)
    blob.extend_from_slice(&2i32.to_le_bytes()); // nRows
    blob.extend_from_slice(&2i32.to_le_bytes()); // nCols
    blob.extend_from_slice(&4i32.to_le_bytes()); // numValidPixel
    blob.extend_from_slice(&8i32.to_le_bytes()); // microBlockSize
    let blobsize_pos = blob.len();
    blob.extend_from_slice(&0i32.to_le_bytes()); // blobSize placeholder
    blob.extend_from_slice(&6i32.to_le_bytes()); // dt = Float
    blob.extend_from_slice(&0.0f64.to_le_bytes()); // maxZError
    blob.extend_from_slice(&f64::from(LERC_VALUE).to_le_bytes()); // zMin
    blob.extend_from_slice(&f64::from(LERC_VALUE).to_le_bytes()); // zMax == zMin
    blob.extend_from_slice(&0i32.to_le_bytes()); // numBytesMask = 0 (all valid)
    let blob_size = blob.len() as i32;
    blob[blobsize_pos..blobsize_pos + 4].copy_from_slice(&blob_size.to_le_bytes());
    blob
}

/// Builds a 2x2, single-band, `Float32`, single-tile LERC GeoTIFF declaring
/// `predictor`, in `bo`.
fn build_lerc_tiff(predictor: Predictor, bo: ByteOrderType) -> Vec<u8> {
    let block = lerc_constant_blob();

    // (tag, field type, count, inline value) — every value fits inline.
    let entries: Vec<(u16, u16, u32, u32)> = {
        let mut e: Vec<(u16, u16, u32, u32)> = vec![
            (TiffTag::ImageWidth as u16, 4, 1, 2),
            (TiffTag::ImageLength as u16, 4, 1, 2),
            (TiffTag::BitsPerSample as u16, 3, 1, 32),
            (
                TiffTag::Compression as u16,
                3,
                1,
                u32::from(Compression::Lerc as u16),
            ),
            (TiffTag::PhotometricInterpretation as u16, 3, 1, 1),
            (TiffTag::SamplesPerPixel as u16, 3, 1, 1),
            (TiffTag::PlanarConfiguration as u16, 3, 1, 1),
            (TiffTag::Predictor as u16, 3, 1, u32::from(predictor as u16)),
            (TiffTag::SampleFormat as u16, 3, 1, 3), // IEEE float
            (TiffTag::TileWidth as u16, 4, 1, 2),
            (TiffTag::TileLength as u16, 4, 1, 2),
            (TiffTag::TileOffsets as u16, 4, 1, 0), // patched below
            (TiffTag::TileByteCounts as u16, 4, 1, block.len() as u32),
        ];
        e.sort_by_key(|(tag, ..)| *tag);
        e
    };

    let ifd_offset: u32 = 8;
    let data_offset = ifd_offset + 2 + 12 * entries.len() as u32 + 4;

    let mut out: Vec<u8> = Vec::new();
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

    put16(&mut out, entries.len() as u16);
    for (tag, field_type, count, value) in &entries {
        put16(&mut out, *tag);
        put16(&mut out, *field_type);
        put32(&mut out, *count);
        let value = if *tag == TiffTag::TileOffsets as u16 {
            data_offset
        } else {
            *value
        };
        // A SHORT value is left-justified in the 4-byte field (TIFF 6.0 p.15).
        if *field_type == 3 {
            put16(&mut out, value as u16);
            put16(&mut out, 0);
        } else {
            put32(&mut out, value);
        }
    }
    put32(&mut out, 0); // no next IFD

    assert_eq!(out.len() as u32, data_offset, "tile data misplaced");
    out.extend_from_slice(&block);
    out
}

/// Plain LERC (no predictor) must keep decoding — in **both** byte orders, since
/// the LERC payload is little-endian whatever the TIFF header says.
#[test]
fn test_issue_14_lerc_without_predictor_still_decodes() {
    for bo in [ByteOrderType::LittleEndian, ByteOrderType::BigEndian] {
        let bytes = build_lerc_tiff(Predictor::None, bo);
        let cog = CogReader::open(MemorySource(bytes.clone())).expect("open CogReader");
        let tile = cog.read_tile(0, 0, 0).expect("plain LERC must decode");
        assert_eq!(tile.len(), 16, "[{bo:?}] 2x2 Float32 = 16 bytes");
        let decoded: Vec<f32> = tile
            .chunks_exact(4)
            .map(|c| f32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        assert_eq!(
            decoded,
            vec![LERC_VALUE; 4],
            "[{bo:?}] LERC samples must arrive in host byte order"
        );

        let reader = GeoTiffReader::open(MemorySource(bytes)).expect("open GeoTiffReader");
        let band = reader.read_band(0, 0).expect("plain LERC band read");
        let decoded: Vec<f32> = band
            .chunks_exact(4)
            .map(|c| f32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        assert_eq!(decoded, vec![LERC_VALUE; 4], "[{bo:?}] read_band");
    }
}

/// LERC + a predictor must be refused, not silently mis-decoded, through every
/// entry point that reverses a predictor.
#[test]
fn test_issue_14_lerc_with_predictor_is_rejected() {
    for predictor in [Predictor::HorizontalDifferencing, Predictor::FloatingPoint] {
        for bo in [ByteOrderType::LittleEndian, ByteOrderType::BigEndian] {
            let bytes = build_lerc_tiff(predictor, bo);
            let cog = CogReader::open(MemorySource(bytes.clone())).expect("open CogReader");

            let err = cog
                .read_tile(0, 0, 0)
                .expect_err("LERC + predictor must not decode silently");
            let msg = format!("{err}");
            assert!(
                msg.contains("LERC") && msg.to_lowercase().contains("predictor"),
                "[{predictor:?} {bo:?}] error must name the combination it refuses, got: {msg}"
            );

            let size = cog.tile_decoded_size(0, 0).expect("tile_decoded_size");
            let mut into = vec![0u8; size];
            cog.read_tile_into(0, 0, 0, &mut into)
                .expect_err("read_tile_into must refuse it too");

            let reader = GeoTiffReader::open(MemorySource(bytes)).expect("open GeoTiffReader");
            reader
                .read_band(0, 0)
                .expect_err("the band path must refuse it too");
        }
    }
}
