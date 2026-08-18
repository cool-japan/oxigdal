//! OxiGeo GeoTIFF Driver - Pure Rust GeoTIFF/COG Support
//!
//! This crate provides a pure Rust implementation of GeoTIFF and Cloud Optimized
//! GeoTIFF (COG) reading and writing capabilities.
//!
//! # Features
//!
//! - `std` (default) - Enable standard library support
//! - `async` - Enable async I/O support
//! - `deflate` (default) - DEFLATE/zlib compression
//! - `lzw` (default) - LZW compression
//! - `zstd` - ZSTD compression
//! - `jpeg` - JPEG compression (planned)
//! - `webp` - WebP compression (pure Rust via `image-webp`; encoder is lossless VP8L only)
//! - `parallel` - Decode a band/window's tiles across rayon workers (off by
//!   default: this crate targets `wasm32-unknown-unknown`, which has no threads)
//!
//! # Example
//!
//! ```ignore
//! use oxigeo_geotiff::cog::CogReader;
//! use oxigeo_core::io::FileDataSource;
//!
//! let source = FileDataSource::open("image.tif")?;
//! let reader = CogReader::open(source)?;
//!
//! println!("Image size: {}x{}", reader.width(), reader.height());
//! println!("Tile size: {:?}", reader.tile_size());
//! println!("Overview count: {}", reader.overview_count());
//!
//! // Read a tile
//! let tile_data = reader.read_tile(0, 0, 0)?;
//! ```
//!
//! # Byte order of decoded samples
//!
//! **Every entry point that yields decoded pixel samples returns them in the
//! *host's* byte order**, whatever the file's `II`/`MM` header says. This is the
//! contract GDAL's `RasterIO` has always had, and it is what the rest of the
//! workspace assumes: [`oxigeo_core::buffer::RasterBuffer`] carries no byte-order
//! field and all of its accessors (`get_pixel`, `get_u16`…`get_f64`, `as_slice`)
//! and [`oxigeo_core::buffer::convert_raw_into`] are native-endian. Before
//! cool-japan/oxigeo#14 the driver handed back samples in the *file's* order, so
//! every numeric value read from a big-endian (`MM`) GeoTIFF anywhere in the
//! workspace was silently byte-reversed.
//!
//! ## Which APIs normalise
//!
//! Normalising (samples come back native-endian):
//!
//! * [`GeoTiffReader::read_band`], [`GeoTiffReader::read_band_into`],
//!   [`GeoTiffReader::read_band_into_typed`]
//! * [`GeoTiffReader::read_window`], [`GeoTiffReader::read_window_into`],
//!   [`GeoTiffReader::read_window_into_typed`]
//! * [`GeoTiffReader::read_tile`], [`GeoTiffReader::read_tile_buffer`],
//!   [`GeoTiffReader::read_tile_band_buffer`]
//! * [`CogReader::read_tile`], [`CogReader::read_tile_into`]
//!
//! **Not** normalising, by design:
//!
//! * [`CogReader::read_tile_raw`] — returns the block's *compressed* bytes
//!   exactly as stored; there are no samples to normalise.
//! * [`compression`] and [`compression::apply_predictor_reverse`] — the predictor
//!   is defined on file-order samples (TIFF 6.0 §14 / TN3) and therefore runs
//!   *before* normalisation. Callers driving the codec layer by hand own the
//!   conversion.
//! * The [`writer`] module, which takes native-order samples and emits them in
//!   the byte order its options declare.
//!
//! ## Scope and limits
//!
//! Normalisation swaps samples of 2, 4 and 8 bytes. Sub-byte `BitsPerSample`
//! (which the reader reports as `bytes_per_sample == 0`), 1-byte samples and any
//! other width (e.g. a 24-bit `BitsPerSample`, which rounds to 3) are passed
//! through untouched — for widths under two bytes there is nothing to swap, and
//! for exotic widths there is no defined sample order to swap *to*. `Lerc`
//! blocks are also passed through: this crate's LERC decoder emits native-order
//! samples already (`lerc_codec::serialize_native`), so swapping them would
//! corrupt data rather than fix it.
//!
//! ## Codec/predictor combinations that are refused
//!
//! A `Lerc` block carrying a `Predictor` tag is rejected outright rather than
//! decoded (see `reject_undefined_predictor`): no encoder defines that
//! combination — libtiff's LERC codec ignores tag 317 entirely and GDAL refuses
//! `PREDICTOR` for LERC — so a decoder cannot know whether a predictor was ever
//! applied. Reversing one anyway, which is what the driver used to do, corrupts
//! every sample of the block with no error raised.
//!
//! A file whose byte order already matches the host — every real-world
//! little-endian GeoTIFF on every target this crate supports — pays a single
//! predictable branch per decoded block and no per-sample work at all.

#![warn(clippy::all)]
// Pedantic disabled to reduce noise - default clippy::all is sufficient
// #![warn(clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]
// Allow dead code for internal writer components
#![allow(dead_code)]
// Allow expect() for internal invariant checks
#![allow(clippy::expect_used)]
// Allow too many arguments for complex geospatial operations
#![allow(clippy::too_many_arguments)]
// Allow clamp patterns for raster data normalization
#![allow(clippy::manual_clamp)]
// Allow push after creation for buffer building patterns
#![allow(clippy::vec_init_then_push)]
// Allow partial documentation during development
#![allow(missing_docs)]

pub mod adaptive_tiling;
pub mod band_algebra;
pub mod band_read;
pub mod cog;
pub mod color_space;
pub mod compression;
pub mod geokeys;
pub mod jpeg_codec;
pub mod lerc_codec;
pub mod overviews;
pub mod tiff;
pub mod writer;

// Re-export commonly used types
pub use cog::CogReader;
pub use geokeys::{GeoKey, GeoKeyDirectory, ModelType, RasterType};
pub use tiff::{
    ByteOrderType, Compression, ImageInfo, PhotometricInterpretation, TiffFile, TiffHeader, TiffTag,
};
pub use writer::{
    CogWriter, CogWriterOptions, GeoTiffWriter, GeoTiffWriterOptions, OverviewResampling,
    WriterConfig,
};

use crate::tiff::Predictor;
use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::error::{FormatError, OxiGeoError, Result};
use oxigeo_core::io::DataSource;
use oxigeo_core::types::{
    ColorInterpretation, GeoTransform, NoDataValue, RasterDataType, RasterMetadata,
};

/// Upper sanity bound (in bytes) on a single decoded raster band buffer.
///
/// Band dimensions come from untrusted IFD tags; this cap keeps a malformed or
/// hostile header from driving [`GeoTiff::read_band`] into a multi-gigabyte
/// allocation (OOM / denial of service). 4 GiB comfortably accommodates any
/// realistic single-band read while bounding a hostile request to a survivable
/// size.
///
/// 4 GiB does not fit in a 32-bit `usize` (`wasm32`, 32-bit ARM/x86), so on
/// those targets the cap becomes a quarter of the address space (1 GiB) — still
/// far past any realistic band, and the largest bound that could plausibly be
/// allocated there anyway.
const MAX_BAND_BYTES: usize = {
    const FOUR_GIB: u64 = 4 * 1024 * 1024 * 1024;
    if FOUR_GIB <= usize::MAX as u64 {
        FOUR_GIB as usize
    } else {
        usize::MAX / 4
    }
};

