//! Revert-proof regression tests for cool-japan/oxigeo#14 — the `openBytes`
//! half of `WasmCogViewer`'s byte-order contract.
//!
//! `WasmCogViewer` serves tiles from one of two readers: `WasmCogReader` on the
//! URL path, and `oxigeo_geotiff::CogReader` on the `openBytes` path. Both now
//! return samples in the **host's** byte order, so `decode_elevation` — which
//! consumes whatever `read_tile` produced — reads host-native and takes no
//! byte-order argument.
//!
//! It used to take one, sourced from the caller's buffer magic (`II`/`MM`).
//! That was correct while the readers returned file-order bytes; once the
//! GeoTIFF driver started normalising, the flag became a *second* swap that
//! corrupted every `MM` file, and the one flag had to mean two different things
//! depending on which reader had produced the tile.
//!
//! `WasmCogViewer::open_bytes` itself calls `web_sys::console`, so it cannot run
//! outside a browser. These tests therefore drive the exact data path it
//! delegates to — `oxigeo_geotiff::CogReader::read_tile` followed by
//! `decode_elevation` — over an `MM` fixture and its byte-identical `II` twin.
//! Re-introduce any file-byte-order compensation in `decode_elevation` and the
//! `MM` case decodes to byte-swapped garbage while `II` stays correct, so both
//! the value assertion and the II/MM equality assertion fail.
//!
//! The sibling test for the URL reader is
//! `cog_reader::tests::finish_tile_decode_is_byte_order_agnostic`.

#![allow(clippy::expect_used, clippy::panic)]

use oxigeo_geotiff::CogReader;
use oxigeo_geotiff::tiff::TiffTag;

use crate::MemorySource;
use crate::decode_elevation;

const SIZE: u32 = 4;

/// Fixture sample values. None is byte-palindromic, so a missed or doubled
/// swap cannot coincidentally produce the right number: `0x7FFF` reversed is
/// `0xFF7F` (65_407), `0x0102` reversed is `0x0201` (513).
fn sample_value(index: u32) -> u16 {
    match index % 4 {
        0 => 0x7FFF,
        1 => 0x0102,
        2 => 1000,
        _ => 40_000 + index as u16,
    }
}

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
}

type Entry = (TiffTag, u16, u32, Vec<u8>);

const SHORT: u16 = 3;
const LONG: u16 = 4;

