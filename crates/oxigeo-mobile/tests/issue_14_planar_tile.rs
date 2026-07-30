//! Regression test for cool-japan/oxigeo#14 — the mobile tile paths on a
//! `PlanarConfiguration = 2` file.
//!
//! `oxigeo_dataset_read_tile` and `oxigeo_mobile_prefetch_tiles` both used to
//! call `GeoTiffReader::read_tile`, which returns one *raw block* addressed by
//! the flat `tile_y * tiles_across + tile_x` index, and then labelled those
//! bytes with the dataset's band count and the caller's requested `tile_size`.
//!
//! That is only true for a chunky file. In a planar file each block holds one
//! band's plane and the blocks run `SamplesPerPixel × TilesPerImage` in
//! plane-major order, so the tile handed to the RGBA converters (and cached, and
//! drawn) was band 0's plane — `1/SamplesPerPixel` of the promised bytes —
//! presented as interleaved RGB. Nothing errored; the picture was merely wrong.
//!
//! Both paths now go through `common::tile_read::read_block_interleaved`, which
//! reads each band with `GeoTiffReader::read_tile_band_buffer` and interleaves
//! the planes, and both report the block's own geometry.
//!
//! The fixtures are hand-built: the crate's writer only ever emits
//! `PlanarConfiguration = Chunky`. Each test builds the *same logical raster*
//! twice — chunky and planar — and asserts the two agree, pixel for pixel.

#![allow(unsafe_code)]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use oxigeo_geotiff::tiff::TiffTag;
use oxigeo_mobile::ffi::types::{OxiGeoBbox, OxiGeoBuffer, OxiGeoDataset, OxiGeoTileCoord};
use oxigeo_mobile::ffi::types::{OxiGeoErrorCode, OxiGeoTile};

const WIDTH: u32 = 8;
const HEIGHT: u32 = 8;
const TILE: u32 = 4;
const BANDS: u16 = 3;

/// Per-band, per-pixel sample. The band offset makes the three planes trivially
/// distinguishable and no sample is ever zero, so neither an all-zero buffer nor
/// a plane read as if it were interleaved can pass unnoticed.
fn sample_value(band: usize, x: u32, y: u32) -> u8 {
    let spatial = (y * WIDTH + x) % 61;
    (band as u32 * 64 + spatial + 1) as u8
}

// ---------------------------------------------------------------------------
// Synthetic TIFF builder (uncompressed, tiled, UInt8, little-endian)
// ---------------------------------------------------------------------------