/// Computes the size of a full raster band buffer from untrusted dimensions,
/// rejecting values that overflow `usize` or exceed [`MAX_BAND_BYTES`].
///
/// Extracted so the memory-safety guard on [`GeoTiff::read_band`] can be unit
/// tested without a crafted TIFF: a hostile `width`/`height`/sample count must
/// yield a typed error rather than a wrapping (then out-of-bounds) or
/// multi-gigabyte allocation.
fn checked_band_bytes(
    width: usize,
    height: usize,
    bytes_per_sample: usize,
    samples_per_pixel: usize,
) -> Result<usize> {
    let band_bytes = width
        .checked_mul(height)
        .and_then(|v| v.checked_mul(bytes_per_sample))
        .and_then(|v| v.checked_mul(samples_per_pixel))
        .ok_or_else(|| {
            OxiGeoError::Format(FormatError::InvalidHeader {
                message: format!(
                    "raster dimensions overflow usize: {width}x{height} x {bytes_per_sample} \
                     bytes x {samples_per_pixel} samples"
                ),
            })
        })?;
    if band_bytes > MAX_BAND_BYTES {
        return Err(OxiGeoError::Format(FormatError::InvalidHeader {
            message: format!(
                "raster band size ({band_bytes} bytes) exceeds the maximum supported \
                 ({MAX_BAND_BYTES} bytes); refusing to allocate (possible malformed or \
                 hostile header)"
            ),
        }));
    }
    Ok(band_bytes)
}

// ---------------------------------------------------------------------------
// Byte-order normalisation (cool-japan/oxigeo#14)
// ---------------------------------------------------------------------------

/// Whether the samples a codec has just produced still need swapping to reach
/// the host's byte order.
///
/// See the crate-level *Byte order of decoded samples* section for the contract
/// this enforces. Three things make the answer `false`:
///
/// * the sample width is not 2, 4 or 8 bytes — sub-byte `BitsPerSample` (which
///   the reader reports as `0`), single-byte samples, and exotic widths such as
///   a 24-bit `BitsPerSample` have no swap to perform or no defined one;
/// * the codec is `Lerc`, whose decoder in this crate already emits native-order
///   samples (`lerc_codec::serialize_native`), so a swap would *introduce* the
///   corruption it is meant to remove;
/// * the file's byte order already matches the host's — the case for every
///   real-world little-endian GeoTIFF on every target this crate supports, which
///   is why this is one branch per block and no per-sample work.
///
/// The host side is `cfg!(target_endian = ..)`, so a big-endian host reading an
/// `II` file is handled by the same rule, symmetrically.
const fn decoded_needs_native_swap(
    byte_order: ByteOrderType,
    bytes_per_sample: usize,
    compression: Compression,
) -> bool {
    if !matches!(bytes_per_sample, 2 | 4 | 8) {
        return false;
    }
    if matches!(compression, Compression::Lerc) {
        return false;
    }
    match byte_order {
        ByteOrderType::LittleEndian => cfg!(target_endian = "big"),
        ByteOrderType::BigEndian => cfg!(target_endian = "little"),
    }
}

/// Rejects a `Compression`/`Predictor` pair whose decode is not defined, so a
/// file carrying one fails loudly instead of decoding to wrong pixels.
///
/// Exactly one pair qualifies today: **`Lerc` with any predictor**.
///
/// * libtiff's LERC codec (`tif_lerc.c`) never calls `TIFFPredictorInit`, so it
///   neither applies tag 317 when writing nor reverses it when reading — to
///   libtiff the predictor does not exist for LERC.
/// * GDAL gates its `PREDICTOR` creation option on `GTIFFSupportsPredictor()`,
///   which covers LZW/DEFLATE/ZSTD and excludes LERC, so `COMPRESS=LERC` never
///   writes a predictor tag.
/// * This crate cannot write LERC at all ([`compression::compress`] returns a
///   typed "not implemented" error) and neither [`cog::analyze_for_cog`] nor
///   [`cog::CogConverter`] can select it.
///
/// No encoder therefore produces a predicted LERC block, and a decoder that
/// meets both tags cannot tell whether a predictor was applied. The driver used
/// to reverse one regardless, over samples the LERC decoder had already put in
/// *host* order using the *file's* declared order — two independent errors, both
/// silent. An explicit `Err` is the only honest answer: the block is real, its
/// interpretation is not knowable.
///
/// One comparison per decoded block, short-circuited on the overwhelmingly
/// common `Predictor::None`; nothing per sample.
///
/// # Errors
/// Returns [`OxiGeoError::NotSupported`] for `Lerc` plus any predictor.
fn reject_undefined_predictor(compression: Compression, predictor: Predictor) -> Result<()> {
    if matches!(predictor, Predictor::None) || !matches!(compression, Compression::Lerc) {
        return Ok(());
    }
    Err(
        OxiGeoError::not_supported_builder("a TIFF Predictor combined with LERC compression")
            .with_operation("decode block")
            .with_parameter("compression", "LERC (34887)")
            .with_parameter("predictor", format!("{predictor:?} ({})", predictor as u16))
            .with_suggestion(
                "No encoder defines this combination (libtiff's LERC codec ignores tag 317 and \
                 GDAL refuses PREDICTOR for LERC), so the block's pixels cannot be reconstructed \
                 unambiguously. Re-encode the file without the Predictor tag, or with a codec \
                 that defines it (LZW, DEFLATE, ZSTD).",
            )
            .build(),
    )
}

/// Reverses every `N`-byte sample of `data` in place.
///
/// A trailing partial sample (which only a malformed block can produce) is left
/// alone rather than half-swapped.
fn swap_sample_bytes<const N: usize>(data: &mut [u8]) {
    for chunk in data.chunks_exact_mut(N) {
        let mut sample = [0u8; N];
        sample.copy_from_slice(chunk);
        sample.reverse();
        chunk.copy_from_slice(&sample);
    }
}

/// Converts one decoded block from the file's byte order to the host's, in place.
///
/// **Ordering matters.** This must run *after*
/// [`compression::apply_predictor_reverse`]: both TIFF predictors are defined on
/// file-order data — horizontal differencing reads and writes whole samples with
/// `byte_order`, and the floating-point predictor de-interleaves byte planes
/// whose most-significant plane is first on disk — so a block is not even a
/// sample array until the predictor has been reversed. Swapping first produces
/// pixels that look plausible and are wrong, which is exactly the failure class
/// #14 exists to eliminate.
///
/// A no-op unless [`decoded_needs_native_swap`] says otherwise, so the common
/// little-endian-file/little-endian-host case costs one branch per block.
fn normalize_samples_to_native(
    data: &mut [u8],
    bytes_per_sample: usize,
    byte_order: ByteOrderType,
    compression: Compression,
) {
    if !decoded_needs_native_swap(byte_order, bytes_per_sample, compression) {
        return;
    }
    match bytes_per_sample {
        2 => swap_sample_bytes::<2>(data),
        4 => swap_sample_bytes::<4>(data),
        8 => swap_sample_bytes::<8>(data),
        // Unreachable: `decoded_needs_native_swap` already rejected every other
        // width. Kept total rather than panicking.
        _ => {}
    }
}

