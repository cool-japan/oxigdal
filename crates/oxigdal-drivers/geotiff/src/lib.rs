//! OxiGDAL GeoTIFF Driver - Pure Rust GeoTIFF/COG Support
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
//! - `webp` - WebP compression (planned)
//!
//! # Example
//!
//! ```ignore
//! use oxigdal_geotiff::cog::CogReader;
//! use oxigdal_core::io::FileDataSource;
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

pub mod cog;
pub mod compression;
pub mod geokeys;
pub mod jpeg_codec;
pub mod lerc_codec;
pub mod tiff;
pub mod writer;

// Re-export commonly used types
pub use cog::CogReader;
pub use geokeys::{GeoKey, GeoKeyDirectory, ModelType, RasterType};
pub use tiff::{Compression, ImageInfo, PhotometricInterpretation, TiffFile, TiffHeader, TiffTag};
pub use writer::{
    CogWriter, CogWriterOptions, GeoTiffWriter, GeoTiffWriterOptions, OverviewResampling,
    WriterConfig,
};

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::error::{FormatError, OxiGdalError, Result};
use oxigdal_core::io::DataSource;
use oxigdal_core::types::{
    ColorInterpretation, GeoTransform, NoDataValue, RasterDataType, RasterMetadata,
};

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

    /// Returns the number of tiles in X and Y directions
    #[must_use]
    pub fn tile_count(&self) -> (u32, u32) {
        self.cog_reader.tile_count()
    }

    /// Reads a tile
    ///
    /// # Errors
    /// Returns an error if the tile cannot be read
    pub fn read_tile(&self, level: usize, tile_x: u32, tile_y: u32) -> Result<Vec<u8>> {
        self.cog_reader.read_tile(level, tile_x, tile_y)
    }

    /// Reads a tile as a RasterBuffer
    ///
    /// # Errors
    /// Returns an error if the tile cannot be read
    pub fn read_tile_buffer(&self, level: usize, tile_x: u32, tile_y: u32) -> Result<RasterBuffer> {
        let data = self.read_tile(level, tile_x, tile_y)?;
        let info = self.cog_reader.primary_info();

        let tile_width = info.tile_width.unwrap_or(info.width as u32) as u64;
        let tile_height = info.tile_height.unwrap_or(info.height as u32) as u64;
        let data_type =
            info.data_type()
                .ok_or(OxiGdalError::Format(FormatError::InvalidDataType {
                    type_id: 0,
                }))?;

        RasterBuffer::new(data, tile_width, tile_height, data_type, self.nodata)
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
            layout: oxigdal_core::types::PixelLayout::Tiled {
                tile_width: info.tile_width.unwrap_or(256),
                tile_height: info.tile_height.unwrap_or(256),
            },
            driver_metadata: Vec::new(),
        }
    }

    /// Reads a band's data
    ///
    /// # Errors
    /// Returns an error if reading fails
    pub fn read_band(&self, level: usize, _band: usize) -> Result<Vec<u8>> {
        // Read all tiles/strips and combine them
        let (tiles_x, tiles_y) = self.tile_count();
        let info = self.cog_reader.primary_info();

        let width = info.width as usize;
        let height = info.height as usize;
        let bytes_per_sample = (info.bits_per_sample.first().copied().unwrap_or(8) / 8) as usize;
        let samples_per_pixel = info.samples_per_pixel as usize;

        let mut result = vec![0u8; width * height * bytes_per_sample * samples_per_pixel];

        // Determine if this is tiled or striped layout
        let is_tiled = info.tile_width.is_some() && info.tile_height.is_some();

        let (tile_width, default_tile_height) = if is_tiled {
            (
                info.tile_width.unwrap_or(width as u32) as usize,
                info.tile_height.unwrap_or(height as u32) as usize,
            )
        } else {
            // Striped layout
            (width, info.rows_per_strip.unwrap_or(height as u32) as usize)
        };

        for ty in 0..tiles_y {
            for tx in 0..tiles_x {
                let tile_data = self.read_tile(level, tx, ty)?;

                // Copy tile/strip data to result
                let x_start = tx as usize * tile_width;
                let y_start = ty as usize * default_tile_height;

                // Calculate actual height of this tile/strip (may be smaller for last one)
                let actual_rows = (height - y_start).min(default_tile_height);

                // For tiled layouts: tile_data always has full tile_width stride
                // For striped layouts: tile_data has image width stride
                let src_stride = if is_tiled { tile_width } else { width };

                for row in 0..actual_rows {
                    let dst_y = y_start + row;
                    if dst_y >= height {
                        break;
                    }

                    let src_offset = row * src_stride * bytes_per_sample * samples_per_pixel;
                    let dst_offset = dst_y * width * bytes_per_sample * samples_per_pixel
                        + x_start * bytes_per_sample * samples_per_pixel;

                    let copy_width = tile_width.min(width - x_start);
                    let copy_bytes = copy_width * bytes_per_sample * samples_per_pixel;

                    if src_offset + copy_bytes <= tile_data.len()
                        && dst_offset + copy_bytes <= result.len()
                    {
                        result[dst_offset..dst_offset + copy_bytes]
                            .copy_from_slice(&tile_data[src_offset..src_offset + copy_bytes]);
                    }
                }
            }
        }

        Ok(result)
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