/// Serialises a single-tile, single-band, uncompressed `UInt16` GeoTIFF in the
/// requested byte order.
///
/// Every multi-byte quantity — header, IFD entries, out-of-line payloads *and*
/// the pixel samples — is written in `endian`, exactly as a real `MM` writer
/// would. The crate's own writer only ever emits `II`, hence the hand-rolled
/// fixture.
fn build_tiff(endian: Endian) -> Vec<u8> {
    let mut block = Vec::with_capacity((SIZE * SIZE) as usize * 2);
    for index in 0..SIZE * SIZE {
        block.extend_from_slice(&endian.u16(sample_value(index)));
    }

    let mut entries: Vec<Entry> = vec![
        (TiffTag::ImageWidth, LONG, 1, endian.u32(SIZE).to_vec()),
        (TiffTag::ImageLength, LONG, 1, endian.u32(SIZE).to_vec()),
        (TiffTag::BitsPerSample, SHORT, 1, endian.u16(16).to_vec()),
        (TiffTag::Compression, SHORT, 1, endian.u16(1).to_vec()),
        (
            TiffTag::PhotometricInterpretation,
            SHORT,
            1,
            endian.u16(1).to_vec(),
        ),
        (TiffTag::SamplesPerPixel, SHORT, 1, endian.u16(1).to_vec()),
        (
            TiffTag::PlanarConfiguration,
            SHORT,
            1,
            endian.u16(1).to_vec(),
        ),
        (TiffTag::TileWidth, LONG, 1, endian.u32(SIZE).to_vec()),
        (TiffTag::TileLength, LONG, 1, endian.u32(SIZE).to_vec()),
        (TiffTag::TileOffsets, LONG, 1, vec![0; 4]),
        (
            TiffTag::TileByteCounts,
            LONG,
            1,
            endian.u32(block.len() as u32).to_vec(),
        ),
        (TiffTag::SampleFormat, SHORT, 1, endian.u16(1).to_vec()),
    ];
    entries.sort_by_key(|(tag, _, _, _)| *tag as u16);

    let ifd_offset = 8u32;
    let ifd_size = 2 + entries.len() as u32 * 12 + 4;
    // Every payload here is <= 4 bytes, so all values sit inline in the IFD and
    // the pixel block starts immediately after it.
    let data_start = ifd_offset + ifd_size;

    for entry in entries.iter_mut() {
        if entry.0 == TiffTag::TileOffsets {
            entry.3 = endian.u32(data_start).to_vec();
        }
    }

    let mut out = Vec::with_capacity((data_start as usize) + block.len());
    out.extend_from_slice(endian.magic());
    out.extend_from_slice(&endian.u16(42));
    out.extend_from_slice(&endian.u32(ifd_offset));
    out.extend_from_slice(&endian.u16(entries.len() as u16));
    for (tag, field_type, count, payload) in &entries {
        out.extend_from_slice(&endian.u16(*tag as u16));
        out.extend_from_slice(&endian.u16(*field_type));
        out.extend_from_slice(&endian.u32(*count));
        // A short value sits left-justified in the 4-byte value field, in both
        // byte orders.
        let mut inline = [0u8; 4];
        inline[..payload.len()].copy_from_slice(payload);
        out.extend_from_slice(&inline);
    }
    out.extend_from_slice(&endian.u32(0)); // no next IFD
    assert_eq!(out.len() as u32, data_start);
    out.extend_from_slice(&block);
    out
}

/// Sanity: the two fixtures really are the same raster, byte-swapped.
#[test]
fn issue_14_openbytes_fixtures_differ_only_in_byte_order() {
    let le = build_tiff(Endian::Little);
    let be = build_tiff(Endian::Big);
    assert_eq!(&le[0..2], b"II");
    assert_eq!(&be[0..2], b"MM");
    assert_eq!(le.len(), be.len());
    assert_ne!(le, be, "an MM fixture identical to II proves nothing");
}

/// The `openBytes` path must produce one answer for both encodings.
#[test]
fn issue_14_openbytes_elevation_is_byte_order_agnostic() {
    let expected: Vec<f32> = (0..SIZE * SIZE)
        .map(|i| f32::from(sample_value(i)))
        .collect();

    let mut decoded = Vec::new();
    for endian in [Endian::Little, Endian::Big] {
        let reader = CogReader::open(MemorySource::new(build_tiff(endian)))
            .unwrap_or_else(|e| panic!("{endian:?}: open: {e}"));

        // This is exactly what `WasmCogViewer::read_tile` does on the
        // `mem_reader` branch.
        let raw = reader
            .read_tile(0, 0, 0)
            .unwrap_or_else(|e| panic!("{endian:?}: read_tile: {e}"));

        // ...and what `readTileElevation` does with the result.
        let values = decode_elevation(&raw, 1, 16);

        assert_eq!(
            values,
            expected,
            "{endian:?}: samples reach decode_elevation already normalised to \
             host order by the GeoTIFF driver. Any byte-order compensation \
             here — such as the `little_endian` flag this viewer used to derive \
             from the buffer's II/MM magic — swaps an MM file a second time and \
             turns 0x{:04X} into 0x{:04X}",
            sample_value(0),
            sample_value(0).swap_bytes(),
        );

        decoded.push(values);
    }

    assert_eq!(
        decoded[0], decoded[1],
        "II and MM encodings of one raster must yield one elevation profile"
    );
}