/// Generates WKT string from GeoKeys
///
/// # Arguments
/// * `geo_keys` - Optional reference to GeoKeyDirectory
///
/// # Returns
/// WKT string if CRS information is available
fn parse_geokeys_to_wkt(geo_keys: Option<&GeoKeyDirectory>) -> Option<String> {
    let geo_keys = geo_keys?;
    let epsg_code = geo_keys.epsg_code()?;

    // Generate WKT based on EPSG code
    // For comprehensive WKT, we'd need a full EPSG database, but we can handle common cases
    Some(match epsg_code {
        // WGS 84
        4326 => {
            r#"GEOGCS["WGS 84",
    DATUM["WGS_1984",
        SPHEROID["WGS 84",6378137,298.257223563,
            AUTHORITY["EPSG","7030"]],
        AUTHORITY["EPSG","6326"]],
    PRIMEM["Greenwich",0,
        AUTHORITY["EPSG","8901"]],
    UNIT["degree",0.0174532925199433,
        AUTHORITY["EPSG","9122"]],
    AXIS["Latitude",NORTH],
    AXIS["Longitude",EAST],
    AUTHORITY["EPSG","4326"]]"#
                .to_string()
        }
        // WGS 84 / Pseudo-Mercator (Web Mercator)
        3857 => {
            r#"PROJCS["WGS 84 / Pseudo-Mercator",
    GEOGCS["WGS 84",
        DATUM["WGS_1984",
            SPHEROID["WGS 84",6378137,298.257223563,
                AUTHORITY["EPSG","7030"]],
            AUTHORITY["EPSG","6326"]],
        PRIMEM["Greenwich",0,
            AUTHORITY["EPSG","8901"]],
        UNIT["degree",0.0174532925199433,
            AUTHORITY["EPSG","9122"]],
        AUTHORITY["EPSG","4326"]],
    PROJECTION["Mercator_1SP"],
    PARAMETER["central_meridian",0],
    PARAMETER["scale_factor",1],
    PARAMETER["false_easting",0],
    PARAMETER["false_northing",0],
    UNIT["metre",1,
        AUTHORITY["EPSG","9001"]],
    AXIS["Easting",EAST],
    AXIS["Northing",NORTH],
    EXTENSION["PROJ4","+proj=merc +a=6378137 +b=6378137 +lat_ts=0 +lon_0=0 +x_0=0 +y_0=0 +k=1 +units=m +nadgrids=@null +wktext +no_defs"],
    AUTHORITY["EPSG","3857"]]"#
                .to_string()
        }
        // WGS 84 / UTM zones (Northern Hemisphere: 32601-32660)
        32601..=32660 => {
            let zone = epsg_code - 32600;
            format!(
                r#"PROJCS["WGS 84 / UTM zone {}N",
    GEOGCS["WGS 84",
        DATUM["WGS_1984",
            SPHEROID["WGS 84",6378137,298.257223563,
                AUTHORITY["EPSG","7030"]],
            AUTHORITY["EPSG","6326"]],
        PRIMEM["Greenwich",0,
            AUTHORITY["EPSG","8901"]],
        UNIT["degree",0.0174532925199433,
            AUTHORITY["EPSG","9122"]],
        AUTHORITY["EPSG","4326"]],
    PROJECTION["Transverse_Mercator"],
    PARAMETER["latitude_of_origin",0],
    PARAMETER["central_meridian",{}],
    PARAMETER["scale_factor",0.9996],
    PARAMETER["false_easting",500000],
    PARAMETER["false_northing",0],
    UNIT["metre",1,
        AUTHORITY["EPSG","9001"]],
    AXIS["Easting",EAST],
    AXIS["Northing",NORTH],
    AUTHORITY["EPSG","{}""]]"#,
                zone,
                zone as i32 * 6 - 183,
                epsg_code
            )
        }
        // WGS 84 / UTM zones (Southern Hemisphere: 32701-32760)
        32701..=32760 => {
            let zone = epsg_code - 32700;
            format!(
                r#"PROJCS["WGS 84 / UTM zone {}S",
    GEOGCS["WGS 84",
        DATUM["WGS_1984",
            SPHEROID["WGS 84",6378137,298.257223563,
                AUTHORITY["EPSG","7030"]],
            AUTHORITY["EPSG","6326"]],
        PRIMEM["Greenwich",0,
            AUTHORITY["EPSG","8901"]],
        UNIT["degree",0.0174532925199433,
            AUTHORITY["EPSG","9122"]],
        AUTHORITY["EPSG","4326"]],
    PROJECTION["Transverse_Mercator"],
    PARAMETER["latitude_of_origin",0],
    PARAMETER["central_meridian",{}],
    PARAMETER["scale_factor",0.9996],
    PARAMETER["false_easting",500000],
    PARAMETER["false_northing",10000000],
    UNIT["metre",1,
        AUTHORITY["EPSG","9001"]],
    AXIS["Easting",EAST],
    AXIS["Northing",NORTH],
    AUTHORITY["EPSG","{}""]]"#,
                zone,
                zone as i32 * 6 - 183,
                epsg_code
            )
        }
        // NAD83
        4269 => {
            r#"GEOGCS["NAD83",
    DATUM["North_American_Datum_1983",
        SPHEROID["GRS 1980",6378137,298.257222101,
            AUTHORITY["EPSG","7019"]],
        AUTHORITY["EPSG","6269"]],
    PRIMEM["Greenwich",0,
        AUTHORITY["EPSG","8901"]],
    UNIT["degree",0.0174532925199433,
        AUTHORITY["EPSG","9122"]],
    AXIS["Latitude",NORTH],
    AXIS["Longitude",EAST],
    AUTHORITY["EPSG","4269"]]"#
                .to_string()
        }
        // NAD27
        4267 => {
            r#"GEOGCS["NAD27",
    DATUM["North_American_Datum_1927",
        SPHEROID["Clarke 1866",6378206.4,294.978698213898,
            AUTHORITY["EPSG","7008"]],
        AUTHORITY["EPSG","6267"]],
    PRIMEM["Greenwich",0,
        AUTHORITY["EPSG","8901"]],
    UNIT["degree",0.0174532925199433,
        AUTHORITY["EPSG","9122"]],
    AXIS["Latitude",NORTH],
    AXIS["Longitude",EAST],
    AUTHORITY["EPSG","4267"]]"#
                .to_string()
        }
        // For other EPSG codes, use a simple reference
        _ => format!("EPSG:{}", epsg_code),
    })
}

