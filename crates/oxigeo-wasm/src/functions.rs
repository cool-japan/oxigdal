//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

pub use super::fetch::FetchBackend;
use crate::buffered_source::BufferedRangeSource;
use oxigeo_core::error::OxiGeoError;
use oxigeo_core::io::ByteRange;
use wasm_bindgen::prelude::*;

use super::types_5::CogLevelGeometry;

/// Initialize the WASM module with better error handling
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

/// Converts an `OxiGeoError` to a `JsValue`
pub fn to_js_error(err: &OxiGeoError) -> JsValue {
    JsValue::from_str(&err.to_string())
}

/// Converts one decoded tile's samples into a `tile_width × tile_height` RGBA
/// buffer for the canvas.
///
/// **`tile_width`/`tile_height` must be the geometry of *this* tile**, i.e. of
/// the level it was read from — see [`WasmCogViewer::level_tile_size`]. The
/// returned buffer is always exactly `tile_width · tile_height · 4` bytes, so it
/// is the buffer `ImageData::new_with_u8_clamped_array_and_sh` expects for the
/// same pair; a tile shorter than that leaves the tail transparent black rather
/// than failing, and a longer one is truncated — the same tolerance both viewers
/// have always had for a partially-read tile.
///
/// Both viewers convert through this one function so they cannot drift apart.
/// The per-band branches keep their long-standing semantics exactly, including
/// the single-band ones consuming the first `pixel_count` **bytes** (which is a
/// pixel each only for 8-bit samples).
pub(super) fn tile_to_rgba(
    tile_data: &[u8],
    band_count: u32,
    tile_width: u32,
    tile_height: u32,
) -> Vec<u8> {
    let pixel_count = (tile_width as usize) * (tile_height as usize);
    let mut rgba = vec![0u8; pixel_count * 4];

    match band_count {
        3 => {
            // RGB
            for i in 0..pixel_count.min(tile_data.len() / 3) {
                rgba[i * 4] = tile_data[i * 3];
                rgba[i * 4 + 1] = tile_data[i * 3 + 1];
                rgba[i * 4 + 2] = tile_data[i * 3 + 2];
                rgba[i * 4 + 3] = 255;
            }
        }
        4 => {
            // RGBA
            for i in 0..pixel_count.min(tile_data.len() / 4) {
                rgba[i * 4] = tile_data[i * 4];
                rgba[i * 4 + 1] = tile_data[i * 4 + 1];
                rgba[i * 4 + 2] = tile_data[i * 4 + 2];
                rgba[i * 4 + 3] = tile_data[i * 4 + 3];
            }
        }
        // Grayscale (1 band), and any other band count: first band only.
        _ => {
            for (i, &v) in tile_data.iter().take(pixel_count).enumerate() {
                rgba[i * 4] = v;
                rgba[i * 4 + 1] = v;
                rgba[i * 4 + 2] = v;
                rgba[i * 4 + 3] = 255;
            }
        }
    }

    rgba
}

/// Decodes raw (already-decompressed) tile sample bytes into `f32` values,
/// honouring the TIFF `SampleFormat` and `BitsPerSample`.
///
/// Supported combinations: 8-bit unsigned, 16-bit unsigned/signed, 32-bit
/// unsigned/signed/float, and 64-bit float. Unknown combinations fall back to
/// treating each byte as a `u8`.
///
/// `raw` is in the **host's** byte order — both readers behind
/// [`WasmCogViewer::read_tile`] normalise before returning — so every read here
/// is `from_ne_bytes` and there is no byte-order parameter to get wrong. This
/// function used to take a `little_endian` flag sourced from the file header;
/// once the readers normalised, that flag byte-swapped `MM` data a second time
/// (cool-japan/oxigeo#14).
pub fn decode_elevation(raw: &[u8], sample_format: u16, bits_per_sample: u16) -> Vec<f32> {
    match (sample_format, bits_per_sample) {
        // 8-bit: nothing to order.
        (_, 8) => raw.iter().map(|&b| f32::from(b)).collect(),
        // 16-bit signed integer (e.g. SRTM elevation).
        (2, 16) => raw
            .chunks_exact(2)
            .map(|c| f32::from(i16::from_ne_bytes([c[0], c[1]])))
            .collect(),
        // 16-bit unsigned integer.
        (_, 16) => raw
            .chunks_exact(2)
            .map(|c| f32::from(u16::from_ne_bytes([c[0], c[1]])))
            .collect(),
        // 32-bit IEEE float.
        (3, 32) => raw
            .chunks_exact(4)
            .map(|c| f32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
        // 32-bit signed integer.
        (2, 32) => raw
            .chunks_exact(4)
            .map(|c| i32::from_ne_bytes([c[0], c[1], c[2], c[3]]) as f32)
            .collect(),
        // 32-bit unsigned integer.
        (_, 32) => raw
            .chunks_exact(4)
            .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]) as f32)
            .collect(),
        // 64-bit IEEE float.
        (3, 64) => raw
            .chunks_exact(8)
            .map(|c| f64::from_ne_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]) as f32)
            .collect(),
        // Fallback: treat bytes as u8 samples.
        _ => raw.iter().map(|&b| f32::from(b)).collect(),
    }
}