type Entry = (TiffTag, u16, u32, Vec<u8>);

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
                let mut block = Vec::with_capacity((TILE * TILE) as usize * samples_in_block);
                for row in 0..TILE {
                    for col in 0..TILE {
                        let (x, y) = (bx * TILE + col, by * TILE + row);
                        for s in 0..samples_in_block {
                            if x >= WIDTH || y >= HEIGHT {
                                block.push(0);
                                continue;
                            }
                            let band = plane * samples_in_block + s;
                            block.push(sample_value(band, x, y));
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

    let mut entries: Vec<Entry> = vec![
        (TiffTag::ImageWidth, LONG, 1, WIDTH.to_le_bytes().to_vec()),
        (TiffTag::ImageLength, LONG, 1, HEIGHT.to_le_bytes().to_vec()),
        (
            TiffTag::BitsPerSample,
            SHORT,
            u32::from(BANDS),
            (0..BANDS).flat_map(|_| 8u16.to_le_bytes()).collect(),
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
            "oxigeo_issue14_mobile_{}_{seq}_{name}",
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
    let path = TempPath::new(&format!("{tag}_planar{planar}.tif"));
    std::fs::write(&path, build_tiff(planar)).expect("write fixture");
    path
}

/// Opens a fixture through the FFI and returns the handle.
fn open_dataset(path: &Path) -> *mut OxiGeoDataset {
    let c_path = CString::new(path.to_str().expect("utf-8 path")).expect("no interior NUL");
    let mut dataset: *mut OxiGeoDataset = std::ptr::null_mut();
    // SAFETY: both pointers are valid for the duration of the call.
    let rc =
        unsafe { oxigeo_mobile::ffi::raster::oxigeo_dataset_open(c_path.as_ptr(), &mut dataset) };
    assert_eq!(rc, OxiGeoErrorCode::Success, "open {}", path.display());
    assert!(!dataset.is_null());
    dataset
}

/// Reads block `(tile_x, tile_y)` through the FFI and returns
/// `(bytes, width, height, channels)`.
fn read_tile(dataset: *mut OxiGeoDataset, tile_x: i32, tile_y: i32) -> (Vec<u8>, i32, i32, i32) {
    let coord = OxiGeoTileCoord {
        z: 0,
        x: tile_x,
        y: tile_y,
    };
    let mut tile: *mut OxiGeoTile = std::ptr::null_mut();
    // SAFETY: `dataset` is a live handle, `coord` and `tile` are valid.
    let rc = unsafe {
        oxigeo_mobile::ffi::raster::oxigeo_dataset_read_tile(dataset, &coord, 256, &mut tile)
    };
    assert_eq!(rc, OxiGeoErrorCode::Success, "read_tile({tile_x},{tile_y})");
    assert!(!tile.is_null());

    let mut buffer = OxiGeoBuffer {
        data: std::ptr::null_mut(),
        length: 0,
        width: 0,
        height: 0,
        channels: 0,
    };
    // SAFETY: `tile` is a live handle from the call above.
    let rc = unsafe { oxigeo_mobile::ffi::raster::oxigeo_tile_get_data(tile, &mut buffer) };
    assert_eq!(rc, OxiGeoErrorCode::Success);
    assert!(!buffer.data.is_null());

    // SAFETY: the tile handle owns `length` readable bytes at `data`.
    let bytes = unsafe { std::slice::from_raw_parts(buffer.data, buffer.length) }.to_vec();
    // SAFETY: `tile` has not been freed yet.
    unsafe {
        oxigeo_mobile::ffi::raster::oxigeo_tile_free(tile);
    }
    (bytes, buffer.width, buffer.height, buffer.channels)
}

/// Asserts that `bytes` is block `(bx, by)` in chunky order.
fn assert_block_matches(bytes: &[u8], bx: u32, by: u32, label: &str) {
    let spp = BANDS as usize;
    assert_eq!(
        bytes.len(),
        (TILE * TILE) as usize * spp,
        "{label}: a {TILE}x{TILE} {BANDS}-band block is {} bytes; a planar-blind \
         read returns only band 0's plane",
        (TILE * TILE) as usize * spp
    );
    for row in 0..TILE {
        for col in 0..TILE {
            let (x, y) = (bx * TILE + col, by * TILE + row);
            for band in 0..spp {
                let index = ((row * TILE + col) as usize) * spp + band;
                assert_eq!(
                    bytes[index],
                    sample_value(band, x, y),
                    "{label}: pixel ({x},{y}) band {band} at byte {index}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// oxigeo_dataset_read_tile
// ---------------------------------------------------------------------------

#[test]
fn test_issue_14_read_tile_is_planar_aware() {
    let mut per_config = Vec::new();
    for planar in [1u16, 2] {
        let path = fixture("read_tile", planar);
        let dataset = open_dataset(&path);

        let mut blocks = Vec::new();
        for by in 0..HEIGHT.div_ceil(TILE) {
            for bx in 0..WIDTH.div_ceil(TILE) {
                let (bytes, width, height, channels) = read_tile(dataset, bx as i32, by as i32);
                assert_eq!(
                    (width, height, channels),
                    (TILE as i32, TILE as i32, BANDS as i32),
                    "planar={planar}: a tile must report the block's own geometry"
                );
                assert_block_matches(&bytes, bx, by, &format!("planar={planar} block({bx},{by})"));
                blocks.push(bytes);
            }
        }
        per_config.push(blocks);

        // SAFETY: `dataset` is a live handle not used after this point.
        unsafe {
            oxigeo_mobile::ffi::raster::oxigeo_dataset_close(dataset);
        }
    }

    assert_eq!(
        per_config[0], per_config[1],
        "chunky and planar encodings of one raster must read back identically"
    );
}

// ---------------------------------------------------------------------------
// oxigeo_mobile_prefetch_tiles
// ---------------------------------------------------------------------------

#[test]
fn test_issue_14_prefetch_caches_planar_aware_tiles() {
    // The whole world at zoom 0 is exactly tile (0, 0), which the prefetch path
    // maps onto block (0, 0) of the dataset.
    let bbox = OxiGeoBbox {
        min_x: -179.0,
        min_y: -85.0,
        max_x: 179.0,
        max_y: 85.0,
    };

    let mut per_config = Vec::new();
    for planar in [1u16, 2] {
        // SAFETY: no arguments; clears the process-global tile cache so the two
        // passes cannot see each other's entry for key "tile_0_0_0".
        unsafe {
            oxigeo_mobile::common::cache::oxigeo_cache_clear();
        }

        let path = fixture("prefetch", planar);
        let dataset = open_dataset(&path);

        // SAFETY: `dataset` is a live handle and `bbox` is a valid pointer.
        let count =
            unsafe { oxigeo_mobile::common::oxigeo_mobile_prefetch_tiles(dataset, &bbox, 0, 0) };
        assert_eq!(count, 1, "planar={planar}: tile (0,0,0) must be prefetched");

        let (bytes, width, height, channels) =
            oxigeo_mobile::common::cache::get_cached_tile("tile_0_0_0")
                .unwrap_or_else(|| panic!("planar={planar}: tile (0,0,0) was not cached"));

        assert_eq!(
            (width, height, channels),
            (TILE as i32, TILE as i32, BANDS as i32),
            "planar={planar}: the cache entry must carry the block's own geometry, \
             not the assumed 256x256xband_count"
        );
        assert_block_matches(&bytes, 0, 0, &format!("planar={planar} cached tile"));
        per_config.push(bytes);

        // SAFETY: `dataset` is a live handle not used after this point.
        unsafe {
            oxigeo_mobile::ffi::raster::oxigeo_dataset_close(dataset);
        }
    }

    assert_eq!(
        per_config[0], per_config[1],
        "chunky and planar encodings of one raster must cache identically"
    );
}