/// Parses color interpretation from photometric interpretation
///
/// # Arguments
/// * `photometric` - The photometric interpretation from TIFF
/// * `samples_per_pixel` - Number of samples (bands) per pixel
///
/// # Returns
/// Vector of color interpretations for each band
fn parse_photometric_interpretation(
    photometric: PhotometricInterpretation,
    samples_per_pixel: u16,
) -> Vec<ColorInterpretation> {
    match photometric {
        PhotometricInterpretation::WhiteIsZero | PhotometricInterpretation::BlackIsZero => {
            // Grayscale - might have alpha channel
            if samples_per_pixel == 1 {
                vec![ColorInterpretation::Gray]
            } else if samples_per_pixel == 2 {
                vec![ColorInterpretation::Gray, ColorInterpretation::Alpha]
            } else {
                // Multiple grayscale bands
                vec![ColorInterpretation::Gray; samples_per_pixel as usize]
            }
        }
        PhotometricInterpretation::Rgb => {
            // RGB or RGBA
            match samples_per_pixel {
                1 => vec![ColorInterpretation::Red],
                2 => vec![ColorInterpretation::Red, ColorInterpretation::Green],
                3 => vec![
                    ColorInterpretation::Red,
                    ColorInterpretation::Green,
                    ColorInterpretation::Blue,
                ],
                4 => vec![
                    ColorInterpretation::Red,
                    ColorInterpretation::Green,
                    ColorInterpretation::Blue,
                    ColorInterpretation::Alpha,
                ],
                _ => {
                    // More than 4 bands - treat extras as undefined
                    let mut interp = vec![
                        ColorInterpretation::Red,
                        ColorInterpretation::Green,
                        ColorInterpretation::Blue,
                    ];
                    if samples_per_pixel > 3 {
                        interp.push(ColorInterpretation::Alpha);
                    }
                    for _ in 4..samples_per_pixel {
                        interp.push(ColorInterpretation::Undefined);
                    }
                    interp
                }
            }
        }
        PhotometricInterpretation::Palette => {
            // Palette color - index plus optional alpha
            if samples_per_pixel == 1 {
                vec![ColorInterpretation::PaletteIndex]
            } else if samples_per_pixel == 2 {
                vec![
                    ColorInterpretation::PaletteIndex,
                    ColorInterpretation::Alpha,
                ]
            } else {
                vec![ColorInterpretation::PaletteIndex; samples_per_pixel as usize]
            }
        }
        PhotometricInterpretation::Cmyk => {
            // CMYK
            match samples_per_pixel {
                1 => vec![ColorInterpretation::Cyan],
                2 => vec![ColorInterpretation::Cyan, ColorInterpretation::Magenta],
                3 => vec![
                    ColorInterpretation::Cyan,
                    ColorInterpretation::Magenta,
                    ColorInterpretation::Yellow,
                ],
                4 => vec![
                    ColorInterpretation::Cyan,
                    ColorInterpretation::Magenta,
                    ColorInterpretation::Yellow,
                    ColorInterpretation::Black,
                ],
                _ => {
                    // More than 4 bands - treat extras as undefined
                    let mut interp = vec![
                        ColorInterpretation::Cyan,
                        ColorInterpretation::Magenta,
                        ColorInterpretation::Yellow,
                        ColorInterpretation::Black,
                    ];
                    for _ in 4..samples_per_pixel {
                        interp.push(ColorInterpretation::Undefined);
                    }
                    interp
                }
            }
        }
        PhotometricInterpretation::YCbCr => {
            // YCbCr
            match samples_per_pixel {
                1 => vec![ColorInterpretation::YCbCrY],
                2 => vec![ColorInterpretation::YCbCrY, ColorInterpretation::YCbCrCb],
                3 => vec![
                    ColorInterpretation::YCbCrY,
                    ColorInterpretation::YCbCrCb,
                    ColorInterpretation::YCbCrCr,
                ],
                _ => {
                    // More than 3 bands - add alpha or undefined
                    let mut interp = vec![
                        ColorInterpretation::YCbCrY,
                        ColorInterpretation::YCbCrCb,
                        ColorInterpretation::YCbCrCr,
                    ];
                    if samples_per_pixel > 3 {
                        interp.push(ColorInterpretation::Alpha);
                    }
                    for _ in 4..samples_per_pixel {
                        interp.push(ColorInterpretation::Undefined);
                    }
                    interp
                }
            }
        }
        // For other photometric interpretations (TransparencyMask, CIE Lab, etc.)
        _ => vec![ColorInterpretation::Undefined; samples_per_pixel as usize],
    }
}

/// GeoTIFF reader (high-level API)
pub struct GeoTiffReader<S: DataSource> {
    cog_reader: CogReader<S>,
    geo_transform: Option<GeoTransform>,
    nodata: NoDataValue,
}

impl<S: DataSource> GeoTiffReader<S> {
    /// Opens a GeoTIFF file
    ///
    /// # Minimum size
    ///
    /// There is no fixed byte floor. The header parser needs
    /// [`TiffHeader::MIN_HEADER_SIZE`] bytes for a classic TIFF and
    /// [`TiffHeader::BIGTIFF_HEADER_SIZE`] for a BigTIFF, and a source with
    /// fewer than that is reported as a too-short header rather than as an
    /// unknown format or an I/O failure. Passing the header is not enough to
    /// open a file, though: the first IFD still has to parse and still has to
    /// carry the mandatory image tags, so a header-only stub is rejected — just
    /// with an error naming what is actually missing.
    ///
    /// # Errors
    /// Returns an error if the file cannot be opened or parsed
    pub fn open(source: S) -> Result<Self> {
        let cog_reader = CogReader::open(source)?;

        // Extract geotransform
        let geo_transform = cog_reader.geo_transform()?;

        // Extract nodata
        let nodata = cog_reader.nodata()?;

        Ok(Self {
            cog_reader,
            geo_transform,
            nodata,
        })
    }

    /// Returns the image width
    #[must_use]
    pub fn width(&self) -> u64 {
        self.cog_reader.width()
    }

    /// Returns the image height
    #[must_use]
    pub fn height(&self) -> u64 {
        self.cog_reader.height()
    }

    /// Returns the number of bands
    #[must_use]
    pub fn band_count(&self) -> u32 {
        u32::from(self.cog_reader.primary_info().samples_per_pixel)
    }

    /// Returns the data type
    #[must_use]
    pub fn data_type(&self) -> Option<RasterDataType> {
        self.cog_reader.primary_info().data_type()
    }

    /// Returns the tile size
    #[must_use]
    pub fn tile_size(&self) -> Option<(u32, u32)> {
        self.cog_reader.tile_size()
    }

    /// Returns the number of overview levels
    #[must_use]
    pub fn overview_count(&self) -> usize {
        self.cog_reader.overview_count()
    }

    /// Returns the GeoTransform
    #[must_use]
    pub fn geo_transform(&self) -> Option<&GeoTransform> {
        self.geo_transform.as_ref()
    }

    /// Returns the NoData value
    #[must_use]
    pub const fn nodata(&self) -> NoDataValue {
        self.nodata
    }

    /// Returns the EPSG code
    #[must_use]
    pub fn epsg_code(&self) -> Option<u32> {
        self.cog_reader.epsg_code()
    }

    /// Returns the compression scheme
    #[must_use]
    pub fn compression(&self) -> Compression {
        self.cog_reader.primary_info().compression
    }

    /// Returns the byte order declared by the file's header (`II` or `MM`).
    ///
    /// This is a property of the *file*, not of the data this reader hands out:
    /// every read API that yields decoded samples normalises them to the host's
    /// byte order (see the crate-level *Byte order of decoded samples* section),
    /// so callers never need this to interpret pixels. It is exposed for
    /// reporting, for round-tripping a file's on-disk layout through
    /// [`writer::GeoTiffWriterOptions`], and so that nothing has to re-parse the
    /// TIFF header out of the [`DataSource`] to find out — which is what a
    /// downstream crate had to do before cool-japan/oxigeo#14.
    #[must_use]
    pub fn byte_order(&self) -> ByteOrderType {
        self.cog_reader.tiff().byte_order()
    }