/// Version information
#[wasm_bindgen]
#[must_use]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Checks if the given URL points to a TIFF file by reading the header
///
/// # Errors
/// Returns an error if the URL cannot be fetched or the header cannot be read
#[wasm_bindgen]
pub async fn is_tiff_url(url: &str) -> std::result::Result<bool, JsValue> {
    let backend = FetchBackend::new(url.to_string())
        .await
        .map_err(|e| to_js_error(&e))?;
    let header = backend
        .read_range_async(ByteRange::from_offset_length(0, 8))
        .await
        .map_err(|e| to_js_error(&e))?;
    Ok(oxigeo_geotiff::is_tiff(&header))
}

/// Collects the geometry of every level `reader` exposes.
///
/// Deliberately re-derives each level's `ImageInfo` **through the reader's own
/// level → IFD map** ([`oxigeo_geotiff::CogReader::level_ifd`], which skips GDAL
/// internal masks and unparsable directories), so the reported pyramid and the
/// levels the tile reads accept cannot disagree. Every range it needs was
/// already read while opening the file, so it costs no extra request — but it is
/// driven through the pull loop anyway, so a cache that has since been trimmed
/// simply refills instead of failing.
pub(super) fn collect_level_geometry(
    reader: &oxigeo_geotiff::CogReader<BufferedRangeSource>,
    source: &BufferedRangeSource,
) -> std::result::Result<Vec<CogLevelGeometry>, OxiGeoError> {
    let tiff = reader.tiff();
    let byte_order = tiff.byte_order();
    let variant = tiff.header.variant;

    let mut levels = Vec::with_capacity(reader.overview_count() + 1);
    for level in 0..=reader.overview_count() {
        let ifd = reader
            .level_ifd(level)
            .ok_or_else(|| OxiGeoError::OutOfBounds {
                message: format!("Level {level} has no IFD"),
            })?;
        let info = oxigeo_geotiff::ImageInfo::from_ifd(ifd, source, byte_order, variant)?;

        // A striped level's block is the full image width by `RowsPerStrip`
        // rows; that is exactly the pair `tiles_across`/`tiles_down` divide by,
        // so the counts below stay consistent with the dimensions reported here.
        let saturating = |value: u64| u32::try_from(value).unwrap_or(u32::MAX);
        let tile_width = info.tile_width.unwrap_or_else(|| saturating(info.width));
        let tile_height = info
            .tile_height
            .or(info.rows_per_strip)
            .unwrap_or_else(|| saturating(info.height));

        levels.push(CogLevelGeometry {
            width: info.width,
            height: info.height,
            tile_width,
            tile_height,
            tiles_x: info.tiles_across(),
            tiles_y: info.tiles_down(),
        });
    }
    Ok(levels)
}

/// `decode_elevation` consumes **host-native** samples, because that is what
/// both of `WasmCogViewer`'s readers produce. Every fixture here is therefore
/// built with `to_ne_bytes`; a fixture built with `to_le_bytes` would silently
/// stop testing anything on a big-endian host.
#[cfg(test)]
pub(super) mod decode_elevation_tests {
    use super::decode_elevation;

    #[test]
    fn decodes_u8_samples() {
        let raw = [0u8, 128, 255];
        let out = decode_elevation(&raw, 1, 8);
        assert_eq!(out, vec![0.0, 128.0, 255.0]);
    }

    #[test]
    fn decodes_i16_samples() {
        let mut raw = Vec::new();
        raw.extend_from_slice(&(-100i16).to_ne_bytes());
        raw.extend_from_slice(&300i16.to_ne_bytes());
        let out = decode_elevation(&raw, 2, 16);
        assert_eq!(out, vec![-100.0, 300.0]);
    }

    /// The decoder must not reinterpret samples by file byte order any more:
    /// a buffer whose bytes are the *opposite* of host order decodes to the
    /// byte-swapped number, because normalisation already happened upstream.
    #[test]
    fn decodes_by_host_order_only() {
        let mut reversed = (-100i16).to_ne_bytes();
        reversed.reverse();
        let out = decode_elevation(&reversed, 2, 16);
        assert_eq!(
            out,
            vec![f32::from((-100i16).swap_bytes())],
            "decode_elevation must interpret bytes as host-native; if it \
             re-applied a file byte order it would decode this back to -100"
        );
    }

    #[test]
    fn decodes_u16_and_i32_and_f32() {
        let u16_raw = 40000u16.to_ne_bytes();
        assert_eq!(decode_elevation(&u16_raw, 1, 16), vec![40000.0]);

        let i32_raw = (-1_000_000i32).to_ne_bytes();
        assert_eq!(decode_elevation(&i32_raw, 2, 32), vec![-1_000_000.0]);

        let f32_raw = 1234.5f32.to_ne_bytes();
        assert_eq!(decode_elevation(&f32_raw, 3, 32), vec![1234.5]);
    }

    #[test]
    fn decodes_f64() {
        let raw = 2.5f64.to_ne_bytes();
        assert_eq!(decode_elevation(&raw, 3, 64), vec![2.5]);
    }
}

/// Tile → RGBA conversion, the geometry-sensitive half of both viewers'
/// `readTileAsImageData`.
///
/// The fetch paths themselves are not testable natively (they need a browser),
/// so the pure helper they both funnel through is pinned here — in particular
/// that its buffer is sized from the geometry it is *given*, which is now the
/// requested level's, not the file's level-0 tile size.
#[cfg(test)]
pub(super) mod tile_to_rgba_tests {
    use super::tile_to_rgba;

    /// The regression this helper exists for: an overview whose IFD declares a
    /// smaller `TileWidth`/`TileLength` than the full-resolution image. Sized
    /// from level 0 (the old behaviour) the buffer was 256·256·4 bytes for a
    /// 128·128 tile — three quarters of it transparent black, and `ImageData`
    /// built at the wrong dimensions. Sized from the level's own geometry it is
    /// exactly the tile.
    #[test]
    fn buffer_is_sized_from_the_geometry_it_is_given() {
        let tile = vec![7u8; 128 * 128];

        let rgba = tile_to_rgba(&tile, 1, 128, 128);
        assert_eq!(rgba.len(), 128 * 128 * 4);
        assert!(
            rgba.chunks_exact(4).all(|px| px == [7, 7, 7, 255]),
            "every pixel of the tile must be present"
        );

        // The same tile sized from a level-0 geometry: over-large buffer, and a
        // tail of transparent black no pixel of the tile ever reaches.
        let mis_sized = tile_to_rgba(&tile, 1, 256, 256);
        assert_eq!(mis_sized.len(), 256 * 256 * 4);
        assert_eq!(&mis_sized[128 * 128 * 4..128 * 128 * 4 + 4], &[0, 0, 0, 0]);
    }

    /// A tile *larger* than the buffer geometry is truncated rather than
    /// panicking — the other direction of the same mismatch (a level whose
    /// tiles are bigger than level 0's).
    #[test]
    fn oversized_tile_is_truncated_not_panicking() {
        let tile = vec![9u8; 64 * 64];
        let rgba = tile_to_rgba(&tile, 1, 32, 32);
        assert_eq!(rgba.len(), 32 * 32 * 4);
        assert!(rgba.chunks_exact(4).all(|px| px == [9, 9, 9, 255]));
    }

    /// Grayscale: one byte per pixel, opaque.
    #[test]
    fn grayscale_replicates_the_sample_across_rgb() {
        let rgba = tile_to_rgba(&[10, 20, 30, 40], 1, 2, 2);
        assert_eq!(
            rgba,
            vec![
                10, 10, 10, 255, //
                20, 20, 20, 255, //
                30, 30, 30, 255, //
                40, 40, 40, 255,
            ]
        );
    }

    /// RGB: three interleaved bytes per pixel, alpha forced opaque.
    #[test]
    fn rgb_interleaves_three_bands_and_forces_alpha() {
        let rgba = tile_to_rgba(&[1, 2, 3, 4, 5, 6], 3, 2, 1);
        assert_eq!(rgba, vec![1, 2, 3, 255, 4, 5, 6, 255]);
    }

    /// RGBA: four interleaved bytes per pixel, alpha preserved.
    #[test]
    fn rgba_preserves_the_alpha_band() {
        let rgba = tile_to_rgba(&[1, 2, 3, 128, 4, 5, 6, 0], 4, 2, 1);
        assert_eq!(rgba, vec![1, 2, 3, 128, 4, 5, 6, 0]);
    }