    /// Returns the number of tiles in X and Y directions
    #[must_use]
    pub fn tile_count(&self) -> (u32, u32) {
        self.cog_reader.tile_count()
    }

    /// Reads a tile
    ///
    /// Samples come back in the **host's** byte order regardless of the file's
    /// `II`/`MM` header; see [`CogReader::read_tile`] and the crate-level
    /// *Byte order of decoded samples* section. On a planar
    /// (`PlanarConfiguration = 2`) raster a block holds one band and `tile_y`
    /// indexes the plane-major block grid — see [`CogReader::tile_decoded_size`].
    ///
    /// # Errors
    /// Returns an error if the tile cannot be read, or if its codec/predictor
    /// combination has no defined reversal (LERC plus a `Predictor` tag), which
    /// is refused rather than decoded to wrong pixels.
    pub fn read_tile(&self, level: usize, tile_x: u32, tile_y: u32) -> Result<Vec<u8>> {
        self.cog_reader.read_tile(level, tile_x, tile_y)
    }

    /// Returns the pixel dimensions `(width, height)` of one resolution level.
    ///
    /// `level` is `0` for full resolution and `1..=`[`Self::overview_count`] for
    /// the overviews. The values are read from the level's **own** IFD
    /// (`ImageWidth`/`ImageLength`), so they are the dimensions actually stored
    /// rather than the `ceil(full / 2^level)` a caller would otherwise have to
    /// infer — an inference that only holds for a strict power-of-two pyramid
    /// and silently mis-describes every overview of a raster built with any
    /// other decimation factor (cool-japan/oxigeo#14).
    ///
    /// Pair it with [`Self::band_byte_len`] / [`Self::band_pixel_count`], which
    /// give only the product, when you need to clamp a window or shape a
    /// [`RasterBuffer`] for `level > 0`.
    ///
    /// # Errors
    /// Returns [`OxiGeoError::OutOfBounds`] if `level` names no overview, or a
    /// format error if the level's IFD declares no dimensions.
    ///
    /// # Examples
    /// ```ignore
    /// let (w, h) = reader.level_size(1)?; // the first overview's real size
    /// ```
    pub fn level_size(&self, level: usize) -> Result<(u64, u64)> {
        let info = self.level_info(level)?;
        Ok((info.width, info.height))
    }

    /// Resolves the [`ImageInfo`] describing `level`.
    ///
    /// Level 0 is the primary image. `CogReader` parses the overview IFDs but
    /// does not expose them, so an overview's own tags are read straight from
    /// its IFD here; they are all single-valued and therefore stored inline, so
    /// no data source is needed. Tags absent from the overview IFD are inherited
    /// from the full-resolution image, which is what overviews always share.
    ///
    /// This mirrors `band_read::LevelGeometry::resolve`, the read engine's
    /// equivalent; the two must agree, and the buffer-length checks in
    /// [`Self::read_window_into`] surface it as a typed error if they ever do
    /// not.
    fn level_info(&self, level: usize) -> Result<ImageInfo> {
        let primary = self.cog_reader.primary_info();
        if level == 0 {
            return Ok(primary.clone());
        }
        let out_of_bounds = || OxiGeoError::OutOfBounds {
            message: format!("Overview level {level} out of bounds"),
        };
        if level > self.cog_reader.overview_count() {
            return Err(out_of_bounds());
        }

        let byte_order = self.cog_reader.tiff().byte_order();
        // Through the reader's level → IFD map, not `ifds[level]`: levels skip
        // GDAL internal masks, so on a masked file the raw chain index names a
        // different image and `level_size` would report the mask's dimensions
        // for a level whose tiles come from the real overview.
        let ifd = self.cog_reader.level_ifd(level).ok_or_else(out_of_bounds)?;
        let scalar = |tag: TiffTag| ifd.get_entry(tag).and_then(|e| e.get_u64(byte_order).ok());

        let mut info = primary.clone();
        info.width =
            scalar(TiffTag::ImageWidth).ok_or(OxiGeoError::Format(FormatError::MissingTag {
                tag: "ImageWidth",
            }))?;
        info.height =
            scalar(TiffTag::ImageLength).ok_or(OxiGeoError::Format(FormatError::MissingTag {
                tag: "ImageLength",
            }))?;
        // Layout tags are per-level: an overview may be striped even when the
        // full-resolution image is tiled, so these are never inherited.
        info.tile_width = scalar(TiffTag::TileWidth).and_then(|v| u32::try_from(v).ok());
        info.tile_height = scalar(TiffTag::TileLength).and_then(|v| u32::try_from(v).ok());
        info.rows_per_strip = scalar(TiffTag::RowsPerStrip).and_then(|v| u32::try_from(v).ok());
        if let Some(v) = scalar(TiffTag::SamplesPerPixel) {
            info.samples_per_pixel = v as u16;
        }
        if let Some(v) = scalar(TiffTag::BitsPerSample) {
            info.bits_per_sample = vec![v as u16];
        }
        Ok(info)
    }

    /// Reads band 0 of one tile (or strip) as a [`RasterBuffer`].
    ///
    /// Shorthand for [`Self::read_tile_band_buffer`] with `band = 0`; see there
    /// for the buffer's exact geometry and for multi-band files, which a
    /// single-band [`RasterBuffer`] cannot represent in one go.
    ///
    /// # Errors
    /// Returns an error if `level` names no overview, if the block coordinates
    /// are out of range, or if the block cannot be read or decoded.
    pub fn read_tile_buffer(&self, level: usize, tile_x: u32, tile_y: u32) -> Result<RasterBuffer> {
        self.read_tile_band_buffer(level, 0, tile_x, tile_y)
    }

    /// Reads one band of one tile (or strip) as a [`RasterBuffer`].
    ///
    /// The band selector matches the rest of the read API
    /// ([`Self::read_band`], [`Self::read_window`]): `level` first, then a
    /// zero-based `band`. Both arguments are honoured — the block geometry comes
    /// from the requested **level's own IFD**, not from the full-resolution
    /// image, and the requested band is de-interleaved (chunky) or
    /// plane-selected (planar) exactly as [`Self::read_band`] does it.
    ///
    /// The returned buffer is one full block: `tile_width × tile_height` for a
    /// tiled level, or `image_width × rows_in_this_strip` for a striped one. A
    /// block that overhangs the raster edge is padded with zeros, so the buffer
    /// is always the block's nominal size and a tile mosaic stays aligned.
    ///
    /// Its samples are in the **host's** byte order, which is what
    /// [`RasterBuffer`]'s accessors assume; see the crate-level *Byte order of
    /// decoded samples* section.
    ///
    /// Before cool-japan/oxigeo#14 this method handed `RasterBuffer::new` a
    /// whole chunky block (`tw·th·bps·spp` bytes) while claiming `tw·th`
    /// pixels, so it could not succeed at all on a `SamplesPerPixel > 1` file,
    /// and it took the tile dimensions from the primary IFD whatever `level`
    /// said.
    ///
    /// # Errors
    /// Returns an error if `band` is out of range for the level, if `level`
    /// names no overview, if `(tile_x, tile_y)` is out of range, if the level
    /// declares zero-sized blocks or an unsupported sample type, or if a block
    /// cannot be read or decoded.
    pub fn read_tile_band_buffer(
        &self,
        level: usize,
        band: usize,
        tile_x: u32,
        tile_y: u32,
    ) -> Result<RasterBuffer> {
        let info = self.level_info(level)?;

        let band_count = info.samples_per_pixel as usize;
        if band >= band_count {
            return Err(OxiGeoError::invalid_parameter_builder(
                "band",
                "band index is out of range for this raster",
            )
            .with_operation("read_tile_band_buffer")
            .with_parameter("band", band.to_string())
            .with_parameter("band_count", band_count.to_string())
            .with_parameter("level", level.to_string())
            .with_suggestion("Band indices are zero-based; use GeoTiffReader::band_count()")
            .build());
        }

        let data_type =
            info.data_type()
                .ok_or(OxiGeoError::Format(FormatError::InvalidDataType {
                    type_id: 0,
                }))?;
        let bytes_per_sample = data_type.size_bytes() as u64;

        // Block geometry, from this level's own tags. A striped level's "tile"
        // is a strip: full image width, `RowsPerStrip` rows.
        let (block_width, block_height) = if info.is_tiled() {
            (
                u64::from(info.tile_width.unwrap_or_default()),
                u64::from(info.tile_height.unwrap_or_default()),
            )
        } else {
            (
                info.width,
                u64::from(info.rows_per_strip.unwrap_or(info.height as u32)),
            )
        };
        if block_width == 0 || block_height == 0 {
            return Err(OxiGeoError::Format(FormatError::InvalidHeader {
                message: format!(
                    "level {level} declares zero-sized blocks ({block_width}x{block_height})"
                ),
            }));
        }

        // Safe now that both divisors are non-zero.
        let (blocks_across, blocks_down) = (info.tiles_across(), info.tiles_down());
        if tile_x >= blocks_across || tile_y >= blocks_down {
            return Err(OxiGeoError::OutOfBounds {
                message: format!(
                    "Tile/strip ({tile_x}, {tile_y}) out of bounds at level {level} \
                     ({blocks_across}x{blocks_down} blocks)"
                ),
            });
        }

        let x0 = u64::from(tile_x) * block_width;
        let y0 = u64::from(tile_y) * block_height;
        // Tiles keep their nominal height (the overhang is padding); the last
        // strip of a striped level is genuinely shorter.
        let rows = if info.is_tiled() {
            block_height
        } else {
            block_height.min(info.height.saturating_sub(y0))
        };
        let valid_width = block_width.min(info.width.saturating_sub(x0));
        let valid_height = rows.min(info.height.saturating_sub(y0));

        let to_usize = |v: u64| {
            usize::try_from(v).map_err(|_| {
                OxiGeoError::Format(FormatError::InvalidHeader {
                    message: format!("block size {v} does not fit in usize on this target"),
                })
            })
        };
        // Bounds the allocation against hostile IFD dimensions, exactly as the
        // whole-band read does.
        let buffer_len = checked_band_bytes(
            to_usize(block_width)?,
            to_usize(rows)?,
            to_usize(bytes_per_sample)?,
            1,
        )?;
        let mut data = vec![0u8; buffer_len];

        if valid_width > 0 && valid_height > 0 {
            let row_bytes = to_usize(valid_width * bytes_per_sample)?;
            let dst_stride = to_usize(block_width * bytes_per_sample)?;
            if valid_width == block_width {
                // The valid region is a contiguous prefix of the block buffer.
                let len = row_bytes * to_usize(valid_height)?;
                self.read_window_into(
                    level,
                    band,
                    x0,
                    y0,
                    valid_width,
                    valid_height,
                    &mut data[..len],
                )?;
            } else {
                let mut window = vec![0u8; row_bytes * to_usize(valid_height)?];
                self.read_window_into(level, band, x0, y0, valid_width, valid_height, &mut window)?;
                for row in 0..to_usize(valid_height)? {
                    let src = row * row_bytes;
                    let dst = row * dst_stride;
                    data[dst..dst + row_bytes].copy_from_slice(&window[src..src + row_bytes]);
                }
            }
        }

        RasterBuffer::new(data, block_width, rows, data_type, self.nodata)
    }

    /// Returns the raster metadata
    #[must_use]
    pub fn metadata(&self) -> RasterMetadata {
        let info = self.cog_reader.primary_info();

        // Generate WKT from GeoKeys
        let crs_wkt = parse_geokeys_to_wkt(self.cog_reader.geo_keys());

        // Parse color interpretation from photometric
        let color_interpretation =
            parse_photometric_interpretation(info.photometric, info.samples_per_pixel);

        RasterMetadata {
            width: info.width,
            height: info.height,
            band_count: u32::from(info.samples_per_pixel),
            data_type: info.data_type().unwrap_or(RasterDataType::UInt8),
            geo_transform: self.geo_transform,
            crs_wkt,
            nodata: self.nodata,
            color_interpretation,
            layout: oxigeo_core::types::PixelLayout::Tiled {
                tile_width: info.tile_width.unwrap_or(256),
                tile_height: info.tile_height.unwrap_or(256),
            },
            driver_metadata: Vec::new(),
            statistics: None,
        }
    }

    /// Creates a new reader (alias for `open`)
    ///
    /// # Errors
    /// Returns an error if the file cannot be opened or parsed
    pub fn new(source: S) -> Result<Self> {
        Self::open(source)
    }
}

/// Checks if data looks like a TIFF file
#[must_use]
pub fn is_tiff(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }

    // Check for TIFF magic
    (data[0] == 0x49 && data[1] == 0x49 && data[2] == 0x2A && data[3] == 0x00)  // Little-endian classic
        || (data[0] == 0x4D && data[1] == 0x4D && data[2] == 0x00 && data[3] == 0x2A) // Big-endian classic
        || (data[0] == 0x49 && data[1] == 0x49 && data[2] == 0x2B && data[3] == 0x00) // Little-endian BigTIFF
        || (data[0] == 0x4D && data[1] == 0x4D && data[2] == 0x00 && data[3] == 0x2B) // Big-endian BigTIFF
}