    /// Any other band count falls back to the first band as grayscale — the
    /// long-standing behaviour, kept byte for byte.
    #[test]
    fn unusual_band_counts_fall_back_to_the_first_band() {
        let rgba = tile_to_rgba(&[5, 6, 7, 8], 7, 2, 2);
        assert_eq!(
            rgba,
            vec![5, 5, 5, 255, 6, 6, 6, 255, 7, 7, 7, 255, 8, 8, 8, 255]
        );
    }

    /// A short tile (a partially-read block) fills what it can and leaves the
    /// rest transparent, rather than erroring.
    #[test]
    fn short_tile_leaves_the_tail_transparent() {
        let rgba = tile_to_rgba(&[1, 2], 1, 2, 2);
        assert_eq!(rgba.len(), 16);
        assert_eq!(&rgba[..8], &[1, 1, 1, 255, 2, 2, 2, 255]);
        assert_eq!(&rgba[8..], &[0, 0, 0, 0, 0, 0, 0, 0]);
    }

    /// A short strip — the geometry `CogReader::tile_pixel_size` reports for the
    /// last strip of a striped level — is a complete image at its own size, not
    /// a padded full-height one.
    #[test]
    fn short_final_strip_is_a_complete_image_at_its_own_height() {
        let strip = vec![3u8; 16 * 4];
        let rgba = tile_to_rgba(&strip, 1, 16, 4);
        assert_eq!(rgba.len(), 16 * 4 * 4);
        assert!(rgba.chunks_exact(4).all(|px| px == [3, 3, 3, 255]));
    }
}

/// The metadata JSON's pyramid block: it must describe the levels the file
/// actually has, not a pyramid synthesised from the image's dimensions.
#[cfg(test)]
pub(super) mod pyramid_metadata_tests {
    use super::super::tile::TilePyramid;
    use super::super::types_4::AdvancedCogViewer;
    use super::*;
    use crate::buffered_source::fixture::{FixtureLayout, TestFetcher, build_fixture, open_reader};

    /// Builds a viewer in the state `open()` leaves it in for a file with the
    /// given levels, without needing a browser.
    fn viewer_with_levels(levels: Vec<CogLevelGeometry>) -> AdvancedCogViewer {
        let mut viewer = AdvancedCogViewer::new();
        let primary = levels.first().copied();
        viewer.url = Some("https://example.invalid/cog.tif".to_string());
        viewer.width = primary.map_or(0, |level| level.width);
        viewer.height = primary.map_or(0, |level| level.height);
        viewer.tile_width = primary.map_or(256, |level| level.tile_width);
        viewer.tile_height = primary.map_or(256, |level| level.tile_height);
        viewer.band_count = 1;
        viewer.overview_count = levels.len().saturating_sub(1);
        viewer.levels = levels;
        viewer
    }

    fn metadata_of(viewer: &AdvancedCogViewer) -> serde_json::Value {
        serde_json::from_str(&viewer.get_metadata()).expect("metadata is JSON")
    }

    /// The geometry the JSON is built from comes from the reader's own level →
    /// IFD map, level by level.
    #[test]
    fn level_geometry_comes_from_the_parsed_ifd_chain() {
        let data = build_fixture(FixtureLayout::default());
        let total = data.len() as u64;
        let fetcher = TestFetcher::new(data);
        let (source, reader) = open_reader(&fetcher, Some(total));
        let reader = reader.expect("the fixture opens");

        let levels = collect_level_geometry(&reader, &source).expect("level geometry");
        assert_eq!(
            levels.len(),
            reader.overview_count() + 1,
            "one entry per level the reader exposes"
        );

        assert_eq!((levels[0].width, levels[0].height), (32, 32));
        assert_eq!((levels[0].tile_width, levels[0].tile_height), (16, 16));
        assert_eq!((levels[0].tiles_x, levels[0].tiles_y), (2, 2));

        assert_eq!((levels[1].width, levels[1].height), (16, 16));
        assert_eq!((levels[1].tile_width, levels[1].tile_height), (16, 16));
        assert_eq!((levels[1].tiles_x, levels[1].tiles_y), (1, 1));

        // Every tile the pyramid block advertises is one the reader accepts.
        for (level, geometry) in levels.iter().enumerate() {
            for tile_y in 0..geometry.tiles_y {
                for tile_x in 0..geometry.tiles_x {
                    assert!(
                        reader.tile_byte_range(level, tile_x, tile_y).is_ok(),
                        "level {level} tile ({tile_x}, {tile_y}) was advertised but is not readable"
                    );
                }
            }
        }

        let metadata = metadata_of(&viewer_with_levels(levels));
        assert_eq!(metadata["pyramid"]["numLevels"], 2);
        assert_eq!(metadata["overviewCount"], 1);
        assert_eq!(metadata["pyramid"]["totalTiles"], 5);
        assert_eq!(
            metadata["pyramid"]["tilesPerLevel"],
            serde_json::json!([[2, 2], [1, 1]])
        );
    }