/// Checks if a TIFF appears to be a COG
pub fn is_cog<S: DataSource>(source: &S) -> Result<bool> {
    let tiff = TiffFile::parse(source)?;
    let validation = cog::validate_cog(&tiff, source);
    Ok(validation.is_valid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geokeys::GeoKeyEntry;

    #[test]
    fn test_is_tiff() {
        // Classic TIFF, little-endian
        assert!(is_tiff(&[0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00]));

        // Classic TIFF, big-endian
        assert!(is_tiff(&[0x4D, 0x4D, 0x00, 0x2A, 0x00, 0x00, 0x00, 0x08]));

        // BigTIFF, little-endian
        assert!(is_tiff(&[0x49, 0x49, 0x2B, 0x00, 0x08, 0x00, 0x00, 0x00]));

        // Not TIFF
        assert!(!is_tiff(&[0x89, 0x50, 0x4E, 0x47])); // PNG
        assert!(!is_tiff(&[0xFF, 0xD8, 0xFF])); // JPEG
        assert!(!is_tiff(&[]));
    }

    /// The host's byte order, as the normalisation rule sees it.
    const HOST: ByteOrderType = if cfg!(target_endian = "big") {
        ByteOrderType::BigEndian
    } else {
        ByteOrderType::LittleEndian
    };

    /// The byte order that is *not* the host's.
    const FOREIGN: ByteOrderType = if cfg!(target_endian = "big") {
        ByteOrderType::LittleEndian
    } else {
        ByteOrderType::BigEndian
    };

    #[test]
    fn native_swap_is_skipped_when_the_file_matches_the_host() {
        // The common case, and the whole of the zero-cost claim: nothing to do.
        for width in [0usize, 1, 2, 3, 4, 6, 8, 16] {
            assert!(
                !decoded_needs_native_swap(HOST, width, Compression::None),
                "width {width} must not be swapped for a host-order file"
            );
        }
    }

    #[test]
    fn native_swap_covers_exactly_the_two_four_and_eight_byte_widths() {
        for width in [2usize, 4, 8] {
            assert!(
                decoded_needs_native_swap(FOREIGN, width, Compression::None),
                "width {width} of a foreign-order file must be swapped"
            );
        }
        // 0 = sub-byte BitsPerSample (bilevel/4-bit), 1 = byte samples, and 3/6
        // stand in for exotic widths such as a 24-bit BitsPerSample: none has a
        // swap to perform or a defined order to swap to.
        for width in [0usize, 1, 3, 5, 6, 7, 9, 12] {
            assert!(
                !decoded_needs_native_swap(FOREIGN, width, Compression::None),
                "width {width} must be passed through untouched"
            );
        }
    }

    #[test]
    fn native_swap_never_touches_lerc_blocks() {
        // This crate's LERC decoder emits native-order samples already
        // (`lerc_codec::serialize_native`), so a swap would introduce the
        // corruption it exists to remove.
        for width in [2usize, 4, 8] {
            assert!(!decoded_needs_native_swap(
                FOREIGN,
                width,
                Compression::Lerc
            ));
            assert!(!decoded_needs_native_swap(HOST, width, Compression::Lerc));
        }
    }

    #[test]
    fn native_swap_reverses_whole_samples_and_leaves_a_partial_tail_alone() {
        let mut two = [1u8, 2, 3, 4, 9];
        swap_sample_bytes::<2>(&mut two);
        assert_eq!(two, [2, 1, 4, 3, 9], "trailing odd byte must be untouched");

        let mut four = [1u8, 2, 3, 4, 5, 6, 7, 8, 0xaa, 0xbb];
        swap_sample_bytes::<4>(&mut four);
        assert_eq!(four, [4, 3, 2, 1, 8, 7, 6, 5, 0xaa, 0xbb]);

        let mut eight = [1u8, 2, 3, 4, 5, 6, 7, 8];
        swap_sample_bytes::<8>(&mut eight);
        assert_eq!(eight, [8, 7, 6, 5, 4, 3, 2, 1]);

        // Swapping twice is the identity, which is what makes a double swap so
        // hard to notice — and why the driver must do it in exactly one place.
        let original = [0x12u8, 0x34, 0x56, 0x78];
        let mut round_trip = original;
        swap_sample_bytes::<4>(&mut round_trip);
        swap_sample_bytes::<4>(&mut round_trip);
        assert_eq!(round_trip, original);
    }

    #[test]
    fn normalize_samples_to_native_is_a_no_op_for_a_host_order_file() {
        let original: Vec<u8> = (0..32).collect();
        let mut data = original.clone();
        normalize_samples_to_native(&mut data, 4, HOST, Compression::None);
        assert_eq!(data, original);

        // ... and does reverse each sample for a foreign-order one.
        normalize_samples_to_native(&mut data, 4, FOREIGN, Compression::None);
        assert_eq!(&data[..4], &[3, 2, 1, 0]);
    }

    #[test]
    fn checked_band_bytes_accepts_reasonable_dimensions() {
        // 4096 x 4096 x 2 bytes x 3 samples = 100 MiB — well within the cap.
        let n = checked_band_bytes(4096, 4096, 2, 3);
        assert!(matches!(n, Ok(v) if v == 4096 * 4096 * 2 * 3));
        // A zero-area band is valid (empty buffer).
        assert!(matches!(checked_band_bytes(0, 0, 4, 1), Ok(0)));
    }

    #[test]
    fn checked_band_bytes_rejects_oversized_dimensions() {
        // 65535 x 65535 x 8 bytes x 4 samples ~= 137 GB — far past the cap.
        let err = checked_band_bytes(65535, 65535, 8, 4);
        assert!(matches!(
            err,
            Err(OxiGeoError::Format(FormatError::InvalidHeader { .. }))
        ));
    }

    #[test]
    fn checked_band_bytes_rejects_multiplication_overflow() {
        // Dimensions chosen so the product wraps `usize`; must be rejected, not
        // silently truncated to a small (then out-of-bounds) allocation.
        let err = checked_band_bytes(usize::MAX, 2, 1, 1);
        assert!(matches!(
            err,
            Err(OxiGeoError::Format(FormatError::InvalidHeader { .. }))
        ));
    }

    #[test]
    fn test_parse_geokeys_to_wkt_none() {
        // Test with None input
        let wkt = parse_geokeys_to_wkt(None);
        assert!(wkt.is_none());
    }

    #[test]
    fn test_parse_geokeys_to_wkt_epsg_4326() {
        // Create a mock GeoKeyDirectory with EPSG:4326
        let geo_dir = GeoKeyDirectory {
            version: 1,
            key_revision_major: 1,
            key_revision_minor: 0,
            entries: vec![GeoKeyEntry {
                key_id: GeoKey::GeographicType as u16,
                tiff_tag_location: 0,
                count: 1,
                value_offset: 4326,
            }],
            double_params: Vec::new(),
            ascii_params: String::new(),
        };

        let wkt = parse_geokeys_to_wkt(Some(&geo_dir));
        assert!(wkt.is_some());
        let wkt_str = wkt.unwrap_or_default();
        assert!(wkt_str.contains("WGS 84"));
        assert!(wkt_str.contains("EPSG"));
        assert!(wkt_str.contains("4326"));
    }

    #[test]
    fn test_parse_geokeys_to_wkt_epsg_3857() {
        // Create a mock GeoKeyDirectory with EPSG:3857 (Web Mercator)
        let geo_dir = GeoKeyDirectory {
            version: 1,
            key_revision_major: 1,
            key_revision_minor: 0,
            entries: vec![GeoKeyEntry {
                key_id: GeoKey::ProjectedCsType as u16,
                tiff_tag_location: 0,
                count: 1,
                value_offset: 3857,
            }],
            double_params: Vec::new(),
            ascii_params: String::new(),
        };

        let wkt = parse_geokeys_to_wkt(Some(&geo_dir));
        assert!(wkt.is_some());
        let wkt_str = wkt.unwrap_or_default();
        assert!(wkt_str.contains("Pseudo-Mercator"));
        assert!(wkt_str.contains("3857"));
    }

    #[test]
    fn test_parse_geokeys_to_wkt_utm_north() {
        // Create a mock GeoKeyDirectory with EPSG:32632 (UTM Zone 32N)
        let geo_dir = GeoKeyDirectory {
            version: 1,
            key_revision_major: 1,
            key_revision_minor: 0,
            entries: vec![GeoKeyEntry {
                key_id: GeoKey::ProjectedCsType as u16,
                tiff_tag_location: 0,
                count: 1,
                value_offset: 32632,
            }],
            double_params: Vec::new(),
            ascii_params: String::new(),
        };

        let wkt = parse_geokeys_to_wkt(Some(&geo_dir));
        assert!(wkt.is_some());
        let wkt_str = wkt.unwrap_or_default();
        assert!(wkt_str.contains("UTM zone 32N"));
        assert!(wkt_str.contains("32632"));
        assert!(wkt_str.contains("central_meridian"));
    }

    #[test]
    fn test_parse_geokeys_to_wkt_utm_south() {
        // Create a mock GeoKeyDirectory with EPSG:32732 (UTM Zone 32S)
        let geo_dir = GeoKeyDirectory {
            version: 1,
            key_revision_major: 1,
            key_revision_minor: 0,
            entries: vec![GeoKeyEntry {
                key_id: GeoKey::ProjectedCsType as u16,
                tiff_tag_location: 0,
                count: 1,
                value_offset: 32732,
            }],
            double_params: Vec::new(),
            ascii_params: String::new(),
        };

        let wkt = parse_geokeys_to_wkt(Some(&geo_dir));
        assert!(wkt.is_some());
        let wkt_str = wkt.unwrap_or_default();
        assert!(wkt_str.contains("UTM zone 32S"));
        assert!(wkt_str.contains("32732"));
        assert!(wkt_str.contains("false_northing"));
    }

    #[test]
    fn test_parse_geokeys_to_wkt_nad83() {
        // Create a mock GeoKeyDirectory with EPSG:4269 (NAD83)
        let geo_dir = GeoKeyDirectory {
            version: 1,
            key_revision_major: 1,
            key_revision_minor: 0,
            entries: vec![GeoKeyEntry {
                key_id: GeoKey::GeographicType as u16,
                tiff_tag_location: 0,
                count: 1,
                value_offset: 4269,
            }],
            double_params: Vec::new(),
            ascii_params: String::new(),
        };

        let wkt = parse_geokeys_to_wkt(Some(&geo_dir));
        assert!(wkt.is_some());
        let wkt_str = wkt.unwrap_or_default();
        assert!(wkt_str.contains("NAD83"));
        assert!(wkt_str.contains("4269"));
    }

    #[test]
    fn test_parse_geokeys_to_wkt_unknown_epsg() {
        // Create a mock GeoKeyDirectory with an unknown EPSG code
        let geo_dir = GeoKeyDirectory {
            version: 1,
            key_revision_major: 1,
            key_revision_minor: 0,
            entries: vec![GeoKeyEntry {
                key_id: GeoKey::ProjectedCsType as u16,
                tiff_tag_location: 0,
                count: 1,
                value_offset: 9999,
            }],
            double_params: Vec::new(),
            ascii_params: String::new(),
        };

        let wkt = parse_geokeys_to_wkt(Some(&geo_dir));
        assert!(wkt.is_some());
        assert_eq!(wkt.unwrap_or_default(), "EPSG:9999");
    }

    #[test]
    fn test_parse_photometric_gray_single() {
        let interp = parse_photometric_interpretation(PhotometricInterpretation::BlackIsZero, 1);
        assert_eq!(interp.len(), 1);
        assert_eq!(interp[0], ColorInterpretation::Gray);
    }

    #[test]
    fn test_parse_photometric_gray_with_alpha() {
        let interp = parse_photometric_interpretation(PhotometricInterpretation::WhiteIsZero, 2);
        assert_eq!(interp.len(), 2);
        assert_eq!(interp[0], ColorInterpretation::Gray);
        assert_eq!(interp[1], ColorInterpretation::Alpha);
    }

    #[test]
    fn test_parse_photometric_rgb() {
        let interp = parse_photometric_interpretation(PhotometricInterpretation::Rgb, 3);
        assert_eq!(interp.len(), 3);
        assert_eq!(interp[0], ColorInterpretation::Red);
        assert_eq!(interp[1], ColorInterpretation::Green);
        assert_eq!(interp[2], ColorInterpretation::Blue);
    }

    #[test]
    fn test_parse_photometric_rgba() {
        let interp = parse_photometric_interpretation(PhotometricInterpretation::Rgb, 4);
        assert_eq!(interp.len(), 4);
        assert_eq!(interp[0], ColorInterpretation::Red);
        assert_eq!(interp[1], ColorInterpretation::Green);
        assert_eq!(interp[2], ColorInterpretation::Blue);
        assert_eq!(interp[3], ColorInterpretation::Alpha);
    }

    #[test]
    fn test_parse_photometric_palette() {
        let interp = parse_photometric_interpretation(PhotometricInterpretation::Palette, 1);
        assert_eq!(interp.len(), 1);
        assert_eq!(interp[0], ColorInterpretation::PaletteIndex);
    }

    #[test]
    fn test_parse_photometric_cmyk() {
        let interp = parse_photometric_interpretation(PhotometricInterpretation::Cmyk, 4);
        assert_eq!(interp.len(), 4);
        assert_eq!(interp[0], ColorInterpretation::Cyan);
        assert_eq!(interp[1], ColorInterpretation::Magenta);
        assert_eq!(interp[2], ColorInterpretation::Yellow);
        assert_eq!(interp[3], ColorInterpretation::Black);
    }

    #[test]
    fn test_parse_photometric_ycbcr() {
        let interp = parse_photometric_interpretation(PhotometricInterpretation::YCbCr, 3);
        assert_eq!(interp.len(), 3);
        assert_eq!(interp[0], ColorInterpretation::YCbCrY);
        assert_eq!(interp[1], ColorInterpretation::YCbCrCb);
        assert_eq!(interp[2], ColorInterpretation::YCbCrCr);
    }

    #[test]
    fn test_parse_photometric_ycbcr_with_alpha() {
        let interp = parse_photometric_interpretation(PhotometricInterpretation::YCbCr, 4);
        assert_eq!(interp.len(), 4);
        assert_eq!(interp[0], ColorInterpretation::YCbCrY);
        assert_eq!(interp[1], ColorInterpretation::YCbCrCb);
        assert_eq!(interp[2], ColorInterpretation::YCbCrCr);
        assert_eq!(interp[3], ColorInterpretation::Alpha);
    }

    #[test]
    fn test_parse_photometric_rgb_extra_bands() {
        let interp = parse_photometric_interpretation(PhotometricInterpretation::Rgb, 6);
        assert_eq!(interp.len(), 6);
        assert_eq!(interp[0], ColorInterpretation::Red);
        assert_eq!(interp[1], ColorInterpretation::Green);
        assert_eq!(interp[2], ColorInterpretation::Blue);
        assert_eq!(interp[3], ColorInterpretation::Alpha);
        assert_eq!(interp[4], ColorInterpretation::Undefined);
        assert_eq!(interp[5], ColorInterpretation::Undefined);
    }

    #[test]
    fn test_parse_photometric_undefined() {
        // Test with an uncommon photometric interpretation
        let interp =
            parse_photometric_interpretation(PhotometricInterpretation::TransparencyMask, 2);
        assert_eq!(interp.len(), 2);
        assert_eq!(interp[0], ColorInterpretation::Undefined);
        assert_eq!(interp[1], ColorInterpretation::Undefined);
    }
}