    /// The regression: a COG with no overviews at all must not report a
    /// pyramid, and `numLevels` must never contradict `overviewCount`.
    #[test]
    fn a_file_without_overviews_reports_exactly_one_level() {
        let viewer = viewer_with_levels(vec![CogLevelGeometry {
            width: 4096,
            height: 4096,
            tile_width: 256,
            tile_height: 256,
            tiles_x: 16,
            tiles_y: 16,
        }]);
        let metadata = metadata_of(&viewer);

        assert_eq!(metadata["overviewCount"], 0);
        assert_eq!(metadata["pyramid"]["numLevels"], 1);
        assert_eq!(
            metadata["pyramid"]["tilesPerLevel"],
            serde_json::json!([[16, 16]])
        );
        assert_eq!(metadata["pyramid"]["totalTiles"], 256);

        // What the dimension-only synthesis claimed for the same file, and why
        // it could not be right: five levels for a file with one.
        let synthesised = TilePyramid::new(4096, 4096, 256, 256);
        assert_eq!(synthesised.num_levels, 5);
        assert_eq!(
            u64::from(synthesised.num_levels),
            metadata["overviewCount"].as_u64().unwrap_or_default() + 5,
            "the synthesised pyramid contradicted overviewCount by four levels"
        );
    }

    /// A striped level's "tile" is a strip, and the reported grid divides by
    /// exactly the geometry reported next to it.
    #[test]
    fn reported_tile_counts_divide_the_reported_dimensions() {
        let viewer = viewer_with_levels(vec![
            CogLevelGeometry {
                width: 1000,
                height: 900,
                tile_width: 1000,
                tile_height: 64,
                tiles_x: 1,
                tiles_y: 15,
            },
            CogLevelGeometry {
                width: 500,
                height: 450,
                tile_width: 500,
                tile_height: 64,
                tiles_x: 1,
                tiles_y: 8,
            },
        ]);
        let metadata = metadata_of(&viewer);
        assert_eq!(metadata["pyramid"]["totalTiles"], 23);

        let levels = metadata["pyramid"]["levels"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert_eq!(levels.len(), 2);
        for level in &levels {
            let width = level["width"].as_u64().unwrap_or_default();
            let height = level["height"].as_u64().unwrap_or_default();
            let tile_width = level["tileWidth"].as_u64().unwrap_or_default();
            let tile_height = level["tileHeight"].as_u64().unwrap_or_default();
            assert_eq!(level["tilesX"].as_u64(), Some(width.div_ceil(tile_width)));
            assert_eq!(level["tilesY"].as_u64(), Some(height.div_ceil(tile_height)));
        }
    }

    /// Nothing opened: the block stays `null`, as it always has.
    #[test]
    fn an_unopened_viewer_reports_a_null_pyramid() {
        let metadata = metadata_of(&AdvancedCogViewer::new());
        assert!(metadata["pyramid"].is_null());
        assert!(metadata["url"].is_null());
        assert_eq!(metadata["overviewCount"], 0);
    }

    /// The keys JavaScript consumers read must not move.
    #[test]
    fn metadata_keys_are_stable() {
        let viewer = viewer_with_levels(vec![CogLevelGeometry {
            width: 512,
            height: 512,
            tile_width: 256,
            tile_height: 256,
            tiles_x: 2,
            tiles_y: 2,
        }]);
        let metadata = metadata_of(&viewer);

        for key in [
            "url",
            "width",
            "height",
            "tileWidth",
            "tileHeight",
            "bandCount",
            "overviewCount",
            "epsgCode",
            "pyramid",
        ] {
            assert!(
                metadata.get(key).is_some(),
                "top-level key `{key}` disappeared"
            );
        }
        for key in ["numLevels", "totalTiles", "tilesPerLevel", "levels"] {
            assert!(
                metadata["pyramid"].get(key).is_some(),
                "pyramid key `{key}` disappeared"
            );
        }
        // `tilesPerLevel` keeps its array-of-pairs shape.
        assert_eq!(
            metadata["pyramid"]["tilesPerLevel"],
            serde_json::json!([[2, 2]])
        );
    }
}
